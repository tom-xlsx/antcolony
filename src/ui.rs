//! Draws a `World` into a ratatui buffer. Each terminal cell is two stacked pixels:
//! `▀` with the foreground as the top pixel and the background as the bottom one.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

use crate::palette::{self, Rgb};
use crate::sim::{State, World};

pub struct Colony<'a> {
    pub world: &'a World,
    pub status: Option<String>,
}

/// Flat RGB pixel buffer the size of the world, painted in priority order.
pub fn paint(world: &World) -> Vec<Rgb> {
    let (w, h) = (world.w, world.h);
    let mut px: Vec<Rgb> = (0..w * h)
        .map(|i| palette::trail(world.home_ph[i], world.food_ph[i]))
        .collect();

    for nest in &world.nests {
        let r = nest.radius();
        let mut rng = fastrand::Rng::with_seed(nest.texture_seed);
        let x0 = (nest.x - r - 1.0).max(0.0) as usize;
        let y0 = (nest.y - r - 1.0).max(0.0) as usize;
        let x1 = ((nest.x + r + 1.0) as usize).min(w - 1);
        let y1 = ((nest.y + r + 1.0) as usize).min(h - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 + 0.5 - nest.x).powi(2) + (y as f32 + 0.5 - nest.y).powi(2)).sqrt();
                if d > r {
                    continue;
                }
                px[y * w + x] = if d > r - 1.0 {
                    palette::NEST_RIM
                } else if rng.u8(..) < 60 {
                    palette::NEST_CHAMBER
                } else {
                    palette::NEST
                };
            }
        }
    }

    for food in &world.food {
        let r = food.radius();
        let x0 = (food.x - r - 1.0).max(0.0) as usize;
        let y0 = (food.y - r - 1.0).max(0.0) as usize;
        let x1 = ((food.x + r + 1.0) as usize).min(w - 1);
        let y1 = ((food.y + r + 1.0) as usize).min(h - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 + 0.5 - food.x).powi(2) + (y as f32 + 0.5 - food.y).powi(2)).sqrt();
                if d <= r {
                    px[y * w + x] = palette::FOOD;
                }
            }
        }
    }

    for ant in &world.ants {
        let (x, y) = (ant.x as usize, ant.y as usize);
        if x < w && y < h {
            px[y * w + x] = match ant.state {
                State::Foraging => palette::ANT,
                State::Returning => palette::ANT_CARRYING,
                State::Founding => palette::ANT_FOUNDING,
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
                    Style::default()
                        .fg(color(palette::STATUS))
                        .bg(color(palette::BACKGROUND)),
                );
            }
        }
    }
}
