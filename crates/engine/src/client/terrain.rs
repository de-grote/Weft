//! The world the client draws, made here rather than sent: cubes in view are generated from the
//! seed exactly as the server would, the dimension's area edits are made on them and they are
//! lit here. The server sends only what it simulates around players and what differs from
//! generation (`ToClient::Saved`, then the cubes); those replace what was generated. Runs on its
//! own thread and hands the client world finished cubes, a batch per round.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, select, unbounded};
use glam::{IVec2, IVec3};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::content::{Content, content};
use crate::server::area::{self, AreaFill};
use crate::world::light::{self, CubeMap, Lighter, SkyBelow};
use crate::world::{Cube, View};

pub enum In {
    Reset {
        dim: u32,
        seed: u64,
    },
    View {
        center: IVec3,
        radius: i32,
    },
    /// A cube the server sent: it wins over generation from now on. `edit`: sent again because
    /// it changed (shown whole with the rest of its round).
    Cube {
        pos: IVec3,
        cube: Arc<Cube>,
        edit: bool,
    },
    Blocks(Vec<(IVec3, u32)>, Vec<(IVec3, Vec<u32>)>),
    Unload(IVec3),
    Saved(Vec<IVec3>),
    Edits {
        specs: Vec<String>,
        all: bool,
    },
    /// What came from the server in one frame, handled in one round (one batch out).
    Many(Vec<In>),
}

/// Cubes around one that are not there and not waited for (`cube::around_mask` bits).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Around {
    /// Never coming, or hidden: solid ground as far as its faces go.
    pub hidden: u32,
    /// Over a column whose ground came first: empty sky for now.
    pub sky: u32,
}

pub enum Out {
    /// `around`: which cubes around it not to wait for.
    Cube {
        pos: IVec3,
        cube: Arc<Cube>,
        around: Around,
    },
    Blocks(Vec<(IVec3, u32)>, Vec<(IVec3, Vec<u32>)>),
    Unload(IVec3),
    /// The view now spans cubes `lo..=hi` up and down.
    Range {
        lo: i32,
        hi: i32,
    },
}

/// What one round handed over: the resets it comes after, whether it has to be shown whole
/// (it carries edits: the server's cubes and blocks, area edits; loading is shown as it comes),
/// and what.
pub struct Batch {
    pub resets: u64,
    pub whole: bool,
    pub out: Vec<Out>,
}

/// The client's end: send what the server says and where the player is, take finished batches.
pub struct TerrainHandle {
    pub tx: Sender<In>,
    /// Batches with the number of resets they come after.
    pub rx: Receiver<Batch>,
    /// Cubes to generate, generating or waiting for the one above: 0 when the view is complete.
    pub pending: Arc<AtomicU64>,
}

/// `gpu`: where jobs go while `gpu_on` (the GPU makes the terrain, `terrain_gpu`).
pub fn spawn(gpu: Sender<super::terrain_gpu::Job>, gpu_on: Arc<std::sync::atomic::AtomicBool>) -> TerrainHandle {
    let (tx, in_rx) = unbounded();
    let (out_tx, rx) = unbounded();
    let pending = Arc::new(AtomicU64::new(0));
    let p = pending.clone();
    std::thread::Builder::new()
        .name("terrain".into())
        .spawn(move || {
            let (gen_tx, gen_rx) = unbounded();
            let mut t = Terrain::new(p, gen_tx);
            t.gpu = Some((gpu, gpu_on));
            t.run(in_rx, gen_rx, out_tx);
        })
        .expect("terrain thread");
    TerrainHandle { tx, rx, pending }
}

/// Whether the visibility fill picks what is generated (`core:client_visibility`: 0 never, 1
/// always, 2 when the CPU makes the terrain). It saves a sixth of the cubes, but its front
/// moving out reaches cubes next to ones already meshed, which are meshed again: with the GPU
/// making terrain that costs more than it saves (view 32: 6 s instead of 4, view 64: about the
/// same), with the CPU it saves (view 64: 47 s instead of 57).
fn visibility_on(gpu: bool) -> bool {
    let c = content();
    match c
        .game
        .config_names
        .get("core:client_visibility")
        .map(|&id| crate::config::get(id) as i32)
    {
        Some(0) => false,
        Some(1) => true,
        _ => !gpu,
    }
}

/// Batches the client may have waiting before no more terrain is asked for.
const BACKLOG: usize = 8;

/// What is asked first of a column seen from above: its ground and this many cubes either way
/// (trees above, the sides of what is not flat under). The rest of a column is empty sky or
/// hidden underground, more than nine cubes in ten of it at the longest view, and comes once
/// the ground of every column in view has.
const BAND: i32 = 2;

/// Cubes queued at most for the rest of the columns while the player moves, so that the ground
/// of columns coming into view never waits long behind them.
const REST_BACKLOG: usize = 8192;

/// Columns this close (in cubes) to a player near the ground come whole at once.
const WHOLE_NEAR: i32 = 12;

/// The cube heights of column (`col` in cubes) where its ground lies, `BAND` around it.
fn band_of(def: &crate::content::DimensionDef, seed: u64, col: IVec2) -> Option<(i32, i32)> {
    def.terrain.as_ref()?;
    let (x, z) = (col.x * 16, col.y * 16);
    let (mut lo, mut hi) = (i32::MAX, def.sea_level().unwrap_or(i32::MIN));
    for (dx, dz) in [(8, 8), (0, 0), (15, 0), (0, 15), (15, 15)] {
        let h = def.column(seed, x + dx, z + dz)?.height;
        (lo, hi) = (lo.min(h), hi.max(h));
    }
    Some(((lo >> 4) - BAND, (hi >> 4) + BAND))
}

/// Threads for lighting what arrived: the background pool's queue is long with generation and
/// meshing while loading, and the terrain thread waits for this. Below normal priority like the
/// other bulk work: the frame's culling comes first.
pub fn pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(crate::workers())
            .thread_name(|i| format!("terrain light {i}"))
            .start_handler(|_| crate::lower_priority())
            .build()
            .expect("terrain light pool")
    })
}

/// The terrain as the visibility fill sees it.
struct Seen<'a> {
    cubes: &'a FxHashMap<IVec3, Arc<Cube>>,
    conn: &'a mut FxHashMap<IVec3, u16>,
}

impl crate::server::visibility::Cubes for Seen<'_> {
    fn has(&self, pos: IVec3) -> bool {
        self.cubes.contains_key(&pos)
    }
    fn connectivity(&mut self, c: &Content, pos: IVec3) -> Option<u16> {
        let cube = self.cubes.get(&pos)?;
        Some(
            *self
                .conn
                .entry(pos)
                .or_insert_with(|| crate::server::dim::see_through(c, cube)),
        )
    }
}

struct Map<'a>(&'a mut FxHashMap<IVec3, Arc<Cube>>);

/// The cubes around one the server sent: its light and that of the others it sent is final, so
/// only cubes made here take part, giving and getting.
struct Sent<'a> {
    cubes: &'a mut FxHashMap<IVec3, Arc<Cube>>,
    server: &'a FxHashSet<IVec3>,
    sent: IVec3,
}

impl CubeMap for Sent<'_> {
    fn cube(&self, c: IVec3) -> Option<&Cube> {
        self.cubes.get(&c).map(|a| &**a)
    }
    fn cube_mut(&mut self, c: IVec3) -> Option<&mut Cube> {
        if c != self.sent && self.server.contains(&c) {
            return None;
        }
        self.cubes.get_mut(&c).map(Arc::make_mut)
    }
    fn writable(&self, c: IVec3) -> bool {
        c == self.sent || !self.server.contains(&c)
    }
    fn gives_light(&self, c: IVec3) -> bool {
        !self.server.contains(&c)
    }
}

/// The cubes, to read only.
struct Read<'a>(&'a FxHashMap<IVec3, Arc<Cube>>);

impl CubeMap for Read<'_> {
    fn cube(&self, c: IVec3) -> Option<&Cube> {
        self.0.get(&c).map(|a| &**a)
    }
    fn cube_mut(&mut self, _: IVec3) -> Option<&mut Cube> {
        None
    }
}

/// A run of cubes down a column being lit on a worker: its own cubes, over the ones already
/// in (read only: light toward them spreads out later, `outward`).
struct Run<'a> {
    cubes: &'a FxHashMap<IVec3, Arc<Cube>>,
    /// Its cubes by height below `top` (none where one was left out).
    own: Vec<Option<(IVec3, Cube)>>,
    top: IVec3,
}

impl Run<'_> {
    fn slot(&self, c: IVec3) -> Option<usize> {
        let i = self.top.y - c.y;
        (c.x == self.top.x && c.z == self.top.z && i >= 0 && (i as usize) < self.own.len() && self.own[i as usize].is_some())
            .then_some(i as usize)
    }
}

impl CubeMap for Run<'_> {
    fn cube(&self, c: IVec3) -> Option<&Cube> {
        match self.slot(c) {
            Some(i) => self.own[i].as_ref().map(|o| &o.1),
            None => self.cubes.get(&c).map(|a| &**a),
        }
    }
    fn cube_mut(&mut self, c: IVec3) -> Option<&mut Cube> {
        let i = self.slot(c)?;
        self.own[i].as_mut().map(|o| &mut o.1)
    }
    fn writable(&self, c: IVec3) -> bool {
        self.slot(c).is_some()
    }
}

/// Lights a run down a column (top first) against `cubes`. Each cube came lit on its own as if
/// open sky were above it (`lit`, unless an edit came since): right when the cube above lets
/// full sky through, and without its sky light when none comes through (block light does not
/// depend on it); otherwise lit again.
fn light_run(
    c: &Content,
    def: &crate::content::DimensionDef,
    cubes: &FxHashMap<IVec3, Arc<Cube>>,
    hi: i32,
    run: Vec<(IVec3, Cube, bool)>,
) -> Vec<(IVec3, Cube)> {
    let top = run[0].0;
    let mut map = Run { cubes, own: Vec::with_capacity((top.y - run[run.len() - 1].0.y + 1) as usize), top };
    for (pos, mut cube, lit) in run {
        // Nothing above and none coming: sky, the top of a column's ground asked before the rest.
        let open_above = light::open_above(def, pos.y) || pos.y >= hi || map.cube(pos + IVec3::Y).is_none();
        let sky = if def.sky_light { light::sky_below(map.cube(pos + IVec3::Y), open_above) } else { SkyBelow::Full };
        let prelit = lit && sky != SkyBelow::Partial;
        if prelit && sky == SkyBelow::Dark {
            cube.mask_light(!0xFF);
        }
        map.own.resize_with((top.y - pos.y) as usize, || None);
        map.own.push(Some((pos, cube)));
        let mut lighter = Lighter::new(&mut map, c, def.sky_light);
        if prelit {
            lighter.light_prelit(pos);
        } else {
            lighter.light_new(pos, open_above);
        }
    }
    map.own.into_iter().flatten().collect()
}

impl CubeMap for Map<'_> {
    fn cube(&self, c: IVec3) -> Option<&Cube> {
        self.0.get(&c).map(|a| &**a)
    }
    fn cube_mut(&mut self, c: IVec3) -> Option<&mut Cube> {
        self.0.get_mut(&c).map(Arc::make_mut)
    }
}

/// A generated cube: the round it was asked in (`Terrain::epoch`), where, what.
pub type Made = (u64, IVec3, Cube);

struct Terrain {
    dim: u32,
    seed: u64,
    /// Bumped on every reset: cubes asked for before it are dropped when they come.
    epoch: u64,
    active: bool,
    cubes: FxHashMap<IVec3, Arc<Cube>>,
    /// Sent by the server: never replaced by generation.
    server: FxHashSet<IVec3>,
    /// The server says they differ from generation and will send them: not generated.
    saved: FxHashSet<IVec3>,
    edits: Arc<Vec<AreaFill>>,
    view: View,
    /// Lowest and highest cube loaded (`lo..=hi`), for the current view.
    lo: i32,
    hi: i32,
    /// Columns by distance from the center, and how far the scan for missing cubes got.
    offsets: Vec<IVec2>,
    cursor: usize,
    /// Columns with every cube loaded, asked for or saved, for the current `lo..=hi`.
    done: FxHashSet<IVec2>,
    /// Where the ground of columns lies (`band_of`), and the columns whose ground was asked for.
    bands: FxHashMap<IVec2, (i32, i32)>,
    ground_done: FxHashSet<IVec2>,
    /// How far the scan for the rest of the columns got, after `cursor` for their ground.
    rest_cursor: usize,
    /// When the view last moved.
    moved: std::time::Instant,
    generating: FxHashSet<IVec3>,
    /// Generated, waiting for the cube above (sky light comes from it); whether its light from
    /// the worker still holds (an edit that came meanwhile changed its blocks).
    waiting: FxHashMap<IVec3, (Cube, bool)>,
    gen_tx: Sender<Made>,
    out: Vec<Out>,
    pending: Arc<AtomicU64>,
    gpu: Option<(Sender<super::terrain_gpu::Job>, Arc<std::sync::atomic::AtomicBool>)>,
    /// Whether the GPU can make this dimension's terrain.
    gpu_able: bool,
    vis: crate::server::visibility::Visibility,
    /// Which faces of each cube see each other, for `vis`; cubes changed since go into
    /// `edited`, and the fill looks at them again.
    conn: FxHashMap<IVec3, u16>,
    edited: Vec<IVec3>,
    /// Cubes not handed to the client world yet (waiting for their neighbors, or just put
    /// in); the others in `cubes` it has. Kept this way round: it is the small set.
    held: FxHashSet<IVec3>,
    /// What the fill wants, farthest first, and when it was gathered.
    wanted: Vec<IVec3>,
    wanted_at: Option<std::time::Instant>,
    /// `visibility_on()` for this round.
    vis_on: bool,
    /// The GPU made the last cubes asked for.
    gpu_on: bool,
    /// What goes out this round carries edits (`Batch::whole`).
    whole: bool,
}

impl Terrain {
    fn new(pending: Arc<AtomicU64>, gen_tx: Sender<Made>) -> Terrain {
        Terrain {
            dim: 0,
            seed: 0,
            epoch: 0,
            active: false,
            cubes: FxHashMap::default(),
            server: FxHashSet::default(),
            saved: FxHashSet::default(),
            edits: Arc::new(Vec::new()),
            view: View::default(),
            lo: 0,
            hi: -1,
            offsets: Vec::new(),
            cursor: 0,
            done: FxHashSet::default(),
            bands: FxHashMap::default(),
            ground_done: FxHashSet::default(),
            rest_cursor: 0,
            moved: std::time::Instant::now(),
            generating: FxHashSet::default(),
            waiting: FxHashMap::default(),
            gen_tx,
            out: Vec::new(),
            pending,
            gpu: None,
            gpu_able: false,
            vis: crate::server::visibility::Visibility::new(),
            conn: FxHashMap::default(),
            edited: Vec::new(),
            held: FxHashSet::default(),
            wanted: Vec::new(),
            wanted_at: None,
            vis_on: false,
            gpu_on: false,
            whole: false,
        }
    }

    fn run(&mut self, in_rx: Receiver<In>, gen_rx: Receiver<Made>, out_tx: Sender<Batch>) {
        loop {
            select! {
                recv(in_rx) -> m => match m {
                    Ok(m) => self.handle(m),
                    Err(_) => return,
                },
                recv(gen_rx) -> m => {
                    if let Ok(m) = m {
                        self.made(m);
                    }
                }
                default(Duration::from_millis(5)) => {}
            }
            let busy = std::time::Instant::now();
            self.vis_on = visibility_on(self.gpu_on);
            // Everything waiting, in one round.
            while let Ok(m) = in_rx.try_recv() {
                self.handle(m);
            }
            while let Ok(m) = gen_rx.try_recv() {
                self.made(m);
            }
            self.insert_waiting();
            // The client takes a few milliseconds of batches a frame: while it is behind, asking
            // for more would only queue them up (and edits behind them).
            if out_tx.len() < BACKLOG {
                self.request();
            }
            if !self.out.is_empty()
                && out_tx
                    .send(Batch { resets: self.epoch, whole: std::mem::take(&mut self.whole), out: std::mem::take(&mut self.out) })
                    .is_err()
            {
                return;
            }
            crate::diag::TERRAIN_BUSY_NS.add(busy.elapsed().as_nanos() as u64);
            let scanning = if self.vis_on { !self.wanted.is_empty() } else { self.rest_cursor < self.offsets.len() };
            let left = self.generating.len() + self.waiting.len() + scanning as usize;
            self.pending.store(left as u64, Ordering::Relaxed);
        }
    }

    fn handle(&mut self, m: In) {
        let c = content();
        match m {
            In::Many(list) => {
                for m in list {
                    self.handle(m);
                }
            }
            In::Reset { dim, seed } => {
                // The client world was cleared with it.
                let gpu = self.gpu.take();
                *self = Terrain {
                    epoch: self.epoch + 1,
                    offsets: std::mem::take(&mut self.offsets),
                    gpu,
                    gpu_on: self.gpu_on,
                    ..Terrain::new(self.pending.clone(), self.gen_tx.clone())
                };
                (self.dim, self.seed, self.active) = (dim, seed, true);
                self.gpu_able = super::terrain_gpu::describe(c, dim, seed).is_some();
                self.view.radius = 0;
            }
            In::View { center, radius } => self.set_view(center, radius),
            In::Cube { pos, cube, edit } => {
                self.whole |= edit;
                self.server.insert(pos);
                self.waiting.remove(&pos);
                if self.cubes.insert(pos, cube).is_some() {
                    self.conn.remove(&pos);
                    self.edited.push(pos);
                } else {
                    self.held.insert(pos);
                }
                // Its light is the server's; light from cubes made here joins it.
                let changed = self.light_sent(pos);
                self.arrived(pos);
                self.emit_all(changed);
            }
            In::Blocks(changes, extras) => {
                self.whole = true;
                for &(p, s) in &changes {
                    let cp = crate::world::cube_of(p);
                    if let Some(cube) = self.cubes.get_mut(&cp) {
                        Arc::make_mut(cube).set(crate::world::local_index(p), s);
                        if self.conn.remove(&cp).is_some() {
                            self.edited.push(cp);
                        }
                    }
                }
                for (p, parts) in &extras {
                    if let Some(cube) = self.cubes.get_mut(&crate::world::cube_of(*p)) {
                        Arc::make_mut(cube).set_extras(crate::world::local_index(*p), parts.clone());
                    }
                }
                self.out.push(Out::Blocks(changes, extras));
            }
            In::Unload(pos) => {
                self.server.remove(&pos);
            }
            In::Saved(list) => {
                for p in list {
                    self.saved.insert(p);
                }
            }
            In::Edits { specs, all } => {
                self.whole |= !all;
                let parsed: Vec<AreaFill> = specs.iter().filter_map(|s| AreaFill::parse(c, s)).collect();
                if all {
                    self.edits = Arc::new(parsed);
                    return;
                }
                let mut dirty = FxHashSet::default();
                for fill in &parsed {
                    area::fill_cubes(c, self.dim, fill, &mut self.cubes, &mut dirty);
                }
                Arc::make_mut(&mut self.edits).extend(parsed);
                for &p in &dirty {
                    if self.conn.remove(&p).is_some() {
                        self.edited.push(p);
                    }
                }
                self.emit_all(dirty);
            }
        }
    }

    fn in_view(&self, p: IVec3) -> bool {
        self.view.column(p) && p.y >= self.lo && p.y <= self.hi
    }

    fn set_view(&mut self, center: IVec3, radius: i32) {
        if !self.active || (center == self.view.center && radius == self.view.radius) {
            return;
        }
        let c = content();
        let def = &c.dims[self.dim as usize];
        if radius != self.view.radius || self.offsets.is_empty() {
            self.offsets = (-radius..=radius)
                .flat_map(|z| (-radius..=radius).map(move |x| IVec2::new(x, z)))
                .filter(|o| o.length_squared() <= radius * radius)
                .collect();
            self.offsets.sort_by_key(|o| o.length_squared());
        }
        let air = (1..=View::air_probe(radius))
            .take_while(|&d| {
                self.cubes
                    .get(&(center - IVec3::Y * d))
                    .is_some_and(|c| c.uniform_state() == Some(crate::content::AIR))
            })
            .count() as i32;
        let view = View::around(center, radius, air);
        let (lo, hi) = view.loaded_y(def);
        let range_moved = (lo, hi) != (self.lo, self.hi) || radius != self.view.radius;
        if (lo, hi) != (self.lo, self.hi) {
            self.done.clear();
            self.ground_done.clear();
            self.out.push(Out::Range { lo, hi });
        }
        let was = self.view.grown(2);
        (self.view, self.lo, self.hi) = (view, lo, hi);
        self.cursor = 0;
        self.rest_cursor = 0;
        self.moved = std::time::Instant::now();
        // What left the view goes (a cube more around it, against flicker at the edge). A step
        // sideways only loses columns on the side left behind: a look at the columns that were
        // kept, not at every cube, which is more than a million at the longest view.
        let keep = view.grown(2);
        let gone: Vec<IVec3> = if range_moved {
            self.cubes
                .keys()
                .copied()
                .filter(|&p| !keep.column(p) || p.y < lo - 2 || p.y > hi + 2)
                .collect()
        } else {
            let r = was.radius;
            let cubes = &self.cubes;
            (-r..=r)
                .flat_map(|z| (-r..=r).map(move |x| IVec3::new(was.center.x + x, 0, was.center.z + z)))
                .filter(|&c| was.column(c) && !keep.column(c))
                .flat_map(|c| (lo - 2..=hi + 2).map(move |y| IVec3::new(c.x, y, c.z)))
                .filter(|p| cubes.contains_key(p))
                .collect()
        };
        for p in gone {
            self.cubes.remove(&p);
            self.server.remove(&p);
            self.conn.remove(&p);
            if !self.held.remove(&p) {
                self.out.push(Out::Unload(p));
            }
        }
        self.saved.retain(|&p| keep.column(p));
        let near = |col: &IVec2| (*col - IVec2::new(center.x, center.z)).length_squared() <= radius * radius;
        self.done.retain(near);
        self.ground_done.retain(near);
        self.bands.retain(|col, _| near(col));
        // What a held cube waited for can only have left the view at the edge of it.
        let inner = (radius - 2).max(0).pow(2);
        let edge: Vec<IVec3> = self
            .held
            .iter()
            .copied()
            .filter(|&p| range_moved || (p - center).with_y(0).length_squared() >= inner)
            .collect();
        for p in edge {
            self.emit(p);
        }
    }

    /// Asks for the nearest missing cubes, each column from the top down: what the visibility
    /// fill reached (`core:client_visibility`), or every cube in view.
    fn request(&mut self) {
        if !self.active || self.view.radius == 0 {
            return;
        }
        // The GPU answers a few frames later whatever the count: it needs far more asked at once
        // to stay busy than the workers do.
        let gpu = self.gpu_able
            && self
                .gpu
                .as_ref()
                .is_some_and(|g| g.1.load(Ordering::Relaxed));
        self.gpu_on = gpu;
        let budget = if gpu { 65536 } else { crate::background().current_num_threads() * 64 };
        if self.vis_on {
            self.request_visible(budget);
            return;
        }
        // The ground of every column first, nearest first; whole columns near a player down there.
        let center = IVec2::new(self.view.center.x, self.view.center.z);
        while self.generating.len() + self.waiting.len() < budget && self.cursor < self.offsets.len() {
            let col = center + self.offsets[self.cursor];
            if self.done.contains(&col) || self.ground_done.contains(&col) {
                self.cursor += 1;
                continue;
            }
            self.find_bands(center);
            let band = self.bands.get(&col).copied();
            let (lo, hi) = band.unwrap_or((self.lo, self.hi));
            let close = (col - center).length_squared() <= WHOLE_NEAR * WHOLE_NEAR && self.view.center.y <= hi + 1;
            let whole = band.is_none() || close;
            let (lo, hi) = if whole { (self.lo, self.hi) } else { (lo.max(self.lo), hi.min(self.hi)) };
            if !self.ask_down(col, lo, hi, budget) {
                return;
            }
            if whole {
                self.done.insert(col);
            } else {
                self.ground_done.insert(col);
            }
            self.cursor += 1;
        }
        // Then the rest, as long as that keeps nothing waiting.
        let limit = if self.moved.elapsed() < Duration::from_secs(1) { REST_BACKLOG } else { budget };
        while self.cursor >= self.offsets.len() && self.rest_cursor < self.offsets.len() {
            if self.generating.len() + self.waiting.len() >= limit {
                return;
            }
            let col = center + self.offsets[self.rest_cursor];
            if !self.done.contains(&col) {
                if !self.ask_down(col, self.lo, self.hi, limit) {
                    return;
                }
                self.done.insert(col);
            }
            self.rest_cursor += 1;
        }
    }

    /// Works out where the ground lies for the next columns to ask for, on every core, and for
    /// the columns beside them: a cube looks at those to see what it need not wait for, and
    /// nothing would make it look again if it found out too late.
    fn find_bands(&mut self, center: IVec2) {
        use rayon::prelude::*;
        if self
            .bands
            .contains_key(&(center + self.offsets[self.cursor]))
        {
            return;
        }
        let c: &'static Content = content();
        let (def, seed) = (&c.dims[self.dim as usize], self.seed);
        let radius = self.view.radius;
        let mut next: FxHashSet<IVec2> = FxHashSet::default();
        for o in self.offsets[self.cursor..].iter().take(1024) {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let col = center + *o + IVec2::new(dx, dz);
                    if (col - center).length_squared() <= radius * radius && !self.bands.contains_key(&col) {
                        next.insert(col);
                    }
                }
            }
        }
        let next: Vec<IVec2> = next.into_iter().collect();
        let found: Vec<(IVec2, Option<(i32, i32)>)> = pool().install(|| {
            next.par_iter()
                .map(|&col| (col, band_of(def, seed, col)))
                .collect()
        });
        self.bands
            .extend(found.into_iter().filter_map(|(col, b)| b.map(|b| (col, b))));
    }

    /// Asks for the missing cubes of a column between two heights, from the top down, up to a
    /// total of `limit` being made. Returns whether it got to the bottom.
    fn ask_down(&mut self, col: IVec2, lo: i32, hi: i32, limit: usize) -> bool {
        let mut complete = true;
        let mut list = Vec::new();
        for y in (lo..=hi).rev() {
            let pos = IVec3::new(col.x, y, col.y);
            if !self.missing(pos) {
                continue;
            }
            if self.generating.len() + self.waiting.len() + list.len() >= limit {
                complete = false;
                break;
            }
            list.push(pos);
        }
        self.ask_column(list);
        complete
    }

    /// Neither here, nor coming from the server or the workers.
    fn missing(&self, pos: IVec3) -> bool {
        !(self.cubes.contains_key(&pos) || self.saved.contains(&pos) || self.generating.contains(&pos) || self.waiting.contains_key(&pos))
    }

    /// The visibility fill (`server::visibility`, the one the server used) goes on from what
    /// arrived; the cubes it reached and the ones above them (sky light comes from above) are
    /// asked for, nearest columns first.
    fn request_visible(&mut self, budget: usize) {
        let c = content();
        let (view, hi) = (self.view, self.hi);
        let (center, r) = (view.center, view.radius);
        let def = &c.dims[self.dim as usize];
        let in_view = |p: IVec3| view.contains_in(def, p);
        let edited = std::mem::take(&mut self.edited);
        let mut seen = Seen { cubes: &self.cubes, conn: &mut self.conn };
        let (arrived, left) = self
            .vis
            .update(&mut seen, c, view, &in_view, crate::server::NEAR, &edited);
        let stale = self
            .wanted_at
            .is_none_or(|t| t.elapsed() > Duration::from_millis(20));
        if (!arrived.is_empty() || !left.is_empty() || self.wanted.is_empty() || !edited.is_empty()) && stale {
            self.wanted_at = Some(std::time::Instant::now());
            let mut list: Vec<IVec3> = self
                .vis
                .waiting
                .keys()
                .copied()
                .filter(|&p| in_view(p))
                .collect();
            // Above the view the columns go up to open sky.
            for o in &self.offsets {
                for y in center.y + r + 1..=hi {
                    list.push(IVec3::new(center.x + o.x, y, center.z + o.y));
                }
            }
            let mut have: FxHashSet<IVec3> = list.iter().copied().collect();
            let mut above = Vec::new();
            for &p in &list {
                let mut q = p + IVec3::Y;
                while q.y <= hi && !self.cubes.contains_key(&q) && have.insert(q) {
                    above.push(q);
                    q += IVec3::Y;
                }
            }
            list.extend(above);
            list.retain(|&p| self.missing(p));
            list.sort_unstable_by_key(|&p| std::cmp::Reverse(view.order(p)));
            self.wanted = list;
        }
        while self.generating.len() + self.waiting.len() < budget {
            let Some(pos) = self.wanted.pop() else { break };
            if self.missing(pos) {
                self.ask(pos);
            }
        }
    }

    /// Has cube `pos` made: on the GPU when it can, else on a worker.
    fn ask(&mut self, pos: IVec3) {
        self.ask_column(vec![pos]);
    }

    /// Has the cubes `list` of one cube column made.
    fn ask_column(&mut self, list: Vec<IVec3>) {
        let c = content();
        let (tx, epoch, dim, seed, edits) = (self.gen_tx.clone(), self.epoch, self.dim, self.seed, self.edits.clone());
        let def = &c.dims[dim as usize];
        let gpu = self
            .gpu
            .as_ref()
            .filter(|g| g.1.load(Ordering::Relaxed))
            .map(|g| g.0.clone());
        let (on_gpu, on_cpu): (Vec<IVec3>, Vec<IVec3>) = list
            .into_iter()
            .partition(|p| gpu.is_some() && self.gpu_able && p.y >= def.min_cube_y && p.y <= def.max_cube_y);
        self.generating.extend(on_gpu.iter().chain(&on_cpu));
        if let Some(gpu) = gpu {
            for pos in on_gpu {
                let _ = gpu.send(super::terrain_gpu::Job { epoch, dim, seed, pos, edits: edits.clone(), done: tx.clone() });
            }
        }
        for pos in on_cpu {
            let (tx, edits) = (tx.clone(), edits.clone());
            crate::background().spawn(move || {
                let mut cube = crate::server::dim::generate(c, dim, seed, pos);
                crate::server::dim::prepare(c, dim, &edits, pos, &mut cube);
                let _ = tx.send((epoch, pos, cube));
            });
        }
    }

    fn made(&mut self, (epoch, pos, mut cube): Made) {
        if epoch != self.epoch || !self.generating.remove(&pos) {
            return;
        }
        if self.server.contains(&pos) || self.saved.contains(&pos) || !self.in_view(pos) {
            return;
        }
        // Edits that came while it was being made.
        let lit = !area::catch_up(&self.edits, pos, &mut cube);
        self.waiting.insert(pos, (cube, lit));
    }

    /// Inserts generated cubes whose cube above is there (or never coming). Each run down a
    /// column is lit on a worker, top down, against what is already here: light from the cubes
    /// around comes in, its own spreads inside. Then, here, what it gives the cubes around
    /// (the ones put in alongside included) spreads out to them.
    fn insert_waiting(&mut self) {
        let tops: Vec<IVec3> = self
            .waiting
            .keys()
            .copied()
            .filter(|&p| {
                let above = p + IVec3::Y;
                !self.waiting.contains_key(&above) && !self.generating.contains(&above)
            })
            .collect();
        let mut runs = Vec::new();
        for top in tops {
            let mut run = Vec::new();
            let mut p = top;
            while let Some((cube, lit)) = self.waiting.remove(&p) {
                if !self.server.contains(&p) && self.in_view(p) {
                    run.push((p, cube, lit));
                }
                p -= IVec3::Y;
            }
            if !run.is_empty() {
                runs.push(run);
            }
        }
        if runs.is_empty() {
            return;
        }
        use rayon::prelude::*;
        let c: &'static Content = content();
        let def = &c.dims[self.dim as usize];
        let (cubes, hi) = (&self.cubes, self.hi);
        let lit: Vec<Vec<(IVec3, Cube)>> = pool().install(|| {
            runs.into_par_iter()
                .map(|run| light_run(c, def, cubes, hi, run))
                .collect()
        });
        let mut new = Vec::new();
        for (p, cube) in lit.into_iter().flatten() {
            self.cubes.insert(p, Arc::new(cube));
            self.held.insert(p);
            new.push(p);
        }
        let read = Read(&self.cubes);
        let seeds: Vec<(IVec3, u8, u8)> = pool().install(|| {
            new.par_iter()
                .fold(Vec::new, |mut out, &p| {
                    crate::world::light::outward(&read, c, def.sky_light, p, &mut out);
                    out
                })
                .flatten()
                .collect()
        });
        let mut map = Map(&mut self.cubes);
        let mut lighter = Lighter::new(&mut map, c, def.sky_light);
        for ch in 0..4u8 {
            lighter.spread(ch as usize, seeds.iter().filter(|s| s.1 == ch).map(|s| (s.0, s.2)));
        }
        let changed = std::mem::take(&mut lighter.changed);
        crate::diag::TERRAIN_JOINED.add(new.len() as u64);
        // What may go now: these, held ones around them, and handed over ones whose light
        // changed. Looking at the 26 around each is done on every core.
        let this = &*self;
        let around: Vec<IVec3> = pool().install(|| {
            new.par_iter()
                .flat_map_iter(|&p| {
                    (0..27)
                        .filter(|&i| i != 13)
                        .map(move |i| p + crate::world::cube::around(i))
                })
                .filter(|n| this.held.contains(n))
                .collect()
        });
        let mut look: FxHashSet<IVec3> = new.into_iter().chain(around).chain(changed).collect();
        look.retain(|p| self.cubes.contains_key(p));
        let this = &*self;
        let ready: Vec<(IVec3, Option<Around>)> = pool().install(|| look.par_iter().map(|&p| (p, this.readiness(p))).collect());
        for (p, r) in ready {
            match r {
                Some(around) => self.hand_over(p, around),
                None => {
                    self.held.insert(p);
                }
            }
        }
    }

    /// Joins the light of a cube the server sent (complete already) with its neighbors; the
    /// other cubes whose light changed.
    fn light_sent(&mut self, pos: IVec3) -> FxHashSet<IVec3> {
        let c: &Content = content();
        let mut map = Sent { cubes: &mut self.cubes, server: &self.server, sent: pos };
        let mut lighter = Lighter::new(&mut map, c, c.dims[self.dim as usize].sky_light);
        lighter.light_prelit(pos);
        let mut changed = std::mem::take(&mut lighter.changed);
        changed.remove(&pos);
        changed
    }

    fn emit_all(&mut self, set: FxHashSet<IVec3>) {
        for p in set {
            self.emit(p);
        }
    }

    /// Hands a cube to the client world once the 26 around it are here (or never coming): its
    /// light still changes with each of them that comes, and the client meshes it once. The
    /// ones never coming go with it.
    fn emit(&mut self, pos: IVec3) {
        if !self.cubes.contains_key(&pos) {
            return;
        }
        match self.readiness(pos) {
            Some(around) => self.hand_over(pos, around),
            None => {
                self.held.insert(pos);
            }
        }
    }

    /// The cubes around `pos` not to wait for, or `None` while one in view is still to come and
    /// it was not handed over yet.
    fn readiness(&self, pos: IVec3) -> Option<Around> {
        let def = &content().dims[self.dim as usize];
        let mut around = Around::default();
        for i in 0..27u32 {
            let n = pos + crate::world::cube::around(i as i32);
            if i == 13 || n.y < def.min_cube_y || n.y > def.max_cube_y || self.cubes.contains_key(&n) {
                continue;
            }
            // Outside the ground of a column not asked for whole yet: above it is sky for now,
            // and under it nothing to see.
            let col = IVec2::new(n.x, n.z);
            if let Some(&(lo, hi)) = self.bands.get(&col).filter(|_| !self.done.contains(&col)) {
                if n.y > hi {
                    around.sky |= 1 << i;
                    continue;
                }
                if n.y < lo {
                    around.hidden |= 1 << i;
                    continue;
                }
            }
            if self.in_view(n) && (!self.vis_on || self.vis.seen.contains(&n)) {
                if self.held.contains(&pos) {
                    return None;
                }
            } else {
                around.hidden |= 1 << i;
            }
        }
        Some(around)
    }

    fn hand_over(&mut self, pos: IVec3, around: Around) {
        self.held.remove(&pos);
        self.out
            .push(Out::Cube { pos, cube: self.cubes[&pos].clone(), around });
    }

    /// A cube just put in: handed over if it can be, and so are held ones around it that
    /// waited only for it.
    fn arrived(&mut self, pos: IVec3) {
        self.emit(pos);
        for i in 0..27 {
            let n = pos + crate::world::cube::around(i);
            if i != 13 && self.held.contains(&n) {
                self.emit(n);
            }
        }
    }
}
