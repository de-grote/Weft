//! Independent environment contributions, shared by content and live client/server sources.

use crate::{Client, Component, WithComponents, World, abi, host};
pub use abi::environment::{EnvironmentOverlay, EnvironmentValues, EnvironmentWeight, blend, observer};

pub static ENVIRONMENT: Component<Vec<abi::environment::EnvironmentLayer>> = Component::named(abi::environment::COMPONENT);

/// A named contribution. Public fields are available through Deref for arbitrary mod code.
#[derive(Clone)]
pub struct Environment(pub abi::environment::EnvironmentLayer);

impl std::ops::Deref for Environment {
    type Target = abi::environment::EnvironmentLayer;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for Environment {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Environment {
    /// Starts an empty contribution; only explicitly selected fields participate.
    pub fn new(name: &str) -> Self {
        Self(abi::environment::EnvironmentLayer::new(crate::full_name(name)))
    }
    /// Higher priorities apply later. Equal priorities are ordered by source identity.
    pub fn priority(mut self, priority: i32) -> Self {
        self.0.priority = priority;
        self
    }
    /// Activates while this condition holds at the chosen observation point.
    pub fn when(mut self, condition: &str) -> Self {
        self.0.when = condition.to_string();
        self
    }
    /// One of REPLACE, ADD, MULTIPLY or MAX, also used for named shader/object weights.
    pub fn blend(mut self, mode: u32) -> Self {
        self.0.blend = mode;
        self
    }
    /// Real seconds to reach full weight, or to disappear after removal/condition failure.
    pub fn fade(mut self, enter: f32, leave: f32) -> Self {
        self.0.fade_in = enter.max(0.0);
        self.0.fade_out = leave.max(0.0);
        self
    }
    /// CAMERA by default; BODY samples the local player's eyes, even with a detached camera.
    pub fn observer(mut self, point: u32) -> Self {
        self.0.observer = point;
        self
    }
    /// An entity-centered zone, full inside inner and zero outside outer, linear between.
    pub fn radius(mut self, inner: f32, outer: f32) -> Self {
        self.0.inner_radius = inner.max(0.0);
        self.0.radius = outer.max(inner).max(0.0);
        self
    }
    /// A fixed zone instead of following the entity that owns it.
    pub fn around(mut self, center: [f64; 3], inner: f32, outer: f32) -> Self {
        self.0.center = center;
        self.0.located = true;
        self.radius(inner, outer)
    }
    /// Absolute fog color (0xRRGGBB) and explicit start/end distances in blocks.
    pub fn fog(mut self, rgb: u32, start: f32, end: f32) -> Self {
        self.values.fog_color = color(rgb);
        self.values.fog_start = start;
        self.values.fog_end = end;
        self.values.fog_gray = 0.0;
        self
    }
    /// Sky illumination, cloud lighting and directional sunlight, independent of sky-object visibility.
    pub fn sunlight(mut self, strength: f32) -> Self {
        self.values.sunlight = strength;
        self
    }
    /// Absolute sky color (0xRRGGBB), independent of illumination.
    pub fn sky_color(mut self, rgb: u32) -> Self {
        self.values.sky_color = color(rgb);
        self.values.sky_gray = 0.0;
        self
    }
    /// Cloud tint (0xRRGGBB) before lighting, replacing the time/weather tint.
    pub fn cloud_color(mut self, rgb: u32) -> Self {
        self.values.cloud_color = color(rgb);
        self.values.cloud_gray = 0.0;
        self.values.cloud_dark = 0.0;
        self
    }
    /// Cloud fog start/end in blocks from the camera, independent of world fog. Zero end hides clouds.
    pub fn cloud_fog(mut self, start: f32, end: f32) -> Self {
        self.values.cloud_fog_start = start;
        self.values.cloud_fog_end = end;
        self
    }
    /// Visibility of the complete sky, including clouds.
    pub fn sky_opacity(mut self, opacity: f32) -> Self {
        self.values.sky_opacity = opacity;
        self
    }
    /// Visibility of a dimension-qualified object (`DIM.sky(&SUN)`), including sunset glow.
    pub fn sky_object(mut self, object: &crate::SkyObject, opacity: f32) -> Self {
        self.sky
            .push(EnvironmentWeight { name: object.name().to_string(), weight: opacity });
        self
    }
    /// World low-pass cutoff in Hz, linear reverb send and linear volume gain.
    pub fn acoustics(mut self, cutoff_hz: f32, reverb: f32, volume: f32) -> Self {
        self.values.lowpass = cutoff_hz;
        self.values.reverb = reverb;
        self.values.volume = volume;
        self
    }
    /// Appends a screen texture; relative paths start at this mod's textures directory, without `.png`.
    pub fn overlay(mut self, texture: &str, opacity: f32) -> Self {
        self.overlays.push(EnvironmentOverlay {
            texture: crate::asset("", texture),
            color: [1.0, 1.0, 1.0, opacity],
            tiles: 1.0,
            lower: false,
            frame: 0,
            block_face: -1,
            lit: false,
        });
        self
    }
    /// Adds a named post shader at this strength.
    pub fn post(mut self, shader: &crate::Shader, strength: f32) -> Self {
        self.post
            .push(EnvironmentWeight { name: shader.name().to_string(), weight: strength });
        self
    }
    /// Crossfades out preceding overlays, even when this layer adds none.
    pub fn replace_overlays(mut self) -> Self {
        self.0.replace_overlays = true;
        self
    }
    pub fn replace_post(mut self) -> Self {
        self.0.replace_post = true;
        self
    }
}

fn color(rgb: u32) -> [f32; 3] {
    [(rgb >> 16 & 255) as f32, (rgb >> 8 & 255) as f32, (rgb & 255) as f32].map(|v| (v / 255.0).powf(2.2))
}

impl Client {
    /// Adds or replaces a persistent local source. Repeated calls replace its target without restarting its fade.
    pub fn environment(&self, layer: &Environment) -> bool {
        let data = abi::data::bytes_of(&layer.0);
        unsafe { (host().client_environment_set)(self.raw(), abi::Str::new(&layer.name), abi::Slice::new(&data)) != 0 }
    }
    /// Removes a local source, retaining its configured fade-out.
    pub fn clear_environment(&self, name: &str) {
        unsafe { (host().client_environment_set)(self.raw(), abi::Str::new(&crate::full_name(name)), abi::Slice::EMPTY) };
    }
    /// The final parameters from the last environment evaluation.
    pub fn environment_values(&self) -> EnvironmentValues {
        let mut values = EnvironmentValues::default();
        unsafe { (host().client_environment_values)(self.raw(), &mut values) };
        values
    }
}

impl World {
    /// A public, saved source owned by an entity. Radius zero affects only its own observer;
    /// a radius affects nearby observers. Uses ordinary component synchronization.
    pub fn environment(&self, entity: u64, layer: &Environment) -> bool {
        let data = abi::data::bytes_of(&layer.0);
        unsafe { (host().environment_set)(self.raw(), entity, abi::Str::new(&layer.name), abi::Slice::new(&data)) != 0 }
    }
    /// Removes a synchronized source, retaining its fade-out on each client.
    pub fn clear_environment(&self, entity: u64, name: &str) {
        unsafe { (host().environment_set)(self.raw(), entity, abi::Str::new(&crate::full_name(name)), abi::Slice::EMPTY) };
    }
}

pub(crate) fn attach<T: WithComponents>(builder: T, layer: Environment) -> T {
    builder.push(&ENVIRONMENT, layer.0)
}
