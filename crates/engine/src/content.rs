//! Frozen registries. Built once from every mod's registrar, then shared read-only.

use glam::IVec3;
use std::sync::OnceLock;

use rustc_hash::{FxHashMap, FxHashSet};

use weft_abi::{self as abi, BlockModel, Callback, ModId, RenderLayer};

use crate::components::{self as comps, Components, RawValue};
use crate::entries::{Entry, RawEntry};
use crate::registry::entries_of;

pub const AIR: u32 = 0;

#[derive(Clone)]
pub struct ModelBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub textures: [u32; 6],
    /// Per face texels (u0, v0, u1, v1, 0..16), negative: from the box's position.
    pub uv: [[f32; 4]; 6],
    /// Per face quarter turns of the picture.
    pub turn: [u8; 6],
    /// Origin in block units.
    pub tilt: weft_abi::BoxTilt,
}

impl ModelBox {
    /// A whole cube showing these textures.
    pub fn whole(textures: [u32; 6]) -> ModelBox {
        ModelBox { min: [0.0; 3], max: [1.0; 3], textures, uv: [weft_abi::UV_FROM_BOX; 6], turn: [0; 6], tilt: weft_abi::BoxTilt::NONE }
    }

    /// Turned with its block (`abi::Turn`), pictures and all: a face taking its picture from where
    /// the box is takes it from where it was written.
    pub fn turned(&self, t: weft_abi::Turn) -> ModelBox {
        let (min, max) = t.bounds(self.min, self.max, 1.0);
        let uv = std::array::from_fn(|f| match self.uv[f] {
            uv if uv == weft_abi::UV_FROM_BOX => weft_abi::box_face_uv(f, self.min.map(|v| v * 16.0), self.max.map(|v| v * 16.0)),
            uv => uv,
        });
        let (textures, uv, turn) = (turned_faces(t, self.textures), turned_faces(t, uv), turned_pictures(t, self.turn));
        let tilt =
            weft_abi::BoxTilt { origin: t.point(self.tilt.origin, 1.0), axis: t.vector_f(self.tilt.axis), degrees: self.tilt.degrees };
        ModelBox { min, max, textures, uv, turn, tilt }
    }
}

/// Values by a block's own face, on the faces they become turned by `t`.
pub fn turned_faces<A: Copy>(t: weft_abi::Turn, v: [A; 6]) -> [A; 6] {
    let mut out = v;
    for (f, x) in v.into_iter().enumerate() {
        out[t.face(f)] = x;
    }
    out
}

/// Quarter turns of its faces' pictures, on the faces they become, with what the turn adds.
pub fn turned_pictures(t: weft_abi::Turn, turns: [u8; 6]) -> [u8; 6] {
    let mut out = [0; 6];
    for (f, n) in turns.into_iter().enumerate() {
        out[t.face(f)] = (n + t.picture(f)) % 4;
    }
    out
}

/// What a state is to redstone (`abi::SignalDesc`, worked out for quick reading).
#[derive(Clone, Copy, Default)]
pub struct Signal {
    pub kind: u32,
    /// What it gives now.
    pub level: u8,
    /// Its faces as bits by direction, turned with the block (`abi::SignalDesc` says what each is).
    pub inputs: u8,
    pub locks: u8,
    pub watches: u8,
    pub gives: u8,
    pub strong: u8,
    /// The way it acts, 6 for none: a piston's head, what a floater carries.
    pub front: u8,
    pub delay: u8,
    /// `abi::signal_flags`.
    pub flags: u8,
    /// How many values its `power` property has (0: none), and states between them.
    pub levels: u8,
    pub stride: u32,
    /// A piston's head block.
    pub head: Option<u32>,
}

pub struct StateInfo {
    pub block: u32,
    /// How its model is turned (`abi::comp::block::TURN`): all below is already turned so.
    pub turn: weft_abi::Turn,
    /// An `on_sides` block's sides it lies on, a bit per face.
    pub sides: u8,
    pub flags: u32,
    /// Emission per channel (r, g, b).
    pub light: [u8; 3],
    /// Total absorption per channel (r, g, b), opacity included.
    pub absorb: [u8; 3],
    pub opacity: u8,
    pub tint: u8,
    pub tint_faces: u8,
    pub layer: RenderLayer,
    pub level: u8,
    pub model: BlockModel,
    /// Texture ids (`Content::textures`) per face.
    pub textures: [u32; 6],
    /// Cube model: quarter turns of each face's picture.
    pub turns: [u8; 6],
    pub boxes: Vec<ModelBox>,
    /// In block units.
    pub collision: Vec<[f32; 6]>,
    /// `abi::comp::block::LIQUID_WALLS`; `Content::liquid_walls` has them worked out.
    pub liquid_walls: u8,
    /// What rays pick and the room it takes in its cell, in block units: its hitbox, else its
    /// model's boxes, a plant's cross, the whole block.
    pub selection: Vec<[f32; 6]>,
    /// What is outlined and cracked when it isn't `selection` (a hollow log outlined whole, the
    /// flower in it still picked); empty: `selection`. See `outlined`.
    pub outline: Vec<[f32; 6]>,
    pub hardness: f32,
    pub friction: f32,
    pub drag: f32,
    /// Has the `conductor` tag (part of energy networks).
    pub conductor: bool,
    /// Liquid id (`Content::liquids`) for liquid blocks.
    pub liquid: Option<u16>,

    /// Spreads onto what a matcher takes (`abi::comp::SPREADS_TO`).
    pub spread: Option<Spread>,
    /// What happens to it by itself (`abi::comp::block::RULES`).
    pub rules: Vec<Rule>,
    /// What it does to entities meeting it (`abi::comp::block::HAZARDS`).
    pub hazards: Vec<Hazard>,
    /// What its events sound and show (`block_event`).
    pub effects: [Fx; 6],
    pub signal: Signal,
    /// What a hit breaks off alone (`abi::comp::block::PIECES`), turned like the boxes.
    pub pieces: Vec<Piece>,
}

/// An attribute modifier something gives (`abi::Modifier`) resolved.
#[derive(Clone, Copy, Debug)]
pub struct SourceMod {
    pub attr: u32,
    pub op: u32,
    pub amount: f64,
    pub worn: bool,
    pub when: Option<u32>,
}

/// What game modes, items, entity kinds and poses give by their `MODIFIERS`, by id.
#[derive(Default)]
pub struct SourceMods {
    pub modes: FxHashMap<u32, Vec<SourceMod>>,
    pub items: FxHashMap<u32, Vec<SourceMod>>,
    pub kinds: FxHashMap<u32, Vec<SourceMod>>,
    pub poses: FxHashMap<u32, Vec<SourceMod>>,
}

impl SourceMods {
    pub fn is_empty(&self) -> bool {
        self.modes.is_empty() && self.items.is_empty() && self.kinds.is_empty() && self.poses.is_empty()
    }
}

/// A hazard (`abi::Hazard`) resolved.
#[derive(Clone, Copy, Debug)]
pub struct Hazard {
    /// `abi::hazard_at`.
    pub at: u32,
    pub damage: f32,
    /// Damage cause id, `None` for no damage.
    pub cause: Option<u32>,
    pub fire: u32,
    pub every: u32,
    pub when: Option<u32>,
}

/// A block rule (`abi::BlockRule`) resolved: its condition is in `Content::conds`.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub when: When,
    pub cond: Option<u32>,
    pub then: Then,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum When {
    /// A neighbor changed, so many ticks ago.
    Neighbor(u32),
    /// Mean ticks between tries.
    Random(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Then {
    Become(u32),
    Grow,
    Break,
    Fall,
}

/// A piece of a state: in `bounds` (block units), leaving `left` and dropping what `alone` does.
#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub bounds: [f32; 6],
    pub left: u32,
    pub alone: u32,
}

impl StateInfo {
    /// What is outlined and cracked.
    pub fn outlined(&self) -> &[[f32; 6]] {
        if self.outline.is_empty() { &self.selection } else { &self.outline }
    }

    /// The piece a hit at `point` (from the cell's corner) on `face` breaks off; `None`: all of it.
    pub fn piece_hit(&self, point: [f32; 3], face: u8) -> Option<&Piece> {
        let n = weft_abi::FACE_DIRS[face as usize % 6];
        // Just under the surface hit, else the piece nearest to it.
        let at: [f32; 3] = std::array::from_fn(|i| point[i] - n[i] as f32 * 1e-3);
        let far = |b: &[f32; 6]| {
            (0..3)
                .map(|i| (b[i] - at[i]).max(at[i] - b[i + 3]).max(0.0))
                .sum::<f32>()
        };
        self.pieces
            .iter()
            .min_by(|a, b| far(&a.bounds).total_cmp(&far(&b.bounds)))
    }

    /// The block texture an item of this block shows as a flat sprite (vanilla's generated item
    /// models): plants and liquids. Other shapes are drawn as their model.
    pub fn item_sprite(&self) -> Option<u32> {
        if self.liquid.is_some() {
            return Some(self.textures[3]);
        }
        match self.model {
            BlockModel::Cross | BlockModel::None | BlockModel::Wire => Some(self.textures[0]),
            _ => None,
        }
    }
}

/// A block's callbacks (`abi::comp::block`), each with the mod it runs as.
#[derive(Clone, Copy, Default)]
pub struct BlockCallbacks {
    pub on_use: Option<(ModId, Callback)>,
    pub on_neighbor: Option<(ModId, Callback)>,
    pub on_scheduled: Option<(ModId, Callback)>,
    pub on_carried: Option<(ModId, Callback)>,
    pub signal_read: Option<(ModId, Callback)>,
    pub placement: Option<(ModId, Callback)>,
    pub merge: Option<(ModId, Callback)>,
    pub on_grow: Option<(ModId, Callback)>,
    pub on_signal: Option<(ModId, Callback)>,
    pub signal_gate: Option<(ModId, Callback)>,
}

pub struct BlockDef {
    pub name: String,
    pub owner: ModId,
    pub first_state: u32,
    pub states: u32,
    /// Named properties and their values; states are every combination, the last property
    /// changing fastest.
    pub props: Vec<Prop>,
    pub cb: BlockCallbacks,
    /// The menu using the block opens.
    pub opens: Option<u32>,
    /// `on_sides`: the faces it may lie on, a bit each; 0 for other blocks.
    pub sides: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Prop {
    pub name: String,
    pub values: Vec<String>,
    /// States between two neighboring values.
    pub stride: u32,
}

impl BlockDef {
    /// The value index of every property in state `i` of this block (0-based within the block).
    pub fn prop_values(&self, i: u32) -> impl Iterator<Item = (&Prop, u32)> {
        self.props
            .iter()
            .map(move |p| (p, (i / p.stride) % p.values.len() as u32))
    }

    /// `state` with property `name` at value index `v`.
    pub fn with_value(&self, state: u32, prop: usize, v: u32) -> u32 {
        let p = &self.props[prop];
        let i = state - self.first_state;
        let cur = (i / p.stride) % p.values.len() as u32;
        self.first_state + i - cur * p.stride + v * p.stride
    }

    /// Its state lying on the `sides` (a bit per face) of an `on_sides` block, the rest as `state`.
    pub fn with_sides(&self, state: u32, sides: u8) -> u32 {
        self.props
            .iter()
            .enumerate()
            .fold(state, |st, (i, p)| match weft_abi::FACE_NAMES.iter().position(|n| *n == p.name) {
                Some(f) if self.sides & (1 << f) != 0 => {
                    let want = if sides & (1 << f) != 0 { "true" } else { "false" };
                    let v = p.values.iter().position(|v| v == want).unwrap_or(0);
                    self.with_value(st, i, v as u32)
                }
                _ => st,
            })
    }

    /// `name` or `name[a=x,b=y]` with every property.
    pub fn state_name(&self, state: u32) -> String {
        if self.props.is_empty() {
            return self.name.clone();
        }
        let props: Vec<String> = self
            .prop_values(state - self.first_state)
            .map(|(p, v)| format!("{}={}", p.name, p.values[v as usize]))
            .collect();
        format!("{}[{}]", self.name, props.join(","))
    }

    /// The state with `props` (`a=x` pairs), the others at their defaults.
    pub fn state_of(&self, props: &[(&str, &str)]) -> Result<u32, String> {
        self.changed(self.first_state, props)
    }

    /// `state` with the properties `text` sets (`a=x,b=y`).
    pub fn changed_text(&self, state: u32, text: &str) -> Result<u32, String> {
        self.changed(state, &prop_pairs(text)?)
    }

    /// `state` with `props` (`a=x` pairs) set.
    pub fn changed(&self, mut state: u32, props: &[(&str, &str)]) -> Result<u32, String> {
        for (k, v) in props {
            let pi = self
                .props
                .iter()
                .position(|p| p.name == *k)
                .ok_or_else(|| format!("{} has no property {k}", self.name))?;
            let vi = self.props[pi]
                .values
                .iter()
                .position(|x| x == v)
                .ok_or_else(|| format!("{} property {k} has no value {v} (values: {})", self.name, self.props[pi].values.join(", ")))?;
            state = self.with_value(state, pi, vi as u32);
        }
        Ok(state)
    }

    /// States matching `props` (`a=x` pairs, the others any value); an error names what does
    /// not exist.
    pub fn matching(&self, props: &[(&str, &str)]) -> Result<Vec<u32>, String> {
        let mut fixed = Vec::new();
        for (name, value) in props {
            let Some(pi) = self.props.iter().position(|p| p.name == *name) else {
                let known: Vec<&str> = self.props.iter().map(|p| p.name.as_str()).collect();
                return Err(format!(
                    "{} has no property {name} (it has: {})",
                    self.name,
                    if known.is_empty() { "none".into() } else { known.join(", ") }
                ));
            };
            let p = &self.props[pi];
            let Some(v) = p.values.iter().position(|x| x == value) else {
                return Err(format!("{} property {name} has no value {value} (values: {})", self.name, p.values.join(", ")));
            };
            fixed.push((p, v as u32));
        }
        Ok((0..self.states)
            .filter(|&i| {
                fixed
                    .iter()
                    .all(|(p, v)| (i / p.stride) % p.values.len() as u32 == *v)
            })
            .map(|i| self.first_state + i)
            .collect())
    }
}

/// Splits `name[a=x,b=y]` into the name and the pairs.
pub fn split_state(text: &str) -> Result<(&str, Vec<(&str, &str)>), String> {
    let Some((name, rest)) = text.split_once('[') else { return Ok((text, Vec::new())) };
    let Some(inner) = rest.strip_suffix(']') else { return Err(format!("{text}: missing ]")) };
    Ok((name, prop_pairs(inner).map_err(|e| format!("{text}: {e}"))?))
}

/// `a=x,b=y` as pairs.
pub fn prop_pairs(text: &str) -> Result<Vec<(&str, &str)>, String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.trim(), v.trim()))
                .ok_or_else(|| format!("{kv} is not property=value"))
        })
        .collect()
}

pub struct ItemDef {
    pub name: String,
    pub owner: ModId,
    /// From the `max_stack`, `durability` and `attack_damage` components, read often enough to keep
    /// at hand.
    pub max_stack: u32,
    pub durability: u32,
    pub attack: f32,
    /// Item icon layer; `u32::MAX` when the icon is the 3D block.
    pub icon: u32,
    pub block: Option<u32>,
    /// And the mod it runs as.
    pub on_use: Option<(ModId, Callback)>,
    /// Where it came in, every mod's in load order: how creative tabs list items.
    pub order: u64,
    /// The player's equipment slot it is worn in (`worn/<slot>` tag), by index.
    pub equip: Option<u32>,
    /// Held like a tool in third person (`handheld` tag).
    pub handheld: bool,
    /// Boxes from its model file (`models/item/<name>.json`) in block units, drawn instead of
    /// the icon in hands, on the ground and in slots; empty: none.
    pub model: Vec<ModelBox>,
}

/// A creative inventory tab: translated as `tab.<ns>.<name>`, or the mod's name for the tab a mod
/// gets automatically for items it put in no tab.
/// Ticks of holding the use button a stack asks for, 0 for at once.
pub fn use_ticks(c: &Content, stack: &weft_abi::ItemStack) -> u32 {
    c.stack_value::<u32>(stack, c.ids.use_ticks).unwrap_or(0)
}

/// Needs both hands.
pub fn two_handed(c: &Content, item: u32) -> bool {
    item != 0
        && c.ids
            .two_handed
            .is_some_and(|t| c.components.has(crate::components::ITEM, item, t))
}

/// The action a two-handed item plays held in one hand, if that may be done at all.
pub fn one_handed_action(c: &Content, item: u32) -> Option<&str> {
    c.ids
        .one_handed
        .and_then(|t| c.components.text(crate::components::ITEM, item, t))
}

/// May go in the other hand, or stay in the main one with something there.
pub fn fits_one_hand(c: &Content, item: u32) -> bool {
    !two_handed(c, item) || one_handed_action(c, item).is_some()
}

/// What is in the other hand.
pub fn in_off_hand(c: &Content, inventory: &[weft_abi::ItemStack]) -> weft_abi::ItemStack {
    c.game
        .player_layout
        .off
        .and_then(|o| inventory.get(o as usize))
        .copied()
        .unwrap_or_default()
}

/// What the main hand really holds: an item that needs both hands does nothing while the other
/// holds something, and shows as an empty hand. It stays in the hotbar.
pub fn in_main_hand(c: &Content, inventory: &[weft_abi::ItemStack], selected: u32) -> weft_abi::ItemStack {
    let main = inventory
        .get(selected as usize)
        .copied()
        .unwrap_or_default();
    if !main.is_empty() && !fits_one_hand(c, main.item) && !in_off_hand(c, inventory).is_empty() {
        return weft_abi::ItemStack::EMPTY;
    }
    main
}

/// What goes first on a right click: the higher `use_priority`, the main hand when equal.
pub fn hands_in_order(c: &Content, inventory: &[weft_abi::ItemStack], selected: u32) -> Vec<u32> {
    let main = in_main_hand(c, inventory, selected);
    let off = in_off_hand(c, inventory);
    let prio = |s: &weft_abi::ItemStack| c.stack_value::<i32>(s, c.ids.use_priority).unwrap_or(0);
    let mut out = Vec::with_capacity(2);
    if !main.is_empty() {
        out.push(selected);
    }
    if let (Some(o), false) = (c.game.player_layout.off, off.is_empty()) {
        if !main.is_empty() && prio(&off) > prio(&main) {
            out.insert(0, o);
        } else {
            out.push(o);
        }
    }
    out
}

/// Full swings per second with a stack in hand.
pub fn swings_per_second(c: &Content, held: &weft_abi::ItemStack) -> f32 {
    c.stack_value::<f32>(held, c.ids.attack_speed)
        .unwrap_or(4.0)
}

/// How charged a swing is after that many ticks, 0..1. Client and server have to agree, with
/// `speed` the `core:attack_speed` attribute.
pub fn swing_charge(c: &Content, held: &weft_abi::ItemStack, ticks: f32, speed: f32) -> f32 {
    ((ticks + 0.5) * swings_per_second(c, held) * speed / crate::world::TPS as f32).clamp(0.0, 1.0)
}

/// What a block's tint goes on, on one face of it, wherever that is drawn.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FacePaint {
    pub base_tinted: bool,
    pub overlay_tinted: bool,
}

/// `has_overlay`: the face's texture has an `_overlay` over it, which takes the whole tint (a grass
/// side's green, on dirt).
pub fn face_paint(s: &StateInfo, face: usize, has_overlay: bool) -> FacePaint {
    let tints = s.tint != 0;
    FacePaint { base_tinted: tints && !has_overlay && s.tint_faces & (1 << face) != 0, overlay_tinted: tints && has_overlay }
}

/// A block state's events, by `StateInfo::effects` index (`abi::comp::EFFECTS` names them).
pub mod block_event {
    pub const BREAK: usize = 0;
    pub const PLACE: usize = 1;
    pub const STEP: usize = 2;
    pub const HIT: usize = 3;
    pub const LAND: usize = 4;
    pub const SPRINT: usize = 5;
    pub const NAMES: [&str; 6] = ["break", "place", "step", "hit", "land", "sprint"];
}

/// What an event sounds (a sound event) and shows (a particle kind), `u32::MAX` for nothing.
#[derive(Clone, Copy, Debug)]
pub struct Fx {
    pub sound: u32,
    pub particle: u32,
}

impl Fx {
    pub const NONE: Fx = Fx { sound: u32::MAX, particle: u32::MAX };
}

/// Whether changing from one to the other leaves a swing as it was: the same item, whatever its
/// count or damage. Client and server have to agree.
pub fn same_in_hand(a: weft_abi::ItemStack, b: weft_abi::ItemStack) -> bool {
    a.is_empty() == b.is_empty() && (a.is_empty() || a.item == b.item)
}

/// Liquids from this hot burn what touches them, and are `hot` without being told.
pub const BURNING: u32 = 1000;

/// A liquid the engine flows (`abi::comp::liquid`); its block has 16 states.
pub struct LiquidDef {
    pub name: String,
    pub first_state: u32,
    pub delay: u32,
    pub drop_off: u32,
    pub slope_find: u32,
    pub renewable: u32,
    /// Lighter than air: flows up.
    pub rises: bool,
    pub temperature: u32,
    /// 0xRRGGBB, `None` for the average of its texture.
    pub fog_color: Option<u32>,
    pub fog_distance: f32,
    /// Asset name, empty for its own texture.
    pub overlay: String,
    pub overlay_opacity: f32,
}

impl LiquidDef {
    pub fn burns(&self) -> bool {
        self.temperature >= BURNING
    }
}

/// Its two sides are matchers.
pub struct ReactionDef {
    pub liquid: u32,
    pub touching: u32,
    pub source: Option<u32>,
    pub flowing: Option<u32>,
    pub falls_into: Option<u32>,
}

pub struct CreativeTab {
    pub name: String,
    pub icon: u32,
    pub items: Vec<u32>,
}

pub struct BiomeDef {
    pub name: String,
    /// Color (0xRRGGBB) per tint kind, by `Content::tint_kinds` index.
    pub tints: Vec<u32>,
}

pub struct DimensionDef {
    pub name: String,
    pub owner: ModId,
    /// The callbacks and the mods they run as.
    pub generator: Option<(ModId, Callback)>,
    pub spawn: Option<(ModId, Callback)>,
    pub column: Option<(ModId, Callback)>,
    pub min_cube_y: i32,
    pub max_cube_y: i32,
    pub sky_cube_y: i32,
    pub sky_light: bool,
    pub gravity: f32,
    pub day_length: u32,
    pub fixed_time: u32,
    pub sky_color: u32,
    pub fog_color: u32,
    pub ambient: u8,
    pub player_spawn: bool,
    /// False for the engine's void: nothing in it is written to the world.
    pub saved: bool,
    /// False for the engine's void: no block can be set there.
    pub editable: bool,
    /// False: blocks never update (`comp::dimension::NO_BLOCK_UPDATES`).
    pub block_updates: bool,
    /// The engine's own dimensions generate their cubes here instead of a mod's `generator`.
    pub builtin: Option<fn(&Content, glam::IVec3, &mut [u32])>,
    /// Where players appear without a `spawn` callback.
    pub spawn_at: [i32; 3],
    /// Placed by the engine after `generator`, in name order.
    pub ores: Vec<OreDef>,
    pub structures: Vec<StructureDef>,
    /// Its `entry_kind::TERRAIN`: fills cubes before `generator` runs.
    pub terrain: Option<Box<crate::server::terrain::Terrain>>,
    /// `comp::dimension::BARE`: only ores, scatters and decorations that name it.
    pub bare: bool,
    /// The weathers that come here after clear (`comp::dimension::WEATHERS`), resolved at freeze.
    pub weathers: Vec<u32>,
    weather_names: Vec<String>,
    /// (turn, level) points sorted by turn, empty for vanilla's curve.
    pub daylight: Vec<[f32; 2]>,
    /// In name order, placed after `generator`.
    pub scatters: Vec<ScatterDef>,
    /// (mod, callback) run after the generator and scatters, in load order.
    pub decorations: Vec<(ModId, Callback)>,
    /// Columns from `column`, for a dimension without a terrain.
    pub column_cache: crate::server::terrain::ColumnCache<[abi::TerrainColumn; 256]>,
    /// Per cube column, the scattered things that may reach into it (`dim::scatter_sites`):
    /// worked out once for the whole column instead of once per cube.
    pub scatter_cache: crate::server::terrain::ColumnCache<Vec<crate::server::dim::Site>>,
}

#[derive(Clone)]
pub struct ScatterDef {
    pub owner: ModId,
    pub cell: i32,
    pub reach: i32,
    /// How far below and above its site it may put blocks.
    pub reach_y: [i32; 2],
    /// Biome component (f32) with the chance of a site per cell.
    pub density: Option<u32>,
    /// Biomes and heights of the site (`abi::comp::common`).
    pub biome: Option<Box<[u64]>>,
    pub y: [i32; 2],
    /// Matcher of the surface block under the site (`None`: any).
    pub ground: Option<u32>,
    pub salt: u64,
    pub shape: Callback,
}

impl DimensionDef {
    /// Weather comes and goes here.
    pub fn has_weather(&self) -> bool {
        !self.weathers.is_empty()
    }

    /// The 256 columns of cube column (`cx`, `cz`), index z * 16 + x: its terrain's or its
    /// `column` callback's. None for a dimension with neither.
    pub fn columns(&self, seed: u64, cx: i32, cz: i32) -> Option<std::sync::Arc<[abi::TerrainColumn; 256]>> {
        if let Some(t) = &self.terrain {
            return Some(std::sync::Arc::new(t.columns(seed, cx, cz).cols));
        }
        let (by, cb) = self.column?;
        Some(self.column_cache.get(seed, cx, cz, || {
            std::array::from_fn(|i| {
                let mut a =
                    abi::ColumnArgs { seed, x: cx * 16 + (i % 16) as i32, z: cz * 16 + (i / 16) as i32, height: i32::MIN, biome: 0 };
                crate::host::invoke(by, cb, &mut a);
                abi::TerrainColumn { height: a.height, biome: a.biome }
            })
        }))
    }

    /// Height and biome of column (`x`, `z`), generated or not.
    pub fn column(&self, seed: u64, x: i32, z: i32) -> Option<abi::TerrainColumn> {
        if let Some(t) = &self.terrain {
            return Some(t.column(seed, x, z));
        }
        let cols = self.columns(seed, x.div_euclid(16), z.div_euclid(16))?;
        Some(cols[(z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize])
    }

    /// Whether caves took the surface block of column (`x`, `z`).
    pub fn carved(&self, seed: u64, x: i32, z: i32) -> bool {
        self.terrain
            .as_ref()
            .is_some_and(|t| t.surface_carved(seed, x, z))
    }

    /// The block the terrain leaves at the top of column (`x`, `z`), 0 if none.
    pub fn surface_block(&self, seed: u64, x: i32, z: i32) -> u32 {
        self.terrain
            .as_ref()
            .map_or(0, |t| t.surface_block(seed, x, z))
    }

    /// The terrain's sea level, if it has a sea.
    pub fn sea_level(&self) -> Option<i32> {
        self.terrain.as_ref().and_then(|t| t.sea_level())
    }

    /// How far the day has gone around the sky at world time `time`, in turns: 0 the sun rising,
    /// 0.25 noon, 0.75 midnight. Vanilla's easing: slower than the clock around noon and midnight.
    pub fn day_turn(&self, time: f64) -> f64 {
        let (t, len) =
            if self.day_length == 0 { (self.fixed_time as f64, 24000.0) } else { (time % self.day_length as f64, self.day_length as f64) };
        let from_noon = (t / len - 0.25).rem_euclid(1.0);
        let eased = 0.5 - (from_noon * std::f64::consts::PI).cos() / 2.0;
        (0.25 + (from_noon * 2.0 + eased) / 3.0).rem_euclid(1.0)
    }

    /// Fraction of full daylight (0..1) at world time `time`: the dimension's curve, or
    /// vanilla's.
    pub fn daylight(&self, time: u64) -> f32 {
        if !self.sky_light {
            return 0.0;
        }
        let turn = self.day_turn(time as f64);
        let pts = &self.daylight;
        if pts.is_empty() {
            return ((turn * std::f64::consts::TAU).sin() * 2.0 + 0.5).clamp(0.0, 1.0) as f32;
        }
        // Between the last point before `turn` and the first after it, around the day.
        let t = turn as f32;
        let after = pts.iter().position(|p| p[0] > t).unwrap_or(0);
        let before = (after + pts.len() - 1) % pts.len();
        let (a, b) = (pts[before], pts[after]);
        let span = (b[0] - a[0]).rem_euclid(1.0);
        if span <= f32::EPSILON {
            return a[1];
        }
        a[1] + (b[1] - a[1]) * ((t - a[0]).rem_euclid(1.0) / span)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Spread {
    /// Matcher of the targets.
    pub to: u32,
    pub mean: f64,
    /// Light the target needs.
    pub light: u32,
    /// From the spreader to the target, inclusive.
    pub min: IVec3,
    pub max: IVec3,
    /// The target must have nothing opaque on top.
    pub open_top: bool,
}

#[derive(Clone)]
pub struct OreDef {
    pub state: u32,
    /// Matcher of what it may replace besides its variants'.
    pub replace: Option<u32>,
    pub min_y: i32,
    pub max_y: i32,
    pub veins: u32,
    pub size: u32,
    /// Derived from the block name, so adding an ore does not move the others.
    pub salt: u64,
    /// Biomes the block's column must be.
    pub biome: Option<Box<[u64]>>,
    /// `abi::vein_shape`, and its sizes.
    pub shape: u32,
    pub shape_size: [f32; 2],
    /// `abi::height_curve`.
    pub height: u32,
    pub plateau: i32,
    /// (matcher of what it meets, ore state put there instead of `state`), the first that takes it.
    pub variants: Vec<(u32, u32)>,
    /// Share of blocks left out next to air.
    pub exposed: f32,
}

impl OreDef {
    /// The state it puts where `state` was, if it replaces that.
    pub fn state_in(&self, m: &crate::refs::Matchers, state: u32) -> Option<u32> {
        self.variants
            .iter()
            .find(|v| m.has(v.0, state))
            .map(|v| v.1)
            .or_else(|| {
                self.replace
                    .filter(|&r| m.has(r, state))
                    .map(|_| self.state)
            })
    }

    /// How far from its start a vein may put blocks: a cube at most, so the cubes around one
    /// are all there is to look at.
    pub fn reach(&self) -> i32 {
        let r = match self.shape {
            abi::vein_shape::SPHERE | abi::vein_shape::DISK => self.shape_size[0].ceil() as i32,
            abi::vein_shape::BLOB => (self.size as f32).cbrt().ceil() as i32 + 2,
            _ => self.size as i32,
        };
        r.clamp(0, 16)
    }

    /// How likely a vein starting at `y` is to be kept (0..1), and how many starts to try per
    /// vein wanted so that the average comes out as `veins`.
    pub fn weight(&self, y: i32) -> f64 {
        let (lo, hi) = (self.min_y as f64, self.max_y as f64);
        let half = (hi - lo) / 2.0;
        if half <= 0.0 || self.height == abi::height_curve::UNIFORM {
            return 1.0;
        }
        let off = (y as f64 - (lo + half)).abs();
        let flat = if self.height == abi::height_curve::TRAPEZOID { (self.plateau as f64 / 2.0).min(half) } else { 0.0 };
        if half - flat <= 0.0 { 1.0 } else { ((half - off) / (half - flat)).clamp(0.0, 1.0) }
    }

    pub fn tries(&self) -> u32 {
        let (lo, hi) = (self.min_y as f64, self.max_y as f64);
        let span = hi - lo;
        let avg = match self.height {
            abi::height_curve::TRIANGLE => 0.5,
            abi::height_curve::TRAPEZOID if span > 0.0 => (1.0 + (self.plateau as f64 / span).min(1.0)) / 2.0,
            _ => 1.0,
        };
        (self.veins as f64 / avg).round() as u32
    }
}

/// A template's blocks, checked at freeze: (offset from the lowest corner, state).
pub struct TemplateDef {
    pub name: String,
    pub blocks: Vec<(glam::IVec3, u32)>,
}

#[derive(Clone)]
pub struct StructureDef {
    pub name: String,
    /// (offset from the lowest corner, state).
    pub blocks: Vec<(glam::IVec3, u32)>,
    pub extent: glam::IVec3,
    pub spacing: i32,
    pub chance: f32,
    pub biome: Option<Box<[u64]>>,
    /// Heights its ground may be at.
    pub y: [i32; 2],
    pub sink: i32,
    pub salt: u64,
}

pub struct ModMeta {
    pub id: String,
    pub version: String,
    pub deps: Vec<String>,
    /// Position when every mod comes after its dependencies (ties by id): the order in which
    /// overrides apply and the later one wins.
    pub order: u32,
    /// Its `assets` folder (block and item model files are read at freeze).
    pub assets: std::path::PathBuf,
}

/// Ids of components the engine reads, resolved once.
#[derive(Default)]
pub struct Ids {
    pub max_stack: Option<u32>,
    pub durability: Option<u32>,
    pub attack_damage: Option<u32>,
    pub mining_speed: Option<u32>,
    pub tool_level: Option<u32>,
    pub use_priority: Option<u32>,
    pub two_handed: Option<u32>,
    pub one_handed: Option<u32>,
    pub harvest_level: Option<u32>,
    pub damage: Option<u32>,
    pub name: Option<u32>,
    pub conductor: Option<u32>,
    pub worn: Option<u32>,
    pub loot_rolls: Option<u32>,
    pub worn_texture: Option<u32>,
    pub wear_per_hit: Option<u32>,
    pub spawn_egg: Option<u32>,
    pub attack_speed: Option<u32>,
    pub use_ticks: Option<u32>,
    pub sweeps: Option<u32>,
    pub no_creative_break: Option<u32>,
    pub inventory: Option<u32>,
    pub contents: Option<u32>,
    pub layers: Option<u32>,
    pub energy: Option<u32>,
}

/// What lighting and meshing read of a state for every block they pass, packed tight: a
/// `StateInfo` spans a few cache lines, these are 12 bytes, five to a line.
#[derive(Clone, Copy, Default)]
pub struct Tight {
    pub flags: u32,
    pub opacity: u8,
    /// Emission per channel (r, g, b).
    pub light: [u8; 3],
    /// Absorption per channel (r, g, b), opacity included.
    pub absorb: [u8; 3],
    /// Faces (`1 << dir`, `world::DIRS` order) its shape covers whole, which light doesn't
    /// pass through though the block lets it in elsewhere: a slab's bottom, as vanilla's.
    pub seals: u8,
}

impl Tight {
    /// Takes all light: nothing gets in.
    pub fn dark(&self) -> bool {
        self.opacity >= 15
    }

    /// Light sees no difference between the two.
    pub fn same_light(&self, o: &Tight) -> bool {
        (self.opacity, self.light, self.absorb, self.seals) == (o.opacity, o.light, o.absorb, o.seals)
    }

    /// Light goes through it whole and every way.
    pub fn clear(&self) -> bool {
        self.opacity == 0 && self.seals == 0
    }

    /// Whether light goes from a block of `self` to the next one, `to`, in direction `dir`.
    #[inline]
    pub fn lets_into(&self, to: &Tight, dir: usize) -> bool {
        self.seals & (1 << dir) == 0 && to.seals & (1 << (dir ^ 1)) == 0
    }
}

/// The faces a state's collision covers whole (bit per `FACE_DIRS` direction), by all its boxes
/// together (a stair's back is two): what a slab keeps light out of and what holds a liquid in.
pub fn covered_faces(s: &StateInfo) -> u8 {
    (0..6).fold(0, |m, dir| {
        let (axis, top) = (dir / 2, dir % 2 == 1);
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        // The face in sixteenths: each cell covered by some box lying on it.
        let mut cells = [false; 256];
        for bx in &s.collision {
            let on_face = if top { bx[axis + 3] >= 1.0 - 1e-4 } else { bx[axis] <= 1e-4 };
            if !on_face {
                continue;
            }
            let span = |lo: f32, hi: f32| ((lo * 16.0).round().max(0.0) as usize, (hi * 16.0).round().min(16.0) as usize);
            let ((a0, a1), (b0, b1)) = (span(bx[a], bx[a + 3]), span(bx[b], bx[b + 3]));
            for u in a0..a1 {
                for v in b0..b1 {
                    cells[u * 16 + v] = true;
                }
            }
        }
        if cells.iter().all(|&c| c) { m | 1 << dir } else { m }
    })
}

/// The faces a state keeps light out of, for `Tight::seals`: as vanilla's
/// `useShapeForLightOcclusion`, only blocks drawn solid (slabs, stairs) do it by their shape; what
/// is seen through (glass, leaves, ice) doesn't, and a block light doesn't pass at all needs none.
fn seals_of(s: &StateInfo) -> u8 {
    if s.opacity >= 15 || s.layer != RenderLayer::Opaque {
        return 0;
    }
    covered_faces(s)
}

pub struct Content {
    pub mods: Vec<ModMeta>,
    pub states: Vec<StateInfo>,
    /// `states`, the parts read per block (`Tight`).
    pub tight: Vec<Tight>,
    /// Per state, the sides a liquid in its cell can't pass (`abi::comp::block::LIQUID_WALLS`).
    pub liquid_walls: Vec<u8>,
    pub blocks: Vec<BlockDef>,
    pub items: Vec<ItemDef>,
    pub biomes: Vec<BiomeDef>,
    /// Word sets command parameters `word(#set)` complete from.
    pub word_sets: FxHashMap<String, Vec<String>>,
    /// Name and default color of each tint kind, kind 1 first.
    pub tint_kinds: Vec<(String, u32)>,
    pub dims: Vec<DimensionDef>,
    /// A bit per state some block spreads onto (`StateInfo::spread`).
    pub spread_targets: Vec<u64>,
    /// From a target, the box holding everything that may spread onto it.
    pub spread_search: (IVec3, IVec3),
    pub templates: Vec<TemplateDef>,
    pub template_names: FxHashMap<String, u32>,
    /// Weighted lists of block states from `blocks` component values, by the index the value holds.
    pub ref_lists: Vec<Vec<[u32; 2]>>,
    pub components: Components,
    pub ids: Ids,
    /// Blocks made of others (`Registry::shapes_of`): by name, the source and the lang key for it.
    pub shapes: FxHashMap<String, (String, String)>,
    /// What may be in a cell with this block though their shapes overlap (a matcher), by block.
    pub shares_cell: FxHashMap<u32, u32>,
    /// Every block matcher the content names (`refs`).
    pub matchers: crate::refs::Matchers,
    /// Attribute modifiers that come with items, kinds and poses.
    pub source_mods: SourceMods,
    /// Conditions by the ids their users keep (`conditions::Texts`).
    pub conds: Vec<crate::conditions::Expr>,
    /// Those met at run time (`{if:..}` in text, mods asking), compiled once each.
    cond_cache: std::sync::Mutex<FxHashMap<String, Option<std::sync::Arc<crate::conditions::Expr>>>>,
    /// (`mineable/<class>`, `tools/<class>`) component ids per tool class.
    pub tool_classes: Vec<(u32, Option<u32>)>,
    /// Every texture by name (`<ns>:block/..`, `<ns>:item/..`, `<ns>:particle/..`); blocks, items
    /// and particles hold ids into it, and the client loads this one list.
    pub textures: Vec<String>,
    /// Block breaking stages, ids into `textures`.
    pub crack_textures: [u32; 10],
    /// Animated flames drawn around burning entities.
    pub fire_texture: u32,
    /// Engine commands first, then mods' in name order.
    pub commands: Vec<crate::commands::CommandDef>,
    pub block_names: FxHashMap<String, u32>,
    pub item_names: FxHashMap<String, u32>,
    pub dim_names: FxHashMap<String, u32>,
    /// Where players start: the dimension marked `PLAYER_SPAWN` by the mod loaded last, else the
    /// first one registered, else the engine's void (`void_dim`).
    pub spawn_dim: u32,
    pub void_dim: u32,
    pub biome_names: FxHashMap<String, u32>,
    pub creative_tabs: Vec<CreativeTab>,
    pub game: crate::registry::Game,
    pub liquids: Vec<LiquidDef>,
    pub reactions: Vec<ReactionDef>,
    /// Item -> (liquid it holds, the empty container left after pouring).
    pub item_liquid: FxHashMap<u32, (u16, u32)>,
    /// (empty container, liquid) -> filled item.
    pub containers: FxHashMap<(u32, u16), u32>,
    /// Each liquid's item (`comp::FLUID`), by liquid; 0 for one without.
    pub fluid_items: Vec<u32>,
    /// Item -> the liquid it is.
    pub item_fluid: FxHashMap<u32, u16>,
    pub loot: FxHashMap<String, crate::loot::Table>,
    /// `abi::stack_flags` of stack components by component id.
    pub stack_flags: FxHashMap<u32, u32>,
    /// What falls in each biome when a weather does (`registry::PrecipitationDef`), by biome.
    pub biome_precipitation: Vec<u16>,
    /// The `immune/<cause>` tag of each damage cause, by cause id, if anything uses it.
    pub damage_immune: Vec<Option<u32>>,
}

static CONTENT: OnceLock<Content> = OnceLock::new();

pub fn content() -> &'static Content {
    CONTENT.get().expect("content not frozen yet")
}

pub fn try_content() -> Option<&'static Content> {
    CONTENT.get()
}

pub(crate) fn set(c: Content) {
    if CONTENT.set(c).is_err() {
        panic!("content frozen twice");
    }
}

/// Color (0xRRGGBB) of a tint kind on items, which have no biome: the kind's default.
pub fn item_tint(tint: u8) -> u32 {
    (tint as usize)
        .checked_sub(1)
        .and_then(|i| content().tint_kinds.get(i))
        .map_or(0xFFFFFF, |k| k.1)
}

impl Content {
    /// In creative this item doesn't break blocks.
    pub fn no_creative_break(&self, item: u32) -> bool {
        self.ids
            .no_creative_break
            .is_some_and(|t| self.components.has(comps::ITEM, item, t))
    }

    pub fn block_of(&self, state: u32) -> &BlockDef {
        &self.blocks[self.states[state as usize].block as usize]
    }

    /// `state` turned by `t` along with the world around it: the state of its block turned so,
    /// the rest of its properties kept as far as they can be; itself where its block can't turn
    /// that way.
    pub fn turned_state(&self, state: u32, t: weft_abi::Turn) -> u32 {
        let s = &self.states[state as usize];
        let (want, sides) = (s.turn.then(t), t.faces(s.sides));
        if want == s.turn && sides == s.sides {
            return state;
        }
        let def = self.block_of(state);
        let differ = |c: u32| {
            def.prop_values(c - def.first_state)
                .zip(def.prop_values(state - def.first_state))
                .filter(|(a, b)| a.1 != b.1)
                .count()
        };
        (def.first_state..def.first_state + def.states)
            .filter(|&c| self.states[c as usize].turn == want && self.states[c as usize].sides == sides)
            .min_by_key(|&c| differ(c))
            .unwrap_or(state)
    }

    /// Whether a rule doing `then` has something to do for a block in `state`: a growth step
    /// ahead of it.
    pub fn acts(&self, state: u32, then: Then) -> bool {
        then != Then::Grow || self.block_of(state).cb.on_grow.is_some() || self.grown(state).is_some()
    }

    /// Whether something may happen to a block in `state` with time: a rule now and then, or
    /// something spreading onto it. Such blocks keep the time they were simulated to when saved,
    /// to catch up later.
    pub fn lives(&self, state: u32) -> bool {
        self.states[state as usize]
            .rules
            .iter()
            .any(|r| matches!(r.when, When::Random(_)) && self.acts(state, r.then))
            || self.spread_target(state)
    }

    /// Whether `state` grows by itself: it has growth steps ahead it takes now and then.
    pub fn grows(&self, state: u32) -> bool {
        self.states[state as usize]
            .rules
            .iter()
            .any(|r| matches!(r.when, When::Random(_)) && r.then == Then::Grow && self.acts(state, r.then))
    }

    /// The side whose block holds it up, if a rule breaks it without one (a torch's wall), turned
    /// as the state is.
    pub fn held_by(&self, state: u32) -> Option<u8> {
        fn side(e: &crate::conditions::Expr) -> Option<u8> {
            use crate::conditions::{Arg, Expr};
            match e {
                Expr::Is(weft_abi::condition::SIDE, Arg::Side(f, _)) => Some(*f),
                Expr::All(v) | Expr::Any(v) => v.iter().find_map(side),
                Expr::Not(e) => side(e),
                _ => None,
            }
        }
        self.states[state as usize]
            .rules
            .iter()
            .filter(|r| r.then == Then::Break)
            .find_map(|r| side(self.conds.get(r.cond? as usize)?))
            .map(|f| self.states[state as usize].turn.face(f as usize) as u8)
    }

    /// Some block spreads onto this state.
    pub fn spread_target(&self, state: u32) -> bool {
        crate::refs::has(&self.spread_targets, state)
    }

    /// The default growth step: the `age` property one higher, `None` at the last age.
    pub fn grown(&self, state: u32) -> Option<u32> {
        let b = self.block_of(state);
        let pi = b.props.iter().position(|p| p.name == "age")?;
        let p = &b.props[pi];
        let v = ((state - b.first_state) / p.stride) % p.values.len() as u32;
        (v + 1 < p.values.len() as u32).then(|| b.with_value(state, pi, v + 1))
    }

    /// The spawn egg of an entity kind.
    pub fn spawn_egg_of(&self, kind: u32) -> Option<u32> {
        let (name, egg) = (&self.game.entities.get(kind as usize)?.name, self.ids.spawn_egg?);
        (1..self.items.len() as u32).find(|&i| self.components.text(comps::ITEM, i, egg) == Some(name.as_str()))
    }

    /// Whether the shapes of two states overlap, in one cell.
    pub fn overlap(&self, a: u32, b: u32) -> bool {
        const TOUCHING: f32 = 1e-4;
        self.states[a as usize].selection.iter().any(|x| {
            self.states[b as usize]
                .selection
                .iter()
                .any(|y| (0..3).all(|i| x[i] + TOUCHING < y[i + 3] && y[i] + TOUCHING < x[i + 3]))
        })
    }

    /// A condition met at run time, compiled the first time; `None` (said once) for a wrong one.
    pub fn condition(&self, text: &str) -> Option<std::sync::Arc<crate::conditions::Expr>> {
        let mut cache = self.cond_cache.lock().unwrap();
        if let Some(e) = cache.get(text) {
            return e.clone();
        }
        cache
            .entry(text.to_string())
            .or_insert_with(|| {
                let look = crate::conditions::Look {
                    game: &self.game,
                    block_names: &self.block_names,
                    blocks: &self.blocks,
                    components: &self.components,
                };
                crate::conditions::compile(text, &look)
                    .map_err(|e| log::warn!("{e}"))
                    .ok()
                    .map(std::sync::Arc::new)
            })
            .clone()
    }

    /// Whether either of two states said it may be in a cell with the other.
    pub fn may_share(&self, a: u32, b: u32) -> bool {
        let says = |x: u32, y: u32| {
            self.shares_cell
                .get(&self.states[x as usize].block)
                .is_some_and(|&m| self.matchers.has(m, y))
        };
        says(a, b) || says(b, a)
    }

    /// The item a middle click on this block picks: the block's own item, else the first its
    /// loot table names.
    pub fn pick_item(&self, state: u32) -> Option<u32> {
        let b = self.block_of(state);
        self.items
            .iter()
            .position(|i| i.block == Some(b.first_state))
            .map(|i| i as u32)
            .filter(|&i| i != 0)
            .or_else(|| {
                self.loot
                    .get(&b.name)?
                    .pools
                    .iter()
                    .flat_map(|p| &p.entries)
                    .map(|e| e.item)
                    .find(|&i| i != 0)
            })
    }

    #[inline]
    pub fn block_has(&self, state: u32, comp: Option<u32>) -> bool {
        comp.is_some_and(|c| self.components.has(comps::BLOCK, state, c))
    }

    #[inline]
    pub fn item_has(&self, item: u32, comp: Option<u32>) -> bool {
        comp.is_some_and(|c| self.components.has(comps::ITEM, item, c))
    }

    pub fn item_value<T: Copy>(&self, item: u32, comp: Option<u32>) -> Option<T> {
        self.components.get(comps::ITEM, item, comp?)
    }

    pub fn block_value<T: Copy>(&self, state: u32, comp: Option<u32>) -> Option<T> {
        self.components.get(comps::BLOCK, state, comp?)
    }

    /// Worn look of an item: inflation and texture.
    pub fn worn(&self, item: u32) -> Option<(abi::Worn, &str)> {
        let w = self.item_value::<abi::Worn>(item, self.ids.worn)?;
        Some((
            w,
            self.components
                .text(comps::ITEM, item, self.ids.worn_texture?)?,
        ))
    }

    /// The roles (`1 << role`) of the wearer's bones an item covers, from its equipment slot.
    pub fn worn_roles(&self, item: u32) -> u32 {
        self.items
            .get(item as usize)
            .and_then(|d| d.equip)
            .map_or(0, |e| self.game.player_layout.equipment[e as usize].roles)
    }

    /// Bones an item's equipment slot covers by name, on top of their roles.
    pub fn worn_bones(&self, item: u32) -> &[String] {
        self.items
            .get(item as usize)
            .and_then(|d| d.equip)
            .map_or(&[], |e| &self.game.player_layout.equipment[e as usize].bones)
    }

    /// Tool level a block needs (`harvest_level`, see `abi::tool_tier`).
    pub fn harvest_level(&self, state: u32) -> u32 {
        self.block_value::<u32>(state, self.ids.harvest_level)
            .unwrap_or(0)
    }

    /// Has a `harvest_level`: drops its loot only to a fitting tool.
    pub fn needs_tool(&self, state: u32) -> bool {
        self.block_value::<u32>(state, self.ids.harvest_level)
            .is_some()
    }

    /// The tool is of a class the block is mineable with (`mineable/pickaxe` on the block,
    /// `tools/pickaxe` on the stack or its item).
    pub fn tool_fits(&self, state: u32, tool: &abi::ItemStack) -> bool {
        !tool.is_empty()
            && self
                .tool_classes
                .iter()
                .any(|&(mineable, tools)| self.components.has(comps::BLOCK, state, mineable) && self.stack_has(tool, tools))
    }

    /// Mining the block with `tool` (empty: the hand) harvests it: it needs no tool, or this
    /// one fits and is of a high enough level.
    pub fn harvests(&self, state: u32, tool: &abi::ItemStack) -> bool {
        !self.needs_tool(state)
            || (self.tool_fits(state, tool)
                && self
                    .stack_value::<u32>(tool, self.ids.tool_level)
                    .unwrap_or(0)
                    >= self.harvest_level(state))
    }

    /// What a stack carries of `comp`: its own value, else its item's. A tool made of parts has
    /// its own durability, speed and level this way, and everything reading them goes by it.
    pub fn stack_value<T: Copy>(&self, stack: &abi::ItemStack, comp: Option<u32>) -> Option<T> {
        let comp = comp?;
        crate::stack::value(stack.data, comp)
            .filter(|b| b.len() >= size_of::<T>())
            .map(|b| unsafe { std::ptr::read_unaligned(b.as_ptr() as *const T) })
            .or_else(|| self.item_value(stack.item, Some(comp)))
    }

    /// The stack or its item has the tag or component.
    pub fn stack_has(&self, stack: &abi::ItemStack, comp: Option<u32>) -> bool {
        comp.is_some_and(|id| crate::stack::value(stack.data, id).is_some()) || self.item_has(stack.item, comp)
    }

    /// Uses the stack takes before it breaks, 0: it doesn't wear.
    pub fn durability(&self, stack: &abi::ItemStack) -> u32 {
        self.stack_value::<u32>(stack, self.ids.durability)
            .unwrap_or(0)
    }

    pub fn max_stack(&self, item: u32) -> u32 {
        self.items.get(item as usize).map_or(64, |d| d.max_stack)
    }

    pub fn mod_name(&self, id: ModId) -> &str {
        self.mods
            .get(id.0 as usize)
            .map_or("engine", |m| m.id.as_str())
    }

    /// Full name of a state for saves, the network and players: `base:furnace[facing=north,lit=false]`.
    pub fn state_name(&self, state: u32) -> String {
        self.block_of(state).state_name(state)
    }

    /// `name` (its default state) or `name[a=x,..]`, the properties not given at their defaults.
    pub fn state_by_name(&self, name: &str) -> Option<u32> {
        self.parse_state(name).ok()
    }

    /// Like `state_by_name`, saying what is wrong.
    pub fn parse_state(&self, text: &str) -> Result<u32, String> {
        crate::refs::state(text, &self.block_names, &self.blocks)
    }

    /// An item by name.
    pub fn item_by_ref(&self, text: &str) -> Option<u32> {
        crate::refs::one(text, &self.item_names, "item").ok()
    }

    /// The value of property `name` in `state`.
    pub fn state_prop(&self, state: u32, name: &str) -> Option<&str> {
        let b = self.block_of(state);
        let p = b.props.iter().find(|p| p.name == name)?;
        Some(&p.values[(((state - b.first_state) / p.stride) % p.values.len() as u32) as usize])
    }

    /// `state` with property `name` set to `value`.
    pub fn state_with(&self, state: u32, name: &str, value: &str) -> Option<u32> {
        let b = self.block_of(state);
        let pi = b.props.iter().position(|p| p.name == name)?;
        let v = b.props[pi].values.iter().position(|x| x == value)?;
        Some(b.with_value(state, pi, v as u32))
    }
}

// ---------------------------------------------------------------- registration side

/// Owned copy of everything one mod registered. Filled without locks, one per mod thread.
pub struct RawRegistrar {
    pub owner: ModId,
    pub game: crate::registry::RawGame,
    pub errors: Vec<String>,
}

impl RawRegistrar {
    pub fn new(owner: ModId) -> Self {
        RawRegistrar { owner, game: Default::default(), errors: Vec::new() }
    }
}

/// A dimension as its entry says (`comp::dimension`); absent values are vanilla's overworld's.
fn bake_dimension(e: &Entry) -> DimensionDef {
    use abi::comp::dimension as d;
    let [min_cube_y, max_cube_y] = e.or(d::CUBES, [i32::MIN, i32::MAX]);
    let mut daylight: Vec<[f32; 2]> = e
        .or::<Vec<abi::DaylightPoint>>(d::DAYLIGHT, Vec::new())
        .iter()
        .filter(|p| p.turn.is_finite() && p.level.is_finite())
        .map(|p| [p.turn.rem_euclid(1.0), p.level.clamp(0.0, 1.0)])
        .collect();
    daylight.sort_by(|a, b| a[0].total_cmp(&b[0]));
    DimensionDef {
        name: e.name.clone(),
        owner: e.owner,
        generator: e.callback(d::GENERATOR),
        spawn: e.callback(d::SPAWN),
        column: e.callback(d::COLUMN),
        min_cube_y,
        max_cube_y,
        sky_cube_y: e.or(d::SKY_ABOVE, i32::MAX),
        sky_light: !e.has(d::NO_SKY_LIGHT),
        gravity: e.or(d::GRAVITY, 32.0f32),
        day_length: e.or(d::DAY_LENGTH, 24000u32),
        fixed_time: e.or(d::FIXED_TIME, 6000u32),
        sky_color: e.or(d::SKY_COLOR, 0x78a7ffu32),
        fog_color: e.or(d::FOG_COLOR, 0xc0d8ffu32),
        ambient: e.or(d::AMBIENT, 0u32).min(15) as u8,
        player_spawn: e.has(d::PLAYER_SPAWN),
        saved: true,
        editable: true,
        block_updates: !e.has(d::NO_BLOCK_UPDATES),
        builtin: None,
        spawn_at: [0, 64, 0],
        ores: Vec::new(),
        structures: Vec::new(),
        terrain: None,
        bare: e.has(d::BARE),
        weathers: Vec::new(),
        weather_names: e.or(d::WEATHERS, Vec::new()),
        daylight,
        scatters: Vec::new(),
        decorations: Vec::new(),
        column_cache: crate::server::terrain::ColumnCache::new(4096),
        scatter_cache: crate::server::terrain::ColumnCache::new(32768),
    }
}

pub const VOID_DIM: &str = "core:void";

fn void_dimension() -> DimensionDef {
    DimensionDef {
        name: VOID_DIM.into(),
        owner: crate::registry::ENGINE,
        generator: None,
        spawn: None,
        column: None,
        min_cube_y: -4,
        max_cube_y: 4,
        sky_cube_y: i32::MIN,
        sky_light: false,
        gravity: 0.0,
        day_length: 0,
        fixed_time: 18000,
        sky_color: 0x0a0a12,
        fog_color: 0x0a0a12,
        ambient: 15,
        player_spawn: false,
        saved: false,
        editable: false,
        block_updates: false,
        builtin: None,
        spawn_at: [0, 64, 0],
        ores: Vec::new(),
        structures: Vec::new(),
        terrain: None,
        bare: true,
        weathers: Vec::new(),
        weather_names: Vec::new(),
        daylight: Vec::new(),
        scatters: Vec::new(),
        decorations: Vec::new(),
        column_cache: crate::server::terrain::ColumnCache::new(1),
        scatter_cache: crate::server::terrain::ColumnCache::new(1),
    }
}

pub const DEBUG_DIM: &str = "core:debug_blocks";

/// Every block state in a grid two blocks apart, like vanilla's debug world: for checking
/// textures and models at a glance. Nothing in it updates, changes or spawns; no gravity, so
/// players float over the grid.
fn debug_dimension() -> DimensionDef {
    DimensionDef {
        name: DEBUG_DIM.into(),
        sky_cube_y: 1,
        sky_light: true,
        day_length: 24000,
        fixed_time: 6000,
        sky_color: 0x78a7ff,
        fog_color: 0xc0d8ff,
        ambient: 0,
        min_cube_y: -1,
        max_cube_y: 1,
        builtin: Some(debug_blocks),
        spawn_at: [-2, 0, -2],
        ..void_dimension()
    }
}

fn debug_blocks(c: &Content, cube: glam::IVec3, blocks: &mut [u32]) {
    if cube.y != 0 {
        return;
    }
    let n = c.states.len() as i32 - 1;
    let side = (n as f64).sqrt().ceil() as i32;
    for z in 0..16 {
        for x in 0..16 {
            let (wx, wz) = (cube.x * 16 + x, cube.z * 16 + z);
            if !(0..side * 2).contains(&wx) || !(0..side * 2).contains(&wz) {
                continue;
            }
            let i = (wz / 2) * side + wx / 2;
            if wx % 2 == 0 && wz % 2 == 0 && i < n {
                blocks[(256 + z * 16 + x) as usize] = i as u32 + 1;
            }
        }
    }
}

/// Every mod after the mods it depends on, ties by id.
pub fn load_order(mods: &[ModMeta]) -> Vec<u32> {
    let mut placed: Vec<Option<u32>> = vec![None; mods.len()];
    let mut next = 0;
    while next < mods.len() as u32 {
        let ready = (0..mods.len()).find(|&i| {
            placed[i].is_none()
                && mods[i].deps.iter().all(|d| {
                    mods.iter()
                        .position(|m| &m.id == d)
                        .is_none_or(|j| placed[j].is_some())
                })
        });
        // A dependency cycle: place the rest by id rather than loop forever.
        let i = ready.unwrap_or_else(|| (0..mods.len()).find(|&i| placed[i].is_none()).unwrap());
        placed[i] = Some(next);
        next += 1;
    }
    placed.into_iter().map(|p| p.unwrap()).collect()
}

/// Registered tabs in mod order, filled with their tagged items in registration order; every mod
/// with leftover items gets one more tab of its own.
/// Whether the mod a texture's name points at has that file.
fn picture_exists(mods: &[ModMeta], name: &str) -> bool {
    let Some((ns, path)) = name.split_once(':') else { return false };
    mods.iter().find(|m| m.id == ns).is_some_and(|m| {
        m.assets
            .join("textures")
            .join(format!("{path}.png"))
            .is_file()
    })
}

/// A tab's number is its entry's, the engine's spawn egg tab after them.
#[allow(clippy::too_many_arguments)]
fn creative_tabs(
    mods: &[ModMeta],
    raw: Vec<RawEntry>,
    items: &[ItemDef],
    item_names: &FxHashMap<String, u32>,
    c: &Components,
    tables: &mut crate::entries::Tables,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) -> Vec<CreativeTab> {
    let (made, _) = tables.bake(raw, abi::entry_kind::CREATIVE_TAB, mod_name, errors, |e, errors| {
        let icon = e.text(abi::comp::CREATIVE_TAB_ICON).unwrap_or_default();
        let Some(&icon) = item_names.get(icon) else {
            errors.push(format!("creative tab {}: unknown icon item {icon}", e.name));
            return None;
        };
        Some(CreativeTab { name: e.name.clone(), icon, items: Vec::new() })
    });
    let mut tabs: Vec<CreativeTab> = made.into_iter().flatten().collect();
    // Every entity's egg goes here, whichever mod's it is. Its icon is the first of them.
    tabs.push(CreativeTab { name: "core:spawn_eggs".into(), icon: 0, items: Vec::new() });
    // The tag `tab/<name>` of each tab, by component id.
    let tab_of: FxHashMap<u32, usize> = tabs
        .iter()
        .enumerate()
        .filter_map(|(i, t)| c.id(&format!("tab/{}", t.name)).map(|id| (id, i)))
        .collect();
    // Liquids by the millibucket only live in liquid slots.
    let fluid = c.id(abi::comp::FLUID);
    let mut ordered: Vec<u32> = (1..items.len() as u32)
        .filter(|&i| fluid.is_none_or(|f| !c.has(comps::ITEM, i, f)))
        .collect();
    ordered.sort_by_key(|&i| items[i as usize].order);
    let mut leftover: Vec<(usize, Vec<u32>)> = Vec::new();
    let hidden = c.id(abi::comp::NO_TAB);
    for i in ordered
        .into_iter()
        .filter(|&i| hidden.is_none_or(|t| !c.has(comps::ITEM, i, t)))
    {
        let def = &items[i as usize];
        // A misspelt tab would quietly send the item to misc.
        for id in c.of(comps::ITEM, i).filter(|id| !tab_of.contains_key(id)) {
            if let Some(tab) = c.names[id as usize].strip_prefix("tab/") {
                errors.push(format!(
                    "{}: {} is in creative tab {tab}, which no mod registers",
                    mods.get(def.owner.0 as usize)
                        .map_or("core", |m| m.id.as_str()),
                    def.name
                ));
            }
        }
        match c.of(comps::ITEM, i).find_map(|id| tab_of.get(&id).copied()) {
            Some(t) => tabs[t].items.push(i),
            None => match leftover.iter_mut().find(|l| l.0 == def.owner.0 as usize) {
                Some(l) => l.1.push(i),
                None => leftover.push((def.owner.0 as usize, vec![i])),
            },
        }
    }
    for (m, list) in leftover {
        let name = format!("{}:misc", mods.get(m).map_or("core", |m| m.id.as_str()));
        tabs.push(CreativeTab { name, icon: list[0], items: list });
    }
    tabs.retain(|t| !t.items.is_empty());
    for t in tabs.iter_mut().filter(|t| t.icon == 0) {
        t.icon = t.items[0];
    }
    for t in &tabs {
        log::debug!(
            "creative tab {}: {}",
            t.name,
            t.items
                .iter()
                .map(|&i| items[i as usize].name.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    tabs
}

/// One list of every texture by name, for blocks, items and particles alike.
struct TextureList {
    names: Vec<String>,
    index: FxHashMap<String, u32>,
}

impl TextureList {
    fn new() -> Self {
        let mut index = FxHashMap::default();
        index.insert("core:missing".to_string(), 0);
        Self { names: vec!["core:missing".into()], index }
    }

    fn intern(&mut self, name: &str) -> u32 {
        if name.is_empty() {
            return 0;
        }
        if let Some(&i) = self.index.get(name) {
            return i;
        }
        self.names.push(name.to_string());
        self.index
            .insert(name.to_string(), self.names.len() as u32 - 1);
        self.names.len() as u32 - 1
    }
}

struct ItemTables {
    item_defs: Vec<ItemDef>,
    item_values: Vec<(ModId, Vec<RawValue>)>,
    item_names: FxHashMap<String, u32>,
}

struct BiomeTables {
    biome_defs: Vec<BiomeDef>,
    biome_values: Vec<Vec<RawValue>>,
    biome_rules: Vec<abi::BiomeRule>,
    biome_names: FxHashMap<String, u32>,
}

struct DimTables {
    dim_defs: Vec<DimensionDef>,
    dim_values: Vec<(ModId, Vec<RawValue>)>,
    dim_names: FxHashMap<String, u32>,
    spawn_dim: u32,
    void_dim: u32,
}

/// What later stages look names up in, once the components are known.
#[derive(Clone, Copy)]
struct Lookups<'a> {
    block_defs: &'a [BlockDef],
    block_names: &'a FxHashMap<String, u32>,
    dim_names: &'a FxHashMap<String, u32>,
    biome_names: &'a FxHashMap<String, u32>,
    components: &'a Components,
}

/// Merges per-mod registrars into frozen content. Ids are assigned in name order, so they do not
/// depend on which mod finished registering first. The order of the stages is the meaning here:
/// each one reads what the ones before it settled.
pub fn freeze(mut mods: Vec<ModMeta>, regs: Vec<RawRegistrar>) -> Result<Content, Vec<String>> {
    let order = load_order(&mods);
    for (m, o) in mods.iter_mut().zip(&order) {
        m.order = *o;
    }
    let mut errors: Vec<String> = Vec::new();
    let mut games = merge_registrars(&mods, regs, &mut errors);
    let mod_name = |m: ModId| {
        mods.get(m.0 as usize)
            .map_or("the engine".to_string(), |m| m.id.clone())
    };
    // Folded now: templates copy their items.
    let item = abi::entry_kind::ITEM;
    let mut raw_items = vec![RawEntry::engine(item, "core:empty").set(abi::comp::item::TEXTURE, String::new())];
    raw_items.extend(entries_of(&mut games, item));
    let mut items = crate::entries::fold(abi::entry_kind::NAMES[item as usize], raw_items, &mod_name, &mut errors);
    // Folded now: a liquid hot enough has its block burn what touches it.
    let liquid = abi::entry_kind::LIQUID;
    let mut liquids = crate::entries::fold(abi::entry_kind::NAMES[liquid as usize], entries_of(&mut games, liquid), &mod_name, &mut errors);
    liquids.apply_patterns(&mut errors);
    let hot: Vec<String> = liquids
        .entries
        .iter()
        .filter(|e| e.or(abi::comp::liquid::HEAT, 300u32) >= BURNING)
        .map(|e| e.name.clone())
        .collect();
    let mut tables = crate::entries::Tables::default();
    let tint_kinds = freeze_tints(entries_of(&mut games, abi::entry_kind::TINT_KIND), &mut tables, &mod_name, &mut errors);

    let mut textures = TextureList::new();
    let crack_textures: [u32; 10] = std::array::from_fn(|i| textures.intern(&format!("core:block/destroy_{i}")));
    let fire_texture = textures.intern("core:block/fire");
    let raw_blocks = entries_of(&mut games, abi::entry_kind::BLOCK);
    let read_model = |name: &str| model_file(&mods, name);
    let liquid_names: Vec<String> = liquids.entries.iter().map(|e| e.name.clone()).collect();
    let mut block_tables = crate::blocks::freeze(
        raw_blocks,
        &mut items,
        &hot,
        &liquid_names,
        &read_model,
        &mut |t| textures.intern(t),
        &mod_name,
        &mut errors,
    );
    let mut patterns = std::mem::take(&mut block_tables.patterns);
    patterns.extend(items.apply_patterns(&mut errors));
    let tag_patterns = check_patterns(patterns, &mod_name, &mut errors);
    if !errors.is_empty() {
        return Err(errors);
    }
    let shapes = std::mem::take(&mut block_tables.shapes);
    let shares_specs = std::mem::take(&mut block_tables.shares);
    let retextures = std::mem::take(&mut block_tables.retextures);
    let (item_tables, item_looks) = number_items(&mods, items, &block_tables, &retextures, &mut textures, &mut errors);
    let biome_tables = number_biomes(entries_of(&mut games, abi::entry_kind::BIOME), &tint_kinds, &mod_name, &mut errors);
    let dim_tables = number_dimensions(&mods, entries_of(&mut games, abi::entry_kind::DIMENSION), &mod_name, &mut errors);
    let entities = crate::registry::freeze_entities(entries_of(&mut games, abi::entry_kind::ENTITY), &mod_name, &mut errors);

    let decls = crate::registry::freeze_components(&mut games, &mod_name, &mut errors);
    let builder = declare_components(
        &mods,
        &games,
        &decls,
        &block_tables,
        &item_tables,
        &entities,
        &biome_tables,
        &dim_tables,
        &liquids,
        &tables,
        &mut errors,
    );
    if !errors.is_empty() {
        return Err(errors);
    }
    let (mut components, ref_lists, named_matchers) =
        fill_components(builder, &block_tables, &item_tables, &entities, &biome_tables, &dim_tables, &tint_kinds, &mut errors);
    // In the order the values got their ids.
    let mut matchers = crate::refs::Matchers::new(block_tables.states.len());
    for (text, who) in &named_matchers {
        if let Err(e) = matchers.add(text, &block_tables.block_names, &block_tables.block_defs, &components) {
            errors.push(format!("{who}: {e}"));
        }
    }
    for (m, pattern) in tag_patterns {
        let tag = &pattern[1..];
        if !(0..components.count()).any(|c| components.is_tag(c) && comps::matches(tag, &components.names[c as usize])) {
            errors.push(format!("{}: {pattern} matches no tag", mod_name(m)));
        }
    }
    let crate::blocks::Blocks { mut states, mut block_defs, opens_names, block_names, rules: raw_rules, hazards: raw_hazards, .. } =
        block_tables;
    let ItemTables { mut item_defs, item_names, .. } = item_tables;
    let BiomeTables { biome_defs, biome_rules, biome_names, .. } = biome_tables;
    let DimTables { mut dim_defs, dim_names, spawn_dim, void_dim, .. } = dim_tables;
    let ids = component_ids(&components);
    flatten_components(&mut states, &mut item_defs, &components, &ids);
    let mut conds = crate::conditions::Texts::default();
    resolve_rules(&mut states, &block_defs, &block_names, raw_rules, &mut conds, &mut errors);
    for b in &block_defs {
        let grows = (b.first_state..b.first_state + b.states).any(|s| {
            states[s as usize]
                .rules
                .iter()
                .any(|r| r.then == Then::Grow)
        });
        if grows && b.cb.on_grow.is_none() && !b.props.iter().any(|p| p.name == "age") {
            let owner = mods
                .get(b.owner.0 as usize)
                .map_or("engine", |m| m.id.as_str());
            errors.push(format!("{owner}: block {} grows, but has no `age` property and no on_grow", b.name));
        }
    }
    let tool_classes = tool_classes(&components);

    let look = Lookups {
        block_defs: &block_defs,
        block_names: &block_names,
        dim_names: &dim_names,
        biome_names: &biome_names,
        components: &components,
    };
    place_features(&mut games, &mut dim_defs, look, &mut matchers, &mut tables, &mod_name, &mut errors);
    let (templates, template_names) = freeze_templates(&mut games, look, &mut tables, &mod_name, &mut errors);
    place_structures(&mut games, &mut dim_defs, &templates, &template_names, look, &mut tables, &mod_name, &mut errors);
    build_terrains(&mut games, &mut dim_defs, &biome_names, &biome_rules, look, &mut tables, &mod_name, &mut errors);
    let commands = freeze_commands(&mods, &mut games, &mut errors);
    let mut word_sets = collect_word_sets(&mut games, &mod_name, &mut errors);
    let (liquids, reactions) = freeze_liquids(liquids, &mut games, &mut states, look, &mut matchers, &mut tables, &mod_name, &mut errors);
    let mut shares_cell: FxHashMap<u32, u32> = FxHashMap::default();
    for (name, text) in &shares_specs {
        let Some(&block) = block_names.get(name) else { continue };
        if let Some(m) = look.matcher(&mut matchers, text, &format!("{name} shares_cell_with"), &mut errors) {
            shares_cell.insert(block, m);
        }
    }
    let (item_liquid, containers) = item_containers(&item_defs, &liquids, &components, &mut errors);
    let (fluid_items, item_fluid) = fluid_items(&item_defs, &liquids, &components, &mut errors);
    let tabs = entries_of(&mut games, abi::entry_kind::CREATIVE_TAB);
    let creative_tabs = creative_tabs(&mods, tabs, &item_defs, &item_names, &components, &mut tables, &mod_name, &mut errors);
    let loot = freeze_loot(&mut games, &item_defs, &item_names, look, &mut tables, &mut conds, &mod_name, &mut errors);
    let stack_flags = collect_stack_flags(&decls, &components);
    let order_of = |m: ModId| mods.get(m.0 as usize).map_or(u32::MAX, |m| m.order);
    let mut game = crate::registry::freeze(
        games,
        entities,
        &block_names,
        &item_names,
        &item_defs,
        &dim_names,
        &biome_names,
        &components,
        &decls,
        &order_of,
        &mut tables,
        &mut conds,
        &mod_name,
        &mut errors,
    );
    resolve_hazards(&mut states, raw_hazards, &game, &components, &mut conds, &mut errors);
    let source_mods = resolve_source_mods(&game, &components, item_defs.len(), &mut conds, &mut errors);
    let conds = crate::conditions::compile_all(
        &conds,
        &crate::conditions::Look { game: &game, block_names: &block_names, blocks: &block_defs, components: &components },
        &mut errors,
    );
    for (kind, entries) in &tables.0 {
        let mut matcher = |text: &str, c: &Components| matchers.add(text, &block_names, &block_defs, c);
        components.add_table(*kind as usize, entries, &mut matcher, &mut errors);
    }
    let ref_lists = std::mem::take(&mut *ref_lists.borrow_mut());
    game.item_model_for_stack = item_looks;
    link_particles(&mut game, &mut textures);
    check_ambient_particles(&game, &components, states.len() as u32, &mut errors);
    link_entity_templates(&mut game, &templates, &template_names, &mut errors);
    link_equipment(&game, &mut item_defs, &components);
    for d in &mut dim_defs {
        let names = std::mem::take(&mut d.weather_names);
        if names.is_empty() {
            continue;
        }
        let count = game.weathers.len() as u32;
        match crate::refs::set(&names.join(" "), abi::entry_kind::WEATHER as usize, &game.weather_names, count, &components, "weather") {
            Ok(w) => d.weathers = w,
            Err(e) => errors.push(format!("dimension {}: {e}", d.name)),
        }
    }
    // `immune/fall`, `immune/mymod:radiation`: the engine's causes go without their namespace.
    let damage_immune = game
        .damage_causes
        .iter()
        .map(|d| components.id(&format!("immune/{}", d.name.strip_prefix("core:").unwrap_or(&d.name))))
        .collect();
    link_menus(&mut game, &mut block_defs, &item_defs, &opens_names, &mods, &mut errors);
    let biome_precipitation = (0..biome_defs.len() as u32)
        .map(|b| {
            let own = components
                .id(abi::comp::PRECIPITATION)
                .and_then(|id| components.text(comps::BIOME, b, id));
            match own {
                Some(name) => game
                    .precipitation_names
                    .get(name)
                    .copied()
                    .unwrap_or_else(|| {
                        errors.push(format!("biome {}: no precipitation {name}", biome_defs[b as usize].name));
                        0
                    }) as u16,
                None => 0,
            }
        })
        .collect();
    word_sets.entry("core:weathers".into()).or_default().extend(
        game.weathers
            .iter()
            .map(|m| m.name.strip_prefix("core:").unwrap_or(&m.name).to_string()),
    );
    word_sets
        .entry("core:game_modes".into())
        .or_default()
        .extend(
            game.game_modes
                .iter()
                .map(|m| m.name.strip_prefix("core:").unwrap_or(&m.name).to_string()),
        );
    word_sets.entry("core:rules".into()).or_default().extend(
        game.configs
            .iter()
            .filter(|d| d.scope == abi::config_scope::WORLD)
            .map(|d| d.name.clone()),
    );
    check_loot_smelting(&loot, &game, &mut errors);
    resolve_effects(&mut states, &mut game, &block_defs, &components, &mut errors);
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut spread_targets = vec![0u64; states.len().div_ceil(64)];
    for sp in states.iter().filter_map(|s| s.spread) {
        spread_targets
            .iter_mut()
            .zip(matchers.set(sp.to))
            .for_each(|(t, m)| *t |= m);
    }
    // Where a target looks for what spreads onto it: every reach, mirrored.
    let spread_search = states
        .iter()
        .filter_map(|s| s.spread)
        .fold((IVec3::ZERO, IVec3::ZERO), |(lo, hi), sp| (lo.min(-sp.max), hi.max(-sp.min)));
    let tight = states
        .iter()
        .map(|s| Tight { flags: s.flags, opacity: s.opacity, light: s.light, absorb: s.absorb, seals: seals_of(s) })
        .collect();
    let liquid_walls = states
        .iter()
        .map(|s| if s.liquid_walls == u8::MAX { covered_faces(s) } else { s.liquid_walls })
        .collect();
    Ok(Content {
        liquid_walls,
        spread_targets,
        spread_search,
        mods,
        tight,
        states,
        blocks: block_defs,
        items: item_defs,
        biomes: biome_defs,
        templates,
        template_names,
        ref_lists,
        word_sets,
        tint_kinds,
        dims: dim_defs,
        components,
        ids,
        shapes: shapes.into_iter().map(|(n, s, k)| (n, (s, k))).collect(),
        shares_cell,
        matchers,
        conds,
        cond_cache: Default::default(),
        source_mods,
        tool_classes,
        textures: textures.names,
        crack_textures,
        fire_texture,
        commands,
        block_names,
        item_names,
        creative_tabs,
        dim_names,
        spawn_dim,
        void_dim,
        biome_names,
        game,
        liquids,
        reactions,
        item_liquid,
        fluid_items,
        item_fluid,
        containers,
        loot,
        stack_flags,
        damage_immune,
        biome_precipitation,
    })
}

fn merge_registrars(mods: &[ModMeta], mut regs: Vec<RawRegistrar>, errors: &mut Vec<String>) -> Vec<crate::registry::RawGame> {
    // Everything goes in load order: of two mods' say on one thing the later one's wins.
    regs.sort_by_key(|r| mods[r.owner.0 as usize].order);
    let mut games = Vec::new();
    for r in regs {
        games.push(r.game);
        errors.extend(
            r.errors
                .into_iter()
                .map(|e| format!("{}: {e}", mods[r.owner.0 as usize].id)),
        );
    }
    games
}

/// Edits by pattern of blocks and items: one naming nothing of either is a typo; a tag pattern
/// is checked against the tags there are once they are known, the patterns given back.
fn check_patterns(
    patterns: Vec<(ModId, String, bool)>,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) -> Vec<(ModId, String)> {
    let mut matched: Vec<(ModId, String, bool)> = Vec::new();
    for (m, name, hit) in patterns {
        match matched.iter_mut().find(|p| p.0 == m && p.1 == name) {
            Some(p) => p.2 |= hit,
            None => matched.push((m, name, hit)),
        }
    }
    let mut tags = Vec::new();
    for (m, name, _) in matched.into_iter().filter(|p| !p.2) {
        if name.starts_with('#') {
            tags.push((m, name));
        } else {
            errors.push(format!("{}: {} names no block or item", mod_name(m), name.trim_end_matches("[]")));
        }
    }
    tags
}

fn model_file(mods: &[ModMeta], name: &str) -> Option<String> {
    let (ns, path) = name.split_once(':')?;
    let dir = &mods.iter().find(|m| m.id == ns)?.assets;
    std::fs::read_to_string(dir.join("models").join(format!("{path}.json"))).ok()
}

fn freeze_tints(
    raw: Vec<RawEntry>,
    tables: &mut crate::entries::Tables,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) -> Vec<(String, u32)> {
    let mut all: Vec<RawEntry> = [("core:grass", 0x8db360u32), ("core:foliage", 0x77ab2f), ("core:water", 0x3f76e4)]
        .iter()
        .map(|&(name, color)| RawEntry::engine(abi::entry_kind::TINT_KIND, name).set(abi::comp::TINT_KIND_DEFAULT, color))
        .collect();
    all.extend(raw);
    let (mut tint_kinds, _) = tables
        .bake(all, abi::entry_kind::TINT_KIND, mod_name, errors, |e, _| (e.name.clone(), e.or(abi::comp::TINT_KIND_DEFAULT, 0xFFFFFFu32)));
    if tint_kinds.len() > abi::tint::MAX_KINDS as usize {
        errors.push(format!("{} tint kinds registered, at most {} fit", tint_kinds.len(), abi::tint::MAX_KINDS));
    }
    tint_kinds.truncate(abi::tint::MAX_KINDS as usize);
    tint_kinds
}

/// Ids in name order, `core:empty` first. Also gives each item's `comp::item::CONDITION`.
fn number_items(
    mods: &[ModMeta],
    items: crate::entries::Folded,
    blocks: &crate::blocks::Blocks,
    retextures: &[(String, Vec<(String, String)>)],
    textures: &mut TextureList,
    errors: &mut Vec<String>,
) -> (ItemTables, FxHashMap<u32, (ModId, Callback)>) {
    use abi::comp::item as it;
    let crate::blocks::Blocks { block_defs, block_names, .. } = blocks;
    let read_model = |name: &str| model_file(mods, name);
    let order: FxHashMap<String, u64> = items
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.clone(), i as u64))
        .collect();
    let by_name = |e: &Entry| (e.owner != crate::registry::ENGINE, e.name.clone());
    let (made, item_names) = crate::entries::finish(items, errors, by_name, |e, errors| {
        let placed = e.text(it::BLOCK).filter(|b| !b.is_empty());
        let block = placed.and_then(|b| match block_names.get(b) {
            Some(&b) => Some(block_defs[b as usize].first_state),
            None => {
                errors.push(format!("item {} places unknown block {b}", e.name));
                None
            }
        });
        let (ns, path) = e.name.split_once(':').unwrap_or(("core", &e.name));
        let mut texture = e
            .text(it::TEXTURE)
            .map_or_else(|| format!("{ns}:item/{path}"), str::to_string);
        for (_, pairs) in retextures
            .iter()
            .filter(|r| r.0 == e.name || Some(r.0.as_str()) == placed)
        {
            if let Some((_, to)) = pairs.iter().find(|p| p.0 == texture) {
                texture = to.clone();
            }
        }
        // An egg its mod drew no picture for. Packs can still replace the generic one.
        if e.has(abi::comp::SPAWN_EGG) && !picture_exists(mods, &texture) {
            texture = "core:item/spawn_egg".into();
        }
        let mut icon = if texture.is_empty() && block.is_some() { u32::MAX } else { textures.intern(&texture) };
        // Its model file: a flat item of another texture, or boxes (faces it leaves to the item
        // show the icon).
        let mut model = Vec::new();
        match crate::block_model::load(&format!("{ns}:item/{path}"), &read_model) {
            Ok(Some(m)) => {
                if let Some(t) = m.sprite.filter(|t| !t.is_empty()) {
                    icon = textures.intern(&t);
                }
                let own = if icon == u32::MAX { 0 } else { icon };
                model = m
                    .boxes
                    .iter()
                    .map(|b| b.model_box(|t, _| if t.is_empty() { own } else { textures.intern(t) }))
                    .collect();
            }
            Ok(None) => {}
            Err(err) => errors.push(err),
        }
        let def = ItemDef {
            name: e.name.clone(),
            owner: e.owner,
            max_stack: 64,
            durability: 0,
            attack: 1.0,
            icon,
            block,
            on_use: e.callback(it::ON_USE),
            order: order[&e.name],
            equip: None,
            handheld: false,
            model,
        };
        (def, (e.owner, e.components("item.")), e.callback(it::CONDITION))
    });
    let mut looks = FxHashMap::default();
    let (mut item_defs, mut item_values) = (Vec::new(), Vec::new());
    for (i, (def, values, look)) in made.into_iter().enumerate() {
        item_defs.push(def);
        item_values.push(values);
        looks.extend(look.map(|l| (i as u32, l)));
    }
    (ItemTables { item_defs, item_values, item_names }, looks)
}

/// Ids in name order.
fn number_biomes(
    raw: Vec<RawEntry>,
    tint_kinds: &[(String, u32)],
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) -> BiomeTables {
    use abi::comp::biome as b;
    let kind = abi::entry_kind::BIOME;
    let (made, biome_names) = crate::entries::bake_by(
        raw,
        kind,
        mod_name,
        errors,
        |e| e.name.clone(),
        |e, errors| {
            let mut tints: Vec<u32> = tint_kinds.iter().map(|k| k.1).collect();
            for t in e.or::<Vec<abi::BiomeTint>>(b::TINTS, Vec::new()) {
                match tint_kinds.iter().position(|k| k.0 == t.kind) {
                    Some(i) => tints[i] = t.color,
                    None => errors.push(format!("biome {}: no tint kind {}", e.name, t.kind)),
                }
            }
            let text = |name: &str| e.text(name).unwrap_or_default().to_string();
            let mut ranges: Vec<abi::RuleRange> = Vec::new();
            for r in e.or::<Vec<abi::RuleRange>>(b::RANGES, Vec::new()) {
                ranges.retain(|x| x.name != r.name);
                ranges.push(r);
            }
            let rule = abi::BiomeRule {
                name: e.name.clone(),
                fields: 0,
                biome: e.name.clone(),
                ranges,
                height: e.or(b::HEIGHT, [i32::MIN, i32::MAX]),
                top: text(b::TOP),
                filler: text(b::FILLER),
                depth: e.or(b::DEPTH, 3),
                under: text(b::UNDER),
                under_layers: e.or(b::UNDER_LAYERS, 0),
                deep_top: text(b::DEEP_TOP),
                deep_below: e.or(b::DEEP_BELOW, i32::MIN),
                bare_above: e.or(b::BARE_ABOVE, i32::MAX),
                ice: text(b::ICE),
            };
            (BiomeDef { name: e.name.clone(), tints }, e.components("biome."), rule)
        },
    );
    // Cubes and face records keep 16 bits; the top one stays free.
    if made.len() > 32768 {
        errors.push(format!("{} biomes registered, at most 32768 fit", made.len()));
    }
    let (mut biome_defs, mut biome_values, mut biome_rules) = (Vec::new(), Vec::new(), Vec::new());
    for (d, v, r) in made {
        biome_defs.push(d);
        biome_values.push(v);
        biome_rules.push(r);
    }
    BiomeTables { biome_defs, biome_values, biome_rules, biome_names }
}

/// Ids in name order, the engine's after the mods'.
fn number_dimensions(mods: &[ModMeta], raw: Vec<RawEntry>, mod_name: &dyn Fn(ModId) -> String, errors: &mut Vec<String>) -> DimTables {
    let kind = abi::entry_kind::DIMENSION;
    let (made, _) = crate::entries::bake_by(
        raw,
        kind,
        mod_name,
        errors,
        |e| e.name.clone(),
        |e, _| (bake_dimension(e), (e.owner, e.components("dimension."))),
    );
    let (mut dim_defs, mut dim_values): (Vec<DimensionDef>, Vec<_>) = made.into_iter().unzip();
    // Always there, so a dimension lookup never fails: where players go when no mod adds a
    // dimension (or theirs is gone).
    let void_dim = dim_defs.len() as u32;
    dim_defs.push(void_dimension());
    dim_values.push((crate::registry::ENGINE, Vec::new()));
    dim_defs.push(debug_dimension());
    dim_values.push((crate::registry::ENGINE, Vec::new()));
    let order_of_mod = |m: ModId| mods.get(m.0 as usize).map_or(0, |m| m.order);
    let spawn_dim = dim_defs[..void_dim as usize]
        .iter()
        .enumerate()
        .filter(|(_, d)| d.player_spawn)
        .max_by_key(|(_, d)| order_of_mod(d.owner))
        .or_else(|| {
            dim_defs[..void_dim as usize]
                .iter()
                .enumerate()
                .min_by_key(|(_, d)| order_of_mod(d.owner))
        })
        .map_or(void_dim, |(i, _)| i as u32);
    let dim_names: FxHashMap<String, u32> = dim_defs
        .iter()
        .enumerate()
        .map(|(i, d)| (d.name.clone(), i as u32))
        .collect();
    DimTables { dim_defs, dim_values, dim_names, spawn_dim, void_dim }
}

#[allow(clippy::too_many_arguments)]
fn declare_components(
    mods: &[ModMeta],
    games: &[crate::registry::RawGame],
    decls: &[crate::registry::ComponentDecl],
    blocks: &crate::blocks::Blocks,
    items: &ItemTables,
    entities: &crate::registry::Entities,
    biomes: &BiomeTables,
    dims: &DimTables,
    liquids: &crate::entries::Folded,
    tables: &crate::entries::Tables,
    errors: &mut Vec<String>,
) -> comps::Builder {
    let crate::blocks::Blocks { block_defs, block_values, .. } = blocks;
    let ItemTables { item_defs, item_values, .. } = items;
    let BiomeTables { biome_defs, biome_values, .. } = biomes;
    let DimTables { dim_defs, dim_values, .. } = dims;
    let mut cb = comps::Builder::default();
    let who = |m: ModId, entry: &str| format!("{}: {entry}", mods.get(m.0 as usize).map_or("engine", |m| m.id.as_str()));
    for (i, (owner, sv)) in block_values.iter().enumerate() {
        let name = &block_defs[i].name;
        for v in sv.iter().flatten() {
            cb.declare_value(v, &|| who(*owner, name), errors);
        }
    }
    for (i, (owner, v)) in item_values.iter().enumerate() {
        for v in v {
            cb.declare_value(v, &|| who(*owner, &item_defs[i].name), errors);
        }
    }
    for (k, (owner, v)) in entities.values.iter().enumerate() {
        for v in v {
            cb.declare_value(v, &|| who(*owner, &entities.kinds[k].name), errors);
        }
    }
    for (i, v) in biome_values.iter().enumerate() {
        for v in v {
            cb.declare_value(v, &|| format!("biome {}", biome_defs[i].name), errors);
        }
    }
    for (i, (owner, v)) in dim_values.iter().enumerate() {
        for v in v {
            cb.declare_value(v, &|| who(*owner, &dim_defs[i].name), errors);
        }
    }
    // The kinds whose tables come later, the engine's entries and settings among them.
    let engine: Vec<RawEntry> = (0..abi::entry_kind::COUNT as u32)
        .flat_map(crate::registry::engine_entries)
        .chain(crate::config::engine_configs(&[]))
        .collect();
    let later = |k: u32| k > abi::entry_kind::DIMENSION && !crate::entries::NO_TABLE.contains(&k);
    for e in games
        .iter()
        .flat_map(|g| &g.entries)
        .chain(&engine)
        .filter(|e| later(e.kind))
    {
        let who = || who(e.owner, &format!("{} {}", abi::entry_kind::NAMES[e.kind as usize], e.name));
        for o in &e.ops {
            use abi::value_op as vo;
            match o.op {
                vo::SET => cb.declare_value(&o.value, &who, errors),
                vo::PUSH | vo::INSERT_BEFORE | vo::INSERT_AFTER => {
                    cb.declare(&o.value.name, &format!("[{}]", o.value.schema), None, &who, errors)
                }
                _ => {}
            }
        }
    }
    for e in &liquids.entries {
        for v in &e.values {
            cb.declare_value(v, &|| who(e.owner, &format!("liquid {}", e.name)), errors);
        }
    }
    // Kinds baked before this, for what the others needed of them.
    for (kind, entries) in &tables.0 {
        for e in entries {
            for v in &e.values {
                cb.declare_value(v, &|| who(e.owner, &format!("{} {}", abi::entry_kind::NAMES[*kind as usize], e.name)), errors);
            }
        }
    }
    for d in decls {
        let size = d.entity.as_ref().map(|c| c.size as usize);
        cb.declare(&d.name, &d.schema, size, &|| who(d.owner, "component"), errors);
    }
    cb
}

#[allow(clippy::too_many_arguments)]
fn fill_components(
    mut cb: comps::Builder,
    blocks: &crate::blocks::Blocks,
    items: &ItemTables,
    entities: &crate::registry::Entities,
    biomes: &BiomeTables,
    dims: &DimTables,
    tint_kinds: &[(String, u32)],
    errors: &mut Vec<String>,
) -> (Components, std::rc::Rc<std::cell::RefCell<Vec<Vec<[u32; 2]>>>>, Vec<(String, String)>) {
    let crate::blocks::Blocks { states, block_defs, block_values, .. } = blocks;
    let ItemTables { item_defs, item_values, item_names } = items;
    let BiomeTables { biome_defs, biome_values, .. } = biomes;
    let DimTables { dim_defs, dim_values, .. } = dims;
    let tint_names: Vec<String> = tint_kinds.iter().map(|k| k.0.clone()).collect();
    cb.start(&[states.len(), item_defs.len(), entities.kinds.len(), biome_defs.len(), dim_defs.len()]);
    // References in values (`block`, `blocks`, `item`) become ids here, so a typo is a load error.
    let ref_lists: std::rc::Rc<std::cell::RefCell<Vec<Vec<[u32; 2]>>>> = Default::default();
    {
        let blocks: FxHashMap<String, (u32, Vec<Prop>)> = block_defs
            .iter()
            .map(|b| (b.name.clone(), (b.first_state, b.props.clone())))
            .collect();
        let lists = ref_lists.clone();
        let items = item_names.clone();
        let state = move |text: &str| -> Result<u32, String> {
            let (name, props) = split_state(text)?;
            let (first, defs) = blocks
                .get(name)
                .ok_or_else(|| format!("unknown block {name}"))?;
            let mut s = *first;
            for (k, v) in props {
                let p = defs
                    .iter()
                    .find(|p| p.name == k)
                    .ok_or_else(|| format!("{name} has no property {k}"))?;
                let vi = p
                    .values
                    .iter()
                    .position(|x| x == v)
                    .ok_or_else(|| format!("{name} property {k} has no value {v}"))?;
                s += vi as u32 * p.stride;
            }
            Ok(s)
        };
        cb.refs = Some(Box::new(move |schema: &str, text: &str| match schema {
            "block" => state(text.trim()).map(|s| s.to_le_bytes().to_vec()),
            "item" => items
                .get(text.trim())
                .map(|i| i.to_le_bytes().to_vec())
                .ok_or_else(|| format!("unknown item {}", text.trim())),
            "tint" => tint_names
                .iter()
                .position(|k| k == text.trim())
                .map(|i| (i as u32 + 1).to_le_bytes().to_vec())
                .ok_or_else(|| format!("no tint kind {}", text.trim())),
            _ => {
                let mut list = Vec::new();
                for e in text.split_whitespace() {
                    // `name:weight`, the weight after the last colon when it is a number.
                    let (name, weight) = match e.rsplit_once(':') {
                        Some((n, w)) if w.parse::<u32>().is_ok() => (n, w.parse().unwrap()),
                        _ => (e, 1),
                    };
                    list.push([state(name)?, weight]);
                }
                let mut all = lists.borrow_mut();
                all.push(list);
                Ok(((all.len() - 1) as u32).to_le_bytes().to_vec())
            }
        }));
    }
    for (i, (_, sv)) in block_values.iter().enumerate() {
        let d = &block_defs[i];
        for (k, values) in sv.iter().enumerate() {
            for v in values {
                cb.put(comps::BLOCK, d.first_state + k as u32, v, &|| d.name.clone());
            }
        }
    }
    for (i, (_, v)) in item_values.iter().enumerate() {
        for v in v {
            cb.put(comps::ITEM, i as u32, v, &|| item_defs[i].name.clone());
        }
    }
    // A block's item carries its block's components, unless it sets the same ones itself.
    for (i, d) in item_defs.iter().enumerate() {
        if let Some(s) = d.block {
            cb.inherit((comps::BLOCK, s), (comps::ITEM, i as u32));
        }
    }
    for (k, (_, v)) in entities.values.iter().enumerate() {
        for v in v {
            cb.put(comps::ENTITY, k as u32, v, &|| entities.kinds[k].name.clone());
        }
    }
    for (i, v) in biome_values.iter().enumerate() {
        for v in v {
            cb.put(comps::BIOME, i as u32, v, &|| biome_defs[i].name.clone());
        }
    }
    for (i, (_, v)) in dim_values.iter().enumerate() {
        for v in v {
            cb.put(comps::DIMENSION, i as u32, v, &|| dim_defs[i].name.clone());
        }
    }
    errors.append(&mut cb.errors);
    let matchers = std::mem::take(&mut cb.matchers);
    (cb.finish(), ref_lists, matchers)
}

fn component_ids(components: &Components) -> Ids {
    let id = |n: &str| components.id(n);
    Ids {
        max_stack: id(abi::comp::MAX_STACK),
        durability: id(abi::comp::DURABILITY),
        attack_damage: id(abi::comp::ATTACK_DAMAGE),
        mining_speed: id(abi::comp::MINING_SPEED),
        tool_level: id(abi::comp::TOOL_LEVEL),
        use_priority: id(abi::comp::USE_PRIORITY),
        two_handed: id(abi::comp::TWO_HANDED),
        one_handed: id(abi::comp::ONE_HANDED),
        harvest_level: id(abi::comp::HARVEST_LEVEL),
        damage: id(abi::comp::WEAR),
        name: id(abi::comp::NAME),
        conductor: id(abi::comp::CONDUCTOR),
        worn: id(abi::comp::WORN),
        loot_rolls: id(abi::comp::LOOT_ROLLS),
        worn_texture: id(abi::comp::WORN_TEXTURE),
        wear_per_hit: id(abi::comp::WEAR_PER_HIT),
        spawn_egg: id(abi::comp::SPAWN_EGG),
        attack_speed: id(abi::comp::ATTACK_SPEED),
        use_ticks: id(abi::comp::USE_TICKS),
        sweeps: id(abi::comp::SWEEPS),
        no_creative_break: id(abi::comp::NO_CREATIVE_BREAK),
        inventory: id(abi::comp::INVENTORY),
        contents: id(abi::comp::CONTENTS),
        layers: id(abi::comp::LAYERS),
        energy: id(abi::comp::ENERGY),
    }
}

/// What hot code reads per state (physics, light, breaking) goes into flat fields.
fn flatten_components(states: &mut [StateInfo], item_defs: &mut [ItemDef], components: &Components, ids: &Ids) {
    let id = |n: &str| components.id(n);
    let get = |state: u32, name: &str| id(name).and_then(|c| components.bytes(comps::BLOCK, state, c));
    let f32_of = |state: u32, name: &str| get(state, name).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let u32_of = |state: u32, name: &str| get(state, name).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    for (i, s) in states.iter_mut().enumerate() {
        let st = i as u32;
        s.conductor = ids
            .conductor
            .is_some_and(|c| components.has(comps::BLOCK, st, c));
        s.hardness = f32_of(st, abi::comp::HARDNESS).unwrap_or(s.hardness);
        s.friction = f32_of(st, abi::comp::FRICTION).unwrap_or(s.friction);
        s.drag = f32_of(st, abi::comp::DRAG).unwrap_or(s.drag);
        if let Some(l) = get(st, abi::comp::LIGHT) {
            s.light = [l[0], l[1], l[2]];
        }
        if let Some(t) = u32_of(st, abi::comp::TINT) {
            s.tint = t as u8;
        }
        if let Some(f) = get(st, abi::comp::TINT_FACES) {
            s.tint_faces = s.turn.faces(f[0]);
        }
        if let Some(to) = u32_of(st, abi::comp::SPREADS_TO) {
            let secs = f32_of(st, abi::comp::SPREAD_TIME).unwrap_or(765.0);
            let (min, max) = match get(st, abi::comp::SPREAD_REACH).filter(|b| b.len() >= 24) {
                Some(b) => {
                    let v: Vec<i32> = b
                        .chunks(4)
                        .take(6)
                        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]).clamp(-4, 4))
                        .collect();
                    let (a, b) = (IVec3::new(v[0], v[1], v[2]), IVec3::new(v[3], v[4], v[5]));
                    (a.min(b), a.max(b))
                }
                None => (IVec3::new(-1, -3, -1), IVec3::new(1, 1, 1)),
            };
            s.spread = Some(Spread {
                to,
                mean: secs as f64 * crate::world::TPS as f64,
                light: u32_of(st, abi::comp::SPREAD_LIGHT).unwrap_or(9),
                min,
                max,
                open_top: !id(abi::comp::SPREADS_COVERED).is_some_and(|t| components.has(comps::BLOCK, st, t)),
            });
        }
    }
    for (i, d) in item_defs.iter_mut().enumerate() {
        let i = i as u32;
        d.max_stack = components
            .get::<u32>(comps::ITEM, i, ids.max_stack.unwrap_or(u32::MAX))
            .unwrap_or(64)
            .max(1);
        d.durability = components
            .get::<u32>(comps::ITEM, i, ids.durability.unwrap_or(u32::MAX))
            .unwrap_or(0);
        d.attack = components
            .get::<f32>(comps::ITEM, i, ids.attack_damage.unwrap_or(u32::MAX))
            .unwrap_or(1.0);
    }
}

fn tool_classes(components: &Components) -> Vec<(u32, Option<u32>)> {
    components
        .names
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            n.strip_prefix("mineable/")
                .map(|class| (i as u32, components.id(&format!("tools/{class}"))))
        })
        .collect()
}

impl Lookups<'_> {
    /// The id of a matcher, compiled now if new; an error says `what` named it.
    fn matcher(&self, m: &mut crate::refs::Matchers, text: &str, what: &str, errors: &mut Vec<String>) -> Option<u32> {
        m.add(text, self.block_names, self.block_defs, self.components)
            .map_err(|e| errors.push(format!("{what}: {e}")))
            .ok()
    }

    fn first_state(&self, block: &str) -> Option<u32> {
        self.block_names
            .get(block)
            .map(|&b| self.block_defs[b as usize].first_state)
    }

    /// The dimensions an entry names (`comp::common::DIMENSIONS`), or every one but bare ones.
    fn targets(&self, dim_defs: &[DimensionDef], what: &str, e: &Entry, errors: &mut Vec<String>) -> Vec<usize> {
        let names: Vec<String> = e.or(abi::comp::common::DIMENSIONS, Vec::new());
        if names.is_empty() {
            return (0..dim_defs.len()).filter(|&d| !dim_defs[d].bare).collect();
        }
        let count = dim_defs.len() as u32;
        crate::refs::set(&names.join(" "), comps::DIMENSION, self.dim_names, count, self.components, "dimension")
            .map_err(|err| errors.push(format!("{what}: {err}")))
            .unwrap_or_default()
            .into_iter()
            .map(|d| d as usize)
            .collect()
    }

    fn biomes(&self, what: &str, e: &Entry, errors: &mut Vec<String>) -> Result<Option<Box<[u64]>>, ()> {
        biomes(self.components, self.biome_names, what, e, errors)
    }
}

/// The biomes an entry's sites go in (`comp::common::BIOME`, any reference to biomes), a bit
/// each; `Err` (logged) for a name that isn't there.
pub(crate) fn biomes(
    components: &Components,
    names: &FxHashMap<String, u32>,
    what: &str,
    e: &Entry,
    errors: &mut Vec<String>,
) -> Result<Option<Box<[u64]>>, ()> {
    match e.text(abi::comp::common::BIOME).unwrap_or_default() {
        "" => Ok(None),
        t => crate::refs::bits(t, comps::BIOME, names, names.len() as u32, components, "biome")
            .map(|b| Some(b.into()))
            .map_err(|err| errors.push(format!("{what}: biome: {err}"))),
    }
}

/// The heights an entry names (`comp::common::Y`), any by default.
fn heights(e: &Entry) -> [i32; 2] {
    e.or(abi::comp::common::Y, [i32::MIN, i32::MAX])
}

/// Each def in every dimension its entry goes in.
fn place<D: Clone>(dim_defs: &mut [DimensionDef], placed: Vec<Option<(Vec<usize>, D)>>, list: fn(&mut DimensionDef) -> &mut Vec<D>) {
    for (dims, def) in placed.into_iter().flatten() {
        for d in dims {
            list(&mut dim_defs[d]).push(def.clone());
        }
    }
}

/// Ores, scatters and decorations go into the dimensions they name, or every one but bare ones;
/// ores and scatters in name order, decorations in load order.
fn place_features(
    games: &mut [crate::registry::RawGame],
    dim_defs: &mut [DimensionDef],
    look: Lookups,
    matchers: &mut crate::refs::Matchers,
    tables: &mut crate::entries::Tables,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) {
    use abi::comp::{ore as o, scatter as s};
    use abi::entry_kind as ek;
    let Lookups { block_defs, block_names, components, .. } = look;
    let by_name = |e: &Entry| e.name.clone();
    let (ores, _) = tables.bake_by(entries_of(games, ek::ORE), ek::ORE, mod_name, errors, by_name, |e, errors| {
        let what = format!("ore {}", e.name);
        let Some(&b) = block_names.get(&e.name) else {
            errors.push(format!("{what}: unknown block"));
            return None;
        };
        let replace = match e.text(o::REPLACES).unwrap_or_default() {
            "" => None,
            t => Some(look.matcher(matchers, t, &format!("{what}: replaces"), errors)?),
        };
        let mut variants: Vec<(u32, u32)> = Vec::new();
        for v in e.or::<Vec<abi::OreVariant>>(o::REPLACES_WITH, Vec::new()) {
            let Some(ore) = look.first_state(&v.ore) else {
                errors.push(format!("{what}: replaces_with: unknown block {}", v.ore));
                continue;
            };
            if let Some(m) = look.matcher(matchers, &v.name, &format!("{what}: replaces_with"), errors) {
                variants.push((m, ore));
            }
        }
        let dims = look.targets(dim_defs, &what, e, errors);
        let biome = look.biomes(&what, e, errors).ok()?;
        // A walk, its starts spread evenly.
        let shape: abi::VeinShape = e.or(o::SHAPE, abi::VeinShape::default());
        let height: abi::OreHeight = e.or(o::HEIGHT, abi::OreHeight::default());
        let size = |v: f32| if v.is_finite() { v.clamp(0.0, 15.0) } else { 0.0 };
        let exposed: f32 = e.or(o::EXPOSED, 0.0);
        let [min_y, max_y] = heights(e);
        let def = OreDef {
            state: block_defs[b as usize].first_state,
            replace,
            min_y,
            max_y,
            veins: e.or(o::VEINS, 4),
            size: e.or(o::SIZE, 8),
            salt: crate::registry::name_hash(&e.name),
            biome,
            shape: shape.kind,
            shape_size: [size(shape.radius), size(shape.thickness)],
            height: height.curve,
            plateau: height.plateau.max(0),
            variants,
            exposed: if exposed.is_finite() { exposed.clamp(0.0, 1.0) } else { 0.0 },
        };
        Some((dims, def))
    });
    place(dim_defs, ores, |d| &mut d.ores);
    let (scatters, _) = tables.bake_by(entries_of(games, ek::SCATTER), ek::SCATTER, mod_name, errors, by_name, |e, errors| {
        let what = format!("scatter {}", e.name);
        let density_name = e.text(s::DENSITY).unwrap_or_default();
        let density = (!density_name.is_empty())
            .then(|| components.id(density_name))
            .flatten();
        if density.is_none() {
            errors.push(format!("{what}: density component {density_name:?} is not set on any biome"));
        }
        let ground = match e.text(s::GROUND).unwrap_or_default() {
            "" => None,
            t => Some(look.matcher(matchers, t, &format!("{what}: ground"), errors)?),
        };
        let (cell, reach) = (e.or(s::CELL, 8), e.or(s::REACH, 0));
        if cell < 1 || reach < 0 {
            errors.push(format!("{what}: cell {cell} and reach {reach} (cell at least 1)"));
            return None;
        }
        let biome = look.biomes(&what, e, errors).ok()?;
        let (owner, shape) = e.callback(s::SHAPE).unwrap_or((e.owner, Callback::NONE));
        let def = ScatterDef {
            owner,
            cell,
            reach,
            reach_y: [e.or(s::REACH_DOWN, 16).clamp(0, 256), e.or(s::REACH_UP, 32).clamp(0, 256)],
            density,
            biome,
            y: heights(e),
            ground,
            salt: crate::registry::name_hash(&e.name),
            shape,
        };
        Some((look.targets(dim_defs, &what, e, errors), def))
    });
    place(dim_defs, scatters, |d| &mut d.scatters);
    let (decorations, _) = tables.bake(entries_of(games, ek::DECORATION), ek::DECORATION, mod_name, errors, |e, errors| {
        let what = format!("decoration {}", e.name);
        if e.has(abi::comp::common::BIOME) || e.has(abi::comp::common::Y) {
            errors.push(format!("{what}: only its dimensions limit it, it decides the rest"));
        }
        let cb = e.callback(abi::comp::decoration::GENERATE)?;
        Some((look.targets(dim_defs, &what, e, errors), cb))
    });
    place(dim_defs, decorations, |d| &mut d.decorations);
}

/// Block names resolved once, so a typo is a load error, not a missing block later.
fn freeze_templates(
    games: &mut [crate::registry::RawGame],
    look: Lookups,
    tables: &mut crate::entries::Tables,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) -> (Vec<TemplateDef>, FxHashMap<String, u32>) {
    let Lookups { block_defs, block_names, .. } = look;
    let kind = abi::entry_kind::TEMPLATE;
    tables.bake_by(
        entries_of(games, kind),
        kind,
        mod_name,
        errors,
        |e| e.name.clone(),
        |e, errors| {
            let list: Vec<abi::TemplateBlock> = e.or(abi::comp::template::BLOCKS, Vec::new());
            let mut blocks = Vec::with_capacity(list.len());
            for b in &list {
                // `base:furnace[facing=east]`: that state, other properties at their defaults.
                let state = split_state(&b.block).and_then(|(block, props)| {
                    block_defs[*block_names
                        .get(block)
                        .ok_or_else(|| format!("unknown block {block}"))? as usize]
                        .state_of(&props)
                });
                match state {
                    Ok(st) => blocks.push((IVec3::from_array(b.pos), st)),
                    Err(err) => errors.push(format!("template {}: {err}", e.name)),
                }
            }
            TemplateDef { name: e.name.clone(), blocks }
        },
    )
}

fn place_structures(
    games: &mut [crate::registry::RawGame],
    dim_defs: &mut [DimensionDef],
    templates: &[TemplateDef],
    template_names: &FxHashMap<String, u32>,
    look: Lookups,
    tables: &mut crate::entries::Tables,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) {
    use abi::comp::structure as s;
    let kind = abi::entry_kind::STRUCTURE;
    let (structures, _) = tables.bake_by(
        entries_of(games, kind),
        kind,
        mod_name,
        errors,
        |e| e.name.clone(),
        |e, errors| {
            let what = format!("structure {}", e.name);
            let Some(&t) = template_names.get(&e.name) else {
                errors.push(format!("{what}: no such template"));
                return None;
            };
            let blocks = templates[t as usize].blocks.clone();
            let biome = look.biomes(&what, e, errors).ok()?;
            let extent = blocks
                .iter()
                .fold(IVec3::ZERO, |m, (p, _)| m.max(*p + IVec3::ONE));
            let def = StructureDef {
                name: e.name.clone(),
                blocks,
                extent,
                spacing: e.or(s::SPACING, 32u32).max(1) as i32,
                chance: e.or(s::CHANCE, 0.5),
                biome,
                y: heights(e),
                sink: e.or(s::BURIED, 0),
                salt: crate::registry::name_hash(&e.name),
            };
            Some((look.targets(dim_defs, &what, e, errors), def))
        },
    );
    place(dim_defs, structures, |d| &mut d.structures);
}

/// Each dimension's terrain: its own mod's entries first, then other mods' in load order.
fn build_terrains(
    games: &mut [crate::registry::RawGame],
    dim_defs: &mut [DimensionDef],
    biome_names: &FxHashMap<String, u32>,
    biome_rules: &[abi::BiomeRule],
    look: Lookups,
    tables: &mut crate::entries::Tables,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) {
    let kind = abi::entry_kind::TERRAIN;
    let mut raw = entries_of(games, kind);
    raw.sort_by_key(|e| {
        look.dim_names
            .get(&e.name)
            .is_none_or(|&d| dim_defs[d as usize].owner != e.owner)
    });
    let names = crate::server::terrain::Names {
        block: &|n| look.first_state(n),
        biome: &|n| biome_names.get(n).copied(),
        biome_rule: &|n| biome_names.get(n).map(|&b| &biome_rules[b as usize]),
    };
    let (terrains, _) = tables.bake(raw, kind, mod_name, errors, |e, errors| {
        let Some(&d) = look.dim_names.get(&e.name) else {
            errors.push(format!("terrain {}: no such dimension", e.name));
            return None;
        };
        Some((d, crate::server::terrain::build(&e.name, e, &names, errors)))
    });
    for (d, t) in terrains.into_iter().flatten() {
        dim_defs[d as usize].terrain = Some(Box::new(t));
    }
}

/// Word sets for `word(#set)` parameters: the engine's, then mods'.
fn collect_word_sets(
    games: &mut [crate::registry::RawGame],
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) -> FxHashMap<String, Vec<String>> {
    use abi::entry_kind::WORDS;
    let gizmos: Vec<String> = crate::client::gizmo::BUILTIN
        .iter()
        .map(|g| g.to_string())
        .collect();
    let mut all = vec![RawEntry::engine(WORDS, "core:gizmos").set(abi::comp::WORDS, gizmos)];
    // Filled from the game's own lists, once it is frozen.
    all.extend(
        ["core:rules", "core:weathers", "core:game_modes"]
            .iter()
            .map(|n| RawEntry::engine(WORDS, n)),
    );
    all.extend(entries_of(games, WORDS));
    let (sets, _) = crate::entries::bake(all, WORDS, mod_name, errors, |e, _| {
        let mut words: Vec<String> = e.or(abi::comp::WORDS, Vec::new());
        let mut seen = FxHashSet::default();
        words.retain(|w| seen.insert(w.clone()));
        (e.name.clone(), words)
    });
    sets.into_iter().collect()
}

fn freeze_commands(mods: &[ModMeta], games: &mut [crate::registry::RawGame], errors: &mut Vec<String>) -> Vec<crate::commands::CommandDef> {
    let mod_name = |m: ModId| mods[m.0 as usize].id.clone();
    // The engine's commands first, through the same list mods add to (and may replace them in).
    let mut commands: Vec<crate::commands::CommandDef> = Vec::new();
    let engine_commands = crate::server::BUILTIN_COMMANDS
        .iter()
        .map(|c| (c.0, c.1, c.3, false))
        .chain(
            crate::app::CLIENT_COMMANDS
                .iter()
                .map(|c| (c.0, c.1, abi::permission::EVERYONE, true)),
        );
    for (name, usage, level, client) in engine_commands {
        let alts = crate::commands::parse_usage(usage).expect("engine command usage");
        let mut def = crate::commands::CommandDef::new(name, usage, alts, None, client);
        def.level = level;
        commands.push(def);
    }
    let mut mod_commands: Vec<(ModId, String, String, Callback, bool)> = games
        .iter()
        .flat_map(|g| {
            g.commands
                .iter()
                .map(|c| (c.0, c.1.clone(), c.2.clone(), c.3, false))
                .chain(
                    g.client_commands
                        .iter()
                        .map(|c| (c.0, c.1.clone(), c.2.clone(), c.3, true)),
                )
        })
        .collect();
    mod_commands.sort_by(|a, b| a.1.cmp(&b.1));
    for (owner, name, usage, cb, client) in mod_commands {
        let alts = match crate::commands::parse_usage(&usage) {
            Ok(alts) => alts,
            Err(e) => {
                errors.push(format!("{}: command /{name}: {e}", mod_name(owner)));
                continue;
            }
        };
        let def = crate::commands::CommandDef::new(&name, &usage, alts, Some((owner, cb)), client);
        match commands.iter_mut().find(|d| d.name == name) {
            // A mod's command takes the place of the engine's of that name.
            Some(d) if d.handler.is_none() => {
                log::info!("{} replaces the engine's command /{name}", mod_name(owner));
                *d = def;
            }
            Some(_) => errors.push(format!("{}: command /{name} is already taken", mod_name(owner))),
            None => commands.push(def),
        }
    }
    for (name, level) in games
        .iter_mut()
        .flat_map(|g| std::mem::take(&mut g.command_permissions))
    {
        match commands.iter_mut().find(|d| d.name == name) {
            Some(d) => d.level = level.min(abi::permission::OWNER),
            None => errors.push(format!("permission of unknown command /{name}")),
        }
    }
    // Other mods extend existing commands, in load order: more completion words, more branches.
    let mut extensions: Vec<(ModId, crate::registry::CommandExtension)> = games
        .iter_mut()
        .flat_map(|g| std::mem::take(&mut g.command_extensions))
        .collect();
    extensions.sort_by_key(|e| mods.get(e.0.0 as usize).map_or(u32::MAX, |m| m.order));
    for (owner, ext) in extensions {
        let who = mod_name(owner);
        match ext {
            crate::registry::CommandExtension::Branch { command, usage, client, cb } => {
                match (commands.iter_mut().find(|d| d.name == command), crate::commands::parse_usage(&usage)) {
                    (Some(d), Ok(_)) if d.client != client => {
                        let (is, use_) = if d.client { ("the client", "client_command_branch") } else { ("the server", "command_branch") };
                        errors.push(format!("{who}: /{command} runs on {is}: its branches are added with {use_}"));
                    }
                    (Some(d), Ok(alts)) => d.add_branch(alts, (owner, cb)),
                    (None, _) => errors.push(format!("{who}: extends unknown command /{command}")),
                    (_, Err(e)) => errors.push(format!("{who}: branch of /{command}: {e}")),
                }
            }
        }
    }
    commands
}

/// The item of each liquid (`comp::FLUID`), and back.
fn fluid_items(
    item_defs: &[ItemDef],
    liquids: &[LiquidDef],
    components: &Components,
    errors: &mut Vec<String>,
) -> (Vec<u32>, FxHashMap<u32, u16>) {
    let (mut by_liquid, mut of_item) = (vec![0; liquids.len()], FxHashMap::default());
    if let Some(fc) = components.id(abi::comp::FLUID) {
        for i in 1..item_defs.len() as u32 {
            let Some(liquid) = components.text(comps::ITEM, i, fc) else { continue };
            match liquids.iter().position(|d| d.name == liquid) {
                Some(l) => {
                    by_liquid[l] = i;
                    of_item.insert(i, l as u16);
                }
                None => errors.push(format!("item {} is unknown liquid {liquid}", item_defs[i as usize].name)),
            }
        }
    }
    (by_liquid, of_item)
}

fn item_containers(
    item_defs: &[ItemDef],
    liquids: &[LiquidDef],
    components: &Components,
    errors: &mut Vec<String>,
) -> (FxHashMap<u32, (u16, u32)>, FxHashMap<(u32, u16), u32>) {
    let (mut item_liquid, mut containers) = (FxHashMap::default(), FxHashMap::default());
    if let (Some(lc), Some(rc)) = (components.id(abi::comp::LIQUID), components.id(abi::comp::REMAINDER)) {
        for i in 1..item_defs.len() as u32 {
            let Some(liquid) = components.text(comps::ITEM, i, lc) else { continue };
            let Some(l) = liquids.iter().position(|d| d.name == liquid) else {
                errors.push(format!("item {} holds unknown liquid {liquid}", item_defs[i as usize].name));
                continue;
            };
            let empty = components.get::<u32>(comps::ITEM, i, rc).unwrap_or(0);
            item_liquid.insert(i, (l as u16, empty));
            if empty != 0 {
                containers.insert((empty, l as u16), i);
            }
        }
    }
    (item_liquid, containers)
}

/// Every state's rules (`blocks::Blocks::rules`) with their conditions and what they become
/// resolved: `into` sets properties of the state (`age=0`) or names a block (`base:dirt`).
fn resolve_rules(
    states: &mut [StateInfo],
    blocks: &[BlockDef],
    names: &FxHashMap<String, u32>,
    raw: Vec<(usize, Vec<abi::BlockRule>)>,
    conds: &mut crate::conditions::Texts,
    errors: &mut Vec<String>,
) {
    for (state, rules) in raw {
        let def = &blocks[states[state].block as usize];
        for r in rules {
            let then = match r.then {
                // Properties of its own (`age=0`), else a block (`base:dirt`, `base:farmland[moisture=0]`).
                abi::rule_then::BECOME if r.into.contains('=') && !r.into.contains('[') => {
                    def.changed_text(state as u32, &r.into).map(Then::Become)
                }
                abi::rule_then::BECOME => crate::refs::state(&r.into, names, blocks).map(Then::Become),
                abi::rule_then::GROW => Ok(Then::Grow),
                abi::rule_then::BREAK => Ok(Then::Break),
                abi::rule_then::FALL => Ok(Then::Fall),
                t => Err(format!("no rule_then {t}")),
            };
            let ticks = r.secs.max(0.0) as f64 * crate::world::TPS as f64;
            let when = match r.when {
                abi::rule_when::NEIGHBOR => Ok(When::Neighbor(ticks.round() as u32)),
                abi::rule_when::RANDOM => Ok(When::Random(ticks.max(1.0))),
                w => Err(format!("no rule_when {w}")),
            };
            match (when, then) {
                (Ok(when), Ok(then)) => states[state]
                    .rules
                    .push(Rule { when, cond: conds.all(&r.conditions), then }),
                (Err(e), _) | (_, Err(e)) => errors.push(format!("block {} rule: {e}", def.name)),
            }
        }
    }
}

/// Every state's hazards with their causes and conditions resolved, and the engine's: an opaque
/// full block suffocates at the head, a `hot` one without its own burns like lava.
fn resolve_hazards(
    states: &mut [StateInfo],
    raw: Vec<(usize, Vec<abi::Hazard>)>,
    game: &crate::registry::Game,
    components: &Components,
    conds: &mut crate::conditions::Texts,
    errors: &mut Vec<String>,
) {
    use abi::hazard_at as at;
    let cause = |name: &str, errors: &mut Vec<String>| {
        if name.is_empty() {
            return None;
        }
        let id = game.damage_cause_names.get(name).copied();
        if id.is_none() {
            errors.push(format!("hazard: no damage cause {name}"));
        }
        id
    };
    for (state, list) in raw {
        for h in list {
            let cause = cause(&h.cause, errors);
            states[state].hazards.push(Hazard {
                at: h.at,
                damage: h.damage,
                cause,
                fire: h.fire,
                every: h.every,
                when: conds.all(&h.when),
            });
        }
    }
    let hot = components.id(abi::comp::HOT);
    let not_flying = conds.all(&["!flying".to_string()]);
    for (i, s) in states.iter_mut().enumerate() {
        if s.hazards.is_empty() && hot.is_some_and(|t| components.has(comps::BLOCK, i as u32, t)) {
            let lava = Some(abi::damage_cause::LAVA);
            s.hazards
                .push(Hazard { at: at::INSIDE, damage: 4.0, cause: lava, fire: 300, every: 0, when: None });
        }
        if s.flags & abi::block_flags::SOLID != 0 && s.opacity >= 15 {
            let choke = Some(abi::damage_cause::SUFFOCATE);
            s.hazards
                .push(Hazard { at: at::HEAD, damage: 1.0, cause: choke, fire: 0, every: 10, when: not_flying });
        }
    }
}

/// The `MODIFIERS` of items, entity kinds and poses, attributes and conditions resolved.
fn resolve_source_mods(
    game: &crate::registry::Game,
    components: &Components,
    items: usize,
    conds: &mut crate::conditions::Texts,
    errors: &mut Vec<String>,
) -> SourceMods {
    let mut out = SourceMods::default();
    let mut resolve = |list: Vec<abi::Modifier>| -> Vec<SourceMod> {
        list.into_iter()
            .filter_map(|m| match game.attr_names.get(&m.attribute) {
                Some(&attr) => Some(SourceMod { attr, op: m.op, amount: m.amount, worn: m.worn, when: conds.all(&m.when) }),
                None => {
                    errors.push(format!("modifier: no attribute {}", m.attribute));
                    None
                }
            })
            .collect()
    };
    for (i, m) in game
        .game_modes
        .iter()
        .enumerate()
        .filter(|m| !m.1.modifiers.is_empty())
    {
        out.modes.insert(i as u32, resolve(m.modifiers.clone()));
    }
    let Some(id) = components.id(abi::comp::MODIFIERS) else { return out };
    let mut of = |kind: usize, count: usize, into: &mut FxHashMap<u32, Vec<SourceMod>>| {
        for i in 0..count as u32 {
            if let Some(list) = components
                .bytes(kind, i, id)
                .and_then(abi::data::from_bytes::<Vec<abi::Modifier>>)
            {
                into.insert(i, resolve(list));
            }
        }
    };
    of(comps::ITEM, items, &mut out.items);
    of(comps::ENTITY, game.entities.len(), &mut out.kinds);
    of(abi::entry_kind::POSE as usize, game.poses.len(), &mut out.poses);
    out
}

fn freeze_loot(
    games: &mut [crate::registry::RawGame],
    item_defs: &[ItemDef],
    item_names: &FxHashMap<String, u32>,
    look: Lookups,
    tables: &mut crate::entries::Tables,
    conds: &mut crate::conditions::Texts,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) -> FxHashMap<String, crate::loot::Table> {
    let Lookups { block_defs, .. } = look;
    let raw = entries_of(games, abi::entry_kind::LOOT);
    // A block without a table of its own drops its item when harvested: a table of the engine's,
    // which mods edit like any.
    let own: FxHashSet<&str> = raw
        .iter()
        .filter(|e| e.mode != abi::entry_mode::EDIT)
        .map(|e| e.name.as_str())
        .collect();
    let mut all: Vec<RawEntry> = block_defs[1..]
        .iter()
        .filter(|b| !own.contains(b.name.as_str()))
        .filter_map(|b| {
            let item = item_defs[1..]
                .iter()
                .find(|i| i.block == Some(b.first_state))?;
            Some(RawEntry::engine(abi::entry_kind::LOOT, &b.name).set(abi::comp::loot::POOLS, vec![crate::loot::harvested(&item.name)]))
        })
        .collect();
    all.extend(raw);
    let mut resolve = crate::loot::Resolve { items: item_names, conds };
    let (made, _) = tables.bake(all, abi::entry_kind::LOOT, mod_name, errors, |e, errors| (e.name.clone(), resolve.table(e, errors)));
    made.into_iter().collect()
}

fn collect_stack_flags(decls: &[crate::registry::ComponentDecl], components: &Components) -> FxHashMap<u32, u32> {
    decls
        .iter()
        .filter_map(|d| Some((components.id(&d.name)?, d.stack?)))
        .collect()
}

fn link_particles(game: &mut crate::registry::Game, textures: &mut TextureList) {
    for p in &mut game.particles {
        for f in &mut p.frames {
            *f = textures.intern(&game.particle_textures[*f as usize]);
        }
    }
    game.particle_textures.clear();
}

fn check_ambient_particles(game: &crate::registry::Game, components: &Components, state_count: u32, errors: &mut Vec<String>) {
    for comp in [abi::comp::AMBIENT_PARTICLE, abi::comp::DRIP, abi::comp::SPLASH, abi::comp::BUBBLE] {
        let Some(id) = components.id(comp) else { continue };
        let mut named: Vec<&str> = (0..state_count)
            .filter_map(|s| components.text(comps::BLOCK, s, id))
            .flat_map(|n| n.split(','))
            .filter(|n| !game.particle_names.contains_key(*n))
            .collect();
        named.dedup();
        errors.extend(
            named
                .into_iter()
                .map(|n| format!("{comp}: unknown particle {n}")),
        );
    }
}

/// A structure's box is what its blocks fill; a set size would only disagree with them.
fn link_entity_templates(
    game: &mut crate::registry::Game,
    templates: &[TemplateDef],
    template_names: &FxHashMap<String, u32>,
    errors: &mut Vec<String>,
) {
    for kind in game
        .entities
        .iter_mut()
        .filter(|k| !k.template_name.is_empty())
    {
        match template_names.get(&kind.template_name) {
            Some(&t) => {
                kind.template = Some(t);
                if let Some((w, h)) = crate::server::structure::extent(&templates[t as usize].blocks) {
                    (kind.width, kind.height, kind.eye) = (w, h, h * crate::registry::EYE);
                }
            }
            None => errors.push(format!("entity {}: no template {}", kind.name, kind.template_name)),
        }
    }
}

/// Items worn in an equipment slot carry its `worn/<slot>` tag, and the ones held like tools
/// `handheld`.
fn link_equipment(game: &crate::registry::Game, item_defs: &mut [ItemDef], components: &Components) {
    if let Some(t) = components.id("handheld") {
        for (i, d) in item_defs.iter_mut().enumerate() {
            d.handheld = components.has(comps::ITEM, i as u32, t);
        }
    }
    for (e, slot) in game.player_layout.equipment.iter().enumerate() {
        if let Some(t) = components.id(&format!("worn/{}", slot.name)) {
            for (i, d) in item_defs.iter_mut().enumerate() {
                if components.has(comps::ITEM, i as u32, t) {
                    d.equip = Some(e as u32);
                }
            }
        }
    }
}

/// Blocks open their menus; one with recipe kinds makes the block a station of them.
fn link_menus(
    game: &mut crate::registry::Game,
    block_defs: &mut [BlockDef],
    item_defs: &[ItemDef],
    opens_names: &[String],
    mods: &[ModMeta],
    errors: &mut Vec<String>,
) {
    for (b, menu) in opens_names.iter().enumerate().filter(|m| !m.1.is_empty()) {
        let def = &mut block_defs[b + 1];
        match game.menu_names.get(menu) {
            Some(&m) => {
                def.opens = Some(m);
                if let Some(item) = item_defs
                    .iter()
                    .position(|i| i.block == Some(def.first_state))
                {
                    for kind in &game.menus[m as usize].recipes {
                        game.recipe_stations
                            .entry(kind.clone())
                            .or_default()
                            .push(item as u32);
                    }
                }
            }
            None => errors.push(format!(
                "{}: block {} opens unknown menu {menu}",
                mods.get(def.owner.0 as usize)
                    .map_or("engine", |m| m.id.as_str()),
                def.name
            )),
        }
    }
}

fn check_loot_smelting(loot: &FxHashMap<String, crate::loot::Table>, game: &crate::registry::Game, errors: &mut Vec<String>) {
    for (table, t) in loot {
        for k in t
            .pools
            .iter()
            .flat_map(|p| &p.entries)
            .filter_map(|e| e.smelt.as_ref())
        {
            if !game.recipe_kinds.contains_key(k) {
                errors.push(format!("loot table {table} smelts by recipe kind {k}, which no mod registers"));
            }
        }
    }
}

/// What each block state's and entity kind's events sound and show: its sound group's events
/// (`SOUND`, `<prefix>.break` ...), the engine's pieces of a block and death smoke, then what its
/// `EFFECTS` say instead.
fn resolve_effects(
    states: &mut [StateInfo],
    game: &mut crate::registry::Game,
    block_defs: &[BlockDef],
    components: &Components,
    errors: &mut Vec<String>,
) {
    let sc = components.id(abi::comp::SOUND);
    let fx = components.id(abi::comp::EFFECTS);
    let particle = |name: &str| game.particle_names.get(name).copied().unwrap_or(u32::MAX);
    let (pieces, smoke) = (particle(abi::particle::BLOCK), particle(abi::particle::POOF));
    // A prefix with none of its events registered is a typo (`.sounds("stnoe")`).
    let mut unknown: Vec<(String, String)> = Vec::new();
    let mut fill = |kind: usize, id: u32, events: &[&str], out: &mut [Fx], what: &str, errors: &mut Vec<String>| {
        let prefix = sc.and_then(|sc| components.text(kind, id, sc));
        if let Some(pre) = prefix {
            for (e, f) in events.iter().zip(out.iter_mut()) {
                f.sound = game
                    .sound_names
                    .get(&format!("{pre}.{e}"))
                    .copied()
                    .unwrap_or(u32::MAX);
            }
            if out.iter().all(|f| f.sound == u32::MAX) && !unknown.iter().any(|u| u.0 == pre) {
                unknown.push((pre.to_string(), what.to_string()));
            }
        }
        let Some(list) = fx
            .and_then(|fx| components.bytes(kind, id, fx))
            .and_then(abi::data::from_bytes::<Vec<abi::Effect>>)
        else {
            return;
        };
        for e in list {
            let Some(i) = events.iter().position(|n| *n == e.event) else {
                errors.push(format!("{what}: no event {} (it has {})", e.event, events.join(", ")));
                continue;
            };
            let mut set = |text: &str, names: &FxHashMap<String, u32>, to: &mut u32| match text {
                "" => {}
                "none" => *to = u32::MAX,
                n => match names.get(n) {
                    Some(&id) => *to = id,
                    None => errors.push(format!("{what}: {} has no {n}", e.event)),
                },
            };
            set(&e.sound, &game.sound_names, &mut out[i].sound);
            set(&e.particle, &game.particle_names, &mut out[i].particle);
        }
    };
    for (i, s) in states.iter_mut().enumerate() {
        use block_event as b;
        for e in [b::BREAK, b::HIT, b::LAND, b::SPRINT] {
            s.effects[e].particle = pieces;
        }
        let what = &block_defs[s.block as usize].name;
        fill(comps::BLOCK, i as u32, &b::NAMES, &mut s.effects, what, errors);
    }
    for k in 0..game.entities.len() {
        let mut out = [Fx::NONE; 4];
        out[crate::registry::entity_event::DEATH].particle = smoke;
        let what = game.entities[k].name.clone();
        fill(comps::ENTITY, k as u32, &crate::registry::entity_event::NAMES, &mut out, &what, errors);
        game.entities[k].effects = out;
    }
    for (prefix, what) in unknown {
        errors.push(format!("{what}: no sound events named {prefix}.*"));
    }
}

/// Liquids by name, folded already (`freeze`); reactions in load order.
fn freeze_liquids(
    folded: crate::entries::Folded,
    games: &mut [crate::registry::RawGame],
    states: &mut [StateInfo],
    look: Lookups,
    matchers: &mut crate::refs::Matchers,
    tables: &mut crate::entries::Tables,
    mod_name: &dyn Fn(ModId) -> String,
    errors: &mut Vec<String>,
) -> (Vec<LiquidDef>, Vec<ReactionDef>) {
    use abi::comp::{liquid as l, reaction as r};
    let blocks = look.block_defs;
    let (liquids, _) = tables.finish(
        abi::entry_kind::LIQUID,
        folded,
        errors,
        |e| e.name.clone(),
        |e, errors| {
            let Some(&b) = look.block_names.get(&e.name) else {
                errors.push(format!("liquid {} has no block of that name", e.name));
                return None;
            };
            let def = &blocks[b as usize];
            if def.states != 16 {
                errors.push(format!("liquid {}: its block needs 16 states, has {}", e.name, def.states));
                return None;
            }
            let fog: abi::LiquidFog = e.or(l::FOG, abi::LiquidFog { color: u32::MAX, distance: 96.0 });
            let overlay: abi::LiquidOverlay = e.or(l::OVERLAY, abi::LiquidOverlay { texture: String::new(), opacity: 0.1 });
            Some(LiquidDef {
                name: e.name.clone(),
                first_state: def.first_state,
                delay: e.or(l::DELAY, 5u32).max(1),
                drop_off: e.or(l::DROP_OFF, 1u32).clamp(1, 7),
                slope_find: e.or(l::SLOPE_FIND, 4u32).min(8),
                renewable: e.or(l::RENEWABLE, u32::MAX),
                rises: e.or(l::DENSITY, 1000) < 0,
                temperature: e.or(l::HEAT, 300),
                fog_color: (fog.color != u32::MAX).then_some(fog.color),
                fog_distance: fog.distance.max(0.1),
                overlay: overlay.texture,
                overlay_opacity: overlay.opacity.clamp(0.0, 1.0),
            })
        },
    );
    // Liquids whose block is missing are errors, so ids stay those of the names.
    let liquids: Vec<LiquidDef> = liquids.into_iter().flatten().collect();
    for (id, liquid) in liquids.iter().enumerate() {
        // States 0..8 are levels (0 the source), 8..16 falling: full height.
        for s in 0..16 {
            let st = &mut states[(liquid.first_state + s) as usize];
            st.liquid = Some(id as u16);
            st.level = if s < 8 { s as u8 } else { 0 };
            st.flags = abi::block_flags::LIQUID | abi::block_flags::REPLACEABLE | abi::block_flags::NOT_TARGETABLE;
            // No longer the solid block it was registered as.
            st.collision.clear();
        }
    }
    let kind = abi::entry_kind::REACTION;
    let (reactions, _) = tables.bake(entries_of(games, kind), kind, mod_name, errors, |e, errors| {
        let what = format!("liquid reaction {}", e.name);
        let mut matcher = |name: &str, errors: &mut Vec<String>| look.matcher(matchers, e.text(name).unwrap_or_default(), &what, errors);
        let result = |name: &str, errors: &mut Vec<String>| -> Option<u32> {
            let block = e.text(name).filter(|s| !s.is_empty())?;
            let b = look.first_state(block);
            if b.is_none() {
                errors.push(format!("{what} makes unknown block {block}"));
            }
            b
        };
        let (liquid, touching) = (matcher(r::LIQUID, errors), matcher(r::TOUCHING, errors));
        Some(ReactionDef {
            liquid: liquid?,
            touching: touching?,
            source: result(r::SOURCE, errors),
            flowing: result(r::FLOWING, errors),
            falls_into: result(r::FALLS_INTO, errors),
        })
    });
    (liquids, reactions.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str, deps: &[&str]) -> ModMeta {
        ModMeta {
            id: id.into(),
            version: "1".into(),
            deps: deps.iter().map(|d| d.to_string()).collect(),
            order: 0,
            assets: Default::default(),
        }
    }

    #[test]
    fn dependencies_load_first() {
        // Sorted by id, as the loader lists them: "aaa" depends on "zzz".
        let mods = [meta("aaa", &["zzz"]), meta("base", &[]), meta("zzz", &["base"])];
        assert_eq!(load_order(&mods), vec![2, 0, 1]);
    }
}
