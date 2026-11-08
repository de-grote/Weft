//! wgpu backend: Metal, DX12 or Vulkan through one API, for machines the Vulkan backend can't
//! run on (macOS, drivers without Vulkan 1.3). The same model as the Vulkan backend: textures in
//! one table (group 0), per-draw buffers in group 1, push constants as immediates.
//!
//! What wgpu doesn't have is made here: buffers the CPU writes keep a copy that goes up whole when
//! it changed, buffers it reads are copied out after the frames that used them, and the per-draw
//! buffers become bind groups, kept while they are in use.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use weft_render::*;

const FRAMES: usize = 2;
/// Timestamp queries per frame: start, after uploads, marks, end.
const MAX_MARKS: u32 = 16;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Bind groups not used for this many frames are dropped (a buffer bound at offsets that move).
const KEEP_GROUPS: u64 = 120;

/// What the CPU sees of a `usage::HOST` or `usage::READBACK` buffer, which the Vulkan backend
/// keeps in memory both share. A frame that may have written it copies it into this slot's
/// staging, and from there it comes into `bytes` once the slot comes round.
struct Mirror {
    bytes: Vec<u8>,
    /// `usage::HOST`: the CPU writes `bytes`, sent whole at the end of a frame they changed in.
    host: bool,
    dirty: bool,
    staging: [Option<wgpu::Buffer>; FRAMES],
}

struct Buffer {
    raw: wgpu::Buffer,
    mirror: Option<Mirror>,
}

struct Texture {
    raw: wgpu::Texture,
    view: wgpu::TextureView,
    desc: TextureDesc,
    /// Index into the texture table, `u32::MAX` for depth.
    index: u32,
}

enum Kind {
    Render(wgpu::RenderPipeline),
    Compute(wgpu::ComputePipeline),
}

struct Pipeline {
    kind: Kind,
    /// Group 1, when the shader has buffers.
    group: Option<wgpu::BindGroupLayout>,
    buffers: Vec<u32>,
    /// Of those, the ones it may write.
    writes: Vec<u32>,
    immediates: u32,
    label: String,
}

/// Every sampled texture by index, and the bind groups made of them. A pass drawing into one of
/// them gets a group with a blank in its place: wgpu refuses a texture bound while drawn to.
struct Table {
    layout: wgpu::BindGroupLayout,
    samplers: [wgpu::Sampler; 4],
    blank: wgpu::TextureView,
    views: Vec<Option<wgpu::TextureView>>,
    free: Vec<u32>,
    /// By the index left out (`u32::MAX`: none); emptied when a texture comes or goes.
    groups: HashMap<u32, wgpu::BindGroup>,
    /// The device takes fewer views than the layout has; otherwise blanks fill the rest.
    partial: bool,
}

impl Table {
    fn group(&mut self, device: &wgpu::Device, without: u32) -> wgpu::BindGroup {
        if let Some(g) = self.groups.get(&without) {
            return g.clone();
        }
        let n = if self.partial { self.views.len().max(1) } else { MAX_TEXTURES as usize };
        let views: Vec<&wgpu::TextureView> = (0..n)
            .map(|i| match self.views.get(i) {
                Some(Some(v)) if i as u32 != without => v,
                _ => &self.blank,
            })
            .collect();
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureViewArray(&views) }];
        for (i, s) in self.samplers.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry { binding: 1 + i as u32, resource: wgpu::BindingResource::Sampler(s) });
        }
        let g = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("textures"), layout: &self.layout, entries: &entries });
        self.groups.insert(without, g.clone());
        g
    }
}

/// A frame slot: its last submission and what to read back once that is done.
#[derive(Default)]
struct Frame {
    submission: Option<wgpu::SubmissionIndex>,
    waited: bool,
    /// Readback buffers whose staging for this slot is being mapped.
    maps: Vec<(u32, Arc<AtomicBool>)>,
    marks: u32,
    times: Option<Arc<AtomicBool>>,
}

/// Timestamps, when the device can write them anywhere in a frame.
struct Timer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: [wgpu::Buffer; FRAMES],
    period: f32,
}

/// Small pipelines of the backend's own, for what wgpu can't do directly.
struct Internal {
    /// Depth into a tightly packed buffer (`Cmd::CopyDepthToBuffer`).
    depth: wgpu::ComputePipeline,
    /// Indirect draws past the count zeroed into `scratch`, where draws with a count don't work.
    compact: Option<wgpu::ComputePipeline>,
    scratch: Option<(wgpu::Buffer, u64)>,
}

/// A pipeline and its buffers: binding, buffer, offset, size.
type GroupKey = (u32, Vec<(u32, u32, u64, u64)>);

pub struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// What the surface is drawn as: its format, with sRGB if it lacks it.
    view_format: wgpu::TextureFormat,
    want_size: (u32, u32),
    dirty_surface: bool,
    current: Option<wgpu::SurfaceTexture>,
    frames: Vec<Frame>,
    slot: usize,
    buffers: Slab<Buffer>,
    textures: Slab<Texture>,
    pipelines: Slab<Pipeline>,
    table: Table,
    /// Group 1 bind groups by what is in them, with the frame they were last used in.
    groups: HashMap<GroupKey, (wgpu::BindGroup, u64)>,
    frame_no: u64,
    internal: Internal,
    timer: Option<Timer>,
    gpu_times: Vec<f32>,
    capture_requested: bool,
    capture: Option<(wgpu::Buffer, u32, u32, u32)>,
    device_name: String,
    /// `wgpu` and the API under it.
    name: String,
    present_modes: Vec<wgpu::PresentMode>,
}

fn err<E: std::fmt::Display>(what: &str) -> impl FnOnce(E) -> Error + '_ {
    move |e| Error(format!("{what}: {e}"))
}

/// The backends tried, in order: the platform's own API first, any after it. `WGPU_BACKEND`
/// chooses instead.
fn backends() -> Vec<wgpu::Backends> {
    if let Some(b) = wgpu::Backends::from_env() {
        return vec![b];
    }
    let own = if cfg!(windows) {
        wgpu::Backends::DX12
    } else if cfg!(target_os = "macos") {
        wgpu::Backends::METAL
    } else {
        wgpu::Backends::VULKAN
    };
    vec![own, wgpu::Backends::PRIMARY]
}

fn wgsl(name: &str, source: &str) -> Result<naga::Module, Error> {
    naga::front::wgsl::parse_str(source).map_err(|e| Error(format!("{name}: {}", e.emit_to_string(source))))
}

impl WgpuBackend {
    pub fn new(desc: &BackendDesc) -> Result<WgpuBackend, Error> {
        let display = desc
            .window
            .display_handle()
            .map_err(err("display handle"))?
            .as_raw();
        let window = desc
            .window
            .window_handle()
            .map_err(err("window handle"))?
            .as_raw();
        let mut tried = Vec::new();
        for backends in backends() {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends,
                // Indirect calls checked: on DX12 that is also what puts `first_instance` and
                // `base_vertex` into the shaders' builtins.
                flags: if desc.debug { wgpu::InstanceFlags::debugging() } else { wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL },
                ..wgpu::InstanceDescriptor::new_without_display_handle_from_env()
            });
            // SAFETY: the window outlives the backend, which the engine drops first.
            let target = wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(display), raw_window_handle: window };
            let surface = unsafe { instance.create_surface_unsafe(target) }.map_err(err("creating surface"))?;
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            }));
            match adapter
                .map_err(|e| e.to_string())
                .and_then(|a| Self::open(desc, surface, a))
            {
                Ok(b) => return Ok(b),
                Err(e) => tried.push(format!("{backends:?}: {e}")),
            }
        }
        Err(Error(tried.join("; ")))
    }

    fn open(desc: &BackendDesc, surface: wgpu::Surface<'static>, adapter: wgpu::Adapter) -> Result<WgpuBackend, String> {
        use wgpu::Features as F;
        let info = adapter.get_info();
        let (device_name, name) = (info.name.clone(), format!("wgpu {}", info.backend.to_str()));
        let has = adapter.features();
        let needed = F::TEXTURE_BINDING_ARRAY
            | F::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
            | F::IMMEDIATES
            | F::INDIRECT_FIRST_INSTANCE;
        if !has.contains(needed) {
            return Err(format!("{device_name} lacks what the game needs: {:?}", needed - has));
        }
        let limits = adapter.limits();
        if limits.max_binding_array_elements_per_shader_stage < MAX_TEXTURES {
            return Err(format!(
                "{device_name} can bind {} textures at once, the game needs {MAX_TEXTURES}",
                limits.max_binding_array_elements_per_shader_stage
            ));
        }
        let timing = F::TIMESTAMP_QUERY | F::TIMESTAMP_QUERY_INSIDE_ENCODERS | F::TIMESTAMP_QUERY_INSIDE_PASSES;
        let wanted = F::PARTIALLY_BOUND_BINDING_ARRAY | F::MULTI_DRAW_INDIRECT_COUNT | F::VERTEX_WRITABLE_STORAGE | timing;
        let features = needed | (has & wanted);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("weft"),
            required_features: features,
            required_limits: limits,
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        device.on_uncaptured_error(Arc::new(|e| match e {
            wgpu::Error::Validation { description, .. } => log::error!(target: "wgpu", "{description}"),
            e => log::error!(target: "wgpu", "{e}"),
        }));
        log::info!("GPU: {device_name} ({name})");

        let caps = surface.get_capabilities(&adapter);
        let format = [wgpu::TextureFormat::Bgra8UnormSrgb, wgpu::TextureFormat::Rgba8UnormSrgb]
            .into_iter()
            .find(|f| caps.formats.contains(f))
            .or_else(|| caps.formats.first().copied())
            .ok_or("the surface has no formats")?;
        let view_format = format.add_srgb_suffix();
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            usage |= wgpu::TextureUsages::COPY_SRC;
        }
        let config = wgpu::SurfaceConfiguration {
            usage,
            format,
            width: desc.width.max(1),
            height: desc.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: FRAMES as u32,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: if view_format == format { Vec::new() } else { vec![view_format] },
            color_space: Default::default(),
        };

        let all = wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE;
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: all,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: std::num::NonZeroU32::new(MAX_TEXTURES),
        }];
        for i in 0..4 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 1 + i,
                visibility: all,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            });
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("textures"), entries: &entries });
        let sampler = |filter: wgpu::FilterMode, mips: bool| {
            // The filtered sampler gets anisotropy: block faces at grazing angles stay sharp.
            let aniso = filter == wgpu::FilterMode::Linear && mips;
            device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: filter,
                min_filter: filter,
                mipmap_filter: if mips { wgpu::MipmapFilterMode::Linear } else { wgpu::MipmapFilterMode::Nearest },
                lod_max_clamp: if mips { 32.0 } else { 0.0 },
                anisotropy_clamp: if aniso { 8 } else { 1 },
                ..Default::default()
            })
        };
        let samplers = [
            sampler(wgpu::FilterMode::Nearest, false),
            sampler(wgpu::FilterMode::Linear, false),
            sampler(wgpu::FilterMode::Nearest, true),
            sampler(wgpu::FilterMode::Linear, true),
        ];
        let blank = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("blank"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());

        // DX12 draws with a count leave `first_instance` out of `instance_index`, which every
        // world draw goes by (wgpu-hal 30 passes it only for draws without one).
        let count_draws = features.contains(F::MULTI_DRAW_INDIRECT_COUNT) && info.backend != wgpu::Backend::Dx12;
        let internal = Internal {
            depth: internal_pipeline(&device, "depth copy", DEPTH_COPY)?,
            compact: if count_draws { None } else { Some(internal_pipeline(&device, "draw compaction", COMPACT)?) },
            scratch: None,
        };
        let timer = features.contains(timing).then(|| {
            let read = |_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("timestamps"),
                    size: MAX_MARKS as u64 * 8,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            };
            Timer {
                queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: MAX_MARKS * FRAMES as u32,
                }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("timestamps"),
                    size: FRAMES as u64 * wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                read: std::array::from_fn(read),
                period: queue.get_timestamp_period(),
            }
        });
        let mut backend = WgpuBackend {
            device,
            queue,
            surface,
            config,
            view_format,
            want_size: (desc.width, desc.height),
            dirty_surface: true,
            current: None,
            frames: (0..FRAMES).map(|_| Frame::default()).collect(),
            slot: 0,
            buffers: Slab::default(),
            textures: Slab::default(),
            pipelines: Slab::default(),
            table: Table {
                layout,
                samplers,
                blank,
                views: Vec::new(),
                free: Vec::new(),
                groups: HashMap::new(),
                partial: features.contains(F::PARTIALLY_BOUND_BINDING_ARRAY),
            },
            groups: HashMap::new(),
            frame_no: 0,
            internal,
            timer,
            gpu_times: Vec::new(),
            capture_requested: false,
            capture: None,
            device_name,
            name,
            present_modes: caps.present_modes.clone(),
        };
        backend.set_vsync(desc.vsync);
        backend.configure();
        Ok(backend)
    }

    fn configure(&mut self) {
        let (w, h) = self.want_size;
        if w == 0 || h == 0 {
            return;
        }
        self.current = None;
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.dirty_surface = false;
    }

    fn format(&self, f: Format) -> wgpu::TextureFormat {
        match f {
            Format::Rgba8Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            Format::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
            Format::R8Unorm => wgpu::TextureFormat::R8Unorm,
            Format::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
            Format::Depth32 => DEPTH,
            Format::Surface => self.view_format,
        }
    }

    /// Waits for this slot's last frame and takes what it read back. Called lazily, so a read
    /// before `begin_frame` gets the finished frame's too.
    fn ensure_slot_ready(&mut self) {
        let slot = self.slot;
        let frame = &mut self.frames[slot];
        if frame.waited {
            return;
        }
        frame.waited = true;
        if let Some(index) = frame.submission.take() {
            let _ = self
                .device
                .poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: None });
        }
        let maps = std::mem::take(&mut frame.maps);
        let times = frame.times.take();
        let marks = frame.marks;
        if maps.iter().any(|m| !m.1.load(Ordering::Acquire)) || times.as_ref().is_some_and(|t| !t.load(Ordering::Acquire)) {
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        }
        for (id, done) in maps {
            let Some(Some(Buffer { mirror: Some(m), .. })) = self.buffers.items.get_mut(id as usize) else { continue };
            let Some(s) = m.staging[slot]
                .as_ref()
                .filter(|_| done.load(Ordering::Acquire))
            else {
                continue;
            };
            if let Ok(view) = s.slice(..).get_mapped_range() {
                let n = m.bytes.len();
                m.bytes.copy_from_slice(&view[..n]);
            }
            s.unmap();
        }
        if let (Some(t), Some(done)) = (&self.timer, times) {
            let read = &t.read[slot];
            if done.load(Ordering::Acquire)
                && let Ok(view) = read.slice(..).get_mapped_range()
            {
                let ts: Vec<u64> = view[..marks as usize * 8]
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| u64::from_le_bytes(*c))
                    .collect();
                drop(view);
                read.unmap();
                let period = t.period as f64 / 1e6;
                self.gpu_times = ts
                    .windows(2)
                    .map(|w| (w[1].saturating_sub(w[0]) as f64 * period) as f32)
                    .collect();
            }
        }
    }

    fn shader_module(&self, shader: &Shader) -> wgpu::ShaderModule {
        self.device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(shader.module.clone())),
            })
    }

    /// Layouts for `shader` run from `entries`: the texture table, then the buffers they use, if
    /// any. Only those: wgpu wants all of a layout bound, and one file may hold several programs.
    fn layout_for(
        &self,
        shader: &Shader,
        entries: &[&str],
        label: &str,
    ) -> (wgpu::PipelineLayout, Option<(wgpu::BindGroupLayout, Vec<u32>)>) {
        let m = &shader.module;
        // Each buffer seen only by the stages that use it: a vertex stage may not write one on
        // every device.
        let mut used: BTreeMap<u32, wgpu::ShaderStages> = BTreeMap::new();
        for (i, e) in m
            .entry_points
            .iter()
            .enumerate()
            .filter(|(_, e)| entries.contains(&e.name.as_str()))
        {
            let stage = match e.stage {
                naga::ShaderStage::Vertex => wgpu::ShaderStages::VERTEX,
                naga::ShaderStage::Fragment => wgpu::ShaderStages::FRAGMENT,
                _ => wgpu::ShaderStages::COMPUTE,
            };
            let info = shader.info.get_entry_point(i);
            for (h, v) in m.global_variables.iter() {
                if let Some(b) = v
                    .binding
                    .as_ref()
                    .filter(|b| b.group == 1 && !info[h].is_empty())
                {
                    *used.entry(b.binding).or_insert(wgpu::ShaderStages::NONE) |= stage;
                }
            }
        }
        let entries: Vec<_> = shader
            .buffers
            .iter()
            .filter_map(|&(binding, kind)| Some((binding, kind, *used.get(&binding)?)))
            .map(|(binding, kind, visibility)| wgpu::BindGroupLayoutEntry {
                binding,
                visibility,
                ty: wgpu::BindingType::Buffer {
                    ty: match kind {
                        BufferKind::Uniform => wgpu::BufferBindingType::Uniform,
                        BufferKind::StorageRead => wgpu::BufferBindingType::Storage { read_only: true },
                        BufferKind::StorageWrite => wgpu::BufferBindingType::Storage { read_only: false },
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let group = (!entries.is_empty()).then(|| {
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &entries });
            (layout, entries.iter().map(|e| e.binding).collect::<Vec<_>>())
        });
        let layouts = [Some(&self.table.layout), group.as_ref().map(|g| &g.0)];
        let layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &layouts[..if group.is_some() { 2 } else { 1 }],
                immediate_size: shader.push_constant_size,
            });
        (layout, group)
    }

    /// Makes something with errors caught instead of logged, and returns the first.
    fn checked<T>(&self, label: &str, make: impl FnOnce() -> T) -> Result<T, Error> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let made = make();
        match pollster::block_on(scope.pop()) {
            Some(wgpu::Error::Validation { description, .. }) => Err(Error(format!("{label}: {description}"))),
            Some(e) => Err(Error(format!("{label}: {e}"))),
            None => Ok(made),
        }
    }

    fn add_pipeline(&mut self, kind: Kind, shader: &Shader, group: Option<(wgpu::BindGroupLayout, Vec<u32>)>, label: &str) -> PipelineId {
        let (group, buffers) = group.unzip();
        let buffers = buffers.unwrap_or_default();
        let writes = shader
            .buffers
            .iter()
            .filter(|b| b.1 == BufferKind::StorageWrite && buffers.contains(&b.0))
            .map(|b| b.0)
            .collect();
        let label = label.to_string();
        PipelineId(
            self.pipelines
                .insert(Pipeline { kind, group, buffers, writes, immediates: shader.push_constant_size, label }),
        )
    }

    /// The bind group of `pipeline`'s buffers as bound now, `None` if one it needs isn't.
    fn buffer_group(&mut self, pipeline: u32, bound: &BTreeMap<u32, (u32, u64, u64)>) -> Option<wgpu::BindGroup> {
        let p = self.pipelines.get(pipeline);
        let layout = p.group.as_ref()?;
        let mut key = Vec::with_capacity(p.buffers.len());
        for b in &p.buffers {
            let Some(&(buffer, offset, size)) = bound.get(b) else {
                log::warn!(target: "wgpu", "{}: nothing bound at {b}, skipped", p.label);
                return None;
            };
            key.push((*b, buffer, offset, size));
        }
        let key = (pipeline, key);
        if let Some((g, used)) = self.groups.get_mut(&key) {
            *used = self.frame_no;
            return Some(g.clone());
        }
        let entries: Vec<_> = key
            .1
            .iter()
            .map(|&(binding, buffer, offset, size)| wgpu::BindGroupEntry {
                binding,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &self.buffers.get(buffer).raw,
                    offset,
                    size: if size == u64::MAX { None } else { std::num::NonZeroU64::new(size) },
                }),
            })
            .collect();
        let g = self
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries });
        self.groups.insert(key, (g.clone(), self.frame_no));
        Some(g)
    }

    /// Zeroes, for `Cmd::Draw*IndirectCount` where draws with a count don't work, the records of the
    /// pass starting at `cmds[0]` past their count, into `scratch`: where each one's copy starts.
    fn compact_draws(&mut self, enc: &mut wgpu::CommandEncoder, cmds: &[Cmd]) -> Vec<u64> {
        let Some(compact) = self.internal.compact.clone() else { return Vec::new() };
        let draws: Vec<(BufferId, u64, BufferId, u64, u32, u64)> = cmds
            .iter()
            .take_while(|c| !matches!(c, Cmd::EndPass))
            .filter_map(|c| match *c {
                Cmd::DrawIndexedIndirectCount { buffer, offset, count_buffer, count_offset, max } => {
                    Some((buffer, offset, count_buffer, count_offset, max, size_of::<DrawIndexedArgs>() as u64))
                }
                Cmd::DrawIndirectCount { buffer, offset, count_buffer, count_offset, max } => {
                    Some((buffer, offset, count_buffer, count_offset, max, size_of::<DrawArgs>() as u64))
                }
                _ => None,
            })
            .collect();
        if draws.is_empty() {
            return Vec::new();
        }
        let total: u64 = draws.iter().map(|d| d.4 as u64 * d.5).sum::<u64>().max(16);
        if self.internal.scratch.as_ref().is_none_or(|s| s.1 < total) {
            let size = total.next_power_of_two();
            let raw = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("compacted draws"),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT,
                mapped_at_creation: false,
            });
            self.internal.scratch = Some((raw, size));
        }
        let scratch = self.internal.scratch.as_ref().unwrap().0.clone();
        let layout = compact.get_bind_group_layout(0);
        let mut pass = enc.begin_compute_pass(&Default::default());
        pass.set_pipeline(&compact);
        let mut starts = Vec::new();
        let mut at = 0u64;
        for (buffer, offset, count_buffer, count_offset, max, stride) in draws {
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.buffers.get(buffer.0).raw.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.buffers.get(count_buffer.0).raw.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: scratch.as_entire_binding() },
                ],
            });
            pass.set_bind_group(0, &group, &[]);
            let args = [(offset / 4) as u32, (count_offset / 4) as u32, max, (at / 4) as u32, (stride / 4) as u32];
            pass.set_immediates(0, bytes(&args));
            pass.dispatch_workgroups(max.div_ceil(64), 1, 1);
            starts.push(at);
            at += max as u64 * stride;
        }
        starts
    }
}

fn bytes(words: &[u32]) -> &[u8] {
    // SAFETY: plain integers, read as their bytes.
    unsafe { std::slice::from_raw_parts(words.as_ptr().cast(), words.len() * 4) }
}

fn internal_pipeline(device: &wgpu::Device, label: &str, source: &str) -> Result<wgpu::ComputePipeline, String> {
    let module = wgsl(label, source).map_err(|e| e.0)?;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
    });
    Ok(device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    }))
}

const DEPTH_COPY: &str = r#"
@group(0) @binding(0) var depth: texture_depth_2d;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(depth);
    if id.x < size.x && id.y < size.y {
        out[id.y * size.x + id.x] = textureLoad(depth, vec2<i32>(id.xy), 0);
    }
}
"#;

const COMPACT: &str = r#"
struct Args { src: u32, count: u32, max: u32, dst: u32, stride: u32 }
@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read> counts: array<u32>;
@group(0) @binding(2) var<storage, read_write> dst: array<u32>;
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

/// What the command list has bound, applied to a pass just before it draws or dispatches.
#[derive(Default)]
struct Bound {
    pipeline: Option<u32>,
    buffers: BTreeMap<u32, (u32, u64, u64)>,
    immediates: Option<(u32, u32)>,
    index: Option<u32>,
    /// What the current pass lacks: pipeline, buffers, immediates, index buffer.
    stale: [bool; 4],
}

#[allow(clippy::large_enum_variant)]
enum Pass {
    None,
    Render { pass: wgpu::RenderPass<'static>, without: u32, size: (u32, u32) },
    Compute(wgpu::ComputePass<'static>),
}

impl Backend for WgpuBackend {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_name(&self) -> &str {
        &self.device_name
    }

    fn frames_in_flight(&self) -> usize {
        FRAMES
    }

    fn surface_size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    fn resize(&mut self, width: u32, height: u32) {
        if self.want_size != (width, height) {
            self.want_size = (width, height);
            self.dirty_surface = true;
        }
    }

    fn set_vsync(&mut self, vsync: bool) {
        let modes = &self.present_modes;
        let mode = if vsync {
            wgpu::PresentMode::Fifo
        } else if modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else if modes.contains(&wgpu::PresentMode::Immediate) {
            wgpu::PresentMode::Immediate
        } else {
            wgpu::PresentMode::Fifo
        };
        if self.config.present_mode != mode {
            self.config.present_mode = mode;
            self.dirty_surface = true;
        }
    }

    fn create_buffer(&mut self, size: u64, usage: u32, label: &str) -> BufferId {
        use wgpu::BufferUsages as U;
        let mut flags = U::COPY_DST | U::COPY_SRC;
        for (ours, theirs) in
            [(usage::STORAGE, U::STORAGE), (usage::INDEX, U::INDEX), (usage::INDIRECT, U::INDIRECT), (usage::UNIFORM, U::UNIFORM)]
        {
            if usage & ours != 0 {
                flags |= theirs;
            }
        }
        let size = size.max(16).next_multiple_of(4);
        let raw = self
            .device
            .create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage: flags, mapped_at_creation: false });
        let mirror = (usage & (usage::HOST | usage::READBACK) != 0).then(|| Mirror {
            bytes: vec![0; size as usize],
            host: usage & usage::HOST != 0,
            dirty: false,
            staging: Default::default(),
        });
        BufferId(self.buffers.insert(Buffer { raw, mirror }))
    }

    fn mapped(&mut self, buffer: BufferId) -> &mut [u8] {
        self.ensure_slot_ready();
        let m = self
            .buffers
            .get_mut(buffer.0)
            .mirror
            .as_mut()
            .expect("buffer is not host visible");
        m.dirty |= m.host;
        &mut m.bytes
    }

    fn write_buffer(&mut self, buffer: BufferId, offset: u64, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let b = self.buffers.get_mut(buffer.0);
        match &mut b.mirror {
            // It goes up whole at the end of the frame, which would undo a write sent apart.
            Some(m) if m.host => {
                m.bytes[offset as usize..offset as usize + data.len()].copy_from_slice(data);
                m.dirty = true;
            }
            _ => {
                debug_assert!(offset.is_multiple_of(4) && data.len().is_multiple_of(4), "wgpu writes buffers by whole words");
                self.queue.write_buffer(&b.raw, offset, data);
            }
        }
    }

    fn destroy_buffer(&mut self, buffer: BufferId) {
        if self.buffers.remove(buffer.0).is_some() {
            // Its slot may be reused at once: nothing may still point at it.
            self.groups
                .retain(|k, _| !k.1.iter().any(|e| e.1 == buffer.0));
            for f in &mut self.frames {
                f.maps.retain(|m| m.0 != buffer.0);
            }
        }
    }

    fn create_texture(&mut self, desc: &TextureDesc, label: &str) -> TextureId {
        use wgpu::TextureUsages as U;
        let depth = desc.format == Format::Depth32;
        let usage = if depth {
            U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_SRC
        } else if desc.render_target {
            U::TEXTURE_BINDING | U::COPY_DST | U::RENDER_ATTACHMENT | U::COPY_SRC
        } else {
            U::TEXTURE_BINDING | U::COPY_DST
        };
        let raw = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: desc.width, height: desc.height, depth_or_array_layers: 1 },
            mip_level_count: desc.mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format(desc.format),
            usage,
            view_formats: &[],
        });
        let view = raw.create_view(&Default::default());
        let index = if depth {
            u32::MAX
        } else {
            let t = &mut self.table;
            let index = t.free.pop().unwrap_or(t.views.len() as u32);
            assert!(index < MAX_TEXTURES, "more than {MAX_TEXTURES} textures");
            if index as usize == t.views.len() {
                t.views.push(None);
            }
            t.views[index as usize] = Some(view.clone());
            t.groups.clear();
            index
        };
        TextureId(
            self.textures
                .insert(Texture { raw, view, desc: desc.clone(), index }),
        )
    }

    fn write_texture(&mut self, texture: TextureId, mip: u32, data: &[u8]) {
        let t = self.textures.get(texture.0);
        let (w, h) = ((t.desc.width >> mip).max(1), (t.desc.height >> mip).max(1));
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &t.raw, mip_level: mip, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * t.desc.format.bytes_per_pixel()), rows_per_image: None },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    fn texture_index(&self, texture: TextureId) -> u32 {
        self.textures.get(texture.0).index
    }

    fn destroy_texture(&mut self, texture: TextureId) {
        if let Some(t) = self.textures.remove(texture.0)
            && t.index != u32::MAX
        {
            self.table.views[t.index as usize] = None;
            self.table.free.push(t.index);
            self.table.groups.clear();
        }
    }

    fn create_pipeline(&mut self, desc: &PipelineDesc) -> Result<PipelineId, Error> {
        let module = self.shader_module(desc.shader);
        let stages: Vec<&str> = std::iter::once(desc.vertex).chain(desc.fragment).collect();
        let (layout, group) = self.layout_for(desc.shader, &stages, desc.label);
        let blend = |c: (wgpu::BlendFactor, wgpu::BlendFactor), a: (wgpu::BlendFactor, wgpu::BlendFactor)| wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: c.0, dst_factor: c.1, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: a.0, dst_factor: a.1, operation: wgpu::BlendOperation::Add },
        };
        use wgpu::BlendFactor as B;
        let blend = match desc.blend {
            Blend::None => None,
            Blend::Alpha => Some(blend((B::SrcAlpha, B::OneMinusSrcAlpha), (B::One, B::OneMinusSrcAlpha))),
            Blend::Premultiplied => Some(blend((B::One, B::OneMinusSrcAlpha), (B::One, B::OneMinusSrcAlpha))),
            Blend::Additive => Some(blend((B::SrcAlpha, B::One), (B::Zero, B::One))),
        };
        let targets = [desc
            .color
            .map(|f| wgpu::ColorTargetState { format: self.format(f), blend, write_mask: wgpu::ColorWrites::ALL })];
        let compare = |c: Compare| match c {
            Compare::Always => wgpu::CompareFunction::Always,
            Compare::Greater => wgpu::CompareFunction::Greater,
            Compare::GreaterEqual => wgpu::CompareFunction::GreaterEqual,
            Compare::Less => wgpu::CompareFunction::Less,
            Compare::LessEqual => wgpu::CompareFunction::LessEqual,
        };
        let raw = self.checked(desc.label, || {
            self.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(desc.label),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some(desc.vertex),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: match desc.topology {
                            Topology::Triangles => wgpu::PrimitiveTopology::TriangleList,
                            Topology::Lines => wgpu::PrimitiveTopology::LineList,
                        },
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: desc.cull_back.then_some(wgpu::Face::Back),
                        ..Default::default()
                    },
                    depth_stencil: desc.depth.map(|d| wgpu::DepthStencilState {
                        format: DEPTH,
                        depth_write_enabled: Some(d.write),
                        depth_compare: Some(compare(d.compare)),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: desc.fragment.map(|f| wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(f),
                        compilation_options: Default::default(),
                        targets: if desc.color.is_some() { &targets } else { &[] },
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        })?;
        Ok(self.add_pipeline(Kind::Render(raw), desc.shader, group, desc.label))
    }

    fn create_compute(&mut self, desc: &ComputeDesc) -> Result<PipelineId, Error> {
        let module = self.shader_module(desc.shader);
        let (layout, group) = self.layout_for(desc.shader, &[desc.entry], desc.label);
        let raw = self.checked(desc.label, || {
            self.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(desc.label),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some(desc.entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
        })?;
        Ok(self.add_pipeline(Kind::Compute(raw), desc.shader, group, desc.label))
    }

    fn destroy_pipeline(&mut self, pipeline: PipelineId) {
        if self.pipelines.remove(pipeline.0).is_some() {
            self.groups.retain(|k, _| k.0 != pipeline.0);
        }
    }

    fn begin_frame(&mut self) -> Option<FrameInfo> {
        self.ensure_slot_ready();
        if self.dirty_surface {
            self.configure();
        }
        if self.dirty_surface {
            return None;
        }
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                self.dirty_surface = true;
                t
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.dirty_surface = true;
                return None;
            }
            _ => return None,
        };
        self.current = Some(texture);
        Some(FrameInfo { width: self.config.width, height: self.config.height, slot: self.slot })
    }

    fn end_frame(&mut self, list: &CommandList) {
        let Some(surface) = self.current.take() else { return };
        self.frame_no += 1;
        let slot = self.slot;
        for b in self.buffers.items.iter_mut().flatten() {
            if let Some(m) = b.mirror.as_mut().filter(|m| m.dirty) {
                self.queue.write_buffer(&b.raw, 0, &m.bytes);
                m.dirty = false;
            }
        }
        let surface_view = surface
            .texture
            .create_view(&wgpu::TextureViewDescriptor { format: Some(self.view_format), ..Default::default() });
        let mut enc = self.device.create_command_encoder(&Default::default());
        let first_query = slot as u32 * MAX_MARKS;
        // Uploads went ahead of the encoder: they take no time here.
        let mut marks = 0;
        if let Some(t) = &self.timer {
            enc.write_timestamp(&t.queries, first_query);
            enc.write_timestamp(&t.queries, first_query + 1);
            marks = 2;
        }
        let mut bound = Bound::default();
        let mut pass = Pass::None;
        let mut read_back: Vec<u32> = Vec::new();
        let mut compacted: Vec<u64> = Vec::new();
        for (i, cmd) in list.cmds.iter().enumerate() {
            match *cmd {
                Cmd::BeginPass { color, depth } => {
                    // The encoder is free again once the pass before is gone.
                    drop(std::mem::replace(&mut pass, Pass::None));
                    compacted = self.compact_draws(&mut enc, &list.cmds[i..]);
                    compacted.reverse();
                    let mut size = (self.config.width, self.config.height);
                    let mut without = u32::MAX;
                    let color = color.map(|(target, clear)| {
                        let view = match target {
                            Target::Surface => surface_view.clone(),
                            Target::Texture(t) => {
                                let t = self.textures.get(t.0);
                                size = (t.desc.width, t.desc.height);
                                without = t.index;
                                t.view.clone()
                            }
                        };
                        let load = match clear {
                            Some(c) => wgpu::LoadOp::Clear(wgpu::Color { r: c[0] as f64, g: c[1] as f64, b: c[2] as f64, a: c[3] as f64 }),
                            None => wgpu::LoadOp::Load,
                        };
                        (view, load)
                    });
                    let depth = depth.map(|(t, clear)| {
                        let t = self.textures.get(t.0);
                        size = (t.desc.width, t.desc.height);
                        (t.view.clone(), clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear))
                    });
                    let colors = [color
                        .as_ref()
                        .map(|(view, load)| wgpu::RenderPassColorAttachment {
                            view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations { load: *load, store: wgpu::StoreOp::Store },
                        })];
                    let p = enc
                        .begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: None,
                            color_attachments: if color.is_some() { &colors } else { &[] },
                            depth_stencil_attachment: depth
                                .as_ref()
                                .map(|(view, load)| wgpu::RenderPassDepthStencilAttachment {
                                    view,
                                    depth_ops: Some(wgpu::Operations { load: *load, store: wgpu::StoreOp::Store }),
                                    stencil_ops: None,
                                }),
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        })
                        .forget_lifetime();
                    pass = Pass::Render { pass: p, without, size };
                    bound.stale = [true; 4];
                }
                Cmd::EndPass => pass = Pass::None,
                Cmd::SetPipeline(p) => {
                    bound.pipeline = Some(p.0);
                    bound.stale = [true, true, true, bound.stale[3]];
                }
                Cmd::BindBuffer { binding, buffer, offset, size } => {
                    // Kept by binding: each pipeline takes those it reads when it draws.
                    bound.buffers.insert(binding, (buffer.0, offset, size));
                    bound.stale[1] = true;
                }
                Cmd::PushConstants { start, len } => {
                    bound.immediates = Some((start, len));
                    bound.stale[2] = true;
                }
                Cmd::IndexBuffer(b) => {
                    bound.index = Some(b.0);
                    bound.stale[3] = true;
                }
                Cmd::Scissor { x, y, w, h } => {
                    if let Pass::Render { pass, size, .. } = &mut pass {
                        // wgpu refuses a scissor past the target, which Vulkan clips.
                        let (x, y) = ((x.max(0) as u32).min(size.0), (y.max(0) as u32).min(size.1));
                        pass.set_scissor_rect(x, y, w.min(size.0 - x), h.min(size.1 - y));
                    }
                }
                Cmd::Draw { .. }
                | Cmd::DrawIndirect { .. }
                | Cmd::DrawIndexedIndirect { .. }
                | Cmd::DrawIndexedIndirectCount { .. }
                | Cmd::DrawIndirectCount { .. } => {
                    let Pass::Render { pass, without, .. } = &mut pass else { continue };
                    let Some(p) = bound.pipeline else { continue };
                    let Kind::Render(raw) = &self.pipelines.get(p).kind else { continue };
                    let (raw, fresh) = (raw.clone(), bound.stale[0]);
                    let Some((group, immediates)) = self.prepare(&mut bound, list, &mut read_back) else { continue };
                    if fresh {
                        pass.set_pipeline(&raw);
                        pass.set_bind_group(0, &self.table.group(&self.device, *without), &[]);
                    }
                    if let Some(g) = group {
                        pass.set_bind_group(1, &g, &[]);
                    }
                    if let Some(d) = immediates {
                        pass.set_immediates(0, d);
                    }
                    if bound.stale[3]
                        && let Some(b) = bound.index
                    {
                        pass.set_index_buffer(self.buffers.get(b).raw.slice(..), wgpu::IndexFormat::Uint32);
                        bound.stale[3] = false;
                    }
                    let raw = |b: BufferId| &self.buffers.get(b.0).raw;
                    match *cmd {
                        Cmd::Draw { vertices, instances, first_vertex, first_instance } => {
                            pass.draw(first_vertex..first_vertex + vertices, first_instance..first_instance + instances)
                        }
                        Cmd::DrawIndirect { buffer, offset, count } => pass.multi_draw_indirect(raw(buffer), offset, count),
                        Cmd::DrawIndexedIndirect { buffer, offset, count } => pass.multi_draw_indexed_indirect(raw(buffer), offset, count),
                        Cmd::DrawIndexedIndirectCount { buffer, offset, count_buffer, count_offset, max } => match compacted.pop() {
                            Some(at) => pass.multi_draw_indexed_indirect(&self.internal.scratch.as_ref().unwrap().0, at, max),
                            None => pass.multi_draw_indexed_indirect_count(raw(buffer), offset, raw(count_buffer), count_offset, max),
                        },
                        Cmd::DrawIndirectCount { buffer, offset, count_buffer, count_offset, max } => match compacted.pop() {
                            Some(at) => pass.multi_draw_indirect(&self.internal.scratch.as_ref().unwrap().0, at, max),
                            None => pass.multi_draw_indirect_count(raw(buffer), offset, raw(count_buffer), count_offset, max),
                        },
                        _ => unreachable!(),
                    }
                }
                Cmd::Dispatch { x, y, z } => {
                    if matches!(pass, Pass::None) {
                        pass = Pass::Compute(
                            enc.begin_compute_pass(&Default::default())
                                .forget_lifetime(),
                        );
                        bound.stale = [true, true, true, bound.stale[3]];
                    }
                    let Pass::Compute(cp) = &mut pass else { continue };
                    let Some(p) = bound.pipeline else { continue };
                    let Kind::Compute(raw) = &self.pipelines.get(p).kind else { continue };
                    let (raw, fresh) = (raw.clone(), bound.stale[0]);
                    let Some((group, immediates)) = self.prepare(&mut bound, list, &mut read_back) else { continue };
                    if fresh {
                        cp.set_pipeline(&raw);
                        cp.set_bind_group(0, &self.table.group(&self.device, u32::MAX), &[]);
                    }
                    if let Some(g) = group {
                        cp.set_bind_group(1, &g, &[]);
                    }
                    if let Some(d) = immediates {
                        cp.set_immediates(0, d);
                    }
                    cp.dispatch_workgroups(x, y, z);
                }
                // wgpu orders what reads after what writes by itself.
                Cmd::Barrier => {}
                Cmd::CopyDepthToBuffer { depth, buffer } => {
                    pass = Pass::None;
                    self.written(buffer.0, &mut read_back);
                    let t = self.textures.get(depth.0);
                    let layout = self.internal.depth.get_bind_group_layout(0);
                    let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout: &layout,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.view) },
                            wgpu::BindGroupEntry { binding: 1, resource: self.buffers.get(buffer.0).raw.as_entire_binding() },
                        ],
                    });
                    let mut cp = enc.begin_compute_pass(&Default::default());
                    cp.set_pipeline(&self.internal.depth);
                    cp.set_bind_group(0, &group, &[]);
                    cp.dispatch_workgroups(t.desc.width.div_ceil(8), t.desc.height.div_ceil(8), 1);
                }
                Cmd::Mark => {
                    if let Some(t) = &self.timer
                        && marks < MAX_MARKS - 1
                    {
                        match &mut pass {
                            Pass::None => enc.write_timestamp(&t.queries, first_query + marks),
                            Pass::Render { pass, .. } => pass.write_timestamp(&t.queries, first_query + marks),
                            Pass::Compute(cp) => cp.write_timestamp(&t.queries, first_query + marks),
                        }
                        marks += 1;
                    }
                }
            }
        }
        drop(pass);

        for &id in &read_back {
            let device = &self.device;
            let Buffer { raw, mirror: Some(m) } = self.buffers.get_mut(id) else { continue };
            let s = m.staging[slot].get_or_insert_with(|| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("readback"),
                    size: raw.size(),
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            });
            enc.copy_buffer_to_buffer(raw, 0, s, 0, raw.size());
        }
        if std::mem::take(&mut self.capture_requested) && self.config.usage.contains(wgpu::TextureUsages::COPY_SRC) {
            let (w, h) = (self.config.width, self.config.height);
            let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("capture"),
                size: (row * h) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            enc.copy_texture_to_buffer(
                surface.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            self.capture = Some((buf, w, h, row));
        }
        if let Some(t) = &self.timer {
            enc.write_timestamp(&t.queries, first_query + marks);
            marks += 1;
            let at = slot as u64 * wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;
            enc.resolve_query_set(&t.queries, first_query..first_query + marks, &t.resolve, at);
            enc.copy_buffer_to_buffer(&t.resolve, at, &t.read[slot], 0, marks as u64 * 8);
        }
        let submission = self.queue.submit([enc.finish()]);
        let mapped = |b: &wgpu::Buffer| {
            let done = Arc::new(AtomicBool::new(false));
            let flag = done.clone();
            b.slice(..)
                .map_async(wgpu::MapMode::Read, move |r| flag.store(r.is_ok(), Ordering::Release));
            done
        };
        let mut maps = Vec::new();
        for id in read_back {
            if let Buffer { mirror: Some(m), .. } = self.buffers.get(id)
                && let Some(s) = &m.staging[slot]
            {
                maps.push((id, mapped(s)));
            }
        }
        let times = self.timer.as_ref().map(|t| mapped(&t.read[slot]));
        let frame = &mut self.frames[slot];
        frame.submission = Some(submission);
        frame.waited = false;
        frame.maps = maps;
        frame.marks = marks;
        frame.times = times;
        self.queue.present(surface);
        if self.frame_no.is_multiple_of(KEEP_GROUPS) {
            let now = self.frame_no;
            self.groups.retain(|_, g| g.1 + KEEP_GROUPS > now);
        }
        self.slot = (slot + 1) % FRAMES;
    }

    fn request_capture(&mut self) {
        self.capture_requested = true;
    }

    fn take_capture(&mut self) -> Option<Capture> {
        let (buf, width, height, row) = self.capture.take()?;
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let data = buf.slice(..).get_mapped_range().ok()?;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            rgba.extend_from_slice(&data[y * row as usize..][..width as usize * 4]);
        }
        if matches!(self.config.format, wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm) {
            for px in rgba.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
        }
        Some(Capture { width, height, rgba })
    }

    fn gpu_times(&self) -> &[f32] {
        &self.gpu_times
    }

    fn wait_idle(&mut self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

impl WgpuBackend {
    /// What of the current pipeline's buffers and immediates the pass lacks: its group 1 and the
    /// bytes to set. `None` when a buffer it needs isn't bound, and the draw can't go.
    fn prepare<'l>(
        &mut self,
        bound: &mut Bound,
        list: &'l CommandList,
        read_back: &mut Vec<u32>,
    ) -> Option<(Option<wgpu::BindGroup>, Option<&'l [u8]>)> {
        let p = bound.pipeline?;
        if bound.stale[1] {
            for i in 0..self.pipelines.get(p).buffers.len() {
                let pl = self.pipelines.get(p);
                let (binding, writes) = (pl.buffers[i], pl.writes.contains(&pl.buffers[i]));
                if let Some(&(b, ..)) = bound.buffers.get(&binding)
                    && (writes || self.buffers.get(b).mirror.as_ref().is_some_and(|m| !m.host))
                {
                    self.written(b, read_back);
                }
            }
        }
        let group =
            if bound.stale[1] && self.pipelines.get(p).group.is_some() { Some(self.buffer_group(p, &bound.buffers)?) } else { None };
        let size = self.pipelines.get(p).immediates;
        let immediates = bound
            .immediates
            .filter(|_| bound.stale[2] && size > 0)
            .map(|(start, len)| &list.data[start as usize..(start + (len.min(size) & !3)) as usize]);
        bound.stale[..3].fill(false);
        Some((group, immediates))
    }

    /// Notes that this frame may write `buffer`: if the CPU sees it, it is copied back.
    fn written(&self, buffer: u32, read_back: &mut Vec<u32>) {
        if self.buffers.get(buffer).mirror.is_some() && !read_back.contains(&buffer) {
            read_back.push(buffer);
        }
    }
}
