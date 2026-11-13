//! Templates: fixed shapes of blocks with material slots, entities, loot and joints, placed
//! turned and mirrored (`Transform`). They come from code, from mods' `structures/` files (ours as
//! JSON, structgen's `.wst`) and from `/structure save`.

use std::sync::Arc;

use glam::{DVec3, IVec3, Vec3};
use rustc_hash::FxHashMap;
use weft_abi as abi;

use crate::content::{AIR, Content};

/// One of the 8 ways a template is placed (`abi::transform`): quarter turns clockwise seen from
/// above, after a mirror across x.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Transform(pub u8);

impl Transform {
    pub fn new(bits: u32) -> Transform {
        Transform((bits & 7) as u8)
    }
    pub fn quarter(self) -> u8 {
        self.0 & abi::transform::TURN as u8
    }
    pub fn mirrored(self) -> bool {
        self.0 as u32 & abi::transform::MIRROR != 0
    }
    /// The turn about the vertical: own east goes south, west, north.
    fn turn(self) -> abi::Turn {
        abi::Turn { east: [1, 5, 0, 4][self.quarter() as usize], up: 3 }
    }
    fn vector(self, v: Vec3) -> Vec3 {
        let v = if self.mirrored() { Vec3::new(-v.x, v.y, v.z) } else { v };
        Vec3::from(self.turn().vector_f(v.to_array()))
    }
    /// Which face its own face `f` becomes.
    pub fn face(self, f: usize) -> usize {
        let f = if self.mirrored() && f < 2 { f ^ 1 } else { f };
        self.turn().face(f)
    }
    /// The size of a box of `size` once placed so.
    pub fn size(self, size: IVec3) -> IVec3 {
        if self.quarter() % 2 == 1 { IVec3::new(size.z, size.y, size.x) } else { size }
    }
    /// Where point `p` of a box of `size` (`0..size` each way) goes in the placed box.
    pub fn point(self, size: IVec3, p: Vec3) -> Vec3 {
        let s = size.as_vec3();
        let lo = (0..8)
            .map(|i| self.vector(Vec3::new((i & 1) as f32 * s.x, (i >> 1 & 1) as f32 * s.y, (i >> 2 & 1) as f32 * s.z)))
            .fold(Vec3::MAX, Vec3::min);
        self.vector(p) - lo
    }
    /// Where cell `p` of a box of `size` goes.
    pub fn cell(self, size: IVec3, p: IVec3) -> IVec3 {
        (self.point(size, p.as_vec3() + Vec3::splat(0.5)) - Vec3::splat(0.5))
            .round()
            .as_ivec3()
    }
    /// `state` placed so with the world around it.
    pub fn state(self, c: &Content, state: u32) -> u32 {
        let s = if self.mirrored() { c.mirrored_state(state) } else { state };
        if self.quarter() == 0 { s } else { c.turned_state(s, self.turn()) }
    }
    /// The transforms `bits` allows (`comp::feature::TRANSFORMS`), as drawn when none.
    pub fn allowed(bits: u32) -> Vec<Transform> {
        let v: Vec<Transform> = (0..8u8)
            .filter(|t| bits & 1 << t != 0)
            .map(Transform)
            .collect();
        if v.is_empty() { vec![Transform(0)] } else { v }
    }
}

/// What a template cell holds before it is placed.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    State(u32),
    /// A material slot: its name, the shape made of its block (`stairs`; empty: the block
    /// itself) and the properties set on what fills it.
    Slot {
        slot: String,
        shape: String,
        props: String,
    },
    /// The column's own top block where it is placed (`@ground`).
    Ground,
}

/// Where a template cell's state comes from once placed: this one, or the ground's.
pub const GROUND: u32 = u32::MAX;

/// A template, checked at freeze.
pub struct TemplateDef {
    pub name: String,
    /// The box its cells fill, from its lowest corner.
    pub size: IVec3,
    pub cells: Vec<(IVec3, Cell)>,
    /// Its cells of one state, for what is built of it as drawn (moving bodies).
    pub blocks: Vec<(IVec3, u32)>,
    /// What each slot takes unless a palette says.
    pub slots: FxHashMap<String, String>,
    pub entities: Vec<abi::TemplateEntity>,
    pub loot: Vec<(IVec3, String)>,
    pub data: Vec<(IVec3, Arc<[u8]>)>,
    pub joints: Vec<abi::TemplateJoint>,
    /// By transform and palette (`u32::MAX`: the defaults), made when first placed so.
    variants: parking_lot::Mutex<FxHashMap<(Transform, u32), Arc<Variant>>>,
}

/// An entity of a placed template: where, kind (`u32::MAX` when `data` says), as saved.
pub type Placed = (DVec3, u32, Option<Arc<[u8]>>);

/// A template as placed one way with one palette: what each cell gets, from the placed box's
/// lowest corner.
pub struct Variant {
    pub size: IVec3,
    /// `GROUND`: the column's top block, or `ground` where it has none.
    pub cells: Vec<(IVec3, u32)>,
    pub ground: u32,
    /// Where it stands on the ground: the columns its bottom layer has a block in.
    pub bottom: Vec<(i32, i32)>,
    pub entities: Vec<Placed>,
    pub loot: Vec<(IVec3, Arc<str>)>,
    pub data: Vec<(IVec3, Arc<[u8]>)>,
    pub joints: Vec<(IVec3, u32, usize)>,
}

impl TemplateDef {
    pub fn new(name: String, cells: Vec<(IVec3, Cell)>) -> TemplateDef {
        let size = cells
            .iter()
            .fold(IVec3::ZERO, |m, (p, _)| m.max(*p + IVec3::ONE));
        TemplateDef {
            name,
            size,
            cells,
            blocks: Vec::new(),
            slots: FxHashMap::default(),
            entities: Vec::new(),
            loot: Vec::new(),
            data: Vec::new(),
            joints: Vec::new(),
            variants: Default::default(),
        }
    }

    /// A template from its parts, each block named resolved by `resolve`; what doesn't resolve
    /// is left out and said in `errors`.
    pub fn from_parts(name: &str, parts: Parts, resolve: impl Fn(&str) -> Result<u32, String>, errors: &mut Vec<String>) -> TemplateDef {
        let mut cells: Vec<(IVec3, Cell)> = Vec::with_capacity(parts.blocks.len());
        let mut at: FxHashMap<IVec3, usize> = FxHashMap::default();
        for b in &parts.blocks {
            let cell = match parse_slot(&b.block) {
                Some(slot) => slot,
                None => match resolve(&b.block) {
                    Ok(s) => Cell::State(s),
                    Err(err) => {
                        errors.push(format!("template {name}: {err}"));
                        continue;
                    }
                },
            };
            let p = IVec3::from_array(b.pos);
            // A later block of the same cell (an edit's) wins.
            match at.get(&p) {
                Some(&i) => cells[i].1 = cell,
                None => {
                    at.insert(p, cells.len());
                    cells.push((p, cell));
                }
            }
        }
        let mut def = TemplateDef::new(name.to_string(), cells);
        for s in parts.slots {
            if let Err(err) = resolve(&s.block) {
                errors.push(format!("template {name}: slot {}: {err}", s.name));
            }
            def.slots.insert(s.name, s.block);
        }
        def.blocks = def
            .cells
            .iter()
            .filter_map(|(p, c)| match c {
                Cell::State(s) => Some((*p, *s)),
                _ => None,
            })
            .collect();
        def.entities = parts.entities;
        def.loot = parts
            .loot
            .into_iter()
            .map(|l| (IVec3::from_array(l.pos), l.table))
            .collect();
        def.data = parts
            .data
            .into_iter()
            .map(|d| (IVec3::from_array(d.pos), Arc::from(d.data)))
            .collect();
        def.joints = parts.joints;
        def
    }

    /// The template placed with `t`, its slots filled from `palette`.
    pub fn variant(&self, c: &Content, t: Transform, palette: Option<u32>) -> Arc<Variant> {
        let key = (t, palette.unwrap_or(u32::MAX));
        if let Some(v) = self.variants.lock().get(&key) {
            return v.clone();
        }
        let v = Arc::new(self.make_variant(c, t, palette));
        self.variants.lock().insert(key, v.clone());
        v
    }

    fn make_variant(&self, c: &Content, t: Transform, palette: Option<u32>) -> Variant {
        let size = t.size(self.size);
        let fill = |slot: &str| {
            palette
                .and_then(|p| c.palettes.get(p as usize))
                .and_then(|p| p.slots.get(slot))
                .or_else(|| self.slots.get(slot))
        };
        let ground = fill("ground")
            .and_then(|b| c.state_by_name(b))
            .map_or(0, |s| t.state(c, s));
        let mut cells = Vec::with_capacity(self.cells.len());
        for (p, cell) in &self.cells {
            let state = match cell {
                Cell::State(s) => t.state(c, *s),
                Cell::Ground => GROUND,
                Cell::Slot { slot, shape, props } => match fill(slot).and_then(|b| slot_state(c, b, shape, props)) {
                    Some(s) => t.state(c, s),
                    None => continue,
                },
            };
            cells.push((t.cell(self.size, *p), state));
        }
        let mut bottom: Vec<(i32, i32)> = cells
            .iter()
            .filter(|(p, s)| p.y == 0 && *s != AIR)
            .map(|(p, _)| (p.x, p.z))
            .collect();
        bottom.sort_unstable();
        bottom.dedup();
        let entities = self
            .entities
            .iter()
            .map(|e| {
                let at = t.point(self.size, Vec3::from_array(e.pos)).as_dvec3();
                let kind = c
                    .game
                    .entity_names
                    .get(&e.kind)
                    .copied()
                    .unwrap_or(u32::MAX);
                (at, kind, (!e.data.is_empty()).then(|| Arc::from(e.data.as_slice())))
            })
            .collect();
        Variant {
            size,
            cells,
            ground,
            bottom,
            entities,
            loot: self
                .loot
                .iter()
                .map(|(p, l)| (t.cell(self.size, *p), Arc::from(l.as_str())))
                .collect(),
            data: self
                .data
                .iter()
                .map(|(p, d)| (t.cell(self.size, *p), d.clone()))
                .collect(),
            joints: self
                .joints
                .iter()
                .enumerate()
                .map(|(i, j)| (t.cell(self.size, IVec3::from_array(j.pos)), t.face(j.face as usize) as u32, i))
                .collect(),
        }
    }
}

/// The state slot text `shape` + `props` takes with `block` in it: the shape `shapes_of` made of
/// it, and of the properties those it has.
fn slot_state(c: &Content, block: &str, shape: &str, props: &str) -> Option<u32> {
    let base = c.state_by_name(block)?;
    let name = if shape.is_empty() {
        c.block_of(base).name.clone()
    } else {
        let source = &c.block_of(base).name;
        let key = format!("shape.{shape}");
        c.shapes
            .iter()
            .find(|(_, (s, k))| s == source && *k == key)
            .map(|(made, _)| made.clone())?
    };
    let def = &c.blocks[*c.block_names.get(&name)? as usize];
    let mut state = if shape.is_empty() { base } else { def.first_state };
    for (k, v) in crate::content::prop_pairs(props).ok()? {
        if let Ok(s) = def.changed(state, &[(k, v)]) {
            state = s;
        }
    }
    Some(state)
}

/// A slot reference `@name[:shape][props]`, if `text` is one.
pub fn parse_slot(text: &str) -> Option<Cell> {
    let rest = text.strip_prefix('@')?;
    let (head, props) = match rest.split_once('[') {
        Some((h, p)) => (h, p.strip_suffix(']').unwrap_or(p)),
        None => (rest, ""),
    };
    if head == "ground" {
        return Some(Cell::Ground);
    }
    let (slot, shape) = head.split_once(':').unwrap_or((head, ""));
    Some(Cell::Slot { slot: slot.to_string(), shape: shape.to_string(), props: props.to_string() })
}

/// What a template file holds, as the component lists a template entry has.
#[derive(Default)]
pub struct Parts {
    pub blocks: Vec<abi::TemplateBlock>,
    pub slots: Vec<abi::PaletteSlot>,
    pub entities: Vec<abi::TemplateEntity>,
    pub loot: Vec<abi::TemplateLoot>,
    pub data: Vec<abi::TemplateData>,
    pub joints: Vec<abi::TemplateJoint>,
}

/// Reads a template file by its extension: `.json` ours, `.wst` structgen's.
pub fn read_file(path: &std::path::Path) -> Result<Parts, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    match path.extension().and_then(|e| e.to_str()) {
        Some("wst") => read_wst(&bytes),
        _ => read_json(&bytes),
    }
}

fn ivec(v: Option<&serde_json::Value>) -> Option<[i32; 3]> {
    let a = v?.as_array()?;
    Some([a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32, a.get(2)?.as_i64()? as i32])
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .filter_map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

const FORMAT: &str = "weft:structure";

/// Our file: `size`, a `palette` of block states and slots, `blocks` as `[x, y, z, palette
/// index]` (a cell not there is left as it is), `slots`, `entities`, `block_entities`, `loot`
/// and `joints`.
pub fn read_json(bytes: &[u8]) -> Result<Parts, String> {
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let text = |v: &serde_json::Value, k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let list = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default()
    };
    let palette: Vec<String> = list("palette")
        .iter()
        .map(|p| p.as_str().unwrap_or("core:air").to_string())
        .collect();
    let mut out = Parts::default();
    for b in list("blocks") {
        let Some(a) = b.as_array().filter(|a| a.len() >= 4) else { return Err("a block is not [x, y, z, index]".into()) };
        let n = |i: usize| a[i].as_i64().unwrap_or(0);
        let block = palette
            .get(n(3) as usize)
            .ok_or_else(|| format!("palette index {} past the palette", n(3)))?;
        out.blocks
            .push(abi::TemplateBlock { pos: [n(0) as i32, n(1) as i32, n(2) as i32], block: block.clone() });
    }
    if let Some(slots) = v.get("slots").and_then(|s| s.as_object()) {
        for (name, block) in slots {
            out.slots
                .push(abi::PaletteSlot { name: name.clone(), block: block.as_str().unwrap_or_default().to_string() });
        }
    }
    for e in list("entities") {
        let pos = e
            .get("pos")
            .and_then(|p| p.as_array())
            .map(|a| [0, 1, 2].map(|i| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32))
            .unwrap_or_default();
        out.entities
            .push(abi::TemplateEntity { pos, kind: text(&e, "kind"), data: hex(&text(&e, "data")) });
    }
    for e in list("block_entities") {
        out.data
            .push(abi::TemplateData { pos: ivec(e.get("pos")).unwrap_or_default(), data: hex(&text(&e, "data")) });
    }
    for e in list("loot") {
        out.loot
            .push(abi::TemplateLoot { pos: ivec(e.get("pos")).unwrap_or_default(), table: text(&e, "table") });
    }
    for j in list("joints") {
        let facing = text(&j, "facing");
        let face = abi::FACE_NAMES
            .iter()
            .position(|n| *n == facing)
            .unwrap_or(4) as u32;
        out.joints.push(abi::TemplateJoint {
            pos: ivec(j.get("pos")).unwrap_or_default(),
            face,
            name: text(&j, "name"),
            target: text(&j, "target"),
            pool: text(&j, "pool"),
        });
    }
    Ok(out)
}

/// Writes `parts` as our file, blocks by a palette of the names they use.
pub fn write_json(size: IVec3, parts: &Parts) -> String {
    let mut palette: Vec<&str> = Vec::new();
    let mut blocks = Vec::with_capacity(parts.blocks.len());
    for b in &parts.blocks {
        let i = match palette.iter().position(|p| *p == b.block) {
            Some(i) => i,
            None => {
                palette.push(&b.block);
                palette.len() - 1
            }
        };
        blocks.push(serde_json::json!([b.pos[0], b.pos[1], b.pos[2], i]));
    }
    let v = serde_json::json!({
        "format": FORMAT,
        "size": [size.x, size.y, size.z],
        "palette": palette,
        "blocks": blocks,
        "slots": parts.slots.iter().map(|s| (s.name.clone(), serde_json::Value::String(s.block.clone()))).collect::<serde_json::Map<_, _>>(),
        "entities": parts.entities.iter().map(|e| serde_json::json!({"pos": e.pos, "kind": e.kind, "data": to_hex(&e.data)})).collect::<Vec<_>>(),
        "block_entities": parts.data.iter().map(|d| serde_json::json!({"pos": d.pos, "data": to_hex(&d.data)})).collect::<Vec<_>>(),
        "loot": parts.loot.iter().map(|l| serde_json::json!({"pos": l.pos, "table": l.table})).collect::<Vec<_>>(),
        "joints": parts.joints.iter().map(|j| serde_json::json!({
            "pos": j.pos, "facing": abi::FACE_NAMES[j.face as usize % 6], "name": j.name, "target": j.target, "pool": j.pool,
        })).collect::<Vec<_>>(),
    });
    serde_json::to_string(&v).unwrap_or_default()
}

/// structgen's `.wst`: `WSTR`, version 2, LZ4 around the size, a palette of state names (`@slot`
/// entries its slots), the slots' defaults and every cell's index packed into words, x fastest,
/// then z, then y.
pub fn read_wst(file: &[u8]) -> Result<Parts, String> {
    let bad = || "not a .wst file of version 2".to_string();
    let rest = file.strip_prefix(b"WSTR").ok_or_else(bad)?;
    let (&version, packed) = rest.split_first().ok_or_else(bad)?;
    if version != 2 {
        return Err(bad());
    }
    let body = lz4_flex::decompress_size_prepended(packed).map_err(|e| e.to_string())?;
    let mut r = Reader(&body);
    let size = [r.u16().ok_or_else(bad)?, r.u16().ok_or_else(bad)?, r.u16().ok_or_else(bad)?];
    let n = r.u16().ok_or_else(bad)?;
    let palette: Vec<String> = (0..n)
        .map(|_| r.name())
        .collect::<Option<_>>()
        .ok_or_else(bad)?;
    let n = r.u16().ok_or_else(bad)?;
    let mut out = Parts::default();
    for _ in 0..n {
        let (name, block) = (r.name().ok_or_else(bad)?, r.name().ok_or_else(bad)?);
        out.slots.push(abi::PaletteSlot { name, block });
    }
    let volume = size.iter().map(|&s| s as usize).product::<usize>();
    if volume > 1 << 24 {
        return Err("bigger than 256 x 256 x 256".into());
    }
    let (&bits, words) = r.0.split_first().ok_or_else(bad)?;
    let bits = bits as usize;
    let index = |i: usize| -> Option<usize> {
        if bits == 0 {
            return Some(0);
        }
        let per = 64 / bits;
        let word = u64::from_le_bytes(words.get(i / per * 8..i / per * 8 + 8)?.try_into().ok()?);
        Some((word >> ((i % per) * bits) & ((1u64 << bits) - 1)) as usize)
    };
    let (sx, sz) = (size[0] as usize, size[2] as usize);
    for i in 0..volume {
        let pos = [(i % sx) as i32, (i / (sx * sz)) as i32, (i / sx % sz) as i32];
        let block = palette.get(index(i).ok_or_else(bad)?).ok_or_else(bad)?;
        out.blocks
            .push(abi::TemplateBlock { pos, block: block.clone() });
    }
    Ok(out)
}

/// The unread rest of a `.wst` body.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let (a, b) = self.0.split_at_checked(n)?;
        self.0 = b;
        Some(a)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn name(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_is_clockwise_from_above_and_cells_stay_in_the_box() {
        let size = IVec3::new(3, 1, 2);
        // The north-west corner goes to the north-east once turned a quarter.
        let t = Transform(1);
        assert_eq!(t.size(size), IVec3::new(2, 1, 3));
        assert_eq!(t.cell(size, IVec3::ZERO), IVec3::new(1, 0, 0));
        assert_eq!(t.face(1), 5, "east goes south");
        for bits in 0..8u8 {
            let t = Transform(bits);
            let placed = t.size(size);
            for x in 0..3 {
                for z in 0..2 {
                    let p = t.cell(size, IVec3::new(x, 0, z));
                    assert!(p.cmpge(IVec3::ZERO).all() && p.cmplt(placed).all(), "{bits}: {p}");
                }
            }
        }
        let m = Transform(abi::transform::MIRROR as u8);
        assert_eq!(m.cell(size, IVec3::ZERO), IVec3::new(2, 0, 0));
        assert_eq!(m.face(0), 1, "west becomes east");
        assert_eq!(m.face(4), 4, "north stays");
    }

    #[test]
    fn a_file_reads_back_as_written() {
        let parts = Parts {
            blocks: vec![
                abi::TemplateBlock { pos: [0, 0, 0], block: "base:stone".into() },
                abi::TemplateBlock { pos: [1, 0, 0], block: "@wall:stairs[facing=north]".into() },
            ],
            slots: vec![abi::PaletteSlot { name: "wall".into(), block: "base:oak_planks".into() }],
            entities: vec![abi::TemplateEntity { pos: [0.5, 1.0, 0.5], kind: "base:zombie".into(), data: vec![1, 2, 255] }],
            loot: vec![abi::TemplateLoot { pos: [1, 0, 0], table: "base:chest".into() }],
            data: vec![abi::TemplateData { pos: [0, 0, 0], data: vec![9] }],
            joints: vec![abi::TemplateJoint { pos: [1, 0, 0], face: 5, name: "a".into(), target: "b".into(), pool: "p".into() }],
        };
        let back = read_json(write_json(IVec3::new(2, 1, 1), &parts).as_bytes()).unwrap();
        assert_eq!(back.blocks, parts.blocks);
        assert_eq!(back.slots, parts.slots);
        assert_eq!(back.entities, parts.entities);
        assert_eq!(back.loot, parts.loot);
        assert_eq!(back.data, parts.data);
        assert_eq!(back.joints, parts.joints);
    }

    #[test]
    fn a_wst_file_reads_x_then_z_then_y() {
        // 2 x 1 x 2 of a slot and air, 4 bits an index, one word.
        let mut body = Vec::new();
        for v in [2u16, 1, 2] {
            body.extend(v.to_le_bytes());
        }
        let name = |b: &mut Vec<u8>, s: &str| {
            b.extend((s.len() as u16).to_le_bytes());
            b.extend(s.as_bytes());
        };
        body.extend(2u16.to_le_bytes());
        name(&mut body, "core:air");
        name(&mut body, "@roof/stair[facing=north]");
        body.extend(1u16.to_le_bytes());
        name(&mut body, "roof/stair");
        name(&mut body, "base:spruce_planks_stairs");
        body.push(4);
        body.extend((0b0000_0001_0000_0001u64 << 4).to_le_bytes());
        let mut file = b"WSTR\x02".to_vec();
        file.extend(lz4_flex::compress_prepend_size(&body));
        let parts = read_wst(&file).unwrap();
        let names: Vec<(&[i32; 3], &str)> = parts
            .blocks
            .iter()
            .map(|b| (&b.pos, b.block.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                (&[0, 0, 0], "core:air"),
                (&[1, 0, 0], "@roof/stair[facing=north]"),
                (&[0, 0, 1], "core:air"),
                (&[1, 0, 1], "@roof/stair[facing=north]"),
            ]
        );
        assert_eq!(parts.slots[0].block, "base:spruce_planks_stairs");
    }

    #[test]
    fn slots_read_with_their_shape_and_properties() {
        assert_eq!(parse_slot("base:stone"), None);
        assert_eq!(parse_slot("@ground"), Some(Cell::Ground));
        assert_eq!(
            parse_slot("@roof/stair[facing=north]"),
            Some(Cell::Slot { slot: "roof/stair".into(), shape: String::new(), props: "facing=north".into() })
        );
        assert_eq!(parse_slot("@wall:slab"), Some(Cell::Slot { slot: "wall".into(), shape: "slab".into(), props: String::new() }));
    }
}
