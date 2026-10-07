//! The handful of colours the farm is drawn in.

pub type Rgb = (u8, u8, u8);

pub const SKY: Rgb = (10, 12, 22);
pub const STAR: Rgb = (48, 52, 80);
pub const GRASS: Rgb = (44, 112, 44);
pub const SOIL: Rgb = (96, 66, 42);
pub const PEBBLE: Rgb = (130, 108, 84);
pub const TUNNEL: Rgb = (30, 22, 15);
pub const CHAMBER: Rgb = (42, 30, 20);
pub const FOOD: Rgb = (92, 200, 72);
pub const TRAIL: Rgb = (214, 152, 42);
pub const ANT: Rgb = (236, 232, 220);
pub const ANT_CARRYING: Rgb = (190, 240, 150);
pub const ANT_DIGGING: Rgb = (224, 188, 132);
pub const ANT_FOUNDING: Rgb = (255, 196, 110);
pub const STATUS: Rgb = (150, 140, 124);
pub const STATUS_BG: Rgb = (24, 18, 12);

pub fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

pub fn shade(c: Rgb, delta: i16) -> Rgb {
    let f = |v: u8| (v as i16 + delta).clamp(0, 255) as u8;
    (f(c.0), f(c.1), f(c.2))
}
