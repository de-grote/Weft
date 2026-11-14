//! Components and tags: typed data on blocks, items, entity kinds, biomes and dimensions, shared
//! between mods by name. Names without a prefix are the common vocabulary (`fuel`, `plant_soil`,
//! `ingots/iron`); `mymod:name` is private to a mod.
//!
//! ```ignore
//! pub static FUEL: Component<u32> = Component::named("fuel");
//! pub static PLANT_SOIL: Tag = Tag::named("plant_soil");
//! r.item(&PEAT).set(&FUEL, 800);
//! r.set("#logs", &FUEL, 300);                        // every block and item tagged `logs`
//! if let Some(ticks) = FUEL.on_item(stack.item) { .. }
//! if PLANT_SOIL.on_block(w.get(below)) { .. }
//! ```
//! Lookups read the engine's frozen tables directly (one load and a bit test, no host call).
//! Every use of a name must agree on its value type; the engine checks that at startup and names
//! the mod and entry that disagree.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use crate::abi::{self, ItemStack, Str, entry_kind as ek};
use crate::host;

pub use abi::data::Data;

/// What a component of a registration can hold (`abi::data`): numbers, `bool`, text (`String`),
/// lists (`Vec`), callbacks, engine structs, structs declared with [`value!`](crate::value)
/// (plain) or [`abi::data!`] (with text and lists).
///
/// `Value` is the plain kind: `repr(C)` data an entity, a stack or an event carries and a system
/// changes in place.
///
/// # Safety
/// The type must be `repr(C)` plain data valid for whatever bytes another mod wrote with the same
/// schema.
pub unsafe trait Value: Data + Copy {}

macro_rules! plain {
    ($($t:ty),*) => {
        $(unsafe impl Value for $t {})*
    };
}
plain!(u8, i8, u16, i16, u32, i32, u64, i64, f32, f64, bool);

unsafe impl<T: Value, const N: usize> Value for [T; N] {}

/// `Data` of a `repr(C)` value, its own bytes.
#[macro_export]
#[doc(hidden)]
macro_rules! raw_data {
    ($t:ty, $schema:expr) => {
        impl $crate::abi::data::Data for $t {
            const FIXED: bool = true;
            fn schema() -> String {
                ($schema).into()
            }
            fn write(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(unsafe { std::slice::from_raw_parts(self as *const Self as *const u8, size_of::<Self>()) });
            }
            fn read(r: &mut &[u8]) -> Option<Self> {
                let b = r.get(..size_of::<Self>())?;
                *r = &r[size_of::<Self>()..];
                Some(unsafe { std::ptr::read_unaligned(b.as_ptr() as *const Self) })
            }
        }
    };
}

unsafe impl Value for abi::Worn {}
unsafe impl Value for abi::Energy {}

/// A stable text view (`str` schema). Values serialize as UTF-8, never as pointers.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Text(Str);

impl Text {
    /// A text value from a static string.
    pub const fn new(s: &'static str) -> Text {
        Text(Str::new(s))
    }
    pub fn as_str(&self) -> &'static str {
        // Values read from the frozen tables live as long as the game.
        unsafe { self.0.as_str() }
    }
}

impl Data for Text {
    const FIXED: bool = false;
    fn schema() -> String {
        "str".into()
    }
    fn write(&self, out: &mut Vec<u8>) {
        self.as_str().to_string().write(out);
    }
    fn read(r: &mut &[u8]) -> Option<Self> {
        Some(String::read(r)?.into())
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        static TEXTS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> =
            std::sync::LazyLock::new(Default::default);
        let mut texts = TEXTS.lock().unwrap();
        let text = match texts.get(text.as_str()) {
            Some(&text) => text,
            None => {
                let text: &'static str = Box::leak(text.into_boxed_str());
                texts.insert(text);
                text
            }
        };
        Text(Str::new(text))
    }
}

/// Values written as text and read as something else: `Text` stays text, `BlockRef`,
/// `BlockList` and `ItemRef` name things that the engine resolves when the game loads (a typo is
/// a load error).
pub trait TextValue: Data {}
impl TextValue for Text {}
impl TextValue for BlockRef {}
impl TextValue for ItemRef {}
impl TextValue for BlockList {}
impl TextValue for Matcher {}
impl TextValue for TintRef {}

/// Which blocks, as a component names them: a block matcher (`set_text(&SUPPORT, "#plant_soil
/// !base:sand")`, syntax in `docs/modding.md`), compiled once when the game loads and read back
/// as its id.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matcher(pub u32);

impl Matcher {
    /// Whether it takes block state `state`.
    pub fn has(&self, state: u32) -> bool {
        unsafe { (host().matcher_has)(self.0, state) != 0 }
    }
}

unsafe impl Value for Matcher {}
raw_data!(Matcher, "match");

/// A tint kind a component names (`core:grass`), read back as its number.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TintRef(pub u32);

unsafe impl Value for TintRef {}
raw_data!(TintRef, "tint");

/// A block (or a state, `base:furnace[lit=true]`) a component names: `set_text(&TREE_LEAVES,
/// "oak_leaves")`, read back as its state id.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockRef(pub u32);

unsafe impl Value for BlockRef {}
raw_data!(BlockRef, "block");

/// An item a component names (`set_text(&REMAINDER, "bucket")`), read back as its id.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemRef(pub u32);

unsafe impl Value for ItemRef {}
raw_data!(ItemRef, "item");

/// Blocks with weights a component names: `set_text(&TREE_KINDS, "birch_log:1 oak_log:4")`
/// (weight 1 when left out), resolved when the game loads.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockList(pub u32);

unsafe impl Value for BlockList {}
raw_data!(BlockList, "blocks");

impl BlockList {
    /// (state, weight) pairs.
    pub fn entries(&self) -> &'static [[u32; 2]] {
        unsafe { (host().block_list)(self.0).as_slice() }
    }
    /// One state by its weight, `roll` being any random number.
    pub fn pick(&self, roll: u64) -> Option<u32> {
        let e = self.entries();
        let total: u64 = e.iter().map(|x| x[1] as u64).sum();
        if total == 0 {
            return None;
        }
        let mut r = roll % total;
        for x in e {
            if r < x[1] as u64 {
                return Some(x[0]);
            }
            r -= x[1] as u64;
        }
        None
    }
}

/// Declares a struct usable as a component value; its schema lists the fields, so two mods that
/// declare the same struct agree without depending on each other.
///
/// ```ignore
/// weft_sdk::value! {
///     pub struct Food { pub hunger: u32, pub saturation: f32 }
/// }
/// ```
#[macro_export]
macro_rules! value {
    ($(#[$m:meta])* $vis:vis struct $name:ident { $($(#[$fm:meta])* $fvis:vis $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$m])*
        #[repr(C)]
        #[derive(Clone, Copy, Debug, PartialEq)]
        $vis struct $name { $($(#[$fm])* $fvis $field: $ty),* }
        unsafe impl $crate::Value for $name {}
        $crate::raw_data!($name, {
            let fields: Vec<String> = vec![$(format!("{}:{}", stringify!($field), <$ty as $crate::abi::data::Data>::schema())),*];
            format!("{{{}}}", fields.join(","))
        });
    };
}

static TABLE: AtomicPtr<abi::ComponentTable> = AtomicPtr::new(std::ptr::null_mut());

fn component_table() -> Option<&'static abi::ComponentTable> {
    let mut p = TABLE.load(Ordering::Relaxed);
    if p.is_null() {
        p = unsafe { (host().component_table)() } as *mut _;
        if p.is_null() {
            return None;
        }
        TABLE.store(p, Ordering::Relaxed);
    }
    Some(unsafe { &*p })
}

#[inline]
fn table_has(kind: u32, entry: u32, comp: u32) -> bool {
    let Some(t) = component_table() else { return false };
    let k = &t.kinds[kind as usize];
    if entry >= k.entries || comp >= t.components {
        return false;
    }
    unsafe { *k.bits.add((entry * k.words + comp / 64) as usize) >> (comp % 64) & 1 != 0 }
}

/// How many entries of `kind` (`abi::entry_kind`) there are, blocks counted by state; 0 before
/// the registries are frozen.
pub fn entry_count(kind: u32) -> u32 {
    component_table()
        .and_then(|t| t.kinds.get(kind as usize))
        .map_or(0, |k| k.entries)
}

/// Name and schema (empty: a tag) of component `id`.
pub fn component_info(id: u32) -> Option<(&'static str, &'static str)> {
    let (mut name, mut schema) = (Str::EMPTY, Str::EMPTY);
    (unsafe { (host().component_info)(id, &mut name, &mut schema) } != 0).then(|| unsafe { (name.as_str(), schema.as_str()) })
}

/// Every component and tag entry `id` of `kind` has, by name and schema: what a thing is, for a
/// mod showing it (WAILA) or building on any mod's things. Values are read with the components'
/// handles, `Component::on_block` and the like.
pub fn components_of(kind: u32, id: u32) -> Vec<(&'static str, &'static str)> {
    let Some(t) = component_table() else { return Vec::new() };
    (0..t.components)
        .filter(|&c| table_has(kind, id, c))
        .filter_map(component_info)
        .collect()
}

/// Every entry of `kind` with component or tag `name`: all logs (`entries_with(BLOCK, "logs")`,
/// one per state), every ore.
pub fn entries_with(kind: u32, name: &str) -> Vec<u32> {
    let Some(t) = component_table() else { return Vec::new() };
    let Some(comp) = (0..t.components).find(|&c| component_info(c).is_some_and(|(n, _)| n == name)) else { return Vec::new() };
    (0..entry_count(kind))
        .filter(|&e| table_has(kind, e, comp))
        .collect()
}

const UNRESOLVED: u32 = u32::MAX - 1;
const MISSING: u32 = u32::MAX;
/// Kinds whose place in the table a handle caches (block to dimension: what hot code reads);
/// the others are looked up on each read.
const CACHED: usize = ek::DIMENSION as usize + 1;

static NO_BITS: u64 = 0;

/// Where one component sits in one kind's table, cached in its handle on first use so a check
/// is one dependent load.
struct Slot {
    /// The word holding this component's bit in entry 0's row; null until filled.
    bits: AtomicPtr<u64>,
    words: AtomicU32,
    entries: AtomicU32,
    data: AtomicPtr<u8>,
    stride: AtomicU32,
}

impl Slot {
    const fn new() -> Slot {
        Slot {
            bits: AtomicPtr::new(std::ptr::null_mut()),
            words: AtomicU32::new(0),
            entries: AtomicU32::new(0),
            data: AtomicPtr::new(std::ptr::null_mut()),
            stride: AtomicU32::new(0),
        }
    }
}

struct Lookup {
    id: AtomicU32,
    slots: [Slot; CACHED],
}

/// Where one component sits in one kind's table: its bit's word in entry 0's row.
struct Place {
    bits: *const u64,
    words: u32,
    entries: u32,
    data: *const u8,
    stride: u32,
}

impl Lookup {
    const fn new() -> Lookup {
        Lookup { id: AtomicU32::new(UNRESOLVED), slots: [const { Slot::new() }; CACHED] }
    }

    fn id(&self, name: &str, schema: &dyn Fn() -> String) -> u32 {
        let id = self.id.load(Ordering::Relaxed);
        if id != UNRESOLVED {
            return id;
        }
        if component_table().is_none() {
            // Registration is still running: nothing to look up yet.
            return MISSING;
        }
        let id = unsafe { (host().find_component)(Str::new(name), Str::new(&schema())) };
        self.id.store(id, Ordering::Relaxed);
        id
    }

    /// Where it sits in `kind`'s table: cached in the handle for the first kinds.
    #[inline]
    fn place(&self, kind: u32, name: &str, schema: &dyn Fn() -> String) -> Option<Place> {
        let Some(s) = self.slots.get(kind as usize) else { return self.find(kind, name, schema) };
        let mut bits = s.bits.load(Ordering::Acquire);
        if bits.is_null() {
            bits = self.fill(kind, name, schema);
            if bits.is_null() {
                return None;
            }
        }
        Some(Place {
            bits,
            words: s.words.load(Ordering::Relaxed),
            entries: s.entries.load(Ordering::Relaxed),
            data: s.data.load(Ordering::Relaxed),
            stride: s.stride.load(Ordering::Relaxed),
        })
    }

    fn find(&self, kind: u32, name: &str, schema: &dyn Fn() -> String) -> Option<Place> {
        let id = self.id(name, schema);
        let t = component_table()?;
        let k = t.kinds.get(kind as usize)?;
        if id >= t.components {
            // Nothing uses it: every check says no.
            return Some(Place { bits: &NO_BITS, words: 0, entries: 0, data: std::ptr::null(), stride: 0 });
        }
        let col = unsafe { *k.columns.add(id as usize) };
        Some(Place {
            bits: unsafe { k.bits.add((id / 64) as usize) },
            words: k.words,
            entries: k.entries,
            data: col.data,
            stride: col.stride,
        })
    }

    #[inline]
    fn has(&self, kind: u32, entry: u32, name: &str, schema: &dyn Fn() -> String) -> bool {
        self.place(kind, name, schema)
            .is_some_and(|p| self.has_at(&p, entry))
    }

    #[inline]
    fn has_at(&self, p: &Place, entry: u32) -> bool {
        entry < p.entries && unsafe { *p.bits.add((entry * p.words) as usize) } >> (self.id.load(Ordering::Relaxed) % 64) & 1 != 0
    }

    #[inline]
    fn read<T: Data>(&self, kind: u32, entry: u32, name: &str, schema: &dyn Fn() -> String) -> Option<T> {
        let p = self.place(kind, name, schema)?;
        if !self.has_at(&p, entry) {
            return None;
        }
        let stride = p.stride as usize;
        let at = unsafe { p.data.add(entry as usize * stride) };
        let mut cell = unsafe { std::slice::from_raw_parts(at as *const u8, stride) };
        // Frozen text already owns a stable view; no decoding or interning is needed.
        if std::any::TypeId::of::<T>() == std::any::TypeId::of::<Text>() {
            return (stride == size_of::<Text>()).then(|| unsafe { std::ptr::read_unaligned(at as *const T) });
        }
        if T::FIXED {
            return (stride == size_of::<T>())
                .then(|| T::read(&mut cell))
                .flatten();
        }
        // Text is kept as a `Str`, other packed values as a `Slice` of their bytes.
        let (ptr, len) = unsafe { std::ptr::read_unaligned(at as *const (*const u8, usize)) };
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        if abi::data::is_text(schema_of::<T>()) {
            let mut packed = (len as u32).to_le_bytes().to_vec();
            packed.extend_from_slice(bytes);
            return T::read(&mut packed.as_slice());
        }
        T::read(&mut &bytes[..])
    }

    #[cold]
    fn fill(&self, kind: u32, name: &str, schema: &dyn Fn() -> String) -> *mut u64 {
        let Some(p) = self.find(kind, name, schema) else { return std::ptr::null_mut() };
        let s = &self.slots[kind as usize];
        s.words.store(p.words, Ordering::Relaxed);
        s.entries.store(p.entries, Ordering::Relaxed);
        s.data.store(p.data as *mut u8, Ordering::Relaxed);
        s.stride.store(p.stride, Ordering::Relaxed);
        s.bits.store(p.bits as *mut u64, Ordering::Release);
        p.bits as *mut u64
    }
}

/// Typed handle to a component, found by name the first time it is used. Component and tag
/// names are a vocabulary shared by all mods, not things a mod owns: `fuel` means the same
/// to everyone and is used as written, a `modid:` prefix only keeps a term to one mod. `K` is
/// the kind whose registrations take it (`crate::kind`), any kind by default.
pub struct Component<T: Data, K = crate::kind::Any> {
    name: &'static str,
    lookup: Lookup,
    _p: PhantomData<fn() -> (T, K)>,
}

impl<T: Data, K> Component<T, K> {
    pub const fn named(name: &'static str) -> Self {
        Component { name, lookup: Lookup::new(), _p: PhantomData }
    }
    pub fn name(&self) -> &'static str {
        self.name
    }
    /// Its declaration, for `Registry::edit`: `r.edit(HUNGER.declaration())`.
    pub fn declaration(&self) -> &'static crate::Declaration {
        crate::Declaration::as_written(self.name)
    }
    /// `u32::MAX` if nothing uses it (or before the registries are frozen).
    #[inline]
    pub fn id(&self) -> u32 {
        self.lookup.id(self.name, &T::schema)
    }
    /// Its value on entry `id` of `kind` (`abi::entry_kind`): a weather's, a game mode's, any
    /// kind's but word sets and component declarations.
    #[inline]
    pub fn on(&self, kind: u32, id: u32) -> Option<T> {
        self.lookup.read(kind, id, self.name, &T::schema)
    }
    #[inline]
    pub fn on_block(&self, state: u32) -> Option<T> {
        self.on(ek::BLOCK, state)
    }
    #[inline]
    pub fn on_item(&self, item: u32) -> Option<T> {
        self.on(ek::ITEM, item)
    }
    /// Its value on a record (`Registry::record`).
    #[inline]
    pub fn on_record(&self, record: u32) -> Option<T> {
        self.on(ek::RECORD, record)
    }
    #[inline]
    pub fn on_entity(&self, kind: u32) -> Option<T> {
        self.on(ek::ENTITY, kind)
    }
    #[inline]
    pub fn on_biome(&self, biome: u32) -> Option<T> {
        self.on(ek::BIOME, biome)
    }
    #[inline]
    pub fn on_dimension(&self, dim: u32) -> Option<T> {
        self.on(ek::DIMENSION, dim)
    }
    /// The stack's own value (damage taken, charge ...), else its item's.
    pub fn on_stack(&self, stack: &ItemStack) -> Option<T> {
        self.on_stack_own(stack)
            .or_else(|| self.on_item(stack.item))
    }
    /// What a stack's item has, nothing for an empty stack: fuel in a fuel slot.
    pub fn on_stack_item(&self, stack: &ItemStack) -> Option<T> {
        if stack.is_empty() { None } else { self.on_item(stack.item) }
    }
    /// Only what the stack itself carries.
    pub fn on_stack_own(&self, stack: &ItemStack) -> Option<T> {
        if stack.data == 0 {
            return None;
        }
        let mut len = 0usize;
        let p = unsafe { (host().stack_get)(stack.data, self.id(), &mut len) };
        (!p.is_null())
            .then(|| T::read(&mut unsafe { std::slice::from_raw_parts(p, len) }))
            .flatten()
    }
    pub(crate) fn raw_value(&self, v: &T) -> crate::content::Pending {
        crate::content::Pending::value(self.name, schema_of::<T>(), abi::data::bytes_of(v))
    }
}

/// `T::schema()`, worked out once per type: a packed struct's is built of its fields' every time.
pub(crate) fn schema_of<T: Data>() -> &'static str {
    thread_local! {
        static KNOWN: std::cell::RefCell<std::collections::HashMap<std::any::TypeId, &'static str>> = Default::default();
    }
    KNOWN.with(|k| {
        *k.borrow_mut()
            .entry(std::any::TypeId::of::<T>())
            .or_insert_with(|| T::schema().leak())
    })
}

fn tag_schema() -> String {
    String::new()
}

fn storage_schema() -> String {
    "storage".into()
}

/// An inventory a holder keeps besides its own, under a name: each player's ender chest, a
/// second pocket of a backpack. Declared with `r.storage(&S, slots)`; entity kinds carry it with
/// `.attach_storage(&S)` (`r.edit(&PLAYER).attach_storage(&S)` for another mod's kind), stacks
/// always can.
/// `w.inventory(S.of(holder))` reaches it and `w.open_menu(player, &MENU, S.of(holder))` shows
/// it, its `.holder()` groups being the storage's slots. Saved with its holder, and kept when a
/// player respawns.
pub struct Storage {
    name: &'static str,
    lookup: Lookup,
}

impl Storage {
    pub const fn named(name: &'static str) -> Self {
        Storage { name, lookup: Lookup::new() }
    }
    pub fn name(&self) -> &'static str {
        self.name
    }
    /// Its declaration, for `Registry::edit`.
    pub fn declaration(&self) -> &'static crate::Declaration {
        crate::Declaration::as_written(self.name)
    }
    /// `u32::MAX` before the registries are frozen or if nothing declared it.
    #[inline]
    pub fn id(&self) -> u32 {
        self.lookup.id(self.name, &storage_schema)
    }
    /// Its slots, 0 before the registries are frozen.
    pub fn slots(&self) -> u32 {
        unsafe { (host().storage_slots)(self.id()) }
    }
    /// This storage of `h` (an entity, a block, a held stack), for inventory calls and menus.
    pub fn of(&self, h: impl Into<crate::Holder>) -> crate::Holder {
        crate::Holder { storage: self.id(), ..h.into() }
    }
}

impl<T: Data, K: crate::kind::HasId> Component<T, K> {
    /// Its value on entry `id` of its kind: `FALLS.on_id(w.weather())`.
    #[inline]
    pub fn on_id(&self, id: u32) -> Option<T> {
        self.on(K::KIND, id)
    }
}

/// A component without a value: membership in a group (`plant_soil`, `logs`, `ingots/iron`).
/// `K` is the kind it is about (`crate::kind`), any kind by default, like `Component`'s.
pub struct Tag<K = crate::kind::Any> {
    name: &'static str,
    lookup: Lookup,
    _p: PhantomData<fn() -> K>,
}

impl<K: crate::kind::HasId> Tag<K> {
    /// Entry `id` of its kind has it: `DRY.on_id(w.weather())`.
    #[inline]
    pub fn on_id(&self, id: u32) -> bool {
        self.on(K::KIND, id)
    }
}

impl<K> Tag<K> {
    pub const fn named(name: &'static str) -> Self {
        Tag { name, lookup: Lookup::new(), _p: PhantomData }
    }
    pub fn name(&self) -> &'static str {
        self.name
    }
    #[inline]
    pub fn id(&self) -> u32 {
        self.lookup.id(self.name, &tag_schema)
    }
    /// Entry `id` of `kind` (`abi::entry_kind`) has it.
    #[inline]
    pub fn on(&self, kind: u32, id: u32) -> bool {
        self.lookup.has(kind, id, self.name, &tag_schema)
    }
    #[inline]
    pub fn on_block(&self, state: u32) -> bool {
        self.on(ek::BLOCK, state)
    }
    #[inline]
    pub fn on_item(&self, item: u32) -> bool {
        self.on(ek::ITEM, item)
    }
    /// A record (`Registry::record`) has it.
    #[inline]
    pub fn on_record(&self, record: u32) -> bool {
        self.on(ek::RECORD, record)
    }
    #[inline]
    pub fn on_entity(&self, kind: u32) -> bool {
        self.on(ek::ENTITY, kind)
    }
    #[inline]
    pub fn on_biome(&self, biome: u32) -> bool {
        self.on(ek::BIOME, biome)
    }
    #[inline]
    pub fn on_dimension(&self, dim: u32) -> bool {
        self.on(ek::DIMENSION, dim)
    }
}

/// By name, for rare lookups; a static `Tag` (`Tag::on_block`) is the fast way.
pub fn block_has_tag(state: u32, tag: &str) -> bool {
    let id = unsafe { (host().find_component)(Str::new(tag), Str::new("")) };
    table_has(ek::BLOCK, state, id)
}

/// By name, for rare lookups; a static `Tag` (`Tag::on_item`) is the fast way.
pub fn item_has_tag(item: u32, tag: &str) -> bool {
    let id = unsafe { (host().find_component)(Str::new(tag), Str::new("")) };
    table_has(ek::ITEM, item, id)
}

pub(crate) fn value_bytes<T: Copy>(v: &T) -> Vec<u8> {
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, size_of::<T>()).to_vec() }
}

/// Stack data (see `ItemStack::data`) beyond what `Component::on_stack` reads.
/// The stack's own name, if a player gave it one (`stack_with_name`).
pub fn stack_name(stack: &ItemStack) -> Option<String> {
    let id = NAME.id();
    if stack.data == 0 || id == u32::MAX {
        return None;
    }
    let mut len = 0usize;
    let p = unsafe { (host().stack_get)(stack.data, id, &mut len) };
    (!p.is_null()).then(|| String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(p, len) }).into_owned())
}

/// The stack named `name` (`None` or empty: its item's name again). It shows in its tooltip and
/// above the hotbar, and a block placed from it keeps the name (a chest's window title) until
/// it is broken back into an item.
pub fn stack_with_name(stack: ItemStack, name: Option<&str>) -> ItemStack {
    let b = name.filter(|n| !n.is_empty()).map(str::as_bytes);
    let data = unsafe { (host().stack_with)(stack.data, NAME.id(), b.map_or(abi::Slice::EMPTY, abi::Slice::new), b.is_none() as u32) };
    ItemStack { data, ..stack }
}

static NAME: Tag = Tag::named(abi::comp::NAME);
static LAYERS: Component<Text> = Component::named(abi::comp::LAYERS);

pub trait StackData: Sized {
    /// This stack with its own value of `c` set.
    fn with<T: Data>(self, c: &Component<T>, v: T) -> Self;
    fn without<T: Data>(self, c: &Component<T>) -> Self;
    /// `count` less, empty when nothing is left.
    fn shrink(self, n: u32) -> Self;
    fn grow(self, n: u32) -> Self;
    /// Wear taken (the `wear` stack component).
    fn wear(&self) -> u32;
    fn with_wear(self, d: u32) -> Self;
    /// `amount` more wear, nothing left when that uses it up; what has no durability stays.
    fn add_wear(self, amount: u32) -> Self;
    /// How many more results of `out` this output slot takes.
    fn room_for(&self, out: ItemStack) -> u32;
    /// This output slot with `n` results of `out` more.
    fn merged(self, out: ItemStack, n: u32) -> Self;
    /// What the stack keeps in `storage` (`r.storage`), one per slot: a tool's parts, each a
    /// stack with data and storages of its own, as deep as a mod goes.
    fn stacks(&self, storage: &Storage) -> Vec<ItemStack>;
    /// The stack keeping `list` in `storage`, as many as it has slots; the rest are emptied.
    fn with_stacks(self, storage: &Storage, list: &[ItemStack]) -> Self;
    /// The stack showing `layers` instead of its item's look: a tool made of parts
    /// (`Layers::new().layer("handle").layer("head").tint(0xFF8800)`). Empty: its item's look
    /// again. In slots, in the hand and on the ground alike.
    fn with_layers(self, layers: Layers) -> Self;
    /// The stack giving `list` while held or worn instead of its item's modifiers (an enchanted
    /// sword's `Modifier::add(&attribute::ATTACK_DAMAGE, 2.0)`). Empty: its item's again.
    fn with_modifiers(self, list: &[crate::Modifier]) -> Self;
}

/// How an item or a stack looks (`abi::comp::LAYERS`): pictures one over the other, the first
/// at the bottom, each tinted or not. The same for an item (`ItemBuilder::layer`) and a stack
/// (`with_layers`).
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Layers(pub(crate) Vec<(String, Option<u32>, Option<String>)>);

impl Layers {
    pub fn new() -> Layers {
        Layers::default()
    }
    /// One more picture on top (under `item/`, `"other:x"` another mod's).
    pub fn layer(mut self, texture: &str) -> Layers {
        self.0.push((crate::asset("item", texture), None, None));
        self
    }
    /// Tints the layer just added `0xRRGGBB` (multiplied: white parts take the color).
    pub fn tint(mut self, color: u32) -> Layers {
        if let Some(last) = self.0.last_mut() {
            last.1 = Some(color & 0xFF_FFFF);
        }
        self
    }
    /// Tints the layer just added by a tint kind's default color (`tints::GRASS`): an item
    /// looking like the block it places away from any biome.
    pub fn tint_kind(mut self, kind: &crate::TintKind) -> Layers {
        if let Some(last) = self.0.last_mut() {
            last.2 = Some(kind.name().to_string());
        }
        self
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub(crate) fn text(&self) -> String {
        abi::layers_text(
            self.0
                .iter()
                .map(|(name, tint, kind)| abi::Layer { name, tint: *tint, kind: kind.as_deref() }),
        )
    }
    /// From `text` (full names, as `text` gives them).
    pub(crate) fn parse(text: &str) -> Layers {
        Layers(
            abi::layers(text)
                .map(|l| (l.name.to_string(), l.tint, l.kind.map(str::to_string)))
                .collect(),
        )
    }
}

/// Bytes a stack takes in a storage's value: item, count, data.
const STACK_BYTES: usize = 12;

impl StackData for ItemStack {
    fn with<T: Data>(self, c: &Component<T>, v: T) -> Self {
        let b = abi::data::bytes_of(&v);
        ItemStack { data: unsafe { (host().stack_with)(self.data, c.id(), abi::Slice::new(&b), 0) }, ..self }
    }
    fn without<T: Data>(self, c: &Component<T>) -> Self {
        ItemStack { data: unsafe { (host().stack_with)(self.data, c.id(), abi::Slice::EMPTY, 1) }, ..self }
    }
    fn shrink(self, n: u32) -> Self {
        if self.count <= n { ItemStack::EMPTY } else { ItemStack { count: self.count - n, ..self } }
    }
    fn grow(self, n: u32) -> Self {
        ItemStack { count: self.count + n, ..self }
    }
    fn wear(&self) -> u32 {
        DAMAGE.on_stack_own(self).unwrap_or(0)
    }
    fn with_wear(self, d: u32) -> Self {
        if d == 0 { self.without(&DAMAGE) } else { self.with(&DAMAGE, d) }
    }
    fn add_wear(self, amount: u32) -> Self {
        let durability = DURABILITY.on_stack(&self).unwrap_or(0);
        if durability == 0 {
            return self;
        }
        let damage = self.wear() + amount;
        if damage >= durability { ItemStack::EMPTY } else { self.with_wear(damage) }
    }
    fn room_for(&self, out: ItemStack) -> u32 {
        let max = MAX_STACK.on_stack(&out).unwrap_or(64).max(1);
        let room = if self.is_empty() {
            max
        } else if self.stacks_with(&out) {
            max.saturating_sub(self.count)
        } else {
            0
        };
        room / out.count.max(1)
    }
    fn merged(self, out: ItemStack, n: u32) -> Self {
        if self.is_empty() { ItemStack { count: out.count * n, ..out } } else { self.grow(out.count * n) }
    }
    fn stacks(&self, storage: &Storage) -> Vec<ItemStack> {
        let slots = storage.slots();
        let mut len = 0usize;
        let p = if self.data == 0 { std::ptr::null() } else { unsafe { (host().stack_get)(self.data, storage.id(), &mut len) } };
        let bytes = if p.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(p, len) } };
        let word = |i: usize| {
            bytes
                .get(i..i + 4)
                .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        (0..slots as usize)
            .map(|i| {
                let at = i * STACK_BYTES;
                let s = ItemStack { item: word(at), count: word(at + 4), data: word(at + 8) };
                if s.is_empty() { ItemStack::EMPTY } else { s }
            })
            .collect()
    }
    fn with_layers(self, layers: Layers) -> Self {
        let text = layers.text();
        if text.is_empty() { self.without(&LAYERS) } else { self.with(&LAYERS, Text::from(text)) }
    }
    fn with_modifiers(self, list: &[crate::Modifier]) -> Self {
        let raw: Vec<abi::Modifier> = list.iter().map(|m| m.0.clone()).collect();
        let bytes = abi::data::bytes_of(&raw);
        let data = unsafe { (host().stack_with)(self.data, MODIFIERS.id(), abi::Slice::new(&bytes), raw.is_empty() as u32) };
        ItemStack { data, ..self }
    }
    fn with_stacks(self, storage: &Storage, list: &[ItemStack]) -> Self {
        let slots = storage.slots() as usize;
        if list.iter().take(slots).all(ItemStack::is_empty) {
            return ItemStack { data: unsafe { (host().stack_with)(self.data, storage.id(), abi::Slice::EMPTY, 1) }, ..self };
        }
        let b: Vec<u8> = (0..slots)
            .flat_map(|i| {
                let s = list
                    .get(i)
                    .copied()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(ItemStack::EMPTY);
                [s.item, s.count, s.data]
            })
            .flat_map(u32::to_le_bytes)
            .collect();
        ItemStack { data: unsafe { (host().stack_with)(self.data, storage.id(), abi::Slice::new(&b), 0) }, ..self }
    }
}

// The components the engine reads itself.
pub static MAX_STACK: Component<u32> = Component::named(abi::comp::MAX_STACK);
pub static DURABILITY: Component<u32> = Component::named(abi::comp::DURABILITY);
pub static ATTACK_DAMAGE: Component<f32> = Component::named(abi::comp::ATTACK_DAMAGE);
pub static ATTACK_SPEED: Component<f32> = Component::named(abi::comp::ATTACK_SPEED);
pub static USE_TICKS: Component<u32> = Component::named(abi::comp::USE_TICKS);
pub static GIVES_EFFECTS: Component<Vec<abi::EffectGift>> = Component::named(abi::comp::GIVES_EFFECTS);
pub static INVENTORY_SLOTS: Component<u32> = Component::named(abi::comp::INVENTORY);
pub static PRECIPITATION: Component<Text> = Component::named(abi::comp::PRECIPITATION);
/// What liquid an item is, by the millibucket (`str`): every liquid's own item has it.
pub static FLUID: Component<Text> = Component::named(abi::comp::FLUID);
pub static MINING_SPEED: Component<f32> = Component::named(abi::comp::MINING_SPEED);
pub static TOOL_LEVEL: Component<u32> = Component::named(abi::comp::TOOL_LEVEL);
pub static HARVEST_LEVEL: Component<u32> = Component::named(abi::comp::HARVEST_LEVEL);
pub static USE_PRIORITY: Component<i32> = Component::named(abi::comp::USE_PRIORITY);
pub static DAMAGE: Component<u32> = Component::named(abi::comp::WEAR);
pub static LOOT_ROLLS: Component<u32> = Component::named(abi::comp::LOOT_ROLLS);
pub static HARDNESS: Component<f32> = Component::named(abi::comp::HARDNESS);
pub static FRICTION: Component<f32> = Component::named(abi::comp::FRICTION);
pub static DRAG: Component<f32> = Component::named(abi::comp::DRAG);
pub static LIGHT: Component<[u8; 3]> = Component::named(abi::comp::LIGHT);
/// A block's tint kind and the sides it is on, to read: the engine sets them from its faces'
/// layers (`tint_kind`).
pub static TINT: Component<TintRef> = Component::named(abi::comp::TINT);
pub static TINT_FACES: Component<u8> = Component::named(abi::comp::TINT_FACES);
pub static SPREADS_TO: Component<Matcher> = Component::named(abi::comp::SPREADS_TO);
pub static SPREAD_TIME: Component<f32> = Component::named(abi::comp::SPREAD_TIME);
pub static SPREAD_WHEN: Component<Vec<String>> = Component::named(abi::comp::SPREAD_WHEN);
pub static SPREAD_REACH: Component<[i32; 6]> = Component::named(abi::comp::SPREAD_REACH);
pub static AMBIENT_PARTICLE: Component<Text> = Component::named(abi::comp::AMBIENT_PARTICLE);
pub static DRIP: Component<Text> = Component::named(abi::comp::DRIP);
pub static SPLASH: Component<Text> = Component::named(abi::comp::SPLASH);
pub static BUBBLE: Component<Text> = Component::named(abi::comp::BUBBLE);
pub static SWIMS_IN: Component<Matcher> = Component::named(abi::comp::SWIMS_IN);
pub static CONDUCTOR: Tag = Tag::named(abi::comp::CONDUCTOR);
pub static ENERGY: Component<abi::Energy> = Component::named(abi::comp::ENERGY);
pub static WORN: Component<abi::Worn> = Component::named(abi::comp::WORN);
pub static WORN_TEXTURE: Component<Text> = Component::named(abi::comp::WORN_TEXTURE);
pub static WORN_MODEL: Component<Text> = Component::named(abi::comp::WORN_MODEL);
pub static WEAR_PER_HIT: Component<u32> = Component::named(abi::comp::WEAR_PER_HIT);
pub static LIQUID: Component<Text> = Component::named(abi::comp::LIQUID);
pub static REMAINDER: Component<ItemRef> = Component::named(abi::comp::REMAINDER);
pub static SHADER: Component<Text> = Component::named(abi::comp::SHADER);
pub static MODEL: Component<Text> = Component::named(abi::comp::MODEL);
pub static ANIMATIONS: Component<Text> = Component::named(abi::comp::ANIMATIONS);
pub static SOUND: Component<Text> = Component::named(abi::comp::SOUND);
pub static EFFECTS: Component<Vec<abi::Effect>> = Component::named(abi::comp::EFFECTS);
pub static MODIFIERS: Component<Vec<abi::Modifier>> = Component::named(abi::comp::MODIFIERS);

/// An `abi::Effect`, its sound event and particle kind by full name.
pub(crate) fn effect(event: &str, sound: &str, particle: &str) -> abi::Effect {
    let name = |n: &str| if matches!(n, "" | "none") { n.to_string() } else { crate::full_name(n) };
    abi::Effect { event: event.to_string(), sound: name(sound), particle: name(particle) }
}

/// The stack after one of it is used up (burnt as fuel, taken by a recipe): one less, and the
/// last one leaves its `remainder` behind (a lava bucket its bucket).
pub fn consume_one(stack: ItemStack) -> ItemStack {
    match REMAINDER.on_item(stack.item) {
        Some(left) if stack.count == 1 => ItemStack::new(left.0, 1),
        _ => stack.shrink(1),
    }
}

/// Stack size of an item (64 unless its `max_stack` says otherwise).
pub fn max_stack(item: u32) -> u32 {
    MAX_STACK.on_item(item).unwrap_or(64)
}

/// The block state an item places, if any.
pub fn item_block(item: u32) -> Option<u32> {
    let s = unsafe { (host().item_block)(item) };
    (s != 0).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_views_serialize_like_owned_text() {
        for s in ["", "base:armor/iron_1", "mod:model@body"] {
            let bytes = abi::data::bytes_of(&Text::new(s));
            assert_eq!(bytes, abi::data::bytes_of(&s.to_string()));
            let view: Text = abi::data::from_bytes(&bytes).unwrap();
            assert_eq!(view.as_str(), s);
            assert_eq!(abi::data::bytes_of(&view), bytes);
        }
    }
}
