//! The colony itself. No terminal types in here: everything is plain numbers and grids
//! so it can be unit-tested and run headless.

use std::f32::consts::PI;

#[derive(Clone, Debug)]
pub struct Config {
    /// Starting ants. `None` scales with the world area.
    pub ants: Option<usize>,
    /// Food piles kept in the world at once.
    pub food_piles: usize,
    /// Spawn more ants as food comes in.
    pub grow: bool,
    pub max_ants: usize,
    /// Let a crowded nest send out a founding party to dig a new one.
    pub bud: bool,
    pub max_nests: usize,
    pub seed: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ants: None,
            food_piles: 3,
            grow: true,
            max_ants: 200,
            bud: true,
            max_nests: 4,
            seed: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Foraging,
    Returning,
    /// Marching off with a founding party to dig a new nest.
    Founding,
}

#[derive(Clone, Debug)]
pub struct Ant {
    pub x: f32,
    pub y: f32,
    pub heading: f32,
    pub state: State,
    /// Index into `World::nests`.
    pub home: usize,
    /// How strongly this ant lays pheromone. Starts at 1 on each pickup or delivery
    /// and fades with distance, so long aimless wanders leave only faint marks.
    strength: f32,
}

#[derive(Clone, Debug)]
pub struct Nest {
    pub x: f32,
    pub y: f32,
    pub population: usize,
    pub delivered: u64,
    /// Seed for the stable chamber texture when drawn.
    pub texture_seed: u64,
    bud_cooldown: u32,
}

/// A group of ants that has left a nest to found a new one. Followers trail the leader.
#[derive(Clone, Debug)]
pub struct Party {
    pub leader: usize,
    pub members: Vec<usize>,
    pub from: usize,
    pub steps: u32,
}

impl Nest {
    /// Dug-out radius in pixels. Area tracks population, so radius goes with its square root.
    pub fn radius(&self) -> f32 {
        1.5 + 0.35 * (self.population as f32).sqrt()
    }
}

#[derive(Clone, Debug)]
pub struct Food {
    pub x: f32,
    pub y: f32,
    pub units: u32,
}

impl Food {
    pub fn radius(&self) -> f32 {
        0.6 + 0.18 * (self.units as f32).sqrt()
    }
}

pub struct World {
    pub w: usize,
    pub h: usize,
    pub nests: Vec<Nest>,
    pub ants: Vec<Ant>,
    pub food: Vec<Food>,
    /// "A nest is this way." Laid by foraging ants, followed by returning ones.
    pub home_ph: Vec<f32>,
    /// "Food is this way." Laid by returning ants, followed by foraging ones.
    pub food_ph: Vec<f32>,
    pub parties: Vec<Party>,
    pub tick: u64,
    pub delivered: u64,
    food_respawn: u32,
    cfg: Config,
    rng: fastrand::Rng,
}

const STEP: f32 = 0.35;
const JITTER: f32 = 0.22;
const SENSOR_DIST: f32 = 3.0;
const SENSOR_ANGLE: f32 = 0.55;
const TURN: f32 = 0.45;
const VISION: f32 = 6.0;
const DEPOSIT: f32 = 0.06;
const STRENGTH_DECAY: f32 = 0.995;
const EVAPORATION: f32 = 0.993;
const BLUR_EVERY: u64 = 3;
const PICKUP_SLACK: f32 = 0.6;
const GROW_EVERY: u64 = 10;
const FOOD_RESPAWN_TICKS: u32 = 150;
const FOOD_NEST_CLEARANCE: f32 = 8.0;
const BUD_POPULATION: usize = 40;
const BUD_PARTY: usize = 8;
const BUD_COOLDOWN: u32 = 1200;
const BUD_MIN_STEPS: u32 = 40;
const BUD_GIVE_UP_STEPS: u32 = 700;
const NEST_GAP: f32 = 10.0;

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
            bud_cooldown: BUD_COOLDOWN,
        };
        let mut world = Self {
            w,
            h,
            nests: vec![nest],
            ants: Vec::with_capacity(n_ants),
            food: Vec::new(),
            parties: Vec::new(),
            home_ph: vec![0.0; w * h],
            food_ph: vec![0.0; w * h],
            tick: 0,
            delivered: 0,
            food_respawn: 0,
            cfg,
            rng,
        };
        for _ in 0..n_ants {
            world.spawn_ant(0);
        }
        for _ in 0..world.cfg.food_piles {
            world.spawn_food();
        }
        world
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
            strength: 1.0,
        });
    }

    /// Drop a new pile somewhere clear of nests and other food. Gives up quietly if the
    /// world is too small to find a spot.
    fn spawn_food(&mut self) {
        let units = self.rng.u32(100..=260);
        for _ in 0..40 {
            let x = 2.0 + self.rng.f32() * (self.w as f32 - 4.0);
            let y = 2.0 + self.rng.f32() * (self.h as f32 - 4.0);
            let clear_of_nests = self
                .nests
                .iter()
                .all(|n| dist(x, y, n.x, n.y) > n.radius() + FOOD_NEST_CLEARANCE);
            let clear_of_food = self.food.iter().all(|f| dist(x, y, f.x, f.y) > f.radius() + 4.0);
            if clear_of_nests && clear_of_food {
                self.food.push(Food { x, y, units });
                return;
            }
        }
    }

    /// Drop a pile at an exact spot (mouse click, `f` key).
    pub fn add_food_at(&mut self, x: f32, y: f32) {
        let units = self.rng.u32(80..=160);
        let x = x.clamp(0.0, self.w as f32 - 1.0);
        let y = y.clamp(0.0, self.h as f32 - 1.0);
        self.food.push(Food { x, y, units });
    }

    pub fn tick(&mut self) {
        self.tick += 1;
        for i in 0..self.ants.len() {
            self.step_ant(i);
        }
        self.settle_parties();
        if self.cfg.bud {
            self.maybe_bud();
        }
        self.evaporate();
        if self.tick % BLUR_EVERY == 0 {
            blur(&mut self.home_ph, self.w, self.h);
            blur(&mut self.food_ph, self.w, self.h);
        }
        self.food.retain(|f| f.units > 0);
        if self.food.len() < self.cfg.food_piles {
            if self.food_respawn == 0 {
                self.spawn_food();
                self.food_respawn = FOOD_RESPAWN_TICKS;
            } else {
                self.food_respawn -= 1;
            }
        }
    }

    fn step_ant(&mut self, i: usize) {
        if self.ants[i].state == State::Founding {
            self.step_founder(i);
            return;
        }
        let (w, h) = (self.w as f32, self.h as f32);
        let ant = self.ants[i].clone();
        let mut heading = ant.heading;

        // Steer: something in sight beats the trail, the trail beats wandering.
        let target = match ant.state {
            State::Founding => unreachable!(),
            State::Foraging => self
                .food
                .iter()
                .map(|f| (f.x, f.y, dist(ant.x, ant.y, f.x, f.y) - f.radius()))
                .filter(|&(_, _, d)| d < VISION)
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(x, y, _)| (x, y)),
            State::Returning => self
                .nests
                .iter()
                .map(|n| (n.x, n.y, dist(ant.x, ant.y, n.x, n.y) - n.radius()))
                .filter(|&(_, _, d)| d < VISION * 1.5)
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(x, y, _)| (x, y)),
        };
        if let Some((tx, ty)) = target {
            let want = (ty - ant.y).atan2(tx - ant.x);
            heading += angle_diff(heading, want).clamp(-TURN * 1.5, TURN * 1.5);
        } else {
            let grid = match ant.state {
                State::Foraging | State::Founding => &self.food_ph,
                State::Returning => &self.home_ph,
            };
            let l = self.sample(grid, ant.x, ant.y, heading - SENSOR_ANGLE);
            let c = self.sample(grid, ant.x, ant.y, heading);
            let r = self.sample(grid, ant.x, ant.y, heading + SENSOR_ANGLE);
            if l.max(c).max(r) > 0.01 {
                if c >= l && c >= r {
                    // keep going
                } else if l > r {
                    heading -= TURN;
                } else {
                    heading += TURN;
                }
            }
        }
        heading += (self.rng.f32() - 0.5) * 2.0 * JITTER;

        // Move, bouncing off the edges.
        let mut nx = ant.x + heading.cos() * STEP;
        let mut ny = ant.y + heading.sin() * STEP;
        if nx < 0.0 || nx >= w {
            heading = PI - heading;
            nx = nx.clamp(0.0, w - 0.001);
        }
        if ny < 0.0 || ny >= h {
            heading = -heading;
            ny = ny.clamp(0.0, h - 0.001);
        }

        // Lay pheromone where we are.
        let idx = ny as usize * self.w + nx as usize;
        let grid = match ant.state {
            State::Foraging | State::Founding => &mut self.home_ph,
            State::Returning => &mut self.food_ph,
        };
        grid[idx] = (grid[idx] + DEPOSIT * ant.strength).min(1.0);
        let mut strength = ant.strength * STRENGTH_DECAY;
        let mut state = ant.state;
        let mut home = ant.home;

        // Pick up or deliver.
        match state {
            State::Founding => {}
            State::Foraging => {
                if let Some(f) = self
                    .food
                    .iter_mut()
                    .find(|f| f.units > 0 && dist(nx, ny, f.x, f.y) <= f.radius() + PICKUP_SLACK)
                {
                    f.units -= 1;
                    state = State::Returning;
                    strength = 1.0;
                    heading += PI;
                }
            }
            State::Returning => {
                if let Some(n) = self
                    .nests
                    .iter()
                    .position(|n| dist(nx, ny, n.x, n.y) <= n.radius() + PICKUP_SLACK)
                {
                    self.deliver(n);
                    if n != home {
                        // Delivered to another nest: that is home now.
                        self.nests[home].population = self.nests[home].population.saturating_sub(1);
                        self.nests[n].population += 1;
                    }
                    home = n;
                    state = State::Foraging;
                    strength = 1.0;
                    heading += PI;
                }
            }
        }

        let ant = &mut self.ants[i];
        ant.x = nx;
        ant.y = ny;
        ant.heading = heading.rem_euclid(2.0 * PI);
        ant.state = state;
        ant.strength = strength;
        ant.home = home;
    }

    /// Founders march in a loose group behind their leader and lay no trail.
    fn step_founder(&mut self, i: usize) {
        let (w, h) = (self.w as f32, self.h as f32);
        let leader = self
            .parties
            .iter()
            .find(|p| p.members.contains(&i))
            .map(|p| p.leader);
        let ant = &self.ants[i];
        let mut heading = ant.heading;
        match leader {
            Some(l) if l != i => {
                let (lx, ly) = (self.ants[l].x, self.ants[l].y);
                if dist(ant.x, ant.y, lx, ly) > 1.5 {
                    let want = (ly - ant.y).atan2(lx - ant.x);
                    heading += angle_diff(heading, want).clamp(-TURN, TURN);
                }
                heading += (self.rng.f32() - 0.5) * 2.0 * JITTER;
            }
            _ => heading += (self.rng.f32() - 0.5) * 2.0 * JITTER * 0.4,
        }
        let ant = &mut self.ants[i];
        let mut nx = ant.x + heading.cos() * STEP;
        let mut ny = ant.y + heading.sin() * STEP;
        if nx < 0.0 || nx >= w {
            heading = PI - heading;
            nx = nx.clamp(0.0, w - 0.001);
        }
        if ny < 0.0 || ny >= h {
            heading = -heading;
            ny = ny.clamp(0.0, h - 0.001);
        }
        ant.x = nx;
        ant.y = ny;
        ant.heading = heading.rem_euclid(2.0 * PI);
    }

    /// A crowded, established nest sends a party of foragers off to dig a new nest.
    fn maybe_bud(&mut self) {
        for n in 0..self.nests.len() {
            let nest = &mut self.nests[n];
            nest.bud_cooldown = nest.bud_cooldown.saturating_sub(1);
            if nest.bud_cooldown > 0
                || nest.population < BUD_POPULATION
                || self.nests.len() + self.parties.len() >= self.cfg.max_nests
            {
                continue;
            }
            let members: Vec<usize> = self
                .ants
                .iter()
                .enumerate()
                .filter(|(_, a)| a.home == n && a.state == State::Foraging)
                .map(|(i, _)| i)
                .take(BUD_PARTY)
                .collect();
            if members.len() < BUD_PARTY {
                continue;
            }
            let heading = self.rng.f32() * 2.0 * PI;
            for &i in &members {
                let a = &mut self.ants[i];
                a.state = State::Founding;
                a.heading = heading;
            }
            self.nests[n].population -= members.len();
            self.nests[n].bud_cooldown = BUD_COOLDOWN;
            self.parties.push(Party { leader: members[0], members, from: n, steps: 0 });
        }
    }

    /// Parties dig in once their leader is somewhere clear, or give up and go home.
    fn settle_parties(&mut self) {
        let mut p = 0;
        while p < self.parties.len() {
            self.parties[p].steps += 1;
            let party = &self.parties[p];
            let (lx, ly) = (self.ants[party.leader].x, self.ants[party.leader].y);
            let clear = party.steps >= BUD_MIN_STEPS
                && lx > 6.0
                && ly > 6.0
                && lx < self.w as f32 - 6.0
                && ly < self.h as f32 - 6.0
                && self.nests.iter().all(|n| dist(lx, ly, n.x, n.y) > n.radius() + NEST_GAP)
                && self.food.iter().all(|f| dist(lx, ly, f.x, f.y) > f.radius() + 3.0);
            if clear {
                let party = self.parties.remove(p);
                let idx = self.nests.len();
                self.nests.push(Nest {
                    x: lx,
                    y: ly,
                    population: party.members.len(),
                    delivered: 0,
                    texture_seed: self.rng.u64(..),
                    bud_cooldown: BUD_COOLDOWN,
                });
                for i in party.members {
                    let a = &mut self.ants[i];
                    a.state = State::Foraging;
                    a.home = idx;
                    a.strength = 1.0;
                }
            } else if party.steps > BUD_GIVE_UP_STEPS {
                let party = self.parties.remove(p);
                self.nests[party.from].population += party.members.len();
                for i in party.members {
                    let a = &mut self.ants[i];
                    a.state = State::Foraging;
                    a.strength = 1.0;
                }
            } else {
                p += 1;
            }
        }
    }

    fn deliver(&mut self, nest: usize) {
        self.delivered += 1;
        let n = &mut self.nests[nest];
        n.delivered += 1;
        if self.cfg.grow && n.delivered % GROW_EVERY == 0 && self.ants.len() < self.cfg.max_ants {
            n.population += 1;
            self.spawn_ant(nest);
        }
    }

    fn sample(&self, grid: &[f32], x: f32, y: f32, angle: f32) -> f32 {
        let sx = x + angle.cos() * SENSOR_DIST;
        let sy = y + angle.sin() * SENSOR_DIST;
        if sx < 0.0 || sy < 0.0 || sx >= self.w as f32 || sy >= self.h as f32 {
            return 0.0;
        }
        grid[sy as usize * self.w + sx as usize]
    }

    fn evaporate(&mut self) {
        for grid in [&mut self.home_ph, &mut self.food_ph] {
            for v in grid.iter_mut() {
                *v *= EVAPORATION;
                if *v < 0.001 {
                    *v = 0.0;
                }
            }
        }
    }

    /// Plain-text picture of the world, one char per pixel, for tests and `--headless`.
    pub fn snapshot(&self) -> String {
        let mut out = String::with_capacity((self.w + 1) * self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let ant = self.ants.iter().find(|a| a.x as usize == x && a.y as usize == y);
                let ch = if let Some(a) = ant {
                    match a.state {
                        State::Returning => 'A',
                        State::Foraging => 'a',
                        State::Founding => '*',
                    }
                } else if self.nests.iter().any(|n| dist(fx, fy, n.x, n.y) <= n.radius()) {
                    'N'
                } else if self.food.iter().any(|f| dist(fx, fy, f.x, f.y) <= f.radius()) {
                    'F'
                } else {
                    let p = self.home_ph[y * self.w + x].max(self.food_ph[y * self.w + x]);
                    if p > 0.3 {
                        '#'
                    } else if p > 0.08 {
                        ':'
                    } else if p > 0.015 {
                        '.'
                    } else {
                        ' '
                    }
                };
                out.push(ch);
            }
            out.push('\n');
        }
        out
    }

    pub fn stats(&self) -> String {
        format!(
            "tick {} · nests {} · ants {} · food {} piles · delivered {}",
            self.tick,
            self.nests.len(),
            self.ants.len(),
            self.food.len(),
            self.delivered
        )
    }
}

fn dist(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
}

/// Signed shortest rotation from `from` to `to`, in -PI..=PI.
fn angle_diff(from: f32, to: f32) -> f32 {
    let d = (to - from).rem_euclid(2.0 * PI);
    if d > PI {
        d - 2.0 * PI
    } else {
        d
    }
}

/// Cheap 4-neighbour blur, mass-preserving in the interior, so trails spread a little.
fn blur(grid: &mut [f32], w: usize, h: usize) {
    if w < 3 || h < 3 {
        return;
    }
    let src = grid.to_vec();
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            let sum = src[i - 1] + src[i + 1] + src[i - w] + src[i + w];
            grid[i] = src[i] * 0.85 + sum * 0.0375;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        World::new(
            Config {
                ants: Some(10),
                food_piles: 0,
                ..Config::default()
            },
            40,
            30,
        )
    }

    #[test]
    fn ant_next_to_food_picks_it_up_and_turns_home() {
        let mut w = world();
        w.food.push(Food { x: 10.0, y: 10.0, units: 5 });
        w.ants[0] = Ant { x: 10.5, y: 10.0, heading: 0.0, state: State::Foraging, home: 0, strength: 0.2 };
        w.step_ant(0);
        assert_eq!(w.ants[0].state, State::Returning);
        assert_eq!(w.food[0].units, 4);
        assert_eq!(w.ants[0].strength, 1.0);
    }

    #[test]
    fn returning_ant_at_nest_delivers() {
        let mut w = world();
        let (nx, ny) = (w.nests[0].x, w.nests[0].y);
        w.ants[0] = Ant { x: nx, y: ny, heading: 0.0, state: State::Returning, home: 0, strength: 0.5 };
        w.step_ant(0);
        assert_eq!(w.ants[0].state, State::Foraging);
        assert_eq!(w.delivered, 1);
        assert_eq!(w.nests[0].delivered, 1);
    }

    #[test]
    fn pheromone_evaporates_to_nothing() {
        let mut w = world();
        w.home_ph[5] = 1.0;
        for _ in 0..2000 {
            w.evaporate();
        }
        assert_eq!(w.home_ph[5], 0.0);
    }

    #[test]
    fn growth_spawns_an_ant_every_few_deliveries() {
        let mut w = world();
        for _ in 0..GROW_EVERY {
            w.deliver(0);
        }
        assert_eq!(w.ants.len(), 11);
        assert_eq!(w.nests[0].population, 11);
    }

    #[test]
    fn tiny_and_odd_sizes_do_not_panic() {
        for (cw, ch) in [(1, 1), (4, 4), (5, 3), (71, 48), (200, 7)] {
            let mut w = World::new(Config::default(), cw, ch);
            for _ in 0..50 {
                w.tick();
            }
            let _ = w.snapshot();
        }
    }

    #[test]
    fn crowded_nest_founds_a_second_one_at_a_distance() {
        let mut w = World::new(Config { seed: 7, ..Config::default() }, 71, 48);
        let mut found_at = None;
        for t in 0..8000 {
            w.tick();
            if w.nests.len() >= 2 {
                found_at = Some(t);
                break;
            }
        }
        assert!(found_at.is_some(), "no second nest after 8000 ticks:\n{}", w.snapshot());
        let (a, b) = (&w.nests[0], &w.nests[1]);
        assert!(dist(a.x, a.y, b.x, b.y) > a.radius() + NEST_GAP);
        assert_eq!(b.population, BUD_PARTY);
        assert!(w.parties.is_empty());
        assert!(w.ants.iter().all(|a| a.state != State::Founding));
    }

    #[test]
    fn no_bud_keeps_one_nest() {
        let mut w = World::new(Config { seed: 7, bud: false, ..Config::default() }, 71, 48);
        for _ in 0..6000 {
            w.tick();
        }
        assert_eq!(w.nests.len(), 1);
    }

    #[test]
    fn trails_form_between_nest_and_food() {
        let mut w = World::new(Config { seed: 7, ..Config::default() }, 71, 48);
        for _ in 0..3000 {
            w.tick();
        }
        assert!(w.delivered > 20, "only {} deliveries:\n{}", w.delivered, w.snapshot());
        let strong = w.food_ph.iter().filter(|&&v| v > 0.08).count();
        assert!(strong > 30, "no real food trail ({strong} cells):\n{}", w.snapshot());
    }
}
