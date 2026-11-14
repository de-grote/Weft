//! Sources contribute independently to one frame's perception; renderers and audio read only its result.

use super::textures::GameTextures;
use super::{Client, sky};
use crate::{
    components,
    content::{Content, content},
};
use glam::{DVec3, Vec3};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use weft_abi::{
    self as abi,
    environment::{EnvironmentLayer as Layer, EnvironmentOverlay as Overlay, EnvironmentValues as Values},
};

#[derive(Default)]
pub struct Environment {
    pub local: FxHashMap<String, Layer>,
    pub remote: FxHashMap<u64, Vec<Layer>>,
    pub static_entities: FxHashSet<u64>,
    cached: FxHashMap<(usize, u32), Arc<Vec<Layer>>>,
    active: FxHashMap<String, Active>,
    pending: FxHashMap<String, (Layer, f32)>,
    pub values: Values,
    pub overlays: Vec<sky::Overlay>,
    pub post: Vec<(String, f32)>,
    pub sky: FxHashMap<String, f32>,
}

struct Active {
    layer: Layer,
    weight: f32,
    seen: bool,
}

impl Environment {
    fn layers(&mut self, c: &Content, kind: usize, id: u32) -> Arc<Vec<Layer>> {
        self.cached
            .entry((kind, id))
            .or_insert_with(|| {
                Arc::new(
                    c.components
                        .id(abi::environment::COMPONENT)
                        .and_then(|comp| c.components.bytes(kind, id, comp))
                        .and_then(abi::data::from_bytes)
                        .unwrap_or_default(),
                )
            })
            .clone()
    }

    fn contribute(&mut self, key: String, layer: Layer, target: f32) {
        if target > 0.0 {
            self.pending.insert(key, (layer, target));
        }
    }
}

fn approach(value: f32, target: f32, seconds: f32, dt: f32) -> f32 {
    if seconds <= 0.0 { target } else { value + (target - value).clamp(-dt / seconds, dt / seconds) }
}

impl Client {
    fn environment_material(&self, mut layer: Layer, point: DVec3, state: Option<u32>) -> Layer {
        let c = content();
        let state = state.unwrap_or_else(|| {
            self.world
                .block_or_air(point.floor().as_ivec3())
                .unwrap_or(0)
        });
        if layer.fog_from_block {
            layer.values.fog_color = self.sky.block_color(state).to_array();
            layer.fog_from_block = false;
        }
        for overlay in &mut layer.overlays {
            if !(0..6).contains(&overlay.block_face) {
                continue;
            }
            let block = &c.states[state as usize];
            let look = super::looks::current();
            let tex = look.drawn(state, block).textures[overlay.block_face as usize];
            let tex = c.layers(tex).first().map_or(tex, |l| l.tex);
            overlay.texture = look.texture_name(c, tex).unwrap_or_default().to_string();
            overlay.block_face = -1;
            if overlay.lit && block.tint != 0 {
                let tint = sky::rgb(crate::content::item_tint(block.tint))
                    .powf(2.2)
                    .to_array();
                for (color, tint) in overlay.color[..3].iter_mut().zip(tint) {
                    *color *= tint;
                }
            }
        }
        layer
    }

    fn environment_source(&self, env: &mut Environment, key: String, layer: Layer, owner: u64, center: DVec3) {
        env.pending.remove(&key);
        let point = if layer.observer == abi::environment::observer::BODY {
            self.smooth_pos() + DVec3::Y * self.player.eye
        } else {
            self.render_camera().pos
        };
        if owner != 0 && owner != self.player_id && layer.radius <= 0.0 {
            return;
        }
        let center = if layer.located { DVec3::from_array(layer.center) } else { center };
        let weight = if layer.radius > 0.0 {
            let distance = point.distance(center) as f32;
            if layer.radius <= layer.inner_radius {
                (distance <= layer.radius) as u8 as f32
            } else {
                ((layer.radius - distance) / (layer.radius - layer.inner_radius)).clamp(0.0, 1.0)
            }
        } else {
            1.0
        };
        if weight <= 0.0 {
            return;
        }
        if !layer.when.is_empty() {
            let Some(condition) = content().condition(&layer.when) else { return };
            let state = self
                .world
                .block_or_air(point.floor().as_ivec3())
                .unwrap_or(0);
            let in_liquid =
                crate::physics::liquid_at(content(), &super::world::SeenCells { world: &self.world, air_if_missing: false }, point)
                    .is_some_and(|l| self.player.swims_in.get(l as usize) == Some(&true));
            let ask = crate::conditions::Ask {
                pos: point,
                state,
                entity: self.player_id,
                body: Some(crate::conditions::Body { in_liquid, flying: self.player.flying, sprinting: self.player.sprinting }),
                ..Default::default()
            };
            if !crate::conditions::client_holds(Some(self), &condition, &ask) {
                return;
            }
        }
        env.contribute(key, self.environment_material(layer, point, None), weight);
    }

    fn environment_layers(&self, env: &mut Environment, layers: &[Layer], prefix: &str, owner: u64, center: DVec3) {
        for layer in layers {
            self.environment_source(env, format!("{prefix}/{}", layer.name), layer.clone(), owner, center);
        }
    }

    pub fn update_environment(&mut self, textures: &GameTextures, dt: f32) {
        let c = content();
        let d = &c.dims[self.world.dim as usize];
        let cam = self.render_camera();
        let body = self.smooth_pos() + DVec3::Y * self.player.eye;
        let mut env = std::mem::take(&mut self.environment);
        self.environment.values = env.values;
        env.remote
            .retain(|id, _| *id == self.player_id || self.entities.contains_key(id) || self.showing_blocks.contains_key(id));
        env.static_entities
            .retain(|id| self.entities.contains_key(id) || self.showing_blocks.contains_key(id));
        for active in env.active.values_mut() {
            active.seen = false;
        }
        let day = d.daylight(self.world.time);
        let height = (d.day_turn(self.smooth_time()) * std::f64::consts::TAU).sin() as f32;
        let cloud_day = if d.sky_light { (height * 2.0 + 0.5).clamp(0.0, 1.0) } else { 1.0 };
        let ambient_level = d.ambient as f32 / 15.0;
        let ambient = (ambient_level / (4.0 - 3.0 * ambient_level)).max(0.04);
        let view_distance = (self.view_radius * 16).max(super::lod::Lod::distance()) as f32;
        let cloud_distance = c
            .game
            .config_names
            .get("core:cloud_distance")
            .map_or(0.0, |&id| crate::config::get(id)) as f32;
        let fog_day = self.daylight();
        let fog = sky::rgb(d.fog_color) * Vec3::new(fog_day * 0.94 + 0.06, fog_day * 0.94 + 0.06, fog_day * 0.91 + 0.09);
        let mut values = Values {
            fog_color: fog.powf(2.2).to_array(),
            sky_color: (sky::rgb(d.sky_color) * if d.sky_light { day } else { 1.0 })
                .powf(2.2)
                .to_array(),
            cloud_color: Vec3::new(cloud_day * 0.9 + 0.1, cloud_day * 0.9 + 0.1, cloud_day * 0.85 + 0.15)
                .powf(2.2)
                .to_array(),
            fog_start: view_distance * 0.6,
            fog_end: view_distance,
            daylight: day,
            ambient,
            sunlight: 1.0,
            sky_opacity: 1.0,
            cloud_opacity: 1.0,
            cloud_fog_start: cloud_distance * 0.5,
            cloud_fog_end: cloud_distance,
            lowpass: 20_000.0,
            volume: 1.0,
            night_vision: self.night_vision,
            ..Default::default()
        };
        let dim_layers = env.layers(c, components::DIMENSION, self.world.dim);
        self.environment_layers(&mut env, &dim_layers, "dimension", 0, cam.pos);

        let mut weather = Layer::new("core:weather".into());
        weather.priority = -900;
        weather.values.daylight =
            if self.lightning > 0.0 { (day * (1.0 - self.look.darkness) + 0.5).min(1.0) } else { day * (1.0 - self.look.darkness) };
        weather.values.sky_gray = self.look.sky_gray;
        weather.values.fog_gray = self.look.fog_gray;
        weather.values.sky_cover = self.look.sky_cover;
        weather.values.cloud_gray = self.look.cloud_gray;
        weather.values.cloud_dark = self.look.cloud_dark;
        env.contribute("weather".into(), weather, 1.0);
        let weather_layers = env.layers(c, abi::entry_kind::WEATHER as usize, self.weather_id as u32);
        self.environment_layers(&mut env, &weather_layers, "weather-data", 0, cam.pos);

        let mut cave = Layer::new("core:enclosed".into());
        cave.priority = -800;
        cave.values.reverb = 10.0f32.powf(-9.0 / 20.0);
        if crate::world::light::enclosed(d, self.world.light(cam.pos.floor().as_ivec3())) {
            env.contribute("enclosed".into(), cave, 1.0);
        }

        for (point, observer) in [(cam.pos, abi::environment::observer::CAMERA), (body, abi::environment::observer::BODY)] {
            let block = self
                .world
                .block_or_air(point.floor().as_ivec3())
                .unwrap_or(0);
            if let Some(biome) = self.world.biome_at(point.floor().as_ivec3()) {
                let layers = env.layers(c, abi::entry_kind::BIOME as usize, biome as u32);
                for layer in layers.iter().filter(|l| l.observer == observer) {
                    self.environment_source(&mut env, format!("biome/{observer}/{biome}/{}", layer.name), layer.clone(), 0, point);
                }
            }
            let block_layers = env.layers(c, components::BLOCK, block);
            for layer in block_layers.iter().filter(|l| l.observer == observer) {
                self.environment_source(&mut env, format!("block/{observer}/{block}/{}", layer.name), layer.clone(), 0, point);
            }
            if let Some(liquid) =
                crate::physics::liquid_at(c, &super::world::SeenCells { world: &self.world, air_if_missing: false }, point)
            {
                for layer in c.liquids[liquid as usize]
                    .environment
                    .iter()
                    .filter(|l| l.observer == observer)
                {
                    let layer = self.environment_material(layer.clone(), point, Some(c.liquids[liquid as usize].first_state));
                    self.environment_source(&mut env, format!("liquid/{observer}/{liquid}/{}", layer.name), layer, 0, point);
                }
            }
        }
        if !self.player.noclip {
            if let Some(block) = self
                .world
                .block_or_air(cam.pos.floor().as_ivec3())
                .filter(|&s| c.solid(s) && c.tight[s as usize].opacity >= 15)
            {
                let mut inside = Layer::new("core:inside_block".into());
                inside.priority = -600;
                inside
                    .overlays
                    .push(Overlay { color: [0.01, 0.01, 0.01, 1.0], tiles: 1.0, block_face: 1, ..Default::default() });
                inside = self.environment_material(inside, cam.pos, Some(block));
                self.environment_source(&mut env, format!("inside/{block}"), inside, 0, cam.pos);
            }
        }
        if self.me.burning {
            let mut fire = Layer::new("core:burning".into());
            fire.priority = -500;
            let ticks = self.start.elapsed().as_secs_f32() * crate::world::TPS as f32;
            textures.texture(c.fire_texture);
            fire.overlays.push(Overlay {
                texture: c.textures[c.fire_texture as usize].clone(),
                color: [1.0, 1.0, 1.0, 0.9],
                tiles: 2.0,
                lower: true,
                frame: textures.strip_frame(c.fire_texture, ticks),
                block_face: -1,
                lit: false,
            });
            env.contribute("burning".into(), fire, 1.0);
        }
        let kind_layers = env.layers(c, components::ENTITY, self.me.kind);
        self.environment_layers(&mut env, &kind_layers, &format!("entity/{}", self.player_id), self.player_id, body);
        let static_entities = std::mem::take(&mut env.static_entities);
        for &owner in &static_entities {
            if owner == self.player_id {
                continue;
            }
            let Some(e) = self
                .entities
                .get(&owner)
                .or_else(|| self.showing_blocks.get(&owner))
            else {
                continue;
            };
            let layers = env.layers(c, components::ENTITY, e.kind);
            self.environment_layers(&mut env, &layers, &format!("entity/{owner}"), owner, e.pos);
        }
        env.static_entities = static_entities;
        for effect in self.me.effects.iter().filter(|f| f.until > self.ticks()) {
            let layers = env.layers(c, abi::entry_kind::ENTITY_EFFECT as usize, effect.id);
            self.environment_layers(&mut env, &layers, &format!("effect/{}", effect.id), self.player_id, body);
        }
        for (slot, stack) in self
            .me
            .worn
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.is_empty())
        {
            let layers = env.layers(c, components::ITEM, stack.item);
            self.environment_layers(&mut env, &layers, &format!("equipment/{slot}"), self.player_id, body);
        }
        let remote = std::mem::take(&mut env.remote);
        for (&owner, layers) in &remote {
            let center = if owner == self.player_id {
                body
            } else {
                let Some(e) = self
                    .entities
                    .get(&owner)
                    .or_else(|| self.showing_blocks.get(&owner))
                else {
                    continue;
                };
                e.pos
            };
            self.environment_layers(&mut env, layers, &format!("entity/{owner}"), owner, center);
        }
        env.remote = remote;
        let local = std::mem::take(&mut env.local);
        for (name, layer) in &local {
            self.environment_source(&mut env, format!("local/{name}"), layer.clone(), 0, cam.pos);
        }
        env.local = local;
        for (key, (layer, target)) in env.pending.drain() {
            let active = env
                .active
                .entry(key)
                .or_insert_with(|| Active { layer: layer.clone(), weight: 0.0, seen: true });
            active.seen = true;
            active.layer = layer;
            let seconds = if target >= active.weight { active.layer.fade_in } else { active.layer.fade_out };
            active.weight = approach(active.weight, target, seconds, dt);
        }
        for active in env.active.values_mut().filter(|a| !a.seen) {
            active.weight = approach(active.weight, 0.0, active.layer.fade_out, dt);
        }
        env.active.retain(|_, a| a.weight > 0.0);
        let mut ordered: Vec<_> = env.active.iter().collect();
        ordered.sort_by(|a, b| (a.1.layer.priority, a.0).cmp(&(b.1.layer.priority, b.0)));
        let mut overlays: Vec<sky::Overlay> = Vec::new();
        let mut post: Vec<(String, f32)> = Vec::new();
        let mut sky_weights = FxHashMap::default();
        for (_, active) in &ordered {
            for object in &active.layer.sky {
                let old = sky_weights.entry(object.name.clone()).or_insert(1.0);
                *old = abi::environment::mix_value(*old, object.weight, active.layer.blend, active.weight);
            }
        }
        values.fog_color = sky::warmed_by_glows(self.world.dim as usize, d, self.smooth_time(), fog, cam.forward(), &sky_weights)
            .powf(2.2)
            .to_array();
        for (_, active) in &ordered {
            values.mix(&active.layer.values, active.layer.blend, active.weight);
        }
        for (_, active) in ordered {
            let (layer, weight) = (&active.layer, active.weight);
            if layer.replace_overlays {
                for o in &mut overlays {
                    o.color[3] *= 1.0 - weight;
                }
            }
            if layer.replace_post {
                for (_, strength) in &mut post {
                    *strength *= 1.0 - weight;
                }
            }
            for effect in &layer.post {
                if let Some((_, strength)) = post.iter_mut().find(|e| e.0 == effect.name) {
                    *strength = abi::environment::mix_value(*strength, effect.weight, layer.blend, weight);
                } else {
                    post.push((effect.name.clone(), abi::environment::mix_value(0.0, effect.weight, layer.blend, weight)));
                }
            }
            for overlay in &layer.overlays {
                let mut color = overlay.color;
                color[3] *= weight;
                if overlay.lit {
                    for v in &mut color[..3] {
                        *v *= values.daylight.max(0.25).powf(2.2);
                    }
                }
                let picture = overlay.texture.clone();
                if !picture.is_empty() {
                    overlays.push(sky::Overlay { picture, color, tiles: overlay.tiles, lower: overlay.lower, frame: overlay.frame });
                }
            }
        }
        values.sunlight = values.sunlight.max(0.0);
        values.fog_end = values.fog_end.max(values.fog_start + 0.001);
        values.cloud_fog_end = values.cloud_fog_end.max(0.0);
        values.cloud_fog_start = values
            .cloud_fog_start
            .clamp(0.0, (values.cloud_fog_end - 0.001).max(0.0));
        values.lowpass = values.lowpass.clamp(20.0, 20_000.0);
        values.reverb = values.reverb.clamp(0.0, 1.0);
        values.volume = values.volume.max(0.0);
        env.values = values;
        env.overlays = overlays;
        post.retain(|(_, weight)| weight.is_finite() && *weight > 0.0);
        env.post = post;
        env.sky = sky_weights;
        self.environment = env;
    }
}
