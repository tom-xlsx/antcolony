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
use crate::sim::{Cell, State, World};

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

/// Flat RGB pixel buffer the size of the world.
pub fn paint(world: &World) -> Vec<Rgb> {
    let (w, h) = (world.w, world.h);
    let mut px = vec![palette::SKY; w * h];

    for y in 0..h {
        for x in 0..w {
            let n = hash(x, y);
            let ground = world.ground[x];
            let c = match world.cell(x, y) {
                Cell::Sky => {
                    if y + 1 == ground {
                        // Surface strip: grass tufts and the foraging trail.
                        let base = if n % 100 < 12 { palette::GRASS } else { palette::SKY };
                        let t = (world.surf_ph[x].min(1.0) * 6.0).round() / 6.0;
                        if t > 0.0 {
                            palette::lerp(base, palette::TRAIL, 0.15 + 0.6 * t)
                        } else {
                            base
                        }
                    } else if n % 1000 < 5 {
                        palette::STAR
                    } else {
                        palette::SKY
                    }
                }
                Cell::Soil => {
                    let depth = y.saturating_sub(ground) as i16;
                    let var = (n % 17) as i16 - 8;
                    if n % 100 < 4 {
                        palette::shade(palette::PEBBLE, var - depth.min(30) / 2)
                    } else {
                        palette::shade(palette::SOIL, var - depth.min(30))
                    }
                }
                Cell::Tunnel => palette::TUNNEL,
                Cell::Chamber => {
                    let s = world.store[y * w + x] as f32;
                    palette::lerp(palette::CHAMBER, palette::FOOD, s / STORE_CAP)
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

    for ant in &world.ants {
        if ant.x < w && ant.y < h {
            px[ant.y * w + ant.x] = match ant.state {
                State::Returning => palette::ANT_CARRYING,
                State::Digging => palette::ANT_DIGGING,
                State::Founding(_) => palette::ANT_FOUNDING,
                State::Foraging | State::ToTip => palette::ANT,
            };
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
