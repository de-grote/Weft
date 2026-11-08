//! Components: typed values attached to content entries (block states, items, entity kinds,
//! biomes, dimensions, weathers, game modes ...), one namespace for every mod. A tag is a component without a value.
//! Names without a prefix are the common vocabulary (`plant_soil`, `fuel`, `ingots/iron`),
//! `modid:` ones are private to a mod.
//!
//! At freeze every name gets an id (sorted, so ids only depend on the set of names) and each entry
//! kind becomes a table: one bitset row per entry (has component) and one dense column per
//! component that carries values on that kind. A check is one load and a bit test; mods read the
//! same tables directly through `abi::ComponentTable`.

use rustc_hash::FxHashMap;
use weft_abi::{self as abi, Str, entry_kind as ek};

pub const BLOCK: usize = ek::BLOCK as usize;
pub const ITEM: usize = ek::ITEM as usize;
pub const ENTITY: usize = ek::ENTITY as usize;
pub const BIOME: usize = ek::BIOME as usize;
pub const DIMENSION: usize = ek::DIMENSION as usize;
pub const KINDS: usize = ek::COUNT;

/// Schemas of the components the engine reads itself.
pub const BUILTIN: &[(&str, &str)] = &[
    (abi::comp::MAX_STACK, "u32"),
    (abi::comp::DURABILITY, "u32"),
    (abi::comp::ATTACK_DAMAGE, "f32"),
    (abi::comp::MINING_SPEED, "f32"),
    (abi::comp::TOOL_LEVEL, "u32"),
    (abi::comp::USE_PRIORITY, "i32"),
    (abi::comp::HARVEST_LEVEL, "u32"),
    (abi::comp::WEAR, "u32"),
    (abi::comp::HARDNESS, "f32"),
    (abi::comp::FRICTION, "f32"),
    (abi::comp::DRAG, "f32"),
    (abi::comp::LIGHT, "[u8;3]"),
    (abi::comp::TINT, "tint"),
    (abi::comp::TINT_FACES, "u8"),
    (abi::comp::SPREADS_TO, "match"),
    (abi::comp::SPREAD_TIME, "f32"),
    (abi::comp::SPREAD_LIGHT, "u32"),
    (abi::comp::AMBIENT_PARTICLE, "str"),
    (abi::comp::DRIP, "str"),
    (abi::comp::SPLASH, "str"),
    (abi::comp::BUBBLE, "str"),
    (abi::comp::SWIMS_IN, "match"),
    (abi::comp::JOINS, "match"),
    (abi::comp::LIQUIDS, ""),
    (abi::comp::UNBREAKABLE, ""),
    (abi::comp::CONDUCTOR, ""),
    (abi::comp::ENERGY, <abi::Energy as abi::Schema>::SCHEMA),
    (abi::comp::WORN, <abi::Worn as abi::Schema>::SCHEMA),
    (abi::comp::WORN_TEXTURE, "str"),
    (abi::comp::WEAR_PER_HIT, "u32"),
    (abi::attribute::MAX_HEALTH, "f64"),
];

/// One value as a mod registered it; `text` holds the copied string of a `str` value.
#[derive(Clone, Debug)]
pub struct RawValue {
    pub name: String,
    pub schema: String,
    pub bytes: Vec<u8>,
    pub text: Option<String>,
}

impl RawValue {
    /// # Safety
    /// The value's pointers must be valid for this call.
    pub unsafe fn copy(v: &abi::CompValue) -> RawValue {
        unsafe {
            let schema = v.schema.as_str().to_string();
            let bytes = v.value.as_slice().to_vec();
            // Text is packed (`abi::data`): its length, then the UTF-8.
            let text = written_as_text(&schema)
                .then(|| abi::data::from_bytes::<String>(&bytes))
                .flatten();
            RawValue { name: v.name.as_str().to_string(), schema, bytes, text }
        }
    }

    pub fn tag(name: &str) -> RawValue {
        RawValue { name: name.to_string(), schema: String::new(), bytes: Vec::new(), text: None }
    }
}

/// Schemas a mod writes as text: `str` stays text; references (`block`: one block or state,
/// `blocks`: a weighted list `name[:weight] ..`, `item`: an item, `match`: a block matcher
/// (`refs`), `tint`: a tint kind) are resolved into ids at freeze, with errors.
pub fn written_as_text(schema: &str) -> bool {
    abi::data::is_text(schema)
}

/// `bytes` of `schema` with every text in them put through `f`, whatever holds it.
pub fn map_texts(schema: &str, bytes: &[u8], f: &mut dyn FnMut(&str) -> String) -> Option<Vec<u8>> {
    use abi::data::{self as d, Data};
    let s = schema.trim();
    let mut out = Vec::new();
    // Packed values one after another, a struct's fields or an array's elements.
    let mut seq = |schemas: &mut dyn Iterator<Item = &str>, out: &mut Vec<u8>| -> Option<()> {
        let mut at = 0;
        for t in schemas {
            let rest = bytes.get(at..)?;
            out.extend(map_texts(t, rest, f)?);
            at += d::value_len(t, rest)?;
        }
        Some(())
    };
    if d::is_text(s) {
        let text: String = d::from_bytes(bytes.get(..d::value_len(s, bytes)?)?)?;
        d::write_text(&f(&text), &mut out);
    } else if d::layout(s).is_some() {
        out.extend_from_slice(bytes.get(..d::value_len(s, bytes)?)?);
    } else if let Some(inner) = s.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
        seq(&mut d::fields(inner)?.into_iter().map(|f| f.1), &mut out)?;
    } else if let Some(t) = d::list_of(s) {
        let all = d::items(s, bytes)?;
        (all.len() as u32).write(&mut out);
        for i in all {
            out.extend(map_texts(t, i, f)?);
        }
    } else {
        let (t, n) = s.strip_prefix('[')?.strip_suffix(']')?.rsplit_once(';')?;
        seq(&mut std::iter::repeat_n(t, n.trim().parse().ok()?), &mut out)?;
    }
    Some(out)
}

/// Byte size a schema implies, if it is a primitive.
fn primitive_size(schema: &str) -> Option<usize> {
    Some(match schema {
        "" => 0,
        "u8" | "i8" | "bool" => 1,
        "u16" | "i16" => 2,
        "u32" | "i32" | "f32" | "block" | "blocks" | "item" | "match" | "tint" => 4,
        "u64" | "i64" | "f64" => 8,
        "str" => size_of::<Str>(),
        "callback" => 16,
        // Packed values are kept as a `Slice` to their bytes.
        s if s.starts_with('<') || abi::data::list_of(s).is_some() => size_of::<abi::Slice<u8>>(),
        _ => return None,
    })
}

struct Table {
    entries: u32,
    words: u32,
    bits: Vec<u64>,
    /// Per component: stride and data (empty when the kind has no values for it).
    cols: Vec<(u32, Vec<u8>)>,
}

impl Table {
    fn new(entries: usize, components: usize) -> Table {
        let words = components.div_ceil(64).max(1);
        Table {
            entries: entries as u32,
            words: words as u32,
            bits: vec![0; entries * words],
            cols: (0..components).map(|_| (0, Vec::new())).collect(),
        }
    }

    #[inline]
    fn has(&self, entry: u32, comp: u32) -> bool {
        entry < self.entries && (self.bits[(entry * self.words + comp / 64) as usize] >> (comp % 64)) & 1 != 0
    }

    fn value(&self, entry: u32, comp: u32) -> Option<&[u8]> {
        let (stride, data) = &self.cols[comp as usize];
        (self.has(entry, comp) && *stride > 0).then(|| &data[(entry * stride) as usize..((entry + 1) * stride) as usize])
    }

    fn put(&mut self, entry: u32, comp: u32, bytes: &[u8]) -> Result<(), String> {
        if entry >= self.entries {
            return Ok(());
        }
        if !bytes.is_empty() {
            let (stride, data) = &mut self.cols[comp as usize];
            if *stride == 0 {
                *stride = bytes.len() as u32;
                data.resize(self.entries as usize * bytes.len(), 0);
            }
            if bytes.len() != *stride as usize {
                // Marking it present with no value would read as zero.
                return Err(format!("has a value of {} bytes, others have {}", bytes.len(), stride));
            }
            let at = (entry * *stride) as usize;
            data[at..at + bytes.len()].copy_from_slice(bytes);
        }
        let w = (entry * self.words + comp / 64) as usize;
        self.bits[w] |= 1 << (comp % 64);
        Ok(())
    }

    fn columns(&self) -> Vec<abi::Column> {
        self.cols
            .iter()
            .map(|(stride, data)| abi::Column { data: data.as_ptr(), stride: *stride })
            .collect()
    }

    fn view(&self, columns: &[abi::Column]) -> abi::KindTable {
        abi::KindTable { entries: self.entries, words: self.words, bits: self.bits.as_ptr(), columns: columns.as_ptr() }
    }
}

type Refs = dyn Fn(&str, &str) -> Result<Vec<u8>, String>;

/// A value as the tables keep it: text and packed values boxed into `texts` and `blobs` (the
/// table holds a `Str` or `Slice` to them), references resolved, a matcher's text to its id.
fn encode<'v>(
    v: &'v RawValue,
    texts: &mut Vec<Box<str>>,
    blobs: &mut Vec<Box<[u8]>>,
    refs: Option<&Refs>,
    matcher: &mut dyn FnMut(&str) -> Result<u32, String>,
) -> Result<std::borrow::Cow<'v, [u8]>, String> {
    match &v.text {
        Some(t) if v.schema == "match" => matcher(t).map(|id| id.to_le_bytes().to_vec().into()),
        Some(t) if v.schema != "str" => match refs {
            Some(resolve) => resolve(&v.schema, t).map(Into::into),
            None => Err("references resolve only at load".into()),
        },
        Some(t) => {
            let boxed: Box<str> = t.as_str().into();
            let s = Str { ptr: boxed.as_ptr(), len: boxed.len() };
            texts.push(boxed);
            Ok(unsafe {
                std::slice::from_raw_parts(&s as *const Str as *const u8, size_of::<Str>())
                    .to_vec()
                    .into()
            })
        }
        None if v.schema.starts_with('<') || abi::data::list_of(&v.schema).is_some() => {
            let boxed: Box<[u8]> = v.bytes.as_slice().into();
            let s = abi::Slice { ptr: boxed.as_ptr(), len: boxed.len() };
            blobs.push(boxed);
            Ok(unsafe {
                std::slice::from_raw_parts(&s as *const abi::Slice<u8> as *const u8, size_of::<abi::Slice<u8>>())
                    .to_vec()
                    .into()
            })
        }
        None => Ok(v.bytes.as_slice().into()),
    }
}

pub struct Components {
    pub names: Vec<String>,
    pub schemas: Vec<String>,
    by_name: FxHashMap<String, u32>,
    tables: Vec<Table>,
    /// Strings `str` values point into; boxed so the pointers stay put.
    texts: Vec<Box<str>>,
    blobs: Vec<Box<[u8]>>,
    columns: Vec<Vec<abi::Column>>,
    refs: Option<Box<Refs>>,
    abi: Box<abi::ComponentTable>,
}

unsafe impl Send for Components {}
unsafe impl Sync for Components {}

impl Components {
    pub fn id(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    pub fn count(&self) -> u32 {
        self.names.len() as u32
    }

    #[inline]
    pub fn has(&self, kind: usize, entry: u32, comp: u32) -> bool {
        (comp as usize) < self.names.len() && self.tables[kind].has(entry, comp)
    }

    /// Value bytes, if the entry has the component and it carries a value.
    pub fn bytes(&self, kind: usize, entry: u32, comp: u32) -> Option<&[u8]> {
        if comp as usize >= self.names.len() {
            return None;
        }
        self.tables[kind].value(entry, comp)
    }

    /// Typed read; the caller knows the schema (engine built-ins).
    pub fn get<T: Copy>(&self, kind: usize, entry: u32, comp: u32) -> Option<T> {
        self.bytes(kind, entry, comp)
            .filter(|b| b.len() >= size_of::<T>())
            .map(|b| unsafe { std::ptr::read_unaligned(b.as_ptr() as *const T) })
    }

    pub fn text(&self, kind: usize, entry: u32, comp: u32) -> Option<&str> {
        (self.schemas.get(comp as usize).map(String::as_str) == Some("str"))
            .then(|| self.get::<Str>(kind, entry, comp))
            .flatten()
            .map(|s| unsafe { s.as_str() })
    }

    /// Component ids an entry has, in id order.
    pub fn of(&self, kind: usize, entry: u32) -> impl Iterator<Item = u32> + '_ {
        let t = &self.tables[kind];
        let row: &[u64] = if entry < t.entries { &t.bits[(entry * t.words) as usize..((entry + 1) * t.words) as usize] } else { &[] };
        row.iter().enumerate().flat_map(|(w, &bits)| {
            (0..64)
                .filter(move |b| bits >> b & 1 != 0)
                .map(move |b| w as u32 * 64 + b)
        })
    }

    pub fn is_tag(&self, comp: u32) -> bool {
        self.schemas
            .get(comp as usize)
            .is_some_and(|s| s.is_empty())
    }

    pub fn table(&self) -> *const abi::ComponentTable {
        &*self.abi
    }

    /// The table of a kind whose ids are known only after the others froze (weathers, game
    /// modes ...): every value of each entry, in id order, its components declared already.
    pub fn add_table(
        &mut self,
        kind: usize,
        entries: &[crate::entries::Entry],
        matcher: &mut dyn FnMut(&str, &Components) -> Result<u32, String>,
        errors: &mut Vec<String>,
    ) {
        let mut t = Table::new(entries.len(), self.names.len());
        let (mut texts, mut blobs) = (Vec::new(), Vec::new());
        for (i, e) in entries.iter().enumerate() {
            for v in &e.values {
                let Some(&comp) = self.by_name.get(&v.name) else { continue };
                let put = encode(v, &mut texts, &mut blobs, self.refs.as_deref(), &mut |m| matcher(m, self))
                    .and_then(|bytes| t.put(i as u32, comp, &bytes));
                if let Err(err) = put {
                    errors.push(format!("{}: {}: {err}", e.name, v.name));
                }
            }
        }
        self.texts.append(&mut texts);
        self.blobs.append(&mut blobs);
        self.columns[kind] = t.columns();
        self.abi.kinds[kind] = t.view(&self.columns[kind]);
        self.tables[kind] = t;
    }
}

/// Size and alignment of a schema's value (`repr(C)`), for what can be read in text.
fn layout_of(schema: &str) -> Option<(usize, usize)> {
    let s = schema.trim();
    match s {
        "u8" | "i8" | "bool" => return Some((1, 1)),
        "u16" | "i16" => return Some((2, 2)),
        "u32" | "i32" | "f32" => return Some((4, 4)),
        "u64" | "i64" | "f64" => return Some((8, 8)),
        _ => {}
    }
    if let Some((t, n)) = s
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .and_then(|r| r.rsplit_once(';'))
    {
        let (size, align) = layout_of(t)?;
        return Some((size * n.trim().parse::<usize>().ok()?, align));
    }
    let (mut at, mut most) = (0usize, 1usize);
    for (_, t) in schema_fields(s)? {
        let (size, align) = layout_of(t)?;
        at = at.next_multiple_of(align) + size;
        most = most.max(align);
    }
    Some((at.next_multiple_of(most), most))
}

/// The fields of a `{name:type,..}` schema.
fn schema_fields(schema: &str) -> Option<Vec<(&str, &str)>> {
    let inner = schema.trim().strip_prefix('{')?.strip_suffix('}')?;
    let (mut out, mut depth, mut start) = (Vec::new(), 0, 0);
    for (i, ch) in inner.char_indices() {
        match ch {
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(inner[start..i].split_once(':')?);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < inner.len() {
        out.push(inner[start..].split_once(':')?);
    }
    Some(out)
}

/// A number in a value by its schema: the field named, or its first number (a lone number's self).
pub fn number_in(schema: &str, bytes: &[u8], field: Option<&str>) -> Option<f32> {
    let s = schema.trim();
    if let Some(fields) = schema_fields(s) {
        let mut at = 0usize;
        for (name, t) in fields {
            let (size, align) = layout_of(t)?;
            at = at.next_multiple_of(align);
            let wanted = field.map_or(true, |f| f == name.trim());
            if wanted {
                if let Some(n) = number_in(t, bytes.get(at..at + size)?, None) {
                    return Some(n);
                }
            }
            at += size;
        }
        return None;
    }
    if let Some(t) = s.strip_prefix('[').and_then(|r| r.split(';').next()) {
        return number_in(t, bytes, None);
    }
    let b = |n: usize| bytes.get(..n);
    Some(match s {
        "u8" | "bool" => *b(1)?.first()? as f32,
        "i8" => *b(1)?.first()? as i8 as f32,
        "u16" => u16::from_le_bytes(b(2)?.try_into().ok()?) as f32,
        "i16" => i16::from_le_bytes(b(2)?.try_into().ok()?) as f32,
        "u32" => u32::from_le_bytes(b(4)?.try_into().ok()?) as f32,
        "i32" => i32::from_le_bytes(b(4)?.try_into().ok()?) as f32,
        "f32" => f32::from_le_bytes(b(4)?.try_into().ok()?),
        "u64" => u64::from_le_bytes(b(8)?.try_into().ok()?) as f32,
        "i64" => i64::from_le_bytes(b(8)?.try_into().ok()?) as f32,
        "f64" => f64::from_le_bytes(b(8)?.try_into().ok()?) as f32,
        _ => return None,
    })
}

/// Collects schemas and values during freeze and turns them into `Components`.
pub struct Builder {
    /// Name -> (schema, who declared it first).
    decl: FxHashMap<String, (String, String)>,
    names: Vec<String>,
    ids: FxHashMap<String, u32>,
    tables: Vec<Table>,
    texts: Vec<Box<str>>,
    /// Bytes of packed values (lists, `<..>` structs) the tables point into.
    blobs: Vec<Box<[u8]>>,
    /// Values that could not be stored (freeze errors).
    pub errors: Vec<String>,
    /// Turns the text of a reference value (`block`, `blocks`) into its bytes.
    pub refs: Option<Box<Refs>>,
    /// Matcher texts values name, by id, with who named each first: compiled once the tables
    /// they test are filled.
    pub matchers: Vec<(String, String)>,
    match_ids: FxHashMap<String, u32>,
}

impl Default for Builder {
    fn default() -> Self {
        let mut b = Builder {
            decl: FxHashMap::default(),
            names: Vec::new(),
            ids: FxHashMap::default(),
            tables: Vec::new(),
            texts: Vec::new(),
            blobs: Vec::new(),
            errors: Vec::new(),
            refs: None,
            matchers: Vec::new(),
            match_ids: FxHashMap::default(),
        };
        for (name, schema) in BUILTIN {
            b.decl
                .insert(name.to_string(), (schema.to_string(), "the engine".into()));
        }
        b
    }
}

impl Builder {
    /// Records a use of `name` with `schema`; uses must agree. `who` names the mod and entry for
    /// the error.
    pub fn declare(&mut self, name: &str, schema: &str, bytes: Option<usize>, who: &dyn Fn() -> String, errors: &mut Vec<String>) {
        if name.is_empty() {
            errors.push(format!("{}: component without a name", who()));
            return;
        }
        if let (Some(n), Some(want)) = (bytes, primitive_size(schema)) {
            if n != want {
                errors.push(format!("{}: component {name} is {schema} ({want} bytes) but the value has {n} bytes", who()));
                return;
            }
        }
        match self.decl.get(name) {
            Some((s, first)) if s != schema => {
                let shown = |s: &str| if s.is_empty() { "a tag".to_string() } else { format!("'{s}'") };
                errors.push(format!("{}: component {name} used as {} but {first} uses it as {}", who(), shown(schema), shown(s)));
            }
            Some(_) => {}
            None => {
                self.decl
                    .insert(name.to_string(), (schema.to_string(), who()));
            }
        }
    }

    /// Declares a value's component.
    pub fn declare_value(&mut self, v: &RawValue, who: &dyn Fn() -> String, errors: &mut Vec<String>) {
        // Packed values (lists, `<..>` structs) are as long as they are.
        let packed = v.schema.starts_with('<') || abi::data::list_of(&v.schema).is_some();
        self.declare(&v.name, &v.schema, (v.text.is_none() && !packed).then_some(v.bytes.len()), who, errors);
    }

    /// Assigns ids and allocates the tables of the first kinds (`entries` of each); values go in
    /// with `put` afterwards, the other kinds' with `Components::add_table`.
    pub fn start(&mut self, entries: &[usize]) {
        let mut names: Vec<String> = self.decl.keys().cloned().collect();
        names.sort();
        self.ids = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i as u32))
            .collect();
        self.tables = (0..KINDS)
            .map(|k| Table::new(entries.get(k).copied().unwrap_or(0), names.len()))
            .collect();
        self.names = names;
    }

    pub fn id(&self, name: &str) -> Option<u32> {
        self.ids.get(name).copied()
    }

    /// Sets a value (or tag) on an entry.
    pub fn put(&mut self, kind: usize, entry: u32, v: &RawValue, label: &dyn Fn() -> String) {
        let Some(comp) = self.id(&v.name) else { return };
        let (matchers, match_ids) = (&mut self.matchers, &mut self.match_ids);
        let mut matcher = |t: &str| {
            let next = matchers.len() as u32;
            let id = *match_ids.entry(t.to_string()).or_insert(next);
            if id == next {
                matchers.push((t.to_string(), format!("{}: {}", label(), v.name)));
            }
            Ok(id)
        };
        let put = encode(v, &mut self.texts, &mut self.blobs, self.refs.as_deref(), &mut matcher)
            .and_then(|bytes| self.tables[kind].put(entry, comp, &bytes));
        if let Err(e) = put {
            self.errors.push(format!("{}: {}: {e}", label(), v.name));
        }
    }

    /// Copies every component of `from` that `to` lacks (block items get their block's).
    pub fn inherit(&mut self, from: (usize, u32), to: (usize, u32)) {
        let src = &self.tables[from.0];
        if from.1 >= src.entries {
            return;
        }
        let row = (from.1 * src.words) as usize;
        let set: Vec<u32> = (0..src.words as usize)
            .flat_map(|w| {
                let bits = src.bits[row + w];
                (0..64u32)
                    .filter(move |b| bits >> b & 1 != 0)
                    .map(move |b| w as u32 * 64 + b)
            })
            .collect();
        for comp in set {
            if self.tables[to.0].has(to.1, comp) {
                continue;
            }
            let value = self.tables[from.0].value(from.1, comp).map(<[u8]>::to_vec);
            let t = &mut self.tables[to.0];
            t.bits[(to.1 * t.words + comp / 64) as usize] |= 1 << (comp % 64);
            if let Some(bytes) = value {
                let (stride, data) = &mut t.cols[comp as usize];
                if *stride == 0 {
                    *stride = bytes.len() as u32;
                    data.resize(t.entries as usize * bytes.len(), 0);
                }
                if bytes.len() == *stride as usize {
                    let at = (to.1 * *stride) as usize;
                    data[at..at + bytes.len()].copy_from_slice(&bytes);
                }
            }
        }
    }

    pub fn finish(self) -> Components {
        let Builder { decl, names, ids, tables, texts, blobs, refs, .. } = self;
        let schemas: Vec<String> = names.iter().map(|n| decl[n].0.clone()).collect();
        let columns: Vec<Vec<abi::Column>> = tables.iter().map(Table::columns).collect();
        let kinds: [abi::KindTable; KINDS] = std::array::from_fn(|k| tables[k].view(&columns[k]));
        let abi = Box::new(abi::ComponentTable { components: names.len() as u32, kinds });
        Components { names, schemas, by_name: ids, tables, texts, blobs, columns, refs, abi }
    }
}

/// `*` in a pattern matches any text: `ingots/*`, `*_ore`.
pub fn matches(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((pre, post)) => name.len() >= pre.len() + post.len() && name.starts_with(pre) && name.ends_with(post),
    }
}

#[cfg(test)]
mod numbers {
    use super::number_in;

    #[test]
    fn reads_by_schema() {
        let mut food = 5u32.to_le_bytes().to_vec();
        food.extend(7.5f32.to_le_bytes());
        let schema = "{hunger:u32,saturation:f32}";
        assert_eq!(number_in(schema, &food, Some("saturation")), Some(7.5));
        assert_eq!(number_in(schema, &food, Some("hunger")), Some(5.0));
        assert_eq!(number_in(schema, &food, None), Some(5.0), "the first number");
        assert_eq!(number_in(schema, &food, Some("nope")), None);
        assert_eq!(number_in("f32", &1.5f32.to_le_bytes(), None), Some(1.5));
        // Padding before a wider field.
        let mut v = vec![9u8, 0, 0, 0];
        v.extend(2.0f32.to_le_bytes());
        assert_eq!(number_in("{a:u8,b:f32}", &v, Some("b")), Some(2.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(name: &str, schema: &str, v: u32) -> RawValue {
        RawValue { name: name.into(), schema: schema.into(), bytes: v.to_le_bytes().to_vec(), text: None }
    }

    #[test]
    fn schemas_must_agree() {
        let mut b = Builder::default();
        let mut errors = Vec::new();
        b.declare("fuel", "u32", Some(4), &|| "base: base:coal".into(), &mut errors);
        b.declare("fuel", "u32", Some(4), &|| "testmod: testmod:peat".into(), &mut errors);
        assert!(errors.is_empty());
        b.declare("fuel", "f32", Some(4), &|| "other: other:wood".into(), &mut errors);
        b.declare("max_stack", "", None, &|| "other: other:thing".into(), &mut errors);
        b.declare("burn", "u32", Some(8), &|| "other: other:x".into(), &mut errors);
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(errors[0].contains("fuel") && errors[0].contains("other:wood") && errors[0].contains("base:coal"));
    }

    /// Block references become ids at freeze; one that names nothing is an error, not a zero.
    #[test]
    fn references_resolve_or_fail() {
        let mut b = Builder::default();
        let mut errors = Vec::new();
        b.declare("leaves", "block", None, &|| "a".into(), &mut errors);
        b.start(&[1]);
        b.refs =
            Some(Box::new(|_, t| if t == "a:oak_leaves" { Ok(7u32.to_le_bytes().to_vec()) } else { Err(format!("unknown block {t}")) }));
        let v = |t: &str| RawValue { name: "leaves".into(), schema: "block".into(), bytes: vec![], text: Some(t.into()) };
        b.put(BLOCK, 0, &v("a:oak_leaves"), &|| "log".into());
        b.put(BLOCK, 0, &v("a:oak_leafs"), &|| "log".into());
        assert_eq!(b.errors, vec!["log: leaves: unknown block a:oak_leafs".to_string()]);
        let c = b.finish();
        assert_eq!(c.get::<u32>(BLOCK, 0, c.id("leaves").unwrap()), Some(7));
    }

    #[test]
    fn tables_hold_bits_values_and_text() {
        let mut b = Builder::default();
        let mut errors = Vec::new();
        b.declare("fuel", "u32", Some(4), &|| "a".into(), &mut errors);
        b.declare("plant_soil", "", Some(0), &|| "a".into(), &mut errors);
        b.declare("label", "str", None, &|| "a".into(), &mut errors);
        b.start(&[3, 2]);
        b.put(BLOCK, 1, &value("fuel", "u32", 300), &|| "x".into());
        b.put(BLOCK, 2, &RawValue::tag("plant_soil"), &|| "x".into());
        b.put(ITEM, 0, &RawValue { name: "label".into(), schema: "str".into(), bytes: vec![], text: Some("hi".into()) }, &|| "x".into());
        b.inherit((BLOCK, 1), (ITEM, 1));
        b.put(BLOCK, 1, &value("fuel", "u32", 100), &|| "x".into());
        let c = b.finish();
        let (fuel, soil, label) = (c.id("fuel").unwrap(), c.id("plant_soil").unwrap(), c.id("label").unwrap());
        assert_eq!(c.get::<u32>(BLOCK, 1, fuel), Some(100));
        assert_eq!(c.get::<u32>(ITEM, 1, fuel), Some(300));
        assert!(c.has(BLOCK, 2, soil) && !c.has(BLOCK, 1, soil) && !c.has(BLOCK, 7, soil));
        assert_eq!(c.text(ITEM, 0, label), Some("hi"));
        assert_eq!(c.of(BLOCK, 2).collect::<Vec<_>>(), vec![soil]);
        // The table mods read says the same.
        let t = unsafe { &*c.table() };
        let k = &t.kinds[BLOCK];
        let word = unsafe { *k.bits.add((2 * k.words + soil / 64) as usize) };
        assert_eq!(word >> (soil % 64) & 1, 1);
    }

    #[test]
    fn patterns() {
        assert!(matches("ingots/*", "ingots/iron"));
        assert!(!matches("ingots/*", "gems/diamond"));
        assert!(matches("*_ore", "tin_ore"));
        assert!(matches("logs", "logs"));
    }
}
