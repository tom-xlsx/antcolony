//! The handful of colours the colony is drawn in, and the pheromone blend.

pub type Rgb = (u8, u8, u8);

pub const BACKGROUND: Rgb = (22, 20, 18);
pub const ANT: Rgb = (236, 232, 220);
pub const ANT_CARRYING: Rgb = (190, 240, 150);
pub const ANT_FOUNDING: Rgb = (255, 196, 110);
pub const FOOD: Rgb = (92, 200, 72);
pub const NEST: Rgb = (96, 66, 42);
pub const NEST_RIM: Rgb = (134, 98, 62);
pub const NEST_CHAMBER: Rgb = (56, 38, 24);
pub const STATUS: Rgb = (120, 112, 100);

const HOME_TRAIL: Rgb = (72, 112, 205);
const FOOD_TRAIL: Rgb = (214, 152, 42);

/// Colour of a pixel carrying `home` and `food` pheromone (both 0..=1).
/// Faint trails get a floor so they stay visible; strong ones saturate.
pub fn trail(home: f32, food: f32) -> Rgb {
    let total = home + food;
    if total < 0.004 {
        return BACKGROUND;
    }
    let fh = home / total;
    let ff = food / total;
    let mix = (
        HOME_TRAIL.0 as f32 * fh + FOOD_TRAIL.0 as f32 * ff,
        HOME_TRAIL.1 as f32 * fh + FOOD_TRAIL.1 as f32 * ff,
        HOME_TRAIL.2 as f32 * fh + FOOD_TRAIL.2 as f32 * ff,
    );
    // Perceptual-ish curve: sqrt brings faint trails up, 0.12 floor keeps them above the background.
    let t = (0.12 + 0.88 * total.min(1.0).sqrt()).min(1.0);
    lerp(BACKGROUND, (mix.0 as u8, mix.1 as u8, mix.2 as u8), t)
}

pub fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}
