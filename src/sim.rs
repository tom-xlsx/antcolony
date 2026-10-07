//! The colony itself. No terminal types in here: everything is plain numbers and grids
//! so it can be unit-tested and run headless.

use std::f32::consts::PI;

#[derive(Clone, Debug)]
pub struct Config {
    /// Starting ants. `None` scales with the world area.
    pub ants: Option<usize>,
    pub seed: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self { ants: None, seed: 1 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Foraging,
}

#[derive(Clone, Debug)]
pub struct Ant {
    pub x: f32,
    pub y: f32,
    pub heading: f32,
    pub state: State,
    /// Index into `World::nests`.
    pub home: usize,
}

#[derive(Clone, Debug)]
pub struct Nest {
    pub x: f32,
    pub y: f32,
    pub population: usize,
    pub delivered: u64,
    /// Seed for the stable chamber texture when drawn.
    pub texture_seed: u64,
}

impl Nest {
    /// Dug-out radius in pixels. Area tracks population, so radius goes with its square root.
    pub fn radius(&self) -> f32 {
        1.5 + 0.35 * (self.population as f32).sqrt()
    }
}

pub struct World {
    pub w: usize,
    pub h: usize,
    pub nests: Vec<Nest>,
    pub ants: Vec<Ant>,
    pub tick: u64,
    cfg: Config,
    rng: fastrand::Rng,
}

const STEP: f32 = 0.35;
const JITTER: f32 = 0.35;

impl World {
    pub fn new(cfg: Config, w: usize, h: usize) -> Self {
        let w = w.max(4);
        let h = h.max(4);
        let mut rng = fastrand::Rng::with_seed(cfg.seed);
        let n_ants = cfg
            .ants
            .unwrap_or_else(|| ((w * h) / 60).clamp(20, 300));
        let nest = Nest {
            x: w as f32 / 2.0,
            y: h as f32 / 2.0,
            population: n_ants,
            delivered: 0,
            texture_seed: rng.u64(..),
        };
        let mut world = Self {
            w,
            h,
            nests: vec![nest],
            ants: Vec::with_capacity(n_ants),
            tick: 0,
            cfg,
            rng,
        };
        for _ in 0..n_ants {
            world.spawn_ant(0);
        }
        world
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Spawn an ant on the rim of nest `home`, facing outward.
    fn spawn_ant(&mut self, home: usize) {
        let nest = &self.nests[home];
        let heading = self.rng.f32() * 2.0 * PI;
        let r = nest.radius();
        self.ants.push(Ant {
            x: nest.x + heading.cos() * r,
            y: nest.y + heading.sin() * r,
            heading,
            state: State::Foraging,
            home,
        });
    }

    pub fn tick(&mut self) {
        self.tick += 1;
        for i in 0..self.ants.len() {
            self.step_ant(i);
        }
    }

    fn step_ant(&mut self, i: usize) {
        let w = self.w as f32;
        let h = self.h as f32;
        let ant = &mut self.ants[i];
        ant.heading += (self.rng.f32() - 0.5) * 2.0 * JITTER;
        let mut nx = ant.x + ant.heading.cos() * STEP;
        let mut ny = ant.y + ant.heading.sin() * STEP;
        if nx < 0.0 || nx >= w {
            ant.heading = PI - ant.heading;
            nx = nx.clamp(0.0, w - 0.001);
        }
        if ny < 0.0 || ny >= h {
            ant.heading = -ant.heading;
            ny = ny.clamp(0.0, h - 0.001);
        }
        ant.x = nx;
        ant.y = ny;
    }
}
