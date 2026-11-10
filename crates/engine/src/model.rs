//! Entity models and keyframe animations from files, in the Bedrock formats Blockbench writes
//! (`models/<name>.geo.json`, `animations/<name>.animation.json`), and posing a model's bones.
//!
//! Coordinates: model pixels, y up from the feet, the model looks along -Z, positions as in the
//! files. Rotations in the files are degrees about x, y, z; the engine applies x first, then y,
//! then z (Java ModelPart order) and flips the sign of x and z, which is how Blockbench maps
//! Bedrock angles onto a y-up frame (a Bedrock arm raised with x = -90 points forward).

use std::sync::Arc;

use glam::{EulerRot, Mat4, Quat, Vec3};
use serde_json::Value;
use weft_abi::MeshVertex;

use crate::registry::{Cuboid, Part};

fn num(v: &Value) -> f32 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0) as f32,
        // Molang expressions are not run; a plain number in a string still is one.
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn vec3(v: Option<&Value>) -> Option<[f32; 3]> {
    match v? {
        Value::Array(a) if a.len() >= 3 => Some([num(&a[0]), num(&a[1]), num(&a[2])]),
        n @ (Value::Number(_) | Value::String(_)) => {
            let x = num(n);
            Some([x; 3])
        }
        _ => None,
    }
}

fn vec2(v: Option<&Value>) -> Option<[f32; 2]> {
    match v? {
        Value::Array(a) if a.len() >= 2 => Some([num(&a[0]), num(&a[1])]),
        _ => None,
    }
}

/// Degrees from a file to the engine's radians (see the module docs).
fn angles(r: [f32; 3]) -> [f32; 3] {
    [-r[0].to_radians(), r[1].to_radians(), -r[2].to_radians()]
}

/// What a bone is, worked out from its name (`role_of`) so code models, model files and
/// animations agree: which stock clips a model gets, what armor covers, where a hand is.
pub mod role {
    pub const STATIC: u32 = 0;
    pub const HEAD: u32 = 1;
    pub const ARM_LEFT: u32 = 2;
    pub const ARM_RIGHT: u32 = 3;
    pub const LEG_LEFT: u32 = 4;
    pub const LEG_RIGHT: u32 = 5;
    /// Quadrupeds' front legs.
    pub const LEG_FRONT_LEFT: u32 = 6;
    pub const LEG_FRONT_RIGHT: u32 = 7;
    /// More than four legs: a spider's.
    pub const LEG_SWEEP: u32 = 10;
    /// Grows out of the head (ears, a hat): no armor layer, gone with the head in the first person.
    pub const ON_HEAD: u32 = 11;
}

/// The role of a bone of this name (Bedrock's names, as Blockbench writes them). `legs`: how many
/// `leg<n>` bones the model has; more than four are a spider's.
pub fn role_of(name: &str, legs: usize) -> u32 {
    let n = name.to_ascii_lowercase().replace('_', "");
    if legs > 4
        && n.strip_prefix("leg")
            .is_some_and(|i| i.parse::<u32>().is_ok())
    {
        return role::LEG_SWEEP;
    }
    if let Some(arm) = arm_role(&n) {
        return arm;
    }
    match n.as_str() {
        "head" => role::HEAD,
        "rightleg" | "leg1" => role::LEG_LEFT,
        "leftleg" | "leg0" => role::LEG_RIGHT,
        "leg2" => role::LEG_FRONT_RIGHT,
        "leg3" => role::LEG_FRONT_LEFT,
        _ => role::STATIC,
    }
}

/// The arm a name means: `mainArm` holds the item, and is the bone Bedrock files call `leftArm`,
/// at +x. `n` in lower case.
fn arm_role(n: &str) -> Option<u32> {
    match n {
        "mainarm" | "leftarm" => Some(role::ARM_RIGHT),
        "offarm" | "rightarm" => Some(role::ARM_LEFT),
        _ => None,
    }
}

/// Every part's role from its name.
pub fn assign_roles(parts: &mut [Part]) {
    let legs = parts
        .iter()
        .filter(|p| {
            p.name
                .to_ascii_lowercase()
                .strip_prefix("leg")
                .is_some_and(|i| i.parse::<u32>().is_ok())
        })
        .count();
    for p in parts.iter_mut() {
        p.role = role_of(&p.name, legs);
    }
    for i in 0..parts.len() {
        let above = parts[i].parent.and_then(|q| parts.get(q as usize));
        if parts[i].role == role::STATIC && above.is_some_and(|a| a.role == role::HEAD || a.role == role::ON_HEAD) {
            parts[i].role = role::ON_HEAD;
        }
    }
}

/// The bone of this name, however its case and underscores go.
pub fn bone_named(parts: &[Part], name: &str) -> Option<usize> {
    fn plain(s: &str) -> impl Iterator<Item = u8> + '_ {
        s.bytes()
            .filter(|c| *c != b'_')
            .map(|c| c.to_ascii_lowercase())
    }
    parts.iter().position(|p| plain(&p.name).eq(plain(name)))
}

/// A corner of each box of a bone and each point of its triangles, in the model's pixels.
pub fn part_points(p: &Part, mut each: impl FnMut(Vec3)) {
    let pivot = Vec3::from_array(p.pivot);
    for c in &p.cuboids {
        let (at, s) = (pivot + Vec3::from_array(c.origin), Vec3::from_array(c.size));
        for i in 0..8 {
            each(at + s * Vec3::new((i & 1) as f32, (i >> 1 & 1) as f32, (i >> 2 & 1) as f32));
        }
    }
    for v in &p.mesh {
        each(Vec3::from_array(v.pos));
    }
}

/// The empty bone a held item goes on, if the model has one: `mainItem` or `offItem`, or Bedrock's
/// names, which go with the arms of those names (`arm_role`).
pub fn item_bone(parts: &[Part], off: bool) -> Option<usize> {
    let names = if off { ["offitem", "rightitem"] } else { ["mainitem", "leftitem"] };
    names.into_iter().find_map(|n| bone_named(parts, n))
}

/// How a bone moves by itself, on top of what plays on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoneMotion {
    /// Pull toward hanging straight down.
    pub gravity: f32,
    /// Pull back to where the clips have it.
    pub spring: f32,
    /// How fast the swing dies down.
    pub drag: f32,
}

impl BoneMotion {
    /// What a bone called `hanging` does: hangs and swings.
    pub const HANGING: BoneMotion = BoneMotion { gravity: 40.0, spring: 0.0, drag: 2.5 };
}

/// What a bone named so does without being told: `hanging` and `hanging_<anything>`.
pub fn motion_by_name(name: &str) -> Option<BoneMotion> {
    let n = name.to_ascii_lowercase();
    n.starts_with("hanging").then_some(BoneMotion::HANGING)
}

/// A sidecar `models/<name>.bones`, lines `bone = gravity spring drag`, for what a file alone
/// has to say (an avatar has no code to say it with).
pub fn load_bones(src: &str) -> Vec<(String, BoneMotion)> {
    crate::kv::pairs(src)
        .filter_map(|(line, bone, v)| {
            let n: Vec<f32> = v.split_whitespace().map_while(|x| x.parse().ok()).collect();
            if v.split_whitespace().count() != 3 || n.len() != 3 {
                log::warn!("bones file, line {}: {bone} wants gravity, spring and drag, has `{v}`", line + 1);
                return None;
            }
            Some((bone.to_string(), BoneMotion { gravity: n[0], spring: n[1], drag: n[2] }))
        })
        .collect()
}

/// The role bits (`1 << role`) of those of `names` that have a role (`head`, `leftArm`, `leg2`):
/// what a worn item covers on any model; the other names cover their bone only.
pub fn roles_of(names: &[String]) -> u32 {
    names
        .iter()
        .map(|n| role_of(n, 4))
        .filter(|&r| r != role::STATIC)
        .fold(0, |m, r| m | 1 << r)
}

/// A model file's parts and the texture size it was made for.
pub struct Geo {
    pub parts: Vec<Part>,
    pub texture: Option<[u32; 2]>,
}

/// The first geometry of a `.geo.json` (format 1.12+, or the old 1.8 `geometry.name` keys) as
/// model parts: bones with their boxes and poly meshes.
pub fn load_geo(src: &str) -> Result<Geo, String> {
    let root: Value = serde_json::from_str(src).map_err(|e| e.to_string())?;
    let geo = match root.get("minecraft:geometry") {
        Some(Value::Array(list)) => list.first().cloned(),
        _ => root.as_object().and_then(|o| {
            o.iter()
                .find(|(k, _)| k.starts_with("geometry."))
                .map(|(_, v)| v.clone())
        }),
    }
    .ok_or("no geometry in the file")?;
    let bones = geo
        .get("bones")
        .and_then(Value::as_array)
        .ok_or("geometry without bones")?;
    let desc = geo.get("description").unwrap_or(&geo);
    let size = |k: &str| desc.get(k).and_then(Value::as_u64).map(|v| v as u32);
    let texture = size("texture_width")
        .zip(size("texture_height"))
        .map(|(w, h)| [w, h]);
    let tex = texture.map_or([64.0, 64.0], |t| [t[0] as f32, t[1] as f32]);
    let names: Vec<String> = bones
        .iter()
        .map(|b| {
            b.get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect();
    let mut parts = Vec::new();
    for b in bones {
        let name = b
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let parent = b
            .get("parent")
            .and_then(Value::as_str)
            .and_then(|p| names.iter().position(|n| n == p))
            .map(|i| i as u32);
        let pivot = vec3(b.get("pivot")).unwrap_or([0.0; 3]);
        let rest = angles(vec3(b.get("rotation")).unwrap_or([0.0; 3]));
        let bone_mirror = b.get("mirror").and_then(Value::as_bool).unwrap_or(false);
        let bone_inflate = b.get("inflate").map(num).unwrap_or(0.0);
        let mut cuboids = Vec::new();
        let mut mesh = Vec::new();
        for c in b
            .get("cubes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let origin = vec3(c.get("origin")).unwrap_or([0.0; 3]);
            let size = vec3(c.get("size")).unwrap_or([1.0; 3]);
            let inflate = c.get("inflate").map(num).unwrap_or(bone_inflate);
            let mirror = c
                .get("mirror")
                .and_then(Value::as_bool)
                .unwrap_or(bone_mirror);
            let rel = [origin[0] - pivot[0], origin[1] - pivot[1], origin[2] - pivot[2]];
            let mut cub = Cuboid { origin: rel, size, uv: [0.0; 2], inflate, mirror, faces: None };
            match c.get("uv") {
                Some(Value::Object(faces)) => {
                    // Per face; Blockbench writes "down" flipped in v compared with the box unwrap.
                    let mut rects = [[0.0f32; 4]; 6];
                    for (i, face) in ["east", "west", "down", "up", "north", "south"]
                        .into_iter()
                        .enumerate()
                    {
                        let Some(f) = faces.get(face) else { continue };
                        let uv = vec2(f.get("uv")).unwrap_or([0.0; 2]);
                        let sz = vec2(f.get("uv_size")).unwrap_or([0.0; 2]);
                        rects[i] = if face == "down" { [uv[0], uv[1] + sz[1], sz[0], -sz[1]] } else { [uv[0], uv[1], sz[0], sz[1]] };
                    }
                    cub.faces = Some(rects);
                }
                v => cub.uv = vec2(v).unwrap_or([0.0; 2]),
            }
            if vec3(c.get("rotation")).is_some_and(|r| r != [0.0; 3]) {
                log::warn!("bone {name}: rotated cubes are drawn unrotated; put them in a rotated bone");
            }
            cuboids.push(cub);
        }
        if let Some(pm) = b.get("poly_mesh") {
            mesh = poly_mesh(pm, tex);
        }
        parts.push(Part { role: 0, name, parent, pivot, rest, cuboids, mesh });
    }
    assign_roles(&mut parts);
    Ok(Geo { parts, texture })
}

/// `parts` as a `.geo.json` (format 1.12) `load_geo` reads back the same, for a texture `texture`
/// texels big: what Blockbench opens, to go on from a model made in code.
pub fn save_geo(name: &str, parts: &[Part], texture: [u32; 2]) -> String {
    use serde_json::json;
    let deg = |r: [f32; 3]| [-r[0].to_degrees(), r[1].to_degrees(), -r[2].to_degrees()];
    let round = |v: f32| (v as f64 * 1e4).round() / 1e4;
    let r3 = |v: [f32; 3]| json!([round(v[0]), round(v[1]), round(v[2])]);
    let bones: Vec<Value> = parts
        .iter()
        .map(|p| {
            let mut b = serde_json::Map::new();
            b.insert("name".into(), json!(p.name));
            if let Some(parent) = p.parent.and_then(|i| parts.get(i as usize)) {
                b.insert("parent".into(), json!(parent.name));
            }
            b.insert("pivot".into(), r3(p.pivot));
            if p.rest != [0.0; 3] {
                b.insert("rotation".into(), r3(deg(p.rest)));
            }
            let cubes: Vec<Value> = p
                .cuboids
                .iter()
                .map(|c| {
                    let origin = [c.origin[0] + p.pivot[0], c.origin[1] + p.pivot[1], c.origin[2] + p.pivot[2]];
                    let mut o = serde_json::Map::new();
                    o.insert("origin".into(), r3(origin));
                    o.insert("size".into(), r3(c.size));
                    if c.inflate != 0.0 {
                        o.insert("inflate".into(), json!(round(c.inflate)));
                    }
                    if c.mirror {
                        o.insert("mirror".into(), json!(true));
                    }
                    let uv = match c.faces {
                        Some(rects) => {
                            let mut faces = serde_json::Map::new();
                            for (i, face) in ["east", "west", "down", "up", "north", "south"]
                                .into_iter()
                                .enumerate()
                            {
                                let [u, v, w, h] = rects[i];
                                let (v, h) = if face == "down" { (v + h, -h) } else { (v, h) };
                                faces.insert(face.into(), json!({ "uv": [round(u), round(v)], "uv_size": [round(w), round(h)] }));
                            }
                            Value::Object(faces)
                        }
                        None => json!([round(c.uv[0]), round(c.uv[1])]),
                    };
                    o.insert("uv".into(), uv);
                    Value::Object(o)
                })
                .collect();
            if !cubes.is_empty() {
                b.insert("cubes".into(), Value::Array(cubes));
            }
            if !p.mesh.is_empty() {
                let positions: Vec<Value> = p.mesh.iter().map(|v| r3(v.pos)).collect();
                let uvs: Vec<Value> = p
                    .mesh
                    .iter()
                    .map(|v| json!([round(v.uv[0]), round(v.uv[1])]))
                    .collect();
                let mut normals = Vec::new();
                let mut polys = Vec::new();
                for (t, tri) in p.mesh.chunks_exact(3).enumerate() {
                    let [a, b2, c] = [Vec3::from(tri[0].pos), Vec3::from(tri[1].pos), Vec3::from(tri[2].pos)];
                    let n = (b2 - a).cross(c - a).normalize_or_zero();
                    normals.push(r3(n.to_array()));
                    polys.push(json!(
                        (0..3)
                            .map(|k| [t * 3 + k, t, t * 3 + k])
                            .collect::<Vec<_>>()
                    ));
                }
                b.insert(
                    "poly_mesh".into(),
                    json!({ "normalized_uvs": false, "positions": positions, "normals": normals, "uvs": uvs, "polys": polys }),
                );
            }
            Value::Object(b)
        })
        .collect();
    let geo = json!({
        "format_version": "1.12.0",
        "minecraft:geometry": [{
            "description": {
                "identifier": format!("geometry.{}", name.rsplit(':').next().unwrap_or(name)),
                "texture_width": texture[0],
                "texture_height": texture[1],
            },
            "bones": bones,
        }],
    });
    inline_numbers(&serde_json::to_string_pretty(&geo).unwrap_or_default())
}

/// Pretty JSON with lists of numbers on one line, as Blockbench writes them.
fn inline_numbers(pretty: &str) -> String {
    let mut out = String::with_capacity(pretty.len());
    let mut rest = pretty;
    while let Some(at) = rest.find('[') {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        match after.find(']') {
            Some(end) if !after[1..end].contains(['[', '{']) => {
                let items: Vec<&str> = after[1..end].split(',').map(str::trim).collect();
                out.push('[');
                out.push_str(&items.join(", "));
                out.push(']');
                rest = &after[end + 1..];
            }
            _ => {
                out.push('[');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Bedrock `poly_mesh`: positions, uvs and polys of 3 or 4 corners `[position, normal, uv]`,
/// made into triangles. With `normalized_uvs` the uvs are 0..1 of a texture `tex` texels big.
fn poly_mesh(pm: &Value, tex: [f32; 2]) -> Vec<MeshVertex> {
    let list = |k: &str| {
        pm.get(k)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let positions: Vec<[f32; 3]> = list("positions")
        .iter()
        .filter_map(|p| vec3(Some(p)))
        .collect();
    let uvs: Vec<[f32; 2]> = list("uvs").iter().filter_map(|p| vec2(Some(p))).collect();
    let normalized = pm
        .get("normalized_uvs")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let polys = match pm.get("polys") {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for poly in polys {
        let corners: Vec<MeshVertex> = poly
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let c = c.as_array()?;
                let p = *positions.get(num(c.first()?) as usize)?;
                let mut uv = c
                    .get(2)
                    .and_then(|i| uvs.get(num(i) as usize))
                    .copied()
                    .unwrap_or([0.0; 2]);
                if normalized {
                    // Normalized uvs count from the bottom of the texture.
                    uv = [uv[0] * tex[0], (1.0 - uv[1]) * tex[1]];
                }
                Some(MeshVertex { pos: p, uv })
            })
            .collect();
        if corners.len() >= 3 {
            out.extend([corners[0], corners[1], corners[2]]);
        }
        if corners.len() == 4 {
            out.extend([corners[0], corners[2], corners[3]]);
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prop {
    Rotation,
    Position,
    Scale,
}

#[derive(Clone, Debug)]
struct Key {
    time: f32,
    pre: [f32; 3],
    post: [f32; 3],
    smooth: bool,
}

#[derive(Clone, Debug)]
pub struct Channel {
    pub bone: String,
    pub prop: Prop,
    keys: Vec<Key>,
}

/// What moves an animation's time along (`anim_time_update`, or what its action goes by).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Drive {
    Clock,
    /// How far the entity walked.
    Distance,
    /// The action's progress: the file's length stretches over however long it takes.
    Progress,
}

#[derive(Clone, Debug)]
pub struct Animation {
    /// What follows `animation.<model>.`, dots included (`swing.first_person`).
    pub name: String,
    pub length: f32,
    pub looping: bool,
    /// Stays on its last frame when done (`"loop": "hold_on_last_frame"`).
    pub hold: bool,
    /// Replaces what is below it on the bones it names, instead of adding to it
    /// (`override_previous_animation`).
    pub replaces: bool,
    /// `anim_time_update`, if it names something known.
    pub drive: Option<Drive>,
    /// `blend_weight`: a number, or by how fast the entity walks.
    pub weight: f32,
    pub by_speed: bool,
    /// Seconds it takes to come and to go, unless its action says (`blend_in`, `blend_out`).
    pub blend_in: Option<f32>,
    pub blend_out: Option<f32>,
    /// `start_delay` and `loop_delay`, seconds.
    pub start_delay: f32,
    pub loop_delay: f32,
    pub channels: Vec<Channel>,
}

/// Every animation in a `.animation.json`.
pub fn load_animations(src: &str) -> Result<Vec<Animation>, String> {
    let root: Value = serde_json::from_str(src).map_err(|e| e.to_string())?;
    let anims = root
        .get("animations")
        .and_then(Value::as_object)
        .ok_or("no animations in the file")?;
    let mut out = Vec::new();
    for (full, a) in anims {
        let name = match full.strip_prefix("animation.") {
            Some(rest) => rest.split_once('.').map_or(rest, |r| r.1).to_string(),
            None => full.to_string(),
        };
        let looping = a.get("loop") == Some(&Value::Bool(true));
        let hold = a.get("loop").and_then(Value::as_str) == Some("hold_on_last_frame");
        let mut channels = Vec::new();
        let mut longest = 0.0f32;
        for (bone, props) in a
            .get("bones")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            for (key, prop) in [("rotation", Prop::Rotation), ("position", Prop::Position), ("scale", Prop::Scale)] {
                let Some(v) = props.get(key) else { continue };
                let mut keys: Vec<Key> = match v {
                    Value::Object(frames) => frames
                        .iter()
                        .filter_map(|(t, f)| {
                            let time: f32 = t.parse().ok()?;
                            let (pre, post, smooth) = match f {
                                Value::Object(o) => {
                                    let post = vec3(o.get("post")).or_else(|| vec3(o.get("pre")))?;
                                    (
                                        vec3(o.get("pre")).unwrap_or(post),
                                        post,
                                        o.get("lerp_mode").and_then(Value::as_str) == Some("catmullrom"),
                                    )
                                }
                                other => {
                                    let v = vec3(Some(other))?;
                                    (v, v, false)
                                }
                            };
                            Some(Key { time, pre, post, smooth })
                        })
                        .collect(),
                    constant => vec3(Some(constant))
                        .map(|v| vec![Key { time: 0.0, pre: v, post: v, smooth: false }])
                        .unwrap_or_default(),
                };
                keys.sort_by(|a, b| a.time.total_cmp(&b.time));
                longest = longest.max(keys.last().map_or(0.0, |k| k.time));
                if !keys.is_empty() {
                    channels.push(Channel { bone: bone.clone(), prop, keys });
                }
            }
        }
        let length = a
            .get("animation_length")
            .map(num)
            .unwrap_or(longest)
            .max(1e-3);
        let says = |key: &str| a.get(key).and_then(Value::as_str).unwrap_or("");
        let drive = match says("anim_time_update") {
            "" => None,
            t if t.contains("distance_moved") => Some(Drive::Distance),
            t if t.contains("progress") => Some(Drive::Progress),
            _ => Some(Drive::Clock),
        };
        let by_speed = says("blend_weight").contains("move_speed");
        let weight = a
            .get("blend_weight")
            .filter(|v| v.is_number())
            .map_or(1.0, num);
        let replaces = a.get("override_previous_animation") == Some(&Value::Bool(true));
        let secs = |key: &str| a.get(key).map(|v| num(v).max(0.0));
        out.push(Animation {
            name,
            length,
            looping,
            hold,
            replaces,
            drive,
            weight,
            by_speed,
            blend_in: secs("blend_in"),
            blend_out: secs("blend_out"),
            start_delay: secs("start_delay").unwrap_or(0.0),
            loop_delay: secs("loop_delay").unwrap_or(0.0),
            channels,
        });
    }
    Ok(out)
}

impl Animation {
    /// Everything it moves stays within this many pixels and this many times its size. For files
    /// from strangers.
    pub fn within(&self, pixels: f32, scale: f32) -> bool {
        self.channels.iter().all(|c| {
            let limit = match c.prop {
                Prop::Position => pixels,
                Prop::Scale => scale,
                Prop::Rotation => 36000.0,
            };
            c.keys
                .iter()
                .all(|k| k.pre.iter().chain(&k.post).all(|v| v.abs() <= limit))
        })
    }

    /// Where a bone no model has (`camera`) is moved at `t`: pixels and radians.
    pub fn free_bone(&self, bone: &str, t: f32) -> Option<(Vec3, Vec3)> {
        let ch = |prop: Prop| {
            self.channels
                .iter()
                .find(|c| c.prop == prop && c.bone.eq_ignore_ascii_case(bone))
        };
        let (rot, pos) = (ch(Prop::Rotation), ch(Prop::Position));
        if rot.is_none() && pos.is_none() {
            return None;
        }
        let r = rot.map_or(Vec3::ZERO, |c| Vec3::from_array(angles(c.sample(t))));
        let p = pos.map_or(Vec3::ZERO, |c| Vec3::from_array(c.sample(t)));
        Some((p, r))
    }
}

impl Channel {
    /// Value at `t` seconds: linear between keys, uniform Catmull-Rom for smooth ones.
    pub fn sample(&self, t: f32) -> [f32; 3] {
        let k = &self.keys;
        let i = k.partition_point(|key| key.time <= t);
        if i == 0 {
            return k[0].pre;
        }
        if i == k.len() {
            return k[i - 1].post;
        }
        let (a, b) = (&k[i - 1], &k[i]);
        let f = ((t - a.time) / (b.time - a.time).max(1e-6)).clamp(0.0, 1.0);
        if a.smooth || b.smooth {
            let p0 = if i >= 2 { k[i - 2].post } else { a.post };
            let p3 = k.get(i + 1).map_or(b.pre, |k| k.pre);
            let (p1, p2) = (a.post, b.pre);
            std::array::from_fn(|j| {
                let (p0, p1, p2, p3) = (p0[j], p1[j], p2[j], p3[j]);
                0.5 * (2.0 * p1
                    + (p2 - p0) * f
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f * f
                    + (3.0 * p1 - p0 - 3.0 * p2 + p3) * f * f * f)
            })
        } else {
            std::array::from_fn(|j| a.post[j] + (b.pre[j] - a.post[j]) * f)
        }
    }
}

/// The bone `name` written in a file means on this part. `mirror` reads the file with right and
/// left swapped, for what the other hand plays.
fn names_part(name: &str, p: &Part, mirror: bool) -> bool {
    let n = name.to_ascii_lowercase().replace('_', "");
    let own = p.name.to_ascii_lowercase().replace('_', "");
    if let Some(arm) = arm_role(&n) {
        let other = if arm == role::ARM_LEFT { role::ARM_RIGHT } else { role::ARM_LEFT };
        return p.role == if mirror { other } else { arm };
    }
    if !mirror {
        return n == own;
    }
    match (n.strip_prefix("right"), n.strip_prefix("left")) {
        (Some(rest), _) => own.strip_prefix("left") == Some(rest),
        (_, Some(rest)) => own.strip_prefix("right") == Some(rest),
        _ => n == own,
    }
}

/// (rotation, position, scale) channel indices.
type Channels = [Option<usize>; 3];

/// An animation on a model: which channel drives each bone, resolved once, as written and with
/// the sides swapped.
pub struct Bound {
    pub anim: Arc<Animation>,
    pub bones: Vec<Channels>,
    pub mirrored: Vec<Channels>,
    /// The whole model moves by these (a channel of a bone called `root`, if it has no such bone).
    root: Channels,
}

impl Bound {
    pub fn new(anim: Arc<Animation>, parts: &[Part]) -> Bound {
        let table = |mirror: bool| {
            parts
                .iter()
                .map(|p| {
                    let find = |prop: Prop| {
                        anim.channels
                            .iter()
                            .position(|c| c.prop == prop && names_part(&c.bone, p, mirror))
                    };
                    [find(Prop::Rotation), find(Prop::Position), find(Prop::Scale)]
                })
                .collect()
        };
        let has_root = parts.iter().any(|p| p.name.eq_ignore_ascii_case("root"));
        let root = [Prop::Rotation, Prop::Position, Prop::Scale].map(|prop| {
            anim.channels
                .iter()
                .position(|c| !has_root && c.prop == prop && c.bone.eq_ignore_ascii_case("root"))
        });
        Bound { bones: table(false), mirrored: table(true), root, anim }
    }

    /// Someone else's clip on a model carried by them: only the bones it names here, the whole of
    /// the carrier moves it already. `None` if it names none.
    pub fn carried(anim: Arc<Animation>, parts: &[Part]) -> Option<Bound> {
        let b = Bound { root: [None; 3], ..Bound::new(anim, parts) };
        b.bones
            .iter()
            .any(|c| c.iter().any(Option::is_some))
            .then_some(b)
    }

    /// Leaves out the bones `over` names: a mod's clip of the same action takes those, and this
    /// one still moves the rest.
    pub fn without(&self, over: &Bound) -> Bound {
        let keep = |own: &Vec<Channels>, top: &Vec<Channels>| {
            own.iter()
                .zip(top)
                .map(|(c, t)| if t.iter().any(Option::is_some) { [None; 3] } else { *c })
                .collect()
        };
        let root = if over.root.iter().any(Option::is_some) { [None; 3] } else { self.root };
        Bound { anim: self.anim.clone(), bones: keep(&self.bones, &over.bones), mirrored: keep(&self.mirrored, &over.mirrored), root }
    }

    /// Moves a bone of this part (index `i`) at all.
    pub fn names(&self, i: usize, mirror: bool) -> bool {
        let t = if mirror { &self.mirrored } else { &self.bones };
        t[i].iter().any(Option::is_some)
    }

    /// Time into the animation for `secs` since it started, looping if the file or the caller
    /// says so.
    pub fn at(&self, secs: f32, looping: bool) -> f32 {
        let a = &self.anim;
        let secs = (secs - a.start_delay).max(0.0);
        if looping || a.looping { secs.rem_euclid(a.length + a.loop_delay).min(a.length) } else { secs.min(a.length) }
    }

    pub fn finished(&self, secs: f32, looping: bool) -> bool {
        let a = &self.anim;
        !looping && !a.looping && !a.hold && secs >= a.start_delay + a.length
    }
}

/// One animation playing on a model this frame.
#[derive(Clone, Copy)]
pub struct Playing<'a> {
    pub bound: &'a Bound,
    pub t: f32,
    pub w: f32,
    /// With right and left swapped.
    pub mirror: bool,
    /// The bones it names follow the view.
    pub aim: bool,
}

/// What moves a model this frame: its keyframes, and the live look that no keyframes know.
pub struct Motion<'a> {
    /// The look: the head's, and the arms' that follow the view.
    pub pitch: f32,
    pub head_yaw: f32,
    /// From the first person they don't turn with the view, they are in it: where a standing
    /// model's bones go with its eyes at the camera. It holds whatever the body does.
    pub aim_root: Option<Mat4>,
    pub clips: &'a [Playing<'a>],
}

impl Playing<'_> {
    /// A rotation channel at this time, in radians.
    fn turn(&self, channel: usize) -> Vec3 {
        let mut r = Vec3::from_array(angles(self.bound.anim.channels[channel].sample(self.t)));
        if self.mirror {
            (r.y, r.z) = (-r.y, -r.z);
        }
        r
    }

    /// A position channel at this time, in pixels.
    fn moved(&self, channel: usize) -> Vec3 {
        let mut v = Vec3::from_array(self.bound.anim.channels[channel].sample(self.t));
        if self.mirror {
            v.x = -v.x;
        }
        v
    }
}

/// Bone transforms (model pixels in, `root` space out), parents before children as models list
/// them; a part listed before its parent gets the root.
pub fn pose(parts: &[Part], root: Mat4, m: &Motion, out: &mut Vec<Mat4>) {
    out.clear();
    let euler = |r: Vec3| Quat::from_euler(EulerRot::ZYX, r.z, r.y, r.x);
    let mut root = root;
    for c in m.clips.iter().filter(|c| c.w > 0.0) {
        let by = c.bound.root[1].map_or(Vec3::ZERO, |i| c.moved(i));
        let turn = c.bound.root[0].map_or(Vec3::ZERO, |i| c.turn(i));
        if by != Vec3::ZERO || turn != Vec3::ZERO {
            root *= Mat4::from_translation(by * c.w) * Mat4::from_quat(euler(turn * c.w));
        }
    }
    let aiming = m.clips.iter().any(|c| c.aim && c.w > 0.0);
    let back = if aiming { root.inverse() } else { Mat4::IDENTITY };
    // Half way: all of it has them through the legs looking down and across the chest looking aside.
    let look = Vec3::new(m.pitch, m.head_yaw, 0.0) * 0.5;
    for (i, p) in parts.iter().enumerate() {
        let rest = Vec3::new(p.rest[0], p.rest[1], p.rest[2]);
        let mut rot = rest;
        if p.role == role::HEAD {
            rot += Vec3::new(m.pitch, m.head_yaw, 0.0);
        }
        let mut offset = Vec3::ZERO;
        let mut scale = Vec3::ONE;
        // How much of it follows the view: as much as what aims it is on, so it comes and goes with the clip.
        let mut aimed = 0.0f32;
        let arm = p.role == role::ARM_LEFT || p.role == role::ARM_RIGHT;
        // In the view an arm is what it holds and does, not how the body under it walks.
        let in_view = arm
            && m.aim_root.is_some()
            && m.clips
                .iter()
                .any(|c| c.aim && c.w > 0.0 && c.bound.names(i, c.mirror));
        for c in m.clips {
            if c.w <= 0.0 || (in_view && !c.aim) {
                continue;
            }
            let ch = if c.mirror { c.bound.mirrored[i] } else { c.bound.bones[i] };
            // Over what is below, its weight says how much of that it hides, and its own angles
            // stay what the file has: 0.9 leaves a tenth of the walk on an arm.
            let replace = c.bound.anim.replaces;
            let own = if replace { (c.w / c.bound.anim.weight.max(1e-3)).min(1.0) } else { 1.0 };
            if c.aim && arm && (ch[0].is_some() || ch[1].is_some()) {
                aimed = aimed.max(if replace { own } else { c.w });
            }
            if let Some(k) = ch[0] {
                let r = c.turn(k) * own;
                rot = if replace { rest + (rot - rest) * (1.0 - c.w) + r } else { rot + r * c.w };
            }
            if let Some(k) = ch[1] {
                let by = c.moved(k) * own;
                offset = if replace { offset * (1.0 - c.w) + by } else { offset + by * c.w };
            }
            if let Some(k) = ch[2] {
                let to = Vec3::from_array(c.bound.anim.channels[k].sample(c.t));
                scale = if replace { scale.lerp(to, c.w) } else { scale * Vec3::ONE.lerp(to, c.w) };
            }
        }
        let pivot = Vec3::from_array(p.pivot);
        let local =
            Mat4::from_translation(offset + pivot) * Mat4::from_quat(euler(rot)) * Mat4::from_scale(scale) * Mat4::from_translation(-pivot);
        let parent = p
            .parent
            .and_then(|q| out.get(q as usize))
            .copied()
            .unwrap_or(root);
        let mut bone = parent * local;
        if aimed > 0.0 {
            bone = match m.aim_root {
                Some(view_root) => view_root * local,
                None => {
                    let around = pivot + offset;
                    let view = Mat4::from_quat(euler(look * aimed));
                    root * Mat4::from_translation(around) * view * Mat4::from_translation(-around) * back * bone
                }
            };
        }
        out.push(bone);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_from_names() {
        assert_eq!((role_of("rightArm", 0), role_of("left_leg", 0), role_of("body", 0)), (role::ARM_LEFT, role::LEG_RIGHT, role::STATIC));
        assert_eq!((role_of("mainArm", 0), role_of("off_arm", 0)), (role::ARM_RIGHT, role::ARM_LEFT));
        assert_eq!(
            [0, 1, 2, 3].map(|i| role_of(&format!("leg{i}"), 4)),
            [role::LEG_RIGHT, role::LEG_LEFT, role::LEG_FRONT_RIGHT, role::LEG_FRONT_LEFT]
        );
        assert_eq!(role_of("leg3", 8), role::LEG_SWEEP);
    }

    #[test]
    fn saved_files_read_back_the_same() {
        let parts = vec![
            Part {
                role: 0,
                name: "body".into(),
                parent: None,
                pivot: [0.0, 24.0, 0.0],
                rest: [0.3, -0.2, 0.1],
                cuboids: vec![Cuboid {
                    origin: [-4.0, -12.0, -2.0],
                    size: [8.0, 12.0, 4.0],
                    uv: [16.0, 16.0],
                    inflate: 0.5,
                    mirror: true,
                    faces: None,
                }],
                mesh: Vec::new(),
            },
            Part {
                role: 0,
                name: "ear".into(),
                parent: Some(0),
                pivot: [1.0, 26.0, -1.0],
                rest: [0.0; 3],
                cuboids: vec![Cuboid {
                    origin: [0.0; 3],
                    size: [1.0, 2.0, 3.0],
                    uv: [0.0; 2],
                    inflate: 0.0,
                    mirror: false,
                    faces: Some([
                        [0.0, 0.0, 3.0, 2.0],
                        [3.0, 0.0, 3.0, 2.0],
                        [6.0, 4.0, 1.0, -3.0],
                        [7.0, 1.0, 1.0, 3.0],
                        [8.0, 0.0, 1.0, 2.0],
                        [9.0, 0.0, 1.0, 2.0],
                    ]),
                }],
                mesh: vec![
                    MeshVertex { pos: [0.0, 0.0, 0.0], uv: [0.0, 0.0] },
                    MeshVertex { pos: [1.0, 0.0, 0.0], uv: [4.0, 0.0] },
                    MeshVertex { pos: [1.0, 1.0, 0.0], uv: [4.0, 4.0] },
                ],
            },
        ];
        let back = load_geo(&save_geo("t", &parts, [64, 32])).unwrap();
        assert_eq!(back.texture, Some([64, 32]));
        for (a, b) in parts.iter().zip(&back.parts) {
            assert_eq!((&a.name, a.parent, a.pivot), (&b.name, b.parent, b.pivot));
            assert!(a.rest.iter().zip(b.rest).all(|(x, y)| (x - y).abs() < 1e-4), "{:?} {:?}", a.rest, b.rest);
            assert_eq!(a.cuboids.len(), b.cuboids.len());
            for (x, y) in a.cuboids.iter().zip(&b.cuboids) {
                assert_eq!((x.origin, x.size, x.uv, x.inflate, x.mirror, x.faces), (y.origin, y.size, y.uv, y.inflate, y.mirror, y.faces));
            }
            assert_eq!(a.mesh, b.mesh);
        }
    }

    #[test]
    fn bedrock_files() {
        let geo = r#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.t","texture_width":64,"texture_height":32},
            "bones":[{"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]},
                     {"name":"rightArm","parent":"body","pivot":[-5,22,0],"rotation":[-90,0,0],"cubes":[{"origin":[-8,12,-2],"size":[4,12,4],"uv":[40,16],"mirror":true}]},
                     {"name":"orb","poly_mesh":{"positions":[[0,0,0],[1,0,0],[1,1,0],[0,1,0]],"uvs":[[0,0],[1,0],[1,1],[0,1]],"polys":[[[0,0,0],[1,0,1],[2,0,2],[3,0,3]]]}}]}]}"#;
        let geo = load_geo(geo).unwrap();
        assert_eq!(geo.texture, Some([64, 32]));
        let parts = geo.parts;
        assert_eq!(parts.len(), 3);
        assert_eq!((parts[1].parent, parts[1].role), (Some(0), role::ARM_LEFT));
        assert_eq!(parts[1].cuboids[0].origin, [-3.0, -10.0, -2.0]);
        assert!(parts[1].cuboids[0].mirror && parts[2].mesh.len() == 6);
        // A Bedrock arm at x = -90 points forward (-Z): its hand ends up in front of the shoulder.
        let mut mats = Vec::new();
        pose(&parts, Mat4::IDENTITY, &Motion { pitch: 0.0, head_yaw: 0.0, aim_root: None, clips: &[] }, &mut mats);
        let hand = mats[1].transform_point3(Vec3::new(-6.0, 12.0, 0.0));
        assert!(hand.z < -8.0 && (hand.y - 22.0).abs() < 0.01, "{hand}");

        let anim = r#"{"format_version":"1.8.0","animations":{"animation.t.wave":{"loop":true,"animation_length":2,
            "bones":{"rightArm":{"rotation":{"0":[0,0,0],"1":[0,0,-90],"2":[0,0,0]}}}}}}"#;
        let a = load_animations(anim).unwrap().remove(0);
        assert_eq!((a.name.as_str(), a.looping, a.length), ("wave", true, 2.0));
        assert_eq!(a.channels[0].sample(0.5), [0.0, 0.0, -45.0]);
        let b = Bound::new(Arc::new(a), &parts);
        assert_eq!((b.bones[1][0], b.bones[0][0]), (Some(0), None));
        assert_eq!(b.at(3.0, false), 1.0);
    }
}

/// Vanilla's `WalkAnimationState`, per second: the swing follows the speed, full at 5 blocks a
/// second and no more, and the limbs move by the swing. However fast it flies, they keep their pace.
pub fn walk_limbs(limb: &mut f32, swing: &mut f32, speed: f32, dt: f32) {
    *swing += ((speed / 5.0).min(1.0) - *swing) * (1.0 - 0.6f32.powf(dt * crate::world::TPS as f32));
    *limb += *swing * 5.0 * dt;
}

#[cfg(test)]
mod walk_tests {
    use super::walk_limbs;

    fn after(speed: f32) -> f32 {
        let (mut limb, mut swing) = (0.0, 0.0);
        for _ in 0..600 {
            walk_limbs(&mut limb, &mut swing, speed, 1.0 / 60.0);
        }
        limb / 10.0
    }

    #[test]
    fn limbs_have_a_top_speed() {
        assert!((after(2.5) - 2.5).abs() < 0.2, "at a walk they follow the feet: {}", after(2.5));
        assert!((after(40.0) - 5.0).abs() < 0.2, "flying fast they don't: {}", after(40.0));
        assert_eq!(after(40.0), after(500.0));
    }
}
