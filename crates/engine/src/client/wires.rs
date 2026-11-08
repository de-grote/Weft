//! Redstone wire on the screen. It is not part of a cube's mesh: its power changes all the time and
//! a mesh would be made again for each change. A quad per piece or arm of one, a number per piece
//! for its power: when only power changes, only those numbers go to the GPU.

use glam::{DVec3, IVec3};
use rustc_hash::FxHashMap;
use weft_render::*;

use super::Camera;
use super::textures::GameTextures;
use super::world::ClientWorld;
use crate::content::{Content, content};
use crate::gpu_layout::GpuWireQuad;
use crate::wire::{self, Link};
use crate::world::DIRS;

/// Past this many pieces changing at once the whole list goes up.
const WHOLE_FROM: usize = 256;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Push {
    origin: [i32; 4],
    frac: [f32; 4],
}

#[derive(Default)]
struct Buffer {
    id: Option<BufferId>,
    capacity: u64,
}

impl Buffer {
    fn fit(&mut self, gpu: &mut dyn Backend, bytes: u64, label: &str) -> BufferId {
        if let Some(id) = self.id.filter(|_| self.capacity >= bytes) {
            return id;
        }
        if let Some(old) = self.id.take() {
            gpu.destroy_buffer(old);
        }
        self.capacity = bytes.max(1 << 12).next_power_of_two();
        let id = gpu.create_buffer(self.capacity, usage::STORAGE, label);
        self.id = Some(id);
        id
    }

    fn destroy(&mut self, gpu: &mut dyn Backend) {
        if let Some(id) = self.id.take() {
            gpu.destroy_buffer(id);
        }
    }
}

/// The pipeline, and the wire of each thing there is wire on: the world (0) and bodies.
pub struct Wires {
    pipe: Option<PipelineId>,
    sets: FxHashMap<u64, WireSet>,
}

#[derive(Default)]
struct WireSet {
    quads: Buffer,
    levels: Buffer,
    /// Wire cells per cube.
    cubes: FxHashMap<IVec3, Vec<IVec3>>,
    /// What the server says of each cell's pieces.
    power: FxHashMap<IVec3, [u8; 6]>,
    /// Where each piece's number is.
    index: FxHashMap<(IVec3, u8), u32>,
    numbers: Vec<u32>,
    changed: Vec<u32>,
    count: u32,
    /// Textures were loaded again.
    stale: bool,
}

/// Whether wire reaches out to a device next to it, in direction `e` from it, and looks like it:
/// toward what it feeds or what feeds it, by the rules of the simulation.
fn reaches_device(c: &Content, state: u32, e: u8) -> bool {
    let s = &c.states[state as usize].signal;
    (s.inputs | s.gives) & (1 << (e ^ 1)) != 0
}

/// The axes a face's quad goes along: u, v.
fn axes(face: u8) -> (u8, u8) {
    match face / 2 {
        0 => (2, 1),
        1 => (0, 2),
        _ => (0, 1),
    }
}

impl Wires {
    pub fn new(gpu: &mut dyn Backend, assets: &crate::assets::Assets) -> Wires {
        let mut w = Wires { pipe: None, sets: FxHashMap::default() };
        w.reload(gpu, assets);
        w
    }

    pub fn reload(&mut self, gpu: &mut dyn Backend, assets: &crate::assets::Assets) {
        let made = crate::packs::compose(assets, "wire.wgsl", &[])
            .map_err(|e| e.0)
            .and_then(|(shader, _)| {
                gpu.create_pipeline(&PipelineDesc {
                    label: "wire",
                    shader: &shader,
                    vertex: "vs",
                    fragment: Some("fs"),
                    color: Some(Format::Surface),
                    depth: Some(DepthState { write: true, compare: Compare::GreaterEqual }),
                    blend: Blend::None,
                    cull_back: false,
                    topology: Topology::Triangles,
                })
                .map_err(|e| e.0)
            });
        match made {
            Ok(pipe) => {
                if let Some(old) = self.pipe.replace(pipe) {
                    gpu.destroy_pipeline(old);
                }
            }
            Err(e) => log::error!("wire shader: {e}"),
        }
        for set in self.sets.values_mut() {
            set.stale = true;
        }
    }

    pub fn destroy(&mut self, gpu: &mut dyn Backend) {
        if let Some(p) = self.pipe.take() {
            gpu.destroy_pipeline(p);
        }
        for set in self.sets.values_mut() {
            set.destroy(gpu);
        }
    }

    /// Another world or dimension. The buffers go when `update` next runs.
    pub fn clear(&mut self) {
        for set in self.sets.values_mut() {
            set.clear();
        }
    }

    /// A cube left: when it comes back, it says what has power there.
    pub fn cube_gone(&mut self, frame: u64, pos: IVec3) {
        if let Some(set) = self.sets.get_mut(&frame) {
            set.power.retain(|p, _| *p >> 4 != pos);
        }
    }

    /// The server's word on some pieces' power.
    pub fn power_changed(&mut self, frame: u64, changes: &[(IVec3, u8, u8)]) {
        self.sets.entry(frame).or_default().power_changed(changes);
    }

    /// Before the frame's passes: brings the buffers of this thing's wire up to date. There are
    /// none for what never had wire.
    pub fn update(&mut self, gpu: &mut dyn Backend, frame: u64, blocks: &mut ClientWorld, textures: &GameTextures) {
        if !self.sets.contains_key(&frame) && !blocks.has_wire_dirty() {
            return;
        }
        self.sets
            .entry(frame)
            .or_default()
            .update(gpu, blocks, textures);
    }

    /// What is on bodies that are gone goes with them.
    pub fn keep_frames(&mut self, gpu: &mut dyn Backend, there: impl Fn(u64) -> bool) {
        self.sets.retain(|&frame, set| {
            let keep = frame == 0 || there(frame);
            if !keep {
                set.destroy(gpu);
            }
            keep
        });
    }

    /// `frames`: where bodies' block (0, 0, 0) is drawn; `lit` the world's light where each is.
    pub fn draw(&self, list: &mut CommandList, cam: &Camera, scene: BufferId, frames: &[(u64, DVec3)], lit: &[u32]) {
        let Some(pipe) = self.pipe else { return };
        for (&frame, set) in &self.sets {
            if frame == 0 {
                set.draw(list, pipe, cam.pos, scene, None);
            } else if let Some(k) = frames.iter().position(|f| f.0 == frame) {
                set.draw(list, pipe, cam.pos - frames[k].1, scene, Some(lit[k]));
            }
        }
    }
}

impl WireSet {
    fn destroy(&mut self, gpu: &mut dyn Backend) {
        self.quads.destroy(gpu);
        self.levels.destroy(gpu);
    }

    fn clear(&mut self) {
        self.cubes.clear();
        self.power.clear();
        self.index.clear();
        self.numbers.clear();
        self.changed.clear();
        self.count = 0;
    }

    fn power_changed(&mut self, changes: &[(IVec3, u8, u8)]) {
        for &(cell, face, level) in changes {
            self.power.entry(cell).or_insert([0; 6])[face as usize % 6] = level;
            if let Some(&i) = self.index.get(&(cell, face)) {
                if self.numbers[i as usize] != level as u32 {
                    self.numbers[i as usize] = level as u32;
                    self.changed.push(i);
                }
            }
        }
    }

    fn rescan(&mut self, c: &Content, world: &mut ClientWorld) -> bool {
        let dirty = world.take_wire_dirty();
        for &cp in &dirty {
            let cells = world
                .cube_rev(cp)
                .map_or(Vec::new(), |(cube, _)| wire::cells(c, cp, cube));
            if cells.is_empty() {
                self.cubes.remove(&cp);
            } else {
                self.cubes.insert(cp, cells);
            }
        }
        !dirty.is_empty()
    }

    fn update(&mut self, gpu: &mut dyn Backend, world: &mut ClientWorld, textures: &GameTextures) {
        let c = content();
        if self.rescan(c, world) || std::mem::take(&mut self.stale) {
            self.build(gpu, c, world, textures);
            return;
        }
        if self.changed.is_empty() {
            return;
        }
        let Some(buf) = self.levels.id else { return };
        if self.changed.len() >= WHOLE_FROM {
            gpu.write_buffer(buf, 0, bytemuck::cast_slice(&self.numbers));
        } else {
            for &i in &self.changed {
                gpu.write_buffer(buf, i as u64 * 4, bytemuck::bytes_of(&self.numbers[i as usize]));
            }
        }
        self.changed.clear();
    }

    fn build(&mut self, gpu: &mut dyn Backend, c: &Content, world: &ClientWorld, textures: &GameTextures) {
        self.index.clear();
        self.numbers.clear();
        self.changed.clear();
        let looks = super::looks::current();
        let mut quads: Vec<GpuWireQuad> = Vec::new();
        let get = |p: IVec3| world.block_or_air(p);
        for cell in self.cubes.values().flatten().copied() {
            let Some(state) = get(cell) else { continue };
            let m = wire::mask(c, state);
            let info = &c.states[state as usize];
            // A pack may have given it a model of its own, which the mesher draws.
            let drawn = looks.drawn(state, info);
            if drawn.model != weft_abi::BlockModel::Wire {
                continue;
            }
            let (dot_tex, line_tex) = (drawn.textures[3], drawn.textures[0]);
            // Not in any mesh, so nothing else asks for these to be loaded.
            textures.ensure_blocks(gpu, [dot_tex, line_tex]);
            let (dot, line) = (textures.texture(dot_tex), textures.texture(line_tex));
            let light = world.light(cell);
            let power = self.power.get(&cell).copied().unwrap_or([0; 6]);
            for f in wire::dirs(m) {
                let piece = self.numbers.len() as u32;
                self.index.insert((cell, f), piece);
                self.numbers.push(power[f as usize] as u32);
                let reach = |e: u8| match wire::link(c, &get, cell, f, e) {
                    Link::Piece(..) => true,
                    _ => get(cell + DIRS[e as usize]).is_some_and(|n| reaches_device(c, n, e)),
                };
                let (u, v) = axes(f);
                let side = |axis: u8, positive: u8| reach(axis * 2 + positive);
                let (up, un, vp, vn) = (side(u, 1), side(u, 0), side(v, 1), side(v, 0));
                let mut push = |mode: u32, tex: u32| {
                    quads.push(GpuWireQuad { pos: [cell.x, cell.y, cell.z, f as i32 | (mode as i32) << 3], data: [tex, light, piece, 0] })
                };
                // A straight run is a line; anything else a dot with arms.
                let any_u = up || un;
                let any_v = vp || vn;
                if any_u && !any_v && up == un {
                    push(1, line);
                } else if any_v && !any_u && vp == vn {
                    push(0, line);
                } else {
                    push(6, dot);
                    for (on, mode) in [(up, 2), (un, 3), (vp, 4), (vn, 5)] {
                        if on {
                            push(mode, line);
                        }
                    }
                }
            }
        }
        self.count = quads.len() as u32;
        if quads.is_empty() {
            return;
        }
        let q = self
            .quads
            .fit(gpu, (quads.len() * size_of::<GpuWireQuad>()) as u64, "wire quads");
        gpu.write_buffer(q, 0, bytemuck::cast_slice(&quads));
        let l = self
            .levels
            .fit(gpu, (self.numbers.len() * 4) as u64, "wire power");
        gpu.write_buffer(l, 0, bytemuck::cast_slice(&self.numbers));
    }

    /// `eye`: the camera in the coordinates of these blocks.
    fn draw(&self, list: &mut CommandList, pipe: PipelineId, eye: DVec3, scene: BufferId, lit: Option<u32>) {
        let (Some(q), Some(l)) = (self.quads.id, self.levels.id) else { return };
        if self.count == 0 {
            return;
        }
        let origin = eye.floor();
        let frac: DVec3 = eye - origin;
        list.pipeline(pipe);
        list.bind(crate::gpu_layout::bind::wire::QUADS, q);
        list.bind(crate::gpu_layout::bind::wire::LEVELS, l);
        list.bind(crate::gpu_layout::bind::wire::SCENE, scene);
        list.push_constants(&Push {
            origin: [origin.x as i32, origin.y as i32, origin.z as i32, lit.unwrap_or(0) as i32],
            frac: [frac.x as f32, frac.y as f32, frac.z as f32, lit.is_some() as u32 as f32],
        });
        list.push(Cmd::Draw { vertices: 6, instances: self.count, first_vertex: 0, first_instance: 0 });
    }
}
