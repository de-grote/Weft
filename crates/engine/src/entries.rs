//! Registration as entries: every kind is a named entry made of components (`abi::EntryDesc`).
//! Mods' entries of a kind are folded in load order (new, edits, replacements, removals) into
//! one `Entry` per name, which the kind's freeze then bakes into what the game reads.

use rustc_hash::FxHashMap;
use weft_abi::{self as abi, ModId, data, entry_mode as em, value_op as vo};

use crate::components::RawValue;

#[derive(Clone, Debug)]
pub struct RawOp {
    pub op: u32,
    pub value: RawValue,
    pub anchor: String,
}

/// One entry as a mod registered it.
#[derive(Clone, Debug)]
pub struct RawEntry {
    pub owner: ModId,
    pub kind: u32,
    pub name: String,
    pub cell: usize,
    pub mode: u32,
    pub ops: Vec<RawOp>,
    pub parts: Vec<(String, Vec<RawOp>)>,
}

impl RawEntry {
    /// One of the engine's own, before every mod's.
    pub fn engine(kind: u32, name: &str) -> RawEntry {
        RawEntry { owner: ModId(u32::MAX), kind, name: name.to_string(), cell: 0, mode: em::NEW, ops: Vec::new(), parts: Vec::new() }
    }

    pub fn set<T: data::Data>(mut self, name: &str, v: T) -> RawEntry {
        self.ops
            .push(RawOp { op: vo::SET, value: value(name, v), anchor: String::new() });
        self
    }

    pub fn tag(mut self, name: &str) -> RawEntry {
        self.ops
            .push(RawOp { op: vo::SET, value: RawValue::tag(name), anchor: String::new() });
        self
    }

    /// # Safety
    /// The descriptor's pointers must be valid for this call.
    pub unsafe fn copy(owner: ModId, d: &abi::EntryDesc) -> RawEntry {
        unsafe {
            let ops = |s: abi::Slice<abi::ValueOp>| -> Vec<RawOp> {
                s.as_slice()
                    .iter()
                    .map(|o| RawOp { op: o.op, value: RawValue::copy(&o.value), anchor: o.anchor.as_str().to_string() })
                    .collect()
            };
            RawEntry {
                owner,
                kind: d.kind,
                name: d.name.as_str().to_string(),
                cell: d.cell as usize,
                mode: d.mode,
                ops: ops(d.ops),
                parts: d
                    .parts
                    .as_slice()
                    .iter()
                    .map(|p| (p.key.as_str().to_string(), ops(p.ops)))
                    .collect(),
            }
        }
    }
}

/// `T::schema()`, worked out once: a packed struct's is built of its fields' every time.
pub fn schema<T: data::Data>() -> &'static str {
    thread_local! {
        static KNOWN: std::cell::RefCell<FxHashMap<std::any::TypeId, &'static str>> = Default::default();
    }
    KNOWN.with(|k| {
        *k.borrow_mut()
            .entry(std::any::TypeId::of::<T>())
            .or_insert_with(|| T::schema().leak())
    })
}

/// `v` as the value of component `name`.
fn value<T: data::Data>(name: &str, v: T) -> RawValue {
    let bytes = data::bytes_of(&v);
    let schema = T::schema();
    let text = data::is_text(&schema)
        .then(|| data::from_bytes::<String>(&bytes))
        .flatten();
    RawValue { name: name.to_string(), schema, bytes, text }
}

/// One entry of a kind after every mod had its say.
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub name: String,
    /// The mod whose registration stands (the first, or the last replacement).
    pub owner: ModId,
    /// Handle cells to fill with its id, every mod's.
    pub cells: Vec<usize>,
    pub values: Vec<RawValue>,
    /// Who set each callback, by its bytes (alone or in a list): it runs as that mod.
    pub callers: Vec<(Vec<u8>, ModId)>,
    /// Blocks: operations on some states, by key, in the order they came.
    pub parts: Vec<(String, Vec<RawOp>)>,
}

impl Entry {
    pub fn value(&self, name: &str) -> Option<&RawValue> {
        self.values.iter().find(|v| v.name == name)
    }

    pub fn has(&self, name: &str) -> bool {
        self.value(name).is_some()
    }

    /// The value of `name` as `T`, if it is there with `T`'s schema.
    pub fn get<T: data::Data>(&self, name: &str) -> Option<T> {
        self.value(name)
            .filter(|v| v.schema == schema::<T>())
            .and_then(|v| data::from_bytes(&v.bytes))
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        self.value(name).and_then(|v| v.text.as_deref())
    }

    pub fn or<T: data::Data>(&self, name: &str, default: T) -> T {
        self.get(name).unwrap_or(default)
    }

    /// Sets a value, as the engine does for what it makes of an entry (a template's items).
    pub fn set<T: data::Data>(&mut self, name: &str, v: T) {
        self.values.retain(|x| x.name != name);
        self.values.push(value(name, v));
    }

    /// Its values but the engine's own of its kind, named `<own>..`: what goes in the kind's
    /// component table.
    pub fn components(&self, own: &str) -> Vec<RawValue> {
        self.values
            .iter()
            .filter(|v| !v.name.starts_with(own))
            .cloned()
            .collect()
    }

    /// A callback value and the mod it runs as.
    pub fn callback(&self, name: &str) -> Option<(ModId, abi::Callback)> {
        self.value(name)
            .filter(|v| v.schema == "callback")
            .and_then(|v| self.caller(&v.bytes))
    }

    /// The callbacks of a list, in order, each with the mod it runs as.
    pub fn callbacks(&self, name: &str) -> Vec<(ModId, abi::Callback)> {
        self.value(name)
            .and_then(|v| data::items(&v.schema, &v.bytes))
            .into_iter()
            .flatten()
            .filter_map(|b| self.caller(b))
            .collect()
    }

    fn caller(&self, bytes: &[u8]) -> Option<(ModId, abi::Callback)> {
        let cb = data::from_bytes::<abi::Callback>(bytes).filter(|c| c.is_some())?;
        let by = self
            .callers
            .iter()
            .rfind(|c| c.0 == bytes)
            .map_or(self.owner, |c| c.1);
        Some((by, cb))
    }

    /// The `flags` whose tag it has.
    pub fn flags(&self, flags: &[(&str, u32)]) -> u32 {
        flags
            .iter()
            .filter(|(tag, _)| self.has(tag))
            .fold(0, |f, (_, bit)| f | bit)
    }

    /// Applies one operation, made by mod `by`; `errors` gets what made no sense.
    pub fn apply(&mut self, op: &RawOp, by: ModId, errors: &mut Vec<String>) {
        if op.value.schema == "callback" {
            self.callers.push((op.value.bytes.clone(), by));
        }
        apply(&mut self.values, op, &self.name, errors);
    }
}

/// Applies `op` to a list of values.
pub fn apply(values: &mut Vec<RawValue>, op: &RawOp, what: &str, errors: &mut Vec<String>) {
    let at = values.iter().position(|v| v.name == op.value.name);
    match op.op {
        vo::SET => match at {
            Some(i) => values[i] = op.value.clone(),
            None => values.push(op.value.clone()),
        },
        vo::UNSET => {
            if let Some(i) = at {
                values.remove(i);
            }
        }
        _ => {
            let name = &op.value.name;
            let list_schema = format!("[{}]", op.value.schema);
            let i = match at {
                Some(i) => i,
                None => {
                    values.push(RawValue { name: name.clone(), schema: list_schema.clone(), bytes: data::list(&[]), text: None });
                    values.len() - 1
                }
            };
            let v = &mut values[i];
            if v.schema != list_schema {
                errors.push(format!("{what}: {name} is {}, not a list of {}", v.schema, op.value.schema));
                return;
            }
            // At the end: the count one up, no element read.
            if op.op == vo::PUSH && v.bytes.len() >= 4 {
                let n = u32::from_le_bytes(v.bytes[..4].try_into().unwrap()) + 1;
                v.bytes[..4].copy_from_slice(&n.to_le_bytes());
                v.bytes.extend_from_slice(&op.value.bytes);
                return;
            }
            let Some(mut items) = data::items(&v.schema, &v.bytes) else {
                errors.push(format!("{what}: {name}: bad list"));
                return;
            };
            let elem = op.value.schema.as_str();
            let found = || -> Option<usize> {
                match op
                    .anchor
                    .strip_prefix('#')
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    Some(n) => (n < items.len()).then_some(n),
                    None => items
                        .iter()
                        .position(|it| data::item_name(elem, it) == Some(op.anchor.as_str())),
                }
            };
            let new = op.value.bytes.as_slice();
            match op.op {
                vo::PUSH => items.push(new),
                vo::INSERT_BEFORE | vo::INSERT_AFTER | vo::REMOVE_ITEM | vo::REPLACE_ITEM => {
                    let Some(n) = found() else {
                        errors.push(format!("{what}: {name} has no element {}", op.anchor));
                        return;
                    };
                    match op.op {
                        vo::INSERT_BEFORE => items.insert(n, new),
                        vo::INSERT_AFTER => items.insert(n + 1, new),
                        vo::REMOVE_ITEM => drop(items.remove(n)),
                        _ => items[n] = new,
                    }
                }
                n => errors.push(format!("{what}: {name}: unknown operation {n}")),
            }
            let bytes = data::list(&items);
            values[i].bytes = bytes;
        }
    }
}

/// A kind's entries folded after the engine's own (`builtins`), patterns applied, ids written into
/// the handles, each baked: the defs in id order and their names.
pub fn bake<D>(
    raw: Vec<RawEntry>,
    kind: u32,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
    make: impl FnMut(&Entry, &mut Vec<String>) -> D,
) -> (Vec<D>, FxHashMap<String, u32>) {
    bake_by(raw, kind, mod_name, errors, |_| (), make)
}

/// `bake`, the ids in the order of `key` (stable: the order they came in where it is equal).
pub fn bake_by<D, K: Ord>(
    raw: Vec<RawEntry>,
    kind: u32,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
    key: impl FnMut(&Entry) -> K,
    make: impl FnMut(&Entry, &mut Vec<String>) -> D,
) -> (Vec<D>, FxHashMap<String, u32>) {
    finish(fold(kind_name(kind), raw, mod_name, errors), errors, key, make)
}

fn kind_name(kind: u32) -> &'static str {
    abi::entry_kind::NAMES
        .get(kind as usize)
        .copied()
        .unwrap_or("entry")
}

/// The rest of `bake_by`, for a kind folded early (what liquids' heat says of their blocks).
pub fn finish<D, K: Ord>(
    f: Folded,
    errors: &mut Vec<String>,
    key: impl FnMut(&Entry) -> K,
    mut make: impl FnMut(&Entry, &mut Vec<String>) -> D,
) -> (Vec<D>, FxHashMap<String, u32>) {
    let (entries, names) = order(f, errors, key);
    (entries.iter().map(|e| make(e, errors)).collect(), names)
}

/// The entries in id order, ids written into the handles, and their names.
fn order<K: Ord>(mut f: Folded, errors: &mut Vec<String>, key: impl FnMut(&Entry) -> K) -> (Vec<Entry>, FxHashMap<String, u32>) {
    f.report_missing(errors);
    f.entries.sort_by_cached_key(key);
    f.apply_patterns(errors);
    f.write_cells();
    let names = f
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.clone(), i as u32))
        .collect();
    (f.entries, names)
}

/// Kinds without a component table. Component declarations are what tables are made of, so
/// theirs would have to exist before them; word sets are only lists of words for commands.
/// Everything else has one by entry: what the game makes of an entry (a recipe expanded per
/// material, an ore copied into each dimension) keeps the entry's number to read it by.
pub const NO_TABLE: [u32; 2] = [abi::entry_kind::WORDS, abi::entry_kind::COMPONENT];

/// Kinds baked through here keep their entries, in id order, for their component tables
/// (`Components::add_table`).
#[derive(Default)]
pub struct Tables(pub Vec<(u32, Vec<Entry>)>);

impl Tables {
    pub fn bake<D>(
        &mut self,
        raw: Vec<RawEntry>,
        kind: u32,
        mod_name: &dyn Fn(ModId) -> String,
        errors: &mut Vec<String>,
        make: impl FnMut(&Entry, &mut Vec<String>) -> D,
    ) -> (Vec<D>, FxHashMap<String, u32>) {
        self.bake_by(raw, kind, mod_name, errors, |_| (), make)
    }

    pub fn bake_by<D, K: Ord>(
        &mut self,
        raw: Vec<RawEntry>,
        kind: u32,
        mod_name: &dyn Fn(ModId) -> String,
        errors: &mut Vec<String>,
        key: impl FnMut(&Entry) -> K,
        make: impl FnMut(&Entry, &mut Vec<String>) -> D,
    ) -> (Vec<D>, FxHashMap<String, u32>) {
        self.finish(kind, fold(kind_name(kind), raw, mod_name, errors), errors, key, make)
    }

    pub fn finish<D, K: Ord>(
        &mut self,
        kind: u32,
        f: Folded,
        errors: &mut Vec<String>,
        key: impl FnMut(&Entry) -> K,
        mut make: impl FnMut(&Entry, &mut Vec<String>) -> D,
    ) -> (Vec<D>, FxHashMap<String, u32>) {
        // Their components were declared by this list, before anything was baked.
        debug_assert!(!NO_TABLE.contains(&kind), "{} has no table", kind_name(kind));
        let (entries, names) = order(f, errors, key);
        let out = entries.iter().map(|e| make(e, errors)).collect();
        self.0.push((kind, entries));
        (out, names)
    }
}

/// Whether an EDIT's name is a pattern, not one entry.
pub fn is_pattern(name: &str) -> bool {
    name.starts_with('#') || name.contains('*') || name.contains('[')
}

/// A pattern's name and the `prop=value,..` inside its brackets, if it has them.
pub fn split_pattern(name: &str) -> (&str, Option<&str>) {
    match name.split_once('[') {
        Some((n, rest)) => (n, Some(rest.trim_end_matches(']'))),
        None => (name, None),
    }
}

/// Folds one kind's entries, in load order, into one per name, in the order they were first
/// registered (the engine's own first). `kind` names it in errors. Edits naming a pattern are
/// left in `patterns` for the kind to apply, as only it knows its tags and states; edits naming
/// nothing are left in `missing`. A block's operations are its first part (an edit's after the
/// parts before it, so they go on every state in their order), but for what is the block's own.
pub fn fold(kind: &str, raw: Vec<RawEntry>, mod_name: &dyn Fn(ModId) -> String, errors: &mut Vec<String>) -> Folded {
    // Name -> (place, entry); a replacement keeps the place.
    let mut by: FxHashMap<String, (usize, Entry)> = FxHashMap::default();
    let mut late: Vec<RawEntry> = Vec::new();
    let mut patterns: Vec<RawEntry> = Vec::new();
    let mut gone_cells: Vec<usize> = Vec::new();
    for (seq, mut e) in raw.into_iter().enumerate() {
        if e.kind == abi::entry_kind::BLOCK && e.mode != em::REMOVE && !is_pattern(&e.name) {
            let ops = std::mem::take(&mut e.ops);
            e.ops = ops
                .iter()
                .filter(|o| crate::blocks::block_only(&o.value.name))
                .cloned()
                .collect();
            e.parts.insert(0, (String::new(), ops));
        }
        match e.mode {
            em::NEW | em::REPLACE => {
                if let (em::NEW, Some((_, old))) = (e.mode, by.get(&e.name)) {
                    errors.push(format!("{kind} {} registered by both {} and {}", e.name, mod_name(old.owner), mod_name(e.owner)));
                    continue;
                }
                let (at, mut cells) = by
                    .remove(&e.name)
                    .map_or((seq, Vec::new()), |(at, o)| (at, o.cells));
                cells.push(e.cell);
                let mut entry =
                    Entry { name: e.name.clone(), owner: e.owner, cells, values: Vec::new(), callers: Vec::new(), parts: e.parts };
                for op in &e.ops {
                    entry.apply(op, e.owner, errors);
                }
                by.insert(e.name, (at, entry));
            }
            em::REMOVE => {
                if let Some((_, old)) = by.remove(&e.name) {
                    gone_cells.extend(old.cells);
                }
            }
            em::EDIT if is_pattern(&e.name) => patterns.push(e),
            em::EDIT => match by.get_mut(&e.name) {
                Some((_, entry)) => edit(entry, e, errors),
                // Its mod may come later in the load order.
                None => late.push(e),
            },
            m => errors.push(format!("{kind} {}: unknown mode {m}", e.name)),
        }
    }
    let mut missing = Vec::new();
    for e in late {
        match by.get_mut(&e.name) {
            Some((_, entry)) => edit(entry, e, errors),
            None => {
                let why = format!("{}: edit of {kind} {}: no such {kind}", mod_name(e.owner), e.name);
                missing.push((e, why));
            }
        }
    }
    let mut entries: Vec<(usize, Entry)> = by.into_values().collect();
    entries.sort_by_key(|e| e.0);
    Folded { entries: entries.into_iter().map(|e| e.1).collect(), patterns, gone_cells, missing }
}

/// An edit's operations and parts onto its entry.
pub fn edit(entry: &mut Entry, e: RawEntry, errors: &mut Vec<String>) {
    entry.cells.push(e.cell);
    for op in &e.ops {
        entry.apply(op, e.owner, errors);
    }
    entry.parts.extend(e.parts);
}

pub struct Folded {
    pub entries: Vec<Entry>,
    /// Edits naming a pattern, in load order.
    pub patterns: Vec<RawEntry>,
    /// Cells of removed entries: their handles stay unknown.
    pub gone_cells: Vec<usize>,
    /// Edits of a name nothing registered, and the error they are unless something makes it.
    pub missing: Vec<(RawEntry, String)>,
}

impl Folded {
    pub fn report_missing(&mut self, errors: &mut Vec<String>) {
        errors.extend(std::mem::take(&mut self.missing).into_iter().map(|m| m.1));
    }

    /// Applies the pattern edits matching by name (`*`, `name[]`) or by a tag value (`#tag`):
    /// what fits every kind. Blocks handle states (`[prop=value]`) themselves, other kinds leave
    /// those out. Gives each pattern's mod and name, and whether it matched anything.
    pub fn apply_patterns(&mut self, errors: &mut Vec<String>) -> Vec<(ModId, String, bool)> {
        let mut out = Vec::new();
        for p in std::mem::take(&mut self.patterns) {
            let (name, props) = split_pattern(&p.name);
            let mut hit_any = false;
            if props.is_none_or(|k| k.trim().is_empty()) {
                let tag = name.strip_prefix('#');
                for e in &mut self.entries {
                    let hit = match tag {
                        Some(t) => e
                            .values
                            .iter()
                            .any(|v| v.schema.is_empty() && crate::components::matches(t, &v.name)),
                        None => crate::components::matches(name, &e.name),
                    };
                    if hit {
                        hit_any = true;
                        for op in &p.ops {
                            e.apply(op, p.owner, errors);
                        }
                    }
                }
            }
            out.push((p.owner, p.name, hit_any));
        }
        out
    }

    /// Writes every entry's id (its place, by name) into its handles' cells.
    pub fn write_cells(&self) {
        for (i, e) in self.entries.iter().enumerate() {
            for &c in &e.cells {
                crate::registry::write_cell(c, i as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Entry {
        fn apply_test(&mut self, op: &RawOp, errors: &mut Vec<String>) {
            self.apply(op, ModId(0), errors)
        }
    }

    fn set(name: &str, v: u32) -> RawOp {
        RawOp {
            op: vo::SET,
            value: RawValue { name: name.into(), schema: "u32".into(), bytes: v.to_le_bytes().to_vec(), text: None },
            anchor: String::new(),
        }
    }

    fn entry(owner: u32, name: &str, mode: u32, ops: Vec<RawOp>) -> RawEntry {
        RawEntry { owner: ModId(owner), kind: abi::entry_kind::ITEM, name: name.into(), cell: 0, mode, ops, parts: Vec::new() }
    }

    #[test]
    fn modes_in_load_order() {
        let mut errors = Vec::new();
        let raw = vec![
            entry(2, "a:x", em::EDIT, vec![set("speed", 9)]),
            entry(1, "a:x", em::NEW, vec![set("speed", 1), set("size", 2)]),
            entry(3, "a:y", em::NEW, vec![set("speed", 1)]),
            entry(4, "a:y", em::REMOVE, vec![]),
            entry(5, "a:z", em::NEW, vec![set("speed", 1)]),
            entry(6, "a:z", em::REPLACE, vec![set("size", 7)]),
            entry(7, "a:z", em::NEW, vec![]),
        ];
        let f = fold("thing", raw, &|m| format!("mod{}", m.0), &mut errors);
        let names: Vec<&str> = f.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a:x", "a:z"], "in the order they first came");
        assert_eq!(f.entries[0].get::<u32>("speed"), Some(9), "an edit before its entry still applies");
        assert_eq!(f.entries[0].get::<u32>("size"), Some(2));
        assert_eq!((f.entries[1].get::<u32>("speed"), f.entries[1].get::<u32>("size")), (None, Some(7)));
        assert_eq!(errors.len(), 1, "{errors:?}");
    }

    #[test]
    fn list_operations() {
        let item = |name: &str, n: u32| {
            let mut b = Vec::new();
            data::write_text(name, &mut b);
            b.extend(n.to_le_bytes());
            RawValue { name: "rules".into(), schema: "<name:str,n:u32>".into(), bytes: b, text: None }
        };
        let op = |op: u32, v: RawValue, anchor: &str| RawOp { op, value: v, anchor: anchor.into() };
        let mut e = Entry::default();
        let mut errors = Vec::new();
        e.apply_test(&op(vo::PUSH, item("a", 1), ""), &mut errors);
        e.apply_test(&op(vo::PUSH, item("c", 3), ""), &mut errors);
        e.apply_test(&op(vo::INSERT_BEFORE, item("b", 2), "c"), &mut errors);
        e.apply_test(&op(vo::REMOVE_ITEM, item("", 0), "a"), &mut errors);
        e.apply_test(&op(vo::REPLACE_ITEM, item("c", 30), "#1"), &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        let v = e.value("rules").unwrap();
        let names: Vec<_> = data::items(&v.schema, &v.bytes)
            .unwrap()
            .into_iter()
            .map(|it| {
                (data::item_name("<name:str,n:u32>", it).unwrap().to_string(), u32::from_le_bytes(it[it.len() - 4..].try_into().unwrap()))
            })
            .collect();
        assert_eq!(names, [("b".to_string(), 2), ("c".to_string(), 30)]);
    }
}
