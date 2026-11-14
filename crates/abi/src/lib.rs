//! The boundary between the engine and mods. Everything here is `repr(C)` so a mod built with
//! any rustc (or another language) can be loaded. Rules:
//! - A mod is loaded only if it was built against this exact file (`ABI_VERSION` is a hash of
//!   it), so layouts may change freely; mods are rebuilt with the engine.
//! - Descriptor structs passed to the host are read during the call only.
//! - Strings are UTF-8 `(ptr, len)`, never NUL terminated.
#![no_std]

extern crate alloc;

pub mod data;
pub mod environment;

use core::ffi::c_void;
use core::sync::atomic::AtomicU32;

/// FNV-1a of this file: any change to it (layouts included) makes mods built before it refuse
/// to load instead of reading the wrong memory.
pub const ABI_VERSION: u64 = fnv(concat!(include_str!("lib.rs"), include_str!("data.rs")).as_bytes());
/// Symbol every mod library exports: `extern "C" fn() -> *const ModDecl`.
pub const ENTRY_SYMBOL: &[u8] = b"weft_mod\0";

pub const fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h = (h ^ bytes[i] as u64).wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    h
}

/// A block's place in the world, or how far one block is from another.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const ZERO: BlockPos = BlockPos { x: 0, y: 0, z: 0 };

    pub const fn new(x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos { x, y, z }
    }

    /// This one moved by (`dx`, `dy`, `dz`).
    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> BlockPos {
        BlockPos { x: self.x + dx, y: self.y + dy, z: self.z + dz }
    }

    pub const fn above(self) -> BlockPos {
        self.offset(0, 1, 0)
    }

    pub const fn below(self) -> BlockPos {
        self.offset(0, -1, 0)
    }

    /// The middle of the block.
    pub fn center(self) -> Vec3 {
        Vec3 { x: self.x as f64 + 0.5, y: self.y as f64 + 0.5, z: self.z as f64 + 0.5 }
    }

    pub const fn to_array(self) -> [i32; 3] {
        [self.x, self.y, self.z]
    }
}

impl From<[i32; 3]> for BlockPos {
    fn from([x, y, z]: [i32; 3]) -> BlockPos {
        BlockPos { x, y, z }
    }
}

impl From<BlockPos> for [i32; 3] {
    fn from(p: BlockPos) -> [i32; 3] {
        p.to_array()
    }
}

impl core::ops::Add for BlockPos {
    type Output = BlockPos;
    fn add(self, o: BlockPos) -> BlockPos {
        self.offset(o.x, o.y, o.z)
    }
}

impl core::ops::Sub for BlockPos {
    type Output = BlockPos;
    fn sub(self, o: BlockPos) -> BlockPos {
        self.offset(-o.x, -o.y, -o.z)
    }
}

/// A point in the world in blocks, or a direction or speed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Schema for Vec3 {
    const SCHEMA: &'static str = "{x:f64,y:f64,z:f64}";
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

    pub const fn new(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    pub fn offset(self, dx: f64, dy: f64, dz: f64) -> Vec3 {
        Vec3 { x: self.x + dx, y: self.y + dy, z: self.z + dz }
    }

    /// The block it is in.
    pub fn block(self) -> BlockPos {
        // No `floor` here: this crate has no std.
        let floor = |v: f64| v as i32 - ((v as i32) as f64 > v) as i32;
        BlockPos { x: floor(self.x), y: floor(self.y), z: floor(self.z) }
    }

    pub fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub const fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
}

impl From<[f64; 3]> for Vec3 {
    fn from([x, y, z]: [f64; 3]) -> Vec3 {
        Vec3 { x, y, z }
    }
}

/// Directions and speeds made in single precision (a spread, a velocity).
impl From<[f32; 3]> for Vec3 {
    fn from([x, y, z]: [f32; 3]) -> Vec3 {
        Vec3 { x: x as f64, y: y as f64, z: z as f64 }
    }
}

impl From<Vec3> for [f64; 3] {
    fn from(v: Vec3) -> [f64; 3] {
        v.to_array()
    }
}

impl core::ops::Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        self.offset(o.x, o.y, o.z)
    }
}

impl core::ops::Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        self.offset(-o.x, -o.y, -o.z)
    }
}

impl core::ops::Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, k: f64) -> Vec3 {
        Vec3 { x: self.x * k, y: self.y * k, z: self.z * k }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Str {
    pub ptr: *const u8,
    pub len: usize,
}

unsafe impl Send for Str {}
unsafe impl Sync for Str {}

impl Str {
    pub const EMPTY: Str = Str { ptr: core::ptr::null(), len: 0 };

    pub const fn new(s: &str) -> Str {
        Str { ptr: s.as_ptr(), len: s.len() }
    }

    /// # Safety
    /// The pointer must reference valid UTF-8 for the chosen lifetime.
    pub unsafe fn as_str<'a>(&self) -> &'a str {
        if self.len == 0 {
            return "";
        }
        unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(self.ptr, self.len)) }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Slice<T> {
    pub ptr: *const T,
    pub len: usize,
}

unsafe impl<T: Sync> Sync for Slice<T> {}
unsafe impl<T: Send> Send for Slice<T> {}

impl<T> Slice<T> {
    pub const EMPTY: Slice<T> = Slice { ptr: core::ptr::null(), len: 0 };

    pub const fn new(s: &[T]) -> Slice<T> {
        Slice { ptr: s.as_ptr(), len: s.len() }
    }

    /// # Safety
    /// Must point to `len` valid elements for the chosen lifetime.
    pub unsafe fn as_slice<'a>(&self) -> &'a [T] {
        if self.len == 0 {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }
}

/// `DEFAULT` for descriptors, so mods fill in only what they set (`..X::DEFAULT`).
macro_rules! defaults {
    ($t:ident { $($f:ident: $v:expr),* $(,)? }) => {
        impl $t {
            pub const DEFAULT: Self = $t { $($f: $v),* };
        }
    };
}

/// A mod-side function plus its captured state. `args` points to the callback-specific args struct.
/// Returns 0 on success, anything else means the callback failed (the SDK catches panics).
pub type CallbackFn = unsafe extern "C" fn(data: *const c_void, args: *mut c_void) -> u32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Callback {
    pub func: Option<CallbackFn>,
    pub data: *const c_void,
}

unsafe impl Send for Callback {}
unsafe impl Sync for Callback {}

impl Default for Callback {
    fn default() -> Callback {
        Callback::NONE
    }
}

impl Callback {
    pub const NONE: Callback = Callback { func: None, data: core::ptr::null() };

    pub fn is_some(&self) -> bool {
        self.func.is_some()
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ModId(pub u32);

/// Opaque per-mod registration container.
#[repr(C)]
pub struct Registrar {
    _p: [u8; 0],
}

/// Opaque view of one world (dimension) handed to callbacks. Only valid during the call.
#[repr(C)]
pub struct WorldCtx {
    _p: [u8; 0],
}

#[repr(C)]
pub struct ModDecl {
    pub abi_version: u64,
    /// The crate name, which is also the library's and the mod folder's name.
    pub id: Str,
    /// The crate version.
    pub version: Str,
    /// Mod ids this one must load after. Registration itself is parallel, this orders what it
    /// registers (entries, edits, hooks) after theirs.
    pub deps: Slice<Str>,
    /// Mod ids this one loads after if they are there, without needing them: its edits win over
    /// theirs where both change one thing.
    pub after: Slice<Str>,
    /// Mod ids this one loads before if they are there: theirs win.
    pub before: Slice<Str>,
    pub init: unsafe extern "C" fn(host: *const Host, me: ModId, reg: *mut Registrar) -> u32,
}

unsafe impl Sync for ModDecl {}

// ---------------------------------------------------------------- components

/// One component value on a content entry. `schema` names the value type (see the SDK's `Value`):
/// empty for a tag, `u32`, `f32`, `str` (a `Str`, copied by the engine), `{a:u32,b:f32}` for
/// structs. Every use of a name must agree on the schema; the engine checks at freeze.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CompValue {
    pub name: Str,
    pub schema: Str,
    pub value: Slice<u8>,
}

/// What `Host::reg_entry` does with an entry of that kind and name.
pub mod entry_mode {
    /// A new one; a second of the same name is an error.
    pub const NEW: u32 = 0;
    /// Its operations go onto the one of that name, whoever registered it, in load order. The
    /// name may be a pattern: `#tag` (every entry with the tag, a block's states by their own),
    /// `*` in it (any text there), `name[prop=value]` (those states of a block, `name[]` all of
    /// them or the entry of that name of another kind). Matching nothing is a load error for blocks
    /// and items (a `#tag` only when no such tag exists), fine for other kinds.
    pub const EDIT: u32 = 1;
    /// Registered from scratch over what was there.
    pub const REPLACE: u32 = 2;
    /// As if nobody had registered it.
    pub const REMOVE: u32 = 3;
}

/// What a `ValueOp` does with its component.
pub mod value_op {
    pub const SET: u32 = 0;
    pub const UNSET: u32 = 1;
    /// List components (`[T]`): the value is one element, added at the end.
    pub const PUSH: u32 = 2;
    /// Before or after the element `anchor` names: its first field `name` (a text element's
    /// text), or `#index`.
    pub const INSERT_BEFORE: u32 = 3;
    pub const INSERT_AFTER: u32 = 4;
    /// Takes out the element `anchor` names.
    pub const REMOVE_ITEM: u32 = 5;
    /// Swaps the element `anchor` names for the value.
    pub const REPLACE_ITEM: u32 = 6;
}

/// One change to an entry's components (`data` encoding for the value).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ValueOp {
    pub op: u32,
    pub value: CompValue,
    pub anchor: Str,
}

/// The operations on some states of a block: `key` is `prop=value,..` (every property not named
/// matches anything), empty for every state.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PartDesc {
    pub key: Str,
    pub ops: Slice<ValueOp>,
}

/// Everything a mod registers: an entry of a kind (`entry_kind`), by name, made of components.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EntryDesc {
    pub kind: u32,
    pub name: Str,
    /// The handle's id cell, filled at freeze; null for none.
    pub cell: *const AtomicU32,
    /// `entry_mode`.
    pub mode: u32,
    pub ops: Slice<ValueOp>,
    /// Blocks: operations on some of their states.
    pub parts: Slice<PartDesc>,
}

/// Registry kinds for `Host::find`; the first `COUNT` also have component tables (blocks per state).
pub mod entry_kind {
    pub const BLOCK: u32 = 0;
    pub const ITEM: u32 = 1;
    pub const ENTITY: u32 = 2;
    pub const BIOME: u32 = 3;
    pub const DIMENSION: u32 = 4;
    pub const MENU: u32 = 5;
    pub const PARTICLE: u32 = 6;
    pub const CONFIG: u32 = 7;
    pub const SOUND: u32 = 8;
    pub const DAMAGE_CAUSE: u32 = 9;
    pub const ENTITY_EFFECT: u32 = 10;
    pub const TEXT_PLACE: u32 = 11;
    pub const GAME_MODE: u32 = 12;
    pub const POSE: u32 = 13;
    pub const WEATHER: u32 = 14;
    pub const PRECIPITATION: u32 = 15;
    pub const CONDITION: u32 = 16;
    pub const MUSIC: u32 = 17;
    pub const CAMERA: u32 = 18;
    pub const EQUIPMENT_SLOT: u32 = 19;
    pub const CREATIVE_TAB: u32 = 20;
    pub const SOUND_CATEGORY: u32 = 21;
    pub const ENTITY_CATEGORY: u32 = 22;
    pub const TINT_KIND: u32 = 23;
    pub const RECIPE_KIND: u32 = 24;
    pub const ATTRIBUTE: u32 = 25;
    pub const KEY: u32 = 26;
    /// Named `<dimension>/<object>`, both full names.
    pub const SKY: u32 = 27;
    pub const HUD_ELEMENT: u32 = 28;
    pub const SCREEN_PART: u32 = 29;
    pub const WORDS: u32 = 30;
    /// A component's declaration, by the component's name.
    pub const COMPONENT: u32 = 31;
    pub const RECIPE: u32 = 32;
    /// Named like the block or entity kind that drops it, or a table of its own.
    pub const LOOT: u32 = 33;
    /// Anything generation puts into the world (`comp::feature`): an ore is named like its block,
    /// a structure like its template, a decoration `<mod>:decoration`, `/2`.. by default.
    pub const FEATURE: u32 = 34;
    /// Named like its block (16 states, see `comp::liquid`).
    pub const LIQUID: u32 = 35;
    /// What happens where two liquids meet, `<mod>:<liquid>><touching>` by default.
    pub const REACTION: u32 = 36;
    pub const TEMPLATE: u32 = 37;
    /// Blocks for templates' material slots (`comp::palette`).
    pub const PALETTE: u32 = 38;
    /// A natural spawning rule, `<mod>:<entity>`, `/2`.. by default.
    pub const SPAWN: u32 = 39;
    /// A dimension's terrain, named like the dimension.
    pub const TERRAIN: u32 = 40;
    /// How a bone of a model file moves by itself, named `<model>/<bone>`.
    pub const BONE_MOTION: u32 = 41;
    /// Code that changes a value (`comp::step`): on a chain, or used by modifiers.
    pub const STEP: u32 = 42;
    /// A place in the order steps run in (`comp::phase`).
    pub const PHASE: u32 = 43;
    /// A mod's own data, nothing the engine reads: its tags and components say what it is
    /// (`structgen:structure`), other mods add, edit and remove records like anything else and
    /// read them once the game is loaded (`Host::entries_with`).
    pub const RECORD: u32 = 44;
    /// How many kinds there are; each has a component table (`ComponentTable`), empty for word
    /// sets and component declarations.
    pub const COUNT: usize = NAMES.len();
    /// Each kind's name for messages, by value.
    pub const NAMES: [&str; 45] = [
        "block",
        "item",
        "entity",
        "biome",
        "dimension",
        "menu",
        "particle",
        "config",
        "sound",
        "damage cause",
        "entity effect",
        "text place",
        "game mode",
        "pose",
        "weather",
        "precipitation",
        "condition",
        "music",
        "camera mode",
        "equipment slot",
        "creative tab",
        "sound category",
        "entity category",
        "tint kind",
        "recipe kind",
        "attribute",
        "key",
        "sky object",
        "HUD element",
        "screen part",
        "word set",
        "component",
        "recipe",
        "loot table",
        "feature",
        "liquid",
        "liquid reaction",
        "template",
        "palette",
        "spawn rule",
        "terrain",
        "bone motion",
        "step",
        "phase",
        "record",
    ];
}

/// Frozen components of one entry kind, readable directly by mods: whether entry `e` has
/// component `c` is bit `c % 64` of `bits[e * words + c / 64]`; its value (if the component has
/// one on this kind) is at `columns[c].data + e * columns[c].stride`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KindTable {
    pub entries: u32,
    pub words: u32,
    pub bits: *const u64,
    /// One per component id; `stride` 0 when no entry of this kind has a value for it.
    pub columns: *const Column,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Column {
    pub data: *const u8,
    pub stride: u32,
}

#[repr(C)]
pub struct ComponentTable {
    /// Number of component ids.
    pub components: u32,
    pub kinds: [KindTable; entry_kind::COUNT],
}

unsafe impl Sync for ComponentTable {}
unsafe impl Send for ComponentTable {}

/// What a block state is, everything its builder set that isn't a component (those are read
/// from the component table). `Host::block_info`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BlockInfo {
    /// Its block, the block's first state and how many it has.
    pub block: u32,
    pub first_state: u32,
    pub states: u32,
    /// The block's name and the mod that registered it.
    pub name: Str,
    pub owner: Str,
    /// `block_flags`.
    pub flags: u32,
    pub opacity: u32,
    pub light: [u8; 3],
    pub absorb: [u8; 3],
    pub layer: RenderLayer,
    pub model: BlockModel,
    pub hardness: f32,
    pub friction: f32,
    pub drag: f32,
    /// Texture names per face (-X, +X, -Y, +Y, -Z, +Z).
    pub textures: [Str; 6],
    /// Its liquid's id, `u32::MAX` for none.
    pub liquid: u32,
    /// The item that places it, `u32::MAX` for none.
    pub item: u32,
}

/// What an item is, everything its builder set that isn't a component. `Host::item_info`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ItemInfo {
    pub name: Str,
    pub owner: Str,
    pub max_stack: u32,
    pub durability: u32,
    pub attack: f32,
    /// The block state it places, `u32::MAX` for none.
    pub block: u32,
    /// The equipment slot it is worn in, empty for none.
    pub worn: Str,
    pub handheld: u32,
}

/// Component names the engine itself reads. Items: `max_stack: u32` (default 64), `durability: u32`,
/// `attack_damage: f32` (default 1), `tool_level: u32` with a `tools/<class>` tag and
/// `mining_speed: f32` (default 1). Blocks: `mineable/<class>` tags, and with `harvest_level: u32`
/// nothing drops without a tool of a matching class and at least that level (SDK `mined_by`). Stacks: `damage: u32`.
/// A block matcher (`match` values, and the `str` ones said to be one) is block names, `#tags`
/// (`*` any text) and states `name[prop=value]` apart by spaces, `!` before one to leave what it
/// names out; only `!` ones: anything but those.
pub mod comp {
    /// A damage cause's: tags `BYPASSES_ARMOR`, `PIERCES_INVULNERABLE` (outside survival too),
    /// `IGNORES_COOLDOWN`, `BURNING` (cooked drops), `NO_KNOCKBACK`, `SILENT`; `DEATH_MESSAGE` and
    /// `DEATH_MESSAGE_BY` (`str` translation keys, `{0}` the one who died, `{1}` who did it).
    pub mod damage_cause {
        /// Armor doesn't soften it (mods read this in their `DAMAGE` modifiers).
        pub const BYPASSES_ARMOR: &str = "damage.bypasses_armor";
        /// Held items cannot block it, independently of armor reduction.
        pub const BYPASSES_BLOCKING: &str = "damage.bypasses_blocking";
        /// Hurts invulnerable entities and players outside survival too.
        pub const PIERCES_INVULNERABLE: &str = "damage.pierces_invulnerable";
        /// Lands in full while the entity is still recovering from the last hit.
        pub const IGNORES_COOLDOWN: &str = "damage.ignores_cooldown";
        /// The victim counts as burning when it dies (cooked drops).
        pub const BURNING: &str = "damage.burning";
        pub const NO_KNOCKBACK: &str = "damage.no_knockback";
        /// No hurt sound.
        pub const SILENT: &str = "damage.silent";
        /// Never kills: what it would take below half a heart it leaves (poison).
        pub const NOT_LETHAL: &str = "damage.not_lethal";
        pub const DEATH_MESSAGE: &str = "damage.death_message";
        pub const DEATH_MESSAGE_BY: &str = "damage.death_message_by";
    }

    /// A game mode's: its tags, and `MODIFIERS`
    /// (`comp::MODIFIERS`, `[Modifier]`) its players have while in it.
    pub mod game_mode {
        /// Blocks break at once.
        pub const INSTANT_BREAK: &str = "game_mode.instant_break";
        /// Blocks don't break at all.
        pub const NO_BREAK: &str = "game_mode.no_break";
        /// Blocks can't be placed.
        pub const NO_PLACE: &str = "game_mode.no_place";
        /// Nothing used up: placed blocks, poured buckets, wear; picking a block gives a stack.
        pub const NO_COSTS: &str = "game_mode.no_costs";
        /// Always flies.
        pub const ALWAYS_FLY: &str = "game_mode.always_fly";
        /// Goes through blocks.
        pub const NOCLIP: &str = "game_mode.noclip";
        /// Can't touch anything: no using, hitting, picking up, dropping or holding.
        pub const NO_INTERACT: &str = "game_mode.no_interact";
        /// Mobs, natural spawning and other players don't notice it; it isn't a body to hit.
        pub const HIDDEN: &str = "game_mode.hidden";
        /// No hotbar or bars.
        pub const NO_HUD: &str = "game_mode.no_hud";
        /// The inventory key opens the creative inventory.
        pub const CREATIVE_INVENTORY: &str = "game_mode.creative_inventory";
    }

    /// A pose's, as shares of standing (1 unless set): `HEIGHT`, `EYE`, `SPEED`; `TILT` (`u32`,
    /// `pose_tilt`), `ACTION` (`str`, the clip), tag `NO_BOB`, `PRIORITY` (`i32`), `KEY` (`str`,
    /// an action), `WHEN` (`[str]`, conditions that all hold, `in_liquid`, `!flying`), `FALLBACK`
    /// (`[str]` poses where it has no room).
    pub mod pose {
        pub const HEIGHT: &str = "pose.height";
        pub const EYE: &str = "pose.eye";
        pub const SPEED: &str = "pose.speed";
        pub const TILT: &str = "pose.tilt";
        pub const ACTION: &str = "pose.action";
        pub const NO_BOB: &str = "pose.no_bob";
        pub const PRIORITY: &str = "pose.priority";
        pub const KEY: &str = "pose.key";
        pub const FALLBACK: &str = "pose.fallback";
        pub const WHEN: &str = "pose.when";
    }

    /// A precipitation's: `TEXTURE` (`str`, a strip of drops), `SPEED` (`f32`, share of rain's),
    /// `DENSITY` (`f32`, share of the picture shown).
    pub mod precipitation {
        pub const TEXTURE: &str = "precipitation.texture";
        pub const SPEED: &str = "precipitation.speed";
        pub const DENSITY: &str = "precipitation.density";
    }

    /// A text place's: `ANCHOR` (`str` `hud_anchor`) and `ORDER` (`i32`), or `AT` (`[f32;4]`, its
    /// point at that point of the window, moved by GUI pixels); `SCALE` (`f32`), `TIMES`
    /// (`[f32;3]` fade in, stay, fade out), tag `STACKS`, `CLASS` and `SAMPLE` (`str`).
    pub mod text_place {
        pub const ANCHOR: &str = "text_place.anchor";
        pub const ORDER: &str = "text_place.order";
        pub const AT: &str = "text_place.at";
        pub const SCALE: &str = "text_place.scale";
        pub const TIMES: &str = "text_place.times";
        pub const STACKS: &str = "text_place.stacks";
        pub const CLASS: &str = "text_place.class";
        pub const SAMPLE: &str = "text_place.sample";
        /// The engine's own: `OWN_AT` (`[f32;2]`) where text stands if no one says, tag `WRAP`,
        /// tag `NO_ELEMENT` (no HUD element of its own: free text, the chat).
        pub const OWN_AT: &str = "text_place.own_at";
        pub const WRAP: &str = "text_place.wrap";
        pub const NO_ELEMENT: &str = "text_place.no_element";
    }

    /// An entity effect's `SHADER` (`str`, `shaders/entity_effect/<name>.wgsl`, its own name by
    /// default, nothing drawn where that file isn't), `HAZARDS` (`[Hazard]`, `hazard_at::SELF`:
    /// poison), `MODIFIERS` (`[Modifier]`: speed) and `LIGHT` while it is on; modifiers and hazards
    /// times its level unless some are given for a level (then those of the highest level up to
    /// it). A status effect, one its entity's player sees, has an `ICON`
    /// (`str`, a texture) and a `COLOR` (`u32`, `0xRRGGBB`, of the swirls around who has it, none
    /// without), tags `BENEFICIAL` or `HARMFUL`, and `INSTANT` for one that acts once when given
    /// (healing) and doesn't stay. `SCREEN` (`str`, `shaders/post/<name>.wgsl`) is laid over
    /// what its player sees while it has it (night vision), its strength fading the last second;
    /// `HIDES` keeps its entity from being drawn for others (invisibility). A condition's `CALL`
    /// (`callback`, `ConditionArgs`).
    pub const ENTITY_EFFECT_SHADER: &str = "entity_effect.shader";
    pub const ENTITY_EFFECT_HAZARDS: &str = "entity_effect.hazards";
    pub const ENTITY_EFFECT_ICON: &str = "entity_effect.icon";
    pub const ENTITY_EFFECT_COLOR: &str = "entity_effect.color";
    pub const ENTITY_EFFECT_BENEFICIAL: &str = "entity_effect.beneficial";
    pub const ENTITY_EFFECT_HARMFUL: &str = "entity_effect.harmful";
    pub const ENTITY_EFFECT_INSTANT: &str = "entity_effect.instant";
    pub const ENTITY_EFFECT_HIDES: &str = "entity_effect.hides";
    /// An entity effect's `callback` (`EffectTickArgs`) every tick with every entity that has it.
    pub const ENTITY_EFFECT_ON_TICK: &str = "entity_effect.on_tick";
    pub const CONDITION_CALL: &str = "condition.call";

    /// A sound event's: `FILES` (`[str]`, `sounds/<name>.ogg` or `.wav`, one picked at random),
    /// `VOLUME` (`f32`, 1), `PITCH` (`[f32;2]`, playback speed picked between, 1), `RANGE` (`f32`,
    /// blocks away still heard, 16), `FULL_WITHIN` (`f32`, blocks at full volume, 1), `FALLOFF`
    /// (`i32` -8..8, the fade from there to `RANGE`: 0 even, n > 0 staying loud longer, n < 0
    /// dropping early), `CATEGORY` (`str`, a sound category, `core:blocks`).
    pub mod sound {
        pub const FILES: &str = "sound.files";
        pub const VOLUME: &str = "sound.volume";
        pub const PITCH: &str = "sound.pitch";
        pub const RANGE: &str = "sound.range";
        pub const FULL_WITHIN: &str = "sound.full_within";
        pub const FALLOFF: &str = "sound.falloff";
        pub const CATEGORY: &str = "sound.category";
    }

    /// Background music's: after a quiet while, a track of the first music whose conditions hold.
    /// `TRACKS` (`[str]`, sound files streamed), `DIMENSION` (`str`, the one it plays in, any
    /// unless set), `WHEN` (`[str]`, conditions that all have to hold).
    pub mod music {
        pub const TRACKS: &str = "music.tracks";
        pub const DIMENSION: &str = "music.dimension";
        pub const WHEN: &str = "music.when";
    }

    /// A camera mode's: its tags (what goes with it), `KEY` (`str`, an action that turns
    /// it on and back to where it was), `CALL` (`callback`, `CameraArgs`: where the camera is each
    /// frame), `TRANSITION` (`f32`, seconds from the camera before, 0) eased by `EASING` (`u32`,
    /// `easing`, `IN_OUT`), `WHEN` (`[str]`, conditions: the player may turn it on only where
    /// they hold for it on the server, and the server lets it, `action::CAMERA`).
    pub mod camera {
        pub const WHEN: &str = "camera.when";
        pub const KEY: &str = "camera.key";
        pub const CALL: &str = "camera.call";
        pub const TRANSITION: &str = "camera.transition";
        pub const EASING: &str = "camera.easing";
        /// The player's own body is drawn.
        pub const BODY: &str = "camera.body";
        /// The first-person hand is drawn.
        pub const HAND: &str = "camera.hand";
        /// The HUD and crosshair are drawn.
        pub const HUD: &str = "camera.hud";
        /// The player's effects move it: walking bob, the wider view sprinting and flying, the tilt
        /// when hurt, what is over the screen inside a block or a liquid.
        pub const EFFECTS: &str = "camera.effects";
        /// Walking bob, hurt tilt and held-item recoil, without FOV changes or screen overlays.
        pub const BOB: &str = "camera.bob";
        /// Pulled in towards `CameraArgs::anchor` where a block is in the way (third person).
        pub const CLIP: &str = "camera.clip";
        /// A small box that bumps into blocks, moved from where it was to where `call` says, and
        /// stopped where the world isn't loaded yet.
        pub const SOLID: &str = "camera.solid";
        /// One of the modes `core:perspective` (F5) goes through.
        pub const CYCLE: &str = "camera.cycle";
        /// Movement keys and the mouse are the camera's (`CameraArgs::keys`); the player stands.
        pub const DETACHED: &str = "camera.detached";
        /// The crosshair aims from the camera, not the eyes.
        pub const AIM: &str = "camera.aim";
        /// No hitting, using or picking.
        pub const NO_INTERACT: &str = "camera.no_interact";
    }

    /// A setting's, shown as `config.<ns>.<key>` (and `.tooltip`) from the language files: `KIND`
    /// (`u32`, `config_kind`, `BOOL`), `DEFAULT` (`f64`, 0), `RANGE` (`[f64;2]`, 0..1) and `STEP`
    /// (`f64`, 0 any) of numbers, `CHOICES` (`[str]`), `SCOPE` (`u32`, `config_scope`, `CLIENT`),
    /// `TEXT` (`str`, the default of a text setting), `PAGE` (`str`, the settings page it is on:
    /// `core:settings`, `core:chat_settings`, `core:sound` or a mod's own, titled `page.<ns>.<name>`;
    /// none: its mod's page), `ORDER` (`i32`, lower first, then by name, 0), tags
    /// `RESTART_REQUIRED`, `PERCENT` (shown times 100), `ON_RELEASE`. Its value shows as `config.<ns>.<key>.<value>`
    /// where there is one (a choice, a number with a name of its own), else through
    /// `config.<ns>.<key>.value` (`{0} cubes`).
    pub mod config {
        pub const KIND: &str = "config.kind";
        pub const DEFAULT: &str = "config.default";
        pub const RANGE: &str = "config.range";
        pub const STEP: &str = "config.step";
        pub const CHOICES: &str = "config.choices";
        pub const SCOPE: &str = "config.scope";
        pub const TEXT: &str = "config.text";
        /// Takes effect after a restart; the settings screen says so.
        pub const RESTART_REQUIRED: &str = "config.restart_required";
        pub const PERCENT: &str = "config.percent";
        /// A slider that takes its value when let go, not on its way there (the GUI scale).
        pub const ON_RELEASE: &str = "config.on_release";
        pub const PAGE: &str = "config.page";
        pub const ORDER: &str = "config.order";
    }

    /// An equipment slot's: `COVERS` (`[str]`, bones what is worn there covers; a name with a role,
    /// `head`, `leftArm`, `leg0`, covers every bone of that role) and `BACKGROUND` (`str`, the GUI
    /// texture of the empty slot).
    pub mod equipment_slot {
        pub const COVERS: &str = "equipment_slot.covers";
        pub const BACKGROUND: &str = "equipment_slot.background";
    }

    /// An entity category's: `CAP` (`u32`, at most this many spawn naturally near a player, 0),
    /// `DESPAWN` (`f32`, further than this from every player they vanish, 0 never), `CAP_RADIUS`
    /// (`f32`, how near a player the cap counts them, 64).
    pub mod entity_category {
        pub const CAP: &str = "entity_category.cap";
        pub const DESPAWN: &str = "entity_category.despawn";
        pub const CAP_RADIUS: &str = "entity_category.cap_radius";
    }

    /// A recipe kind's, for the recipe viewer: `INPUTS` and `OUTPUTS` (`[u32;2]`, grids this big
    /// whatever a recipe uses, 0 as many as it has), `INFO` (`str`, a translation under each
    /// recipe, `{0}`.. filled from `INFO_VALUES`, `[str]`: `time` in seconds or components the
    /// recipe carries), `LAYOUT` (`callback`, `RecipeUiArgs`: builds one recipe instead).
    pub mod recipe_kind {
        pub const INPUTS: &str = "recipe_kind.inputs";
        pub const OUTPUTS: &str = "recipe_kind.outputs";
        pub const INFO: &str = "recipe_kind.info";
        pub const INFO_VALUES: &str = "recipe_kind.info_values";
        pub const LAYOUT: &str = "recipe_kind.layout";
    }

    /// An attribute's `DEFAULT` (`f64`, 0) and `RANGE` (`[f64;2]`, unbounded) its value stays in.
    pub mod attribute {
        pub const DEFAULT: &str = "attribute.default";
        pub const RANGE: &str = "attribute.range";
        /// Tag: the base of it for kinds that don't set their own is a world rule named like it
        /// (`/rule`), `DEFAULT` and `RANGE` its own; `/attribute` sets one entity's apart.
        pub const RULE: &str = "attribute.rule";
    }

    /// A creative tab's `ICON` (`str`, an item); a sound category's tag `WORLD` (heard through
    /// caves and water like block sounds, else like music); a tint kind's `DEFAULT` (`u32`,
    /// 0xRRGGBB where the biome sets none and on items, white); a key's `DEFAULT` (`str`, the
    /// key by its settings name, `KeyJ`, `mouse3`).
    pub const CREATIVE_TAB_ICON: &str = "creative_tab.icon";
    pub const SOUND_CATEGORY_WORLD: &str = "sound_category.world";
    pub const TINT_KIND_DEFAULT: &str = "tint_kind.default";
    pub const KEY_DEFAULT: &str = "key.default";
    /// Attribute modifiers (`[Modifier]`) of a game mode, an item, a pose, an entity kind or
    /// an entity effect.
    pub const MODIFIERS: &str = "modifiers";
    /// A player's own, for its client: game mode (`u32` id), permission level (`u32`), every
    /// attribute's value (`f32` by id).
    pub const GAME_MODE: &str = "core:game_mode";
    pub const PERMISSION: &str = "core:permission";
    pub const ATTRIBUTES: &str = "core:attributes";

    /// A key's contexts, tags of its entry: where it acts. Two keys on one binding conflict
    /// only where their contexts meet; a key with none acts wherever the world has the input
    /// (all but `SCREEN`). A camera that takes the moves (`camera::DETACHED`) has `MOVEMENT` and
    /// `RIDING` for itself, one that doesn't interact (`camera::NO_INTERACT`) `HANDS`.
    pub mod key {
        pub const MOVEMENT: &str = "key.movement";
        pub const HANDS: &str = "key.hands";
        pub const RIDING: &str = "key.riding";
        pub const DEBUG: &str = "key.debug";
        pub const SCREEN: &str = "key.screen";
    }

    /// A particle kind's (particles live only on clients; the server and mods send bursts):
    /// `TEXTURES` (`[str]`, frames over its life; none: a piece of the burst's block texture),
    /// `LIFE` (`[f32;2]`, seconds picked between, 0.5..1), `SIZE` (`[f32;2]`, edge length in
    /// blocks, 0.1..0.2), `GRAVITY` (`f32`, blocks per second squared pulling down, 0), `DRAG`
    /// (`f32`, share of its speed kept per tick, 0.98), `COLOR` (`[u32;2]`, 0xRRGGBBAA at birth
    /// and at the end of its life, white), `SHADER` (`str`, `shaders/particle/<name>.wgsl`),
    /// `RULES` (`[ParticleRule]`), tags `COLLIDE`, `FULLBRIGHT`, `TRANSLUCENT`, `SHRINK`.
    pub mod particle {
        pub const TEXTURES: &str = "particle.textures";
        pub const LIFE: &str = "particle.life";
        pub const SIZE: &str = "particle.size";
        pub const GRAVITY: &str = "particle.gravity";
        pub const DRAG: &str = "particle.drag";
        pub const COLOR: &str = "particle.color";
        pub const SHADER: &str = "particle.shader";
        pub const RULES: &str = "particle.rules";
        /// Stops on blocks, like vanilla's particles: lies where it lands, slides along walls. Its
        /// `ParticleRule`s for landing and hitting things say otherwise; any of them turns this on.
        pub const COLLIDE: &str = "particle.collide";
        /// Ignores light (flames, sparks).
        pub const FULLBRIGHT: &str = "particle.fullbright";
        /// Alpha blended (smoke fading out) instead of cut out.
        pub const TRANSLUCENT: &str = "particle.translucent";
        /// Shrinks to nothing over its life.
        pub const SHRINK: &str = "particle.shrink";
    }

    /// A thing in a dimension's sky (`entry_kind::SKY`): `KIND` (`u32`, `sky_kind`, `BODY`),
    /// `TEXTURE` (`str`, under `textures/sky/`; a dome tells its layout by its shape), `SHADER`
    /// (`str`, `shaders/sky/<name>.wgsl` instead of the plain picture), `PHASE` (`f32`, turns on
    /// its orbit at time 0: 0 rising in the east, 0.25 overhead), `ORBIT` (`f32`, ticks per turn,
    /// negative the other way, 0 still, infinite following the dimension's day; a dome's 0, the
    /// others' infinite), `TILT` and `HEADING` (`f32`, degrees the orbit leans south of overhead
    /// and turns around the vertical), `SIZE` (`f32`, degrees across, of each star for stars, 20
    /// and 0.25), `SPIN` (`f32`, ticks per turn around its middle, 0 none), `OPACITY` (`[f32;2]`,
    /// how much shows by full day and by deep night, 1 and 1, stars 0 and 0.22), `COLOR` (`u32`,
    /// 0xRRGGBB multiplied in, white), `FRAMES` (`[u32;2]`, the picture's grid of frames, 1 by
    /// 1), `FRAME_TICKS` (`u32`, ticks each lasts, 0 a day of the dimension), `GLOW` (`u32`,
    /// 0xRRGGBB along the horizon while it rises and sets, 0 none), tag `ADDITIVE` (adds its light
    /// to what is behind it, else covers it), stars' `COUNT` (`u32`, 1500) and `SEED` (`u32`),
    /// clouds' `HEIGHT` (`f32`, of their underside, 224.33) and `DRIFT` (`f32`, blocks per tick
    /// along x, 0.03), `ORDER` (`i32`, drawn low to high: bodies 0, stars -100, domes -200, clouds
    /// 100).
    pub mod sky {
        pub const KIND: &str = "sky.kind";
        pub const TEXTURE: &str = "sky.texture";
        pub const SHADER: &str = "sky.shader";
        pub const PHASE: &str = "sky.phase";
        pub const ORBIT: &str = "sky.orbit";
        pub const TILT: &str = "sky.tilt";
        pub const HEADING: &str = "sky.heading";
        pub const SIZE: &str = "sky.size";
        pub const SPIN: &str = "sky.spin";
        pub const OPACITY: &str = "sky.opacity";
        pub const COLOR: &str = "sky.color";
        pub const FRAMES: &str = "sky.frames";
        pub const FRAME_TICKS: &str = "sky.frame_ticks";
        pub const GLOW: &str = "sky.glow";
        pub const ADDITIVE: &str = "sky.additive";
        pub const COUNT: &str = "sky.count";
        pub const SEED: &str = "sky.seed";
        pub const HEIGHT: &str = "sky.height";
        pub const DRIFT: &str = "sky.drift";
        pub const ORDER: &str = "sky.order";
    }

    /// A HUD element's: `ANCHOR` (`str`, `hud_anchor`), `ORDER` (`i32`, 0 nearest the anchor's
    /// edge), `CALL` (`callback`, `UiArgs`: builds it). Players can move any element to another
    /// anchor in `config/hud.txt`.
    pub mod hud_element {
        pub const ANCHOR: &str = "hud_element.anchor";
        pub const ORDER: &str = "hud_element.order";
        pub const CALL: &str = "hud_element.call";
        pub const REBUILD: &str = "hud_element.rebuild";
    }

    /// A screen part's, what a mod adds to place `AREA` (`str`) of screen `SCREEN` (`str`):
    /// `ORDER` (`i32`, lower first), `CALL` (`callback`, `UiArgs`: builds it).
    pub mod screen_part {
        pub const SCREEN: &str = "screen_part.screen";
        pub const AREA: &str = "screen_part.area";
        pub const ORDER: &str = "screen_part.order";
        pub const CALL: &str = "screen_part.call";
    }

    /// A word set's `WORDS` (`[str]`): command parameters `word(#set)` complete from them.
    pub const WORDS: &str = "words.words";

    /// A component's declaration (`entry_kind::COMPONENT`, named like it): `SCHEMA` (`str`) of its
    /// value; `DEFAULT` (`[u8]`): entities carry it, starting with these bytes (its size), with a
    /// tag per `component_flags` (`FLAGS`); `SLOTS` (`u32`): a storage, an inventory of so many
    /// slots entities and stacks carry, saved and kept on respawn; tag `STACK`: stacks carry it,
    /// with a tag per `stack_flags` (`STACK_FLAGS`).
    pub mod component {
        use crate::component_flags as f;
        pub const SCHEMA: &str = "component.schema";
        pub const DEFAULT: &str = "component.default";
        pub const SLOTS: &str = "component.slots";
        pub const STACK: &str = "component.stack";
        pub const FLAGS: [(&str, u32); 6] = [
            ("component.save", f::SAVE),
            ("component.sync_owner", f::SYNC_OWNER),
            ("component.sync_watchers", f::SYNC_WATCHERS),
            ("component.sync_menu", f::SYNC_MENU),
            ("component.keep_on_respawn", f::KEEP_ON_RESPAWN),
            ("component.carried", f::CARRIED),
        ];
        pub const STACK_FLAGS: [(&str, u32); 3] = [
            ("component.server_only", crate::stack_flags::SERVER_ONLY),
            ("component.no_save", crate::stack_flags::NO_SAVE),
            ("component.visual", crate::stack_flags::VISUAL),
        ];
    }

    /// A recipe's (`entry_kind::RECIPE`): `KIND` (`str`, a recipe kind), `PATTERN` (`[str]`, rows
    /// of key characters of a shaped one) with `KEYS` (`[RecipeKey]`), `INPUTS` (`[str]`, of a
    /// shapeless or processing one: item names or `#tags`, a liquid by its name) with `AMOUNTS`
    /// (`[u32]`, of each, millibuckets for liquids, 1), `OUTPUT` (`str`) and `COUNT` (`u32`, 1),
    /// `TIME` (`u32`, ticks, the kind says what they mean, 0), `OUTPUTS` (`[LootPool]`, more
    /// results, rolled as loot is: a 5% one is a pool of it and nothing by weight), `USES`
    /// (`[RecipeUse]`, what a run does to an ingredient other than using it up), tag `HIDDEN` (works, the recipe viewer doesn't list it). Any other component is
    /// data it carries for what makes it (energy per tick).
    pub mod recipe {
        pub const KIND: &str = "recipe.kind";
        pub const PATTERN: &str = "recipe.pattern";
        pub const KEYS: &str = "recipe.keys";
        pub const INPUTS: &str = "recipe.inputs";
        pub const AMOUNTS: &str = "recipe.amounts";
        pub const OUTPUT: &str = "recipe.output";
        pub const COUNT: &str = "recipe.count";
        pub const TIME: &str = "recipe.time";
        pub const OUTPUTS: &str = "recipe.outputs";
        pub const USES: &str = "recipe.uses";
        pub const HIDDEN: &str = "recipe.hidden";
    }

    /// A loot table's (`entry_kind::LOOT`): `POOLS` (`[LootPool]`), then `SWAPS` (`[LootSwap]`)
    /// in order. Without a table a block drops its own item when harvested.
    pub mod loot {
        pub const POOLS: &str = "loot.pools";
        pub const SWAPS: &str = "loot.swaps";
    }

    /// Where a feature or a spawn rule goes; what isn't set doesn't limit it. `DIMENSIONS`
    /// (`[str]`, references to dimensions; none: every one but `dimension::BARE` ones), `BIOME`
    /// (`str`, a reference to biomes: `#cave`, `base:plains`, `#forest !base:birch_forest`), `Y`
    /// (`[i32;2]`, inclusive heights: where a feature starts, its ground for one standing on it,
    /// a spawn spot). A feature generating whole cubes decides biome and height itself.
    pub mod common {
        pub const DIMENSIONS: &str = "dimensions";
        pub const BIOME: &str = "biome";
        pub const Y: &str = "y";
    }

    /// A feature (`entry_kind::FEATURE`): where (`common`), how its starts spread, its shape, and
    /// how it writes its blocks. Everything generation puts into the world after the terrain is
    /// one: ores, geodes, trees, boulders, structures, villages, grass.
    ///
    /// Order: `STAGE` (`u32`, `stage`, by its spread unless set), then `AFTER` and `BEFORE`
    /// (`[str]`, other features), then load order.
    ///
    /// Spread (`SPREAD`, `u32`, `spread`):
    /// - `PER_CUBE`: `COUNT` (`u32`, 4) starts in every cube, over `common::Y` as `HEIGHT`
    ///   (`HeightCurve`, evenly) says;
    /// - `GRID`: one start per `SPACING` x `SPACING` square (`i32`, 32) at a seeded spot on the
    ///   ground, kept with `CHANCE` (`f32`, 1) or the chance the biome's `DENSITY` component gives
    ///   (`str`, an `f32` 0..1 on biomes; none there: no starts); on `GROUND` (`match`; none:
    ///   any), not under the terrain's sea unless tagged `OVER_WATER`, not where caves took the
    ///   ground unless `OVER_CAVES`, where its footprint's ground varies by at most `MAX_SLOPE`
    ///   blocks (`i32`, any), its bottom `BURIED` blocks into the ground (`i32`, 0). One of a
    ///   higher `PRIORITY` (`i32`, 0) keeps it `SEPARATION` blocks away (`i32`, 0); only what
    ///   really stands counts.
    /// - `EVERY_CUBE`: `GENERATE` (`callback`, `GenArgs`) runs on every generated cube, pure like a
    ///   generator.
    ///
    /// Shape (`SHAPE`, `u32`, `shape`, from what is set unless said):
    /// - `VEIN`: `VEIN` (`VeinShape`, a walk) of `SIZE` blocks (`u32`, 8) about its start;
    /// - `STEPS`: `STEPS` (`[ShapeStep]`), any shape about its start (a geode's shell);
    /// - `TEMPLATE`: `TEMPLATE` (`str`) from its lowest corner, turned and mirrored as
    ///   `TRANSFORMS` (`u32`, bits of the 8 `transform`s allowed, 1: as drawn) and its slots filled
    ///   by one of `PALETTES` (`[str]`, the seed picks one; none: the template's defaults);
    /// - `PLAN`: `PLAN` (`callback`, `PlanArgs`, pure), worked out once per start: blocks,
    ///   templates and entities up to `REACH` (`[i32;3]`, sideways, up, down: 0, 32, 16) from it.
    ///
    /// Writing (veins and steps put `BLOCK`, `str`): only into what `REPLACES` takes (`match`;
    /// none: anything), `REPLACES_WITH` (`[Variant]`) what they meet; `EXPOSED` (`f32`, 0) of
    /// what would touch air left out. A template's or plan's pieces stand on `FOUNDATION` (`str`,
    /// a block or `ground`: the column's own top block), the ground above them cut away with tag
    /// `CLEAR_ABOVE`.
    pub mod feature {
        pub const STAGE: &str = "feature.stage";
        pub const AFTER: &str = "feature.after";
        pub const BEFORE: &str = "feature.before";
        pub const SPREAD: &str = "feature.spread";
        pub const COUNT: &str = "feature.count";
        pub const HEIGHT: &str = "feature.height";
        pub const SPACING: &str = "feature.spacing";
        pub const CHANCE: &str = "feature.chance";
        pub const DENSITY: &str = "feature.density";
        pub const GROUND: &str = "feature.ground";
        pub const OVER_WATER: &str = "feature.over_water";
        pub const OVER_CAVES: &str = "feature.over_caves";
        pub const MAX_SLOPE: &str = "feature.max_slope";
        pub const BURIED: &str = "feature.buried";
        pub const PRIORITY: &str = "feature.priority";
        pub const SEPARATION: &str = "feature.separation";
        pub const GENERATE: &str = "feature.generate";
        pub const SHAPE: &str = "feature.shape";
        pub const VEIN: &str = "feature.vein";
        pub const SIZE: &str = "feature.size";
        pub const STEPS: &str = "feature.steps";
        pub const TEMPLATE: &str = "feature.template";
        pub const TRANSFORMS: &str = "feature.transforms";
        pub const PALETTES: &str = "feature.palettes";
        pub const PLAN: &str = "feature.plan";
        pub const REACH: &str = "feature.reach";
        pub const BLOCK: &str = "feature.block";
        pub const REPLACES: &str = "feature.replaces";
        pub const REPLACES_WITH: &str = "feature.replaces_with";
        pub const EXPOSED: &str = "feature.exposed";
        pub const FOUNDATION: &str = "feature.foundation";
        pub const CLEAR_ABOVE: &str = "feature.clear_above";
    }

    /// A liquid's flow (`entry_kind::LIQUID`; its block has 16 states: 0 the source, 1..7 flowing
    /// with less and less, 8 falling): `DELAY` (`u32`, ticks between flow steps, 5), `DROP_OFF`
    /// (`u32`, level lost per block of spread, 1: 7 blocks), `SLOPE_FIND` (`u32`, how far flowing
    /// liquid looks for a way down, 4), `RENEWABLE` (`u32`, a spot over solid ground or a source
    /// with at least this many sources beside it becomes one, 0 always; never), `DENSITY` (`i32`,
    /// negative rises, 1000), `HEAT` (`u32`, roughly Kelvin, 300; from 1000 up it burns what
    /// touches it). Perception is configured with environment layers.
    pub mod liquid {
        pub const DELAY: &str = "liquid.delay";
        pub const DROP_OFF: &str = "liquid.drop_off";
        pub const SLOPE_FIND: &str = "liquid.slope_find";
        pub const RENEWABLE: &str = "liquid.renewable";
        pub const DENSITY: &str = "liquid.density";
        pub const HEAT: &str = "liquid.heat";
    }

    /// What happens where two liquids meet (`entry_kind::REACTION`): when `LIQUID` (`match`) has
    /// `TOUCHING` (`match`) beside or above it, it turns into
    /// `SOURCE` (`str`, a block) if it was a source, else `FLOWING` (`str`); when it comes down
    /// onto `TOUCHING`, that block becomes `FALLS_INTO` (`str`). What is not set stays as it is.
    /// Each is block rules of the liquids' states at freeze (`comp::block::RULES`).
    pub mod reaction {
        pub const LIQUID: &str = "reaction.liquid";
        pub const TOUCHING: &str = "reaction.touching";
        pub const SOURCE: &str = "reaction.source";
        pub const FLOWING: &str = "reaction.flowing";
        pub const FALLS_INTO: &str = "reaction.falls_into";
    }

    /// A template (`entry_kind::TEMPLATE`): a fixed shape of blocks for features, moving bodies,
    /// `Host::place` and `/structure`, checked when the game loads. `BLOCKS` (`[TemplateBlock]`)
    /// from its lowest corner, cells it doesn't name left as they are; `SLOTS`
    /// (`[PaletteSlot]`, the blocks its `@slot`s take unless a palette says); `ENTITIES`
    /// (`[TemplateEntity]`); `LOOT` (`[TemplateLoot]`, what a block entity's inventory is filled
    /// with when it is placed); `DATA` (`[TemplateData]`, a block entity as it was saved);
    /// `JOINTS` (`[TemplateJoint]`, where pieces of a plan join). A mod's `structures/<path>.json`
    /// (or structgen's `.wst`) is template `<mod>:<path>` by itself; an entry of that name adds to it.
    pub mod template {
        pub const BLOCKS: &str = "template.blocks";
        pub const SLOTS: &str = "template.slots";
        pub const ENTITIES: &str = "template.entities";
        pub const LOOT: &str = "template.loot";
        pub const DATA: &str = "template.data";
        pub const JOINTS: &str = "template.joints";
    }

    /// A palette (`entry_kind::PALETTE`): `SLOTS` (`[PaletteSlot]`), the block each slot of a
    /// template takes. A slot `@wall:stairs` takes the stairs `shapes_of` made of `wall`'s block.
    pub mod palette {
        pub const SLOTS: &str = "palette.slots";
    }

    /// A natural spawning rule's (`entry_kind::SPAWN`). Every two seconds the engine tries a few
    /// spots 20 to 45 blocks from each player; a spot takes one of the rules that hold there, by
    /// `WEIGHT` (`u32`, 1), while that entity category is under its cap near the player (8
    /// hostile, 6 passive, 6 other within 64 blocks). `ENTITY` (`str`) spawns `GROUP` (`[u32;2]`,
    /// at least and at most, 1 and 1) at a time with `CHANCE` (`f32`, 1) per attempt once picked,
    /// at a `PLACE` (`u32`, `spawn_place`, on the ground) where the conditions `WHEN` (`[str]`)
    /// all hold, asked about the spot and the block it rests on (under it, above it for a ceiling,
    /// the liquid itself): `light(..7)`, `block(#grass)`. The `core:mob_spawning` rule turns it off.
    pub mod spawn {
        pub const ENTITY: &str = "spawn.entity";
        pub const PLACE: &str = "spawn.place";
        pub const GROUP: &str = "spawn.group";
        pub const WEIGHT: &str = "spawn.weight";
        pub const CHANCE: &str = "spawn.chance";
        pub const WHEN: &str = "spawn.when";
    }

    /// A dimension's terrain (`entry_kind::TERRAIN`): `STONE` (`str`, the block it is made of),
    /// `BASE_HEIGHT` (`f64`, the ground before features, 64), `SEA` (`TerrainSea`, none), `CAVES`
    /// (`TerrainCaves`, none), `FIELDS` (`[TerrainField]`, after the engine's `temperature`,
    /// `humidity` and `continent`; one named again takes the earlier one's place), `LANDFORMS`
    /// (`[Landform]`), `MASKS` (`[LandformMask]`), `RULES` (`[BiomeRule]`, tried in order:
    /// the first that fits wins, the last where none does), `RULE_EDITS` (`[BiomeRule]`: what
    /// each one's `fields` give replaces that of the rule of its name), `CAVE_BIOMES`
    /// (`[CaveBiome]`).
    pub mod terrain {
        pub const STONE: &str = "terrain.stone";
        pub const BASE_HEIGHT: &str = "terrain.base_height";
        pub const SEA: &str = "terrain.sea";
        pub const CAVES: &str = "terrain.caves";
        pub const FIELDS: &str = "terrain.fields";
        pub const LANDFORMS: &str = "terrain.landforms";
        pub const MASKS: &str = "terrain.masks";
        pub const RULES: &str = "terrain.rules";
        pub const RULE_EDITS: &str = "terrain.rule_edits";
        pub const CAVE_BIOMES: &str = "terrain.cave_biomes";
    }

    /// An item's own (`entry_kind::ITEM`), besides the shared ones (`max_stack`, `durability`,
    /// tools, food ...) that go in its component table: `comp::LAYERS` (`str`, how it looks,
    /// as a stack's: one layer, `<ns>:item/<name>`, when not given; empty with `BLOCK`: the
    /// block's model), `BLOCK` (`str`, the block it
    /// places), `ON_USE` (`callback`, `ItemUseArgs`), `ON_RELEASE` (`callback`, `ItemUseArgs`: the
    /// use button let go while using it, `held_ticks` after how long: a bow), `CONDITION`
    /// (`callback`, `ItemModelArgs`: how a stack looks by what is in it, asked once per kind of
    /// stack).
    pub mod item {
        pub const BLOCK: &str = "item.block";
        pub const ON_USE: &str = "item.on_use";
        pub const ON_RELEASE: &str = "item.on_release";
        pub const CONDITION: &str = "item.condition";
    }

    /// A block's (`entry_kind::BLOCK`). Its states are every combination of `PROPS`
    /// (`[BlockProp]`, the first value the default, the last property changing fastest; none: one
    /// state). Each state gets the block's ops, then those of each part (`EntryDesc::parts`) whose
    /// key fits it, in order; an edit's ops come after the parts before it, so they win on every
    /// state. The block's own: callbacks `ON_USE` (`UseArgs`, `handled = 1` consumes the click),
    /// `ON_NEIGHBOR`, `ON_SCHEDULED`, `ON_CARRIED` (`BlockEventArgs`; on a moving body it came to
    /// another cell of the world: `pos` that cell, `other` where it is on the body), `ON_GROW`
    /// (`BlockEventArgs`, a growth step; without, a step moves to the next state and the last one
    /// stops growing), `PLACEMENT` (`PlaceArgs`, the state a player places), `MERGE` (`PlaceArgs`,
    /// its own item placed in its cell: another state in `state` takes the item, the same one
    /// leaves it to be placed as usual), `ON_SIGNAL` (`SignalArgs`, the power around a
    /// `signal_kind::SINK` changed), `SIGNAL_GATE` (`GateArgs`, a `signal_kind::GATE`'s output),
    /// `SIGNAL_READ` (`SignalReadArgs`, what a comparator behind it reads instead of how full it
    /// is); `OPENS` (`str`, a menu using it opens, on its block entity if it has one: a station of
    /// the menu's recipe kinds), `SNEAK_USE` (`u32`, `sneak_use`: whether a sneaking player uses it
    /// or what it holds; none: vanilla's, used with empty hands), `SHARES_CELL` (`match`: what may share its cell
    /// though their shapes overlap), `MODEL_FILE` (`str`, `models/block/<path>.json` of the named
    /// mod instead of the one named like the block), `MODEL_TEXTURES` (`[ModelTexture]`),
    /// `SHAPES_OF` (`ShapesOf`: no block of its own but a template, one for each full cube among
    /// the sources), `RETEXTURE` (`[Retexture]`, textures renamed on its states and its item). Per
    /// state: a tag per `block_flags` (`FLAGS`) and `NOT_SOLID`; `LIGHT_FILTER` (`[u8;3]`,
    /// absorption on top of the opacity, colored glass), `LIGHT_OPACITY` (`u8`, light it takes
    /// away, 15 for an opaque one, else 0), `LAYER` (`u32`, `RenderLayer`: cutout for boxes, else
    /// opaque), `MODEL` (`u32`, `BlockModel`, a cube), `TEXTURES` (`[str;6]`, by face, each
    /// `LAYERS` text, `<ns>:block/<name>`; layers tinted by a kind take its biome color, which
    /// gives the state its `TINT` and `TINT_FACES`), `TURNS` (`[u8;6]`, quarter turns of a cube's face pictures), `BOXES`
    /// (`[ModelBox]`), `HITBOX` (`[[f32;6]]`, min and max in sixteenths: what stops a walker if it
    /// is solid and what the crosshair picks; none: the boxes, else a plant's cross, else the
    /// whole block) and `OUTLINE` (likewise, what is outlined and cracked when it isn't the
    /// hitbox), `SIGNAL` (`SignalDesc`), `LIQUID_WALLS` (`u8`, a bit per `FACE_DIRS` side a liquid
    /// in its cell can't pass; none: the sides its hitbox covers whole), `PIECES` (`[Piece]`, what a
    /// hit breaks off alone; none: all of it), `CELLS` (`[[i32;3]]`, the other cells it takes
    /// besides its own, a door's upper half; joined by faces), `RULES` (`[BlockRule]`, what happens to it by
    /// itself), `HAZARDS` (`[Hazard]`, what it does to entities touching it), `TURN` (`u8`, `Turn::index`:
    /// all of the above written facing north, turned so; none: as written). It is opaque (hides its
    /// neighbors' faces, gets ambient occlusion) as a cube on the opaque layer. Any other component
    /// goes in its component table, per state; its item gets the first state's unless it sets
    /// its own.
    pub mod block {
        pub const ON_SIDES: &str = "on_sides";
        pub const PROPS: &str = "block.props";
        pub const ON_USE: &str = "block.on_use";
        pub const ON_NEIGHBOR: &str = "block.on_neighbor";
        pub const ON_SCHEDULED: &str = "block.on_scheduled";
        pub const ON_CARRIED: &str = "block.on_carried";
        pub const ON_GROW: &str = "block.on_grow";
        pub const PLACEMENT: &str = "block.placement";
        pub const MERGE: &str = "block.merge";
        pub const ON_SIGNAL: &str = "block.on_signal";
        pub const SIGNAL_GATE: &str = "block.signal_gate";
        pub const SIGNAL_READ: &str = "block.signal_read";
        pub const OPENS: &str = "block.opens";
        pub const SNEAK_USE: &str = "block.sneak_use";
        pub const SHARES_CELL: &str = "block.shares_cell";
        pub const MODEL_FILE: &str = "block.model_file";
        pub const MODEL_TEXTURES: &str = "block.model_textures";
        pub const SHAPES_OF: &str = "block.shapes_of";
        pub const RETEXTURE: &str = "block.retexture";
        pub const LIGHT_FILTER: &str = "block.light_filter";
        pub const LIGHT_OPACITY: &str = "block.light_opacity";
        pub const LAYER: &str = "block.layer";
        pub const MODEL: &str = "block.model";
        /// Item sprite from this face (u8, FACE_DIRS); 255 draws the model.
        pub const ITEM_FACE: &str = "block.item_face";
        pub const TEXTURES: &str = "block.textures";
        pub const TURNS: &str = "block.turns";
        pub const TURN: &str = "block.turn";
        pub const BOXES: &str = "block.boxes";
        pub const HITBOX: &str = "block.hitbox";
        pub const OUTLINE: &str = "block.outline";
        pub const SIGNAL: &str = "block.signal";
        pub const LIQUID_WALLS: &str = "block.liquid_walls";
        pub const NOT_SOLID: &str = "block.not_solid";
        pub const REPLACEABLE: &str = "block.replaceable";
        pub const CLIMBABLE: &str = "block.climbable";
        pub const NOT_TARGETABLE: &str = "block.not_targetable";
        pub const CULL_SAME: &str = "block.cull_same";
        pub const IMMOVABLE: &str = "block.immovable";
        pub const SIGNAL_BLOCKER: &str = "block.signal_blocker";
        pub const STICKY: &str = "block.sticky";
        pub const NOT_STOOD_ON: &str = "block.not_stood_on";
        pub const PIECES: &str = "block.pieces";
        pub const CELLS: &str = "block.cells";
        pub const RULES: &str = "block.rules";
        pub const HAZARDS: &str = "block.hazards";
    }

    /// A biome's own climate and surface (`entry_kind::BIOME`), what every terrain rule placing
    /// it starts from (`BiomeRule` says what each is): `RANGES` (`[RuleRange]`, a field named again
    /// replaces the earlier range; none: everywhere), `HEIGHT` (`[i32;2]`, any), `TOP`, `FILLER`
    /// (`str`, none: stone), `DEPTH` (`i32`, 3), `UNDER` (`str`), `UNDER_LAYERS` (`i32`, 0),
    /// `DEEP_TOP` (`str`), `DEEP_BELOW` (`i32`), `BARE_ABOVE` (`i32`), `ICE` (`str`). `TINTS`
    /// (`[BiomeTint]`, a kind given again wins; the others keep their default). Any other
    /// component goes in its component table (`precipitation`, scatters' densities).
    pub mod biome {
        pub const RANGES: &str = "biome.ranges";
        pub const HEIGHT: &str = "biome.height";
        pub const TOP: &str = "biome.top";
        pub const FILLER: &str = "biome.filler";
        pub const DEPTH: &str = "biome.depth";
        pub const UNDER: &str = "biome.under";
        pub const UNDER_LAYERS: &str = "biome.under_layers";
        pub const DEEP_TOP: &str = "biome.deep_top";
        pub const DEEP_BELOW: &str = "biome.deep_below";
        pub const BARE_ABOVE: &str = "biome.bare_above";
        pub const ICE: &str = "biome.ice";
        pub const TINTS: &str = "biome.tints";
    }

    /// A dimension's (`entry_kind::DIMENSION`): `GENERATOR` (`callback`, `GenArgs`, called in
    /// parallel, a pure function of its args), `SPAWN` (`callback`, `SpawnArgs`; none: open ground
    /// near 0, 0 on its columns, biomes tagged `spawn_biome` first), `COLUMN` (`callback`,
    /// `ColumnArgs`, pure: surface and biome of a column of a dimension without a terrain, which
    /// the generator, structures, scatters and the spawn search all stand on), `CUBES`
    /// (`[i32;2]`, cube-y bounds, inclusive, unbounded), `SKY_ABOVE` (`i32`, cubes from here up are
    /// empty, so sky light there is full without generating them; unknown), `GRAVITY` (`f32`, 32),
    /// `DAY_LENGTH` (`u32`, ticks, 24000; 0: time stays at `FIXED_TIME`, `u32`, 6000), `DAYLIGHT`
    /// (`[DaylightPoint]`, none: vanilla's curve), `SKY_COLOR` and `FOG_COLOR` (`u32` 0xRRGGBB),
    /// `AMBIENT` (`u32`, light everything gets at least, 0..15, 0), `WEATHERS` (`[str]`, the weathers
    /// that come after clear weather; none: always clear). Tags: `NO_SKY_LIGHT`, `PLAYER_SPAWN`
    /// (players start and respawn here; of several, the one of the mod loaded last), `NO_BLOCK_UPDATES`
    /// (no neighbor updates, scheduled, random or growth ticks), `BARE` (only ores, scatters and
    /// decorations that name it). Any other component goes in its component table.
    pub mod dimension {
        pub const GENERATOR: &str = "dimension.generator";
        pub const SPAWN: &str = "dimension.spawn";
        pub const COLUMN: &str = "dimension.column";
        pub const CUBES: &str = "dimension.cubes";
        pub const SKY_ABOVE: &str = "dimension.sky_above";
        pub const HAZARDS: &str = "dimension.hazards";
        pub const GRAVITY: &str = "dimension.gravity";
        pub const DAY_LENGTH: &str = "dimension.day_length";
        pub const FIXED_TIME: &str = "dimension.fixed_time";
        pub const DAYLIGHT: &str = "dimension.daylight";
        pub const SKY_COLOR: &str = "dimension.sky_color";
        pub const FOG_COLOR: &str = "dimension.fog_color";
        pub const AMBIENT: &str = "dimension.ambient";
        pub const WEATHERS: &str = "dimension.weathers";
        pub const NO_SKY_LIGHT: &str = "dimension.no_sky_light";
        pub const PLAYER_SPAWN: &str = "dimension.player_spawn";
        pub const NO_BLOCK_UPDATES: &str = "dimension.no_block_updates";
        pub const BARE: &str = "dimension.bare";
    }

    /// A menu's (`entry_kind::MENU`, screens included): `GROUPS` (`[SlotGroupDef]`, its slots in
    /// this order), `RECIPES` (`[str]`, recipe kinds its `arrow` bars open in the recipe viewer),
    /// callbacks `ON_CHANGE` (`MenuArgs`, server: a slot changed), `ON_TAKE` (`MenuArgs` with
    /// `slot`, server: an output slot was taken), `ON_TICK` (`MenuArgs`, server, every tick while
    /// open), `SCREEN` (`UiArgs`, client: builds the screen, its slots by group name), `ON_KEY`
    /// (`ScreenKeyArgs`, every key while its screen is on top, before Escape and the other global
    /// keys), and its tags (how its screen behaves).
    pub mod menu {
        pub const GROUPS: &str = "menu.groups";
        pub const RECIPES: &str = "menu.recipes";
        pub const ON_CHANGE: &str = "menu.on_change";
        pub const ON_TAKE: &str = "menu.on_take";
        pub const ON_TICK: &str = "menu.on_tick";
        pub const SCREEN: &str = "menu.screen";
        pub const ON_KEY: &str = "menu.on_key";
        /// Single player stops while it is open.
        pub const PAUSES: &str = "menu.pauses";
        /// Escape doesn't close it (the death screen).
        pub const KEEP_ON_ESCAPE: &str = "menu.keep_on_escape";
        /// Closes when the player respawns.
        pub const CLOSES_ON_RESPAWN: &str = "menu.closes_on_respawn";
        /// Gets keys before Escape and the other global keys.
        pub const KEYS_FIRST: &str = "menu.keys_first";
        /// Like the chat screen: the HUD leaves the messages to it, and Enter sends the text of its
        /// INPUT of class `chat`.
        pub const SHOWS_CHAT: &str = "menu.shows_chat";
    }

    /// An entity kind's (`entry_kind::ENTITY`, the engine's `core:player`, `core:item`,
    /// `core:falling_block`, `core:moving_blocks` and `core:platform` too). Its body: `SIZE`
    /// (`BodySize`, 0.6 by 1.8), `EYE` (`f32` blocks, `EYE_SHARE` of its height), `SPEED` (`f32`,
    /// walking, blocks per second, 3), `CARRY_HEIGHT` (`f32`, a solid body's riders up to this high
    /// above it move with it, their own motion relative to it; 0: only standing on it),
    /// `RIDE_CAMERA` (`f32`, blocks the third-person camera goes further back riding it; none: by
    /// its size). Its look: `TEXTURE` (`str`, `<ns>:entity/<name>`), `TEXTURE_SIZE` (`[u32;2]`,
    /// width and height of the skin `PARTS` are laid out on, 64 by 32), `PARTS` (`[ModelPart]`, its
    /// model in code; a model file, `comp::MODEL`, wins), `BONE_MODELS` (`[BoneModel]`). `CATEGORY`
    /// (`str`, an `entry_kind::ENTITY_CATEGORY`, `core:misc`), `TEMPLATE` (`str`, a moving body built
    /// of that template, block (0, 0, 0) centered on it), `BLOCK` (`str`, the block it is the block
    /// entity of: made with its inventory where one is placed, gone with its items when it breaks),
    /// `SEATS` (`[Seat]`, where riders sit, numbered in this order), `ATTACH` (`[Attach]`, components
    /// each one carries, starting as the kind's own value of each), `LIGHT_AT` (`[f32;3]`, where the
    /// light it gives comes from: blocks from its feet, +Z ahead, turned with it; none: its middle, a
    /// block entity's block),
    /// `SHOWN_SLOTS` (`[u32]`, slots of its inventory everyone around sees, their items in
    /// `core:shown`: what a pedestal holds; they give their light too). Callbacks: `ON_TICK` (`EntityTickArgs`, every one of the kind in one
    /// call), `ON_INTERACT` (`EntityEventArgs`, a player right clicked one and
    /// `action::INTERACT_ENTITY` let it; `handled = 1` swings the player's arm), `STEER`
    /// (`SteerArgs`, on the client of the player in a seat that steers: what the mount does with
    /// its keys; without it that player rides along), `DISMOUNT` (`DismountArgs`, where a rider of a
    /// `seat_exit::CALL` seat gets off), `FRAMES` (`[callback]`, `ClientEntityArgs` every frame on
    /// the client for each one, in this order, each as the mod that added it). The player's own:
    /// `SLOT_GROUPS` (`[SlotGroupDef]`, its inventory as named groups, one `slot_flags::HOTBAR`;
    /// the engine lays them out, the hotbar first and the equipment slots last; a hotbar of 9 and 27
    /// more), `INVENTORY_MENU` (`str`, the menu its inventory key opens). Tags: the ones below,
    /// `NO_FALL_DAMAGE` and `UNPUSHABLE` turning off what every kind does. Any other component goes in its component table.
    pub mod entity {
        pub const SIZE: &str = "entity.size";
        pub const EYE: &str = "entity.eye";
        pub const SPEED: &str = "entity.speed";
        pub const CARRY_HEIGHT: &str = "entity.carry_height";
        pub const RIDE_CAMERA: &str = "entity.ride_camera";
        pub const TEXTURE: &str = "entity.texture";
        pub const TEXTURE_SIZE: &str = "entity.texture_size";
        pub const PARTS: &str = "entity.parts";
        pub const BONE_MODELS: &str = "entity.bone_models";
        pub const CATEGORY: &str = "entity.category";
        pub const TEMPLATE: &str = "entity.template";
        pub const BLOCK: &str = "entity.block";
        pub const SEATS: &str = "entity.seats";
        pub const LIGHT_AT: &str = "entity.light_at";
        pub const SHOWN_SLOTS: &str = "entity.shown_slots";
        pub const ATTACH: &str = "entity.attach";
        pub const ON_TICK: &str = "entity.on_tick";
        pub const ON_INTERACT: &str = "entity.on_interact";
        pub const STEER: &str = "entity.steer";
        pub const DISMOUNT: &str = "entity.dismount";
        pub const FRAMES: &str = "entity.frames";
        pub const SLOT_GROUPS: &str = "entity.slot_groups";
        pub const INVENTORY_MENU: &str = "entity.inventory_menu";
        pub const NO_GRAVITY: &str = "entity.no_gravity";
        /// `[Hazard]`: what it does to entities, itself (`hazard_at::SELF`: the sun burning
        /// the undead) or what it squeezes (`CRUSHED`).
        pub const HAZARDS: &str = "entity.hazards";
        pub const SWIMS: &str = "entity.swims";
        /// A solid body: others stand on it (riding along), bump into it and get pushed by it.
        pub const SOLID: &str = "entity.solid";
        /// Moved exactly by its velocity (set by mod logic): no gravity, no block collision.
        pub const KINEMATIC: &str = "entity.kinematic";
        /// Its riders take the damage of its falls too, as a horse's rider does.
        pub const RIDERS_FALL: &str = "entity.riders_fall";
        /// Has the equipment slots (`Registry::equipment_slot`) the player has, after its own:
        /// armor on a zombie, shown on its model and counted like a player's.
        pub const WEARS: &str = "entity.wears";
        /// Picks up the dropped items it touches into its inventory, as a player does (a zombie,
        /// an allay).
        pub const PICKS_UP: &str = "entity.picks_up";
        /// Takes the poses a player does, by the keys its walk holds (`walk::*`) and the poses'
        /// conditions: crouches, swims lying down, crawls where nothing higher fits. A cow
        /// without it stands where it stands.
        pub const POSES: &str = "entity.poses";
        /// Takes no fall damage, which every kind does otherwise.
        pub const NO_FALL_DAMAGE: &str = "entity.no_fall_damage";
        /// Others don't push it away, which every kind does otherwise.
        pub const UNPUSHABLE: &str = "entity.unpushable";
        /// It flies as a projectile (`Projectile`): the engine moves it, and each tick a ray along
        /// its speed decides what it hits (`action::PROJECTILE_HIT`).
        pub const PROJECTILE: &str = "entity.projectile";
        /// What hits land on, part by part (`Hitboxes`); without, its whole box.
        pub const HITBOXES: &str = "entity.hitboxes";
    }

    /// A step's (`entry_kind::STEP`): `CHAIN` (`str`, the value it changes every time it is
    /// computed; none: it only runs where a modifier names it), `PHASE` (`str`, `phase::*` or a
    /// mod's), `AFTER` and `BEFORE` (`[str]`, steps of the same phase it runs after or before),
    /// `CALL` (`callback`, `StepArgs`; the engine's own steps have none until a mod sets one).
    pub mod step {
        pub const CHAIN: &str = "step.chain";
        pub const PHASE: &str = "step.phase";
        pub const AFTER: &str = "step.after";
        pub const BEFORE: &str = "step.before";
        pub const CALL: &str = "step.call";
    }

    /// A phase's (`entry_kind::PHASE`): `AFTER` (`str`), the phase it comes right after.
    pub mod phase {
        pub const AFTER: &str = "phase.after";
    }

    /// A bone motion's (`entry_kind::BONE_MOTION`): `GRAVITY`, the pull toward hanging straight
    /// down, `SPRING`, the pull back to where the clips have it, `DRAG`, how fast a swing dies down
    /// (`f32` each, 0).
    pub mod bone_motion {
        pub const GRAVITY: &str = "bone_motion.gravity";
        pub const SPRING: &str = "bone_motion.spring";
        pub const DRAG: &str = "bone_motion.drag";
    }

    /// A weather's (`entry_kind::WEATHER`). How it looks, each `f32` 0..1 (rain's unless set):
    /// `FALLS` how much of the precipitation falls, `SKY_GRAY`, `SKY_COVER` (sun, moon, stars),
    /// `FOG_GRAY`, `CLOUD_GRAY`, `CLOUD_DARK` (thunder 0.95), `DARKNESS` the share of daylight
    /// taken. `SOUND` (`str`) a sound event now and then under the open sky while it falls,
    /// `DURATION` (`[u32;2]`) ticks at least and at most, `WEIGHT` (`u32`) how likely it comes
    /// after clear weather. Tags: `DRY` nothing falls, `LIGHTNING`, `UNSHELTERED` things burn in
    /// the sun under it.
    pub mod weather {
        pub const FALLS: &str = "weather.falls";
        pub const SKY_GRAY: &str = "weather.sky_gray";
        pub const SKY_COVER: &str = "weather.sky_cover";
        pub const FOG_GRAY: &str = "weather.fog_gray";
        pub const CLOUD_GRAY: &str = "weather.cloud_gray";
        pub const CLOUD_DARK: &str = "weather.cloud_dark";
        pub const DARKNESS: &str = "weather.darkness";
        pub const SOUND: &str = "weather.sound";
        pub const DURATION: &str = "weather.duration";
        pub const WEIGHT: &str = "weather.weight";
        pub const DRY: &str = "weather.dry";
        pub const LIGHTNING: &str = "weather.lightning";
        pub const UNSHELTERED: &str = "weather.unsheltered";
    }
    pub const MAX_STACK: &str = "max_stack";
    pub const DURABILITY: &str = "durability";
    pub const ATTACK_DAMAGE: &str = "attack_damage";
    /// Item: ticks of holding the use button before it takes effect (food 32); slows while held.
    /// `USE_HOLD`: it never does by itself, only its `ON_RELEASE` when let go (a bow).
    pub const USE_TICKS: &str = "use_ticks";
    /// Item: `Blocking`, attacks stopped while using it. Weapon: `DISABLE_BLOCKING` seconds.
    pub const BLOCKS_ATTACKS: &str = "blocks_attacks";
    pub const DISABLE_BLOCKING: &str = "disable_blocking";
    /// Item: `ProjectileCatch`, blocked sticking projectiles are kept in its stack data.
    pub const CATCH_PROJECTILES: &str = "catch_projectiles";
    /// Stack: `[EmbeddedProjectile]`, retained when moved, saved and shared with clients.
    pub const EMBEDDED_PROJECTILES: &str = "embedded_projectiles";
    /// Item: `[EffectGift]` its user gets when it is used (after `USE_TICKS`), the item turning
    /// into its `REMAINDER` (a potion's bottle) or one fewer; nothing else needed for a potion.
    pub const GIVES_EFFECTS: &str = "gives_effects";
    /// Item: full swings per second (a sword 1.6). On an entity kind, its bare hand's (the
    /// player 4); 1 when neither says.
    pub const ATTACK_SPEED: &str = "attack_speed";
    /// Item: a full swing standing on the ground also hits what stands next to the target.
    pub const SWEEPS: &str = "sweeps";
    /// On an item: the entity it is the spawn egg of, by name. It is called after it unless it has a
    /// name of its own, and looks generic without a texture of its own.
    pub const SPAWN_EGG: &str = "spawn_egg";
    /// On an item: in creative it doesn't break blocks (a sword). Client and server both read it.
    pub const NO_CREATIVE_BREAK: &str = "no_creative_break";
    /// Tag of items in no creative tab nor its search (one a mod hands out, a variant), though
    /// obtainable like any.
    pub const NO_TAB: &str = "no_tab";
    pub const MINING_SPEED: &str = "mining_speed";
    pub const TOOL_LEVEL: &str = "tool_level";
    /// The block drops its loot only to a fitting tool of at least this level (0 = any tier).
    pub const HARVEST_LEVEL: &str = "harvest_level";
    /// Items: right click tries the hand whose item has the higher one first (default 0).
    pub const USE_PRIORITY: &str = "use_priority";
    /// Items, a tag: needs both hands.
    pub const TWO_HANDED: &str = "two_handed";
    /// Items: a two-handed item may be held in one hand after all, playing this action.
    pub const ONE_HANDED: &str = "one_handed";
    /// How many times a block's loot is rolled when it breaks (two slabs in one block).
    pub const LOOT_ROLLS: &str = "loot_rolls";
    pub const WEAR: &str = "wear";
    /// Block: seconds to break by hand at speed 1 (`f32`, 1 by default); negative = unbreakable.
    pub const HARDNESS: &str = "hardness";
    /// What a block state or entity kind sounds and shows on an event (`[Effect]`), over what its
    /// `SOUND` group and the engine give: a block's `break`, `place`, `step`, `hit`, `land`
    /// (fallen onto), `sprint` (run on), an entity's `hurt`, `death`, `ambient`, `step`.
    pub const EFFECTS: &str = "effects";
    /// Tag of blocks that burn what is in them as lava does (15 seconds of fire, 4 damage), unless
    /// they say what they do (`block::HAZARDS`); a liquid of 1000 degrees has it by itself.
    pub const HOT: &str = "hot";
    /// Block: how slippery its top is (`f32`, 0.6 by default; ice 0.98).
    pub const FRICTION: &str = "friction";
    /// Block: movement speed multiplier inside it (`f32`, 1 by default; cobweb, liquids).
    pub const DRAG: &str = "drag";
    /// Emitted light per color channel (`[u8;3]`). 15 is a full-strength source (torch 14);
    /// higher values reach further. On an item (held, worn, shown or dropped), an entity kind, an
    /// entity effect or an entity itself (attached, set while it lives) it moves with the entity,
    /// drawn by clients alone: it lights what players see, not the world.
    pub const LIGHT: &str = "light";
    /// Block: a tint kind by name (`tint`: `core:grass`, `core:foliage`, `core:water` or a mod's
    /// own), colored by the biome. Set by the engine from the faces' layers (`Layer::kind`).
    pub const TINT: &str = "tint";
    /// Block: faces the tint applies to (`u8`, a bit per face in -X, +X, -Y, +Y, -Z, +Z order),
    /// from the faces' layers too.
    pub const TINT_FACES: &str = "tint_faces";
    /// The own name of one stack or block entity (UTF-8), as a player gave it: shown instead of
    /// the item's or block's, and carried from the stack to the placed block and back.
    pub const NAME: &str = "core:name";
    /// Entity tag the engine keeps: it never despawns (`entity_state::PERSISTENT`).
    pub const PERSISTENT: &str = "persistent";
    /// Blocks connecting energy networks.
    pub const CONDUCTOR: &str = "conductor";
    /// Entity or stack component, layout `Energy`.
    pub const ENERGY: &str = "energy";
    /// Item: it is a liquid (`str`, the liquid's block name) by the millibucket, what liquid
    /// slots hold and recipes count. Every liquid has one, named like it.
    pub const FLUID: &str = "fluid";
    /// Items and entity kinds: slots of the inventory each stack or entity carries (`u32`).
    pub const INVENTORY: &str = "inventory";
    /// Stack component the engine keeps: what an item with `inventory` holds.
    pub const CONTENTS: &str = "contents";
    /// How an item and its stacks look, and each face of a block (`block::TEXTURES`): pictures
    /// one over the other, the first at the bottom (a tool made of parts: handle, head, binding;
    /// a grass block's side: dirt and the green over it; a plain one is one). Text: texture
    /// names, space separated, each tinted as `Layer` says. On the item (`entry_kind::ITEM`) it is
    /// every stack's; a stack's own replaces it.
    pub const LAYERS: &str = "layers";
    /// Item shown on the wearer's model: `Worn`, plus `worn_texture: str` in the wearer's skin layout.
    pub const WORN: &str = "worn";
    pub const WORN_TEXTURE: &str = "worn_texture";
    /// Biome: what falls there (`str`, a precipitation's name); without it rain.
    pub const PRECIPITATION: &str = "precipitation";
    /// Item: the liquid (block name, `str`) it holds; with `remainder` (the empty container's
    /// name) the engine fills and empties it like a bucket.
    pub const LIQUID: &str = "liquid";
    /// Item left behind when this one is used up (fuel, crafting, pouring): `item`.
    pub const REMAINDER: &str = "remainder";
    /// Entity kinds and blocks: shaded by `shaders/entity/<name>.wgsl` or `shaders/block/<name>.wgsl`
    /// (`str`).
    pub const SHADER: &str = "shader";
    /// Entity kinds: model file (`models/<name>.geo.json`, Bedrock geometry as Blockbench
    /// writes it) used instead of the parts in code; without it `<ns>:<kind name>` is tried.
    pub const MODEL: &str = "model";
    /// A worn item shows as this model on the wearer: `model@bone`.
    pub const WORN_MODEL: &str = "worn_model";
    /// Entity kinds: keyframe animations (`animations/<name>.animation.json`, Bedrock format);
    /// `idle` and `walk` play by themselves, others when the server says so. Without it
    /// `<ns>:<kind name>` is tried.
    pub const ANIMATIONS: &str = "animations";
    /// Sound event prefix (`str`): blocks play `<prefix>.break`, `.place`, `.step`, `.hit`;
    /// entity kinds `.hurt`, `.death`, `.ambient`, `.step` (each only if registered).
    pub const SOUND: &str = "sound";
    /// Items: durability lost per hit on a mob (`u32`); without it tools lose 2, like vanilla's
    /// tools made for blocks (swords set 1).
    pub const WEAR_PER_HIT: &str = "wear_per_hit";
    /// Tag of biomes players may start in (the engine's spawn search tries them first).
    pub const SPAWN_BIOME: &str = "spawn_biome";
    /// Block: spreads onto these blocks (`match`; grass onto dirt) within `spread_reach` while
    /// `spread_when` holds for the target. In `spread_time` seconds on average, sooner with more
    /// of it around.
    pub const SPREADS_TO: &str = "spreads_to";
    /// `[i32;6]`: the box it reaches, from itself to the target, min x y z then max x y z, at
    /// most 4 each way; by default 1 sideways, 3 down and 1 up (grass).
    pub const SPREAD_REACH: &str = "spread_reach";
    /// `f32`: mean seconds for one spreading block to take a target (vanilla grass about 765).
    pub const SPREAD_TIME: &str = "spread_time";
    /// `[str]`: conditions about the target (`condition`), by default `light(9.., up)` and
    /// `!covered` (vanilla grass); moss up a wall takes `light(9.., any)`.
    pub const SPREAD_WHEN: &str = "spread_when";
    /// Block: a particle kind (`str`) clients show around it now and then, near the player only
    /// (vanilla's torch smoke); nothing of it reaches the server.
    pub const AMBIENT_PARTICLE: &str = "ambient_particle";
    /// Liquid block: a particle kind (`str`) that drips from the ceiling of a cave under it,
    /// near the player only (vanilla's water through a ceiling).
    pub const DRIP: &str = "drip";
    /// Liquid block: the particle kind (`str`) thrown up when something falls into it.
    pub const SPLASH: &str = "splash";
    /// Liquid block: the particle kind (`str`) that rises from what is under it and from a
    /// falling in.
    pub const BUBBLE: &str = "bubble";
    /// Entity kinds and worn items: the liquids sprinting swims in (`match`); a worn item's add
    /// to its wearer's.
    pub const SWIMS_IN: &str = "swims_in";
    /// Tag the engine puts on every liquid's block.
    pub const LIQUIDS: &str = "liquids";
    /// Tag the engine puts on blocks of negative `hardness`, liquids aside.
    pub const UNBREAKABLE: &str = "unbreakable";
}

/// Harvest levels (`tool_level` of a tool, `harvest_level` of a block) of the vanilla tiers.
/// Hundreds apart so mods fit their own tiers in between (a 250 tool mines up to 250).
pub mod tool_tier {
    pub const WOOD: u32 = 0;
    pub const STONE: u32 = 100;
    pub const IRON: u32 = 200;
    pub const DIAMOND: u32 = 300;
    /// `mined_by`: drops whatever mines it, by hand too.
    pub const HAND: u32 = u32::MAX;
}

// ---------------------------------------------------------------- content flags and values

pub mod block_flags {
    /// Entities collide with it.
    pub const SOLID: u32 = 1;
    /// Full opaque cube: hides neighbor faces, blocks light, gets ambient occlusion.
    pub const OPAQUE: u32 = 2;
    /// Placing a block into it replaces it (air, grass, water).
    pub const REPLACEABLE: u32 = 4;
    pub const LIQUID: u32 = 8;
    pub const CLIMBABLE: u32 = 16;
    /// Raycasts pass through (air, liquids for normal interaction).
    pub const NOT_TARGETABLE: u32 = 32;
    /// Hide faces between two blocks of this kind (glass). Without it see-through blocks keep
    /// their inner faces, which is what makes leaves look dense.
    pub const CULL_SAME: u32 = 128;
    /// Pistons can't move it.
    pub const IMMOVABLE: u32 = 256;
    /// Doesn't carry redstone power, the way glass doesn't. Wire can still lie on it.
    pub const SIGNAL_BLOCKER: u32 = 512;
    /// A piston moving it takes what touches it along, like slime.
    pub const STICKY: u32 = 1024;
    /// On a moving body it shoves, but can't be stood on and be taken along by: a piston's head
    /// sliding out from under whoever stands on the piston.
    pub const NOT_STOOD_ON: u32 = 2048;
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockModel {
    None = 0,
    /// Preset: a single full box, expanded when content is frozen.
    Cube = 1,
    /// Preset: two flat, rotated boxes per cell, expanded when content is frozen.
    Cross = 2,
    /// `boxes` with their own textures.
    Boxes = 4,
    /// Redstone wire: not meshed, the client draws it from its signal (its textures: the dot on
    /// top, the line on the sides).
    Wire = 8,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderLayer {
    Opaque = 0,
    /// Alpha tested (leaves, glass, plants).
    Cutout = 1,
    Translucent = 2,
}

/// How a struct shared as a component or event value is laid out, written next to it so the
/// engine and every mod compare the same text (`"{field:type,...}"`).
pub trait Schema {
    const SCHEMA: &'static str;
}

/// Faces in order: -X, +X, -Y, +Y, -Z, +Z.
/// A model box turned around an axis, a torch on a wall: around `origin` (1/16 block units), about
/// `axis`, by `degrees`, right-handed.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BoxTilt {
    pub origin: [f32; 3],
    pub axis: [f32; 3],
    pub degrees: f32,
}

impl BoxTilt {
    pub const NONE: BoxTilt = BoxTilt { origin: [0.0; 3], axis: [0.0, 1.0, 0.0], degrees: 0.0 };
}

impl Default for BoxTilt {
    fn default() -> BoxTilt {
        BoxTilt::NONE
    }
}

impl Schema for BoxTilt {
    const SCHEMA: &'static str = "{origin:[f32;3],axis:[f32;3],degrees:f32}";
}

crate::data! {
    /// Where blocked projectiles enter an item's model, in the named bone's pixels.
    pub struct ProjectileCatch {
        pub bone: crate::data::String,
        pub min: [f32; 3],
        pub max: [f32; 3],
        pub direction: [f32; 3],
    }
}

crate::data! {
    /// A box of a block state's model (`comp::block::BOXES`).
    pub struct ModelBox {
        /// In 1/16 block units.
        pub min: [f32; 3],
        pub max: [f32; 3],
        /// Per face; empty takes the state's texture of that face.
        pub textures: [alloc::string::String; 6],
        /// Per face texel rectangle (u0, v0, u1, v1, 0..16), `UV_FROM_BOX` or `UV_NO_FACE`.
        pub uv: [[f32; 4]; 6],
        /// Per face quarter turns of the picture (`FACE_CORNERS`).
        pub turn: [u8; 6],
        pub tilt: BoxTilt,
        /// Drawn without the shading by side, as cross plants are (a model file's `shade: false`).
        pub flat: bool,
    }
}

crate::data! {
    /// A piece of a block state that a player can break off alone (`comp::block::PIECES`): a hit
    /// inside the box leaves the state with the properties `left` sets, dropping what the state
    /// with those `alone` sets drops (`type=top`, `type=bottom` for a double slab's lower half).
    pub struct Piece {
        /// In 1/16 block units, written like the boxes.
        pub min: [f32; 3],
        pub max: [f32; 3],
        pub left: alloc::string::String,
        pub alone: alloc::string::String,
    }
}

crate::data! {
    /// What `event` sounds and shows (`comp::EFFECTS`): a sound event and a particle kind by
    /// name, empty for what it would be anyway (its sound group's, the engine's pieces of a
    /// block, a death's smoke), `none` for nothing.
    pub struct Effect {
        pub event: alloc::string::String,
        pub sound: alloc::string::String,
        pub particle: alloc::string::String,
    }
}

/// Where an entity meets a block for a `Hazard`.
pub mod hazard_at {
    /// Its box is in the block's shape (a liquid's cell): fire, lava, a berry bush.
    pub const INSIDE: u32 = 0;
    /// It stands on it: magma.
    pub const ON: u32 = 1;
    /// Its eyes are in it: what suffocates.
    pub const HEAD: u32 = 2;
    /// Its box is within a sixteenth of the shape: a cactus.
    pub const TOUCH: u32 = 3;
    /// Wherever it is, asked at its eyes: an entity kind's own (the sun burning a zombie) or a
    /// dimension's (the void).
    pub const SELF: u32 = 4;
    /// A body of this kind squeezes it into something solid; a kind with one crushes instead of
    /// only pushing.
    pub const CRUSHED: u32 = 5;
}

crate::data! {
    /// What a block, an entity kind or a dimension does to an entity meeting it `at` (`hazard_at`,
    /// `comp::block::HAZARDS`, `comp::entity::HAZARDS`, `comp::dimension::HAZARDS`): every
    /// `every` ticks (0 or 1: each tick, a hurt entity's pause between hits aside) while all of
    /// `when` hold for the entity there (`condition`, `!flying`), `damage` of `cause` (a damage
    /// cause, empty for none) and `fire` ticks of burning. Every opaque full block suffocates
    /// at the head; a `hot` one without hazards of its own burns like lava; every dimension's
    /// void hurts 64 blocks below its floor.
    pub struct Hazard {
        pub at: u32,
        pub damage: f32,
        pub cause: alloc::string::String,
        pub fire: u32,
        pub every: u32,
        pub when: alloc::vec::Vec<alloc::string::String>,
        /// An entity effect's: only at this level of it, as it is; 0, at every level, `damage`
        /// times the level. Negative damage heals.
        pub level: u32,
    }
}

/// When a `BlockRule` is asked.
pub mod rule_when {
    /// Something next to it changed, or it was placed: `secs` later (0: at once). A state's
    /// rules that wait are all asked at the first of their waits.
    pub const NEIGHBOR: u32 = 0;
    /// Now and then, every `secs` on average, like a random tick, while its conditions may
    /// hold; timed by world time, so a block in an unloaded area catches up when it loads.
    pub const RANDOM: u32 = 1;
}

/// What a `BlockRule` does when its conditions hold.
pub mod rule_then {
    /// It becomes `into`: properties set (`age=0`) or another block (`base:dirt`).
    pub const BECOME: u32 = 0;
    /// It grows: the block's `on_grow`, else its `age` one higher (nothing at the last).
    pub const GROW: u32 = 1;
    /// It breaks off, dropping what it drops (no support left).
    pub const BREAK: u32 = 2;
    /// It falls as a moving block (`Host::fall_block`, sand).
    pub const FALL: u32 = 3;
    /// A piece of it comes off: it becomes `into` (properties, `north=false`), dropping what
    /// the piece left so drops alone (a side of a vine or of wire); with nothing left, air.
    pub const SHED: u32 = 4;
}

crate::data! {
    /// What happens to a block state by itself (`comp::block::RULES`): when `when` (`rule_when`,
    /// with `secs`) asks and all of `conditions` hold for it (`condition`, about its own cell
    /// and state; sides named in them, `side(down)`, are of the state's own frame and turn with
    /// it), it does `then` (`rule_then`, with `into`). A neighbor rule that breaks it also keeps
    /// it from being placed where it would.
    pub struct BlockRule {
        pub when: u32,
        pub secs: f32,
        pub conditions: alloc::vec::Vec<alloc::string::String>,
        pub then: u32,
        pub into: alloc::string::String,
    }
}

/// A box's texel rectangle saying to take it from where the box is, as a cube's face would.
pub const UV_FROM_BOX: [f32; 4] = [-1.0, 0.0, 0.0, 0.0];
/// A box's texel rectangle saying it has no such face.
pub const UV_NO_FACE: [f32; 4] = [-2.0, 0.0, 0.0, 0.0];

/// The terms of a block matcher (`comp`): apart by spaces, except inside a state's `[..]`.
pub fn matcher_terms(text: &str) -> impl Iterator<Item = &str> {
    let mut depth = 0i32;
    text.split(move |ch: char| {
        match ch {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        depth == 0 && ch.is_whitespace()
    })
    .filter(|t| !t.is_empty())
}

/// One picture of `comp::LAYERS` text: `name`, `name#RRGGBB` (multiplied by that color),
/// `name@kind` (by a tint kind's color: a block's by the biome it stands in, an item's by the
/// kind's default), or both, `name#RRGGBB@kind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layer<'a> {
    pub name: &'a str,
    pub tint: Option<u32>,
    pub kind: Option<&'a str>,
}

impl<'a> Layer<'a> {
    pub fn plain(name: &'a str) -> Layer<'a> {
        Layer { name, tint: None, kind: None }
    }
}

/// `comp::LAYERS` text as its layers, the bottom first.
pub fn layers(text: &str) -> impl Iterator<Item = Layer<'_>> {
    text.split_whitespace().map(|l| {
        let (l, kind) = match l.split_once('@') {
            Some((l, kind)) => (l, Some(kind)),
            None => (l, None),
        };
        match l.rsplit_once('#') {
            Some((name, hex)) => Layer { name, tint: u32::from_str_radix(hex, 16).ok(), kind },
            None => Layer { name: l, tint: None, kind },
        }
    })
}

/// Layers as `comp::LAYERS` text, as `layers` reads it.
pub fn layers_text<'a>(layers: impl IntoIterator<Item = Layer<'a>>) -> alloc::string::String {
    use core::fmt::Write;
    let mut text = alloc::string::String::new();
    for l in layers {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(l.name);
        if let Some(t) = l.tint {
            let _ = write!(text, "#{:06X}", t & 0xFF_FFFF);
        }
        if let Some(k) = l.kind {
            text.push('@');
            text.push_str(k);
        }
    }
    text
}

/// The texels a box between `min` and `max` (texels too) shows on a face when it says nothing, a
/// box reaching out of its cell going on with the edge of the picture.
pub fn box_face_uv(face: usize, min: [f32; 3], max: [f32; 3]) -> [f32; 4] {
    let (min, max) = (min.map(|v| v.clamp(0.0, 16.0)), max.map(|v| v.clamp(0.0, 16.0)));
    match face {
        0 => [min[2], 16.0 - max[1], max[2], 16.0 - min[1]],
        1 => [16.0 - max[2], 16.0 - max[1], 16.0 - min[2], 16.0 - min[1]],
        2 | 3 => [min[0], min[2], max[0], max[2]],
        4 => [16.0 - max[0], 16.0 - max[1], 16.0 - min[0], 16.0 - min[1]],
        _ => [min[0], 16.0 - max[1], max[0], 16.0 - min[1]],
    }
}

/// The six directions, in the order of faces everywhere (-X, +X, -Y, +Y, -Z, +Z).
pub const FACE_DIRS: [[i32; 3]; 6] = [[-1, 0, 0], [1, 0, 0], [0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1]];
/// What they are called in properties and model files.
pub const FACE_NAMES: [&str; 6] = ["west", "east", "down", "up", "north", "south"];

/// A face's corners, in the order its picture goes around them (-X, +X, -Y, +Y, -Z, +Z).
pub const FACE_CORNERS: [[[i32; 3]; 4]; 6] = [
    [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]],
    [[1, 0, 1], [1, 0, 0], [1, 1, 0], [1, 1, 1]],
    [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]],
    [[0, 1, 1], [1, 1, 1], [1, 1, 0], [0, 1, 0]],
    [[1, 0, 0], [0, 0, 0], [0, 1, 0], [1, 1, 0]],
    [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
];

/// One of the 24 ways a block's model turns onto itself (`comp::block::TURN`): where its own east
/// (+X) and its own up (+Y) end up, as `FACE_DIRS` indices. A model is written facing north (-Z),
/// its top up; the engine turns all of it that way: boxes, model files, hitboxes, a cube's faces,
/// liquid walls, support and tinted faces.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Turn {
    pub east: u8,
    pub up: u8,
}

impl Turn {
    pub const NONE: Turn = Turn { east: 1, up: 3 };

    /// The value `comp::block::TURN` holds.
    pub const fn index(self) -> u8 {
        self.east * 6 + self.up
    }

    pub fn from_index(i: u8) -> Option<Turn> {
        let t = Turn { east: i / 6, up: i % 6 };
        (t.east < 6 && t.east / 2 != t.up / 2).then_some(t)
    }

    /// All 24.
    pub fn all() -> impl Iterator<Item = Turn> {
        (0..36).filter_map(Turn::from_index)
    }

    /// Where own +X, +Y and +Z go.
    fn axes(self) -> [[i32; 3]; 3] {
        let (e, u) = (FACE_DIRS[self.east as usize % 6], FACE_DIRS[self.up as usize % 6]);
        [e, u, [e[1] * u[2] - e[2] * u[1], e[2] * u[0] - e[0] * u[2], e[0] * u[1] - e[1] * u[0]]]
    }

    pub fn vector(self, v: [i32; 3]) -> [i32; 3] {
        let a = self.axes();
        core::array::from_fn(|i| v[0] * a[0][i] + v[1] * a[1][i] + v[2] * a[2][i])
    }

    pub fn vector_f(self, v: [f32; 3]) -> [f32; 3] {
        let a = self.axes();
        core::array::from_fn(|i| v[0] * a[0][i] as f32 + v[1] * a[1][i] as f32 + v[2] * a[2][i] as f32)
    }

    /// A point of a block `size` units across (16 for texels, 1 for blocks), turned about its middle.
    pub fn point(self, p: [f32; 3], size: f32) -> [f32; 3] {
        let h = size / 2.0;
        self.vector_f(p.map(|v| v - h)).map(|v| v + h)
    }

    /// The box between two corners, turned.
    pub fn bounds(self, min: [f32; 3], max: [f32; 3], size: f32) -> ([f32; 3], [f32; 3]) {
        let (a, b) = (self.point(min, size), self.point(max, size));
        (core::array::from_fn(|i| a[i].min(b[i])), core::array::from_fn(|i| a[i].max(b[i])))
    }

    /// Which face its own face `f` becomes.
    pub fn face(self, f: usize) -> usize {
        let d = self.vector(FACE_DIRS[f % 6]);
        FACE_DIRS.iter().position(|x| *x == d).unwrap_or(f)
    }

    /// Bits by own face (`1 << face`) as bits by face.
    pub fn faces(self, mask: u8) -> u8 {
        (0..6)
            .filter(|&f| mask & 1 << f != 0)
            .fold(0, |m, f| m | 1 << self.face(f))
    }

    /// Quarter turns the picture of its own face `f` takes, landing on `face(f)`.
    pub fn picture(self, f: usize) -> u8 {
        let c = FACE_CORNERS[f % 6][0];
        let to = self.vector(c.map(|v| 2 * v - 1)).map(|v| (v + 1) / 2);
        FACE_CORNERS[self.face(f)]
            .iter()
            .position(|x| *x == to)
            .unwrap_or(0) as u8
    }

    /// This turn, then `after`.
    pub fn then(self, after: Turn) -> Turn {
        Turn { east: after.face(self.east as usize) as u8, up: after.face(self.up as usize) as u8 }
    }

    pub fn inverse(self) -> Turn {
        Turn::all()
            .find(|t| self.then(*t) == Turn::NONE)
            .unwrap_or(Turn::NONE)
    }
}

impl Default for Turn {
    fn default() -> Turn {
        Turn::NONE
    }
}

/// What a block is to redstone.
pub mod signal_kind {
    pub const NONE: u32 = 0;
    /// Lies on faces; its state is `first_state + mask of them`.
    pub const WIRE: u32 = 1;
    /// Gives its `power` property's level (full without one), whatever comes in.
    pub const SWITCH: u32 = 2;
    /// On unless an input has power; flickering too fast burns it out a while.
    pub const TORCH: u32 = 3;
    /// Full power after `delay` while an input has any, held as it is while a lock has power.
    pub const REPEATER: u32 = 4;
    /// What comes in opposite its output, or a container's fullness there, unless the other
    /// inputs have more; `SUBTRACT` takes theirs from it.
    pub const COMPARATOR: u32 = 5;
    /// A pulse when the block at a face it watches changes.
    pub const OBSERVER: u32 = 6;
    /// `comp::block::SIGNAL_GATE` works out what it gives from its inputs.
    pub const GATE: u32 = 7;
    /// `power` and `comp::block::ON_SIGNAL` follow the power at its inputs.
    pub const SINK: u32 = 8;
    /// Pushes blocks at `front` with power at an input; `STICKY` pulls them back.
    pub const PISTON: u32 = 9;
    pub const PISTON_HEAD: u32 = 10;
    /// With power it takes all it is stuck to and carries it along `front`, faster the more power;
    /// without, that lands. `BREAKS` breaks what is in the way, instead of stopping at it.
    pub const FLOATER: u32 = 11;
}

pub mod signal_flags {
    /// A comparator takes its sides from what is behind it.
    pub const SUBTRACT: u32 = 1;
    /// A piston brings back what is against its head.
    pub const STICKY: u32 = 2;
    /// A floater breaks what is in its way.
    pub const BREAKS: u32 = 4;
}

crate::data! {
    /// What a block state is to redstone (`comp::block::SIGNAL`). A property named `power` holds
    /// its level: two values are off and full. Its faces are bits by face index as its model is
    /// written (front north, bottom down), and turn with it (`comp::block::TURN`).
    pub struct SignalDesc {
        /// `signal_kind`: what it makes of what comes in.
        pub kind: u32,
        /// Where it reads power.
        pub inputs: u32,
        /// Where a repeater or comparator giving into it holds it as it is.
        pub locks: u32,
        /// Where a block changing sets it off.
        pub watches: u32,
        /// Where it gives power, and of those where strongly: into the block there and on.
        pub gives: u32,
        pub strong: u32,
        /// The face it acts at, 6 for none: a piston's head, what a floater carries.
        pub front: u32,
        /// Game ticks.
        pub delay: u32,
        /// `signal_flags`.
        pub flags: u32,
        /// A piston's head block.
        pub head: alloc::string::String,
    }
}
defaults!(SignalDesc {
    kind: signal_kind::NONE,
    inputs: 0,
    locks: 0,
    watches: 0,
    gives: 0,
    strong: 0,
    front: 6,
    delay: 2,
    flags: 0,
    head: alloc::string::String::new(),
});

crate::data! {
    /// A property of a block (`comp::block::PROPS`): `facing`, `lit`, `age`, each with its values.
    pub struct BlockProp { pub name: alloc::string::String, pub values: alloc::vec::Vec<alloc::string::String> }
}

crate::data! {
    /// The texture a block puts where its model file says `#name` (`comp::block::MODEL_TEXTURES`):
    /// one model, each block its own pictures.
    pub struct ModelTexture { pub name: alloc::string::String, pub texture: alloc::string::String }
}

crate::data! {
    /// A template of blocks (`comp::block::SHAPES_OF`): one for each full cube among `sources`
    /// (names and `#tags`, space separated, `!` leaving out), named as that one plus `suffix`.
    /// Texts in its values may say `{source}` and `{source.side}` (its texture there), `a | b` for
    /// the first texture that exists.
    pub struct ShapesOf { pub sources: alloc::string::String, pub suffix: alloc::string::String }
}

crate::data! {
    /// A texture renamed, full names (`comp::block::RETEXTURE`).
    pub struct Retexture { pub from: alloc::string::String, pub to: alloc::string::String }
}

#[repr(C)]
pub struct SignalArgs {
    pub world: *mut WorldCtx,
    pub pos: BlockPos,
    pub state: u32,
    pub level: u32,
    /// What it was before.
    pub before: u32,
}

/// What a comparator behind this block reads, `comp::block::SIGNAL_READ`: set `output`, 0..15.
#[repr(C)]
pub struct SignalReadArgs {
    pub world: *mut WorldCtx,
    pub pos: BlockPos,
    pub state: u32,
    pub output: u32,
}

/// What comes in on each face of a `signal_kind::GATE` (by face index as its model is written,
/// 0 where it doesn't read), and `output` to set: what it gives now.
#[repr(C)]
pub struct GateArgs {
    pub world: *mut WorldCtx,
    pub pos: BlockPos,
    pub state: u32,
    pub inputs: [u32; 6],
    pub output: u32,
}

/// A generation snapshot, borrowed for the duration of a generation callback.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GenSource {
    pub dim: u32,
    pub seed: u64,
    /// Completed stages strictly before this one.
    pub before: u32,
    /// Engine-local writes; null for a standalone snapshot.
    pub ctx: *const core::ffi::c_void,
}

#[repr(C)]
pub struct GenArgs {
    pub source: GenSource,
    /// Optional engine write-intent masks (64 words for blocks, 4 for biomes).
    /// Writers set bits for explicit assignments, including assigning the input value.
    /// Null when the entire buffer is the output, as in a dimension generator.
    pub written: *mut u64,
    pub biomes_written: *mut u64,
    pub seed: u64,
    pub cube: BlockPos,
    /// 4096 state ids, index = (y * 16 + z) * 16 + x. Pre-filled with 0 (air).
    pub blocks: *mut u32,
    /// 256 biome ids per column, index = z * 16 + x.
    pub biomes: *mut u16,
    /// The dimension being generated.
    pub dim: u32,
    /// The dimension's 256 columns of this cube (from its terrain or `comp::dimension::COLUMN`),
    /// index = z * 16 + x. Null for a dimension without either.
    pub columns: *const TerrainColumn,
}

/// What a plan works on (`Host::plan_op`), the engine's.
#[repr(C)]
pub struct PlanCtx {
    _private: [u8; 0],
}

/// A feature's plan (`comp::feature::PLAN`), once per start.
#[repr(C)]
pub struct PlanArgs {
    pub source: GenSource,
    pub seed: u64,
    pub dim: u32,
    /// The start: the block above the ground for a feature on a grid.
    pub pos: BlockPos,
    pub biome: u32,
    /// Random bits of this start, the same every time.
    pub random: u64,
    pub ctx: *mut PlanCtx,
}

/// One thing a plan puts (`Host::plan_op`), in world coordinates.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PlanOp {
    /// `plan_op`.
    pub kind: u32,
    /// `TEMPLATE`: `transform`.
    pub transform: u32,
    pub pos: BlockPos,
    /// `FILL`, `REPLACE`: the opposite corner.
    pub to: BlockPos,
    /// The state put; `TEMPLATE`: its foundation's (0 none, `u32::MAX` the ground's own).
    pub state: u32,
    /// `TEMPLATE`: `piece_flags`.
    pub flags: u32,
    /// `TEMPLATE`: its name; `ENTITY`: the kind; `LOOT`: the table; `REPLACE`: what it replaces
    /// (a block matcher).
    pub name: Str,
    /// `TEMPLATE`: the palette (empty: the plan's, or the template's defaults).
    pub extra: Str,
}

/// `PlanOp::kind`.
pub mod plan_op {
    /// `state` where there is air.
    pub const PUT: u32 = 0;
    /// `state` whatever is there.
    pub const SET: u32 = 1;
    /// A box of `state`.
    pub const FILL: u32 = 2;
    /// What `name` takes in the box becomes `state`.
    pub const REPLACE: u32 = 3;
    /// Template `name`, its lowest corner at `pos` once transformed.
    pub const TEMPLATE: u32 = 4;
    /// An entity of kind `name` at the middle of block `pos`.
    pub const ENTITY: u32 = 5;
    /// The block entity at `pos` filled from loot table `name` when placed.
    pub const LOOT: u32 = 6;
}

/// `PlanOp::flags` of a template.
pub mod piece_flags {
    /// The ground above it is cut away.
    pub const CLEAR_ABOVE: u32 = 1;
}

/// How a template is placed: `TURN` quarters (`& 3`) clockwise seen from above, after a mirror
/// across x (`MIRROR`) if set; 8 in all, 0 as drawn. Blocks turn and mirror with it, a door's hinge
/// and a stair's corner too.
pub mod transform {
    pub const TURN: u32 = 3;
    pub const MIRROR: u32 = 4;
    /// `comp::feature::TRANSFORMS` bits: any quarter turn.
    pub const TURNS: u32 = 0x0F;
    /// Any turn, mirrored or not.
    pub const ALL: u32 = 0xFF;
}

/// Where a feature's turn comes in generation (`comp::feature::STAGE`).
pub mod stage {
    /// Ores, geodes, fossils: in the ground the terrain made.
    pub const UNDERGROUND: u32 = 0;
    /// Structures and other templates on the ground.
    pub const STRUCTURES: u32 = 1;
    /// Trees and other things scattered over the ground.
    pub const VEGETATION: u32 = 2;
    /// Grass, flowers and the rest decided column by column.
    pub const DECORATION: u32 = 3;
    /// Work on the completed terrain, structures and vegetation.
    pub const POST_PROCESS: u32 = 4;
}

/// `comp::feature::SPREAD`.
pub mod spread {
    pub const PER_CUBE: u32 = 0;
    pub const GRID: u32 = 1;
    pub const EVERY_CUBE: u32 = 2;
}

/// `comp::feature::SHAPE`.
pub mod shape {
    pub const VEIN: u32 = 0;
    pub const STEPS: u32 = 1;
    pub const TEMPLATE: u32 = 2;
    pub const PLAN: u32 = 3;
}

/// A joint of a template (`Host::template_joints`), as placed.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Joint {
    /// From the template's lowest corner once transformed.
    pub pos: BlockPos,
    /// The way it faces (`FACE_DIRS` index): a piece joined to it goes there.
    pub face: u32,
    pub name: Str,
    /// What the joint of a piece joined here must be called.
    pub target: Str,
    /// Where that piece comes from (a plan's pool).
    pub pool: Str,
}

#[repr(C)]
pub struct SpawnArgs {
    pub seed: u64,
    /// Output, block position the player stands on top of.
    pub pos: BlockPos,
}

#[repr(C)]
pub struct ColumnArgs {
    pub seed: u64,
    pub x: i32,
    pub z: i32,
    /// Output: the top solid block's height and the biome there.
    pub height: i32,
    pub biome: u32,
}

crate::data! {
    /// A block of a template (`comp::template::BLOCKS`) from its lowest corner: a block or one of
    /// its states (`base:furnace[lit=true]`), `core:air` clearing what is there, or a slot
    /// (`@wall`, `@wall:stairs[facing=north]`, `@ground`: the column's own top block).
    pub struct TemplateBlock { pub pos: [i32; 3], pub block: alloc::string::String }
}

crate::data! {
    /// The block a slot takes (`comp::template::SLOTS`, `comp::palette::SLOTS`).
    pub struct PaletteSlot { pub name: alloc::string::String, pub block: alloc::string::String }
}

crate::data! {
    /// An entity of a template (`comp::template::ENTITIES`): where, from the lowest corner; its
    /// kind; and as it was saved (`data`, the save's bytes, empty for a new one).
    pub struct TemplateEntity { pub pos: [f32; 3], pub kind: alloc::string::String, pub data: alloc::vec::Vec<u8> }
}

crate::data! {
    /// What the block entity at `pos` holds when placed (`comp::template::LOOT`): loot table `table`.
    pub struct TemplateLoot { pub pos: [i32; 3], pub table: alloc::string::String }
}

crate::data! {
    /// The block entity at `pos` as it was saved (`comp::template::DATA`): its inventory and
    /// components, the save's bytes.
    pub struct TemplateData { pub pos: [i32; 3], pub data: alloc::vec::Vec<u8> }
}

crate::data! {
    /// Where pieces of a plan join (`comp::template::JOINTS`, see `Joint`): `face` a `FACE_DIRS`
    /// index.
    pub struct TemplateJoint {
        pub pos: [i32; 3],
        pub face: u32,
        pub name: alloc::string::String,
        pub target: alloc::string::String,
        pub pool: alloc::string::String,
    }
}

// ---------------------------------------------------------------- callback args

#[repr(C)]
pub struct BlockEventArgs {
    pub world: *mut WorldCtx,
    pub pos: BlockPos,
    pub state: u32,
    /// `on_neighbor`: the neighbor that changed. `on_carried`: where the block is on the body.
    pub other: BlockPos,
    pub entity: u64,
}

#[repr(C)]
pub struct UseArgs {
    pub world: *mut WorldCtx,
    pub pos: BlockPos,
    pub state: u32,
    pub face: u32,
    pub player: u64,
    pub handled: u32,
    /// 1: the player sneaks (it got here by the block's `comp::block::SNEAK_USE`).
    pub sneaking: u32,
}

/// Whether a sneaking player uses a block or, past it, what it holds (`comp::block::SNEAK_USE`).
pub mod sneak_use {
    /// Vanilla's: the block with empty hands, else what is held (placing against a chest).
    pub const EMPTY_HANDS: u32 = 0;
    /// The block whatever is held; its `on_use` sees `sneaking` and may pass (`handled = 0`).
    pub const ALWAYS: u32 = 1;
    /// Never the block.
    pub const NEVER: u32 = 2;
}

#[repr(C)]
pub struct PlaceArgs {
    pub pos: BlockPos,
    pub face: u32,
    /// Player look direction.
    pub look: [f32; 3],
    /// In/out: starts as the first state (for `comp::block::MERGE`, the state there now).
    pub state: u32,
    /// The block's first state.
    pub first: u32,
    /// Just outside where it was clicked, from `pos`'s corner: 0..1 inside this cell, past that
    /// when it had no room and goes in the next.
    pub hit: [f32; 3],
    /// For `comp::block::MERGE`: the state its item would be placed in.
    pub placed: u32,
}

#[repr(C)]
pub struct ItemUseArgs {
    pub world: *mut WorldCtx,
    /// Who uses it: a player, or a mob (`World::use_item`).
    pub entity: u64,
    /// In/out: the held stack; change it to consume (eating, placing).
    pub stack: ItemStack,
    /// The slot of its inventory it is in.
    pub slot: u32,
    /// Target block if any (`has_target`).
    pub pos: BlockPos,
    pub face: u32,
    pub has_target: u32,
    pub handled: u32,
    /// Ticks the use button was held (`comp::item::ON_RELEASE`); 0 for a use at once.
    pub held_ticks: u32,
}

/// `comp::USE_TICKS` of an item held until the use button is let go.
pub const USE_HOLD: u32 = u32::MAX;

// ---------------------------------------------------------------- items, entities, menus

/// In a mod: `ITEM.stack(count)`, and `.with(&COMP, v)` for data, never these fields by hand.
/// `data` is 0 for a plain stack, otherwise a handle to its stack components (damage taken,
/// charge, contents ...). Handles are interned: two stacks with equal data have equal handles,
/// so stacks merge exactly when `item` and `data` match. Read and change data through the host.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash)]
pub struct ItemStack {
    pub item: u32,
    pub count: u32,
    pub data: u32,
}
impl Schema for ItemStack {
    const SCHEMA: &'static str = "{item:u32,count:u32,data:u32}";
}

impl ItemStack {
    pub const EMPTY: ItemStack = ItemStack { item: 0, count: 0, data: 0 };

    pub fn new(item: u32, count: u32) -> ItemStack {
        ItemStack { item, count, data: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.item == 0 || self.count == 0
    }

    /// Same item and data, so the two can share a slot.
    pub fn stacks_with(&self, other: &ItemStack) -> bool {
        self.item == other.item && self.data == other.data
    }
}

/// What holds an inventory, energy or other per-instance data: an entity, the block entity at a
/// block, or the stack in one slot of an entity's inventory (a backpack, a battery). The same
/// calls take any of them. Inventory calls reach its own inventory, or with `storage` set one of
/// the storages it carries (`comp::component::SLOTS`: a player's ender chest).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash)]
pub struct Holder {
    /// `holder::*`.
    pub kind: u32,
    /// `HELD`: the slot of `entity`'s inventory.
    pub slot: u32,
    pub entity: u64,
    /// `BLOCK`: the block, in the caller's frame (a body's coordinates on a body).
    pub pos: BlockPos,
    /// Inventory calls: the storage (its component id) instead of the holder's own inventory,
    /// 0 for that.
    pub storage: u32,
}

pub mod holder {
    /// Nothing: menus without an owner (a crafting table's grid).
    pub const NONE: u32 = 0;
    pub const ENTITY: u32 = 1;
    pub const BLOCK: u32 = 2;
    pub const HELD: u32 = 3;
}

impl Holder {
    pub const NONE: Holder = Holder { kind: holder::NONE, slot: 0, entity: 0, pos: BlockPos::ZERO, storage: 0 };

    pub fn entity(id: u64) -> Holder {
        Holder { kind: if id == 0 { holder::NONE } else { holder::ENTITY }, entity: id, ..Holder::NONE }
    }

    pub fn block(pos: BlockPos) -> Holder {
        Holder { kind: holder::BLOCK, pos, ..Holder::NONE }
    }

    /// The stack in `slot` of `entity`'s inventory.
    pub fn held(entity: u64, slot: u32) -> Holder {
        Holder { kind: holder::HELD, slot, entity, ..Holder::NONE }
    }
}

impl From<u64> for Holder {
    fn from(id: u64) -> Holder {
        Holder::entity(id)
    }
}

impl From<[i32; 3]> for Holder {
    fn from(pos: [i32; 3]) -> Holder {
        Holder::block(pos.into())
    }
}

impl From<BlockPos> for Holder {
    fn from(pos: BlockPos) -> Holder {
        Holder::block(pos)
    }
}

/// The movement keys `entity_walk` holds, as a player's are: the same physics and poses answer.
pub mod walk {
    /// Jumps; swims and flies up.
    pub const JUMP: u32 = 1;
    /// Never steps off an edge, holds on to ladders, crouches (the `core:sneak` poses); flies down.
    pub const SNEAK: u32 = 2;
    /// Faster, a longer jump, swims lying down in a liquid it swims in.
    pub const SPRINT: u32 = 4;
    /// Takes off and flies, if its `core:can_fly` attribute is 1 or more; it comes down when it
    /// lands without `JUMP`, as a player does.
    pub const FLY: u32 = 8;
}

/// `EntityState::flags`: what happened to the entity, not what its kind is (its tags,
/// `comp::entity`).
pub mod entity_state {
    /// Its last move ran into a wall.
    pub const BLOCKED: u32 = 1 << 31;
    /// It doesn't despawn however far from players (named, it picked something up, a mod said).
    pub const PERSISTENT: u32 = 1 << 30;
}

/// A box of a model part (`ModelPart`) in model pixels (1/16 block) relative to the part pivot;
/// `uv` is the top-left texel of the classic box unwrap, or with `per_face` set each face takes
/// its own rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelCuboid {
    pub origin: [f32; 3],
    pub extent: [f32; 3],
    pub uv: [f32; 2],
    pub inflate: f32,
    /// Box unwrap mirrored left to right (Bedrock/Java `mirror`).
    pub mirror: u32,
    /// Texel rectangles (u, v, width, height; negative sizes flip) of the faces in -X, +X, -Y,
    /// +Y, -Z, +Z order, used when `per_face` is non-zero.
    pub faces: [[f32; 4]; 6],
    pub per_face: u32,
}
defaults!(ModelCuboid { origin: [0.0; 3], extent: [1.0; 3], uv: [0.0; 2], inflate: 0.0, mirror: 0, faces: [[0.0; 4]; 6], per_face: 0 });
impl Schema for ModelCuboid {
    const SCHEMA: &'static str = "{origin:[f32;3],extent:[f32;3],uv:[f32;2],inflate:f32,mirror:u32,faces:[[f32;4];6],per_face:u32}";
}

/// A corner of a model's free-form triangles (spheres, curved shapes): model pixels like the
/// pivots, texels.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeshVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
}
impl Schema for MeshVertex {
    const SCHEMA: &'static str = "{pos:[f32;3],uv:[f32;2]}";
}

crate::data! {
    /// A bone of an entity kind's model in code (`comp::entity::PARTS`).
    pub struct ModelPart {
        /// What keyframe animations address, and what decides how it moves by itself (`head`
        /// looks, `rightArm`/`leftArm`/`rightLeg`/`leftLeg` swing, `body` leans when sneaking,
        /// `leg0`..`leg3` of a quadruped, `leg0`..`leg7` of a spider sweep).
        pub name: alloc::string::String,
        /// Index of the part this one hangs from (it moves with it), -1 for none.
        pub parent: i32,
        /// Pivot in model pixels, y up from the feet.
        pub pivot: [f32; 3],
        /// Resting rotation in radians about x, y, z, applied x first; walking and looking add to it.
        pub rest: [f32; 3],
        pub cuboids: alloc::vec::Vec<ModelCuboid>,
        /// Triangles, three corners each, besides the boxes.
        pub mesh: alloc::vec::Vec<MeshVertex>,
    }
}

crate::data! {
    /// An entity kind's box standing (`comp::entity::SIZE`), in blocks.
    pub struct BodySize { pub width: f32, pub height: f32 }
}

crate::data! {
    /// A model file on a bone of every entity of a kind (`comp::entity::BONE_MODELS`), drawn in its
    /// own pixels with the same origin as the kind's; what plays on the entity plays on it too.
    pub struct BoneModel { pub model: alloc::string::String, pub bone: alloc::string::String }
}

crate::data! {
    /// A component every entity of a kind carries (`comp::entity::ATTACH`, any mod's or the
    /// engine's), starting as `start` (the component's size in bytes), empty: its default.
    pub struct Attach { pub name: alloc::string::String, pub start: alloc::vec::Vec<u8> }
}

crate::data! {
    /// A place on a mount (`comp::entity::SEATS`, `Host::entity_mount`).
    pub struct Seat {
        /// Where a rider's feet go, in blocks from the mount's position, turned with it: +Z ahead.
        pub pos: Vec3,
        /// Who sits here steers it (a player with their keys and the kind's `STEER`, a mob with
        /// its AI); nobody steering, its own AI moves it.
        pub steers: bool,
        /// `seat_exit`: where a rider of this seat gets off.
        pub exit: u32,
        /// `seat_exit::OFFSET`: from the mount, turned with it. `NEAREST`: x is how far to look.
        pub exit_at: Vec3,
    }
}

/// Where a rider gets off (`Seat::exit`).
pub mod seat_exit {
    /// The nearest place it fits beside the mount, within `exit_at.x` blocks; none: where it sat.
    pub const NEAREST: u32 = 0;
    /// At `exit_at` from the mount, turned with it (a gondola's door).
    pub const OFFSET: u32 = 1;
    /// Right where it sat, in the air if that is where it was.
    pub const STAY: u32 = 2;
    /// The kind's `comp::entity::DISMOUNT` decides.
    pub const CALL: u32 = 3;
}

/// What the keys say of one action (`Host::keys_state`, `client_key_state`). Ticks are the
/// client's game ticks, so a charge reads the same however fast frames come.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyState {
    /// Set off this frame.
    pub pressed: u32,
    pub held: u32,
    /// Ticks it has been held, 0 when it isn't.
    pub held_ticks: u32,
    /// Let go this tick after being held that many ticks (a charge), 0 otherwise.
    pub released: u32,
}

/// The keys as a mount's steering or a camera mode reads them (`Host::keys_state`).
#[repr(C)]
pub struct KeysCtx {
    _private: [u8; 0],
}

/// What a mount does this tick (`SteerArgs::out`), starting from nothing.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Steer {
    /// Where it goes, ahead and to the right of its own facing, length up to 1.
    pub forward: f32,
    pub strafe: f32,
    /// Times its kind's walking speed.
    pub speed: f32,
    /// Its new facing, radians; `f32::NAN` keeps it.
    pub yaw: f32,
    /// Blocks per second up when it is on the ground this tick (a jump), 0 for none.
    pub jump: f32,
    /// Non-zero: no gravity this tick, `rise` blocks per second up (negative down).
    pub flies: u32,
    pub rise: f32,
}
defaults!(Steer { forward: 0.0, strafe: 0.0, speed: 1.0, yaw: f32::NAN, jump: 0.0, flies: 0, rise: 0.0 });

#[repr(C)]
pub struct SteerArgs {
    /// The keys of the player steering, and where it looks (radians).
    pub keys: *const KeysCtx,
    pub yaw: f32,
    pub pitch: f32,
    /// The mount as it is.
    pub mount: EntityState,
    pub out: Steer,
}

#[repr(C)]
pub struct DismountArgs {
    pub world: *mut WorldCtx,
    pub mount: u64,
    pub rider: u64,
    pub seat: u32,
    /// Where it gets off; the engine's choice (`seat_exit::NEAREST`) until changed.
    pub out: Vec3,
}

/// Who gets on or off what (`action::MOUNT`, `action::DISMOUNT`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Mounting {
    pub rider: u64,
    pub mount: u64,
    pub seat: u32,
    pub pad: u32,
}
impl Schema for Mounting {
    const SCHEMA: &'static str = "{rider:u64,mount:u64,seat:u32,pad:u32}";
}

/// What a camera mode (`entry_kind::CAMERA`, `comp::camera`) is and does: each frame on the
/// client its `call` (`CameraArgs`) says where the camera is, and these what goes with it. The
/// engine's are `core:first_person`, `core:behind`, `core:front` (F5 goes through those with
/// `CYCLE`) and `core:free` (`core:freecam`, P).

/// How cubes kept loaded with no player near go on (`Host::force_load`).
pub mod force_load {
    /// Let go.
    pub const OFF: u32 = 0;
    /// Block entities, systems, scheduled ticks and redstone; mobs stand still and
    /// nothing grows (vanilla's lazy chunks).
    pub const BLOCK_ENTITIES: u32 = 1;
    /// All of it, as near a player.
    pub const ALL: u32 = 2;
}

/// How a move goes from start to end (`comp::camera::EASING`, `Host::player_camera`).
pub mod easing {
    pub const LINEAR: u32 = 0;
    /// Slow at first.
    pub const IN: u32 = 1;
    /// Slow at the end.
    pub const OUT: u32 = 2;
    /// Slow at both.
    pub const IN_OUT: u32 = 3;
    /// Smoothstep: gentler than `IN_OUT`.
    pub const SMOOTH: u32 = 4;
    /// Smootherstep: no jolt at either end.
    pub const SMOOTHER: u32 = 5;
    pub const SINE: u32 = 6;
    /// Fast, then a long settle.
    pub const EXPO_OUT: u32 = 7;
    /// Goes a little past the end and comes back.
    pub const BACK_OUT: u32 = 8;
    /// Springs about the end before it settles.
    pub const ELASTIC_OUT: u32 = 9;
    /// Lands and bounces.
    pub const BOUNCE_OUT: u32 = 10;
}

/// Where a camera is and looks.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CameraPose {
    pub pos: Vec3,
    /// Radians, as the player's.
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    /// Times the field of view the player set.
    pub fov: f32,
}

defaults!(CameraPose { pos: Vec3 { x: 0.0, y: 0.0, z: 0.0 }, yaw: 0.0, pitch: 0.0, roll: 0.0, fov: 1.0 });

/// A camera mode's frame (`comp::camera::CALL`), on the client.
#[repr(C)]
pub struct CameraArgs {
    /// The keys, and the look: the camera's own, turned by the mouse, when it's
    /// `comp::camera::DETACHED`, the player's otherwise. A detached one reads the keys it took.
    pub keys: *const KeysCtx,
    pub yaw: f32,
    pub pitch: f32,
    /// The player's eyes and where they look.
    pub eye: Vec3,
    pub eye_yaw: f32,
    pub eye_pitch: f32,
    /// What it rides, 0 for nothing, and how far back third person sits for that: 4 blocks
    /// and the mount's `comp::entity::RIDE_CAMERA`.
    pub vehicle: u64,
    pub back: f32,
    /// Seconds since the last frame, and since this mode came on.
    pub dt: f32,
    pub time: f32,
    /// 1 the first frame of the mode: `out` is the camera before it then.
    pub first: u32,
    /// Where a `comp::camera::CLIP` camera is pulled in towards; the eyes unless changed.
    pub anchor: Vec3,
    /// Comes in as last frame's, to be changed.
    pub out: CameraPose,
}

/// Eye height as a share of an entity's height unless it says otherwise (vanilla's default).
pub const EYE_SHARE: f32 = 0.85;

#[repr(C)]
pub struct EntityTickArgs {
    pub world: *mut WorldCtx,
    pub kind: u32,
    pub ids: *const u64,
    pub count: usize,
}

#[repr(C)]
pub struct EntityEventArgs {
    pub world: *mut WorldCtx,
    pub entity: u64,
    /// The player interacting.
    pub other: u64,
    pub handled: u32,
}

/// The engine's damage causes, `DAMAGE_CAUSE` entries registered first and in this order, so
/// their ids are these; mods' own come after. What a cause does is its tags
/// (`comp::damage_cause`): one with `BYPASSES_ARMOR` isn't reduced by armor (falling, starving,
/// drowning, the void).
pub mod damage_cause {
    pub const GENERIC: u32 = 0;
    pub const FALL: u32 = 1;
    pub const ATTACK: u32 = 2;
    pub const FIRE: u32 = 3;
    pub const STARVE: u32 = 4;
    pub const DROWN: u32 = 5;
    /// Falling out of the world, and `/clear`: gets through everything.
    pub const VOID: u32 = 6;
    /// Inside a burning hot liquid.
    pub const LAVA: u32 = 7;
    /// Squeezed against blocks by a crushing body.
    pub const CRUSH: u32 = 8;
    /// Its head stuck in a block.
    pub const SUFFOCATE: u32 = 9;
    /// Caught in a blast.
    pub const EXPLOSION: u32 = 10;
    /// What `World::heal` gives when nothing more specific healed.
    pub const HEAL: u32 = 11;
    /// Names, by id.
    /// What a projectile does (`comp::entity::PROJECTILE`).
    pub const PROJECTILE: u32 = 12;
    /// Too many pushing at once (`core:max_entity_cramming`).
    pub const CRAMMING: u32 = 13;
    pub const NAMES: [&str; 14] = [
        "core:generic",
        "core:fall",
        "core:attack",
        "core:fire",
        "core:starve",
        "core:drown",
        "core:void",
        "core:lava",
        "core:crush",
        "core:suffocate",
        "core:explosion",
        "core:heal",
        "core:projectile",
        "core:cramming",
    ];
}

/// What a ray looks for (`Host::raycast`).
pub mod ray {
    pub const BLOCKS: u32 = 1;
    /// Liquids stop it too.
    pub const LIQUIDS: u32 = 2;
    /// What can be hit: bodies, not dropped stacks, the dead or spectators.
    pub const ENTITIES: u32 = 4;
    /// Blocks by what stops a body (their collision), not by what is aimed at: what an arrow hits,
    /// flying through grass.
    pub const SOLID: u32 = 8;
}

/// What a ray hit (`Host::raycast`): an entity, or a block (`entity` 0). A block is at `pos`,
/// its own cell for a big one, on a body (`body` its entity) in that body's blocks; `state` is the
/// block of the cell it entered, the cell's main one or a part. `face` is the face it went in
/// through, of the block's or the entity's box (-X, +X, -Y, +Y, -Z, +Z). `point` is where, in the
/// caller's frame; `local` where on what it hit, from the block's or the box's low corner (a
/// block's 0..1, times 16 its pixel). An entity's box turns with its body (`EntityState::body_yaw`):
/// its face and `local` are in that turned box, -Z its front and +Z its back, `local.y` how high
/// up from its feet.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RayHit {
    pub entity: u64,
    pub body: u64,
    pub pos: BlockPos,
    pub state: u32,
    pub face: u32,
    /// The part of the entity's hitbox it hit (`hitbox_part`): `face` and `local` are that part's.
    pub part: u32,
    pub point: Vec3,
    pub local: Vec3,
    pub distance: f64,
}

/// What the client aims at: `entity` is nonzero for a mob, `state` for a block. Block positions
/// are in `body`'s frame (0 for the world), so moving platforms keep their own coordinates.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Target {
    pub entity: u64,
    pub body: u64,
    pub pos: BlockPos,
    pub state: u32,
}

crate::data! {
    /// Directional protection while using an item. Arc is degrees; wear applies only above
    /// the threshold, as base + floor(damage * factor).
    pub struct Blocking {
        pub delay_ticks: u32,
        pub arc: f32,
        pub wear_threshold: f32,
        pub wear_base: u32,
        pub wear_factor: f32,
    }
}

crate::data! {
    /// A projectile model retained in an item. Position and direction are in model pixels.
    pub struct EmbeddedProjectile {
        pub kind: crate::data::String,
        pub bone: crate::data::String,
        pub position: [f32; 3],
        pub direction: [f32; 3],
    }
}

crate::data! {
    /// How a projectile flies and what it does where it hits: `damage` per block a tick of its
    /// speed (rounded up, vanilla's arrow 2), the share of its speed kept each tick in the air and
    /// in a liquid (0.99, 0.6), and whether it stays stuck in a block (`stuck_ticks` before it goes,
    /// vanilla's arrow 1200) or goes at once.
    pub struct Projectile {
        pub damage: f32,
        pub drag: f32,
        pub liquid_drag: f32,
        pub sticks: bool,
        pub stuck_ticks: u32,
    }
}

crate::data! {
    /// One part of an entity's hitbox: a box in model pixels of the kind (its feet at 0, -Z its
    /// front) turning about `pivot` as `follows` says (`hitbox_follows`); a `STRIDE` part swings
    /// `swing` radians at a full stride, `phase` (radians) after the stride's own beat.
    pub struct HitboxPart {
        pub name: crate::data::String,
        pub min: [f32; 3],
        pub max: [f32; 3],
        pub pivot: [f32; 3],
        pub follows: u32,
        pub swing: f32,
        pub phase: f32,
    }
}

crate::data! {
    /// What hits land on (`comp::entity::HITBOXES`), part by part, in pixels (1/16 block) at the
    /// kind's own size: one made bigger or smaller (`set_size`) has them so too, a crouch squeezes
    /// them down, a lying pose lays them down, and they turn with its body. Rays, arrows and blows
    /// find the part they hit (`RayHit::part`, `Damage::part`).
    pub struct Hitboxes {
        pub parts: crate::data::Vec<HitboxPart>,
    }
}

/// How a part of a hitbox moves (`HitboxPart::follows`).
pub mod hitbox_follows {
    /// With the body only.
    pub const BODY: u32 = 0;
    /// With the head: the look's yaw past the body's, and its pitch.
    pub const LOOK: u32 = 1;
    /// Swinging forward and back as it walks, a leg or an arm.
    pub const STRIDE: u32 = 2;
}

/// A hitbox part's id from its name, as hits tell it (`RayHit::part`, `Damage::part`).
pub const fn hitbox_part_id(name: &str) -> u32 {
    let h = fnv(name.as_bytes());
    (h ^ (h >> 32)) as u32 | 1
}

/// The parts the engine's shapes (the SDK's `Hitboxes::humanoid` ...) name.
pub mod hitbox_part {
    use super::hitbox_part_id as id;
    pub const HEAD: u32 = id("head");
    pub const BODY: u32 = id("body");
    pub const RIGHT_ARM: u32 = id("right_arm");
    pub const LEFT_ARM: u32 = id("left_arm");
    pub const RIGHT_LEG: u32 = id("right_leg");
    pub const LEFT_LEG: u32 = id("left_leg");
    /// A four-legged one's: front right, front left, back right, back left.
    pub const FRONT_RIGHT_LEG: u32 = id("front_right_leg");
    pub const FRONT_LEFT_LEG: u32 = id("front_left_leg");
    pub const BACK_RIGHT_LEG: u32 = id("back_right_leg");
    pub const BACK_LEFT_LEG: u32 = id("back_left_leg");
    /// A spider's legs, each side as one, and a bird's wings.
    pub const RIGHT_LEGS: u32 = id("right_legs");
    pub const LEFT_LEGS: u32 = id("left_legs");
    pub const RIGHT_WING: u32 = id("right_wing");
    pub const LEFT_WING: u32 = id("left_wing");
}

/// A projectile's hit (`action::PROJECTILE_HIT`): `target` 0 for a block at `pos`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectileHit {
    pub projectile: u64,
    pub target: u64,
    pub owner: u64,
    pub pos: BlockPos,
    pub face: u32,
    /// The part of `target`'s hitbox (`hitbox_part`).
    pub part: u32,
    pub point: Vec3,
}
impl Schema for ProjectileHit {
    const SCHEMA: &'static str = "{projectile:u64,target:u64,owner:u64,pos:[i32;3],face:u32,part:u32,point:[f64;3]}";
}

/// What `Host::entity_spawn` makes. What `set` leaves out takes the kind's defaults: a random
/// yaw, and for items a small random toss and half a second before anyone picks it up.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SpawnEntityDesc {
    pub kind: u32,
    pub pos: Vec3,
    /// `spawn_set`: which of the fields below are given.
    pub set: u32,
    /// What an item entity is.
    pub stack: ItemStack,
    /// Blocks per second.
    pub velocity: Vec3,
    pub yaw: f32,
    /// Ticks before an item can be picked up.
    pub pickup_delay: u32,
    /// `spawn_cause`, what `action::SPAWN` is told.
    pub cause: u32,
    /// Who it belongs to: what a projectile doesn't hit while it leaves them, and whose its hits
    /// are.
    pub owner: u64,
}

/// Why an entity came into the world (`action::SPAWN`).
pub mod spawn_cause {
    /// A mod's `World::spawn`.
    pub const MOD: u32 = 0;
    /// Natural spawning (`entry_kind::SPAWN` rules).
    pub const NATURAL: u32 = 1;
    /// `/summon`.
    pub const COMMAND: u32 = 2;
    /// A spawn egg.
    pub const EGG: u32 = 3;
    /// A dropped stack: thrown, fallen out of something, a block's or a mob's loot.
    pub const DROP: u32 = 4;
}

/// Why an entity left the world (`event::ENTITY_REMOVED`).
pub mod removal {
    /// A mod took it away (`World::despawn`).
    pub const MOD: u32 = 0;
    pub const DIED: u32 = 1;
    /// Too far from every player (its category's `despawn`).
    pub const DESPAWNED: u32 = 2;
    /// Its cube was unloaded; it comes back with it.
    pub const UNLOADED: u32 = 3;
    /// A dropped stack burned up, ran out of time, was picked up or merged into another.
    pub const BURNED: u32 = 4;
    pub const EXPIRED: u32 = 5;
    pub const PICKED_UP: u32 = 6;
    pub const MERGED: u32 = 7;
    /// A player left the game.
    pub const LEFT: u32 = 8;
    /// A projectile that hit something and went.
    pub const HIT: u32 = 9;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spawned {
    pub entity: u64,
    pub kind: u32,
    /// `spawn_cause`.
    pub cause: u32,
    pub pos: Vec3,
}
impl Schema for Spawned {
    const SCHEMA: &'static str = "{entity:u64,kind:u32,cause:u32,pos:[f64;3]}";
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Removed {
    pub entity: u64,
    pub kind: u32,
    /// `removal`.
    pub reason: u32,
    pub pos: Vec3,
}
impl Schema for Removed {
    const SCHEMA: &'static str = "{entity:u64,kind:u32,reason:u32,pos:[f64;3]}";
}

/// An effect given or ended (`action::EFFECT`, `event::EFFECT_ENDED`); `ticks` 0 when it ended.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectChange {
    pub entity: u64,
    pub effect: u32,
    pub level: u32,
    pub ticks: u32,
    pub pad: u32,
}
impl Schema for EffectChange {
    const SCHEMA: &'static str = "{entity:u64,effect:u32,level:u32,ticks:u32,pad:u32}";
}

/// A player's action pressed (`down` 1) or let go after `held_ticks` (`event::KEY`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyChange {
    pub player: u64,
    /// The key's id (`entry_kind::KEY`).
    pub key: u32,
    pub down: u32,
    pub held_ticks: u32,
    pub pad: u32,
}
impl Schema for KeyChange {
    const SCHEMA: &'static str = "{player:u64,key:u32,down:u32,held_ticks:u32,pad:u32}";
}

/// An effect's tick (`comp::ENTITY_EFFECT_ON_TICK`): every entity with it, and its level there.
#[repr(C)]
pub struct EffectTickArgs {
    pub world: *mut WorldCtx,
    pub effect: u32,
    pub ids: *const u64,
    pub levels: *const u32,
    pub count: usize,
}

/// Which fields of `EntityState` `Host::entity_set` writes.
pub mod entity_set {
    /// Players: a teleport within the dimension.
    pub const POS: u32 = 1;
    pub const VELOCITY: u32 = 2;
    pub const YAW: u32 = 4;
    pub const HEALTH: u32 = 8;
    /// 0: the next landing hurts as if it started there.
    pub const FALL_DISTANCE: u32 = 16;
    /// Players: flying, whether the game mode lets them or not (it stops on landing then).
    pub const FLYING: u32 = 32;
    pub const FIRE: u32 = 64;
    /// The slot in its main hand: a player's hotbar, a mob's own slots (`comp::entity::WEARS`).
    pub const SELECTED: u32 = 128;
    pub const PITCH: u32 = 256;
    /// `entity_state::PERSISTENT` of `flags`.
    pub const PERSISTENT: u32 = 512;
}

pub mod spawn_set {
    pub const VELOCITY: u32 = 1;
    pub const YAW: u32 = 2;
    pub const PICKUP_DELAY: u32 = 4;
    pub const INHERIT_VELOCITY: u32 = 8;
}

/// An effect on an entity (`Host::entity_effects`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EffectOn {
    pub effect: u32,
    /// 1 and up.
    pub level: u32,
    /// `INFINITE_TICKS` for one that doesn't end.
    pub ticks_left: u32,
}

/// Ticks of an entity effect that never ends (`Host::entity_effect`, `EffectGift`), vanilla's
/// `infinite`.
pub const INFINITE_TICKS: u32 = u32::MAX;

/// The engine's entity effects, registered first in this order.
pub mod entity_effect {
    /// White, a flash (something about to go off).
    pub const FLASH: u32 = 0;
    /// Red, what taking damage shows by itself.
    pub const HURT: u32 = 1;
    /// Lit up whatever the light around.
    pub const GLOW: u32 = 2;
    /// Not drawn: the light every burning entity gives.
    pub const BURNING: u32 = 3;
    pub const NAMES: [&str; 4] = ["core:flash", "core:hurt", "core:glow", "core:burning"];
}

/// What a damage cause does besides the damage itself.

/// What `Host::entities_near` looks for.
/// What `Host::entities_in` looks for.
pub mod entity_filter {
    /// Only what can be hit: no dropped items, block entities, spectators or the dead.
    pub const TARGETS: u32 = 1;
    /// Any part of its box in the shape, not just its feet.
    pub const TOUCHING: u32 = 2;
}

/// Where a group of slots is: its first, how many and how many a row.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SlotGroup {
    pub start: u32,
    pub len: u32,
    pub cols: u32,
}

/// The engine's game modes, `GAME_MODE` entries registered first in this order.
pub mod game_mode {
    pub const SURVIVAL: u32 = 0;
    pub const CREATIVE: u32 = 1;
    pub const SPECTATOR: u32 = 2;
}

crate::data! {
    /// An entity effect an item gives when used (`comp::GIVES_EFFECTS`): its name, its level (1
    /// and up) and for how many ticks (`INFINITE_TICKS`: for good).
    pub struct EffectGift {
        pub effect: alloc::string::String,
        pub level: u32,
        pub ticks: u32,
    }
}

crate::data! {
    /// An attribute modifier something gives (`comp::MODIFIERS`): a game mode its players, an item
    /// while held in the main hand or (`worn`) in an equipment slot, a pose while in it, an entity
    /// kind while all of `when` hold for it (`condition`; checked each tick). It runs `step`
    /// (`step::ADD`, `MUL`, `SET`, `PERCENT` or a mod's) with `amount`, in `phase` (empty: the
    /// step's own).
    pub struct Modifier {
        pub attribute: alloc::string::String,
        pub step: alloc::string::String,
        pub phase: alloc::string::String,
        pub amount: f64,
        pub worn: bool,
        pub when: alloc::vec::Vec<alloc::string::String>,
        /// An entity effect's: only at this level of it; 0, once for every level it has.
        pub level: u32,
    }
}

/// The engine's weathers, registered first in this order. Clear comes
/// between all the others.
pub mod weather {
    pub const CLEAR: u32 = 0;
    pub const RAIN: u32 = 1;
    pub const THUNDER: u32 = 2;
}

/// How a weather looks, each 0..1; the client goes over to a new one's in five seconds.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeatherLook {
    /// How much of the precipitation falls.
    pub falls: f32,
    /// How far the sky's color goes to gray (rain 0.75).
    pub sky_gray: f32,
    /// How much of the sun, moon and stars it hides (rain 1).
    pub sky_cover: f32,
    pub fog_gray: f32,
    pub cloud_gray: f32,
    /// How far the clouds go dark (thunder 0.95).
    pub cloud_dark: f32,
    /// Share of daylight it takes away (rain 0.3125).
    pub darkness: f32,
}

/// The engine's poses (`entry_kind::POSE`), registered first in this order; `EntityState::pose`
/// holds one of them or a mod's.
pub mod pose {
    pub const STAND: u32 = 0;
    pub const CROUCH: u32 = 1;
    pub const SWIM: u32 = 2;
    pub const CRAWL: u32 = 3;
    pub const SLEEP: u32 = 4;
}

/// How a pose lays the model down.
pub mod pose_tilt {
    pub const UPRIGHT: u32 = 0;
    /// Along where it looks (swimming).
    pub const ALONG_LOOK: u32 = 1;
    /// Flat on its front (crawling, gliding).
    pub const FACE_DOWN: u32 = 2;
    /// Flat on its back (sleeping).
    pub const ON_BACK: u32 = 3;
}

/// An entity's box in blocks: as wide as `width` both ways, `height` tall, its eyes `eye` up.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EntitySize {
    pub width: f32,
    pub height: f32,
    pub eye: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EntityState {
    pub kind: u32,
    pub flags: u32,
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: u32,
    pub in_liquid: u32,
    /// Sprinting (players), as its client says.
    pub sprinting: u32,
    /// Left the ground by jumping this tick (players).
    pub jumped: u32,
    /// A pose id: `pose::*` or a registered one.
    pub pose: u32,
    pub flying: u32,
    pub health: f32,
    pub max_health: f32,
    pub age: u64,
    /// Blocks fallen since last on ground: what a landing hurts by.
    pub fall_distance: f32,
    /// Ticks it burns for yet.
    pub fire: u32,
    /// The slot in its main hand: a player's hotbar, a mob's own slots (`comp::entity::WEARS`).
    pub selected: u32,
    /// Distance walked this tick (hunger exhaustion); read on a client, the limb swing (0
    /// standing, 1 running) in the frame of what it stands on.
    pub moved: f32,
    /// Its box now (crouching and `set_size` included).
    pub size: EntitySize,
    /// Where its body faces, radians like `yaw`: it lags behind the head, turning to where it
    /// walks; what hits land on turns with it.
    pub body_yaw: f32,
    /// The item it is using (eating, a bow drawn), 0 for none, and for how many ticks so far.
    pub using: u32,
    pub using_ticks: u32,
}

impl EntityState {
    /// Where its eyes are.
    pub fn eye(&self) -> Vec3 {
        self.pos.offset(0.0, self.size.eye as f64, 0.0)
    }

    pub fn crouching(&self) -> bool {
        self.pose == pose::CROUCH
    }

    /// The block its feet are in.
    pub fn block_pos(&self) -> BlockPos {
        self.pos.block()
    }
}
defaults!(EntityState {
    kind: 0,
    flags: 0,
    pos: Vec3::ZERO,
    vel: Vec3::ZERO,
    yaw: 0.0,
    pitch: 0.0,
    on_ground: 0,
    in_liquid: 0,
    sprinting: 0,
    jumped: 0,
    pose: 0,
    flying: 0,
    health: 0.0,
    max_health: 0.0,
    age: 0,
    fall_distance: 0.0,
    fire: 0,
    selected: 0,
    moved: 0.0,
    size: EntitySize { width: 0.0, height: 0.0, eye: 0.0 },
    body_yaw: 0.0,
    using: 0,
    using_ticks: 0,
});

impl Default for EntityState {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// One step of a shape, the steps of a whole shape in postfix order: `UNION`, `INTERSECT` and
/// `SUBTRACT` take the two shapes just before them. At most 16 results wait at once.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeStep {
    /// `shape_kind`.
    pub kind: u32,
    /// `CYLINDER`: the face direction (`FACE_DIRS` index) it goes along from its base.
    pub axis: u32,
    /// `NOISY`: the seed of its bumps.
    pub seed: u64,
    /// Numbers, by kind (see `shape_kind`).
    pub v: [f64; 8],
}
defaults!(ShapeStep { kind: 0, axis: 3, seed: 0, v: [0.0; 8] });
impl Schema for ShapeStep {
    const SCHEMA: &'static str = "{kind:u32,axis:u32,seed:u64,v:[f64;8]}";
}

pub mod shape_kind {
    /// Center x y z, radius.
    pub const BALL: u32 = 0;
    /// Min x y z, max x y z.
    pub const BOX: u32 = 1;
    /// Base x y z, radius, height (along `axis`).
    pub const CYLINDER: u32 = 2;
    /// Center x y z, radii x y z.
    pub const ELLIPSOID: u32 = 3;
    /// A ball with a bumpy edge: center x y z, radius, how far the radius wanders (0..1 of it),
    /// how many bumps across (a vanilla-like blast: 0.25, 1.5).
    pub const NOISY: u32 = 4;
    pub const UNION: u32 = 5;
    pub const INTERSECT: u32 = 6;
    pub const SUBTRACT: u32 = 7;
}

/// A shape filled with one block (`Host::fill`): a crater with air.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FillDesc {
    /// Blocks whose middle is inside change.
    pub shape: Slice<ShapeStep>,
    pub state: u32,
    /// A block matcher: blocks it takes stay. Empty: none.
    pub keep: Str,
    /// A block matcher: when not empty, only blocks it takes change.
    pub only: Str,
}
defaults!(FillDesc { shape: Slice::EMPTY, state: 0, keep: Str::EMPTY, only: Str::EMPTY });

pub mod component_flags {
    pub const SAVE: u32 = 1;
    /// Sent to the player that owns it (HUD values like hunger).
    pub const SYNC_OWNER: u32 = 2;
    /// Sent to every player that sees the entity, when it changes.
    pub const SYNC_WATCHERS: u32 = 4;
    /// Sent to players with a menu open on the entity (a furnace's progress), when it changes; read
    /// on the client with `ClientView::menu_component`.
    pub const SYNC_MENU: u32 = 8;
    /// Kept when the player respawns (a bed, advancements, statistics); the others start over
    /// from their defaults.
    pub const KEEP_ON_RESPAWN: u32 = 16;
    /// Goes from a block entity onto its block's item when the block breaks, and back onto the
    /// block entity when that item is placed (a battery's charge, a renamed chest's name).
    pub const CARRIED: u32 = 32;
    /// An inventory (`comp::component::SLOTS`), its value one `ItemStack` per slot.
    pub const STORAGE: u32 = 64;
}

crate::data! {
    /// A key of a shaped recipe's pattern: the character, and an item name or `#tag`.
    pub struct RecipeKey { pub name: alloc::string::String, pub item: alloc::string::String }
}

crate::data! {
    /// What a run of a recipe does to one of its ingredients (`comp::recipe::USES`), other than
    /// using it up: `key` its pattern character, or for a recipe without a pattern its place among
    /// the inputs (`"0"`); `how` a `recipe_use`, `amount` the wear a run gives, `item` what is
    /// left in its place.
    pub struct RecipeUse {
        pub key: alloc::string::String,
        pub how: u32,
        pub amount: u32,
        pub item: alloc::string::String,
    }
}

/// `RecipeUse::how`. Without one an ingredient is used up, the last of a stack leaving its item's
/// `comp::REMAINDER` (a lava bucket its bucket).
pub mod recipe_use {
    /// It stays as it is: a mold, a catalyst.
    pub const KEEP: u32 = 1;
    /// It stays and wears by `amount` a run, breaking when its durability is gone: a hammer.
    pub const WEAR: u32 = 2;
    /// Used up, `item` taking its place (any item, also one with no `remainder`).
    pub const LEAVES: u32 = 3;
}

/// A recipe the viewer shows with a kind's own `display`.
#[repr(C)]
pub struct RecipeUiArgs {
    pub ui: *mut UiCtx,
    pub client: *mut ClientCtx,
    pub kind: Str,
    /// The recipe's number in its kind, for `Host::recipe_info` and friends.
    pub recipe: u32,
}

/// What `Host::recipe_info` tells about one recipe.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RecipeInfo {
    pub time: u32,
    pub inputs: u32,
    pub outputs: u32,
    /// Its entry (`entry_kind::RECIPE`), whose component table row holds what else it carries.
    pub entry: u32,
}

pub mod slot_flags {
    /// Can only be taken from (crafting result).
    pub const OUTPUT: u32 = 1;
    /// Can only be put into, nothing comes back out by hand.
    pub const INPUT: u32 = 2;
    /// The player's hotbar (a group of `comp::entity::SLOT_GROUPS`): selected with the number keys,
    /// shown at the bottom of the screen.
    pub const HOTBAR: u32 = 4;
    /// The other hand: takes any item that can be held in one hand.
    pub const OFF_HAND: u32 = 8;
    /// Nothing goes in or out: the slot holding the stack a menu is open on.
    pub const LOCKED: u32 = 16;
    /// Holds a liquid (its `fluid` item, the count in millibuckets, `limit` the capacity):
    /// filled and emptied with containers like a bucket, never picked up.
    pub const LIQUID: u32 = 32;
    /// The player's trash, one slot (Terraria's): it keeps the last thing put in until the next
    /// takes its place, or the player leaves; never filled by a shift-click. `core:trash`
    /// (Delete) over a slot sends that stack there.
    pub const TRASH: u32 = 64;
    /// A crafting grid of its menu's first recipe kind (`comp::menu::RECIPES`): the engine shows
    /// what it makes in the menu's output group, and taking that uses the grid up as the recipe
    /// says (`comp::recipe::USES`), with shift as many runs as fit in the player's inventory.
    pub const GRID: u32 = 128;
}

/// The equipment slot of the second hand. Without it, players have one.
pub const OFF_HAND_SLOT: &str = "core:off_hand";

/// Whose inventory a slot group shows.
pub mod slot_owner {
    /// The menu's own slots, emptied back to the player on close (crafting grids).
    pub const MENU: u32 = 0;
    /// What the menu opened on: a block entity, an entity, a storage.
    pub const HOLDER: u32 = 1;
    /// The player's inventory from `start`.
    pub const PLAYER: u32 = 2;
    /// The player's main inventory, laid out like `comp::entity::SLOT_GROUPS` says.
    pub const PLAYER_MAIN: u32 = 3;
    pub const PLAYER_HOTBAR: u32 = 4;
    /// Stands for one group per equipment slot, named like the slots, where it is in the list:
    /// of the entity the menu is open on when it wears (`comp::entity::WEARS`), else the
    /// player's. Only data: `ui_kind::EQUIPMENT` (`ui.equipment()`) draws them where a screen says.
    pub const EQUIPMENT: u32 = 5;
    /// One of those slots, `start` its place among them (what `EQUIPMENT` becomes).
    pub const WORN: u32 = 6;
}

crate::data! {
    /// Slots shown and handled together (`comp::menu::GROUPS`, the player's
    /// `comp::entity::SLOT_GROUPS`): a grid of `cols` by `rows` of one `owner`'s (`slot_owner`)
    /// inventory with the same rules, found by `name` by the screen and the menu's callbacks.
    /// `start` is the first index in the owner's inventory (`u32::MAX`: right after the previous
    /// group of the same owner), `flags` are `slot_flags`; only the items reference `accepts`
    /// names go in (`#fuel #mymod:fire_starter`; empty: any), at most `limit` a slot (0: as many as the item stacks to);
    /// `size` is a slot's side in GUI pixels (0: 18), `background` a GUI texture and `tooltip` a
    /// translation shown while it is empty. Shift-clicks from the player's inventory fill groups
    /// with a higher `priority` first.
    pub struct SlotGroupDef {
        pub name: alloc::string::String,
        pub owner: u32,
        pub start: u32,
        pub cols: u32,
        pub rows: u32,
        pub flags: u32,
        pub accepts: alloc::string::String,
        pub limit: u32,
        pub size: f32,
        pub background: alloc::string::String,
        pub tooltip: alloc::string::String,
        pub priority: i32,
    }
}

#[repr(C)]
pub struct MenuCtx {
    _p: [u8; 0],
}

#[repr(C)]
pub struct MenuArgs {
    pub world: *mut WorldCtx,
    pub menu: *mut MenuCtx,
    pub player: u64,
    /// What the menu was opened on (a block entity, a stack in a slot), `Holder::NONE` for the
    /// player's own and menus of no one (a crafting table).
    pub owner: Holder,
    pub slot: u32,
    /// `on_take` of a crafting grid's result: how many runs of the recipe it was (shift crafts
    /// all that fit at once); 1 otherwise.
    pub times: u32,
}

// ---------------------------------------------------------------- mod UI

#[repr(C)]
pub struct UiCtx {
    _p: [u8; 0],
}

/// Client-side game view for HUD bindings.
#[repr(C)]
pub struct ClientCtx {
    _p: [u8; 0],
}

/// Layout of the `worn` item component; see `comp::WORN`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Worn {
    /// Model pixels added on every side, so layers sit above the skin and each other.
    pub inflate: f32,
}
impl Schema for Worn {
    const SCHEMA: &'static str = "{inflate:f32}";
}

pub mod ui_kind {
    pub const CONTAINER: u32 = 0;
    pub const PANEL: u32 = 1;
    pub const TEXT: u32 = 2;
    pub const BUTTON: u32 = 3;
    /// Slots of the group named by `class`: all of them as its grid, or only number `slot` when
    /// it is not `u32::MAX`.
    pub const SLOT: u32 = 4;
    /// Bound value 0..1 from `bind`, filling in direction `dir`.
    pub const PROGRESS: u32 = 5;
    pub const IMAGE: u32 = 6;
    pub const SPACER: u32 = 7;
    /// Row of `count` icons (full/half/empty textures from the theme `class`) driven by `bind`;
    /// `data` 1 fills from the right.
    pub const ICONS: u32 = 8;
    /// The local player's model with what it wears, turning towards the mouse.
    pub const PLAYER_MODEL: u32 = 9;
    /// In a recipe's `display`: its inputs (`data` 0) or outputs (1) as the kind's grid, or only
    /// number `slot` when it is not `u32::MAX`.
    pub const RECIPE_SLOT: u32 = 10;
    /// What mods add to place `text` of screen `class` (`entry_kind::SCREEN_PART`).
    pub const SCREEN_PARTS: u32 = 11;
    /// The player's equipment slots of the open menu (`slot_owner::EQUIPMENT`), a column.
    pub const EQUIPMENT: u32 = 13;
    /// The open menu's title: the own name of what it belongs to (a renamed chest), else the
    /// name of the block it was opened on, else the translation `menu.<ns>.<name>`.
    pub const TITLE: u32 = 14;
    /// A one-line text field showing `text` (a ready value, not a key), at most `count`
    /// characters (0: any); `on_change` gets each edit.
    pub const INPUT: u32 = 15;
    /// A slider at `bind` (0..1); `on_change` gets the value the player drags it to. Clicked, the
    /// arrow keys move it one of `count` steps across (0: a hundred).
    pub const SLIDER: u32 = 16;
    /// Item `data` as its icon, with `count` under it when above 1.
    pub const ITEM: u32 = 17;
    /// A progress bar (`bind`, class `arrow` unless given) that opens the open menu's recipe
    /// kinds when clicked, with "Recipes" over it.
    pub const RECIPE_ARROW: u32 = 12;
}

/// Fill directions of progress bars.
pub mod ui_dir {
    pub const RIGHT: u32 = 0;
    pub const UP: u32 = 1;
    pub const LEFT: u32 = 2;
    pub const DOWN: u32 = 3;
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UiArg {
    /// `ui_arg`.
    pub kind: u32,
    pub text: Str,
    pub num: f64,
    /// Decimals of a float.
    pub digits: u32,
}
defaults!(UiArg { kind: 0, text: Str::EMPTY, num: 0.0, digits: 1 });

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UiNode {
    pub kind: u32,
    pub class: Str,
    /// Translation key (or texture for IMAGE). Empty with one argument: that argument as it is.
    pub text: Str,
    pub args: Slice<UiArg>,
    /// NaN = automatic.
    pub width: f32,
    pub height: f32,
    /// Percent of the parent instead, NaN = not.
    pub width_pct: f32,
    pub height_pct: f32,
    pub min: [f32; 2],
    pub max_width: f32,
    /// `ui_align` of this node in its parent, `u32::MAX` = the parent's.
    pub self_align: u32,
    /// `ui_flags`.
    pub flags: u32,
    /// Text size, 1 = normal.
    pub scale: f32,
    pub tooltip_args: Slice<UiArg>,
    /// INPUT and SLIDER: `UiChangeArgs` when the player changes the value.
    pub on_change: Callback,
    pub grow: f32,
    pub gap: f32,
    pub pad: [f32; 4],
    pub row: u32,
    /// 0 start, 1 center, 2 end, 3 stretch.
    pub align: u32,
    /// 0 start, 1 center, 2 end, 3 space between.
    pub justify: u32,
    pub slot: u32,
    pub data: u32,
    pub count: u32,
    /// `UiBindArgs`: fills `value` (and text via `Host::ui_text`) every frame.
    pub bind: Callback,
    pub on_click: Callback,
    /// BUTTON: clicked twice in a row, after `on_click` (a world in a list played).
    pub on_double_click: Callback,
    pub tooltip: Str,
    /// `ui_dir` for progress bars.
    pub dir: u32,
    /// ICONS: how the row moves and lights up (`IconEffect`).
    pub effects: Slice<IconEffect>,
}

/// Something a row of icons does (`UiNode::effects`), each by its own value read every frame.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IconEffect {
    /// `icon_effect::*`.
    pub kind: u32,
    /// `UiBindArgs`: what it goes by, as each kind says; none: 1.
    pub bind: Callback,
    /// GUI pixels for moves, seconds for flashes and pulses.
    pub amount: f32,
    /// `LOOK`: the class drawn instead.
    pub class: Str,
}

/// `IconEffect::kind`.
pub mod icon_effect {
    /// Icons jump about by up to the value times `amount` pixels (0: still).
    pub const SHAKE: u32 = 0;
    /// For `amount` seconds after the row's value goes up, what it gained blinks, and
    /// `<class>_flash` lights the row where there is one (hearts healing).
    pub const FLASH_GAIN: u32 = 1;
    /// The same after it goes down, showing what was lost (hearts hurt).
    pub const FLASH_LOSS: u32 = 2;
    /// What the row would be at the value, fading in and out every `amount` seconds (what the
    /// held food would fill); below the row's own value nothing.
    pub const PREVIEW: u32 = 3;
    /// A second row at the value, in `<class>_overlay` icons over the first (saturation).
    pub const OVERLAY: u32 = 4;
    /// While the value is above 0, a wave runs along the row, `amount` pixels high (hearts
    /// regenerating).
    pub const WAVE: u32 = 5;
    /// The halves the row holds (none: twice the icon count): past the count, more rows of it
    /// go on above, closer together the more there are (hearts past twenty health).
    pub const MAX: u32 = 6;
    /// While the value is above 0, the row is drawn in `class`'s icons instead (food turned
    /// green by hunger, poisoned hearts).
    pub const LOOK: u32 = 7;
}
defaults!(UiNode {
    kind: 0,
    class: Str::EMPTY,
    text: Str::EMPTY,
    args: Slice::EMPTY,
    width: f32::NAN,
    height: f32::NAN,
    width_pct: f32::NAN,
    height_pct: f32::NAN,
    min: [0.0; 2],
    max_width: f32::INFINITY,
    self_align: u32::MAX,
    flags: 0,
    scale: 1.0,
    tooltip_args: Slice::EMPTY,
    on_change: Callback::NONE,
    grow: 0.0,
    gap: 0.0,
    pad: [0.0; 4],
    row: 0,
    align: 0,
    justify: 0,
    slot: 0,
    data: u32::MAX,
    count: 0,
    bind: Callback::NONE,
    on_click: Callback::NONE,
    on_double_click: Callback::NONE,
    tooltip: Str::EMPTY,
    dir: 0,
    effects: Slice::EMPTY,
});

#[repr(C)]
pub struct UiArgs {
    pub ui: *mut UiCtx,
    pub client: *mut ClientCtx,
}

#[repr(C)]
pub struct UiBindArgs {
    pub client: *mut ClientCtx,
    /// Out: numeric value.
    pub value: f32,
    /// Pass to `Host::ui_text` to set text.
    pub text: *mut UiCtx,
}

#[repr(C)]
pub struct UiRevisionArgs {
    pub client: *mut ClientCtx,
    pub revision: u64,
}

/// A player changed an INPUT (`text`) or SLIDER (`value`, 0..1).
#[repr(C)]
pub struct UiChangeArgs {
    pub client: *mut ClientCtx,
    pub text: Str,
    pub value: f32,
}

/// `UiNode::flags`.
pub mod ui_flags {
    /// Children scroll inside the node (mouse wheel) and are clipped to it.
    pub const SCROLL: u32 = 1;
    /// Text stays on one line.
    pub const NO_WRAP: u32 = 2;
    /// Text with digit-wide cells, so changing numbers don't move what follows.
    pub const MONO: u32 = 4;
    /// A button that can't be pressed.
    pub const DISABLED: u32 = 8;
    /// A button shown as chosen (the open tab).
    pub const SELECTED: u32 = 16;
}

#[repr(C)]
pub struct ScreenKeyArgs {
    pub client: *mut ClientCtx,
    /// The key's settings name (`Enter`, `KeyE`).
    pub key: Str,
}

// ---------------------------------------------------------------- host

/// Whose stack `Host::client_slot` reads.
pub mod slot_container {
    /// The player's own inventory.
    pub const INVENTORY: u32 = 0;
    /// The open menu's slots.
    pub const MENU: u32 = 1;
    /// The stack held on the cursor in a menu (slot 0).
    pub const CURSOR: u32 = 2;
}

pub mod log_level {
    pub const ERROR: u32 = 1;
    pub const WARN: u32 = 2;
    pub const INFO: u32 = 3;
    pub const DEBUG: u32 = 4;
    pub const TRACE: u32 = 5;
}

#[repr(C)]
pub struct Host {
    pub log: unsafe extern "C" fn(me: ModId, level: u32, msg: Str),
    /// Like `log` under a sub-target of the mod's own: `mod:<id>::<target>` (`mod:<id>` if empty),
    /// so players can filter it with `--log`/`/log`.
    pub log_at: unsafe extern "C" fn(me: ModId, level: u32, target: Str, msg: Str),
    /// Non-zero when a record at `level` for that target would be written; check before building
    /// costly diagnostics.
    pub log_enabled: unsafe extern "C" fn(me: ModId, level: u32, target: Str) -> u32,

    /// `cb` (`WorldTickArgs`) once per tick in every region of every dimension, for logic that
    /// isn't one entity's or block's (a dimension's loaded parts are regions run in parallel).
    pub reg_world_tick: unsafe extern "C" fn(reg: *mut Registrar, cb: Callback),
    /// In `ticks` ticks `event` fires with `data` (one value of its type). The timer is saved with
    /// its holder: an entity (gone with it), a block (with its cube, `event::BLOCK_TICK` being the
    /// block's own tick), or `NONE` for the dimension. Time an unloaded holder misses counts, so a
    /// timer due while away fires when it loads. A stack (`HELD`) doesn't tick, so it can't hold one.
    pub timer: unsafe extern "C" fn(world: *mut WorldCtx, holder: *const Holder, ticks: u32, event: u32, data: Slice<u8>),
    /// Chat command `/name`, arguments described by `usage` (see the engine's command syntax);
    /// `cb` gets `CommandArgs` with one value per named parameter.
    /// `client` non-zero: it runs on the client that types it, never sent to the server, `cb` getting
    /// `CommandArgs` with `client` set.
    pub reg_command: unsafe extern "C" fn(reg: *mut Registrar, name: Str, usage: Str, client: u32, cb: Callback),

    /// Looks up a frozen id by full name ("base:stone"), `u32::MAX` if missing. Blocks give their
    /// first state and write the state count to `count` (others write 1). `kind` is `entry_kind`.
    pub find: unsafe extern "C" fn(kind: u32, name: Str, count: *mut u32) -> u32,
    /// State id -> first state id of its block.
    pub block_of: unsafe extern "C" fn(state: u32) -> u32,
    /// The value of property `name` in `state` (empty if the block has no such property).
    pub state_prop: unsafe extern "C" fn(state: u32, name: Str) -> Str,
    /// `state` with property `name` set to `value`, the others kept; `u32::MAX` if the block has
    /// no such property or value.
    pub state_with: unsafe extern "C" fn(state: u32, name: Str, value: Str) -> u32,
    /// `state` with property `name` at its next value, the first after the last; `u32::MAX` if the
    /// block has no such property.
    pub state_cycle: unsafe extern "C" fn(state: u32, name: Str) -> u32,
    /// `state` turned by `Turn` `turn` (its index) with the world around it: another facing, axis
    /// or sides; itself where its block can't turn so, `u32::MAX` for no such state or turn.
    pub state_turned: unsafe extern "C" fn(state: u32, turn: u8) -> u32,
    pub state_flags: unsafe extern "C" fn(state: u32) -> u32,
    /// Component id by name, `u32::MAX` if nothing uses it. A non-empty `schema` that differs from
    /// the registered one is an error (logged) and also gives `u32::MAX`.
    pub find_component: unsafe extern "C" fn(name: Str, schema: Str) -> u32,
    /// Whether block matcher `matcher` (a `match` value's id) takes `state`.
    pub matcher_has: unsafe extern "C" fn(matcher: u32, state: u32) -> u32,
    /// The frozen component tables (null before freeze); stable for the whole session.
    pub component_table: unsafe extern "C" fn() -> *const ComponentTable,
    /// Value bytes of stack component `comp` on stack data `data`, null if absent.
    pub stack_get: unsafe extern "C" fn(data: u32, comp: u32, len: *mut usize) -> *const u8,
    /// Stack data with `comp` set to `value` (or removed when `remove` is non-zero).
    pub stack_with: unsafe extern "C" fn(data: u32, comp: u32, value: Slice<u8>, remove: u32) -> u32,
    /// Slots of storage `comp` (`comp::component::SLOTS`), 0 for what isn't one.
    pub storage_slots: unsafe extern "C" fn(comp: u32) -> u32,
    /// Name and value schema of component `comp` (empty schema: a tag); 0 if there is none.
    pub component_info: unsafe extern "C" fn(comp: u32, name: *mut Str, schema: *mut Str) -> u32,
    /// What block state `state` is; 0 if there is none.
    pub block_info: unsafe extern "C" fn(state: u32, out: *mut BlockInfo) -> u32,
    /// What item `item` is; 0 if there is none.
    pub item_info: unsafe extern "C" fn(item: u32, out: *mut ItemInfo) -> u32,

    /// The block where the running callback is: the world, or the body a block of it is on or
    /// `world_enter_body` entered. `u32::MAX` for what isn't loaded.
    pub block_get: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos) -> u32,
    /// The id of the dimension this callback runs in.
    pub world_dimension: unsafe extern "C" fn(world: *mut WorldCtx) -> u32,
    /// Like `block_get`, where it reads. Returns 0 if the position is not loaded.
    pub block_set: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos, state: u32) -> u32,
    /// What is in a cell, the one `block_get` gives first: writes up to `cap`, returns how many.
    pub part_list: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos, out: *mut u32, cap: u32) -> u32,
    /// Puts another block in a cell; 0 if not loaded or two parts would both want to be its main.
    pub part_add: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos, state: u32) -> u32,
    /// Replaces a part of a cell with another state, air taking it out; 0 if it isn't there.
    pub part_replace: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos, old: u32, new: u32) -> u32,
    pub game_time: unsafe extern "C" fn(world: *mut WorldCtx) -> u64,
    /// Deterministic per-world random stream.
    pub random: unsafe extern "C" fn(world: *mut WorldCtx) -> u64,
    pub light: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos) -> u32,
    /// 0..1 sun strength for the world's time of day.
    pub daylight: unsafe extern "C" fn(world: *mut WorldCtx) -> f32,
    /// A number of its own for each time a world is opened: entity ids and time start over in
    /// another world, so what a mod keeps by them goes by this too.
    pub world_session: unsafe extern "C" fn(world: *mut WorldCtx) -> u64,

    /// An entity as `desc` says; 0 if it can't be (an unknown kind, one that belongs to a
    /// block, an item kind without a stack).
    pub entity_spawn: unsafe extern "C" fn(world: *mut WorldCtx, desc: *const SpawnEntityDesc) -> u64,
    pub entity_despawn: unsafe extern "C" fn(world: *mut WorldCtx, id: u64),
    /// Puts `rider` on `mount` in `seat` (`u32::MAX`: the first free one), at the end of the
    /// tick, after `action::MOUNT` let it. False when it can't: no such seat, taken, or the
    /// mount rides the rider somewhere down its chain.
    pub entity_mount: unsafe extern "C" fn(world: *mut WorldCtx, rider: u64, mount: u64, seat: u32) -> u32,
    /// Takes `rider` off what it rides, after `action::DISMOUNT` let it.
    pub entity_dismount: unsafe extern "C" fn(world: *mut WorldCtx, rider: u64),
    /// What `rider` sits on and in which seat; 0 for nothing.
    pub entity_vehicle: unsafe extern "C" fn(world: *mut WorldCtx, rider: u64, seat: *mut u32) -> u64,
    /// The riders of `mount`, one per seat (0 for an empty one), up to `cap`; returns its seats.
    pub entity_riders: unsafe extern "C" fn(world: *mut WorldCtx, mount: u64, out: *mut u64, cap: u32) -> u32,
    pub entity_get: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, out: *mut EntityState) -> u32,
    /// Adds `v` (blocks per second) to the entity's velocity: a mob's here, a player's on its
    /// client, the way a hit's knockback goes.
    pub entity_push: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, v: *const Vec3),
    /// Writes the fields of `state` that `set` names (`entity_set::*`), players' on their client
    /// too. Health only comes back by respawning: 0 or less is a death by `damage_cause::VOID`.
    pub entity_set: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, set: u32, state: *const EntityState),
    /// AI steering for this tick: horizontal direction, speed factor, and the movement keys a
    /// player would hold (`walk::*`), which the same physics and poses answer.
    pub entity_walk: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, dir: *const [f32; 2], speed: f32, keys: u32),
    /// Hurts `id` (a negative amount heals). Non-zero if it landed.
    pub entity_damage: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, damage: *const Damage) -> u32,
    /// `attacker` hits `target` with what it holds: the `ATTACK` question, its `ATTACK_DAMAGE`
    /// against them through `chain::MELEE` (charge, crit), knockback and a sweep. Non-zero if the
    /// hit landed.
    pub entity_attack: unsafe extern "C" fn(world: *mut WorldCtx, attacker: u64, target: u64) -> u32,
    /// `id` uses what is in its main hand, or its other one (`off`), on the block at `target`
    /// (null: on nothing) from `face`, as a player's right click does. 1 if it did something
    /// or started to (eating takes time; it goes on while the hand holds the item).
    pub entity_use_item: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, off: u32, target: *const BlockPos, face: u32) -> u32,
    /// `id` lets go of what it is using, as a player letting go of the use button: its
    /// `comp::item::ON_RELEASE` hears after how long (a skeleton's bow).
    pub entity_release_use: unsafe extern "C" fn(world: *mut WorldCtx, id: u64),
    /// What a player holds of action `key` on the server (`held`, `held_ticks`; `pressed` and
    /// `released` are its client's): what its client last said.
    pub entity_key: unsafe extern "C" fn(world: *mut WorldCtx, player: u64, key: Str, out: *mut KeyState),
    /// `id` starts breaking the block at `pos` (null: stops), as a player holding the button does:
    /// as fast as what it holds mines it, cracks shown, broken when done within its reach. 0 when
    /// it may not (`action::BREAK_BLOCK`).
    pub entity_mine: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, pos: *const BlockPos) -> u32,
    /// Whether `target` is within `id`'s `ENTITY_REACH` attribute, from its eyes to the nearest
    /// point of the target's box. Non-zero if so.
    pub entity_in_reach: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, target: u64) -> u32,
    /// What a ray from `from` along `dir` hits first within `max` blocks, of what `flags` say
    /// (`ray::*`), past entity `skip`; `from` in the caller's frame. Non-zero if it hit.
    pub raycast: unsafe extern "C" fn(
        world: *mut WorldCtx,
        from: *const Vec3,
        dir: *const Vec3,
        max: f64,
        flags: u32,
        skip: u64,
        out: *mut RayHit,
    ) -> u32,
    pub in_liquid: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const Vec3) -> u32,
    pub entity_swing_strength: unsafe extern "C" fn(world: *mut WorldCtx, id: u64) -> f32,
    /// Entities in a shape (their feet, or with `entity_filter::TOUCHING` any of their box),
    /// of `kind` (`u32::MAX`: any): writes up to `max` ids, returns how many there are.
    pub entities_in:
        unsafe extern "C" fn(world: *mut WorldCtx, shape: Slice<ShapeStep>, kind: u32, flags: u32, out: *mut u64, max: usize) -> usize,
    /// An entity's component bytes, null if it has none. Changes are found by comparing with
    /// what was last sent, so writing through the pointer is enough.
    pub component: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, comp: u32) -> *mut u8,
    pub player_count: unsafe extern "C" fn(world: *mut WorldCtx) -> u32,
    pub player_at: unsafe extern "C" fn(world: *mut WorldCtx, index: u32) -> u64,
    /// `game_mode`, survival for what isn't a player.
    pub game_mode: unsafe extern "C" fn(world: *mut WorldCtx, entity: u64) -> u32,

    /// Slots of what `holder` holds, 0 when it has no inventory.
    pub inventory_len: unsafe extern "C" fn(world: *mut WorldCtx, holder: *const Holder) -> u32,
    pub inventory_get: unsafe extern "C" fn(world: *mut WorldCtx, holder: *const Holder, slot: u32) -> ItemStack,
    pub inventory_set: unsafe extern "C" fn(world: *mut WorldCtx, holder: *const Holder, slot: u32, stack: ItemStack),
    /// Merges into existing stacks first; returns what did not fit.
    pub inventory_insert: unsafe extern "C" fn(world: *mut WorldCtx, holder: *const Holder, stack: ItemStack) -> ItemStack,
    pub selected_slot: unsafe extern "C" fn(world: *mut WorldCtx, player: u64) -> u32,
    pub block_entity: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos) -> u64,
    /// Opens `menu` on `holder` (`Holder::NONE` for menus without one). 0 when the holder isn't
    /// there or has no inventory: the reason is logged and nothing opens.
    pub open_menu: unsafe extern "C" fn(world: *mut WorldCtx, player: u64, menu: u32, holder: *const Holder) -> u32,

    /// Group `name` of an open menu: its first menu slot and how many slots it has. 0 if none.
    /// Group `name` of an open menu: its first slot, slot count and columns.
    pub menu_group: unsafe extern "C" fn(menu: *mut MenuCtx, name: Str, out: *mut SlotGroup) -> u32,
    /// Group `name` of `holder`'s inventory (a block entity's: the `holder` groups of the menu its
    /// block opens; a player's: its layout): first index and count. 0 if it has no such group.
    pub inventory_group: unsafe extern "C" fn(world: *mut WorldCtx, holder: *const Holder, name: Str, out: *mut SlotGroup) -> u32,
    pub menu_get: unsafe extern "C" fn(menu: *mut MenuCtx, slot: u32) -> ItemStack,
    pub menu_set: unsafe extern "C" fn(menu: *mut MenuCtx, slot: u32, stack: ItemStack),

    /// Matches a grid against recipes of `kind`; returns the output stack (empty if none).
    pub recipe_match: unsafe extern "C" fn(kind: Str, grid: *const ItemStack, width: u32, height: u32) -> ItemStack,
    /// Processing recipe for one input item: output stack, time written to `time_out`.
    pub recipe_process: unsafe extern "C" fn(kind: Str, input: u32, time_out: *mut u32) -> ItemStack,
    /// The first recipe of `kind` from number `from` on whose inputs (with counts) are all in
    /// `stacks`, any order: its number + 1, or 0.
    pub recipe_find: unsafe extern "C" fn(kind: Str, stacks: *const ItemStack, count: u32, from: u32) -> u32,
    pub recipe_info: unsafe extern "C" fn(kind: Str, recipe: u32, out: *mut RecipeInfo) -> u32,
    /// Output `index` of a recipe, a liquid's as its item by the millibucket, and its chance: the
    /// main one, then each its pools can give, with the most of it.
    pub recipe_output: unsafe extern "C" fn(kind: Str, recipe: u32, index: u32, stack: *mut ItemStack, chance: *mut f32) -> u32,
    /// Rolls the recipe's pools for what else one run of it gives, like `roll_loot`.
    pub recipe_roll: unsafe extern "C" fn(world: *mut WorldCtx, kind: Str, recipe: u32, out: *mut ItemStack, max: usize) -> usize,
    /// Takes the recipe's inputs out of `stacks` (counts, the matching stacks); 0 if they are not
    /// all there, then nothing changes.
    pub recipe_consume: unsafe extern "C" fn(kind: Str, recipe: u32, stacks: *mut ItemStack, count: u32) -> u32,
    /// The block state an item places, 0 if none.
    pub item_block: unsafe extern "C" fn(item: u32) -> u32,

    pub ui_open: unsafe extern "C" fn(ui: *mut UiCtx, node: *const UiNode),
    pub ui_close: unsafe extern "C" fn(ui: *mut UiCtx),
    /// From a text binding: set the translation key and args shown (an empty key with one
    /// argument shows that argument as it is).
    pub ui_text: unsafe extern "C" fn(ui: *mut UiCtx, key: Str, args: Slice<UiArg>),
    pub client_player: unsafe extern "C" fn(client: *mut ClientCtx) -> u64,
    pub client_attribute: unsafe extern "C" fn(client: *mut ClientCtx, name: Str) -> f64,
    /// The level of entity effect `effect` on the local player, 0 without it.
    pub client_effect_level: unsafe extern "C" fn(client: *mut ClientCtx, effect: u32) -> u32,
    pub client_swing_strength: unsafe extern "C" fn(client: *mut ClientCtx) -> f32,
    /// Ticks left of the item being used, negative when none.
    pub client_use_ticks_left: unsafe extern "C" fn(client: *mut ClientCtx) -> f32,
    pub client_day_time: unsafe extern "C" fn(client: *mut ClientCtx) -> u64,
    /// Synced component of an entity the client knows (own player always), null if unknown.
    pub client_component: unsafe extern "C" fn(client: *mut ClientCtx, id: u64, comp: u32) -> *const u8,
    pub client_entity: unsafe extern "C" fn(client: *mut ClientCtx, id: u64, out: *mut EntityState) -> u32,
    pub client_target: unsafe extern "C" fn(client: *mut ClientCtx, out: *mut Target) -> u32,

    /// Energy contract (`energy` component, on an entity or a stack): moves `amount` in (positive)
    /// or out (negative), respecting capacity and rate limits. Returns what moved (or would, when
    /// `simulate` is non-zero), with the same sign.
    pub energy_move: unsafe extern "C" fn(world: *mut WorldCtx, holder: *const Holder, amount: i64, simulate: u32) -> i64,
    /// Copies the holder's `comp` (an entity's, a block entity's or a stack's) into `out`, up to
    /// `cap` bytes; returns its length, 0 when it has none.
    pub component_get: unsafe extern "C" fn(world: *mut WorldCtx, holder: *const Holder, comp: u32, out: *mut u8, cap: u32) -> u32,
    /// Sets the holder's `comp`, or takes it away with an empty value (stacks only: entities
    /// keep what their kind attaches). Non-zero if it took.
    pub component_set: unsafe extern "C" fn(world: *mut WorldCtx, holder: *const Holder, comp: u32, value: Slice<u8>) -> u32,
    /// A line for the player who ran the command: translation `key` with `list`, formatted by
    /// their client in its language. Call again for more lines.
    pub command_reply: unsafe extern "C" fn(args: *mut CommandArgs, key: Str, list: Slice<UiArg>),
    /// Puts text on players' screens.
    pub show: unsafe extern "C" fn(w: *mut WorldCtx, who: Who, desc: *const ShowDesc),
    /// Puts text on this client's screen.
    pub client_show: unsafe extern "C" fn(client: *mut ClientCtx, desc: *const ShowDesc),
    /// Makes who writes players' chat lines; the last mod loaded wins.
    pub reg_chat_format: unsafe extern "C" fn(r: *mut Registrar, cb: Callback),
    pub chat_format_set: unsafe extern "C" fn(args: *mut ChatFormatArgs, key: Str, list: Slice<UiArg>),
    /// Non-zero when a player in this world shows gizmo group `group` (`/gizmo <group>`).
    pub gizmo_shown: unsafe extern "C" fn(world: *mut WorldCtx, group: Str) -> u32,
    /// Debug shape (`gizmo_kind`) for `who` of the players showing `group` (empty: all of them),
    /// from `a` to `b`; `color` is 0xRRGGBBAA, kept for `ticks`.
    pub gizmo:
        unsafe extern "C" fn(world: *mut WorldCtx, who: Who, group: Str, kind: u32, a: *const Vec3, b: *const Vec3, color: u32, ticks: u32),
    /// The translation `key` with `args` at `pos` as `gizmo` shows a shape, `size` times as big as
    /// menu text (0 counts as 1), in each viewer's language.
    pub gizmo_text: unsafe extern "C" fn(
        world: *mut WorldCtx,
        who: Who,
        group: Str,
        pos: *const Vec3,
        size: f32,
        color: u32,
        ticks: u32,
        key: Str,
        args: Slice<UiArg>,
    ),

    // ------------------------------------------------ client side and mod channels
    /// `ClientFrameArgs`, every frame on the client before input is handled; may drive the client.
    pub reg_client_frame: unsafe extern "C" fn(reg: *mut Registrar, cb: Callback),
    /// Moves a bone of what `entity` is or has on, for the next frame: radians about x, y, z, pixels, scale.
    pub client_pose_bone:
        unsafe extern "C" fn(c: *mut ClientCtx, entity: u64, bone: Str, turn: *const [f32; 3], by: *const [f32; 3], scale: *const [f32; 3]),
    /// The action `entity` plays now, in `out`; 0 for none.
    pub client_action_current: unsafe extern "C" fn(c: *mut ClientCtx, entity: u64, out: *mut Str) -> u32,
    /// Seconds a clip of an action takes on `entity`; below 0 if it has none.
    pub client_clip_length: unsafe extern "C" fn(c: *mut ClientCtx, entity: u64, name: Str) -> f32,
    /// The item in `entity`'s `hand`; 0 for none.
    pub environment_set: unsafe extern "C" fn(w: *mut WorldCtx, entity: u64, name: Str, data: Slice<u8>) -> u32,
    pub client_environment_set: unsafe extern "C" fn(c: *mut ClientCtx, name: Str, data: Slice<u8>) -> u32,
    pub client_environment_values: unsafe extern "C" fn(c: *mut ClientCtx, out: *mut environment::EnvironmentValues),
    pub client_displayed: unsafe extern "C" fn(c: *mut ClientCtx, entity: u64, source: u32, out: *mut ItemStack, max: usize) -> usize,
    /// Where a bone of what `entity` is or has on was, the last frame. 0 if there is none.
    pub client_bone_position: unsafe extern "C" fn(c: *mut ClientCtx, entity: u64, bone: Str, out: *mut Vec3) -> u32,
    /// Message channel `name` between a mod's client and server parts, carrying values of
    /// `schema` (both sides must agree, sends with another are refused). `side` 0: `cb` runs on
    /// the server with `PacketArgs` when a client sends; 1: on the client with `ClientPacketArgs`.
    pub reg_packet: unsafe extern "C" fn(reg: *mut Registrar, name: Str, schema: Str, side: u32, cb: Callback),

    /// A mistake in what a mod registers that the SDK caught (a bad asset path ...): loading
    /// stops at freeze with it, named with the mod.
    pub reg_mistake: unsafe extern "C" fn(reg: *mut Registrar, message: Str),
    /// Value of `--name value` on the command line, empty if not given.
    pub launch_option: unsafe extern "C" fn(name: Str) -> Str,

    /// Server: sends `data` on channel `name` to the clients of `who` (after this tick's changes).
    pub send_packet: unsafe extern "C" fn(world: *mut WorldCtx, who: Who, name: Str, schema: Str, data: Slice<u8>),
    /// Moves an entity (players too, their client follows) at the end of the tick; `dim` is a
    /// dimension id, `u32::MAX` = the same one.
    pub entity_teleport: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, dim: u32, pos: *const Vec3),
    /// Sets this dimension's time of day counter at the end of the tick.
    pub set_day_time: unsafe extern "C" fn(world: *mut WorldCtx, time: u64),
    /// The time of day's clock, which `set_day_time` sets (0 sunrise, 6000 noon, then on past a day).
    pub day_time: unsafe extern "C" fn(world: *mut WorldCtx) -> u64,
    /// As if `ticks` passed at once: the day moves on, and growth, machines and timers catch up.
    pub skip_ticks: unsafe extern "C" fn(world: *mut WorldCtx, ticks: u64),
    /// Fills a shape with one block at the end of the tick, the whole dimension at once: loaded
    /// cubes on every core, the rest when they load (`FillDesc`).
    pub fill: unsafe extern "C" fn(world: *mut WorldCtx, desc: *const FillDesc),
    /// Whether `p` is inside a shape; 0 also for steps that don't make one.
    pub shape_contains: unsafe extern "C" fn(shape: Slice<ShapeStep>, p: *const Vec3) -> u32,
    /// A box around a shape; 0 for steps that don't make one.
    pub shape_bounds: unsafe extern "C" fn(shape: Slice<ShapeStep>, min: *mut Vec3, max: *mut Vec3) -> u32,
    /// 0 survival, 1 creative, 2 spectator.
    pub set_game_mode: unsafe extern "C" fn(world: *mut WorldCtx, player: u64, mode: u32),

    /// Client, during any client callback: the local view, see `ClientState`.
    pub client_state: unsafe extern "C" fn(client: *mut ClientCtx, out: *mut ClientState),
    /// Client frame hooks only: simulated input, see `client_input`. Returns 0 for an unknown
    /// action or key.
    pub client_input: unsafe extern "C" fn(client: *mut ClientCtx, op: u32, text: Str, x: f32, y: f32) -> u32,
    /// An action's keys in a frame hook, all up while a screen has the input.
    pub client_key_state: unsafe extern "C" fn(client: *mut ClientCtx, action: Str, out: *mut KeyState),
    /// An action's keys as steering or a camera mode sees them (`SteerArgs::keys`).
    pub keys_state: unsafe extern "C" fn(keys: *const KeysCtx, action: Str, out: *mut KeyState),
    /// Client frame hooks only: moves/turns the local player directly.
    pub client_control: unsafe extern "C" fn(client: *mut ClientCtx, control: *const PlayerControl),
    /// Client frame hooks only: see `client_op`.
    pub client_op: unsafe extern "C" fn(client: *mut ClientCtx, op: u32, text: Str, num: f64) -> u64,
    /// Texts the client knows, see `client_text`. Valid until the next call.
    pub client_text: unsafe extern "C" fn(client: *mut ClientCtx, which: u32, index: u32) -> Str,
    /// 1 and the answer's lines (in the client's language, joined by `\n`) once command `id`
    /// (`client_op::RUN_COMMAND`) was answered, 0 before. An answer is read once.
    pub client_command_reply: unsafe extern "C" fn(client: *mut ClientCtx, id: u32, out: *mut Str) -> u32,
    /// Block the client has at `pos`, `u32::MAX` if that cube is not loaded there.
    pub client_block_in: unsafe extern "C" fn(client: *mut ClientCtx, body: u64, pos: *const BlockPos) -> u32,
    /// 1 if the cube holding `pos` is here and has been drawn.
    pub client_cube_shown: unsafe extern "C" fn(client: *mut ClientCtx, pos: *const BlockPos) -> u32,
    /// Light the client has at `pos` (sky level in the low byte, then block red, green, blue),
    /// `u32::MAX` if that cube is not loaded there.
    pub client_light: unsafe extern "C" fn(client: *mut ClientCtx, pos: *const BlockPos) -> u32,
    /// Group `name` of the player inventory (`comp::entity::SLOT_GROUPS`, equipment slots,
    /// `core:equipment`): first slot and count.
    pub player_inventory_group: unsafe extern "C" fn(name: Str, out: *mut SlotGroup) -> u32,
    /// `container` is a `slot_container`.
    pub client_slot: unsafe extern "C" fn(client: *mut ClientCtx, container: u32, slot: u32) -> ItemStack,
    /// The open menu; 0 if none.
    pub client_menu: unsafe extern "C" fn(client: *mut ClientCtx, out: *mut OpenMenu) -> u32,
    /// Ids of entities the client knows (the local player not included), returns how many.
    pub client_entities: unsafe extern "C" fn(client: *mut ClientCtx, out: *mut u64, max: usize) -> usize,
    /// Where a UI element of the top screen is, as fractions of the window:
    /// `slot:N` a menu slot, `item:N` a creative inventory entry of item id N, `id:name` a node
    /// id, `text:words` the first text containing them (case-insensitive).
    pub client_ui_rect: unsafe extern "C" fn(client: *mut ClientCtx, query: Str, out: *mut Rect) -> u32,
    /// Client: a `gizmo_kind` shape for this frame (0xRRGGBBAA); with a `group` only while the
    /// player shows it.
    pub client_draw: unsafe extern "C" fn(client: *mut ClientCtx, group: Str, kind: u32, a: *const Vec3, b: *const Vec3, color: u32),
    /// The translation `key` with `args` at `pos` this frame, `size` times as big as menu text.
    pub client_draw_text:
        unsafe extern "C" fn(client: *mut ClientCtx, group: Str, pos: *const Vec3, size: f32, color: u32, key: Str, args: Slice<UiArg>),
    /// The translation `key` with `args` this frame, centered at (`x`, `y`) in fractions of the
    /// window, `size` times as big as menu text: part of the GUI, F1 hides it.
    pub client_screen_text:
        unsafe extern "C" fn(client: *mut ClientCtx, x: f32, y: f32, size: f32, color: u32, key: Str, args: Slice<UiArg>),
    /// Client: sends `data` on channel `name` to the server.
    pub client_send_packet: unsafe extern "C" fn(client: *mut ClientCtx, name: Str, schema: Str, data: Slice<u8>),

    /// RGBA pixels of an image (a screenshot id from `client_op::SCREENSHOT` once taken, or one
    /// from `image_load`), null while not available.
    pub image_pixels: unsafe extern "C" fn(id: u64, w: *mut u32, h: *mut u32) -> *const u8,
    /// Reads a PNG file, 0 if it cannot.
    pub image_load: unsafe extern "C" fn(path: Str) -> u64,
    pub image_save: unsafe extern "C" fn(path: Str, rgba: Slice<u8>, w: u32, h: u32) -> u32,
    pub image_unload: unsafe extern "C" fn(id: u64),
    /// Full name of a frozen id (`find` backwards); a block state gives `ns:block` or
    /// `ns:block#i` for its `i`-th state. Empty if unknown. Valid for the whole session.
    pub name_of: unsafe extern "C" fn(kind: u32, id: u32) -> Str,

    // ------------------------------------------------ liquids
    /// Liquid id of a block state, `u32::MAX` for none.
    pub liquid_of: unsafe extern "C" fn(state: u32) -> u32,
    /// Places or takes a liquid like a bucket would for `player` holding `stack` (what the engine
    /// does for items with a `liquid` component or empty containers of one). Returns the stack
    /// the player holds afterwards; `handled` is set non-zero if anything happened.
    pub use_container: unsafe extern "C" fn(world: *mut WorldCtx, player: u64, stack: ItemStack, handled: *mut u32) -> ItemStack,

    // ------------------------------------------------ events, questions, steps, loot
    /// Subscribes to event `name` carrying values of `schema` (checked against every other use);
    /// `cb` gets `EventArgs` once per tick and region with every value queued since.
    pub reg_event_handler: unsafe extern "C" fn(reg: *mut Registrar, name: Str, schema: Str, cb: Callback),
    /// Event id, `u32::MAX` when nobody listens (then there is nothing to emit).
    pub find_event: unsafe extern "C" fn(name: Str, schema: Str) -> u32,
    /// Queues one value of event `event` (its schema's size) for the end of the tick.
    pub emit: unsafe extern "C" fn(world: *mut WorldCtx, event: u32, value: Slice<u8>),
    /// Question `name` (`question::*`, `action::*` or a mod's own): its chain run from 1 (yes);
    /// 1 if the answer is yes.
    pub ask: unsafe extern "C" fn(world: *mut WorldCtx, name: Str, query: *const Query) -> u32,
    /// Runs chain `name` from `base`. With `explain` non-zero the steps are kept for
    /// `explanation`.
    pub chain_compute: unsafe extern "C" fn(world: *mut WorldCtx, name: Str, base: f64, ctx: *const Query, explain: u32) -> f64,
    /// Steps of the last explained `chain_compute` or `attribute` on this thread.
    pub explanation: unsafe extern "C" fn() -> Str,

    /// Its current value for an entity: its chain run from its base (the kind's component of the
    /// same name, else a world rule or the default) with what the entity has on it, clamped.
    /// With `explain` non-zero the steps are kept for `explanation`.
    pub attribute: unsafe extern "C" fn(world: *mut WorldCtx, entity: u64, name: Str, explain: u32) -> f64,
    /// Puts (or replaces, same `id`) step `step` with `amount` on it for `ticks` (0 = until
    /// removed), in `phase` (empty: the step's own). Saved.
    pub attribute_add_modifier:
        unsafe extern "C" fn(world: *mut WorldCtx, entity: u64, name: Str, id: Str, step: Str, phase: Str, amount: f64, ticks: u64),
    pub attribute_remove_modifier: unsafe extern "C" fn(world: *mut WorldCtx, entity: u64, name: Str, id: Str),
    /// An entry: a new one, an edit, a replacement or a removal (`EntryDesc::mode`).
    pub reg_entry: unsafe extern "C" fn(reg: *mut Registrar, entry: *const EntryDesc),
    /// Whether mod `id` is loaded, known from before any mod registers.
    pub mod_loaded: unsafe extern "C" fn(id: Str) -> u32,
    /// Whether condition `text` (`condition`) holds at `pos` for `entity` (0 for nobody); 0 for
    /// one that doesn't, or doesn't parse (said once in the log).
    pub holds: unsafe extern "C" fn(world: *mut WorldCtx, text: Str, pos: Vec3, entity: u64) -> u32,
    /// Rolls table `name`; writes at most `max` stacks to `out` and returns how many it made.
    pub roll_loot: unsafe extern "C" fn(world: *mut WorldCtx, name: Str, ctx: *const LootCtx, out: *mut ItemStack, max: usize) -> usize,
    /// Breaks a block as if mined with `tool` (by `player`, 0 for none): asks `break_block`,
    /// drops its loot when the tool may harvest it and `drops` is non-zero, fires `block_broken`.
    /// 0 if nothing was broken.
    pub break_block: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos, tool: ItemStack, player: u64, drops: u32) -> u32,
    /// Client: the entity the open menu is on (a block entity, a mob), 0 if none.
    pub client_menu_owner: unsafe extern "C" fn(client: *mut ClientCtx) -> u64,
    /// A system: once per tick and region `cb` gets `SystemArgs` with every entity that has the
    /// per-entity component `name` and their values side by side, to change in place.
    pub reg_system: unsafe extern "C" fn(reg: *mut Registrar, name: Str, schema: Str, cb: Callback),
    /// The `permission` level a server command asks for (by default `GAME_MASTER`).
    pub reg_command_permission: unsafe extern "C" fn(reg: *mut Registrar, command: Str, level: u32),
    /// Weather `weather` in this dimension for `ticks`, after which it changes by itself.
    pub set_weather: unsafe extern "C" fn(world: *mut WorldCtx, weather: u32, ticks: u32),
    /// The weather's id now.
    pub weather_now: unsafe extern "C" fn(world: *mut WorldCtx) -> u32,
    /// The weather the client has now.
    pub client_weather: unsafe extern "C" fn(client: *mut ClientCtx) -> u32,
    /// Effect `effect` on an entity at `level` (1 and up) for `ticks`, seen by everyone (players
    /// coming along later too), with its hazards and modifiers; 0 ticks ends it, `INFINITE_TICKS`
    /// keeps it on until taken off. Several at once,
    /// one of each: giving one again replaces it; an instant one acts once and doesn't stay.
    pub entity_effect: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, effect: u32, level: u32, ticks: u32),
    /// The effects on entity `id`, at most `cap` of them into `out`; returns how many it has.
    pub entity_effects: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, out: *mut EffectOn, cap: u32) -> u32,
    /// Holds an entity in `pose` (players too: their client follows), or with `u32::MAX` lets
    /// it go back to what its movement makes. `yaw` (radians) keeps it facing that way and still
    /// while it holds, `NaN` leaves it free (a bed, against a pet that sits).
    pub entity_set_pose: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, pose: u32, yaw: f32),
    /// A player's `permission` level.
    pub player_permission: unsafe extern "C" fn(world: *mut WorldCtx, player: u64) -> u32,
    /// Sets it, remembered with the world.
    pub set_player_permission: unsafe extern "C" fn(world: *mut WorldCtx, player: u64, level: u32),
    /// Adds alternatives (`usage` syntax) to an existing command, handled by `cb` with
    /// `CommandArgs` laid out by this usage; `client` as in `reg_command`.
    pub reg_command_branch: unsafe extern "C" fn(reg: *mut Registrar, command: Str, usage: Str, client: u32, cb: Callback),

    // ------------------------------------------------ text, shaders, particles
    /// Server: particles for `who`.
    pub particle_burst: unsafe extern "C" fn(world: *mut WorldCtx, who: Who, burst: *const ParticleBurst),
    /// The strongest redstone power coming into a cell.
    pub signal_at: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos) -> u32,
    /// Client: particles only this client shows.
    pub client_particle_burst: unsafe extern "C" fn(client: *mut ClientCtx, burst: *const ParticleBurst),

    // ------------------------------------------------ configs, sound, models
    /// Current value of config `id` (bools 0/1, choices their index); 0 for unknown ids.
    pub config_value: unsafe extern "C" fn(id: u32) -> f64,
    /// Changes config `id` (clamped to its range, saved). Synced ones only on the server.
    pub config_set: unsafe extern "C" fn(id: u32, value: f64),
    /// The value of a `TEXT` setting.
    pub config_text: unsafe extern "C" fn(id: u32) -> Str,
    pub config_set_text: unsafe extern "C" fn(id: u32, value: Str),

    /// Server: entity `id` plays action `name` for everyone who sees it; `looping` keeps it going
    /// until another one starts, empty `name` ends it.
    pub action_play: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, name: Str, looping: u32),
    /// Entity `id` plays action `to` wherever it would play `from`; `to` empty ends that.
    pub action_override: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, from: Str, to: Str),
    /// Server: sound event `sound` (`find` with `entry_kind::SOUND`) at `pos` for `who` (`Around`:
    /// the players in its range); `pos` null = not positioned (a UI sound); `pitch` 0 picks one
    /// from the event's range.
    pub play_sound: unsafe extern "C" fn(world: *mut WorldCtx, who: Who, sound: u32, pos: *const Vec3, volume: f32, pitch: f32),
    /// Client: only this client hears it; `pos` null = not positioned (UI).
    pub client_play_sound: unsafe extern "C" fn(client: *mut ClientCtx, sound: u32, pos: *const Vec3, volume: f32, pitch: f32),
    /// Biome id at a block (underground biomes included); 0 where nothing is loaded.
    pub biome_at: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos) -> u32,
    /// Entity `id` walks to `target` by itself along a path around obstacles (a moving target can
    /// be given every tick), at `speed` times its walking speed; 0 stops it. Returns 1 when the
    /// walk was accepted.
    pub navigate: unsafe extern "C" fn(world: *mut WorldCtx, id: u64, target: *const Vec3, speed: f32) -> u32,
    /// Whether `id` is still walking somewhere.
    pub navigating: unsafe extern "C" fn(world: *mut WorldCtx, id: u64) -> u32,
    /// A `blocks` component value's list: (state, weight) pairs.
    pub block_list: unsafe extern "C" fn(index: u32) -> Slice<[u32; 2]>,
    /// Places template `name` with its lowest corner at `pos` once turned by `transform`, its slots
    /// filled by `palette` (empty: its defaults); or feature `name` as if it started at `pos`. Returns
    /// what undoes it for `unplace`, 0 if there is no such thing or part of it isn't loaded
    /// (nothing is placed then).
    pub place: unsafe extern "C" fn(world: *mut WorldCtx, name: Str, pos: *const BlockPos, transform: u32, palette: Str) -> u64,
    /// Puts back what `place` replaced (the last 16 placings of a dimension are kept). 0 if it
    /// is gone.
    pub unplace: unsafe extern "C" fn(world: *mut WorldCtx, placing: u64) -> u32,
    /// A plan puts something (`PlanOp`); 0 where it goes past the feature's reach (left out).
    pub plan_op: unsafe extern "C" fn(ctx: *mut PlanCtx, op: *const PlanOp) -> u32,
    /// A block in a generation snapshot, including this callback's earlier writes.
    pub generation_read: unsafe extern "C" fn(source: *const GenSource, pos: BlockPos) -> u32,
    /// Template `name`'s size once transformed; 0 if there is no such template.
    pub template_size: unsafe extern "C" fn(name: Str, transform: u32, out: *mut BlockPos) -> u32,
    /// Template `name`'s joints once transformed, up to `cap` into `out`; how many it has.
    pub template_joints: unsafe extern "C" fn(name: Str, transform: u32, out: *mut Joint, cap: u32) -> u32,
    /// The world's seed, the one terrain is generated from.
    pub world_seed: unsafe extern "C" fn(world: *mut WorldCtx) -> u64,
    /// The nearest start of feature `name` (one on a grid) within `radius` blocks of `pos`, whether
    /// generated yet or not, in `out`. Returns 0 if none.
    pub structure_near: unsafe extern "C" fn(world: *mut WorldCtx, name: Str, pos: *const BlockPos, radius: i32, out: *mut BlockPos) -> u32,
    /// The block at `pos` falls as an entity and is placed again where it lands; returns the
    /// entity (0 if there was no block).
    pub fall_block: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos) -> u64,

    /// An entity's own box (width, height, eye height) instead of its kind's; `width` <= 0 goes
    /// back to the kind's. Its pose (crouching, swimming) scales whichever applies.
    pub set_entity_size: unsafe extern "C" fn(world: *mut WorldCtx, entity: u64, size: EntitySize),
    /// Draws an entity with another kind's model, texture and animations (it stays what it is);
    /// empty goes back to its own.
    pub set_entity_appearance: unsafe extern "C" fn(world: *mut WorldCtx, entity: u64, kind: Str),
    /// Writes a kind's box (width, height, eye height); 0 if there is no such kind.
    pub entity_kind_size: unsafe extern "C" fn(kind: Str, out: *mut EntitySize) -> u32,
    /// Takes blocks of the world out into a new moving body: `center` is its block
    /// (0, 0, 0), they keep their places around that. Returns its entity, 0 if nothing came.
    pub body_from_blocks: unsafe extern "C" fn(world: *mut WorldCtx, center: *const BlockPos, cells: *const BlockPos, count: usize) -> u64,
    /// A moving body's blocks go back into the world where it is, at the nearest whole block.
    pub body_to_blocks: unsafe extern "C" fn(world: *mut WorldCtx, entity: u64),
    /// A moving body goes to `pos` in exactly `ticks`, whatever it was doing.
    /// A moving body falls under its weight and is put down as blocks where it lands,
    /// like falling sand: what can't stand there drops.
    pub body_fall: unsafe extern "C" fn(world: *mut WorldCtx, entity: u64),
    /// What a piston behind `pos` facing `dir` (`FACE_DIRS` index) would push goes one block
    /// that way, slime and all, as a piston moves it. 0 if it can't.
    pub push_blocks: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos, dir: u32) -> u32,
    /// Every call with this world addresses body `entity`'s blocks, in its coordinates, until
    /// `world_leave_body`; they nest. 0 if it isn't a body here.
    pub world_enter_body: unsafe extern "C" fn(world: *mut WorldCtx, entity: u64) -> u32,
    pub world_leave_body: unsafe extern "C" fn(world: *mut WorldCtx),
    pub body_move_to: unsafe extern "C" fn(world: *mut WorldCtx, entity: u64, pos: *const Vec3, ticks: u32),
    /// What a comparator would read of the block at `pos` may be different now.
    pub signal_read_changed: unsafe extern "C" fn(world: *mut WorldCtx, pos: *const BlockPos),
    /// Column (`x`, `z`) of dimension `dim` for `seed` (its terrain's or `comp::dimension::COLUMN`),
    /// generated or not. Returns 0 if the dimension has neither.
    pub dim_column: unsafe extern "C" fn(dim: u32, seed: u64, x: i32, z: i32, out: *mut TerrainColumn) -> u32,

    /// Puts `player` in camera mode `mode` (`u32::MAX`: back to its own choice); `locked`, it
    /// can't leave it until told. The change takes `transition` seconds eased by `easing`
    /// (NaN: as the mode says).
    /// `t` (0..1) eased by `kind` (`easing::*`).
    pub ease: unsafe extern "C" fn(kind: u32, t: f32) -> f32,
    /// 1 if mod `name`'s (`ns:path`) asset `<kind>/<path>.<ext>` is a file of that mod (not of
    /// a resource pack): `textures`, `png` for a texture.
    pub asset_exists: unsafe extern "C" fn(name: Str, kind: Str, ext: Str) -> u32,
    /// Keeps the cubes from block `from` to `to` loaded for `ticks` more, as `mode`
    /// (`force_load::*`, `OFF` lets them go), under ticket `key`: the same key again moves or
    /// renews it. 0 when `action::FORCE_LOAD` was denied.
    pub force_load:
        unsafe extern "C" fn(world: *mut WorldCtx, key: u64, from: *const BlockPos, to: *const BlockPos, mode: u32, ticks: u32) -> u32,
    pub player_camera: unsafe extern "C" fn(world: *mut WorldCtx, player: u64, mode: u32, locked: u32, transition: f32, easing: u32),
}

/// Where a natural spawn stands. Every kind needs room for the mob's height.
pub mod spawn_place {
    /// On a solid block, in air.
    pub const GROUND: u32 = 0;
    /// Inside a liquid (fish, squid).
    pub const IN_LIQUID: u32 = 1;
    /// Hanging under a solid block, in air (bats).
    pub const CEILING: u32 = 2;
}

/// Light as the world stores it: a byte each for sky, red, green and blue, 0..15.
pub mod light {
    pub const fn sky(l: u32) -> u32 {
        l & 0xFF
    }
    /// The strongest block light channel.
    pub fn block(l: u32) -> u32 {
        ((l >> 8) & 0xFF)
            .max((l >> 16) & 0xFF)
            .max((l >> 24) & 0xFF)
    }
    /// What spawning and growth go by: sky light dimmed by the daylight (0..1), or block light,
    /// whichever is more.
    pub fn effective(l: u32, daylight: f32) -> u32 {
        ((sky(l) as f32 * daylight) as u32).max(block(l)).min(15)
    }
}

/// `UiArg::kind`.
pub mod ui_arg {
    pub const TEXT: u32 = 0;
    /// A translation key.
    pub const KEY: u32 = 1;
    pub const INT: u32 = 2;
    pub const FLOAT: u32 = 3;
    /// An item id, shown as its icon.
    pub const ITEM: u32 = 4;
    /// A display name, `text` like in `{name:..}`: `item:base:coal`, `mod:base`, `key:core:jump`.
    pub const NAME: u32 = 5;
}

/// `UiNode::align`.
pub mod ui_align {
    pub const START: u32 = 0;
    pub const CENTER: u32 = 1;
    pub const END: u32 = 2;
    pub const STRETCH: u32 = 3;
}

/// `UiNode::justify`.
pub mod ui_justify {
    pub const START: u32 = 0;
    pub const CENTER: u32 = 1;
    pub const END: u32 = 2;
    pub const BETWEEN: u32 = 3;
}

/// Debug shapes, the same for `Host::gizmo` (server, for some ticks) and `Host::client_draw`
/// (this frame).
pub mod gizmo_kind {
    /// From `a` to `b`.
    pub const LINE: u32 = 0;
    /// Edges of the box `a`..`b`.
    pub const BOX: u32 = 1;
    /// The box `a`..`b` filled, see-through by the color's alpha.
    pub const FILL: u32 = 2;
}

/// Sources of displayed stacks (`Host::client_displayed`). Slot order and empty slots are preserved.
pub mod display_source {
    pub const HANDS: u32 = 0;
    pub const EQUIPMENT: u32 = 1;
    pub const SHOWN: u32 = 2;
    pub const GROUND: u32 = 3;
}

/// Which hand.
pub mod hand {
    pub const MAIN: u32 = 0;
    pub const OFF: u32 = 1;
}

/// The menu a client has open (`Host::client_menu`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenMenu {
    /// The menu kind's id.
    pub kind: u32,
    /// Which window of the client's this is (the server counts them).
    pub window: u32,
    pub slots: u32,
}

/// A rectangle on the screen: its top left corner and its size.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Tint kinds of block faces: color from the biome. The engine has `core:grass`,
/// `core:foliage` and `core:water`; mods add their own (`entry_kind::TINT_KIND`) and name them
/// in `comp::TINT`.
pub mod tint {
    /// Kinds that fit the face and quad records, the engine's three included.
    pub const MAX_KINDS: u32 = 15;
}

/// Kinds of settings (`comp::config::KIND`).
pub mod config_kind {
    pub const BOOL: u32 = 0;
    pub const INT: u32 = 1;
    pub const FLOAT: u32 = 2;
    /// One of `choices`, stored as its index.
    pub const CHOICE: u32 = 3;
    /// Text (`Host::config_text`); its default is `comp::config::TEXT`. Client scope only.
    pub const TEXT: u32 = 4;
}

/// Where a setting's value lives.
pub mod config_scope {
    /// Each player's own, in their `config/<ns>.txt`.
    pub const CLIENT: u32 = 0;
    /// The server's, in its `config/<ns>.txt`; its players get it, and every change.
    pub const SERVER: u32 = 1;
    /// Saved with the world (game rules, changed with `/rule`); players get it like the server's.
    pub const WORLD: u32 = 2;
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ConditionArgs {
    /// The server's world when the server asks (loot, spawning), else null.
    pub world: *mut WorldCtx,
    /// The client when it asks (music, text), else null.
    pub client: *mut ClientCtx,
    /// Where: the spot, the block, the player.
    pub pos: Vec3,
    /// Who it is about, 0 for nobody: the killer or breaker of loot.
    pub entity: u64,
    /// What is between its parentheses where it is used, `mymod:near(lava)`: `lava`.
    pub arg: Str,
    /// Out: non-zero when the condition holds.
    pub result: u32,
}

/// The engine's conditions, the first entries of `entry_kind::CONDITION` in this order. A
/// condition is written `name` or `name(argument)`, put together with `&`, `|`, `!` and
/// parentheses: `night & light(..7)`; the engine's go without their namespace. Where a
/// component takes conditions (`[str]`) all of them have to hold.
pub mod condition {
    pub const DAY: u32 = 0;
    pub const NIGHT: u32 = 1;
    /// No sky over it.
    pub const UNDERGROUND: u32 = 2;
    pub const SURFACE: u32 = 3;
    /// `light(min..max)`, `light(..7)`, `light(9..)`: the light there is in the range, the
    /// brighter of sky light times daylight and block light (0..15); `light(9.., up)` on that
    /// side of the cell, `light(9.., any)` on its brightest.
    pub const LIGHT: u32 = 4;
    /// `chance(0.05)`: holds that share of the times it is asked; only where the world is.
    pub const CHANCE: u32 = 5;
    /// `block(#grass)`: the block asked about (a block matcher): the one a loot table is rolled
    /// for, the one under a spawn.
    pub const BLOCK: u32 = 6;
    /// `state(age=7)`: that block has these properties.
    pub const STATE: u32 = 7;
    /// `tool(tools/shears)`: what it is broken or killed with has the tag.
    pub const TOOL: u32 = 8;
    /// `tool_level(2)`: and its `tool_level` is at least that.
    pub const TOOL_LEVEL: u32 = 9;
    pub const KILLED_BY_PLAYER: u32 = 10;
    pub const ON_FIRE: u32 = 11;
    /// The block was harvested: broken by the world, or mined with a tool its `harvest_level`
    /// accepts. Block tables made from `drops` have it on their pool.
    pub const HARVESTED: u32 = 12;
    /// The body asked about is in a liquid it swims in (`comp::SWIMS_IN`): at its eyes, or at its
    /// feet once it is low (poses).
    pub const IN_LIQUID: u32 = 13;
    /// It flies (or moves through blocks).
    pub const FLYING: u32 = 14;
    /// It sprints: the sprint action held while moving forward, with the food for it. A pose it
    /// falls back to goes without.
    pub const SPRINTING: u32 = 15;
    /// `attribute(core:can_fly, 1..)`: an attribute of the body asked about is in the range (as
    /// `light`).
    pub const ATTRIBUTE: u32 = 16;
    /// `side(down)`: what is on that side of the cell asked about is solid; `side(down, #soil)`:
    /// a block there is one the matcher takes. Unloaded, it is not known, and a rule waits.
    pub const SIDE: u32 = 17;
    /// `beside(down, #water)`: a block horizontally next to the cell on that side is one the
    /// matcher takes (`beside(#water)`: next to the cell itself).
    pub const BESIDE: u32 = 18;
    /// `open(down)`: that side can be fallen into (air, a liquid, what placing replaces).
    pub const OPEN: u32 = 19;
    /// Something opaque is on top.
    pub const COVERED: u32 = 20;
    /// `sky(15)`, `sky(..7)`: the sky light there, 0..15, whatever the time of day.
    pub const SKY: u32 = 21;
    /// The weather now shelters from the sun (not `comp::weather::UNSHELTERED`): rain.
    pub const SHELTERED: u32 = 22;
    /// `height(..-64)`: the height there, in blocks.
    pub const HEIGHT: u32 = 23;
    /// `wears(head)`: the body asked about wears something over that bone (a helmet keeping the
    /// sun off), or in the equipment slot of that name (`wears(base:feet)`).
    pub const WEARS: u32 = 24;
    pub const NAMES: [&str; 25] = [
        "core:day",
        "core:night",
        "core:underground",
        "core:surface",
        "core:light",
        "core:chance",
        "core:block",
        "core:state",
        "core:tool",
        "core:tool_level",
        "core:killed_by_player",
        "core:on_fire",
        "core:harvested",
        "core:in_liquid",
        "core:flying",
        "core:sprinting",
        "core:attribute",
        "core:side",
        "core:beside",
        "core:open",
        "core:covered",
        "core:sky",
        "core:sheltered",
        "core:height",
        "core:wears",
    ];
}

/// What happens to a particle that a `ParticleRule` answers.
pub mod particle_event {
    /// Came down on top of a block.
    pub const LAND: u32 = 0;
    /// Ran into the side of one.
    pub const WALL: u32 = 1;
    /// Ran into the underside of one.
    pub const CEILING: u32 = 2;
    pub const ENTER_LIQUID: u32 = 3;
    pub const LEAVE_LIQUID: u32 = 4;
    /// Its life is over.
    pub const EXPIRE: u32 = 5;
}

/// Where a `ParticleRule` holds: the block it hit, or for the others the one it is in.
pub mod particle_when {
    pub const ANY: u32 = 0;
    pub const LIQUID: u32 = 1;
    /// A block with the rule's tag.
    pub const TAG: u32 = 2;
}

/// What a `ParticleRule` does.
pub mod particle_then {
    pub const DIE: u32 = 0;
    /// Stays where it is for the rest of its life.
    pub const STOP: u32 = 1;
    /// Loses its speed across what it hit and keeps going along it.
    pub const SLIDE: u32 = 2;
    /// Comes back off what it hit with `amount` of its speed.
    pub const BOUNCE: u32 = 3;
    /// Becomes a particle of kind `into` where it is, keeping its speed, its life starting anew.
    pub const BECOME: u32 = 4;
    /// `BECOME`, standing still.
    pub const BECOME_STILL: u32 = 5;
}

crate::data! {
    /// One way a particle kind answers an event (`comp::particle::RULES`): the first of its rules
    /// for that event whose `when` holds is what happens. Without one, a landing stops it, a wall
    /// makes it slide, the end of its life is the end of it, and going in or out of a liquid does
    /// nothing.
    pub struct ParticleRule {
        /// `particle_event`.
        pub event: u32,
        /// `particle_when`.
        pub when: u32,
        /// `particle_when::TAG`: the tag. At most 5 different ones over all kinds.
        pub tag: alloc::string::String,
        /// `particle_then`.
        pub then: u32,
        /// `BOUNCE`: share of the speed kept.
        pub amount: f32,
        /// `BECOME`: the kind, by name; it must blend the same way (`TRANSLUCENT` or not).
        pub into: alloc::string::String,
    }
}

/// How much a player may do, for commands and anything else that asks: modern's levels. A player
/// may do what asks for its level or less.
pub mod permission {
    /// Everybody.
    pub const EVERYONE: u32 = 0;
    /// May get past protection a mod sets up, no commands of the game.
    pub const MODERATOR: u32 = 1;
    /// May change the game: `/give`, `/tp`, `/time`, `/gamemode`. What a mod's command asks for
    /// unless it says otherwise.
    pub const GAME_MASTER: u32 = 2;
    /// May change the rules and who may do what: `/rule`, `/tick`, `/op`.
    pub const ADMIN: u32 = 3;
    /// The host.
    pub const OWNER: u32 = 4;
}

/// What a thing in the sky is (`comp::sky::KIND`).
pub mod sky_kind {
    /// A picture on an orbit: a sun, a moon.
    pub const BODY: u32 = 0;
    /// Many small points scattered over the sky, turning together.
    pub const STARS: u32 = 1;
    /// The whole sky: a skybox picture or a shader.
    pub const DOME: u32 = 2;
    /// A layer of boxes in the air, where a picture's pixels are filled.
    pub const CLOUDS: u32 = 3;
}

/// Particles of one kind appearing together.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleBurst {
    /// Particle id (`find` with `entry_kind::PARTICLE`).
    pub kind: u32,
    pub pos: Vec3,
    /// Each particle starts up to this far from `pos` on each axis.
    pub spread: Vec3,
    /// Starting velocity in blocks per second.
    pub velocity: Vec3,
    /// Random extra velocity up to this, in any direction.
    pub speed: f32,
    /// Non-zero: each flies away from the middle of the burst, more or less, like the pieces of a
    /// broken block, instead of in any direction.
    pub outward: u32,
    pub count: u32,
    /// Block state the textureless kinds take their texture from.
    pub block: u32,
    /// 0xRRGGBB the kind's color is multiplied by (redstone dust, a potion's color).
    pub color: u32,
}
defaults!(ParticleBurst {
    kind: 0,
    pos: Vec3::ZERO,
    spread: Vec3::ZERO,
    velocity: Vec3::ZERO,
    speed: 0.0,
    outward: 0,
    count: 1,
    block: 0,
    color: 0xFFFFFF
});

/// Particle kinds the engine itself uses.
pub mod particle {
    /// Pieces of a block's texture (breaking, landing).
    pub const BLOCK: &str = "core:block";
    /// Smoke puffs (a mob dying).
    pub const POOF: &str = "core:poof";
    /// Sparks off a critical hit.
    pub const CRIT: &str = "core:crit";
    /// A flame's tip (a torch, a lit furnace).
    pub const FLAME: &str = "core:flame";
    /// A thin rising smoke.
    pub const SMOKE: &str = "core:smoke";
    /// Rising through water.
    pub const BUBBLE: &str = "core:bubble";
    /// Thrown up when something enters water.
    pub const SPLASH: &str = "core:splash";
    /// Falling from a ceiling with water above it.
    pub const DRIP: &str = "core:drip";
    /// Swirls around who has an entity effect with a color, tinted by it.
    pub const EFFECT: &str = "core:effect";
}

/// Values are copied out before the call and written back after it (changes of synced
/// components get sent), so they form one dense, aligned array.
#[repr(C)]
pub struct SystemArgs {
    pub world: *mut WorldCtx,
    pub component: u32,
    pub ids: *const u64,
    pub values: *mut u8,
    pub count: u32,
    pub stride: u32,
    /// Per entity, the ticks this call stands for: 1, or more while it catches up on time it
    /// missed (loaded again, the world's time jumped).
    pub ticks: *const u32,
}

/// Engine events (`Host::reg_event_handler`); values are the structs named in each line.
pub mod event {
    /// `[i32; 3]`, the block: its tick (liquids flowing, block rules, `block.on_scheduled`), when a
    /// timer on a block with this event is due (`Host::timer`).
    pub const BLOCK_TICK: &str = "core:block_tick";
    /// `PlayerEvent`.
    pub const PLAYER_JOINED: &str = "core:player_joined";
    /// `PlayerEvent`: a dead player came back, at the spawn point with fresh components (but the
    /// `KEEP_ON_RESPAWN` ones), before anyone saw them there; a mod may move them on (a bed).
    pub const PLAYER_RESPAWNED: &str = "core:player_respawned";
    /// `Hurt` with what was healed as `amount`: health went up (after every `HEAL` modifier).
    pub const ENTITY_HEALED: &str = "core:entity_healed";
    /// `WeatherTick`: every tick in each dimension with weather, what the weather is.
    pub const WEATHER_TICK: &str = "core:weather_tick";
    /// `Removed`: an entity left the world, and why.
    pub const ENTITY_REMOVED: &str = "core:entity_removed";
    /// `KeyChange`: a player pressed or let go of an action (any key binding, the mouse buttons'
    /// `core:use` and `core:attack` too) while the world had the input.
    pub const KEY: &str = "core:key";
    /// `EffectChange`: an effect ran out or was taken off.
    pub const EFFECT_ENDED: &str = "core:effect_ended";
    /// `FeaturePlaced`: a feature on a grid stands in the world now, its start's cube generated
    /// (a village to put on a map, its guards to name).
    pub const FEATURE_PLACED: &str = "core:feature_placed";
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FeaturePlaced {
    /// The feature's id (`Host::find` with `entry_kind::FEATURE`).
    pub feature: u32,
    pub dim: u32,
    /// Its start.
    pub pos: BlockPos,
    /// How its template was placed (`transform`), 0 for a plan.
    pub transform: u32,
}
impl Schema for FeaturePlaced {
    const SCHEMA: &'static str = "{feature:u32,dim:u32,pos:[i32;3],transform:u32}";
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeatherTick {
    pub dim: u32,
    pub weather: u32,
}
impl Schema for WeatherTick {
    const SCHEMA: &'static str = "{dim:u32,weather:u32}";
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interaction {
    pub player: u64,
    pub entity: u64,
    pub item: u32,
    pub pad: u32,
}
impl Schema for Interaction {
    const SCHEMA: &'static str = "{player:u64,entity:u64,item:u32,pad:u32}";
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DimensionChange {
    pub entity: u64,
    pub from: u32,
    pub to: u32,
}
impl Schema for DimensionChange {
    const SCHEMA: &'static str = "{entity:u64,from:u32,to:u32}";
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemUsed {
    pub entity: u64,
    /// As it was before the use.
    pub stack: ItemStack,
    pub pos: BlockPos,
    /// 1 when used on the block at `pos`.
    pub has_target: u32,
}
impl Schema for ItemUsed {
    const SCHEMA: &'static str = "{entity:u64,stack:{item:u32,count:u32,data:u32},pos:[i32;3],has_target:u32}";
}

#[repr(C)]
pub struct WorldTickArgs {
    pub world: *mut WorldCtx,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hurt {
    pub entity: u64,
    pub source: u64,
    /// What hit, when not `source` itself (an arrow); 0 otherwise.
    pub direct: u64,
    pub amount: f32,
    pub cause: u32,
    pub crit: u32,
    /// The part of its hitbox it landed on (`hitbox_part`), 0 for none in particular.
    pub part: u32,
}
impl Schema for Hurt {
    const SCHEMA: &'static str = "{entity:u64,source:u64,direct:u64,amount:f32,cause:u32,crit:u32,part:u32}";
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockChange {
    pub player: u64,
    pub pos: BlockPos,
    pub state: u32,
    pub item: u32,
}
impl Schema for BlockChange {
    const SCHEMA: &'static str = "{player:u64,pos:[i32;3],state:u32,item:u32}";
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Death {
    pub entity: u64,
    pub source: u64,
    pub kind: u32,
    pub cause: u32,
    pub pos: Vec3,
}
impl Schema for Death {
    const SCHEMA: &'static str = "{entity:u64,killer:u64,kind:u32,cause:u32,pos:[f64;3]}";
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerEvent {
    pub player: u64,
}
impl Schema for PlayerEvent {
    const SCHEMA: &'static str = "{player:u64}";
}

/// A chat line's most: 256 characters of 4 bytes.
pub const CHAT_BYTES: usize = 1024;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChatMessage {
    pub player: u64,
    pub len: u32,
    pub bytes: [u8; CHAT_BYTES],
}
impl Schema for ChatMessage {
    const SCHEMA: &'static str = "{player:u64,len:u32,bytes:[u8;1024]}";
}
impl ChatMessage {
    pub fn new(player: u64, text: &str) -> ChatMessage {
        let mut end = text.len().min(CHAT_BYTES);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut bytes = [0; CHAT_BYTES];
        bytes[..end].copy_from_slice(&text.as_bytes()[..end]);
        ChatMessage { player, len: end as u32, bytes }
    }
    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.bytes[..(self.len as usize).min(CHAT_BYTES)]).unwrap_or("")
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pickup {
    pub entity: u64,
    pub stack: ItemStack,
}
impl Schema for Pickup {
    const SCHEMA: &'static str = "{entity:u64,stack:{item:u32,count:u32,data:u32}}";
}

#[repr(C)]
pub struct EventArgs {
    pub world: *mut WorldCtx,
    pub event: u32,
    pub data: *const u8,
    pub count: u32,
    /// Bytes per value.
    pub stride: u32,
}

/// The engine's actions: each name is both the question asked before (a chain of yes or no,
/// `Host::ask`) and the event sent after, with the value named in its doc.
pub mod action {
    /// `BlockChange`. Asked with `entity` the player (0 for `break_block` without one), `pos`,
    /// `state`, `item` the tool.
    pub const BREAK_BLOCK: &str = "core:break_block";
    /// `BlockChange` (`item` the item used). Asked with `entity`, `pos`, `state` to be placed, `item`.
    pub const PLACE_BLOCK: &str = "core:place_block";
    /// `BlockChange` (`item` what it held). A right click on a block: `entity`, `pos`, `state`.
    pub const USE_BLOCK: &str = "core:use_block";
    /// `ChatMessage`. A player (`entity`) about to say `text` in the chat. Commands don't ask.
    pub const CHAT: &str = "core:chat";
    /// `DimensionChange`. `entity` about to change dimension to `dim`.
    pub const ENTER_DIMENSION: &str = "core:enter_dimension";
    /// `Pickup`. `entity` (what has `comp::entity::PICKS_UP`) picking up `item` (count in
    /// `amount`), `other` the item entity.
    pub const PICKUP_ITEM: &str = "core:pickup_item";
    /// `Hurt`, sent after armor and every modifier. Asked with `entity` about to be hurt by
    /// `other` for `amount` of `cause`, before the damage chain.
    pub const HURT: &str = "core:hurt";
    /// `Interaction`. A player (`other`) right clicking `entity` holding `item`.
    pub const INTERACT_ENTITY: &str = "core:interact_entity";
    /// `ItemUsed`, sent when something happened. A player (`entity`) using the held `item`, at
    /// block `pos` when `state` is not 0.
    pub const USE_ITEM: &str = "core:use_item";
    /// `Death`. `entity` about to die of `amount` from `other` and `cause`. Denied, it lives on
    /// with one health (a totem); what saves it is the answering step's to do.
    pub const DIE: &str = "core:die";
    /// `Mounting`. `entity` getting onto `other` in seat `amount`.
    pub const MOUNT: &str = "core:mount";
    /// `Mounting`. `entity` getting off `other` (`amount` its seat).
    pub const DISMOUNT: &str = "core:dismount";
    /// `Query`: player `entity` turning on camera mode `amount` (its id), one with conditions
    /// (`comp::camera::WHEN`). With nobody allowing or denying, it may where they hold.
    pub const CAMERA: &str = "core:camera";
    /// `Query`: cubes from `pos` kept loaded (`Host::force_load`), `amount` the mode, `other`
    /// the ticket's key.
    pub const FORCE_LOAD: &str = "core:force_load";
    /// `Spawned`. Asked with `entity` made but not yet seen, `cause` (`spawn_cause`), `pos`:
    /// no, and it never was. Moving blocks and block entities are blocks: their comings and goings
    /// are the blocks', neither asked here nor told as `event::ENTITY_REMOVED`.
    pub const SPAWN: &str = "core:spawn";
    /// `EffectChange`. Asked with `entity` about to get effect `cause` at level `amount` for
    /// `state` ticks; vanilla's immunities are tags `immune_effect/<effect>` on the kind.
    pub const EFFECT: &str = "core:effect";
    /// `ProjectileHit`. Asked with `entity` the projectile, `other` what it hits (0: the block at
    /// `pos`), `amount` its speed in blocks a tick: no, and it flies on past them.
    pub const PROJECTILE_HIT: &str = "core:projectile_hit";
}

pub mod question {
    /// `entity` attacking `other` with `item`.
    pub const ATTACK: &str = "core:attack";
    /// Whether `entity`'s melee hit on `other` with `item`, charged `charge`, is a critical one;
    /// from no: `core:vanilla_crit` (`BASE`: full charge, falling, not on the ground, in a liquid,
    /// on a ladder or sprinting).
    pub const CRIT: &str = "core:crit";
    /// Whether the hit also sweeps those beside `other`; from no: `core:vanilla_sweep` (`BASE`:
    /// full charge with an item with `sweeps`, on the ground, not sprinting, not a crit).
    pub const SWEEP: &str = "core:sweep";
    /// Whether `entity` is still recovering from its last hurt, so that of this one only what goes
    /// beyond the last lands, and it doesn't knock; from no: `core:hurt_cooldown` (`BASE`: its
    /// `attribute::HURT_COOLDOWN` hasn't run out, and `cause` doesn't ignore it).
    pub const RECOVERING: &str = "core:recovering";
}

/// What a question or a value chain is about; fields one does not use are 0.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Query {
    pub entity: u64,
    pub other: u64,
    /// What hit, when not `other` itself: the arrow `other` shot.
    pub direct: u64,
    pub pos: BlockPos,
    pub state: u32,
    pub item: u32,
    pub dim: u32,
    pub cause: u32,
    pub amount: f64,
    /// A melee hit's: how charged its swing was (0..1) and whether it is a critical one.
    pub charge: f32,
    pub crit: u32,
    /// The part of `entity`'s hitbox a hurt lands on (`hitbox_part`), 0 for none in particular.
    pub part: u32,
    /// Valid for the call.
    pub text: Str,
}
defaults!(Query {
    entity: 0,
    other: 0,
    direct: 0,
    pos: BlockPos::ZERO,
    state: 0,
    item: 0,
    dim: 0,
    cause: 0,
    amount: 0.0,
    charge: 0.0,
    crit: 0,
    part: 0,
    text: Str::EMPTY
});

/// A hurt described (`Host::entity_damage`): `amount` of `cause` from `source` (who; 0 for
/// nothing), through `direct` (what hit: the arrow; 0 for `source` itself), with `item` (what
/// `source` hit with), a critical one or not. `knock` the way it pushes (0: away from `direct`, else
/// `source`). `part` the part of the hitbox it lands on (`hitbox_part`, 0 none in particular).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Damage {
    pub amount: f32,
    pub cause: u32,
    pub source: u64,
    pub direct: u64,
    pub item: ItemStack,
    pub knock: Vec3,
    pub crit: u32,
    pub part: u32,
}
impl Damage {
    /// `amount` of `cause` from nobody.
    pub const fn new(amount: f32, cause: u32) -> Damage {
        Damage { amount, cause, ..Damage::DEFAULT }
    }
}
defaults!(Damage { amount: 0.0, cause: 0, source: 0, direct: 0, item: ItemStack::EMPTY, knock: Vec3::ZERO, crit: 0, part: 0 });

/// The engine's phases, in the order they run; a mod's goes after one of them
/// (`comp::phase::AFTER`). Numbers usually use `ADD`, `PERCENT`, `MULTIPLY` and `OVERRIDE`;
/// questions `RULES` and `OVERRIDE`; `FINAL` is for what has to see the finished value.
pub mod phase {
    /// What the thing is: a weapon's own damage, a set base.
    pub const BASE: &str = "core:base";
    pub const ADD: &str = "core:add";
    /// Shares of the value this phase started from, added up (vanilla's percent modifiers).
    pub const PERCENT: &str = "core:percent";
    pub const MULTIPLY: &str = "core:multiply";
    /// Where questions are answered by default.
    pub const RULES: &str = "core:rules";
    pub const OVERRIDE: &str = "core:override";
    pub const FINAL: &str = "core:final";
    /// All of them in order.
    pub const ALL: [&str; 7] = [BASE, ADD, PERCENT, MULTIPLY, RULES, OVERRIDE, FINAL];
}

/// The engine's steps that take a number (`StepArgs::arg`), for modifiers: `ADD` adds it
/// (phase `ADD`), `MUL` multiplies by it (`MULTIPLY`), `SET` makes the value it (`OVERRIDE`),
/// `PERCENT` adds that share of the value the phase started from (`PERCENT`).
pub mod step {
    pub const ADD: &str = "core:add";
    pub const MUL: &str = "core:mul";
    pub const SET: &str = "core:set";
    pub const PERCENT: &str = "core:percent";
}

/// Value chains the engine runs.
pub mod chain {
    /// Damage an entity takes (`entity` hurt by `other` through `direct`, `cause`, `crit`):
    /// what it resists comes off last, `core:resistance` then `core:not_lethal` (`FINAL`).
    pub const DAMAGE: &str = "core:damage";
    /// What a melee hit of `entity` on `other` with `item` does, from its `attribute::ATTACK_DAMAGE`
    /// against them: `core:charge` (`MULTIPLY`, 0.2 + 0.8 x `charge` squared) and `core:crit`
    /// (`MULTIPLY`, times `attribute::CRIT_DAMAGE` when `crit`).
    pub const MELEE: &str = "core:melee";
    /// How hard a hurt knocks `entity` away (0.4, vanilla's), before its knockback resistance.
    pub const KNOCKBACK: &str = "core:knockback";
    /// What a sweep does to each one beside the target, from the hit's damage: `core:sweep_damage`
    /// (`BASE`, 1 + `attribute::SWEEPING_DAMAGE_RATIO` of it).
    pub const SWEEP_DAMAGE: &str = "core:sweep_damage";
    /// Health an entity gains (`entity` healed by `other`, `cause`).
    pub const HEAL: &str = "core:heal";
    /// What projectile `entity` does to `other` it hits, from its `Projectile::damage`:
    /// `core:by_speed` (`BASE`, times `amount`, its speed in blocks a tick, rounded up).
    pub const PROJECTILE_DAMAGE: &str = "core:projectile_damage";
    /// What a fall of `amount` blocks does to `entity`, from that distance: `core:safe_fall`
    /// (`BASE`, past its `SAFE_FALL`, times its `FALL_DAMAGE_MULTIPLIER`; none for a kind that
    /// takes no fall damage). A soft block is a step reading what is under it.
    pub const FALL_DAMAGE: &str = "core:fall_damage";
}

/// One step's run: what the value is about, where it began (`base`) and where this phase began
/// (`start`), the value to change, and the number a modifier gave the step (`arg`, 0 on a chain).
/// A question's value is 1 for yes and 0 for no.
#[repr(C)]
pub struct StepArgs {
    pub world: *mut WorldCtx,
    pub chain: Str,
    pub ctx: *const Query,
    pub base: f64,
    pub start: f64,
    /// In/out.
    pub value: f64,
    pub arg: f64,
}

/// Attributes the engine reads.
pub mod attribute {
    pub const MAX_HEALTH: &str = "core:max_health";
    /// Multiplier on walking speed (1 = the kind's speed).
    pub const MOVEMENT_SPEED: &str = "core:movement_speed";
    /// Multiplier on how far mobs notice it (1; 0 in creative, so mobs ignore the player; a mod
    /// can lower it, like vanilla's mob heads).
    pub const VISIBILITY: &str = "core:visibility";
    /// Multiplier on jump speed (1: a block and a quarter high).
    pub const JUMP_STRENGTH: &str = "core:jump_strength";
    /// Multiplier on how fast a swing recharges.
    pub const ATTACK_SPEED: &str = "core:attack_speed";
    /// Below 1: can't sprint (the base game while too hungry).
    pub const CAN_SPRINT: &str = "core:can_sprint";
    /// How far a player reaches blocks (4.5; 5 in creative).
    pub const BLOCK_REACH: &str = "core:block_reach";
    /// How far a player reaches entities (3; 5 in creative).
    pub const ENTITY_REACH: &str = "core:entity_reach";
    /// Multiplier on how fast a player mines (1).
    pub const MINING_SPEED: &str = "core:mining_speed";
    /// Multiplier on a player's field of view (1), on top of the setting and of flying and
    /// sprinting: a zoom, a speed rush.
    pub const FOV: &str = "core:fov";
    /// Whether it may fly, jumping twice (0; 1 in creative): a flying ring is a modifier.
    pub const CAN_FLY: &str = "core:can_fly";
    /// 1: takes no damage, hunger or air (creative, a block entity, a shield effect) but from
    /// causes that pierce it (the void); mods read it as `World::outside_survival`.
    pub const INVULNERABLE: &str = "core:invulnerable";
    /// Blocks it falls without harm (3; jump boost raises it, slow falling takes the harm away).
    pub const SAFE_FALL: &str = "core:safe_fall";
    /// Damage per block fallen past `SAFE_FALL` (1).
    pub const FALL_DAMAGE_MULTIPLIER: &str = "core:fall_damage_multiplier";
    /// Blocks it walks up without jumping (0.6).
    pub const STEP_HEIGHT: &str = "core:step_height";
    /// Multiplier on its dimension's gravity (1; slow falling 0.125, below 0 floats up).
    pub const GRAVITY: &str = "core:gravity";
    /// Share of knockback it shrugs off (0..1).
    pub const KNOCKBACK_RESISTANCE: &str = "core:knockback_resistance";
    /// Its own melee damage, as vanilla's (1; a kind's `.attack_damage(..)`), what it holds adding
    /// to it (`core:weapon`, `ADD`: a 6 sword's 5 over a bare hand), strength +3 a level; what a
    /// hit does before its charge and crit (`chain::MELEE`).
    pub const ATTACK_DAMAGE: &str = "core:attack_damage";
    /// What a critical hit multiplies by (1.5).
    pub const CRIT_DAMAGE: &str = "core:crit_damage";
    /// Share of a hit's damage a sweep adds to its 1 (0; vanilla's sweeping edge).
    pub const SWEEPING_DAMAGE_RATIO: &str = "core:sweeping_damage_ratio";
    /// Extra knockback of its melee hits (0; a full charged sprinting hit 1 more, `core:sprint_knockback`).
    pub const ATTACK_KNOCKBACK: &str = "core:attack_knockback";
    /// Ticks after a hurt in which a weaker one doesn't land (10).
    pub const HURT_COOLDOWN: &str = "core:hurt_cooldown";
    /// Multiplier on how much it pushes itself in the air (1): a light mob that steers well.
    pub const AIR_CONTROL: &str = "core:air_control";
    /// Multiplier on how slippery the ground under it is (1, at most nearly 1 a tick kept): skates.
    pub const SLIPPERINESS: &str = "core:slipperiness";
    /// Multiplier on how fast it goes up and down ladders and vines (1).
    pub const CLIMB_SPEED: &str = "core:climb_speed";
    /// Share of damage taken off, after armor and the `DAMAGE` chain (0..1; resistance 0.2 a
    /// level). What pierces invulnerability gets through.
    pub const DAMAGE_RESISTANCE: &str = "core:damage_resistance";
    /// Share of burning damage (`damage_cause::BURNING`) taken off (0..1; fire resistance 1).
    pub const FIRE_RESISTANCE: &str = "core:fire_resistance";
    /// Multiplier on how long what sets it on fire keeps it burning (1).
    pub const BURNING_TIME: &str = "core:burning_time";
    /// Multiplier on mining speed with its head in a liquid (0.2).
    pub const SUBMERGED_MINING_SPEED: &str = "core:submerged_mining_speed";
    /// Multiplier on mining speed off the ground (0.2).
    pub const AIRBORNE_MINING_SPEED: &str = "core:airborne_mining_speed";
    /// How much its player sees as if everything were under the noon sky, lamps still adding
    /// their warmth (0..1; the night vision effect 1).
    pub const NIGHT_VISION: &str = "core:night_vision";
    /// Multiplier on how fast it moves itself in liquids (1).
    pub const SWIM_SPEED: &str = "core:swim_speed";
    /// Multiplier on how fast it flies (1), on top of a player's own flight speed (the wheel).
    pub const FLYING_SPEED: &str = "core:flying_speed";
}

crate::data! {
    /// One weighted choice of a pool: `item` (name, or `#tag` for the tag's first item; empty = no
    /// item), `min..=max` of it. A burning victim drops it cooked by recipe kind `smelt` if set.
    pub struct LootEntry {
        pub item: alloc::string::String,
        pub weight: u32,
        pub min: u32,
        pub max: u32,
        /// Conditions (`condition`) that all hold.
        pub when: alloc::vec::Vec<alloc::string::String>,
        pub smelt: alloc::string::String,
    }
}

crate::data! {
    /// Rolls `rolls_min..=rolls_max` times if its conditions pass; each roll picks one entry by
    /// weight among those whose conditions pass.
    pub struct LootPool {
        pub rolls_min: u32,
        pub rolls_max: u32,
        /// Conditions (`condition`) that all hold.
        pub when: alloc::vec::Vec<alloc::string::String>,
        pub entries: alloc::vec::Vec<LootEntry>,
    }
}

crate::data! {
    /// Every entry of item `name` in a table's pools gives `to` instead; an empty `to` leaves them
    /// out.
    pub struct LootSwap { pub name: alloc::string::String, pub to: alloc::string::String }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LootCtx {
    pub tool: ItemStack,
    pub killer: u64,
    pub by_player: u32,
    pub on_fire: u32,
    /// The block state being mined with `tool` (0 = none), for `condition::HARVESTED`.
    pub mined: u32,
}
defaults!(LootCtx { tool: ItemStack::EMPTY, killer: 0, by_player: 0, on_fire: 0, mined: 0 });

/// Flags of stack components (`comp::component::STACK_FLAGS`).
pub mod stack_flags {
    /// Kept on the server: clients get the stack without it.
    pub const SERVER_ONLY: u32 = 1;
    /// Not written to saves.
    pub const NO_SAVE: u32 = 2;
    /// Read by an item's conditional look. Only public visual data participates in its cache.
    pub const VISUAL: u32 = 4;
}

// ---------------------------------------------------------------- client side

#[repr(C)]
pub struct ClientFrameArgs {
    pub client: *mut ClientCtx,
}

/// `comp::item::CONDITION`: the model and the texture `stack` shows as, each left
/// empty for its usual.
#[repr(C)]
pub struct ItemModelArgs {
    pub stack: ItemStack,
    pub model: Str,
    pub texture: Str,
    pub worn_model: Str,
    pub worn_texture: Str,
}

/// One entity on the client, for `comp::entity::FRAMES`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ClientEntityArgs {
    pub client: *mut ClientCtx,
    pub entity: u64,
    pub kind: u32,
    pub pos: Vec3,
    pub yaw: f32,
    /// Distance walked so far (it drives the limbs); steps go by how it grows.
    pub walked: f32,
    /// Seconds into its death animation (1 = over, its last call), negative while alive.
    pub dying: f32,
    /// Seconds since the client first saw it.
    pub age: f32,
    /// Seconds since the last call.
    pub dt: f32,
}

#[repr(C)]
pub struct PacketArgs {
    pub world: *mut WorldCtx,
    pub player: u64,
    pub data: Slice<u8>,
}

#[repr(C)]
pub struct ClientPacketArgs {
    pub client: *mut ClientCtx,
    pub data: Slice<u8>,
}

pub mod client_flags {
    /// A world is open (local or joined).
    pub const IN_WORLD: u32 = 1;
    /// The server accepted the player.
    pub const JOINED: u32 = 2;
    /// The world around the player is loaded and meshed.
    pub const LOADED: u32 = 4;
    /// Nothing left to generate, light, send or mesh (local server only).
    pub const IDLE: u32 = 8;
    /// The top screen is laid out and was drawn with its textures.
    pub const SCREEN_READY: u32 = 16;
    pub const DEAD: u32 = 32;
    pub const FLYING: u32 = 64;
    pub const ON_GROUND: u32 = 128;
    pub const MENU_OPEN: u32 = 256;
    /// Hosting and the join ticket is known.
    pub const HOSTING: u32 = 512;
    /// The F3 debug overlay is shown.
    pub const DEBUG: u32 = 1024;
    /// Background music is playing.
    pub const MUSIC: u32 = 2048;
    /// Single player stands still: an open screen pauses it.
    pub const PAUSED: u32 = 4096;
}

/// What a client callback can see of the local client, filled by `Host::client_state`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ClientState {
    pub flags: u32,
    pub frame: u64,
    /// Seconds since the previous frame.
    pub dt: f32,
    /// Averages over the last half second.
    pub fps: f32,
    pub frame_ms: f32,
    /// GPU time of the last measured frame and the server's last tick, milliseconds.
    pub gpu_ms: f32,
    pub server_ms: f32,
    pub window: [u32; 2],
    pub gui_scale: f32,
    pub dim: u32,
    pub player: u64,
    pub pos: Vec3,
    pub vel: Vec3,
    /// Radians, like `EntityState`.
    pub yaw: f32,
    pub pitch: f32,
    pub selected: u32,
    /// 0 survival, 1 creative, 2 spectator.
    pub mode: u32,
    pub health: f32,
    /// Loaded cubes, cubes waiting for a mesh, known entities.
    pub cubes: u32,
    pub pending_meshes: u32,
    pub entities: u32,
    /// Top screen (`core:menu`, `core:pause`, ...), empty in the world; valid during the call.
    pub screen: Str,
    /// Frames since the top screen changed.
    pub screen_frames: u32,
    /// Local server load totals: cubes generated and their average microseconds on a worker,
    /// cubes inserted and their average on the server thread.
    pub generated: u64,
    pub gen_us: f32,
    pub inserted: u64,
    pub insert_us: f32,
    /// Finished `client_op::BENCH` runs, and the numbers of the last one.
    /// How much walking shakes the view now, 0 standing, in the air or swimming.
    pub bob: f32,
    pub benches: u32,
    pub bench_fps: f32,
    pub bench_avg_ms: f32,
    pub bench_p99_ms: f32,
    pub bench_gpu_ms: f32,
    pub bench_server_ms: f32,
    /// Particles alive on this client.
    pub particles: u32,
    /// Sounds started since the game started (0 without a sound device).
    pub sounds: u32,
    /// Where the player is drawn this frame (between physics ticks), unlike `pos`.
    pub shown: Vec3,
    /// Columns distant terrain takes from saved cubes, not the terrain (`ToClient::LodColumns`).
    pub lod_columns: u32,
    /// The camera mode on (its id) and where the camera is this frame.
    pub camera: u32,
    pub camera_pos: Vec3,
    /// The block looked at and the structure (entity) it is part of, 0 for the world;
    /// `target_body` `u64::MAX` when nothing is looked at.
    pub target: BlockPos,
    pub target_body: u64,
    /// Cubes whose terrain the GPU made, and how many of them differed from the CPU's (counted
    /// with `core:terrain_check` on).
    pub gpu_cubes: u64,
    pub gpu_mismatches: u64,
    /// Distant terrain tiles made and shown, and asked for and not back yet.
    pub lod_tiles: u32,
    pub lod_pending: u32,
}
defaults!(ClientState {
    lod_columns: 0,
    camera: 0,
    camera_pos: Vec3 { x: 0.0, y: 0.0, z: 0.0 },
    flags: 0,
    frame: 0,
    dt: 0.0,
    fps: 0.0,
    frame_ms: 0.0,
    gpu_ms: 0.0,
    server_ms: 0.0,
    window: [0; 2],
    gui_scale: 1.0,
    dim: 0,
    player: 0,
    pos: Vec3::ZERO,
    vel: Vec3::ZERO,
    yaw: 0.0,
    pitch: 0.0,
    selected: 0,
    mode: 0,
    health: 0.0,
    cubes: 0,
    pending_meshes: 0,
    entities: 0,
    screen: Str::EMPTY,
    screen_frames: 0,
    generated: 0,
    gen_us: 0.0,
    inserted: 0,
    insert_us: 0.0,
    bob: 0.0,
    benches: 0,
    bench_fps: 0.0,
    bench_avg_ms: 0.0,
    bench_p99_ms: 0.0,
    bench_gpu_ms: 0.0,
    bench_server_ms: 0.0,
    particles: 0,
    sounds: 0,
    shown: Vec3::ZERO,
    target: BlockPos::ZERO,
    target_body: u64::MAX,
    gpu_cubes: 0,
    gpu_mismatches: 0,
    lod_tiles: 0,
    lod_pending: 0,
});

/// `Host::client_input` operations. Keys and buttons are held for `x` seconds (0 = one tap).
pub mod client_input {
    /// `text`: an action (`core:jump`, `mymod:jetpack`), pressed with its bound key.
    pub const ACTION: u32 = 0;
    /// `text`: a key by its settings name (`Tab`, `Enter`, `KeyE`, `F3`).
    pub const KEY: u32 = 1;
    /// Mouse button `y` (0 left, 1 right, 2 middle).
    pub const CLICK: u32 = 2;
    /// Moves the mouse to (`x`, `y`) as fractions of the window.
    pub const CURSOR: u32 = 3;
    /// Mouse wheel by `x` steps (negative = down).
    pub const SCROLL: u32 = 4;
    /// `text` typed into the focused field.
    pub const TYPE: u32 = 5;
    pub const RELEASE_ALL: u32 = 6;
}

/// Fields of `PlayerControl` to apply.
pub mod control_mask {
    pub const POS: u32 = 1;
    pub const VEL: u32 = 2;
    pub const LOOK: u32 = 4;
    pub const FLYING: u32 = 8;
    pub const FLY_SPEED: u32 = 16;
    pub const SELECTED: u32 = 32;
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PlayerControl {
    pub mask: u32,
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub flying: u32,
    /// Spectator/creative flight speed multiplier.
    pub fly_speed: f32,
    pub selected: u32,
}
defaults!(PlayerControl { mask: 0, pos: Vec3::ZERO, vel: Vec3::ZERO, yaw: 0.0, pitch: 0.0, flying: 0, fly_speed: 1.0, selected: 0 });

/// `Host::client_op` operations.
pub mod client_op {
    /// A chat line or `/command` as if typed.
    pub const CHAT: u32 = 0;
    /// Captures the next frame into `text` (a path; empty = `screenshots/<time>.png`, a bare
    /// name goes into `screenshots/`). Returns the image id for `image_pixels`.
    pub const SCREENSHOT: u32 = 1;
    /// Leaves the game with exit code `num`.
    pub const QUIT: u32 = 2;
    /// Opens world `text` (created with seed `num` if missing).
    pub const OPEN_WORLD: u32 = 3;
    /// Like `OPEN_WORLD`, deleting a save of that name first.
    pub const NEW_WORLD: u32 = 4;
    pub const LEAVE_WORLD: u32 = 5;
    /// Starts hosting the open world; the ticket appears in `client_text::TICKET`.
    pub const HOST: u32 = 6;
    /// Joins the host with ticket `text`.
    pub const JOIN: u32 = 7;
    /// `core:pause`, `core:settings`, `core:language`, `core:controls`, `core:worlds`,
    /// `core:create_world`, `core:title`, `core:chat`, `core:inventory`, `core:mod_settings`.
    pub const OPEN_SCREEN: u32 = 8;
    /// Closes the top screen (and its menu).
    pub const CLOSE_SCREEN: u32 = 9;
    pub const RELOAD_ASSETS: u32 = 10;
    /// Frame, GPU and server statistics over `num` seconds, logged as a `BENCH` line labeled
    /// `text`; the numbers land in `ClientState::bench_*`.
    pub const BENCH: u32 = 11;
    pub const RESPAWN: u32 = 12;
    /// Post-processing effect `text` (`shaders/post/<name>.wgsl`) at strength `num`, 0 turns it
    /// off. Effects stay on until changed and run in the order they were turned on.
    /// Runs command `text` (`/tp ~ ~5 ~`) as the player, here if it is a client command, else
    /// on the server; returns an id for `Host::client_command_reply`. The chat stays out of it.
    pub const RUN_COMMAND: u32 = 16;
    /// Sorts the group of slot `num` of the open menu, items of a kind together and stacked;
    /// `num` below 0: what the menu is opened on (a chest), else the player's main inventory.
    /// Only storage is sorted, never inputs, outputs or slots for some items only.
    pub const MENU_SORT: u32 = 17;
}

/// `Host::client_text` sources.
pub mod client_text {
    /// Join ticket while hosting.
    pub const TICKET: u32 = 0;
    /// The game's `run` directory.
    pub const RUN_DIR: u32 = 1;
    /// Chat line `index` counted from the newest (0).
    pub const CHAT: u32 = 2;
    /// The last `BENCH` line.
    pub const BENCH: u32 = 3;
    /// Text of the top screen's focused input field (chat, search).
    pub const INPUT: u32 = 4;
}

crate::data! {
    /// What a vein looks like (`comp::feature::VEIN`): `kind` (`vein_shape`), the `radius` of a
    /// sphere or a disk and the `thickness` of a disk.
    pub struct VeinShape { pub kind: u32, pub radius: f32, pub thickness: f32 }
}

crate::data! {
    /// How starts spread over a feature's heights (`comp::feature::HEIGHT`): `curve`
    /// (`height_curve`), and how many blocks in the middle a trapezoid is at its most.
    pub struct HeightCurve { pub curve: u32, pub plateau: i32 }
}

crate::data! {
    /// Where a vein meets what `name` takes (a block matcher), `block` goes there instead (granite
    /// iron ore in granite).
    pub struct Variant { pub name: alloc::string::String, pub block: alloc::string::String }
}

pub mod vein_shape {
    /// A random walk of `blocks` steps.
    pub const WALK: u32 = 0;
    /// A lump of about `blocks` blocks, like modern vanilla's.
    pub const BLOB: u32 = 1;
    pub const SPHERE: u32 = 2;
    /// Flat and round, like clay and gravel under water.
    pub const DISK: u32 = 3;
}

pub mod height_curve {
    /// As many at any height.
    pub const UNIFORM: u32 = 0;
    /// Most in the middle, none at the ends (modern vanilla's iron and diamonds).
    pub const TRIANGLE: u32 = 1;
    /// Flat at its most for `plateau` blocks in the middle, falling to none at the ends.
    pub const TRAPEZOID: u32 = 2;
}

/// Surface height and biome of one column of a terrain.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerrainColumn {
    /// The top solid block of the terrain itself (water, caves and decorations aside).
    pub height: i32,
    pub biome: u32,
}

crate::data! {
    /// One step of a terrain expression, the steps of a whole expression in postfix order: a step
    /// takes its operands from the stack the steps before it left and pushes its result.
    /// Everything is a 32-bit integer so every machine and the GPU get the same bits: positions
    /// and lengths are in 1/16 block (Q4), noise values, strengths and factors in 1/4096 (Q12,
    /// 4096 is 1.0). `kind` is `expr`, `salt` names the noise of `expr::NOISE` and `expr::SITE`
    /// (two with the same salt and size line up), `name` the field of `expr::FIELD`.
    pub struct ExprStep { pub kind: u32, pub salt: u64, pub a: i32, pub b: i32, pub c: i32, pub name: alloc::string::String }
}

/// What an `ExprStep` does. In a binary step `rhs` is the value pushed last. Sums and
/// differences wrap around at 32 bits, nothing traps.
pub mod expr {
    /// Pushes `a`.
    pub const CONST: u32 = 0;
    /// Push the column's position, Q4.
    pub const X: u32 = 1;
    pub const Z: u32 = 2;
    /// Pushes the value of the field named `name` at this position; a field is an expression
    /// of its own (`comp::terrain::FIELDS`) and may only use fields declared before it.
    pub const FIELD: u32 = 3;
    pub const ADD: u32 = 4;
    pub const SUB: u32 = 5;
    /// `lhs * rhs / 4096`, each of them held to +-46340 so the product fits 32 bits.
    pub const MUL: u32 = 6;
    /// Integer division, rounded toward zero; 0 where `rhs` is 0.
    pub const DIV: u32 = 7;
    /// `lhs` modulo `rhs` (at least 1), always 0 or above.
    pub const REM: u32 = 8;
    pub const MIN: u32 = 9;
    pub const MAX: u32 = 10;
    pub const ABS: u32 = 11;
    /// The value just pushed, 0 at `a`, 4096 at `b` and straight between them (`a > b`
    /// turns it around); both a step when `a == b`.
    pub const RAMP: u32 = 12;
    /// Pops z then x, pushes noise of those coordinates in Q12 (about -4096..4096): `a`
    /// is the size of its patches (Q4), `b` the octaves (each twice as fine and half as strong).
    pub const NOISE: u32 = 13;
    /// Pops z then x, pushes how far they are from the nearest of the points scattered one
    /// per `a` (Q4) square at most (each there with chance `b`, Q12), as the square of the
    /// distance in units of that point's radius (`c`, Q4, times 0.5 to 1.5 per point), Q12:
    /// below 4096 is inside it, 8192 or more is out of reach of every one.
    pub const SITE: u32 = 14;
}

crate::data! {
    /// A named value over a terrain's columns (`comp::terrain::FIELDS`) that rules ask for and
    /// expressions use (`expr::FIELD`); the name is used as written.
    pub struct TerrainField { pub name: alloc::string::String, pub value: alloc::vec::Vec<ExprStep> }
}

crate::data! {
    /// Adds `height` (Q4) to every column of a terrain at the strength of its `mask` (0 to 4096;
    /// nothing is added where it isn't above 0; empty: everywhere at full strength). Other mods
    /// find it by its full `name`.
    pub struct Landform {
        pub name: alloc::string::String,
        pub height: alloc::vec::Vec<ExprStep>,
        pub mask: alloc::vec::Vec<ExprStep>,
    }
}

crate::data! {
    /// Multiplied into the mask of the feature `name` (`comp::terrain::MASKS`).
    pub struct LandformMask { pub name: alloc::string::String, pub mask: alloc::vec::Vec<ExprStep> }
}

crate::data! {
    /// A rule's condition: field `name` is in `min <= value < max`.
    pub struct RuleRange { pub name: alloc::string::String, pub min: f64, pub max: f64 }
}

crate::data! {
    /// Where a biome goes in a terrain (`comp::terrain::RULES`), tried in order: the first that
    /// fits wins, the last where none does. Other mods find it by its full `name`; what `fields`
    /// (`rule_field`) doesn't give comes from the biome's own (`comp::biome`). It holds where the
    /// fields of `ranges` (`temperature`, `humidity`, `continent`, or ones the terrain declares)
    /// are in range and the surface in `height` (inclusive): `top`, `depth` blocks of `filler`
    /// (empty: stone), the lowest `under_layers` of them `under`; `deep_top` on columns lower than
    /// `deep_below`, plain stone above `bare_above`, the water surface frozen into `ice`.
    pub struct BiomeRule {
        pub name: alloc::string::String,
        pub fields: u32,
        pub biome: alloc::string::String,
        pub ranges: alloc::vec::Vec<RuleRange>,
        pub height: [i32; 2],
        pub top: alloc::string::String,
        pub filler: alloc::string::String,
        pub depth: i32,
        pub under: alloc::string::String,
        pub under_layers: i32,
        pub deep_top: alloc::string::String,
        pub deep_below: i32,
        pub bare_above: i32,
        pub ice: alloc::string::String,
    }
}

crate::data! {
    /// A biome in whole cubes (`comp::terrain::CAVE_BIOMES`): biome `name` at least `depth`
    /// blocks under the surface, where a 3D noise of patches `size` across is above `threshold`.
    pub struct CaveBiome { pub name: alloc::string::String, pub depth: i32, pub size: f64, pub threshold: f64 }
}

crate::data! {
    /// Everything up to `level` that isn't ground is `block` (`comp::terrain::SEA`).
    pub struct TerrainSea { pub level: i32, pub block: alloc::string::String }
}

crate::data! {
    /// Tunnels and caverns (`comp::terrain::CAVES`); lakes of `lava` between `from` and `to`.
    pub struct TerrainCaves { pub lava: alloc::string::String, pub from: i32, pub to: i32 }
}

crate::data! {
    /// The color (0xRRGGBB) blocks tinted with tint kind `kind` take in a biome (`comp::biome::TINTS`).
    pub struct BiomeTint { pub kind: alloc::string::String, pub color: u32 }
}

crate::data! {
    /// Daylight `level` (0..1) at `turn` of the sky (`comp::dimension::DAYLIGHT`): 0 sunrise, 0.25
    /// noon, 0.5 sunset, 0.75 midnight, straight lines between the points and around.
    pub struct DaylightPoint { pub turn: f32, pub level: f32 }
}

/// Which fields of a `BiomeRule` are given (an edit changes only these).
pub mod rule_field {
    pub const BIOME: u32 = 1;
    /// `ranges`: in an edit each one replaces the rule's range of that field, or adds one.
    pub const RANGES: u32 = 2;
    pub const HEIGHT: u32 = 4;
    pub const SURFACE: u32 = 8;
    pub const UNDER: u32 = 16;
    pub const DEEP_TOP: u32 = 32;
    pub const BARE_ABOVE: u32 = 64;
    pub const FROZEN: u32 = 128;
}

/// One command argument; kinds in `command_value`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CommandValue {
    pub kind: u32,
    pub int: i64,
    pub float: f64,
    /// Text, or the full registry name for ids.
    pub text: Str,
    pub pos: Vec3,
    /// The parameter's name in the usage string.
    pub name: Str,
}

pub mod command_value {
    /// Optional parameter left out.
    pub const NONE: u32 = 0;
    pub const INT: u32 = 1;
    pub const FLOAT: u32 = 2;
    pub const TEXT: u32 = 3;
    pub const POS: u32 = 4;
    /// `int` is the registry id (item, block state, entity kind, dimension), `text` its name.
    pub const ID: u32 = 5;
    /// Selected entities: `text` holds their ids separated by spaces, `int` how many.
    pub const TARGETS: u32 = 6;
}

#[repr(C)]
pub struct ChatFormatArgs {
    pub world: *mut WorldCtx,
    pub player: u64,
    pub text: Str,
    /// Engine side result, written through `Host::chat_format_set`.
    pub out: *mut core::ffi::c_void,
}

/// The engine's text places, `TEXT_PLACE` entries registered first in this order. A place is a
/// set of defaults for what a `ShowDesc` leaves unset, and an element of the HUD the text joins
/// when it has no `at`.
pub mod show_place {
    /// By itself, at `at` or the middle of the screen.
    pub const FREE: u32 = 0;
    pub const CHAT: u32 = 1;
    pub const HINT: u32 = 2;
    pub const TITLE: u32 = 3;
    pub const SUBTITLE: u32 = 4;
    pub const NAMES: [&str; 5] = ["core:free", "core:chat", "core:hint", "core:title", "core:subtitle"];
}

/// Who gets what the server puts out (text, sounds, particles, gizmo shapes, packets): one player
/// (an entity id works as it is), or a group.
#[repr(C, u32)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Who {
    /// Whoever it reaches by itself, from where it is: a sound's range, the players near
    /// particles, the players showing a gizmo group, everyone in the dimension for text and packets.
    Around,
    Player(u64),
    /// Everyone on the server, in any dimension.
    All,
    /// Everyone in the dimension of this id (`World::dimension`).
    Dimension(u32),
    /// Everyone in a dimension within a distance of a point.
    Near(u32, Vec3, f64),
}

impl From<u64> for Who {
    fn from(player: u64) -> Who {
        Who::Player(player)
    }
}

/// Text for the screen, the one way of putting any there.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShowDesc {
    /// `show_place`.
    pub place: u32,
    /// A translation, or text as it is. Empty, with no args, takes away what `place` and `id` show.
    pub key: Str,
    pub args: Slice<UiArg>,
    /// Text with the same place and id replaces this one, in the chat where that line stands.
    pub id: Str,
    /// Seconds of fading in, staying and fading out; negative = the place's.
    pub times: [f32; 3],
    /// 0 = the place's.
    pub scale: f32,
    /// This point of the text lies at the same point of the window, in fractions of either.
    /// Negative = where the place has it.
    pub at: [f32; 2],
    /// `0xRRGGBB`, where the text doesn't color itself; negative = white.
    pub color: i64,
}
defaults!(ShowDesc {
    place: show_place::FREE,
    key: Str::EMPTY,
    args: Slice::EMPTY,
    id: Str::EMPTY,
    times: [-1.0; 3],
    scale: 0.0,
    at: [-1.0; 2],
    color: -1,
});

#[repr(C)]
pub struct CommandArgs {
    pub world: *mut WorldCtx,
    /// Entity id of the player who ran it.
    pub player: u64,
    pub values: *const CommandValue,
    pub count: u32,
    /// Engine side reply buffer, written through `Host::command_reply`.
    pub reply: *mut core::ffi::c_void,
    /// Client commands (`Host::reg_command` with `client`): the client that typed it; `world` is null.
    pub client: *mut ClientCtx,
}

/// Places for HUD elements. `HOTBAR_LEFT`/`HOTBAR_RIGHT` stack upwards above the hotbar's halves.
pub mod hud_anchor {
    pub const TOP_LEFT: &str = "top_left";
    pub const TOP_CENTER: &str = "top_center";
    pub const TOP_RIGHT: &str = "top_right";
    pub const BOTTOM_LEFT: &str = "bottom_left";
    pub const BOTTOM_RIGHT: &str = "bottom_right";
    pub const HOTBAR: &str = "hotbar";
    pub const HOTBAR_LEFT: &str = "hotbar_left";
    pub const HOTBAR_RIGHT: &str = "hotbar_right";
    /// Centered above the hotbar and what stands on it (the held item's name, `core:held_name`).
    pub const ABOVE_HOTBAR: &str = "above_hotbar";
    /// Just under the crosshair, without moving it (the attack indicator).
    pub const UNDER_CROSSHAIR: &str = "under_crosshair";
    /// A panel of the F3 overlay (left or right side), titled by the element's name as a
    /// translation key; the engine's five groups take F3+1..5, mods' the next numbers up to 9.
    pub const DEBUG_LEFT: &str = "debug_left";
    pub const DEBUG_RIGHT: &str = "debug_right";
}

/// Layout of the engine's `energy` entity component. Blocks tagged `conductor` connect the
/// block entities next to them into a network the engine balances every tick.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Energy {
    pub stored: i64,
    pub capacity: i64,
    /// Rate limits per tick; a network pulls from entities with `max_out > 0` and feeds entities
    /// with `max_in > 0`.
    pub max_in: i64,
    pub max_out: i64,
}
impl Schema for Energy {
    const SCHEMA: &'static str = "{stored:i64,capacity:i64,max_in:i64,max_out:i64}";
}
