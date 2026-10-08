//! A side-on ant farm: sky on top, soil below, colonies digging tunnels and chambers,
//! foraging on the surface, each ruled by a queen. No terminal types in here, so it can
//! be unit-tested and run headless.

use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct Config {
    /// Starting ants. `None` scales with the world area.
    pub ants: Option<usize>,
    /// Food piles kept on the surface at once.
    pub food_piles: usize,
    /// Queens lay eggs (off = fixed population, no budding).
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
    /// A royal chamber: twice the size of a room, dug deep for the queen.
    Royal,
}

impl Cell {
    pub fn dug(self) -> bool {
        matches!(self, Cell::Tunnel | Cell::Chamber | Cell::Royal)
    }
    pub fn room(self) -> bool {
        matches!(self, Cell::Chamber | Cell::Royal)
    }
}

/// What an ant is doing for the nest. Reassigned as the food situation changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    /// Works the surface near home, follows trails, hauls food.
    Forager = 0,
    /// Keeps to the rooms, feeds the queen and hungry diggers, minds the stores.
    Nurse = 1,
    /// Extends tunnels and rooms; digs the royal chamber when the nest is ready.
    Digger = 2,
    /// Ranges far along the surface, finds food first, founds new nests.
    Scout = 3,
}

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
    /// Digging out the royal chamber.
    DigRoyal,
    /// Walking along the surface to open a new nest at this column.
    Founding(usize),
    /// Pottering about underground for a while.
    Idle,
    /// Hungry and nobody came: walking to a room with food in it.
    GoEat,
    /// A nurse on her way to the nearest larder for the queen.
    Fetch,
    /// A nurse carrying a unit back to the queen.
    Deliver,
    /// Out of food in the body. Wandering off to die.
    Starving,
}

#[derive(Clone, Debug)]
pub struct Ant {
    pub x: usize,
    pub y: usize,
    pub state: State,
    pub job: Job,
    /// On the surface or in a tunnel.
    pub underground: bool,
    /// Index into `World::nests`.
    pub home: usize,
    /// Food in the body, 0..=FED_MAX.
    pub fed: f32,
    /// Holding a unit of food.
    pub carrying: bool,
    /// Ticks spent hungry waiting for a nurse.
    hungry_for: u32,
    /// Ticks left of idling, wandering while starving, or walking to die.
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

impl Ant {
    pub fn hungry(&self) -> bool {
        self.fed < HUNGRY
    }
}

#[derive(Clone, Debug)]
pub struct Food {
    pub x: usize,
    pub units: u32,
}

#[derive(Clone, Debug)]
pub struct Queen {
    pub x: usize,
    pub y: usize,
    pub fed: f32,
    /// Walking to the royal chamber.
    pub relocating: bool,
    pub relocations: u8,
    last_lay: u64,
    settle_until: u64,
    last_relocation: u64,
    cooldown: u8,
}

#[derive(Clone, Debug)]
pub struct Egg {
    pub cell: usize,
    pub nest: usize,
    hatch_at: u64,
}

#[derive(Clone, Debug)]
pub struct Corpse {
    pub x: usize,
    pub y: usize,
    pub queen: bool,
    pub ttl: u32,
    pub max_ttl: u32,
}

/// A royal chamber being dug, then (once `cells` is empty) waiting for the queen.
#[derive(Clone, Debug)]
pub struct Royal {
    /// Soil cells still to dig.
    pub cells: Vec<usize>,
    /// Cells already opened, where the queen will settle.
    pub dug: Vec<usize>,
    /// Every cell of the planned hall, dug or not.
    pub all: Vec<usize>,
}

/// How the nest's larder looks, measured in days it would last in a drought.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Plentiful,
    Normal,
    Scarce,
}

/// One connected tunnel system with at least one entrance.
#[derive(Clone, Debug)]
pub struct Nest {
    pub label: char,
    pub entrances: Vec<usize>,
    pub workers: usize,
    /// Lost its last worker. The tunnels stay as a husk.
    pub dead: bool,
    pub queen: Option<Queen>,
    /// Had a queen and lost her: no new one will come.
    pub queen_lost: bool,
    pub royal: Option<Royal>,
    pub band: Band,
    pub food_days: f32,
    /// Food (in units) nurses have gathered to hand out this tick.
    feed_acc: f32,
}

impl Nest {
    fn new(label: char, entrances: Vec<usize>) -> Self {
        Self {
            label,
            entrances,
            workers: 0,
            dead: false,
            queen: None,
            queen_lost: false,
            royal: None,
            band: Band::Normal,
            food_days: 0.0,
            feed_acc: 0.0,
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
    /// Ants and queens lost since the previous drought ended.
    pub ants_lost: u64,
    pub queens_lost: u64,
}

pub struct World {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<Cell>,
    /// Food units stored in each room cell.
    pub store: Vec<u8>,
    /// Row of the topmost soil cell in each column (sky above, mounds raise it).
    pub ground: Vec<usize>,
    /// Row of the original flat surface.
    pub sky: usize,
    /// Columns with an entrance at ground level.
    pub entrances: Vec<usize>,
    pub ants: Vec<Ant>,
    pub food: Vec<Food>,
    pub eggs: Vec<Egg>,
    pub corpses: Vec<Corpse>,
    /// "Food is this way" trail along the surface, one value per column.
    pub surf_ph: Vec<f32>,
    pub nests: Vec<Nest>,
    /// Nest index per cell, `NO_NEST` for soil and sky.
    pub nest_of: Vec<u16>,
    /// How busy each tunnel cell has been lately; idle diggers widen the busiest.
    pub traffic: Vec<u16>,
    /// The gold outline in the dirt around a queen's chamber: 0 none, 255 fresh. It stays
    /// after she moves and fades once her line is dead.
    pub royal_mark: Vec<u8>,
    /// Label of the nest whose queen each outline belongs to.
    royal_owner: Vec<u8>,
    pub season: Season,
    pub reports: Vec<DroughtReport>,
    pub tick: u64,
    pub delivered: u64,
    pub dug: usize,
    pub deaths: u64,
    pub queen_deaths: u64,
    /// Deaths and queen deaths as they stood when the previous drought ended.
    losses_at_last_drought: (u64, u64),
    /// Cells too close to a decaying nest to dig in until it has crumbled away.
    husk_zone: Vec<bool>,
    /// Last tick any nest had a living queen.
    last_queen_seen: u64,
    next_label: u8,
    drought_before: (usize, usize, u32),
    /// (report index, store level to reach) while a rebuild is being timed.
    rebuilding: Option<(usize, u32)>,
    exit_dist: Vec<u16>,
    store_dist: Vec<u16>,
    eat_dist: Vec<u16>,
    room_dist: Vec<u16>,
    tip_dist: Vec<u16>,
    royal_dist: Vec<u16>,
    queen_dist: Vec<u16>,
    dead_tip: Vec<bool>,
    /// Cells opened to widen a corridor: never tips, never widened again.
    widened: Vec<bool>,
    dirty: bool,
    queens_moved: bool,
    food_respawn: u32,
    cfg: Config,
    rng: fastrand::Rng,
}

#[derive(Clone, Copy)]
enum Field {
    Exit,
    Store,
    Eat,
    Room,
    Tip,
    Royal,
    Queen,
}

const FAR: u16 = u16::MAX;
const MOVE_TICKS: u8 = 3;
const DIG_TICKS: u8 = 24;
/// Living nests may hollow out this share of the soil between them.
const DIG_SHARE: usize = 40;
const VISION: usize = 8;
const SCOUT_VISION: usize = 14;
const IDLE_TICKS: u16 = 300;
const STORE_CAP: u8 = 6;
pub const NO_NEST: u16 = u16::MAX;
const DEPOSIT: f32 = 0.08;
const STRENGTH_DECAY: f32 = 0.985;
const EVAPORATION: f32 = 0.994;
const FOOD_RESPAWN_TICKS: u32 = 200;
const BUD_POPULATION: usize = 60;
const BUD_PARTY: usize = 5;
const BUD_EVERY: u64 = 900;
const NEST_GAP: usize = 18;

// Hunger. Every ant carries food in its body and eats a unit from the store when low.
pub const FED_MAX: f32 = 100.0;
pub const HUNGRY: f32 = 40.0;
const WORKER_CONSUME: f32 = 0.012;
const QUEEN_CONSUME: f32 = 0.06;
/// Fed points one food unit is worth.
const FOOD_VALUE: f32 = 40.0;
/// Hunger runs this much faster in a drought (heat, no surface water): it is what makes
/// droughts dangerous rather than merely dull.
const DROUGHT_HUNGER: f32 = 6.0;
/// Ticks a hungry digger or queen waits for a nurse before going to the store.
const PATIENCE: u32 = 400;
const STARVE_WANDER: u16 = 40;
const STARVE_TTL: u16 = 150;
const CORPSE_DECAY: u32 = 900;
const QUEEN_CORPSE_DECAY: u32 = 1500;
/// Food units one nurse can hand to a hungry digger per tick (the queen is fed by hand).
const NURSE_FEED: f32 = 0.01;
/// Nurses fetching for the queen at once.
const QUEEN_FETCHERS: usize = 3;
/// Nurses keep at least this much food in the rooms within `QUEEN_PANTRY` cells of the queen.
const QUEEN_PANTRY_FOOD: u32 = 30;
const QUEEN_PANTRY: usize = 3;
/// Traffic (recent passes, halved every `TRAFFIC_HALF_LIFE` ticks) above which an idle
/// digger will widen a corridor cell, up to `MAX_WIDTH` cells across.
const WIDEN_TRAFFIC: u16 = 30;
const MAX_WIDTH: usize = 3;
const TRAFFIC_HALF_LIFE: u64 = 200;
/// Dead-tip marks expire so a boxed-in spot gets another look once things have changed.
const DEAD_TIP_TTL: u64 = 3000;

// The queen. She carries a far bigger reserve than a worker: a founding queen lives off
// it for weeks, and in a drought it is what lets the workers starve before she does.
pub const QUEEN_FED_MAX: f32 = 400.0;
const LAY_INTERVAL: u64 = 300;
const LAY_THRESHOLD: f32 = 100.0;
/// She also skips the lay unless the larder would feed the nest this many drought-days:
/// the colony stockpiles before it grows.
const LAY_MIN_DAYS: f32 = 12.0;
/// A queen with fewer workers than this lays from her own reserve, larder or not.
const FOUNDING_WORKERS: usize = 3;
/// With no queen anywhere for this long, a new founding queen arrives from outside.
const REFOUND_AFTER: u64 = 50 * DAY;
/// A nest that has lost its queen fills back in from its extremities, one cell this often,
/// so the ground is free for whoever comes next.
const DECAY_TICKS: u64 = 30;
/// Nobody digs or founds within this many cells of a decaying nest until it is gone.
const HUSK_GAP: usize = 5;
const HATCH_TIME: u64 = 600;
const QUEEN_TICKS: u8 = 10;
const SETTLE_TIME: u64 = 500;

// Jobs. Targets are percent (forager, nurse, digger); scouts take a slice of the foragers.
const RICH_DAYS: f32 = 15.0;
const SCARCE_DAYS: f32 = 5.0;
const JOBS_PLENTIFUL: [u32; 3] = [40, 30, 30];
const JOBS_NORMAL: [u32; 3] = [60, 30, 10];
const JOBS_SCARCE: [u32; 3] = [85, 15, 0];
const SCOUT_SHARE: u32 = 10;
const REASSIGN_EVERY: u64 = 15;
const MIN_NURSES: usize = 2;

// Relocation.
const RELOCATE_GAP: usize = 8;
const RELOCATE_COOLDOWN: u64 = 300 * DAY;
const MAX_RELOCATIONS: u8 = 2;
const RELOCATE_CHANCE: f32 = 0.002;
const ROYAL_RX: usize = 4;
const ROYAL_RY: usize = 2;
const ROYAL_DIGGERS: usize = 3;
/// Outline fade step every `OUTLINE_FADE_EVERY` ticks once the queen's line is dead.
const OUTLINE_FADE_EVERY: u64 = 50;
const OUTLINE_FADE_STEP: u8 = 4;

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

impl World {
    pub fn new(cfg: Config, w: usize, h: usize) -> Self {
        let w = w.max(8);
        let h = h.max(12);
        let mut rng = fastrand::Rng::with_seed(cfg.seed);
        let sky = (h / 5).max(4);
        let n = w * h;
        let mut world = Self {
            w,
            h,
            cells: (0..n).map(|i| if i / w < sky { Cell::Sky } else { Cell::Soil }).collect(),
            store: vec![0; n],
            ground: vec![sky; w],
            sky,
            entrances: Vec::new(),
            ants: Vec::new(),
            food: Vec::new(),
            eggs: Vec::new(),
            corpses: Vec::new(),
            surf_ph: vec![0.0; w],
            nests: Vec::new(),
            nest_of: vec![NO_NEST; n],
            traffic: vec![0; n],
            royal_mark: vec![0; n],
            royal_owner: vec![0; n],
            season: Season::Calm { until: GRACE_PERIOD },
            reports: Vec::new(),
            tick: 0,
            delivered: 0,
            dug: 0,
            deaths: 0,
            queen_deaths: 0,
            losses_at_last_drought: (0, 0),
            husk_zone: vec![false; n],
            last_queen_seen: 0,
            next_label: 0,
            drought_before: (0, 0, 0),
            rebuilding: None,
            exit_dist: vec![FAR; n],
            store_dist: vec![FAR; n],
            eat_dist: vec![FAR; n],
            room_dist: vec![FAR; n],
            tip_dist: vec![FAR; n],
            royal_dist: vec![FAR; n],
            queen_dist: vec![FAR; n],
            dead_tip: vec![false; n],
            widened: vec![false; n],
            dirty: true,
            queens_moved: true,
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
        world.tend_queens();

        let n_ants = cfg.ants.unwrap_or_else(|| ((w * h) / 110).clamp(12, 120));
        for _ in 0..n_ants {
            world.spawn_ant(0, None);
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
        self.cell(x, y).dug()
    }

    fn soil_cells(&self) -> usize {
        self.w * (self.h - self.sky)
    }

    /// Each living nest gets a share of the soil to hollow out; a lone nest gets half.
    fn dig_capped(&self, nest: usize) -> bool {
        let budget = self.soil_cells() * DIG_SHARE / 100 / self.live_nests().max(2);
        self.nest_cells(nest) > budget
    }

    fn in_drought(&self) -> bool {
        matches!(self.season, Season::Drought { .. })
    }

    pub fn stored(&self) -> u32 {
        self.store.iter().map(|&s| s as u32).sum()
    }

    pub fn chambers(&self) -> usize {
        self.cells.iter().filter(|c| c.room()).count()
    }

    pub fn live_nests(&self) -> usize {
        self.nests.iter().filter(|n| !n.dead).count()
    }

    pub fn dead_nests(&self) -> usize {
        self.nests.iter().filter(|n| n.dead).count()
    }

    pub fn queenless_nests(&self) -> usize {
        self.nests.iter().filter(|n| !n.dead && n.queen.is_none()).count()
    }

    pub fn moving_queens(&self) -> usize {
        self.nests.iter().filter(|n| n.queen.as_ref().is_some_and(|q| q.relocating)).count()
    }

    pub fn royal_digs(&self) -> usize {
        self.nests.iter().filter(|n| !n.dead && n.royal.as_ref().is_some_and(|r| !r.cells.is_empty())).count()
    }

    /// Lost its queen, no eggs left to hatch: nothing more will ever come of this nest.
    fn doomed(&self, n: usize) -> bool {
        self.nests.get(n).is_some_and(|nest| !nest.dead && nest.queen.is_none() && nest.queen_lost)
            && !self.eggs.iter().any(|e| e.nest == n)
            && self.nest_food(n) == 0 // they eat the larder down first, then go
    }

    /// The nearest living nest with a queen, by entrance, for an orphan at column `x`.
    fn adoptive_nest(&self, x: usize) -> Option<usize> {
        self.live_entrances()
            .filter_map(|e| self.nest_at_entrance(e).map(|n| (n, e)))
            .filter(|&(n, _)| self.nests[n].queen.is_some())
            .min_by_key(|&(_, e)| e.abs_diff(x))
            .map(|(n, _)| n)
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

    fn nest_alive_at(&self, i: usize) -> bool {
        let n = self.nest_of[i];
        n != NO_NEST && !self.nests[n as usize].dead
    }

    /// Food stored in one nest's rooms.
    pub fn nest_food(&self, n: usize) -> u32 {
        self.nest_of
            .iter()
            .zip(&self.store)
            .filter(|(&o, _)| o as usize == n)
            .map(|(_, &s)| s as u32)
            .sum()
    }

    fn nest_cells(&self, n: usize) -> usize {
        self.nest_of.iter().filter(|&&o| o as usize == n).count()
    }

    /// Take one unit from the room cell `i`. False if it is empty.
    fn take_here(&mut self, i: usize) -> bool {
        if !self.cells[i].room() || self.store[i] == 0 {
            return false;
        }
        if self.store[i] >= STORE_CAP || self.store[i] == 1 {
            self.dirty = true;
        }
        self.store[i] -= 1;
        true
    }

    /// Food in the rooms around the queen of nest `n`.
    fn queen_pantry(&self, n: usize) -> u32 {
        let Some(q) = self.nests[n].queen.as_ref() else { return 0 };
        let mut sum = 0;
        for y in q.y.saturating_sub(QUEEN_PANTRY)..=(q.y + QUEEN_PANTRY).min(self.h - 1) {
            for x in q.x.saturating_sub(QUEEN_PANTRY)..=(q.x + QUEEN_PANTRY).min(self.w - 1) {
                let i = self.idx(x, y);
                if self.cells[i].room() {
                    sum += self.store[i] as u32;
                }
            }
        }
        sum
    }

    fn near_queen(&self, n: usize, i: usize) -> bool {
        self.nests[n]
            .queen
            .as_ref()
            .is_some_and(|q| (i % self.w).abs_diff(q.x) <= QUEEN_PANTRY && (i / self.w).abs_diff(q.y) <= QUEEN_PANTRY)
    }

    /// Take food out of a nest, fullest rooms first. False if it did not have enough.
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
            if self.store[i] >= STORE_CAP || self.store[i] == take as u8 {
                self.dirty = true; // a full room has room again, or an emptied one stops being a larder
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

    // ---- jobs -------------------------------------------------------------------------

    fn job_targets(&self, n: usize) -> [u32; 4] {
        let nest = &self.nests[n];
        if nest.queen.is_none() && !nest.queen_lost {
            return [40, 0, 60, 0]; // a young nest: dig the first room, feed the diggers
        }
        let base = if self.in_drought() {
            JOBS_SCARCE
        } else {
            match self.nests[n].band {
                Band::Plentiful => JOBS_PLENTIFUL,
                Band::Normal => JOBS_NORMAL,
                Band::Scarce => JOBS_SCARCE,
            }
        };
        // Scouts take a slice of the foraging allocation.
        let scouts = base[0] * SCOUT_SHARE / 100;
        [base[0] - scouts, base[1], base[2], scouts]
    }

    /// Target head counts per job for a nest of `total` workers: shares rounded, then a
    /// floor so even a small nest keeps a digger and a scout, and nurses for the queen.
    fn job_quotas(&self, n: usize, total: usize) -> [usize; 4] {
        let shares = self.job_targets(n);
        let mut q = [0usize; 4];
        for i in 0..4 {
            q[i] = (shares[i] as usize * total + 50) / 100;
        }
        if shares[2] > 0 && total >= 4 {
            q[2] = q[2].max(1);
        }
        if shares[3] > 0 && total >= 6 {
            q[3] = q[3].max(1);
        }
        if self.nests[n].queen.is_some() && shares[1] > 0 {
            q[1] = q[1].max(MIN_NURSES.min(total.saturating_sub(1)));
        }
        q
    }

    fn job_counts(&self, n: usize) -> [usize; 4] {
        let mut c = [0; 4];
        for a in self.ants.iter().filter(|a| a.home == n && a.state != State::Starving) {
            c[a.job as usize] += 1;
        }
        c
    }

    /// The job the nest is shortest of, for a newly hatched ant.
    fn pick_job(&self, n: usize) -> Job {
        let counts = self.job_counts(n);
        let total = counts.iter().sum::<usize>() + 1;
        let quotas = self.job_quotas(n, total);
        let mut best = Job::Forager;
        let mut best_gap = i64::MIN;
        for (i, job) in [Job::Forager, Job::Nurse, Job::Digger, Job::Scout].into_iter().enumerate() {
            let gap = quotas[i] as i64 - counts[i] as i64;
            if gap > best_gap {
                best_gap = gap;
                best = job;
            }
        }
        best
    }

    pub fn job_tally(&self) -> [usize; 4] {
        let mut c = [0; 4];
        for a in &self.ants {
            c[a.job as usize] += 1;
        }
        c
    }

    /// `f66 n12 d30 s12`
    pub fn job_summary(&self) -> String {
        let c = self.job_tally();
        format!("f{} n{} d{} s{}", c[0], c[1], c[2], c[3])
    }

    /// Every so often, move one ant per nest from the job most in surplus to the job most
    /// in deficit, so the workforce drifts rather than snaps.
    fn allocate_jobs(&mut self) {
        if !self.tick.is_multiple_of(REASSIGN_EVERY) {
            return;
        }
        for n in 0..self.nests.len() {
            if self.nests[n].dead {
                continue;
            }
            let counts = self.job_counts(n);
            let total = counts.iter().sum::<usize>();
            if total == 0 {
                continue;
            }
            let quotas = self.job_quotas(n, total);
            let gap = |i: usize| quotas[i] as i64 - counts[i] as i64;
            let (mut need, mut need_gap) = (0, i64::MIN);
            let (mut spare, mut spare_gap) = (0, i64::MAX);
            for i in 0..4 {
                if gap(i) > need_gap {
                    need = i;
                    need_gap = gap(i);
                }
                if gap(i) < spare_gap {
                    spare = i;
                    spare_gap = gap(i);
                }
            }
            if let Some(z) = (0..4).find(|&i| quotas[i] == 0 && counts[i] > 0) {
                spare = z;
                spare_gap = -1;
            }
            if need_gap <= 0 || spare_gap >= 0 {
                continue;
            }
            let queen_alive = self.nests[n].queen.is_some();
            if spare == Job::Nurse as usize && queen_alive && counts[1] <= MIN_NURSES {
                continue;
            }
            let new_job = [Job::Forager, Job::Nurse, Job::Digger, Job::Scout][need];
            let candidate = self.ants.iter().position(|a| {
                a.home == n
                    && a.job as usize == spare
                    && !a.carrying
                    && matches!(a.state, State::Idle | State::ToTip | State::Foraging | State::Digging)
            });
            if let Some(i) = candidate {
                self.ants[i].job = new_job;
                let here = self.idx(self.ants[i].x, self.ants[i].y);
                let mut ant = self.ants[i].clone();
                if ant.underground {
                    self.assign(&mut ant, here);
                } else if !matches!(new_job, Job::Forager | Job::Scout) {
                    ant.state = State::Idle; // heads home from the surface
                }
                self.ants[i] = ant;
            }
        }
    }

    /// Days the nest's store would last in a drought, and the band that puts it in.
    fn update_bands(&mut self) {
        for n in 0..self.nests.len() {
            let mouths = self.ants.iter().filter(|a| a.home == n).count() as f32 * WORKER_CONSUME
                + if self.nests[n].queen.is_some() { QUEEN_CONSUME } else { 0.0 };
            let per_tick = mouths * DROUGHT_HUNGER / FOOD_VALUE;
            let days = if per_tick > 0.0 { self.nest_food(n) as f32 / (per_tick * DAY as f32) } else { 999.0 };
            let nest = &mut self.nests[n];
            nest.food_days = days;
            nest.band = if days > RICH_DAYS {
                Band::Plentiful
            } else if days < SCARCE_DAYS {
                Band::Scarce
            } else {
                Band::Normal
            };
        }
    }

    // ---- spawning ---------------------------------------------------------------------

    /// New ant appears in nest `home`, at `cell` if given, else somewhere in its tunnels.
    fn spawn_ant(&mut self, home: usize, cell: Option<usize>) {
        let i = match cell {
            Some(c) => c,
            None => {
                let cells: Vec<usize> = (0..self.cells.len()).filter(|&i| self.nest_of[i] as usize == home).collect();
                if cells.is_empty() {
                    return;
                }
                cells[self.rng.usize(..cells.len())]
            }
        };
        let job = self.pick_job(home);
        let state = match job {
            Job::Digger => State::ToTip,
            Job::Nurse => State::Idle,
            Job::Forager | Job::Scout => State::Foraging,
        };
        self.ants.push(Ant {
            x: i % self.w,
            y: i / self.w,
            state,
            job,
            underground: true,
            home,
            fed: FED_MAX * 0.6,
            carrying: false,
            hungry_for: 0,
            idle: IDLE_TICKS,
            dir: if self.rng.bool() { 1 } else { -1 },
            cooldown: self.rng.u8(..MOVE_TICKS * 4),
            dig_dir: (0, 1),
            dig_budget: 0,
            strength: 0.0,
        });
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

    // ---- tick -------------------------------------------------------------------------

    pub fn tick(&mut self) {
        self.tick += 1;
        for i in 0..self.ants.len() {
            if self.ants[i].cooldown > 0 {
                self.ants[i].cooldown -= 1;
                continue;
            }
            self.step_ant(i);
        }
        self.bury_the_dead();
        if self.tick.is_multiple_of(TRAFFIC_HALF_LIFE) {
            for t in &mut self.traffic {
                *t /= 2;
            }
        }
        if self.tick.is_multiple_of(DEAD_TIP_TTL) {
            self.dead_tip.fill(false);
            self.dirty = true;
        }
        if self.dirty || self.queens_moved {
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
        self.update_bands();
        self.hunger();
        if self.tick.is_multiple_of(OUTLINE_FADE_EVERY) {
            self.fade_outlines();
        }
        self.tend_queens();
        self.hatch_eggs();
        self.allocate_jobs();
        self.relocation();
        for c in &mut self.corpses {
            c.ttl = c.ttl.saturating_sub(1);
        }
        self.corpses.retain(|c| c.ttl > 0);
        if let Some((i, target)) = self.rebuilding {
            if self.stored() >= target {
                self.reports[i].rebuilt_days = Some((self.tick - self.reports[i].ended) / DAY);
                self.rebuilding = None;
            }
        }
        if self.cfg.bud && self.cfg.grow && self.tick.is_multiple_of(BUD_EVERY) {
            self.maybe_bud();
        }
        if self.tick.is_multiple_of(DECAY_TICKS) {
            self.decay();
        }
        if self.nests.iter().any(|n| !n.dead && n.queen.is_some()) {
            self.last_queen_seen = self.tick;
        } else if self.cfg.grow && self.tick - self.last_queen_seen > REFOUND_AFTER {
            self.refound();
        }
    }

    /// Queenless nests and stray fragments of tunnel crumble back to soil from their tips
    /// inward, one cell per call per system, leaving the entrance for last.
    fn decay(&mut self) {
        let mut groups: Vec<Vec<usize>> = vec![Vec::new(); self.nests.len() + 1];
        for i in 0..self.cells.len() {
            if !self.cells[i].dug() {
                continue;
            }
            let n = self.nest_of[i];
            let decaying = n == NO_NEST || self.nests[n as usize].dead || self.nests[n as usize].queen_lost && self.nests[n as usize].queen.is_none();
            // Only the edge crumbles: cells with soil beside them, leaves first.
            if !decaying || self.dug_neighbours(i) == 4 || self.store[i] > 0 {
                continue;
            }
            let (x, y) = (i % self.w, i / self.w);
            let occupied = self.ants.iter().any(|a| a.underground && a.x == x && a.y == y)
                || self.eggs.iter().any(|e| e.cell == i)
                || self.nests.iter().any(|nst| nst.queen.as_ref().is_some_and(|q| (q.x, q.y) == (x, y)));
            if occupied {
                continue;
            }
            let is_entrance = y == self.ground[x] && self.entrances.contains(&x);
            let last = n == NO_NEST || self.nest_cells(n as usize) <= 1;
            if is_entrance && !last {
                continue;
            }
            let g = if n == NO_NEST { self.nests.len() } else { n as usize };
            groups[g].push(i);
        }
        for (g, cells) in groups.into_iter().enumerate() {
            if cells.is_empty() {
                continue;
            }
            // Erode from the extremities: fewest dug neighbours first, then the deepest and
            // the farthest from the entrance, so corridors shorten from their ends and rooms
            // shrink evenly instead of being gnawed into odd snakes.
            let ex = self.nests.get(g).and_then(|n| n.entrances.first().copied()).unwrap_or(self.w / 2);
            let key = |w: &Self, i: usize| {
                let (x, y) = (i % w.w, i / w.w);
                (w.dug_neighbours(i), std::cmp::Reverse(y + x.abs_diff(ex)))
            };
            let best = cells.iter().map(|&i| key(self, i)).min().unwrap();
            let ties: Vec<usize> = cells.iter().copied().filter(|&i| key(self, i) == best).collect();
            let i = ties[self.rng.usize(..ties.len())];
            let (x, y) = (i % self.w, i / self.w);
            self.cells[i] = Cell::Soil;
            self.widened[i] = false;
            self.dead_tip[i] = false;
            self.traffic[i] = 0;
            self.dug = self.dug.saturating_sub(1);
            self.corpses.retain(|c| (c.x, c.y) != (x, y)); // buried
            if y == self.ground[x] {
                self.entrances.retain(|&e| e != x);
            }
            self.dirty = true; // also recomputes the keep-out zone
        }
    }

    /// The farm has gone quiet: a founding queen lands and digs in somewhere clear.
    fn refound(&mut self) {
        self.last_queen_seen = self.tick;
        let mut x = None;
        for _ in 0..40 {
            let c = 4 + self.rng.usize(..self.w.saturating_sub(8).max(1));
            if self.entrances.iter().all(|&e| e.abs_diff(c) >= 6) && self.ground[c] + 8 < self.h && !self.husk_zone[self.idx(c, self.ground[c])] {
                x = Some(c);
                break;
            }
        }
        let Some(x) = x else { return };
        let g = self.ground[x];
        for d in 0..5 {
            if self.cell(x, g + d) == Cell::Soil {
                self.dug += 1;
            }
            self.set(x, g + d, Cell::Tunnel);
        }
        self.carve_chamber(x, g + 5, 2, 1);
        self.entrances.push(x);
        self.refresh_fields();
        self.tend_queens();
    }

    // ---- ants -------------------------------------------------------------------------

    fn step_ant(&mut self, i: usize) {
        let ant = self.ants[i].clone();
        let next = if ant.state == State::Starving {
            self.step_starving(ant)
        } else if ant.underground {
            self.step_underground(ant)
        } else {
            self.step_surface(ant)
        };
        self.ants[i] = next;
    }

    fn step_surface(&mut self, mut ant: Ant) -> Ant {
        ant.cooldown = MOVE_TICKS;
        let x = ant.x;
        // Orphans of a doomed nest throw in their lot with the nearest queen.
        if self.doomed(ant.home) && !matches!(ant.state, State::Founding(_)) {
            if let Some(n) = self.adoptive_nest(x) {
                ant.home = n;
            }
        }
        match ant.state {
            State::Foraging => {
                // Food in sight wins, then the trail (foragers only), then a wander.
                // Scouts see further, ignore trails, and keep going the way they were.
                let scout = ant.job == Job::Scout;
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
                        if ant.hungry() {
                            ant.fed = (ant.fed + FOOD_VALUE).min(FED_MAX); // eat on the spot
                            return ant;
                        }
                        ant.carrying = true;
                        ant.state = State::Returning;
                        // A scout's trail is the strong one: it is what brings the foragers.
                        ant.strength = if scout { 2.5 } else { 1.0 };
                        ant.dir = self.toward_entrance(x, ant.home);
                        return ant;
                    }
                    ant.dir = if fx > x { 1 } else { -1 };
                } else if ant.hungry() {
                    // Nothing in sight and running on empty: go home and eat from the store.
                    let dir = self.toward_entrance(x, ant.home);
                    if dir == 0 && self.is_live_entrance(x) {
                        ant.underground = true;
                        ant.y = self.ground[x];
                        ant.state = State::Idle;
                        ant.idle = 0;
                        return ant;
                    }
                    ant.dir = dir;
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
                    ant.job = Job::Digger; // founders dig the new nest out; jobs drift later
                    ant.state = State::Digging;
                    ant.dig_dir = (0, 1);
                    ant.dig_budget = 10;
                    return ant;
                }
                ant.dir = if target > x { 1 } else { -1 };
                self.walk(&mut ant);
            }
            _ => {
                // Underground jobs caught outside head home.
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
        if self.cells[here] == Cell::Tunnel {
            self.traffic[here] = self.traffic[here].saturating_add(1);
        }
        match ant.state {
            State::Foraging | State::Founding(_) => {
                if ant.hungry() && self.eat_dist[here] != FAR && !matches!(ant.state, State::Founding(_)) {
                    ant.state = State::GoEat;
                    return ant;
                }
                if self.exit_dist[here] == 0 {
                    if self.in_drought() && self.food.is_empty() {
                        // Nothing out there. Wait by the door.
                        ant.state = State::Idle;
                        ant.idle = 100;
                        return ant;
                    }
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
                if self.store_dist[here] == 0 && self.cells[here].room() {
                    if ant.hungry() {
                        ant.fed = (ant.fed + FOOD_VALUE).min(FED_MAX); // eats what it carried
                    } else {
                        self.store[here] += 1;
                        self.delivered += 1;
                        if self.store[here] >= STORE_CAP || self.store[here] == 1 {
                            self.dirty = true;
                        }
                    }
                    ant.carrying = false;
                    self.assign(&mut ant, here);
                } else if self.store_dist[here] == FAR {
                    // Nowhere to put it: no room with space. Go and dig one.
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
                    ant.dig_dir = if self.cells[here].room() { (0, 1) } else { self.pick_dig_dir((0, 1)) };
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
                        self.dig(nx, ny, ant.x, ant.home);
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
                    // Boxed in. A real tip is marked dead for a while; either way back off.
                    if self.dug_neighbours(here) <= 1 {
                        self.dead_tip[here] = true;
                        self.dirty = true;
                    }
                    ant.state = State::Idle;
                    ant.idle = 100;
                }
            }
            State::DigRoyal => {
                let n = ant.home;
                let active = self.nests[n].royal.as_ref().is_some_and(|r| !r.cells.is_empty())
                    && self.nests[n].band == Band::Plentiful
                    && !self.in_drought();
                if !active {
                    self.assign(&mut ant, here); // down tools until things look up
                    return ant;
                }
                if self.royal_dist[here] == 0 {
                    // Open an adjacent cell of the plan.
                    let target = self.nests[n]
                        .royal
                        .as_ref()
                        .and_then(|r| r.cells.iter().copied().find(|&c| self.adjacent(c, here)));
                    if let Some(c) = target {
                        self.cells[c] = Cell::Royal;
                        self.dug += 1;
                        self.dirty = true;
                        let r = self.nests[n].royal.as_mut().unwrap();
                        r.cells.retain(|&k| k != c);
                        r.dug.push(c);
                        ant.x = c % self.w;
                        ant.y = c / self.w;
                        ant.cooldown = DIG_TICKS;
                        self.raise_mound(ant.x);
                    } else {
                        self.assign(&mut ant, here);
                    }
                } else if self.royal_dist[here] == FAR {
                    self.assign(&mut ant, here);
                } else if let Some((nx, ny)) = self.descend(Field::Royal, ant.x, ant.y) {
                    ant.x = nx;
                    ant.y = ny;
                }
            }
            State::GoEat => {
                if self.eat_dist[here] == 0 && self.store[here] > 0 {
                    if self.take_here(here) {
                        ant.fed = (ant.fed + FOOD_VALUE).min(FED_MAX);
                        ant.hungry_for = 0;
                    }
                    self.assign(&mut ant, here);
                } else if self.eat_dist[here] == FAR {
                    self.assign(&mut ant, here); // nothing to eat anywhere
                } else if let Some((nx, ny)) = self.descend(Field::Eat, ant.x, ant.y) {
                    ant.x = nx;
                    ant.y = ny;
                }
            }
            State::Fetch => {
                if self.eat_dist[here] == 0 && self.store[here] > 0 {
                    let queen_low = self.nests[ant.home].queen.as_ref().is_some_and(|q| q.fed < QUEEN_FED_MAX - FOOD_VALUE);
                    if !queen_low && self.near_queen(ant.home, here) {
                        // Only stocking the pantry, and this already is the pantry.
                        self.assign(&mut ant, here);
                    } else if self.take_here(here) {
                        ant.carrying = true;
                        ant.state = State::Deliver;
                    } else {
                        self.assign(&mut ant, here);
                    }
                } else if self.eat_dist[here] == FAR {
                    self.assign(&mut ant, here); // nothing left anywhere
                } else if let Some((nx, ny)) = self.descend(Field::Eat, ant.x, ant.y) {
                    ant.x = nx;
                    ant.y = ny;
                }
            }
            State::Deliver => {
                let queen_low = self.nests[ant.home].queen.as_ref().is_some_and(|q| q.fed < QUEEN_FED_MAX - FOOD_VALUE);
                let pantry_cell = |w: &Self, i: usize| w.near_queen(ant.home, i) && w.cells[i].room() && w.store[i] < STORE_CAP;
                if !queen_low && pantry_cell(self, here) {
                    self.store[here] += 1; // into the pantry beside her
                    self.dirty = true;
                    ant.carrying = false;
                    self.assign(&mut ant, here);
                } else if self.queen_dist[here] <= 1 {
                    if queen_low {
                        if let Some(q) = self.nests[ant.home].queen.as_mut() {
                            q.fed = (q.fed + FOOD_VALUE).min(QUEEN_FED_MAX);
                        }
                        ant.carrying = false;
                        self.assign(&mut ant, here);
                    } else {
                        // Standing in the doorway: step into the hall to put it down.
                        let spot = [(1, 0), (-1, 0), (0, 1), (0, -1)].into_iter().find_map(|(dx, dy)| {
                            let (nx, ny) = (ant.x as i32 + dx, ant.y as i32 + dy);
                            if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                                return None;
                            }
                            let j = self.idx(nx as usize, ny as usize);
                            pantry_cell(self, j).then_some((nx as usize, ny as usize))
                        });
                        if let Some((nx, ny)) = spot {
                            ant.x = nx;
                            ant.y = ny;
                        } else {
                            // No room to put it: she can take a top-up after all.
                            if let Some(q) = self.nests[ant.home].queen.as_mut() {
                                q.fed = (q.fed + FOOD_VALUE).min(QUEEN_FED_MAX);
                            }
                            ant.carrying = false;
                            self.assign(&mut ant, here);
                        }
                    }
                } else if self.queen_dist[here] == FAR {
                    // No queen to bring it to: eat it or put it down.
                    if ant.hungry() {
                        ant.fed = (ant.fed + FOOD_VALUE).min(FED_MAX);
                    } else if self.cells[here].room() && self.store[here] < STORE_CAP {
                        self.store[here] += 1;
                        self.dirty = true;
                    }
                    ant.carrying = false;
                    self.assign(&mut ant, here);
                } else if let Some((nx, ny)) = self.descend(Field::Queen, ant.x, ant.y) {
                    ant.x = nx;
                    ant.y = ny;
                }
            }
            State::Idle => {
                if ant.idle == 0 {
                    self.assign(&mut ant, here);
                    return ant;
                }
                ant.idle -= 1;
                if ant.job == Job::Digger && self.rng.f32() < 0.1 {
                    if let Some(c) = self.widen_spot(here) {
                        self.cells[c] = Cell::Tunnel;
                        self.widened[c] = true;
                        self.dug += 1;
                        self.dirty = true;
                        ant.cooldown = DIG_TICKS;
                        return ant;
                    }
                }
                if ant.job == Job::Nurse {
                    // Nurses eat from the larder they stand in, fetch for the queen when
                    // she is getting low, and otherwise keep near her.
                    if ant.hungry() && self.take_here(here) {
                        ant.fed = (ant.fed + FOOD_VALUE).min(FED_MAX);
                    }
                    let queen_low = self.nests[ant.home].queen.as_ref().is_some_and(|q| q.fed < QUEEN_FED_MAX - FOOD_VALUE);
                    let pantry_low = self.nests[ant.home].queen.is_some() && self.queen_pantry(ant.home) < QUEEN_PANTRY_FOOD;
                    if (queen_low || pantry_low) && !ant.hungry() && self.eat_dist[here] != FAR {
                        let fetching = self
                            .ants
                            .iter()
                            .filter(|a| a.home == ant.home && matches!(a.state, State::Fetch | State::Deliver))
                            .count();
                        if fetching < QUEEN_FETCHERS {
                            ant.state = State::Fetch;
                            return ant;
                        }
                    }
                    let d = self.queen_dist[here];
                    if d != FAR && d > 2 {
                        if let Some((nx, ny)) = self.descend(Field::Queen, ant.x, ant.y) {
                            ant.x = nx;
                            ant.y = ny;
                        }
                        return ant;
                    }
                }
                if let Some((nx, ny)) = self.wander(ant.x, ant.y, ant.job == Job::Nurse) {
                    ant.x = nx;
                    ant.y = ny;
                }
            }
            State::Starving => unreachable!(),
        }
        ant
    }

    /// A starving ant drops everything and wanders off to die: on the surface after a few
    /// steps, underground on reaching a room or when its time runs out.
    fn step_starving(&mut self, mut ant: Ant) -> Ant {
        ant.cooldown = MOVE_TICKS;
        if ant.idle == 0 {
            return ant; // dies this tick, see bury_the_dead
        }
        ant.idle -= 1;
        if ant.underground {
            let here = self.idx(ant.x, ant.y);
            if self.room_dist[here] == 0 {
                ant.idle = 0;
            } else if let Some((nx, ny)) = self.descend(Field::Room, ant.x, ant.y) {
                ant.x = nx;
                ant.y = ny;
            }
        } else {
            if self.rng.f32() < 0.2 {
                ant.dir = -ant.dir;
            }
            self.walk(&mut ant);
        }
        ant
    }

    /// Remove ants whose time is up and leave a corpse where they fell.
    fn bury_the_dead(&mut self) {
        let mut i = 0;
        while i < self.ants.len() {
            if self.ants[i].state == State::Starving && self.ants[i].idle == 0 {
                let a = self.ants.swap_remove(i);
                self.corpses.push(Corpse { x: a.x, y: a.y, queen: false, ttl: CORPSE_DECAY, max_ttl: CORPSE_DECAY });
                self.deaths += 1;
            } else {
                i += 1;
            }
        }
    }

    /// What an ant does next after finishing a job underground.
    fn assign(&mut self, ant: &mut Ant, here: usize) {
        if self.doomed(ant.home) && self.exit_dist[here] != FAR {
            ant.state = State::Foraging; // nothing to stay for: head out and find a new home
            return;
        }
        let waits_for_nurse = ant.job == Job::Digger;
        if ant.hungry() && (!waits_for_nurse || ant.hungry_for > PATIENCE) && self.eat_dist[here] != FAR {
            ant.state = State::GoEat;
            return;
        }
        ant.state = match ant.job {
            Job::Nurse => {
                ant.idle = IDLE_TICKS;
                State::Idle
            }
            Job::Digger => {
                let n = ant.home;
                let royal_wanted = self.nests[n].royal.as_ref().is_some_and(|r| !r.cells.is_empty())
                    && self.nests[n].band == Band::Plentiful
                    && !self.in_drought()
                    && self.ants.iter().filter(|a| a.home == n && a.state == State::DigRoyal).count() < ROYAL_DIGGERS
                    && self.royal_dist[here] != FAR;
                if royal_wanted {
                    State::DigRoyal
                } else if self.dig_capped(n) || self.tip_dist[here] == FAR {
                    ant.idle = IDLE_TICKS;
                    State::Idle
                } else {
                    State::ToTip
                }
            }
            Job::Scout | Job::Forager => State::Foraging,
        };
    }

    /// Step to a random dug neighbour, preferring rooms if asked.
    fn wander(&mut self, x: usize, y: usize, prefer_room: bool) -> Option<(usize, usize)> {
        let mut any = Vec::with_capacity(4);
        let mut rooms = Vec::with_capacity(4);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if self.is_dug(nx, ny) {
                any.push((nx, ny));
                if self.cell(nx, ny).room() {
                    rooms.push((nx, ny));
                }
            }
        }
        let pick = if prefer_room && !rooms.is_empty() && self.rng.f32() < 0.85 { rooms } else { any };
        if pick.is_empty() {
            None
        } else {
            Some(pick[self.rng.usize(..pick.len())])
        }
    }

    /// Step to the neighbouring dug cell that is closer to the field's sources, or
    /// wander if none is.
    fn descend(&mut self, which: Field, x: usize, y: usize) -> Option<(usize, usize)> {
        let field: &[u16] = match which {
            Field::Exit => &self.exit_dist,
            Field::Store => &self.store_dist,
            Field::Eat => &self.eat_dist,
            Field::Room => &self.room_dist,
            Field::Tip => &self.tip_dist,
            Field::Royal => &self.royal_dist,
            Field::Queen => &self.queen_dist,
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

    /// A soil cell beside a busy corridor cell that an idle digger may open, widening the
    /// corridor up to `MAX_WIDTH` across. Only corridors (two opposite dug neighbours whose
    /// other two sides are soil or earlier widening), never near the surface or another tunnel.
    fn widen_spot(&mut self, here: usize) -> Option<usize> {
        if self.cells[here] != Cell::Tunnel || self.widened[here] {
            return None;
        }
        let (x, y) = (here % self.w, here / self.w);
        if x < 2 || y < 2 || x + 2 >= self.w || y + 2 >= self.h || y < self.ground[x] + 3 {
            return None;
        }
        let at = |w: &Self, dx: i32, dy: i32| w.idx((x as i32 + dx) as usize, (y as i32 + dy) as usize);
        let dug = |w: &Self, dx: i32, dy: i32| w.cells[at(w, dx, dy)].dug();
        // A side is "open" if it is soil or one of our own widened cells.
        let side_ok = |w: &Self, dx: i32, dy: i32| !dug(w, dx, dy) || w.widened[at(w, dx, dy)];
        let horizontal = dug(self, 1, 0) && dug(self, -1, 0) && side_ok(self, 0, 1) && side_ok(self, 0, -1);
        let vertical = dug(self, 0, 1) && dug(self, 0, -1) && side_ok(self, 1, 0) && side_ok(self, -1, 0);
        let sides: [(i32, i32); 2] = if horizontal {
            [(0, 1), (0, -1)]
        } else if vertical {
            [(1, 0), (-1, 0)]
        } else {
            return None;
        };
        let already = sides.iter().filter(|&&(dx, dy)| dug(self, dx, dy)).count();
        if already + 1 >= MAX_WIDTH {
            return None;
        }
        // Along-corridor neighbours, to judge whether a side continues existing widening.
        let along: [(i32, i32); 2] = if horizontal { [(1, 0), (-1, 0)] } else { [(0, 1), (0, -1)] };
        let widened_at = |w: &Self, dx: i32, dy: i32| {
            let (ax, ay) = (x as i32 + dx, y as i32 + dy);
            ax >= 0 && ay >= 0 && (ax as usize) < w.w && (ay as usize) < w.h && w.widened[w.idx(ax as usize, ay as usize)]
        };
        // Smoothing: a gap between two widened cells is filled first, whatever the traffic;
        // extending an existing widened stretch comes next and needs less traffic than
        // starting a fresh one, so widening grows in runs instead of scattered bumps.
        let mut ranked: Vec<((i32, i32), u8)> = sides
            .iter()
            .map(|&(dx, dy)| {
                let before = widened_at(self, dx + along[0].0, dy + along[0].1);
                let after = widened_at(self, dx + along[1].0, dy + along[1].1);
                let rank = if before && after { 0 } else if before || after { 1 } else { 2 };
                ((dx, dy), rank)
            })
            .collect();
        ranked.sort_by_key(|&(_, r)| r);
        if self.rng.bool() && ranked[0].1 == ranked[1].1 {
            ranked.swap(0, 1);
        }
        for ((dx, dy), rank) in ranked {
            let needed = match rank {
                0 => 0,
                1 => WIDEN_TRAFFIC / 2,
                _ => WIDEN_TRAFFIC,
            };
            if self.traffic[here] < needed {
                continue;
            }
            let (cx, cy) = ((x as i32 + dx) as usize, (y as i32 + dy) as usize);
            let c = self.idx(cx, cy);
            if self.cells[c] != Cell::Soil || self.planned_royal(c) {
                continue;
            }
            // Beyond the corridor, the new cell may only touch other widened cells.
            let mut clear = true;
            for (ex, ey) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = ((cx as i32 + ex) as usize, (cy as i32 + ey) as usize);
                let j = self.idx(nx, ny);
                if j == here {
                    continue;
                }
                if self.is_dug(nx, ny) && !self.widened[j] {
                    clear = false;
                }
            }
            if clear {
                return Some(c);
            }
        }
        None
    }

    fn adjacent(&self, a: usize, b: usize) -> bool {
        let (ax, ay) = (a % self.w, a / self.w);
        let (bx, by) = (b % self.w, b / self.w);
        ax.abs_diff(bx) + ay.abs_diff(by) == 1
    }

    // ---- hunger, queens, eggs ---------------------------------------------------------

    /// Everyone's body burns food; nurses hand out the nest's store to the queen and to
    /// hungry diggers; whoever hits zero starts dying.
    fn hunger(&mut self) {
        let rate = if self.in_drought() { DROUGHT_HUNGER } else { 1.0 };
        for n in 0..self.nests.len() {
            if self.nests[n].dead {
                continue;
            }
            let nurses = self
                .ants
                .iter()
                .filter(|a| a.home == n && a.job == Job::Nurse && a.state != State::Starving && a.underground)
                .count();
            self.nests[n].feed_acc = (self.nests[n].feed_acc + nurses as f32 * NURSE_FEED).min(2.0);
            // The queen is fed by nurses carrying food to her (see `Fetch`/`Deliver`);
            // this hand-out is for the hungriest digger.
            while self.nests[n].feed_acc >= 1.0 {
                let hungriest = self
                    .ants
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| a.home == n && a.underground && a.job == Job::Digger && a.hungry() && a.state != State::Starving)
                    .min_by(|a, b| a.1.fed.total_cmp(&b.1.fed))
                    .map(|(i, _)| i);
                let Some(i) = hungriest else { break };
                if !self.take_food(n, 1) {
                    break;
                }
                self.nests[n].feed_acc -= 1.0;
                self.ants[i].fed = (self.ants[i].fed + FOOD_VALUE).min(FED_MAX);
                self.ants[i].hungry_for = 0;
            }
        }
        for a in &mut self.ants {
            a.fed -= WORKER_CONSUME * rate;
            if a.hungry() {
                a.hungry_for += 1;
            } else {
                a.hungry_for = 0;
            }
            if a.fed <= 0.0 && a.state != State::Starving {
                a.fed = 0.0;
                a.state = State::Starving;
                a.carrying = false;
                a.idle = if a.underground { STARVE_TTL } else { STARVE_WANDER };
            }
        }
        for n in 0..self.nests.len() {
            let Some(q) = self.nests[n].queen.as_mut() else { continue };
            q.fed -= QUEEN_CONSUME * rate;
            if q.fed <= 0.0 {
                let (x, y) = (q.x, q.y);
                self.corpses.push(Corpse { x, y, queen: true, ttl: QUEEN_CORPSE_DECAY, max_ttl: QUEEN_CORPSE_DECAY });
                self.queen_deaths += 1;
                self.nests[n].queen = None;
                self.nests[n].queen_lost = true;
                self.nests[n].royal = None;
                self.queens_moved = true;
            }
        }
    }

    /// Make the room around (x, y) the queen's chamber: its cells become royal and the
    /// soil around it gets the outline, owned by nest `n`'s line.
    fn crown(&mut self, n: usize, x: usize, y: usize) {
        let start = self.idx(x, y);
        if !self.cells[start].room() {
            return;
        }
        let label = self.nests[n].label as u8;
        let mut hall = vec![start];
        let mut q = VecDeque::from([start]);
        let mut seen = vec![false; self.cells.len()];
        seen[start] = true;
        while let Some(i) = q.pop_front() {
            let (cx, cy) = (i % self.w, i / self.w);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
                if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                    continue;
                }
                let j = self.idx(nx as usize, ny as usize);
                if !seen[j] && self.cells[j].room() {
                    seen[j] = true;
                    hall.push(j);
                    q.push_back(j);
                }
            }
        }
        for &i in &hall {
            self.cells[i] = Cell::Royal;
            let (cx, cy) = (i % self.w, i / self.w);
            for dy in -1..=1_i32 {
                for dx in -1..=1_i32 {
                    let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                        continue;
                    }
                    let j = self.idx(nx as usize, ny as usize);
                    if self.cells[j] == Cell::Soil {
                        self.royal_mark[j] = 255;
                        self.royal_owner[j] = label;
                    }
                }
            }
        }
        self.dirty = true;
    }

    /// Outlines whose line has died fade away.
    fn fade_outlines(&mut self) {
        let mut alive = [false; 256];
        for nest in &self.nests {
            if !nest.dead && nest.queen.is_some() {
                alive[nest.label as u8 as usize] = true;
            }
        }
        for i in 0..self.royal_mark.len() {
            if self.royal_mark[i] > 0 && (!alive[self.royal_owner[i] as usize] || self.cells[i] != Cell::Soil) {
                self.royal_mark[i] = self.royal_mark[i].saturating_sub(OUTLINE_FADE_STEP);
            }
        }
    }

    /// A nest that has never had a queen gets one in its first room; a queen with a full
    /// belly lays an egg every so often.
    fn tend_queens(&mut self) {
        for n in 0..self.nests.len() {
            if self.nests[n].dead {
                continue;
            }
            if self.nests[n].queen.is_none() && !self.nests[n].queen_lost {
                let room = (0..self.cells.len()).find(|&i| self.nest_of[i] as usize == n && self.cells[i].room());
                if let Some(i) = room {
                    self.nests[n].queen = Some(Queen {
                        x: i % self.w,
                        y: i / self.w,
                        fed: QUEEN_FED_MAX,
                        relocating: false,
                        relocations: 0,
                        last_lay: self.tick,
                        settle_until: self.tick,
                        last_relocation: self.tick,
                        cooldown: 0,
                    });
                    self.crown(n, i % self.w, i / self.w);
                    self.queens_moved = true;
                }
            }
            let crowded = {
                let nest = &self.nests[n];
                nest.workers * 2 >= self.nest_cells(n) + 4
            };
            let stocked = self.nests[n].food_days >= LAY_MIN_DAYS || self.nests[n].workers < FOUNDING_WORKERS;
            let can_lay = self.cfg.grow && self.ants.len() + self.eggs.len() < self.cfg.max_ants && !crowded && stocked;
            let Some(q) = self.nests[n].queen.as_mut() else { continue };
            if q.relocating || self.tick < q.settle_until || self.tick - q.last_lay < LAY_INTERVAL {
                continue;
            }
            if q.fed >= LAY_THRESHOLD && can_lay {
                q.last_lay = self.tick;
                let (qx, qy) = (q.x, q.y);
                let cell = self.egg_spot(n, qx, qy);
                self.eggs.push(Egg { cell, nest: n, hatch_at: self.tick + HATCH_TIME });
            } else {
                q.last_lay = self.tick; // skipped this one: hungry, or the larder is thin
            }
        }
    }

    /// A room cell near the queen for an egg, ideally not already holding one.
    fn egg_spot(&mut self, n: usize, qx: usize, qy: usize) -> usize {
        let qi = self.idx(qx, qy);
        let mut spots: Vec<usize> = (0..self.cells.len())
            .filter(|&i| {
                self.nest_of[i] as usize == n
                    && self.cells[i].room()
                    && i != qi
                    && (i % self.w).abs_diff(qx) <= 3
                    && (i / self.w).abs_diff(qy) <= 2
                    && !self.eggs.iter().any(|e| e.cell == i)
            })
            .collect();
        if spots.is_empty() {
            spots.push(qi);
        }
        spots[self.rng.usize(..spots.len())]
    }

    fn hatch_eggs(&mut self) {
        let mut i = 0;
        while i < self.eggs.len() {
            if self.eggs[i].hatch_at <= self.tick {
                let e = self.eggs.swap_remove(i);
                if self.cells[e.cell].dug() {
                    let n = self.nest_of[e.cell];
                    let home = if n == NO_NEST { e.nest } else { n as usize };
                    if home < self.nests.len() && !self.nests[home].dead {
                        self.spawn_ant(home, Some(e.cell));
                    }
                }
            } else {
                i += 1;
            }
        }
    }

    // ---- relocation -------------------------------------------------------------------

    /// A deep, prosperous nest digs a royal chamber near the bottom and the queen walks
    /// down to it once it is open.
    fn relocation(&mut self) {
        for n in 0..self.nests.len() {
            if self.nests[n].dead {
                continue;
            }
            let Some(q) = self.nests[n].queen.clone() else { continue };
            if q.relocating {
                self.walk_queen(n);
                continue;
            }
            if let Some(r) = &self.nests[n].royal {
                if r.cells.is_empty() {
                    self.nests[n].queen.as_mut().unwrap().relocating = true;
                    self.queens_moved = true; // the royal field now leads to the chamber
                    self.dirty = true;
                }
                continue;
            }
            let depth = (0..self.cells.len())
                .filter(|&i| self.nest_of[i] as usize == n)
                .map(|i| i / self.w)
                .max()
                .unwrap_or(0);
            let ready = self.nests[n].band == Band::Plentiful
                && !self.in_drought()
                && depth >= q.y + RELOCATE_GAP
                && self.tick - q.last_relocation >= RELOCATE_COOLDOWN
                && q.relocations < MAX_RELOCATIONS
                && self.rng.f32() < RELOCATE_CHANCE;
            if ready {
                self.plan_royal(n, q.y + RELOCATE_GAP);
            }
        }
    }

    /// Find a suitable deeper chamber: an existing room below `min_y` by preference (the
    /// diggers then enlarge it into a hall), else a stretch of tunnel to carve one around.
    fn plan_royal(&mut self, n: usize, min_y: usize) {
        let mut rooms: Vec<usize> = (0..self.cells.len())
            .filter(|&i| self.nest_of[i] as usize == n && self.cells[i] == Cell::Chamber && i / self.w >= min_y)
            .collect();
        rooms.sort_by_key(|&i| std::cmp::Reverse(i / self.w)); // deepest first
        let tunnels: Vec<usize> = (0..self.cells.len())
            .filter(|&i| self.nest_of[i] as usize == n && self.cells[i] == Cell::Tunnel && i / self.w >= min_y)
            .collect();
        let mut tries: Vec<usize> = rooms.iter().copied().take(12).collect();
        for _ in 0..20 {
            if tunnels.is_empty() {
                break;
            }
            tries.push(tunnels[self.rng.usize(..tunnels.len())]);
        }
        for a in tries {
            let (cx, cy) = (a % self.w, a / self.w);
            if let Some((cells, all)) = self.royal_blob(cx, cy) {
                let dug: Vec<usize> = all.iter().copied().filter(|&i| self.cells[i].dug()).collect();
                self.nests[n].royal = Some(Royal { cells, dug, all });
                self.dirty = true;
                return;
            }
        }
    }

    /// Soil cells of an ellipse around (cx, cy) to dig for a royal chamber. The nest's own
    /// tunnels inside the ellipse simply become part of the hall; another nest's anywhere
    /// near it rule the spot out.
    fn royal_blob(&self, cx: usize, cy: usize) -> Option<(Vec<usize>, Vec<usize>)> {
        let own_nest = self.nest_of[self.idx(cx, cy)];
        let mut cells = Vec::new();
        let mut all = Vec::new();
        for dy in -(ROYAL_RY as i32)..=(ROYAL_RY as i32) {
            for dx in -(ROYAL_RX as i32)..=(ROYAL_RX as i32) {
                let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                if x < 1 || y < 1 || x >= self.w as i32 - 1 || y >= self.h as i32 - 1 {
                    return None;
                }
                let e = (dx as f32 / (ROYAL_RX as f32 + 0.5)).powi(2) + (dy as f32 / (ROYAL_RY as f32 + 0.5)).powi(2);
                if e > 1.0 {
                    continue;
                }
                let (x, y) = (x as usize, y as usize);
                if (x, y) == (cx, cy) || y <= self.ground[x] + 2 {
                    continue;
                }
                let i = self.idx(x, y);
                match self.cell(x, y) {
                    Cell::Soil if self.husk_zone[i] => return None,
                    Cell::Soil => {
                        cells.push(i);
                        all.push(i);
                    }
                    Cell::Sky => return None,
                    _ if self.nest_of[i] == own_nest => all.push(i),
                    _ => return None,
                }
            }
        }
        all.push(self.idx(cx, cy));
        for &c in &cells {
            let (x, y) = (c % self.w, c / self.w);
            for dy in -1..=1_i32 {
                for dx in -1..=1_i32 {
                    let (ax, ay) = ((x as i32 + dx) as usize, (y as i32 + dy) as usize);
                    let j = self.idx(ax, ay);
                    if self.is_dug(ax, ay) && self.nest_of[j] != own_nest {
                        return None;
                    }
                }
            }
        }
        (cells.len() >= 8).then_some((cells, all))
    }

    fn walk_queen(&mut self, n: usize) {
        let q = self.nests[n].queen.as_mut().unwrap();
        if q.cooldown > 0 {
            q.cooldown -= 1;
            return;
        }
        q.cooldown = QUEEN_TICKS;
        let (qx, qy) = (q.x, q.y);
        let here = self.idx(qx, qy);
        if self.royal_dist[here] == 0 {
            let q = self.nests[n].queen.as_mut().unwrap();
            q.relocating = false;
            q.relocations += 1;
            q.last_relocation = self.tick;
            q.settle_until = self.tick + SETTLE_TIME;
            // The hall is hers now: every opened cell of the plan becomes royal.
            if let Some(r) = self.nests[n].royal.take() {
                for i in r.all {
                    if self.cells[i].dug() {
                        self.cells[i] = Cell::Royal;
                    }
                }
            }
            self.crown(n, qx, qy);
            self.queens_moved = true;
            self.dirty = true;
            return;
        }
        if self.royal_dist[here] == FAR {
            // Cut off somehow; stay put and give up the move.
            let q = self.nests[n].queen.as_mut().unwrap();
            q.relocating = false;
            self.nests[n].royal = None;
            self.queens_moved = true;
            return;
        }
        if let Some((nx, ny)) = self.descend(Field::Royal, qx, qy) {
            let q = self.nests[n].queen.as_mut().unwrap();
            q.x = nx;
            q.y = ny;
            self.queens_moved = true;
        }
    }

    // ---- digging ----------------------------------------------------------------------

    fn pick_dig_dir(&mut self, current: (i32, i32)) -> (i32, i32) {
        if self.rng.f32() < 0.6 && current != (0, 0) {
            return current;
        }
        // Mostly down, some sideways, rarely up.
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
        if ny < self.ground[nx] || self.cell(nx, ny) != Cell::Soil || self.husk_zone[self.idx(nx, ny)] {
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
                if self.cell(ax, ay).room() && ax.abs_diff(fx) <= 3 && ay.abs_diff(fy) <= 3 {
                    continue; // our own room, when digging out of it
                }
                if self.is_dug(ax, ay) {
                    return false; // husks included: they crumble away in their own time
                }
                if self.planned_royal(self.idx(ax, ay)) {
                    return false; // leave the royal chamber's earth alone
                }
            }
        }
        true
    }

    fn planned_royal(&self, i: usize) -> bool {
        self.nests.iter().any(|n| n.royal.as_ref().is_some_and(|r| r.cells.contains(&i)))
    }

    fn dig(&mut self, x: usize, y: usize, from_x: usize, home: usize) {
        self.set(x, y, Cell::Tunnel);
        self.dug += 1;
        let depth = y.saturating_sub(self.ground[x]);
        // A nest without a queen yet is desperate for its first room.
        let young = self.nests.get(home).is_some_and(|n| n.queen.is_none() && !n.queen_lost);
        let (min_depth, p_room) = if young { (4, 0.5) } else { (4, 0.12) };
        if depth >= min_depth && self.rng.f32() < p_room && !self.chamber_near(x, y, 5) {
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
                if self.cell(cx, cy).room() {
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
                if self.husk_zone[self.idx(x, y)] && self.cell(x, y) == Cell::Soil {
                    return; // too close to a decaying nest
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
                    if self.is_dug(ax, ay) || self.planned_royal(self.idx(ax, ay)) {
                        return;
                    }
                }
            }
        }
        for (x, y) in cells {
            if self.cell(x, y) == Cell::Soil {
                self.dug += 1;
            }
            if self.cell(x, y) != Cell::Royal {
                self.set(x, y, Cell::Chamber);
            }
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
        if self.dirty {
            self.label_nests();
            self.mark_husk_zone();
        }
        self.dirty = false;
        self.queens_moved = false;
        let n = self.cells.len();
        let exits: Vec<usize> = self
            .entrances
            .iter()
            .map(|&x| self.idx(x, self.ground[x]))
            .filter(|&i| self.cells[i].dug() && self.nest_alive_at(i))
            .collect();
        self.exit_dist = self.bfs(&exits);

        let stores: Vec<usize> = (0..n)
            .filter(|&i| self.cells[i].room() && self.store[i] < STORE_CAP && self.nest_alive_at(i))
            .collect();
        self.store_dist = self.bfs(&stores);

        let larders: Vec<usize> = (0..n).filter(|&i| self.cells[i].room() && self.store[i] > 0).collect();
        self.eat_dist = self.bfs(&larders);

        let rooms: Vec<usize> = (0..n).filter(|&i| self.cells[i].room()).collect();
        self.room_dist = self.bfs(&rooms);

        let usable = |w: &Self, i: usize| w.cells[i] == Cell::Tunnel && !w.dead_tip[i] && !w.widened[i] && w.nest_alive_at(i);
        let mut tips: Vec<usize> = (0..n)
            .filter(|&i| usable(self, i) && self.dug_neighbours(i) <= 1 && i / self.w > self.ground[i % self.w])
            .collect();
        // A nest with no proper tip (a shaft running straight into its room, say) may dig
        // from any of its tunnel cells; this has to be judged nest by nest.
        for nest in 0..self.nests.len() {
            if self.nests[nest].dead || tips.iter().any(|&i| self.nest_of[i] as usize == nest) {
                continue;
            }
            let floor = |w: &Self, i: usize| {
                w.cells[i].room() && i + w.w < n && w.cells[i + w.w] == Cell::Soil && !w.dead_tip[i]
            };
            tips.extend((0..n).filter(|&i| self.nest_of[i] as usize == nest && (usable(self, i) || floor(self, i))));
        }
        self.tip_dist = self.bfs(&tips);

        // Royal field: while digging, the dug cells next to the plan; once open, the
        // chamber itself (for the queen's walk).
        let mut royal: Vec<usize> = Vec::new();
        for nest in &self.nests {
            if let Some(r) = &nest.royal {
                if r.cells.is_empty() {
                    royal.extend(r.dug.iter().copied());
                } else {
                    royal.extend((0..n).filter(|&i| self.cells[i].dug() && r.cells.iter().any(|&c| self.adjacent(c, i))));
                }
            }
        }
        self.royal_dist = self.bfs(&royal);

        let queens: Vec<usize> = self.nests.iter().filter_map(|nst| nst.queen.as_ref().map(|q| self.idx(q.x, q.y))).collect();
        self.queen_dist = self.bfs(&queens);
    }

    /// Cells within `HUSK_GAP` of any decaying nest: off limits to new digging and founding
    /// until the husk has crumbled away.
    fn mark_husk_zone(&mut self) {
        self.husk_zone.fill(false);
        let decaying: Vec<bool> = self
            .nests
            .iter()
            .map(|nst| nst.dead || (nst.queen_lost && nst.queen.is_none()))
            .collect();
        for i in 0..self.cells.len() {
            let n = self.nest_of[i];
            let husk = self.cells[i].dug() && (n == NO_NEST || decaying[n as usize]);
            if !husk {
                continue;
            }
            let (x, y) = (i % self.w, i / self.w);
            for yy in y.saturating_sub(HUSK_GAP)..=(y + HUSK_GAP).min(self.h - 1) {
                for xx in x.saturating_sub(HUSK_GAP)..=(x + HUSK_GAP).min(self.w - 1) {
                    self.husk_zone[yy * self.w + xx] = true;
                }
            }
        }
    }

    /// Group dug cells into connected systems and match them to nests. A nest keeps its
    /// label across digs; two systems that join become one nest under the older label.
    fn label_nests(&mut self) {
        let n = self.cells.len();
        let mut comp = vec![NO_NEST; n];
        let mut comps: Vec<Vec<usize>> = Vec::new();
        for start in 0..n {
            if comp[start] != NO_NEST || !self.cells[start].dug() {
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
                    if comp[j] == NO_NEST && self.cells[j].dug() {
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
                    let other = &old[m];
                    base.workers += other.workers;
                    base.dead = base.dead && other.dead;
                    // One queen per nest: the older nest's stays, the other's is lost.
                    match (&base.queen, &other.queen) {
                        (None, Some(q)) if !base.queen_lost => base.queen = Some(q.clone()),
                        (Some(_), Some(q)) => {
                            self.corpses.push(Corpse {
                                x: q.x,
                                y: q.y,
                                queen: true,
                                ttl: QUEEN_CORPSE_DECAY,
                                max_ttl: QUEEN_CORPSE_DECAY,
                            });
                            self.queen_deaths += 1;
                        }
                        _ => {}
                    }
                    if base.royal.is_none() {
                        base.royal = other.royal.clone();
                    }
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
        // Ants and eggs always need a nest to point at. If every nest has crumbled away,
        // keep one dead placeholder until a founding queen starts things again.
        if nests.is_empty() && !(self.ants.is_empty() && self.eggs.is_empty()) {
            let mut husk = Nest::new('-', Vec::new());
            husk.dead = true;
            husk.queen_lost = true;
            nests.push(husk);
        }
        self.nests = nests;

        // Ants underground belong to the system they stand in; ants on the surface keep
        // their nest unless it has gone, then take the nearest live one. Eggs likewise.
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
        for e in &mut self.eggs {
            let here = self.nest_of[e.cell];
            if here != NO_NEST {
                e.nest = here as usize;
            }
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
        for n in 0..self.nests.len() {
            let nest = &self.nests[n];
            let eggs_left = self.eggs.iter().any(|e| e.nest == n);
            // A queen alone in an empty nest is not dead: she lays again once she can.
            if nest.workers == 0 && !nest.dead && !eggs_left && nest.queen.is_none() {
                let nest = &mut self.nests[n];
                nest.dead = true;
                nest.royal = None;
                if let Some(q) = nest.queen.take() {
                    self.corpses.push(Corpse { x: q.x, y: q.y, queen: true, ttl: QUEEN_CORPSE_DECAY, max_ttl: QUEEN_CORPSE_DECAY });
                    self.queen_deaths += 1;
                }
                self.dirty = true;
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
                if dist[j] == FAR && self.cells[j].dug() {
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
                    ants_lost: self.deaths - self.losses_at_last_drought.0,
                    queens_lost: self.queen_deaths - self.losses_at_last_drought.1,
                });
                self.losses_at_last_drought = (self.deaths, self.queen_deaths);
                self.rebuilding = Some((self.reports.len() - 1, self.drought_before.2));
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

    /// A big colony sends a few scouts off to open a nest somewhere clear.
    fn maybe_bud(&mut self) {
        if self.live_nests() >= self.cfg.max_nests || self.ants.len() < BUD_POPULATION {
            return;
        }
        let mut target = None;
        for _ in 0..30 {
            let x = 4 + self.rng.usize(..self.w.saturating_sub(8).max(1));
            // Keep well clear of living nests, and out of a decaying nest's ground entirely.
            let clear = self.entrances.iter().all(|&e| !self.is_live_entrance(e) || e.abs_diff(x) >= NEST_GAP)
                && !self.husk_zone[self.idx(x, self.ground[x])];
            if clear && self.food.iter().all(|f| f.x.abs_diff(x) > 4) {
                target = Some(x);
                break;
            }
        }
        let Some(target) = target else { return };
        let out = |a: &Ant| !a.underground && a.state == State::Foraging && !a.hungry();
        let mut party: Vec<usize> = self
            .ants
            .iter()
            .enumerate()
            .filter(|(_, a)| out(a) && a.job == Job::Scout)
            .map(|(i, _)| i)
            .take(BUD_PARTY)
            .collect();
        if party.len() < BUD_PARTY {
            let more: Vec<usize> = self
                .ants
                .iter()
                .enumerate()
                .filter(|(i, a)| out(a) && a.job == Job::Forager && !party.contains(i))
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
                let i = self.idx(x, y);
                let ch = if self.nests.iter().any(|n| n.queen.as_ref().is_some_and(|q| (q.x, q.y) == (x, y))) {
                    'Q'
                } else if let Some(a) = self.ants.iter().find(|a| a.x == x && a.y == y) {
                    match (a.state, a.job) {
                        (State::Starving, _) => 'x',
                        (State::Returning | State::Deliver, _) => 'A',
                        (State::Founding(_), _) => '*',
                        (_, Job::Forager) => 'f',
                        (_, Job::Nurse) => 'n',
                        (_, Job::Digger) => 'd',
                        (_, Job::Scout) => 's',
                    }
                } else if self.corpses.iter().any(|c| (c.x, c.y) == (x, y)) {
                    '+'
                } else if self.eggs.iter().any(|e| e.cell == i) {
                    'e'
                } else if y + 1 == self.ground[x] && self.food.iter().any(|f| f.x == x) {
                    'F'
                } else if self.planned_royal(i) {
                    'r'
                } else {
                    let husk = {
                        let n = self.nest_of[i];
                        n != NO_NEST && self.nests[n as usize].dead
                    };
                    match self.cell(x, y) {
                        Cell::Sky => ' ',
                        Cell::Soil if self.royal_mark[i] > 0 => '=',
                        Cell::Soil => '#',
                        _ if husk => 'x',
                        Cell::Tunnel => '.',
                        Cell::Royal => 'R',
                        Cell::Chamber => {
                            if self.store[i] > 0 {
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

    /// `nests 3 (1 queenless, 1 moving, 1 dead)` or just `nests 3`.
    pub fn nest_summary(&self) -> String {
        let mut notes = Vec::new();
        if self.queenless_nests() > 0 {
            notes.push(format!("{} queenless", self.queenless_nests()));
        }
        if self.royal_digs() > 0 {
            notes.push(format!("{} preparing a hall", self.royal_digs()));
        }
        if self.moving_queens() > 0 {
            notes.push(format!("{} moving", self.moving_queens()));
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

    /// One line per nest for `--headless`: label, state, jobs, larder.
    pub fn nest_lines(&self) -> Vec<String> {
        (0..self.nests.len())
            .map(|n| {
                let nest = &self.nests[n];
                let c = self.job_counts(n);
                let rooms = (0..self.cells.len()).filter(|&i| self.nest_of[i] as usize == n && self.cells[i].room()).count();
                let queen = match &nest.queen {
                    Some(q) if q.relocating => format!("Q moving fed {:.0}", q.fed),
                    Some(q) => format!("Q fed {:.0} moves {}", q.fed, q.relocations),
                    None if nest.queen_lost => "Q dead".to_string(),
                    None => "no Q yet".to_string(),
                };
                let royal = match &nest.royal {
                    Some(r) if r.cells.is_empty() => " · royal ready".to_string(),
                    Some(r) => format!(" · royal {} to dig", r.cells.len()),
                    None => String::new(),
                };
                format!(
                    "nest {} {} · w{} (f{} n{} d{} s{}) · cells {} rooms {} · food {} ({:.1} days, {:?}) · {}{}",
                    nest.label,
                    if nest.dead { "DEAD" } else { "" },
                    nest.workers,
                    c[0],
                    c[1],
                    c[2],
                    c[3],
                    self.nest_cells(n),
                    rooms,
                    self.nest_food(n),
                    nest.food_days,
                    nest.band,
                    queen,
                    royal
                )
            })
            .collect()
    }

    pub fn stats(&self) -> String {
        let season = self.season_label().map(|s| format!("{s} · ")).unwrap_or_default();
        format!(
            "tick {} · {season}{} · ants {} ({}) · eggs {} · dug {} · rooms {} · stored {} · food {} · delivered {} · deaths {}",
            self.tick,
            self.nest_summary(),
            self.ants.len(),
            self.job_summary(),
            self.eggs.len(),
            self.dug,
            self.chambers(),
            self.stored(),
            self.food.len(),
            self.delivered,
            self.deaths
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

    /// Dig a second, separate tunnel system with an entrance at column `x`.
    fn dig_system(w: &mut World, x: usize, depth: usize) {
        let g = w.ground[x];
        for d in 0..depth {
            w.set(x, g + d, Cell::Tunnel);
        }
        w.carve_chamber(x, g + depth, 2, 1);
        w.entrances.push(x);
        w.refresh_fields();
        w.tend_queens();
    }

    fn fill_nest(w: &mut World, n: usize) {
        for i in 0..w.store.len() {
            if w.nest_of[i] as usize == n && w.cells[i].room() {
                w.store[i] = STORE_CAP;
            }
        }
        w.dirty = true;
    }

    fn ant_at(x: usize, y: usize, job: Job, state: State, underground: bool) -> Ant {
        Ant {
            x,
            y,
            state,
            job,
            underground,
            home: 0,
            fed: FED_MAX,
            carrying: false,
            hungry_for: 0,
            idle: 0,
            dir: 1,
            cooldown: 0,
            dig_dir: (0, 1),
            dig_budget: 0,
            strength: 0.0,
        }
    }

    #[test]
    fn ants_dig_tunnels_and_rooms() {
        let mut w = world(1);
        let (start, rooms) = (w.dug, w.chambers());
        assert!(rooms > 5, "starter room missing:\n{}", w.snapshot());
        run(&mut w, 4000);
        assert!(w.dug > start + 30, "dug only {} cells:\n{}", w.dug - start, w.snapshot());
        assert!(w.chambers() > rooms, "no new rooms:\n{}", w.snapshot());
        assert!(w.dug * 100 < w.soil_cells() * 40, "farm is a maze:\n{}", w.snapshot());
    }

    #[test]
    fn ants_bring_food_home() {
        let mut w = world(2);
        run(&mut w, 6000);
        assert!(w.delivered > 30, "only {} deliveries:\n{}", w.delivered, w.snapshot());
    }

    #[test]
    fn the_queen_lays_when_fed_and_eggs_hatch_into_workers() {
        let mut w = World::new(Config { seed: 2, bud: false, drought: false, ants: Some(10), ..Config::default() }, 71, 48);
        assert!(w.nests[0].queen.is_some(), "no queen");
        fill_nest(&mut w, 0);
        w.food.clear();
        w.cfg.food_piles = 0;
        let before = w.ants.len();
        run(&mut w, LAY_INTERVAL + 2);
        assert_eq!(w.eggs.len(), 1, "no egg laid:\n{}", w.stats());
        run(&mut w, HATCH_TIME + 2);
        assert!(w.ants.len() > before, "egg did not hatch");
        assert!(w.eggs.len() <= 3, "{} eggs", w.eggs.len());
    }

    #[test]
    fn a_hungry_queen_skips_the_lay() {
        let mut w = World::new(Config { seed: 2, bud: false, drought: false, ants: Some(10), ..Config::default() }, 71, 48);
        w.food.clear();
        w.cfg.food_piles = 0;
        for i in 0..w.store.len() {
            w.store[i] = 0;
        }
        w.nests[0].queen.as_mut().unwrap().fed = LAY_THRESHOLD - 1.0;
        run(&mut w, LAY_INTERVAL + 2);
        assert!(w.eggs.is_empty(), "laid while hungry");
    }

    #[test]
    fn surface_ant_on_food_picks_it_up() {
        let mut w = world(3);
        w.food.clear();
        w.food.push(Food { x: 10, units: 5 });
        w.ants[0] = ant_at(10, w.ground[10] - 1, Job::Forager, State::Foraging, false);
        w.step_ant(0);
        assert_eq!(w.ants[0].state, State::Returning);
        assert!(w.ants[0].carrying);
        assert_eq!(w.food[0].units, 4);
    }

    #[test]
    fn a_hungry_forager_eats_what_it_finds() {
        let mut w = world(3);
        w.food.clear();
        w.food.push(Food { x: 10, units: 5 });
        w.ants[0] = ant_at(10, w.ground[10] - 1, Job::Forager, State::Foraging, false);
        w.ants[0].fed = HUNGRY - 5.0;
        w.step_ant(0);
        assert_eq!(w.ants[0].state, State::Foraging);
        assert!(!w.ants[0].carrying);
        assert!(w.ants[0].fed > HUNGRY);
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
            for nest in &w.nests {
                if let Some(q) = &nest.queen {
                    assert!(w.is_dug(q.x, q.y), "queen in soil");
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
    fn a_big_colony_opens_a_second_nest_with_its_own_queen() {
        let mut w = World::new(Config { seed: 5, ants: Some(70), ..Config::default() }, 120, 60);
        let mut when = None;
        for t in 0..12000 {
            w.tick();
            if w.nests.len() >= 2 {
                when = Some(t);
                break;
            }
        }
        assert!(when.is_some(), "no second nest:\n{}", w.snapshot());
        run(&mut w, 3000);
        assert!(w.nests[1].queen.is_some() || w.nests[1].dead, "second nest never got a queen:\n{}", w.snapshot());
    }

    #[test]
    fn no_bud_keeps_entrances_from_founding_parties() {
        let mut w = World::new(Config { seed: 5, ants: Some(70), bud: false, ..Config::default() }, 120, 60);
        run(&mut w, 3000);
        assert!(w.ants.iter().all(|a| !matches!(a.state, State::Founding(_))));
    }

    #[test]
    fn nests_are_labelled_and_keep_their_label_while_digging() {
        let mut w = world(1);
        assert_eq!(w.nests.len(), 1);
        assert_eq!(w.nests[0].label, 'A');
        run(&mut w, 2500);
        assert_eq!(w.nests[0].label, 'A');
        assert!(w.ants.iter().all(|a| a.home < w.nests.len()));
    }

    #[test]
    fn joined_systems_merge_under_the_older_label_keeping_one_queen() {
        let mut w = World::new(Config { seed: 2, bud: false, drought: false, ..Config::default() }, 90, 48);
        dig_system(&mut w, 70, 6);
        assert_eq!(w.nests.len(), 2);
        assert_eq!(w.nests[1].label, 'B');
        assert!(w.nests[1].queen.is_some());
        w.spawn_ant(1, None);
        w.spawn_ant(1, None);
        w.recount_workers();
        let a_workers = w.nests[0].workers;
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
        assert!(w.nests[0].queen.is_some());
        assert!(w.corpses.iter().any(|c| c.queen), "the second queen should be a corpse");
    }

    #[test]
    fn drought_stops_surface_food_and_drains_the_stores() {
        let mut w = World::new(Config { seed: 3, ants: Some(40), ..Config::default() }, 71, 48);
        fill_nest(&mut w, 0);
        let full = w.nest_food(0);
        w.food.clear();
        w.season = Season::Drought { until: w.tick + 50 * DAY, started: w.tick };
        run(&mut w, 500);
        assert!(w.food.is_empty(), "food spawned in a drought");
        assert!(w.nest_food(0) < full, "stores did not drain");
        assert!(w.nest_food(0) > 0, "the larder should still have something in it");
        assert_eq!(w.ants.len(), 40, "nobody should starve while there is food:\n{}", w.stats());
    }

    #[test]
    fn with_no_food_at_all_ants_starve_one_by_one_and_the_nest_dies() {
        let mut w = World::new(Config { seed: 3, ants: Some(6), bud: false, ..Config::default() }, 71, 48);
        w.food.clear();
        w.cfg.food_piles = 0;
        for i in 0..w.store.len() {
            w.store[i] = 0;
        }
        w.season = Season::Drought { until: w.tick + 500 * DAY, started: w.tick };
        let mut seen_corpse = false;
        let mut seen_partial = false;
        for _ in 0..8000 {
            w.tick();
            seen_corpse |= !w.corpses.is_empty();
            seen_partial |= !w.ants.is_empty() && w.deaths > 0;
            if w.ants.is_empty() && w.eggs.is_empty() && w.nests[0].queen.is_none() {
                run(&mut w, 2); // the nest is declared dead on the next count
                break;
            }
        }
        assert!(seen_corpse && seen_partial, "deaths should come one at a time");
        assert!(w.ants.is_empty(), "{}", w.stats());
        assert!(w.nests[0].dead && w.nests[0].queen.is_none());
        assert!(w.corpses.iter().any(|c| c.queen), "queen should leave a corpse");
        assert_eq!(w.live_entrances().count(), 0);
    }

    #[test]
    fn stores_drain_in_calm_weather_without_anyone_dying() {
        let mut w = World::new(Config { seed: 3, ants: Some(40), drought: false, ..Config::default() }, 71, 48);
        fill_nest(&mut w, 0);
        let full = w.nest_food(0);
        w.food.clear();
        w.cfg.food_piles = 0;
        run(&mut w, 2000);
        assert!(w.nest_food(0) < full, "stores did not drain");
        assert_eq!(w.deaths, 0, "somebody died with food in the store");
    }

    #[test]
    fn jobs_shift_toward_foraging_in_a_drought() {
        let mut w = World::new(Config { seed: 8, ants: Some(60), bud: false, ..Config::default() }, 120, 60);
        fill_nest(&mut w, 0);
        run(&mut w, 3000);
        let before = w.job_tally();
        w.season = Season::Drought { until: w.tick + 40 * DAY, started: w.tick };
        run(&mut w, 1500);
        let after = w.job_tally();
        assert!(after[0] > before[0], "foragers {:?} -> {:?}", before, after);
        assert_eq!(after[2], 0, "diggers should be stood down in a drought: {:?}", after);
        assert!(after[1] >= MIN_NURSES);
    }

    #[test]
    fn a_rich_nest_puts_more_ants_on_digging() {
        let mut w = World::new(Config { seed: 8, ants: Some(30), bud: false, drought: false, ..Config::default() }, 120, 60);
        run(&mut w, 2000);
        fill_nest(&mut w, 0);
        for _ in 0..2000 {
            w.tick();
            if w.tick.is_multiple_of(50) {
                fill_nest(&mut w, 0);
            }
        }
        assert_eq!(w.nests[0].band, Band::Plentiful, "days {}", w.nests[0].food_days);
        let c = w.job_tally();
        assert!(c[2] * 100 / w.ants.len() >= 20, "diggers {:?}", c);
    }

    #[test]
    fn a_scout_keeps_going_where_a_forager_dithers() {
        let mut w = World::new(Config { seed: 10, ants: Some(2), ..Config::default() }, 200, 48);
        w.food.clear();
        w.cfg.food_piles = 0;
        w.ants[0] = ant_at(20, w.ground[20] - 1, Job::Scout, State::Foraging, false);
        w.ants[1] = ant_at(20, w.ground[20] - 1, Job::Forager, State::Foraging, false);
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
    fn a_deep_rich_nest_digs_a_royal_chamber_and_the_queen_moves_in() {
        let mut w = World::new(Config { seed: 12, ants: Some(30), bud: false, drought: false, ..Config::default() }, 120, 60);
        // Give it a deep shaft so the depth gap is met, a full larder, and no cooldown.
        let ex = w.entrances[0];
        let g = w.ground[ex];
        for y in g..(g + 30) {
            w.set(ex, y, Cell::Tunnel);
        }
        // Rooms enough that a full larder counts as plentiful for thirty ants.
        w.carve_chamber(ex, g + 14, 3, 1);
        w.carve_chamber(ex, g + 21, 3, 1);
        w.refresh_fields();
        fill_nest(&mut w, 0);
        {
            let q = w.nests[0].queen.as_mut().unwrap();
            q.last_relocation = 0;
        }
        w.tick = RELOCATE_COOLDOWN + 1;
        let (qx, qy) = {
            let q = w.nests[0].queen.as_ref().unwrap();
            (q.x, q.y)
        };
        let mut planned = false;
        let mut moved = false;
        for _ in 0..40000 {
            w.tick();
            if w.tick.is_multiple_of(50) {
                fill_nest(&mut w, 0); // keep it plentiful
            }
            planned |= w.nests[0].royal.is_some();
            let q = w.nests[0].queen.as_ref().expect("queen died");
            if q.relocations >= 1 {
                moved = true;
                assert!(q.y >= qy + RELOCATE_GAP, "queen did not go deeper: {} -> {}", qy, q.y);
                assert!((q.x, q.y) != (qx, qy));
                assert_eq!(w.cell(q.x, q.y), Cell::Royal);
                break;
            }
        }
        assert!(planned, "no royal chamber was ever planned:\n{}", w.snapshot());
        assert!(moved, "queen never moved:\n{}", w.snapshot());
        assert!(w.cells.iter().filter(|&&c| c == Cell::Royal).count() >= 8);
    }

    #[test]
    fn nurses_fetch_from_the_nearest_larder_for_a_hungry_queen() {
        let mut w = World::new(Config { seed: 13, ants: Some(12), bud: false, drought: false, ..Config::default() }, 90, 48);
        w.food.clear();
        w.cfg.food_piles = 0;
        fill_nest(&mut w, 0);
        for a in &mut w.ants {
            a.job = Job::Nurse;
            a.state = State::Idle;
            a.idle = IDLE_TICKS;
        }
        let before = w.nests[0].queen.as_ref().unwrap().fed;
        w.nests[0].queen.as_mut().unwrap().fed = QUEEN_FED_MAX * 0.3;
        let mut carried = false;
        for _ in 0..1500 {
            w.tick();
            carried |= w.ants.iter().any(|a| a.state == State::Deliver && a.carrying);
            if w.nests[0].queen.as_ref().unwrap().fed > QUEEN_FED_MAX * 0.6 {
                break;
            }
        }
        assert!(carried, "no nurse ever carried food to the queen");
        assert!(w.nests[0].queen.as_ref().unwrap().fed > QUEEN_FED_MAX * 0.5, "queen still hungry (was {before})");
    }

    #[test]
    fn an_idle_digger_widens_a_busy_corridor_but_only_to_three() {
        let mut w = World::new(Config { seed: 14, ants: Some(1), bud: false, drought: false, ..Config::default() }, 71, 48);
        let ex = w.entrances[0];
        let g = w.ground[ex];
        // A long shaft, a digger idling in it, heavy traffic on one cell.
        for y in g..(g + 20) {
            w.set(ex, y, Cell::Tunnel);
        }
        w.refresh_fields();
        let y = g + 15; // well below the starter room
        let here = w.idx(ex, y);
        w.ants[0] = ant_at(ex, y, Job::Digger, State::Idle, true);
        w.ants[0].idle = 5000;
        w.traffic[here] = WIDEN_TRAFFIC + 50;
        let mut widened = 0;
        for _ in 0..2000 {
            w.tick();

            w.ants[0].x = ex;
            w.ants[0].y = y;
            w.ants[0].job = Job::Digger; // a nest of one would otherwise stand its digger down
            w.ants[0].state = State::Idle;
            w.ants[0].idle = 5000;
            w.traffic[here] = WIDEN_TRAFFIC + 50;
            widened = [(ex - 1, y), (ex + 1, y)].iter().filter(|&&(x, yy)| w.is_dug(x, yy)).count();
        }
        assert_eq!(widened, 2, "corridor should end up exactly three wide:\n{}", w.snapshot());
        // And never wider: the cells either side of the widened ones stay soil.
        assert!(!w.is_dug(ex - 2, y) && !w.is_dug(ex + 2, y), "{}", w.snapshot());
    }

    #[test]
    fn a_queenless_nest_crumbles_back_to_soil_from_its_tips() {
        let mut w = World::new(Config { seed: 17, ants: Some(0), bud: false, drought: false, grow: false, ..Config::default() }, 90, 48);
        run(&mut w, 1);
        let cells = w.nest_cells(0);
        assert!(cells > 10);
        w.nests[0].queen = None;
        w.nests[0].queen_lost = true;
        w.food.clear();
        w.cfg.food_piles = 0;
        for i in 0..w.store.len() {
            w.store[i] = 0;
        }
        run(&mut w, DECAY_TICKS * 6);
        let later = w.nest_cells(0);
        assert!(later < cells && later > 0, "{cells} -> {later}:\n{}", w.snapshot());
        let ex = w.entrances[0];
        assert!(w.is_dug(ex, w.ground[ex]), "entrance should go last");
        // Long enough and it is all soil again, entrance included; the next queen starts fresh.
        run(&mut w, DECAY_TICKS * (cells as u64 + 5));
        assert!(w.cells.iter().all(|c| !c.dug()), "{}", w.snapshot());
        assert!(w.entrances.is_empty());
    }

    #[test]
    fn orphans_stay_until_the_larder_is_empty() {
        let mut w = World::new(Config { seed: 15, ants: Some(8), bud: false, drought: false, ..Config::default() }, 71, 48);
        fill_nest(&mut w, 0);
        w.nests[0].queen = None;
        w.nests[0].queen_lost = true;
        assert!(!w.doomed(0), "still food in the rooms");
        for i in 0..w.store.len() {
            w.store[i] = 0;
        }
        assert!(w.doomed(0));
    }

    #[test]
    fn a_new_queen_arrives_when_the_farm_has_gone_quiet() {
        let mut w = World::new(Config { seed: 16, ants: Some(0), bud: false, drought: false, ..Config::default() }, 90, 48);
        w.nests[0].queen = None;
        w.nests[0].queen_lost = true;
        w.food.clear();
        w.cfg.food_piles = 0;
        run(&mut w, REFOUND_AFTER + 5); // the old nest has mostly crumbled by now
        assert!(w.nests.iter().any(|n| !n.dead && n.queen.is_some()), "nobody came:\n{}", w.snapshot());
        // She lays from her reserve with no workers and no larder.
        run(&mut w, LAY_INTERVAL + HATCH_TIME + 10);
        assert!(!w.ants.is_empty(), "the founding queen never hatched a worker");
    }

    #[test]
    fn even_a_small_nest_keeps_a_digger() {
        let mut w = World::new(Config { seed: 18, ants: Some(5), bud: false, drought: false, ..Config::default() }, 90, 48);
        fill_nest(&mut w, 0);
        for a in &mut w.ants {
            a.job = Job::Forager;
            a.state = State::Foraging;
        }
        for _ in 0..600 {
            w.tick();
            if w.tick.is_multiple_of(50) {
                fill_nest(&mut w, 0);
            }
        }
        let c = w.job_tally();
        assert!(c[2] >= 1, "no digger in a nest of {}: {:?}", w.ants.len(), c);
        assert!(c[1] >= 1, "no nurse: {:?}", c);
    }

    #[test]
    fn decay_shortens_corridors_from_the_end_before_touching_rooms() {
        let mut w = World::new(Config { seed: 19, ants: Some(0), bud: false, drought: false, grow: false, ..Config::default() }, 90, 48);
        let ex = w.entrances[0];
        let g = w.ground[ex];
        // A side corridor off the starter shaft with a dead end.
        for x in (ex + 1)..(ex + 8) {
            w.set(x, g + 3, Cell::Tunnel);
        }
        w.refresh_fields();
        w.nests[0].queen = None;
        w.nests[0].queen_lost = true;
        for i in 0..w.store.len() {
            w.store[i] = 0;
        }
        let rooms_before = w.chambers();
        let tip = w.idx(ex + 7, g + 3);
        let dug_before = w.cells.iter().filter(|c| c.dug()).count();
        run(&mut w, DECAY_TICKS);
        // One extremity goes: the corridor's dead end or a protruding room end, never a
        // cell from the middle of anything.
        let dug_after = w.cells.iter().filter(|c| c.dug()).count();
        assert_eq!(dug_after, dug_before - 1);
        assert!(!w.cells[tip].dug() || w.chambers() == rooms_before - 1, "a middle cell went first:\n{}", w.snapshot());
        for x in (ex + 1)..(ex + 7) {
            assert!(w.is_dug(x, g + 3) || !w.is_dug(x + 1, g + 3), "corridor split at {x}:\n{}", w.snapshot());
        }
        run(&mut w, DECAY_TICKS * 14);
        assert!(!w.is_dug(ex + 1, g + 3), "corridor should be eaten from the end back to the shaft:\n{}", w.snapshot());
    }

    #[test]
    fn a_nest_whose_room_swallowed_its_shaft_can_still_dig_out() {
        let mut w = World::new(Config { seed: 20, ants: Some(0), bud: false, drought: false, grow: false, ..Config::default() }, 90, 48);
        // Build the sealed shape by hand: a two-cell shaft straight into a room.
        let x = 70;
        let g = w.ground[x];
        w.set(x, g, Cell::Tunnel);
        w.set(x, g + 1, Cell::Tunnel);
        w.carve_chamber(x, g + 3, 2, 1);
        w.entrances.push(x);
        w.refresh_fields();
        w.tend_queens();
        let n = w.nests.iter().position(|nst| nst.entrances.contains(&x)).unwrap();
        let cells = w.nest_cells(n);
        for _ in 0..6 {
            w.spawn_ant(n, None);
            let a = w.ants.last_mut().unwrap();
            a.job = Job::Digger;
            a.state = State::ToTip;
        }
        w.recount_workers();
        fill_nest(&mut w, n);
        for _ in 0..3000 {
            w.tick();
            if w.tick.is_multiple_of(50) {
                fill_nest(&mut w, n);
            }
        }
        assert!(w.nest_cells(n) > cells + 10, "sealed nest never dug out: {} -> {}\n{}", cells, w.nest_cells(n), w.snapshot());
    }

    #[test]
    fn the_queens_chamber_is_outlined_and_the_outline_fades_with_her_line() {
        let mut w = World::new(Config { seed: 21, ants: Some(10), bud: false, drought: false, ..Config::default() }, 90, 48);
        let (qx, qy) = {
            let q = w.nests[0].queen.as_ref().unwrap();
            (q.x, q.y)
        };
        assert_eq!(w.cell(qx, qy), Cell::Royal, "her first room should be royal");
        let marked = w.royal_mark.iter().filter(|&&m| m > 0).count();
        assert!(marked >= 8, "no outline around the chamber:\n{}", w.snapshot());
        w.nests[0].queen = None;
        w.nests[0].queen_lost = true;
        run(&mut w, OUTLINE_FADE_EVERY * (255 / OUTLINE_FADE_STEP as u64 + 2));
        assert_eq!(w.royal_mark.iter().filter(|&&m| m > 0).count(), 0, "outline should have faded");
    }

    #[test]
    fn diggers_leave_husks_alone() {
        let mut w = World::new(Config { seed: 6, ants: Some(4), bud: false, drought: false, ..Config::default() }, 90, 48);
        w.food.clear();
        w.cfg.food_piles = 0;
        w.season = Season::Drought { until: w.tick + 500 * DAY, started: w.tick };
        for _ in 0..4000 {
            w.tick();
            if w.ants.is_empty() && w.nests.iter().all(|n| n.dead) {
                break;
            }
        }
        assert!(w.ants.is_empty() && w.nests.iter().all(|n| n.dead), "{}", w.stats());
        w.season = Season::Calm { until: u64::MAX };
        let x = 60; // beside the husk (the starter nest sits at column 45)
        dig_system(&mut w, x, 4);
        let n = w.nests.iter().position(|nst| nst.entrances.contains(&x)).unwrap();
        let label = w.nests[n].label;
        for _ in 0..12 {
            w.spawn_ant(n, None);
        }
        w.recount_workers();
        let husk: Vec<(usize, usize)> = (0..w.cells.len())
            .filter(|&i| w.cells[i].dug() && w.nest_of[i] as usize != n)
            .map(|i| (i % w.w, i / w.w))
            .collect();
        assert!(!husk.is_empty());
        let near_husk = |x: usize, y: usize| husk.iter().any(|&(hx, hy)| hx.abs_diff(x) <= HUSK_GAP && hy.abs_diff(y) <= HUSK_GAP);
        for _ in 0..60 {
            run(&mut w, 100);
            // Nest A's tunnels only ever crumble; nobody tunnels in and brings it back.
            assert!(!w.nests.iter().any(|nst| nst.label == 'A' && !nst.dead), "husk A came back to life");
            assert!(w.nests.iter().filter(|nst| !nst.dead).all(|nst| nst.label == label));
            // And while any of it stands, nobody digs near it.
            let standing = husk.iter().any(|&(hx, hy)| w.is_dug(hx, hy));
            if standing {
                for i in 0..w.cells.len() {
                    if w.cells[i].dug() && w.nest_of[i] as usize == w.nests.iter().position(|nst| nst.label == label).unwrap() {
                        let (x, y) = (i % w.w, i / w.w);
                        assert!(!near_husk(x, y) || x == 60 || x.abs_diff(60) <= 2 && y <= w.ground[60] + 6, "dug at {x},{y} within {HUSK_GAP} of the husk:\n{}", w.snapshot());
                    }
                }
            }
        }
    }

    #[test]
    fn widening_fills_gaps_before_starting_new_bumps() {
        let mut w = World::new(Config { seed: 22, ants: Some(1), bud: false, drought: false, ..Config::default() }, 71, 48);
        let ex = w.entrances[0];
        let g = w.ground[ex];
        for y in g..(g + 24) {
            w.set(ex, y, Cell::Tunnel);
        }
        for y in [g + 15, g + 17] {
            let c = w.idx(ex + 1, y);
            w.cells[c] = Cell::Tunnel;
            w.widened[c] = true;
        }
        w.refresh_fields();
        let y = g + 16;
        let here = w.idx(ex, y);
        w.ants[0] = ant_at(ex, y, Job::Digger, State::Idle, true);
        for _ in 0..400 {
            w.tick();
            w.ants[0].x = ex;
            w.ants[0].y = y;
            w.ants[0].job = Job::Digger;
            w.ants[0].state = State::Idle;
            w.ants[0].idle = 5000;
            w.traffic[here] = 0; // no traffic at all: only the gap rule may act
            if w.is_dug(ex + 1, y) {
                break;
            }
        }
        assert!(w.is_dug(ex + 1, y), "gap not filled:\n{}", w.snapshot());
        assert!(!w.is_dug(ex - 1, y), "started a fresh bump with no traffic");
    }

    #[test]
    fn tiny_and_odd_sizes_do_not_panic() {
        for (cw, ch) in [(1, 1), (8, 6), (9, 7), (71, 48), (200, 12), (30, 200)] {
            let mut w = World::new(Config::default(), cw, ch);
            run(&mut w, 400);
            let _ = w.snapshot();
        }
    }

    #[test]
    fn no_drought_never_leaves_calm() {
        let mut w = World::new(Config { seed: 4, drought: false, ..Config::default() }, 71, 48);
        run(&mut w, GRACE_PERIOD + 10 * DAY);
        assert!(matches!(w.season, Season::Calm { .. }));
        assert!(w.reports.is_empty());
    }

    #[test]
    fn seasons_cycle_and_report_the_losses() {
        let mut w = World::new(Config { seed: 5, ..Config::default() }, 71, 48);
        run(&mut w, GRACE_PERIOD + WARNING_LENGTH + MAX_DURATION + BLOOM_LENGTH + 1);
        assert_eq!(w.reports.len(), 1, "season is {:?}", w.season);
        let r = &w.reports[0];
        assert!(r.ended > r.started);
        assert!(r.ants_lost <= w.deaths && r.queens_lost <= w.queen_deaths);
    }
}



