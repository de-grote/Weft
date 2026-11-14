//! Deterministic generation snapshots. Reads never depend on loaded or edited world cubes.
use crate::content::{Content, DimensionDef};
use crate::features::{Cells, Start};
use crate::world::cube::VOLUME;
use crate::world::local_index;
use glam::IVec3;
use rustc_hash::FxHashMap;
use std::sync::{Arc, Mutex};
use weft_abi as abi;

thread_local! {
    static COLUMNS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static ACTIVE: std::cell::RefCell<Vec<(u32, u64, u32, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn during<T>(snapshot: Snapshot<'_>, run: impl FnOnce() -> T) -> T {
    struct Pop;
    impl Drop for Pop {
        fn drop(&mut self) {
            ACTIVE.with_borrow_mut(|v| {
                v.pop();
            });
        }
    }
    ACTIVE.with_borrow_mut(|v| v.push((snapshot.dim, snapshot.seed, snapshot.before, snapshot.terrain_only)));
    let _pop = Pop;
    run()
}

pub fn during_column<T>(run: impl FnOnce() -> T) -> T {
    struct Pop;
    impl Drop for Pop {
        fn drop(&mut self) {
            COLUMNS.with(|n| n.set(n.get() - 1));
        }
    }
    COLUMNS.with(|n| n.set(n.get() + 1));
    let _pop = Pop;
    run()
}

pub fn can_read(source: &abi::GenSource) -> bool {
    if COLUMNS.with(|n| n.get() != 0) {
        return false;
    }
    ACTIVE.with_borrow(|v| {
        v.iter()
            .all(|&(dim, seed, before, terrain)| dim != source.dim || seed != source.seed || (!terrain && source.before <= before))
    })
}

pub fn validate(c: &Content, dim: u32, blocks: &mut [u32], what: &'static str) {
    let max = c.states.len() as u32;
    let top = blocks.iter().copied().max().unwrap_or(0);
    if top >= max {
        for b in blocks.iter_mut().filter(|b| **b >= max) {
            *b = 0;
        }
        crate::host::misuse(what, || format!("{} wrote state {top}, which does not exist (air instead)", c.dims[dim as usize].name));
    }
}

type Key = (u64, u32, bool, IVec3);
#[derive(Default)]
pub struct Snapshots(Mutex<FxHashMap<Key, Arc<Vec<u32>>>>);

#[derive(Clone, Copy)]
pub struct Snapshot<'a> {
    pub c: &'a Content,
    pub dim: u32,
    pub seed: u64,
    pub before: u32,
    pub terrain_only: bool,
}

impl<'a> Snapshot<'a> {
    pub fn new(c: &'a Content, dim: u32, seed: u64, before: u32) -> Self {
        Self { c, dim, seed, before, terrain_only: false }
    }
    pub fn def(&self) -> &'a DimensionDef {
        &self.c.dims[self.dim as usize]
    }
    pub fn source(&self, ctx: *const ReadCtx<'_>) -> abi::GenSource {
        abi::GenSource { dim: self.dim, seed: self.seed, before: self.before, ctx: ctx.cast() }
    }
    pub fn read(&self, p: IVec3) -> u32 {
        let def = self.def();
        let cube = p.div_euclid(IVec3::splat(16));
        if cube.y < def.min_cube_y || cube.y > def.max_cube_y {
            return 0;
        }
        if def.builtin.is_none() && (self.terrain_only || def.generator.is_none()) && !def.features.iter().any(|f| f.stage < self.before) {
            return def
                .terrain
                .as_ref()
                .map_or(0, |t| t.read(self.seed, p.x, p.y, p.z));
        }
        self.cube(cube)[local_index(p.rem_euclid(IVec3::splat(16)))]
    }
    pub fn surface_block(&self, x: i32, z: i32) -> u32 {
        self.def()
            .column(self.seed, x, z)
            .filter(|c| c.height != i32::MIN)
            .map_or(0, |c| self.read(IVec3::new(x, c.height, z)))
    }
    fn cube(&self, pos: IVec3) -> Arc<Vec<u32>> {
        let key = (self.seed, self.before, self.terrain_only, pos);
        let cache = &self.def().snapshots.0;
        if let Some(v) = cache.lock().unwrap().get(&key).cloned() {
            return v;
        }
        // Compute outside the lock: earlier phases can be queried recursively.
        let mut base = base(self.c, self.dim, self.seed, pos);
        if !self.terrain_only {
            run_generator(self.c, self.dim, self.seed, pos, &mut base.blocks, &mut base.biomes, base.columns.as_deref());
            validate(self.c, self.dim, &mut base.blocks, "generator");
            let columns = base
                .columns
                .as_ref()
                .map_or(std::ptr::null(), |v| v.as_ptr());
            crate::features::place_before(self.c, self.dim, self.seed, pos, &mut base.blocks, &mut base.biomes, columns, self.before);
            validate(self.c, self.dim, &mut base.blocks, "feature");
        }
        let made = Arc::new(base.blocks);
        let mut cache = cache.lock().unwrap();
        if cache.len() >= 128 {
            cache.clear();
        }
        cache.entry(key).or_insert_with(|| made.clone()).clone()
    }
}

pub struct Base {
    pub blocks: Vec<u32>,
    pub biomes: [u16; 256],
    pub columns: Option<Arc<[abi::TerrainColumn; 256]>>,
}

pub fn base(c: &Content, dim: u32, seed: u64, pos: IVec3) -> Base {
    let def = &c.dims[dim as usize];
    let mut out = Base { blocks: vec![0; VOLUME], biomes: [0; 256], columns: None };
    if pos.y < def.min_cube_y || pos.y > def.max_cube_y {
        return out;
    }
    if let Some(f) = def.builtin {
        f(c, pos, &mut out.blocks);
    } else if let Some(t) = &def.terrain {
        out.columns = Some(Arc::new(
            t.generate(seed, pos.to_array(), &mut out.blocks, &mut out.biomes)
                .cols,
        ));
    } else {
        out.columns = def.columns(seed, pos.x, pos.z);
    }
    out
}

pub fn run_generator(
    c: &Content,
    dim: u32,
    seed: u64,
    pos: IVec3,
    blocks: &mut [u32],
    biomes: &mut [u16; 256],
    columns: Option<&[abi::TerrainColumn; 256]>,
) {
    let def = &c.dims[dim as usize];
    if def.builtin.is_some() {
        return;
    }
    if let Some((owner, cb)) = def.generator {
        let mut snapshot = Snapshot::new(c, dim, seed, 0);
        snapshot.terrain_only = true;
        let ctx = ReadCtx { snapshot, local: Some((pos, blocks.as_ptr())), plan: None };
        let mut args = abi::GenArgs {
            source: snapshot.source(&ctx),
            written: std::ptr::null_mut(),
            biomes_written: std::ptr::null_mut(),
            seed,
            dim,
            cube: pos.to_array().into(),
            blocks: blocks.as_mut_ptr(),
            biomes: biomes.as_mut_ptr(),
            columns: columns.map_or(std::ptr::null(), |v| v.as_ptr()),
        };
        during(snapshot, || crate::host::invoke(owner, cb, &mut args));
    }
}

/// Borrowed only while the engine invokes a callback. Raw pointers reflect its latest writes.
pub struct ReadCtx<'a> {
    pub snapshot: Snapshot<'a>,
    pub local: Option<(IVec3, *const u32)>,
    pub plan: Option<*const Start>,
}
impl ReadCtx<'_> {
    pub fn surface_block(&self, x: i32, z: i32) -> u32 {
        self.snapshot
            .def()
            .column(self.snapshot.seed, x, z)
            .filter(|c| c.height != i32::MIN)
            .map_or(0, |c| self.read(IVec3::new(x, c.height, z)))
    }
    pub fn read(&self, p: IVec3) -> u32 {
        let cube_y = p.y.div_euclid(16);
        let def = self.snapshot.def();
        if cube_y < def.min_cube_y || cube_y > def.max_cube_y {
            return 0;
        }
        if let Some((cube, blocks)) = self.local {
            let local = p - cube * 16;
            if local.cmpge(IVec3::ZERO).all() && local.cmplt(IVec3::splat(16)).all() {
                return unsafe { *blocks.add(local_index(local)) };
            }
        }
        let mut point = Point { pos: p, state: self.snapshot.read(p) };
        if let Some(plan) = self.plan {
            crate::features::write_start(self.snapshot.c, self.snapshot.def(), self.snapshot.seed, unsafe { &*plan }, &mut point);
        }
        point.state
    }
}
struct Point {
    pos: IVec3,
    state: u32,
}
impl Cells for Point {
    fn bounds(&self) -> (IVec3, IVec3) {
        (self.pos, self.pos)
    }
    fn get(&self, p: IVec3) -> Option<u32> {
        (p == self.pos).then_some(self.state)
    }
    fn set(&mut self, p: IVec3, state: u32) {
        if p == self.pos {
            self.state = state;
        }
    }
}
