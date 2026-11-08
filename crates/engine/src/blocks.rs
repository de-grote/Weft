//! Blocks as entries (`abi::comp::block`). A block's states are every combination of its
//! properties, each made of the block's parts whose keys fit it, in order, then the edits by tag.
//! Templates (`SHAPES_OF`) make a block for each source first, which edits and patterns then reach
//! like any other.

use std::borrow::Cow;

use rustc_hash::FxHashMap;
use weft_abi::{self as abi, BlockModel, ModId, RenderLayer, block_flags as bf, comp::block as b, data, value_op as vo};

use crate::components::{RawValue, matches};
use crate::content::{AIR, BlockCallbacks, BlockDef, ModelBox, Piece, Prop, Signal, StateInfo};
use crate::entries::{self, Entry, Folded, RawEntry, RawOp};

type Part = (String, Vec<RawOp>);

/// What the blocks came to, in id order (`core:air` first).
pub(crate) struct Blocks {
    pub states: Vec<StateInfo>,
    pub block_defs: Vec<BlockDef>,
    /// Per block, its owner and each state's components.
    pub block_values: Vec<(ModId, Vec<Vec<RawValue>>)>,
    /// The menu each block but air opens, by name.
    pub opens_names: Vec<String>,
    pub block_names: FxHashMap<String, u32>,
    /// What may share a cell with a block though their shapes overlap (a matcher).
    pub shares: Vec<(String, String)>,
    /// Made by templates: (name, source, lang key).
    pub shapes: Vec<(String, String, String)>,
    /// (block, [(texture, its replacement)]): its item's follow.
    pub retextures: Vec<(String, Vec<(String, String)>)>,
    /// Edits by pattern: mod, name, whether they matched anything.
    pub patterns: Vec<(ModId, String, bool)>,
    /// Each state's rules, still text.
    pub rules: Vec<(usize, Vec<abi::BlockRule>)>,
    /// Each state's hazards, still text.
    pub hazards: Vec<(usize, Vec<abi::Hazard>)>,
}

/// Every mod's block entries in load order, frozen. Templates copy their item in `items` for each
/// block they make; `hot` blocks burn what touches them; `liquids` are liquids' blocks; `tex`
/// gives a texture's id.
#[allow(clippy::too_many_arguments)]
pub(crate) fn freeze(
    raw: Vec<RawEntry>,
    items: &mut Folded,
    hot: &[String],
    liquids: &[String],
    read_model: &dyn Fn(&str) -> Option<String>,
    tex: &mut dyn FnMut(&str) -> u32,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) -> Blocks {
    let mut f = entries::fold("block", raw, mod_name, errors);
    for e in &mut f.entries {
        for (list, tag) in [(hot, abi::comp::HOT), (liquids, abi::comp::LIQUIDS)] {
            if list.contains(&e.name) {
                e.parts
                    .push((String::new(), vec![set_op(RawValue::tag(tag))]));
            }
        }
    }
    let (tagged, named): (Vec<RawEntry>, Vec<RawEntry>) = std::mem::take(&mut f.patterns)
        .into_iter()
        .partition(|p| p.name.starts_with('#'));
    let (mut named_hits, mut tag_hits) = (vec![false; named.len()], vec![false; tagged.len()]);
    let mut blocks = f.entries;
    let own: Vec<usize> = blocks.iter().map(|e| e.parts.len()).collect();
    by_name(&mut blocks, &named, &mut named_hits, errors);
    let (made, shapes) = expand_templates(&mut blocks, &own, items, errors);
    let first_made = blocks.len();
    blocks.extend(made);
    for (e, why) in std::mem::take(&mut f.missing) {
        match blocks[first_made..].iter_mut().find(|m| m.name == e.name) {
            Some(m) => entries::edit(m, e, errors),
            None => errors.push(why),
        }
    }
    for (e, why) in std::mem::take(&mut items.missing) {
        match items.entries.iter_mut().find(|i| i.name == e.name) {
            Some(i) => entries::edit(i, e, errors),
            None => items.missing.push((e, why)),
        }
    }
    by_name(&mut blocks[first_made..], &named, &mut named_hits, errors);
    blocks.sort_by(|a, b| a.name.cmp(&b.name));

    let mut out = Blocks {
        states: vec![air()],
        block_defs: vec![BlockDef {
            name: "core:air".into(),
            owner: crate::registry::ENGINE,
            first_state: 0,
            states: 1,
            props: Vec::new(),
            opens: None,
            cb: BlockCallbacks::default(),
            sides: 0,
        }],
        block_values: vec![(crate::registry::ENGINE, vec![Vec::new()])],
        opens_names: Vec::new(),
        block_names: FxHashMap::default(),
        shares: Vec::new(),
        rules: Vec::new(),
        hazards: Vec::new(),
        shapes,
        retextures: Vec::new(),
        patterns: Vec::new(),
    };
    let mut heads: Vec<(usize, String)> = Vec::new();
    for mut e in blocks {
        let index = out.block_defs.len() as u32;
        let first = out.states.len() as u32;
        for &c in &e.cells {
            crate::registry::write_cell(c, first);
        }
        let (props, count) = layout(&e, errors);
        let picks = picks(&e.name, &props, count, &e.parts, errors);
        let retexture: Vec<(String, String)> = e
            .or::<Vec<abi::Retexture>>(b::RETEXTURE, Vec::new())
            .into_iter()
            .map(|r| (r.from, r.to))
            .collect();
        // Its states may each have a model file and textures for it of their own: each read once.
        let mut files: FxHashMap<(String, Vec<(String, String)>), Option<crate::block_model::FileModel>> = FxHashMap::default();
        let mut state_values = Vec::with_capacity(count as usize);
        let mut pieces = Vec::new();
        let mut tagged_here = vec![false; tagged.len()];
        for picked in &picks {
            let mut s = Values::of(&e.parts, picked, &e.name, errors);
            for (p, hit) in tagged.iter().zip(tagged_here.iter_mut()) {
                if s.0
                    .iter()
                    .any(|x| x.schema.is_empty() && matches(&p.name[1..], &x.name))
                {
                    *hit = true;
                    for op in p.ops.iter().filter(|op| !block_only(&op.value.name)) {
                        s.apply(op, &e.name, errors);
                    }
                }
            }
            if s.or(abi::comp::HARDNESS, 1.0f32) < 0.0 && !liquids.contains(&e.name) {
                s.0.push(Cow::Owned(RawValue::tag(abi::comp::UNBREAKABLE)));
            }
            let file_name = s
                .text(b::MODEL_FILE)
                .filter(|f| !f.is_empty())
                .unwrap_or(&e.name);
            let own: Vec<(String, String)> = s
                .or::<Vec<abi::ModelTexture>>(b::MODEL_TEXTURES, Vec::new())
                .into_iter()
                .map(|t| (t.name, t.texture))
                .collect();
            let file = files
                .entry((file_name.to_string(), own))
                .or_insert_with_key(|(name, own)| model_file(name, own, read_model, errors));
            let (info, head, raw, own_rules) = bake_state(&e.name, index, &s, &retexture, file.as_ref(), tex, errors);
            if !own_rules.is_empty() {
                out.rules.push((out.states.len(), own_rules));
            }
            let hazards = s.or::<Vec<abi::Hazard>>(b::HAZARDS, Vec::new());
            if !hazards.is_empty() {
                out.hazards.push((out.states.len(), hazards));
            }
            if !head.is_empty() {
                heads.push((out.states.len(), head));
            }
            if !raw.is_empty() {
                pieces.push((out.states.len(), raw));
            }
            out.states.push(info);
            state_values.push(
                s.0.into_iter()
                    .filter(|v| !v.name.starts_with("block."))
                    .map(Cow::into_owned)
                    .collect(),
            );
        }
        // On its sides: which ones each state lies on, from its `west`..`south` properties.
        let on_sides = state_values
            .first()
            .is_some_and(|v: &Vec<RawValue>| v.iter().any(|x| x.name == abi::comp::block::ON_SIDES));
        let side_props: Vec<(u8, &Prop)> = props
            .iter()
            .filter(|_| on_sides)
            .filter_map(|p| {
                let f = abi::FACE_NAMES.iter().position(|n| *n == p.name)?;
                (p.values.len() == 2 && p.values.iter().any(|v| v == "true")).then_some((f as u8, p))
            })
            .collect();
        let sides = side_props.iter().fold(0u8, |m, (f, _)| m | 1 << f);
        for (i, st) in out.states[first as usize..].iter_mut().enumerate() {
            st.sides = side_props
                .iter()
                .filter(|(_, p)| p.values[(i as u32 / p.stride) as usize % 2] == "true")
                .fold(0, |m, (f, _)| m | 1 << f);
        }
        // What a state gives, from its `power` property: two values are off and full.
        let power = props
            .iter()
            .find(|p| p.name == "power")
            .map(|p| (p.values.len() as u32, p.stride));
        for (i, st) in out.states[first as usize..].iter_mut().enumerate() {
            match power {
                Some((n, each)) => {
                    let at = (i as u32 / each) % n;
                    st.signal.levels = n.min(16) as u8;
                    st.signal.stride = each;
                    st.signal.level = if n == 2 { at as u8 * 15 } else { at.min(15) as u8 };
                }
                None if st.signal.kind == abi::signal_kind::SWITCH => st.signal.level = 15,
                None => {}
            }
        }
        // What an edit by tag says of the block as a whole, if a state of it has the tag.
        for (i, p) in tagged.iter().enumerate().filter(|(i, _)| tagged_here[*i]) {
            tag_hits[i] = true;
            for op in p.ops.iter().filter(|op| block_only(&op.value.name)) {
                e.apply(op, p.owner, errors);
            }
        }
        out.block_values.push((e.owner, state_values));
        out.opens_names
            .push(e.text(b::OPENS).unwrap_or_default().to_string());
        if let Some(shares) = e.text(b::SHARES_CELL).filter(|t| !t.is_empty()) {
            out.shares.push((e.name.clone(), shares.to_string()));
        }
        if !retexture.is_empty() {
            out.retextures.push((e.name.clone(), retexture));
        }
        let cb = BlockCallbacks {
            on_use: e.callback(b::ON_USE),
            on_neighbor: e.callback(b::ON_NEIGHBOR),
            on_scheduled: e.callback(b::ON_SCHEDULED),
            on_carried: e.callback(b::ON_CARRIED),
            signal_read: e.callback(b::SIGNAL_READ),
            placement: e.callback(b::PLACEMENT),
            merge: e.callback(b::MERGE),
            on_grow: e.callback(b::ON_GROW),
            on_signal: e.callback(b::ON_SIGNAL),
            signal_gate: e.callback(b::SIGNAL_GATE),
        };
        let def = BlockDef { name: e.name, owner: e.owner, first_state: first, states: count, props, cb, opens: None, sides };
        // A piece's properties are of the state it is in.
        for (state, raw) in pieces {
            let to = |text: &str| def.changed_text(state as u32, text);
            for (bounds, left, alone) in raw {
                match (to(&left), to(&alone)) {
                    (Ok(left), Ok(alone)) => {
                        // What lies on sides is gone with its last one.
                        let left = if sides != 0 && out.states[left as usize].sides == 0 { AIR } else { left };
                        out.states[state].pieces.push(Piece { bounds, left, alone });
                    }
                    (Err(e), _) | (_, Err(e)) => errors.push(format!("{} piece: {e}", def.name)),
                }
            }
        }
        out.block_defs.push(def);
    }
    out.block_names = out
        .block_defs
        .iter()
        .enumerate()
        .map(|(i, b)| (b.name.clone(), i as u32))
        .collect();
    for (state, name) in heads {
        out.states[state].signal.head = out.block_names.get(&name).copied();
    }
    let hits = named_hits.into_iter().chain(tag_hits);
    out.patterns = named
        .into_iter()
        .chain(tagged)
        .zip(hits)
        .map(|(p, hit)| (p.owner, p.name, hit))
        .collect();
    out
}

fn set_op(value: RawValue) -> RawOp {
    RawOp { op: abi::value_op::SET, value, anchor: String::new() }
}

/// Edits by name (`*` in it, `name[prop=value]`) onto the blocks they name, as parts; without
/// properties their operations go on the block itself too.
fn by_name(blocks: &mut [Entry], named: &[RawEntry], hits: &mut [bool], errors: &mut Vec<String>) {
    let index: FxHashMap<String, usize> = blocks
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.clone(), i))
        .collect();
    for (p, hit) in named.iter().zip(hits.iter_mut()) {
        let (name, key) = entries::split_pattern(&p.name);
        let key = key.unwrap_or_default().trim();
        let found: Vec<usize> = match name.contains('*') {
            true => (0..blocks.len())
                .filter(|&i| matches(name, &blocks[i].name))
                .collect(),
            false => index.get(name).copied().into_iter().collect(),
        };
        for i in found {
            *hit = true;
            let e = &mut blocks[i];
            if key.is_empty() {
                for op in p.ops.iter().filter(|op| block_only(&op.value.name)) {
                    e.apply(op, p.owner, errors);
                }
            }
            e.parts.push((key.to_string(), p.ops.clone()));
        }
    }
}

/// A block's properties, each with the states between two of its values, and how many states
/// that makes: every combination, the last property changing fastest.
fn layout(e: &Entry, errors: &mut Vec<String>) -> (Vec<Prop>, u32) {
    let mut props: Vec<Prop> = Vec::new();
    let mut stride = 1u32;
    for p in e
        .or::<Vec<abi::BlockProp>>(b::PROPS, Vec::new())
        .into_iter()
        .rev()
    {
        if p.values.is_empty() || props.iter().any(|q| q.name == p.name) {
            errors.push(format!("block {}: property {} has no values or is there twice", e.name, p.name));
            continue;
        }
        let n = p.values.len() as u32;
        props.push(Prop { name: p.name, values: p.values, stride });
        stride *= n;
    }
    props.reverse();
    (props, stride)
}

/// For each state, the parts that go on it, in order.
fn picks(name: &str, props: &[Prop], count: u32, parts: &[Part], errors: &mut Vec<String>) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); count as usize];
    for (i, (key, _)) in parts.iter().enumerate() {
        match picked(props, count, key) {
            Ok(states) => states
                .into_iter()
                .for_each(|s| out[s as usize].push(i as u32)),
            Err(err) => errors.push(format!("block {name}: {err}")),
        }
    }
    out
}

/// The states (from 0) a key `prop=value,..` picks: those with every value it names.
fn picked(props: &[Prop], count: u32, key: &str) -> Result<Vec<u32>, String> {
    let mut fixed: Vec<(&Prop, u32)> = Vec::new();
    for (k, v) in crate::content::prop_pairs(key)? {
        let Some(p) = props.iter().find(|p| p.name == k) else {
            let known: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
            return Err(format!("no property {k} (it has: {})", if known.is_empty() { "none".into() } else { known.join(", ") }));
        };
        let Some(i) = p.values.iter().position(|x| x == v) else {
            return Err(format!("property {k} has no value {v} (values: {})", p.values.join(", ")));
        };
        fixed.push((p, i as u32));
    }
    let fits = |s: u32| {
        fixed
            .iter()
            .all(|(p, v)| (s / p.stride) % p.values.len() as u32 == *v)
    };
    // Every property named: one state, worked out.
    if props
        .iter()
        .all(|p| fixed.iter().any(|f| std::ptr::eq(f.0, p)))
    {
        let s = fixed.iter().map(|(p, v)| v * p.stride).sum();
        return Ok(if fits(s) { vec![s] } else { Vec::new() });
    }
    Ok((0..count).filter(|&s| fits(s)).collect())
}

/// The key naming every property's value in state `s` of a layout.
fn full_key(props: &[Prop], s: u32) -> String {
    props
        .iter()
        .map(|p| format!("{}={}", p.name, p.values[((s / p.stride) % p.values.len() as u32) as usize]))
        .collect::<Vec<_>>()
        .join(",")
}

/// What only a block as a whole has: its states leave it out.
const BLOCK_ONLY: [&str; 15] = [
    b::PROPS,
    b::ON_USE,
    b::ON_NEIGHBOR,
    b::ON_SCHEDULED,
    b::ON_CARRIED,
    b::ON_GROW,
    b::PLACEMENT,
    b::MERGE,
    b::ON_SIGNAL,
    b::SIGNAL_GATE,
    b::SIGNAL_READ,
    b::OPENS,
    b::SHARES_CELL,
    b::SHAPES_OF,
    b::RETEXTURE,
];

pub(crate) fn block_only(name: &str) -> bool {
    name.starts_with("block.") && BLOCK_ONLY.contains(&name)
}

/// One state's values, those its parts set borrowed from them.
struct Values<'a>(Vec<Cow<'a, RawValue>>);

impl<'a> Values<'a> {
    /// The parts `picked` of a block, in order.
    fn of(parts: &'a [Part], picked: &[u32], what: &str, errors: &mut Vec<String>) -> Values<'a> {
        let mut v = Values(Vec::new());
        for &i in picked {
            for op in parts[i as usize]
                .1
                .iter()
                .filter(|op| !block_only(&op.value.name))
            {
                v.apply(op, what, errors);
            }
        }
        v
    }

    fn apply(&mut self, op: &'a RawOp, what: &str, errors: &mut Vec<String>) {
        let at = self.0.iter().position(|v| v.name == op.value.name);
        match (op.op, at) {
            (vo::SET, Some(i)) => self.0[i] = Cow::Borrowed(&op.value),
            (vo::SET, None) => self.0.push(Cow::Borrowed(&op.value)),
            (vo::UNSET, Some(i)) => drop(self.0.remove(i)),
            (vo::UNSET, None) => {}
            _ => {
                // A list operation, on its own copy of the list.
                let mut one: Vec<RawValue> = at
                    .map(|i| self.0.remove(i).into_owned())
                    .into_iter()
                    .collect();
                entries::apply(&mut one, op, what, errors);
                self.0.extend(one.into_iter().map(Cow::Owned));
            }
        }
    }

    fn value(&self, name: &str) -> Option<&RawValue> {
        self.0.iter().map(|v| &**v).find(|v| v.name == name)
    }

    fn has(&self, name: &str) -> bool {
        self.value(name).is_some()
    }

    fn text(&self, name: &str) -> Option<&str> {
        self.value(name).and_then(|v| v.text.as_deref())
    }

    fn get<T: data::Data>(&self, name: &str) -> Option<T> {
        self.value(name)
            .filter(|v| v.schema == entries::schema::<T>())
            .and_then(|v| data::from_bytes(&v.bytes))
    }

    fn or<T: data::Data>(&self, name: &str, default: T) -> T {
        self.get(name).unwrap_or(default)
    }

    /// Value `name` of `schema` read in place by `f`.
    fn read<'v, T>(&'v self, name: &str, schema: &str, f: impl FnOnce(&mut &'v [u8]) -> Option<T>) -> Option<T> {
        self.value(name)
            .filter(|v| v.schema == schema)
            .and_then(|v| f(&mut v.bytes.as_slice()))
    }

    fn owned(self) -> Entry {
        Entry { values: self.0.into_iter().map(Cow::into_owned).collect(), ..Entry::default() }
    }
}

/// The first state of a block, from `parts`.
fn first_state<'a>(e: &Entry, parts: &'a [Part]) -> Values<'a> {
    let (props, _) = layout(e, &mut Vec::new());
    let picked: Vec<u32> = (0..parts.len() as u32)
        .filter(|&i| picked(&props, 1, &parts[i as usize].0).is_ok_and(|s| s.contains(&0)))
        .collect();
    Values::of(parts, &picked, &e.name, &mut Vec::new())
}

fn read<T: data::Data>(r: &mut &[u8]) -> Option<T> {
    T::read(r)
}

/// A packed `str` off the front of `r`, in place.
fn name<'a>(r: &mut &'a [u8]) -> Option<&'a str> {
    let n = read::<u32>(r)? as usize;
    let (text, rest) = (r.get(..n)?, r.get(n..)?);
    *r = rest;
    std::str::from_utf8(text).ok()
}

fn names<'a>(r: &mut &'a [u8]) -> Option<[&'a str; 6]> {
    let mut out = [""; 6];
    for t in &mut out {
        *t = name(r)?;
    }
    Some(out)
}

fn floats<const N: usize>(r: &mut &[u8]) -> Option<[f32; N]> {
    let mut out = [0.0; N];
    for v in &mut out {
        *v = read(r)?;
    }
    Some(out)
}

/// An `abi::ModelBox` read in place.
struct BoxView<'a> {
    min: [f32; 3],
    max: [f32; 3],
    textures: [&'a str; 6],
    uv: [[f32; 4]; 6],
    turn: [u8; 6],
    tilt: abi::BoxTilt,
}

impl<'a> BoxView<'a> {
    fn read(r: &mut &'a [u8]) -> Option<BoxView<'a>> {
        let (min, max, textures) = (floats(r)?, floats(r)?, names(r)?);
        let mut uv = [[0.0; 4]; 6];
        for f in &mut uv {
            *f = floats(r)?;
        }
        let mut turn = [0; 6];
        for t in &mut turn {
            *t = read(r)?;
        }
        let tilt = abi::BoxTilt { origin: floats(r)?, axis: floats(r)?, degrees: read(r)? };
        Some(BoxView { min, max, textures, uv, turn, tilt })
    }
}

fn model_of(v: u32) -> BlockModel {
    match v {
        0 => BlockModel::None,
        2 => BlockModel::Cross,
        4 => BlockModel::Boxes,
        8 => BlockModel::Wire,
        _ => BlockModel::Cube,
    }
}

fn layer_of(v: u32) -> RenderLayer {
    match v {
        1 => RenderLayer::Cutout,
        2 => RenderLayer::Translucent,
        _ => RenderLayer::Opaque,
    }
}

/// `<ns>:block/<name>` on every face.
fn own_textures(block: &str) -> [String; 6] {
    let (ns, path) = block.split_once(':').unwrap_or(("core", block));
    std::array::from_fn(|_| format!("{ns}:block/{path}"))
}

/// A state's model file (`models/block/<name>.json` of its namespace's mod, the block's name or
/// `MODEL_FILE`), `own` its textures for the file's `#names`: read here, so collision and
/// selection follow it on the server and every client alike.
fn model_file(
    file: &str,
    own: &[(String, String)],
    read: &dyn Fn(&str) -> Option<String>,
    errors: &mut Vec<String>,
) -> Option<crate::block_model::FileModel> {
    let (ns, path) = file.split_once(':')?;
    crate::block_model::load_with(&format!("{ns}:block/{path}"), read, own).unwrap_or_else(|err| {
        errors.push(err);
        None
    })
}

/// One state as its values say (absent: the defaults), its textures renamed by `retexture` and
/// shaped by its block's model file. Also gives its piston head's name and its pieces, their
/// properties still text.
fn bake_state(
    name: &str,
    block: u32,
    s: &Values,
    retexture: &[(String, String)],
    file: Option<&crate::block_model::FileModel>,
    tex: &mut dyn FnMut(&str) -> u32,
    errors: &mut Vec<String>,
) -> (StateInfo, String, Vec<([f32; 6], String, String)>, Vec<abi::BlockRule>) {
    let mut model = model_of(s.or(b::MODEL, BlockModel::Cube as u32));
    let own_layer = if model == BlockModel::Boxes { RenderLayer::Cutout } else { RenderLayer::Opaque };
    let mut layer = layer_of(s.or(b::LAYER, own_layer as u32));
    // Names read where they are: a state's are mostly the same as many others'.
    let own;
    let mut textures: [&str; 6] = match s.read(b::TEXTURES, entries::schema::<[String; 6]>(), names) {
        Some(t) => t,
        None => {
            own = own_textures(name);
            std::array::from_fn(|i| own[i].as_str())
        }
    };
    let mut boxes: Vec<BoxView> = s
        .read(b::BOXES, entries::schema::<Vec<abi::ModelBox>>(), |r| (0..read::<u32>(r)?).map(|_| BoxView::read(r)).collect())
        .unwrap_or_default();
    for t in textures
        .iter_mut()
        .chain(boxes.iter_mut().flat_map(|b| b.textures.iter_mut()))
    {
        if let Some((_, to)) = retexture.iter().find(|r| r.0 == *t) {
            *t = to;
        }
    }
    // A model file: its elements become the boxes (faces left to the block keep the state's
    // textures), a cube parent sets the face textures.
    let mut opacity = s.get::<u8>(b::LIGHT_OPACITY);
    if let Some(m) = file {
        if let Some(cube) = &m.cube {
            for (i, t) in cube.iter().enumerate().filter(|(_, t)| !t.is_empty()) {
                textures[i] = t;
            }
        }
        if !m.boxes.is_empty() {
            model = BlockModel::Boxes;
            opacity = Some(0);
            if layer == RenderLayer::Opaque {
                layer = RenderLayer::Cutout;
            }
            boxes = m
                .boxes
                .iter()
                .map(|x| BoxView {
                    min: x.min,
                    max: x.max,
                    textures: std::array::from_fn(|i| x.textures[i].as_str()),
                    uv: x.uv,
                    turn: x.turn,
                    tilt: x.tilt,
                })
                .collect();
        }
    }
    let mut flags = crate::tag_bits::block::TAGS
        .iter()
        .filter(|f| s.has(f.0))
        .fold(if s.has(b::NOT_SOLID) { 0 } else { bf::SOLID }, |all, f| all | f.1);
    if model == BlockModel::Cube && layer == RenderLayer::Opaque {
        flags |= bf::OPAQUE;
    }
    let opacity = opacity.unwrap_or(if flags & bf::OPAQUE != 0 { 15 } else { 0 });
    let filter: [u8; 3] = s.or(b::LIGHT_FILTER, [0; 3]);
    let turn = match s.get::<u8>(b::TURN) {
        Some(i) => weft_abi::Turn::from_index(i).unwrap_or_else(|| {
            errors.push(format!("{name}: turn {i} is none of the 24"));
            weft_abi::Turn::NONE
        }),
        None => weft_abi::Turn::NONE,
    };
    let sixteenths = |name: &str| -> Vec<[f32; 6]> {
        s.or::<Vec<[f32; 6]>>(name, Vec::new())
            .into_iter()
            .map(|x| {
                let (a, b) = turn.bounds([x[0], x[1], x[2]], [x[3], x[4], x[5]], 16.0);
                [a[0], a[1], a[2], b[0], b[1], b[2]].map(|v| v / 16.0)
            })
            .collect()
    };
    let (hitbox, outline) = (sixteenths(b::HITBOX), sixteenths(b::OUTLINE));
    let rules = s.or::<Vec<abi::BlockRule>>(b::RULES, Vec::new());
    let pieces = s
        .or::<Vec<abi::Piece>>(b::PIECES, Vec::new())
        .into_iter()
        .map(|p| {
            let (a, b) = turn.bounds(p.min, p.max, 16.0);
            ([a[0], a[1], a[2], b[0], b[1], b[2]].map(|v| v / 16.0), p.left, p.alone)
        })
        .collect();
    let signal: abi::SignalDesc = s.get(b::SIGNAL).unwrap_or(abi::SignalDesc::DEFAULT);
    let mut boxes: Vec<ModelBox> = boxes
        .iter()
        .map(|x| ModelBox {
            turn: x.turn,
            tilt: weft_abi::BoxTilt { origin: x.tilt.origin.map(|v| v / 16.0), ..x.tilt },
            min: x.min.map(|v| v / 16.0),
            max: x.max.map(|v| v / 16.0),
            textures: std::array::from_fn(|i| tex(if x.textures[i].is_empty() { textures[i] } else { x.textures[i] })),
            uv: x.uv,
        })
        .map(|b| if turn == weft_abi::Turn::NONE { b } else { b.turned(turn) })
        .collect();
    boxes.shrink_to_fit();
    let of_model = || -> Vec<[f32; 6]> {
        boxes
            .iter()
            .map(|b| [b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]])
            .collect()
    };
    for bx in &hitbox {
        if bx.iter().any(|v| !(-1.0..=2.0).contains(v)) {
            errors
                .push(format!("{name}: hitbox {:?} reaches more than a block out of its cell (it is in sixteenths)", bx.map(|v| v * 16.0)));
        }
    }
    // What stops a walker: nothing at all unless it is solid, decided here once for whoever asks.
    let collision = if flags & bf::SOLID == 0 {
        Vec::new()
    } else if !hitbox.is_empty() {
        hitbox.clone()
    } else if model == BlockModel::Boxes {
        of_model()
    } else {
        vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]
    };
    let selection: Vec<[f32; 6]> = if !hitbox.is_empty() {
        hitbox
    } else if model == BlockModel::Cube {
        vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]
    } else if !boxes.is_empty() {
        of_model()
    } else if model == BlockModel::Cross {
        vec![[0.125, 0.0, 0.125, 0.875, 0.8, 0.875]]
    } else {
        vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]
    };
    let own: [u32; 6] = if model == BlockModel::None { [0; 6] } else { std::array::from_fn(|i| tex(textures[i])) };
    let info = StateInfo {
        liquid_walls: match s.or(b::LIQUID_WALLS, u8::MAX) {
            u8::MAX => u8::MAX,
            walls => turn.faces(walls),
        },
        block,
        turn,
        sides: 0,
        flags,
        // Light, tint, hardness, friction, drag and support come from components.
        light: [0; 3],
        absorb: std::array::from_fn(|i| opacity.saturating_add(filter[i])),
        opacity,
        tint: 0,
        tint_faces: 0x3F,
        layer,
        level: 0,
        model,
        signal: Signal {
            kind: signal.kind,
            inputs: turn.faces(signal.inputs as u8),
            locks: turn.faces(signal.locks as u8),
            watches: turn.faces(signal.watches as u8),
            gives: turn.faces(signal.gives as u8),
            strong: turn.faces(signal.strong as u8),
            front: if signal.front < 6 { turn.face(signal.front as usize) as u8 } else { 6 },
            delay: signal.delay.min(31) as u8,
            flags: signal.flags as u8,
            ..Signal::default()
        },
        // A box model's faces default to these (the particles of breaking it show them too).
        textures: crate::content::turned_faces(turn, own),
        turns: crate::content::turned_pictures(turn, s.or(b::TURNS, [0u8; 6]).map(|t| t % 4)),
        boxes,
        collision,
        selection,
        outline,
        hardness: 1.0,
        friction: 0.6,
        drag: 1.0,
        conductor: false,
        liquid: None,
        spread: None,
        effects: [crate::content::Fx::NONE; 6],
        pieces: Vec::new(),
        rules: Vec::new(),
        hazards: Vec::new(),
    };
    (info, signal.head, pieces, rules)
}

fn air() -> StateInfo {
    StateInfo {
        turn: weft_abi::Turn::NONE,
        sides: 0,
        liquid_walls: u8::MAX,
        signal: Signal::default(),
        block: 0,
        flags: bf::REPLACEABLE | bf::NOT_TARGETABLE,
        light: [0; 3],
        absorb: [0; 3],
        opacity: 0,
        tint: 0,
        tint_faces: 0,
        layer: RenderLayer::Opaque,
        level: 0,
        model: BlockModel::None,
        textures: [0; 6],
        turns: [0; 6],
        boxes: Vec::new(),
        collision: Vec::new(),
        selection: Vec::new(),
        outline: Vec::new(),
        hardness: -1.0,
        friction: 0.6,
        drag: 1.0,
        conductor: false,
        liquid: None,
        spread: None,
        effects: [crate::content::Fx::NONE; 6],
        pieces: Vec::new(),
        rules: Vec::new(),
        hazards: Vec::new(),
    }
}

/// Is it what a block should be counted for: a value, or a tag saying how it is mined. Other tags
/// say what it is, which a stair of it isn't.
fn carries_over(v: &RawValue) -> bool {
    // Its steps are states of that block, which a stair hasn't got.
    if v.name.starts_with("block.") {
        return false;
    }
    !v.schema.is_empty() || !v.bytes.is_empty() || v.text.is_some() || v.name.starts_with("mineable/")
}

/// The blocks templates make (`Registry::shapes_of`), of every mod's, each template gone from
/// the lists, each with a copy of the template's item where that was. A source is a full cube
/// by its own registration, found by its name or a tag edits by name may have given it; a made
/// block shows its textures and is mined like it. `own` is how many parts each block had before
/// the edits by name. Gives the blocks and (name, source's name, lang key) of each.
fn expand_templates(
    blocks: &mut Vec<Entry>,
    own: &[usize],
    items: &mut Folded,
    errors: &mut Vec<String>,
) -> (Vec<Entry>, Vec<(String, String, String)>) {
    let mut templates: Vec<usize> = (0..blocks.len())
        .filter(|&i| blocks[i].has(b::SHAPES_OF))
        .collect();
    if templates.is_empty() {
        return (Vec::new(), Vec::new());
    }
    templates.sort_by(|&a, &b| blocks[a].name.cmp(&blocks[b].name));
    // What any template takes; what it leaves out (`!#tag`) is checked per template.
    let words: Vec<String> = templates
        .iter()
        .flat_map(|&t| {
            let of: abi::ShapesOf = blocks[t].or(b::SHAPES_OF, abi::ShapesOf::default());
            abi::matcher_terms(&of.sources)
                .filter(|w| !w.starts_with('!'))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    // The first state of each block a template may take, by its own registration, and its tags
    // with what edits by name gave it.
    let firsts: Vec<Option<(Values, Vec<&str>)>> = blocks
        .iter()
        .zip(own)
        .map(|(e, &own)| {
            let named = |w: &String| match w.strip_prefix('#') {
                Some(pattern) => e
                    .parts
                    .iter()
                    .flat_map(|p| &p.1)
                    .any(|op| op.value.schema.is_empty() && matches(pattern, &op.value.name)),
                None => *w == e.name,
            };
            if e.has(b::SHAPES_OF) || !words.iter().any(named) {
                return None;
            }
            let tags = first_state(e, &e.parts)
                .0
                .into_iter()
                .filter_map(|v| match v {
                    Cow::Borrowed(v) if v.schema.is_empty() => Some(v.name.as_str()),
                    _ => None,
                })
                .collect();
            Some((first_state(e, &e.parts[..own]), tags))
        })
        .collect();
    let mut made: Vec<Entry> = Vec::new();
    // By the template's item.
    let mut made_items: FxHashMap<String, Vec<Entry>> = FxHashMap::default();
    let mut source_tags: Vec<(usize, String)> = Vec::new();
    let mut shapes = Vec::new();
    for &t in &templates {
        let template = &blocks[t];
        let of: abi::ShapesOf = template.or(b::SHAPES_OF, abi::ShapesOf::default());
        let short = template
            .name
            .split_once(':')
            .map_or(template.name.as_str(), |x| x.1);
        let item = items
            .entries
            .iter()
            .find(|i| i.text(abi::comp::item::BLOCK) == Some(template.name.as_str()));
        let (props, count) = layout(template, errors);
        let picks = picks(&template.name, &props, count, &template.parts, errors);
        let states: Vec<Vec<RawValue>> = picks
            .iter()
            .map(|p| {
                Values::of(&template.parts, p, &template.name, errors)
                    .owned()
                    .values
            })
            .collect();
        for (s, source) in blocks.iter().enumerate() {
            let Some((first, tags)) = &firsts[s] else { continue };
            let fits = |w: &str| match w.strip_prefix('#') {
                Some(pattern) => tags.iter().any(|t| matches(pattern, t)),
                None => source.name == w,
            };
            let terms: Vec<&str> = abi::matcher_terms(&of.sources).collect();
            let taken =
                terms.iter().any(|w| !w.starts_with('!') && fits(w)) && !terms.iter().any(|w| w.strip_prefix('!').is_some_and(fits));
            if model_of(first.or(b::MODEL, BlockModel::Cube as u32)) != BlockModel::Cube || !taken {
                continue;
            }
            let name = format!("{}{}", source.name, of.suffix);
            // Two mods asking for the same, or a mod's own block by that name, are the one.
            if made.iter().any(|m| m.name == name) || blocks.iter().any(|b| b.name == name) {
                continue;
            }
            let id = source.name.replace(':', ".");
            source_tags.push((s, format!("{short}_of/{id}")));
            let mut textures: [String; 6] = first
                .get(b::TEXTURES)
                .unwrap_or_else(|| own_textures(&source.name));
            for r in source.or::<Vec<abi::Retexture>>(b::RETEXTURE, Vec::new()) {
                textures
                    .iter_mut()
                    .filter(|t| **t == r.from)
                    .for_each(|t| *t = r.to.clone());
            }
            let of_source = |v: RawValue| of_source(v, &source.name, &textures);
            let parts: Vec<Part> = states
                .iter()
                .enumerate()
                .map(|(i, own)| {
                    let v = shaped(own.clone(), first, &textures, &format!("{short}/{id}"));
                    (full_key(&props, i as u32), v.into_iter().map(of_source).map(set_op).collect())
                })
                .collect();
            let mut values: Vec<RawValue> = template.values.iter().cloned().map(of_source).collect();
            values.retain(|v| v.name != b::SHAPES_OF);
            shapes.push((name.clone(), source.name.clone(), format!("shape.{short}")));
            if let Some(i) = item {
                let mut it = Entry { name: name.clone(), cells: Vec::new(), ..i.clone() };
                it.set(abi::comp::item::BLOCK, name.clone());
                it.set(abi::comp::item::TEXTURE, String::new());
                made_items.entry(i.name.clone()).or_default().push(it);
            }
            made.push(Entry { name, owner: template.owner, cells: Vec::new(), values, callers: template.callers.clone(), parts });
        }
    }
    drop(firsts);
    for (s, tag) in source_tags {
        blocks[s]
            .parts
            .push((String::new(), vec![set_op(RawValue::tag(&tag))]));
    }
    let names: Vec<String> = templates.iter().map(|&t| blocks[t].name.clone()).collect();
    blocks.retain(|e| !e.has(b::SHAPES_OF));
    items.entries = std::mem::take(&mut items.entries)
        .into_iter()
        .flat_map(|i| match made_items.remove(&i.name) {
            Some(list) => list,
            None if names
                .iter()
                .any(|n| i.text(abi::comp::item::BLOCK) == Some(n.as_str())) =>
            {
                Vec::new()
            }
            None => vec![i],
        })
        .collect();
    (made, shapes)
}

/// `v` of a block made of `source` (its name) with `{source}` in its texts as that name and
/// `{source.side}` (`top`, `bottom`, or a face: `north`..) as its texture there. Of texts with
/// alternatives (`{source.side}_inner | {source.top}`) the first texture that exists is taken.
fn of_source(v: RawValue, source: &str, textures: &[String; 6]) -> RawValue {
    if !v.bytes.windows(7).any(|w| w == b"{source") {
        return v;
    }
    let face = |name: &str| match name {
        "side" => Some(4),
        "top" => Some(3),
        "bottom" => Some(2),
        n => abi::FACE_NAMES.iter().position(|f| *f == n),
    };
    let one = |t: &str| -> String {
        let mut out = t.trim().replace("{source}", source);
        while let Some(at) = out.find("{source.") {
            let Some(end) = out[at..].find('}') else { break };
            let tex = face(&out[at + 8..at + end])
                .map_or("", |f| textures[f].as_str())
                .to_string();
            out.replace_range(at..at + end + 1, &tex);
        }
        out
    };
    let mut f = |t: &str| -> String {
        if !t.contains("{source") {
            return t.to_string();
        }
        let all: Vec<String> = t.split('|').map(one).collect();
        let exists = |n: &&String| crate::modloader::asset_exists(n, "textures", "png");
        all.iter()
            .find(exists)
            .or(all.last())
            .cloned()
            .unwrap_or_default()
    };
    let bytes = crate::components::map_texts(&v.schema, &v.bytes, &mut f).unwrap_or(v.bytes);
    let text = v
        .text
        .as_ref()
        .map(|_| abi::data::from_bytes::<String>(&bytes).unwrap_or_default());
    RawValue { bytes, text, ..v }
}

/// A template's state `own` made of `source`'s first state: its textures and light filter; for a
/// cube its look and flags too, for another shape a translucent source's layer; its values (but
/// how it grows) unless the template sets them; and the tag `tag` naming the source.
fn shaped(own: Vec<RawValue>, source: &Values, textures: &[String; 6], tag: &str) -> Vec<RawValue> {
    let set = |own: &mut Vec<RawValue>, name: &str, v: Option<&RawValue>| {
        own.retain(|x| x.name != name);
        own.extend(v.cloned());
    };
    let mut e = Entry { values: own, ..Entry::default() };
    e.set(b::TEXTURES, textures.clone());
    let mut own = e.values;
    set(&mut own, b::LIGHT_FILTER, source.value(b::LIGHT_FILTER));
    let model = model_of(Entry { values: own.clone(), ..Entry::default() }.or(b::MODEL, BlockModel::Cube as u32));
    if model == BlockModel::Cube {
        for name in [b::LAYER, b::LIGHT_OPACITY, b::NOT_SOLID]
            .into_iter()
            .chain(crate::tag_bits::block::TAGS.iter().map(|f| f.0))
        {
            set(&mut own, name, source.value(name));
        }
    } else if layer_of(source.or(b::LAYER, 0)) == RenderLayer::Translucent {
        set(&mut own, b::LAYER, source.value(b::LAYER));
        if source.has(b::CULL_SAME) {
            set(&mut own, b::CULL_SAME, source.value(b::CULL_SAME));
        }
    }
    let carried: Vec<RawValue> = source
        .0
        .iter()
        .filter(|v| carries_over(v) && !own.iter().any(|o| o.name == v.name))
        .map(|v| v.as_ref().clone())
        .collect();
    own.extend(carried);
    own.push(RawValue::tag(tag));
    own
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_pick_states() {
        let prop = |name: &str, values: &[&str], stride| Prop {
            name: name.into(),
            values: values.iter().map(|v| v.to_string()).collect(),
            stride,
        };
        let props = [prop("facing", &["north", "east", "south", "west"], 2), prop("lit", &["false", "true"], 1)];
        assert_eq!(picked(&props, 8, "").unwrap().len(), 8);
        assert_eq!(picked(&props, 8, "lit=true").unwrap(), [1, 3, 5, 7]);
        assert_eq!(picked(&props, 8, " lit = true , facing=south").unwrap(), [5], "in any order, the last property fastest");
        assert_eq!(picked(&props, 8, "lit=true,lit=false").unwrap(), Vec::<u32>::new());
        assert!(picked(&props, 8, "age=1").is_err() && picked(&props, 8, "lit=maybe").is_err());
        assert_eq!(full_key(&props, 5), "facing=south,lit=true");
    }

    #[test]
    fn values_of_the_source() {
        let textures: [String; 6] = std::array::from_fn(|i| format!("m:block/log{}", if i / 2 == 1 { "_top" } else { "" }));
        let mut e = Entry::default();
        e.set(b::MODEL_TEXTURES, vec![abi::ModelTexture { name: "inside".into(), texture: "{source.side}_none | {source.top}".into() }]);
        e.set(abi::comp::item::BLOCK, "{source}".to_string());
        let [list, name] = [&e.values[0], &e.values[1]].map(|v| of_source(v.clone(), "m:log", &textures));
        let list: Vec<abi::ModelTexture> = abi::data::from_bytes(&list.bytes).unwrap();
        // No such texture: the next one.
        assert_eq!(list[0].texture, "m:block/log_top");
        assert_eq!(name.text.as_deref(), Some("m:log"));
    }
}
