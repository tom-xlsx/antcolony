//! Draws a `World` into a ratatui buffer. Each terminal cell is two stacked pixels:
//! `▀` with the foreground as the top pixel and the background as the bottom one.
//!
//! Colours are kept stable frame to frame wherever nothing moved (soil texture comes
//! from a hash, trail brightness is quantised), so the terminal diff stays small.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

use crate::palette::{self, Rgb};
use crate::sim::{Cell, Job, Season, State, World, HUNGRY, NO_NEST};

pub struct Colony<'a> {
    pub world: &'a World,
    pub status: Option<String>,
}

/// Cheap stable per-pixel noise.
fn hash(x: usize, y: usize) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h
}

const STORE_CAP: f32 = 6.0;

/// How the season tints the world: fixed per phase so unchanged cells stay unchanged.
struct Weather {
    sky: Rgb,
    soil: Rgb,
    pebble: Rgb,
    grass: Rgb,
    /// Grass tufts per hundred surface cells.
    tufts: u32,
    sun: bool,
}

fn weather(season: Season) -> Weather {
    let dust = |t: f32| Weather {
        sky: palette::lerp(palette::SKY, palette::DUST_SKY, t),
        soil: palette::lerp(palette::SOIL, palette::DUST_SOIL, t),
        pebble: palette::lerp(palette::PEBBLE, palette::DUST_SOIL, t * 0.5),
        grass: palette::lerp(palette::GRASS, palette::STRAW, t),
        tufts: 12,
        sun: false,
    };
    match season {
        Season::Drought { .. } => Weather { sun: true, ..dust(0.55) },
        Season::Warning { .. } => dust(0.25),
        Season::Bloom { .. } => Weather { grass: palette::BLOOM_GRASS, tufts: 20, ..dust(0.0) },
        Season::Calm { .. } | Season::Cooldown { .. } => dust(0.0),
    }
}

/// Flat RGB pixel buffer the size of the world.
pub fn paint(world: &World) -> Vec<Rgb> {
    let (w, h) = (world.w, world.h);
    let wx = weather(world.season);
    let mut px = vec![wx.sky; w * h];
    // Sun sits top-right of the sky during a drought.
    let sun = (w as i32 - 6, 3_i32);

    for y in 0..h {
        for x in 0..w {
            let n = hash(x, y);
            let ground = world.ground[x];
            let c = match world.cell(x, y) {
                Cell::Sky => {
                    if y + 1 == ground {
                        // Surface strip: grass tufts and the foraging trail.
                        let base = if n % 100 < wx.tufts { wx.grass } else { wx.sky };
                        let t = (world.surf_ph[x].min(1.0) * 6.0).round() / 6.0;
                        if t > 0.0 {
                            palette::lerp(base, palette::TRAIL, 0.15 + 0.6 * t)
                        } else {
                            base
                        }
                    } else if wx.sun && (x as i32 - sun.0).pow(2) + (y as i32 - sun.1).pow(2) <= 5 {
                        palette::SUN
                    } else if n % 1000 < 5 {
                        palette::STAR
                    } else {
                        wx.sky
                    }
                }
                Cell::Soil => {
                    let depth = y.saturating_sub(ground) as i16;
                    let var = (n % 17) as i16 - 8;
                    if n % 100 < 4 {
                        palette::shade(wx.pebble, var - depth.min(30) / 2)
                    } else {
                        palette::shade(wx.soil, var - depth.min(30))
                    }
                }
                Cell::Tunnel | Cell::Chamber | Cell::Royal => {
                    let i = y * w + x;
                    let base = match world.cell(x, y) {
                        Cell::Tunnel => palette::TUNNEL,
                        Cell::Royal => palette::lerp(palette::ROYAL, palette::FOOD, world.store[i] as f32 / STORE_CAP),
                        _ => palette::lerp(palette::CHAMBER, palette::FOOD, world.store[i] as f32 / STORE_CAP),
                    };
                    let nest = world.nest_of[i];
                    if nest != NO_NEST && world.nests[nest as usize].dead {
                        palette::ABANDONED
                    } else {
                        base
                    }
                }
            };
            px[y * w + x] = c;
        }
    }

    for food in &world.food {
        let gy = world.ground[food.x];
        let mut put = |x: i32, y: i32| {
            if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                px[y as usize * w + x as usize] = palette::FOOD;
            }
        };
        let (x, y) = (food.x as i32, gy as i32 - 1);
        put(x, y);
        if food.units > 20 {
            put(x, y - 1);
        }
        if food.units > 45 {
            put(x - 1, y);
            put(x + 1, y);
        }
        if food.units > 70 {
            put(x - 1, y - 1);
            put(x + 1, y - 1);
        }
    }

    // The planned royal chamber shows through the soil while it is being dug.
    for nest in &world.nests {
        if let Some(r) = &nest.royal {
            for &c in &r.cells {
                px[c] = palette::ROYAL_PLAN;
            }
        }
    }

    for egg in &world.eggs {
        px[egg.cell] = palette::EGG;
    }

    for c in &world.corpses {
        if c.x < w && c.y < h {
            let under = px[c.y * w + c.x];
            let fade = c.ttl as f32 / c.max_ttl as f32;
            px[c.y * w + c.x] = palette::lerp(under, palette::CORPSE, 0.3 + 0.7 * fade);
            if c.queen && c.x + 1 < w && world.cell(c.x + 1, c.y).dug() {
                px[c.y * w + c.x + 1] = palette::lerp(under, palette::CORPSE, 0.3 + 0.7 * fade);
            }
        }
    }

    for ant in &world.ants {
        if ant.x < w && ant.y < h {
            let base = match (ant.state, ant.job) {
                (State::Starving, _) => palette::ANT_STARVING,
                (State::Returning, _) => palette::ANT_CARRYING,
                (State::Founding(_), _) => palette::ANT_FOUNDING,
                (_, Job::Forager) => palette::ANT,
                (_, Job::Digger) => palette::ANT_DIGGING,
                (_, Job::Scout) => palette::ANT_SCOUT,
                (_, Job::Nurse) => palette::ANT_NURSE,
            };
            // Hungry ants go dim.
            px[ant.y * w + ant.x] = if ant.fed < HUNGRY && ant.state != State::Starving {
                palette::lerp(base, palette::TUNNEL, 0.45)
            } else {
                base
            };
        }
    }

    // The queen is two pixels wide and never dimmed; her second pixel fills a dug cell.
    for nest in &world.nests {
        if let Some(q) = &nest.queen {
            let c = if q.relocating { palette::QUEEN_MOVING } else { palette::QUEEN };
            px[q.y * w + q.x] = c;
            if q.x + 1 < w && world.cell(q.x + 1, q.y).dug() {
                px[q.y * w + q.x + 1] = c;
            }
        }
    }
    px
}

fn color(c: Rgb) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

impl Widget for Colony<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let world = self.world;
        let px = paint(world);
        let cols = (area.width as usize).min(world.w);
        let rows = (area.height as usize).min(world.h / 2);
        for cy in 0..rows {
            for cx in 0..cols {
                let top = px[(2 * cy) * world.w + cx];
                let bottom = px[(2 * cy + 1) * world.w + cx];
                let cell = &mut buf[(area.x + cx as u16, area.y + cy as u16)];
                cell.set_symbol("▀");
                cell.set_fg(color(top));
                cell.set_bg(color(bottom));
            }
        }
        if let Some(text) = self.status {
            if area.height >= 6 {
                let text: String = text.chars().take(area.width as usize).collect();
                let len = text.chars().count() as u16;
                let x = area.x + area.width - len;
                let y = area.y + area.height - 1;
                buf.set_string(
                    x,
                    y,
                    &text,
                    Style::default().fg(color(palette::STATUS)).bg(color(palette::STATUS_BG)),
                );
            }
        }
    }
}
