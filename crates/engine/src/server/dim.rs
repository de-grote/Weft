use std::cmp::Reverse;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::tag_bits::damage as df;
use crate::tag_bits::entity as ef;
use glam::{DVec3, IVec3};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use weft_abi::{self as abi, ItemStack, block_flags as bf, damage_cause as damage};

use super::entity::{Entities, Entity, Motion, Told, step};
use super::space::Space;
use crate::components as comps;
use crate::content::{AIR, Content, Then, When};
use crate::host::{self, CallCtx};
use crate::world::cube::VOLUME;
use crate::world::light::{self, CubeMap, Lighter};
use crate::world::{Cube, DIRS, Rng, cube_of, local_index};

const MAX_NEIGHBOR_UPDATES: usize = 1 << 16;
/// Missed ticks are made up in at most this many parts, all in one tick (`run_systems`).
const CATCH_UP_PARTS: u64 = 20;
const ITEM_LIFETIME: u64 = crate::world::TPS as u64 * 60 * 5;

pub enum DimEvent {
    /// `on`: the inventory it opened on, `None` for a menu of its own (a crafting table).
    /// `block`: the state it was opened on, 0 when opened from code.
    OpenMenu {
        player: u64,
        menu: u32,
        on: Option<crate::server::holder::Inv>,
        block: u32,
    },
    /// `killer`: the kind of what did it.
    Died {
        player: u64,
        cause: u32,
        killer: Option<u32>,
    },
    SetPermission {
        player: u64,
        level: u32,
    },
    SetWeather {
        dim: u32,
        weather: u16,
        ticks: u32,
    },

    /// Its arm swings for those who see it.
    EntitySwing {
        off: bool,
        id: u64,
    },
    /// A sprinting hit landed: the client stops sprinting.
    StopSprint {
        player: u64,
    },
    /// `Host::force_load`.
    ForceLoad {
        dim: u32,
        key: u64,
        min: IVec3,
        max: IVec3,
        mode: u32,
        ticks: u32,
    },
    /// `ToClient::Camera` to it.
    Camera {
        player: u64,
        mode: u32,
        locked: bool,
        transition: f32,
        easing: u32,
    },
    Packet {
        player: u64,
        channel: u32,
        data: Vec<u8>,
    },
    /// `dim` u32::MAX = the region's own dimension.
    Teleport {
        from: u32,
        id: u64,
        dim: u32,
        pos: DVec3,
    },
    SetTime {
        dim: u32,
        time: u64,
    },
    SkipTicks {
        dim: u32,
        ticks: u64,
    },
    /// `server::area`, done at the end of the tick across every region of `dim`.
    AreaFill {
        dim: u32,
        fill: super::area::AreaFill,
    },
    SetMode {
        player: u64,
        mode: crate::world::GameMode,
    },
    Particles {
        dim: u32,
        burst: abi::ParticleBurst,
    },
    Sound {
        dim: u32,
        sound: u32,
        pos: DVec3,
        volume: f32,
        pitch: f32,
    },
    PlayAction {
        id: u64,
        name: String,
        looping: bool,
    },
    Show {
        who: abi::Who,
        shown: crate::net::Shown,
    },
}

/// Path searches a region does per tick at most (in parallel); more wait for the next tick (a
/// spawned crowd all starting to walk at once).
const PATHS_PER_TICK: usize = 128;

pub struct Dim {
    pub id: u32,
    /// The world seed.
    pub seed: u64,
    /// The blocks being worked on: the world's, or a body's for as long as `in_frame` runs.
    pub space: Box<Space>,
    /// Whose those are: 0 for the world, else the body's entity; and where they are.
    pub frame: u64,
    pub place: crate::physics::Transform,
    /// The blocks of bodies of this region, by entity.
    pub bodies: FxHashMap<u64, Box<Space>>,
    /// Frames a mod entered (`enter_frame`), innermost last, with what was here before.
    pub entered: Vec<(u64, Box<Space>, u64, crate::physics::Transform)>,
    pub time: u64,
    /// What `/time` moved the time of day by, from `time`.
    pub day_offset: i64,
    /// The weather now (`registry::WeatherDef`).
    pub weather: u16,
    rng: Rng,
    /// Cubes whose copy on disk holds entities or ticks, written again even if nothing else
    /// changed: what left them must not come back from there.
    pub on_disk: FxHashSet<IVec3>,
    /// Cubes only machines tickets keep (`server::forced`): mobs stand still, nothing grows.
    pub idle: FxHashSet<IVec3>,
    /// Players mods see this tick (not spectators), listed once on first use.
    players: Option<Vec<u64>>,
    pub entities: Entities,
    pub events: Vec<DimEvent>,
    pub(super) next_id: Arc<AtomicU64>,
    /// Cubes whose sky light is final: open sky above, or the cube above is settled. Only these
    /// are sent, so clients never see light being filled in afterwards.
    pub settled: FxHashSet<IVec3>,
    /// Bumped whenever a cube becomes settled, so senders know when to look again.
    pub settled_gen: u64,
    /// Cubes settled since the server last looked (to send them to players).
    pub newly_settled: Vec<IVec3>,
    /// Loaded cubes per section, kept as cubes come and go (`sections` for regionizing).
    section_cubes: FxHashMap<IVec3, u32>,
    /// Gizmo shapes mods drew this tick, and the groups some player here shows (set before the
    /// tick, so mods can skip drawing nobody sees).
    pub gizmos: Vec<crate::net::GizmoShape>,
    pub gizmo_groups: Arc<[String]>,
}

fn physics_body<'a>(c: &Content, e: &Entity, built: Option<&'a Box<Space>>, delta: DVec3) -> Option<crate::physics::Body<'a>> {
    let crate::physics::Dims { half_width: hw, height: h, .. } = e.dims(c);
    let built = built.map(|s| &**s as &dyn crate::physics::Blocks);
    crate::physics::Body::of(e.id, &c.game.entities[e.kind as usize], e.pos, (hw, h), delta, built)
}

pub(super) struct Cubes<'a>(pub(super) &'a mut FxHashMap<IVec3, Arc<Cube>>);

impl CubeMap for Cubes<'_> {
    fn cube(&self, c: IVec3) -> Option<&Cube> {
        self.0.get(&c).map(|a| &**a)
    }
    fn cube_mut(&mut self, c: IVec3) -> Option<&mut Cube> {
        self.0.get_mut(&c).map(Arc::make_mut)
    }
}

/// What every cube goes through once made (generated, loaded, or on the GPU) before it joins a
/// world, on the server or a client: the area edits made since, then its light on its own as if
/// open sky were above (`light::prelight`; joining the world fixes that up).
pub fn prepare(c: &Content, dim: u32, edits: &[super::area::AreaFill], pos: IVec3, cube: &mut Cube) {
    super::area::catch_up(edits, pos, cube);
    light::prelight(c, cube, c.dims[dim as usize].sky_light);
}

pub fn generate(c: &Content, dim: u32, seed: u64, pos: IVec3) -> Cube {
    let def = &c.dims[dim as usize];
    if pos.y < def.min_cube_y || pos.y > def.max_cube_y {
        return Cube::uniform(AIR, 0);
    }
    let mut blocks = vec![0u32; VOLUME];
    let mut biomes = [0u16; 256];
    let cols = match (def.builtin, &def.terrain, def.generator) {
        (Some(f), _, _) => {
            f(c, pos, &mut blocks);
            return finish_generated(c, dim, seed, pos, blocks, biomes, None, true);
        }
        (None, None, None) => return Cube::uniform(AIR, 0),
        (None, Some(t), _) => Some(Arc::new(
            t.generate(seed, pos.to_array(), &mut blocks, &mut biomes)
                .cols,
        )),
        (None, None, Some(_)) => def.columns(seed, pos.x, pos.z),
    };
    finish_generated(c, dim, seed, pos, blocks, biomes, cols, false)
}

/// Everything generation does after the terrain (which the GPU may have made): the
/// dimension's generator, scattered things, decorations, ores and structures. `builtin`: the
/// blocks came from a built-in dimension, which has none of the first three.
#[allow(clippy::too_many_arguments)]
pub fn finish_generated(
    c: &Content,
    dim: u32,
    seed: u64,
    pos: IVec3,
    mut blocks: Vec<u32>,
    mut biomes: [u16; 256],
    cols: Option<Arc<[abi::TerrainColumn; 256]>>,
    builtin: bool,
) -> Cube {
    let def = &c.dims[dim as usize];
    if !builtin {
        let columns = cols.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
        let mut args =
            abi::GenArgs { seed, cube: pos.to_array().into(), blocks: blocks.as_mut_ptr(), biomes: biomes.as_mut_ptr(), dim, columns };
        if let Some((by, cb)) = def.generator {
            host::invoke(by, cb, &mut args);
        }
        place_scatters(c, def, dim, seed, pos, &mut blocks);
        let mut args = abi::GenArgs { blocks: blocks.as_mut_ptr(), biomes: biomes.as_mut_ptr(), ..args };
        for &(owner, cb) in &def.decorations {
            host::invoke(owner, cb, &mut args);
        }
    }
    // One look over the blocks for what is asked of them below: most cubes are all one thing.
    let max = c.states.len() as u32;
    let (top, varied) = blocks
        .iter()
        .fold((0, 0), |(top, varied), &b| (top.max(b), varied | (b ^ blocks[0])));
    if top >= max {
        for b in blocks.iter_mut().filter(|b| **b >= max) {
            *b = AIR;
        }
        host::misuse("generator", || format!("{} wrote state {top}, which does not exist (air instead)", def.name));
    }
    let uniform = varied == 0 && top < max;
    let ores = place_ores(c, def, seed, pos, &mut blocks, &biomes, uniform);
    let built = place_structures(def, seed, pos, &mut blocks);
    let mut cube =
        if uniform && !ores && !built { Cube::uniform(blocks[0], 0) } else { Cube::from_states(blocks.as_slice().try_into().unwrap()) };
    if biomes.iter().any(|&b| b != 0) {
        cube.biomes = Some(Box::new(biomes));
    }
    cube
}

use weft_noise::hash;

/// Veins start in this and the 26 neighboring cubes and wander a block per step, so the result
/// does not depend on which cube is generated first.
/// Returns whether any went in. `uniform`: the cube is all one state.
fn place_ores(
    c: &Content,
    def: &crate::content::DimensionDef,
    seed: u64,
    pos: IVec3,
    blocks: &mut [u32],
    biomes: &[u16; 256],
    uniform: bool,
) -> bool {
    let o = pos * 16;
    let mut placed = false;
    // The states in the cube: a vein has nothing to do where it could replace none of them
    // (air and water, the most of what is generated), and a look costs far less than its veins.
    let mut present: Vec<u32> = vec![blocks[0]];
    if !uniform {
        let mut last = blocks[0];
        for &b in blocks.iter() {
            if b != last {
                last = b;
                if !present.contains(&b) {
                    present.push(b);
                }
            }
        }
    }
    for ore in &def.ores {
        let reach = ore.reach();
        if o.y > ore.max_y.saturating_add(reach) || o.y + 15 < ore.min_y.saturating_sub(reach) {
            continue;
        }
        if !present
            .iter()
            .any(|&s| ore.state_in(&c.matchers, s).is_some())
        {
            continue;
        }
        for nz in pos.z - 1..=pos.z + 1 {
            for ny in pos.y - 1..=pos.y + 1 {
                for nx in pos.x - 1..=pos.x + 1 {
                    for v in 0..ore.tries() {
                        let h = hash(seed ^ ore.salt, nx as i64, ny as i64 * 31 + v as i64, nz as i64);
                        let start =
                            IVec3::new(nx * 16 + (h % 16) as i32, ny * 16 + ((h >> 4) % 16) as i32, nz * 16 + ((h >> 8) % 16) as i32);
                        // Veins starting farther away than they reach never get here.
                        let far = (o - start).max(start - o - IVec3::splat(15)).max_element();
                        if start.y > ore.max_y || start.y < ore.min_y || far > reach {
                            continue;
                        }
                        if weft_noise::unit(hash(h, 7, 7, 7)) >= ore.weight(start.y) {
                            continue;
                        }
                        for p in vein(ore, start, h) {
                            let l = p - o;
                            if l.cmpge(IVec3::ZERO).all() && l.cmplt(IVec3::splat(16)).all() {
                                let i = local_index(l);
                                let biome_ok = ore
                                    .biome
                                    .as_ref()
                                    .is_none_or(|b| crate::refs::has(b, biomes[(l.z * 16 + l.x) as usize] as u32));
                                // Vanilla's ores in caves: some of what would touch air is left out.
                                let bare = ore.exposed > 0.0
                                    && crate::world::DIRS.iter().any(|d| {
                                        let n = l + *d;
                                        n.cmpge(IVec3::ZERO).all()
                                            && n.cmplt(IVec3::splat(16)).all()
                                            && blocks[local_index(n)] == crate::content::AIR
                                    })
                                    && weft_noise::unit(hash(h, p.x as i64, p.y as i64, p.z as i64)) < ore.exposed as f64;
                                if biome_ok
                                    && !bare
                                    && let Some(s) = ore.state_in(&c.matchers, blocks[i])
                                {
                                    blocks[i] = s;
                                    placed = true;
                                    if !present.contains(&s) {
                                        present.push(s);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    placed
}

/// The blocks of one vein from `start`, decided by `h` alone, so every cube it reaches agrees.
fn vein(ore: &crate::content::OreDef, start: IVec3, h: u64) -> Vec<IVec3> {
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
    match ore.shape {
        v::SPHERE => ball(start, ore.shape_size[0], &mut out),
        v::DISK => {
            let (r, t) = (ore.shape_size[0], ore.shape_size[1].max(1.0) as i32);
            let n = r.ceil() as i32;
            for z in -n..=n {
                for x in -n..=n {
                    if ((x * x + z * z) as f32) <= r * r {
                        for y in 0..t {
                            out.push(start + IVec3::new(x, y - t / 2, z));
                        }
                    }
                }
            }
        }
        v::BLOB => {
            // Small balls along a short random line, about `size` blocks in all.
            let r = (ore.size as f32 * 3.0 / (4.0 * std::f32::consts::PI))
                .cbrt()
                .max(0.8);
            let dir = IVec3::new((h >> 20) as i32 % 3 - 1, (h >> 24) as i32 % 3 - 1, (h >> 28) as i32 % 3 - 1);
            ball(start, r * 0.8, &mut out);
            ball(start + dir, r * 0.8, &mut out);
            out.sort_unstable_by_key(|p| (p.x, p.y, p.z));
            out.dedup();
        }
        _ => {
            let mut p = start;
            for k in 0..ore.size {
                let hk = hash(h, k as i64, 0, 0);
                p += IVec3::new((hk % 3) as i32 - 1, ((hk >> 2) % 3) as i32 - 1, ((hk >> 4) % 3) as i32 - 1);
                out.push(p);
            }
        }
    }
    let reach = ore.reach();
    out.retain(|p| (*p - start).abs().max_element() <= reach);
    out
}

/// Where structure `s` stands in its grid cell (`cell` x, z), if it does there: its lowest
/// corner. Depends only on the seed and the dimension's columns, so every cube agrees.
pub fn structure_in(def: &crate::content::DimensionDef, s: &crate::content::StructureDef, seed: u64, cell: (i32, i32)) -> Option<IVec3> {
    let h = hash(seed ^ s.salt, cell.0 as i64, 7, cell.1 as i64);
    if weft_noise::unit(h) >= s.chance as f64 {
        return None;
    }
    let room = (s.spacing - s.extent.x.max(s.extent.z)).max(1) as u64;
    let (x, z) = (cell.0 * s.spacing + ((h >> 3) % room) as i32, cell.1 * s.spacing + ((h >> 29) % room) as i32);
    let col = def.column(seed, x + s.extent.x / 2, z + s.extent.z / 2)?;
    if s.biome
        .as_ref()
        .is_some_and(|b| !crate::refs::has(b, col.biome))
        || !(s.y[0]..=s.y[1]).contains(&col.height)
    {
        return None;
    }
    Some(IVec3::new(x, col.height + 1 - s.sink, z))
}

/// The site of scatter `s` in its grid cell, if it has one there: the block above the ground,
/// the biome and the site's random bits. Depends only on the seed and the columns.
pub fn scatter_site(
    def: &crate::content::DimensionDef,
    c: &Content,
    s: &crate::content::ScatterDef,
    seed: u64,
    cell: (i32, i32),
) -> Option<(IVec3, u32, u64)> {
    let h = hash(seed ^ s.salt, cell.0 as i64, 0, cell.1 as i64);
    let (x, z) = (cell.0 * s.cell + (h % s.cell as u64) as i32, cell.1 * s.cell + ((h >> 8) % s.cell as u64) as i32);
    let col = def.column(seed, x, z)?;
    if col.height == i32::MIN
        || def.sea_level().is_some_and(|l| col.height < l)
        || !(s.y[0]..=s.y[1]).contains(&(col.height + 1))
        || s.biome
            .as_ref()
            .is_some_and(|b| !crate::refs::has(b, col.biome))
    {
        return None;
    }
    if let Some(ground) = s.ground {
        // A zero surface state means there is no usable terrain surface.
        let state = def.surface_block(seed, x, z);
        if state == 0 || !c.matchers.has(ground, state) {
            return None;
        }
    }
    let density = s
        .density
        .and_then(|d| c.components.get::<f32>(comps::BIOME, col.biome, d))
        .unwrap_or(0.0);
    if weft_noise::unit(h.rotate_left(17)) >= density as f64 || def.carved(seed, x, z) {
        return None;
    }
    Some((IVec3::new(x, col.height + 1, z), col.biome, hash(h, 1, 2, 3)))
}

/// A scattered thing: where it stands and its blocks (the shape callback's, made once and
/// shared by every cube it reaches), with the corners of the box around them.
pub struct Site {
    pos: IVec3,
    blocks: Vec<abi::ScatterBlock>,
    lo: IVec3,
    hi: IVec3,
}

/// Every site whose thing may reach into cube column (`x`, `z`), from this and neighboring
/// cells: the same for every cube of the column, so worked out once for it.
fn scatter_sites(c: &Content, def: &crate::content::DimensionDef, dim: u32, seed: u64, x: i32, z: i32) -> std::sync::Arc<Vec<Site>> {
    def.scatter_cache.get(seed, x, z, || {
        let o = IVec3::new(x, 0, z) * 16;
        let mut out = Vec::new();
        let mut buf = vec![abi::ScatterBlock { offset: [0; 3].into(), state: 0, force: 0 }; 4096];
        let states = c.states.len() as u32;
        for s in &def.scatters {
            let cells = |lo: i32| (lo - s.reach).div_euclid(s.cell)..=(lo + 15 + s.reach).div_euclid(s.cell);
            for cz in cells(o.z) {
                for cx in cells(o.x) {
                    let Some((pos, biome, random)) = scatter_site(def, c, s, seed, (cx, cz)) else { continue };
                    let mut args = abi::ScatterArgs {
                        seed,
                        dim,
                        pos: pos.to_array().into(),
                        biome,
                        random,
                        out: buf.as_mut_ptr(),
                        cap: buf.len() as u32,
                        len: 0,
                    };
                    host::invoke(s.owner, s.shape, &mut args);
                    let blocks: Vec<abi::ScatterBlock> = buf[..(args.len as usize).min(buf.len())]
                        .iter()
                        .filter(|b| b.state < states && (-s.reach_y[0]..=s.reach_y[1]).contains(&b.offset.y))
                        .copied()
                        .collect();
                    let (lo, hi) = blocks.iter().fold((IVec3::MAX, IVec3::MIN), |(lo, hi), b| {
                        (lo.min(pos + crate::world::ivec(b.offset)), hi.max(pos + crate::world::ivec(b.offset)))
                    });
                    if !blocks.is_empty() {
                        out.push(Site { pos, blocks, lo, hi });
                    }
                }
            }
        }
        out
    })
}

/// Works out the scattered things of cube column (`x`, `z`) ahead of its cubes: they all need
/// them, and the ones finished meanwhile waited for whichever was working them out.
pub fn warm_scatters(c: &Content, dim: u32, seed: u64, x: i32, z: i32) {
    let def = &c.dims[dim as usize];
    if !def.scatters.is_empty() {
        scatter_sites(c, def, dim, seed, x, z);
    }
}

/// The parts of scattered things (trees) that fall into this cube, from sites in this and
/// neighboring cells.
fn place_scatters(c: &Content, def: &crate::content::DimensionDef, dim: u32, seed: u64, pos: IVec3, blocks: &mut [u32]) {
    if def.scatters.is_empty() {
        return;
    }
    let o = pos * 16;
    let sites = scatter_sites(c, def, dim, seed, pos.x, pos.z);
    for site in sites
        .iter()
        .filter(|s| s.lo.cmple(o + 15).all() && s.hi.cmpge(o).all())
    {
        let p = site.pos;
        for b in &site.blocks {
            let l = p + crate::world::ivec(b.offset) - o;
            if l.cmpge(IVec3::ZERO).all() && l.cmplt(IVec3::splat(16)).all() {
                let i = local_index(l);
                if b.force != 0 || blocks[i] == AIR {
                    blocks[i] = b.state;
                }
            }
        }
    }
}

/// Whether a scattered thing stands within its reach of column (`x`, `z`): spawn search keeps
/// players out of trees.
pub fn scattered_near(def: &crate::content::DimensionDef, c: &Content, seed: u64, x: i32, z: i32) -> bool {
    def.scatters.iter().any(|s| {
        let r = s.reach.max(1);
        let cells = |v: i32| (v - r).div_euclid(s.cell)..=(v + r).div_euclid(s.cell);
        cells(z).any(|cz| {
            cells(x).any(|cx| scatter_site(def, c, s, seed, (cx, cz)).is_some_and(|(p, ..)| (p.x - x).abs() <= r && (p.z - z).abs() <= r))
        })
    })
}

/// Structure blocks that fall into this cube.
/// Returns whether it put anything in.
fn place_structures(def: &crate::content::DimensionDef, seed: u64, pos: IVec3, blocks: &mut [u32]) -> bool {
    let o = pos * 16;
    let mut built = false;
    for s in &def.structures {
        let cells = |lo: i32, size: i32| (lo - size).div_euclid(s.spacing)..=(lo + 15).div_euclid(s.spacing);
        for cz in cells(o.z, s.extent.z) {
            for cx in cells(o.x, s.extent.x) {
                let Some(a) = structure_in(def, s, seed, (cx, cz)) else { continue };
                if a.y > o.y + 15 || a.y + s.extent.y <= o.y {
                    continue;
                }
                for &(off, state) in &s.blocks {
                    let l = a + off - o;
                    if l.cmpge(IVec3::ZERO).all() && l.cmplt(IVec3::splat(16)).all() {
                        blocks[local_index(l)] = state;
                        built = true;
                    }
                }
            }
        }
    }
    built
}

/// Appends a timer: fire time, event name and value, as the `TIMERS` component keeps them.
pub fn push_timer(out: &mut Vec<u8>, until: u64, event: &str, data: &[u8]) {
    out.extend_from_slice(&until.to_le_bytes());
    out.extend_from_slice(&(event.len().min(u16::MAX as usize) as u16).to_le_bytes());
    out.extend_from_slice(&event.as_bytes()[..event.len().min(u16::MAX as usize)]);
    out.extend_from_slice(&(data.len().min(u16::MAX as usize) as u16).to_le_bytes());
    out.extend_from_slice(&data[..data.len().min(u16::MAX as usize)]);
}

pub fn read_timers(mut b: &[u8]) -> Vec<(u64, String, Vec<u8>)> {
    let mut out = Vec::new();
    let take = |b: &mut &[u8], n: usize| -> Option<Vec<u8>> {
        let (head, tail) = (b.get(..n)?.to_vec(), b.get(n..)?);
        *b = tail;
        Some(head)
    };
    loop {
        let Some(until) = take(&mut b, 8).map(|v| u64::from_le_bytes(v.try_into().unwrap())) else { break };
        let Some(n) = take(&mut b, 2).map(|v| u16::from_le_bytes([v[0], v[1]]) as usize) else { break };
        let Some(name) = take(&mut b, n).map(|v| String::from_utf8_lossy(&v).into_owned()) else { break };
        let Some(n) = take(&mut b, 2).map(|v| u16::from_le_bytes([v[0], v[1]]) as usize) else { break };
        let Some(data) = take(&mut b, n) else { break };
        out.push((until, name, data));
    }
    out
}

/// Face connectivity through blocks that do not hide what is behind them.
pub fn see_through(c: &Content, cube: &Cube) -> u16 {
    cube.connectivity(|s| c.states[s as usize].flags & bf::OPAQUE == 0)
}

/// The share of a block a player's mining does in a tick, which client and server each add up as it
/// goes: vanilla's five times as slow with the head in a liquid, and again off the ground, and its
/// slower rate without a tool that harvests it. All at once in creative or at 1 and up, nothing for
/// what can't be broken.
pub fn mining_rate(c: &Content, state: u32, tool: &ItemStack, head_wet: bool, on_ground: bool, speed: f32, creative: bool) -> f32 {
    let s = &c.states[state as usize];
    if s.hardness < 0.0 {
        return 0.0;
    }
    if creative || s.hardness == 0.0 {
        return 1.0;
    }
    let tool_speed = if c.tool_fits(state, tool) {
        c.stack_value::<f32>(tool, c.ids.mining_speed)
            .unwrap_or(1.0)
    } else {
        1.0
    };
    let secs = s.hardness * if c.harvests(state, tool) { 1.5 } else { 5.0 } / tool_speed;
    let secs = secs * if head_wet { 5.0 } else { 1.0 } * if on_ground { 1.0 } else { 5.0 } / speed;
    crate::world::TICK as f32 / secs
}

/// What an entity's move brought on it, applied after the parallel part.
struct TickHit {
    id: u64,
    land: Option<f32>,
    /// How far it fell before landing, and whether it fell into a liquid: for `fall_effects`.
    fell: Option<f32>,
    splash: bool,
    burn: bool,
    crushed: bool,
    /// Hazards it meets (`content::Hazard`): the state and which of its.
    met: Vec<(u32, usize)>,
}

/// The hazards entity `e` meets, by state and which of its: those in the cells its box is in or
/// a sixteenth from, at its eyes, under its feet. What isn't a body meets only those it is in.
fn hazards_met(c: &Content, block: &impl crate::physics::Cells, e: &super::entity::Entity) -> Vec<(u32, usize)> {
    use abi::hazard_at as at;
    const GROW: f64 = 1.0 / 16.0;
    let (b, body) = (e.aabb(c), e.is_body());
    let mut out = Vec::new();
    let look = |p: IVec3, out: &mut Vec<(u32, usize)>, f: &dyn Fn(&crate::content::Hazard, u32) -> bool| {
        let Some((main, extras)) = block.cell(p) else { return };
        for s in std::iter::once(main).chain(extras.iter().copied()) {
            for (i, h) in c.states[s as usize].hazards.iter().enumerate() {
                if f(h, s) && !out.contains(&(s, i)) {
                    out.push((s, i));
                }
            }
        }
    };
    let overlaps = |s: u32, p: IVec3, grow: f64| {
        let st = &c.states[s as usize];
        let whole = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
        let boxes: &[[f32; 6]] = if st.liquid.is_some() || st.selection.is_empty() { &whole } else { &st.selection };
        boxes
            .iter()
            .any(|x| (0..3).all(|i| p[i] as f64 + x[i] as f64 - grow < b.max[i] && b.min[i] < p[i] as f64 + x[i + 3] as f64 + grow))
    };
    let (lo, hi) = ((b.min - GROW).floor().as_ivec3(), (b.max + GROW).floor().as_ivec3());
    for y in lo.y..=hi.y {
        for z in lo.z..=hi.z {
            for x in lo.x..=hi.x {
                let p = IVec3::new(x, y, z);
                look(p, &mut out, &|h, s| match h.at {
                    at::INSIDE => overlaps(s, p, 0.0),
                    at::TOUCH => body && overlaps(s, p, GROW),
                    _ => false,
                });
            }
        }
    }
    if body {
        look((e.pos + DVec3::Y * e.eye(c)).floor().as_ivec3(), &mut out, &|h, _| h.at == at::HEAD);
        if e.on_ground {
            look((e.pos - DVec3::Y * 0.05).floor().as_ivec3(), &mut out, &|h, _| h.at == at::ON);
        }
    }
    out
}

impl Dim {
    /// Writes the fields clients show (`registry::HEALTH` ...) into their synced components when
    /// they differ, so they travel like mods' components. Absent ones stay absent while default.
    pub fn mirror_fields(&mut self, c: &Content) {
        let [health, burning, worn, shape, held, off_held, using, .., riding] = c.game.field_comps;
        let maxes: Vec<(u64, f32)> = self
            .entities
            .list
            .iter()
            .filter(|e| e.player && !e.removed)
            .map(|e| e.id)
            .collect::<Vec<_>>()
            .into_iter()
            .map(|id| (id, self.max_health(c, id)))
            .collect();
        let Entities { list, comps, inventories, .. } = &mut self.entities;
        for e in list.iter().filter(|e| e.in_world()) {
            let put = |comps: &mut FxHashMap<(u64, u32), Box<[u8]>>, comp: u32, bytes: &[u8]| match comps.get_mut(&(e.id, comp)) {
                Some(v) if **v != *bytes => *v = bytes.into(),
                Some(_) => {}
                None if *c.game.components[comp as usize].default != *bytes => {
                    comps.insert((e.id, comp), bytes.into());
                }
                None => {}
            };
            let max = maxes.iter().find(|m| m.0 == e.id).map_or(0.0, |m| m.1);
            let mut hp = [0u8; 8];
            hp[..4].copy_from_slice(&e.health.max(0.0).to_le_bytes());
            hp[4..].copy_from_slice(&max.to_le_bytes());
            put(comps, health, &hp);
            put(comps, burning, &[(e.fire > 0) as u8]);
            let items: Vec<u8> = e.worn.iter().flat_map(|i| i.to_le_bytes()).collect();
            put(comps, worn, &items);
            let mut sh = [0u8; 16];
            let size = e.size.unwrap_or_default();
            for (i, v) in [size.width, size.height, size.eye].into_iter().enumerate() {
                sh[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            sh[12..].copy_from_slice(&e.look.unwrap_or(u32::MAX).to_le_bytes());
            put(comps, shape, &sh);
            let rides: Vec<u8> = e.vehicle.map_or(Vec::new(), |(m, seat)| {
                m.to_le_bytes()
                    .into_iter()
                    .chain((seat as u32).to_le_bytes())
                    .collect()
            });
            put(comps, riding, &rides);
            if e.player {
                let inv = inventories.get(&e.id).map_or(&[][..], |i| &i[..]);
                let item = crate::content::in_main_hand(c, inv, e.selected);
                let item = if item.is_empty() { 0 } else { item.item };
                put(comps, held, &item.to_le_bytes());
                let off = crate::content::in_off_hand(c, inv);
                let off = if off.is_empty() { 0 } else { off.item };
                put(comps, off_held, &off.to_le_bytes());
                let use_value = e.using.0 | (e.using.1 as u32) << 31;
                put(comps, using, &use_value.to_le_bytes());
            }
        }
    }

    pub fn new(id: u32, seed: u64, next_id: Arc<AtomicU64>) -> Dim {
        Dim {
            id,
            seed,
            space: Box::default(),
            frame: 0,
            place: Default::default(),
            bodies: FxHashMap::default(),
            entered: Vec::new(),
            time: 1000,
            day_offset: 0,
            weather: 0,
            on_disk: Default::default(),
            idle: Default::default(),
            rng: Rng(weft_noise::mix(seed ^ id as u64)),
            players: None,
            entities: Entities::default(),
            events: Vec::new(),
            next_id,
            settled: FxHashSet::default(),
            settled_gen: 0,
            newly_settled: Vec::new(),
            section_cubes: FxHashMap::default(),
            gizmos: Vec::new(),
            gizmo_groups: Arc::from([]),
        }
    }

    /// Runs `f` with the blocks of `frame`, a body's entity, being what `get`, `set` and all that
    /// work on, and positions going through `place` on their way to and from the world. For
    /// the world's while a body's are out, there is nothing to go to: this nests inward.
    pub fn in_frame<R>(&mut self, frame: u64, f: impl FnOnce(&mut Dim) -> R) -> Option<R> {
        if frame == self.frame {
            return Some(f(self));
        }
        let place = crate::physics::Transform::of(self.entities.get(frame)?.pos);
        let body = self.bodies.remove(&frame)?;
        let outer = std::mem::replace(&mut self.space, body);
        let (was_frame, was_place) = (self.frame, self.place);
        (self.frame, self.place) = (frame, place);
        let out = f(self);
        (self.frame, self.place) = (was_frame, was_place);
        self.bodies
            .insert(frame, std::mem::replace(&mut self.space, outer));
        Some(out)
    }

    /// From now on this is body `frame`'s blocks, as within `in_frame`, until `leave_frame`.
    /// False if it isn't a body here.
    pub fn enter_frame(&mut self, frame: u64) -> bool {
        let Some(e) = self.entities.get(frame) else { return false };
        let place = crate::physics::Transform::of(e.pos);
        let Some(body) = self.bodies.remove(&frame) else { return false };
        let outer = std::mem::replace(&mut self.space, body);
        self.entered.push((frame, outer, self.frame, self.place));
        (self.frame, self.place) = (frame, place);
        true
    }

    pub fn leave_frame(&mut self) {
        if let Some((frame, outer, was_frame, was_place)) = self.entered.pop() {
            (self.frame, self.place) = (was_frame, was_place);
            self.bodies
                .insert(frame, std::mem::replace(&mut self.space, outer));
        }
    }

    /// The block entity of the cell at `p`: inside a callback of a part of it, that part's (as
    /// `seen`); else of its block, else of another in it.
    pub fn anchored(&self, p: IVec3) -> Option<u64> {
        match self.space.acting {
            Some((at, part)) if at == p => self.anchored_part(crate::content::content(), p, part),
            _ => self.entities.anchored(self.frame, p),
        }
    }

    /// The block entity of the block of `state` in the cell at `p`.
    pub fn anchored_part(&self, c: &Content, p: IVec3, state: u32) -> Option<u64> {
        let kind = *c.game.block_entities.get(&c.states[state as usize].block)?;
        self.entities.anchored_kind(self.frame, p, kind)
    }

    pub fn new_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// The solid body right under feet at `pos` (a step at most), and where that body is.
    pub fn body_under(&self, c: &Content, pos: DVec3, half_width: f64) -> Option<(u64, DVec3)> {
        let bodies: Vec<crate::physics::Body> = self
            .entities
            .list
            .iter()
            .filter(|e| !e.removed && !e.player && c.game.entities[e.kind as usize].is(ef::SOLID))
            .filter_map(|e| physics_body(c, e, self.bodies.get(&e.id), DVec3::ZERO))
            .collect();
        let id = crate::physics::body_below(c, pos, half_width, &bodies, 0.6)?;
        self.entities.get(id).map(|e| (id, e.pos))
    }

    pub fn has(&self, pos: IVec3) -> bool {
        self.space.cubes.contains_key(&pos)
    }

    pub fn snapshot(&self, pos: IVec3) -> Option<Arc<Cube>> {
        self.space.cubes.get(&pos).cloned()
    }

    /// `prelit`: the cube carries light from `light::prelight`.
    pub fn insert(&mut self, c: &'static Content, pos: IVec3, mut cube: Cube, prelit: bool) {
        if self.space.cubes.contains_key(&pos) {
            return;
        }
        self.space.grid.cube_loaded(c, pos, &cube);
        self.space.signals.cube_loaded(c, pos, &cube);
        let def = &c.dims[self.id as usize];
        let open_above = light::open_above(def, pos.y);
        // Clear cubes have a cheaper path of their own in `light_new`.
        let clear = cube
            .palette()
            .iter()
            .all(|&s| c.tight[s as usize].opacity == 0);
        // The worker's light assumed open sky above.
        let prelit = prelit
            && !clear
            && (!def.sky_light
                || match light::sky_below(self.space.cubes.get(&(pos + IVec3::Y)).map(|a| &**a), open_above) {
                    light::SkyBelow::Full => true,
                    light::SkyBelow::Dark => {
                        cube.mask_light(!0xFF);
                        true
                    }
                    light::SkyBelow::Partial => false,
                });
        // What time acts on continues from when the cube was last simulated: steps due while it
        // was away run in the next tick, in order. Spreading and covered blocks only in cubes
        // that were saved with some: freshly generated terrain stays as generated until
        // something changes near it (a cliff of dirt does not start greening everywhere at once,
        // which kept every new area remeshing).
        let mut lives = Vec::new();
        let saved = cube.ticked != 0;
        let since = if saved { cube.ticked.min(self.time) } else { self.time };
        let wanted = |s: u32| if saved { c.lives(s) } else { c.grows(s) };
        if cube.palette().iter().any(|&s| wanted(s)) {
            lives.extend(
                (0..VOLUME)
                    .filter(|&i| wanted(cube.get(i)))
                    .map(|i| pos * 16 + crate::world::cube::offset(i)),
            );
        }
        for (i, parts) in cube.all_extras() {
            if parts.iter().any(|&s| wanted(s)) {
                lives.push(pos * 16 + crate::world::cube::offset(i));
            }
        }
        self.add_cube(pos, Arc::new(cube));
        for p in lives {
            self.consider(c, p, since);
        }
        let mut map = Cubes(&mut self.space.cubes);
        let mut lighter = Lighter::new(&mut map, c, def.sky_light);
        if prelit {
            lighter.light_prelit(pos);
        } else {
            lighter.light_new(pos, open_above);
        }
        let changed = std::mem::take(&mut lighter.changed);
        changed.into_iter().filter(|&p| p != pos).for_each(|p| {
            self.space.light_dirty.insert(p);
        });
        if open_above || !def.sky_light || self.settled.contains(&(pos + IVec3::Y)) {
            self.settle_down(pos);
        }
    }

    /// Drops cubes `keep` rejects; returns the ones that must be saved (modified or holding
    /// entities) together with the entities that lived in them, which leave the world.
    /// The cubes beside the one at `pos` for its distant light (`lod_tops::Around`): loaded, or
    /// as `gone` has them.
    pub fn around(&self, pos: IVec3, gone: impl Fn(IVec3) -> Option<Arc<Cube>>) -> crate::server::lod_tops::Around {
        [IVec3::NEG_X, IVec3::X, IVec3::NEG_Z, IVec3::Z].map(|d| {
            self.space
                .cubes
                .get(&(pos + d))
                .cloned()
                .or_else(|| gone(pos + d))
        })
    }

    pub fn retain_cubes(&mut self, c: &Content, keep: impl Fn(IVec3) -> bool) -> Vec<(IVec3, Arc<Cube>, Vec<u64>)> {
        let mut out = Vec::new();
        let gone: FxHashSet<IVec3> = self
            .space
            .cubes
            .keys()
            .copied()
            .filter(|&p| !keep(p))
            .collect();
        if gone.is_empty() {
            return out;
        }
        // What is on its way lands first, to be with the cube it lands in.
        self.finish_moving(crate::content::content());
        // One pass over entities and growth for all cubes leaving, not one per cube (a crowd of
        // mobs times thousands of cubes).
        let mut inside: FxHashMap<IVec3, Vec<u64>> = FxHashMap::default();
        for e in self.entities.list.iter().filter(|e| e.saved_with_cube()) {
            let cp = cube_of(e.pos.floor().as_ivec3());
            if gone.contains(&cp) {
                inside.entry(cp).or_default().push(e.id);
            }
        }
        let mut grew = false;
        let mut gone: Vec<IVec3> = gone.into_iter().collect();
        gone.sort_unstable_by_key(|p| (p.x, p.y, p.z));
        for pos in gone {
            let inside = inside.remove(&pos).unwrap_or_default();
            let mut cube = self.space.cubes.remove(&pos).unwrap();
            self.cube_gone(pos);
            self.space.grid.cube_unloaded(pos);
            self.space.signals.cube_unloaded(c, pos);
            self.settled.remove(&pos);
            self.space.conn.remove(&pos);
            // Only cubes where something happened are saved; the rest generate again the same.
            // Saved ones keep the time their growing blocks were simulated to, to catch up later
            // (untouched ones start growing again when they come back, like vanilla's).
            let grows = cube.palette().iter().any(|&s| c.lives(s));
            grew |= grows;
            let modified = self.space.modified.remove(&pos);
            let on_disk = self.on_disk.remove(&pos);
            if modified || on_disk || !inside.is_empty() || !self.ticks_in(pos).is_empty() {
                if grows {
                    Arc::make_mut(&mut cube).ticked = self.time;
                }
                out.push((pos, cube, inside));
            }
        }
        if grew {
            let cubes = &self.space.cubes;
            self.space
                .growth
                .retain(|p, _| cubes.contains_key(&cube_of(*p)));
        }
        out
    }

    /// `None` for what isn't loaded; a body has nothing that isn't, only air where it has no cube.
    pub fn get(&self, p: IVec3) -> Option<u32> {
        self.space.get(p).or((self.frame != 0).then_some(AIR))
    }

    pub fn light(&self, p: IVec3) -> u32 {
        self.space
            .cubes
            .get(&cube_of(p))
            .map_or(0, |cube| cube.light(local_index(p)))
    }

    /// 0..1 strength of the sun.
    pub fn daylight(&self, c: &Content) -> f32 {
        self.daylight_at(c, self.time)
    }

    /// `time` by the tick counter: the sky is that plus what `/time` moved it by.
    pub fn daylight_at(&self, c: &Content, time: u64) -> f32 {
        c.dims[self.id as usize].daylight(time.saturating_add_signed(self.day_offset))
    }

    /// Writes a block, relights, records the change for clients, queues neighbor updates and keeps
    /// block entities in sync with their block.
    pub fn set(&mut self, c: &'static Content, p: IVec3, state: u32) -> Option<u32> {
        self.write_block(c, p, state, false)
    }

    /// `carried`: the block is coming or going with what it has, so nothing of that is made or
    /// broken. What else is in the cell stays with the same block, a property changing; it goes
    /// with another block.
    pub(super) fn write_block(&mut self, c: &'static Content, p: IVec3, state: u32, carried: bool) -> Option<u32> {
        self.write_cell(c, p, state, None, carried)
    }

    /// The cell at `p` holds `state` and `extras` (`None`: as `write_block` says).
    pub(super) fn write_cell(&mut self, c: &'static Content, p: IVec3, state: u32, extras: Option<Vec<u32>>, carried: bool) -> Option<u32> {
        if !c.dims[self.id as usize].editable {
            return None;
        }
        let cp = cube_of(p);
        if self.frame != 0 && state != AIR {
            self.space.structure_cube(c, cp);
        }
        let cube = Arc::make_mut(self.space.cubes.get_mut(&cp)?);
        let i = local_index(p);
        let had = cube.extras(i).to_vec();
        let same = state != AIR && c.states[cube.get(i) as usize].block == c.states[state as usize].block;
        let extras = extras.unwrap_or_else(|| if same { had.clone() } else { Vec::new() });
        let old = cube.set(i, state);
        cube.set_extras(i, extras.clone());
        // Clients lose what else is in a cell with its block, so it comes again after it.
        if extras != had || (old != state && !extras.is_empty()) {
            self.extras_changed(c, p, &had, extras.clone());
        }
        if !carried && (old != state || extras != had) {
            let before: Vec<u32> = std::iter::once(old).chain(had).collect();
            self.block_entities_follow(c, p, &before, &[&[state][..], &extras].concat());
        }
        if old == state {
            return Some(old);
        }
        self.space.changes.push((p, state));
        self.space.modified.insert(cp);
        self.space.conn.remove(&cp);
        self.space.edited.insert(cp);
        if self.frame != 0 {
            self.space.grow_bounds(p, state);
            self.space.relight.insert(cp);
            self.space.emitters_changed |= c.states[old as usize].light != c.states[state as usize].light;
        } else {
            let def = &c.dims[self.id as usize];
            let Space { cubes, body_light, .. } = &mut *self.space;
            let changed = light::block_changed(&mut Cubes(cubes), c, def.sky_light, body_light, p, old);
            self.space.light_dirty.extend(changed);
        }
        // The new block hears about itself too (like vanilla's onBlockAdded): a liquid starts
        // flowing, a plant without support drops, whoever set it.
        self.space.neighbor_queue.push_back((p, p));
        for d in DIRS {
            self.space.neighbor_queue.push_back((p + d, p));
        }
        self.space.grid.block_changed(c, p, old, state);
        if c.dims[self.id as usize].block_updates {
            self.space.signals.block_changed(c, p, old, state);
        }
        let from = self.space.growing_at.unwrap_or(self.time);
        // What may now happen with time: here, from now on, to the block it covers or uncovers,
        // and around a block that spreads.
        self.space.growth.remove(&p);
        self.consider(c, p, from);
        self.consider(c, p - IVec3::Y, from);
        if let Some(sp) = c.states[state as usize].spread {
            for dy in sp.min.y..=sp.max.y {
                for dz in sp.min.z..=sp.max.z {
                    for dx in sp.min.x..=sp.max.x {
                        self.consider(c, p + IVec3::new(dx, dy, dz), from);
                    }
                }
            }
        }
        if !carried && c.states[old as usize].block != c.states[state as usize].block {
            self.signal_block_gone(c, p, old, state);
        }
        Some(old)
    }

    /// Block entities go with their blocks from the cell at `p` and come with new ones, what one
    /// held dropped.
    fn block_entities_follow(&mut self, c: &Content, p: IVec3, before: &[u32], after: &[u32]) {
        let blocks = |parts: &[u32]| -> Vec<u32> { parts.iter().map(|&s| c.states[s as usize].block).collect() };
        let (was, now) = (blocks(before), blocks(after));
        for &b in was.iter().filter(|b| !now.contains(b)) {
            let Some(id) = c
                .game
                .block_entities
                .get(&b)
                .and_then(|&kind| self.entities.anchored_kind(self.frame, p, kind))
            else {
                continue;
            };
            if let Some(items) = self.entities.inventories.get(&id).cloned() {
                for s in items.into_iter().filter(|s| !s.is_empty()) {
                    self.spawn_item(c, p.as_dvec3() + DVec3::splat(0.5), s);
                }
            }
            self.entities.remove(id);
        }
        for b in now.iter().filter(|b| !was.contains(b)) {
            if let Some(&kind) = c.game.block_entities.get(b) {
                self.add_block_entity(c, p, kind);
            }
        }
    }

    /// All that is in the cell at `p`, the block `get` gives first. Air has none.
    pub fn parts(&self, p: IVec3) -> Vec<u32> {
        let Some(main) = self.get(p).filter(|&s| s != AIR) else { return Vec::new() };
        std::iter::once(main)
            .chain(self.space.extras(p).iter().copied())
            .collect()
    }

    /// Whether `part` is in the cell at `p`.
    pub fn has_part(&self, p: IVec3, part: u32) -> bool {
        self.get(p) == Some(part) || self.space.extras(p).contains(&part)
    }

    /// What else is in the cell at `p` went from `had` to `extras`: it is saved and sent, lit, and
    /// those around hear of it.
    fn extras_changed(&mut self, c: &Content, p: IVec3, had: &[u32], extras: Vec<u32>) {
        let cp = cube_of(p);
        self.space.extra_changes.push((p, extras.clone()));
        self.space.modified.insert(cp);
        self.space.edited.insert(cp);
        let glows = |parts: &[u32]| parts.iter().any(|&s| c.states[s as usize].light != [0; 3]);
        if glows(had) || glows(&extras) {
            if self.frame != 0 {
                self.space.relight.insert(cp);
                self.space.emitters_changed = true;
            } else {
                let sky = c.dims[self.id as usize].sky_light;
                let Space { cubes, body_light, .. } = &mut *self.space;
                let changed = light::parts_changed(&mut Cubes(cubes), c, sky, body_light, p);
                self.space.light_dirty.extend(changed);
            }
        }
        self.space.neighbor_queue.push_back((p, p));
        for d in DIRS {
            self.space.neighbor_queue.push_back((p + d, p));
        }
    }

    /// The block the cell at `p` stands for and what else is in it.
    fn set_cell(&mut self, c: &'static Content, p: IVec3, main: u32, extras: Vec<u32>) {
        self.write_cell(c, p, main, Some(extras), false);
    }

    /// Whether a block has what only one in a cell can: a circuit or a block entity (what moves
    /// cells carries one). That one is what the cell stands for.
    fn must_be_main(c: &Content, state: u32) -> bool {
        let s = &c.states[state as usize];
        s.signal.kind != abi::signal_kind::NONE || c.game.block_entities.contains_key(&s.block)
    }

    /// Puts another block in the cell at `p` with what is there, whatever its shape.
    pub fn add_part(&mut self, c: &'static Content, p: IVec3, state: u32) -> bool {
        let Some(main) = self.get(p) else { return false };
        if main == AIR {
            return self.set(c, p, state).is_some();
        }
        let mut extras = self.space.extras(p).to_vec();
        let first = Self::must_be_main(c, state);
        if first && (Self::must_be_main(c, main) || extras.iter().any(|&s| Self::must_be_main(c, s))) {
            return false;
        }
        if first || c.states[main as usize].liquid.is_some() {
            extras.push(main);
            self.set_cell(c, p, state, extras);
        } else {
            extras.push(state);
            self.set_cell(c, p, main, extras);
        }
        true
    }

    /// One block of the cell at `p` becomes another state, air taking it out (`cube::cell_after`).
    pub fn replace_part(&mut self, c: &'static Content, p: IVec3, old: u32, new: u32) -> bool {
        let Some(main) = self.get(p) else { return false };
        let is_liquid = |s: u32| c.states[s as usize].liquid.is_some();
        let Some((main, extras)) = crate::world::cube::cell_after(main, self.space.extras(p), old, new, is_liquid) else { return false };
        self.set_cell(c, p, main, extras);
        true
    }

    /// A callback of `part` of the cell at `p` is running: what it gets and sets there is that
    /// part, as if it were alone. When it is, there is nothing to keep apart.
    fn as_part<R>(&mut self, p: IVec3, part: u32, f: impl FnOnce(&mut Self) -> R) -> R {
        let was = self.space.acting;
        self.space.acting = (!self.space.extras(p).is_empty()).then_some((p, part));
        let r = f(self);
        self.space.acting = was;
        r
    }

    /// `get` for a mod: inside a callback of a part of a cell, that part.
    pub fn seen(&self, p: IVec3) -> Option<u32> {
        match self.space.acting {
            Some((at, part)) if at == p => Some(part),
            _ => self.get(p),
        }
    }

    /// `set` for a mod: inside a callback of a part of a cell, of that part.
    pub fn set_seen(&mut self, c: &'static Content, p: IVec3, state: u32) -> Option<u32> {
        match self.space.acting {
            Some((at, part)) if at == p => {
                self.replace_part(c, p, part, state);
                self.space.acting = Some((p, state));
                Some(part)
            }
            _ => self.set(c, p, state),
        }
    }

    /// The share of the cell's room its shapes take.
    fn fill_of(c: &Content, state: u32) -> f32 {
        c.states[state as usize]
            .selection
            .iter()
            .map(|b| (b[3] - b[0]) * (b[4] - b[1]) * (b[5] - b[2]))
            .sum()
    }

    /// Whether the source `liquid` may be in the cell at `p` along with what is in it: all of it
    /// may share a cell with that liquid (`shares_cell`), and leaves it room.
    pub(super) fn can_hold_liquid(&self, c: &Content, p: IVec3, liquid: u32) -> bool {
        let parts = self.parts(p);
        let solids = parts
            .iter()
            .filter(|&&s| c.states[s as usize].liquid.is_none());
        solids.clone().all(|&s| c.may_share(s, liquid)) && solids.map(|&s| Self::fill_of(c, s)).sum::<f32>() < 0.999
    }

    /// A liquid goes from a cell that can't hold it (any more), or that it just flowed through.
    fn drain_if_no_room(&mut self, c: &'static Content, p: IVec3) {
        let Some(liquid) = self
            .parts(p)
            .into_iter()
            .find(|&s| c.states[s as usize].liquid.is_some())
        else {
            return;
        };
        let source = c.liquids[c.states[liquid as usize].liquid.unwrap_or(0) as usize].first_state == liquid;
        if self.parts(p).len() > 1 && !(source && self.can_hold_liquid(c, p, liquid)) {
            self.replace_part(c, p, liquid, AIR);
        }
    }

    fn add_block_entity(&mut self, c: &Content, p: IVec3, kind: u32) {
        let id = self.new_id();
        let mut e = Entity::new(id, kind, self.place.to_world(p.as_dvec3() + DVec3::splat(0.5)), 1.0);
        e.anchor = Some(p);
        e.frame = (self.frame != 0).then_some(self.frame);
        self.entities.insert(c, e);
    }

    /// Whether a saved block entity still has its block: a block in the cell at its anchor has
    /// this kind, and no other entity took the place.
    pub fn block_entity_fits(&self, c: &Content, e: &Entity) -> bool {
        e.anchor.is_some_and(|a| {
            self.entities.anchored_kind(self.frame, a, e.kind).is_none()
                && self
                    .parts(a)
                    .iter()
                    .any(|s| c.game.block_entities.get(&c.states[*s as usize].block) == Some(&e.kind))
        })
    }

    /// The same for all of a body, once the ones saved with it are in.
    pub fn add_missing_on_body(&mut self, c: &Content, id: u64) {
        let cubes: Vec<IVec3> = self
            .bodies
            .get(&id)
            .map_or(Vec::new(), |s| s.cubes.keys().copied().collect());
        for cp in cubes {
            self.in_frame(id, |d| d.add_missing_block_entities(c, cp));
        }
    }

    /// Gives every block of the cube that has a block entity one, if it has none: blocks a
    /// generator placed, or a block a mod gave an entity after the world was saved.
    pub fn add_missing_block_entities(&mut self, c: &Content, pos: IVec3) {
        let Some(cube) = self.space.cubes.get(&pos).cloned() else { return };
        let has = |s: u32| {
            c.game
                .block_entities
                .contains_key(&c.states[s as usize].block)
        };
        let mains = cube.palette().iter().any(|&s| has(s));
        let parts = cube
            .all_extras()
            .filter(|(_, parts)| parts.iter().any(|&s| has(s)));
        let cells = (0..if mains { VOLUME } else { 0 })
            .map(|i| (i, cube.get(i)))
            .chain(parts.flat_map(|(i, parts)| parts.iter().map(move |&s| (i, s))));
        for (i, state) in cells {
            let Some(&kind) = c.game.block_entities.get(&c.states[state as usize].block) else { continue };
            let p = pos * 16 + crate::world::cube::offset(i);
            if self.entities.anchored_kind(self.frame, p, kind).is_none() {
                self.add_block_entity(c, p, kind);
            }
        }
    }

    /// 0 for kinds that belong to a block: those come only with their block.
    pub fn spawn(&mut self, c: &Content, kind: u32, pos: DVec3) -> u64 {
        if c.game.is_block_entity(kind) {
            return 0;
        }
        let id = self.new_id();
        let k = &c.game.entities[kind as usize];
        if let Some(blocks) = super::structure::template(c, kind) {
            self.bodies
                .insert(id, Box::new(Space::with_blocks(c, blocks)));
        }
        let mut e = Entity::new(id, kind, self.place.to_world(pos), 0.0);
        // Solid bodies collide as axis-aligned boxes, so they start square to the world.
        if !k.is(ef::SOLID) {
            e.yaw = (self.rng.float() * std::f32::consts::TAU) - std::f32::consts::PI;
        }
        self.entities.insert(c, e);
        self.add_missing_on_body(c, id);
        let max = self.max_health(c, id);
        if let Some(e) = self.entities.get_mut(id) {
            e.health = max;
        }
        id
    }

    /// The block at `p` starts falling (sand, gravel, anvils): a moving structure of that one
    /// block, drawn and colliding like any. It is placed again where it lands, or drops what it
    /// drops if it cannot be.
    pub fn fall_block(&mut self, c: &'static Content, p: IVec3) -> u64 {
        if self.get(p).is_none_or(|s| s == AIR) {
            return 0;
        }
        let block = super::structure::Lifted { from: p, local: IVec3::ZERO, to: p, part: None };
        self.lift(c, c.game.falling_kind, p.as_dvec3() + DVec3::new(0.5, 0.0, 0.5), [block], &[], super::structure::Liveness::Free)
            .0
    }

    /// Body `id` falls under its weight from now on and lands as blocks (`land_falling`).
    pub fn body_fall(&mut self, id: u64) {
        if !self.bodies.contains_key(&id) {
            return;
        }
        if let Some(e) = self
            .entities
            .get_mut(id)
            .filter(|e| e.motion != Motion::Parked)
        {
            e.falls = true;
            e.motion = Motion::Free;
        }
    }

    /// Falling bodies that came down: each block placed where it landed, on what it landed on,
    /// else dropped as loot.
    fn land_falling(&mut self, c: &'static Content) {
        let bodies = &self.bodies;
        let landed: Vec<(u64, DVec3, Vec<(IVec3, u32)>, bool, Option<u64>)> = self
            .entities
            .list
            .iter()
            .filter(|e| e.falls && !e.removed && e.motion != Motion::Parked)
            .filter(|e| e.on_ground || e.age > 600 || bodies.get(&e.id).is_none_or(|s| s.all_blocks().is_empty()))
            .map(|e| (e.id, e.pos, bodies.get(&e.id).map_or(Vec::new(), |s| s.all_blocks()), e.on_ground, e.frame))
            .collect();
        for (id, pos, blocks, ground, on) in landed {
            // One without its blocks (loaded from a save mid-fall) just goes.
            if blocks.is_empty() {
                self.entities.remove(id);
                continue;
            }
            let entities: Vec<Option<u64>> = blocks
                .iter()
                .map(|(local, _)| self.entities.anchored(id, *local))
                .collect();
            self.in_frame(on.unwrap_or(0), |d| {
                let cell = crate::physics::Transform::of(d.place.to_local(pos)).cell(None);
                let mut down = Vec::new();
                let mut broke = Vec::new();
                for ((local, state), entity) in blocks.into_iter().zip(entities) {
                    let p = cell + local;
                    let under = d.get(p);
                    let free = under.is_some_and(|s| c.states[s as usize].flags & bf::REPLACEABLE != 0 || s == AIR);
                    if ground && free && d.supported(c, p, state) {
                        if let Some(s) = under.filter(|&s| s != AIR && c.states[s as usize].liquid.is_none()) {
                            d.drop_block_loot(c, p, s, ItemStack::EMPTY, false, 0);
                        }
                        down.push(super::structure::Carried { local, to: p, state, entity });
                    } else {
                        broke.push((p, state, entity));
                    }
                }
                if down.is_empty() {
                    d.entities.remove(id);
                } else {
                    d.put_down(c, id, down, Vec::new(), None);
                }
                for (p, state, entity) in broke {
                    d.broke_landing(c, p, state, entity);
                }
            });
        }
    }

    /// How far a player's actions on blocks reach, with room for what the client saw last.
    pub fn block_reach(&self, c: &Content, player: u64) -> f64 {
        self.attribute(c, player, super::extend::BLOCK_REACH, false) + super::BLOCK_SLACK
    }

    /// `stack` thrown by `player`: from just below its eyes where it looks, not to be picked up
    /// for two seconds.
    pub fn throw(&mut self, c: &Content, player: u64, stack: ItemStack) -> u64 {
        let Some(e) = self.entities.get(player) else { return 0 };
        let (from, look) = (e.drop_point(c), crate::physics::look_dir(e.yaw, e.pitch));
        let id = self.spawn_item(c, from, stack);
        if let Some(item) = self.entities.get_mut(id) {
            item.vel = (look + DVec3::Y * 0.2) * 6.0;
            item.pickup_delay = 40;
        }
        id
    }

    pub fn spawn_item(&mut self, c: &Content, pos: DVec3, stack: ItemStack) -> u64 {
        // A liquid by the millibucket spills away instead: it only lives in liquid slots.
        if stack.is_empty() || c.item_fluid.contains_key(&stack.item) {
            return 0;
        }
        let id = self.new_id();
        let mut e = Entity::new(id, c.game.item_kind, self.place.to_world(pos), 1.0);
        e.item = stack;
        e.pickup_delay = 10;
        let (a, b) = (self.rng.float() - 0.5, self.rng.float() - 0.5);
        e.vel = DVec3::new(a as f64 * 2.0, 3.0, b as f64 * 2.0);
        self.entities.insert(c, e);
        id
    }

    /// Calls a mod callback with this dimension as its world.
    pub fn call<A>(&mut self, owner: abi::ModId, cb: abi::Callback, make: impl FnOnce(*mut abi::WorldCtx) -> A) -> A {
        let mut ctx = CallCtx { dim: self as *mut Dim };
        let mut a = make(ctx.raw());
        host::invoke(owner, cb, &mut a);
        a
    }

    fn call_block<A>(
        &mut self,
        c: &'static Content,
        state: u32,
        pick: impl Fn(&crate::content::BlockCallbacks) -> Option<(abi::ModId, abi::Callback)>,
        args: impl FnOnce(*mut abi::WorldCtx) -> A,
    ) -> Option<A> {
        let (owner, cb) = pick(&c.block_of(state).cb)?;
        Some(self.call(owner, cb, args))
    }

    /// A melee hit of `attacker` on `target`, the same for players and mobs: the `ATTACK`
    /// question, then the `ATTACK_DAMAGE` chain from the held item's attack (players) or the
    /// kind's `attack_damage` component (mobs, 1 without), then the damage. Mobs wait a second
    /// between hits, like vanilla's; players are only held back by the target's hurt time.
    pub fn attack(&mut self, c: &'static Content, attacker: u64, target: u64, held: ItemStack) -> bool {
        let (Some(me), Some(t)) = (self.entities.get(attacker), self.entities.get(target)) else { return false };
        if target == attacker || !t.is_body() || me.attack_cooldown > 0 {
            return false;
        }
        let (kind, player) = (me.kind, me.player);
        let mut q = abi::Query { entity: attacker, other: target, item: held.item, ..abi::Query::DEFAULT };
        if !self.ask(c, abi::question::ATTACK, &mut q) {
            return false;
        }
        let base = if held.item != 0 {
            c.stack_value::<f32>(&held, c.ids.attack_damage)
                .unwrap_or(1.0)
        } else if player {
            1.0
        } else {
            c.components
                .id(abi::comp::ATTACK_DAMAGE)
                .and_then(|id| c.components.get::<f32>(crate::components::ENTITY, kind, id))
                .unwrap_or(1.0)
        };
        if !player {
            // A second a hit unless its kind says otherwise.
            let own = c
                .ids
                .attack_speed
                .and_then(|id| c.components.get::<f32>(crate::components::ENTITY, kind, id))
                .unwrap_or(1.0);
            let per_second = own * self.attribute(c, attacker, super::extend::ATTACK_SPEED, false) as f32;
            if let Some(e) = self.entities.get_mut(attacker) {
                e.attack_cooldown = (crate::world::TPS as f32 / per_second.max(0.05)) as _;
            }
        }
        self.events
            .push(DimEvent::EntitySwing { id: attacker, off: false });
        let dmg = self.compute(c, abi::chain::ATTACK_DAMAGE, base as f64, &q, false) as f32;
        if !player {
            return self.damage(c, target, dmg, attacker, damage::ATTACK);
        }
        // Modern's swing: charged by time since the last, and only a full one crits or knocks. It
        // may have changed what it holds since the last tick looked.
        if let Some(me) = self.entities.get_mut(attacker) {
            me.notice_hand(held);
        }
        let strength = self.swing_strength(c, attacker, held);
        let Some(me) = self.entities.get_mut(attacker) else { return false };
        me.swing_ticks = 0;
        let full = strength > 0.9;
        let (sprinting, pos, height) = (me.sprinting, me.pos, me.height(c));
        let here = crate::physics::around(c, &*self.space, pos, height);
        let Some(me) = self.entities.get(attacker) else { return false };
        let crit = full && me.fall > 0.0 && !me.on_ground && !here.in_liquid && !here.climbing && !sprinting;
        let dmg = dmg * (0.2 + strength * strength * 0.8) * if crit { 1.5 } else { 1.0 };
        let hit = self.damage(c, target, dmg, attacker, damage::ATTACK);
        if hit && crit {
            if let Some(t) = self.entities.get(target) {
                let (pos, id) = (t.pos + DVec3::Y * (t.dims(c).height * 0.5), self.id);
                self.events.push(DimEvent::Particles {
                    dim: id,
                    burst: abi::ParticleBurst {
                        kind: c
                            .game
                            .particle_names
                            .get(abi::particle::CRIT)
                            .copied()
                            .unwrap_or(0),
                        pos: pos.to_array().into(),
                        count: 16,
                        spread: [0.3, 0.5, 0.3].into(),
                        speed: 3.0,
                        ..abi::ParticleBurst::DEFAULT
                    },
                });
            }
        }
        if hit && full && sprinting {
            // A sprint's hit knocks further, and ends the sprint.
            let away = (self.entities.get(target).map_or(pos, |t| t.pos) - pos)
                .with_y(0.0)
                .normalize_or_zero();
            self.push(target, away * 10.0 + DVec3::Y * 2.0);
            if let Some(me) = self.entities.get_mut(attacker) {
                me.sprinting = false;
            }
            self.events.push(DimEvent::StopSprint { player: attacker });
        }
        // Standing still with a sword, a full swing also sweeps what stands next to the target.
        let sweeps = c
            .ids
            .sweeps
            .is_some_and(|id| held.item != 0 && c.components.has(crate::components::ITEM, held.item, id));
        let on_ground = self.entities.get(attacker).is_some_and(|e| e.on_ground);
        if hit && full && sweeps && on_ground && !sprinting && !crit {
            let center = self.entities.get(target).map_or(pos, |t| t.pos);
            let near: Vec<u64> = self
                .entities
                .list
                .iter()
                .filter(|e| {
                    e.is_body()
                        && e.id != attacker
                        && e.id != target
                        && (e.pos - center).length_squared() <= 2.0
                        && (e.pos - pos).length_squared() <= 9.0
                })
                .map(|e| e.id)
                .collect();
            for id in near {
                self.damage(c, id, 1.0, attacker, damage::ATTACK);
            }
        }
        hit
    }

    /// How charged a player's next swing is, 0..1.
    pub fn swing_strength(&self, c: &Content, player: u64, held: ItemStack) -> f32 {
        let Some(e) = self.entities.get(player) else { return 1.0 };
        crate::content::swing_charge(c, &held, e.swing_ticks as f32, self.attribute(c, player, super::extend::ATTACK_SPEED, false) as f32)
    }

    /// What everyone near sees of a fall: dust where entity `id` landed from `fell` blocks up
    /// (vanilla's, from a fall that would hurt), and a splash where it fell into a liquid with
    /// one (`comp::SPLASH`, `comp::BUBBLE`).
    pub fn fall_effects(&mut self, c: &'static Content, id: u64, fell: Option<f32>, splash: bool) {
        let Some(e) = self.entities.get(id) else { return };
        let (pos, w) = (e.pos, e.dims(c).half_width as f32);
        let mut bursts = Vec::new();
        let safe = super::entity::SAFE_FALL;
        let under = self
            .get((pos - DVec3::Y * 0.2).floor().as_ivec3())
            .unwrap_or(AIR);
        let kind = c.states[under as usize].effects[crate::content::block_event::LAND].particle;
        if let Some(drop) = fell.filter(|&d| d > safe && under != AIR && kind != u32::MAX) {
            // Vanilla's `checkFallDamage`, and what its particles make of no speed of their own.
            let count = (150.0 * (0.2 + (drop - safe).ceil() / 15.0).min(2.5)) as u32;
            bursts.push(abi::ParticleBurst {
                kind,
                block: under,
                pos: pos.to_array().into(),
                count,
                velocity: [0.0, 2.0, 0.0].into(),
                speed: 3.6,
                outward: 1,
                ..abi::ParticleBurst::DEFAULT
            });
        }
        let liquid = splash
            .then(|| crate::physics::liquid_at(c, &*self.space, pos))
            .flatten()
            .map(|l| c.liquids[l as usize].first_state);
        if let Some(state) = liquid {
            for (comp, count) in [(abi::comp::SPLASH, 24), (abi::comp::BUBBLE, 12)] {
                let Some(&kind) = c
                    .components
                    .id(comp)
                    .and_then(|id| c.components.text(comps::BLOCK, state, id))
                    .and_then(|n| c.game.particle_names.get(n))
                else {
                    continue;
                };
                bursts.push(abi::ParticleBurst {
                    kind,
                    pos: pos.to_array().into(),
                    count,
                    spread: [w * 2.0, 0.2, w * 2.0].into(),
                    speed: 3.0,
                    velocity: [0.0, 3.0, 0.0].into(),
                    ..abi::ParticleBurst::DEFAULT
                });
            }
        }
        let dim = self.id;
        self.events.extend(
            bursts
                .into_iter()
                .map(|burst| DimEvent::Particles { dim, burst }),
        );
    }

    /// Damage to an entity, or with a negative `amount` healing: one entry for every change of
    /// health that mods may watch or change.
    pub fn damage(&mut self, c: &'static Content, id: u64, amount: f32, source: u64, cause: u32) -> bool {
        let Some(e) = self.entities.get(id) else { return false };
        let k = &c.game.entities[e.kind as usize];
        if !e.alive() || amount.is_nan() {
            return false;
        }
        if amount < 0.0 {
            return self.heal(c, id, -amount, source, cause);
        }
        let Some(def) = c.game.damage_causes.get(cause as usize) else {
            crate::host::misuse("damage", || format!("no damage cause {cause}"));
            return false;
        };
        let immune = c
            .damage_immune
            .get(cause as usize)
            .copied()
            .flatten()
            .is_some_and(|t| c.components.has(crate::components::ENTITY, e.kind, t));
        // A rule may turn the cause off (`core:damage.fall`).
        let off = c.game.damage_rules[cause as usize].is_some_and(|r| crate::config::get(r) == 0.0);
        // The void gets through everything, or /clear would silently fail.
        if immune || off || (!def.has(df::PIERCES_INVULNERABLE) && (k.is(ef::INVULNERABLE) || e.mode.invulnerable())) {
            return false;
        }
        let recovering = e.hurt_cooldown > 0 && !def.has(df::IGNORES_COOLDOWN);
        // One path: the `HURT` veto, the `DAMAGE` chain, then the `ENTITY_HURT` event.
        let mut q = abi::Query { entity: id, other: source, cause, amount: amount as f64, ..abi::Query::DEFAULT };
        if !self.ask(c, abi::action::HURT, &mut q) {
            return false;
        }
        let amount =
            self.compute(c, abi::chain::DAMAGE, amount as f64, &q, crate::log::on(crate::log::Channel::Modifiers, log::Level::Debug))
                as f32;
        if crate::log::on(crate::log::Channel::Modifiers, log::Level::Debug) {
            log::debug!(target: "modifiers", "{}", super::extend::explanation());
        }
        // While recovering only what goes beyond the last hit lands, and it doesn't knock.
        let last = self.entities.get(id).map_or(0.0, |e| e.last_hurt);
        let landing = if recovering { amount - last } else { amount };
        if landing <= 0.0 {
            return false;
        }
        let amount = landing;
        self.emit_engine(c, abi::action::HURT, &abi::Hurt { entity: id, source, amount, cause });
        let knock = self
            .entities
            .get(source)
            .filter(|_| !recovering && !def.has(df::NO_KNOCKBACK))
            .map(|s| s.pos);
        // A death mods may refuse (a totem): it lives on with one health.
        let dying = self
            .entities
            .get(id)
            .is_some_and(|e| e.health - amount <= 0.0);
        let spared = dying && {
            let mut q = abi::Query { entity: id, other: source, cause, amount: amount as f64, ..abi::Query::DEFAULT };
            !self.ask(c, abi::action::DIE, &mut q)
        };
        let Some(e) = self.entities.get_mut(id) else { return false };
        e.health = if spared { 1.0 } else { (e.health - amount).max(0.0) };
        e.last_hurt = if recovering { last + amount } else { amount };
        if !recovering {
            e.hurt_cooldown = 10;
        }
        let mut knocked = None;
        if let Some(from) = knock {
            let away = (e.pos - from)
                .with_y(0.0)
                .try_normalize()
                .unwrap_or(DVec3::X);
            if e.player {
                knocked = Some(away.as_vec3().to_array());
            } else {
                e.vel = crate::physics::knock(e.vel * crate::world::TICK, away, e.on_ground) * crate::world::TPS as f64;
            }
        }
        let (health, player, pos, kind, on_fire, height) = (e.health, e.player, e.pos, e.kind, e.fire > 0, e.height(c));
        let effects = k.effects;
        use crate::registry::entity_event;
        if health <= 0.0 || !def.has(df::SILENT) {
            self.sound(
                effects[if health <= 0.0 { entity_event::DEATH } else { entity_event::HURT }].sound,
                pos + DVec3::Y * (height * 0.5),
                1.0,
                0.0,
            );
        }
        if let Some(away) = knocked {
            self.entities.told.push((id, Told::Knock(away)));
        }
        if health <= 0.0 {
            self.entities.killed.push(id);
        }
        if health <= 0.0 {
            self.emit_engine(c, abi::action::DIE, &abi::Death { entity: id, source: source, kind, cause, pos: pos.to_array().into() });
            if !player {
                let by_player = self.entities.get(source).is_some_and(|s| s.player);
                let tool = self
                    .entities
                    .get(source)
                    .filter(|s| s.player)
                    .map(|_| self.entities.held(source))
                    .unwrap_or_default();
                let ctx = crate::loot::Ctx { tool, by_player, on_fire: on_fire || def.has(df::BURNING), ..Default::default() };
                for s in self.roll_loot(c, &k.name, &ctx, pos, source) {
                    self.spawn_item(c, pos + DVec3::Y * 0.5, s);
                }
            }
            if player {
                let killer = self.entities.get(source).map(|s| s.kind);
                self.events
                    .push(DimEvent::Died { player: id, cause, killer });
            } else {
                self.entities.remove(id);
            }
        }
        true
    }

    /// `amount` more health up to the most it has, through the `HEAL` chain; the dead stay dead.
    /// No hurt pause, knockback, sound or armor: none of damage's consequences.
    fn heal(&mut self, c: &'static Content, id: u64, amount: f32, source: u64, cause: u32) -> bool {
        let q = abi::Query { entity: id, other: source, cause, amount: amount as f64, ..abi::Query::DEFAULT };
        let amount = self.compute(c, abi::chain::HEAL, amount as f64, &q, false) as f32;
        let max = self.max_health(c, id);
        let Some(e) = self.entities.get_mut(id) else { return false };
        let gained = (e.health + amount).min(max) - e.health;
        if gained <= 0.0 || !gained.is_finite() {
            return false;
        }
        e.health += gained;
        self.emit_engine(c, abi::event::ENTITY_HEALED, &abi::Hurt { entity: id, source, amount: gained, cause });
        true
    }

    /// Far below the world's floor: vanilla's 4 damage, 64 blocks under it.
    fn fall_into_void(&mut self, c: &'static Content) {
        let floor = c.dims[self.id as usize].min_cube_y;
        if self.time % 10 != 0 || floor == i32::MIN {
            return;
        }
        let limit = (floor * 16 - 64) as f64;
        let falling: Vec<u64> = self
            .entities
            .list
            .iter()
            .filter(|e| e.in_world() && e.pos.y < limit && !e.mode.has(crate::tag_bits::game_mode::NOCLIP))
            .map(|e| e.id)
            .collect();
        for id in falling {
            self.damage(c, id, 4.0, 0, damage::VOID);
        }
    }

    /// What a dead player carried falls where it died.
    pub fn drop_inventory(&mut self, c: &Content, id: u64) {
        let Some(items) = self.entities.inventories.get(&id).cloned() else { return };
        let pos = self
            .entities
            .get(id)
            .map_or(DVec3::ZERO, |e| self.place.to_local(e.pos));
        for (i, s) in items.into_iter().enumerate() {
            if !s.is_empty() {
                let item = self.spawn_item(c, pos + DVec3::Y, s);
                // Vanilla's: flung out in a random direction, and not to be grabbed back at once.
                let (a, f) =
                    (self.random() as f64 / u64::MAX as f64 * std::f64::consts::TAU, self.random() as f64 / u64::MAX as f64 * 10.0);
                if let Some(e) = self.entities.get_mut(item) {
                    e.vel = DVec3::new(a.cos() * f, 4.0, a.sin() * f);
                    e.pickup_delay = 40;
                }
                self.entities.set_slot(id, i as u32, ItemStack::EMPTY);
            }
        }
    }

    pub fn tick(&mut self, c: &'static Content) {
        self.time += 1;
        // Whatever changes from here on changes in this tick, before signals have their turn.
        self.space.signals.now = self.time;
        self.players = None;
        self.tick_blocks_first(c);
        self.fall_into_void(c);
        self.source_modifiers(c);
        self.natural_spawns(c);
        self.tick_entities(c);
        self.tick_bodies(c);
        self.tick_blocks_after(c);
        self.entities.compact();
        self.finish_tick(c);
    }

    fn tick_blocks_first(&mut self, c: &'static Content) {
        let updates = c.dims[self.id as usize].block_updates;
        if !updates {
            self.space.scheduled.clear();
        }

        while let Some(&Reverse((at, x, y, z))) = self.space.scheduled.peek() {
            if at > self.time {
                break;
            }
            self.space.scheduled.pop();
            // The same block scheduled several times for one tick runs once.
            while self.space.scheduled.peek() == Some(&Reverse((at, x, y, z))) {
                self.space.scheduled.pop();
            }
            let p = IVec3::new(x, y, z);
            for state in self.parts(p) {
                if !self.has_part(p, state) {
                    continue;
                }
                if c.states[state as usize].liquid.is_some() {
                    self.liquid_tick(c, p, state);
                } else if !self.neighbor_rules(c, p, state, true) {
                    self.as_part(p, state, |d| {
                        d.call_block(
                            c,
                            state,
                            |b| b.on_scheduled,
                            |w| abi::BlockEventArgs { world: w, pos: p.to_array().into(), state, other: [0; 3].into(), entity: 0 },
                        )
                    });
                }
            }
        }

        if updates {
            self.grow(c);
        }
    }

    fn tick_blocks_after(&mut self, c: &'static Content) {
        let frame = self.frame;
        self.space.grid.tick(&mut self.entities, frame, c, None);
        if c.dims[self.id as usize].block_updates {
            self.land_moving(c);
            self.signal_tick(c);
        }
        self.flush_neighbors(c);
    }

    /// Bodies' blocks live like the world's. Those whose entity is gone go with it, and what
    /// belonged to them.
    fn tick_bodies(&mut self, c: &'static Content) {
        let entities = &self.entities;
        self.bodies.retain(|id, _| entities.get(*id).is_some());
        let bodies = &self.bodies;
        let orphans: Vec<u64> = self
            .entities
            .list
            .iter()
            .filter(|e| e.anchor.is_some() && e.frame.is_some_and(|f| !bodies.contains_key(&f)))
            .map(|e| e.id)
            .collect();
        for id in orphans {
            self.entities.remove(id);
        }
        for b in self.bodies.values_mut() {
            b.signals.now = self.time;
        }
        self.cross_cells(c);
        let mut ids: Vec<u64> = self.bodies.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let landed = self
                .entities
                .get(id)
                .is_some_and(|e| e.motion == Motion::Parked);
            self.in_frame(id, |d| {
                // What has landed is the world's now: it is only there to be seen.
                if !landed {
                    d.tick_blocks_first(c);
                    d.tick_blocks_after(c);
                }
                d.finish_tick(c);
            });
        }
        self.exchange_all_signals(c);
        self.forget_gone_bodies(c);
        self.drive_bodies(c);
    }

    /// Players mods see (spectators left out, so mobs neither chase nor spawn around them).
    pub fn visible_players(&mut self) -> &[u64] {
        let list = &self.entities.list;
        self.players.get_or_insert_with(|| {
            list.iter()
                .filter(|e| e.player && !e.removed && !e.mode.has(crate::tag_bits::game_mode::HIDDEN))
                .map(|e| e.id)
                .collect()
        })
    }

    /// Starts or retargets `id`'s walk to `target`. A target close to the current one only
    /// moves it (the path follows at the next look); for a new one the path is looked for in
    /// `navigation`, after the mods' tick, together with every other walk started this tick.
    pub fn navigate(&mut self, id: u64, target: DVec3, speed: f32) -> bool {
        let Some(e) = self.entities.get_mut(id) else { return false };
        if speed <= 0.0 {
            e.nav = None;
            return false;
        }
        let age = e.age;
        // On a body, somewhere on that.
        let frame = e.frame.filter(|f| self.bodies.contains_key(f));
        let place = frame
            .and_then(|f| self.entities.get(f))
            .map(|b| crate::physics::Transform::of(b.pos));
        let target = place.map_or(target, |t| t.to_local(target));
        let Some(e) = self.entities.get_mut(id) else { return false };
        if let Some(nav) = e
            .nav
            .as_mut()
            .filter(|n| n.frame == frame && (n.target - target).length_squared() < 1.0)
        {
            (nav.target, nav.speed) = (target, speed);
            return true;
        }
        e.nav = Some(super::path::Nav { target, speed, path: Vec::new(), next: 1, repath: age, found_for: None, stuck: 0, frame });
        true
    }

    /// Entities walking somewhere by themselves: a new path when the target moved off or the walk
    /// got stuck (at most once a second), steering towards the next cell, done at the target or
    /// after two seconds stuck. Steering a mod gave this
    /// tick (`walk`) wins.
    fn navigation(&mut self, c: &Content) {
        let cubes = &self.space.cubes;
        let block = super::space::CubeCells(cubes);
        let walks = |e: &Entity| e.nav.is_some() && !e.removed && !e.player && e.walk.is_none();
        // On a body its path is on that, in its coordinates.
        let bodies = &self.bodies;
        let places: FxHashMap<u64, crate::physics::Transform> = self
            .entities
            .list
            .iter()
            .filter(|e| bodies.contains_key(&e.id))
            .map(|e| (e.id, crate::physics::Transform::of(e.pos)))
            .collect();
        let on_body = |e: &Entity| {
            e.nav
                .as_ref()
                .and_then(|n| n.frame)
                .and_then(|f| places.get(&f).map(|t| (f, *t)))
        };
        let list = &mut self.entities.list;
        let due: Vec<usize> = (0..list.len())
            .filter(|&i| walks(&list[i]) && list[i].nav.as_ref().is_some_and(|n| n.stale(list[i].age)))
            .take(PATHS_PER_TICK)
            .collect();
        let found: Vec<Vec<IVec3>> = due
            .par_iter()
            .map(|&i| {
                let (e, nav) = (&list[i], list[i].nav.as_ref().unwrap());
                let height = e.height(c).ceil() as i32;
                match on_body(e) {
                    Some((f, place)) => {
                        let deck = &bodies[&f];
                        super::path::find(
                            c,
                            &super::space::DeckCells(deck),
                            place.to_local(e.pos).floor().as_ivec3(),
                            nav.target.floor().as_ivec3(),
                            height,
                        )
                    }
                    None => super::path::find(c, &block, e.pos.floor().as_ivec3(), nav.target.floor().as_ivec3(), height),
                }
            })
            .collect();
        for (&i, path) in due.iter().zip(found) {
            let e = &mut list[i];
            let nav = e.nav.as_mut().unwrap();
            nav.path = path;
            (nav.next, nav.repath, nav.found_for, nav.stuck) = (1, e.age + 20, Some(nav.target), 0);
        }
        for e in list.iter_mut().filter(|e| walks(e)) {
            let frame = e.nav.as_ref().and_then(|n| n.frame);
            let place = frame.and_then(|f| places.get(&f).copied());
            // Its body went.
            if frame.is_some() && place.is_none() {
                e.nav = None;
                continue;
            }
            let pos = place.map_or(e.pos, |t| t.to_local(e.pos));
            let nav = e.nav.as_mut().unwrap();
            nav.stuck = if e.moved < 0.02 { nav.stuck + 1 } else { 0 };
            let close = (nav.target - pos).with_y(0.0).length() < 1.0 && (nav.target.y - pos.y).abs() < 1.5;
            match super::path::steer(nav, pos, e.blocked) {
                _ if close || nav.stuck > 40 => e.nav = None,
                Some((dir, jump)) => {
                    e.yaw = (-dir[0]).atan2(-dir[1]);
                    e.walk = Some((dir, nav.speed, jump));
                }
                // The end of a path that stopped short: look again.
                None => nav.found_for = None,
            }
        }
    }

    /// Natural spawning (`abi::comp::spawn`) around this region's players every two seconds.
    fn natural_spawns(&mut self, c: &'static Content) {
        // Nothing spawns where nothing may change (the debug blocks, the void).
        if self.time % 40 != 0 || c.game.spawns.is_empty() || !c.dims[self.id as usize].editable {
            return;
        }
        let on = crate::config::get(c.game.mob_spawning) != 0.0;
        if !on {
            return;
        }
        let cats = &c.game.categories;
        let players: Vec<DVec3> = self
            .entities
            .list
            .iter()
            .filter(|e| e.player && !e.removed && e.alive())
            .map(|e| e.pos)
            .collect();
        for p in players {
            let mut count = vec![0u32; cats.len()];
            for e in self
                .entities
                .list
                .iter()
                .filter(|e| e.is_body() && !e.player)
            {
                let cat = c.game.entities[e.kind as usize].category as usize;
                if (e.pos - p).length_squared() < cats[cat].cap_radius * cats[cat].cap_radius {
                    count[cat] += 1;
                }
            }
            for _ in 0..4 {
                let a = self.rng.float() as f64 * std::f64::consts::TAU;
                let r = 20.0 + self.rng.float() as f64 * 25.0;
                let (x, z) = ((p.x + a.cos() * r).floor() as i32, (p.z + a.sin() * r).floor() as i32);
                // Per rule, the first spot of its kind with room for its mob, scanning down from
                // above the player. Unloaded counts as solid; liquids are not solid.
                let state = |y: i32| self.get(IVec3::new(x, y, z)).map(|s| &c.states[s as usize]);
                let solid = |y: i32| state(y).is_none_or(|s| s.flags & bf::SOLID != 0);
                let air = |y: i32| !solid(y) && state(y).is_some_and(|s| s.liquid.is_none());
                let wet = |y: i32| state(y).is_some_and(|s| s.liquid.is_some());
                let top = p.y.floor() as i32 + 16;
                let spot_for = |s: &crate::registry::SpawnRule| -> Option<(i32, IVec3)> {
                    use abi::spawn_place as sp;
                    let h = s.height;
                    let (y, rests_on) = match s.place {
                        sp::IN_LIQUID => {
                            let y = (top - 32..top)
                                .rev()
                                .find(|&y| (0..h).all(|k| wet(y - k)))?;
                            (y - h + 1, IVec3::new(x, y - h + 1, z))
                        }
                        sp::CEILING => {
                            let y = (top - 32..top)
                                .rev()
                                .find(|&y| solid(y + 1) && (0..h).all(|k| air(y - k)))?;
                            (y - h + 1, IVec3::new(x, y + 1, z))
                        }
                        _ => {
                            let y = (top - 32..top)
                                .rev()
                                .find(|&y| solid(y - 1) && air(y) && (1..h.max(2)).all(|k| !solid(y + k)))?;
                            (y, IVec3::new(x, y - 1, z))
                        }
                    };
                    Some((y, rests_on))
                };
                let biome_at = |spot: IVec3| {
                    self.space
                        .cubes
                        .get(&cube_of(spot))
                        .map_or(0, |cube| cube.biome((x & 15) as usize, (z & 15) as usize)) as u32
                };
                let fits = |s: &crate::registry::SpawnRule| -> Option<(i32, u32)> {
                    if count[s.category as usize] >= cats[s.category as usize].cap || !s.dims.is_empty() && !s.dims.contains(&self.id) {
                        return None;
                    }
                    let (y, rests_on) = spot_for(s)?;
                    let spot = IVec3::new(x, y, z);
                    ((s.y[0]..=s.y[1]).contains(&y)
                        && s.biome
                            .as_ref()
                            .is_none_or(|b| crate::refs::has(b, biome_at(spot))))
                    .then(|| (y, self.get(rests_on).unwrap_or(AIR)))
                };
                let spots: Vec<Option<(i32, u32)>> = c.game.spawns.iter().map(fits).collect();
                // Asked about the spot and what it rests on.
                let found: Vec<Option<i32>> = c
                    .game
                    .spawns
                    .iter()
                    .zip(spots)
                    .map(|(s, at)| {
                        let (y, ground) = at?;
                        let pos = DVec3::new(x as f64 + 0.5, y as f64, z as f64 + 0.5);
                        let ask = crate::conditions::Ask { pos, state: ground, ..Default::default() };
                        s.when
                            .is_none_or(|w| crate::conditions::on_server(self, c, w, &ask))
                            .then_some(y)
                    })
                    .collect();
                let total: u32 = c
                    .game
                    .spawns
                    .iter()
                    .zip(&found)
                    .filter(|(_, y)| y.is_some())
                    .map(|(s, _)| s.weight)
                    .sum();
                if total == 0 {
                    continue;
                }
                let mut pick = (self.rng.next() % total as u64) as u32;
                let Some((rule, y)) = c
                    .game
                    .spawns
                    .iter()
                    .zip(found)
                    .filter_map(|(s, y)| y.map(|y| (s, y)))
                    .find(|(s, _)| {
                        let hit = pick < s.weight;
                        pick = pick.saturating_sub(s.weight);
                        hit
                    })
                else {
                    continue;
                };
                if self.rng.float() >= rule.chance {
                    continue;
                }
                let n = rule.group[0] + (self.rng.next() % (rule.group[1] - rule.group[0] + 1) as u64) as u32;
                for _ in 0..n {
                    self.spawn(c, rule.entity, DVec3::new(x as f64 + 0.5, y as f64, z as f64 + 0.5));
                }
                count[rule.category as usize] += n;
            }
        }
    }

    /// Timers (`Host::entity_after`) of this region's entities that are due fire their events.
    fn fire_timers(&mut self, c: &'static Content) {
        let comp = c.game.timers_comp;
        let ids: Vec<u64> = self
            .entities
            .list
            .iter()
            .filter(|e| !e.removed)
            .map(|e| e.id)
            .filter(|id| self.entities.comps.contains_key(&(*id, comp)))
            .collect();
        for id in ids {
            let Some(bytes) = self.entities.comps.get(&(id, comp)) else { continue };
            let mut due = Vec::new();
            let mut kept = Vec::new();
            for (until, name, data) in read_timers(bytes) {
                if until <= self.time {
                    due.push((name, data));
                } else {
                    push_timer(&mut kept, until, &name, &data);
                }
            }
            if due.is_empty() {
                continue;
            }
            if kept.is_empty() {
                self.entities.comps.remove(&(id, comp));
            } else {
                self.entities
                    .comps
                    .insert((id, comp), kept.into_boxed_slice());
            }
            for (name, data) in due {
                match c.game.event_names.get(&name) {
                    Some(&e) => self.emit(c, e, &data),
                    None => log::warn!("entity {id}: a timer for event {name}, which no mod has now"),
                }
            }
        }
    }

    fn tick_entities(&mut self, c: &'static Content) {
        // Mod logic: one call per kind and callback with every entity of that kind.
        let mut by_kind: FxHashMap<(u64, u32), Vec<u64>> = FxHashMap::default();
        // Copied, mods' calls borrow the region; empty but where only machines are kept.
        let idle = (!self.idle.is_empty()).then(|| self.idle.clone());
        let still = |e: &super::entity::Entity| {
            idle.as_ref()
                .is_some_and(|i| !e.player && e.anchor.is_none() && i.contains(&cube_of(e.pos.floor().as_ivec3())))
        };
        for e in &self.entities.list {
            if !e.removed && (!e.player || e.alive()) && !still(e) {
                by_kind
                    .entry((e.block_frame(), e.kind))
                    .or_default()
                    .push(e.id);
            }
        }
        let mut kinds: Vec<(u64, u32)> = by_kind.keys().copied().collect();
        kinds.sort();
        for (frame, kind) in kinds {
            let ids = &by_kind[&(frame, kind)];
            for &(owner, cb) in &c.game.entities[kind as usize].on_tick {
                let ids_ptr = ids.as_ptr();
                let count = ids.len();
                self.in_frame(frame, |d| d.call(owner, cb, |w| abi::EntityTickArgs { world: w, kind, ids: ids_ptr, count }));
            }
        }
        self.run_systems(c);
        for &(owner, cb) in &c.game.world_ticks {
            self.call(owner, cb, |w| abi::WorldTickArgs { world: w });
        }
        self.fire_timers(c);
        self.navigation(c);
        self.steer_mounts(c);

        let gravity = c.dims[self.id as usize].gravity as f64;
        let day = self.daylight(c);
        let sheltered = c.game.weathers[self.weather as usize].shelters();
        let players: Vec<DVec3> = self
            .entities
            .list
            .iter()
            .filter(|e| e.player && !e.removed)
            .map(|e| e.pos)
            .collect();
        let cubes = &self.space.cubes;
        let block = super::space::CubeCells(cubes);
        // Solid bodies move first, in list order, so riders and whatever they run into see where
        // they ended up; a body may ride one moved before it.
        let mut bodies: Vec<crate::physics::Body> = Vec::new();
        let body_blocks = &self.bodies;
        for e in self.entities.list.iter_mut() {
            let k = &c.game.entities[e.kind as usize];
            if e.removed || e.player || !k.is(ef::SOLID) {
                continue;
            }
            let before = e.pos;
            step(c, e, gravity, &block, &bodies);
            bodies.extend(physics_body(c, e, body_blocks.get(&e.id), e.pos - before));
        }
        let bodies = &bodies;
        // What belongs to a body's blocks is where those are.
        if !self.bodies.is_empty() {
            let at: FxHashMap<u64, crate::physics::Transform> = bodies
                .iter()
                .filter_map(|b| b.blocks.map(|(_, place)| (b.id, place)))
                .collect();
            for e in self
                .entities
                .list
                .iter_mut()
                .filter(|e| e.anchor.is_some() && e.frame.is_some())
            {
                if let (Some(place), Some(a)) = (e.frame.and_then(|f| at.get(&f)), e.anchor) {
                    e.pos = place.to_world(a.as_dvec3() + DVec3::splat(0.5));
                }
            }
        }
        // Physics only reads blocks and bodies and writes its own entity, so entities move in
        // parallel; damage is applied afterwards in list order to stay deterministic.
        let inventories = &self.entities.inventories;
        let hits: Vec<TickHit> = self
            .entities
            .list
            .par_iter_mut()
            .with_min_len(64)
            .filter_map(|e| {
                if e.removed || still(e) {
                    return None;
                }
                e.age += 1;
                e.hurt_cooldown = e.hurt_cooldown.saturating_sub(1);
                e.swing_ticks = e.swing_ticks.saturating_add(1);
                e.attack_cooldown = e.attack_cooldown.saturating_sub(1);
                if e.player {
                    let inv = inventories.get(&e.id).map_or(&[][..], |i| &i[..]);
                    let held = crate::content::in_main_hand(c, inv, e.selected);
                    e.notice_hand(held);
                    // Mods saw it this tick; the next `Move` tells of the next.
                    e.jumped = false;
                }
                if e.player && (!e.alive() || e.mode.has(crate::tag_bits::game_mode::NO_INTERACT)) {
                    return None;
                }
                let k = &c.game.entities[e.kind as usize];
                let (mut land, mut fell, mut splash) = (None, None, false);
                let mut crushed = false;
                // Players move on their client, bodies moved above; the rest here.
                if !e.player && !k.is(ef::SOLID) {
                    let (wet, falling) = (e.in_liquid, -e.vel.y * crate::world::TICK);
                    let s = step(c, e, gravity, &block, bodies);
                    crushed = s.crushed;
                    (fell, splash) = (s.land, super::entity::splashes(wet, e.in_liquid, falling));
                    land = s.land.and_then(|fall| super::entity::fall_damage(k, fall));
                }
                // The liquids where the entity is now, and whether they burn; what sets it on fire
                // keeps it burning.
                let wet = [e.pos, e.pos + DVec3::Y * crate::physics::body_level(e.height(c))]
                    .iter()
                    .any(|p| {
                        crate::physics::Cells::liquid(&block, c, p.floor().as_ivec3())
                            .and_then(|s| c.states[s as usize].liquid)
                            .is_some_and(|l| !c.liquids[l as usize].burns())
                    });
                let met = hazards_met(c, &block, e);
                let hot = met
                    .iter()
                    .any(|&(s, i)| c.states[s as usize].hazards[i].fire > 0);
                // What sets things burning burns a dropped stack up, as lava and fire do in vanilla.
                if hot && !e.item.is_empty() {
                    e.removed = true;
                }
                // Sunlight sets them on fire for 8 seconds; burning hurts once a second, water puts it out.
                if k.is(ef::BURNS_IN_SUN) && day > 0.5 && !sheltered && e.age % crate::world::TPS as u64 == 0 && !e.in_liquid {
                    let head = (e.pos + DVec3::Y * e.eye(c)).floor().as_ivec3();
                    let sky = cubes
                        .get(&cube_of(head))
                        .map_or(0, |cb| abi::light::sky(cb.light(local_index(head))));
                    if sky >= 15 {
                        e.fire = e.fire.max(160);
                    }
                }
                if wet && !hot {
                    e.fire = 0;
                }
                e.fire = e.fire.saturating_sub(1);
                let burn = e.fire > 0 && e.fire % crate::world::TPS == 0 && !hot;
                let despawn = c.game.categories[k.category as usize].despawn;
                if despawn > 0.0 && e.age % 100 == 0 {
                    let near = players
                        .iter()
                        .any(|p| (*p - e.pos).length_squared() < despawn * despawn);
                    if !near {
                        e.removed = true;
                    }
                }
                if !e.item.is_empty() {
                    e.pickup_delay = e.pickup_delay.saturating_sub(1);
                    if e.age > ITEM_LIFETIME {
                        e.removed = true;
                    }
                }
                (land.is_some() || fell.is_some() || splash || burn || crushed || !met.is_empty()).then_some(TickHit {
                    id: e.id,
                    land,
                    fell,
                    splash,
                    burn,
                    crushed,
                    met,
                })
            })
            .collect();
        for (i, push) in super::entity::soft_push(c, &self.entities.list) {
            self.entities.list[i].vel += push;
        }
        self.seat_riders(c);
        for TickHit { id, land, fell, splash, burn, crushed, met } in hits {
            self.fall_effects(c, id, fell, splash);
            if let Some(amount) = land {
                self.damage(c, id, amount, 0, damage::FALL);
                self.riders_fall(c, id, amount);
            }
            if crushed {
                self.damage(c, id, 1.0, 0, damage::CRUSH);
            }
            if burn {
                self.damage(c, id, 1.0, 0, damage::FIRE);
            }
            for (state, i) in met {
                let h = c.states[state as usize].hazards[i];
                let Some(e) = self
                    .entities
                    .get(id)
                    .filter(|_| h.every <= 1 || (self.time + id) % h.every as u64 == 0)
                else {
                    continue;
                };
                let ask = crate::conditions::Ask { pos: e.pos, entity: id, state, body: Some(e.body()), ..Default::default() };
                if h.when
                    .is_some_and(|w| !crate::conditions::on_server(self, c, w, &ask))
                {
                    continue;
                }
                if let Some(e) = self.entities.get_mut(id) {
                    e.fire = e.fire.max(h.fire);
                }
                if let Some(cause) = h.cause {
                    self.damage(c, id, h.damage, 0, cause);
                }
            }
        }
        self.items(c);
        self.land_falling(c);
    }

    /// Systems over per-entity components: values copied into one aligned array, handed over
    /// in one call, written back (changes of synced components marked for sending).
    fn run_systems(&mut self, c: &'static Content) {
        if c.game.systems.is_empty() {
            return;
        }
        // Every entity a system works on remembers the time it was simulated to. One that fell
        // behind (unloaded, the time jumped) makes the missed ticks up right here, before its
        // normal tick: in at most `CATCH_UP_PARTS` calls whatever was missed, each followed by
        // the energy network moving that many ticks' worth, so machines that feed each other
        // keep pace (a generator does not fill its buffer and stop while the furnace waits).
        let (sim, now) = (c.game.simulated_comp, self.time);
        let worked: Vec<u64> = self
            .entities
            .list
            .iter()
            .filter(|e| {
                !e.removed
                    && c.game
                        .systems
                        .iter()
                        .any(|s| self.entities.comps.contains_key(&(e.id, s.1)))
            })
            .map(|e| e.id)
            .collect();
        let mut missed: Vec<(u64, u64)> = Vec::new();
        for id in worked {
            let last = self
                .entities
                .comps
                .get(&(id, sim))
                .and_then(|b| b.get(..8))
                .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
                .filter(|&t| t < now);
            if let Some(t) = last.filter(|&t| now - t > 1) {
                missed.push((id, now - t - 1));
            }
            self.entities
                .comps
                .insert((id, sim), now.to_le_bytes().into());
        }
        let mut part = FxHashMap::default();
        for k in 0..CATCH_UP_PARTS {
            part.clear();
            for &(id, n) in &missed {
                let parts = n.min(CATCH_UP_PARTS);
                if k < parts {
                    let ticks = n * (k + 1) / parts - n * k / parts;
                    part.insert(id, ticks.min(u32::MAX as u64) as u32);
                }
            }
            if part.is_empty() {
                break;
            }
            self.systems_once(c, Some(&part));
            for frame in self.frames_of(&part) {
                self.in_frame(frame, |d| {
                    let frame = d.frame;
                    d.space.grid.tick(&mut d.entities, frame, c, Some(&part));
                });
            }
        }
        self.systems_once(c, None);
    }

    /// The frames the blocks of these entities are in.
    fn frames_of(&self, only: &FxHashMap<u64, u32>) -> Vec<u64> {
        let mut frames: Vec<u64> = only
            .keys()
            .filter_map(|id| self.entities.get(*id))
            .map(|e| e.block_frame())
            .collect();
        frames.sort_unstable();
        frames.dedup();
        frames
    }

    /// Every system once: over `only` (entity, ticks) when given, else over all with one tick,
    /// and for the blocks of each frame in that frame.
    fn systems_once(&mut self, c: &'static Content, only: Option<&FxHashMap<u64, u32>>) {
        for &(owner, comp, cb) in &c.game.systems {
            let size = c.game.components[comp as usize].size as usize;
            if size == 0 {
                continue;
            }
            let mut by_frame: FxHashMap<u64, Vec<u64>> = FxHashMap::default();
            for e in self
                .entities
                .list
                .iter()
                .filter(|e| !e.removed && self.entities.comps.contains_key(&(e.id, comp)) && only.is_none_or(|o| o.contains_key(&e.id)))
            {
                by_frame.entry(e.block_frame()).or_default().push(e.id);
            }
            let mut frames: Vec<u64> = by_frame.keys().copied().collect();
            frames.sort_unstable();
            for frame in frames {
                let ids = &by_frame[&frame];
                let mut buf = vec![0u64; (ids.len() * size).div_ceil(8)];
                let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, ids.len() * size) };
                for (i, id) in ids.iter().enumerate() {
                    bytes[i * size..(i + 1) * size].copy_from_slice(&self.entities.comps[&(*id, comp)]);
                }
                let ticks: Vec<u32> = ids
                    .iter()
                    .map(|id| only.and_then(|o| o.get(id).copied()).unwrap_or(1))
                    .collect();
                let (ids_ptr, values, count, ticks) = (ids.as_ptr(), bytes.as_mut_ptr(), ids.len() as u32, ticks.as_ptr());
                self.in_frame(frame, |d| {
                    d.call(owner, cb, |w| abi::SystemArgs {
                        world: w,
                        component: comp,
                        ids: ids_ptr,
                        values,
                        count,
                        stride: size as u32,
                        ticks,
                    })
                });
                let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, ids.len() * size) };
                for (i, id) in ids.iter().enumerate() {
                    if let Some(old) = self.entities.comps.get_mut(&(*id, comp)) {
                        old.copy_from_slice(&bytes[i * size..(i + 1) * size]);
                    }
                }
            }
        }
    }

    /// Item entities: merge neighbors, get picked up by players.
    fn items(&mut self, c: &'static Content) {
        let players: Vec<(u64, DVec3)> = self
            .entities
            .list
            .iter()
            .filter(|e| e.player && !e.removed && e.alive() && !e.mode.has(crate::tag_bits::game_mode::HIDDEN))
            .map(|e| (e.id, e.pos))
            .collect();
        // Who is next to whom, once a merge is due: everyone asking everyone was the one thing
        // that took a tick over its time with thousands of items lying around.
        let merging = self.time % 10 == 0;
        let mut cells: FxHashMap<IVec3, Vec<usize>> = FxHashMap::default();
        if merging {
            for (i, e) in self
                .entities
                .list
                .iter()
                .enumerate()
                .filter(|(_, e)| !e.removed && !e.item.is_empty())
            {
                cells.entry(e.pos.floor().as_ivec3()).or_default().push(i);
            }
        }
        let items: Vec<(u64, DVec3)> = self
            .entities
            .list
            .iter()
            .filter(|e| !e.removed && !e.item.is_empty())
            .map(|e| (e.id, e.pos))
            .collect();
        for (id, pos) in items {
            let Some(e) = self.entities.get(id) else { continue };
            if e.pickup_delay > 0 {
                continue;
            }
            let stack = e.item;
            // The nearest one that has room, not just the first in reach.
            let takers = players
                .iter()
                .filter(|(_, pp)| (*pp + DVec3::Y * 0.5 - pos).length_squared() < 2.25);
            if let Some(&(pid, _)) = takers
                .filter(|(p, _)| self.entities.can_insert(c, *p, stack))
                .min_by(|a, b| {
                    (a.1 - pos)
                        .length_squared()
                        .total_cmp(&(b.1 - pos).length_squared())
                })
            {
                let mut q = abi::Query { entity: pid, other: id, item: stack.item, amount: stack.count as f64, ..abi::Query::DEFAULT };
                if !self.ask(c, abi::action::PICKUP_ITEM, &mut q) {
                    continue;
                }
                let rest = self.entities.insert_item(c, pid, stack);
                let taken = stack.count - rest.count;
                if taken > 0 {
                    let pickup = c
                        .game
                        .sound_names
                        .get("core:item.pickup")
                        .copied()
                        .unwrap_or(u32::MAX);
                    self.sound(pickup, pos, 1.0, 0.0);
                    self.emit_engine(c, abi::action::PICKUP_ITEM, &abi::Pickup { player: pid, stack: ItemStack { count: taken, ..stack } });
                }
                if let Some(e) = self.entities.get_mut(id) {
                    if rest.is_empty() {
                        e.removed = true;
                        self.entities.taken.push((id, pid));
                    } else {
                        e.item = rest;
                    }
                }
                continue;
            }
            if merging {
                let max = c.max_stack(stack.item);
                let at = pos.floor().as_ivec3();
                let list = &self.entities.list;
                let other = (-1..=1)
                    .flat_map(|dx| (-1..=1).flat_map(move |dy| (-1..=1).map(move |dz| at + IVec3::new(dx, dy, dz))))
                    .flat_map(|cell| cells.get(&cell).into_iter().flatten().copied())
                    .find(|&i| {
                        let o = &list[i];
                        o.id != id
                            && !o.removed
                            && o.item.item == stack.item
                            && o.item.data == stack.data
                            && o.item.count + stack.count <= max
                            && (o.pos - pos).length_squared() < 1.0
                    });
                if let Some(oi) = other {
                    self.entities.list[oi].item.count += stack.count;
                    if let Some(e) = self.entities.get_mut(id) {
                        e.removed = true;
                    }
                }
            }
        }
    }

    pub fn flush_neighbors(&mut self, c: &'static Content) {
        if !c.dims[self.id as usize].block_updates {
            self.space.neighbor_queue.clear();
            self.space.edge_updates.clear();
            return;
        }
        let share = self.space.edge_updates.len().min(MAX_NEIGHBOR_UPDATES / 2);
        for p in self.space.edge_updates.drain(..share) {
            self.space.neighbor_queue.push_back((p, p));
        }
        let mut budget = MAX_NEIGHBOR_UPDATES;
        while let Some((p, from)) = self.space.neighbor_queue.pop_front() {
            if budget == 0 {
                log::warn!("neighbor update storm in dimension {}, dropping {} updates", self.id, self.space.neighbor_queue.len());
                self.space.neighbor_queue.clear();
                break;
            }
            budget -= 1;
            let Some(state) = self.get(p) else { continue };
            self.neighbor_changed(c, p, state, from);
            if self.space.extras(p).is_empty() {
                continue;
            }
            for part in self.space.extras(p).to_vec() {
                if self.has_part(p, part) {
                    self.neighbor_changed(c, p, part, from);
                }
            }
        }
    }

    /// A part of the cell at `p` hears of a change at `from`.
    fn neighbor_changed(&mut self, c: &'static Content, p: IVec3, state: u32, from: IVec3) {
        if self.sides_fall(c, p, state) || self.neighbor_rules(c, p, state, false) {
            return;
        }
        for r in &c.states[state as usize].rules {
            if let When::Neighbor(d) = r.when
                && d > 0
            {
                self.schedule(p, d);
            }
        }
        self.consider(c, p, self.time);
        if c.states[state as usize].liquid.is_some() {
            self.liquid_neighbor(c, p, state);
            return;
        }
        self.as_part(p, state, |d| {
            d.call_block(
                c,
                state,
                |b| b.on_neighbor,
                |w| abi::BlockEventArgs { world: w, pos: p.to_array().into(), state, other: from.to_array().into(), entity: 0 },
            )
        });
    }

    /// The block `state` would be held up at `p`: no neighbor rule of it breaks it, and what it
    /// lies on holds (unloaded neighbors count as holding).
    fn supported(&mut self, c: &'static Content, p: IVec3, state: u32) -> bool {
        if self.sides_held(c, p, state) != c.states[state as usize].sides {
            return false;
        }
        let ask = self.ask_at(p, state);
        !c.states[state as usize]
            .rules
            .iter()
            .filter(|r| matches!(r.when, When::Neighbor(_)) && r.then == Then::Break)
            .any(|r| {
                r.cond
                    .is_none_or(|w| crate::conditions::on_server(self, c, w, &ask))
            })
    }

    /// What `state` at `p` asks its conditions about.
    /// Light counts with the daylight of the moment being caught up on, if it is one.
    fn ask_at(&self, p: IVec3, state: u32) -> crate::conditions::Ask<'static> {
        let daylight = self
            .space
            .growing_at
            .map(|at| self.daylight_at(crate::content::content(), at));
        crate::conditions::Ask {
            pos: self.place.to_world(p.as_dvec3() + DVec3::splat(0.5)),
            state,
            daylight,
            own_frame: true,
            ..Default::default()
        }
    }

    /// `part`'s neighbor rules, those at once or (`later`) those after a while: the first that
    /// holds is done. Whether one was.
    fn neighbor_rules(&mut self, c: &'static Content, p: IVec3, part: u32, later: bool) -> bool {
        let ask = self.ask_at(p, part);
        for r in &c.states[part as usize].rules {
            let When::Neighbor(d) = r.when else { continue };
            if (d > 0) == later
                && c.acts(part, r.then)
                && r.cond
                    .is_none_or(|w| crate::conditions::on_server(self, c, w, &ask))
            {
                self.act(c, p, part, r.then);
                return true;
            }
        }
        false
    }

    /// What a rule of `part` of the cell at `p` does.
    fn act(&mut self, c: &'static Content, p: IVec3, part: u32, then: Then) {
        match then {
            Then::Become(s) => {
                self.replace_part(c, p, part, s);
            }
            Then::Grow => match c.block_of(part).cb.on_grow {
                Some(_) => {
                    self.as_part(p, part, |d| {
                        d.call_block(
                            c,
                            part,
                            |b| b.on_grow,
                            |w| abi::BlockEventArgs { world: w, pos: p.to_array().into(), state: part, other: [0; 3].into(), entity: 0 },
                        )
                    });
                }
                None => {
                    if let Some(next) = c.grown(part) {
                        self.replace_part(c, p, part, next);
                    }
                }
            },
            Then::Break => self.break_off(c, p, part),
            Then::Fall => {
                self.fall_block(c, p);
            }
        }
    }

    /// Which sides of an `on_sides` block something holds up; the unloaded holds.
    fn sides_held(&self, c: &Content, p: IVec3, state: u32) -> u8 {
        let mask = c.states[state as usize].sides;
        (0..6u8)
            .filter(|&f| {
                mask & 1 << f != 0
                    && self
                        .get(p + DIRS[f as usize])
                        .is_none_or(|s| crate::wire::holds_wire(c, s))
            })
            .fold(0, |m, f| m | 1 << f)
    }

    /// Sides of an `on_sides` block that lost what held them up come off, each dropping what it
    /// would alone. Whether any did.
    fn sides_fall(&mut self, c: &'static Content, p: IVec3, state: u32) -> bool {
        let (mask, left) = (c.states[state as usize].sides, self.sides_held(c, p, state));
        if left == mask {
            return false;
        }
        let def = c.block_of(state);
        for f in (0..6u8).filter(|&f| (mask ^ left) & 1 << f != 0) {
            self.drop_block_loot(c, p, def.with_sides(state, 1 << f), ItemStack::EMPTY, false, 0);
        }
        self.block_dust(c, p, state);
        self.replace_part(c, p, state, if left == 0 { AIR } else { def.with_sides(state, left) });
        true
    }

    /// Breaks a block that lost its support, dropping what it drops.
    pub(super) fn break_off(&mut self, c: &'static Content, p: IVec3, state: u32) {
        self.replace_part(c, p, state, AIR);
        self.block_dust(c, p, state);
        self.drop_block_loot(c, p, state, ItemStack::EMPTY, false, 0);
        self.emit_engine(c, abi::action::BREAK_BLOCK, &abi::BlockChange { player: 0, pos: p.to_array().into(), state, item: 0 });
    }

    /// What happened since the last settling takes effect: neighbor updates, then events.
    /// After every player's or mod's action (`Server::settle`) and at the end of a tick.
    pub fn settle(&mut self, c: &'static Content) {
        self.flush_neighbors(c);
        self.finish_tick(c);
    }

    pub fn finish_tick(&mut self, c: &'static Content) {
        if self.frame != 0 {
            self.space.relight();
        }
        self.deliver_events(c);
        if self.frame == 0 && !self.bodies.is_empty() && !self.space.light_dirty.is_empty() {
            self.bodies_see_light();
        }
        for cp in std::mem::take(&mut self.space.light_dirty) {
            if let Some(cube) = self.space.cubes.get(&cp) {
                self.space.resend.push((cp, cube.clone()));
            }
        }
        // Clients are told of these as they happen, which goes for a landed block changing again.
        let Space { changes, extra_changes, not_shown, .. } = &mut *self.space;
        if !not_shown.is_empty() {
            for (p, _) in changes.iter() {
                not_shown.remove(p);
            }
            // What else is in a cell comes with its block, when that is shown.
            extra_changes.retain(|(p, _)| !not_shown.contains_key(p));
        }
        if !self.space.resend.is_empty() {
            // Taken now, not when the light changed: finish_tick runs after every message, and
            // the block changes dropped below must be in what is sent.
            let Space { resend, cubes, not_shown, .. } = &mut *self.space;
            for (cp, cube) in resend {
                if let Some(now) = cubes.get(cp) {
                    *cube = now.clone();
                    // Leaving out what clients aren't to see yet.
                    for (p, (was, _)) in not_shown.iter() {
                        if cube_of(*p) == *cp {
                            Arc::make_mut(cube).set(local_index(*p), *was);
                        }
                    }
                }
            }
            let resent: FxHashSet<IVec3> = self.space.resend.iter().map(|r| r.0).collect();
            self.space
                .changes
                .retain(|(p, _)| !resent.contains(&cube_of(*p)));
        }
    }

    /// What `action::BREAK_BLOCK` says about it.
    pub fn may_break(&mut self, c: &'static Content, p: IVec3, state: u32, item: u32, player: u64) -> bool {
        let mut q = abi::Query { entity: player, pos: p.to_array().into(), state, item, ..abi::Query::DEFAULT };
        self.ask(c, abi::action::BREAK_BLOCK, &mut q)
    }

    /// The surface of a liquid counts, not only the block it is in.
    pub fn head_in_liquid(&self, c: &Content, e: &super::entity::Entity) -> bool {
        crate::physics::liquid_at(c, &*self.space, e.pos + DVec3::Y * e.eye(c)).is_some()
    }

    /// The share of `part` of the cell at `pos` in `frame` a player's mining does this tick, by what
    /// it holds and where it is now.
    pub fn mining_rate_of(&mut self, c: &Content, player: u64, frame: u64, pos: IVec3, part: u32) -> f32 {
        let Some(e) = self.entities.get(player) else { return 0.0 };
        let (wet, on_ground, creative) = (self.head_in_liquid(c, e), e.on_ground, e.mode.has(crate::tag_bits::game_mode::INSTANT_BREAK));
        let tool = self.entities.held(player);
        let speed = self.attribute(c, player, super::extend::MINING_SPEED, false) as f32;
        let state = self
            .in_frame(frame, |d| d.aimed_part(c, pos, part))
            .unwrap_or(AIR);
        mining_rate(c, state, &tool, wet, on_ground, speed, creative)
    }

    /// Pushes an entity, by blocks per second. Its client moves a player, so the push goes there.
    pub fn push(&mut self, id: u64, v: DVec3) {
        let Some(e) = self.entities.get_mut(id) else { return };
        if e.player {
            let add = (v / crate::world::TPS as f64).as_vec3().to_array();
            self.entities.told.push((id, Told::Add(add)));
        } else {
            e.vel += v;
        }
    }

    /// One melee hit, with what a player holds, which wears from it. Mobs hit with their kind's damage.
    pub fn melee(&mut self, c: &'static Content, attacker: u64, target: u64) -> bool {
        let Some((player, slot)) = self.entities.get(attacker).map(|e| (e.player, e.selected)) else { return false };
        let held = if player { self.entities.held(attacker) } else { ItemStack::EMPTY };
        let hit = self.attack(c, attacker, target, held);
        // Bare hands wear nothing, whatever sits in the slot doing nothing.
        if hit && player && !held.is_empty() {
            super::actions::wear(c, self, attacker, slot, c.stack_value::<u32>(&held, c.ids.wear_per_hit).unwrap_or(2));
        }
        hit
    }

    /// `break_part` of what a player hit at `hit` on `face`: the piece hit, if the state has
    /// pieces (`StateInfo::piece_hit`), goes alone.
    #[allow(clippy::too_many_arguments)]
    pub fn break_piece(
        &mut self,
        c: &'static Content,
        p: IVec3,
        part: u32,
        face: u8,
        hit: [f32; 3],
        tool: ItemStack,
        player: u64,
        drops: bool,
    ) -> bool {
        let state = self.aimed_part(c, p, part);
        let Some(&piece) = c.states[state as usize]
            .piece_hit(hit, face)
            .filter(|piece| piece.left != AIR)
        else {
            return self.break_part(c, p, state, tool, player, drops);
        };
        if !self.may_break(c, p, piece.alone, tool.item, player) {
            return false;
        }
        self.replace_part(c, p, state, piece.left);
        self.block_dust(c, p, piece.alone);
        self.sound(c.states[state as usize].effects[crate::content::block_event::BREAK].sound, p.as_dvec3() + DVec3::splat(0.5), 1.0, 0.0);
        if drops {
            self.drop_block_loot(c, p, piece.alone, tool, true, 0);
        }
        self.emit_engine(
            c,
            abi::action::BREAK_BLOCK,
            &abi::BlockChange { player, pos: p.to_array().into(), state: piece.alone, item: tool.item },
        );
        true
    }

    /// A client's picture of a cell it was wrong about is put right.
    pub(super) fn resend_cell(&mut self, p: IVec3) {
        if let Some(state) = self.get(p) {
            self.space.changes.push((p, state));
            self.space
                .extra_changes
                .push((p, self.space.extras(p).to_vec()));
        }
    }

    /// The part a client says it aimed at, if it is there and can be; the block of the cell if not.
    pub fn aimed_part(&self, c: &Content, p: IVec3, part: u32) -> u32 {
        let can = self.has_part(p, part)
            && c.states
                .get(part as usize)
                .is_some_and(|s| s.flags & bf::NOT_TARGETABLE == 0);
        if can { part } else { self.get(p).unwrap_or(AIR) }
    }

    /// `break_block` of all that is in the cell that can be aimed at: a liquid in it stays.
    pub fn break_block(&mut self, c: &'static Content, p: IVec3, tool: ItemStack, player: u64, drops: bool) -> bool {
        let Some(main) = self.get(p) else { return false };
        if self.space.extras(p).is_empty() {
            return self.break_part(c, p, main, tool, player, drops);
        }
        let mut any = false;
        for part in self
            .parts(p)
            .into_iter()
            .filter(|&s| c.states[s as usize].flags & bf::NOT_TARGETABLE == 0)
        {
            any |= self.break_part(c, p, part, tool, player, drops);
        }
        any
    }

    /// Breaks one block of the cell at `p`.
    pub fn break_part(&mut self, c: &'static Content, p: IVec3, state: u32, tool: ItemStack, player: u64, drops: bool) -> bool {
        // A piston's head is broken by breaking the piston.
        if let Some(base) = self.head_of_piston(c, p, state) {
            return self.break_block(c, base, tool, player, drops);
        }
        if state == AIR || c.states[state as usize].hardness < 0.0 {
            return false;
        }
        if !self.may_break(c, p, state, tool.item, player) {
            return false;
        }
        // What the block entity carries goes back onto the block's item.
        let carried = self.anchored_part(c, p, state).map_or(0, |id| {
            crate::stack::intern(
                c.game
                    .carried
                    .iter()
                    .filter_map(|&comp| {
                        self.entities
                            .comps
                            .get(&(id, comp))
                            .map(|v| (comp, v.clone()))
                    })
                    .filter(|(_, v)| !v.is_empty())
                    .collect(),
            )
        });
        self.replace_part(c, p, state, AIR);
        self.block_dust(c, p, state);
        self.sound(c.states[state as usize].effects[crate::content::block_event::BREAK].sound, p.as_dvec3() + DVec3::splat(0.5), 1.0, 0.0);
        if drops {
            self.drop_block_loot(c, p, state, tool, true, carried);
        }
        self.emit_engine(c, abi::action::BREAK_BLOCK, &abi::BlockChange { player, pos: p.to_array().into(), state, item: tool.item });
        true
    }

    /// `state`'s item used on `face` of `clicked`, at `hit` from its corner: where it goes, if it
    /// does. It goes where the point just outside the surface is, a bottom slab's top being still
    /// in its own cell; something with no room there goes in the next.
    #[allow(clippy::too_many_arguments)]
    pub fn place_clicked(
        &mut self,
        c: &'static Content,
        clicked: IVec3,
        face: u8,
        state: u32,
        look: [f32; 3],
        hit: [f32; 3],
        player: u64,
        item: u32,
    ) -> Option<IVec3> {
        const OUTSIDE: f32 = 1e-3;
        let normal = DIRS[face as usize];
        let (axis, up) = ((face / 2) as usize, face % 2 == 1);
        let parts = self.parts(clicked);
        // What placing replaces counts as not there: the click is on what it stands on.
        let replacing = self
            .get(clicked)
            .is_some_and(|m| c.states[m as usize].flags & bf::REPLACEABLE != 0)
            && parts
                .iter()
                .all(|&s| c.states[s as usize].flags & bf::REPLACEABLE != 0);
        let mut point = hit;
        point[axis] = if replacing { if up { OUTSIDE } else { 1.0 - OUTSIDE } } else { hit[axis] + if up { OUTSIDE } else { -OUTSIDE } };
        let inside = (0.0..1.0).contains(&point[axis]) && !replacing;
        let beside = |off: IVec3| std::array::from_fn(|a| point[a] - off[a] as f32);
        if replacing || inside {
            if self.place(c, clicked, face, state, look, if replacing { point } else { beside(IVec3::ZERO) }, player, item) {
                return Some(clicked);
            }
            if replacing {
                return None;
            }
        }
        let next = clicked + normal;
        self.place(c, next, face, state, look, beside(normal), player, item)
            .then_some(next)
    }

    /// `state`'s item placed in the cell at `p`, with `hit` from its corner. It joins what is
    /// there if it is the same block and that takes it in (`merge`), or as one more in the cell
    /// where their shapes don't overlap or they said they may. What placing replaces (a flower,
    /// tall grass) goes where they do.
    #[allow(clippy::too_many_arguments)]
    pub fn place(
        &mut self,
        c: &'static Content,
        p: IVec3,
        face: u8,
        state: u32,
        look: [f32; 3],
        hit: [f32; 3],
        player: u64,
        item: u32,
    ) -> bool {
        if self.get(p).is_none() || self.space.signals.reserved(p) {
            return false;
        }
        let def = c.block_of(state);
        let mut chosen = def.first_state;
        if let Some((owner, cb)) = def.cb.placement {
            let mut args = abi::PlaceArgs {
                pos: p.to_array().into(),
                face: face as u32,
                look,
                state: def.first_state,
                first: def.first_state,
                hit,
                placed: 0,
            };
            host::invoke(owner, cb, &mut args);
            if args.state >= def.first_state && args.state < def.first_state + def.states {
                chosen = args.state;
            } else {
                host::misuse("placement", || format!("{} chose state {}, not one of its own", def.name, args.state));
            }
        }
        let parts = self.parts(p);
        // The same block may take it in.
        let mut merged = None;
        if let Some((owner, cb)) = def.cb.merge {
            for &part in parts
                .iter()
                .filter(|&&s| c.states[s as usize].block == c.states[chosen as usize].block)
            {
                let mut args = abi::PlaceArgs {
                    pos: p.to_array().into(),
                    face: face as u32,
                    look,
                    state: part,
                    first: def.first_state,
                    hit,
                    placed: chosen,
                };
                host::invoke(owner, cb, &mut args);
                if args.state != part && args.state >= def.first_state && args.state < def.first_state + def.states {
                    merged = Some((part, args.state));
                    break;
                }
            }
        }
        // What is there in the way goes if placing replaces it, or there is no room. A liquid
        // fills its whole cell.
        let mut replaced = Vec::new();
        if merged.is_none() {
            for &part in &parts {
                let there = &c.states[part as usize];
                if !(there.liquid.is_some() || c.overlap(part, chosen)) || c.may_share(part, chosen) {
                    continue;
                }
                // Tall grass doesn't replace tall grass: that would only use the item up.
                if there.flags & bf::REPLACEABLE == 0 || there.block == c.states[chosen as usize].block {
                    return false;
                }
                replaced.push(part);
            }
            let stays = parts.iter().filter(|s| !replaced.contains(s));
            if Self::must_be_main(c, chosen) && stays.into_iter().any(|&s| Self::must_be_main(c, s)) {
                return false;
            }
        }
        let shape_of = merged.map_or(chosen, |m| m.1);
        if !self.supported(c, p, shape_of) {
            return false;
        }
        // Not into anyone there, the one placing it too, as far as it fills the block. A structure
        // is where its blocks are, not where its entity is.
        let mut shape = Vec::new();
        crate::physics::boxes(c, shape_of, &mut shape);
        let at = p.as_dvec3();
        let taken = shape.iter().any(|b| {
            let part = self
                .place
                .box_to_world(crate::physics::Aabb { min: at + DVec3::new(b[0], b[1], b[2]), max: at + DVec3::new(b[3], b[4], b[5]) });
            self.entities
                .list
                .iter()
                .any(|e| e.is_body() && e.id != self.frame && !self.bodies.contains_key(&e.id) && e.aabb(c).intersects(&part))
        });
        if taken {
            return false;
        }
        let mut q = abi::Query { entity: player, pos: p.to_array().into(), state: chosen, item, ..abi::Query::DEFAULT };
        if !self.ask(c, abi::action::PLACE_BLOCK, &mut q) {
            return false;
        }
        // What it replaces (a flower, tall grass) drops what it would when broken, unlike vanilla,
        // where it is lost or in the way. Creative loses it, as when breaking.
        let creative = self
            .entities
            .get(player)
            .is_some_and(|e| e.mode.has(crate::tag_bits::game_mode::NO_COSTS));
        for &gone in &replaced {
            if !creative {
                self.drop_block_loot(c, p, gone, ItemStack::EMPTY, false, 0);
            }
            self.replace_part(c, p, gone, AIR);
        }
        match merged {
            Some((part, whole)) => {
                self.replace_part(c, p, part, whole);
                chosen = whole;
            }
            None => {
                self.add_part(c, p, chosen);
            }
        }
        self.drain_if_no_room(c, p);
        self.sound(c.states[chosen as usize].effects[crate::content::block_event::PLACE].sound, p.as_dvec3() + DVec3::splat(0.5), 1.0, 0.0);
        self.emit_engine(c, abi::action::PLACE_BLOCK, &abi::BlockChange { player, pos: p.to_array().into(), state: chosen, item });
        true
    }

    /// True if the block consumed the click.
    pub fn player_use(&mut self, c: &'static Content, p: IVec3, part: u32, face: u8, player: u64) -> bool {
        let state = self.aimed_part(c, p, part);
        if state == AIR {
            return false;
        }
        let opens = c.block_of(state).opens;
        if c.block_of(state).cb.on_use.is_some() || opens.is_some() {
            let mut q = abi::Query { entity: player, pos: p.to_array().into(), state, ..abi::Query::DEFAULT };
            if !self.ask(c, abi::action::USE_BLOCK, &mut q) {
                // Refused uses are consumed: no item use or placement behind them.
                return true;
            }
            let item = self.entities.held(player).item;
            self.emit_engine(c, abi::action::USE_BLOCK, &abi::BlockChange { player, pos: p.to_array().into(), state, item });
        }
        if let Some(menu) = opens {
            // Its block entity's menu, or one of its own (a crafting table).
            let on = self
                .anchored_part(c, p, state)
                .map(|id| crate::server::holder::Inv::own(crate::server::holder::At::Entity(id)));
            self.events
                .push(DimEvent::OpenMenu { player, menu, on, block: state });
            return true;
        }
        let r = self.as_part(p, state, |d| {
            d.call_block(
                c,
                state,
                |b| b.on_use,
                |w| abi::UseArgs { world: w, pos: p.to_array().into(), state, face: face as u32, player, handled: 0 },
            )
        });
        r.is_some_and(|a| a.handled != 0)
    }

    /// What time does to `p` next, in `mean` ticks on average from `from` (an exponential wait
    /// drawn from the place and time, like a random tick would come).
    fn schedule_at(&mut self, p: IVec3, mean: f64, from: u64) {
        let h = hash(0x6F0 ^ from ^ (self.id as u64) << 48, p.x as i64, p.y as i64, p.z as i64);
        let u = ((h >> 11) as f64 + 1.0) / (1u64 << 53) as f64;
        let at = from + (-u.ln() * mean).max(1.0) as u64;
        self.space.growth.insert(p, at);
        self.space.growth_queue.push(Reverse((at, p.x, p.y, p.z)));
    }

    /// What may happen to the cell at `p` by itself, and how often (a share per tick): a rule
    /// now and then of a part of it that may hold some time, and something spreading onto it.
    fn chances(&mut self, c: &'static Content, p: IVec3) -> Vec<(Chance, f64)> {
        let mut out = Vec::new();
        for part in self.parts(p) {
            let ask = self.ask_at(p, part);
            for (i, r) in c.states[part as usize].rules.iter().enumerate() {
                let When::Random(mean) = r.when else { continue };
                if c.acts(part, r.then)
                    && r.cond
                        .is_none_or(|w| crate::conditions::may_server(self, c, w, &ask))
                {
                    out.push((Chance::Rule(part, i), 1.0 / mean));
                }
            }
        }
        if let Some(state) = self.get(p).filter(|&s| c.spread_target(s)) {
            let covered = self
                .get(p + IVec3::Y)
                .is_some_and(|s| c.states[s as usize].flags & bf::OPAQUE != 0);
            if let (Some((_, sp)), n) = self.spreaders_near(c, p, state, covered) {
                out.push((Chance::Spread, n as f64 / sp.mean));
            }
        }
        out
    }

    /// What time does to `p` next, from `from`, if anything can: one wait for all its chances
    /// together. One waits at a time.
    fn consider(&mut self, c: &'static Content, p: IVec3, from: u64) {
        if self.space.growth.contains_key(&p) {
            return;
        }
        let rate: f64 = self.chances(c, p).iter().map(|x| x.1).sum();
        if rate > 0.0 {
            self.schedule_at(p, 1.0 / rate, from);
        }
    }

    /// A block that spreads onto `state` at `p` from where it reaches it (and, while `covered`,
    /// one that spreads onto covered blocks): its state and rule, and how many there are.
    fn spreaders_near(&self, c: &Content, p: IVec3, state: u32, covered: bool) -> (Option<(u32, crate::content::Spread)>, u32) {
        let mut first = None;
        let mut n = 0;
        let (lo, hi) = c.spread_search;
        for dy in lo.y..=hi.y {
            for dz in lo.z..=hi.z {
                for dx in lo.x..=hi.x {
                    let at = IVec3::new(dx, dy, dz);
                    let Some(s) = self.get(p + at) else { continue };
                    if let Some(sp) = c.states[s as usize].spread
                        && (!covered || !sp.open_top)
                        && (-at).cmpge(sp.min).all()
                        && (-at).cmple(sp.max).all()
                        && c.matchers.has(sp.to, state)
                    {
                        first.get_or_insert((s, sp));
                        n += 1;
                    }
                }
            }
        }
        (first, n)
    }

    /// Something spreading onto the block at `p` comes due at `at`: it takes it where the light
    /// is enough, else tries again later.
    fn spread_onto(&mut self, c: &'static Content, p: IVec3, state: u32, at: u64) {
        let covered = self
            .get(p + IVec3::Y)
            .is_some_and(|s| c.states[s as usize].flags & bf::OPAQUE != 0);
        if let (Some((into, sp)), _) = self.spreaders_near(c, p, state, covered) {
            // Covered, the light of its brightest side.
            let light = if sp.open_top {
                self.light(p + IVec3::Y)
            } else {
                crate::world::DIRS
                    .iter()
                    .map(|&d| self.light(p + d))
                    .max_by_key(|&l| abi::light::effective(l, 1.0))
                    .unwrap_or(0)
            };
            if abi::light::effective(light, self.daylight_at(c, at)) >= sp.light {
                self.set(c, p, into);
            }
        }
    }

    /// What time does comes due, in time order (catching up on unloaded time included): of the
    /// cell's chances one is picked by how often it comes, and done if it holds then.
    fn grow(&mut self, c: &'static Content) {
        while let Some(&Reverse((at, x, y, z))) = self.space.growth_queue.peek() {
            if at > self.time {
                break;
            }
            self.space.growth_queue.pop();
            let p = IVec3::new(x, y, z);
            if self.space.growth.get(&p) != Some(&at) {
                continue;
            }
            // Where only machines are kept, it waits as in an unloaded cube.
            if self.idle.contains(&cube_of(p)) {
                continue;
            }
            self.space.growth.remove(&p);
            self.space.growing_at = Some(at);
            // A rule breaking it whose cause no neighbor noticed (water two blocks off).
            let parts = self.parts(p);
            if !parts.iter().any(|&s| self.neighbor_rules(c, p, s, false)) {
                let chances = self.chances(c, p);
                let total: f64 = chances.iter().map(|x| x.1).sum();
                let mut pick = weft_noise::unit(hash(0x6F1 ^ at, p.x as i64, p.y as i64, p.z as i64)) * total;
                if let Some(&(chance, _)) = chances.iter().find(|x| {
                    pick -= x.1;
                    pick < 0.0
                }) {
                    match chance {
                        Chance::Rule(part, i) => {
                            let r = c.states[part as usize].rules[i];
                            let ask = self.ask_at(p, part);
                            if self.has_part(p, part)
                                && r.cond
                                    .is_none_or(|w| crate::conditions::on_server(self, c, w, &ask))
                            {
                                self.act(c, p, part, r.then);
                            }
                        }
                        Chance::Spread => {
                            if let Some(state) = self.get(p) {
                                self.spread_onto(c, p, state, at);
                            }
                        }
                    }
                }
            }
            // The next counts from this one, not from now.
            self.consider(c, p, at);
            self.space.growing_at = None;
        }
    }

    /// Block ticks waiting in a cube: (position in the cube, ticks left).
    pub fn ticks_in(&self, cube: IVec3) -> Vec<(u16, u32)> {
        let base = cube * 16;
        let mut out: Vec<(u16, u32)> = self
            .space
            .scheduled
            .iter()
            .filter_map(|Reverse((at, x, y, z))| {
                let l = IVec3::new(*x, *y, *z) - base;
                ((0..16).contains(&l.x) && (0..16).contains(&l.y) && (0..16).contains(&l.z))
                    .then(|| ((l.x | l.y << 4 | l.z << 8) as u16, at.saturating_sub(self.time).max(1) as u32))
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The ticks a saved cube was still waiting for.
    pub fn restore_ticks(&mut self, cube: IVec3, ticks: &[(u16, u32)]) {
        for &(at, left) in ticks {
            let at = at as i32;
            self.schedule(cube * 16 + IVec3::new(at & 15, (at >> 4) & 15, (at >> 8) & 15), left);
        }
    }

    pub fn schedule(&mut self, p: IVec3, delay: u32) {
        let at = self.time + delay.max(1) as u64;
        self.space.scheduled.push(Reverse((at, p.x, p.y, p.z)));
    }

    pub fn random(&mut self) -> u64 {
        self.rng.next()
    }

    /// Uniform in 0..1 from the same stream.
    pub fn random_unit(&mut self) -> f32 {
        self.rng.float()
    }

    /// Sound event `sound` (`u32::MAX`: nothing) for players in its range.
    pub fn sound(&mut self, sound: u32, pos: DVec3, volume: f32, pitch: f32) {
        if sound != u32::MAX {
            self.events
                .push(DimEvent::Sound { dim: self.id, sound, pos: self.place.to_world(pos), volume, pitch });
        }
    }

    /// Vanilla's 4x4x4 pieces of a broken block flying apart, for everyone watching.
    pub fn block_dust(&mut self, c: &Content, p: IVec3, state: u32) {
        let kind = c.states[state as usize].effects[crate::content::block_event::BREAK].particle;
        if kind == u32::MAX {
            return;
        }
        // Vanilla's: a grid over each box of its shape, a piece every quarter block or at least two
        // along a side, far enough from the box's faces not to start inside a neighbor.
        for b in &c.states[state as usize].selection {
            let size = [b[3] - b[0], b[4] - b[1], b[5] - b[2]];
            let cells = size.map(|s| ((s / 0.25).ceil() as u32).max(2));
            let pos = self
                .place
                .to_world(DVec3::from_array([0, 1, 2].map(|i| p[i] as f64 + (b[i] + size[i] / 2.0) as f64)))
                .to_array();
            let spread = [0, 1, 2].map(|i| size[i] * (0.5 - 0.5 / cells[i] as f32));
            let burst = abi::ParticleBurst {
                kind,
                pos: pos.into(),
                spread: spread.into(),
                velocity: [0.0, 2.0, 0.0].into(),
                speed: 3.6,
                outward: 1,
                count: cells[0] * cells[1] * cells[2],
                block: state,
                ..abi::ParticleBurst::DEFAULT
            };
            self.events
                .push(DimEvent::Particles { dim: self.id, burst });
        }
    }

    /// Spawns what a block's loot table gives for `tool`; `mined`: by that tool, not by the world
    /// (support, liquids), which always harvests.
    /// `own_name`: what a renamed block entity was called, given back to the block's item.
    /// `carried`: stack data the block's own item gets (what its block entity carried).
    pub fn drop_block_loot(&mut self, c: &'static Content, p: IVec3, state: u32, tool: ItemStack, mined: bool, carried: u32) {
        let b = c.block_of(state);
        let own_item = c
            .items
            .iter()
            .position(|i| i.block == Some(b.first_state))
            .map(|i| i as u32);
        for _ in 0..c.block_value::<u32>(state, c.ids.loot_rolls).unwrap_or(1) {
            for s in self.roll_loot(
                c,
                &b.name,
                &crate::loot::Ctx { tool, mined: if mined { state } else { 0 }, state, ..Default::default() },
                p.as_dvec3() + DVec3::splat(0.5),
                0,
            ) {
                let s = if Some(s.item) == own_item && carried != 0 { crate::stack::merged_data(s, carried) } else { s };
                self.spawn_item(c, p.as_dvec3() + DVec3::splat(0.5), s);
            }
        }
    }
}

/// Section = 4x4x4 cubes, the unit regions are made of.
#[inline]
pub fn section_of_cube(cube: IVec3) -> IVec3 {
    cube >> 2
}

#[inline]
pub fn section_of(p: DVec3) -> IVec3 {
    section_of_cube(cube_of(p.floor().as_ivec3()))
}

type Taken = (Entity, Option<Vec<ItemStack>>, Vec<(u32, Box<[u8]>)>);

/// Everything stored for a set of sections, moved between regions when they merge or split.
#[derive(Default)]
pub struct Parcel {
    cubes: Vec<(IVec3, Arc<Cube>, bool, bool)>,
    on_disk: Vec<IVec3>,
    newly_settled: Vec<IVec3>,
    edge_updates: Vec<IVec3>,
    light_dirty: Vec<IVec3>,
    edited: Vec<IVec3>,
    body_light_changes: Vec<(IVec3, [u8; 3])>,
    entities: Vec<Taken>,
    bodies: Vec<(u64, Box<Space>)>,
    scheduled: Vec<Reverse<(u64, i32, i32, i32)>>,
    growth: Vec<(IVec3, u64)>,
}

impl Dim {
    /// Sections this region occupies: loaded cubes plus where its entities stand.
    pub fn add_cube(&mut self, pos: IVec3, cube: Arc<Cube>) {
        if self.space.cubes.insert(pos, cube).is_none() {
            *self.section_cubes.entry(section_of_cube(pos)).or_default() += 1;
        }
    }

    fn cube_gone(&mut self, pos: IVec3) {
        let s = section_of_cube(pos);
        if let Some(n) = self.section_cubes.get_mut(&s) {
            *n -= 1;
            if *n == 0 {
                self.section_cubes.remove(&s);
            }
        }
    }

    pub fn sections(&self) -> FxHashSet<IVec3> {
        let mut s: FxHashSet<IVec3> = self.section_cubes.keys().copied().collect();
        s.extend(
            self.entities
                .list
                .iter()
                .filter(|e| !e.removed)
                .map(|e| section_of(self.entities.follows(e).pos)),
        );
        s
    }

    /// Moves out cubes, entities and pending ticks located in sections where `take` is true.
    pub fn take_sections(&mut self, take: impl Fn(IVec3) -> bool) -> Parcel {
        self.finish_moving(crate::content::content());
        let mut p = Parcel::default();
        let cubes: Vec<IVec3> = self
            .space
            .cubes
            .keys()
            .copied()
            .filter(|&c| take(section_of_cube(c)))
            .collect();
        for pos in cubes {
            let cube = self.space.cubes.remove(&pos).unwrap();
            self.cube_gone(pos);
            self.space.grid.cube_unloaded(pos);
            self.space
                .signals
                .cube_unloaded(crate::content::content(), pos);
            self.space.conn.remove(&pos);
            p.cubes
                .push((pos, cube, self.space.modified.remove(&pos), self.settled.remove(&pos)));
            if self.on_disk.remove(&pos) {
                p.on_disk.push(pos);
            }
        }
        // What was waiting on these cubes goes with them.
        p.newly_settled = self
            .newly_settled
            .extract_if(.., |q| take(section_of_cube(*q)))
            .collect();
        let (moved, kept): (Vec<IVec3>, Vec<IVec3>) = self
            .space
            .edge_updates
            .drain(..)
            .partition(|q| take(section_of_cube(cube_of(*q))));
        p.edge_updates = moved;
        self.space.edge_updates.extend(kept);
        p.light_dirty = self
            .space
            .light_dirty
            .iter()
            .copied()
            .filter(|q| take(section_of_cube(*q)))
            .collect();
        for q in &p.light_dirty {
            self.space.light_dirty.remove(q);
        }
        p.edited = self
            .space
            .edited
            .iter()
            .copied()
            .filter(|q| take(section_of_cube(*q)))
            .collect();
        for q in &p.edited {
            self.space.edited.remove(q);
        }
        // Not heard by clients yet, and this region may be going.
        p.body_light_changes = self
            .space
            .body_light_changes
            .extract_if(.., |(q, _)| take(section_of_cube(cube_of(*q))))
            .collect();
        let ids: Vec<u64> = self
            .entities
            .list
            .iter()
            .filter(|e| take(section_of(self.entities.follows(e).pos)))
            .map(|e| e.id)
            .collect();
        for id in ids {
            if let Some(t) = self.entities.take(id) {
                p.entities.push(t);
            }
            if let Some(st) = self.bodies.remove(&id) {
                p.bodies.push((id, st));
            }
        }
        let (moved, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.space.scheduled)
            .into_iter()
            .partition(|Reverse((_, x, y, z))| take(section_of_cube(cube_of(IVec3::new(*x, *y, *z)))));
        self.space.scheduled = kept.into_iter().collect();
        p.scheduled = moved;
        p.growth = self
            .space
            .growth
            .iter()
            .filter(|(q, _)| take(section_of_cube(cube_of(**q))))
            .map(|(q, at)| (*q, *at))
            .collect();
        for (q, _) in &p.growth {
            self.space.growth.remove(q);
        }
        p
    }

    pub fn absorb(&mut self, c: &Content, p: Parcel) {
        self.on_disk.extend(p.on_disk);
        self.newly_settled.extend(p.newly_settled);
        self.space.edge_updates.extend(p.edge_updates);
        self.space.light_dirty.extend(p.light_dirty);
        self.space.edited.extend(p.edited);
        self.space.body_light_changes.extend(p.body_light_changes);
        for (pos, cube, modified, settled) in p.cubes {
            self.space.grid.cube_loaded(c, pos, &cube);
            self.space.signals.cube_loaded(c, pos, &cube);
            self.add_cube(pos, cube);
            if modified {
                self.space.modified.insert(pos);
            }
            if settled {
                self.settled.insert(pos);
                self.settled_gen += 1;
            }
        }
        for (e, inv, comps) in p.entities {
            self.entities.insert_with(c, e, inv, comps);
        }
        self.bodies.extend(p.bodies);
        self.entities.sort_by_id();
        self.space.scheduled.extend(p.scheduled);
        for (q, at) in p.growth {
            self.space.growth.insert(q, at);
            self.space.growth_queue.push(Reverse((at, q.x, q.y, q.z)));
        }
    }

    pub fn connectivity(&mut self, c: &Content, pos: IVec3) -> Option<u16> {
        if let Some(&v) = self.space.conn.get(&pos) {
            return Some(v);
        }
        let v = see_through(c, self.space.cubes.get(&pos)?);
        self.space.conn.insert(pos, v);
        Some(v)
    }

    /// Marks `pos` and the loaded cubes below it settled: nothing above will ever bring sky
    /// light (it is not going to be loaded), so their light is final as it is.
    pub fn settle_down(&mut self, pos: IVec3) {
        let mut p = pos;
        while self.space.cubes.contains_key(&p) && self.settled.insert(p) {
            self.settled_gen += 1;
            self.newly_settled.push(p);
            p -= IVec3::Y;
        }
    }

    pub fn set_rng(&mut self, seed: u64) {
        self.rng = Rng(seed);
    }
}

/// What may happen to a cell with time (`Dim::chances`).
#[derive(Clone, Copy)]
enum Chance {
    /// Rule `.1` of part `.0`.
    Rule(u32, usize),
    Spread,
}
