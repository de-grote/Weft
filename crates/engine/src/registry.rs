//! Gameplay registries: entity kinds, components, menus, recipes, HUD parts. Frozen together with
//! blocks and items, ids again assigned by sorted name.

use std::sync::atomic::{AtomicU32, Ordering};

use crate::tag_bits::entity as ef;
use rustc_hash::FxHashMap;
use weft_abi::{self as abi, Callback, ModId, entry_kind as ek};

use crate::components::{self as comps, Components, RawValue};
use crate::entries::{Entry, RawEntry, bake, bake_by};

pub const ENGINE: ModId = ModId(u32::MAX);
pub const PLAYER: &str = "core:player";

#[derive(Clone)]
pub struct HudElement {
    pub name: String,
    /// Translation key of its F3 panel title: the name with `.` for `:`.
    pub title: String,
    pub anchor: String,
    pub order: i32,
    pub owner: ModId,
    pub cb: Callback,
    pub rebuild: Option<(ModId, Callback)>,
}
pub const ITEM: &str = "core:item";
/// A block falling as an entity (`Host::fall_block`), placed again where it lands.
pub const FALLING: &str = "core:falling_block";
pub const MOVING: &str = "core:moving_blocks";
/// What a floater carries, and what `World::lift` makes.
pub const PLATFORM: &str = "core:platform";

#[derive(Clone, Debug)]
pub struct Cuboid {
    /// Model pixels, relative to the part's pivot.
    pub origin: [f32; 3],
    pub size: [f32; 3],
    pub uv: [f32; 2],
    pub inflate: f32,
    pub mirror: bool,
    /// Own texel rectangle per face (-X, +X, -Y, +Y, -Z, +Z), instead of the box unwrap.
    pub faces: Option<[[f32; 4]; 6]>,
}

impl Cuboid {
    /// Texel rectangles of the faces (-X, +X, -Y, +Y, -Z, +Z): its own, or the classic box
    /// unwrap (mirrored: each face flipped and the side faces swapped).
    pub fn face_rects(&self) -> [[f32; 4]; 6] {
        if let Some(f) = self.faces {
            return f;
        }
        let [w, h, d] = self.size;
        let [u, v] = self.uv;
        // Wrapped around the box as they lie in the unwrap: seen from the front (-Z), what is left
        // of it there is on its left, which is +X. The top lies above the front, so it is turned
        // to meet it. As Blockbench does it.
        let mut r = [
            [u + d + w, v + d, d, h],
            [u, v + d, d, h],
            [u + d + w, v, w, d],
            [u + d + w, v + d, -w, -d],
            [u + d, v + d, w, h],
            [u + d * 2.0 + w, v + d, w, h],
        ];
        if self.mirror {
            r.swap(0, 1);
            for f in &mut r {
                f[0] += f[2];
                f[2] = -f[2];
            }
        }
        r
    }
}

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    /// Index of the part it hangs from.
    pub parent: Option<u32>,
    pub pivot: [f32; 3],
    pub role: u32,
    pub cuboids: Vec<Cuboid>,
    pub rest: [f32; 3],
    /// Free-form triangles: model pixels (absolute, not pivot relative) and texels.
    pub mesh: Vec<weft_abi::MeshVertex>,
}

pub struct EntityKind {
    pub name: String,
    pub owner: ModId,
    pub width: f32,
    pub height: f32,
    pub eye: f32,
    pub speed: f32,
    pub flags: u32,
    pub category: u32,
    /// Set by the mod instead of `category`; resolved at freeze.
    pub category_name: String,
    pub texture: String,
    pub texture_size: [u32; 2],
    pub parts: Vec<Part>,
    /// Model files on its bones: (model, bone).
    pub bone_models: Vec<(String, String)>,
    /// The kind's own callbacks (empty or one).
    pub on_tick: Vec<(ModId, Callback)>,
    pub on_interact: Vec<(ModId, Callback)>,
    /// Every frame on the client for each one, in order (`comp::entity::FRAMES`).
    pub frames: Vec<(ModId, Callback)>,
    /// Components attached at spawn.
    pub components: Vec<u32>,
    /// Starting values of attached components for this kind, where they differ from the
    /// component's default.
    pub values: Vec<(u32, Box<[u8]>)>,
    /// Size of the inventory created with it (its `inventory` component), 0 = none.
    pub slots: u32,
    /// Its equipment slots in that inventory (`entity_flags::WEARS`, the player's): never filled
    /// by pickups, worn on the model. Laid out at freeze.
    pub worn: std::ops::Range<u32>,
    /// The slot of its second hand, after its equipment (the player's, and kinds that wear).
    pub off: Option<u32>,
    /// How many of its first slots its main hand may be on (`Entity::selected`): the player's
    /// hotbar, a wearing kind's own slots (one at least); 0 for a kind without hands.
    pub hand_slots: u32,
    /// What its events sound and show (`entity_event`).
    pub effects: [crate::content::Fx; 4],
    /// Solid bodies: riders up to this high above it move with it.
    pub carry_height: f32,
    /// Where its light comes from, from its position turned with it: its middle unless said, a
    /// block entity's block's.
    pub light_at: glam::DVec3,
    /// Inventory slots everyone around sees (`SHOWN`).
    pub shown_slots: Vec<u32>,
    /// What it does to entities, itself among them (`abi::comp::entity::HAZARDS`): as
    /// registered, then resolved at freeze.
    pub raw_hazards: Vec<abi::Hazard>,
    pub hazards: Vec<crate::content::Hazard>,
    /// A moving structure: its template (`Content::templates`), by name until the freeze.
    pub template_name: String,
    pub template: Option<u32>,
    /// Where riders sit (`abi::Seat`).
    pub seats: Vec<Seat>,
    /// How it flies when it is a projectile (`comp::entity::PROJECTILE`).
    pub projectile: Option<abi::Projectile>,
    /// What hits land on, part by part (`comp::entity::HITBOXES`); none: its box.
    pub hitboxes: Option<abi::Hitboxes>,
    /// What the keys of a player steering it do (`abi::SteerArgs`), and where riders get off
    /// when a seat asks it (`abi::DismountArgs`).
    pub steer: Option<(ModId, Callback)>,
    pub dismount: Option<(ModId, Callback)>,
    /// Blocks the third-person camera goes further back on it, `NAN`: by its size.
    pub ride_camera: f32,
}

/// A place on a mount, in its frame: +Z ahead.
#[derive(Clone, Copy, Debug)]
pub struct Seat {
    pub pos: glam::DVec3,
    pub steers: bool,
    /// `abi::seat_exit`, and its offset or reach.
    pub exit: u32,
    pub exit_at: glam::DVec3,
}

impl EntityKind {
    /// Its box standing, as `Entity::dims` starts from.
    pub fn size(&self) -> weft_abi::EntitySize {
        weft_abi::EntitySize { width: self.width, height: self.height, eye: self.eye }
    }

    pub fn is(&self, f: u32) -> bool {
        self.flags & f != 0
    }
}

/// Per-entity storage of a component (indexed by component id; `runtime` false for components
/// that only exist on content entries).
#[derive(Clone, Default)]
pub struct ComponentDef {
    pub name: String,
    pub size: u32,
    pub flags: u32,
    pub default: Vec<u8>,
    pub runtime: bool,
}

pub struct MenuDef {
    pub name: String,
    /// Slots of the menu's own inventory (its `slot_owner::MENU` groups).
    pub temp_slots: u32,
    pub groups: Vec<GroupDef>,
    /// Recipe kinds its `arrow` bars open.
    pub recipes: Vec<String>,
    /// The callbacks (`abi::comp::menu`) and the mods they run as.
    pub on_change: Option<(ModId, Callback)>,
    pub on_take: Option<(ModId, Callback)>,
    pub on_tick: Option<(ModId, Callback)>,
    pub screen: Option<(ModId, Callback)>,
    pub on_key: Option<(ModId, Callback)>,
    /// How its screen behaves, `crate::tag_bits::screen`.
    pub flags: u32,
}

/// A slot group (`abi::SlotGroupDef`), its start resolved.
#[derive(Clone, Debug)]
pub struct GroupDef {
    pub name: String,
    /// `abi::slot_owner`.
    pub owner: u32,
    pub start: u32,
    pub cols: u32,
    pub rows: u32,
    pub flags: u32,
    /// What items it takes, a reference (`#fuel #mymod:fire_starter !base:lava_bucket`), empty
    /// for any.
    pub accepts: String,
    /// `accepts` as a bit per item, made at the freeze; `None` any.
    pub accepted: Option<std::sync::Arc<[u64]>>,
    pub limit: u32,
    pub size: f32,
    pub background: Option<String>,
    pub tooltip: Option<String>,
    pub priority: i32,
}

impl GroupDef {
    /// (owner inventory start, slot count, columns); the player's presets follow `layout`.
    pub fn span(&self, layout: &PlayerLayout) -> (u32, u32, u32) {
        use weft_abi::slot_owner as so;
        match self.owner {
            so::PLAYER_MAIN => (layout.hotbar, layout.main().count() as u32, layout.hotbar.max(1)),
            so::PLAYER_HOTBAR => (0, layout.hotbar, layout.hotbar.max(1)),
            _ => (self.start, self.cols * self.rows, self.cols.max(1)),
        }
    }

    /// Owner inventory index of the group's `i`-th slot (the main inventory skips worn slots).
    pub fn index(&self, layout: &PlayerLayout, i: u32) -> u32 {
        let (start, _, _) = self.span(layout);
        if self.owner == weft_abi::slot_owner::PLAYER_MAIN { layout.main().nth(i as usize).unwrap_or(u32::MAX) } else { start + i }
    }
}

impl MenuDef {
    /// (group, its first menu slot, slot count) in menu slot order.
    pub fn slot_groups(&self, layout: &PlayerLayout) -> impl Iterator<Item = (&GroupDef, u32, u32)> + '_ {
        let mut at = 0;
        let layout = layout.clone();
        self.groups.iter().map(move |g| {
            let n = g.span(&layout).1;
            at += n;
            (g, at - n, n)
        })
    }

    pub fn group(&self, layout: &PlayerLayout, name: &str) -> Option<(&GroupDef, u32, u32)> {
        self.slot_groups(layout).find(|g| g.0.name == name)
    }

    /// The group of menu slot `slot` and the slot's place in it.
    pub fn group_of(&self, layout: &PlayerLayout, slot: u32) -> Option<(&GroupDef, u32)> {
        self.slot_groups(layout)
            .find(|g| slot >= g.1 && slot < g.1 + g.2)
            .map(|g| (g.0, slot - g.1))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ingredient {
    Empty,
    Item(u32),
    /// Component id of the tag.
    Tag(u32),
    /// Any of these items (a tag pattern, `#ingots/*`).
    Any(Vec<u32>),
}

impl Ingredient {
    #[inline]
    pub fn matches(&self, item: u32, c: &Components) -> bool {
        match self {
            Ingredient::Empty => item == 0,
            Ingredient::Item(i) => *i == item,
            Ingredient::Tag(t) => item != 0 && c.has(comps::ITEM, item, *t),
            Ingredient::Any(list) => list.contains(&item),
        }
    }
}

/// What a run of a recipe does to an ingredient (`abi::RecipeUse`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Use {
    /// Used up; the last of a stack leaves its item's `remainder`.
    Consume,
    Keep,
    Wear(u32),
    /// Used up, this item left in its place.
    Leaves(u32),
}

pub struct Recipe {
    /// Shaped: width, height, cells (row major).
    pub shaped: Option<(u32, u32, Vec<Ingredient>)>,
    pub inputs: Vec<Ingredient>,
    /// Per cell of a shaped one, else per input: what a run does to it.
    pub uses: Vec<Use>,
    /// How many of each input (millibuckets for fluids).
    pub input_counts: Vec<u32>,
    /// The main result, then each its pools can give (`extras`) with the most of it and its chance.
    pub outputs: Vec<RecipeOutput>,
    /// What else one run gives, rolled as loot.
    pub extras: crate::loot::Table,
    pub time: u32,
    /// Its entry, whose component table row has what else it carries (`comp::recipe::HIDDEN`, a
    /// mod's values).
    pub entry: u32,
}

#[derive(Clone, Debug)]
pub struct RecipeOutput {
    pub what: Ingredient,
    pub count: u32,
    pub chance: f32,
}

impl Recipe {
    /// The main result as an item stack (empty when it is a fluid).
    pub fn result(&self) -> weft_abi::ItemStack {
        match self.outputs.first() {
            Some(RecipeOutput { what: Ingredient::Item(i), count, .. }) => weft_abi::ItemStack::new(*i, *count),
            _ => weft_abi::ItemStack::EMPTY,
        }
    }
}

/// How the recipe viewer shows a kind of recipe (`abi::comp::recipe_kind`).
pub struct RecipeKindDef {
    pub owner: ModId,
    pub input_grid: [u32; 2],
    pub output_grid: [u32; 2],
    pub info: Option<String>,
    pub info_values: Vec<String>,
    pub display: Option<Callback>,
}

#[derive(Default)]
pub struct Game {
    /// Camera modes by id (`abi::comp::camera`): the engine's (`camera::*`) first.
    pub cameras: Vec<CameraMode>,
    pub camera_names: FxHashMap<String, u32>,
    pub entities: Vec<EntityKind>,
    pub entity_names: FxHashMap<String, u32>,
    pub components: Vec<ComponentDef>,
    pub component_names: FxHashMap<String, u32>,
    pub menus: Vec<MenuDef>,
    pub menu_names: FxHashMap<String, u32>,
    pub recipes: FxHashMap<String, Vec<Recipe>>,
    pub recipe_kinds: FxHashMap<String, u32>,
    pub recipe_kind_defs: FxHashMap<String, RecipeKindDef>,
    /// Entity categories by `EntityKind::category`: the engine's three first.
    pub categories: Vec<CategoryDef>,
    /// By id: the engine's (`abi::damage_cause`) first.
    pub damage_causes: Vec<DamageCauseDef>,
    pub damage_cause_names: FxHashMap<String, u32>,
    /// By id: the engine's (`abi::entity_effect`) first.
    pub entity_effects: Vec<EntityEffectDef>,
    pub entity_effect_names: FxHashMap<String, u32>,
    /// By id: the engine's (`abi::show_place`) first.
    pub text_places: Vec<crate::hud::Place>,
    pub text_place_names: FxHashMap<String, u32>,
    /// By id: the engine's (`abi::game_mode`) first.
    pub game_modes: Vec<GameModeDef>,
    pub game_mode_names: FxHashMap<String, u32>,
    /// By id: the engine's (`abi::pose`) first.
    pub poses: Vec<PoseDef>,
    /// Poses by priority, highest first.
    pub pose_order: Vec<crate::world::Pose>,
    pub pose_names: FxHashMap<String, u32>,
    /// By id: the engine's (`abi::weather`) first.
    pub weathers: Vec<WeatherDef>,
    pub weather_names: FxHashMap<String, u32>,
    /// By id: rain, snow, then mods'.
    pub precipitations: Vec<PrecipitationDef>,
    pub precipitation_names: FxHashMap<String, u32>,
    /// Sound categories by `SoundDef::category`: the engine's six first.
    pub sound_categories: Vec<SoundCategory>,
    pub huds: Vec<HudElement>,
    pub chat_format: Option<(ModId, Callback)>,
    pub player_menu: Option<u32>,
    pub player_layout: PlayerLayout,
    pub screen_parts: Vec<ScreenPart>,
    /// Block index -> (entity kind, inventory slots).
    pub block_entities: FxHashMap<u32, u32>,
    pub player_kind: u32,
    pub item_kind: u32,
    pub falling_kind: u32,
    /// Blocks a piston is moving, an entity while they do.
    pub moving_kind: u32,
    pub platform_kind: u32,
    pub energy: u32,
    /// `Host::reg_client_frame`, in mod order.
    pub client_frames: Vec<(ModId, Callback)>,
    /// (model, bone, how it moves), from `entry_kind::BONE_MOTION`.
    pub bone_motions: Vec<(String, String, crate::model::BoneMotion)>,
    /// Who says which model a stack of an item shows as (`comp::item::CONDITION`).
    pub item_model_for_stack: FxHashMap<u32, (ModId, Callback)>,
    /// Mod message channels by id (sorted by name, so both sides agree).
    pub packets: Vec<PacketDef>,
    pub packet_names: FxHashMap<String, u32>,
    /// Key bindings, the engine's and the mods'.
    pub keys: Vec<KeyDef>,
    pub events: Vec<EventDef>,
    pub event_names: FxHashMap<String, u32>,
    pub steps: crate::steps::Steps,
    pub attributes: Vec<AttrDef>,
    pub attr_names: FxHashMap<String, u32>,
    /// The engine component holding entities' attribute modifiers.
    pub modifiers_comp: u32,
    /// `TIMERS` as an entity component.
    pub timers_comp: u32,
    /// `abi::event::BLOCK_TICK`.
    pub block_tick: u32,
    /// (owner, callback) run once per tick in every region, in mod order.
    pub world_ticks: Vec<(ModId, Callback)>,
    /// `abi::comp::NAME` as an entity component.
    pub name_comp: u32,
    /// `abi::comp::PERSISTENT` as an entity component.
    pub persistent_comp: u32,
    /// Components with `component_flags::CARRIED`: between a block's item and its block entity.
    pub carried: Vec<u32>,
    pub fields: FieldComps,
    /// `SIMULATED`.
    pub simulated_comp: u32,
    /// Entity components with `SYNC_WATCHERS` / `SYNC_MENU`.
    pub watched_comps: Vec<u32>,
    pub menu_comps: Vec<u32>,
    /// Per component: synced to anyone at all (its changes are looked for every tick).
    pub synced_comps: Vec<bool>,
    /// Config id of `core:mob_spawning`.
    pub mob_spawning: u32,
    /// Per attribute, the world rule players' base of it is (`comp::attribute::RULE`).
    pub attr_rules: Vec<Option<u32>>,
    /// Per damage cause, the world rule that turns it off (`core:damage.<cause>`); none for what
    /// gets through everything.
    pub damage_rules: Vec<Option<u32>>,
    /// Config ids of `core:lives` and `core:rules_locked`.
    pub lives: u32,
    pub rules_locked: u32,
    /// Config id of `core:keep_inventory`.
    pub keep_inventory: u32,
    /// Config id of `core:max_entity_cramming`.
    pub max_cramming: u32,
    /// Config id of `core:simulation_distance`.
    pub simulation_distance: u32,
    /// (owner, component, system) in mod order.
    pub systems: Vec<(ModId, u32, Callback)>,
    /// `{if:name}` conditions mods answer.
    /// By id: the engine's (`abi::condition`, answered by the engine) first, then mods'.
    pub conditions: Vec<ConditionDef>,
    pub condition_names: FxHashMap<String, u32>,
    /// What is in the sky of each dimension, in drawing order.
    pub sky: Vec<Vec<SkyDef>>,
    /// Particle kinds by id (sorted by name).
    pub particles: Vec<ParticleDef>,
    pub particle_names: FxHashMap<String, u32>,
    /// Particle texture names while freezing; the content moves them into its one list.
    pub particle_textures: Vec<String>,
    /// Block tags particle rules ask about, in the order of their bits.
    pub particle_tags: Vec<String>,
    /// Items shown with a recipe type in the recipe viewer (the machines making them).
    pub recipe_stations: FxHashMap<String, Vec<u32>>,
    /// Sound events by id (sorted by name).
    pub sounds: Vec<SoundDef>,
    pub sound_names: FxHashMap<String, u32>,
    /// Background music, in mod load order.
    pub music: Vec<MusicDef>,
    /// Natural spawning rules, in mod load order.
    pub spawns: Vec<SpawnRule>,
    /// Settings by id (sorted by name), see `config.rs`.
    pub configs: Vec<crate::config::ConfigDef>,
    pub config_names: FxHashMap<String, u32>,
}

pub struct SoundDef {
    pub name: String,
    pub files: Vec<String>,
    pub category: u32,
    pub volume: f32,
    pub pitch: [f32; 2],
    pub range: f32,
    pub full_within: f32,
    pub falloff: i32,
}

pub struct MusicDef {
    pub name: String,
    pub tracks: Vec<String>,
    pub dimension: Option<u32>,
    /// A condition (`Content::conds`) that holds while it may play.
    pub when: Option<u32>,
}

/// A condition and who answers it.
#[derive(Clone)]
pub struct ConditionDef {
    pub name: String,
    pub answer: Answer,
}

#[derive(Clone, Copy)]
pub enum Answer {
    /// One of `abi::condition`.
    Engine,
    /// Every game mode is a condition of its name: the entity, or the client's player, is in it.
    Mode(crate::world::GameMode),
    Mod(ModId, Callback),
}

pub struct ParticleDef {
    pub name: String,
    /// Frames over its life (ids into `Content::textures`); empty for block dust.
    pub frames: Vec<u32>,
    pub life_min: f32,
    pub life_max: f32,
    pub size_min: f32,
    pub size_max: f32,
    pub gravity: f32,
    pub drag: f32,
    pub color: u32,
    pub color_end: u32,
    pub flags: u32,
    pub shader: String,
    pub rules: Vec<ParticleRule>,
    /// Seconds one may live at most, the kinds it turns into included.
    pub longest: f32,
}

/// `abi::ParticleRule` with what it names as numbers.
#[derive(Clone, Copy, Debug)]
pub struct ParticleRule {
    pub event: u32,
    pub when: u32,
    /// `particle_when::TAG`: its index in `Game::particle_tags`.
    pub tag: u32,
    pub then: u32,
    pub amount: f32,
    /// `BECOME`: the kind.
    pub into: u32,
}

/// Tags particle rules ask about at most: the GPU keeps them in the bits a block's word has left.
pub const PARTICLE_TAGS: usize = 5;

/// A thing in a dimension's sky (`abi::comp::sky`).
pub struct SkyDef {
    pub name: String,
    pub kind: u32,
    /// Asset name of the picture, empty for none.
    pub texture: String,
    pub shader: String,
    pub phase: f32,
    /// Ticks per turn, 0 still, infinite following the day.
    pub orbit: f32,
    pub tilt: f32,
    pub heading: f32,
    pub size: f32,
    pub spin: f32,
    pub opacity: [f32; 2],
    pub color: u32,
    pub frames: [u32; 2],
    /// 0: one frame per day of the dimension.
    pub frame_ticks: u32,
    pub height: f32,
    pub drift: f32,
    pub glow: u32,
    pub additive: bool,
    pub count: u32,
    pub seed: u32,
    pub order: i32,
}

/// A sky object from its entry (`<dimension>/<object>`): the dimension's id and what it is.
fn bake_sky(e: &Entry, dims: &FxHashMap<String, u32>, errors: &mut Vec<String>) -> Option<(u32, SkyDef)> {
    use abi::comp::sky as s;
    use abi::sky_kind as k;
    // Either name may have slashes of its own.
    let Some(dim) = e
        .name
        .match_indices('/')
        .find_map(|(i, _)| dims.get(&e.name[..i]).copied())
    else {
        errors.push(format!("sky object {}: not <dimension>/<object> of a dimension there is", e.name));
        return None;
    };
    let kind = e.or(s::KIND, k::BODY);
    let stars = kind == k::STARS;
    let texture = e.text(s::TEXTURE).unwrap_or_default().to_string();
    let shader = e.text(s::SHADER).unwrap_or_default().to_string();
    // Would be a white square across the sky.
    if !stars && texture.is_empty() && shader.is_empty() {
        errors.push(format!("sky object {} has no texture or shader", e.name));
    }
    let [columns, rows] = e.or(s::FRAMES, [1u32, 1]);
    Some((
        dim,
        SkyDef {
            name: e.name.clone(),
            kind,
            texture,
            shader,
            phase: e.or(s::PHASE, 0.0),
            // A dome stands still by default, what moves across the sky follows the day.
            orbit: e.or(s::ORBIT, if kind == k::DOME { 0.0 } else { f32::INFINITY }),
            tilt: e.or(s::TILT, 0.0f32).to_radians(),
            heading: e.or(s::HEADING, 0.0f32).to_radians(),
            size: e
                .or(s::SIZE, if stars { 0.25f32 } else { 20.0 })
                .to_radians(),
            spin: e.or(s::SPIN, 0.0),
            // Stars show at night only.
            opacity: e.or(s::OPACITY, if stars { [0.0, 0.22] } else { [1.0, 1.0] }),
            color: e.or(s::COLOR, 0xFFFFFF),
            frames: [columns.max(1), rows.max(1)],
            frame_ticks: e.or(s::FRAME_TICKS, 0),
            height: e.or(s::HEIGHT, 224.33),
            drift: e.or(s::DRIFT, 0.03),
            glow: e.or(s::GLOW, 0),
            additive: e.has(s::ADDITIVE),
            count: e.or(s::COUNT, 1500),
            seed: e.or(s::SEED, 10842),
            // Behind the stars domes, in front of them bodies.
            order: e.or(
                s::ORDER,
                [0, -100, -200, 100]
                    .get(kind as usize)
                    .copied()
                    .unwrap_or(0),
            ),
        },
    ))
}

#[derive(Default)]
pub struct PacketDef {
    pub server: Option<(ModId, Callback)>,
    pub client: Option<(ModId, Callback)>,
    /// The value schema both sides agree on; sends with another are refused.
    pub schema: String,
}

// ---------------------------------------------------------------- raw registration

/// A component's declaration (`abi::comp::component`): its schema, and what entities and stacks
/// carrying it hold.
pub struct ComponentDecl {
    pub owner: ModId,
    pub name: String,
    pub schema: String,
    /// Entities carry it.
    pub entity: Option<ComponentDef>,
    /// Stacks carry it, with these `abi::stack_flags`.
    pub stack: Option<u32>,
}

/// What storages say they hold, for mods that look one up by name.
pub const STORAGE_SCHEMA: &str = "storage";

/// The engine's own components, declared like mods' are.
fn engine_components() -> Vec<RawEntry> {
    use abi::comp::component as k;
    use abi::component_flags as cf;
    let decl = |name: &str, schema: &str| RawEntry::engine(ek::COMPONENT, name).set(k::SCHEMA, schema.to_string());
    let entity = |name: &str, schema: &str, flags: u32, default: Vec<u8>| {
        k::FLAGS
            .iter()
            .filter(|f| flags & f.1 != 0)
            .fold(decl(name, schema).set(k::DEFAULT, default), |e, f| e.tag(f.0))
    };
    let shown = cf::SYNC_WATCHERS;
    let own = cf::SYNC_OWNER | shown;
    let mut shape = vec![0u8; 12];
    shape.extend(u32::MAX.to_le_bytes());
    let energy = (<abi::Energy as abi::Schema>::SCHEMA, cf::SAVE | cf::SYNC_MENU, vec![0; size_of::<abi::Energy>()]);
    // What items, entity kinds, poses and entity effects give (`content::SourceMods`).
    let modifiers = <Vec<abi::Modifier> as abi::data::Data>::schema();
    let mut all = vec![decl(abi::comp::CONTENTS, "bytes"), decl(abi::comp::LAYERS, "str"), decl(abi::comp::MODIFIERS, &modifiers)];
    // Variable length (no bytes): attribute modifiers, timers, a block entity's own name (a
    // renamed chest).
    for (name, schema, flags, default) in [
        (abi::comp::ENERGY, energy.0, energy.1, energy.2),
        (MODIFIERS, "bytes", cf::SAVE, Vec::new()),
        (TIMERS, "bytes", cf::SAVE, Vec::new()),
        (abi::comp::NAME, "bytes", cf::SAVE | cf::CARRIED, Vec::new()),
        (abi::comp::PERSISTENT, "bytes", cf::SAVE, Vec::new()),
        (HEALTH, "[f32;2]", own, vec![0; 8]),
        (BURNING, "u8", shown, vec![0]),
        (abi::comp::LIGHT, "[u8;3]", own, vec![0; 3]),
        (SHOWN, "bytes", shown, Vec::new()),
        (GROUND, "bytes", shown, Vec::new()),
        (abi::environment::COMPONENT, &<Vec<abi::environment::EnvironmentLayer> as abi::data::Data>::schema(), own | cf::SAVE, Vec::new()),
        (WORN, "bytes", shown, Vec::new()),
        (SHAPE, "bytes", own, shape),
        (HELD, "bytes", shown, Vec::new()),
        (OFF_HELD, "bytes", shown, Vec::new()),
        (USING, "u32", shown, vec![0; 4]),
        (SWAPS, "bytes", own | shown, Vec::new()),
        (LOOPING_ACTION, "bytes", shown, Vec::new()),
        (EFFECT, "bytes", own | cf::SAVE, Vec::new()),
        (FORCED_POSE, "bytes", own | shown, Vec::new()),
        (RIDING, "bytes", own | shown | cf::SAVE, Vec::new()),
        (STUCK, "bytes", shown, Vec::new()),
        (GAME_MODE, "u32", cf::SYNC_OWNER, vec![0; 4]),
        (PERMISSION, "u32", cf::SYNC_OWNER, vec![0; 4]),
        (ATTRIBUTES, "bytes", cf::SYNC_OWNER, Vec::new()),
        (SIMULATED, "u64", cf::SAVE, vec![0; 8]),
        (TRASH, "bytes", 0, Vec::new()),
    ] {
        all.push(entity(name, schema, flags, default));
    }
    all
}

fn bake_component(e: &Entry) -> ComponentDecl {
    use abi::comp::component as k;
    use abi::component_flags as cf;
    let def = |size: u32, flags: u32, default: Vec<u8>| ComponentDef { name: e.name.clone(), size, flags, default, runtime: true };
    let flags = e.flags(&k::FLAGS);
    let slots = e.get::<u32>(k::SLOTS);
    let (schema, entity) = match slots {
        // One `ItemStack` per slot, empty ones all zero.
        Some(n) => {
            let size = n.min(1 << 12) * size_of::<abi::ItemStack>() as u32;
            (STORAGE_SCHEMA.to_string(), Some(def(size, flags | cf::SAVE | cf::KEEP_ON_RESPAWN | cf::STORAGE, vec![0; size as usize])))
        }
        None => (
            e.text(k::SCHEMA).unwrap_or_default().to_string(),
            e.get::<Vec<u8>>(k::DEFAULT)
                .map(|d| def(d.len() as u32, flags, d)),
        ),
    };
    let stack = (slots.is_some() || e.has(k::STACK)).then(|| e.flags(&k::STACK_FLAGS));
    ComponentDecl { owner: e.owner, name: e.name.clone(), schema, entity, stack }
}

/// Every component's declaration: the engine's, then mods' (`abi::entry_kind::COMPONENT`).
pub fn freeze_components(raws: &mut [RawGame], mod_name: &dyn Fn(ModId) -> String, errors: &mut Vec<String>) -> Vec<ComponentDecl> {
    let mut all = engine_components();
    all.extend(entries_of(raws, ek::COMPONENT));
    bake(all, ek::COMPONENT, mod_name, errors, |e, _| bake_component(e)).0
}

#[derive(Default)]
pub struct RawRecipe {
    pub kind: String,
    pub pattern: Vec<String>,
    pub keys: Vec<(char, String)>,
    pub inputs: Vec<String>,
    pub input_counts: Vec<u32>,
    pub output: String,
    pub count: u32,
    pub time: u32,
    /// Pools for what else it gives; a liquid is its item.
    pub outputs: Vec<abi::LootPool>,
    pub uses: Vec<abi::RecipeUse>,
    /// Its entry: recipes expanded from one share it.
    pub entry: u32,
}

#[derive(Default)]
pub struct RawGame {
    /// `Host::reg_entry`, in the order the mod made them.
    pub entries: Vec<crate::entries::RawEntry>,
    /// (owner, callback) run once per tick in every region (`Host::reg_world_tick`).
    pub world_ticks: Vec<(ModId, Callback)>,
    pub chat_format: Option<(ModId, Callback)>,
    /// (owner, name, usage, callback).
    pub commands: Vec<(ModId, String, String, Callback)>,
    /// Commands that run on the client: (owner, name, usage, callback).
    pub client_commands: Vec<(ModId, String, String, Callback)>,
    pub command_extensions: Vec<(ModId, CommandExtension)>,
    pub client_frames: Vec<(ModId, Callback)>,
    /// (owner, name, side, callback).
    /// (owner, channel, schema, side, callback).
    pub packets: Vec<(ModId, String, String, u32, Callback)>,
    /// (command, level), mods' say on what their commands ask for.
    pub command_permissions: Vec<(String, u32)>,
    /// (owner, event, schema, handler).
    pub event_handlers: Vec<(ModId, String, String, Callback)>,
    /// (owner, component, schema, system).
    pub systems: Vec<(ModId, String, String, Callback)>,
}

/// A spawning rule with its names resolved (see `abi::comp::spawn`).
pub struct SpawnRule {
    pub entity: u32,
    pub category: u32,
    pub biome: Option<Box<[u64]>>,
    /// `abi::spawn_place`.
    pub place: u32,
    /// A condition (`Content::conds`) the spot and the block it rests on meet.
    pub when: Option<u32>,
    /// Blocks of room the mob needs up.
    pub height: i32,
    /// Dimensions it spawns in, empty: any.
    pub dims: Vec<u32>,
    pub y: [i32; 2],
    pub group: [u32; 2],
    pub weight: u32,
    pub chance: f32,
}

/// A mod's addition to a command registered elsewhere.
pub enum CommandExtension {
    /// `client`: the branch was written for a client command (its callback wants a client).
    Branch { command: String, usage: String, client: bool, cb: Callback },
}

/// An event mods can subscribe to; `size` is learned from the first value emitted when the
/// engine does not know it.
pub struct EventDef {
    pub name: String,
    pub schema: String,
    pub size: std::sync::atomic::AtomicU32,
    pub handlers: Vec<(ModId, Callback)>,
}

/// A mod's screen.
/// Something a mod adds to a place of a screen.
#[derive(Clone)]
pub struct ScreenPart {
    pub screen: String,
    pub area: String,
    pub order: i32,
    pub owner: ModId,
    pub cb: Callback,
}

/// Slots of every player's inventory, laid out by the engine from the groups and equipment
/// slots mods declare.
#[derive(Clone, Debug)]
pub struct PlayerLayout {
    pub slots: u32,
    /// The first this many are the hotbar.
    pub hotbar: u32,
    /// The slot of the second hand, past the equipment, if there is one.
    pub off: Option<u32>,
    /// Equipment: never filled by pickups, worn on the model.
    pub worn: std::ops::Range<u32>,
    /// Named groups: the declared ones, each equipment slot, and `core:equipment` for all of them.
    pub groups: Vec<PlayerGroup>,
    pub equipment: Vec<EquipmentSlot>,
}

#[derive(Clone, Debug)]
pub struct PlayerGroup {
    pub name: String,
    pub start: u32,
    pub count: u32,
    pub cols: u32,
}

#[derive(Clone, Debug)]
pub struct EquipmentSlot {
    pub name: String,
    /// Bones it covers by name, and the roles (`1 << role`) of the names that have one.
    pub bones: Vec<String>,
    pub roles: u32,
    pub background: String,
}

/// The group holding every equipment slot.
pub const EQUIPMENT_GROUP: &str = "core:equipment";

impl Default for PlayerLayout {
    fn default() -> Self {
        PlayerLayout::new(&[("hotbar".into(), 9, 1, weft_abi::slot_flags::HOTBAR), ("main".into(), 9, 3, 0)], Vec::new())
    }
}

impl PlayerLayout {
    /// The hotbar group first, then the other groups in order, then the equipment slots.
    pub fn new(groups: &[(String, u32, u32, u32)], mut equipment: Vec<EquipmentSlot>) -> PlayerLayout {
        // The other hand shows among the equipment but isn't worn: last, past `worn`.
        equipment.sort_by_key(|e| e.name == weft_abi::OFF_HAND_SLOT);
        let off = equipment.iter().any(|e| e.name == weft_abi::OFF_HAND_SLOT) as u32;
        let mut ordered: Vec<&(String, u32, u32, u32)> = groups
            .iter()
            .filter(|g| g.3 & weft_abi::slot_flags::HOTBAR != 0)
            .take(1)
            .collect();
        ordered.extend(
            groups
                .iter()
                .filter(|g| g.3 & weft_abi::slot_flags::HOTBAR == 0),
        );
        let hotbar = ordered
            .first()
            .filter(|g| g.3 & weft_abi::slot_flags::HOTBAR != 0)
            .map_or(0, |g| g.1 * g.2);
        let mut at = 0;
        let mut out = Vec::new();
        for (name, cols, rows, _) in ordered {
            out.push(PlayerGroup { name: name.clone(), start: at, count: cols * rows, cols: *cols });
            at += cols * rows;
        }
        let worn = at..at + equipment.len() as u32 - off;
        for (i, e) in equipment.iter().enumerate() {
            out.push(PlayerGroup { name: e.name.clone(), start: worn.start + i as u32, count: 1, cols: 1 });
        }
        out.push(PlayerGroup { name: EQUIPMENT_GROUP.into(), start: worn.start, count: worn.len() as u32, cols: 1 });
        PlayerLayout { slots: worn.end + off, off: (off == 1).then_some(worn.end), hotbar, worn, groups: out, equipment }
    }

    pub fn group(&self, name: &str) -> Option<&PlayerGroup> {
        self.groups.iter().find(|g| g.name == name)
    }
}

impl PlayerLayout {
    /// Main inventory slots: neither hotbar nor equipment.
    pub fn main(&self) -> impl Iterator<Item = u32> + '_ {
        (self.hotbar..self.slots).filter(move |i| !self.worn.contains(i) && Some(*i) != self.off)
    }
}

/// A sound category: its volume setting and whether caves and water change it.
#[derive(Clone, Debug)]
pub struct SoundCategory {
    pub name: String,
    /// `<ns>:volume_<name>`, none for `core:ui`.
    pub setting: Option<String>,
    pub world: bool,
}

impl SoundCategory {
    pub fn new(name: &str, world: bool) -> SoundCategory {
        let (ns, short) = name.split_once(':').unwrap_or(("core", name));
        let setting = (name != "core:ui").then(|| format!("{ns}:volume_{short}"));
        SoundCategory { name: name.to_string(), setting, world }
    }
}

/// Music first (`MUSIC_CATEGORY`); the order is also that of the volume settings.
fn engine_sound_categories() -> Vec<RawEntry> {
    ["music", "blocks", "creatures", "players", "ambient", "ui"]
        .iter()
        .map(|n| {
            let e = RawEntry::engine(abi::entry_kind::SOUND_CATEGORY, &format!("core:{n}"));
            if *n == "music" || *n == "ui" { e } else { e.tag(abi::comp::SOUND_CATEGORY_WORLD) }
        })
        .collect()
}

/// `SoundDef::category` of the engine's music track: the first category.
pub const MUSIC_CATEGORY: u32 = 0;

/// A damage cause: what the engine does differently for it (`crate::tag_bits::damage`), and how a
/// death by it reads.
#[derive(Clone, Debug)]
pub struct DamageCauseDef {
    pub name: String,
    pub flags: u32,
    pub death_message: String,
    pub death_message_by: String,
}

fn engine_damage_causes() -> Vec<RawEntry> {
    use abi::comp::damage_cause as d;
    let tags: [&[&str]; 14] = [
        &[],
        &[d::BYPASSES_ARMOR, d::BYPASSES_BLOCKING],
        &[],
        &[d::BURNING, d::BYPASSES_BLOCKING],
        &[d::BYPASSES_ARMOR, d::BYPASSES_BLOCKING],
        &[d::BYPASSES_ARMOR, d::BYPASSES_BLOCKING],
        &[d::BYPASSES_ARMOR, d::PIERCES_INVULNERABLE, d::IGNORES_COOLDOWN, d::BYPASSES_BLOCKING],
        &[d::BURNING, d::BYPASSES_BLOCKING],
        &[d::BYPASSES_BLOCKING],
        &[d::BYPASSES_ARMOR, d::BYPASSES_BLOCKING],
        &[],
        &[],
        &[],
        &[d::BYPASSES_ARMOR, d::BYPASSES_BLOCKING],
    ];
    let messages = [
        "generic",
        "fall",
        "generic",
        "fire",
        "starve",
        "drown",
        "void",
        "lava",
        "crush",
        "suffocate",
        "explosion",
        "generic",
        "projectile",
        "cramming",
    ];
    abi::damage_cause::NAMES
        .iter()
        .zip(tags)
        .zip(messages)
        .map(|((name, tags), m)| {
            let mut e = RawEntry::engine(abi::entry_kind::DAMAGE_CAUSE, name).set(d::DEATH_MESSAGE, format!("death.{m}"));
            if *name == "core:attack" {
                e = e.set(d::DEATH_MESSAGE_BY, "death.attack".to_string());
            }
            if *name == "core:projectile" {
                e = e.set(d::DEATH_MESSAGE_BY, "death.projectile_by".to_string());
            }
            tags.iter().fold(e, |e, t| e.tag(t))
        })
        .collect()
}

fn bake_damage_cause(e: &Entry) -> DamageCauseDef {
    use abi::comp::damage_cause as d;
    let text = |c: &str| e.text(c).unwrap_or_default().to_string();
    DamageCauseDef {
        name: e.name.clone(),
        flags: e.flags(crate::tag_bits::damage::TAGS),
        death_message: text(d::DEATH_MESSAGE),
        death_message_by: text(d::DEATH_MESSAGE_BY),
    }
}

impl DamageCauseDef {
    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

#[derive(Clone, Debug)]
pub struct WeatherDef {
    pub name: String,
    /// `crate::tag_bits::weather`.
    pub flags: u32,
    pub look: abi::WeatherLook,
    pub sound_name: String,
    /// `sound_name` resolved at freeze.
    pub sound: Option<u32>,
    pub duration: [u64; 2],
    pub weight: u32,
}

impl WeatherDef {
    pub fn falls(&self) -> bool {
        self.flags & crate::tag_bits::weather::FALLS != 0
    }
    pub fn lightning(&self) -> bool {
        self.flags & crate::tag_bits::weather::LIGHTNING != 0
    }
    pub fn shelters(&self) -> bool {
        self.flags & crate::tag_bits::weather::SHELTERS != 0
    }
}

/// Vanilla's timing: clear for 10 minutes to 2.5 hours, rain for 10 to 20 minutes, a quarter of
/// it with thunder.
fn engine_weathers() -> Vec<RawEntry> {
    use weft_abi::comp::weather as w;
    let e = |name: &str, duration: [u32; 2], weight: u32| {
        RawEntry::engine(weft_abi::entry_kind::WEATHER, &format!("core:{name}"))
            .set(w::DURATION, duration)
            .set(w::WEIGHT, weight)
    };
    let clear = [w::FALLS, w::SKY_GRAY, w::SKY_COVER, w::FOG_GRAY, w::CLOUD_GRAY, w::DARKNESS]
        .into_iter()
        .fold(e("clear", [12000, 180000], 0), |e, c| e.set(c, 0.0f32))
        .tag(w::DRY)
        .tag(w::UNSHELTERED);
    vec![
        clear,
        e("rain", [12000, 24000], 3),
        e("thunder", [12000, 24000], 1)
            .set(w::CLOUD_DARK, 0.95f32)
            .tag(w::LIGHTNING),
    ]
}

/// A weather from its entry: rain's look and flags where it says nothing.
fn bake_weather(e: &crate::entries::Entry) -> WeatherDef {
    use crate::tag_bits::weather as f;
    use weft_abi::comp::weather as w;
    let share = |c: &str, rain: f32| e.or(c, rain).clamp(0.0, 1.0);
    let flags =
        (!e.has(w::DRY) as u32 * f::FALLS) | (e.has(w::LIGHTNING) as u32 * f::LIGHTNING) | (!e.has(w::UNSHELTERED) as u32 * f::SHELTERS);
    let duration: [u32; 2] = e.or(w::DURATION, [12000, 24000]);
    WeatherDef {
        name: e.name.clone(),
        flags,
        look: abi::WeatherLook {
            falls: share(w::FALLS, 1.0),
            sky_gray: share(w::SKY_GRAY, 0.75),
            sky_cover: share(w::SKY_COVER, 1.0),
            fog_gray: share(w::FOG_GRAY, 0.5),
            cloud_gray: share(w::CLOUD_GRAY, 0.95),
            cloud_dark: share(w::CLOUD_DARK, 0.0),
            darkness: share(w::DARKNESS, 0.3125),
        },
        sound_name: e.text(w::SOUND).unwrap_or_default().to_string(),
        sound: None,
        duration: [duration[0].max(1) as u64, duration[1].max(duration[0]).max(1) as u64],
        weight: e.or(w::WEIGHT, 1),
    }
}

#[derive(Clone, Debug)]
pub struct PrecipitationDef {
    pub name: String,
    pub texture: String,
    pub speed: f32,
    pub density: f32,
}

/// A pose: its box and eyes as shares of standing, how fast it goes, how the model lies and
/// the clip it plays, and when a player takes it (`abi::PoseDesc`).
#[derive(Clone, Debug)]
pub struct PoseDef {
    pub name: String,
    pub height: f64,
    pub eye: f64,
    pub speed: f32,
    /// `abi::pose_tilt`.
    pub tilt: u32,
    pub action: String,
    /// The camera bobs while walking in it.
    pub bob: bool,
    pub priority: i32,
    /// The action held for it, empty for none.
    pub key: String,
    /// A condition (`Content::conds`) it is taken under.
    pub when: Option<u32>,
    /// Names as registered, then the poses they are (`fallback`).
    pub fallback_names: String,
    pub fallback: Vec<crate::world::Pose>,
}

fn engine_poses() -> Vec<RawEntry> {
    use abi::comp::pose as p;
    use abi::pose_tilt as t;
    let pose = |name: &str, height: f32, eye: f32, speed: f32, tilt: u32, action: &str, priority: i32| {
        RawEntry::engine(abi::entry_kind::POSE, &format!("core:{name}"))
            .set(p::HEIGHT, height)
            .set(p::EYE, eye)
            .set(p::SPEED, speed)
            .set(p::TILT, tilt)
            .set(p::ACTION, action.to_string())
            .set(p::PRIORITY, priority)
    };
    let fallback = |e: RawEntry, poses: &[&str]| e.set(p::FALLBACK, poses.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    let when = |e: RawEntry, c: &str| e.set(p::WHEN, vec![c.to_string()]);
    // Modern vanilla's: the player's 1.8 and 1.62 standing. Under a low ceiling it crouches, and
    // where that has no room either it swims in a liquid and crawls out of one.
    vec![
        fallback(pose("stand", 1.0, 1.0, 1.0, t::UPRIGHT, "", 0), &["core:crouch", "core:swim", "core:crawl"]),
        when(fallback(pose("crouch", 1.5 / 1.8, 1.27 / 1.62, 0.3, t::UPRIGHT, "sneak", 10), &["core:swim", "core:crawl"]), "!flying")
            .set(p::KEY, "core:sneak".to_string()),
        when(
            fallback(pose("swim", 0.6 / 1.8, 0.4 / 1.62, 1.0, t::ALONG_LOOK, "swim", 30), &["core:crouch", "core:crawl"]),
            "sprinting & in_liquid & !flying",
        )
        .tag(p::NO_BOB),
        when(pose("crawl", 0.6 / 1.8, 0.4 / 1.62, 0.3, t::FACE_DOWN, "crawl", 20), "!flying").set(p::KEY, "core:crawl".to_string()),
        // Only where a mod holds it.
        pose("sleep", 0.2 / 1.8, 0.45 / 1.62, 0.0, t::ON_BACK, "", i32::MIN).tag(p::NO_BOB),
    ]
}

fn bake_pose(e: &Entry, conds: &mut crate::conditions::Texts) -> PoseDef {
    use abi::comp::pose as p;
    let share = |c: &str| {
        e.get::<f32>(c)
            .filter(|v| v.is_finite() && *v > 0.0)
            .map_or(1.0, |v| v as f64)
    };
    PoseDef {
        name: e.name.clone(),
        height: share(p::HEIGHT),
        eye: share(p::EYE),
        speed: e.or(p::SPEED, 1.0f32).max(0.0),
        tilt: e.or(p::TILT, 0u32).min(abi::pose_tilt::ON_BACK),
        action: e.text(p::ACTION).unwrap_or_default().to_string(),
        bob: !e.has(p::NO_BOB),
        priority: e.or(p::PRIORITY, 0),
        key: e.text(p::KEY).unwrap_or_default().to_string(),
        when: conds.all(&e.or::<Vec<String>>(p::WHEN, Vec::new())),
        fallback_names: e.or::<Vec<String>>(p::FALLBACK, Vec::new()).join(" "),
        fallback: Vec::new(),
    }
}

pub fn engine_pose_defs() -> &'static [PoseDef] {
    static DEFS: std::sync::OnceLock<Vec<PoseDef>> = std::sync::OnceLock::new();
    DEFS.get_or_init(|| {
        let (mut defs, _) = crate::entries::bake(engine_poses(), abi::entry_kind::POSE, &|_| String::new(), &mut Vec::new(), |e, _| {
            bake_pose(e, &mut Default::default())
        });
        let names: Vec<String> = defs.iter().map(|d| d.name.clone()).collect();
        for d in &mut defs {
            d.fallback = d
                .fallback_names
                .split_whitespace()
                .filter_map(|n| names.iter().position(|x| x == n))
                .map(|i| crate::world::Pose(i as u16))
                .collect();
        }
        defs
    })
}

/// A game mode: what its players may do (`crate::tag_bits::game_mode`) and the attribute modifiers
/// they get, (attribute, op, amount).
#[derive(Clone, Debug)]
pub struct GameModeDef {
    pub name: String,
    pub flags: u32,
    /// What its players get (`content::SourceMods`).
    pub modifiers: Vec<abi::Modifier>,
}

fn engine_game_modes() -> Vec<RawEntry> {
    let tag = |f: u32| {
        crate::tag_bits::game_mode::TAGS
            .iter()
            .find(|t| t.1 == f)
            .unwrap()
            .0
    };
    let modifier = |attribute: &str, step: &str, amount: f64| abi::Modifier {
        attribute: attribute.to_string(),
        step: step.to_string(),
        amount,
        ..Default::default()
    };
    let mode = |name: &str, flags: &[u32], modifiers: Vec<abi::Modifier>| {
        flags
            .iter()
            .fold(RawEntry::engine(abi::entry_kind::GAME_MODE, name), |e, f| e.tag(tag(*f)))
            .set(abi::comp::MODIFIERS, modifiers)
    };
    let flies = || modifier(abi::attribute::CAN_FLY, abi::step::ADD, 1.0);
    let spared = || modifier(abi::attribute::INVULNERABLE, abi::step::SET, 1.0);
    use crate::tag_bits::game_mode as f;
    // Unseen by mobs outside survival; creative reaches further.
    let unseen = || modifier(abi::attribute::VISIBILITY, abi::step::MUL, 0.0);
    vec![
        mode("core:survival", &[], Vec::new()),
        mode(
            "core:creative",
            &[f::INSTANT_BREAK, f::NO_COSTS, f::CREATIVE_INVENTORY],
            vec![
                modifier(abi::attribute::BLOCK_REACH, abi::step::ADD, 0.5),
                modifier(abi::attribute::ENTITY_REACH, abi::step::ADD, 2.0),
                unseen(),
                flies(),
                spared(),
            ],
        ),
        mode(
            "core:spectator",
            &[f::ALWAYS_FLY, f::NOCLIP, f::NO_INTERACT, f::HIDDEN, f::NO_HUD, f::NO_COSTS],
            vec![unseen(), flies(), spared()],
        ),
    ]
}

fn bake_game_mode(e: &Entry) -> GameModeDef {
    GameModeDef {
        name: e.name.clone(),
        flags: e.flags(crate::tag_bits::game_mode::TAGS),
        modifiers: e.or(abi::comp::MODIFIERS, Vec::new()),
    }
}

fn engine_precipitations() -> Vec<RawEntry> {
    use abi::comp::precipitation as p;
    let e = |name: &str, texture: &str, speed: f32, density: f32| {
        RawEntry::engine(abi::entry_kind::PRECIPITATION, name)
            .set(p::TEXTURE, texture.to_string())
            .set(p::SPEED, speed)
            .set(p::DENSITY, density)
    };
    vec![e("core:rain", "core:environment/rain", 1.0, 0.1), e("core:snow", "core:environment/snow", 0.1, 0.5)]
}

fn bake_precipitation(e: &Entry) -> PrecipitationDef {
    use abi::comp::precipitation as p;
    PrecipitationDef {
        name: e.name.clone(),
        texture: e.text(p::TEXTURE).unwrap_or_default().to_string(),
        speed: e.or(p::SPEED, 1.0f32).max(0.0),
        density: e.or(p::DENSITY, 0.1f32).clamp(0.0, 1.0),
    }
}

/// A shader effect on entities: the snippet that draws it, `ns:name`.
#[derive(Clone)]
pub struct EntityEffectDef {
    pub name: String,
    /// Empty: none.
    pub shader: String,
    /// Named by its mod, not its own name by default: missing, it is a mistake.
    pub shader_given: bool,
    /// A status effect's (its player sees it): the texture of its icon; empty for none.
    pub icon: String,
    /// Of the swirls around who has it, `0xRRGGBB`; none without.
    pub color: Option<u32>,
    /// `effect_kind`: beneficial, harmful or neither.
    pub kind: u8,
    /// Acts once when given and doesn't stay.
    pub instant: bool,
    /// Its entity isn't drawn for others.
    pub hides: bool,
    /// As registered, then resolved at freeze.
    pub raw_hazards: Vec<abi::Hazard>,
    pub hazards: Vec<crate::content::Hazard>,
    /// Given off while it is on.
    pub light: [u8; 3],
    /// Its tick with every entity that has it (`comp::ENTITY_EFFECT_ON_TICK`).
    pub on_tick: Option<(ModId, Callback)>,
}

/// Whether an entity effect is good for who has it (`EntityEffectDef::kind`).
pub mod effect_kind {
    pub const NEUTRAL: u8 = 0;
    pub const BENEFICIAL: u8 = 1;
    pub const HARMFUL: u8 = 2;
}

impl EntityEffectDef {
    /// Its hazards at `level`: those given for the highest level up to it, else the others times
    /// the level.
    pub fn hazards_at(&self, level: u32) -> Vec<crate::content::Hazard> {
        if let Some(at) = self
            .hazards
            .iter()
            .map(|h| h.level)
            .filter(|&l| l > 0 && l <= level)
            .max()
        {
            return self
                .hazards
                .iter()
                .filter(|h| h.level == at)
                .copied()
                .collect();
        }
        self.hazards
            .iter()
            .filter(|h| h.level == 0)
            .map(|h| crate::content::Hazard { damage: h.damage * level as f32, ..*h })
            .collect()
    }

    /// A status effect: its player sees it.
    pub fn shown(&self) -> bool {
        !self.icon.is_empty()
    }
}

/// What natural spawning and despawning know of entities by category.
#[derive(Clone, Debug)]
pub struct CategoryDef {
    pub name: String,
    /// At most this many spawn naturally within 64 blocks of a player.
    pub cap: u32,
    /// Further than this from every player they vanish (checked every 5 s); 0 = never.
    pub despawn: f64,
    /// The cap counts them this close to a player.
    pub cap_radius: f64,
}

fn engine_categories() -> Vec<RawEntry> {
    use abi::comp::entity_category as c;
    [("core:misc", 6, 0.0f32), ("core:hostile", 8, 128.0), ("core:passive", 6, 0.0)]
        .iter()
        .map(|&(name, cap, despawn)| {
            RawEntry::engine(abi::entry_kind::ENTITY_CATEGORY, name)
                .set(c::CAP, cap as u32)
                .set(c::DESPAWN, despawn)
        })
        .collect()
}

fn bake_category(e: &Entry) -> CategoryDef {
    use abi::comp::entity_category as c;
    CategoryDef {
        name: e.name.clone(),
        cap: e.or(c::CAP, 0),
        despawn: e.or(c::DESPAWN, 0.0f32) as f64,
        cap_radius: e
            .get::<f32>(c::CAP_RADIUS)
            .filter(|r| *r > 0.0)
            .map_or(64.0, |r| r as f64),
    }
}

pub struct AttrDef {
    pub name: String,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    /// Stable key in saved modifiers.
    pub hash: u64,
    /// The entity kind component of the same name holding a kind's own value.
    pub comp: Option<u32>,
    /// Players' base of it is the world rule of its name (`comp::attribute::RULE`).
    pub rule: bool,
}

/// The engine's attributes by id, first in the registry in `engine_attributes`' order.
pub mod attr {
    pub const MAX_HEALTH: u32 = 0;
    pub const MOVEMENT_SPEED: u32 = 1;
    pub const VISIBILITY: u32 = 2;
    pub const JUMP_STRENGTH: u32 = 3;
    pub const ATTACK_SPEED: u32 = 4;
    pub const CAN_SPRINT: u32 = 5;
    pub const BLOCK_REACH: u32 = 6;
    pub const ENTITY_REACH: u32 = 7;
    pub const MINING_SPEED: u32 = 8;
    pub const FOV: u32 = 9;
    pub const CAN_FLY: u32 = 10;
    pub const INVULNERABLE: u32 = 11;
    pub const SAFE_FALL: u32 = 12;
    pub const FALL_DAMAGE_MULTIPLIER: u32 = 13;
    pub const STEP_HEIGHT: u32 = 14;
    pub const GRAVITY: u32 = 15;
    pub const KNOCKBACK_RESISTANCE: u32 = 16;
    pub const ATTACK_DAMAGE: u32 = 17;
    pub const DAMAGE_RESISTANCE: u32 = 18;
    pub const FIRE_RESISTANCE: u32 = 19;
    pub const BURNING_TIME: u32 = 20;
    pub const SUBMERGED_MINING_SPEED: u32 = 21;
    pub const AIRBORNE_MINING_SPEED: u32 = 22;
    pub const NIGHT_VISION: u32 = 23;
    pub const SWIM_SPEED: u32 = 24;
    pub const FLYING_SPEED: u32 = 25;
    pub const CRIT_DAMAGE: u32 = 26;
    pub const SWEEPING_DAMAGE_RATIO: u32 = 27;
    pub const ATTACK_KNOCKBACK: u32 = 28;
    pub const HURT_COOLDOWN: u32 = 29;
    pub const AIR_CONTROL: u32 = 30;
    pub const SLIPPERINESS: u32 = 31;
    pub const CLIMB_SPEED: u32 = 32;
}

fn engine_attributes() -> Vec<RawEntry> {
    use abi::attribute as a;
    // In `attr`'s order.
    [
        (a::MAX_HEALTH, 20.0, 0.5, 1024.0),
        (a::MOVEMENT_SPEED, 1.0, 0.0, 16.0),
        (a::VISIBILITY, 1.0, 0.0, 16.0),
        (a::JUMP_STRENGTH, 1.0, 0.0, 16.0),
        (a::ATTACK_SPEED, 1.0, 0.0, 16.0),
        (a::CAN_SPRINT, 1.0, 0.0, 1.0),
        (a::BLOCK_REACH, 4.5, 0.0, 1024.0),
        (a::ENTITY_REACH, 3.0, 0.0, 1024.0),
        (a::MINING_SPEED, 1.0, 0.0, 1024.0),
        (a::FOV, 1.0, 0.05, 8.0),
        (a::CAN_FLY, 0.0, 0.0, 1.0),
        (a::INVULNERABLE, 0.0, 0.0, 1.0),
        (a::SAFE_FALL, 3.0, 0.0, 1024.0),
        (a::FALL_DAMAGE_MULTIPLIER, 1.0, 0.0, 100.0),
        (a::STEP_HEIGHT, 0.6, 0.0, 10.0),
        (a::GRAVITY, 1.0, -8.0, 8.0),
        (a::KNOCKBACK_RESISTANCE, 0.0, 0.0, 1.0),
        (a::ATTACK_DAMAGE, 1.0, 0.0, 2048.0),
        (a::DAMAGE_RESISTANCE, 0.0, 0.0, 1.0),
        (a::FIRE_RESISTANCE, 0.0, 0.0, 1.0),
        (a::BURNING_TIME, 1.0, 0.0, 100.0),
        (a::SUBMERGED_MINING_SPEED, 0.2, 0.0, 20.0),
        (a::AIRBORNE_MINING_SPEED, 0.2, 0.0, 20.0),
        (a::NIGHT_VISION, 0.0, 0.0, 1.0),
        (a::SWIM_SPEED, 1.0, 0.0, 16.0),
        (a::FLYING_SPEED, 1.0, 0.0, 16.0),
        (a::CRIT_DAMAGE, 1.5, 0.0, 1024.0),
        (a::SWEEPING_DAMAGE_RATIO, 0.0, 0.0, 1024.0),
        (a::ATTACK_KNOCKBACK, 0.0, 0.0, 16.0),
        (a::HURT_COOLDOWN, 10.0, 0.0, 1024.0),
        (a::AIR_CONTROL, 1.0, 0.0, 16.0),
        (a::SLIPPERINESS, 1.0, 0.0, 2.0),
        (a::CLIMB_SPEED, 1.0, 0.0, 16.0),
    ]
    .iter()
    .map(|&(name, default, min, max)| {
        let e = RawEntry::engine(abi::entry_kind::ATTRIBUTE, name)
            .set(abi::comp::attribute::DEFAULT, default)
            .set(abi::comp::attribute::RANGE, [min, max]);
        // How many hearts players (and kinds that don't say) have is the world's to say.
        if name == a::MAX_HEALTH { e.tag(abi::comp::attribute::RULE) } else { e }
    })
    .collect()
}

/// Events the engine emits, with the schemas of their `abi` structs.
pub const ENGINE_EVENTS: &[(&str, &str, u32)] = {
    use abi::Schema;
    &[
        (abi::event::BLOCK_TICK, "[i32;3]", 12),
        (abi::action::BREAK_BLOCK, abi::BlockChange::SCHEMA, size_of::<abi::BlockChange>() as u32),
        (abi::action::PLACE_BLOCK, abi::BlockChange::SCHEMA, size_of::<abi::BlockChange>() as u32),
        (abi::action::DIE, abi::Death::SCHEMA, size_of::<abi::Death>() as u32),
        (abi::event::PLAYER_JOINED, abi::PlayerEvent::SCHEMA, size_of::<abi::PlayerEvent>() as u32),
        (abi::event::PLAYER_RESPAWNED, abi::PlayerEvent::SCHEMA, size_of::<abi::PlayerEvent>() as u32),
        (abi::action::PICKUP_ITEM, abi::Pickup::SCHEMA, size_of::<abi::Pickup>() as u32),
        (abi::action::CHAT, abi::ChatMessage::SCHEMA, size_of::<abi::ChatMessage>() as u32),
        (abi::action::HURT, abi::Hurt::SCHEMA, size_of::<abi::Hurt>() as u32),
        (abi::action::USE_ITEM, abi::ItemUsed::SCHEMA, size_of::<abi::ItemUsed>() as u32),
        (abi::event::ENTITY_HEALED, abi::Hurt::SCHEMA, size_of::<abi::Hurt>() as u32),
        (abi::event::WEATHER_TICK, abi::WeatherTick::SCHEMA, size_of::<abi::WeatherTick>() as u32),
        (abi::action::USE_BLOCK, abi::BlockChange::SCHEMA, size_of::<abi::BlockChange>() as u32),
        (abi::action::INTERACT_ENTITY, abi::Interaction::SCHEMA, size_of::<abi::Interaction>() as u32),
        (abi::action::MOUNT, abi::Mounting::SCHEMA, size_of::<abi::Mounting>() as u32),
        (abi::action::DISMOUNT, abi::Mounting::SCHEMA, size_of::<abi::Mounting>() as u32),
        (abi::action::ENTER_DIMENSION, abi::DimensionChange::SCHEMA, size_of::<abi::DimensionChange>() as u32),
        (abi::action::SPAWN, abi::Spawned::SCHEMA, size_of::<abi::Spawned>() as u32),
        (abi::event::ENTITY_REMOVED, abi::Removed::SCHEMA, size_of::<abi::Removed>() as u32),
        (abi::action::EFFECT, abi::EffectChange::SCHEMA, size_of::<abi::EffectChange>() as u32),
        (abi::event::EFFECT_ENDED, abi::EffectChange::SCHEMA, size_of::<abi::EffectChange>() as u32),
        (abi::action::PROJECTILE_HIT, abi::ProjectileHit::SCHEMA, size_of::<abi::ProjectileHit>() as u32),
        (abi::event::KEY, abi::KeyChange::SCHEMA, size_of::<abi::KeyChange>() as u32),
    ]
};

/// The engine's own entries of a kind, before every mod's (settings aside: they come of the keys).
pub fn engine_entries(kind: u32) -> Vec<RawEntry> {
    match kind {
        ek::PARTICLE => engine_particles(),
        ek::SOUND_CATEGORY => engine_sound_categories(),
        ek::SOUND => engine_sounds(),
        ek::ENTITY_CATEGORY => engine_categories(),
        ek::CAMERA => engine_cameras(),
        ek::KEY => engine_keys(),
        ek::ATTRIBUTE => engine_attributes(),
        ek::DAMAGE_CAUSE => engine_damage_causes(),
        ek::GAME_MODE => engine_game_modes(),
        ek::POSE => engine_poses(),
        ek::PRECIPITATION => engine_precipitations(),
        ek::TEXT_PLACE => crate::hud::engine_places(),
        ek::WEATHER => engine_weathers(),
        // Fire's light, without a look of its own.
        ek::ENTITY_EFFECT => abi::entity_effect::NAMES
            .iter()
            .enumerate()
            .map(|(i, n)| match RawEntry::engine(kind, n) {
                e if i as u32 == abi::entity_effect::BURNING => e
                    .set(abi::comp::ENTITY_EFFECT_SHADER, String::new())
                    .set(abi::comp::LIGHT, [15u8, 12, 6]),
                e => e,
            })
            .collect(),
        ek::CONDITION => abi::condition::NAMES
            .iter()
            .map(|n| RawEntry::engine(kind, n))
            .collect(),
        ek::PHASE | ek::STEP => crate::steps::engine_entries(kind),
        _ => Vec::new(),
    }
}

/// An action players bind keys to (`entry_kind::KEY`).
pub struct KeyDef {
    pub name: String,
    pub default: crate::input::Slots,
    /// `input::context` bits.
    pub contexts: u32,
}

fn engine_keys() -> Vec<RawEntry> {
    crate::input::ENGINE_KEYS
        .iter()
        .map(|&(name, keys, contexts)| {
            crate::input::context::TAGS
                .iter()
                .filter(|t| contexts & t.1 != 0)
                .fold(RawEntry::engine(ek::KEY, name).set(abi::comp::KEY_DEFAULT, keys.to_string()), |e, t| e.tag(t.0))
        })
        .collect()
}

/// FNV-1a, for names stored in saves.
pub fn name_hash(s: &str) -> u64 {
    weft_noise::fnv(s.as_bytes())
}

/// A natural spawning rule with its names resolved; `None` (and an error) for a name that isn't there.
fn bake_spawn(
    e: &Entry,
    g: &Game,
    c: &Components,
    conds: &mut crate::conditions::Texts,
    dim_names: &FxHashMap<String, u32>,
    biome_names: &FxHashMap<String, u32>,
    errors: &mut Vec<String>,
) -> Option<SpawnRule> {
    use abi::comp::{common as p, spawn as s};
    let what = format!("spawn rule {}", e.name);
    let name = e.text(s::ENTITY).unwrap_or_default();
    let Some(&entity) = g.entity_names.get(name) else {
        errors.push(format!("{what}: unknown entity {name:?}"));
        return None;
    };
    let biome = crate::content::biomes(c, biome_names, &what, e, errors).ok()?;
    let names: Vec<String> = e.or(p::DIMENSIONS, Vec::new());
    let dims = match names.is_empty() {
        true => Vec::new(),
        false => crate::refs::set(&names.join(" "), comps::DIMENSION, dim_names, dim_names.len() as u32, c, "dimension")
            .map_err(|err| errors.push(format!("{what}: {err}")))
            .ok()?,
    };
    let kind = &g.entities[entity as usize];
    let [min, max] = e.or(s::GROUP, [1u32, 1]);
    Some(SpawnRule {
        entity,
        category: kind.category,
        biome,
        place: e
            .or(s::PLACE, abi::spawn_place::GROUND)
            .min(abi::spawn_place::CEILING),
        height: (kind.height.ceil() as i32).max(1),
        when: conds.all(&e.or::<Vec<String>>(s::WHEN, Vec::new())),
        dims,
        y: e.or(p::Y, [i32::MIN, i32::MAX]),
        group: [min.max(1), max.max(min).max(1)],
        weight: e.or(s::WEIGHT, 1),
        chance: e.or(s::CHANCE, 1.0),
    })
}

/// A model's parts as the engine draws them.
fn parts_of(list: Vec<abi::ModelPart>) -> Vec<Part> {
    let mut parts = Vec::new();
    for p in list {
        let cuboids = p
            .cuboids
            .iter()
            .map(|c| Cuboid {
                origin: c.origin,
                size: c.extent,
                uv: c.uv,
                inflate: c.inflate,
                mirror: c.mirror != 0,
                faces: (c.per_face != 0).then_some(c.faces),
            })
            .collect();
        let n = parts.len() as i32;
        let parent = (p.parent >= 0 && p.parent < n).then_some(p.parent as u32);
        parts.push(Part { name: p.name, parent, pivot: p.pivot, role: 0, rest: p.rest, cuboids, mesh: p.mesh });
    }
    crate::model::assign_roles(&mut parts);
    parts
}

/// An entity kind as its entry says (`abi::comp::entity`).
fn bake_entity(e: &Entry) -> EntityKind {
    use abi::comp::entity as k;
    let size = e.or(k::SIZE, abi::BodySize { width: 0.6, height: 1.8 });
    let (ns, path) = e.name.split_once(':').unwrap_or(("core", &e.name));
    let text = |name: &str| e.text(name).unwrap_or_default().to_string();
    EntityKind {
        name: e.name.clone(),
        owner: e.owner,
        width: size.width,
        height: size.height,
        eye: e.or(k::EYE, size.height * EYE),
        speed: e.or(k::SPEED, 3.0),
        flags: (ef::FALL_DAMAGE | ef::PUSHABLE | e.flags(crate::tag_bits::entity::TAGS)) & !e.flags(crate::tag_bits::entity::OFF),
        category: 0,
        category_name: text(k::CATEGORY),
        texture: e
            .text(k::TEXTURE)
            .map_or_else(|| format!("{ns}:entity/{path}"), str::to_string),
        texture_size: e.or(k::TEXTURE_SIZE, [64, 32]),
        parts: parts_of(e.or(k::PARTS, Vec::new())),
        bone_models: e
            .or::<Vec<abi::BoneModel>>(k::BONE_MODELS, Vec::new())
            .into_iter()
            .map(|b| (b.model, b.bone))
            .collect(),
        on_tick: e.callback(k::ON_TICK).into_iter().collect(),
        on_interact: e.callback(k::ON_INTERACT).into_iter().collect(),
        frames: e.callbacks(k::FRAMES),
        components: Vec::new(),
        values: Vec::new(),
        slots: 0,
        worn: 0..0,
        off: None,
        hand_slots: 0,
        effects: [crate::content::Fx::NONE; 4],
        carry_height: e.or(k::CARRY_HEIGHT, 0.0),
        light_at: match e.get::<[f32; 3]>(k::LIGHT_AT) {
            Some(v) => glam::Vec3::from(v).as_dvec3(),
            None if e.text(k::BLOCK).is_some() => glam::DVec3::ZERO,
            None => glam::DVec3::Y * (size.height as f64 * 0.5),
        },
        shown_slots: e.or(k::SHOWN_SLOTS, Vec::new()),
        raw_hazards: e.or(k::HAZARDS, Vec::new()),
        hazards: Vec::new(),
        template_name: text(k::TEMPLATE),
        template: None,
        seats: e
            .or::<Vec<abi::Seat>>(k::SEATS, Vec::new())
            .into_iter()
            .take(255)
            .map(|s| Seat { pos: crate::world::dvec(s.pos), steers: s.steers, exit: s.exit, exit_at: crate::world::dvec(s.exit_at) })
            .collect(),
        projectile: e.get::<abi::Projectile>(k::PROJECTILE),
        hitboxes: e
            .get::<abi::Hitboxes>(k::HITBOXES)
            .filter(|h| !h.parts.is_empty()),
        steer: e.callback(k::STEER),
        dismount: e.callback(k::DISMOUNT),
        ride_camera: e.or(k::RIDE_CAMERA, f32::NAN),
    }
}

/// A menu as its entry says (`abi::comp::menu`).
fn bake_menu(e: &Entry) -> MenuDef {
    use abi::comp::menu as m;
    // Groups without a start take the slots right after the previous one of their owner.
    let mut next = [0u32; 5];
    let text = |s: String| (!s.is_empty()).then_some(s);
    let groups: Vec<GroupDef> = e
        .or::<Vec<abi::SlotGroupDef>>(m::GROUPS, Vec::new())
        .into_iter()
        .map(|g| {
            let o = (g.owner as usize).min(4);
            let start = if g.start == u32::MAX { next[o] } else { g.start };
            next[o] = next[o].max(start + g.cols * g.rows);
            GroupDef {
                name: g.name,
                owner: g.owner,
                start,
                cols: g.cols.max(1),
                rows: g.rows.max(1),
                flags: g.flags,
                accepts: g.accepts,
                accepted: None,
                limit: g.limit,
                size: if g.size > 0.0 { g.size } else { 18.0 },
                background: text(g.background),
                tooltip: text(g.tooltip),
                priority: g.priority,
            }
        })
        .collect();
    MenuDef {
        name: e.name.clone(),
        temp_slots: groups
            .iter()
            .filter(|g| g.owner == weft_abi::slot_owner::MENU)
            .map(|g| g.start + g.cols * g.rows)
            .max()
            .unwrap_or(0),
        groups,
        recipes: e.or(m::RECIPES, Vec::new()),
        on_change: e.callback(m::ON_CHANGE),
        on_take: e.callback(m::ON_TAKE),
        on_tick: e.callback(m::ON_TICK),
        screen: e.callback(m::SCREEN),
        on_key: e.callback(m::ON_KEY),
        flags: e.flags(crate::tag_bits::screen::TAGS),
    }
}

/// The engine's own entity kinds.
fn engine_entities() -> Vec<RawEntry> {
    use abi::comp::entity as k;
    let group = |name: &str, rows, flags| abi::SlotGroupDef { name: name.into(), cols: 9, rows, flags, ..Default::default() };
    let groups = vec![group("hotbar", 1, weft_abi::slot_flags::HOTBAR), group("main", 3, 0)];
    let kind = |name: &str, width: f32, height: f32| {
        RawEntry::engine(ek::ENTITY, name)
            .set(k::SIZE, abi::BodySize { width, height })
            .set(k::SPEED, 4.3f32)
    };
    // No skin, no falls, not pushed around.
    let thing = |name: &str, width: f32, height: f32| {
        kind(name, width, height)
            .set(k::TEXTURE, String::new())
            .set(
                abi::comp::MODIFIERS,
                vec![abi::Modifier {
                    attribute: abi::attribute::INVULNERABLE.into(),
                    step: abi::step::SET.into(),
                    amount: 1.0,
                    ..Default::default()
                }],
            )
            .tag(k::NO_FALL_DAMAGE)
            .tag(k::UNPUSHABLE)
    };
    vec![
        kind(PLAYER, 0.6, 1.8)
            .set(k::EYE, 1.62f32)
            .set(k::SLOT_GROUPS, groups)
            .set(abi::comp::ATTACK_SPEED, 4.0f32)
            .tag(k::PICKS_UP)
            .tag(k::POSES),
        thing(ITEM, 0.25, 0.25),
        thing(FALLING, 0.98, 1.0).tag(k::SOLID),
        thing(MOVING, 1.0, 1.0)
            .tag(k::SOLID)
            .tag(k::KINEMATIC)
            .set(k::CARRY_HEIGHT, 1.0f32),
        thing(PLATFORM, 1.0, 1.0)
            .tag(k::SOLID)
            .tag(k::KINEMATIC)
            .set(k::CARRY_HEIGHT, 3.0f32),
    ]
}

/// A recipe from its entry; what else it carries stays in the entry's component table row.
fn raw_recipe(e: &Entry) -> RawRecipe {
    use abi::comp::recipe as r;
    let text = |name: &str| e.text(name).unwrap_or_default().to_string();
    RawRecipe {
        kind: text(r::KIND),
        pattern: e.or(r::PATTERN, Vec::new()),
        keys: e
            .or::<Vec<abi::RecipeKey>>(r::KEYS, Vec::new())
            .into_iter()
            .map(|k| (k.name.chars().next().unwrap_or(' '), k.item))
            .collect(),
        inputs: e.or(r::INPUTS, Vec::new()),
        input_counts: e.or(r::AMOUNTS, Vec::new()),
        output: text(r::OUTPUT),
        count: e.or(r::COUNT, 1u32).max(1),
        time: e.or(r::TIME, 0),
        outputs: e.or(r::OUTPUTS, Vec::new()),
        uses: e.or(r::USES, Vec::new()),
        entry: 0,
    }
}

/// The entries of `kind`, taken out of `all`, in load order.
/// Every mod's entries of a kind, in load order.
pub(crate) fn entries_of(raws: &mut [RawGame], kind: u32) -> Vec<RawEntry> {
    raws.iter_mut()
        .flat_map(|r| take_kind(&mut r.entries, kind))
        .collect()
}

pub(crate) fn take_kind(all: &mut Vec<RawEntry>, kind: u32) -> Vec<RawEntry> {
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(all)
        .into_iter()
        .partition(|e| e.kind == kind);
    *all = rest;
    mine
}

pub(crate) fn write_cell(cell: usize, id: u32) {
    if cell != 0 {
        unsafe { (*(cell as *const AtomicU32)).store(id, Ordering::Relaxed) }
    }
}

pub use weft_abi::EYE_SHARE as EYE;

/// An entity kind's events, by `EntityKind::effects` index.
pub mod entity_event {
    pub const HURT: usize = 0;
    pub const DEATH: usize = 1;
    pub const AMBIENT: usize = 2;
    pub const STEP: usize = 3;
    pub const NAMES: [&str; 4] = ["hurt", "death", "ambient", "step"];
}

/// Entity component with attribute modifiers.
pub const MODIFIERS: &str = "core:modifiers";
/// Timers set with `Host::entity_after`, saved with their entity.
pub const TIMERS: &str = "core:timers";
/// Engine fields clients show, kept as synced components (`Dim::mirror_fields`): health and
/// max health (`[f32; 2]`, max only for players), on fire (`u8`), worn items (`u32` each), own
/// size and look (`[f32; 3]` with width 0 for the kind's, `u32` kind with `u32::MAX` for its own),
/// and displayed public stacks (the stack wire codec).
pub const HEALTH: &str = "core:health";
pub const BURNING: &str = "core:burning";
pub const WORN: &str = "core:worn";
pub const GROUND: &str = "core:ground";
pub const SHAPE: &str = "core:shape";
pub const HELD: &str = "core:held";
pub const OFF_HELD: &str = "core:off_held";
pub const USING: &str = "core:using";
pub const SWAPS: &str = "core:swaps";
pub const LOOPING_ACTION: &str = "core:looping_action";
/// The shader effect on it: effect id, strength (`f32`), the tick it ends (`u64`).
pub const EFFECT: &str = "core:effect";
/// A pose a mod holds it in (`u32` pose id, `f32` yaw or NaN), empty: as it moves.
pub const FORCED_POSE: &str = "core:forced_pose";
/// What it rides (`u64` id) and its seat (`u32`), empty: nothing. Saved, and seen by everyone.
pub const RIDING: &str = "core:riding";
/// A projectile stuck in an entity: that entity (`u64`), where in its box (`[f32; 3]`, turned
/// with its body) and its own yaw there (`f32`); empty while it isn't. Clients hold it on the
/// bone of the model it went into.
pub const STUCK: &str = "core:stuck";
/// Entity component (stack list wire codec, synced to watchers): its kind's `SHOWN_SLOTS`.
pub const SHOWN: &str = "core:shown";
use abi::comp::{ATTRIBUTES, GAME_MODE, PERMISSION};
/// Entity component (`ItemStack`, not saved): what a player last put in its trash
/// (`slot_flags::TRASH`).
pub const TRASH: &str = "core:trash";
/// Entity component (`u64`, saved): the world time a system last simulated it to, so it can
/// catch up on what it missed (`Dim::run_systems`).
pub const SIMULATED: &str = "core:simulated";

/// The components of engine fields clients show (`HEALTH` ...), by id.
#[derive(Default, Clone, Copy)]
pub struct FieldComps {
    pub health: u32,
    pub burning: u32,
    pub worn: u32,
    pub shape: u32,
    pub held: u32,
    pub off_held: u32,
    pub using: u32,
    pub swaps: u32,
    pub looping: u32,
    pub effect: u32,
    pub forced: u32,
    pub riding: u32,
    pub stuck: u32,
    pub game_mode: u32,
    pub permission: u32,
    pub attributes: u32,
    pub shown: u32,
    pub ground: u32,
    pub trash: u32,
}

/// Entity kinds, frozen before components (which attach to them).
#[derive(Default)]
pub struct Entities {
    pub kinds: Vec<EntityKind>,
    pub names: FxHashMap<String, u32>,
    /// Each kind's own components and its owner.
    pub values: Vec<(ModId, Vec<RawValue>)>,
    /// What each kind's entities carry, by component name until those are known.
    pub attach: Vec<Vec<abi::Attach>>,
    /// (block, kind) of block entities.
    pub blocks: Vec<(String, u32)>,
    /// The player's inventory groups, and the menu its key opens.
    pub player_groups: Vec<abi::SlotGroupDef>,
    pub player_menu: Option<String>,
}

/// Every entity kind, the engine's among the mods', ids by name.
pub fn freeze_entities(raw: Vec<RawEntry>, mod_name: &dyn Fn(ModId) -> String, errors: &mut Vec<String>) -> Entities {
    use abi::comp::entity as k;
    let mut all = engine_entities();
    all.extend(raw);
    let mut out = Entities::default();
    let (made, names) = bake_by(
        all,
        ek::ENTITY,
        mod_name,
        errors,
        |e| e.name.clone(),
        |e, _| {
            if e.name == PLAYER {
                out.player_groups = e.or(k::SLOT_GROUPS, Vec::new());
                out.player_menu = e.text(k::INVENTORY_MENU).map(str::to_string);
            }
            let block = e
                .text(k::BLOCK)
                .filter(|b| !b.is_empty())
                .map(str::to_string);
            (bake_entity(e), (e.owner, e.components("entity.")), e.or(k::ATTACH, Vec::new()), block)
        },
    );
    for (i, (kind, values, attach, block)) in made.into_iter().enumerate() {
        out.kinds.push(kind);
        out.values.push(values);
        out.attach.push(attach);
        out.blocks.extend(block.map(|b| (b, i as u32)));
    }
    out.names = names;
    out
}

pub fn freeze(
    mut raws: Vec<RawGame>,
    entities: Entities,
    block_names: &FxHashMap<String, u32>,
    item_names: &FxHashMap<String, u32>,
    items: &[crate::content::ItemDef],
    dim_names: &FxHashMap<String, u32>,
    biome_names: &FxHashMap<String, u32>,
    c: &Components,
    decls: &[ComponentDecl],
    order: &dyn Fn(ModId) -> u32,
    tables: &mut crate::entries::Tables,
    conds: &mut crate::conditions::Texts,
    mod_name: &dyn Fn(ModId) -> String,
    step_order: &str,
    errors: &mut Vec<String>,
) -> Game {
    let mut g = Game::default();
    let mut world_ticks = Vec::new();
    let mut packets: Vec<(ModId, String, String, u32, Callback)> = Vec::new();
    let mut event_handlers: Vec<(ModId, String, String, Callback)> = Vec::new();
    let mut systems: Vec<(ModId, String, String, Callback)> = Vec::new();
    let mut entries: Vec<RawEntry> = raws
        .iter_mut()
        .flat_map(|r| std::mem::take(&mut r.entries))
        .collect();
    for r in raws {
        g.client_frames.extend(r.client_frames);
        packets.extend(r.packets);
        event_handlers.extend(r.event_handlers);
        systems.extend(r.systems);
        world_ticks.extend(r.world_ticks);
        g.chat_format = r.chat_format.or(g.chat_format);
    }
    g.entities = entities.kinds;
    g.entity_names = entities.names;
    g.player_kind = g.entity_names[PLAYER];
    g.item_kind = g.entity_names[ITEM];
    g.falling_kind = g.entity_names[FALLING];
    g.moving_kind = g.entity_names[MOVING];
    g.platform_kind = g.entity_names[PLATFORM];
    g.client_frames.sort_by_key(|h| order(h.0));
    let configs = take_kind(&mut entries, ek::CONFIG);
    let mut kind = |kind: u32| {
        let mut all = engine_entries(kind);
        all.extend(take_kind(&mut entries, kind));
        all
    };
    // Ids by name.
    let (particles, names) = tables.bake_by(kind(ek::PARTICLE), ek::PARTICLE, mod_name, errors, |e| e.name.clone(), |e, _| e.clone());
    g.particle_names = names;
    freeze_particles(&mut g, &particles, errors);
    let (sky, _) = tables.bake(kind(ek::SKY), ek::SKY, mod_name, errors, |e, errors| bake_sky(e, dim_names, errors));
    g.sky = (0..dim_names.len()).map(|_| Vec::new()).collect();
    for (dim, s) in sky.into_iter().flatten() {
        g.sky[dim as usize].push(s);
    }
    for list in &mut g.sky {
        list.sort_by_key(|s| s.order);
    }
    (g.huds, _) = tables.bake(kind(ek::HUD_ELEMENT), ek::HUD_ELEMENT, mod_name, errors, |e, _| {
        use abi::comp::hud_element as h;
        let (owner, cb) = e.callback(h::CALL).unwrap_or((e.owner, Callback::NONE));
        let anchor = e.text(h::ANCHOR).unwrap_or_default().to_string();
        HudElement {
            name: e.name.clone(),
            title: crate::ui::text::dotted(&e.name),
            anchor,
            order: e.or(h::ORDER, 0),
            owner,
            cb,
            rebuild: e.callback(h::REBUILD),
        }
    });
    (g.bone_motions, _) = tables.bake(kind(ek::BONE_MOTION), ek::BONE_MOTION, mod_name, errors, |e, _| {
        use abi::comp::bone_motion as b;
        let (model, bone) = e.name.rsplit_once('/').unwrap_or((&e.name, ""));
        let motion = crate::model::BoneMotion { gravity: e.or(b::GRAVITY, 0.0), spring: e.or(b::SPRING, 0.0), drag: e.or(b::DRAG, 0.0) };
        (model.to_string(), bone.to_string(), motion)
    });
    (g.screen_parts, _) = tables.bake(kind(ek::SCREEN_PART), ek::SCREEN_PART, mod_name, errors, |e, _| {
        use abi::comp::screen_part as s;
        let (owner, cb) = e.callback(s::CALL).unwrap_or((e.owner, Callback::NONE));
        let text = |name: &str| e.text(name).unwrap_or_default().to_string();
        ScreenPart { screen: text(s::SCREEN), area: text(s::AREA), order: e.or(s::ORDER, 0), owner, cb }
    });
    g.screen_parts.sort_by_key(|p| (p.order, order(p.owner)));
    let (mut recipes, _) = tables.bake(kind(ek::RECIPE), ek::RECIPE, mod_name, errors, |e, _| raw_recipe(e));
    for (i, r) in recipes.iter_mut().enumerate() {
        r.entry = i as u32;
    }
    (g.sound_categories, _) = tables.bake(kind(ek::SOUND_CATEGORY), ek::SOUND_CATEGORY, mod_name, errors, |e, _| {
        SoundCategory::new(&e.name, e.has(abi::comp::SOUND_CATEGORY_WORLD))
    });
    (g.sounds, g.sound_names) = tables.bake_by(
        kind(ek::SOUND),
        ek::SOUND,
        mod_name,
        errors,
        |e| e.name.clone(),
        |e, errors| bake_sound(e, &g.sound_categories, errors),
    );
    (g.keys, _) = tables.bake(kind(ek::KEY), ek::KEY, mod_name, errors, |e, errors| {
        let key = e.text(abi::comp::KEY_DEFAULT).unwrap_or_default();
        let slots = crate::input::Binding::parse_slots(key).unwrap_or_else(|| {
            errors.push(format!("key {}: unknown key {key:?}", e.name));
            [None; crate::input::SLOTS]
        });
        KeyDef { name: e.name.clone(), default: slots, contexts: e.flags(crate::input::context::TAGS) }
    });
    (g.attributes, g.attr_names) = tables.bake(kind(ek::ATTRIBUTE), ek::ATTRIBUTE, mod_name, errors, |e, _| {
        let [min, max] = e.or(abi::comp::attribute::RANGE, [f64::NEG_INFINITY, f64::INFINITY]);
        let default = e.or(abi::comp::attribute::DEFAULT, 0.0);
        AttrDef {
            hash: name_hash(&e.name),
            comp: c.id(&e.name),
            name: e.name.clone(),
            default,
            min: min.min(max),
            max,
            rule: e.has(abi::comp::attribute::RULE),
        }
    });
    (g.damage_causes, g.damage_cause_names) =
        tables.bake(kind(ek::DAMAGE_CAUSE), ek::DAMAGE_CAUSE, mod_name, errors, |e, _| bake_damage_cause(e));
    let (phases, _) = tables.bake(kind(ek::PHASE), ek::PHASE, mod_name, errors, |e, _| e.clone());
    let phases = crate::steps::order_phases(&phases, errors);
    let (steps, _) = tables.bake(kind(ek::STEP), ek::STEP, mod_name, errors, |e, _| e.clone());
    g.steps = crate::steps::freeze(phases, &steps, step_order, errors);
    g.steps.attributes = g
        .attributes
        .iter()
        .map(|a| g.steps.chains.get(&a.name).cloned().unwrap_or_default())
        .collect();
    // The settings, with a binding per key.
    let keys: Vec<(String, String)> = g
        .keys
        .iter()
        .map(|k| (k.name.clone(), crate::input::Binding::slots_id(&k.default)))
        .collect();
    let mut all = crate::config::engine_configs(&keys);
    all.extend(
        g.sound_categories
            .iter()
            .filter_map(|k| k.setting.as_deref())
            .map(crate::config::volume),
    );
    // Rules that come of other things: an attribute that is one, a damage cause to turn off.
    let damage_rule = |d: &DamageCauseDef| {
        (!d.has(crate::tag_bits::damage::PIERCES_INVULNERABLE) && d.name != "core:heal").then(|| {
            format!(
                "core:damage.{}",
                d.name
                    .strip_prefix("core:")
                    .unwrap_or(&d.name)
                    .replace(':', ".")
            )
        })
    };
    all.extend(
        g.attributes
            .iter()
            .filter(|a| a.rule)
            .map(|a| crate::config::rule(&a.name, abi::config_kind::FLOAT, a.default, a.min, a.max)),
    );
    all.extend(
        g.damage_causes
            .iter()
            .filter_map(damage_rule)
            .map(|name| crate::config::rule(&name, abi::config_kind::BOOL, 1.0, 0.0, 1.0)),
    );
    all.extend(configs);
    // By name: a mod's settings come together on its screen.
    (g.configs, g.config_names) = tables.bake_by(all, ek::CONFIG, mod_name, errors, |e| e.name.clone(), crate::config::bake);
    g.attr_rules = g
        .attributes
        .iter()
        .map(|a| a.rule.then(|| g.config_names[&a.name]))
        .collect();
    g.damage_rules = g
        .damage_causes
        .iter()
        .map(|d| damage_rule(d).map(|name| g.config_names[&name]))
        .collect();
    g.lives = g.config_names["core:lives"];
    g.rules_locked = g.config_names["core:rules_locked"];
    g.mob_spawning = g.config_names["core:mob_spawning"];
    g.keep_inventory = g.config_names["core:keep_inventory"];
    g.max_cramming = g.config_names["core:max_entity_cramming"];
    g.simulation_distance = g.config_names["core:simulation_distance"];
    (g.categories, _) = tables.bake(kind(ek::ENTITY_CATEGORY), ek::ENTITY_CATEGORY, mod_name, errors, |e, _| bake_category(e));
    for k in g
        .entities
        .iter_mut()
        .filter(|k| !k.category_name.is_empty())
    {
        match g.categories.iter().position(|d| d.name == k.category_name) {
            Some(i) => k.category = i as u32,
            None => errors.push(format!("entity {}: no category {}", k.name, k.category_name)),
        }
    }
    (g.cameras, g.camera_names) = tables.bake(kind(ek::CAMERA), ek::CAMERA, mod_name, errors, |e, _| bake_camera(e, conds));
    let (recipe_kinds, names) = tables.bake(kind(ek::RECIPE_KIND), ek::RECIPE_KIND, mod_name, errors, |e, _| {
        use abi::comp::recipe_kind as r;
        let layout = e.callback(r::LAYOUT);
        let def = RecipeKindDef {
            owner: layout.map_or(e.owner, |l| l.0),
            input_grid: e.or(r::INPUTS, [0; 2]),
            output_grid: e.or(r::OUTPUTS, [0; 2]),
            info: e
                .text(r::INFO)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            info_values: e.or(r::INFO_VALUES, Vec::new()),
            display: layout.map(|l| l.1),
        };
        (e.name.clone(), def)
    });
    (g.recipe_kind_defs, g.recipe_kinds) = (recipe_kinds.into_iter().collect(), names);
    // The other hand numbered last, as the player's layout has it.
    let off_last = |e: &Entry| e.name == abi::OFF_HAND_SLOT;
    let (equipment, _) = tables.bake_by(kind(ek::EQUIPMENT_SLOT), ek::EQUIPMENT_SLOT, mod_name, errors, off_last, |e, _| {
        let bones: Vec<String> = e.or(abi::comp::equipment_slot::COVERS, Vec::new());
        let background = e
            .text(abi::comp::equipment_slot::BACKGROUND)
            .unwrap_or_default()
            .to_string();
        EquipmentSlot { name: e.name.clone(), roles: crate::model::roles_of(&bones), bones, background }
    });
    (g.game_modes, g.game_mode_names) = tables.bake(kind(ek::GAME_MODE), ek::GAME_MODE, mod_name, errors, |e, _| bake_game_mode(e));
    (g.poses, g.pose_names) = tables.bake(kind(ek::POSE), ek::POSE, mod_name, errors, |e, _| bake_pose(e, conds));
    (g.precipitations, g.precipitation_names) =
        tables.bake(kind(ek::PRECIPITATION), ek::PRECIPITATION, mod_name, errors, |e, _| bake_precipitation(e));
    (g.text_places, g.text_place_names) =
        tables.bake(kind(ek::TEXT_PLACE), ek::TEXT_PLACE, mod_name, errors, |e, _| crate::hud::Place::bake(e));
    (g.entity_effects, g.entity_effect_names) =
        tables.bake(kind(ek::ENTITY_EFFECT), ek::ENTITY_EFFECT, mod_name, errors, |e, _| EntityEffectDef {
            name: e.name.clone(),
            shader: e
                .text(abi::comp::ENTITY_EFFECT_SHADER)
                .unwrap_or(&e.name)
                .to_string(),
            shader_given: e.text(abi::comp::ENTITY_EFFECT_SHADER).is_some(),
            icon: e
                .text(abi::comp::ENTITY_EFFECT_ICON)
                .unwrap_or_default()
                .to_string(),
            color: e.get::<u32>(abi::comp::ENTITY_EFFECT_COLOR),
            kind: if e.has(abi::comp::ENTITY_EFFECT_HARMFUL) {
                effect_kind::HARMFUL
            } else if e.has(abi::comp::ENTITY_EFFECT_BENEFICIAL) {
                effect_kind::BENEFICIAL
            } else {
                effect_kind::NEUTRAL
            },
            instant: e.has(abi::comp::ENTITY_EFFECT_INSTANT),
            hides: e.has(abi::comp::ENTITY_EFFECT_HIDES),
            raw_hazards: e.or(abi::comp::ENTITY_EFFECT_HAZARDS, Vec::new()),
            hazards: Vec::new(),
            on_tick: e.callback(abi::comp::ENTITY_EFFECT_ON_TICK),
            light: e.or(abi::comp::LIGHT, [0; 3]),
        });
    (g.conditions, g.condition_names) = tables.bake(kind(ek::CONDITION), ek::CONDITION, mod_name, errors, |e, _| ConditionDef {
        name: e.name.clone(),
        answer: e
            .callback(abi::comp::CONDITION_CALL)
            .map_or(Answer::Engine, |(by, cb)| Answer::Mod(by, cb)),
    });
    // Every game mode is a condition of its name too.
    for (i, m) in g.game_modes.iter().enumerate() {
        if g.condition_names.contains_key(&m.name) {
            errors.push(format!("condition {} is a game mode's name", m.name));
            continue;
        }
        g.condition_names
            .insert(m.name.clone(), g.conditions.len() as u32);
        g.conditions
            .push(ConditionDef { name: m.name.clone(), answer: Answer::Mode(crate::world::GameMode(i as u16)) });
    }
    (g.music, _) = tables.bake(kind(ek::MUSIC), ek::MUSIC, mod_name, errors, |e, errors| {
        use abi::comp::music as m;
        let dimension = e
            .text(m::DIMENSION)
            .filter(|d| !d.is_empty())
            .and_then(|d| {
                let id = dim_names.get(d).copied();
                if id.is_none() {
                    errors.push(format!("music {} plays in unknown dimension {d}", e.name));
                }
                id
            });
        let when = conds.all(&e.or::<Vec<String>>(m::WHEN, Vec::new()));
        MusicDef { name: e.name.clone(), tracks: e.or(m::TRACKS, Vec::new()), dimension, when }
    });
    (g.weathers, g.weather_names) = tables.bake(kind(ek::WEATHER), ek::WEATHER, mod_name, errors, |e, errors| {
        let mut d = bake_weather(e);
        if !d.sound_name.is_empty() {
            d.sound = g.sound_names.get(&d.sound_name).copied();
            if d.sound.is_none() {
                errors.push(format!("weather {}: no sound {}", d.name, d.sound_name));
            }
        }
        d
    });
    for i in 0..g.poses.len() {
        let names = g.poses[i].fallback_names.clone();
        g.poses[i].fallback = names
            .split_whitespace()
            .filter_map(|n| match g.pose_names.get(n) {
                Some(&p) => Some(crate::world::Pose(p as u16)),
                None => {
                    errors.push(format!("pose {}: no pose {n} to fall back to", g.poses[i].name));
                    None
                }
            })
            .collect();
    }
    // Tried highest first, the engine's before a mod's of the same priority.
    g.pose_order = (0..g.poses.len() as u16).map(crate::world::Pose).collect();
    g.pose_order
        .sort_by_key(|p| std::cmp::Reverse(g.poses[p.0 as usize].priority));
    let (spawns, _) =
        tables.bake(kind(ek::SPAWN), ek::SPAWN, mod_name, errors, |e, errors| bake_spawn(e, &g, c, conds, dim_names, biome_names, errors));
    g.spawns = spawns.into_iter().flatten().collect();
    link_handlers(&mut g, event_handlers, packets, order, errors);

    freeze_entity_storage(&mut g, c, decls, systems, entities.attach, order, errors);

    world_ticks.sort_by_key(|h: &(ModId, Callback)| order(h.0));
    g.world_ticks = world_ticks;

    (g.menus, g.menu_names) = tables.bake_by(kind(ek::MENU), ek::MENU, mod_name, errors, |e| e.name.clone(), |e, _| bake_menu(e));
    lay_out_slots(&mut g, entities.player_groups, equipment, c, errors);
    g.player_menu = entities.player_menu.and_then(|n| {
        let m = g.menu_names.get(&n).copied();
        if m.is_none() {
            errors.push(format!("player menu {n} is not registered"));
        }
        m
    });

    for (block, kind) in entities.blocks {
        match block_names.get(&block) {
            Some(&b) => {
                if g.block_entities
                    .insert(b, kind)
                    .is_some_and(|old| old != kind)
                {
                    errors.push(format!("block {block} has two block entities"));
                }
            }
            None => errors.push(format!("entity {}: the block entity of unknown block {block}", g.entities[kind as usize].name)),
        }
    }

    freeze_recipes(&mut g, recipes, items, item_names, c, conds, errors);
    g
}

/// Events with their handlers and packet channels, each in mod order.
fn link_handlers(
    g: &mut Game,
    mut event_handlers: Vec<(ModId, String, String, Callback)>,
    mut packets: Vec<(ModId, String, String, u32, Callback)>,
    order: &dyn Fn(ModId) -> u32,
    errors: &mut Vec<String>,
) {
    let mut events: Vec<EventDef> = ENGINE_EVENTS
        .iter()
        .map(|(n, s, size)| EventDef { name: n.to_string(), schema: s.to_string(), size: (*size).into(), handlers: Vec::new() })
        .collect();
    event_handlers.sort_by_key(|h| order(h.0));
    for (owner, name, schema, cb) in event_handlers {
        match events.iter_mut().find(|e| e.name == name) {
            Some(e) if e.schema != schema => errors.push(format!("event {name} is '{}' but a handler takes '{schema}'", e.schema)),
            Some(e) => e.handlers.push((owner, cb)),
            None => events.push(EventDef { name, schema, size: 0.into(), handlers: vec![(owner, cb)] }),
        }
    }
    events.sort_by(|a, b| a.name.cmp(&b.name));
    g.event_names = events
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.clone(), i as u32))
        .collect();
    g.block_tick = g.event_names[abi::event::BLOCK_TICK];
    g.events = events;
    packets.sort_by(|a, b| a.1.cmp(&b.1).then(a.3.cmp(&b.3)));
    for (owner, name, schema, side, cb) in packets {
        let id = *g.packet_names.entry(name.clone()).or_insert_with(|| {
            g.packets
                .push(PacketDef { schema: schema.clone(), ..Default::default() });
            g.packets.len() as u32 - 1
        });
        let def = &mut g.packets[id as usize];
        if def.schema != schema {
            errors.push(format!("channel {name} carries '{}' on one side and '{schema}' on the other", def.schema));
        }
        let slot = if side == 0 { &mut def.server } else { &mut def.client };
        if slot.is_some() {
            errors.push(format!("packet channel {name} has two {} handlers", if side == 0 { "server" } else { "client" }));
        }
        *slot = Some((owner, cb));
    }
}

/// Entity storage, indexed by component id like everything else: the components' definitions,
/// the ones the engine reads, who gets them synced, systems over them and what kinds attach.
fn freeze_entity_storage(
    g: &mut Game,
    c: &Components,
    decls: &[ComponentDecl],
    mut systems: Vec<(ModId, String, String, Callback)>,
    attach: Vec<Vec<abi::Attach>>,
    order: &dyn Fn(ModId) -> u32,
    errors: &mut Vec<String>,
) {
    g.components = c
        .names
        .iter()
        .map(|n| ComponentDef { name: n.clone(), ..Default::default() })
        .collect();
    for d in decls {
        if let (Some(def), Some(id)) = (&d.entity, c.id(&d.name)) {
            g.components[id as usize] = def.clone();
        }
    }
    g.component_names = g
        .components
        .iter()
        .enumerate()
        .filter(|(_, d)| d.runtime)
        .map(|(i, d)| (d.name.clone(), i as u32))
        .collect();
    g.energy = g.component_names[abi::comp::ENERGY];
    g.modifiers_comp = g.component_names[MODIFIERS];
    g.timers_comp = g.component_names[TIMERS];
    g.name_comp = g.component_names[abi::comp::NAME];
    g.persistent_comp = g.component_names[abi::comp::PERSISTENT];
    g.carried = g
        .components
        .iter()
        .enumerate()
        .filter(|(_, d)| d.runtime && d.flags & weft_abi::component_flags::CARRIED != 0)
        .map(|(i, _)| i as u32)
        .collect();
    let id = |n: &str| g.component_names[n];
    g.fields = FieldComps {
        health: id(HEALTH),
        burning: id(BURNING),
        worn: id(WORN),
        shape: id(SHAPE),
        held: id(HELD),
        off_held: id(OFF_HELD),
        using: id(USING),
        swaps: id(SWAPS),
        looping: id(LOOPING_ACTION),
        effect: id(EFFECT),
        forced: id(FORCED_POSE),
        riding: id(RIDING),
        stuck: id(STUCK),
        game_mode: id(GAME_MODE),
        permission: id(PERMISSION),
        attributes: id(ATTRIBUTES),
        shown: id(SHOWN),
        ground: id(GROUND),
        trash: id(TRASH),
    };
    g.simulated_comp = g.component_names[SIMULATED];
    let with_flag = |f: u32| {
        g.components
            .iter()
            .enumerate()
            .filter(|(_, d)| d.runtime && d.flags & f != 0)
            .map(|(i, _)| i as u32)
            .collect::<Vec<u32>>()
    };
    g.watched_comps = with_flag(abi::component_flags::SYNC_WATCHERS);
    g.menu_comps = with_flag(abi::component_flags::SYNC_MENU);
    let any_sync = abi::component_flags::SYNC_OWNER | abi::component_flags::SYNC_WATCHERS | abi::component_flags::SYNC_MENU;
    g.synced_comps = g
        .components
        .iter()
        .map(|d| d.runtime && d.flags & any_sync != 0)
        .collect();
    systems.sort_by_key(|s| order(s.0));
    for (owner, name, schema, cb) in systems {
        match g.component_names.get(&name) {
            Some(&id) if c.schemas[id as usize] == schema || schema.is_empty() => g.systems.push((owner, id, cb)),
            Some(&id) => errors.push(format!("system over {name}: it is '{}', the system takes '{schema}'", c.schemas[id as usize])),
            None => errors.push(format!("system over {name}: no entity component of that name")),
        }
    }
    for (k, list) in attach.into_iter().enumerate() {
        let kind = &mut g.entities[k];
        for mut a in list {
            let Some(&cid) = g.component_names.get(&a.name) else {
                errors.push(format!("entity {}: attaches {}, which is no entity component", kind.name, a.name));
                continue;
            };
            let size = g.components[cid as usize].size as usize;
            let own = c
                .bytes(comps::ENTITY, k as u32, cid)
                .filter(|b| b.len() == size);
            // Without a start of its own it starts as the kind has it.
            if a.start.is_empty() {
                a.start = own.map_or(Vec::new(), <[u8]>::to_vec);
            }
            if !a.start.is_empty() && a.start.len() != size {
                errors.push(format!("entity {}: attaches {} with {} bytes, the component has {size}", kind.name, a.name, a.start.len()));
                continue;
            }
            // Attached again: the last start stands.
            kind.values.retain(|v| v.0 != cid);
            kind.components.retain(|&c| c != cid);
            if !a.start.is_empty() {
                kind.values.push((cid, a.start.into_boxed_slice()));
            }
            kind.components.push(cid);
        }
    }
}

/// The player's inventory layout, the slots each kind carries and wears, and a group per
/// equipment slot in the menus that show equipment.
fn lay_out_slots(
    g: &mut Game,
    player_groups: Vec<abi::SlotGroupDef>,
    equipment: Vec<EquipmentSlot>,
    c: &Components,
    errors: &mut Vec<String>,
) {
    let groups: Vec<(String, u32, u32, u32)> = player_groups
        .into_iter()
        .map(|g| (g.name, g.cols.max(1), g.rows.max(1), g.flags))
        .collect();
    if !groups
        .iter()
        .any(|g| g.3 & weft_abi::slot_flags::HOTBAR != 0)
    {
        errors.push("player inventory: no group is the hotbar (`.hotbar()`)".into());
    }
    g.player_layout = PlayerLayout::new(&groups, equipment);
    // Their `inventory` component is the slots each one carries; kinds that wear get the
    // equipment slots after those, the player where its layout says.
    let inventory = c.id(weft_abi::comp::INVENTORY);
    for (i, k) in g.entities.iter_mut().enumerate() {
        k.slots = inventory
            .and_then(|id| c.get::<u32>(comps::ENTITY, i as u32, id))
            .unwrap_or(0)
            .min(1 << 16);
        if k.name == PLAYER {
            (k.slots, k.worn, k.off, k.hand_slots) =
                (g.player_layout.slots, g.player_layout.worn.clone(), g.player_layout.off, g.player_layout.hotbar);
        } else if k.flags & crate::tag_bits::entity::WEARS != 0 {
            // Its own slots are what its hand holds from, one if it declared none.
            k.hand_slots = k.slots.max(1);
            k.worn = k.hand_slots..k.hand_slots + g.player_layout.worn.len() as u32;
            k.off = g.player_layout.off.map(|_| k.worn.end);
            k.slots = k.worn.end + k.off.is_some() as u32;
        }
    }
    // Menus showing equipment get a group per slot: what the entity the menu is on wears, or the
    // player (`menu::slots`). The other hand is the slot after the equipment, theirs too.
    let layout = g.player_layout.clone();
    for m in &mut g.menus {
        let Some(at) = m
            .groups
            .iter()
            .position(|g| g.owner == weft_abi::slot_owner::EQUIPMENT)
        else {
            continue;
        };
        m.groups.remove(at);
        for (i, e) in layout.equipment.iter().enumerate().rev() {
            let off = e.name == weft_abi::OFF_HAND_SLOT;
            // The other hand shows with the equipment but is numbered last, as in vanilla.
            let at = if off { m.groups.len() } else { at };
            m.groups.insert(
                at,
                GroupDef {
                    name: e.name.clone(),
                    owner: weft_abi::slot_owner::WORN,
                    start: i as u32,
                    cols: 1,
                    rows: 1,
                    flags: if off { weft_abi::slot_flags::OFF_HAND } else { 0 },
                    accepts: if off { String::new() } else { format!("#worn/{}", e.name) },
                    accepted: None,
                    limit: if off { 0 } else { 1 },
                    size: 18.0,
                    background: (!e.background.is_empty()).then(|| e.background.clone()),
                    tooltip: None,
                    priority: 1,
                },
            );
        }
    }
}

/// Recipes with their variables expanded and their ingredients and outputs resolved, by kind.
fn freeze_recipes(
    g: &mut Game,
    recipes: Vec<RawRecipe>,
    items: &[crate::content::ItemDef],
    item_names: &FxHashMap<String, u32>,
    c: &Components,
    conds: &mut crate::conditions::Texts,
    errors: &mut Vec<String>,
) {
    // The tags items carry, what `{m}` in a recipe goes through.
    let tag_names: FxHashMap<&str, u32> = (1..items.len() as u32)
        .flat_map(|i| c.of(comps::ITEM, i))
        .filter(|&t| c.is_tag(t))
        .map(|t| (c.names[t as usize].as_str(), t))
        .collect();
    let mut expanded = Vec::new();
    for r in recipes {
        match expand_variables(r, &tag_names) {
            Ok(list) => expanded.extend(list),
            Err(e) => errors.push(e),
        }
    }
    let ing = |s: &str, errors: &mut Vec<String>| -> Ingredient {
        if s.is_empty() {
            return Ingredient::Empty;
        }
        let r = match crate::refs::Ref::parse(s) {
            Ok(r) => r,
            Err(e) => {
                errors.push(format!("recipe: {e}"));
                return Ingredient::Empty;
            }
        };
        // One tag stays a tag, read when matching; anything wider is the items it stands for.
        let found = match &r {
            crate::refs::Ref::Tags(p) if !p.contains('*') && !s.trim().contains(' ') => {
                Ok(c.id(p).map_or(Ingredient::Any(Vec::new()), Ingredient::Tag))
            }
            crate::refs::Ref::Name(..) if !s.trim().contains(' ') => crate::refs::one(s, item_names, "item").map(Ingredient::Item),
            _ => crate::refs::set(s, comps::ITEM, item_names, items.len() as u32, c, "item").map(Ingredient::Any),
        };
        found.unwrap_or_else(|e| {
            errors.push(format!("recipe: {e}"));
            Ingredient::Empty
        })
    };
    // A result by tag (`#planks/{m}`, logs into planks of the same material) is the first item
    // with it in mod load order; anything else names its item.
    let one = |s: &str| match crate::refs::Ref::parse(s)? {
        crate::refs::Ref::Tags(_) => crate::refs::set(s, comps::ITEM, item_names, items.len() as u32, c, "item")?
            .into_iter()
            .min_by_key(|&i| items[i as usize].order)
            .ok_or_else(|| format!("no item is {}", s.trim())),
        _ => crate::refs::one(s, item_names, "item"),
    };
    let recipes = expanded;
    for r in recipes {
        if !g.recipe_kinds.contains_key(&r.kind) {
            errors.push(format!("recipe for {}: no recipe kind {}", r.output, r.kind));
            continue;
        }
        let output = match one(&r.output) {
            Ok(o) => o,
            Err(e) => {
                errors.push(format!("recipe for {}: {e}", r.output));
                continue;
            }
        };
        let shaped = if r.pattern.is_empty() {
            None
        } else {
            let h = r.pattern.len() as u32;
            let w = r
                .pattern
                .iter()
                .map(|p| p.chars().count())
                .max()
                .unwrap_or(0) as u32;
            let mut cells = Vec::new();
            for row in &r.pattern {
                let chars: Vec<char> = row.chars().collect();
                for x in 0..w as usize {
                    let ch = chars.get(x).copied().unwrap_or(' ');
                    let cell = if ch == ' ' {
                        Ingredient::Empty
                    } else {
                        match r.keys.iter().find(|k| k.0 == ch) {
                            Some((_, item)) => ing(item, errors),
                            None => {
                                errors.push(format!("recipe for {}: key '{ch}' not defined", r.output));
                                Ingredient::Empty
                            }
                        }
                    };
                    cells.push(cell);
                }
            }
            Some((w, h, cells))
        };
        let inputs: Vec<Ingredient> = r.inputs.iter().map(|s| ing(s, errors)).collect();
        // What runs do to each ingredient, by its pattern key or its place among the inputs.
        let keys: Vec<String> = match &shaped {
            Some((w, _, cells)) => (0..cells.len())
                .map(|i| {
                    let row = &r.pattern[i / *w as usize];
                    row.chars().nth(i % *w as usize).unwrap_or(' ').to_string()
                })
                .collect(),
            None => (0..inputs.len()).map(|i| i.to_string()).collect(),
        };
        let uses: Vec<Use> = keys
            .iter()
            .map(|k| match r.uses.iter().rfind(|u| u.key == *k) {
                None => Use::Consume,
                Some(u) => match u.how {
                    abi::recipe_use::KEEP => Use::Keep,
                    abi::recipe_use::WEAR => Use::Wear(u.amount.max(1)),
                    abi::recipe_use::LEAVES => match one(&u.item) {
                        Ok(i) => Use::Leaves(i),
                        Err(e) => {
                            errors.push(format!("recipe for {}: leaves {e}", r.output));
                            Use::Consume
                        }
                    },
                    _ => Use::Consume,
                },
            })
            .collect();
        for u in r.uses.iter().filter(|u| !keys.contains(&u.key)) {
            errors.push(format!("recipe for {}: no ingredient {} to say how it is used", r.output, u.key));
        }
        let input_counts = (0..inputs.len())
            .map(|i| r.input_counts.get(i).copied().unwrap_or(1).max(1))
            .collect();
        // A tag stands for its first item here as in the main output.
        let mut pools = r.outputs;
        for e in pools.iter_mut().flat_map(|p| &mut p.entries) {
            if e.item.starts_with('#') {
                match one(&e.item) {
                    Ok(i) => e.item = items[i as usize].name.clone(),
                    Err(err) => errors.push(format!("recipe for {}: output: {err}", r.output)),
                }
            }
        }
        let extras = crate::loot::Resolve { items: item_names, conds: &mut *conds }.pools(&pools, errors, &r.output);
        let mut outputs = vec![RecipeOutput { what: Ingredient::Item(output), count: r.count, chance: 1.0 }];
        for p in &extras.pools {
            let total = p.entries.iter().map(|e| e.weight).sum::<u32>().max(1) as f32;
            let rolls = (p.rolls.0 + p.rolls.1) as f32 / 2.0;
            outputs.extend(
                p.entries
                    .iter()
                    .filter(|e| e.item != 0)
                    .map(|e| RecipeOutput {
                        what: Ingredient::Item(e.item),
                        count: e.max,
                        chance: (e.weight as f32 / total * rolls).min(1.0),
                    }),
            );
        }
        g.recipes.entry(r.kind).or_default().push(Recipe {
            shaped,
            inputs,
            uses,
            input_counts,
            outputs,
            extras,
            time: r.time,
            entry: r.entry,
        });
    }
}

impl Game {
    /// Entities of `kind` belong to a block (machines, chests): they come and go with it.
    pub fn is_block_entity(&self, kind: u32) -> bool {
        self.block_entities.values().any(|&k| k == kind)
    }

    pub fn component(&self, name: &str) -> Option<u32> {
        self.component_names.get(name).copied()
    }

    /// Bytes read back for a component, cut or filled to its size as it is now (0: variable, like
    /// attribute modifiers).
    pub fn fit_component(&self, id: u32, mut bytes: Vec<u8>) -> Box<[u8]> {
        let size = self.components[id as usize].size as usize;
        if size > 0 {
            bytes.resize(size, 0);
        }
        bytes.into_boxed_slice()
    }

    /// Finds the recipe whose pattern fits the grid (any position, mirrored allowed) or whose
    /// shapeless inputs match the grid contents exactly.
    pub fn match_grid(&self, kind: &str, grid: &[u32], w: u32, h: u32, tags: &Components) -> Option<&Recipe> {
        self.match_cells(kind, grid, w, h, tags).map(|m| m.0)
    }

    /// `match_grid`, and per grid slot the recipe's ingredient there (its cell of a shaped one,
    /// its input of another), what says how the slot is used up.
    pub fn match_cells(&self, kind: &str, grid: &[u32], w: u32, h: u32, tags: &Components) -> Option<(&Recipe, Vec<Option<usize>>)> {
        let list = self.recipes.get(kind)?;
        // Trim the grid to the bounding box of its items.
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for y in 0..h {
            for x in 0..w {
                if grid[(y * w + x) as usize] != 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        if x1 == 0 {
            return None;
        }
        let (bw, bh) = (x1 - x0, y1 - y0);
        let at = |x: u32, y: u32| grid[((y0 + y) * w + x0 + x) as usize];
        list.iter().find_map(|r| {
            let mut cells = vec![None; grid.len()];
            match &r.shaped {
                Some((rw, rh, pattern)) => {
                    if *rw != bw || *rh != bh {
                        return None;
                    }
                    let fits = |mirror: bool| {
                        (0..bh).all(|y| {
                            (0..bw).all(|x| {
                                let cx = if mirror { bw - 1 - x } else { x };
                                pattern[(y * rw + cx) as usize].matches(at(x, y), tags)
                            })
                        })
                    };
                    let mirror = if fits(false) {
                        false
                    } else if fits(true) {
                        true
                    } else {
                        return None;
                    };
                    for y in 0..bh {
                        for x in 0..bw {
                            let cx = if mirror { bw - 1 - x } else { x };
                            cells[((y0 + y) * w + x0 + x) as usize] = (at(x, y) != 0).then_some((y * rw + cx) as usize);
                        }
                    }
                }
                None => {
                    let filled: Vec<usize> = (0..grid.len()).filter(|&i| grid[i] != 0).collect();
                    if r.inputs.len() != filled.len() {
                        return None;
                    }
                    for (k, ing) in r.inputs.iter().enumerate() {
                        let i = filled
                            .iter()
                            .copied()
                            .find(|&i| cells[i].is_none() && ing.matches(grid[i], tags))?;
                        cells[i] = Some(k);
                    }
                }
            }
            Some((r, cells))
        })
    }

    pub fn process(&self, kind: &str, input: u32, tags: &Components) -> Option<&Recipe> {
        self.recipes
            .get(kind)?
            .iter()
            .find(|r| r.inputs.first().is_some_and(|i| i.matches(input, tags)))
    }

    /// For each input of `r`, the stack in `stacks` that holds enough of it (each stack used
    /// once); `None` if one is missing. Fluids are not in stacks and never match.
    pub fn find_inputs(&self, r: &Recipe, stacks: &[weft_abi::ItemStack], tags: &Components) -> Option<Vec<usize>> {
        let mut used = vec![false; stacks.len()];
        let mut out = Vec::with_capacity(r.inputs.len());
        for (ing, &n) in r.inputs.iter().zip(&r.input_counts) {
            if matches!(ing, Ingredient::Empty) {
                continue;
            }
            let i = stacks
                .iter()
                .enumerate()
                .position(|(i, s)| !used[i] && !s.is_empty() && s.count >= n && ing.matches(s.item, tags))?;
            used[i] = true;
            out.push(i);
        }
        Some(out)
    }
}

/// Sounds the engine plays itself (files in its own assets, packs may replace them).
fn engine_sounds() -> Vec<RawEntry> {
    use abi::comp::sound as s;
    let one = |name: &str, file: &str, category: &str, volume: f32, pitch: [f32; 2]| {
        RawEntry::engine(abi::entry_kind::SOUND, name)
            .set(s::FILES, vec![file.to_string()])
            .set(s::CATEGORY, category.to_string())
            .set(s::VOLUME, volume)
            .set(s::PITCH, pitch)
    };
    vec![
        one("core:ui.click", "core:ui/click", "core:ui", 0.6, [1.0, 1.0]),
        one("core:item.pickup", "core:item/pickup", "core:players", 0.4, [1.4, 2.2]),
    ]
}

/// A sound from its entry, its category's index among `categories`.
fn bake_sound(e: &Entry, categories: &[SoundCategory], errors: &mut Vec<String>) -> SoundDef {
    use abi::comp::sound as s;
    let category = e.text(s::CATEGORY).unwrap_or("core:blocks");
    let pitch: [f32; 2] = e.or(s::PITCH, [1.0, 1.0]);
    SoundDef {
        name: e.name.clone(),
        files: e.or(s::FILES, Vec::new()),
        category: categories
            .iter()
            .position(|k| k.name == category)
            .unwrap_or_else(|| {
                errors.push(format!("sound {}: no category {category}", e.name));
                0
            }) as u32,
        volume: e.or(s::VOLUME, 1.0),
        pitch: [pitch[0].min(pitch[1]).max(0.01), pitch[0].max(pitch[1]).max(0.01)],
        range: e.or(s::RANGE, 16.0f32).max(1.0),
        full_within: e.or(s::FULL_WITHIN, 1.0f32).max(0.0),
        falloff: e.or(s::FALLOFF, 0i32).clamp(-8, 8),
    }
}

/// Kinds the engine spawns itself; mods change or replace their look like any other.
fn engine_particles() -> Vec<RawEntry> {
    use crate::tag_bits::particle as pf;
    use abi::comp::particle as p;
    use abi::{particle as k, particle_event as pe, particle_then as pt, particle_when as pw};
    let kind = |name: &str, textures: Vec<String>, [life, size]: [[f32; 2]; 2], [gravity, drag]: [f32; 2], color: [u32; 2], flags: u32| {
        let e = RawEntry::engine(ek::PARTICLE, name)
            .set(p::TEXTURES, textures)
            .set(p::LIFE, life)
            .set(p::SIZE, size)
            .set(p::GRAVITY, gravity)
            .set(p::DRAG, drag)
            .set(p::COLOR, color);
        crate::tag_bits::particle::TAGS
            .iter()
            .filter(|f| flags & f.1 != 0)
            .fold(e, |e, f| e.tag(f.0))
    };
    let rule = |event, then, into: &str| {
        vec![abi::ParticleRule { event, when: pw::ANY, tag: String::new(), then, amount: 0.0, into: into.into() }]
    };
    let generic: Vec<String> = (0..8)
        .map(|i| format!("core:particle/generic_{}", 7 - i))
        .collect();
    let one = |t: &str| vec![format!("core:particle/{t}")];
    let white = [0xFFFFFFFF; 2];
    vec![
        kind(k::BLOCK, Vec::new(), [[0.2, 1.0], [0.05, 0.1]], [16.0, 0.98], [0x999999FF; 2], pf::COLLIDE),
        kind(k::POOF, generic.clone(), [[0.4, 1.1], [0.15, 0.3]], [-2.0, 0.9], [0xD0D0D0FF, 0x909090FF], 0),
        kind(k::CRIT, generic.clone(), [[0.3, 0.7], [0.1, 0.2]], [8.0, 0.8], [0xFFE9A0FF, 0xC08030FF], pf::FULLBRIGHT),
        kind(k::FLAME, one("flame"), [[0.3, 0.8], [0.08, 0.14]], [0.0, 0.96], white, pf::FULLBRIGHT | pf::SHRINK),
        kind(k::SMOKE, generic, [[0.6, 1.4], [0.08, 0.16]], [-1.5, 0.96], [0x505050FF, 0x303030FF], 0),
        // Pops at the surface, as vanilla's does.
        kind(k::BUBBLE, one("bubble"), [[0.5, 1.5], [0.05, 0.1]], [-4.0, 0.85], white, 0)
            .set(p::RULES, rule(pe::LEAVE_LIQUID, pt::DIE, "")),
        kind(k::SPLASH, one("splash"), [[0.2, 0.5], [0.08, 0.14]], [16.0, 0.98], white, pf::COLLIDE),
        // Rising swirls around who has an effect, tinted by the effect's color.
        kind(
            k::EFFECT,
            (0..8)
                .map(|i| format!("core:particle/effect_{}", 7 - i))
                .collect(),
            [[0.8, 1.4], [0.08, 0.12]],
            [-0.6, 0.96],
            white,
            0,
        ),
        // Splashes where it lands, as vanilla's falling drip turns into a landing one.
        kind(k::DRIP, one("drip"), [[0.8, 1.2], [0.04, 0.06]], [16.0, 0.98], white, 0)
            .set(p::RULES, rule(pe::LAND, pt::BECOME_STILL, k::SPLASH)),
    ]
}

/// Particle kinds from their entries, in id order (`Game::particle_names` set).
fn freeze_particles(g: &mut Game, entries: &[Entry], errors: &mut Vec<String>) {
    use abi::comp::particle as p;
    use abi::{particle_event as pe, particle_then as pt, particle_when as pw};
    let mut tex_ids: FxHashMap<String, u32> = FxHashMap::default();
    for e in entries {
        let name = &e.name;
        let frames = e
            .or::<Vec<String>>(p::TEXTURES, Vec::new())
            .into_iter()
            .map(|t| {
                *tex_ids.entry(t.clone()).or_insert_with(|| {
                    g.particle_textures.push(t);
                    g.particle_textures.len() as u32 - 1
                })
            })
            .collect();
        let mut rules = Vec::new();
        for r in e.or::<Vec<abi::ParticleRule>>(p::RULES, Vec::new()) {
            let into = match r.then {
                pt::BECOME | pt::BECOME_STILL => match g.particle_names.get(&r.into) {
                    Some(&id) => id,
                    None => {
                        errors.push(format!("particle {name} turns into {}, which no one registers", r.into));
                        continue;
                    }
                },
                _ => 0,
            };
            let tag = if r.when == pw::TAG {
                match g.particle_tags.iter().position(|t| *t == r.tag) {
                    Some(i) => i as u32,
                    None if g.particle_tags.len() < PARTICLE_TAGS => {
                        g.particle_tags.push(r.tag.clone());
                        g.particle_tags.len() as u32 - 1
                    }
                    None => {
                        errors.push(format!(
                            "particle {name} asks about tag {}: particle rules ask about {} tags at most ({})",
                            r.tag,
                            PARTICLE_TAGS,
                            g.particle_tags.join(", ")
                        ));
                        continue;
                    }
                }
            } else {
                0
            };
            if r.event > pe::EXPIRE || r.when > pw::TAG || r.then > pt::BECOME_STILL {
                errors.push(format!("particle {name}: a rule with an event, place or action that doesn't exist"));
                continue;
            }
            rules.push(ParticleRule { event: r.event, when: r.when, tag, then: r.then, amount: r.amount, into });
        }
        let hits = rules
            .iter()
            .any(|r| matches!(r.event, pe::LAND | pe::WALL | pe::CEILING));
        let flags = e.flags(crate::tag_bits::particle::TAGS) | if hits { crate::tag_bits::particle::COLLIDE } else { 0 };
        let [life_min, life_max] = e.or(p::LIFE, [0.5f32, 1.0]);
        let [size_min, size_max] = e.or(p::SIZE, [0.1f32, 0.2]);
        let [color, color_end] = e.or(p::COLOR, [0xFFFFFFFFu32; 2]);
        g.particles.push(ParticleDef {
            name: name.clone(),
            frames,
            life_min: life_min.min(life_max).max(0.01),
            life_max: life_max.max(life_min).max(0.01),
            size_min: size_min.min(size_max),
            size_max: size_max.max(size_min),
            gravity: e.or(p::GRAVITY, 0.0),
            drag: e.or(p::DRAG, 0.98f32).clamp(0.0, 1.0),
            color,
            color_end,
            flags,
            shader: e.text(p::SHADER).unwrap_or_default().to_string(),
            rules,
            longest: 0.0,
        });
    }
    let translucent = |k: &ParticleDef| k.flags & crate::tag_bits::particle::TRANSLUCENT != 0;
    for k in &g.particles {
        for r in k
            .rules
            .iter()
            .filter(|r| matches!(r.then, pt::BECOME | pt::BECOME_STILL))
        {
            let into = &g.particles[r.into as usize];
            if translucent(k) != translucent(into) {
                errors.push(format!("particle {} turns into {}, which is drawn the other way (translucent or not)", k.name, into.name));
            }
        }
    }
    // Turning into each other in a loop lives on; a chain is counted this many kinds deep.
    fn longest(all: &[ParticleDef], k: usize, depth: u32) -> f32 {
        let next = all[k]
            .rules
            .iter()
            .filter(|r| depth > 0 && matches!(r.then, pt::BECOME | pt::BECOME_STILL))
            .map(|r| longest(all, r.into as usize, depth - 1))
            .fold(0.0, f32::max);
        all[k].life_max + next
    }
    let lives: Vec<f32> = (0..g.particles.len())
        .map(|k| longest(&g.particles, k, 8))
        .collect();
    for (k, l) in g.particles.iter_mut().zip(lives) {
        k.longest = l;
    }
}

/// A recipe naming tags with a variable, `#ores/{m}` -> `#ingots/{m}`, stands for one recipe per
/// value: the first tag with the variable gives the values, and the copies whose other tags no
/// item carries are left out. So every mod's ores smelt into the matching ingots. (`*` is "any",
/// not a variable.) A variable with no value at all is an error.
fn expand_variables(r: RawRecipe, tags: &FxHashMap<&str, u32>) -> Result<Vec<RawRecipe>, String> {
    let names: Vec<&String> = r
        .keys
        .iter()
        .map(|k| &k.1)
        .chain(&r.inputs)
        .chain(std::iter::once(&r.output))
        .chain(r.outputs.iter().flat_map(|p| &p.entries).map(|e| &e.item))
        .collect();
    let Some(pattern) = names
        .iter()
        .find_map(|n| n.strip_prefix('#').filter(|t| t.contains('{')))
    else {
        return Ok(vec![r]);
    };
    let (pre, rest) = pattern.split_once('{').unwrap();
    let Some((var, post)) = rest.split_once('}') else { return Err(format!("recipe for {}: #{pattern} has no closing }}", r.output)) };
    let var = format!("{{{var}}}");
    let mut values: Vec<&str> = tags
        .keys()
        .filter_map(|t| t.strip_prefix(pre).and_then(|rest| rest.strip_suffix(post)))
        .filter(|v| !v.is_empty())
        .collect();
    values.sort_unstable();
    if values.is_empty() {
        return Err(format!("recipe for {}: no item has a tag like #{pattern}", r.output));
    }
    let mut out = Vec::new();
    for v in values {
        let sub = |s: &String| s.replace(&var, v);
        let usable = names.iter().all(|n| {
            n.strip_prefix('#')
                .is_none_or(|t| tags.contains_key(t.replace(&var, v).as_str()) || t.contains('*'))
        });
        if usable {
            out.push(RawRecipe {
                kind: r.kind.clone(),
                pattern: r.pattern.clone(),
                keys: r.keys.iter().map(|(c, n)| (*c, sub(n))).collect(),
                inputs: r.inputs.iter().map(sub).collect(),
                input_counts: r.input_counts.clone(),
                output: sub(&r.output),
                count: r.count,
                time: r.time,
                outputs: r
                    .outputs
                    .iter()
                    .map(|p| abi::LootPool {
                        entries: p
                            .entries
                            .iter()
                            .map(|e| abi::LootEntry { item: sub(&e.item), ..e.clone() })
                            .collect(),
                        ..p.clone()
                    })
                    .collect(),
                uses: r
                    .uses
                    .iter()
                    .map(|u| abi::RecipeUse { item: sub(&u.item), ..u.clone() })
                    .collect(),
                entry: r.entry,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod box_unwrap {
    use super::Cuboid;

    fn arm(mirror: bool) -> [[f32; 4]; 6] {
        Cuboid { origin: [0.0; 3], size: [4.0, 12.0, 4.0], uv: [40.0, 16.0], inflate: 0.0, mirror, faces: None }.face_rects()
    }

    #[test]
    fn wraps_around_the_box() {
        let r = arm(false);
        // Each side's edge meets the same edge of the strip beside it in the unwrap.
        assert_eq!(r[1], [40.0, 20.0, 4.0, 12.0], "+X, left of the front");
        assert_eq!(r[4], [44.0, 20.0, 4.0, 12.0], "-Z, the front");
        assert_eq!(r[0], [48.0, 20.0, 4.0, 12.0], "-X, right of the front");
        assert_eq!(r[5], [52.0, 20.0, 4.0, 12.0], "+Z, the back");
        assert_eq!(r[3], [48.0, 20.0, -4.0, -4.0], "the top, meeting the front");
    }

    #[test]
    fn mirrored_is_the_other_side_read_backwards() {
        let r = arm(true);
        assert_eq!(r[0], [44.0, 20.0, -4.0, 12.0], "-X has what +X had");
        assert_eq!(r[4], [48.0, 20.0, -4.0, 12.0], "the front flipped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaped_slots_follow_ingredients() {
        let mut tags = crate::components::Builder::default();
        tags.start(&[]);
        let tags = tags.finish();
        for mask in 1u32..512 {
            let filled: Vec<_> = (0..9).filter(|i| mask & (1 << i) != 0).collect();
            let x0 = filled.iter().map(|i| i % 3).min().unwrap();
            let y0 = filled.iter().map(|i| i / 3).min().unwrap();
            let w = filled.iter().map(|i| i % 3).max().unwrap() + 1 - x0;
            let h = filled.iter().map(|i| i / 3).max().unwrap() + 1 - y0;
            let pattern: Vec<_> = (0..h)
                .flat_map(|y| (0..w).map(move |x| (y + y0) * 3 + x + x0))
                .map(|i| if mask & (1 << i) == 0 { Ingredient::Empty } else { Ingredient::Item(i + 1) })
                .collect();
            let mut game = Game::default();
            game.recipes.insert(
                "crafting".into(),
                vec![Recipe {
                    shaped: Some((w, h, pattern.clone())),
                    inputs: Vec::new(),
                    uses: vec![Use::Consume; pattern.len()],
                    input_counts: Vec::new(),
                    outputs: Vec::new(),
                    extras: Default::default(),
                    time: 0,
                    entry: 0,
                }],
            );
            for mirror in [false, true] {
                for top in 0..=3 - h {
                    for left in 0..=3 - w {
                        let mut grid = [0; 9];
                        let mut expected = vec![None; 9];
                        for y in 0..h {
                            for x in 0..w {
                                let k = (y * w + if mirror { w - 1 - x } else { x }) as usize;
                                if let Ingredient::Item(item) = pattern[k] {
                                    let slot = ((top + y) * 3 + left + x) as usize;
                                    grid[slot] = item;
                                    expected[slot] = Some(k);
                                }
                            }
                        }
                        let (_, cells) = game.match_cells("crafting", &grid, 3, 3, &tags).unwrap();
                        assert_eq!(cells, expected, "mask {mask}, mirror {mirror}, at {left},{top}");
                        let slot = grid.iter().position(|&item| item != 0).unwrap();
                        grid[slot] = 100;
                        assert!(game.match_cells("crafting", &grid, 3, 3, &tags).is_none(), "wrong ingredient in slot {slot}");
                    }
                }
            }
        }
    }

    #[test]
    fn variable_recipes_expand_per_material() {
        let tags: FxHashMap<&str, u32> = [("ores/iron", 1), ("ingots/iron", 2), ("ores/tin", 3), ("ingots/tin", 4), ("ores/ruby", 5)]
            .into_iter()
            .collect();
        let r = RawRecipe {
            kind: "smelting".into(),
            inputs: vec!["#ores/{m}".into()],
            output: "#ingots/{m}".into(),
            count: 1,
            time: 200,
            ..Default::default()
        };
        let out: Vec<(String, String)> = expand_variables(r, &tags)
            .unwrap()
            .into_iter()
            .map(|r| (r.inputs[0].clone(), r.output))
            .collect();
        // Ruby has no ingot, so it is left out.
        assert_eq!(out, vec![("#ores/iron".into(), "#ingots/iron".into()), ("#ores/tin".into(), "#ingots/tin".into())]);
    }
}

#[cfg(test)]
mod sky_tests {
    use super::*;

    /// Named by dimension and object, defaults where nothing is set.
    #[test]
    fn baked_from_entries() {
        use abi::comp::sky as s;
        let dims: FxHashMap<String, u32> = [("a:d/x".to_string(), 0), ("a:e".to_string(), 1)]
            .into_iter()
            .collect();
        let e = |name: &str| crate::entries::Entry { name: name.into(), ..Default::default() };
        let mut moon = e("a:e/a:moons/1");
        let value = |name: &str, v: f32| RawValue { name: name.into(), schema: "f32".into(), bytes: v.to_le_bytes().to_vec(), text: None };
        moon.values = vec![value(s::SIZE, 40.0), value(s::PHASE, 0.5), RawValue { text: Some("a:x".into()), ..value(s::TEXTURE, 0.0) }];
        let mut errors = Vec::new();
        let (dim, m) = bake_sky(&moon, &dims, &mut errors).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!((dim, m.name.as_str(), m.size, m.phase), (1, "a:e/a:moons/1", 40f32.to_radians(), 0.5));
        assert!(m.orbit.is_infinite(), "nobody set an orbit, so it follows the day");
        assert!(bake_sky(&e("a:d/x/stars"), &dims, &mut errors).is_some_and(|(d, s)| d == 0 && s.name == "a:d/x/stars"));
        assert!(bake_sky(&e("a:f/sun"), &dims, &mut errors).is_none());
    }
}

/// A camera mode as frozen; `call` none for the engine's own, which it moves itself.
#[derive(Clone)]
pub struct CameraMode {
    pub name: String,
    pub flags: u32,
    pub key: Option<String>,
    pub call: Option<(ModId, Callback)>,
    pub transition: f32,
    pub easing: u32,
    /// A condition (`Content::conds`) the server checks for a player turning it on, with
    /// `action::CAMERA`; `None`: the client turns it on by itself.
    pub when: Option<u32>,
}

impl CameraMode {
    pub fn has(&self, f: u32) -> bool {
        self.flags & f != 0
    }
}

/// The engine's camera modes, in id order.
pub mod camera {
    pub const FIRST_PERSON: u32 = 0;
    pub const BEHIND: u32 = 1;
    pub const FRONT: u32 = 2;
    pub const FREE: u32 = 3;
}

fn engine_cameras() -> Vec<RawEntry> {
    use crate::tag_bits::camera as cf;
    let mode = |name: &str, flags: u32| {
        crate::tag_bits::camera::TAGS
            .iter()
            .filter(|f| flags & f.1 != 0)
            .fold(RawEntry::engine(abi::entry_kind::CAMERA, name), |e, f| e.tag(f.0))
    };
    vec![
        mode("core:first_person", cf::HAND | cf::HUD | cf::EFFECTS | cf::CYCLE),
        mode("core:behind", cf::BODY | cf::HUD | cf::BOB | cf::CLIP | cf::CYCLE),
        mode("core:front", cf::BODY | cf::HUD | cf::BOB | cf::CLIP | cf::CYCLE),
        mode("core:free", cf::BODY | cf::SOLID | cf::DETACHED | cf::NO_INTERACT)
            .set(abi::comp::camera::WHEN, vec!["attribute(core:can_fly, 1..)".to_string()])
            .set(abi::comp::camera::KEY, "core:freecam".to_string()),
    ]
}

fn bake_camera(e: &Entry, conds: &mut crate::conditions::Texts) -> CameraMode {
    use abi::comp::camera as c;
    CameraMode {
        name: e.name.clone(),
        flags: e.flags(crate::tag_bits::camera::TAGS),
        key: e.text(c::KEY).filter(|k| !k.is_empty()).map(str::to_string),
        call: e.callback(c::CALL),
        transition: e
            .get::<f32>(c::TRANSITION)
            .filter(|t| t.is_finite())
            .map_or(0.0, |t| t.max(0.0)),
        easing: e.or(c::EASING, abi::easing::IN_OUT),
        when: conds.all(&e.or::<Vec<String>>(c::WHEN, Vec::new())),
    }
}
