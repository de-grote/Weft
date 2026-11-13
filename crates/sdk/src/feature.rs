//! Features: everything generation puts into the world after the terrain, ores and trees,
//! boulders and structures, villages and grass. Each is where it goes, how its starts spread,
//! its shape and how it writes, and `r.ore`, `r.scatter`, `r.structure` and `r.decoration` are
//! the usual mixes of those:
//!
//! ```ignore
//! r.ore(&TIN_ORE).y(-64, 90).per_cube(5).vein_size(8).replaces("#stone_ore_replaceables");
//! r.structure(&WELL).spacing(64).chance(0.3).biome("#animals").turns();
//! r.feature(&GEODES).per_cube(1).height(Height::uniform(-50, 0)).vein(Vein::shape(&shell)).block(&AMETHYST);
//! r.scatter(&TREES).spacing(5).density(&TREE_DENSITY).reach(3).plan(|p| { .. });
//! r.feature(&VILLAGE).spacing(512).chance(0.3).reach(96, 32, 16).plan(village);
//! ```

use crate::abi::{self, BlockPos, Str};
use crate::comps::feature as f;
use crate::terrain::Column;
use crate::{Block, Feature, Gen, LootTable, Palette, Placed, Registry, Template, WithComponents, callback, host};

/// One of the 8 ways a template is placed: quarter turns clockwise seen from above, after a
/// mirror across x. Blocks turn and mirror with it, a door's hinge and a stair's corner too.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Transform(pub u8);

impl Transform {
    pub const NONE: Transform = Transform(0);
    /// `quarters` clockwise.
    pub fn turned(quarters: u32) -> Transform {
        Transform((quarters & abi::transform::TURN) as u8)
    }
    /// The same, mirrored across x first.
    pub fn mirrored(self) -> Transform {
        Transform(self.0 | abi::transform::MIRROR as u8)
    }
    pub fn quarters(self) -> u32 {
        self.0 as u32 & abi::transform::TURN
    }
    pub fn is_mirrored(self) -> bool {
        self.0 as u32 & abi::transform::MIRROR != 0
    }
    /// All 8.
    pub fn all() -> impl Iterator<Item = Transform> {
        (0..8).map(Transform)
    }
    /// Which face its own face `face` (`FACE_DIRS` index) becomes.
    pub fn face(self, face: usize) -> usize {
        let f = if self.is_mirrored() && face < 2 { face ^ 1 } else { face };
        // East goes south, west, north.
        let turn = abi::Turn { east: [1, 5, 0, 4][self.quarters() as usize], up: 3 };
        turn.face(f)
    }
}

/// Heights a feature's starts spread over (`FeatureBuilder::height`).
#[derive(Clone, Copy, Debug)]
pub struct Height {
    min: i32,
    max: i32,
    curve: u32,
    plateau: i32,
}

impl Height {
    pub fn uniform(min: i32, max: i32) -> Height {
        Height { min, max, curve: abi::height_curve::UNIFORM, plateau: 0 }
    }
    /// Most in the middle, fewer towards the ends, none past them.
    pub fn triangle(min: i32, max: i32) -> Height {
        Height { min, max, curve: abi::height_curve::TRIANGLE, plateau: 0 }
    }
    /// At its most over `plateau` blocks in the middle, then falling off.
    pub fn trapezoid(min: i32, max: i32, plateau: i32) -> Height {
        Height { min, max, curve: abi::height_curve::TRAPEZOID, plateau }
    }
}

/// What a vein looks like (`FeatureBuilder::vein`). Every vein stays within 16 blocks of where it
/// starts.
#[derive(Clone, Debug)]
pub struct Vein {
    kind: u32,
    radius: f32,
    thickness: f32,
    blocks: u32,
    steps: Vec<abi::ShapeStep>,
}

impl Vein {
    /// A random walk of `blocks` steps, the default.
    pub fn walk(blocks: u32) -> Vein {
        Vein { kind: abi::vein_shape::WALK, radius: 0.0, thickness: 0.0, blocks, steps: Vec::new() }
    }
    /// A lump of about `blocks`, like modern vanilla's.
    pub fn blob(blocks: u32) -> Vein {
        Vein { kind: abi::vein_shape::BLOB, radius: 0.0, thickness: 0.0, blocks, steps: Vec::new() }
    }
    pub fn sphere(radius: f32) -> Vein {
        Vein { kind: abi::vein_shape::SPHERE, radius, thickness: 0.0, blocks: 0, steps: Vec::new() }
    }
    /// Flat and round (clay, gravel and sand under water).
    pub fn disk(radius: f32, thickness: f32) -> Vein {
        Vein { kind: abi::vein_shape::DISK, radius, thickness, blocks: 0, steps: Vec::new() }
    }
    /// Any shape about its start, as a fill takes (`Shape::ball(..).subtract(..)`: a geode's shell).
    pub fn shape(shape: &crate::Shape) -> Vein {
        Vein { kind: 0, radius: 0.0, thickness: 0.0, blocks: 0, steps: shape.steps().to_vec() }
    }
}

impl Registry {
    /// A feature with nothing set: say how it spreads (`per_cube`, `spacing`, `generate`), its
    /// shape (`vein`, `template`, `plan`) and what it writes. The presets below are the usual
    /// mixes.
    pub fn feature(&mut self, feature: &'static Feature) -> FeatureBuilder<'_> {
        FeatureBuilder { reg: self, e: crate::entry::Entry::of(feature), reach: None }
    }

    /// An ore, named like its block: 4 veins of 8 blocks per cube unless said, in every
    /// dimension but bare ones (`.dimension(..)` limits it), only where it `replaces`:
    /// `r.ore(&TIN_ORE).y(0, 64).per_cube(6).vein_size(8).replaces("#stone")`.
    pub fn ore(&mut self, block: &Block) -> FeatureBuilder<'_> {
        self.feature(block.ore())
            .set(&f::SPREAD, abi::spread::PER_CUBE)
            .set(&f::SHAPE, abi::shape::VEIN)
            .set_text(&f::BLOCK, block.name())
    }

    /// Something scattered over the ground (trees): one start per `spacing` x `spacing` square
    /// (8 unless said) kept with the chance a biome component gives (`density`), drawn by a
    /// `plan`. Starts under the sea or where caves took the ground are left out, as are those
    /// that would stand in a structure.
    ///
    /// ```ignore
    /// r.scatter(&TREES).spacing(5).reach(2).density(&TREE_DENSITY).plan(|p| {
    ///     let BlockPos { x, y, z } = p.pos();
    ///     for dy in 0..5 { p.set([x, y + dy, z], LOG.id()); }
    /// });
    /// ```
    pub fn scatter(&mut self, feature: &'static Feature) -> FeatureBuilder<'_> {
        self.feature(feature)
            .spacing(8)
            .set(&f::STAGE, abi::stage::VEGETATION)
    }

    /// A template placed now and then on the ground, named like it: a `chance` of 0.5 per
    /// `spacing` of 32 unless said, in every dimension but bare ones. It keeps trees out, and gives
    /// way to structures of a higher priority (`priority`, 1 here).
    pub fn structure(&mut self, template: &Template) -> FeatureBuilder<'_> {
        self.feature(template.structure())
            .spacing(32)
            .chance(0.5)
            .template(template)
            .priority(1)
    }

    /// Runs `f` on every generated cube after everything else, column by column (grass,
    /// flowers), in every dimension but bare ones unless `.dimension(..)` limits it.
    /// `g.columns()` has the surface of every column. Pure, like a generator. Named
    /// `<mod>:decoration`, `/2`.. unless `named`, for other mods' `edit` and `remove`.
    pub fn decoration(&mut self, f: impl Fn(&mut Gen) + Send + Sync + 'static) -> FeatureBuilder<'_> {
        let name = self.derive_name("decoration");
        FeatureBuilder { reg: self, e: crate::entry::Entry::new(abi::entry_kind::FEATURE, name), reach: None }.generate(f)
    }
}

/// A feature being registered or changed (`r.edit(IRON_ORE.ore())`: what it sets replaces the
/// feature's, lists add to it).
pub struct FeatureBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
    /// Sideways, up, down, sent once done.
    reach: Option<[i32; 3]>,
}

fn feature_entry<'a>(reg: &'a mut Registry, feature: &'static Feature) -> FeatureBuilder<'a> {
    reg.feature(feature)
}

impl Registry {
    fn feature_entry(&mut self, feature: &'static Feature) -> FeatureBuilder<'_> {
        feature_entry(self, feature)
    }
}

crate::entry::entry_builder!(
    Feature => FeatureBuilder, feature_entry, finish;
);

impl FeatureBuilder<'_> {
    /// Its name instead of the one it was given (`<mod>:decoration`).
    pub fn named(mut self, name: &str) -> Self {
        let derived = std::mem::replace(&mut self.e.name, crate::qualify(self.reg.ns, name));
        self.reg.derived.retain(|n| *n != derived);
        self
    }

    // Order.

    /// When its turn comes (`abi::stage`; by its spread unless said: underground per cube,
    /// structures on a grid, decoration on every cube).
    pub fn stage(self, stage: u32) -> Self {
        self.set(&f::STAGE, stage)
    }
    /// After `other` in the same dimension.
    pub fn after(self, other: &Feature) -> Self {
        self.push(&f::AFTER, other.name().to_string())
    }
    pub fn before(self, other: &Feature) -> Self {
        self.push(&f::BEFORE, other.name().to_string())
    }

    // Spread.

    /// `n` starts tried in every cube, over its heights (`y` or `height`).
    pub fn per_cube(self, n: u32) -> Self {
        self.set(&f::SPREAD, abi::spread::PER_CUBE)
            .set(&f::COUNT, n)
    }
    /// How its starts spread over heights: `Height::triangle(-24, 56)` puts most in the middle,
    /// like modern vanilla's iron.
    pub fn height(self, h: Height) -> Self {
        self.y(h.min, h.max)
            .set(&f::HEIGHT, abi::HeightCurve { curve: h.curve, plateau: h.plateau })
    }
    /// One start per `blocks` x `blocks` square, on the ground.
    pub fn spacing(self, blocks: i32) -> Self {
        self.set(&f::SPREAD, abi::spread::GRID)
            .set(&f::SPACING, blocks)
    }
    /// The chance of a start per square (1 unless said).
    pub fn chance(self, c: f32) -> Self {
        self.set(&f::CHANCE, c)
    }
    /// The biome component (0..1) that is the chance of a start per square; biomes without it get
    /// none.
    pub fn density(self, component: &crate::Component<f32>) -> Self {
        self.set_text(&f::DENSITY, component.name())
    }
    /// Only where `what` takes the surface block (a block matcher, `.ground("#plant_soil")`).
    pub fn ground(self, what: &str) -> Self {
        self.set_text(&f::GROUND, what)
    }
    /// It may stand where the ground is under the sea.
    pub fn over_water(self) -> Self {
        self.tag(abi::comp::feature::OVER_WATER)
    }
    /// It may stand where caves took the ground.
    pub fn over_caves(self) -> Self {
        self.tag(abi::comp::feature::OVER_CAVES)
    }
    /// Only where the ground under it varies by at most `blocks`.
    pub fn max_slope(self, blocks: i32) -> Self {
        self.set(&f::MAX_SLOPE, blocks)
    }
    /// How far into the ground its bottom layer goes.
    pub fn buried(self, blocks: i32) -> Self {
        self.set(&f::BURIED, blocks)
    }
    /// One with a higher priority keeps it `separation` blocks away; only what really stands
    /// counts.
    pub fn priority(self, p: i32) -> Self {
        self.set(&f::PRIORITY, p)
    }
    pub fn separation(self, blocks: i32) -> Self {
        self.set(&f::SEPARATION, blocks)
    }
    /// `f` on every generated cube, deciding everything itself (a decoration).
    pub fn generate(self, f: impl Fn(&mut Gen) + Send + Sync + 'static) -> Self {
        self.set(&f::SPREAD, abi::spread::EVERY_CUBE)
            .set(&f::GENERATE, callback(move |a: &mut abi::GenArgs| f(Gen::wrap(a))))
    }

    // Shape.

    /// What a vein looks like (a random walk of 8 blocks unless said).
    pub fn vein(self, v: Vein) -> Self {
        if !v.steps.is_empty() {
            return self
                .set(&f::SHAPE, abi::shape::STEPS)
                .set(&f::STEPS, v.steps);
        }
        let b = self
            .set(&f::SHAPE, abi::shape::VEIN)
            .set(&f::VEIN, abi::VeinShape { kind: v.kind, radius: v.radius, thickness: v.thickness });
        if v.blocks > 0 { b.vein_size(v.blocks) } else { b }
    }
    /// Blocks per vein (8 unless said).
    pub fn vein_size(self, n: u32) -> Self {
        self.set(&f::SIZE, n)
    }
    /// Template `t` from its lowest corner.
    pub fn template(self, t: &Template) -> Self {
        self.set(&f::SHAPE, abi::shape::TEMPLATE)
            .set_text(&f::TEMPLATE, t.name())
    }
    /// Each one turned a random quarter, its blocks with it.
    pub fn turns(self) -> Self {
        self.transforms(abi::transform::TURNS)
    }
    /// Each one turned a random quarter and mirrored or not.
    pub fn turns_and_mirrors(self) -> Self {
        self.transforms(abi::transform::ALL)
    }
    /// The ways its template may be placed: a bit per `Transform` (`1 << t.0`).
    pub fn transforms(self, bits: u32) -> Self {
        self.set(&f::TRANSFORMS, bits)
    }
    /// One more palette for its slots; the seed picks one per start.
    pub fn palette(self, p: &Palette) -> Self {
        self.push(&f::PALETTES, p.name().to_string())
    }
    /// `f` lays out each start: blocks, templates, entities. It runs once per start, on the
    /// server and on clients alike, so it may use only the seed (`p.random()`) and the ground.
    pub fn plan(self, f: impl Fn(&mut Plan) + Send + Sync + 'static) -> Self {
        self.set(&f::SHAPE, abi::shape::PLAN)
            .set(&f::PLAN, callback(move |a: &mut abi::PlanArgs| f(Plan::wrap(a))))
    }
    /// How far a plan may put things sideways from its start (0 unless said); past that they are
    /// left out. Up and down it may go 32 and 16 unless `reach_y` says.
    pub fn reach(mut self, blocks: i32) -> Self {
        self.reach.get_or_insert([0, 32, 16])[0] = blocks;
        self
    }
    pub fn reach_y(mut self, up: i32, down: i32) -> Self {
        let r = self.reach.get_or_insert([0, 32, 16]);
        (r[1], r[2]) = (up, down);
        self
    }
    fn finish(&mut self) {
        if let Some(r) = self.reach.take() {
            self.e.ops.push(f::REACH.raw_value(&r));
        }
    }

    // Writing.

    /// What a vein or steps put (an ore named like its block puts that).
    pub fn block(self, b: &Block) -> Self {
        self.set_text(&f::BLOCK, b.name())
    }
    /// What it may replace (a block matcher); anything unless said.
    pub fn replaces(self, what: &str) -> Self {
        self.set_text(&f::REPLACES, what)
    }
    /// Where a vein meets what `what` takes (a block matcher), `block` goes there instead: one vein
    /// through stone and granite, iron ore in one and granite iron ore in the other.
    pub fn replaces_with(self, what: &str, block: &Block) -> Self {
        let v = abi::Variant { name: crate::qualify_matcher(self.reg.ns, what), block: block.name().to_string() };
        self.push(&f::REPLACES_WITH, v)
    }
    /// Share of its blocks left out where they would touch air, as vanilla's ores in caves (0.5
    /// for most of them).
    pub fn exposed(self, share: f32) -> Self {
        self.set(&f::EXPOSED, share)
    }
    /// Its template stands on `block` filled down to the ground.
    pub fn foundation(self, block: &Block) -> Self {
        self.set_text(&f::FOUNDATION, block.name())
    }
    /// Its template stands on the ground's own top block filled down (grass on plains, sand on a
    /// beach).
    pub fn foundation_of_ground(self) -> Self {
        self.set_text(&f::FOUNDATION, "ground")
    }
    /// The ground above its template is cut away.
    pub fn clear_above(self) -> Self {
        self.tag(abi::comp::feature::CLEAR_ABOVE)
    }
}

/// One joint of a placed template (`Plan::joints`).
#[derive(Clone, Debug)]
pub struct Joint {
    /// From the template's lowest corner as placed.
    pub pos: BlockPos,
    /// The way it faces (`FACE_DIRS` index): a piece joined here goes that way.
    pub face: usize,
    pub name: String,
    /// What the joint of the piece joined here must be called.
    pub target: String,
    /// The pool that piece comes from.
    pub pool: String,
}

/// Where `Plan::grow` takes pieces from: pools by name, each a weighted list of templates.
#[derive(Default)]
pub struct Pools<'a> {
    pools: Vec<PiecePool<'a>>,
}

/// A pool: its name, its templates by weight, whether each is put down on the ground.
type PiecePool<'a> = (&'a str, Vec<(&'a Template, u32)>, bool);

impl<'a> Pools<'a> {
    pub fn new() -> Pools<'a> {
        Pools::default()
    }
    /// Pool `name`: these templates, by weight, placed as their joints say.
    pub fn pool(mut self, name: &'a str, pieces: &[(&'a Template, u32)]) -> Self {
        self.pools.push((name, pieces.to_vec(), false));
        self
    }
    /// Pool `name`, each piece put down on the ground where it lands (streets over hills).
    pub fn pool_on_ground(mut self, name: &'a str, pieces: &[(&'a Template, u32)]) -> Self {
        self.pools.push((name, pieces.to_vec(), true));
        self
    }
}

/// One start of a feature being laid out (`FeatureBuilder::plan`), in world coordinates.
pub struct Plan {
    args: abi::PlanArgs,
}

impl Plan {
    fn wrap(a: &mut abi::PlanArgs) -> &mut Plan {
        unsafe { &mut *(a as *mut abi::PlanArgs as *mut Plan) }
    }
    /// The start: the block above the ground for one on a grid.
    pub fn pos(&self) -> BlockPos {
        self.args.pos
    }
    pub fn biome(&self) -> u32 {
        self.args.biome
    }
    pub fn dimension(&self) -> u32 {
        self.args.dim
    }
    pub fn seed(&self) -> u64 {
        self.args.seed
    }
    /// Random bits of this start, the same every time it is laid out.
    pub fn random(&self) -> u64 {
        self.args.random
    }
    /// More random bits, the same every time for the same `n`.
    pub fn random_n(&self, n: u64) -> u64 {
        crate::noise::hash(self.args.random, n as i64, 0, 0)
    }

    fn op(&mut self, op: abi::PlanOp) -> bool {
        unsafe { (host().plan_op)(self.args.ctx, &op) != 0 }
    }
    fn simple(kind: u32, pos: BlockPos, to: BlockPos, state: u32, name: &str) -> abi::PlanOp {
        abi::PlanOp { kind, transform: 0, pos, to, state, flags: 0, name: Str::new(name), extra: Str::new("") }
    }

    /// `state` at `pos` where there is air. Whether it went in (not past the reach).
    pub fn put(&mut self, pos: impl Into<BlockPos>, state: u32) -> bool {
        let p = pos.into();
        self.op(Plan::simple(abi::plan_op::PUT, p, p, state, ""))
    }
    /// `state` at `pos` whatever is there (a trunk through another tree's leaves).
    pub fn set(&mut self, pos: impl Into<BlockPos>, state: u32) -> bool {
        let p = pos.into();
        self.op(Plan::simple(abi::plan_op::SET, p, p, state, ""))
    }
    /// A box of `state` between two opposite corners.
    pub fn fill(&mut self, from: impl Into<BlockPos>, to: impl Into<BlockPos>, state: u32) -> bool {
        self.op(Plan::simple(abi::plan_op::FILL, from.into(), to.into(), state, ""))
    }
    /// What `what` takes in the box (a block matcher, `#logs`) becomes `state`.
    pub fn replace(&mut self, from: impl Into<BlockPos>, to: impl Into<BlockPos>, what: &str, state: u32) -> bool {
        self.op(Plan::simple(abi::plan_op::REPLACE, from.into(), to.into(), state, what))
    }
    /// An entity of `kind` on block `pos` once the start's cube joins the world.
    pub fn entity(&mut self, kind: &crate::EntityKind, pos: impl Into<BlockPos>) -> bool {
        let p = pos.into();
        self.op(Plan::simple(abi::plan_op::ENTITY, p, p, 0, kind.name()))
    }
    /// The block entity at `pos` (a chest) filled from `table` once it joins the world.
    pub fn loot(&mut self, pos: impl Into<BlockPos>, table: &LootTable) -> bool {
        let p = pos.into();
        self.op(Plan::simple(abi::plan_op::LOOT, p, p, 0, table.name()))
    }
    /// Template `t` with its lowest corner at `corner` once placed with `transform`; say more on
    /// the piece, it goes in when dropped or `put`.
    pub fn template<'p>(&'p mut self, t: &'p Template, corner: impl Into<BlockPos>, transform: Transform) -> Piece<'p> {
        Piece { plan: self, name: t.name(), pos: corner.into(), transform, foundation: 0, flags: 0, palette: None, done: false }
    }

    /// The ground of column (`x`, `z`): its height and biome.
    pub fn ground(&self, x: i32, z: i32) -> Column {
        let mut out = Column { height: i32::MIN, biome: 0 };
        unsafe { (host().dim_column)(self.args.dim, self.args.seed, x, z, &mut out) };
        out
    }
    /// Whether caves took the surface block of column (`x`, `z`).
    pub fn surface_carved(&self, x: i32, z: i32) -> bool {
        unsafe { (host().dim_surface_carved)(self.args.dim, self.args.seed, x, z) != 0 }
    }
    /// Template `t`'s size once placed with `transform`; zero for none.
    pub fn size(&self, t: &Template, transform: Transform) -> BlockPos {
        template_size(t, transform)
    }
    /// Template `t`'s joints once placed with `transform`.
    pub fn joints(&self, t: &Template, transform: Transform) -> Vec<Joint> {
        template_joints(t, transform)
    }
    /// Where template `t` placed with `transform` would stand with its north-west corner at column
    /// (`x`, `z`): its lowest corner on the ground (its middle column's), if the ground under it
    /// varies by at most `max_slope` and caves took none of it.
    pub fn fit(&self, t: &Template, x: i32, z: i32, transform: Transform, max_slope: i32) -> Option<BlockPos> {
        let s = self.size(t, transform);
        if s.x <= 0 {
            return None;
        }
        let mid = self.ground(x + s.x / 2, z + s.z / 2).height;
        let (mut lo, mut hi) = (mid, mid);
        for (cx, cz) in [(x, z), (x + s.x - 1, z), (x, z + s.z - 1), (x + s.x - 1, z + s.z - 1)] {
            if self.surface_carved(cx, cz) {
                return None;
            }
            let h = self.ground(cx, cz).height;
            lo = lo.min(h);
            hi = hi.max(h);
        }
        (mid != i32::MIN && hi - lo <= max_slope).then_some(BlockPos { x, y: mid + 1, z })
    }

    /// Joins pieces from `pools` onto template `start` (placed at `corner` with `transform`)
    /// by their joints, up to `depth` steps out: each open joint takes a piece from its pool
    /// whose joint is called its `target`, faced back at it, that overlaps nothing placed so far
    /// and stays within the reach. How many pieces went in.
    pub fn grow(&mut self, start: &Template, corner: impl Into<BlockPos>, transform: Transform, pools: &Pools, depth: u32) -> usize {
        let corner = corner.into();
        if !self.template(start, corner, transform).put() {
            return 0;
        }
        let size = self.size(start, transform);
        let mut boxes = vec![(corner, add(corner, size, -1))];
        let mut open: std::collections::VecDeque<(BlockPos, Joint, u32)> = self
            .joints(start, transform)
            .into_iter()
            .map(|j| (corner, j, 1))
            .collect();
        let mut placed = 1;
        let mut n = 0u64;
        while let Some((at, joint, d)) = open.pop_front() {
            if d > depth {
                continue;
            }
            let Some((_, pieces, on_ground)) = pools.pools.iter().find(|p| p.0 == joint.pool) else { continue };
            let total: u32 = pieces.iter().map(|p| p.1).sum();
            if total == 0 {
                continue;
            }
            // Where the joint is and the cell it opens onto.
            let from = add(at, joint.pos, 0);
            let dir = abi::FACE_DIRS[joint.face % 6];
            let onto = BlockPos { x: from.x + dir[0], y: from.y + dir[1], z: from.z + dir[2] };
            // Pieces in a seeded order by weight, each tried every way it faces back.
            n += 1;
            let mut order: Vec<(&Template, u64)> = pieces
                .iter()
                .enumerate()
                .map(|(i, (t, w))| {
                    let r = crate::noise::unit(self.random_n(n * 64 + i as u64));
                    // Weighted random order: smaller keys first.
                    (*t, (-(r.max(1e-12)).ln() / *w.max(&1) as f64 * 1e9) as u64)
                })
                .collect();
            order.sort_by_key(|o| o.1);
            'pick: for (t, _) in order {
                for tr in Transform::all() {
                    for j in self.joints(t, tr) {
                        if j.name != joint.target || j.face != (joint.face ^ 1) {
                            continue;
                        }
                        let mut c = BlockPos { x: onto.x - j.pos.x, y: onto.y - j.pos.y, z: onto.z - j.pos.z };
                        let s = self.size(t, tr);
                        if *on_ground {
                            let g = self.ground(c.x + s.x / 2, c.z + s.z / 2).height;
                            if g != i32::MIN {
                                c.y = g + 1;
                            }
                        }
                        let b = (c, add(c, s, -1));
                        if boxes.iter().any(|o| overlaps(*o, b)) {
                            continue;
                        }
                        if !self.template(t, c, tr).put() {
                            continue;
                        }
                        boxes.push(b);
                        placed += 1;
                        for next in self
                            .joints(t, tr)
                            .into_iter()
                            .filter(|o| o.pos != j.pos || o.name != j.name)
                        {
                            open.push_back((c, next, d + 1));
                        }
                        break 'pick;
                    }
                }
            }
        }
        placed
    }
}

fn add(a: BlockPos, b: BlockPos, k: i32) -> BlockPos {
    BlockPos { x: a.x + b.x + k, y: a.y + b.y + k, z: a.z + b.z + k }
}

fn overlaps(a: (BlockPos, BlockPos), b: (BlockPos, BlockPos)) -> bool {
    a.0.x <= b.1.x && a.1.x >= b.0.x && a.0.y <= b.1.y && a.1.y >= b.0.y && a.0.z <= b.1.z && a.1.z >= b.0.z
}

/// Template `t`'s size once placed with `transform`; zero for none.
pub fn template_size(t: &Template, transform: Transform) -> BlockPos {
    let mut out = BlockPos::default();
    unsafe { (host().template_size)(Str::new(t.name()), transform.0 as u32, &mut out) };
    out
}

/// Template `t`'s joints once placed with `transform`.
pub fn template_joints(t: &Template, transform: Transform) -> Vec<Joint> {
    let name = Str::new(t.name());
    let n = unsafe { (host().template_joints)(name, transform.0 as u32, std::ptr::null_mut(), 0) } as usize;
    if n == 0 {
        return Vec::new();
    }
    let empty = abi::Joint { pos: BlockPos::default(), face: 0, name: Str::new(""), target: Str::new(""), pool: Str::new("") };
    let mut out = vec![empty; n];
    unsafe { (host().template_joints)(name, transform.0 as u32, out.as_mut_ptr(), n as u32) };
    out.iter()
        .map(|j| unsafe {
            Joint {
                pos: j.pos,
                face: j.face as usize,
                name: j.name.as_str().to_string(),
                target: j.target.as_str().to_string(),
                pool: j.pool.as_str().to_string(),
            }
        })
        .collect()
}

/// A template a plan puts (`Plan::template`).
pub struct Piece<'p> {
    plan: &'p mut Plan,
    name: &'p str,
    pos: BlockPos,
    transform: Transform,
    foundation: u32,
    flags: u32,
    palette: Option<&'p str>,
    done: bool,
}

impl<'p> Piece<'p> {
    /// It stands on `block` filled down to the ground.
    pub fn foundation(mut self, block: u32) -> Self {
        self.foundation = block;
        self
    }
    /// It stands on the ground's own top block filled down.
    pub fn foundation_of_ground(mut self) -> Self {
        self.foundation = u32::MAX;
        self
    }
    /// The ground above it is cut away.
    pub fn clear_above(mut self) -> Self {
        self.flags |= abi::piece_flags::CLEAR_ABOVE;
        self
    }
    /// Its slots filled from `p` (else from one of the feature's palettes, else its defaults).
    pub fn palette(mut self, p: &'p Palette) -> Self {
        self.palette = Some(p.name());
        self
    }
    /// Puts it in now: whether it went in (it exists and stays within the reach).
    pub fn put(mut self) -> bool {
        self.send()
    }
    fn send(&mut self) -> bool {
        if self.done {
            return false;
        }
        self.done = true;
        let op = abi::PlanOp {
            kind: abi::plan_op::TEMPLATE,
            transform: self.transform.0 as u32,
            pos: self.pos,
            to: self.pos,
            state: self.foundation,
            flags: self.flags,
            name: Str::new(self.name),
            extra: Str::new(self.palette.unwrap_or("")),
        };
        self.plan.op(op)
    }
}

impl Drop for Piece<'_> {
    fn drop(&mut self) {
        self.send();
    }
}
