//! Edits of a whole area at once (a crater): the shape is applied cube by cube on every core,
//! cubes it covers whole become one block, light is redone once per cube instead of once per
//! block, only blocks along the edge hear about it, and clients get whole cubes. Cubes that are
//! not loaded get it when they come (`DimSet::insert`), so an area far bigger than anything
//! loaded costs nothing until someone goes there.

use std::sync::Arc;

use glam::{DVec3, IVec2, IVec3};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use super::dim::Dim;
use crate::content::{AIR, Content};
use crate::shape::Shape;
use crate::world::cube::{Cube, VOLUME};
use crate::world::light::Lighter;

/// A shape of one block: every block whose middle is inside becomes `state`, unless matcher
/// `keep` takes it or `only` is given and doesn't.
#[derive(Clone, Debug)]
pub struct AreaFill {
    /// Order of the dimension's edits, from 1; cubes remember the last one they got.
    pub id: u32,
    pub shape: Shape,
    pub state: u32,
    pub keep: String,
    pub only: String,
    /// A bit per state that stays, from the two.
    kept: std::sync::Arc<[u64]>,
    /// `Content::covered`: a big block's other cells stay, the block goes whole after.
    covered: u32,
}

/// Light work for one cube of an area edit.
#[derive(Clone, Copy)]
enum LightJob {
    /// A cube lit on its own (or set to full sky) joins its neighbors (`Lighter::light_prelit`).
    Join(IVec3),
    /// A cube lit from scratch (`Lighter::light_new`), with whether the sky is open above it.
    New(IVec3, bool),
}

impl LightJob {
    fn pos(self) -> IVec3 {
        match self {
            LightJob::Join(p) | LightJob::New(p, _) => p,
        }
    }
}

/// What one light task sees: the 3x3 columns around its own between `low` and `top`, read
/// from the map as they are and copied on the first write (no other task of its round reaches
/// them). Boxed, so the lighter's cached pointers stay put as more are copied.
struct Overlay<'a> {
    base: &'a FxHashMap<IVec3, Arc<Cube>>,
    own: FxHashMap<IVec3, Box<Cube>>,
    key: IVec2,
    low: i32,
    top: i32,
}

impl Overlay<'_> {
    fn reaches(&self, p: IVec3) -> bool {
        (p.x - self.key.x).abs() <= 1 && (p.z - self.key.y).abs() <= 1 && p.y >= self.low && p.y <= self.top
    }
}

impl crate::world::light::CubeMap for Overlay<'_> {
    fn cube(&self, p: IVec3) -> Option<&Cube> {
        if !self.reaches(p) {
            return None;
        }
        self.own
            .get(&p)
            .map(|b| &**b)
            .or_else(|| self.base.get(&p).map(|a| &**a))
    }

    fn cube_mut(&mut self, p: IVec3) -> Option<&mut Cube> {
        if !self.reaches(p) {
            return None;
        }
        if !self.own.contains_key(&p) {
            let copy = Box::new((**self.base.get(&p)?).clone());
            self.own.insert(p, copy);
        }
        self.own.get_mut(&p).map(|b| &mut **b)
    }
}

/// What one fill did, for the log.
#[derive(Default)]
pub struct FillStats {
    pub cubes: usize,
    pub whole: usize,
    pub relit: usize,
    pub edges: usize,
    pub apply_ms: f64,
    pub light_ms: f64,
}

impl AreaFill {
    /// The fill, its matchers compiled (an empty one: none).
    pub fn new(c: &Content, shape: Shape, state: u32, keep: &str, only: &str) -> Result<AreaFill, String> {
        let (keep, only) = (keep.trim(), only.trim());
        let kept = c
            .matchers
            .kept(keep, only, &c.block_names, &c.blocks, &c.components)?;
        Ok(AreaFill { id: 0, shape, state, keep: keep.into(), only: only.into(), kept, covered: c.covered })
    }

    /// As saved in `world.txt`: `id block <shape>|<keep>|<only>`.
    pub fn spec(&self, c: &Content) -> String {
        format!("{} {} {}|{}|{}", self.id, c.state_name(self.state), self.shape.spec(), self.keep, self.only)
    }

    pub fn parse(c: &Content, spec: &str) -> Option<AreaFill> {
        let mut parts = spec.split('|');
        let w: Vec<&str> = parts.next()?.split_whitespace().collect();
        let (keep, only) = (parts.next()?, parts.next()?);
        let [id, block, shape @ ..] = &w[..] else { return None };
        let shape = Shape::parse(shape.iter().copied())?;
        let fill = AreaFill::new(c, shape, c.state_by_name(block)?, keep, only).ok()?;
        Some(AreaFill { id: id.parse().ok()?, ..fill })
    }

    /// Stays as it is.
    fn keeps(&self, s: u32) -> bool {
        crate::refs::has(&self.kept, s)
    }

    /// Blocks it keeps may be anywhere in terrain: cubes it covers are then made from what
    /// was there, not as one block.
    pub fn keeps_any(&self) -> bool {
        self.kept.iter().any(|&w| w != 0)
    }

    /// Block corners of cube `cp`, grown by `by` blocks.
    fn cube_box(cp: IVec3, by: f64) -> (DVec3, DVec3) {
        let lo = (cp * 16).as_dvec3();
        (lo - by, lo + 16.0 + by)
    }

    /// The cube may hold a block it changes, or one next to the edge (`by` 1); with a larger
    /// `by`, a cube that far from it.
    pub fn near(&self, cp: IVec3, by: f64) -> bool {
        let (lo, hi) = Self::cube_box(cp, by);
        self.shape.test_box(lo, hi).1
    }

    pub fn touches(&self, cp: IVec3) -> bool {
        self.near(cp, 1.0)
    }

    /// The middle of every block of the cube is inside.
    pub fn covers(&self, cp: IVec3) -> bool {
        let (lo, hi) = Self::cube_box(cp, -0.5);
        self.shape.test_box(lo, hi).0
    }

    fn inside(&self, p: IVec3) -> bool {
        self.shape.contains(p.as_dvec3() + DVec3::splat(0.5))
    }

    /// The cube with the fill applied, if that changes it, and the blocks outside next to a
    /// changed one (they get a neighbor update: sand on the rim falls).
    pub fn apply(&self, cp: IVec3, cube: &Cube) -> Option<(Cube, Vec<IVec3>)> {
        let keep = |s: u32| self.keeps(s) || s.wrapping_sub(self.covered) < 6;
        let mut out = if self.covers(cp) && !cube.palette().iter().any(|&s| keep(s)) {
            if cube.palette() == [self.state] {
                return None;
            }
            (Cube::uniform(self.state, 0), Vec::new())
        } else {
            let base = cp * 16;
            let mut states = [0u32; VOLUME];
            let mut changed = false;
            let mut edges = Vec::new();
            for (i, s) in states.iter_mut().enumerate() {
                let old = cube.get(i);
                *s = old;
                let p = base + crate::world::cube::offset(i);
                if self.inside(p) {
                    if old != self.state && !keep(old) {
                        *s = self.state;
                        changed = true;
                    }
                } else if old != self.state && crate::world::DIRS.iter().any(|&d| self.inside(p + d)) {
                    edges.push(p);
                }
            }
            if !changed {
                return (!edges.is_empty()).then(|| (cube.clone(), edges));
            }
            (Cube::from_states(&states), edges)
        };
        out.0.biomes = cube.biomes.clone();
        out.0.ticked = cube.ticked;
        out.0.edits = cube.edits;
        Some(out)
    }
}

impl Dim {
    /// Applies `fill` to every loaded cube of this region it reaches (see the module), and makes
    /// the `fresh` cubes (never loaded, filled whole) as they end up.
    pub fn area_fill(&mut self, c: &'static Content, fill: &AreaFill, fresh: &[IVec3]) -> FillStats {
        let mut stats = FillStats::default();
        let start = std::time::Instant::now();
        let targets: Vec<(IVec3, Arc<Cube>)> = self
            .space
            .cubes
            .iter()
            .filter(|(cp, _)| fill.touches(**cp))
            .map(|(cp, cube)| (*cp, cube.clone()))
            .collect();
        // With whether the cube had a light source.
        let done: Vec<(IVec3, Option<(Cube, Vec<IVec3>)>, bool)> = targets
            .par_iter()
            .map(|(cp, cube)| {
                (
                    *cp,
                    fill.apply(*cp, cube),
                    cube.palette()
                        .iter()
                        .any(|&s| c.states[s as usize].light != [0; 3]),
                )
            })
            .collect();
        let mut relight: Vec<IVec3> = Vec::new();
        // Cubes that lost a light source: those around them may get darker.
        let mut lost_light: Vec<IVec3> = Vec::new();
        for (cp, result, emitted) in done {
            let Some((mut cube, edges)) = result else {
                // Reached but unchanged: it still counts as having had this edit.
                if let Some(cube) = self.space.cubes.get_mut(&cp) {
                    Arc::make_mut(cube).edits = fill.id;
                }
                continue;
            };
            stats.edges += edges.len();
            self.space.edge_updates.extend(edges);
            cube.edits = fill.id;
            stats.cubes += 1;
            stats.whole += cube.uniform_state().is_some() as usize;
            // Block entities whose block went: gone with it (an explosion takes what they held).
            let gone: Vec<(IVec3, u64)> = self
                .entities
                .anchors
                .iter()
                .filter(|((frame, p), _)| {
                    let i = crate::world::local_index(*p);
                    *frame == 0
                        && crate::world::cube_of(*p) == cp
                        && (cube.get(i) != self.get(*p).unwrap_or(u32::MAX) || cube.extras(i) != self.space.extras(*p))
                })
                .flat_map(|((_, p), ids)| ids.iter().map(|id| (*p, *id)))
                .collect();
            for (_, id) in gone {
                self.entities.remove(id, super::dim::UNTOLD);
            }
            if emitted {
                lost_light.push(cp);
            }
            self.space.grid.cube_unloaded(cp);
            self.space.signals.cube_unloaded(c, cp);
            self.space.grid.cube_loaded(c, cp, &cube);
            self.space.signals.cube_loaded(c, cp, &cube);
            self.space.cubes.insert(cp, Arc::new(cube));
            self.space.edited.insert(cp);
            self.space.conn.remove(&cp);
            // Clients get the blocks now, whatever the light does.
            self.space.light_dirty.insert(cp);
            relight.push(cp);
        }
        self.fill_big_blocks(c, fill, &targets);
        for &cp in fresh {
            let mut cube = Cube::uniform(fill.state, 0);
            cube.edits = fill.id;
            self.space.grid.cube_loaded(c, cp, &cube);
            self.space.signals.cube_loaded(c, cp, &cube);
            self.add_cube(cp, Arc::new(cube));
            relight.push(cp);
            stats.cubes += 1;
            stats.whole += 1;
        }
        stats.apply_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = std::time::Instant::now();
        stats.relit = relight_after(c, self.id, fill.state, &mut self.space.cubes, &mut self.space.light_dirty, &relight, &lost_light);
        stats.light_ms = start.elapsed().as_secs_f64() * 1000.0;
        // Lit for good: they go out to players like any loaded cube.
        let mut fresh = fresh.to_vec();
        fresh.sort_unstable_by_key(|p| -p.y);
        for cp in fresh {
            self.settle_down(cp);
        }
        stats
    }
}

impl Dim {
    /// Big blocks go whole: one the fill reached another cell of goes, and so do the other cells
    /// of one it took. `before`: the cubes as they were.
    fn fill_big_blocks(&mut self, c: &'static Content, fill: &AreaFill, before: &[(IVec3, Arc<Cube>)]) {
        let big = |s: u32| !c.states[s as usize].cells.is_empty() || c.covered_step(s).is_some();
        let (mut reached, mut left) = (Vec::new(), Vec::new());
        for (cp, cube) in before
            .iter()
            .filter(|(_, cube)| cube.palette().iter().any(|&s| big(s)))
        {
            for i in 0..VOLUME {
                let (s, p) = (cube.get(i), cp * 16 + crate::world::cube::offset(i));
                if !fill.inside(p) {
                    continue;
                }
                if c.covered_step(s).is_some() {
                    reached.push(p);
                } else if self.get(p) != Some(s) {
                    left.extend(c.states[s as usize].cells.iter().map(|(o, _)| p + *o));
                }
            }
        }
        for p in reached {
            if let Some((at, _)) = self
                .block_at(c, p)
                .filter(|(at, s)| *at != p && *s != AIR && !fill.keeps(*s))
            {
                self.set(c, at, if fill.inside(at) { fill.state } else { AIR });
            }
        }
        for p in left {
            if self.get(p).is_some_and(|s| c.covered_step(s).is_some()) && self.block_at(c, p).is_some_and(|b| b.0 == p) {
                self.set(c, p, if fill.inside(p) { fill.state } else { AIR });
            }
        }
    }
}

/// Gives a cube coming from storage or generation the area `edits` made while it was away (or
/// before it was ever generated), once each. Whether that changed it.
pub fn catch_up(edits: &[AreaFill], pos: IVec3, cube: &mut Cube) -> bool {
    let Some(last) = edits.last().map(|e| e.id).filter(|&l| cube.edits < l) else { return false };
    let had = cube.edits;
    let mut edited = false;
    for e in edits.iter().filter(|e| e.id > had && e.touches(pos)) {
        if let Some((next, _)) = e.apply(pos, cube) {
            *cube = next;
            edited = true;
        }
    }
    cube.edits = last;
    edited
}

/// `fill` made on cubes held outside a region (a client's own world): applied on every core to
/// the ones it reaches and lit; every cube changed, in blocks or light, goes into `dirty`.
pub fn fill_cubes(c: &'static Content, dim: u32, fill: &AreaFill, cubes: &mut FxHashMap<IVec3, Arc<Cube>>, dirty: &mut FxHashSet<IVec3>) {
    let targets: Vec<(IVec3, Arc<Cube>)> = cubes
        .iter()
        .filter(|(cp, _)| fill.touches(**cp))
        .map(|(cp, cube)| (*cp, cube.clone()))
        .collect();
    let done: Vec<(IVec3, Option<Cube>, bool)> = targets
        .par_iter()
        .map(|(cp, cube)| {
            (
                *cp,
                fill.apply(*cp, cube).map(|r| r.0),
                cube.palette()
                    .iter()
                    .any(|&s| c.states[s as usize].light != [0; 3]),
            )
        })
        .collect();
    let (mut changed, mut lost) = (Vec::new(), Vec::new());
    for (cp, result, emitted) in done {
        match result {
            Some(mut cube) => {
                cube.edits = fill.id;
                cubes.insert(cp, Arc::new(cube));
                dirty.insert(cp);
                changed.push(cp);
                if emitted {
                    lost.push(cp);
                }
            }
            None => {
                if let Some(cube) = cubes.get_mut(&cp) {
                    Arc::make_mut(cube).edits = fill.id;
                }
            }
        }
    }
    relight_after(c, dim, fill.state, cubes, dirty, &changed, &lost);
}

/// Light after `state` was filled into the `changed` cubes (`lost_light`: those of them that
/// had a light source), every cube whose light changed going into `dirty`. Returns how many
/// cubes were relit.
pub fn relight_after(
    c: &'static Content,
    dim: u32,
    state: u32,
    cubes: &mut FxHashMap<IVec3, Arc<Cube>>,
    dirty: &mut FxHashSet<IVec3>,
    changed: &[IVec3],
    lost_light: &[IVec3],
) -> usize {
    let st = &c.states[state as usize];
    if st.opacity == 0 && st.light == [0; 3] && st.liquid.is_none() {
        relight_opened(c, dim, cubes, dirty, changed, lost_light)
    } else {
        relight_around(c, dim, cubes, dirty, changed)
    }
}

/// Light for cubes that only lost blocks (a crater): light around them can only grow, so
/// only they are lit again and push it out, except around lost light sources. Cubes open to
/// full sky above need no flood fill: whole air ones are set to full sky, partial ones are
/// lit alone in parallel (as generation does) and joined to their neighbors.
fn relight_opened(
    c: &'static Content,
    dim: u32,
    cubes: &mut FxHashMap<IVec3, Arc<Cube>>,
    dirty: &mut FxHashSet<IVec3>,
    changed: &[IVec3],
    lost_light: &[IVec3],
) -> usize {
    let def = &c.dims[dim as usize];
    let full = crate::world::light::SKY_FULL as u32;
    let mut order = changed.to_vec();
    order.sort_unstable_by_key(|p| (-p.y, p.x, p.z));
    let open_above = |cp: IVec3| crate::world::light::open_above(def, cp.y);
    let sky_above = |cubes: &rustc_hash::FxHashMap<IVec3, Arc<Cube>>, cp: IVec3| {
        crate::world::light::sky_below(cubes.get(&(cp + IVec3::Y)).map(|a| &**a), open_above(cp)) == crate::world::light::SkyBelow::Full
    };
    let mut whole = Vec::new();
    let mut alone = Vec::new();
    let mut flood = Vec::new();
    for &cp in &order {
        let air = cubes[&cp].uniform_state().is_some();
        if !def.sky_light || !sky_above(cubes, cp) {
            flood.push(cp);
        } else if air {
            Arc::make_mut(cubes.get_mut(&cp).unwrap()).fill_light(full);
            whole.push(cp);
        } else {
            alone.push(cp);
        }
    }
    let lit: Vec<(IVec3, Cube)> = alone
        .par_iter()
        .map(|&cp| {
            let mut cube = (*cubes[&cp]).clone();
            crate::world::light::prelight(c, &mut cube, true);
            (cp, cube)
        })
        .collect();
    for (cp, cube) in lit {
        cubes.insert(cp, Arc::new(cube));
    }
    // Around lost light sources, the cubes left as they were may be too bright now.
    let mut dimmed: Vec<IVec3> = lost_light
        .iter()
        .flat_map(|&cp| (-1..=1).flat_map(move |y| (-1..=1).flat_map(move |z| (-1..=1).map(move |x| cp + IVec3::new(x, y, z)))))
        .filter(|p| cubes.contains_key(p) && !changed.contains(p))
        .collect();
    dimmed.sort_unstable_by_key(|p| (-p.y, p.x, p.z));
    dimmed.dedup();
    let relit = changed.len() + dimmed.len();
    // Whole air cubes among others like them have nothing to push or pull.
    let full: FxHashSet<IVec3> = whole.iter().copied().collect();
    let mut jobs: Vec<LightJob> = alone.iter().map(|&cp| LightJob::Join(cp)).collect();
    jobs.extend(
        flood
            .iter()
            .chain(&dimmed)
            .map(|&cp| LightJob::New(cp, open_above(cp))),
    );
    jobs.extend(
        whole
            .iter()
            .filter(|&&cp| {
                crate::world::DIRS
                    .iter()
                    .any(|d| !full.contains(&(cp + *d)))
            })
            .map(|&cp| LightJob::Join(cp)),
    );
    run_light_jobs(c, dim, cubes, dirty, jobs);
    relit
}

/// Runs light jobs on every core. Light leaves a cube by at most 15 blocks (less than a
/// cube) except full sky going straight down, so jobs are grouped by column and columns
/// three apart are done at once, each seeing only the 3x3 columns around it (`Overlay`);
/// nine rounds cover every column. Within a column the jobs keep their order (top down).
fn run_light_jobs(
    c: &'static Content,
    dim: u32,
    cubes: &mut FxHashMap<IVec3, Arc<Cube>>,
    dirty: &mut FxHashSet<IVec3>,
    jobs: Vec<LightJob>,
) {
    let def = &c.dims[dim as usize];
    let mut columns: FxHashMap<IVec2, Vec<LightJob>> = FxHashMap::default();
    for job in jobs {
        let p = job.pos();
        columns.entry(IVec2::new(p.x, p.z)).or_default().push(job);
    }
    let opaque = |cube: &Cube| {
        cube.palette()
            .iter()
            .all(|&s| c.states[s as usize].opacity >= 15)
    };
    for round in 0..9 {
        let keys: Vec<IVec2> = columns
            .keys()
            .copied()
            .filter(|k| k.x.rem_euclid(3) + 3 * k.y.rem_euclid(3) == round)
            .collect();
        let mut work: Vec<(IVec2, Vec<LightJob>, i32, i32)> = Vec::with_capacity(keys.len());
        for key in keys {
            let jobs = columns.remove(&key).unwrap();
            let top = jobs.iter().map(|j| j.pos().y).max().unwrap() + 1;
            // Full sky goes on down whatever shaft is below, and sideways from there.
            let mut low = jobs.iter().map(|j| j.pos().y).min().unwrap();
            while cubes
                .get(&IVec3::new(key.x, low - 1, key.y))
                .is_some_and(|cube| !opaque(cube))
            {
                low -= 1;
            }
            work.push((key, jobs, low - 1, top));
        }
        let base = &*cubes;
        let done: Vec<(FxHashMap<IVec3, Box<Cube>>, FxHashSet<IVec3>)> = work
            .into_par_iter()
            .map(|(key, jobs, low, top)| {
                let mut map = Overlay { base, own: FxHashMap::default(), key, low, top };
                let mut lighter = Lighter::new(&mut map, c, def.sky_light);
                for job in jobs {
                    match job {
                        LightJob::Join(cp) => lighter.light_prelit(cp),
                        LightJob::New(cp, open) => lighter.light_new(cp, open),
                    }
                }
                let changed = std::mem::take(&mut lighter.changed);
                (map.own, changed)
            })
            .collect();
        for (own, changed) in done {
            for (p, cube) in own {
                cubes.insert(p, Arc::from(cube));
            }
            dirty.extend(changed);
        }
    }
}

/// The light of cubes `set` thrown away and made again from nothing, top down, on every core: what
/// went wrong in it (a cube lit before its neighbors came) gone. Every cube whose light changed
/// goes into `dirty`.
pub fn relight_anew(c: &'static Content, dim: u32, cubes: &mut FxHashMap<IVec3, Arc<Cube>>, dirty: &mut FxHashSet<IVec3>, set: &[IVec3]) {
    let mut order: Vec<IVec3> = set
        .iter()
        .copied()
        .filter(|p| cubes.contains_key(p))
        .collect();
    order.sort_unstable_by_key(|p| (-p.y, p.x, p.z));
    for p in &order {
        Arc::make_mut(cubes.get_mut(p).unwrap()).fill_light(0);
    }
    let def = &c.dims[dim as usize];
    let jobs = order
        .iter()
        .map(|&cp| LightJob::New(cp, crate::world::light::open_above(def, cp.y)))
        .collect();
    run_light_jobs(c, dim, cubes, dirty, jobs);
}

/// Light once per cube, the changed ones and those around them (light reaches 15 blocks:
/// less than a cube), from the top down so sky light comes from cubes already done.
fn relight_around(
    c: &'static Content,
    dim: u32,
    cubes: &mut FxHashMap<IVec3, Arc<Cube>>,
    dirty: &mut FxHashSet<IVec3>,
    relight: &[IVec3],
) -> usize {
    let mut around: Vec<IVec3> = relight
        .iter()
        .flat_map(|&cp| (-1..=1).flat_map(move |y| (-1..=1).flat_map(move |z| (-1..=1).map(move |x| cp + IVec3::new(x, y, z)))))
        .filter(|p| cubes.contains_key(p))
        .collect();
    around.sort_unstable_by_key(|p| (-p.y, p.x, p.z));
    around.dedup();
    let relit = around.len();
    let def = &c.dims[dim as usize];
    let jobs = around
        .iter()
        .map(|&cp| LightJob::New(cp, crate::world::light::open_above(def, cp.y)))
        .collect();
    run_light_jobs(c, dim, cubes, dirty, jobs);
    relit
}
