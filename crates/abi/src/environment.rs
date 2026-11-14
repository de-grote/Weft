//! Shared parameters and contributions to client perception. NaN fields leave a parameter alone.

use alloc::{string::String, vec::Vec};

pub const COMPONENT: &str = "environment";

pub mod blend {
    pub const REPLACE: u32 = 0;
    pub const ADD: u32 = 1;
    pub const MULTIPLY: u32 = 2;
    pub const MAX: u32 = 3;
}

pub mod observer {
    pub const CAMERA: u32 = 0;
    pub const BODY: u32 = 1;
}

pub fn mix_value(current: f32, value: f32, mode: u32, weight: f32) -> f32 {
    match mode {
        blend::ADD => current + value * weight,
        blend::MULTIPLY => current * (1.0 + (value - 1.0) * weight),
        blend::MAX => current + (current.max(value) - current) * weight,
        _ => current + (value - current) * weight,
    }
}

crate::data! {
    #[repr(C)]
    #[derive(Copy)]
    pub struct EnvironmentValues {
        /// Linear RGB, independent of illumination.
        pub fog_color: [f32; 3],
        pub sky_color: [f32; 3],
        /// Linear cloud tint before illumination and the sky object's own color.
        pub cloud_color: [f32; 3],
        pub fog_start: f32,
        pub fog_end: f32,
        pub daylight: f32,
        /// Linear ambient illumination, zero removes the native minimum light as well.
        pub ambient: f32,
        /// Multiplies sky illumination, including its night floor, and directional sunlight.
        pub sunlight: f32,
        pub sky_opacity: f32,
        pub sky_cover: f32,
        pub sky_gray: f32,
        pub fog_gray: f32,
        pub cloud_gray: f32,
        pub cloud_dark: f32,
        pub cloud_opacity: f32,
        /// Separate cloud fog distances from the camera in blocks; zero end hides clouds.
        pub cloud_fog_start: f32,
        pub cloud_fog_end: f32,
        /// World sound low-pass cutoff in Hz, linear reverb send and linear gain.
        pub lowpass: f32,
        pub reverb: f32,
        pub volume: f32,
        pub night_vision: f32,
    }
}

impl EnvironmentValues {
    pub fn unset() -> Self {
        Self {
            fog_color: [f32::NAN; 3],
            sky_color: [f32::NAN; 3],
            cloud_color: [f32::NAN; 3],
            fog_start: f32::NAN,
            fog_end: f32::NAN,
            daylight: f32::NAN,
            ambient: f32::NAN,
            sunlight: f32::NAN,
            sky_opacity: f32::NAN,
            sky_cover: f32::NAN,
            sky_gray: f32::NAN,
            fog_gray: f32::NAN,
            cloud_gray: f32::NAN,
            cloud_dark: f32::NAN,
            cloud_opacity: f32::NAN,
            cloud_fog_start: f32::NAN,
            cloud_fog_end: f32::NAN,
            lowpass: f32::NAN,
            reverb: f32::NAN,
            volume: f32::NAN,
            night_vision: f32::NAN,
        }
    }

    pub fn mix(&mut self, from: &Self, mode: u32, weight: f32) {
        let mix = |to: &mut f32, value: f32| {
            if !value.is_finite() {
                return;
            }
            *to = mix_value(*to, value, mode, weight);
        };
        for (to, value) in self.fog_color.iter_mut().zip(from.fog_color) {
            mix(to, value);
        }
        for (to, value) in self.sky_color.iter_mut().zip(from.sky_color) {
            mix(to, value);
        }
        for (to, value) in self.cloud_color.iter_mut().zip(from.cloud_color) {
            mix(to, value);
        }
        macro_rules! fields { ($($field:ident),*) => { $(mix(&mut self.$field, from.$field);)* }; }
        fields!(
            fog_start,
            fog_end,
            daylight,
            ambient,
            sunlight,
            sky_opacity,
            sky_cover,
            sky_gray,
            fog_gray,
            cloud_gray,
            cloud_dark,
            cloud_opacity,
            cloud_fog_start,
            cloud_fog_end,
            lowpass,
            reverb,
            volume,
            night_vision
        );
    }
}

crate::data! {
    pub struct EnvironmentOverlay {
        pub texture: String,
        pub color: [f32; 4],
        pub tiles: f32,
        pub lower: bool,
        pub frame: u32,
        /// -1 uses texture; 0..5 uses the sampled block's face and its resource-pack texture.
        pub block_face: i32,
        pub lit: bool,
    }
}

crate::data! {
    pub struct EnvironmentWeight { pub name: String, pub weight: f32 }
}

crate::data! {
    pub struct EnvironmentLayer {
        pub name: String,
        pub priority: i32,
        pub blend: u32,
        pub fade_in: f32,
        pub fade_out: f32,
        pub observer: u32,
        pub when: String,
        /// Entity source radius in blocks. Zero affects only that entity's observer.
        pub radius: f32,
        pub inner_radius: f32,
        pub center: [f64; 3],
        pub located: bool,
        pub values: EnvironmentValues,
        pub overlays: Vec<EnvironmentOverlay>,
        pub post: Vec<EnvironmentWeight>,
        /// Full SkyObject names and their independent visibility multipliers.
        pub sky: Vec<EnvironmentWeight>,
        pub replace_overlays: bool,
        pub replace_post: bool,
        pub fog_from_block: bool,
    }
}

impl EnvironmentLayer {
    pub fn new(name: String) -> Self {
        Self { name, values: EnvironmentValues::unset(), ..Self::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_parameters_and_blend_endpoints() {
        let base = EnvironmentValues {
            fog_start: -8.0,
            fog_end: 80.0,
            cloud_fog_end: 80.0,
            sunlight: 1.0,
            lowpass: 20_000.0,
            ..Default::default()
        };
        for mode in [blend::REPLACE, blend::ADD, blend::MULTIPLY, blend::MAX] {
            let mut patch = EnvironmentValues::unset();
            patch.fog_end = 4.0;
            patch.cloud_fog_end = 4.0;
            let mut result = base;
            result.mix(&patch, mode, 0.0);
            assert_eq!(result, base);
            result.mix(&patch, mode, 1.0);
            let expected = match mode {
                blend::ADD => 84.0,
                blend::MULTIPLY => 320.0,
                blend::MAX => 80.0,
                _ => 4.0,
            };
            assert_eq!(result.fog_end, expected);
            assert_eq!(result.cloud_fog_end, expected);
            assert_eq!(result.fog_start, base.fog_start);
            assert_eq!(result.sunlight, base.sunlight);
            assert_eq!(result.lowpass, base.lowpass);
        }
    }

    #[test]
    fn higher_replacement_can_suppress_a_named_effect() {
        assert_eq!(mix_value(-8.0, -2.0, blend::MAX, 0.0), -8.0);
        assert_eq!(mix_value(-8.0, -2.0, blend::MAX, 0.5), -5.0);
        let strongest = mix_value(1.0, 0.25, blend::MAX, 1.0);
        assert_eq!(strongest, 1.0);
        assert_eq!(mix_value(strongest, 0.0, blend::REPLACE, 1.0), 0.0);
        assert_eq!(mix_value(strongest, 0.0, blend::REPLACE, 0.25), 0.75);
    }
}
