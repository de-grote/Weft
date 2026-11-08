//! Particles: kinds are data (`registry::ParticleDef`), particles live only on clients. The CPU
//! only makes them: the GPU moves them every frame (`shaders/particle_sim.wgsl`), stopped by the
//! blocks around the camera it is told about, answers what happens to them by their kinds' rules
//! (`registry::ParticleRule`), and draws them as instanced camera-facing quads: one pipeline for
//! the cut-out ones, one for the blended ones.

use std::collections::VecDeque;

use crate::tag_bits::particle as pf;
use glam::{DVec3, IVec3, Vec3};
use weft_abi::ParticleBurst;
use weft_render::*;

use super::textures::GameTextures;
use super::world::ClientWorld;
use super::world_render::Camera;
use crate::content::content;
use crate::gpu_layout::{GpuParticle, GpuParticleKind, GpuParticleRule, GpuSimParticle};
use crate::shaders::{Hook, Slots};

/// Particles a client keeps at most, half of them for each way of drawing; bursts beyond it are
/// dropped.
const MAX: usize = 1 << 18;
/// Kinds that may live longer than this (their turning into others included) get rings of their
/// own: a slot comes back only when the longest of its burst has lived, and one long burst would
/// hold up every short one after it.
const LONG_LIFE: f32 = 4.0;
/// The share of each way of drawing the long-lived ones get.
const LONG_SHARE: usize = 8;

pub const HOOK: Hook = Hook {
    kind: "particle",
    entry: "shade",
    dispatch: "particle_shade",
    params: "p: ParticleFrag",
    args: "p",
    ret: "vec4<f32>",
    fallback: "return p.albedo * p.color;",
};

/// Vanilla's particles collide as a box 0.2 across and 0.2 up from where they are.
fn particle_box(pos: DVec3) -> crate::physics::Aabb {
    crate::physics::Aabb::around(pos, 0.1, 0.2)
}

/// A particle on its way to the GPU.
struct Spawned {
    slot: u32,
    through: bool,
    in_liquid: bool,
    pos: DVec3,
    vel: Vec3,
    life: f32,
    size: f32,
    kind: u16,
    /// Block dust: the block texture and the corner of its quarter, in sixteenths.
    layer: u32,
    piece: [u8; 2],
    /// 0xRRGGBB its color is multiplied by: the burst's, and for dust what the block's biome
    /// colors it (grass and leaves are gray without it).
    tint: u32,
}

/// Slots handed out in turn, one kind of drawing in each. The GPU moves all between `tail` and
/// `head`; a burst's are given back once the longest of it has lived, others in it draw nothing
/// until then.
struct Ring {
    start: u32,
    capacity: u32,
    head: u64,
    tail: u64,
    /// Per burst the `head` it ended at and the time it is over.
    bursts: VecDeque<(u64, f32)>,
}

impl Ring {
    fn new(start: u32, capacity: u32) -> Ring {
        Ring { start, capacity, head: 0, tail: 0, bursts: VecDeque::new() }
    }

    fn len(&self) -> usize {
        (self.head - self.tail) as usize
    }

    /// The live slots, in up to two runs: (first, count).
    fn runs(&self) -> [(u32, u32); 2] {
        let cap = self.capacity as u64;
        let (from, n) = (self.tail % cap, self.head - self.tail);
        let first = n.min(cap - from);
        [(self.start + from as u32, first as u32), (self.start, (n - first) as u32)]
    }
}

pub struct Particles {
    /// Short-lived cut out, short-lived blended, then the same for long-lived kinds.
    rings: [Ring; 4],
    spawned: Vec<Spawned>,
    /// Seconds since this began, and those not yet simulated.
    clock: f32,
    dt: f32,
    rng: weft_noise::Rng,
    pipes: Option<[PipelineId; 2]>,
    sim_pipe: Option<PipelineId>,
    /// Shader slot of each kind.
    kind_slots: Vec<u32>,
    sim: BufferId,
    dst: BufferId,
    cells: BufferId,
    kinds_buf: Vec<(BufferId, u32)>,
    rules_buf: Vec<(BufferId, u32)>,
    frames_buf: Vec<(BufferId, u32)>,
    /// What `cells` holds per slot: the cube and its revision.
    loaded: Vec<Option<(IVec3, u32)>>,
    /// Offsets of the cubes of the window, nearest first.
    window: Vec<IVec3>,
    /// A block's word in `cells` but for its light, per state.
    blocks: Vec<u32>,
    origin: IVec3,
}

/// The cells' window reaches this many cubes from the camera's.
const REACH: i32 = 3;
const SIDE: i32 = REACH * 2 + 1;
const CUBE_CELLS: u64 = 4096;
/// Cubes copied to the GPU a frame, nearest first: a few hundred kilobytes.
const CUBES_PER_FRAME: usize = 16;

/// What `particle_sim.wgsl` reads of a block: bottom and top of what is solid in sixteenths (the
/// lowest and highest of its boxes, over the whole block: a particle needs no more), whether it
/// is a liquid, and which of the tags particle rules ask about it has.
fn block_word(s: &crate::content::StateInfo, tags: &[bool]) -> u32 {
    let span = if s.collision.is_empty() {
        0
    } else {
        let (lo, hi) = s
            .collision
            .iter()
            .fold((1.0f32, 0.0f32), |(l, h), b| (l.min(b[1]), h.max(b[4])));
        let sixteenths = |v: f32| (v.clamp(0.0, 1.0) * 16.0).round() as u32;
        sixteenths(lo) << 16 | sixteenths(hi) << 21
    };
    let tags = tags
        .iter()
        .enumerate()
        .fold(0, |m, (i, &has)| m | (has as u32) << (TAG_SHIFT + i as u32));
    span | (s.liquid.is_some() as u32) << LIQUID_BIT | tags
}

/// Where `block_word` puts the liquid bit and the first tag, as `particle_sim.wgsl` has them.
const LIQUID_BIT: u32 = 26;
const TAG_SHIFT: u32 = 27;

/// A block's word in `cells`: its light as four nibbles and the rest from `block_word`.
fn cell_word(block: u32, light: u32) -> u32 {
    let ch = |c: u32| ((light >> (c * 8)) & 255).min(15);
    block | ch(0) | ch(1) << 4 | ch(2) << 8 | ch(3) << 12
}

/// Bits of `GpuSimParticle::data.x` above the kind, as `particle_sim.wgsl` has them.
const THROUGH: u32 = 1 << 16;
const IN_LIQUID: u32 = 1 << 19;

/// 0xRRGGBB times 0xRRGGBB.
fn multiply(a: u32, b: u32) -> u32 {
    (0..3).fold(0, |out, i| {
        let ch = |c: u32| (c >> (i * 8)) & 255;
        out | (ch(a) * ch(b) / 255) << (i * 8)
    })
}

fn shader(assets: &crate::assets::Assets) -> Result<(Shader, Slots), String> {
    let snippets = crate::shaders::snippets(assets, &HOOK);
    let (s, mut slots) = crate::packs::compose(assets, "particle.wgsl", &[(&HOOK, &snippets)]).map_err(|e| e.0)?;
    Ok((s, slots.pop().unwrap_or_default()))
}

impl Particles {
    pub fn new(gpu: &mut dyn Backend, assets: &crate::assets::Assets) -> Particles {
        let sim = gpu.create_buffer((MAX * size_of::<GpuSimParticle>()) as u64, usage::STORAGE, "particles");
        let dst = gpu.create_buffer((MAX * size_of::<GpuParticle>()) as u64, usage::STORAGE, "particles drawn");
        let slots = (SIDE * SIDE * SIDE) as usize;
        let cells = gpu.create_buffer(slots as u64 * CUBE_CELLS * 4, usage::STORAGE, "particle cells");
        gpu.write_buffer(cells, 0, &vec![0u8; slots * CUBE_CELLS as usize * 4]);
        let mut window: Vec<IVec3> = (-REACH..=REACH)
            .flat_map(|z| (-REACH..=REACH).flat_map(move |y| (-REACH..=REACH).map(move |x| IVec3::new(x, y, z))))
            .collect();
        window.sort_by_key(|o| o.length_squared());
        let c = content();
        let tag_ids: Vec<Option<u32>> = c
            .game
            .particle_tags
            .iter()
            .map(|t| c.components.id(t))
            .collect();
        let blocks = (0..c.states.len() as u32)
            .map(|s| {
                let tags: Vec<bool> = tag_ids
                    .iter()
                    .map(|t| t.is_some_and(|t| c.components.has(crate::components::BLOCK, s, t)))
                    .collect();
                block_word(&c.states[s as usize], &tags)
            })
            .collect();
        let half = MAX / 2;
        let long = half / LONG_SHARE;
        let ring = |n: usize, start: usize| Ring::new(start as u32, n as u32);
        let mut p = Particles {
            rings: [ring(half - long, 0), ring(half - long, half), ring(long, half - long), ring(long, MAX - long)],
            spawned: Vec::new(),
            clock: 0.0,
            dt: 0.0,
            rng: weft_noise::Rng(0x9E37_79B9_7F4A_7C15),
            pipes: None,
            sim_pipe: None,
            kind_slots: Vec::new(),
            sim,
            dst,
            cells,
            kinds_buf: Vec::new(),
            rules_buf: Vec::new(),
            frames_buf: Vec::new(),
            loaded: vec![None; slots],
            window,
            blocks,
            origin: IVec3::ZERO,
        };
        p.reload(gpu, assets);
        p
    }

    /// How many there are, as far as it can be told here: the ones of a burst count until the
    /// longest of them has lived.
    pub fn len(&self) -> usize {
        self.rings.iter().map(Ring::len).sum()
    }

    /// Recompiles the shader with the particle snippets mods and packs have now.
    pub fn reload(&mut self, gpu: &mut dyn Backend, assets: &crate::assets::Assets) {
        let made = shader(assets).and_then(|(s, slots)| {
            let mut make = |label: &str, fs: &str, depth: DepthState, blend: Blend| {
                gpu.create_pipeline(&PipelineDesc {
                    label,
                    shader: &s,
                    vertex: "vs",
                    fragment: Some(fs),
                    color: Some(Format::Surface),
                    depth: Some(depth),
                    blend,
                    cull_back: false,
                    topology: Topology::Triangles,
                })
                .map_err(|e| e.0)
            };
            let cut = make("particles cutout", "fs_cutout", DepthState { write: true, compare: Compare::GreaterEqual }, Blend::None)?;
            let blend = make("particles blended", "fs_blend", DepthState { write: false, compare: Compare::GreaterEqual }, Blend::Alpha)?;
            Ok(([cut, blend], slots))
        });
        match made {
            Ok((pipes, slots)) => {
                if let Some(old) = self.pipes.replace(pipes) {
                    old.into_iter().for_each(|p| gpu.destroy_pipeline(p));
                }
                self.kind_slots = content()
                    .game
                    .particles
                    .iter()
                    .map(|k| slots.slot(&k.shader))
                    .collect();
                for k in content()
                    .game
                    .particles
                    .iter()
                    .filter(|k| !k.shader.is_empty() && slots.slot(&k.shader) == 0)
                {
                    log::warn!("particle {}: no shader {}", k.name, k.shader);
                }
            }
            Err(e) => log::error!("particle shader: {e}"),
        }
        // Not a shader packs may replace: it decides where things are, not how they look.
        let sim = crate::packs::engine_shader(assets, "particle_sim.wgsl", &crate::gpu_layout::particle_sim_prelude(REACH))
            .map_err(|e| e.0)
            .and_then(|s| {
                gpu.create_compute(&ComputeDesc { label: "particles", shader: &s, entry: "simulate" })
                    .map_err(|e| e.0)
            });
        match sim {
            Ok(pipe) => {
                if let Some(old) = self.sim_pipe.replace(pipe) {
                    gpu.destroy_pipeline(old);
                }
            }
            Err(e) => log::error!("particle simulation shader: {e}"),
        }
        // Their pictures may be gone.
        self.clear();
    }

    pub fn destroy(&mut self, gpu: &mut dyn Backend) {
        for p in self
            .pipes
            .take()
            .into_iter()
            .flatten()
            .chain(self.sim_pipe.take())
        {
            gpu.destroy_pipeline(p);
        }
        for b in [self.sim, self.dst, self.cells] {
            gpu.destroy_buffer(b);
        }
        for (b, _) in self
            .kinds_buf
            .drain(..)
            .chain(self.rules_buf.drain(..))
            .chain(self.frames_buf.drain(..))
        {
            gpu.destroy_buffer(b);
        }
    }

    /// None left, and what `cells` holds is of another world.
    pub fn clear(&mut self) {
        self.spawned.clear();
        for r in &mut self.rings {
            r.tail = r.head;
            r.bursts.clear();
        }
        self.loaded.fill(None);
    }

    fn random(&mut self) -> f32 {
        self.rng.float()
    }

    pub fn spawn(&mut self, b: &ParticleBurst, world: &ClientWorld) {
        let c = content();
        let Some(k) = c.game.particles.get(b.kind as usize) else { return };
        let pos = crate::world::dvec(b.pos);
        if !pos.is_finite() {
            return;
        }
        // Block dust shows the block's side texture (a box model's first box's).
        let looks = super::looks::current();
        let side = |s: &crate::content::StateInfo| {
            let d = looks.drawn(b.block as u32, s);
            match d.boxes.first() {
                Some(b) if d.model == weft_abi::BlockModel::Boxes => b.textures[1],
                // A piece of wire shows its dot, red: the picture has no color of its own.
                _ if d.model == weft_abi::BlockModel::Wire => d.textures[3],
                _ => d.textures[1],
            }
        };
        let dust = c
            .states
            .get(b.block as usize)
            .filter(|_| k.frames.is_empty());
        let layer = dust.map_or(0, side);
        // Only if the side it shows is a tinted one: grass blocks' sides are dirt with a tinted
        // overlay, and their dust stays brown like vanilla's. Wire's picture is gray, its red
        // comes from its power when drawn: dust takes vanilla's wire at middling power.
        let dust_tint = if dust.is_some_and(|s| s.model == weft_abi::BlockModel::Wire) {
            0xC8_1414
        } else {
            dust.filter(|s| s.tint != 0 && s.tint_faces & 2 != 0)
                .map_or(0xFF_FFFF, |s| {
                    let p = pos.floor().as_ivec3();
                    let biome = world
                        .cube(p >> 4i32)
                        .map_or(0, |cube| cube.biome((p.x & 15) as usize, (p.z & 15) as usize));
                    c.biomes
                        .get(biome as usize)
                        .and_then(|b| b.tints.get(s.tint as usize - 1))
                        .copied()
                        .unwrap_or_else(|| crate::content::item_tint(s.tint))
                })
        };
        let tint = multiply(dust_tint, b.color & 0xFF_FFFF);
        // Drawn apart by how they blend and kept apart by how long they live; bursts beyond
        // what there is room for are dropped.
        let which = (k.flags & pf::TRANSLUCENT != 0) as usize + 2 * (k.longest > LONG_LIFE) as usize;
        let (head, capacity, start) = {
            let r = &self.rings[which];
            (r.head, r.capacity as u64, r.start)
        };
        let n = (b.count as u64).min(capacity - self.rings[which].len() as u64) as usize;
        if n == 0 {
            return;
        }
        for i in 0..n {
            let mut r3 = || Vec3::new(self.random() * 2.0 - 1.0, self.random() * 2.0 - 1.0, self.random() * 2.0 - 1.0);
            let place = r3();
            let off = place * crate::world::dvec(b.spread).as_vec3();
            let mut dir = r3();
            if b.outward != 0 {
                // Vanilla's `Particle`: away from the middle, loosened, at (r + r + 1) / 3 of the speed.
                dir = (place * 0.375 + dir * 0.4).normalize_or_zero() * (self.random() + self.random() + 1.0) / 3.0;
            } else {
                while dir.length_squared() > 1.0 {
                    dir = r3();
                }
            }
            let vel = crate::world::dvec(b.velocity).as_vec3() + dir * b.speed;
            let life = k.life_min + (k.life_max - k.life_min) * self.random();
            let size = k.size_min + (k.size_max - k.size_min) * self.random();
            let piece = [(self.random() * 12.0) as u8, (self.random() * 12.0) as u8];
            let at = pos + off.as_dvec3();
            // A block can still be here on this client: the news of it going come separately.
            let through = k.flags & pf::COLLIDE != 0 && crate::physics::collides(c, &|p| world.block_or_air(p), particle_box(at));
            // Born in a liquid is no going into one.
            let middle = (at + DVec3::Y * 0.1).floor().as_ivec3();
            let in_liquid = c.states[world.block(middle) as usize].liquid.is_some();
            let slot = start + ((head + i as u64) % capacity) as u32;
            self.spawned
                .push(Spawned { slot, through, in_liquid, pos: at, vel, life, size, kind: b.kind as u16, layer, piece, tint });
        }
        let r = &mut self.rings[which];
        r.head += n as u64;
        r.bursts.push_back((r.head, self.clock + k.longest));
    }

    /// `dt` seconds more for the next `simulate`, and the slots of what has lived given back.
    pub fn update(&mut self, dt: f32) {
        self.clock += dt;
        self.dt += dt;
        let now = self.clock;
        for r in &mut self.rings {
            while let Some(&(end, over)) = r.bursts.front() {
                if over > now {
                    break;
                }
                r.tail = end;
                r.bursts.pop_front();
            }
        }
    }

    /// Copies what changed of the blocks around the camera to `cells`, nearest first.
    fn update_cells(&mut self, gpu: &mut dyn Backend, world: &ClientWorld, center: IVec3) {
        let mut budget = CUBES_PER_FRAME;
        let mut words = vec![0u32; CUBE_CELLS as usize];
        for &offset in &self.window {
            if budget == 0 {
                break;
            }
            let cp = center + offset;
            let at = cp.rem_euclid(IVec3::splat(SIDE));
            let slot = (at.x + SIDE * (at.y + SIDE * at.z)) as usize;
            let cube = world.cube_stamped(cp);
            let now = cube.as_ref().map_or((cp, 0), |c| (cp, c.1));
            if self.loaded[slot] == Some(now) {
                continue;
            }
            self.loaded[slot] = Some(now);
            budget -= 1;
            match cube {
                Some((cube, _)) => {
                    for (i, w) in words.iter_mut().enumerate() {
                        *w = cell_word(self.blocks.get(cube.get(i) as usize).copied().unwrap_or(0), cube.light(i));
                    }
                }
                // Under the open sky, as `ClientWorld::light` has it.
                None => words.fill(15),
            }
            gpu.write_buffer(self.cells, slot as u64 * CUBE_CELLS * 4, bytemuck::cast_slice(&words));
        }
    }

    /// Before the frame's passes: new particles go up, and all of them take a step.
    pub fn simulate(
        &mut self,
        gpu: &mut dyn Backend,
        list: &mut CommandList,
        slot: usize,
        textures: &GameTextures,
        world: &ClientWorld,
        cam: &Camera,
    ) {
        let center = cam.pos.floor().as_ivec3() >> 4i32;
        self.update_cells(gpu, world, center);
        let Some(pipe) = self.sim_pipe else { return };
        if self.len() == 0 {
            self.spawned.clear();
            self.dt = 0.0;
            return;
        }
        let c = content();
        let dt = std::mem::take(&mut self.dt);
        // Positions are kept near the origin, as floats; it follows the camera now and then.
        let origin = self.origin;
        let mut shift = Vec3::ZERO;
        let here = cam.pos.floor().as_ivec3();
        if (here - origin).abs().max_element() > 256 {
            shift = (origin - here).as_vec3();
            self.origin = here;
        }
        // New ones where they are from the origin they started at: the step below shifts them too.
        let mut run: Vec<GpuSimParticle> = Vec::new();
        let mut first = 0u32;
        let stride = size_of::<GpuSimParticle>() as u64;
        for s in std::mem::take(&mut self.spawned) {
            if !run.is_empty() && first + run.len() as u32 != s.slot {
                gpu.write_buffer(self.sim, first as u64 * stride, bytemuck::cast_slice(&run));
                run.clear();
            }
            if run.is_empty() {
                first = s.slot;
            }
            let k = &c.game.particles[s.kind as usize];
            let (u, v) = (s.piece[0] as u32 * 255 / 16, s.piece[1] as u32 * 255 / 16);
            let uv = u | v << 8 | (u + 64).min(255) << 16 | (v + 64).min(255) << 24;
            let texture = if k.frames.is_empty() { textures.texture(s.layer) } else { 0 };
            let rel = (s.pos - origin.as_dvec3()).as_vec3();
            run.push(GpuSimParticle {
                pos_size: [rel.x, rel.y, rel.z, s.size],
                vel_age: [s.vel.x, s.vel.y, s.vel.z, 0.0],
                life: [s.life, 0.0, 0.0, 0.0],
                data: [s.kind as u32 | if s.through { THROUGH } else { 0 } | if s.in_liquid { IN_LIQUID } else { 0 }, s.tint, texture, uv],
            });
        }
        if !run.is_empty() {
            gpu.write_buffer(self.sim, first as u64 * stride, bytemuck::cast_slice(&run));
        }
        // What a kind does in this step's time, and what its frames are now.
        let ticks = dt * crate::world::TPS as f32;
        let ground = 0.7f32.powf(ticks);
        let mut frames: Vec<u32> = Vec::new();
        let mut rules: Vec<GpuParticleRule> = Vec::new();
        let kinds: Vec<GpuParticleKind> =
            c.game
                .particles
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    let (at, first_rule) = (frames.len() as u32, rules.len() as u32);
                    frames.extend(k.frames.iter().map(|&f| textures.texture(f)));
                    rules.extend(k.rules.iter().map(|r| GpuParticleRule {
                        data: [r.event | r.when << 4 | r.tag << 8 | r.then << 16, r.amount.to_bits(), r.into, 0],
                    }));
                    GpuParticleKind {
                        motion: [k.gravity, k.drag.powf(ticks), ground, 0.0],
                        data: [k.color, k.color_end, k.flags, self.kind_slots.get(i).copied().unwrap_or(0)],
                        frames: [at, k.frames.len() as u32, first_rule, k.rules.len() as u32],
                        start: [k.life_min, k.life_max, k.size_min, k.size_max],
                    }
                })
                .collect();
        if frames.is_empty() {
            frames.push(0);
        }
        if rules.is_empty() {
            rules.push(GpuParticleRule::default());
        }
        let upload = |gpu: &mut dyn Backend, slots: &mut Vec<(BufferId, u32)>, bytes: &[u8], n: u32, each: u64, label: &str| {
            let buf = crate::gpu_layout::frame_buffer(gpu, slots, slot, n, each, usage::STORAGE, label);
            gpu.mapped(buf)[..bytes.len()].copy_from_slice(bytes);
            buf
        };
        let kinds_buf = upload(
            gpu,
            &mut self.kinds_buf,
            bytemuck::cast_slice(&kinds),
            kinds.len() as u32,
            size_of::<GpuParticleKind>() as u64,
            "particle kinds",
        );
        let rules_buf = upload(
            gpu,
            &mut self.rules_buf,
            bytemuck::cast_slice(&rules),
            rules.len() as u32,
            size_of::<GpuParticleRule>() as u64,
            "particle rules",
        );
        let frames_buf = upload(gpu, &mut self.frames_buf, bytemuck::cast_slice(&frames), frames.len() as u32, 4, "particle frames");
        use crate::gpu_layout::bind::particle_sim as b;
        // The last frame's drawing is done with what this one writes.
        list.push(Cmd::Barrier);
        list.pipeline(pipe);
        list.bind(b::SIM, self.sim);
        list.bind(b::DST, self.dst);
        list.bind(b::KINDS, kinds_buf);
        list.bind(b::RULES, rules_buf);
        list.bind(b::FRAMES, frames_buf);
        list.bind(b::CELLS, self.cells);
        let rel = (self.origin.as_dvec3() - cam.pos).as_vec3();
        for (first, count) in self.rings.iter().flat_map(Ring::runs).filter(|r| r.1 > 0) {
            list.push_constants(&crate::gpu_layout::ParticleSimArgs {
                first,
                count,
                dt,
                pad: 0.0,
                shift: shift.extend(0.0).to_array(),
                rel: rel.extend(0.0).to_array(),
                origin: self.origin.extend(0).to_array(),
                center: center.extend(0).to_array(),
            });
            list.push(Cmd::Dispatch { x: count.div_ceil(256), y: 1, z: 1 });
        }
        list.push(Cmd::Barrier);
    }

    /// Cutout particles into `list`, blended ones into `late` (after translucent blocks).
    /// `scene`: the frame's `gpu_layout::Scene`.
    pub fn draw(&mut self, list: &mut CommandList, late: &mut CommandList, scene: BufferId) {
        let Some(pipes) = self.pipes else { return };
        for (blend, list) in [list, late].into_iter().enumerate() {
            let rings = [&self.rings[blend], &self.rings[blend + 2]];
            if rings.iter().all(|r| r.len() == 0) {
                continue;
            }
            list.pipeline(pipes[blend]);
            list.bind(crate::gpu_layout::bind::particle::PARTICLES, self.dst);
            list.bind(crate::gpu_layout::bind::particle::SCENE, scene);
            for (first, count) in rings.iter().flat_map(|r| r.runs()).filter(|r| r.1 > 0) {
                list.push(Cmd::Draw { vertices: 6, instances: count, first_vertex: 0, first_instance: first });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_wraps_in_two_runs() {
        let mut r = Ring::new(100, 10);
        (r.tail, r.head) = (7, 12);
        assert_eq!(r.len(), 5);
        assert_eq!(r.runs(), [(107, 3), (100, 2)]);
        (r.tail, r.head) = (2, 4);
        assert_eq!(r.runs(), [(102, 2), (100, 0)]);
    }

    #[test]
    fn colors_multiply() {
        assert_eq!(multiply(0xFF_8000, 0xFF_FFFF), 0xFF_8000);
        assert_eq!(multiply(0x80_FF00, 0xFF_8080), 0x80_8000);
    }

    #[test]
    fn a_cell_holds_light_and_span() {
        // Sky 15, red 1, green 2, blue 3, half a block solid from the bottom.
        let word = cell_word(8 << 21, 15 | 1 << 8 | 2 << 16 | 3 << 24);
        assert_eq!(word & 0xFFFF, 15 | 1 << 4 | 2 << 8 | 3 << 12);
        assert_eq!((word >> 16) & 31, 0, "bottom");
        assert_eq!((word >> 21) & 31, 8, "top");
    }
}
