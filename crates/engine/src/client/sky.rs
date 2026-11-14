//! The sky: what is in it is data (`registry::SkyDef`), registered by mods per dimension, and every
//! object moves on its own. All of it is one pipeline and a few draws after the opaque world, so
//! only the pixels no block covers pay for it; stars are made in the vertex shader from their
//! number and need no buffer at all.

use glam::{Mat3, Vec3};
use rustc_hash::FxHashMap;
use weft_abi::sky_kind;
use weft_render::*;

use super::world_render::Camera;
use crate::assets::{Assets, decode_png};
use crate::content::{DimensionDef, content};
use crate::gpu_layout::{GpuSky, bind, sky as kind};
use crate::registry::SkyDef;
use crate::shaders::{Hook, Slots};

pub const HOOK: Hook = Hook {
    kind: "sky",
    entry: "shade",
    dispatch: "sky_shade",
    params: "p: SkyPixel",
    args: "p",
    ret: "vec4<f32>",
    fallback: "return p.albedo * p.color;",
};

/// A color (not linear) under rain: vanilla's going gray.
pub fn rained(c: Vec3, rain: f32, by: f32) -> Vec3 {
    let gray = (c.x * 0.3 + c.y * 0.59 + c.z * 0.11) * 0.6;
    c.lerp(Vec3::splat(gray), rain * by)
}

/// 0xRRGGBB as 0..1, not yet linear.
pub fn rgb(c: u32) -> Vec3 {
    Vec3::new(((c >> 16) & 255) as f32, ((c >> 8) & 255) as f32, (c & 255) as f32) / 255.0
}

/// Where on its orbit it is, in turns.
fn turn_of(s: &SkyDef, d: &DimensionDef, time: f64) -> f64 {
    let moved = if s.orbit.is_infinite() {
        d.day_turn(time)
    } else if s.orbit == 0.0 {
        0.0
    } else {
        time / s.orbit as f64
    };
    (moved + s.phase as f64).rem_euclid(1.0)
}

/// Its frame in the world at `turn`: x toward it, y along its way, z out of the orbit's plane.
fn frame_of(s: &SkyDef, turn: f64) -> Mat3 {
    Mat3::from_rotation_y(s.heading) * Mat3::from_rotation_x(s.tilt) * Mat3::from_rotation_z((turn * std::f64::consts::TAU) as f32)
}

/// How far around the sunset of a body is at `height` (the sine of its elevation): how much it
/// glows and how far along it is, 0 at the end of the dusk, 1 at the start. Vanilla's
/// getSunriseColor.
fn sunset(height: f32) -> Option<(f32, f32)> {
    (height.abs() <= 0.4).then(|| {
        let along = height / 0.4 * 0.5 + 0.5;
        let strength = 1.0 - (1.0 - (along * std::f32::consts::PI).sin()) * 0.99;
        (strength * strength, along)
    })
}

/// The glow's color so far along a sunset: `glow` at its start, deep red at the end.
fn glow_color(glow: u32, along: f32) -> Vec3 {
    rgb(glow) * Vec3::new(along * 0.3 + 0.7, (along * along * 0.7 + 0.2) / 0.9, 1.0)
}

/// Where the glowing bodies of a dimension stand now: toward them along the horizon, their
/// color so far (not linear), how strong, and how far along.
fn glows<'a>(
    dim: usize,
    d: &'a DimensionDef,
    time: f64,
    weights: &'a FxHashMap<String, f32>,
) -> impl Iterator<Item = (Vec3, Vec3, f32, f32)> + 'a {
    let list = content().game.sky.get(dim).map_or(&[][..], |l| &l[..]);
    list.iter()
        .filter(|s| s.glow != 0 && s.kind == sky_kind::BODY)
        .filter_map(move |s| {
            let toward = frame_of(s, turn_of(s, d, time)).x_axis;
            let (strength, along) = sunset(toward.y)?;
            let flat = Vec3::new(toward.x, 0.0, toward.z).try_normalize()?;
            Some((flat, glow_color(s.glow, along), strength * weights.get(&s.name).copied().unwrap_or(1.0), along))
        })
}

/// A fog color (not linear) warmed by the sunsets the camera looks at, like vanilla's.
pub fn warmed_by_glows(dim: usize, d: &DimensionDef, time: f64, fog: Vec3, forward: Vec3, weights: &FxHashMap<String, f32>) -> Vec3 {
    glows(dim, d, time, weights).fold(fog, |c, (flat, color, strength, _)| c.lerp(color, flat.dot(forward).max(0.0) * strength))
}

/// Directions through the screen's middle and one screen unit right and up of it. From the eye:
/// bobbing moves the camera by more than the near plane is away from it.
pub(crate) fn screen_rays(cam: &Camera, aspect: f32) -> (Vec3, Vec3, Vec3) {
    let view = cam.view();
    let inverse = (cam.proj(aspect) * view).inverse();
    let eye = view.inverse().w_axis.truncate();
    // Reversed depth: any point on the near plane gives the ray through a screen position.
    let ray = |x: f32, y: f32| inverse.project_point3(Vec3::new(x, y, 1.0)) - eye;
    (ray(0.0, 0.0), ray(1.0, 0.0), ray(0.0, 1.0))
}

struct Picture {
    texture: TextureId,
    index: u32,
    size: [u32; 2],
}

pub struct Sky {
    pipe: Option<PipelineId>,
    clouds_pipe: Option<PipelineId>,
    /// Shader slot of each object, per dimension.
    slots: Vec<Vec<u32>>,
    buffers: Vec<(BufferId, u32)>,
    /// Pictures as they were first drawn, `None` for missing ones.
    pictures: FxHashMap<String, Option<Picture>>,
    assets: Assets,
    list: Vec<GpuSky>,
    /// The fog color of each liquid, linear.
    block_colors: std::cell::RefCell<FxHashMap<u32, Vec3>>,
    /// This frame's buffer and the entries over the screen: where they start, how many.
    overlay: Option<(BufferId, u32, u32)>,
}

/// A picture over the whole screen, drawn last: from inside a liquid, a block or a fire.
#[derive(Clone)]
pub struct Overlay {
    pub picture: String,
    /// Linear rgb and how much shows.
    pub color: [f32; 4],
    pub tiles: f32,
    /// Only the lower part of the screen.
    pub lower: bool,
    /// Which frame of an animated picture.
    pub frame: u32,
}

/// The average color of a liquid's first frame, its tint in.
fn average_color(assets: &Assets, state: u32) -> Vec3 {
    let c = content();
    let st = &c.states[state as usize];
    let img = c
        .textures
        .get(st.textures[0] as usize)
        .and_then(|n| assets.read(&assets.picture(weft_abi::layers(n).next()?.name), "textures", "png"))
        .and_then(|b| decode_png(&b));
    let Some(img) = img else { return Vec3::splat(0.5) };
    let (mut sum, mut weight) = (Vec3::ZERO, 0.0);
    for p in img
        .rgba
        .chunks_exact(4)
        .take((img.width * img.width) as usize)
    {
        let a = p[3] as f32 / 255.0;
        sum += Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32) / 255.0 * a;
        weight += a;
    }
    let tint = if st.tint != 0 { rgb(crate::content::item_tint(st.tint)) } else { Vec3::ONE };
    sum / f32::max(weight, 1.0) * tint
}

impl Sky {
    pub fn new(gpu: &mut dyn Backend, assets: &Assets) -> Sky {
        let mut s = Sky {
            pipe: None,
            clouds_pipe: None,
            slots: Vec::new(),
            buffers: Vec::new(),
            pictures: FxHashMap::default(),
            assets: assets.clone(),
            list: Vec::new(),
            block_colors: Default::default(),
            overlay: None,
        };
        s.reload(gpu, assets);
        s
    }

    /// Recompiles the shader with the snippets mods and packs have now, and reads pictures anew.
    pub fn reload(&mut self, gpu: &mut dyn Backend, assets: &Assets) {
        self.assets = assets.clone();
        self.drop_pictures(gpu);
        self.block_colors.get_mut().clear();
        let snippets = crate::shaders::snippets(assets, &HOOK);
        let made = crate::packs::compose(assets, "sky.wgsl", &[(&HOOK, &snippets)])
            .map_err(|e| e.0)
            .and_then(|(shader, mut slots)| {
                let pipe = gpu
                    .create_pipeline(&PipelineDesc {
                        label: "sky",
                        shader: &shader,
                        vertex: "vs",
                        fragment: Some("fs"),
                        color: Some(Format::Surface),
                        // Depth 0 is the far plane: it passes only where nothing was drawn.
                        depth: Some(DepthState { write: false, compare: Compare::GreaterEqual }),
                        blend: Blend::Premultiplied,
                        cull_back: false,
                        topology: Topology::Triangles,
                    })
                    .map_err(|e| e.0)?;
                // Clouds are things in the world: they hide what is behind them, and the shader says
                // how far each pixel is. At the far end of their reach they fade out.
                let clouds = gpu
                    .create_pipeline(&PipelineDesc {
                        label: "clouds",
                        shader: &shader,
                        vertex: "vs_clouds",
                        fragment: Some("fs_clouds"),
                        color: Some(Format::Surface),
                        depth: Some(DepthState { write: true, compare: Compare::GreaterEqual }),
                        blend: Blend::Premultiplied,
                        cull_back: false,
                        topology: Topology::Triangles,
                    })
                    .map_err(|e| e.0)?;
                Ok((pipe, clouds, slots.pop().unwrap_or_default()))
            });
        match made {
            Ok((pipe, clouds, slots)) => {
                if let Some(old) = self.pipe.replace(pipe) {
                    gpu.destroy_pipeline(old);
                }
                if let Some(old) = self.clouds_pipe.replace(clouds) {
                    gpu.destroy_pipeline(old);
                }
                self.set_slots(&slots);
            }
            Err(e) => log::error!("sky shader: {e}"),
        }
    }

    pub fn block_color(&self, state: u32) -> Vec3 {
        *self
            .block_colors
            .borrow_mut()
            .entry(state)
            .or_insert_with(|| average_color(&self.assets, state).powf(2.2))
    }

    /// What goes over the screen, after the hand.
    pub fn draw_overlay(&self, list: &mut CommandList, scene: BufferId) {
        let (Some(pipe), Some((buf, first, n))) = (self.pipe, self.overlay) else { return };
        list.pipeline(pipe);
        list.bind(bind::sky::OBJECTS, buf);
        list.bind(bind::sky::SCENE, scene);
        for at in first..first + n {
            list.push(Cmd::Draw { vertices: 3, instances: 1, first_vertex: 0, first_instance: at });
        }
    }

    /// The entries of what goes over the screen, after what is in the list so far.
    fn overlay_entries(&mut self, gpu: &mut dyn Backend, overlays: &[Overlay]) -> u32 {
        let mut n = 0;
        for o in overlays {
            let Some((index, [w, h])) = self.picture(gpu, &o.picture) else { continue };
            let frame_h = w as f32 / h as f32;
            self.list.push(GpuSky {
                axes: [[0.0; 4]; 3],
                uv: [0.0, 0.0, 1.0, frame_h],
                color: o.color,
                ids: [index, kind::OVERLAY, 0, 0],
                extra: [o.tiles, o.lower as u32 as f32, (o.frame as f32 * frame_h).fract(), 0.0],
            });
            n += 1;
        }
        n
    }

    fn set_slots(&mut self, slots: &Slots) {
        self.slots = content()
            .game
            .sky
            .iter()
            .map(|list| list.iter().map(|o| slots.slot(&o.shader)).collect())
            .collect();
        for o in content()
            .game
            .sky
            .iter()
            .flatten()
            .filter(|o| !o.shader.is_empty() && slots.slot(&o.shader) == 0)
        {
            log::warn!("sky object {}: no shader {}", o.name, o.shader);
        }
    }

    fn drop_pictures(&mut self, gpu: &mut dyn Backend) {
        for p in self.pictures.drain().filter_map(|(_, p)| p) {
            gpu.destroy_texture(p.texture);
        }
    }

    pub fn destroy(&mut self, gpu: &mut dyn Backend) {
        self.drop_pictures(gpu);
        for p in [self.pipe.take(), self.clouds_pipe.take()]
            .into_iter()
            .flatten()
        {
            gpu.destroy_pipeline(p);
        }
        for (b, _) in self.buffers.drain(..) {
            gpu.destroy_buffer(b);
        }
    }

    fn picture(&mut self, gpu: &mut dyn Backend, name: &str) -> Option<(u32, [u32; 2])> {
        if name.is_empty() {
            return None;
        }
        if !self.pictures.contains_key(name) {
            let loaded = self
                .assets
                .read(&self.assets.picture(name), "textures", "png")
                .and_then(|b| decode_png(&b));
            if loaded.is_none() {
                log::error!("sky: no picture {name}");
            }
            let picture = loaded.map(|img| {
                let texture = gpu.create_texture(
                    &TextureDesc { width: img.width, height: img.height, mips: 1, format: Format::Rgba8Srgb, render_target: false },
                    "sky picture",
                );
                gpu.write_texture(texture, 0, &img.rgba);
                Picture { texture, index: gpu.texture_index(texture), size: [img.width, img.height] }
            });
            self.pictures.insert(name.to_string(), picture);
        }
        self.pictures[name].as_ref().map(|p| (p.index, p.size))
    }

    /// `time`: game ticks with the fraction of the current one. `eye_y`: where the horizon's dark
    /// lower half starts to show.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gpu: &mut dyn Backend,
        list: &mut CommandList,
        frame: &FrameInfo,
        cam: &Camera,
        scene: BufferId,
        dim: usize,
        time: f64,
        eye_y: f64,
        environment: &weft_abi::environment::EnvironmentValues,
        weights: &FxHashMap<String, f32>,
        overlays: &[Overlay],
    ) {
        self.overlay = None;
        let Some(pipe) = self.pipe else { return };
        let c = content();
        let d = &c.dims[dim];
        if environment.sky_opacity <= 0.0 {
            self.list.clear();
            let n = self.overlay_entries(gpu, overlays);
            if n > 0 {
                let buf = crate::gpu_layout::frame_buffer(
                    gpu,
                    &mut self.buffers,
                    frame.slot,
                    n,
                    size_of::<GpuSky>() as u64,
                    usage::STORAGE,
                    "sky",
                );
                gpu.mapped(buf)[..self.list.len() * size_of::<GpuSky>()].copy_from_slice(bytemuck::cast_slice(&self.list));
                self.overlay = Some((buf, 0, n));
            }
            return;
        }
        let turn = d.day_turn(time);
        let height = (turn * std::f64::consts::TAU).sin() as f32;
        // 0 by day, 1 at night: vanilla's star brightness, before its last factor of 0.5.
        let night = if d.sky_light { (2.0 * (0.75 - height * 2.0)).clamp(0.0, 1.0).powi(2) } else { 1.0 };

        self.list.clear();
        let aspect = frame.width as f32 / frame.height.max(1) as f32;
        let (o, px, py) = screen_rays(cam, aspect);
        let axes = |m: Mat3| [m.x_axis.extend(0.0).to_array(), m.y_axis.extend(0.0).to_array(), m.z_axis.extend(0.0).to_array()];
        let below = d.sea_level().is_some_and(|sea| eye_y < sea as f64);
        self.list.push(GpuSky {
            axes: axes(Mat3::from_cols(px - o, py - o, o)),
            uv: [below as u32 as f32, 0.0, 0.0, 0.0],
            color: (rained(Vec3::from_array(environment.sky_color).powf(1.0 / 2.2), 1.0, environment.sky_gray).powf(2.2))
                .extend(environment.sky_opacity)
                .to_array(),
            ids: [u32::MAX, kind::GRADIENT, 0, 0],
            extra: [0.0; 4],
        });
        let mut draws: Vec<(u32, u32)> = vec![(3, 0)];

        let objects = c.game.sky.get(dim).map_or(&[][..], |l| &l[..]);
        let mut clouds = None;
        for (i, s) in objects.iter().enumerate() {
            // Cloud cover hides celestial objects, not the clouds or sky behind them.
            let opacity = (s.opacity[0] + (s.opacity[1] - s.opacity[0]) * night)
                * if matches!(s.kind, sky_kind::BODY | sky_kind::STARS) { 1.0 - environment.sky_cover } else { 1.0 }
                * environment.sky_opacity
                * weights.get(&s.name).copied().unwrap_or(1.0);
            if opacity <= 0.0 || (s.kind == sky_kind::CLOUDS && (environment.cloud_opacity <= 0.0 || environment.cloud_fog_end <= 0.0)) {
                continue;
            }
            if s.kind == sky_kind::CLOUDS {
                const CELL: f64 = 12.0;
                let setting = |name: &str| {
                    c.game
                        .config_names
                        .get(name)
                        .map_or(0.0, |&id| crate::config::get(id))
                };
                let lifted = s.height as f64 + setting("core:cloud_height");
                let low = (lifted - cam.pos.y) as f32;
                if low >= environment.cloud_fog_end || low + 4.0 <= -environment.cloud_fog_end {
                    continue;
                }
                let Some((index, [w, h])) = self.picture(gpu, &s.texture) else { continue };
                let (x, z) = ((cam.pos.x + time * s.drift as f64) / CELL, cam.pos.z / CELL);
                let color = rained(rgb(s.color), 1.0, environment.cloud_gray)
                    * Vec3::from_array(environment.cloud_color)
                        .max(Vec3::ZERO)
                        .powf(1.0 / 2.2);
                let color = color.lerp(Vec3::splat((color.x * 0.3 + color.y * 0.59 + color.z * 0.11) * 0.2), environment.cloud_dark);
                let ambient = environment.ambient.max(0.0);
                let light = ambient + (1.0 - ambient).max(0.0) * environment.sunlight;
                let color = color.powf(2.2) * light;
                clouds = Some(self.list.len() as u32);
                self.list.push(GpuSky {
                    axes: [[0.0; 4]; 3],
                    uv: [
                        x.rem_euclid(1.0) as f32,
                        z.rem_euclid(1.0) as f32,
                        x.floor().rem_euclid(4096.0) as f32,
                        z.floor().rem_euclid(4096.0) as f32,
                    ],
                    color: color
                        .extend(0.8 * environment.cloud_opacity * opacity)
                        .to_array(),
                    ids: [index, kind::CLOUDS, w, h],
                    extra: [CELL as f32, 4.0, low, 0.0],
                });
                continue;
            }
            if opacity <= 0.0 {
                continue;
            }
            let t = turn_of(s, d, time);
            let spin =
                if s.spin != 0.0 { Mat3::from_rotation_x((time / s.spin as f64 * std::f64::consts::TAU) as f32) } else { Mat3::IDENTITY };
            let m = frame_of(s, t) * spin;
            let picture = self.picture(gpu, &s.texture);
            let slot = self
                .slots
                .get(dim)
                .and_then(|l| l.get(i))
                .copied()
                .unwrap_or(0);
            let ticks = if s.frame_ticks != 0 {
                s.frame_ticks as u64
            } else if d.day_length != 0 {
                d.day_length as u64
            } else {
                24000
            };
            let frame_at = (time as u64 / ticks) % (s.frames[0] * s.frames[1]) as u64;
            let (cols, rows) = (s.frames[0] as f32, s.frames[1] as f32);
            let (u, v) = ((frame_at % s.frames[0] as u64) as f32 / cols, (frame_at / s.frames[0] as u64) as f32 / rows);
            let half = (s.size * 0.5).tan();
            let (kind_id, uv, vertices) = match s.kind {
                sky_kind::STARS => (kind::STARS, [half, s.seed as f32, 0.0, 0.0], 6 * s.count),
                sky_kind::DOME => {
                    let net = picture.is_some_and(|(_, [w, h])| w * 2 == h * 3);
                    (kind::DOME, [net as u32 as f32, 0.0, 0.0, 0.0], 3)
                }
                _ => (kind::BODY, [u, v, u + 1.0 / cols, v + 1.0 / rows], 6),
            };
            let at = self.list.len() as u32;
            self.list.push(GpuSky {
                axes: axes(m),
                uv,
                color: rgb(s.color).powf(2.2).extend(opacity).to_array(),
                ids: [picture.map_or(u32::MAX, |p| p.0), kind_id, slot, s.additive as u32],
                extra: [half, 0.0, 0.0, 0.0],
            });
            draws.push((vertices, at));
        }
        // Sunsets over the sky and the stars, under the bodies: right after the domes and stars.
        let first_body = draws
            .iter()
            .skip(1)
            .position(|&(_, at)| self.list[at as usize].ids[1] == kind::BODY)
            .map_or(draws.len(), |p| p + 1);
        let glowing: Vec<(Vec3, Vec3, f32, f32)> = glows(dim, d, time, weights).collect();
        for (n, (flat, color, strength, along)) in glowing.into_iter().enumerate() {
            let at = self.list.len() as u32;
            self.list.push(GpuSky {
                axes: axes(Mat3::from_cols(flat, Vec3::Y, flat.cross(Vec3::Y))),
                uv: [along, 0.0, 0.0, 0.0],
                color: color
                    .powf(2.2)
                    .extend(strength * environment.sky_opacity * (1.0 - environment.sky_cover))
                    .to_array(),
                ids: [u32::MAX, kind::GLOW, 0, 0],
                extra: [0.0; 4],
            });
            draws.insert(first_body + n, (3, at));
        }

        let over_from = self.list.len() as u32;
        let over = self.overlay_entries(gpu, overlays);
        let n = self.list.len() as u32;
        let stride = size_of::<GpuSky>() as u64;
        let buf = crate::gpu_layout::frame_buffer(gpu, &mut self.buffers, frame.slot, n, stride, usage::STORAGE, "sky");
        gpu.mapped(buf)[..self.list.len() * stride as usize].copy_from_slice(bytemuck::cast_slice(&self.list));
        self.overlay = (over > 0).then_some((buf, over_from, over));
        list.pipeline(pipe);
        list.bind(bind::sky::OBJECTS, buf);
        list.bind(bind::sky::SCENE, scene);
        for (vertices, first_instance) in draws {
            list.push(Cmd::Draw { vertices, instances: 1, first_vertex: 0, first_instance });
        }
        if let (Some(first_instance), Some(pipe)) = (clouds, self.clouds_pipe) {
            list.pipeline(pipe);
            list.bind(bind::sky::OBJECTS, buf);
            list.bind(bind::sky::SCENE, scene);
            list.push(Cmd::Draw { vertices: 3, instances: 1, first_vertex: 0, first_instance });
        }
    }
}
