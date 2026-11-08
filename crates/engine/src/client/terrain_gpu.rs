//! Terrain made on the GPU (`shaders/terrain.wgsl`, its layout in `gpu_layout::terrain`): the dimension's terrain description and a
//! batch of cube positions go up, blocks, biomes and columns come back a couple of frames later,
//! and the rest of generation (the dimension's generator, trees, ores, structures, light) is
//! finished on the CPU workers. Integer math only on both sides, so a cube is the same bits
//! whoever made it (`core:terrain_check` compares every one).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crossbeam_channel::{Receiver, Sender};
use glam::IVec3;
use weft_abi::{TerrainColumn, expr as ek};
use weft_noise::fixed::seed32;
use weft_render::{Backend, BufferId, Cmd, CommandList, ComputeDesc, PipelineId, usage};

use crate::content::{Content, content};
use crate::server::area::{self, AreaFill};
use crate::server::terrain::Expr;
use crate::world::Cube;

use crate::gpu_layout::terrain::{self as layout, binding, d, words};

const OUT_WORDS: usize = words::OUT as usize;
/// Words per job: the cube's position and one unused.
const JOB_WORDS: usize = 4;
/// Cubes per frame at most.
const BATCH: usize = 4096;
/// Distant terrain tiles per frame at most (`lod`).
const LOD_BATCH: usize = 64;

pub static GPU_CUBES: AtomicU64 = AtomicU64::new(0);
pub static GPU_MISMATCHES: AtomicU64 = AtomicU64::new(0);

/// A cube for the GPU to make.
pub struct Job {
    pub epoch: u64,
    pub dim: u32,
    pub seed: u64,
    pub pos: IVec3,
    pub edits: Arc<Vec<AreaFill>>,
    pub done: Sender<super::terrain::Made>,
}

/// What `terrain.wgsl` reads for dimension `dim`; `None` when the GPU cannot make it.
pub fn describe(c: &Content, dim: u32, seed: u64) -> Option<Vec<i32>> {
    let def = &c.dims[dim as usize];
    let t = def.terrain.as_ref().filter(|_| def.builtin.is_none())?;
    if c.states.len() > 65536 {
        return None;
    }
    let mut w = vec![0i32; d::LEN as usize];
    let mut put = |at: u32, v: i32| w[at as usize] = v;
    put(d::STONE, t.stone as i32);
    put(d::BASE, t.base);
    if let Some((level, water)) = t.sea {
        put(d::HAS_SEA, 1);
        put(d::SEA, level);
        put(d::WATER, water as i32);
    }
    if let Some(caves) = &t.caves {
        put(d::HAS_CAVES, 1);
        put(d::LAVA, caves.lava as i32);
        put(d::LAVA_FROM, caves.lava_from);
        put(d::LAVA_TO, caves.lava_to);
    }
    put(d::FIELDS, t.fields.len() as i32);
    put(d::FEATURES, t.features.len() as i32);
    put(d::RULES, t.rules.len() as i32);
    put(d::CAVE_BIOMES, t.cave_biomes.len() as i32);
    for (i, (_, salt)) in layout::SEEDS.iter().enumerate() {
        put(d::SEEDS + i as u32, seed32(seed, *salt) as i32);
    }
    // The records of fields, features and rules; their steps and conditions go after them.
    w[d::FIELD_OFF as usize] = w.len() as i32;
    let at = w.len();
    let fw = words::FIELD as usize;
    w.resize(at + t.fields.len() * fw, 0);
    for (i, f) in t.fields.iter().enumerate() {
        let (start, len) = steps(&mut w, &f.expr, seed);
        w[at + i * fw..at + (i + 1) * fw].copy_from_slice(&[start, len]);
    }
    w[d::FEAT_OFF as usize] = w.len() as i32;
    let at = w.len();
    let fw = words::FEATURE as usize;
    w.resize(at + t.features.len() * fw, 0);
    for (i, f) in t.features.iter().enumerate() {
        let (height, mask) = (steps(&mut w, &f.height, seed), f.mask.as_ref().map_or((0, 0), |m| steps(&mut w, m, seed)));
        w[at + i * fw..at + (i + 1) * fw].copy_from_slice(&[height.0, height.1, mask.0, mask.1]);
    }
    w[d::RULE_OFF as usize] = w.len() as i32;
    let at = w.len();
    let rw = words::RULE as usize;
    w.resize(at + t.rules.len() * rw, 0);
    let opt = |v: Option<u32>| v.map_or(-1, |v| v as i32);
    for (i, r) in t.rules.iter().enumerate() {
        let conditions = w.len() as i32;
        for &(field, from, to) in &r.ranges {
            w.extend_from_slice(&[field as i32, from, to]);
        }
        let under = r.under.map_or((-1, 0), |(b, n)| (b as i32, n));
        let deep = r.deep_top.map_or((-1, 0), |(b, y)| (b as i32, y));
        let bare = r.bare_above.map_or((0, 0), |y| (1, y));
        let rule: [i32; words::RULE as usize] = [
            r.biome as i32,
            r.height.0,
            r.height.1,
            conditions,
            r.ranges.len() as i32,
            opt(r.top),
            opt(r.filler),
            r.depth,
            under.0,
            under.1,
            deep.0,
            deep.1,
            opt(r.ice),
            bare.0,
            bare.1,
        ];
        w[at + i * rw..at + (i + 1) * rw].copy_from_slice(&rule);
    }
    w[d::CAVE_OFF as usize] = w.len() as i32;
    for (k, b) in t.cave_biomes.iter().enumerate() {
        let cave: [i32; words::CAVE as usize] =
            [b.biome as i32, b.depth, b.size, b.threshold, seed32(seed, weft_noise::fixed::salt::CAVE_BIOME ^ k as u64) as i32];
        w.extend_from_slice(&cave);
    }
    Some(w)
}

/// Appends the steps of `e` (the noise seeds made from `seed`); returns where they start and how many.
fn steps(w: &mut Vec<i32>, e: &Expr, seed: u64) -> (i32, i32) {
    let start = w.len() as i32;
    for s in &e.0 {
        let noisy = matches!(s.kind, ek::NOISE | ek::SITE);
        w.extend_from_slice(&[s.kind as i32, if noisy { seed32(seed, s.salt) as i32 } else { 0 }, s.a, s.b, s.c]);
    }
    (start, e.0.len() as i32)
}

/// `describe` for distant terrain, with the dimension's scattered things (trees) after it
/// (`d::SCATTER_OFF` points there): per scatter its cell, reach and hash seed, then per biome the chance
/// as a threshold on the site's bits (`dim::scatter_site`'s test, exactly), and the canopy its
/// shape callback makes there (radius, top above the site, block), from a few sample runs; then
/// the ground it stands on (`ScatterDef::ground`) as state ranges (count, then first and count
/// each), none for any.
pub fn describe_lod(c: &Content, dim: u32, seed: u64) -> Option<Vec<i32>> {
    let mut w = describe(c, dim, seed)?;
    let def = &c.dims[dim as usize];
    w[d::SCATTER_OFF as usize] = w.len() as i32;
    w.push(def.scatters.len() as i32);
    let biomes = c.biomes.len().max(1);
    let mut buf = vec![weft_abi::ScatterBlock { offset: [0; 3].into(), state: 0, force: 0 }; 4096];
    for sc in &def.scatters {
        let key = seed ^ sc.salt;
        w.extend_from_slice(&[sc.cell, sc.reach, key as u32 as i32, (key >> 32) as u32 as i32, biomes as i32]);
        for b in 0..biomes as u32 {
            let density = sc
                .density
                .and_then(|d| c.components.get::<f32>(crate::components::BIOME, b, d))
                .unwrap_or(0.0);
            let threshold = (density as f64 * (1u64 << 53) as f64).ceil() as u64;
            let (mut tops, mut radii, mut states) = (Vec::new(), Vec::new(), rustc_hash::FxHashMap::<u32, u32>::default());
            for k in 0..8u64 {
                let mut args = weft_abi::ScatterArgs {
                    seed,
                    dim,
                    pos: [0, 64, 0].into(),
                    biome: b,
                    random: weft_noise::mix(k),
                    out: buf.as_mut_ptr(),
                    cap: buf.len() as u32,
                    len: 0,
                };
                if threshold == 0 || !crate::host::invoke(sc.owner, sc.shape, &mut args) {
                    continue;
                }
                let blocks = &buf[..(args.len as usize).min(buf.len())];
                let Some(top) = blocks.iter().map(|b| b.offset.y).max() else { continue };
                tops.push(top);
                radii.push(
                    blocks
                        .iter()
                        .filter(|b| b.offset.y >= 1)
                        .map(|b| b.offset.x.abs().max(b.offset.z.abs()))
                        .max()
                        .unwrap_or(0),
                );
                for b in blocks.iter().filter(|b| b.force == 0) {
                    *states.entry(b.state).or_default() += 1;
                }
            }
            if tops.is_empty() {
                w.extend_from_slice(&[0; words::SCATTER_BIOME as usize]);
                continue;
            }
            tops.sort_unstable();
            radii.sort_unstable();
            let state = states.into_iter().max_by_key(|e| e.1).map_or(0, |e| e.0);
            let entry: [i32; words::SCATTER_BIOME as usize] =
                [threshold as u32 as i32, (threshold >> 32) as u32 as i32, radii[radii.len() / 2], tops[tops.len() / 2], state as i32];
            w.extend_from_slice(&entry);
        }
        // The states its ground takes, as runs (first, count); none: any.
        let runs = sc
            .ground
            .map_or(Vec::new(), |m| crate::refs::runs(c.matchers.set(m), c.states.len() as u32));
        w.push(runs.len() as i32);
        for (first, count) in runs {
            w.extend_from_slice(&[first as i32, count as i32]);
        }
    }
    Some(w)
}

#[derive(Default)]
struct Slot {
    desc: Option<(BufferId, usize)>,
    desc_key: Option<(u32, u64)>,
    jobs: Option<(BufferId, usize)>,
    /// Two, so the next batch goes into one while what came back in the other is read.
    out: [Option<(BufferId, usize)>; 2],
    /// Which `out` the batch in flight writes.
    flight_out: usize,
    flight: Vec<Job>,
    /// What came back, while workers still read it.
    reading: [Option<Arc<Readback>>; 2],
    /// The same for distant terrain tiles (`lod`), read back right away (a few kilobytes each).
    lod_desc: Option<(BufferId, usize)>,
    lod_desc_key: Option<(u32, u64)>,
    lod_jobs: Option<(BufferId, usize)>,
    lod_out: Option<(BufferId, usize)>,
    lod_flight: Vec<super::lod::LodJob>,
}

/// A frame slot's output buffer as workers read it, straight from where the GPU wrote it.
struct Readback(*const u32, usize);

// The buffer is neither written nor freed while anyone holds this (`Slot::reading`).
unsafe impl Send for Readback {}
unsafe impl Sync for Readback {}

impl Readback {
    fn cube(&self, i: usize) -> &[u32] {
        assert!((i + 1) * OUT_WORDS <= self.1);
        unsafe { std::slice::from_raw_parts(self.0.add(i * OUT_WORDS), OUT_WORDS) }
    }
}

/// The main thread's end: dispatches batches into the frame and hands what came back to the
/// workers.
pub struct GpuTerrain {
    pipeline: Option<PipelineId>,
    lod_pipeline: Option<PipelineId>,
    rx: Receiver<Job>,
    queue: VecDeque<Job>,
    /// Distant terrain tiles asked for (`lod::Lod`).
    pub lod_tx: Sender<super::lod::LodJob>,
    lod_rx: Receiver<super::lod::LodJob>,
    lod_queue: VecDeque<super::lod::LodJob>,
    slots: Vec<Slot>,
    /// (dim, seed) and its description, for cubes and for tiles.
    desc: Option<((u32, u64), Vec<i32>)>,
    lod_desc: Option<((u32, u64), Vec<i32>)>,
    /// Set once the pipeline works: the terrain thread sends jobs here instead of to the CPU.
    pub on: Arc<AtomicBool>,
}

impl GpuTerrain {
    pub fn new(gpu: &mut dyn Backend, assets: &crate::assets::Assets, rx: Receiver<Job>, on: Arc<AtomicBool>) -> GpuTerrain {
        // Not a shader packs may replace: it has to make the CPU's blocks exactly.
        let pipelines = crate::packs::engine_shader(assets, "terrain.wgsl", &layout::prelude())
            .map_err(|e| e.0)
            .and_then(|s| {
                let terrain = gpu
                    .create_compute(&ComputeDesc { label: "terrain", shader: &s, entry: "generate" })
                    .map_err(|e| e.0)?;
                let lod = gpu
                    .create_compute(&ComputeDesc { label: "terrain lod", shader: &s, entry: "lod" })
                    .map_err(|e| e.0)?;
                Ok((terrain, lod))
            });
        let (pipeline, lod_pipeline) = match pipelines {
            Ok((p, l)) => (Some(p), Some(l)),
            Err(e) => {
                log::error!("terrain on the GPU unavailable, generating on the CPU: {e}");
                (None, None)
            }
        };
        on.store(pipeline.is_some() && enabled(), Ordering::Relaxed);
        let (lod_tx, lod_rx) = crossbeam_channel::unbounded();
        GpuTerrain {
            pipeline,
            lod_pipeline,
            rx,
            queue: VecDeque::new(),
            lod_tx,
            lod_rx,
            lod_queue: VecDeque::new(),
            slots: Vec::new(),
            desc: None,
            lod_desc: None,
            on,
        }
    }

    /// Frees its buffers, once the workers reading them are done.
    pub fn destroy(&mut self, gpu: &mut dyn Backend) {
        for s in self.slots.drain(..) {
            while s.reading.iter().flatten().any(|r| Arc::strong_count(r) > 1) {
                std::thread::yield_now();
            }
            for (b, _) in [s.desc, s.jobs, s.out[0], s.out[1], s.lod_desc, s.lod_jobs, s.lod_out]
                .into_iter()
                .flatten()
            {
                gpu.destroy_buffer(b);
            }
        }
        for p in [self.pipeline.take(), self.lod_pipeline.take()]
            .into_iter()
            .flatten()
        {
            gpu.destroy_pipeline(p);
        }
    }

    /// Takes in what the GPU finished in this slot's last frame and records the next batches.
    pub fn frame(&mut self, gpu: &mut dyn Backend, list: &mut CommandList, slot: usize) {
        let Some(pipeline) = self.pipeline else { return };
        self.on.store(enabled(), Ordering::Relaxed);
        while self.slots.len() < gpu.frames_in_flight() {
            self.slots.push(Slot::default());
        }
        self.cubes(gpu, list, slot, pipeline);
        self.tiles(gpu, list, slot);
    }

    /// Distant terrain tiles: what came back goes to the workers to mesh, the next ones go up.
    fn tiles(&mut self, gpu: &mut dyn Backend, list: &mut CommandList, slot: usize) {
        use super::lod::LOD_WORDS_PER_TILE;
        let Some(pipeline) = self.lod_pipeline else { return };
        let s = &mut self.slots[slot];
        if !s.lod_flight.is_empty() {
            let (out, _) = s.lod_out.unwrap();
            let words: &[u32] = bytemuck::cast_slice(&gpu.mapped(out)[..s.lod_flight.len() * LOD_WORDS_PER_TILE * 4]);
            for (i, job) in s.lod_flight.drain(..).enumerate() {
                super::lod::made(job, words[i * LOD_WORDS_PER_TILE..(i + 1) * LOD_WORDS_PER_TILE].to_vec());
            }
        }
        self.lod_queue.extend(self.lod_rx.try_iter());
        let Some(first) = self.lod_queue.front() else { return };
        let key = (first.dim, first.seed);
        if self.lod_desc.as_ref().is_none_or(|d| d.0 != key) {
            let Some(words) = describe_lod(content(), key.0, key.1) else {
                self.lod_queue.clear();
                return;
            };
            self.lod_desc = Some((key, words));
        }
        let (_, words) = self.lod_desc.as_ref().unwrap();
        let mut batch = Vec::new();
        while batch.len() < LOD_BATCH
            && self
                .lod_queue
                .front()
                .is_some_and(|j| (j.dim, j.seed) == key)
        {
            batch.push(self.lod_queue.pop_front().unwrap());
        }
        let mut jobs = vec![0i32; batch.len() * JOB_WORDS];
        for (i, j) in batch.iter().enumerate() {
            let o = j.key.origin();
            jobs[i * JOB_WORDS..i * JOB_WORDS + 3].copy_from_slice(&[o.x, o.y, j.key.level as i32]);
        }
        let desc = sized(gpu, &mut s.lod_desc, words.len() * 4, usage::STORAGE | usage::HOST, "terrain lod desc");
        if s.lod_desc_key != Some(key) {
            gpu.mapped(desc)[..words.len() * 4].copy_from_slice(bytemuck::cast_slice(words));
            s.lod_desc_key = Some(key);
        }
        let jobs_buf = sized(gpu, &mut s.lod_jobs, LOD_BATCH * JOB_WORDS * 4, usage::STORAGE | usage::HOST, "terrain lod jobs");
        gpu.mapped(jobs_buf)[..jobs.len() * 4].copy_from_slice(bytemuck::cast_slice(&jobs));
        let out = sized(gpu, &mut s.lod_out, LOD_BATCH * LOD_WORDS_PER_TILE * 4, usage::STORAGE | usage::READBACK, "terrain lod out");
        list.push(Cmd::SetPipeline(pipeline));
        list.bind(binding::DESC, desc);
        list.bind(binding::JOBS, jobs_buf);
        list.bind(binding::OUT, out);
        list.push_constants(&[batch.len() as u32]);
        list.push(Cmd::Dispatch { x: batch.len() as u32, y: 1, z: 1 });
        list.push(Cmd::Barrier);
        s.lod_flight = batch;
    }

    /// Cubes: what came back goes to the workers to finish, the next batch goes up.
    fn cubes(&mut self, gpu: &mut dyn Backend, list: &mut CommandList, slot: usize, pipeline: PipelineId) {
        let s = &mut self.slots[slot];
        if !s.flight.is_empty() {
            let (out, _) = s.out[s.flight_out].unwrap();
            let words: &[u32] = bytemuck::cast_slice(&gpu.mapped(out)[..s.flight.len() * OUT_WORDS * 4]);
            let back = Arc::new(Readback(words.as_ptr(), words.len()));
            // A column's cubes come together: what they share is worked out first, once.
            let mut columns: rustc_hash::FxHashMap<(i32, i32), Vec<(usize, Job)>> = Default::default();
            for (i, job) in s.flight.drain(..).enumerate() {
                columns
                    .entry((job.pos.x, job.pos.z))
                    .or_default()
                    .push((i, job));
            }
            // Unpacked on threads of their own (the other pools are busy while loading): the
            // slot waits for it.
            for ((x, z), jobs) in columns {
                let back = back.clone();
                unpacking().spawn(move || {
                    let made: Vec<(Job, Vec<u32>)> = jobs
                        .into_iter()
                        .map(|(i, job)| (job, back.cube(i).to_vec()))
                        .collect();
                    drop(back);
                    crate::server::dim::warm_scatters(content(), made[0].0.dim, made[0].0.seed, x, z);
                    for (job, words) in made {
                        crate::background().spawn(move || finish(job, unpack(&words)));
                    }
                });
            }
            s.reading[s.flight_out] = Some(back);
        }
        for r in &mut s.reading {
            if r.as_ref().is_some_and(|r| Arc::strong_count(r) == 1) {
                *r = None;
            }
        }
        let Some(next) = (0..2).find(|&k| s.reading[k].is_none()) else { return };
        self.queue.extend(self.rx.try_iter());
        let Some(first) = self.queue.front() else { return };
        let key = (first.dim, first.seed);
        if self.desc.as_ref().is_none_or(|d| d.0 != key) {
            let Some(words) = describe(content(), key.0, key.1) else {
                // The terrain thread asked for a dimension the GPU cannot make: done by the CPU.
                for job in self.queue.drain(..) {
                    crate::background().spawn(move || cpu(job));
                }
                return;
            };
            self.desc = Some((key, words));
        }
        let (_, words) = self.desc.as_ref().unwrap();
        let mut batch = Vec::new();
        while batch.len() < BATCH && self.queue.front().is_some_and(|j| (j.dim, j.seed) == key) {
            batch.push(self.queue.pop_front().unwrap());
        }
        let mut jobs = vec![0i32; batch.len() * JOB_WORDS];
        for (i, j) in batch.iter().enumerate() {
            jobs[i * JOB_WORDS..i * JOB_WORDS + 3].copy_from_slice(&j.pos.to_array());
        }
        let desc = sized(gpu, &mut s.desc, words.len() * 4, usage::STORAGE | usage::HOST, "terrain desc");
        if s.desc_key != Some(key) {
            gpu.mapped(desc)[..words.len() * 4].copy_from_slice(bytemuck::cast_slice(words));
            s.desc_key = Some(key);
        }
        let jobs_buf = sized(gpu, &mut s.jobs, BATCH * JOB_WORDS * 4, usage::STORAGE | usage::HOST, "terrain jobs");
        gpu.mapped(jobs_buf)[..jobs.len() * 4].copy_from_slice(bytemuck::cast_slice(&jobs));
        let out = sized(gpu, &mut s.out[next], BATCH * OUT_WORDS * 4, usage::STORAGE | usage::READBACK, "terrain out");
        s.flight_out = next;
        list.push(Cmd::SetPipeline(pipeline));
        list.bind(binding::DESC, desc);
        list.bind(binding::JOBS, jobs_buf);
        list.bind(binding::OUT, out);
        list.push_constants(&[batch.len() as u32]);
        list.push(Cmd::Dispatch { x: batch.len() as u32, y: 1, z: 1 });
        crate::diag::TERRAIN_GPU.add(batch.len() as u64);
        list.push(Cmd::Barrier);
        s.flight = batch;
    }
}

fn unpacking() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .thread_name(|i| format!("terrain unpack {i}"))
            .start_handler(|_| crate::lower_priority())
            .build()
            .expect("terrain unpack pool")
    })
}

fn enabled() -> bool {
    let c = content();
    c.game
        .config_names
        .get("core:terrain_gpu")
        .is_none_or(|&id| crate::config::get(id) != 0.0)
}

fn checking() -> bool {
    let c = content();
    c.game
        .config_names
        .get("core:terrain_check")
        .is_some_and(|&id| crate::config::get(id) != 0.0)
}

/// A buffer of at least `bytes`, made again when too small.
fn sized(gpu: &mut dyn Backend, slot: &mut Option<(BufferId, usize)>, bytes: usize, usage: u32, label: &str) -> BufferId {
    match *slot {
        Some((b, size)) if size >= bytes => b,
        old => {
            if let Some((b, _)) = old {
                gpu.destroy_buffer(b);
            }
            let b = gpu.create_buffer(bytes.max(4) as u64, usage, label);
            *slot = Some((b, bytes));
            b
        }
    }
}

/// The CPU's way, for jobs the GPU cannot take.
fn cpu(job: Job) {
    let c = content();
    let mut cube = crate::server::dim::generate(c, job.dim, job.seed, job.pos);
    done(c, job, &mut cube);
}

/// Unpacks what the GPU made and finishes the cube like the CPU would.
/// A cube as the GPU made it: blocks, biomes, columns.
type Unpacked = (Vec<u32>, [u16; 256], [TerrainColumn; 256]);

fn unpack(w: &[u32]) -> Unpacked {
    let blocks: Vec<u32> = (0..4096)
        .map(|i| (w[i / 2] >> ((i & 1) * 16)) & 0xFFFF)
        .collect();
    let half = |at: usize, i: usize| ((w[at + i / 2] >> ((i & 1) * 16)) & 0xFFFF) as u16;
    use crate::gpu_layout::terrain::out;
    let biomes: [u16; 256] = std::array::from_fn(|i| half(out::BIOMES as usize, i));
    let cols: [TerrainColumn; 256] = std::array::from_fn(|i| TerrainColumn {
        height: w[out::HEIGHTS as usize + i] as i32,
        biome: half(out::COLUMN_BIOMES as usize, i) as u32,
    });
    (blocks, biomes, cols)
}

fn finish(job: Job, (blocks, biomes, cols): Unpacked) {
    let t = std::time::Instant::now();
    let c = content();
    let mut cube = crate::server::dim::finish_generated(c, job.dim, job.seed, job.pos, blocks, biomes, Some(Arc::new(cols)), false);
    GPU_CUBES.fetch_add(1, Ordering::Relaxed);
    if checking() {
        let want = crate::server::dim::generate(c, job.dim, job.seed, job.pos);
        let differs = (0..4096).find(|&i| cube.get(i) != want.get(i));
        let biome_differs = (0..256).find(|&i| cube.biome(i % 16, i / 16) != want.biome(i % 16, i / 16));
        // Light too, against the way it was done before (lit as it will be below).
        let mut lit = cube.clone();
        area::catch_up(&job.edits, job.pos, &mut lit);
        let mut old = lit.clone();
        crate::world::light::prelight(c, &mut lit, c.dims[job.dim as usize].sky_light);
        crate::world::light::prelight_reference(c, &mut old, c.dims[job.dim as usize].sky_light);
        let light_differs = (0..4096).find(|&i| lit.light(i) != old.light(i));
        if let Some(i) = light_differs {
            if GPU_MISMATCHES.fetch_add(1, Ordering::Relaxed) < 8 {
                log::warn!("prelight differs at cube {} block {i}: {:#x} instead of {:#x}", job.pos, lit.light(i), old.light(i));
            }
        }
        if differs.is_some() || biome_differs.is_some() {
            if GPU_MISMATCHES.fetch_add(1, Ordering::Relaxed) < 8 {
                match differs {
                    Some(i) => log::warn!("GPU terrain differs at cube {} block {i}: {} instead of {}", job.pos, cube.get(i), want.get(i)),
                    None => log::warn!("GPU terrain differs at cube {} in biomes", job.pos),
                }
            }
        }
    }
    done(c, job, &mut cube);
    crate::diag::TERRAIN_FINISHED.add(1);
    crate::diag::TERRAIN_FINISH_NS.add(t.elapsed().as_nanos() as u64);
}

fn done(c: &Content, job: Job, cube: &mut Cube) {
    crate::server::dim::prepare(c, job.dim, &job.edits, job.pos, cube);
    let _ = job
        .done
        .send((job.epoch, job.pos, std::mem::replace(cube, Cube::uniform(0, 0))));
}
