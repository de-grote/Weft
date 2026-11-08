//! A dimension split into independently simulated regions, like Folia. A region is a connected
//! set of loaded sections (4x4x4 cubes, 26-neighborhood). Players far apart end up in different
//! regions that tick in parallel; when they approach each other the regions merge, when they part
//! the region splits. Anything built from loaded blocks (multiblocks, cables) lies in adjacent
//! sections and therefore always inside one region, so cross-region access never happens.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use glam::{DVec3, IVec3};
use rustc_hash::{FxHashMap, FxHashSet};
use weft_abi as abi;

use super::dim::{Dim, section_of, section_of_cube};
use crate::content::Content;
use crate::host;
use crate::world::{Cube, cube_of};

pub struct DimSet {
    pub id: u32,
    pub regions: Vec<Dim>,
    pub generating: FxHashSet<IVec3>,
    pub time: u64,
    pub day_offset: i64,
    /// `/time` moved the sky: players hear at once, not at the next second.
    pub day_moved: bool,
    /// The weather now (`registry::WeatherDef`).
    pub weather: u16,
    /// When the weather changes by itself.
    pub weather_until: u64,
    /// Changed: players hear at once.
    pub weather_moved: bool,
    seed: u64,
    next_id: Arc<AtomicU64>,
    owner: FxHashMap<IVec3, usize>,
    pub dirty: bool,
    /// Area edits made in this dimension, in order (`server::area`); saved with the world.
    /// Shared with the workers that make cubes, which apply them (`dim::prepare`).
    pub edits: Arc<Vec<super::area::AreaFill>>,
}

impl DimSet {
    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn new(id: u32, seed: u64, next_id: Arc<AtomicU64>) -> DimSet {
        DimSet {
            id,
            regions: Vec::new(),
            generating: FxHashSet::default(),
            time: 1000,
            day_offset: 0,
            day_moved: false,
            weather: 0,
            weather_until: 0,
            weather_moved: false,
            seed,
            next_id,
            owner: FxHashMap::default(),
            dirty: false,
            edits: Arc::default(),
        }
    }

    fn new_region(&mut self, section: IVec3) -> usize {
        let mut d = Dim::new(self.id, self.seed, self.next_id.clone());
        d.set_rng(weft_noise::hash(self.seed ^ self.time, section.x as i64, section.y as i64, section.z as i64));
        d.time = self.time;
        d.day_offset = self.day_offset;
        d.weather = self.weather;
        self.regions.push(d);
        self.regions.len() - 1
    }

    /// Region owning `section`, or the neighboring one, or a new region. Touching several regions
    /// merges them on the spot: data that is about to connect must already share a region, so
    /// lighting and anything else reading neighbors never meets a border. Splits can wait.
    fn region_for(&mut self, c: &Content, section: IVec3) -> usize {
        if let Some(&r) = self.owner.get(&section) {
            return r;
        }
        let mut near: Vec<usize> = Vec::new();
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(&r) = self.owner.get(&(section + IVec3::new(dx, dy, dz))) {
                        near.push(r);
                    }
                }
            }
        }
        near.sort_unstable();
        near.dedup();
        let r = match near.len() {
            0 => self.new_region(section),
            1 => near[0],
            _ => self.merge(c, &near),
        };
        self.owner.insert(section, r);
        r
    }

    /// Folds regions into the largest of them; returns its index afterwards.
    fn merge(&mut self, c: &Content, list: &[usize]) -> usize {
        let target = *list
            .iter()
            .max_by_key(|&&i| self.regions[i].space.cubes.len())
            .unwrap();
        for &i in list.iter().rev() {
            if i != target {
                let parcel = self.regions[i].take_sections(|_| true);
                self.regions[target].absorb(c, parcel);
                self.regions.remove(i);
            }
        }
        log::debug!("dimension {}: {} regions merged", self.id, list.len());
        self.rebuild_owner();
        target - list.iter().filter(|&&i| i < target).count()
    }

    pub fn region_index(&self, pos: IVec3) -> Option<usize> {
        self.owner.get(&section_of_cube(cube_of(pos))).copied()
    }

    pub fn region_at(&mut self, pos: IVec3) -> Option<&mut Dim> {
        let r = self.region_index(pos)?;
        self.regions.get_mut(r)
    }

    /// Region for a position, created if needed (teleports, spawning players).
    pub fn ensure_region(&mut self, c: &Content, pos: DVec3) -> &mut Dim {
        let r = self.region_for(c, section_of(pos));
        &mut self.regions[r]
    }

    pub fn entity_region(&mut self, id: u64) -> Option<&mut Dim> {
        self.regions
            .iter_mut()
            .find(|r| r.entities.get(id).is_some())
    }

    pub fn entity_region_ref(&self, id: u64) -> Option<&Dim> {
        self.regions.iter().find(|r| r.entities.get(id).is_some())
    }

    pub fn entity(&self, id: u64) -> Option<&super::entity::Entity> {
        self.entity_region_ref(id).and_then(|r| r.entities.get(id))
    }

    pub fn has(&self, pos: IVec3) -> bool {
        self.owner
            .get(&section_of_cube(pos))
            .is_some_and(|&r| self.regions[r].has(pos))
    }

    /// Cubes settled since the last call.
    pub fn take_settled(&mut self) -> Vec<IVec3> {
        self.regions
            .iter_mut()
            .flat_map(|r| r.newly_settled.drain(..))
            .collect()
    }

    /// Cubes with block changes since the last call.
    pub fn take_edited(&mut self) -> Vec<IVec3> {
        let mut out: Vec<IVec3> = self
            .regions
            .iter_mut()
            .flat_map(|r| r.space.edited.drain())
            .collect();
        out.sort_unstable_by_key(|p| (p.x, p.y, p.z));
        out
    }

    /// Changes whenever a cube was loaded or became settled, or regions were reshaped.
    pub fn settled_version(&self) -> u64 {
        let settled: u64 = self.regions.iter().map(|r| r.settled_gen).sum();
        let cubes: u64 = self
            .regions
            .iter()
            .map(|r| r.space.cubes.len() as u64)
            .sum();
        settled ^ cubes << 24 ^ (self.regions.len() as u64) << 52
    }

    pub fn settled(&self, pos: IVec3) -> bool {
        self.owner
            .get(&section_of_cube(pos))
            .is_some_and(|&r| self.regions[r].settled.contains(&pos))
    }

    pub fn snapshot(&self, pos: IVec3) -> Option<Arc<Cube>> {
        let r = *self.owner.get(&section_of_cube(pos))?;
        self.regions[r].snapshot(pos)
    }

    /// Loaded cubes of nothing but air right under `pos`, at most `max`.
    pub fn air_below(&self, pos: IVec3, max: i32) -> i32 {
        (1..=max)
            .take_while(|&d| {
                self.snapshot(pos - IVec3::Y * d)
                    .is_some_and(|c| c.palette() == [crate::content::AIR])
            })
            .count() as i32
    }

    pub fn get(&self, p: IVec3) -> Option<u32> {
        let r = *self.owner.get(&section_of_cube(cube_of(p)))?;
        self.regions[r].get(p)
    }

    pub fn insert(&mut self, c: &'static Content, pos: IVec3, mut cube: Cube, entities: &[u8], mut conn: u16, mut prelit: bool) {
        self.generating.remove(&pos);
        // Loaded meanwhile (an area edit loads what it opens up at once).
        if self.has(pos) {
            return;
        }
        // Edits made while it was being made.
        let edited = self.catch_up_edits(pos, &mut cube);
        if edited {
            prelit = false;
            conn = super::dim::see_through(c, &cube);
        }
        let r = self.region_for(c, section_of_cube(pos));
        let d = &mut self.regions[r];
        d.insert(c, pos, cube, prelit);
        d.space.conn.insert(pos, conn);
        if edited {
            d.space.modified.insert(pos);
        }
        // What the save held may need writing again once it is gone from here.
        let ticks = super::persist::read_ticks(entities);
        d.restore_ticks(pos, &ticks);
        if !ticks.is_empty() || entities.len() > 12 {
            d.on_disk.insert(pos);
        }
        let mut bodies = Vec::new();
        for (e, inv, comps, blocks) in super::persist::read(c, entities) {
            let id = e.id;
            if d.entities.get(id).is_some() {
                continue;
            }
            if c.game.is_block_entity(e.kind)
                && !d
                    .in_frame(e.block_frame(), |d| d.block_entity_fits(c, &e))
                    .unwrap_or(false)
            {
                // Its block is gone (or it never had one): what it held stays in the world.
                for s in inv.into_iter().flatten() {
                    d.spawn_item(c, e.pos, s);
                }
                continue;
            }
            if let Some(blocks) = blocks {
                d.bodies
                    .insert(id, Box::new(super::space::Space::with_blocks(c, &blocks)));
                bodies.push(id);
            }
            d.entities.insert_with(c, e, inv, comps);
        }
        for id in bodies {
            d.add_missing_on_body(c, id);
        }
        d.add_missing_block_entities(c, pos);
    }

    /// Gives a cube coming from storage or generation the area edits made while it was away
    /// (or before it was ever generated), once each. Whether that changed it.
    pub fn catch_up_edits(&self, pos: IVec3, cube: &mut Cube) -> bool {
        super::area::catch_up(&self.edits, pos, cube)
    }

    /// Loaded cubes changed since they were generated or loaded (saved when they unload).
    pub fn modified(&self) -> impl Iterator<Item = IVec3> + '_ {
        self.regions
            .iter()
            .flat_map(|r| r.space.modified.iter().copied())
    }

    pub fn connectivity(&mut self, c: &Content, pos: IVec3) -> Option<u16> {
        let r = *self.owner.get(&section_of_cube(pos))?;
        self.regions[r].connectivity(c, pos)
    }

    pub fn settle_down(&mut self, pos: IVec3) {
        if let Some(&r) = self.owner.get(&section_of_cube(pos)) {
            self.regions[r].settle_down(pos);
        }
    }

    pub fn spawn_point(&self, c: &Content) -> DVec3 {
        let def = &c.dims[self.id as usize];
        let mut pos = def.spawn_at;
        if let Some((by, cb)) = def.spawn {
            let mut args = abi::SpawnArgs { seed: self.seed, pos: pos.into() };
            host::invoke(by, cb, &mut args);
            pos = args.pos.to_array();
        } else if let Some(p) = search_spawn(c, def, self.seed) {
            pos = p;
        }
        DVec3::new(pos[0] as f64 + 0.5, pos[1] as f64 + 1.0, pos[2] as f64 + 0.5)
    }

    /// Makes area edit `fill` (its id given here): every region's loaded cubes now, the rest
    /// when they load. `fresh` cubes are not loaded and filled whole: made directly.
    pub fn area_fill(&mut self, c: &'static Content, mut fill: super::area::AreaFill, fresh: &[IVec3]) {
        let start = std::time::Instant::now();
        fill.id = self.edits.last().map_or(1, |e| e.id + 1);
        Arc::make_mut(&mut self.edits).push(fill.clone());
        // Regions first (making one may merge others), then who gets what.
        for &cp in fresh {
            self.region_for(c, section_of_cube(cp));
        }
        let mut made: Vec<Vec<IVec3>> = vec![Vec::new(); self.regions.len()];
        for &cp in fresh {
            made[self.owner[&section_of_cube(cp)]].push(cp);
        }
        let mut total = super::area::FillStats::default();
        for (r, made) in self.regions.iter_mut().zip(&made) {
            let s = r.area_fill(c, &fill, made);
            total.cubes += s.cubes;
            total.whole += s.whole;
            total.relit += s.relit;
            total.edges += s.edges;
            total.apply_ms += s.apply_ms;
            total.light_ms += s.light_ms;
        }
        log::info!(
            "area fill {}: {} cubes changed ({} whole), {:.0} ms applying, {} relit in {:.0} ms, {} edge updates queued, {:.0} ms in all",
            fill.id,
            total.cubes,
            total.whole,
            total.apply_ms,
            total.relit,
            total.light_ms,
            total.edges,
            start.elapsed().as_secs_f64() * 1000.0
        );
    }

    pub fn set_time(&mut self, t: u64) {
        self.time = t;
        for r in &mut self.regions {
            r.time = t;
        }
    }

    /// `/time`: moves the sky only. `time` keeps counting, since everything waiting on a tick
    /// (flowing water, timers, growth, effects) is timed by it.
    pub fn set_day_time(&mut self, t: u64) {
        self.day_offset = t as i64 - self.time as i64;
        self.day_moved = true;
        for r in &mut self.regions {
            r.day_offset = self.day_offset;
        }
    }

    /// The time of day's own clock.
    pub fn set_weather(&mut self, weather: u16, ticks: u64) {
        self.weather = weather;
        self.weather_until = self.time + ticks;
        self.weather_moved = true;
        for r in &mut self.regions {
            r.weather = self.weather;
        }
    }

    /// After clear weather one of the others by their weights, after any other clear again, each
    /// for as long as its `duration` says.
    pub fn tick_weather(&mut self, c: &Content) {
        let tick = abi::WeatherTick { dim: self.id, weather: self.weather as u32 };
        if let Some(r) = self.regions.first_mut() {
            r.emit_engine(c, abi::event::WEATHER_TICK, &tick);
        }
        if self.time < self.weather_until {
            return;
        }
        let all = &c.game.weathers;
        let r = weft_noise::hash(self.seed ^ 0x5745_4154, self.time as i64, 0, 0);
        let first = self.weather_until == 0;
        let here = &c.dims[self.id as usize].weathers;
        let total: u64 = here.iter().map(|&w| all[w as usize].weight as u64).sum();
        let next = if first || self.weather != 0 || total == 0 {
            0
        } else {
            let mut pick = (r >> 32) % total;
            here.iter()
                .copied()
                .find(|&w| {
                    let weight = all[w as usize].weight as u64;
                    let hit = pick < weight;
                    pick = pick.saturating_sub(weight);
                    hit
                })
                .unwrap_or(0) as usize
        };
        let [lo, hi] = all[next].duration;
        self.set_weather(next as u16, lo + r % (hi - lo + 1));
    }

    pub fn day_time(&self) -> u64 {
        self.time.saturating_add_signed(self.day_offset)
    }

    /// Recomputes regions from the sections in use: merges touching regions, splits separated
    /// parts. Data moves in parcels; each region keeps the part it holds most of.
    pub fn regionize(&mut self, c: &Content) {
        self.dirty = false;
        self.regions
            .retain(|r| !r.space.cubes.is_empty() || !r.entities.list.is_empty());
        let per_region: Vec<FxHashSet<IVec3>> = self.regions.iter().map(|r| r.sections()).collect();
        let all: Vec<IVec3> = per_region
            .iter()
            .flatten()
            .copied()
            .collect::<FxHashSet<_>>()
            .into_iter()
            .collect();
        let index: FxHashMap<IVec3, usize> = all.iter().enumerate().map(|(i, &s)| (s, i)).collect();
        let mut parent: Vec<usize> = (0..all.len()).collect();
        fn find(p: &mut [usize], mut x: usize) -> usize {
            while p[x] != x {
                p[x] = p[p[x]];
                x = p[x];
            }
            x
        }
        for (i, &s) in all.iter().enumerate() {
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if let Some(&j) = index.get(&(s + IVec3::new(dx, dy, dz))) {
                            let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                            if a != b {
                                parent[a.max(b)] = a.min(b);
                            }
                        }
                    }
                }
            }
        }
        // Canonical component ids: the smallest section in each, so the result does not depend on
        // hash order.
        let mut root_min: FxHashMap<usize, IVec3> = FxHashMap::default();
        for (i, &s) in all.iter().enumerate() {
            let r = find(&mut parent, i);
            let e = root_min.entry(r).or_insert(s);
            if (s.x, s.y, s.z) < (e.x, e.y, e.z) {
                *e = s;
            }
        }
        let comp_of = |s: IVec3, parent: &mut Vec<usize>| root_min[&find(parent, index[&s])];

        // Primary component of each region: where most of its sections are.
        let mut primary: Vec<Option<IVec3>> = Vec::new();
        for secs in &per_region {
            let mut counts: FxHashMap<IVec3, usize> = FxHashMap::default();
            for &s in secs {
                *counts.entry(comp_of(s, &mut parent)).or_default() += 1;
            }
            primary.push(
                counts
                    .into_iter()
                    .max_by(|a, b| {
                        a.1.cmp(&b.1)
                            .then((b.0.x, b.0.y, b.0.z).cmp(&(a.0.x, a.0.y, a.0.z)))
                    })
                    .map(|x| x.0),
            );
        }
        let sec_comp: FxHashMap<IVec3, IVec3> = all.iter().map(|&s| (s, comp_of(s, &mut parent))).collect();
        let changed = per_region
            .iter()
            .enumerate()
            .any(|(i, secs)| secs.iter().any(|s| Some(sec_comp[s]) != primary[i]))
            || {
                let mut seen = FxHashSet::default();
                primary.iter().flatten().any(|p| !seen.insert(*p))
            };
        if !changed {
            self.owner = per_region
                .iter()
                .enumerate()
                .flat_map(|(i, secs)| secs.iter().map(move |&s| (s, i)))
                .collect();
            return;
        }

        // Split: every region gives away what is not in its primary component.
        let mut parcels: Vec<(IVec3, super::dim::Parcel)> = Vec::new();
        for (i, r) in self.regions.iter_mut().enumerate() {
            let mine = primary[i];
            let foreign: FxHashSet<IVec3> = per_region[i]
                .iter()
                .filter(|s| Some(sec_comp[*s]) != mine)
                .map(|s| sec_comp[s])
                .collect();
            for comp in foreign {
                let sc = &sec_comp;
                parcels.push((comp, r.take_sections(|s| sc.get(&s) == Some(&comp))));
            }
        }
        // Merge: regions sharing a primary component fold into the first one.
        let mut by_comp: FxHashMap<IVec3, usize> = FxHashMap::default();
        let mut order: Vec<usize> = (0..self.regions.len()).collect();
        order.sort_by_key(|&i| primary[i].map(|p| (p.x, p.y, p.z)));
        let mut removed = vec![false; self.regions.len()];
        for i in order {
            let Some(p) = primary[i] else {
                removed[i] = true;
                continue;
            };
            match by_comp.get(&p) {
                Some(&target) => {
                    let all_parcel = self.regions[i].take_sections(|_| true);
                    self.regions[target].absorb(c, all_parcel);
                    removed[i] = true;
                    log::debug!("dimension {}: regions merged", self.id);
                }
                None => {
                    by_comp.insert(p, i);
                }
            }
        }
        for (comp, parcel) in parcels {
            let target = match by_comp.get(&comp) {
                Some(&t) => t,
                None => {
                    let t = self.new_region(comp);
                    removed.push(false);
                    by_comp.insert(comp, t);
                    log::debug!("dimension {}: region split", self.id);
                    t
                }
            };
            self.regions[target].absorb(c, parcel);
        }
        let mut i = 0;
        self.regions.retain(|_| {
            let keep = !removed[i];
            i += 1;
            keep
        });
        self.rebuild_owner();
    }

    fn rebuild_owner(&mut self) {
        self.owner.clear();
        for (i, r) in self.regions.iter().enumerate() {
            for s in r.sections() {
                self.owner.insert(s, i);
            }
        }
    }
}

/// Open ground near 0, 0 on the dimension's columns: biomes tagged `spawn_biome` first, then
/// any land above the sea, never where caves took the ground or in a scattered thing (trees).
fn search_spawn(c: &Content, def: &crate::content::DimensionDef, seed: u64) -> Option<[i32; 3]> {
    def.column(seed, 0, 0)?;
    let sea = def.sea_level().unwrap_or(i32::MIN + 2);
    let preferred = c.components.id(abi::comp::SPAWN_BIOME);
    let good = |x: i32, z: i32, prefer: bool| {
        let col = def.column(seed, x, z)?;
        let biome_ok = !prefer || preferred.is_some_and(|t| c.components.has(crate::components::BIOME, col.biome, t));
        (col.height != i32::MIN
            && col.height > sea + 1
            && biome_ok
            && !def.carved(seed, x, z)
            && !super::dim::scattered_near(def, c, seed, x, z))
        .then_some([x, col.height, z])
    };
    for prefer in [true, false] {
        for r in 0..200 {
            for k in 0..8 {
                let a = k as f64 / 8.0 * std::f64::consts::TAU;
                let (x, z) = ((r as f64 * 16.0 * a.cos()) as i32, (r as f64 * 16.0 * a.sin()) as i32);
                if let Some(p) = good(x, z, prefer) {
                    return Some(p);
                }
            }
        }
    }
    let col = def.column(seed, 0, 0)?;
    Some([0, col.height.max(def.sea_level().unwrap_or(col.height)), 0])
}
