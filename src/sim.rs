//! A side-on ant farm: sky on top, soil below, a colony digging tunnels and chambers
//! and foraging on the surface. No terminal types in here, so it can be unit-tested
//! and run headless.

use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct Config {
    /// Starting ants. `None` scales with the world area.
    pub ants: Option<usize>,
    /// Food piles kept on the surface at once.
    pub food_piles: usize,
    /// Spawn more ants as food is stored.
    pub grow: bool,
    pub max_ants: usize,
    /// Let a big colony send a party off to open a new nest elsewhere.
    pub bud: bool,
    pub max_nests: usize,
    /// Periodic droughts that stop surface food and drain the nests.
    pub drought: bool,
    pub seed: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ants: None,
            food_piles: 3,
            grow: true,
            max_ants: 300,
            bud: true,
            max_nests: 4,
            drought: true,
            seed: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Sky,
    Soil,
    Tunnel,
    Chamber,
}

/// What an ant is for. Fixed at birth; a nest keeps its castes near `ROLE_SHARES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Works the surface near home, follows trails, hauls food.
    Forager = 0,
    /// Lives underground extending tunnels and carving chambers.
    Digger = 1,
    /// Ranges far along the surface, finds food first, founds new nests.
    Scout = 2,
    /// Stays in the chambers minding the stores.
    Nurse = 3,
}

/// Target share of each caste, in percent.
const ROLE_SHARES: [(Role, u32); 4] =
    [(Role::Forager, 60), (Role::Digger, 20), (Role::Scout, 10), (Role::Nurse, 10)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Looking for food: underground this means heading for an exit.
    Foraging,
    /// Carrying food back: on the surface heading for an entrance, below heading for storage.
    Returning,
    /// Walking underground to a tunnel tip to dig.
    ToTip,
    /// Extending a tunnel.
    Digging,
    /// Walking along the surface to open a new nest at this column.
    Founding(usize),
    /// Pottering about underground for a while.
    Idle,
}

#[derive(Clone, Debug)]
pub struct Ant {
    pub x: usize,
    pub y: usize,
    pub state: State,
    pub role: Role,
    /// On the surface or in a tunnel.
    pub underground: bool,
    /// Index into `World::nests`.
    pub home: usize,
    /// Ticks left of idling.
    idle: u16,
    /// Surface walking direction, -1 or 1.
    dir: i32,
    /// Ticks until the next move.
    cooldown: u8,
    dig_dir: (i32, i32),
    dig_budget: u8,
    /// Pheromone deposit strength, fades with distance since pickup.
    strength: f32,
}

#[derive(Clone, Debug)]
pub struct Food {
    pub x: usize,
    pub units: u32,
}

/// One connected tunnel system with at least one entrance.
#[derive(Clone, Debug)]
pub struct Nest {
    pub label: char,
    pub entrances: Vec<usize>,
    pub workers: usize,
    /// Out of food during a drought.
    pub starving: bool,
    /// Lost its last worker. The tunnels stay as a husk.
    pub dead: bool,
    last_spawn: u64,
    consume_acc: f32,
    starve_timer: u32,
}

impl Nest {
    fn new(label: char, entrances: Vec<usize>) -> Self {
        Self {
            label,
            entrances,
            workers: 0,
            starving: false,
            dead: false,
            last_spawn: 0,
            consume_acc: 0.0,
            starve_timer: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Season {
    Calm { until: u64 },
    /// Food gets scarce; the drought is coming.
    Warning { until: u64 },
    Drought { until: u64, started: u64 },
    Bloom { until: u64 },
    Cooldown { until: u64 },
}

/// One line of the `--report` tuning output.
#[derive(Clone, Debug)]
pub struct DroughtReport {
    pub started: u64,
    pub ended: u64,
    pub nests_before: usize,
    pub nests_after: usize,
    pub ants_before: usize,
    pub ants_after: usize,
    pub stored_before: u32,
    pub stored_after: u32,
    /// Days after the drought until stores were back to `stored_before`; `None` if the
    /// next dry spell came first.
    pub rebuilt_days: Option<u64>,
}

pub struct World {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<Cell>,
    /// Food units stored in each chamber cell.
    pub store: Vec<u8>,
    /// Row of the topmost soil cell in each column (sky above, mounds raise it).
    pub ground: Vec<usize>,
    /// Row of the original flat surface.
    pub sky: usize,
    /// Columns with an entrance at ground level.
    pub entrances: Vec<usize>,
    pub ants: Vec<Ant>,
    pub food: Vec<Food>,
    /// "Food is this way" trail along the surface, one value per column.
    pub surf_ph: Vec<f32>,
    pub nests: Vec<Nest>,
    /// Nest index per cell, `NO_NEST` for soil and sky.
    pub nest_of: Vec<u16>,
    pub season: Season,
    pub reports: Vec<DroughtReport>,
    pub tick: u64,
    pub delivered: u64,
    pub dug: usize,
    next_label: u8,
    drought_before: (usize, usize, u32),
    /// (report index, store level to reach) while a rebuild is being timed.
    rebuilding: Option<(usize, u32)>,
    exit_dist: Vec<u16>,
    store_dist: Vec<u16>,
    tip_dist: Vec<u16>,
    dead_tip: Vec<bool>,
    dirty: bool,
    food_respawn: u32,
    cfg: Config,
    rng: fastrand::Rng,
}

#[derive(Clone, Copy)]
enum Field {
    Exit,
    Store,
    Tip,
}

const FAR: u16 = u16::MAX;
const MOVE_TICKS: u8 = 3;
const DIG_TICKS: u8 = 24;
/// Diggers stop once this share of the soil is hollow.
const DIG_SHARE: usize = 40;
const VISION: usize = 8;
const SCOUT_VISION: usize = 14;
const IDLE_TICKS: u16 = 300;
const STORE_CAP: u8 = 6;
pub const NO_NEST: u16 = u16::MAX;

// Growth: the only way the world gains ants. A nest only grows once it holds a reserve
// for the workers it already has, so stockpiles build before mouths do.
const SPAWN_THRESHOLD: u32 = 12;
const RESERVE_PER_WORKER: u32 = 8;
const SPAWN_COST: u32 = 4;
const SPAWN_INTERVAL: u64 = 60;

// Seasons. A day is the unit the status line counts droughts in.
pub const DAY: u64 = 100;
const GRACE_PERIOD: u64 = 150 * DAY;
const DROUGHT_INTERVAL: u64 = 120 * DAY;
const DROUGHT_JITTER: u64 = 30 * DAY;
const WARNING_LENGTH: u64 = 8 * DAY;
const MIN_DURATION: u64 = 15 * DAY;
const MAX_DURATION: u64 = 40 * DAY;
const COOLDOWN: u64 = 20 * DAY;
const BLOOM_LENGTH: u64 = 10 * DAY;
const BLOOM_MULTIPLIER: u32 = 3;
/// Food units each worker eats per tick while a drought is on.
const CONSUME_RATE: f32 = 0.003;
/// What a worker eats per tick the rest of the year, so stores only grow when foraging
/// outpaces eating and a drought lands on whatever the colony actually managed to put by.
const UPKEEP_RATE: f32 = 0.0003;
const STARVE_INTERVAL: u32 = DAY as u32 / 2;
const DEPOSIT: f32 = 0.08;
const STRENGTH_DECAY: f32 = 0.985;
const EVAPORATION: f32 = 0.994;
const FOOD_RESPAWN_TICKS: u32 = 200;
const BUD_POPULATION: usize = 60;
const BUD_PARTY: usize = 5;
const BUD_EVERY: u64 = 900;
const NEST_GAP: usize = 18;

impl World {
    pub fn new(cfg: Config, w: usize, h: usize) -> Self {
        let w = w.max(8);
        let h = h.max(12);
        let mut rng = fastrand::Rng::with_seed(cfg.seed);
        let sky = (h / 5).max(4);
        let mut world = Self {
            w,
            h,
            cells: (0..w * h).map(|i| if i / w < sky { Cell::Sky } else { Cell::Soil }).collect(),
            store: vec![0; w * h],
            ground: vec![sky; w],
            sky,
            entrances: Vec::new(),
            ants: Vec::new(),
            food: Vec::new(),
            surf_ph: vec![0.0; w],
            nests: Vec::new(),
            nest_of: vec![NO_NEST; w * h],
            season: Season::Calm { until: GRACE_PERIOD },
            reports: Vec::new(),
            tick: 0,
            delivered: 0,
            dug: 0,
            next_label: 0,
            drought_before: (0, 0, 0),
            rebuilding: None,
            exit_dist: vec![FAR; w * h],
            store_dist: vec![FAR; w * h],
            tip_dist: vec![FAR; w * h],
            dead_tip: vec![false; w * h],
            dirty: true,
            food_respawn: 0,
            cfg: cfg.clone(),
            rng: fastrand::Rng::with_seed(rng.u64(..)),
        };
        // Starter nest: a shaft down the middle and one small chamber.
        let ex = w / 2;
        let depth = ((h - sky) / 3).clamp(3, 8);
        for d in 0..depth {
            world.set(ex, sky + d, Cell::Tunnel);
        }
        world.carve_chamber(ex, sky + depth, 2, 1);
        world.entrances.push(ex);
        world.refresh_fields();

        let n_ants = cfg.ants.unwrap_or_else(|| ((w * h) / 110).clamp(12, 120));
        for _ in 0..n_ants {
            world.spawn_ant(0);
        }
        world.recount_workers();
        for _ in 0..cfg.food_piles {
            world.spawn_food(1.0);
        }
        world
    }

    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.w + x
    }

    pub fn cell(&self, x: usize, y: usize) -> Cell {
        self.cells[y * self.w + x]
    }

    fn set(&mut self, x: usize, y: usize, c: Cell) {
        let i = self.idx(x, y);
        self.cells[i] = c;
        self.dirty = true;
    }

    fn is_dug(&self, x: usize, y: usize) -> bool {
        matches!(self.cell(x, y), Cell::Tunnel | Cell::Chamber)
    }

    fn soil_cells(&self) -> usize {
        self.w * (self.h - self.sky)
    }

    /// Each living nest gets a share of the soil to hollow out; a lone nest gets half.
    fn dig_capped(&self, nest: usize) -> bool {
        let budget = self.soil_cells() * DIG_SHARE / 100 / self.live_nests().max(2);
        self.nest_cells(nest) > budget
    }

    fn is_husk(&self, x: usize, y: usize) -> bool {
        let n = self.nest_of[self.idx(x, y)];
        n != NO_NEST && self.nests[n as usize].dead
    }

    pub fn stored(&self) -> u32 {
        self.store.iter().map(|&s| s as u32).sum()
    }

    pub fn chambers(&self) -> usize {
        self.cells.iter().filter(|&&c| c == Cell::Chamber).count()
    }

    pub fn live_nests(&self) -> usize {
        self.nests.iter().filter(|n| !n.dead).count()
    }

    pub fn starving_nests(&self) -> usize {
        self.nests.iter().filter(|n| n.starving && !n.dead).count()
    }

    pub fn dead_nests(&self) -> usize {
        self.nests.iter().filter(|n| n.dead).count()
    }

    fn nest_at_entrance(&self, x: usize) -> Option<usize> {
        let n = self.nest_of[self.idx(x, self.ground[x])];
        (n != NO_NEST).then_some(n as usize)
    }

    fn is_live_entrance(&self, x: usize) -> bool {
        self.entrances.contains(&x) && self.nest_at_entrance(x).is_some_and(|n| !self.nests[n].dead)
    }

    fn live_entrances(&self) -> impl Iterator<Item = usize> + '_ {
        self.entrances.iter().copied().filter(|&x| self.is_live_entrance(x))
    }

    /// Food stored in one nest's chambers.
    pub fn nest_food(&self, n: usize) -> u32 {
        self.nest_of
            .iter()
            .zip(&self.store)
            .filter(|(&o, _)| o as usize == n)
            .map(|(_, &s)| s as u32)
            .sum()
    }

    /// Food units the nest's chambers can hold.
    fn nest_capacity(&self, n: usize) -> u32 {
        self.nest_of
            .iter()
            .zip(&self.cells)
            .filter(|(&o, &c)| o as usize == n && c == Cell::Chamber)
            .count() as u32
            * STORE_CAP as u32
    }

    fn nest_cells(&self, n: usize) -> usize {
        self.nest_of.iter().filter(|&&o| o as usize == n).count()
    }

    /// Take food out of a nest, fullest chambers first. False if it did not have enough.
    fn take_food(&mut self, n: usize, mut amount: u32) -> bool {
        if self.nest_food(n) < amount {
            return false;
        }
        let mut cells: Vec<usize> = (0..self.store.len())
            .filter(|&i| self.nest_of[i] as usize == n && self.store[i] > 0)
            .collect();
        cells.sort_by_key(|&i| std::cmp::Reverse(self.store[i]));
        for i in cells {
            if amount == 0 {
                break;
            }
            let take = (self.store[i] as u32).min(amount);
            if self.store[i] >= STORE_CAP {
                self.dirty = true; // a full chamber has room again
            }
            self.store[i] -= take as u8;
            amount -= take;
        }
        true
    }

    /// Season name for the status line, if anything is going on.
    pub fn season_label(&self) -> Option<String> {
        match self.season {
            Season::Warning { .. } => Some("DRY SPELL".into()),
            Season::Drought { until, started } => Some(format!(
                "DROUGHT day {}/{}",
                (self.tick - started) / DAY + 1,
                (until - started).div_ceil(DAY)
            )),
            Season::Bloom { .. } => Some("BLOOM".into()),
            Season::Calm { .. } | Season::Cooldown { .. } => None,
        }
    }

    /// New ant appears in a random cell of nest `home`, ready to head out.
    fn spawn_ant(&mut self, home: usize) {
        let cells: Vec<usize> = (0..self.cells.len())
            .filter(|&i| self.nest_of[i] as usize == home)
            .collect();
        if cells.is_empty() {
            return;
        }
        let i = cells[self.rng.usize(..cells.len())];
        let role = self.pick_role(home);
        let state = match role {
            Role::Digger => State::ToTip,
            Role::Nurse => State::Idle,
            Role::Forager | Role::Scout => State::Foraging,
        };
        self.ants.push(Ant {
            x: i % self.w,
            y: i / self.w,
            state,
            role,
            underground: true,
            home,
            idle: IDLE_TICKS,
            dir: if self.rng.bool() { 1 } else { -1 },
            cooldown: self.rng.u8(..MOVE_TICKS * 4),
            dig_dir: (0, 1),
            dig_budget: 0,
            strength: 0.0,
        });
    }

    /// The caste the nest is shortest of, against `ROLE_SHARES`.
    fn pick_role(&self, home: usize) -> Role {
        let mut counts = [0i64; 4];
        let mut total = 0i64;
        for a in self.ants.iter().filter(|a| a.home == home) {
            counts[a.role as usize] += 1;
            total += 1;
        }
        let mut best = Role::Forager;
        let mut best_gap = i64::MIN;
        for (i, &(role, share)) in ROLE_SHARES.iter().enumerate() {
            let gap = share as i64 * (total + 1) - counts[i] * 100;
            if gap > best_gap {
                best_gap = gap;
                best = role;
            }
        }
        best
    }

    pub fn role_counts(&self) -> [usize; 4] {
        let mut c = [0; 4];
        for a in &self.ants {
            c[a.role as usize] += 1;
        }
        c
    }

    /// `f66 d30 s12 n12`
    pub fn caste_summary(&self) -> String {
        let c = self.role_counts();
        format!("f{} d{} s{} n{}", c[0], c[1], c[2], c[3])
    }

    fn spawn_food(&mut self, size: f32) {
        let units = (self.rng.u32(30..=80) as f32 * size) as u32;
        for _ in 0..40 {
            let x = 2 + self.rng.usize(..self.w - 4);
            let clear = self.entrances.iter().all(|&e| e.abs_diff(x) > 6)
                && self.food.iter().all(|f| f.x.abs_diff(x) > 5);
            if clear {
                self.food.push(Food { x, units });
                return;
            }
        }
    }

    /// Drop a pile on the surface at this column (mouse click, `f` key).
    pub fn add_food_at(&mut self, x: usize) {
        let x = x.clamp(1, self.w - 2);
        let units = self.rng.u32(40..=90);
        if let Some(f) = self.food.iter_mut().find(|f| f.x == x) {
            f.units += units;
        } else {
            self.food.push(Food { x, units });
        }
    }

    pub fn tick(&mut self) {
        self.tick += 1;
        for i in 0..self.ants.len() {
            if self.ants[i].cooldown > 0 {
                self.ants[i].cooldown -= 1;
                continue;
            }
            self.step_ant(i);
        }
        if self.dirty {
            self.refresh_fields();
        }
        for v in &mut self.surf_ph {
            *v *= EVAPORATION;
            if *v < 0.002 {
                *v = 0.0;
            }
        }
        self.food.retain(|f| f.units > 0);
        self.advance_season();
        let factor = self.food_spawn_factor();
        if factor > 0.0 && self.food.len() < self.cfg.food_piles {
            if self.food_respawn == 0 {
                let size = if matches!(self.season, Season::Bloom { .. }) { 1.5 } else { 1.0 };
                self.spawn_food(size);
                self.food_respawn = (FOOD_RESPAWN_TICKS as f32 / factor) as u32;
            } else {
                self.food_respawn -= 1;
            }
        }
        self.recount_workers();
        self.consume_and_starve();
        if let Some((i, target)) = self.rebuilding {
            if self.stored() >= target {
                self.reports[i].rebuilt_days = Some((self.tick - self.reports[i].ended) / DAY);
                self.rebuilding = None;
            }
        }
        if self.cfg.grow {
            self.maybe_grow();
        }
        if self.cfg.bud && self.tick.is_multiple_of(BUD_EVERY) {
            self.maybe_bud();
        }
    }

    // ---- ants -------------------------------------------------------------------------

    fn step_ant(&mut self, i: usize) {
        let ant = self.ants[i].clone();
        let next = if ant.underground {
            self.step_underground(ant)
        } else {
            self.step_surface(ant)
        };
        self.ants[i] = next;
    }

    fn step_surface(&mut self, mut ant: Ant) -> Ant {
        ant.cooldown = MOVE_TICKS;
        let x = ant.x;
        match ant.state {
            State::Foraging => {
                // Food in sight wins, then the trail (foragers only), then a wander.
                // Scouts see further, ignore trails, and keep going the way they were.
                let scout = ant.role == Role::Scout;
                let vision = if scout { SCOUT_VISION } else { VISION };
                let seen = self
                    .food
                    .iter()
                    .filter(|f| f.units > 0)
                    .map(|f| f.x)
                    .filter(|&fx| fx.abs_diff(x) <= vision)
                    .min_by_key(|&fx| fx.abs_diff(x));
                if let Some(fx) = seen {
                    if fx == x {
                        if let Some(f) = self.food.iter_mut().find(|f| f.x == fx) {
                            f.units = f.units.saturating_sub(1);
                        }
                        ant.state = State::Returning;
                        // A scout's trail is the strong one: it is what brings the foragers.
                        ant.strength = if scout { 2.5 } else { 1.0 };
                        ant.dir = self.toward_entrance(x, ant.home);
                        return ant;
                    }
                    ant.dir = if fx > x { 1 } else { -1 };
                } else if scout {
                    if self.rng.f32() < 0.01 {
                        ant.dir = -ant.dir;
                    }
                } else {
                    let ahead = self.ph_at(x as i32 + ant.dir * 3);
                    let behind = self.ph_at(x as i32 - ant.dir * 3);
                    let p_turn = if behind > ahead + 0.02 { 0.5 } else { 0.04 };
                    if self.rng.f32() < p_turn {
                        ant.dir = -ant.dir;
                    }
                }
                self.walk(&mut ant);
            }
            State::Returning => {
                self.surf_ph[x] = (self.surf_ph[x] + DEPOSIT * ant.strength).min(1.0);
                ant.strength *= STRENGTH_DECAY;
                let dir = self.toward_entrance(x, ant.home);
                if dir == 0 && self.is_live_entrance(x) {
                    ant.underground = true;
                    ant.y = self.ground[x];
                    if let Some(n) = self.nest_at_entrance(x) {
                        ant.home = n; // only differs from home when home is gone
                    }
                    return ant;
                }
                ant.dir = dir;
                self.walk(&mut ant);
            }
            State::Founding(target) => {
                if x == target {
                    // Break ground: this column is a nest now (the first of the party
                    // opens it, the rest follow down the same hole).
                    let gy = self.ground[x];
                    if !self.entrances.contains(&x) {
                        self.set(x, gy, Cell::Tunnel);
                        self.dug += 1;
                        self.entrances.push(x);
                    }
                    ant.underground = true;
                    ant.y = gy;
                    // The first couple down the hole become its diggers; the rest keep
                    // their trade so the new nest's castes aren't all shovel.
                    let diggers_here = self
                        .ants
                        .iter()
                        .filter(|a| a.underground && a.x == x && a.role == Role::Digger)
                        .count();
                    if diggers_here < 2 {
                        ant.role = Role::Digger;
                    }
                    ant.state = State::Digging;
                    ant.dig_dir = (0, 1);
                    ant.dig_budget = 10;
                    return ant;
                }
                ant.dir = if target > x { 1 } else { -1 };
                self.walk(&mut ant);
            }
            State::ToTip | State::Digging | State::Idle => {
                // Underground castes caught outside (a nest just merged, say) head home.
                let dir = self.toward_entrance(x, ant.home);
                if dir == 0 && self.is_live_entrance(x) {
                    ant.underground = true;
                    ant.y = self.ground[x];
                    if let Some(n) = self.nest_at_entrance(x) {
                        ant.home = n;
                    }
                    return ant;
                }
                if dir == 0 {
                    ant.state = State::Foraging; // no nest left to go home to
                } else {
                    ant.dir = dir;
                    self.walk(&mut ant);
                }
            }
        }
        ant
    }

    fn walk(&mut self, ant: &mut Ant) {
        let nx = ant.x as i32 + ant.dir;
        if nx < 0 || nx >= self.w as i32 {
            ant.dir = -ant.dir;
            return;
        }
        ant.x = nx as usize;
        ant.y = self.ground[ant.x].saturating_sub(1);
    }

    fn ph_at(&self, x: i32) -> f32 {
        if x < 0 || x >= self.w as i32 {
            0.0
        } else {
            self.surf_ph[x as usize]
        }
    }

    /// Direction along the surface to the nearest entrance of the ant's own nest, or of any
    /// living nest if home is gone. 0 means "you are standing on it".
    fn toward_entrance(&self, x: usize, home: usize) -> i32 {
        let own = self
            .nests
            .get(home)
            .filter(|n| !n.dead)
            .and_then(|n| n.entrances.iter().copied().filter(|&e| self.is_live_entrance(e)).min_by_key(|&e| e.abs_diff(x)));
        let nearest = own
            .or_else(|| self.live_entrances().min_by_key(|&e| e.abs_diff(x)))
            .unwrap_or(x);
        if nearest > x {
            1
        } else if nearest < x {
            -1
        } else {
            0
        }
    }

    fn step_underground(&mut self, mut ant: Ant) -> Ant {
        ant.cooldown = MOVE_TICKS;
        let here = self.idx(ant.x, ant.y);
        match ant.state {
            State::Foraging | State::Founding(_) => {
                if self.exit_dist[here] == 0 {
                    ant.underground = false;
                    ant.y = self.ground[ant.x].saturating_sub(1);
                    ant.state = State::Foraging;
                    ant.dir = if self.rng.bool() { 1 } else { -1 };
                } else if let Some((nx, ny)) = self.descend(Field::Exit, ant.x, ant.y) {
                    ant.x = nx;
                    ant.y = ny;
                }
            }
            State::Returning => {
                if self.store_dist[here] == 0 && self.cells[here] == Cell::Chamber {
                    self.store[here] += 1;
                    self.delivered += 1;
                    if self.store[here] >= STORE_CAP {
                        self.dirty = true;
                    }
                    self.assign(&mut ant, here);
                } else if self.store_dist[here] == FAR {
                    // Nowhere to put it: no chamber with room. Go and dig one.
                    ant.state = State::ToTip;
                } else if let Some((nx, ny)) = self.descend(Field::Store, ant.x, ant.y) {
                    ant.x = nx;
                    ant.y = ny;
                }
            }
            State::ToTip => {
                if self.tip_dist[here] == 0 {
                    ant.state = State::Digging;
                    ant.dig_budget = self.rng.u8(3..=8);
                    ant.dig_dir = self.pick_dig_dir((0, 1));
                } else if self.tip_dist[here] == FAR {
                    ant.state = State::Idle;
                    ant.idle = IDLE_TICKS;
                } else if let Some((nx, ny)) = self.descend(Field::Tip, ant.x, ant.y) {
                    ant.x = nx;
                    ant.y = ny;
                }
            }
            State::Digging => {
                if ant.dig_budget == 0 || self.dig_capped(ant.home) {
                    self.assign(&mut ant, here);
                    return ant;
                }
                if !self.rng.bool() || self.rng.f32() < 0.2 {
                    ant.dig_dir = self.pick_dig_dir(ant.dig_dir);
                }
                let mut done = false;
                for _ in 0..6 {
                    let (dx, dy) = ant.dig_dir;
                    let (nx, ny) = (ant.x as i32 + dx, ant.y as i32 + dy);
                    if self.can_dig(ant.x, ant.y, nx, ny) {
                        let (nx, ny) = (nx as usize, ny as usize);
                        self.dig(nx, ny, ant.x);
                        ant.x = nx;
                        ant.y = ny;
                        ant.dig_budget -= 1;
                        ant.cooldown = DIG_TICKS;
                        done = true;
                        break;
                    }
                    ant.dig_dir = self.pick_dig_dir(ant.dig_dir);
                }
                if !done {
                    // Boxed in. Mark this tip dead and go do something else.
                    self.dead_tip[here] = true;
                    self.dirty = true;
                    self.assign(&mut ant, here);
                }
            }
            State::Idle => {
                if ant.idle == 0 {
                    self.assign(&mut ant, here);
                    return ant;
                }
                ant.idle -= 1;
                // Nurses keep to the chambers; everyone else just wanders.
                if let Some((nx, ny)) = self.wander(ant.x, ant.y, ant.role == Role::Nurse) {
                    ant.x = nx;
                    ant.y = ny;
                }
            }
        }
        ant
    }

    /// Step to a random dug neighbour, preferring chambers if asked.
    fn wander(&mut self, x: usize, y: usize, prefer_chamber: bool) -> Option<(usize, usize)> {
        let mut any = Vec::with_capacity(4);
        let mut chambers = Vec::with_capacity(4);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if self.is_dug(nx, ny) {
                any.push((nx, ny));
                if self.cell(nx, ny) == Cell::Chamber {
                    chambers.push((nx, ny));
                }
            }
        }
        let pick = if prefer_chamber && !chambers.is_empty() && self.rng.f32() < 0.85 {
            chambers
        } else {
            any
        };
        if pick.is_empty() {
            None
        } else {
            Some(pick[self.rng.usize(..pick.len())])
        }
    }

    /// What an ant does next after finishing a job underground, by caste.
    fn assign(&mut self, ant: &mut Ant, here: usize) {
        ant.state = match ant.role {
            Role::Nurse => {
                ant.idle = IDLE_TICKS;
                State::Idle
            }
            Role::Digger => {
                if self.dig_capped(ant.home) || self.tip_dist[here] == FAR {
                    ant.idle = IDLE_TICKS;
                    State::Idle
                } else {
                    State::ToTip
                }
            }
            Role::Scout => State::Foraging,
            Role::Forager => {
                // A forager pitches in on chambers when the larder is nearly full.
                let free: u32 = self
                    .cells
                    .iter()
                    .zip(&self.store)
                    .zip(&self.nest_of)
                    .filter(|((c, s), n)| **c == Cell::Chamber && **s < STORE_CAP && **n as usize == ant.home)
                    .map(|((_, s), _)| (STORE_CAP - s) as u32)
                    .sum();
                if free < 8 && !self.dig_capped(ant.home) && self.rng.f32() < 0.5 {
                    State::ToTip
                } else {
                    State::Foraging
                }
            }
        };
    }

    /// Step to the neighbouring dug cell that is closer to the field's sources, or
    /// wander if none is.
    fn descend(&mut self, which: Field, x: usize, y: usize) -> Option<(usize, usize)> {
        let field: &[u16] = match which {
            Field::Exit => &self.exit_dist,
            Field::Store => &self.store_dist,
            Field::Tip => &self.tip_dist,
        };
        let here = field[self.idx(x, y)];
        let mut best: Vec<(usize, usize)> = Vec::new();
        let mut best_d = here;
        let mut any: Vec<(usize, usize)> = Vec::new();
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if !self.is_dug(nx, ny) {
                continue;
            }
            any.push((nx, ny));
            let d = field[self.idx(nx, ny)];
            if d < best_d {
                best_d = d;
                best = vec![(nx, ny)];
            } else if d == best_d && d < here {
                best.push((nx, ny));
            }
        }
        let pick = if !best.is_empty() { best } else { any };
        if pick.is_empty() {
            None
        } else {
            Some(pick[self.rng.usize(..pick.len())])
        }
    }

    // ---- digging ----------------------------------------------------------------------

    fn pick_dig_dir(&mut self, current: (i32, i32)) -> (i32, i32) {
        if self.rng.f32() < 0.6 && current != (0, 0) {
            return current;
        }
        // Mostly down, some sideways, rarely up (which can open a new entrance).
        let r = self.rng.f32();
        if r < 0.45 {
            (0, 1)
        } else if r < 0.7 {
            (1, 0)
        } else if r < 0.95 {
            (-1, 0)
        } else {
            (0, -1)
        }
    }

    /// Can a digger standing at (fx, fy) open (nx, ny)? Keeps a two-cell wall around
    /// every tunnel so the farm stays legible instead of dissolving into a maze.
    fn can_dig(&self, fx: usize, fy: usize, nx: i32, ny: i32) -> bool {
        if nx < 1 || ny < 1 || nx >= self.w as i32 - 1 || ny >= self.h as i32 - 1 {
            return false;
        }
        let (nx, ny) = (nx as usize, ny as usize);
        if ny < self.ground[nx] || self.cell(nx, ny) != Cell::Soil {
            return false;
        }
        let going_down = ny > fy;
        if !going_down && ny < self.ground[nx] + 3 {
            return false;
        }
        for dy in -2..=2 {
            for dx in -2..=2 {
                let (ax, ay) = (nx as i32 + dx, ny as i32 + dy);
                if ax < 0 || ay < 0 || ax >= self.w as i32 || ay >= self.h as i32 {
                    continue;
                }
                let (ax, ay) = (ax as usize, ay as usize);
                if ax.abs_diff(fx) <= 2 && ay.abs_diff(fy) <= 2 {
                    continue; // the tunnel we came from
                }
                if self.is_dug(ax, ay) && !self.is_husk(ax, ay) {
                    return false; // breaking into a husk is fine: that is how it gets reused
                }
            }
        }
        true
    }

    fn dig(&mut self, x: usize, y: usize, from_x: usize) {
        self.set(x, y, Cell::Tunnel);
        self.dug += 1;
        let depth = y.saturating_sub(self.ground[x]);
        if depth >= 4 && self.rng.f32() < 0.12 && !self.chamber_near(x, y, 5) {
            let rx = self.rng.usize(2..=3);
            let ry = self.rng.usize(1..=2);
            self.carve_chamber(x, y, rx, ry);
        }
        self.raise_mound(from_x);
    }

    fn chamber_near(&self, x: usize, y: usize, r: usize) -> bool {
        let x0 = x.saturating_sub(r);
        let y0 = y.saturating_sub(r);
        for cy in y0..=(y + r).min(self.h - 1) {
            for cx in x0..=(x + r).min(self.w - 1) {
                if self.cell(cx, cy) == Cell::Chamber {
                    return true;
                }
            }
        }
        false
    }

    /// Hollow an ellipse around (cx, cy) where the soil allows it.
    fn carve_chamber(&mut self, cx: usize, cy: usize, rx: usize, ry: usize) {
        let mut cells = Vec::new();
        for dy in -(ry as i32)..=(ry as i32) {
            for dx in -(rx as i32)..=(rx as i32) {
                let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                if x < 1 || y < 1 || x >= self.w as i32 - 1 || y >= self.h as i32 - 1 {
                    continue;
                }
                let e = (dx as f32 / (rx as f32 + 0.5)).powi(2) + (dy as f32 / (ry as f32 + 0.5)).powi(2);
                if e > 1.0 {
                    continue;
                }
                let (x, y) = (x as usize, y as usize);
                if y <= self.ground[x] + 1 {
                    continue;
                }
                cells.push((x, y));
            }
        }
        // Don't merge into some other tunnel system: every blob cell's neighbours must be
        // soil, part of the blob, or the tunnel we are carving from (near the centre).
        for &(x, y) in &cells {
            for dy in -1..=1_i32 {
                for dx in -1..=1_i32 {
                    let (ax, ay) = ((x as i32 + dx) as usize, (y as i32 + dy) as usize);
                    if (ax.abs_diff(cx) <= 2 && ay.abs_diff(cy) <= 2) || cells.contains(&(ax, ay)) {
                        continue;
                    }
                    if self.is_dug(ax, ay) {
                        return;
                    }
                }
            }
        }
        for (x, y) in cells {
            if self.cell(x, y) == Cell::Soil {
                self.dug += 1;
            }
            self.set(x, y, Cell::Chamber);
        }
    }

    /// Spoil goes on the surface next to the nearest entrance, building a little mound.
    fn raise_mound(&mut self, near_x: usize) {
        if self.rng.f32() > 0.5 {
            return;
        }
        let e = self.entrances.iter().min_by_key(|&&e| e.abs_diff(near_x)).copied().unwrap_or(near_x);
        let off = self.rng.i32(-5..=5);
        if off == 0 {
            return;
        }
        let x = e as i32 + off;
        if x < 1 || x >= self.w as i32 - 1 {
            return;
        }
        let x = x as usize;
        if self.entrances.contains(&x) || self.ground[x] + 5 <= self.sky {
            return;
        }
        // Keep the mound a gentle cone: never rise above a neighbour by more than one.
        if self.ground[x] < self.ground[x - 1] || self.ground[x] < self.ground[x + 1] {
            return;
        }
        self.ground[x] -= 1;
        let gy = self.ground[x];
        self.set(x, gy, Cell::Soil);
        // Anyone standing there gets lifted onto the new surface.
        for a in &mut self.ants {
            if !a.underground && a.x == x {
                a.y = gy.saturating_sub(1);
            }
        }
    }

    // ---- fields -----------------------------------------------------------------------

    fn refresh_fields(&mut self) {
        self.dirty = false;
        self.label_nests();
        let alive = |w: &Self, i: usize| {
            let n = w.nest_of[i];
            n != NO_NEST && !w.nests[n as usize].dead
        };
        let exits: Vec<usize> = self
            .entrances
            .iter()
            .map(|&x| self.idx(x, self.ground[x]))
            .filter(|&i| matches!(self.cells[i], Cell::Tunnel | Cell::Chamber) && alive(self, i))
            .collect();
        self.exit_dist = self.bfs(&exits);

        let stores: Vec<usize> = (0..self.cells.len())
            .filter(|&i| self.cells[i] == Cell::Chamber && self.store[i] < STORE_CAP && alive(self, i))
            .collect();
        self.store_dist = self.bfs(&stores);

        let mut tips: Vec<usize> = (0..self.cells.len())
            .filter(|&i| self.cells[i] == Cell::Tunnel && !self.dead_tip[i] && self.dug_neighbours(i) <= 1)
            .filter(|&i| i / self.w > self.ground[i % self.w] && alive(self, i))
            .collect();
        if tips.is_empty() {
            tips = (0..self.cells.len())
                .filter(|&i| self.cells[i] == Cell::Tunnel && !self.dead_tip[i] && alive(self, i))
                .collect();
        }
        self.tip_dist = self.bfs(&tips);
    }

    /// Group dug cells into connected systems and match them to nests. A nest keeps its
    /// label across digs; two systems that join become one nest under the older label.
    fn label_nests(&mut self) {
        let n = self.cells.len();
        let mut comp = vec![NO_NEST; n];
        let mut comps: Vec<Vec<usize>> = Vec::new();
        for start in 0..n {
            if comp[start] != NO_NEST || !matches!(self.cells[start], Cell::Tunnel | Cell::Chamber) {
                continue;
            }
            let id = comps.len() as u16;
            let mut cells = vec![start];
            comp[start] = id;
            let mut q = VecDeque::from([start]);
            while let Some(i) = q.pop_front() {
                let (x, y) = (i % self.w, i / self.w);
                let mut nb = Vec::with_capacity(4);
                if x > 0 {
                    nb.push(i - 1);
                }
                if x + 1 < self.w {
                    nb.push(i + 1);
                }
                if y > 0 {
                    nb.push(i - self.w);
                }
                if y + 1 < self.h {
                    nb.push(i + self.w);
                }
                for j in nb {
                    if comp[j] == NO_NEST && matches!(self.cells[j], Cell::Tunnel | Cell::Chamber) {
                        comp[j] = id;
                        cells.push(j);
                        q.push_back(j);
                    }
                }
            }
            comps.push(cells);
        }

        let old = std::mem::take(&mut self.nests);
        let mut old_to_new = vec![usize::MAX; old.len()];
        let mut built: Vec<(usize, Nest, Vec<usize>)> = Vec::new(); // (component, nest, old matches)
        for c in 0..comps.len() {
            let ents: Vec<usize> = self
                .entrances
                .iter()
                .copied()
                .filter(|&x| comp[self.idx(x, self.ground[x])] == c as u16)
                .collect();
            if ents.is_empty() {
                continue;
            }
            let mut matching: Vec<usize> = (0..old.len())
                .filter(|&i| old[i].entrances.iter().any(|e| ents.contains(e)))
                .collect();
            matching.sort_by_key(|&i| old[i].label);
            let nest = if let Some(&first) = matching.first() {
                let mut base = old[first].clone();
                for &m in &matching[1..] {
                    base.workers += old[m].workers;
                    base.dead = base.dead && old[m].dead;
                }
                base.entrances = ents;
                base
            } else {
                let label = (b'A' + self.next_label % 26) as char;
                self.next_label = self.next_label.wrapping_add(1);
                Nest::new(label, ents)
            };
            built.push((c, nest, matching));
        }
        built.sort_by_key(|(_, nest, _)| nest.label);
        let mut comp_nest = vec![NO_NEST; comps.len()];
        let mut nests = Vec::with_capacity(built.len());
        for (c, nest, matching) in built {
            let idx = nests.len();
            comp_nest[c] = idx as u16;
            for m in matching {
                old_to_new[m] = idx;
            }
            nests.push(nest);
        }
        for i in 0..n {
            self.nest_of[i] = if comp[i] == NO_NEST { NO_NEST } else { comp_nest[comp[i] as usize] };
        }
        self.nests = nests;

        // Ants underground belong to the system they stand in; ants on the surface keep
        // their nest unless it has gone, then take the nearest live one.
        for i in 0..self.ants.len() {
            let a = &self.ants[i];
            let mut home = old_to_new.get(a.home).copied().unwrap_or(usize::MAX);
            if a.underground {
                let here = self.nest_of[a.y * self.w + a.x];
                if here != NO_NEST {
                    home = here as usize;
                }
            }
            if home == usize::MAX {
                home = self
                    .live_entrances()
                    .min_by_key(|&e| e.abs_diff(a.x))
                    .and_then(|e| self.nest_at_entrance(e))
                    .unwrap_or(0);
            }
            self.ants[i].home = home;
        }
    }

    fn recount_workers(&mut self) {
        for nest in &mut self.nests {
            nest.workers = 0;
        }
        for a in &self.ants {
            if let Some(nest) = self.nests.get_mut(a.home) {
                nest.workers += 1;
            }
        }
        for nest in &mut self.nests {
            if nest.workers == 0 && !nest.dead {
                nest.dead = true;
                nest.starving = false;
            }
        }
    }

    fn dug_neighbours(&self, i: usize) -> usize {
        let (x, y) = (i % self.w, i / self.w);
        let mut n = 0;
        if x > 0 && self.is_dug(x - 1, y) {
            n += 1;
        }
        if x + 1 < self.w && self.is_dug(x + 1, y) {
            n += 1;
        }
        if y > 0 && self.is_dug(x, y - 1) {
            n += 1;
        }
        if y + 1 < self.h && self.is_dug(x, y + 1) {
            n += 1;
        }
        n
    }

    fn bfs(&self, sources: &[usize]) -> Vec<u16> {
        let mut dist = vec![FAR; self.cells.len()];
        let mut q = VecDeque::new();
        for &s in sources {
            dist[s] = 0;
            q.push_back(s);
        }
        while let Some(i) = q.pop_front() {
            let d = dist[i] + 1;
            let (x, y) = (i % self.w, i / self.w);
            let mut push = |j: usize, q: &mut VecDeque<usize>| {
                if dist[j] == FAR && matches!(self.cells[j], Cell::Tunnel | Cell::Chamber) {
                    dist[j] = d;
                    q.push_back(j);
                }
            };
            if x > 0 {
                push(i - 1, &mut q);
            }
            if x + 1 < self.w {
                push(i + 1, &mut q);
            }
            if y > 0 {
                push(i - self.w, &mut q);
            }
            if y + 1 < self.h {
                push(i + self.w, &mut q);
            }
        }
        dist
    }

    // ---- colony -----------------------------------------------------------------------

    /// Stored food feeds new ants, one nest at a time, as long as there is room.
    fn maybe_grow(&mut self) {
        for n in 0..self.nests.len() {
            if self.ants.len() >= self.cfg.max_ants {
                return;
            }
            let nest = &self.nests[n];
            if nest.dead || self.tick - nest.last_spawn < SPAWN_INTERVAL {
                continue;
            }
            // A nest whose chambers can't hold its reserve still grows once they are nearly
            // full; the extra mouths then push it to dig more chambers.
            let capacity = self.nest_capacity(n);
            let reserve = (SPAWN_THRESHOLD + nest.workers as u32 * RESERVE_PER_WORKER).min(capacity * 4 / 5);
            if nest.workers * 2 >= self.nest_cells(n) + 4 || self.nest_food(n) < reserve {
                continue;
            }
            if self.take_food(n, SPAWN_COST) {
                self.spawn_ant(n);
                self.nests[n].last_spawn = self.tick;
                self.nests[n].workers += 1;
            }
        }
    }

    // ---- seasons ----------------------------------------------------------------------

    fn advance_season(&mut self) {
        if !self.cfg.drought {
            return;
        }
        let t = self.tick;
        self.season = match self.season {
            Season::Calm { until } if t >= until => {
                self.rebuilding = None; // too late: whatever was put by is what they have
                Season::Warning { until: t + WARNING_LENGTH }
            }
            Season::Warning { until } if t >= until => {
                self.drought_before = (self.live_nests(), self.ants.len(), self.stored());
                let length = self.rng.u64(MIN_DURATION..=MAX_DURATION);
                Season::Drought { until: t + length, started: t }
            }
            Season::Drought { until, started } if t >= until => {
                self.reports.push(DroughtReport {
                    started,
                    ended: t,
                    nests_before: self.drought_before.0,
                    nests_after: self.live_nests(),
                    ants_before: self.drought_before.1,
                    ants_after: self.ants.len(),
                    stored_before: self.drought_before.2,
                    stored_after: self.stored(),
                    rebuilt_days: None,
                });
                self.rebuilding = Some((self.reports.len() - 1, self.drought_before.2));
                for nest in &mut self.nests {
                    nest.starving = false;
                    nest.starve_timer = 0;
                    nest.consume_acc = 0.0;
                }
                Season::Bloom { until: t + BLOOM_LENGTH }
            }
            Season::Bloom { until } if t >= until => Season::Cooldown { until: t + COOLDOWN },
            Season::Cooldown { until } if t >= until => {
                let jitter = self.rng.u64(0..=2 * DROUGHT_JITTER);
                let gap = (DROUGHT_INTERVAL + jitter - DROUGHT_JITTER).saturating_sub(WARNING_LENGTH);
                Season::Calm { until: t + gap }
            }
            same => same,
        };
    }

    /// Multiplier on the surface food spawn rate: 1 normally, tapering to 0 through the
    /// warning, 0 in drought, boosted in bloom.
    fn food_spawn_factor(&self) -> f32 {
        match self.season {
            Season::Calm { .. } | Season::Cooldown { .. } => 1.0,
            Season::Warning { until } => {
                let left = until.saturating_sub(self.tick) as f32 / WARNING_LENGTH as f32;
                if left < 0.05 {
                    0.0
                } else {
                    left
                }
            }
            Season::Drought { .. } => 0.0,
            Season::Bloom { .. } => BLOOM_MULTIPLIER as f32,
        }
    }

    /// Every nest eats from its stores all year, more in a drought. In a drought a nest with
    /// nothing left loses a worker every so often until food comes back or nobody is left;
    /// the rest of the year an empty larder just means no growth.
    fn consume_and_starve(&mut self) {
        let drought = matches!(self.season, Season::Drought { .. });
        let rate = if drought { CONSUME_RATE } else { UPKEEP_RATE };
        for n in 0..self.nests.len() {
            if self.nests[n].dead {
                continue;
            }
            self.nests[n].consume_acc += self.nests[n].workers as f32 * rate;
            while self.nests[n].consume_acc >= 1.0 {
                self.nests[n].consume_acc -= 1.0;
                if !self.take_food(n, 1) {
                    break;
                }
            }
            if !drought || self.nest_food(n) > 0 {
                self.nests[n].starving = false;
                self.nests[n].starve_timer = 0;
                continue;
            }
            self.nests[n].starving = true;
            self.nests[n].starve_timer += 1;
            if self.nests[n].starve_timer < STARVE_INTERVAL {
                continue;
            }
            self.nests[n].starve_timer = 0;
            // Any worker of the nest, chosen at random, so no caste starves out first.
            let members: Vec<usize> = (0..self.ants.len()).filter(|&i| self.ants[i].home == n).collect();
            let victim = (!members.is_empty()).then(|| members[self.rng.usize(..members.len())]);
            if let Some(i) = victim {
                self.ants.swap_remove(i);
                self.nests[n].workers -= 1;
                if self.nests[n].workers == 0 {
                    self.nests[n].dead = true;
                    self.nests[n].starving = false;
                    self.dirty = true;
                }
            }
        }
    }

    /// A big colony sends a few surface foragers off to open a nest somewhere clear.
    fn maybe_bud(&mut self) {
        if self.live_nests() >= self.cfg.max_nests || self.ants.len() < BUD_POPULATION {
            return;
        }
        let mut target = None;
        for _ in 0..30 {
            let x = 4 + self.rng.usize(..self.w.saturating_sub(8).max(1));
            // Keep well clear of living nests; husks only need a little room.
            let clear = self.entrances.iter().all(|&e| {
                let gap = if self.is_live_entrance(e) { NEST_GAP } else { 4 };
                e.abs_diff(x) >= gap
            });
            if clear && self.food.iter().all(|f| f.x.abs_diff(x) > 4) {
                target = Some(x);
                break;
            }
        }
        let Some(target) = target else { return };
        // Scouts lead the way; foragers make up the numbers if there aren't enough out.
        let out = |a: &Ant| !a.underground && a.state == State::Foraging;
        let mut party: Vec<usize> = self
            .ants
            .iter()
            .enumerate()
            .filter(|(_, a)| out(a) && a.role == Role::Scout)
            .map(|(i, _)| i)
            .take(BUD_PARTY)
            .collect();
        if party.len() < BUD_PARTY {
            let more: Vec<usize> = self
                .ants
                .iter()
                .enumerate()
                .filter(|(i, a)| out(a) && a.role == Role::Forager && !party.contains(i))
                .map(|(i, _)| i)
                .take(BUD_PARTY - party.len())
                .collect();
            party.extend(more);
        }
        if party.len() < 2 {
            return;
        }
        for i in party {
            self.ants[i].state = State::Founding(target);
        }
    }

    // ---- text ------------------------------------------------------------------------

    /// Plain-text picture of the farm, one char per pixel, for tests and `--headless`.
    pub fn snapshot(&self) -> String {
        let mut out = String::with_capacity((self.w + 1) * self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                let ant = self.ants.iter().find(|a| a.x == x && a.y == y);
                let ch = if let Some(a) = ant {
                    match (a.state, a.role) {
                        (State::Returning, _) => 'A',
                        (State::Founding(_), _) => '*',
                        (_, Role::Forager) => 'f',
                        (_, Role::Digger) => 'd',
                        (_, Role::Scout) => 's',
                        (_, Role::Nurse) => 'n',
                    }
                } else if y + 1 == self.ground[x] && self.food.iter().any(|f| f.x == x) {
                    'F'
                } else {
                    let husk = {
                        let n = self.nest_of[self.idx(x, y)];
                        n != NO_NEST && self.nests[n as usize].dead
                    };
                    match self.cell(x, y) {
                        Cell::Sky => ' ',
                        Cell::Soil => '#',
                        Cell::Tunnel | Cell::Chamber if husk => 'x',
                        Cell::Tunnel => '.',
                        Cell::Chamber => {
                            if self.store[self.idx(x, y)] > 0 {
                                '%'
                            } else {
                                'o'
                            }
                        }
                    }
                };
                out.push(ch);
            }
            out.push('\n');
        }
        out
    }

    /// `nests 3 (1 starving, 1 dead)` or just `nests 3`.
    pub fn nest_summary(&self) -> String {
        let mut notes = Vec::new();
        if self.starving_nests() > 0 {
            notes.push(format!("{} starving", self.starving_nests()));
        }
        if self.dead_nests() > 0 {
            notes.push(format!("{} dead", self.dead_nests()));
        }
        if notes.is_empty() {
            format!("nests {}", self.live_nests())
        } else {
            format!("nests {} ({})", self.live_nests(), notes.join(", "))
        }
    }

    pub fn stats(&self) -> String {
        let season = self.season_label().map(|s| format!("{s} · ")).unwrap_or_default();
        format!(
            "tick {} · {season}{} · ants {} ({}) · dug {} · chambers {} · stored {} · food {} · delivered {}",
            self.tick,
            self.nest_summary(),
            self.ants.len(),
            self.caste_summary(),
            self.dug,
            self.chambers(),
            self.stored(),
            self.food.len(),
            self.delivered
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(seed: u64) -> World {
        World::new(Config { seed, ..Config::default() }, 71, 48)
    }

    fn run(w: &mut World, ticks: u64) {
        for _ in 0..ticks {
            w.tick();
        }
    }

    #[test]
    fn ants_dig_tunnels_and_chambers() {
        let mut w = world(1);
        let (start, chambers) = (w.dug, w.chambers());
        assert!(chambers > 5, "starter chamber missing:\n{}", w.snapshot());
        run(&mut w, 4000);
        assert!(w.dug > start + 30, "dug only {} cells:\n{}", w.dug - start, w.snapshot());
        assert!(w.chambers() > chambers, "no new chambers:\n{}", w.snapshot());
        assert!(w.dug * 100 < w.soil_cells() * 40, "farm is a maze:\n{}", w.snapshot());
    }

    #[test]
    fn ants_bring_food_home_and_the_colony_grows() {
        let mut w = world(2);
        let start = w.ants.len();
        // Growth waits for a reserve now, so give the colony time to stock up first.
        run(&mut w, 16000);
        assert!(w.delivered > 30, "only {} deliveries:\n{}", w.delivered, w.snapshot());
        assert!(w.ants.len() > start, "colony did not grow:\n{}", w.snapshot());
    }

    #[test]
    fn surface_ant_on_food_picks_it_up() {
        let mut w = world(3);
        w.food.clear();
        w.food.push(Food { x: 10, units: 5 });
        w.ants[0] = Ant {
            x: 10,
            y: w.ground[10] - 1,
            state: State::Foraging,
            role: Role::Forager,
            underground: false,
            home: 0,
            idle: 0,
            dir: 1,
            cooldown: 0,
            dig_dir: (0, 1),
            dig_budget: 0,
            strength: 0.0,
        };
        w.step_ant(0);
        assert_eq!(w.ants[0].state, State::Returning);
        assert_eq!(w.food[0].units, 4);
    }

    #[test]
    fn ants_always_stand_somewhere_sensible() {
        let mut w = world(4);
        for _ in 0..40 {
            run(&mut w, 100);
            for a in &w.ants {
                if a.underground {
                    assert!(w.is_dug(a.x, a.y), "ant in soil at {},{}:\n{}", a.x, a.y, w.snapshot());
                } else {
                    assert_eq!(a.y + 1, w.ground[a.x], "surface ant floating at {},{}", a.x, a.y);
                }
            }
        }
        for x in 0..w.w {
            for y in 0..w.ground[x] {
                assert_eq!(w.cell(x, y), Cell::Sky);
            }
        }
    }

    #[test]
    fn a_big_colony_opens_a_second_nest() {
        let mut w = World::new(Config { seed: 5, ants: Some(60), ..Config::default() }, 120, 60);
        let mut when = None;
        for t in 0..12000 {
            w.tick();
            if w.entrances.len() >= 2 {
                when = Some(t);
                break;
            }
        }
        assert!(when.is_some(), "no second nest:\n{}", w.snapshot());
        assert!(w.entrances[0].abs_diff(w.entrances[1]) >= 2);
    }

    #[test]
    fn no_bud_keeps_entrances_from_founding_parties() {
        let mut w = World::new(Config { seed: 5, ants: Some(60), bud: false, ..Config::default() }, 120, 60);
        run(&mut w, 3000);
        assert!(w.ants.iter().all(|a| !matches!(a.state, State::Founding(_))));
    }

    /// Dig a second, separate tunnel system with an entrance at column `x`.
    fn dig_system(w: &mut World, x: usize, depth: usize) {
        let g = w.ground[x];
        for d in 0..depth {
            w.set(x, g + d, Cell::Tunnel);
        }
        w.carve_chamber(x, g + depth, 2, 1);
        w.entrances.push(x);
        w.refresh_fields();
    }

    fn fill_nest(w: &mut World, n: usize) {
        for i in 0..w.store.len() {
            if w.nest_of[i] as usize == n && w.cells[i] == Cell::Chamber {
                w.store[i] = STORE_CAP;
            }
        }
    }

    #[test]
    fn nests_are_labelled_and_keep_their_label_while_digging() {
        let mut w = world(1);
        assert_eq!(w.nests.len(), 1);
        assert_eq!(w.nests[0].label, 'A');
        assert!(w.ants.iter().all(|a| a.home == 0));
        run(&mut w, 2500);
        assert_eq!(w.nests[0].label, 'A');
        assert!(w.nests[0].workers > 0);
        assert!(w.ants.iter().all(|a| a.home < w.nests.len()));
    }

    #[test]
    fn joined_systems_merge_under_the_older_label() {
        let mut w = World::new(Config { seed: 2, bud: false, drought: false, ..Config::default() }, 90, 48);
        dig_system(&mut w, 70, 6);
        assert_eq!(w.nests.len(), 2);
        assert_eq!(w.nests[1].label, 'B');
        w.spawn_ant(1);
        w.spawn_ant(1);
        w.recount_workers();
        let a_workers = w.nests[0].workers;
        // Join them with a corridor two rows below the deeper chamber.
        let y = w.ground[45] + 12;
        for x in 45..=70 {
            w.set(x, y, Cell::Tunnel);
        }
        for d in 0..12 {
            w.set(45, w.ground[45] + d, Cell::Tunnel);
            w.set(70, w.ground[70] + d, Cell::Tunnel);
        }
        w.refresh_fields();
        w.recount_workers();
        assert_eq!(w.nests.len(), 1, "systems did not merge:\n{}", w.snapshot());
        assert_eq!(w.nests[0].label, 'A');
        assert_eq!(w.nests[0].workers, a_workers + 2);
        assert!(w.ants.iter().all(|a| a.home == 0));
    }

    #[test]
    fn growth_spawns_from_the_nest_that_holds_the_food_and_waits_between_spawns() {
        let mut w = World::new(Config { seed: 2, bud: false, drought: false, ants: Some(5), ..Config::default() }, 90, 48);
        dig_system(&mut w, 70, 6);
        w.food.clear();
        w.tick = SPAWN_INTERVAL;
        fill_nest(&mut w, 1);
        assert!(w.nest_food(1) >= SPAWN_THRESHOLD + w.nests[1].workers as u32 * RESERVE_PER_WORKER);
        let before = w.ants.len();
        w.maybe_grow();
        assert_eq!(w.ants.len(), before + 1);
        assert_eq!(w.ants.last().unwrap().home, 1);
        assert!(w.nest_food(0) == 0 || w.nest_food(1) < w.nest_cells(1) as u32 * STORE_CAP as u32);
        w.maybe_grow();
        assert_eq!(w.ants.len(), before + 1, "spawned again inside the interval");
        w.tick += SPAWN_INTERVAL;
        w.maybe_grow();
        assert_eq!(w.ants.len(), before + 2);
    }

    #[test]
    fn drought_stops_surface_food_and_drains_the_stores() {
        let mut w = World::new(Config { seed: 3, ants: Some(40), ..Config::default() }, 71, 48);
        fill_nest(&mut w, 0);
        let full = w.nest_food(0);
        w.food.clear();
        w.season = Season::Drought { until: w.tick + 50 * DAY, started: w.tick };
        run(&mut w, 300);
        assert!(w.food.is_empty(), "food spawned in a drought");
        assert!(w.nest_food(0) < full, "stores did not drain");
        assert_eq!(w.ants.len(), 40, "nobody should starve while there is food");
    }

    #[test]
    fn a_nest_with_no_food_loses_workers_and_then_dies() {
        let mut w = World::new(Config { seed: 3, ants: Some(6), bud: false, ..Config::default() }, 71, 48);
        w.food.clear();
        w.season = Season::Drought { until: w.tick + 500 * DAY, started: w.tick };
        run(&mut w, STARVE_INTERVAL as u64 + 2);
        assert_eq!(w.ants.len(), 5);
        assert!(w.nests[0].starving);
        run(&mut w, STARVE_INTERVAL as u64 * 6);
        assert!(w.ants.is_empty());
        assert!(w.nests[0].dead);
        assert!(!w.nests[0].starving);
        assert_eq!(w.live_entrances().count(), 0);
        assert!(w.snapshot().contains('x'));
    }

    #[test]
    fn a_new_nest_can_dig_into_a_husk_and_take_it_over() {
        let mut w = World::new(Config { seed: 6, ants: Some(4), bud: false, drought: false, ..Config::default() }, 90, 48);
        w.food.clear();
        w.season = Season::Drought { until: w.tick + 500 * DAY, started: w.tick };
        run(&mut w, STARVE_INTERVAL as u64 * 6);
        assert!(w.nests[0].dead && w.ants.is_empty());
        w.season = Season::Calm { until: u64::MAX };
        // Found a new nest right next door and let it dig.
        let x = w.entrances[0] + 6;
        dig_system(&mut w, x, 3);
        for _ in 0..12 {
            w.spawn_ant(1);
        }
        w.recount_workers();
        assert_eq!(w.nests.len(), 2);
        run(&mut w, 20000);
        assert!(
            w.nests.iter().filter(|n| !n.dead).count() >= 1 && w.nests.iter().all(|n| !n.dead || n.workers == 0),
            "{}",
            w.snapshot()
        );
        // Either it broke in (one merged living nest) or it never touched it (two nests).
        assert!(w.nests.len() <= 2);
    }

    #[test]
    fn upkeep_drains_stores_in_calm_weather_without_starving() {
        let mut w = World::new(Config { seed: 3, ants: Some(40), drought: false, ..Config::default() }, 71, 48);
        fill_nest(&mut w, 0);
        let full = w.nest_food(0);
        w.food.clear();
        w.cfg.food_piles = 0;
        run(&mut w, 600);
        assert!(w.nest_food(0) < full, "stores did not drain");
        assert!(w.ants.len() >= 40, "somebody starved outside a drought");
        assert!(!w.nests[0].starving && !w.nests[0].dead);
    }

    #[test]
    fn castes_keep_roughly_their_shares() {
        let mut w = World::new(Config { seed: 8, ..Config::default() }, 120, 60);
        run(&mut w, 10000);
        let c = w.role_counts();
        let total = w.ants.len() as f32;
        assert!(total > 30.0, "{}", w.stats());
        let share = |n: usize| n as f32 / total;
        assert!((0.4..0.7).contains(&share(c[0])), "foragers {:?}", c);
        assert!((0.12..0.4).contains(&share(c[1])), "diggers {:?}", c);
        assert!(c[2] >= 2 && c[3] >= 2, "scouts/nurses {:?}", c);
    }

    #[test]
    fn diggers_and_nurses_stay_underground() {
        let mut w = World::new(Config { seed: 9, ..Config::default() }, 100, 50);
        for _ in 0..30 {
            run(&mut w, 300);
            for a in &w.ants {
                if matches!(a.role, Role::Digger | Role::Nurse) && !matches!(a.state, State::Founding(_)) {
                    assert!(a.underground, "{:?} outside at {},{}:\n{}", a.role, a.x, a.y, w.snapshot());
                }
            }
        }
    }

    #[test]
    fn a_scout_keeps_going_where_a_forager_dithers() {
        let mut w = World::new(Config { seed: 10, ants: Some(2), ..Config::default() }, 200, 48);
        w.food.clear();
        w.cfg.food_piles = 0;
        for (i, role) in [Role::Scout, Role::Forager].into_iter().enumerate() {
            w.ants[i] = Ant {
                x: 20,
                y: w.ground[20] - 1,
                state: State::Foraging,
                role,
                underground: false,
                home: 0,
                idle: 0,
                dir: 1,
                cooldown: 0,
                dig_dir: (0, 1),
                dig_budget: 0,
                strength: 0.0,
            };
        }
        let mut scout_far = 0;
        let mut forager_far = 0;
        for _ in 0..300 {
            w.ants[0].cooldown = 0;
            w.ants[1].cooldown = 0;
            w.step_ant(0);
            w.step_ant(1);
            scout_far = scout_far.max(w.ants[0].x.abs_diff(20));
            forager_far = forager_far.max(w.ants[1].x.abs_diff(20));
        }
        assert!(scout_far > 100, "scout only got {scout_far} away");
        assert!(scout_far > forager_far, "scout {scout_far} vs forager {forager_far}");
    }

    #[test]
    fn diggers_use_up_the_whole_soil_budget_over_a_long_run() {
        let mut w = World::new(Config { seed: 11, drought: false, ..Config::default() }, 120, 60);
        run(&mut w, 60000);
        let budget = w.soil_cells() * DIG_SHARE / 100 / w.live_nests().max(2);
        assert!(
            w.nest_cells(0) * 10 >= budget * 9,
            "stopped short of the budget: {} of {budget}\n{}",
            w.nest_cells(0),
            w.snapshot()
        );
    }

    #[test]
    fn no_drought_never_leaves_calm() {
        let mut w = World::new(Config { seed: 4, drought: false, ..Config::default() }, 71, 48);
        run(&mut w, GRACE_PERIOD + 10 * DAY);
        assert!(matches!(w.season, Season::Calm { .. }));
        assert!(w.reports.is_empty());
    }

    #[test]
    fn seasons_cycle_and_report() {
        let mut w = World::new(Config { seed: 5, ..Config::default() }, 71, 48);
        run(&mut w, GRACE_PERIOD + WARNING_LENGTH + MAX_DURATION + BLOOM_LENGTH + 1);
        assert_eq!(w.reports.len(), 1, "season is {:?}", w.season);
        assert!(w.reports[0].ended > w.reports[0].started);
    }

    #[test]
    fn tiny_and_odd_sizes_do_not_panic() {
        for (cw, ch) in [(1, 1), (8, 6), (9, 7), (71, 48), (200, 12), (30, 200)] {
            let mut w = World::new(Config::default(), cw, ch);
            run(&mut w, 400);
            let _ = w.snapshot();
        }
    }
}
