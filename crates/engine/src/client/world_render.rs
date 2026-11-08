//! Draws meshed cubes. All face records live in a few big storage-buffer pages; each frame the CPU
//! culls cubes and writes one indirect draw per (cube, layer) plus the cube origin it uses.
//! Cubes of moving structures are meshed in the structure's own coordinates and drawn moved
//! by its pose (an offset per structure, `origin.w` in the shader): a block placed on one
//! remeshes only its cube.

use glam::{DVec3, IVec3, Mat4, Vec3, Vec4};
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use weft_render::*;

use super::mesher::Mesh;
use super::textures::GameTextures;
use crate::content::Content;
pub use crate::gpu_layout::Globals;

const FACE_WORDS: u32 = 4;
const QUAD_WORDS: u32 = 8;
const PAGE_BYTES: u64 = 64 << 20;
/// How far from the camera, in blocks, translucent quads are kept sorted far to near. Past that
/// overlaps are too small to tell.
const SORT_REACH: f64 = 48.0;
/// A cube holds at most 6 * 4096 faces; plants and boxes add a few quads per block.
const MAX_QUADS_PER_DRAW: u32 = 6 * 4096 * 2;

#[derive(Clone, Copy)]
struct Alloc {
    page: u32,
    offset: u32,
    len: u32,
}

struct Pool {
    words: u32,
    pages: Vec<(BufferId, FreeList)>,
    label: &'static str,
}

/// Free ranges of a page by start and by (length, start): best fit and merging with the
/// neighbors in log time. A big edit frees and allocates tens of thousands of ranges at once,
/// which a scanned list took seconds for.
#[derive(Default)]
struct FreeList {
    by_start: std::collections::BTreeMap<u32, u32>,
    by_len: std::collections::BTreeSet<(u32, u32)>,
}

impl FreeList {
    fn add(&mut self, start: u32, len: u32) {
        self.by_start.insert(start, len);
        self.by_len.insert((len, start));
    }

    fn take(&mut self, start: u32, len: u32) {
        self.by_start.remove(&start);
        self.by_len.remove(&(len, start));
    }
}

impl Pool {
    fn records_per_page(&self) -> u32 {
        (PAGE_BYTES / (self.words as u64 * 4)) as u32
    }

    fn alloc(&mut self, gpu: &mut dyn Backend, n: u32) -> Alloc {
        let per = self.records_per_page();
        assert!(n <= per, "mesh larger than a page");
        for (pi, (_, free)) in self.pages.iter_mut().enumerate() {
            if let Some(&(len, start)) = free.by_len.range((n, 0)..).next() {
                free.take(start, len);
                if len > n {
                    free.add(start + n, len - n);
                }
                return Alloc { page: pi as u32, offset: start, len: n };
            }
        }
        let buf = gpu.create_buffer(PAGE_BYTES, usage::STORAGE, self.label);
        let mut free = FreeList::default();
        if per > n {
            free.add(n, per - n);
        }
        self.pages.push((buf, free));
        Alloc { page: self.pages.len() as u32 - 1, offset: 0, len: n }
    }

    fn free(&mut self, a: Alloc) {
        let free = &mut self.pages[a.page as usize].1;
        let (mut start, mut len) = (a.offset, a.len);
        if let Some((&s, &l)) = free
            .by_start
            .range(..start)
            .next_back()
            .filter(|(s, l)| **s + **l == start)
        {
            free.take(s, l);
            (start, len) = (s, l + len);
        }
        if let Some(&l) = free.by_start.get(&(start + len)) {
            free.take(start + len, l);
            len += l;
        }
        free.add(start, len);
    }
}

#[derive(Clone, Copy)]
struct GpuMesh {
    /// Its places in `CubeTable`, none for `u32::MAX`.
    slots: [u32; 4],
    faces: Option<Alloc>,
    /// Per layer, per direction rank (see `mesher::DIR_RANK`).
    face_counts: [[u32; 6]; 3],
    quads: Option<Alloc>,
    quad_counts: [u32; 3],
}

#[derive(Clone, Copy)]
pub struct Camera {
    pub pos: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
    /// Walking sway in view space: offset, roll and extra pitch (vanilla's view bobbing).
    pub bob: (Vec3, f32, f32),
}

impl Camera {
    pub fn forward(&self) -> Vec3 {
        crate::physics::look_dir(self.yaw, self.pitch).as_vec3()
    }

    /// The walking sway alone, in view space: the world moves by it, and so does the hand.
    pub fn bobbing(&self) -> Mat4 {
        let (offset, roll, pitch) = self.bob;
        Mat4::from_translation(offset) * Mat4::from_rotation_z(roll) * Mat4::from_rotation_x(pitch)
    }

    pub fn view(&self) -> Mat4 {
        self.bobbing() * glam::camera::rh::view::look_to_mat4(Vec3::ZERO, self.forward(), Vec3::Y)
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        // Y-up clip space: naga already flips Y for Vulkan.
        glam::camera::rh::proj::directx::perspective_infinite_reverse(self.fov.to_radians(), aspect, 0.05)
    }
}

/// One indirect draw: kind (0 faces, 1 quads, 3 distant terrain), layer, page, arguments, cube origin (w: offset
/// index), cube center relative to the camera.
type DrawRecord = (u8, u8, u32, DrawIndexedArgs, [i32; 4], Vec3);

/// A mapped buffer written from every core, each record by one.
#[derive(Clone, Copy)]
struct Mapped(*mut u8);

unsafe impl Send for Mapped {}
unsafe impl Sync for Mapped {}

impl Mapped {
    /// SAFETY: `off` + what is written there stays inside the buffer.
    unsafe fn at(&self, off: usize) -> *mut u8 {
        unsafe { self.0.add(off) }
    }
}

/// Meshes are kept in regions of this many cubes a side (log2), culled as a whole first.
const REGION: i32 = 3;

fn region_of(key: MeshKey) -> (u64, IVec3) {
    (key.0, key.1 >> REGION)
}

/// The draws of one visible mesh (its corner `lo` relative to the camera, `k` its structure or
/// 0), returning the faces drawn. A face direction can be seen only from the side its normal
/// points to; contiguous visible runs become one draw.
/// The draws the CPU makes of a mesh: a structure's all, and of the world's what is translucent, the
/// rest being `CubeTable`'s.
fn mesh_draws(m: &GpuMesh, key: MeshKey, lo: Vec3, k: i32, cam_cube: IVec3, out: &mut Vec<DrawRecord>) -> u64 {
    let first_layer = if k == 0 { 2 } else { 0 };
    let rel = if k == 0 { key.1 - cam_cube } else { key.1 };
    let origin = [rel.x, rel.y, rel.z, k];
    let center = lo + Vec3::splat(8.0);
    let mut faces = 0u64;
    let args = |s: u32, len: u32| DrawIndexedArgs {
        indices: len * 6,
        instances: 1,
        first_index: 0,
        vertex_offset: (s * 4) as i32,
        first_instance: 0,
    };
    if let Some(a) = m.faces {
        // Directions by rank: +X +Y +Z -X -Y -Z.
        let seen = [lo.x < 0.0, lo.y < 0.0, lo.z < 0.0, lo.x > -16.0, lo.y > -16.0, lo.z > -16.0];
        let mut off = a.offset;
        for layer in 0..3 {
            let counts = m.face_counts[layer];
            if layer < first_layer {
                off += counts.iter().sum::<u32>();
                continue;
            }
            let mut run: Option<(u32, u32)> = None;
            for rank in 0..6 {
                let n = counts[rank];
                if n > 0 && seen[rank] {
                    faces += n as u64;
                    run = Some(match run {
                        Some((s, len)) => (s, len + n),
                        None => (off, n),
                    });
                } else if n > 0 {
                    if let Some((s, len)) = run.take() {
                        out.push((0, layer as u8, a.page, args(s, len), origin, center));
                    }
                }
                off += n;
            }
            if let Some((s, len)) = run {
                out.push((0, layer as u8, a.page, args(s, len), origin, center));
            }
        }
    }
    if let Some(a) = m.quads {
        let mut off = a.offset;
        for layer in 0..3 {
            let n = m.quad_counts[layer];
            if n > 0 && layer >= first_layer {
                out.push((1, layer as u8, a.page, args(off, n), origin, center));
            }
            off += n;
        }
    }
    faces
}

/// A mesh's place: the structure (entity) it belongs to, 0 for the world, and its cube.
pub type MeshKey = (u64, IVec3);

/// A cube's translucent quads and where they are on the GPU.
struct SortedQuads {
    page: u32,
    offset: u32,
    quads: Vec<[u32; 8]>,
    /// The block the camera was in when they were put in order, in the cube's own coordinates.
    seen_from: Option<IVec3>,
}

/// Which blocks around the camera cast shadows, kept on the GPU for shader packs (`solid_at`
/// in the world shader's prelude); only while the chosen pack's world shader uses it. 16^3 cube
/// slots addressed by cube position modulo 16: first a tag per slot (cube x y z, 1 when
/// filled), then 128 words of bits per slot in `cube::index` order.
struct Solid {
    buf: BufferId,
    on: bool,
    /// Per slot: the cube it holds and the `cube_rev` of its blocks then.
    held: Vec<Option<(IVec3, u32)>>,
    /// The tags as uploaded: sent whole when one changes, since copies within a frame have no
    /// order between them and a slot's tag must never lose to an older write.
    tags: Vec<[u32; 4]>,
    /// Per block state: casts a shadow.
    casts: Vec<bool>,
}

const SOLID_SLOTS: usize = 4096;
/// Cubes rebuilt per frame at most; the rest follow in the next frames.
const SOLID_PER_FRAME: usize = 192;

impl Solid {
    fn new(gpu: &mut dyn Backend, c: &Content) -> Solid {
        let casts = c
            .states
            .iter()
            .map(|st| st.opacity > 0 && st.liquid.is_none())
            .collect();
        let mut s = Solid { buf: gpu.create_buffer(4, usage::STORAGE, "solid"), on: false, held: Vec::new(), tags: Vec::new(), casts };
        s.set_on(gpu, crate::packs::pack_uses("world.wgsl", "solid_at"));
        s
    }

    fn set_on(&mut self, gpu: &mut dyn Backend, on: bool) {
        if on == self.on {
            return;
        }
        gpu.destroy_buffer(self.buf);
        self.on = on;
        self.held = vec![None; if on { SOLID_SLOTS } else { 0 }];
        self.tags = vec![[0; 4]; if on { SOLID_SLOTS } else { 0 }];
        let words = if on { SOLID_SLOTS * (4 + 128) } else { 1 };
        self.buf = gpu.create_buffer(words as u64 * 4, usage::STORAGE, "solid");
        gpu.write_buffer(self.buf, 0, &vec![0u8; words * 4]);
    }
}

pub struct WorldRenderer {
    face_pipes: [PipelineId; 3],
    quad_pipes: [PipelineId; 3],
    lod_pipe: PipelineId,
    /// Distant terrain tiles (`lod`): their quads (in `faces`, four words like a face), count
    /// and the heights they span; the ones to draw this frame; and the columns whose cubes show
    /// their ground (`Lod::mask`, half its width), where their pixels are left to the cubes.
    lod: FxHashMap<super::lod::TileKey, (Alloc, u32, (i32, i32))>,
    pub lod_draw: Vec<super::lod::TileKey>,
    pub lod_half: u32,
    lod_mask: (BufferId, usize),
    faces: Pool,
    quads: Pool,
    /// Index into its region's meshes by key. A frame culls the regions, then goes through the
    /// meshes of the ones seen, nearest first, in parallel.
    meshes: FxHashMap<MeshKey, usize>,
    regions: FxHashMap<(u64, IVec3), Vec<(MeshKey, GpuMesh)>>,
    /// Meshes the CPU has nothing to do for each frame, there being no translucent draws: they are
    /// in `table` and not in `regions`.
    table_only: FxHashMap<MeshKey, GpuMesh>,
    table: CubeTable,
    globals: Vec<BufferId>,
    origins: Vec<(BufferId, u32)>,
    /// Per frame: camera-relative positions of the structures drawn (index 0: none, the world).
    offsets: Vec<(BufferId, u32)>,
    indirect: Vec<(BufferId, u32)>,
    /// Not drawing what the last frame shows to be behind something.
    pub occlusion: Occlusion,
    /// Of the last `draw`: its runs (kind, layer, page, first record, records), and its
    /// translucent draws, sorted far to near.
    runs: Vec<(u8, u8, u32, u32, u32)>,
    trans: Vec<DrawRecord>,
    /// Translucent quads by cube, to keep them in order.
    sorted: FxHashMap<MeshKey, SortedQuads>,
    /// The block the camera was in at the last sort, unless there is more to sort since.
    sorted_from: Option<IVec3>,
    /// Draws of each region seen, kept between frames.
    parts: Vec<Vec<DrawRecord>>,
    /// Of the last `draw`: where its translucent draws start in `scratch.2`, and the buffers
    /// they use (globals, origins, indirect, offsets, the frame's scene).
    translucent: usize,
    buffers: Option<(BufferId, BufferId, BufferId, BufferId, BufferId)>,
    /// Biome table, then per block column of the loaded area its blended colors: tiles of
    /// 16x16 blocks addressed by column position modulo `tint_tiles`.
    biomes: BufferId,
    biome_table: Vec<u32>,
    tint_tiles: u32,
    tex_info: BufferId,
    anim: BufferId,
    /// Start of the overlay table in `anim`.
    overlays: u32,
    /// Block shader slot per texture id (`shader_table`).
    tex_shader: BufferId,
    /// `GameTextures::glow_buffer`, bound as it is.
    glow: BufferId,
    /// `GameTextures::generation` the tables were built from.
    tex_generation: u64,
    /// (0 1 2)(0 2 3) for as many quads as one draw can hold; every face and quad draw shares it
    /// so each corner is shaded once instead of once per triangle.
    quad_index: BufferId,
    solid: Solid,
    pub drawn: u32,
    pub faces_drawn: u64,
}

/// Block shaders: `fn shade(p: BlockPixel) -> vec4<f32>` in `shaders/block/<name>.wgsl`, for
/// blocks with a `shader` component.
pub const HOOK: crate::shaders::Hook = crate::shaders::Hook {
    kind: "block",
    entry: "shade",
    dispatch: "block_shade",
    params: "p: BlockPixel",
    args: "p",
    ret: "vec4<f32>",
    fallback: "return vec4(p.albedo.rgb * p.light, p.albedo.a);",
};

pub fn world_shader(assets: &crate::assets::Assets) -> Result<(Shader, crate::shaders::Slots), ShaderError> {
    let snippets = crate::shaders::snippets(assets, &HOOK);
    let (s, mut slots) = crate::packs::compose(assets, "world.wgsl", &[(&HOOK, &snippets)])?;
    Ok((s, slots.pop().unwrap_or_default()))
}

/// Block shader slot per texture id: a block's `shader` goes with its textures.
fn shader_table(gpu: &mut dyn Backend, c: &Content, textures: &GameTextures, slots: &crate::shaders::Slots) -> BufferId {
    let mut table = vec![0u32; textures.count().max(1)];
    if let Some(comp) = c.components.id(weft_abi::comp::SHADER) {
        for (i, s) in c.states.iter().enumerate() {
            let Some(name) = c.components.text(crate::components::BLOCK, i as u32, comp) else { continue };
            let slot = slots.slot(name);
            if slot == 0 {
                log::warn!("block {}: no block shader {name}", c.block_of(i as u32).name);
                continue;
            }
            for t in s
                .textures
                .iter()
                .chain(s.boxes.iter().flat_map(|b| b.textures.iter()))
            {
                if let Some(e) = table.get_mut(*t as usize) {
                    *e = slot;
                }
            }
        }
    }
    let buf = gpu.create_buffer((table.len() * 4) as u64, usage::STORAGE, "texture shaders");
    gpu.write_buffer(buf, 0, bytemuck::cast_slice(&table));
    buf
}

impl WorldRenderer {
    pub fn new(
        gpu: &mut dyn Backend,
        c: &Content,
        textures: &GameTextures,
        assets: &crate::assets::Assets,
    ) -> Result<WorldRenderer, String> {
        let (shader, slots) = world_shader(assets).map_err(|e| e.0)?;
        let (face_pipes, quad_pipes, lod_pipe) = Self::pipelines(gpu, &shader)?;
        let tex_shader = shader_table(gpu, c, textures, &slots);

        let (tex_info, anim, overlays) = Self::texture_info(gpu, textures);

        // Tint kind colors per biome, then the blended columns (`set_tint_tiles`).
        let biome_data: Vec<u32> = if c.biomes.is_empty() {
            c.tint_kinds.iter().map(|k| k.1).collect()
        } else {
            c.biomes
                .iter()
                .flat_map(|b| b.tints.iter().copied())
                .collect()
        };
        let biomes = gpu.create_buffer((biome_data.len() * 4) as u64, usage::STORAGE, "biomes");
        gpu.write_buffer(biomes, 0, bytemuck::cast_slice(&biome_data));

        let indices: Vec<u32> = (0..MAX_QUADS_PER_DRAW)
            .flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i))
            .collect();
        let quad_index = gpu.create_buffer((indices.len() * 4) as u64, usage::INDEX, "quad indices");
        gpu.write_buffer(quad_index, 0, bytemuck::cast_slice(&indices));

        let frames = gpu.frames_in_flight();
        let globals = (0..frames)
            .map(|_| gpu.create_buffer(size_of::<Globals>() as u64, usage::STORAGE | usage::HOST, "globals"))
            .collect();
        Ok(WorldRenderer {
            face_pipes,
            quad_pipes,
            lod_pipe,
            lod: FxHashMap::default(),
            lod_draw: Vec::new(),
            lod_half: 0,
            lod_mask: (gpu.create_buffer(4, usage::STORAGE, "lod mask"), 1),
            faces: Pool { words: FACE_WORDS, pages: Vec::new(), label: "face page" },
            quads: Pool { words: QUAD_WORDS, pages: Vec::new(), label: "quad page" },
            meshes: FxHashMap::default(),
            regions: FxHashMap::default(),
            table_only: FxHashMap::default(),
            table: CubeTable::new(gpu),
            parts: Vec::new(),
            offsets: Vec::new(),
            globals,
            origins: Vec::new(),
            indirect: Vec::new(),
            occlusion: Occlusion::new(gpu, assets)?,
            runs: Vec::new(),
            trans: Vec::new(),
            sorted: FxHashMap::default(),
            sorted_from: None,
            translucent: 0,
            buffers: None,
            biomes,
            biome_table: biome_data,
            tint_tiles: 0,
            tex_info,
            anim,
            overlays,
            glow: textures.glow_buffer(),
            tex_shader,
            tex_generation: textures.generation(),
            quad_index,
            solid: Solid::new(gpu, c),
            drawn: 0,
            faces_drawn: 0,
        })
    }

    /// The block texture tables (`GameTextures::world_tables`) on the GPU.
    fn texture_info(gpu: &mut dyn Backend, textures: &GameTextures) -> (BufferId, BufferId, u32) {
        let (info, frames, overlays) = textures.world_tables();
        let tex_info = gpu.create_buffer((info.len().max(1) * 4) as u64, usage::STORAGE, "texture info");
        gpu.write_buffer(tex_info, 0, bytemuck::cast_slice(&info));
        let anim = gpu.create_buffer((frames.len() * 4) as u64, usage::STORAGE, "texture frames");
        gpu.write_buffer(anim, 0, bytemuck::cast_slice(&frames));
        (tex_info, anim, overlays)
    }

    /// Builds the texture tables again when textures were loaded since (cheap: a few words per
    /// block texture).
    pub fn set_textures(&mut self, gpu: &mut dyn Backend, textures: &GameTextures) {
        self.glow = textures.glow_buffer();
        if self.tex_generation == textures.generation() {
            return;
        }
        gpu.destroy_buffer(self.tex_info);
        gpu.destroy_buffer(self.anim);
        (self.tex_info, self.anim, self.overlays) = Self::texture_info(gpu, textures);
        self.tex_generation = textures.generation();
    }

    fn pipelines(gpu: &mut dyn Backend, shader: &Shader) -> Result<([PipelineId; 3], [PipelineId; 3], PipelineId), String> {
        let depth_write = Some(DepthState { write: true, compare: Compare::GreaterEqual });
        let depth_read = Some(DepthState { write: false, compare: Compare::GreaterEqual });
        let mut make = |label: &str, vs: &str, fs: &str, depth, blend, cull| {
            gpu.create_pipeline(&PipelineDesc {
                label,
                shader,
                vertex: vs,
                fragment: Some(fs),
                color: Some(Format::Surface),
                depth,
                blend,
                cull_back: cull,
                topology: Topology::Triangles,
            })
            .map_err(|e| e.0)
        };
        let faces = [
            make("faces opaque", "vs_face", "fs_opaque", depth_write, Blend::None, true)?,
            make("faces cutout", "vs_face", "fs_cutout", depth_write, Blend::None, true)?,
            make("faces translucent", "vs_face", "fs_translucent", depth_read, Blend::Alpha, true)?,
        ];
        let quads = [
            make("quads opaque", "vs_quad", "fs_opaque", depth_write, Blend::None, true)?,
            make("quads cutout", "vs_quad", "fs_cutout", depth_write, Blend::None, true)?,
            make("quads translucent", "vs_quad", "fs_translucent", depth_read, Blend::Alpha, true)?,
        ];
        let lod = make("distant terrain", "vs_lod", "fs_lod", depth_write, Blend::None, true)?;
        Ok((faces, quads, lod))
    }

    /// Recompiles the world shader; keeps the old pipelines when the new source has errors.
    pub fn reload_shaders(&mut self, gpu: &mut dyn Backend, textures: &GameTextures, assets: &crate::assets::Assets) {
        self.occlusion.reload(gpu, assets);
        match world_shader(assets)
            .map_err(|e| e.0)
            .and_then(|(s, slots)| Ok((Self::pipelines(gpu, &s)?, slots)))
        {
            Ok(((f, q, l), slots)) => {
                for p in self
                    .face_pipes
                    .into_iter()
                    .chain(self.quad_pipes)
                    .chain([self.lod_pipe])
                {
                    gpu.destroy_pipeline(p);
                }
                self.face_pipes = f;
                self.quad_pipes = q;
                self.lod_pipe = l;
                gpu.destroy_buffer(self.tex_shader);
                self.tex_shader = shader_table(gpu, crate::content::content(), textures, &slots);
                self.solid
                    .set_on(gpu, crate::packs::pack_uses("world.wgsl", "solid_at"));
                log::info!("world shader reloaded");
            }
            Err(e) => log::error!("world shader: {e}"),
        }
    }

    /// Sizes the blended color area to `tiles` x `tiles` columns; true when it was reallocated
    /// and every column has to be uploaded again.
    pub fn set_tint_tiles(&mut self, gpu: &mut dyn Backend, tiles: u32) -> bool {
        if tiles == self.tint_tiles {
            return false;
        }
        gpu.destroy_buffer(self.biomes);
        let size = (self.biome_table.len() + (tiles * tiles) as usize * 256 * crate::content::content().tint_kinds.len()) * 4;
        self.biomes = gpu.create_buffer(size as u64, usage::STORAGE, "biomes");
        gpu.write_buffer(self.biomes, 0, bytemuck::cast_slice(&self.biome_table));
        // Zero marks a block without blended colors yet.
        gpu.write_buffer(self.biomes, self.biome_table.len() as u64 * 4, &vec![0u8; size - self.biome_table.len() * 4]);
        self.tint_tiles = tiles;
        true
    }

    pub fn upload_tints(&mut self, gpu: &mut dyn Backend, col: glam::IVec2, tints: &super::world::ColumnTints) {
        let t = self.tint_tiles as i32;
        let slot = (col.x.rem_euclid(t) + col.y.rem_euclid(t) * t) as usize;
        gpu.write_buffer(self.biomes, ((self.biome_table.len() + slot * tints.len()) * 4) as u64, bytemuck::cast_slice(tints));
    }

    pub fn remove(&mut self, key: MeshKey) {
        let m = if let Some(m) = self.table_only.remove(&key) {
            m
        } else {
            let Some(i) = self.meshes.remove(&key) else { return };
            self.sorted.remove(&key);
            let r = region_of(key);
            let list = self.regions.get_mut(&r).unwrap();
            let (_, m) = list.swap_remove(i);
            if let Some(&(moved, _)) = list.get(i) {
                self.meshes.insert(moved, i);
            }
            if list.is_empty() {
                self.regions.remove(&r);
            }
            m
        };
        self.table.remove(m.slots);
        if let Some(a) = m.faces {
            self.faces.free(a);
        }
        if let Some(a) = m.quads {
            self.quads.free(a);
        }
    }

    pub fn destroy(&mut self, gpu: &mut dyn Backend) {
        self.meshes.clear();
        self.regions.clear();
        self.table_only.clear();
        self.table.destroy(gpu);
        self.lod.clear();
        for (b, _) in self.faces.pages.drain(..).chain(self.quads.pages.drain(..)) {
            gpu.destroy_buffer(b);
        }
        for b in self.globals.drain(..).chain([
            self.biomes,
            self.tex_info,
            self.anim,
            self.quad_index,
            self.tex_shader,
            self.lod_mask.0,
            self.solid.buf,
        ]) {
            gpu.destroy_buffer(b);
        }
        for (b, _) in self
            .origins
            .drain(..)
            .chain(self.indirect.drain(..))
            .chain(self.offsets.drain(..))
        {
            gpu.destroy_buffer(b);
        }
        self.occlusion.destroy(gpu);
        for p in self
            .face_pipes
            .into_iter()
            .chain(self.quad_pipes)
            .chain([self.lod_pipe])
        {
            gpu.destroy_pipeline(p);
        }
    }

    /// Brings `Solid` up to date for the cubes within 7 of `center` (their slots are distinct).
    pub fn update_solid(&mut self, gpu: &mut dyn Backend, world: &super::world::ClientWorld, center: IVec3) {
        let s = &mut self.solid;
        if !s.on {
            return;
        }
        let mut rebuilt = 0;
        let mut bits = [0u32; 128];
        // Nearest first, so a frame out of budget has fixed what is close.
        'shells: for r in 0i32..=7 {
            for dy in -r..=r {
                for dz in -r..=r {
                    for dx in -r..=r {
                        if dx.abs().max(dy.abs()).max(dz.abs()) != r {
                            continue;
                        }
                        let p = center + IVec3::new(dx, dy, dz);
                        let slot = ((p.y & 15) * 256 + (p.z & 15) * 16 + (p.x & 15)) as usize;
                        let now = world.cube_rev(p);
                        let key = now.map(|(_, v)| (p, v));
                        if s.held[slot] == key {
                            continue;
                        }
                        if rebuilt >= SOLID_PER_FRAME {
                            break 'shells;
                        }
                        rebuilt += 1;
                        s.held[slot] = key;
                        let Some((cube, _)) = now else {
                            s.tags[slot] = [0; 4];
                            continue;
                        };
                        let casts = |st: u32| s.casts.get(st as usize).copied().unwrap_or(false);
                        match cube.uniform_state() {
                            Some(st) => bits.fill(if casts(st) { !0 } else { 0 }),
                            None => {
                                bits.fill(0);
                                for i in 0..crate::world::cube::VOLUME {
                                    if casts(cube.get(i)) {
                                        bits[i / 32] |= 1 << (i % 32);
                                    }
                                }
                            }
                        }
                        gpu.write_buffer(s.buf, ((SOLID_SLOTS * 4 + slot * 128) * 4) as u64, bytemuck::cast_slice(&bits));
                        s.tags[slot] = [p.x as u32, p.y as u32, p.z as u32, 1];
                    }
                }
            }
        }
        if rebuilt > 0 {
            gpu.write_buffer(s.buf, 0, bytemuck::cast_slice(&s.tags));
        }
    }

    /// A distant terrain tile's quads (`lod::mesh`), spanning heights `y`.
    pub fn upload_lod(
        &mut self,
        gpu: &mut dyn Backend,
        textures: &GameTextures,
        key: super::lod::TileKey,
        quads: &[[u32; 4]],
        y: (i32, i32),
    ) {
        self.remove_lod(key);
        if quads.is_empty() {
            return;
        }
        let mut used: Vec<u32> = quads
            .iter()
            .map(|q| q[1] & crate::gpu_layout::face::TEX_MASK)
            .collect();
        used.sort_unstable();
        used.dedup();
        textures.ensure_blocks(gpu, used);
        let a = self.faces.alloc(gpu, quads.len() as u32);
        gpu.write_buffer(self.faces.pages[a.page as usize].0, a.offset as u64 * 16, bytemuck::cast_slice(quads));
        self.lod.insert(key, (a, quads.len() as u32, y));
    }

    /// The columns shown by cubes (`Lod::mask`).
    pub fn set_lod_mask(&mut self, gpu: &mut dyn Backend, mask: &[u32]) {
        if self.lod_mask.1 < mask.len() {
            gpu.destroy_buffer(self.lod_mask.0);
            self.lod_mask = (gpu.create_buffer((mask.len() * 4) as u64, usage::STORAGE, "lod mask"), mask.len());
        }
        gpu.write_buffer(self.lod_mask.0, 0, bytemuck::cast_slice(mask));
    }

    /// Distant terrain tiles held and their quads.
    pub fn lod_counts(&self) -> (usize, usize) {
        (self.lod.len(), self.lod.values().map(|t| t.1 as usize).sum())
    }

    pub fn remove_lod(&mut self, key: super::lod::TileKey) {
        if let Some((a, _, _)) = self.lod.remove(&key) {
            self.faces.free(a);
        }
    }

    /// Every cube of structure `frame` goes (it left).
    pub fn remove_frame(&mut self, frame: u64) {
        let keys: Vec<MeshKey> = self
            .regions
            .iter()
            .filter(|(r, _)| r.0 == frame)
            .flat_map(|(_, l)| l.iter().map(|e| e.0))
            .collect();
        for k in keys {
            self.remove(k);
        }
    }

    /// Translucent quads near the camera go far to near again when it has come into another block
    /// of their cube, as vanilla does. `frames`: where the structures drawn are.
    fn sort_translucent(&mut self, gpu: &mut dyn Backend, cam: DVec3, frames: &[(u64, DVec3)]) {
        let at = cam.floor().as_ivec3();
        if self.sorted_from == Some(at) && frames.is_empty() {
            return;
        }
        self.sorted_from = Some(at);
        let began = std::time::Instant::now();
        let redone: Vec<MeshKey> = self
            .sorted
            .par_iter_mut()
            .filter_map(|(key, s)| {
                let origin = if key.0 == 0 { DVec3::ZERO } else { frames.iter().find(|f| f.0 == key.0)?.1 };
                let here = cam - origin - (key.1 * 16).as_dvec3();
                if (here - DVec3::splat(8.0)).length() > SORT_REACH + 14.0 {
                    return None;
                }
                let block = here.floor().as_ivec3();
                if s.seen_from == Some(block) {
                    return None;
                }
                s.seen_from = Some(block);
                let eye = (here * 32.0).as_ivec3();
                s.quads.sort_unstable_by_key(|q| {
                    let m = crate::gpu_layout::quad_middle(q);
                    let d = [m[0] - eye.x, m[1] - eye.y, m[2] - eye.z].map(|v| v as i64);
                    std::cmp::Reverse(d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
                });
                Some(*key)
            })
            .collect();
        let mut quads = 0;
        for key in &redone {
            let s = &self.sorted[key];
            quads += s.quads.len();
            gpu.write_buffer(self.quads.pages[s.page as usize].0, s.offset as u64 * 32, bytemuck::cast_slice(&s.quads));
        }
        if !redone.is_empty() {
            log::debug!(target: "frame", "translucent: {} cubes, {quads} quads sorted in {:.2} ms", redone.len(), began.elapsed().as_secs_f64() * 1000.0);
        }
    }

    pub fn upload(&mut self, gpu: &mut dyn Backend, textures: &GameTextures, key: MeshKey, mesh: &Mesh) {
        self.remove(key);
        if mesh.is_empty() {
            return;
        }
        // The textures it shows are loaded before it is drawn.
        let mut used: Vec<u32> = mesh
            .faces
            .iter()
            .flatten()
            .map(|f| f[1] & crate::gpu_layout::face::TEX_MASK)
            .chain(
                mesh.quads
                    .iter()
                    .flatten()
                    .map(|q| q[4] & crate::gpu_layout::face::TEX_MASK),
            )
            .collect();
        used.sort_unstable();
        used.dedup();
        textures.ensure_blocks(gpu, used);
        let face_counts = mesh.faces.each_ref().map(|v| {
            let mut n = [0u32; 6];
            for f in v {
                n[super::mesher::DIR_RANK[crate::gpu_layout::face_dir(f)] as usize] += 1;
            }
            n
        });
        let quad_counts = mesh.quads.each_ref().map(|v| v.len() as u32);
        let total_f: u32 = face_counts.iter().flatten().sum();
        let total_q: u32 = quad_counts.iter().sum();
        let faces = (total_f > 0).then(|| {
            let a = self.faces.alloc(gpu, total_f);
            let data: Vec<[u32; 4]> = mesh.faces.concat();
            gpu.write_buffer(self.faces.pages[a.page as usize].0, a.offset as u64 * 16, bytemuck::cast_slice(&data));
            a
        });
        let quads = (total_q > 0).then(|| {
            let a = self.quads.alloc(gpu, total_q);
            let data: Vec<[u32; 8]> = mesh.quads.concat();
            gpu.write_buffer(self.quads.pages[a.page as usize].0, a.offset as u64 * 32, bytemuck::cast_slice(&data));
            if !mesh.quads[2].is_empty() {
                self.sorted.insert(
                    key,
                    SortedQuads {
                        page: a.page,
                        offset: a.offset + quad_counts[0] + quad_counts[1],
                        quads: mesh.quads[2].clone(),
                        seen_from: None,
                    },
                );
                self.sorted_from = None;
            }
            a
        });
        let slots = if key.0 == 0 {
            self.table
                .add(key.1, faces, &face_counts, quads, &quad_counts)
        } else {
            [u32::MAX; 4]
        };
        let mesh = GpuMesh { slots, faces, face_counts, quads, quad_counts };
        if key.0 == 0 && face_counts[2].iter().sum::<u32>() + quad_counts[2] == 0 {
            self.table_only.insert(key, mesh);
            return;
        }
        let list = self.regions.entry(region_of(key)).or_default();
        self.meshes.insert(key, list.len());
        list.push((key, mesh));
    }

    /// `frames`: structures to draw and where their block (0, 0, 0) is in the world.
    pub fn draw(
        &mut self,
        gpu: &mut dyn Backend,
        list: &mut CommandList,
        early: &mut CommandList,
        frame: &FrameInfo,
        cam: &Camera,
        scene: BufferId,
        frames: &[(u64, DVec3)],
        lights: &FxHashMap<MeshKey, u32>,
    ) {
        self.sort_translucent(gpu, cam.pos, frames);
        let aspect = frame.width as f32 / frame.height.max(1) as f32;
        let view_proj = cam.proj(aspect) * cam.view();
        let last_frame = self.occlusion.next_frame(view_proj, cam.pos);
        let cam_block = cam.pos.floor();
        let cam_cube: IVec3 = cam_block.as_ivec3() >> 4i32;
        let cam_frac = (cam.pos - (cam_cube * 16).as_dvec3()).as_vec3();
        let globals = Globals {
            cam_frac: [cam_frac.x, cam_frac.y, cam_frac.z, 0.0],
            ids: [0, 0, (self.biome_table.len() / crate::content::content().tint_kinds.len()) as u32, self.tint_tiles],
            cam_cube: cam_cube.extend(0).to_array(),
            extra: [self.overlays, 0, crate::content::content().tint_kinds.len() as u32, self.lod_half],
        };
        let gbuf = self.globals[frame.slot];
        gpu.mapped(gbuf)[..size_of::<Globals>()].copy_from_slice(bytemuck::bytes_of(&globals));

        let planes = frustum_planes(view_proj);
        // Camera-relative offset of each structure drawn (k + 1 its index; 0 the world).
        let frame_offsets: Vec<Vec3> = frames
            .iter()
            .map(|f| (f.1 - (cam_cube * 16).as_dvec3()).as_vec3())
            .collect();
        // What a structure's meshes pick: its place and the world's light on that cube of it,
        // full sky until it is known.
        let mut picks: Vec<(Vec3, u32)> = frame_offsets
            .iter()
            .map(|o| (*o, crate::world::light::SKY_FULL as u32))
            .collect();
        let mut cube_pick: FxHashMap<MeshKey, i32> = FxHashMap::default();
        for (&key, &light) in lights {
            if let Some(k) = frames.iter().position(|f| f.0 == key.0) {
                picks.push((frame_offsets[k], light));
                cube_pick.insert(key, picks.len() as i32);
            }
        }
        // Regions in view, nearest first: what is drawn first then hides what is behind it.
        let side = (16 << REGION) as f32;
        let mut seen: Vec<(f32, &Vec<(MeshKey, GpuMesh)>)> = self
            .regions
            .iter()
            .filter_map(|(&(f, r), list)| {
                if f != 0 {
                    return Some((0.0, list));
                }
                let center = ((r << REGION) - cam_cube).as_vec3() * 16.0 - cam_frac + Vec3::splat(side / 2.0);
                planes
                    .iter()
                    .all(|pl| pl.dot(center.extend(1.0)) > -side * 0.8661)
                    .then(|| (center.length_squared(), list))
            })
            .collect();
        seen.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));

        // A list per region seen, kept between frames: allocating them anew on every core each
        // frame cost more than the culling.
        let mut parts = std::mem::take(&mut self.parts);
        parts.resize_with(seen.len().max(parts.len()), Vec::new);
        let faces_drawn: u64 = seen
            .par_iter()
            .zip(parts.par_iter_mut())
            .map(|((_, list), out)| {
                out.clear();
                let mut faces = 0u64;
                for &(key, m) in list.iter() {
                    let (lo, k) = if key.0 == 0 {
                        ((key.1 - cam_cube).as_vec3() * 16.0 - cam_frac, 0)
                    } else {
                        let Some(k) = frames.iter().position(|f| f.0 == key.0) else { continue };
                        (frame_offsets[k] + key.1.as_vec3() * 16.0 - cam_frac, cube_pick.get(&key).copied().unwrap_or(k as i32 + 1))
                    };
                    let center = lo + Vec3::splat(8.0);
                    if planes.iter().all(|pl| pl.dot(center.extend(1.0)) > -13.86) {
                        faces += mesh_draws(&m, key, lo, k, cam_cube, out);
                    }
                }
                faces
            })
            .sum();
        // Into runs of one pipeline and page (layer, kind, page; translucent last), written
        // straight into the mapped buffers from every core: each region knows where its draws
        // of each run go, so the near-to-far order holds inside runs without a sort.
        let seen_parts = &parts[..seen.len()];
        let pages = self.faces.pages.len().max(self.quads.pages.len()).max(1);
        // Opaque and cutout runs; translucent ones are sorted far to near over all of them.
        let nb = 4 * pages;
        let bucket = |d: &DrawRecord| (d.1 as usize * 2 + d.0 as usize) * pages + d.2 as usize;
        let counts: Vec<Vec<u32>> = seen_parts
            .par_iter()
            .map(|p| {
                let mut c = vec![0u32; nb];
                for d in p.iter().filter(|d| d.1 < 2) {
                    c[bucket(d)] += 1;
                }
                c
            })
            .collect();
        let mut totals = vec![0u32; nb];
        for c in &counts {
            for (t, v) in totals.iter_mut().zip(c) {
                *t += v;
            }
        }
        let mut next = Vec::with_capacity(nb);
        let mut opaque = 0u32;
        for t in &totals {
            next.push(opaque);
            opaque += t;
        }
        let mut starts: Vec<Vec<u32>> = Vec::with_capacity(counts.len());
        for c in &counts {
            starts.push(next.clone());
            for (n, v) in next.iter_mut().zip(c) {
                *n += v;
            }
        }
        let mut trans = std::mem::take(&mut self.trans);
        trans.clear();
        for p in seen_parts {
            trans.extend(p.iter().filter(|d| d.1 == 2));
        }
        // Far first; distances are positive, so their bits order like them. Inside a cube
        // `sort_translucent` has seen to it.
        trans.par_sort_unstable_by_key(|d| std::cmp::Reverse(d.5.length_squared().to_bits()));

        // Distant terrain after the cubes (nearer, they hide most of it), a run per page.
        let mut lod: Vec<DrawRecord> = self
            .lod_draw
            .iter()
            .filter_map(|key| {
                let &(a, count, (y0, y1)) = self.lod.get(key)?;
                let o = key.origin();
                let half = Vec3::new(key.size() as f32, (y1 - y0) as f32, key.size() as f32) / 2.0;
                let center = Vec3::new(o.x as f32, y0 as f32, o.y as f32) + half - (cam_cube * 16).as_vec3() - cam_frac;
                if !planes
                    .iter()
                    .all(|pl| pl.dot(center.extend(1.0)) > -half.length())
                {
                    return None;
                }
                let args = DrawIndexedArgs {
                    indices: count * 6,
                    instances: 1,
                    first_index: 0,
                    vertex_offset: (a.offset * 4) as i32,
                    first_instance: 0,
                };
                Some((3, 0, a.page, args, [o.x / 16 - cam_cube.x, -cam_cube.y, o.y / 16 - cam_cube.z, 0], center))
            })
            .collect();
        lod.sort_by_key(|d| (d.2, d.5.length_squared().to_bits()));
        let lods = lod.len() as u32;
        let n = opaque + lods + trans.len() as u32;
        let (gpu_draws, gpu_faces) = self.table.drawn_last(gpu, frame.slot);
        self.drawn = n + gpu_draws;
        self.faces_drawn = faces_drawn + gpu_faces;
        let mut runs = std::mem::take(&mut self.runs);
        runs.clear();
        let obuf = crate::gpu_layout::frame_buffer(gpu, &mut self.origins, frame.slot, n, 16, usage::STORAGE, "origins");
        let fbuf = crate::gpu_layout::frame_buffer(
            gpu,
            &mut self.offsets,
            frame.slot,
            picks.len() as u32 + 1,
            16,
            usage::STORAGE,
            "frame offsets",
        );
        let offs = gpu.mapped(fbuf);
        offs[..16].fill(0);
        for (k, (o, light)) in picks.iter().enumerate() {
            offs[(k + 1) * 16..(k + 1) * 16 + 12].copy_from_slice(bytemuck::cast_slice(&o.to_array()));
            offs[(k + 1) * 16 + 12..(k + 1) * 16 + 16].copy_from_slice(&light.to_le_bytes());
        }
        let ibuf =
            crate::gpu_layout::frame_buffer(gpu, &mut self.indirect, frame.slot, n, 20, usage::INDIRECT | usage::STORAGE, "indirect");
        let origins = Mapped(gpu.mapped(obuf)[..n as usize * 16].as_mut_ptr());
        let indirect = Mapped(gpu.mapped(ibuf)[..n as usize * 20].as_mut_ptr());
        let put = |k: u32, d: &DrawRecord| {
            let mut a = d.3;
            a.first_instance = k;
            // SAFETY: both buffers hold `n` records and every k below `n` is written once.
            unsafe {
                std::ptr::copy_nonoverlapping(bytemuck::bytes_of(&d.4).as_ptr(), origins.at(k as usize * 16), 16);
                std::ptr::copy_nonoverlapping(bytemuck::bytes_of(&a).as_ptr(), indirect.at(k as usize * 20), 20);
            }
        };
        seen_parts
            .par_iter()
            .zip(starts.par_iter_mut())
            .for_each(|(p, at)| {
                for d in p.iter().filter(|d| d.1 < 2) {
                    let b = bucket(d);
                    put(at[b], d);
                    at[b] += 1;
                }
            });
        for (j, d) in lod.iter().enumerate() {
            put(opaque + j as u32, d);
        }
        for (j, d) in trans.iter().enumerate() {
            put(opaque + lods + j as u32, d);
        }
        let mut start = 0;
        for (b, &t) in totals.iter().enumerate() {
            if t > 0 {
                runs.push(((b / pages % 2) as u8, (b / pages / 2) as u8, (b % pages) as u32, start, t));
            }
            start += t;
        }
        for (j, d) in lod.iter().enumerate() {
            match runs.last_mut() {
                Some(r) if r.0 == 3 && r.2 == d.2 => r.4 += 1,
                _ => runs.push((3, 0, d.2, opaque + j as u32, 1)),
            }
        }
        self.translucent = runs.len();
        for (j, d) in trans.iter().enumerate() {
            match runs.last_mut() {
                Some(r) if r.1 == 2 && (r.0, r.1, r.2) == (d.0, d.1, d.2) => r.4 += 1,
                _ => runs.push((d.0, d.1, d.2, opaque + lods + j as u32, 1)),
            }
        }
        (self.parts, self.trans, self.runs) = (parts, trans, runs);
        self.buffers = Some((gbuf, obuf, ibuf, fbuf, scene));
        let moved = last_frame.map_or(Vec3::ZERO, |l| (cam.pos - l.1).as_vec3());
        let (size, levels) = self.occlusion.depth_size();
        let view = crate::gpu_layout::OcclusionView {
            prev: last_frame
                .map_or(Mat4::IDENTITY, |l| l.0)
                .to_cols_array_2d(),
            planes: planes.map(|p| p.to_array()),
            // What goes from there to here is bigger by how far it came, what it might have been behind.
            moved: moved.extend(moved.length() + 0.5).to_array(),
            cam_frac: cam_frac.extend(0.0).to_array(),
            cam_cube: cam_cube.extend(0).to_array(),
            size,
            skip: [opaque, opaque + lods],
            levels,
            hiz: last_frame.is_some() as u32,
            pad: [0; 2],
        };
        let vbuf = crate::gpu_layout::frame_buffer(
            gpu,
            &mut self.table.views,
            frame.slot,
            1,
            size_of::<crate::gpu_layout::OcclusionView>() as u64,
            usage::STORAGE,
            "occlusion view",
        );
        gpu.mapped(vbuf)[..size_of::<crate::gpu_layout::OcclusionView>()].copy_from_slice(bytemuck::bytes_of(&view));
        let depths = self.occlusion.depths();
        if last_frame.is_some() && n > 0 {
            self.occlusion.test(early, vbuf, obuf, ibuf, n);
        }
        if let Some(pipe) = self.occlusion.cull_pipe() {
            self.table.cull(gpu, early, frame.slot, pipe, vbuf, depths);
        }
        let groups = self.table.groups();
        for &(kind, layer, page, id, base, max) in &groups {
            let (pipe, buf) = self.pipe_of(kind, layer, page);
            self.bind_run(list, pipe, buf, self.table.out.as_ref().map_or(obuf, |o| o.origins));
            let Some(o) = self.table.out.as_ref() else { continue };
            list.push(Cmd::DrawIndexedIndirectCount {
                buffer: o.draws,
                offset: base as u64 * 20,
                count_buffer: self.table.buckets[frame.slot].0,
                count_offset: id as u64 * 8,
                max,
            });
        }
        self.emit(list, 0..self.translucent);
    }

    /// The translucent part of the last `draw`, after everything that may stand behind it.
    pub fn draw_translucent(&self, list: &mut CommandList) {
        let n = self.runs.len();
        if self.buffers.is_some() && self.translucent < n {
            self.emit(list, self.translucent..n);
        }
    }

    /// Runs `range` of the last `draw`.
    fn emit(&self, list: &mut CommandList, range: std::ops::Range<usize>) {
        let Some((_, obuf, ibuf, _, _)) = self.buffers else { return };
        for &(kind, layer, page, start, count) in &self.runs[range] {
            let (pipe, buf) = self.pipe_of(kind, layer, page);
            self.bind_run(list, pipe, buf, obuf);
            list.push(Cmd::DrawIndexedIndirect { buffer: ibuf, offset: start as u64 * 20, count });
        }
    }

    /// The pipeline and records of a run: kind 0 faces, 1 quads, 3 distant terrain.
    fn pipe_of(&self, kind: u8, layer: u8, page: u32) -> (PipelineId, BufferId) {
        if kind == 0 {
            (self.face_pipes[layer as usize], self.faces.pages[page as usize].0)
        } else if kind == 3 {
            (self.lod_pipe, self.faces.pages[page as usize].0)
        } else {
            (self.quad_pipes[layer as usize], self.quads.pages[page as usize].0)
        }
    }

    fn bind_run(&self, list: &mut CommandList, pipe: PipelineId, records: BufferId, origins: BufferId) {
        let Some((gbuf, _, _, fbuf, scene)) = self.buffers else { return };
        list.push(Cmd::IndexBuffer(self.quad_index));
        list.pipeline(pipe);
        use crate::gpu_layout::bind::world as b;
        list.bind(b::G, gbuf);
        list.bind(b::ORIGINS, origins);
        list.bind(b::RECORDS, records);
        list.bind(b::BIOMES, self.biomes);
        list.bind(b::TEX_INFO, self.tex_info);
        list.bind(b::FRAMES, self.anim);
        list.bind(b::OFFSETS, fbuf);
        list.bind(b::GLOW_MAP, self.glow);
        list.bind(b::TEX_SHADER, self.tex_shader);
        list.bind(b::SCENE, scene);
        list.bind(b::LOD_MASK, self.lod_mask.0);
        list.bind(b::SOLID, self.solid.buf);
    }
}

/// Every cube of the world with something opaque or cutout to draw, kept on the GPU and changed only
/// when meshes do: a compute shader makes each frame's draws from it, so the CPU's cost doesn't grow
/// with how many cubes are in view.
struct CubeTable {
    entries: Vec<crate::gpu_layout::CubeEntry>,
    free: Vec<u32>,
    /// Places changed since the last upload.
    dirty: Vec<u32>,
    /// On the GPU, and how many places it has.
    buffer: BufferId,
    capacity: u32,
    /// How many places each bucket (`CubeEntry::bucket`) has.
    per_bucket: Vec<u32>,
    /// Per frame: per bucket how many draws were written and where they start, then the faces.
    buckets: Vec<(BufferId, u32)>,
    views: Vec<(BufferId, u32)>,
    out: Option<TableOut>,
}

/// What `cull` writes: draws and where each one's cube is. One of each serves all frames in flight,
/// they run in order on the GPU.
struct TableOut {
    draws: BufferId,
    origins: BufferId,
    capacity: u32,
}

/// Direction runs of a cube: as many sides as face a camera outside it, in order.
const DRAWS_PER_FACES: u32 = 3;

impl CubeTable {
    fn new(gpu: &mut dyn Backend) -> CubeTable {
        CubeTable {
            entries: Vec::new(),
            free: Vec::new(),
            dirty: Vec::new(),
            buffer: gpu.create_buffer(32 * 1024, usage::STORAGE, "cube table"),
            capacity: 1024,
            per_bucket: Vec::new(),
            buckets: Vec::new(),
            views: Vec::new(),
            out: None,
        }
    }

    fn put(&mut self, cube: IVec3, bucket: u32, start: u32, counts: [u32; 3]) -> u32 {
        let e = crate::gpu_layout::CubeEntry { cube: cube.to_array(), bucket, start, counts };
        let slot = self.free.pop().unwrap_or_else(|| {
            self.entries.push(e);
            self.entries.len() as u32 - 1
        });
        self.entries[slot as usize] = e;
        self.dirty.push(slot);
        if self.per_bucket.len() <= bucket as usize {
            self.per_bucket.resize(bucket as usize + 1, 0);
        }
        self.per_bucket[bucket as usize] += 1;
        slot
    }

    /// A mesh's opaque and cutout layers: their places.
    fn add(
        &mut self,
        cube: IVec3,
        faces: Option<Alloc>,
        face_counts: &[[u32; 6]; 3],
        quads: Option<Alloc>,
        quad_counts: &[u32; 3],
    ) -> [u32; 4] {
        let mut slots = [u32::MAX; 4];
        if let Some(a) = faces {
            let mut start = a.offset;
            for layer in 0..2 {
                let c = face_counts[layer];
                let total: u32 = c.iter().sum();
                if total > 0 {
                    let counts = [c[0] | c[1] << 16, c[2] | c[3] << 16, c[4] | c[5] << 16];
                    slots[layer] = self.put(cube, a.page * 4 + layer as u32, start, counts);
                }
                start += total;
            }
        }
        if let Some(a) = quads {
            let mut start = a.offset;
            for layer in 0..2 {
                let n = quad_counts[layer];
                if n > 0 {
                    slots[2 + layer] = self.put(cube, a.page * 4 + 2 + layer as u32, start, [n, 0, 0]);
                }
                start += n;
            }
        }
        slots
    }

    fn remove(&mut self, slots: [u32; 4]) {
        for slot in slots.into_iter().filter(|s| *s != u32::MAX) {
            let e = &mut self.entries[slot as usize];
            self.per_bucket[e.bucket as usize] -= 1;
            e.bucket = u32::MAX;
            self.free.push(slot);
            self.dirty.push(slot);
        }
    }

    /// Brings the GPU's copy up to date, in runs of neighbours.
    fn upload(&mut self, gpu: &mut dyn Backend) {
        let n = self.entries.len() as u32;
        if n > self.capacity {
            gpu.destroy_buffer(self.buffer);
            self.capacity = n.next_power_of_two();
            self.buffer = gpu.create_buffer(self.capacity as u64 * 32, usage::STORAGE, "cube table");
            gpu.write_buffer(self.buffer, 0, bytemuck::cast_slice(&self.entries));
            self.dirty.clear();
            return;
        }
        self.dirty.sort_unstable();
        self.dirty.dedup();
        let mut i = 0;
        while i < self.dirty.len() {
            let first = self.dirty[i];
            let mut end = i + 1;
            while end < self.dirty.len() && self.dirty[end] == first + (end - i) as u32 {
                end += 1;
            }
            let run = &self.entries[first as usize..first as usize + (end - i)];
            gpu.write_buffer(self.buffer, first as u64 * 32, bytemuck::cast_slice(run));
            i = end;
        }
        self.dirty.clear();
    }

    /// What this frame slot's last use drew, for showing: draws and faces.
    fn drawn_last(&mut self, gpu: &mut dyn Backend, slot: usize) -> (u32, u64) {
        let Some(&(b, _)) = self.buckets.get(slot) else { return (0, 0) };
        let words: &[u32] = bytemuck::cast_slice(gpu.mapped(b));
        let n = self.per_bucket.len().min(words.len() / 2);
        // The GPU's counts, read back: a frame's worth may be stale or half written.
        let draws = words[..n * 2]
            .chunks_exact(2)
            .fold(0u32, |sum, c| sum.saturating_add(c[0]));
        (draws, words.get(self.faces_slot() * 2).copied().unwrap_or(0) as u64)
    }

    /// Where the faces' counter is in the buckets: after the last bucket, at an offset any API
    /// can bind a buffer from (DX12 and Metal ask for 256 bytes).
    fn faces_slot(&self) -> usize {
        (self.per_bucket.len() * 8).next_multiple_of(256) / 8
    }

    /// Makes this frame's draws. Before the world's pass.
    fn cull(
        &mut self,
        gpu: &mut dyn Backend,
        list: &mut CommandList,
        slot: usize,
        pipe: PipelineId,
        view: BufferId,
        depths: Option<BufferId>,
    ) {
        self.upload(gpu);
        let needed: u32 = self
            .per_bucket
            .iter()
            .enumerate()
            .map(|(id, &n)| n * if id & 2 == 0 { DRAWS_PER_FACES } else { 1 })
            .sum::<u32>()
            .max(1);
        if self.out.as_ref().is_none_or(|o| o.capacity < needed) {
            if let Some(o) = self.out.take() {
                gpu.destroy_buffer(o.draws);
                gpu.destroy_buffer(o.origins);
            }
            let capacity = needed.next_power_of_two().max(4096);
            self.out = Some(TableOut {
                draws: gpu.create_buffer(capacity as u64 * 20, usage::INDIRECT | usage::STORAGE, "table draws"),
                origins: gpu.create_buffer(capacity as u64 * 16, usage::STORAGE, "table origins"),
                capacity,
            });
        }
        let Some(o) = &self.out else { return };
        let faces = self.faces_slot();
        let buckets = crate::gpu_layout::frame_buffer(
            gpu,
            &mut self.buckets,
            slot,
            faces as u32 + 1,
            8,
            usage::STORAGE | usage::INDIRECT,
            "table buckets",
        );
        {
            let words: &mut [u32] = bytemuck::cast_slice_mut(gpu.mapped(buckets));
            let mut base = 0;
            for (id, &n) in self.per_bucket.iter().enumerate() {
                words[id * 2] = 0;
                words[id * 2 + 1] = base;
                base += n * if id & 2 == 0 { DRAWS_PER_FACES } else { 1 };
            }
            words[faces * 2] = 0;
        }
        use crate::gpu_layout::bind::occlusion as b;
        list.push(Cmd::Barrier);
        list.pipeline(pipe);
        list.bind(b::VIEW, view);
        list.bind(b::TABLE, self.buffer);
        list.bind(b::BUCKETS, buckets);
        list.bind(b::OUT_DRAWS, o.draws);
        list.bind(b::OUT_ORIGINS, o.origins);
        list.bind_range(b::FACES_DRAWN, buckets, faces as u64 * 8, 8);
        // Its use is decided in the shader, but all a shader mentions has to be bound: by one it
        // writes anyway, as a buffer read in one binding can't be written in another.
        list.bind(b::DEPTHS, depths.unwrap_or(buckets));
        let count = self.entries.len() as u32;
        list.push_constants(&crate::gpu_layout::OcclusionArgs { size: [0; 2], src: 0, dst: 0, count, pad: 0 });
        list.push(Cmd::Dispatch { x: count.div_ceil(256), y: 1, z: 1 });
        list.push(Cmd::Barrier);
    }

    /// The buckets with anything in them, in drawing order, opaque before cutout: kind, layer, page,
    /// bucket, where its draws start and how many it may have.
    fn groups(&self) -> Vec<(u8, u8, u32, u32, u32, u32)> {
        let mut base = 0;
        let mut out = Vec::new();
        for (id, &n) in self.per_bucket.iter().enumerate() {
            let id = id as u32;
            let (page, kind, layer) = (id / 4, (id >> 1 & 1) as u8, (id & 1) as u8);
            let max = n * if kind == 0 { DRAWS_PER_FACES } else { 1 };
            if n > 0 {
                out.push((kind, layer, page, id, base, max));
            }
            base += max;
        }
        out.sort_by_key(|g| (g.1, g.0, g.2));
        out
    }

    fn destroy(&mut self, gpu: &mut dyn Backend) {
        gpu.destroy_buffer(self.buffer);
        for (b, _) in self.buckets.drain(..).chain(self.views.drain(..)) {
            gpu.destroy_buffer(b);
        }
        if let Some(o) = self.out.take() {
            gpu.destroy_buffer(o.draws);
            gpu.destroy_buffer(o.origins);
        }
    }
}

/// How far the camera may go in a frame for what the last one saw to still say anything.
const OCCLUSION_MOVE: f64 = 8.0;

/// Occlusion culling on the GPU, by the depth of the frame before: the cubes it shows to be behind
/// something get no draw. A cube wrongly left out is back the frame after, since it then isn't in
/// that depth and nothing there hides it any longer.
pub struct Occlusion {
    /// The setting.
    pub on: bool,
    pipes: Option<[PipelineId; 3]>,
    /// Depth and its smaller copies; its size; how many levels.
    depths: Option<(BufferId, [u32; 2], u32)>,
    /// This frame's camera, and the last one's if `depths` has its depth.
    now: Option<(Mat4, DVec3)>,
    last: Option<(Mat4, DVec3)>,
}

impl Occlusion {
    /// Without its shader nothing opaque would get drawn, so it is a reason not to start.
    fn new(gpu: &mut dyn Backend, assets: &crate::assets::Assets) -> Result<Occlusion, String> {
        Ok(Occlusion { on: true, pipes: Some(Self::pipes(gpu, assets)?), depths: None, now: None, last: None })
    }

    fn pipes(gpu: &mut dyn Backend, assets: &crate::assets::Assets) -> Result<[PipelineId; 3], String> {
        // Not a shader packs may replace: it decides what is drawn, not how it looks.
        let s = crate::packs::engine_shader(assets, "occlusion.wgsl", &crate::gpu_layout::occlusion_prelude());
        s.map_err(|e| e.0).and_then(|s| {
            let mut pipe = |entry| {
                gpu.create_compute(&ComputeDesc { label: "occlusion", shader: &s, entry })
                    .map_err(|e| e.0)
            };
            Ok([pipe("down")?, pipe("test")?, pipe("cull")?])
        })
    }

    fn reload(&mut self, gpu: &mut dyn Backend, assets: &crate::assets::Assets) {
        match Self::pipes(gpu, assets) {
            Ok(new) => {
                for p in self.pipes.replace(new).into_iter().flatten() {
                    gpu.destroy_pipeline(p);
                }
            }
            Err(e) => log::error!("occlusion shader: {e}"),
        }
    }

    fn destroy(&mut self, gpu: &mut dyn Backend) {
        for p in self.pipes.take().into_iter().flatten() {
            gpu.destroy_pipeline(p);
        }
        if let Some((b, ..)) = self.depths.take() {
            gpu.destroy_buffer(b);
        }
    }

    /// What this frame may go by: the last frame's camera, unless it went too far since.
    fn next_frame(&mut self, view_proj: Mat4, pos: DVec3) -> Option<(Mat4, DVec3)> {
        self.now = Some((view_proj, pos));
        self.last
            .filter(|l| self.on && (l.1 - pos).length() < OCCLUSION_MOVE)
    }

    /// The size of the depth kept and how many levels it has.
    fn depth_size(&self) -> ([u32; 2], u32) {
        self.depths.map_or(([1; 2], 1), |d| (d.1, d.2))
    }

    fn depths(&self) -> Option<BufferId> {
        self.depths.map(|d| d.0)
    }

    fn cull_pipe(&self) -> Option<PipelineId> {
        self.pipes.map(|p| p[2])
    }

    /// Takes what the last frame shows to be hidden out of the draws the CPU made.
    fn test(&self, list: &mut CommandList, view: BufferId, origins: BufferId, draws: BufferId, count: u32) {
        let (Some([_, pipe, _]), Some((depths, ..))) = (self.pipes, self.depths) else { return };
        use crate::gpu_layout::bind::occlusion as b;
        list.push(Cmd::Barrier);
        list.pipeline(pipe);
        list.bind(b::VIEW, view);
        list.bind(b::DEPTHS, depths);
        list.bind(b::ORIGINS, origins);
        list.bind(b::DRAWS, draws);
        list.push_constants(&crate::gpu_layout::OcclusionArgs { size: [0; 2], src: 0, dst: 0, count, pad: 0 });
        list.push(Cmd::Dispatch { x: count.div_ceil(256), y: 1, z: 1 });
        list.push(Cmd::Barrier);
    }

    /// Takes this frame's depth for the next frame to go by. After the world's pass.
    pub fn take_depth(&mut self, gpu: &mut dyn Backend, list: &mut CommandList, depth: TextureId, size: [u32; 2]) {
        let Some([down, ..]) = self.pipes.filter(|_| self.on) else {
            self.last = None;
            return;
        };
        let (texels, levels) = {
            let (mut s, mut texels, mut levels) = (size, 0u64, 0);
            loop {
                texels += s[0] as u64 * s[1] as u64;
                levels += 1;
                if s[0] <= 1 && s[1] <= 1 {
                    break (texels, levels);
                }
                s = s.map(|v| v.div_ceil(2));
            }
        };
        let buffer = match self.depths.filter(|d| d.1 == size) {
            Some(d) => d.0,
            None => {
                if let Some((old, ..)) = self.depths.take() {
                    gpu.destroy_buffer(old);
                }
                let b = gpu.create_buffer(texels * 4, usage::STORAGE, "depth levels");
                self.depths = Some((b, size, levels));
                b
            }
        };
        list.push(Cmd::CopyDepthToBuffer { depth, buffer });
        list.push(Cmd::Barrier);
        list.pipeline(down);
        list.bind(crate::gpu_layout::bind::occlusion::DEPTHS, buffer);
        let (mut s, mut src) = (size, 0u32);
        for _ in 1..levels {
            let dst = src + s[0] * s[1];
            list.push_constants(&crate::gpu_layout::OcclusionArgs { size: s, src, dst, count: 0, pad: 0 });
            s = s.map(|v| v.div_ceil(2));
            list.push(Cmd::Dispatch { x: s[0].div_ceil(32), y: s[1].div_ceil(32), z: 1 });
            list.push(Cmd::Barrier);
            src = dst;
        }
        self.last = self.now.take();
    }
}

/// Left, right, bottom, top planes; the reverse-Z infinite projection has no useful far plane.
pub fn frustum_planes(m: Mat4) -> [Vec4; 4] {
    let r = |i: usize| m.row(i);
    let norm = |p: Vec4| p / p.truncate().length();
    [norm(r(3) + r(0)), norm(r(3) - r(0)), norm(r(3) + r(1)), norm(r(3) - r(1))]
}
