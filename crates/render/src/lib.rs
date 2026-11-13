//! Backend-neutral rendering interface. Only crates under `backends/` talk to a graphics API.
//!
//! Model: no vertex buffers (shaders pull from storage buffers), all sampled textures live in a
//! global bindless table (group 0), per-draw buffers are group 1, small per-draw data may go
//! through push constants (`var<immediate>`). Shaders are WGSL, prefixed with [`PRELUDE`].

mod shader;

pub use shader::{BufferKind, Shader, ShaderError};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// Handles to a backend's objects: indices into a list with free slots reused.
pub struct Slab<T> {
    pub items: Vec<Option<T>>,
    free: Vec<u32>,
}

impl<T> Default for Slab<T> {
    fn default() -> Self {
        Slab { items: Vec::new(), free: Vec::new() }
    }
}

impl<T> Slab<T> {
    pub fn insert(&mut self, v: T) -> u32 {
        if let Some(i) = self.free.pop() {
            self.items[i as usize] = Some(v);
            i
        } else {
            self.items.push(Some(v));
            self.items.len() as u32 - 1
        }
    }
    pub fn remove(&mut self, i: u32) -> Option<T> {
        let v = self.items.get_mut(i as usize)?.take();
        if v.is_some() {
            self.free.push(i);
        }
        v
    }
    pub fn get(&self, i: u32) -> &T {
        self.items[i as usize]
            .as_ref()
            .expect("stale render handle")
    }
    pub fn get_mut(&mut self, i: u32) -> &mut T {
        self.items[i as usize]
            .as_mut()
            .expect("stale render handle")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BufferId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PipelineId(pub u32);

/// Global WGSL declarations every shader can use. Texture handles index into these arrays.
/// The `mipped` samplers clamp too: textures are never tiled, and wrapping would blend the edge
/// texels of a face with the opposite edge (seams between overlays and cutout blocks).
pub const PRELUDE: &str = r#"enable wgpu_binding_array;
@group(0) @binding(0) var textures: binding_array<texture_2d<f32>, 65536>;
@group(0) @binding(1) var nearest_clamp: sampler;
@group(0) @binding(2) var linear_clamp: sampler;
@group(0) @binding(3) var nearest_mipped: sampler;
@group(0) @binding(4) var linear_mipped: sampler;
"#;

/// For GPUs without draws whose count a shader wrote: copies `max` draw records of `stride` words
/// to `dst`, zeroing those at or past the count, so a plain indirect draw of `max` does the same.
pub const DRAW_COMPACTION: &str = r#"
struct Args { src: u32, count: u32, max: u32, dst: u32, stride: u32 }
@group(1) @binding(0) var<storage, read> src: array<u32>;
@group(1) @binding(1) var<storage, read> counts: array<u32>;
@group(1) @binding(2) var<storage, read_write> dst: array<u32>;
var<immediate> a: Args;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= a.max {
        return;
    }
    let live = i < counts[a.count];
    for (var w = 0u; w < a.stride; w++) {
        dst[a.dst + i * a.stride + w] = select(0u, src[a.src + i * a.stride + w], live);
    }
}
"#;

/// A draw whose count a shader wrote, on a GPU without those: its records are copied by
/// `DRAW_COMPACTION` to `at` in a buffer of compacted ones, `args` being the shader's immediates.
pub struct CountDraw {
    pub buffer: BufferId,
    pub count_buffer: BufferId,
    pub max: u32,
    pub at: u64,
    pub args: [u32; 5],
}

/// The count draws of the pass starting at `cmds[0]`, one after another, and the bytes they take.
pub fn count_draws(cmds: &[Cmd]) -> (Vec<CountDraw>, u64) {
    let mut at = 0;
    let draws = cmds
        .iter()
        .take_while(|c| !matches!(c, Cmd::EndPass))
        .filter_map(|c| {
            let (buffer, offset, count_buffer, count_offset, max, stride) = match *c {
                Cmd::DrawIndexedIndirectCount { buffer, offset, count_buffer, count_offset, max } => {
                    (buffer, offset, count_buffer, count_offset, max, size_of::<DrawIndexedArgs>() as u64)
                }
                Cmd::DrawIndirectCount { buffer, offset, count_buffer, count_offset, max } => {
                    (buffer, offset, count_buffer, count_offset, max, size_of::<DrawArgs>() as u64)
                }
                _ => return None,
            };
            let args = [(offset / 4) as u32, (count_offset / 4) as u32, max, (at / 4) as u32, (stride / 4) as u32];
            let draw = CountDraw { buffer, count_buffer, max, at, args };
            at += max as u64 * stride;
            Some(draw)
        })
        .collect();
    (draws, at)
}

/// Every texture of every kind is its own entry (blocks, items, particles, skins, GUI, each
/// animation frame): big packs have thousands; face records keep 16-bit indices.
pub const MAX_TEXTURES: u32 = 65536;

pub mod usage {
    pub const STORAGE: u32 = 1;
    pub const INDIRECT: u32 = 2;
    pub const UNIFORM: u32 = 4;
    /// Persistently mapped and written directly by the CPU. The caller must not overwrite a range the
    /// GPU may still read (use one range per frame in flight).
    pub const HOST: u32 = 8;
    pub const INDEX: u32 = 16;
    /// Written by the GPU and read by the CPU through `mapped`, once the frame that wrote it is
    /// done (the next time its frame slot comes round).
    pub const READBACK: u32 = 32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Rgba8Srgb,
    Rgba8Unorm,
    R8Unorm,
    Rgba16Float,
    Depth32,
    /// Whatever the presentation surface uses.
    Surface,
}

impl Format {
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Format::R8Unorm => 1,
            Format::Rgba16Float => 8,
            _ => 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TextureDesc {
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    pub format: Format,
    pub render_target: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    None,
    Alpha,
    Premultiplied,
    Additive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    Always,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
}

#[derive(Clone, Copy, Debug)]
pub struct DepthState {
    pub write: bool,
    pub compare: Compare,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topology {
    Triangles,
    Lines,
}

pub struct PipelineDesc<'a> {
    pub label: &'a str,
    pub shader: &'a Shader,
    pub vertex: &'a str,
    pub fragment: Option<&'a str>,
    pub color: Option<Format>,
    pub depth: Option<DepthState>,
    pub blend: Blend,
    pub cull_back: bool,
    pub topology: Topology,
}

pub struct ComputeDesc<'a> {
    pub label: &'a str,
    pub shader: &'a Shader,
    pub entry: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Surface,
    Texture(TextureId),
}

#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    BeginPass {
        color: Option<(Target, Option<[f32; 4]>)>,
        depth: Option<(TextureId, Option<f32>)>,
    },
    EndPass,
    SetPipeline(PipelineId),
    BindBuffer {
        binding: u32,
        buffer: BufferId,
        offset: u64,
        size: u64,
    },
    /// Range inside [`CommandList::data`].
    PushConstants {
        start: u32,
        len: u32,
    },
    Scissor {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    },
    Draw {
        vertices: u32,
        instances: u32,
        first_vertex: u32,
        first_instance: u32,
    },
    DrawIndirect {
        buffer: BufferId,
        offset: u64,
        count: u32,
    },
    /// 32-bit indices for the following indexed draws.
    IndexBuffer(BufferId),
    /// `count` [`DrawIndexedArgs`] records.
    DrawIndexedIndirect {
        buffer: BufferId,
        offset: u64,
        count: u32,
    },
    /// Up to `max` [`DrawIndexedArgs`] records, how many a shader wrote at `count_offset`.
    DrawIndexedIndirectCount {
        buffer: BufferId,
        offset: u64,
        count_buffer: BufferId,
        count_offset: u64,
        max: u32,
    },
    DrawIndirectCount {
        buffer: BufferId,
        offset: u64,
        count_buffer: BufferId,
        count_offset: u64,
        max: u32,
    },
    Dispatch {
        x: u32,
        y: u32,
        z: u32,
    },
    /// Makes shader writes visible to later commands. Only needed between compute and its consumers.
    Barrier,
    /// Copies a depth texture to the start of a buffer, 32-bit floats row by row. Outside a pass.
    CopyDepthToBuffer {
        depth: TextureId,
        buffer: BufferId,
    },
    /// GPU timestamp, see [`Backend::gpu_times`].
    Mark,
}

/// Layout of one indexed indirect draw, matches every API we care about.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawIndexedArgs {
    pub indices: u32,
    pub instances: u32,
    pub first_index: u32,
    pub vertex_offset: i32,
    pub first_instance: u32,
}

/// Layout of one non-indexed indirect draw, matches every API we care about.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawArgs {
    pub vertices: u32,
    pub instances: u32,
    pub first_vertex: u32,
    pub first_instance: u32,
}

#[derive(Default)]
pub struct CommandList {
    pub cmds: Vec<Cmd>,
    pub data: Vec<u8>,
}

impl CommandList {
    pub fn clear(&mut self) {
        self.cmds.clear();
        self.data.clear();
    }

    /// Moves `other`'s commands to the end of this list.
    pub fn append(&mut self, other: &mut CommandList) {
        let base = self.data.len() as u32;
        self.cmds.extend(other.cmds.drain(..).map(|c| match c {
            Cmd::PushConstants { start, len } => Cmd::PushConstants { start: start + base, len },
            c => c,
        }));
        self.data.append(&mut other.data);
    }

    pub fn push(&mut self, cmd: Cmd) {
        self.cmds.push(cmd);
    }

    pub fn push_constants<T: bytemuck::Pod>(&mut self, value: &T) {
        let start = self.data.len() as u32;
        self.data.extend_from_slice(bytemuck::bytes_of(value));
        self.cmds
            .push(Cmd::PushConstants { start, len: size_of::<T>() as u32 });
    }

    pub fn pipeline(&mut self, p: PipelineId) {
        self.cmds.push(Cmd::SetPipeline(p));
    }

    pub fn bind(&mut self, binding: u32, buffer: BufferId) {
        self.cmds
            .push(Cmd::BindBuffer { binding, buffer, offset: 0, size: u64::MAX });
    }

    pub fn bind_range(&mut self, binding: u32, buffer: BufferId, offset: u64, size: u64) {
        self.cmds
            .push(Cmd::BindBuffer { binding, buffer, offset, size });
    }

    pub fn draw(&mut self, vertices: u32, instances: u32) {
        self.cmds
            .push(Cmd::Draw { vertices, instances, first_vertex: 0, first_instance: 0 });
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FrameInfo {
    pub width: u32,
    pub height: u32,
    /// Index of the frame-in-flight slot, for callers that keep per-frame host buffers.
    pub slot: usize,
}

pub struct Capture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub trait Window: HasWindowHandle + HasDisplayHandle {}
impl<T: HasWindowHandle + HasDisplayHandle> Window for T {}

pub struct BackendDesc<'a> {
    pub window: &'a dyn Window,
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
    pub debug: bool,
    /// Where compiled shaders may be kept between runs: a phone's driver takes most of a minute
    /// over the terrain shader.
    pub cache: Option<&'a std::path::Path>,
}

pub trait Backend: Send {
    fn name(&self) -> &str;
    /// The GPU in use, for diagnostics.
    fn device_name(&self) -> &str {
        ""
    }
    fn frames_in_flight(&self) -> usize;
    fn surface_size(&self) -> (u32, u32);
    fn resize(&mut self, width: u32, height: u32);
    fn set_vsync(&mut self, vsync: bool);
    /// The window's surface went away (an Android app sent to the background), or came back with
    /// `Some`: no frames begin in between. Only phones do this, and only Vulkan runs there.
    fn set_surface(&mut self, _window: Option<&dyn Window>) {}

    fn create_buffer(&mut self, size: u64, usage: u32, label: &str) -> BufferId;
    /// Only valid for `usage::HOST` buffers.
    fn mapped(&mut self, buffer: BufferId) -> &mut [u8];
    /// Copied on the GPU timeline at the start of the next submitted frame, before any pass.
    fn write_buffer(&mut self, buffer: BufferId, offset: u64, data: &[u8]);
    fn destroy_buffer(&mut self, buffer: BufferId);

    /// Returns the texture and its bindless index (into `textures`).
    fn create_texture(&mut self, desc: &TextureDesc, label: &str) -> TextureId;
    /// `data` holds one mip level, tightly packed.
    fn write_texture(&mut self, texture: TextureId, mip: u32, data: &[u8]);
    fn texture_index(&self, texture: TextureId) -> u32;
    fn destroy_texture(&mut self, texture: TextureId);

    fn create_pipeline(&mut self, desc: &PipelineDesc) -> Result<PipelineId, Error>;
    fn create_compute(&mut self, desc: &ComputeDesc) -> Result<PipelineId, Error>;
    /// Compiles `desc` on another thread into the kept shaders (`BackendDesc::cache`), so that
    /// `create_compute` of it later takes no time: a phone's driver needs most of a minute over a
    /// big shader. Without a cache of its own a backend compiles nothing ahead.
    fn precompile(&mut self, _desc: &ComputeDesc) {}
    /// Whether what `precompile` started is still compiling.
    fn compiling(&self) -> bool {
        false
    }
    fn destroy_pipeline(&mut self, pipeline: PipelineId);

    /// `None` when the surface cannot be drawn to right now (minimized).
    fn begin_frame(&mut self) -> Option<FrameInfo>;
    fn end_frame(&mut self, cmds: &CommandList);
    /// The next `end_frame` copies the surface image back to the CPU.
    fn request_capture(&mut self);
    /// Stalls until the requested capture is ready. Debug and screenshots only.
    fn take_capture(&mut self) -> Option<Capture>;
    /// GPU milliseconds of the last finished frame, split at [`Cmd::Mark`]s: uploads, then one
    /// entry per mark, then the rest of the frame. Empty if the backend cannot measure.
    fn gpu_times(&self) -> &[f32] {
        &[]
    }
    fn wait_idle(&mut self);
}
