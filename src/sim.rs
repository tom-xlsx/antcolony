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
    pub seed: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ants: None,
            food_piles: 3,
            grow: true,
            max_ants: 150,
            bud: true,
            max_nests: 4,
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
}

#[derive(Clone, Debug)]
pub struct Ant {
    pub x: usize,
    pub y: usize,
    pub state: State,
    /// On the surface or in a tunnel.
    pub underground: bool,
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
    pub tick: u64,
    pub delivered: u64,
    pub dug: usize,
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
const DIG_SHARE: usize = 22;
const VISION: usize = 8;
const STORE_CAP: u8 = 6;
const GROW_COST: u32 = 4;
const DEPOSIT: f32 = 0.08;
const STRENGTH_DECAY: f32 = 0.985;
const EVAPORATION: f32 = 0.994;
const FOOD_RESPAWN_TICKS: u32 = 200;
const BUD_POPULATION: usize = 45;
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
            tick: 0,
            delivered: 0,
            dug: 0,
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

        let n_ants = cfg.ants.unwrap_or_else(|| ((w * h) / 110).clamp(12, 120));
        for _ in 0..n_ants {
            world.spawn_ant();
        }
        for _ in 0..cfg.food_piles {
            world.spawn_food();
        }
        world.refresh_fields();
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

    fn dig_capped(&self) -> bool {
        self.dug * 100 > self.soil_cells() * DIG_SHARE
    }

    pub fn stored(&self) -> u32 {
        self.store.iter().map(|&s| s as u32).sum()
    }

    pub fn chambers(&self) -> usize {
        self.cells.iter().filter(|&&c| c == Cell::Chamber).count()
    }

    /// New ant appears in a random dug cell, ready to head out.
    fn spawn_ant(&mut self) {
        let dug: Vec<usize> = (0..self.cells.len())
            .filter(|&i| matches!(self.cells[i], Cell::Tunnel | Cell::Chamber))
            .collect();
        let i = dug[self.rng.usize(..dug.len())];
        self.ants.push(Ant {
            x: i % self.w,
            y: i / self.w,
            state: State::Foraging,
            underground: true,
            dir: if self.rng.bool() { 1 } else { -1 },
            cooldown: self.rng.u8(..MOVE_TICKS * 4),
            dig_dir: (0, 1),
            dig_budget: 0,
            strength: 0.0,
        });
    }

    fn spawn_food(&mut self) {
        let units = self.rng.u32(30..=80) + self.ants.len() as u32 / 2;
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
        if self.food.len() < self.cfg.food_piles {
            if self.food_respawn == 0 {
                self.spawn_food();
                self.food_respawn = FOOD_RESPAWN_TICKS;
            } else {
                self.food_respawn -= 1;
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
                // Food in sight wins, then the trail, then a wander.
                let seen = self
                    .food
                    .iter()
                    .filter(|f| f.units > 0)
                    .map(|f| f.x)
                    .filter(|&fx| fx.abs_diff(x) <= VISION)
                    .min_by_key(|&fx| fx.abs_diff(x));
                if let Some(fx) = seen {
                    if fx == x {
                        if let Some(f) = self.food.iter_mut().find(|f| f.x == fx) {
                            f.units = f.units.saturating_sub(1);
                        }
                        ant.state = State::Returning;
                        ant.strength = 1.0;
                        ant.dir = self.toward_entrance(x);
                        return ant;
                    }
                    ant.dir = if fx > x { 1 } else { -1 };
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
                if self.entrances.contains(&x) {
                    ant.underground = true;
                    ant.y = self.ground[x];
                    return ant;
                }
                ant.dir = self.toward_entrance(x);
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
                    ant.state = State::Digging;
                    ant.dig_dir = (0, 1);
                    ant.dig_budget = 10;
                    return ant;
                }
                ant.dir = if target > x { 1 } else { -1 };
                self.walk(&mut ant);
            }
            State::ToTip | State::Digging => {
                // Shouldn't be on the surface in these states; go back to foraging.
                ant.state = State::Foraging;
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

    fn toward_entrance(&self, x: usize) -> i32 {
        let nearest = self.entrances.iter().min_by_key(|&&e| e.abs_diff(x)).copied().unwrap_or(x);
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
                    ant.state = self.pick_task();
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
                    ant.state = State::Foraging;
                } else if let Some((nx, ny)) = self.descend(Field::Tip, ant.x, ant.y) {
                    ant.x = nx;
                    ant.y = ny;
                }
            }
            State::Digging => {
                if ant.dig_budget == 0 || self.dig_capped() {
                    ant.state = self.pick_task();
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
                    ant.state = self.pick_task();
                }
            }
        }
        ant
    }

    /// What an ant does after finishing a job underground.
    fn pick_task(&mut self) -> State {
        let free: u32 = self
            .cells
            .iter()
            .zip(&self.store)
            .filter(|(c, s)| **c == Cell::Chamber && **s < STORE_CAP)
            .map(|(_, s)| (STORE_CAP - s) as u32)
            .sum();
        if self.dig_capped() {
            return State::Foraging;
        }
        let crowded = self.ants.len() * 2 > self.dug;
        let mut p_dig = 0.2;
        if free < 8 {
            p_dig += 0.4;
        }
        if crowded {
            p_dig += 0.2;
        }
        if self.rng.f32() < p_dig {
            State::ToTip
        } else {
            State::Foraging
        }
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
                if self.is_dug(ax, ay) {
                    return false;
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
        let exits: Vec<usize> = self
            .entrances
            .iter()
            .map(|&x| self.idx(x, self.ground[x]))
            .filter(|&i| matches!(self.cells[i], Cell::Tunnel | Cell::Chamber))
            .collect();
        self.exit_dist = self.bfs(&exits);

        let stores: Vec<usize> = (0..self.cells.len())
            .filter(|&i| self.cells[i] == Cell::Chamber && self.store[i] < STORE_CAP)
            .collect();
        self.store_dist = self.bfs(&stores);

        let mut tips: Vec<usize> = (0..self.cells.len())
            .filter(|&i| self.cells[i] == Cell::Tunnel && !self.dead_tip[i] && self.dug_neighbours(i) <= 1)
            .filter(|&i| i / self.w > self.ground[i % self.w])
            .collect();
        if tips.is_empty() {
            tips = (0..self.cells.len())
                .filter(|&i| self.cells[i] == Cell::Tunnel && !self.dead_tip[i])
                .collect();
        }
        self.tip_dist = self.bfs(&tips);
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

    /// Stored food feeds new ants, as long as there is room in the tunnels.
    fn maybe_grow(&mut self) {
        if self.ants.len() >= self.cfg.max_ants || self.ants.len() * 2 >= self.dug + 4 {
            return;
        }
        if !self.tick.is_multiple_of(30) {
            return;
        }
        let mut need = GROW_COST;
        let full: Vec<usize> = (0..self.store.len()).filter(|&i| self.store[i] > 0).collect();
        let have: u32 = full.iter().map(|&i| self.store[i] as u32).sum();
        if have < need {
            return;
        }
        for i in full {
            let take = (self.store[i] as u32).min(need);
            self.store[i] -= take as u8;
            need -= take;
            if need == 0 {
                break;
            }
        }
        self.dirty = true;
        self.spawn_ant();
    }

    /// A big colony sends a few surface foragers off to open a nest somewhere clear.
    fn maybe_bud(&mut self) {
        if self.entrances.len() >= self.cfg.max_nests || self.ants.len() < BUD_POPULATION {
            return;
        }
        let mut target = None;
        for _ in 0..30 {
            let x = 4 + self.rng.usize(..self.w.saturating_sub(8).max(1));
            if self.entrances.iter().all(|&e| e.abs_diff(x) >= NEST_GAP)
                && self.food.iter().all(|f| f.x.abs_diff(x) > 4)
            {
                target = Some(x);
                break;
            }
        }
        let Some(target) = target else { return };
        let party: Vec<usize> = self
            .ants
            .iter()
            .enumerate()
            .filter(|(_, a)| !a.underground && a.state == State::Foraging)
            .map(|(i, _)| i)
            .take(BUD_PARTY)
            .collect();
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
                    match a.state {
                        State::Returning => 'A',
                        State::Digging => 'd',
                        State::Founding(_) => '*',
                        _ => 'a',
                    }
                } else if y + 1 == self.ground[x] && self.food.iter().any(|f| f.x == x) {
                    'F'
                } else {
                    match self.cell(x, y) {
                        Cell::Sky => ' ',
                        Cell::Soil => '#',
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

    pub fn stats(&self) -> String {
        format!(
            "tick {} · nests {} · ants {} · dug {} · chambers {} · stored {} · food {} · delivered {}",
            self.tick,
            self.entrances.len(),
            self.ants.len(),
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
        assert!(w.dug * 100 < w.soil_cells() * 35, "farm is a maze:\n{}", w.snapshot());
    }

    #[test]
    fn ants_bring_food_home_and_the_colony_grows() {
        let mut w = world(2);
        let start = w.ants.len();
        run(&mut w, 5000);
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
            underground: false,
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

    #[test]
    fn tiny_and_odd_sizes_do_not_panic() {
        for (cw, ch) in [(1, 1), (8, 6), (9, 7), (71, 48), (200, 12), (30, 200)] {
            let mut w = World::new(Config::default(), cw, ch);
            run(&mut w, 400);
            let _ = w.snapshot();
        }
    }
}
