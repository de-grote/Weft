//! Features: everything generation puts into the world after the terrain
//! (`abi::comp::feature`). Each is where × how its starts spread × its shape × how it writes,
//! and one loop places them all in order, on the server and on clients alike. Ores, trees,
//! structures, villages and grass are presets of it.
//!
//! Plans read deterministic snapshots of completed earlier stages, plus their own writes,
//! so every cube and every client agrees. A start on a grid is worked out once (`DimensionDef::starts`) and
//! each cube it reaches takes its part.

use std::sync::Arc;

use glam::{DVec3, IVec3};
use weft_abi as abi;
use weft_noise::hash;

use crate::content::{AIR, Content, DimensionDef};
use crate::template::{GROUND, Transform, Variant};
use crate::world::local_index;
use weft_abi::ModId;

/// A feature in a dimension, frozen.
#[derive(Clone)]
pub struct FeatureDef {
    pub name: String,
    /// Index in `Content::features`.
    pub id: u32,
    pub owner: ModId,
    pub stage: u32,
    /// From the name, so adding one moves none of the others.
    pub salt: u64,
    pub biome: Option<Box<[u64]>>,
    pub y: [i32; 2],
    pub fit: Fit,
    pub spread: Spread,
    pub shape: Shape,
    pub write: Write,
    /// How far from its start it may put blocks: sideways, up, down.
    pub reach: [i32; 3],
    pub priority: i32,
    pub separation: i32,
    /// The ways its template may be placed (`Transform::allowed`).
    pub transforms: Vec<Transform>,
    /// Palette ids, the seed picks one per start; none: the template's defaults.
    pub palettes: Vec<u32>,
}

/// Where a feature on a grid may stand.
#[derive(Clone, Default)]
pub struct Fit {
    pub ground: Option<u32>,
    pub over_water: bool,
    pub over_caves: bool,
    pub max_slope: Option<i32>,
    pub buried: i32,
}

#[derive(Clone)]
pub enum Spread {
    /// Starts in every cube over its heights.
    PerCube { count: u32, curve: u32, plateau: i32 },
    /// One start per square of `spacing`.
    Grid { spacing: i32, chance: Chance },
    /// A callback on every cube (`GenArgs`).
    EveryCube(ModId, abi::Callback),
}

/// A grid start's chance: fixed, or what a biome component says.
#[derive(Clone, Copy)]
pub enum Chance {
    Fixed(f32),
    Density(Option<u32>),
}

#[derive(Clone)]
pub enum Shape {
    Vein {
        kind: u32,
        size: u32,
        radius: f32,
        thickness: f32,
    },
    /// Cells about the start, worked out at freeze.
    Steps(Arc<[IVec3]>),
    Template(u32),
    Plan(ModId, abi::Callback),
    /// Its callback does it all (`Spread::EveryCube`).
    Cube,
}

/// How veins and steps write, and what pieces stand on.
#[derive(Clone, Default)]
pub struct Write {
    pub block: u32,
    pub replaces: Option<u32>,
    /// (matcher of what it meets, state put there instead).
    pub variants: Vec<(u32, u32)>,
    pub exposed: f32,
    /// 0 none, `GROUND` the column's own top block, else that state.
    pub foundation: u32,
    pub clear_above: bool,
}

impl Write {
    /// The state it puts where `state` was, if it replaces that.
    pub fn state_in(&self, m: &crate::refs::Matchers, state: u32) -> Option<u32> {
        self.variants
            .iter()
            .find(|v| m.has(v.0, state))
            .map(|v| v.1)
            .or_else(|| match self.replaces {
                Some(r) => m.has(r, state).then_some(self.block),
                None => Some(self.block),
            })
    }
}

impl FeatureDef {
    /// How far from its start a vein may put blocks.
    pub fn vein_reach(&self) -> i32 {
        match self.shape {
            Shape::Vein { kind, size, radius, .. } => match kind {
                abi::vein_shape::SPHERE | abi::vein_shape::DISK => radius.ceil() as i32,
                abi::vein_shape::BLOB => (size as f32).cbrt().ceil() as i32 + 2,
                _ => size as i32,
            }
            .clamp(0, 16),
            _ => self.reach[0],
        }
    }

    /// How likely a start at `y` is to be kept (0..1), as its height curve says.
    fn weight(&self, y: i32) -> f64 {
        let Spread::PerCube { curve, plateau, .. } = self.spread else { return 1.0 };
        let (lo, hi) = (self.y[0] as f64, self.y[1] as f64);
        let half = (hi - lo) / 2.0;
        if half <= 0.0 || curve == abi::height_curve::UNIFORM {
            return 1.0;
        }
        let off = (y as f64 - (lo + half)).abs();
        let flat = if curve == abi::height_curve::TRAPEZOID { (plateau as f64 / 2.0).min(half) } else { 0.0 };
        if half - flat <= 0.0 { 1.0 } else { ((half - off) / (half - flat)).clamp(0.0, 1.0) }
    }

    /// Starts to try per cube so that the average kept is `count`.
    fn tries(&self) -> u32 {
        let Spread::PerCube { count, curve, plateau } = self.spread else { return 0 };
        let span = self.y[1] as f64 - self.y[0] as f64;
        let avg = match curve {
            abi::height_curve::TRIANGLE => 0.5,
            abi::height_curve::TRAPEZOID if span > 0.0 => (1.0 + (plateau as f64 / span).min(1.0)) / 2.0,
            _ => 1.0,
        };
        (count as f64 / avg).round() as u32
    }

    pub fn grid(&self) -> Option<i32> {
        match self.spread {
            Spread::Grid { spacing, .. } => Some(spacing),
            _ => None,
        }
    }
}

/// Something a start puts, in world coordinates.
pub enum Put {
    Block {
        at: IVec3,
        state: u32,
        force: bool,
    },
    /// A box of `state`, only where `only` (states, a bit each) takes what is there when set.
    Fill {
        lo: IVec3,
        hi: IVec3,
        state: u32,
        only: Option<Arc<[u64]>>,
    },
    /// Cells of a vein or steps, written as the feature writes.
    Cells(Vec<IVec3>),
    Piece(Piece),
}

/// A template placed by a start.
pub struct Piece {
    pub at: IVec3,
    pub variant: Arc<Variant>,
    pub foundation: u32,
    pub clear_above: bool,
    /// Per column of its box (x fastest): the ground's height and top block.
    pub ground: Arc<[(i32, u32)]>,
}

impl Piece {
    fn hi(&self) -> IVec3 {
        self.at + self.variant.size - IVec3::ONE
    }
    fn column(&self, x: i32, z: i32) -> (i32, u32) {
        self.ground
            .get((z * self.variant.size.x + x) as usize)
            .copied()
            .unwrap_or((i32::MIN, 0))
    }
}

/// Something else a start brings: an entity, or what a block entity holds.
#[derive(Clone)]
pub struct Extra {
    pub at: DVec3,
    /// A block entity's block: `data` goes into it, `loot` fills it.
    pub anchor: Option<IVec3>,
    /// `u32::MAX`: from `data`.
    pub kind: u32,
    pub data: Option<Arc<[u8]>>,
    pub loot: Option<Arc<str>>,
}

/// What a generated cube brings beyond its blocks, for the server to put in once it joins the
/// world.
#[derive(Clone, Default)]
pub struct Generated {
    pub extras: Vec<Extra>,
    /// Starts whose cube this is: (feature, start, transform).
    pub placed: Vec<(u32, IVec3, u8)>,
}

/// One start of a feature: all it puts, the box around that, and how its template was placed.
pub struct Start {
    pub feature: usize,
    pub pos: IVec3,
    pub transform: Transform,
    pub lo: IVec3,
    pub hi: IVec3,
    pub puts: Vec<Put>,
    pub extras: Vec<Extra>,
}

impl Start {
    fn new(feature: usize, pos: IVec3, transform: Transform) -> Start {
        Start { feature, pos, transform, lo: IVec3::MAX, hi: IVec3::MIN, puts: Vec::new(), extras: Vec::new() }
    }

    /// One of no feature, for a template placed by hand.
    pub fn loose(pos: IVec3, transform: Transform) -> Start {
        Start::new(usize::MAX, pos, transform)
    }

    fn grow(&mut self, lo: IVec3, hi: IVec3) {
        self.lo = self.lo.min(lo);
        self.hi = self.hi.max(hi);
    }

    pub fn put(&mut self, p: Put) {
        let (lo, hi) = match &p {
            Put::Block { at, .. } => (*at, *at),
            Put::Fill { lo, hi, .. } => (*lo, *hi),
            Put::Cells(v) => v
                .iter()
                .fold((IVec3::MAX, IVec3::MIN), |(a, b), p| (a.min(*p), b.max(*p))),
            Put::Piece(piece) => (piece.at, piece.hi()),
        };
        if lo.cmple(hi).all() {
            self.grow(lo, hi);
        }
        self.puts.push(p);
    }

    fn touches(&self, lo: IVec3, hi: IVec3) -> bool {
        self.lo.cmple(hi).all() && self.hi.cmpge(lo).all()
    }
}

/// Where a feature on a grid starts in a cell, if it does: the start (above the ground, sunk by
/// its burying), the biome, its random bits and the transform its template takes. From the seed
/// and the columns alone; distant terrain repeats the test for trees exactly
/// (`terrain_gpu::describe_lod`).
pub fn grid_site(
    c: &Content,
    def: &DimensionDef,
    dim: u32,
    f: &FeatureDef,
    seed: u64,
    cell: (i32, i32),
) -> Option<(IVec3, u32, u64, Transform)> {
    let Spread::Grid { spacing, chance } = f.spread else { return None };
    let h = hash(seed ^ f.salt, cell.0 as i64, 0, cell.1 as i64);
    let random = hash(h, 1, 2, 3);
    let transform = f.transforms[(random >> 40) as usize % f.transforms.len()];
    let foot = match f.shape {
        Shape::Template(t) => transform.size(c.templates[t as usize].size),
        _ => IVec3::ZERO,
    };
    let room = (spacing - foot.x.max(foot.z)).max(1);
    let (x, z) = (cell.0 * spacing + (h % room as u64) as i32, cell.1 * spacing + ((h >> 8) % room as u64) as i32);
    let mid = (x + foot.x / 2, z + foot.z / 2);
    let col = def.column(seed, mid.0, mid.1)?;
    let fit = &f.fit;
    if col.height == i32::MIN
        || (!fit.over_water && def.sea_level().is_some_and(|l| col.height < l))
        || !(f.y[0]..=f.y[1]).contains(&(col.height + 1))
        || f.biome
            .as_ref()
            .is_some_and(|b| !crate::refs::has(b, col.biome))
    {
        return None;
    }
    let chance = match chance {
        Chance::Fixed(p) => p,
        Chance::Density(d) => d
            .and_then(|d| {
                c.components
                    .get::<f32>(crate::components::BIOME, col.biome, d)
            })
            .unwrap_or(0.0),
    };
    if weft_noise::unit(h.rotate_left(17)) >= chance as f64 {
        return None;
    }
    let surface = (fit.ground.is_some() || !fit.over_caves)
        .then(|| crate::generation::Snapshot::new(c, dim, seed, f.stage).surface_block(mid.0, mid.1));
    if let Some(ground) = fit.ground {
        let state = surface.unwrap_or(0);
        if state == 0 || !c.matchers.has(ground, state) {
            return None;
        }
    }
    if !fit.over_caves && surface == Some(0) {
        return None;
    }
    if let Some(most) = fit.max_slope {
        let (x1, z1) = (x + (foot.x - 1).max(0), z + (foot.z - 1).max(0));
        let heights = [(x, z), (x1, z), (x, z1), (x1, z1)].map(|(x, z)| def.column(seed, x, z).map_or(col.height, |c| c.height));
        let (lo, hi) = heights
            .iter()
            .fold((col.height, col.height), |(a, b), &v| (a.min(v), b.max(v)));
        if hi - lo > most {
            return None;
        }
    }
    Some((IVec3::new(x, col.height + 1 - fit.buried, z), col.biome, random, transform))
}

/// Start `fi` of the dimension's features in grid cell `cell`, if it stands there: what it puts,
/// worked out once. One of a higher priority that would come within its separation keeps it
/// out.
pub fn start_at(c: &Content, def: &DimensionDef, dim: u32, seed: u64, fi: usize, cell: (i32, i32)) -> Option<Arc<Start>> {
    let cache = def.starts.get(fi)?;
    let made = cache.get(seed, cell.0, cell.1, || {
        let f = &def.features[fi];
        let (pos, biome, random, t) = grid_site(c, def, dim, f, seed, cell)?;
        let start = make_start(c, def, dim, seed, fi, pos, biome, random, t)?;
        let room = IVec3::new(f.separation, 0, f.separation);
        for (gi, g) in def.features.iter().enumerate() {
            let Some(spacing) = g
                .grid()
                .filter(|_| g.priority > f.priority && g.stage <= f.stage)
            else {
                continue;
            };
            let (lo, hi) = (start.lo - room - IVec3::splat(g.reach[0]), start.hi + room + IVec3::splat(g.reach[0]));
            for cz in lo.z.div_euclid(spacing)..=hi.z.div_euclid(spacing) {
                for cx in lo.x.div_euclid(spacing)..=hi.x.div_euclid(spacing) {
                    if start_at(c, def, dim, seed, gi, (cx, cz)).is_some_and(|o| o.touches(start.lo - room, start.hi + room)) {
                        return None;
                    }
                }
            }
        }
        Some(Arc::new(start))
    });
    (*made).clone()
}

/// What feature `fi` puts from a start at `pos`.
#[allow(clippy::too_many_arguments)]
pub fn make_start(
    c: &Content,
    def: &DimensionDef,
    dim: u32,
    seed: u64,
    fi: usize,
    pos: IVec3,
    biome: u32,
    random: u64,
    t: Transform,
) -> Option<Start> {
    let f = &def.features[fi];
    let mut start = Start::new(fi, pos, t);
    match &f.shape {
        Shape::Template(id) => {
            let palette = (!f.palettes.is_empty()).then(|| f.palettes[(random >> 48) as usize % f.palettes.len()]);
            let variant = c.templates[*id as usize].variant(c, t, palette);
            let view =
                crate::generation::ReadCtx { snapshot: crate::generation::Snapshot::new(c, dim, seed, f.stage), local: None, plan: None };
            let piece = make_piece(&view, pos, variant, f.write.foundation, f.write.clear_above);
            add_piece(&mut start, piece);
        }
        Shape::Plan(owner, cb) => {
            let mut ctx = PlanCtx { c, def, dim, seed, fi, random, start, warned: false };
            let snapshot = crate::generation::Snapshot::new(c, dim, seed, f.stage);
            let read = crate::generation::ReadCtx { snapshot, local: None, plan: Some(std::ptr::addr_of!(ctx.start)) };
            let mut args = abi::PlanArgs {
                source: snapshot.source(&read),
                seed,
                dim,
                pos: pos.to_array().into(),
                biome,
                random,
                ctx: std::ptr::addr_of_mut!(ctx).cast(),
            };
            crate::generation::during(snapshot, || crate::host::invoke(*owner, *cb, &mut args));
            start = ctx.start;
        }
        Shape::Vein { .. } | Shape::Steps(_) => {
            let cells = vein(f, pos, random);
            start.put(Put::Cells(cells));
        }
        Shape::Cube => return None,
    }
    (!start.puts.is_empty()).then_some(start)
}

/// Adds template `variant` at `at` to `start`, with what it stands on and brings.
#[allow(clippy::too_many_arguments)]
pub fn make_piece(view: &crate::generation::ReadCtx<'_>, at: IVec3, variant: Arc<Variant>, foundation: u32, clear_above: bool) -> Piece {
    let snapshot = view.snapshot;
    let (def, seed) = (snapshot.def(), snapshot.seed);
    let size = variant.size;
    let needs_ground = foundation != 0 || clear_above || variant.cells.iter().any(|c| c.1 == GROUND);
    let ground: Arc<[(i32, u32)]> = if needs_ground {
        (0..size.z)
            .flat_map(|z| (0..size.x).map(move |x| (x, z)))
            .map(|(x, z)| {
                let (wx, wz) = (at.x + x, at.z + z);
                (def.column(seed, wx, wz).map_or(i32::MIN, |c| c.height), view.surface_block(wx, wz))
            })
            .collect()
    } else {
        Arc::from([])
    };
    Piece { at, variant, foundation, clear_above, ground }
}

pub fn add_piece(start: &mut Start, piece: Piece) {
    let (at, variant) = (piece.at, &piece.variant);
    for (p, kind, data) in &variant.entities {
        start
            .extras
            .push(Extra { at: at.as_dvec3() + *p, anchor: None, kind: *kind, data: data.clone(), loot: None });
    }
    for (p, data) in &variant.data {
        let b = at + *p;
        start.extras.push(Extra {
            at: b.as_dvec3() + DVec3::splat(0.5),
            anchor: Some(b),
            kind: u32::MAX,
            data: Some(data.clone()),
            loot: None,
        });
    }
    for (p, table) in &variant.loot {
        let b = at + *p;
        start.extras.push(Extra {
            at: b.as_dvec3() + DVec3::splat(0.5),
            anchor: Some(b),
            kind: u32::MAX,
            data: None,
            loot: Some(table.clone()),
        });
    }
    // What it stands on and what it cuts away count as its own.
    if piece.foundation != 0 || piece.clear_above {
        let (mut lo, mut hi) = (piece.at, piece.hi());
        for &(h, _) in piece.ground.iter().filter(|g| g.0 != i32::MIN) {
            lo.y = lo.y.min(h + 1);
            hi.y = hi.y.max(h);
        }
        start.grow(lo, hi);
    }
    start.put(Put::Piece(piece));
}

/// A plan working out a start (`Host::plan_op`).
pub struct PlanCtx<'a> {
    pub c: &'a Content,
    pub def: &'a DimensionDef,
    pub dim: u32,
    pub seed: u64,
    pub fi: usize,
    random: u64,
    pub start: Start,
    warned: bool,
}

impl PlanCtx<'_> {
    /// Whether the box stays within the feature's reach of the start.
    fn within(&mut self, lo: IVec3, hi: IVec3) -> bool {
        let f = &self.def.features[self.fi];
        let s = self.start.pos;
        let (rlo, rhi) = (s - IVec3::new(f.reach[0], f.reach[2], f.reach[0]), s + IVec3::new(f.reach[0], f.reach[1], f.reach[0]));
        let ok = lo.cmpge(rlo).all() && hi.cmple(rhi).all();
        if !ok && !self.warned {
            self.warned = true;
            let (name, r) = (f.name.clone(), f.reach);
            crate::host::misuse("plan", || format!("{name} put something past its reach {r:?} (left out); `.reach` says how far it goes"));
        }
        ok
    }

    /// One op of a plan; whether it went in.
    pub fn op(&mut self, op: &abi::PlanOp) -> bool {
        use abi::plan_op as k;
        let c = self.c;
        let (pos, to) = (crate::world::ivec(op.pos), crate::world::ivec(op.to));
        let name = unsafe { op.name.as_str() };
        match op.kind {
            k::PUT | k::SET => {
                if op.state as usize >= c.states.len() || !self.within(pos, pos) {
                    return false;
                }
                self.start
                    .put(Put::Block { at: pos, state: op.state, force: op.kind == k::SET });
            }
            k::FILL | k::REPLACE => {
                let (lo, hi) = (pos.min(to), pos.max(to));
                if op.state as usize >= c.states.len() || !self.within(lo, hi) {
                    return false;
                }
                let only = if op.kind == k::REPLACE {
                    match crate::refs::compile(name, &c.block_names, &c.blocks, &c.components) {
                        Ok(m) => Some(Arc::from(m)),
                        Err(e) => {
                            crate::host::misuse("plan", || format!("replace: {e}"));
                            return false;
                        }
                    }
                } else {
                    None
                };
                self.start.put(Put::Fill { lo, hi, state: op.state, only });
            }
            k::TEMPLATE => {
                let Some(&t) = c.template_names.get(name) else {
                    crate::host::misuse("plan", || format!("no template {name}"));
                    return false;
                };
                let f = &self.def.features[self.fi];
                let extra = unsafe { op.extra.as_str() };
                let palette = if extra.is_empty() {
                    let h = hash(self.random, op.pos.x as i64, op.pos.y as i64, op.pos.z as i64);
                    (!f.palettes.is_empty()).then(|| f.palettes[(h >> 40) as usize % f.palettes.len()])
                } else {
                    match c.palette_names.get(extra) {
                        Some(&p) => Some(p),
                        None => {
                            crate::host::misuse("plan", || format!("no palette {extra}"));
                            return false;
                        }
                    }
                };
                let variant = c.templates[t as usize].variant(c, Transform::new(op.transform), palette);
                if !self.within(pos, pos + variant.size - IVec3::ONE) {
                    return false;
                }
                let snapshot = crate::generation::Snapshot::new(c, self.dim, self.seed, f.stage);
                let view = crate::generation::ReadCtx { snapshot, local: None, plan: Some(std::ptr::addr_of!(self.start)) };
                let piece = make_piece(&view, pos, variant, op.state, op.flags & abi::piece_flags::CLEAR_ABOVE != 0);
                add_piece(&mut self.start, piece);
            }
            k::ENTITY => {
                let Some(&kind) = c.game.entity_names.get(name) else {
                    crate::host::misuse("plan", || format!("no entity kind {name}"));
                    return false;
                };
                if !self.within(pos, pos) {
                    return false;
                }
                self.start.extras.push(Extra {
                    at: pos.as_dvec3() + DVec3::new(0.5, 0.0, 0.5),
                    anchor: None,
                    kind,
                    data: None,
                    loot: None,
                });
            }
            k::LOOT => {
                if !self.within(pos, pos) {
                    return false;
                }
                let at = pos.as_dvec3() + DVec3::splat(0.5);
                self.start
                    .extras
                    .push(Extra { at, anchor: Some(pos), kind: u32::MAX, data: None, loot: Some(Arc::from(name)) });
            }
            _ => return false,
        }
        true
    }
}

/// The cells of one vein or steps from `start`, decided by `h` alone, so every cube it reaches
/// agrees.
fn vein(f: &FeatureDef, start: IVec3, h: u64) -> Vec<IVec3> {
    use abi::vein_shape as v;
    let ball = |c: IVec3, r: f32, out: &mut Vec<IVec3>| {
        let n = r.ceil() as i32;
        for z in -n..=n {
            for y in -n..=n {
                for x in -n..=n {
                    if ((x * x + y * y + z * z) as f32) <= r * r {
                        out.push(c + IVec3::new(x, y, z));
                    }
                }
            }
        }
    };
    let mut out = Vec::new();
    match &f.shape {
        Shape::Steps(cells) => out.extend(cells.iter().map(|d| start + *d)),
        &Shape::Vein { kind: v::SPHERE, radius, .. } => ball(start, radius, &mut out),
        &Shape::Vein { kind: v::DISK, radius, thickness, .. } => {
            let t = thickness.max(1.0) as i32;
            let n = radius.ceil() as i32;
            for z in -n..=n {
                for x in -n..=n {
                    if ((x * x + z * z) as f32) <= radius * radius {
                        for y in 0..t {
                            out.push(start + IVec3::new(x, y - t / 2, z));
                        }
                    }
                }
            }
        }
        &Shape::Vein { kind: v::BLOB, size, .. } => {
            // Small balls along a short random line, about `size` blocks in all.
            let r = (size as f32 * 3.0 / (4.0 * std::f32::consts::PI))
                .cbrt()
                .max(0.8);
            let dir = IVec3::new((h >> 20) as i32 % 3 - 1, (h >> 24) as i32 % 3 - 1, (h >> 28) as i32 % 3 - 1);
            ball(start, r * 0.8, &mut out);
            ball(start + dir, r * 0.8, &mut out);
            out.sort_unstable_by_key(|p| (p.x, p.y, p.z));
            out.dedup();
        }
        &Shape::Vein { size, .. } => {
            let mut p = start;
            for k in 0..size {
                let hk = hash(h, k as i64, 0, 0);
                p += IVec3::new((hk % 3) as i32 - 1, ((hk >> 2) % 3) as i32 - 1, ((hk >> 4) % 3) as i32 - 1);
                out.push(p);
            }
        }
        _ => {}
    }
    let reach = f.vein_reach();
    out.retain(|p| (*p - start).abs().max_element() <= reach);
    out
}

/// The cells of a shape (`abi::ShapeStep`s) about its start, for `Shape::Steps`.
pub fn steps_cells(shape: &crate::shape::Shape) -> Vec<IVec3> {
    let (lo, hi) = shape.bounds();
    let (lo, hi) = (lo.floor().as_ivec3(), hi.ceil().as_ivec3());
    let mut out = Vec::new();
    for z in lo.z..=hi.z {
        for y in lo.y..=hi.y {
            for x in lo.x..=hi.x {
                let d = IVec3::new(x, y, z);
                if shape.contains(d.as_dvec3() + DVec3::splat(0.5)) {
                    out.push(d);
                }
            }
        }
    }
    out
}

fn grid_starts(c: &Content, def: &DimensionDef, dim: u32, seed: u64, fi: usize, x: i32, z: i32) -> Vec<Arc<Start>> {
    let f = &def.features[fi];
    let Some(spacing) = f.grid() else { return Vec::new() };
    let (lo, hi) = (IVec3::new(x * 16, i32::MIN, z * 16), IVec3::new(x * 16 + 15, i32::MAX, z * 16 + 15));
    let mut out = Vec::new();
    let r = f.reach[0];
    for cz in (lo.z - r).div_euclid(spacing)..=(hi.z + r).div_euclid(spacing) {
        for cx in (lo.x - r).div_euclid(spacing)..=(hi.x + r).div_euclid(spacing) {
            if let Some(s) = start_at(c, def, dim, seed, fi, (cx, cz)).filter(|s| s.touches(lo, hi)) {
                out.push(s);
            }
        }
    }
    out
}

/// Every start on a grid whose box reaches into cube column (`x`, `z`), in feature order: the
/// same for every cube of the column, so worked out once for it.
fn column_starts(c: &Content, def: &DimensionDef, dim: u32, seed: u64, x: i32, z: i32) -> Arc<Vec<Arc<Start>>> {
    def.sites.get(seed, x, z, || {
        (0..def.features.len())
            .flat_map(|fi| grid_starts(c, def, dim, seed, fi, x, z))
            .collect()
    })
}

/// Works out the starts of cube column (`x`, `z`) ahead of its cubes: they all need them, and the
/// ones finished meanwhile waited for whichever was working them out.
pub fn warm(c: &Content, dim: u32, seed: u64, x: i32, z: i32) {
    let def = &c.dims[dim as usize];
    if def.features.iter().any(|f| f.grid().is_some()) {
        column_starts(c, def, dim, seed, x, z);
    }
}

/// Where a start's blocks go: a cube being generated, or the world itself.
pub trait Cells {
    /// The box it takes blocks in.
    fn bounds(&self) -> (IVec3, IVec3);
    /// What is at `p`, `None` past the bounds.
    fn get(&self, p: IVec3) -> Option<u32>;
    fn set(&mut self, p: IVec3, state: u32);
}

/// Writes what `start` puts into `to`, as far as its bounds go.
pub fn write_start(c: &Content, def: &DimensionDef, seed: u64, start: &Start, to: &mut impl Cells) {
    let (clo, chi) = to.bounds();
    if !start.touches(clo, chi) {
        return;
    }
    for put in &start.puts {
        match put {
            Put::Block { at, state, force } => {
                if to.get(*at).is_some_and(|s| *force || s == AIR) {
                    to.set(*at, *state);
                }
            }
            Put::Fill { lo, hi, state, only } => {
                let (lo, hi) = (lo.max(clo), hi.min(chi));
                for y in lo.y..=hi.y {
                    for z in lo.z..=hi.z {
                        for x in lo.x..=hi.x {
                            let p = IVec3::new(x, y, z);
                            if to
                                .get(p)
                                .is_some_and(|s| only.as_ref().is_none_or(|m| crate::refs::has(m, s)))
                            {
                                to.set(p, *state);
                            }
                        }
                    }
                }
            }
            Put::Cells(cells) => {
                let Some(f) = def.features.get(start.feature) else { continue };
                let h = hash(seed ^ f.salt, start.pos.x as i64, start.pos.y as i64, start.pos.z as i64);
                for &p in cells {
                    write_cell(c, f, to, p, h);
                }
            }
            Put::Piece(piece) => write_piece(piece, to),
        }
    }
}

/// Writes a vein's or steps' cell as feature `f` writes.
fn write_cell(c: &Content, f: &FeatureDef, to: &mut impl Cells, p: IVec3, h: u64) {
    let Some(here) = to.get(p) else { return };
    // Vanilla's ores in caves: some of what would touch air is left out.
    let bare = f.write.exposed > 0.0
        && crate::world::DIRS
            .iter()
            .any(|d| to.get(p + *d) == Some(AIR))
        && weft_noise::unit(hash(h, p.x as i64, p.y as i64, p.z as i64)) < f.write.exposed as f64;
    if !bare && let Some(s) = f.write.state_in(&c.matchers, here) {
        to.set(p, s);
    }
}

fn write_piece(piece: &Piece, to: &mut impl Cells) {
    let v = &piece.variant;
    let (lo, hi) = to.bounds();
    for &(p, state) in &v.cells {
        let at = piece.at + p;
        if to.get(at).is_none() {
            continue;
        }
        let state = match (state, piece.column(p.x, p.z).1) {
            (GROUND, 0) if v.ground == 0 => continue,
            (GROUND, 0) => v.ground,
            (GROUND, top) => top,
            (s, _) => s,
        };
        to.set(at, state);
    }
    let inside = |x: i32, z: i32| x >= lo.x && x <= hi.x && z >= lo.z && z <= hi.z;
    if piece.foundation != 0 {
        for &(x, z) in &v.bottom {
            let (h, top) = piece.column(x, z);
            let (wx, wz) = (piece.at.x + x, piece.at.z + z);
            if h == i32::MIN || !inside(wx, wz) {
                continue;
            }
            let state = match piece.foundation {
                GROUND if top == 0 => v.ground,
                GROUND => top,
                s => s,
            };
            if state == 0 {
                continue;
            }
            for y in (h + 1).max(lo.y)..piece.at.y.min(hi.y + 1) {
                to.set(IVec3::new(wx, y, wz), state);
            }
        }
    }
    if piece.clear_above {
        let top = piece.hi().y + 1;
        for z in 0..v.size.z {
            for x in 0..v.size.x {
                let (h, _) = piece.column(x, z);
                let (wx, wz) = (piece.at.x + x, piece.at.z + z);
                if !inside(wx, wz) {
                    continue;
                }
                for y in top.max(lo.y)..=h.min(hi.y) {
                    to.set(IVec3::new(wx, y, wz), AIR);
                }
            }
        }
    }
}

/// A cube being generated: its blocks and what it brings beyond them.
pub struct CubeGen<'a> {
    pub c: &'a Content,
    pub def: &'a DimensionDef,
    pub seed: u64,
    pub pos: IVec3,
    pub blocks: &'a mut [u32],
    pub generated: Generated,
    /// States in the cube, once a per-cube feature asked.
    present: Option<Vec<u32>>,
    pub changed: bool,
}

impl Cells for CubeGen<'_> {
    fn bounds(&self) -> (IVec3, IVec3) {
        (self.origin(), self.origin() + IVec3::splat(15))
    }
    fn get(&self, p: IVec3) -> Option<u32> {
        self.local(p).map(|i| self.blocks[i])
    }
    fn set(&mut self, p: IVec3, state: u32) {
        let Some(i) = self.local(p) else { return };
        if self.blocks[i] != state {
            self.blocks[i] = state;
            self.changed = true;
            if let Some(p) = &mut self.present
                && !p.contains(&state)
            {
                p.push(state);
            }
        }
    }
}

impl<'a> CubeGen<'a> {
    pub fn new(c: &'a Content, def: &'a DimensionDef, seed: u64, pos: IVec3, blocks: &'a mut [u32]) -> CubeGen<'a> {
        CubeGen { c, def, seed, pos, blocks, generated: Generated::default(), present: None, changed: false }
    }

    fn origin(&self) -> IVec3 {
        self.pos * 16
    }

    fn local(&self, p: IVec3) -> Option<usize> {
        let l = p - self.origin();
        (l.cmpge(IVec3::ZERO).all() && l.cmplt(IVec3::splat(16)).all()).then(|| local_index(l))
    }

    /// Writes what `start` puts into this cube, keeping what it brings that is here.
    fn write_start(&mut self, start: &Start) {
        let (c, def, seed) = (self.c, self.def, self.seed);
        write_start(c, def, seed, start, self);
        for e in &start.extras {
            if self.local(e.at.floor().as_ivec3()).is_some() {
                self.generated.extras.push(e.clone());
            }
        }
        let f = &def.features[start.feature];
        if self.local(start.pos).is_some() && f.grid().is_some() {
            self.generated
                .placed
                .push((f.id, start.pos, start.transform.0));
        }
    }

    /// The veins (and other per-cube starts) of feature `fi` that reach into this cube, from it and
    /// the cubes around within their reach.
    fn per_cube(&mut self, dim: u32, fi: usize, biomes: &[u16; 256]) {
        let f = &self.def.features[fi];
        let o = self.origin();
        let reach = match f.shape {
            Shape::Vein { .. } | Shape::Steps(_) => f.vein_reach(),
            _ => f.reach[0].max(f.reach[1]).max(f.reach[2]),
        };
        if o.y > f.y[1].saturating_add(reach) || o.y + 15 < f.y[0].saturating_sub(reach) {
            return;
        }
        // A vein has nothing to do where it could replace none of the states here (air and water,
        // the most of what is generated), and a look costs far less than its veins.
        if matches!(f.shape, Shape::Vein { .. } | Shape::Steps(_)) {
            let blocks = &*self.blocks;
            let present = self.present.get_or_insert_with(|| {
                let mut v = vec![blocks[0]];
                let mut last = blocks[0];
                for &b in blocks.iter() {
                    if b != last {
                        last = b;
                        if !v.contains(&b) {
                            v.push(b);
                        }
                    }
                }
                v
            });
            if !present
                .iter()
                .any(|&s| f.write.state_in(&self.c.matchers, s).is_some())
            {
                return;
            }
        }
        let n = (reach + 15).div_euclid(16).max(1);
        let pos = self.pos;
        for nz in pos.z - n..=pos.z + n {
            for ny in pos.y - n..=pos.y + n {
                for nx in pos.x - n..=pos.x + n {
                    for v in 0..f.tries() {
                        let h = hash(self.seed ^ f.salt, nx as i64, ny as i64 * 31 + v as i64, nz as i64);
                        let start =
                            IVec3::new(nx * 16 + (h % 16) as i32, ny * 16 + ((h >> 4) % 16) as i32, nz * 16 + ((h >> 8) % 16) as i32);
                        // Starts farther away than they reach never get here.
                        let far = (o - start).max(start - o - IVec3::splat(15)).max_element();
                        if start.y > f.y[1] || start.y < f.y[0] || far > reach {
                            continue;
                        }
                        if weft_noise::unit(hash(h, 7, 7, 7)) >= f.weight(start.y) {
                            continue;
                        }
                        if let Some(b) = &f.biome {
                            let l = start - o;
                            let here = l.x >= 0 && l.x < 16 && l.z >= 0 && l.z < 16;
                            let biome = if here {
                                biomes[(l.z * 16 + l.x) as usize] as u32
                            } else {
                                self.def
                                    .column(self.seed, start.x, start.z)
                                    .map_or(0, |c| c.biome)
                            };
                            if !crate::refs::has(b, biome) {
                                continue;
                            }
                        }
                        match f.shape {
                            Shape::Vein { .. } | Shape::Steps(_) => {
                                let c = self.c;
                                for p in vein(f, start, h) {
                                    write_cell(c, f, self, p, h);
                                }
                            }
                            _ => {
                                let t = f.transforms[(h >> 40) as usize % f.transforms.len()];
                                if let Some(s) = make_start(self.c, self.def, dim, self.seed, fi, start, 0, hash(h, 1, 2, 3), t) {
                                    self.write_start(&s);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Places every feature of the dimension into a cube the terrain and generator filled, in order.
pub fn place(
    c: &Content,
    dim: u32,
    seed: u64,
    pos: IVec3,
    blocks: &mut [u32],
    biomes: &mut [u16; 256],
    columns: *const abi::TerrainColumn,
) -> (bool, Generated) {
    place_before(c, dim, seed, pos, blocks, biomes, columns, u32::MAX)
}

#[allow(clippy::too_many_arguments)]
pub fn place_before(
    c: &Content,
    dim: u32,
    seed: u64,
    pos: IVec3,
    blocks: &mut [u32],
    biomes: &mut [u16; 256],
    columns: *const abi::TerrainColumn,
    before: u32,
) -> (bool, Generated) {
    let def = &c.dims[dim as usize];
    let mut g = CubeGen::new(c, def, seed, pos, blocks);
    let starts =
        (before == u32::MAX && def.features.iter().any(|f| f.grid().is_some())).then(|| column_starts(c, def, dim, seed, pos.x, pos.z));
    let mut phase = None;
    let mut input = Vec::new();
    let mut input_biomes = *biomes;
    for (fi, f) in def
        .features
        .iter()
        .enumerate()
        .take_while(|(_, f)| f.stage < before)
    {
        if phase != Some(f.stage) {
            phase = Some(f.stage);
            if def
                .features
                .iter()
                .any(|g| g.stage == f.stage && matches!(g.spread, Spread::EveryCube(..)))
            {
                input = g.blocks.to_vec();
                input_biomes = *biomes;
            }
        }
        match &f.spread {
            Spread::PerCube { .. } => g.per_cube(dim, fi, biomes),
            Spread::Grid { .. } => {
                let local;
                let sites = match &starts {
                    Some(v) => &**v,
                    None => {
                        local = grid_starts(c, def, dim, seed, fi, pos.x, pos.z);
                        &local
                    }
                };
                for s in sites.iter().filter(|s| s.feature == fi) {
                    g.write_start(s);
                }
            }
            Spread::EveryCube(owner, cb) => {
                let mut blocks = input.clone();
                let mut next_biomes = input_biomes;
                let mut written = [0u64; 64];
                let mut biomes_written = [0u64; 4];
                let snapshot = crate::generation::Snapshot::new(c, dim, seed, f.stage);
                let read = crate::generation::ReadCtx { snapshot, local: Some((pos, blocks.as_ptr())), plan: None };
                let mut args = abi::GenArgs {
                    source: snapshot.source(&read),
                    written: written.as_mut_ptr(),
                    biomes_written: biomes_written.as_mut_ptr(),
                    seed,
                    cube: pos.to_array().into(),
                    blocks: blocks.as_mut_ptr(),
                    biomes: next_biomes.as_mut_ptr(),
                    dim,
                    columns,
                };
                crate::generation::during(snapshot, || crate::host::invoke(*owner, *cb, &mut args));
                for (i, (&old, &new)) in input.iter().zip(&blocks).enumerate() {
                    if old != new || written[i / 64] & (1 << (i % 64)) != 0 {
                        g.blocks[i] = new;
                    }
                }
                for (i, (&old, &new)) in input_biomes.iter().zip(&next_biomes).enumerate() {
                    if old != new || biomes_written[i / 64] & (1 << (i % 64)) != 0 {
                        biomes[i] = new;
                    }
                }
                g.changed = true;
                g.present = None;
            }
        }
    }
    (g.changed, g.generated)
}

/// Whether a feature on a grid stands within its reach of column (`x`, `z`): spawn search keeps
/// players out of trees.
pub fn stands_near(c: &Content, def: &DimensionDef, dim: u32, seed: u64, x: i32, z: i32) -> bool {
    def.features.iter().any(|f| {
        let Some(spacing) = f.grid() else { return false };
        let r = f.reach[0].max(1);
        let cells = |v: i32| (v - r).div_euclid(spacing)..=(v + r).div_euclid(spacing);
        cells(z).any(|cz| {
            cells(x).any(|cx| grid_site(c, def, dim, f, seed, (cx, cz)).is_some_and(|(p, ..)| (p.x - x).abs() <= r && (p.z - z).abs() <= r))
        })
    })
}

/// The nearest start of feature `name` within `radius` of `pos`, generated yet or not.
pub fn nearest(c: &Content, dim: u32, seed: u64, name: &str, pos: IVec3, radius: i32) -> Option<IVec3> {
    let def = &c.dims[dim as usize];
    let fi = def.features.iter().position(|f| f.name == name)?;
    let spacing = def.features[fi].grid()?;
    let cells = |v: i32| (v - radius).div_euclid(spacing)..=(v + radius).div_euclid(spacing);
    let mut best: Option<IVec3> = None;
    for cz in cells(pos.z) {
        for cx in cells(pos.x) {
            if let Some(s) = start_at(c, def, dim, seed, fi, (cx, cz)) {
                let d = |p: IVec3| (p - pos).with_y(0).as_vec3().length();
                if d(s.pos) <= radius as f32 && best.is_none_or(|b| d(s.pos) < d(b)) {
                    best = Some(s.pos);
                }
            }
        }
    }
    best
}

/// The blocks a feature on a grid puts from a start at `pos`, relative to it, with whether each
/// goes over what is there: what distant terrain samples of trees.
pub fn sample(c: &Content, dim: u32, seed: u64, fi: usize, pos: IVec3, biome: u32, random: u64) -> Vec<(IVec3, u32, bool)> {
    let def = &c.dims[dim as usize];
    let Some(s) = make_start(c, def, dim, seed, fi, pos, biome, random, Transform(0)) else { return Vec::new() };
    s.puts
        .iter()
        .filter_map(|p| match p {
            Put::Block { at, state, force } => Some((*at - pos, *state, *force)),
            _ => None,
        })
        .collect()
}
