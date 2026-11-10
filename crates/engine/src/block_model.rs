//! Block and item shapes from files: the Java block model Blockbench writes ("Java Block/Item"),
//! `models/block/<name>.json` and `models/item/<name>.json`. Entities use Bedrock's `geo.json`
//! instead (`model.rs`): bones and keyframes are what those need, faces textured from many
//! files what blocks need.
//!
//! What is read: `elements` (`from`, `to`, `rotation`, `faces` with `texture`, `uv` and
//! `rotation`), `textures` with `#name` references, and `parent` (another file, or vanilla's cube parents and
//! `item/generated`, which the engine knows without their files). A face's `#name` that no
//! texture sets takes the block's own texture of that face, so a file can give only the shape.

use rustc_hash::FxHashMap;
use serde_json::Value;

/// One element: corners in model pixels (0..16), per face (-X, +X, -Y, +Y, -Z, +Z) the texture
/// name (empty: the block's own of that face), texels (negative: from the position; `NO_FACE` in
/// u0: not drawn) and quarter turns of its picture, and how the element is tilted.
#[derive(Clone, Debug, PartialEq)]
pub struct FileBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub textures: [String; 6],
    pub uv: [[f32; 4]; 6],
    pub turn: [u8; 6],
    pub tilt: weft_abi::BoxTilt,
    /// `shade: false`: no shading by side.
    pub flat: bool,
}

impl FileBox {
    /// As drawn: in blocks, `texture` giving the id of a face's name.
    pub fn model_box(&self, mut texture: impl FnMut(&str, usize) -> u32) -> crate::content::ModelBox {
        crate::content::ModelBox {
            turn: self.turn,
            tilt: weft_abi::BoxTilt { origin: self.tilt.origin.map(|v| v / 16.0), ..self.tilt },
            min: self.min.map(|v| v / 16.0),
            max: self.max.map(|v| v / 16.0),
            textures: std::array::from_fn(|i| texture(&self.textures[i], i)),
            uv: self.uv,
            flat: self.flat,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileModel {
    /// Elements, if the model (or a parent) has any.
    pub boxes: Vec<FileBox>,
    /// A full cube from one of vanilla's cube parents: the texture per face.
    pub cube: Option<[String; 6]>,
    /// `item/generated` or `item/handheld`: a flat item showing `layer0`.
    pub sprite: Option<String>,
    /// Where it goes in the hand, if the model or a parent says.
    pub display: Option<Display>,
}

/// An item model's place in the hand, as Blockbench's Display tab writes it: degrees, pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub rotation: [f32; 3],
    pub translation: [f32; 3],
    pub scale: [f32; 3],
}

fn read_display(root: &Value) -> Option<Display> {
    let d = root.get("display")?.get("thirdperson_righthand")?;
    let three = |key: &str, default: f32| -> [f32; 3] {
        d.get(key)
            .and_then(|v| v.as_array())
            .map_or([default; 3], |a| {
                std::array::from_fn(|i| {
                    a.get(i)
                        .and_then(|v| v.as_f64())
                        .map_or(default, |v| v as f32)
                })
            })
    };
    Some(Display { rotation: three("rotation", 0.0), translation: three("translation", 0.0), scale: three("scale", 1.0) })
}

use weft_abi::FACE_NAMES as FACES;

/// Reads the model `name` (`ns:block/lamp`) through `read` (file text by asset name), following
/// parents. Errors name the file.
pub fn load(name: &str, read: &dyn Fn(&str) -> Option<String>) -> Result<Option<FileModel>, String> {
    load_with(name, read, &[])
}

/// `load` with `textures` (a `#name` and the texture there) over whatever the files say: one
/// model for many blocks, each with its own pictures.
pub fn load_with(name: &str, read: &dyn Fn(&str) -> Option<String>, own: &[(String, String)]) -> Result<Option<FileModel>, String> {
    let Some(src) = read(name) else { return Ok(None) };
    let mut textures: FxHashMap<String, String> = own
        .iter()
        .map(|(k, v)| (k.trim_start_matches('#').to_string(), v.clone()))
        .collect();
    let mut elements: Option<Vec<Value>> = None;
    let mut builtin: Option<String> = None;
    let mut display = None;
    let (mut src, mut current) = (src, name.to_string());
    // Child first: its textures win, its elements replace the parent's.
    for _ in 0..16 {
        let root: Value = serde_json::from_str(&src).map_err(|e| format!("model {current}: {e}"))?;
        let ns = current.split_once(':').map_or("", |x| x.0).to_string();
        display = display.or_else(|| read_display(&root));
        for (k, v) in root
            .get("textures")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if let Some(v) = v.as_str() {
                textures.entry(k.clone()).or_insert_with(|| qualify(&ns, v));
            }
        }
        if elements.is_none() {
            elements = root.get("elements").and_then(Value::as_array).cloned();
        }
        let Some(parent) = root.get("parent").and_then(Value::as_str) else { break };
        let parent = qualify(&ns, parent);
        let short = parent.split_once(':').map_or(parent.as_str(), |x| x.1);
        if matches!(
            short,
            "block/cube_all"
                | "block/cube"
                | "block/cube_column"
                | "block/cube_bottom_top"
                | "item/generated"
                | "item/handheld"
                | "block/block"
        ) {
            builtin = Some(short.to_string());
            break;
        }
        src = read(&parent).ok_or_else(|| format!("model {current}: no parent {parent}"))?;
        current = parent;
    }
    let resolve = |t: &str| -> String {
        let mut t = t.to_string();
        for _ in 0..8 {
            match t.strip_prefix('#') {
                Some(var) => match textures.get(var) {
                    Some(v) => t = v.clone(),
                    None => return String::new(),
                },
                None => return t,
            }
        }
        String::new()
    };
    let mut out = FileModel { display, ..FileModel::default() };
    if let Some(list) = elements {
        for (i, e) in list.iter().enumerate() {
            let v3 = |k: &str| -> Result<[f32; 3], String> {
                let a = e
                    .get(k)
                    .and_then(Value::as_array)
                    .filter(|a| a.len() == 3)
                    .ok_or_else(|| format!("model {name}: element {i} without `{k}`"))?;
                Ok(std::array::from_fn(|j| a[j].as_f64().unwrap_or(0.0) as f32))
            };
            let (from, to) = (v3("from")?, v3("to")?);
            let mut b = FileBox {
                min: std::array::from_fn(|j| from[j].min(to[j])),
                max: std::array::from_fn(|j| from[j].max(to[j])),
                textures: Default::default(),
                uv: [weft_abi::UV_NO_FACE; 6],
                turn: [0; 6],
                tilt: tilt(e),
                flat: e.get("shade").and_then(Value::as_bool) == Some(false),
            };
            let faces = e.get("faces").and_then(Value::as_object);
            for (d, face) in FACES.iter().enumerate() {
                let Some(f) = faces.and_then(|f| f.get(*face)) else { continue };
                b.textures[d] = f
                    .get("texture")
                    .and_then(Value::as_str)
                    .map_or(String::new(), |t| resolve(t));
                b.uv[d] = match f
                    .get("uv")
                    .and_then(Value::as_array)
                    .filter(|a| a.len() == 4)
                {
                    Some(a) => std::array::from_fn(|j| a[j].as_f64().unwrap_or(0.0) as f32),
                    None => weft_abi::UV_FROM_BOX,
                };
                // Files turn a picture clockwise, `ModelBox::turn` goes the other way round.
                let quarters = f
                    .get("rotation")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .div_euclid(90);
                b.turn[d] = (-quarters).rem_euclid(4) as u8;
            }
            out.boxes.push(b);
        }
    }
    let t = |k: &str| textures.get(k).cloned().unwrap_or_default();
    match builtin.as_deref() {
        _ if !out.boxes.is_empty() => {}
        Some("block/cube_all") => out.cube = Some(std::array::from_fn(|_| t("all"))),
        Some("block/cube") => out.cube = Some(FACES.map(t)),
        Some("block/cube_column") => out.cube = Some([t("side"), t("side"), t("end"), t("end"), t("side"), t("side")]),
        Some("block/cube_bottom_top") => out.cube = Some([t("side"), t("side"), t("bottom"), t("top"), t("side"), t("side")]),
        Some("item/generated" | "item/handheld") => out.sprite = Some(t("layer0")),
        _ => return Err(format!("model {name}: no elements and no parent the engine knows")),
    }
    Ok(Some(out))
}

/// An element's `rotation` (pixels, about one axis).
fn tilt(e: &Value) -> weft_abi::BoxTilt {
    let Some(r) = e.get("rotation") else { return weft_abi::BoxTilt::NONE };
    let degrees = r.get("angle").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let axis = match r.get("axis").and_then(Value::as_str) {
        Some("x") => [1.0, 0.0, 0.0],
        Some("y") => [0.0, 1.0, 0.0],
        Some("z") => [0.0, 0.0, 1.0],
        _ => return weft_abi::BoxTilt::NONE,
    };
    let origin = r
        .get("origin")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 3)
        .map_or([8.0; 3], |a| std::array::from_fn(|j| a[j].as_f64().unwrap_or(8.0) as f32));
    weft_abi::BoxTilt { origin, axis, degrees }
}

/// A name in a file: `block/x` is the model's own namespace's, `minecraft:` is dropped.
fn qualify(ns: &str, v: &str) -> String {
    let v = v.strip_prefix("minecraft:").unwrap_or(v);
    if v.starts_with('#') || v.contains(':') { v.to_string() } else { format!("{ns}:{v}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_models() {
        let files: FxHashMap<&str, &str> = [
            ("m:block/lamp", r##"{"parent":"m:block/lamp_base","textures":{"glass":"m:block/glass"},
                "elements":[{"from":[4,0,4],"to":[12,10,12],"rotation":{"angle":22.5,"axis":"y","origin":[8,0,8]},"faces":{"up":{"texture":"#glass","uv":[4.25,4,12,12],"rotation":90},"north":{"texture":"#side"},"south":{"texture":"#nothing"}}}]}"##),
            ("m:block/lamp_base", r#"{"textures":{"side":"block/lamp_side"}}"#),
            ("m:block/log", r#"{"parent":"block/cube_column","textures":{"end":"m:block/log_top","side":"m:block/log"}}"#),
            ("m:item/gem", r#"{"parent":"minecraft:item/generated","textures":{"layer0":"item/gem"}}"#),
        ]
        .into_iter()
        .collect();
        let read = |n: &str| files.get(n).map(|s| s.to_string());
        let lamp = load("m:block/lamp", &read).unwrap().unwrap();
        let b = &lamp.boxes[0];
        assert_eq!((b.min, b.max), ([4.0, 0.0, 4.0], [12.0, 10.0, 12.0]));
        // up: its own texture and texels; north: the parent's; south: the block's own; west: none.
        assert_eq!((b.textures[3].as_str(), b.uv[3]), ("m:block/glass", [4.25, 4.0, 12.0, 12.0]));
        assert_eq!((b.turn[3], b.turn[4]), (3, 0));
        assert_eq!((b.tilt.origin, b.tilt.axis, b.tilt.degrees), ([8.0, 0.0, 8.0], [0.0, 1.0, 0.0], 22.5));
        assert_eq!((b.textures[4].as_str(), b.uv[4][0]), ("m:block/lamp_side", -1.0));
        assert_eq!((b.textures[5].as_str(), b.uv[0]), ("", weft_abi::UV_NO_FACE));
        let log = load("m:block/log", &read).unwrap().unwrap();
        assert_eq!(log.cube.unwrap()[3], "m:block/log_top");
        assert_eq!(
            load("m:item/gem", &read)
                .unwrap()
                .unwrap()
                .sprite
                .as_deref(),
            Some("m:item/gem")
        );
        assert!(load("m:block/none", &read).unwrap().is_none());
    }
}
