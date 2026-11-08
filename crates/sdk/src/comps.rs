//! The components registrations of each kind are made of, by kind: `comps::weather::DARKNESS`
//! is a weather's. The builders' methods set them; `r.edit(&X).set(&comps::..)` changes one of
//! another mod's, and the engine reads them when the game loads (absent: its default).

/// What a weather is made of: rain's look and flags where nothing is set.
pub mod weather {
    use crate::abi::comp::weather as w;
    use crate::kind::Weather;
    use crate::{Component, Tag, Text};
    pub static DRY: Tag<Weather> = Tag::named(w::DRY);
    pub static LIGHTNING: Tag<Weather> = Tag::named(w::LIGHTNING);
    pub static UNSHELTERED: Tag<Weather> = Tag::named(w::UNSHELTERED);
    pub static FALLS: Component<f32, Weather> = Component::named(w::FALLS);
    pub static SKY_GRAY: Component<f32, Weather> = Component::named(w::SKY_GRAY);
    pub static SKY_COVER: Component<f32, Weather> = Component::named(w::SKY_COVER);
    pub static FOG_GRAY: Component<f32, Weather> = Component::named(w::FOG_GRAY);
    pub static CLOUD_GRAY: Component<f32, Weather> = Component::named(w::CLOUD_GRAY);
    pub static CLOUD_DARK: Component<f32, Weather> = Component::named(w::CLOUD_DARK);
    pub static DARKNESS: Component<f32, Weather> = Component::named(w::DARKNESS);
    pub static SOUND: Component<Text, Weather> = Component::named(w::SOUND);
    pub static DURATION: Component<[u32; 2], Weather> = Component::named(w::DURATION);
    pub static WEIGHT: Component<u32, Weather> = Component::named(w::WEIGHT);
}

/// What a precipitation is made of.
pub mod precipitation {
    use crate::abi::comp::precipitation as p;
    use crate::kind::Precipitation;
    use crate::{Component, Text};
    pub static TEXTURE: Component<Text, Precipitation> = Component::named(p::TEXTURE);
    pub static SPEED: Component<f32, Precipitation> = Component::named(p::SPEED);
    pub static DENSITY: Component<f32, Precipitation> = Component::named(p::DENSITY);
}

/// What a pose is made of (shares of standing unless said).
pub mod pose {
    use crate::abi::comp::pose as p;
    use crate::kind::Pose;
    use crate::{Component, Text};
    pub static HEIGHT: Component<f32, Pose> = Component::named(p::HEIGHT);
    pub static EYE: Component<f32, Pose> = Component::named(p::EYE);
    pub static SPEED: Component<f32, Pose> = Component::named(p::SPEED);
    pub static TILT: Component<u32, Pose> = Component::named(p::TILT);
    pub static ACTION: Component<Text, Pose> = Component::named(p::ACTION);
    pub static PRIORITY: Component<i32, Pose> = Component::named(p::PRIORITY);
    pub static KEY: Component<Text, Pose> = Component::named(p::KEY);
    pub static FALLBACK: Component<Vec<String>, Pose> = Component::named(p::FALLBACK);
    pub static WHEN: Component<Vec<String>, Pose> = Component::named(p::WHEN);
}

/// What a game mode is made of: a tag per ability (`abi::comp::game_mode::FLAGS`) and the
/// modifiers its players get.
pub mod game_mode {
    use crate::abi::comp::game_mode as g;
    use crate::kind::GameMode;
    use crate::{Component, Tag};
    pub static MODIFIERS: Component<Vec<crate::abi::Modifier>, GameMode> = Component::named(crate::abi::comp::MODIFIERS);
    pub static INSTANT_BREAK: Tag<GameMode> = Tag::named(g::INSTANT_BREAK);
    pub static NO_BREAK: Tag<GameMode> = Tag::named(g::NO_BREAK);
    pub static NO_PLACE: Tag<GameMode> = Tag::named(g::NO_PLACE);
    pub static NO_COSTS: Tag<GameMode> = Tag::named(g::NO_COSTS);
    pub static INVULNERABLE: Tag<GameMode> = Tag::named(g::INVULNERABLE);
    pub static ALWAYS_FLY: Tag<GameMode> = Tag::named(g::ALWAYS_FLY);
    pub static NOCLIP: Tag<GameMode> = Tag::named(g::NOCLIP);
    pub static NO_INTERACT: Tag<GameMode> = Tag::named(g::NO_INTERACT);
    pub static HIDDEN: Tag<GameMode> = Tag::named(g::HIDDEN);
    pub static NO_HUD: Tag<GameMode> = Tag::named(g::NO_HUD);
    pub static CREATIVE_INVENTORY: Tag<GameMode> = Tag::named(g::CREATIVE_INVENTORY);
}

/// What a damage cause is made of: tags for what it does (`abi::comp::damage_cause`) and its
/// death messages.
pub mod damage_cause {
    use crate::abi::comp::damage_cause as d;
    use crate::kind::DamageCause;
    use crate::{Component, Tag, Text};
    pub static BYPASSES_ARMOR: Tag<DamageCause> = Tag::named(d::BYPASSES_ARMOR);
    pub static PIERCES_INVULNERABLE: Tag<DamageCause> = Tag::named(d::PIERCES_INVULNERABLE);
    pub static IGNORES_COOLDOWN: Tag<DamageCause> = Tag::named(d::IGNORES_COOLDOWN);
    pub static BURNING: Tag<DamageCause> = Tag::named(d::BURNING);
    pub static NO_KNOCKBACK: Tag<DamageCause> = Tag::named(d::NO_KNOCKBACK);
    pub static SILENT: Tag<DamageCause> = Tag::named(d::SILENT);
    pub static DEATH_MESSAGE: Component<Text, DamageCause> = Component::named(d::DEATH_MESSAGE);
    pub static DEATH_MESSAGE_BY: Component<Text, DamageCause> = Component::named(d::DEATH_MESSAGE_BY);
}

/// What a text place is made of (`abi::comp::text_place`).
pub mod text_place {
    use crate::abi::comp::text_place as t;
    use crate::kind::TextPlace;
    use crate::{Component, Text};
    pub static ANCHOR: Component<Text, TextPlace> = Component::named(t::ANCHOR);
    pub static ORDER: Component<i32, TextPlace> = Component::named(t::ORDER);
    pub static AT: Component<[f32; 4], TextPlace> = Component::named(t::AT);
    pub static SCALE: Component<f32, TextPlace> = Component::named(t::SCALE);
    pub static TIMES: Component<[f32; 3], TextPlace> = Component::named(t::TIMES);
    pub static CLASS: Component<Text, TextPlace> = Component::named(t::CLASS);
    pub static SAMPLE: Component<Text, TextPlace> = Component::named(t::SAMPLE);
}

/// What an entity effect is made of.
pub mod entity_effect {
    use crate::kind::EntityEffect;
    use crate::{Component, Text};
    pub static SHADER: Component<Text, EntityEffect> = Component::named(crate::abi::comp::ENTITY_EFFECT_SHADER);
}

/// What a sound event is made of (`abi::comp::sound`).
pub mod sound {
    use crate::abi::comp::sound as s;
    use crate::kind::Sound;
    use crate::{Component, Text};
    pub static FILES: Component<Vec<String>, Sound> = Component::named(s::FILES);
    pub static VOLUME: Component<f32, Sound> = Component::named(s::VOLUME);
    pub static PITCH: Component<[f32; 2], Sound> = Component::named(s::PITCH);
    pub static RANGE: Component<f32, Sound> = Component::named(s::RANGE);
    pub static FULL_WITHIN: Component<f32, Sound> = Component::named(s::FULL_WITHIN);
    pub static FALLOFF: Component<i32, Sound> = Component::named(s::FALLOFF);
    pub static CATEGORY: Component<Text, Sound> = Component::named(s::CATEGORY);
}

/// What background music is made of.
pub mod music {
    use crate::abi::comp::music as m;
    use crate::kind::Music;
    use crate::{Component, Text};
    pub static TRACKS: Component<Vec<String>, Music> = Component::named(m::TRACKS);
    pub static DIMENSION: Component<Text, Music> = Component::named(m::DIMENSION);
    pub static WHEN: Component<Vec<String>, Music> = Component::named(m::WHEN);
}

/// What a camera mode is made of: a tag per flag (`abi::comp::camera::FLAGS`) and these.
pub mod camera {
    use crate::abi::comp::camera as c;
    use crate::kind::CameraMode;
    use crate::{Component, Text};
    pub static KEY: Component<Text, CameraMode> = Component::named(c::KEY);
    pub static CALL: Component<crate::abi::Callback, CameraMode> = Component::named(c::CALL);
    pub static TRANSITION: Component<f32, CameraMode> = Component::named(c::TRANSITION);
    pub static EASING: Component<u32, CameraMode> = Component::named(c::EASING);
    pub static WHEN: Component<Vec<String>, CameraMode> = Component::named(c::WHEN);
}

/// What a setting is made of (`abi::comp::config`).
pub mod config {
    use crate::abi::comp::config as c;
    use crate::kind::Config;
    use crate::{Component, Text};
    pub static KIND: Component<u32, Config> = Component::named(c::KIND);
    pub static DEFAULT: Component<f64, Config> = Component::named(c::DEFAULT);
    pub static RANGE: Component<[f64; 2], Config> = Component::named(c::RANGE);
    pub static STEP: Component<f64, Config> = Component::named(c::STEP);
    pub static CHOICES: Component<Vec<String>, Config> = Component::named(c::CHOICES);
    pub static SCOPE: Component<u32, Config> = Component::named(c::SCOPE);
    pub static TEXT: Component<Text, Config> = Component::named(c::TEXT);
}

/// What an equipment slot is made of.
pub mod equipment_slot {
    use crate::abi::comp::equipment_slot as e;
    use crate::kind::EquipmentSlot;
    use crate::{Component, Text};
    pub static COVERS: Component<Vec<String>, EquipmentSlot> = Component::named(e::COVERS);
    pub static BACKGROUND: Component<Text, EquipmentSlot> = Component::named(e::BACKGROUND);
}

/// What an entity category is made of.
pub mod entity_category {
    use crate::Component;
    use crate::abi::comp::entity_category as c;
    use crate::kind::EntityCategory;
    pub static CAP: Component<u32, EntityCategory> = Component::named(c::CAP);
    pub static DESPAWN: Component<f32, EntityCategory> = Component::named(c::DESPAWN);
    pub static CAP_RADIUS: Component<f32, EntityCategory> = Component::named(c::CAP_RADIUS);
}

/// What a recipe kind is made of (`abi::comp::recipe_kind`).
pub mod recipe_kind {
    use crate::abi::comp::recipe_kind as r;
    use crate::kind::RecipeKind;
    use crate::{Component, Text};
    pub static INPUTS: Component<[u32; 2], RecipeKind> = Component::named(r::INPUTS);
    pub static OUTPUTS: Component<[u32; 2], RecipeKind> = Component::named(r::OUTPUTS);
    pub static INFO: Component<Text, RecipeKind> = Component::named(r::INFO);
    pub static INFO_VALUES: Component<Vec<String>, RecipeKind> = Component::named(r::INFO_VALUES);
    pub static LAYOUT: Component<crate::abi::Callback, RecipeKind> = Component::named(r::LAYOUT);
}

/// What an attribute is made of.
pub mod attribute {
    use crate::Component;
    use crate::kind::Attribute;
    pub static DEFAULT: Component<f64, Attribute> = Component::named(crate::abi::comp::attribute::DEFAULT);
    pub static RANGE: Component<[f64; 2], Attribute> = Component::named(crate::abi::comp::attribute::RANGE);
}

/// A creative tab's icon.
pub mod creative_tab {
    use crate::kind::CreativeTab;
    use crate::{Component, Text};
    pub static ICON: Component<Text, CreativeTab> = Component::named(crate::abi::comp::CREATIVE_TAB_ICON);
}

/// A tint kind's color where no biome gives one.
pub mod tint_kind {
    use crate::Component;
    use crate::kind::TintKind;
    pub static DEFAULT: Component<u32, TintKind> = Component::named(crate::abi::comp::TINT_KIND_DEFAULT);
}

/// A key's default binding.
pub mod key {
    use crate::kind::Key;
    use crate::{Component, Text};
    pub static DEFAULT: Component<Text, Key> = Component::named(crate::abi::comp::KEY_DEFAULT);
}

/// What a particle kind is made of: tags for its flags (`abi::comp::particle`) and these.
pub mod particle {
    use crate::abi::comp::particle as p;
    use crate::kind::Particle;
    use crate::{Component, Text};
    pub static TEXTURES: Component<Vec<String>, Particle> = Component::named(p::TEXTURES);
    pub static LIFE: Component<[f32; 2], Particle> = Component::named(p::LIFE);
    pub static SIZE: Component<[f32; 2], Particle> = Component::named(p::SIZE);
    pub static GRAVITY: Component<f32, Particle> = Component::named(p::GRAVITY);
    pub static DRAG: Component<f32, Particle> = Component::named(p::DRAG);
    pub static COLOR: Component<[u32; 2], Particle> = Component::named(p::COLOR);
    pub static SHADER: Component<Text, Particle> = Component::named(p::SHADER);
    pub static RULES: Component<Vec<crate::abi::ParticleRule>, Particle> = Component::named(p::RULES);
}

/// What a thing in the sky is made of (`abi::comp::sky`).
pub mod sky {
    use crate::abi::comp::sky as s;
    use crate::kind::SkyObject;
    use crate::{Component, Text};
    pub static KIND: Component<u32, SkyObject> = Component::named(s::KIND);
    pub static TEXTURE: Component<Text, SkyObject> = Component::named(s::TEXTURE);
    pub static SHADER: Component<Text, SkyObject> = Component::named(s::SHADER);
    pub static PHASE: Component<f32, SkyObject> = Component::named(s::PHASE);
    pub static ORBIT: Component<f32, SkyObject> = Component::named(s::ORBIT);
    pub static TILT: Component<f32, SkyObject> = Component::named(s::TILT);
    pub static HEADING: Component<f32, SkyObject> = Component::named(s::HEADING);
    pub static SIZE: Component<f32, SkyObject> = Component::named(s::SIZE);
    pub static SPIN: Component<f32, SkyObject> = Component::named(s::SPIN);
    pub static OPACITY: Component<[f32; 2], SkyObject> = Component::named(s::OPACITY);
    pub static COLOR: Component<u32, SkyObject> = Component::named(s::COLOR);
    pub static FRAMES: Component<[u32; 2], SkyObject> = Component::named(s::FRAMES);
    pub static FRAME_TICKS: Component<u32, SkyObject> = Component::named(s::FRAME_TICKS);
    pub static GLOW: Component<u32, SkyObject> = Component::named(s::GLOW);
    pub static COUNT: Component<u32, SkyObject> = Component::named(s::COUNT);
    pub static SEED: Component<u32, SkyObject> = Component::named(s::SEED);
    pub static HEIGHT: Component<f32, SkyObject> = Component::named(s::HEIGHT);
    pub static DRIFT: Component<f32, SkyObject> = Component::named(s::DRIFT);
    pub static ORDER: Component<i32, SkyObject> = Component::named(s::ORDER);
}

/// What a HUD element is made of.
pub mod hud_element {
    use crate::abi::comp::hud_element as h;
    use crate::kind::HudElement;
    use crate::{Component, Text};
    pub static ANCHOR: Component<Text, HudElement> = Component::named(h::ANCHOR);
    pub static ORDER: Component<i32, HudElement> = Component::named(h::ORDER);
    pub static CALL: Component<crate::abi::Callback, HudElement> = Component::named(h::CALL);
}

/// What a screen part is made of.
pub mod screen_part {
    use crate::abi::comp::screen_part as s;
    use crate::kind::ScreenPart;
    use crate::{Component, Text};
    pub static SCREEN: Component<Text, ScreenPart> = Component::named(s::SCREEN);
    pub static AREA: Component<Text, ScreenPart> = Component::named(s::AREA);
    pub static ORDER: Component<i32, ScreenPart> = Component::named(s::ORDER);
    pub static CALL: Component<crate::abi::Callback, ScreenPart> = Component::named(s::CALL);
}

/// A word set's words.
pub mod words {
    use crate::Component;
    use crate::kind::WordSet;
    pub static WORDS: Component<Vec<String>, WordSet> = Component::named(crate::abi::comp::WORDS);
}

/// What a component's declaration is made of: tags for its flags (`abi::comp::component`) and
/// these.
pub mod component {
    use crate::abi::comp::component as c;
    use crate::kind::Declaration;
    use crate::{Component, Text};
    pub static SCHEMA: Component<Text, Declaration> = Component::named(c::SCHEMA);
    pub static DEFAULT: Component<Vec<u8>, Declaration> = Component::named(c::DEFAULT);
    pub static SLOTS: Component<u32, Declaration> = Component::named(c::SLOTS);
}

/// What a recipe is made of (`abi::comp::recipe`); any other component is data it carries.
pub mod recipe {
    use crate::abi::comp::recipe as r;
    use crate::kind::Recipe;
    use crate::{Component, Text};
    pub static KIND: Component<Text, Recipe> = Component::named(r::KIND);
    pub static PATTERN: Component<Vec<String>, Recipe> = Component::named(r::PATTERN);
    pub static KEYS: Component<Vec<crate::abi::RecipeKey>, Recipe> = Component::named(r::KEYS);
    pub static INPUTS: Component<Vec<String>, Recipe> = Component::named(r::INPUTS);
    pub static AMOUNTS: Component<Vec<u32>, Recipe> = Component::named(r::AMOUNTS);
    pub static OUTPUT: Component<Text, Recipe> = Component::named(r::OUTPUT);
    pub static COUNT: Component<u32, Recipe> = Component::named(r::COUNT);
    pub static TIME: Component<u32, Recipe> = Component::named(r::TIME);
    pub static OUTPUTS: Component<Vec<crate::abi::RecipeOutput>, Recipe> = Component::named(r::OUTPUTS);
}

/// What a loot table is made of.
pub mod loot {
    use crate::Component;
    use crate::kind::LootTable;
    pub static POOLS: Component<Vec<crate::abi::LootPool>, LootTable> = Component::named(crate::abi::comp::loot::POOLS);
    pub static SWAPS: Component<Vec<crate::abi::LootSwap>, LootTable> = Component::named(crate::abi::comp::loot::SWAPS);
}

/// Where an ore, scatter, structure, decoration or spawn rule goes (`Placed` sets them).
pub mod common {
    use crate::abi::comp::common as c;
    use crate::kind::Placement;
    use crate::{Component, Text};
    pub static DIMENSIONS: Component<Vec<String>, Placement> = Component::named(c::DIMENSIONS);
    pub static BIOME: Component<Text, Placement> = Component::named(c::BIOME);
    pub static Y: Component<[i32; 2], Placement> = Component::named(c::Y);
}

/// What an ore is made of (`abi::comp::ore`).
pub mod ore {
    use crate::Component;
    use crate::abi::{self, comp::ore as o};
    use crate::kind::Ore;
    pub static VEINS: Component<u32, Ore> = Component::named(o::VEINS);
    pub static SIZE: Component<u32, Ore> = Component::named(o::SIZE);
    pub static SHAPE: Component<abi::VeinShape, Ore> = Component::named(o::SHAPE);
    pub static HEIGHT: Component<abi::OreHeight, Ore> = Component::named(o::HEIGHT);
    pub static REPLACES: Component<crate::Matcher, Ore> = Component::named(o::REPLACES);
    pub static REPLACES_WITH: Component<Vec<abi::OreVariant>, Ore> = Component::named(o::REPLACES_WITH);
    pub static EXPOSED: Component<f32, Ore> = Component::named(o::EXPOSED);
}

/// How a liquid flows and looks from inside (`abi::comp::liquid`).
pub mod liquid {
    use crate::Component;
    use crate::abi::{self, comp::liquid as l};
    use crate::kind::Liquid;
    pub static DELAY: Component<u32, Liquid> = Component::named(l::DELAY);
    pub static DROP_OFF: Component<u32, Liquid> = Component::named(l::DROP_OFF);
    pub static SLOPE_FIND: Component<u32, Liquid> = Component::named(l::SLOPE_FIND);
    pub static RENEWABLE: Component<u32, Liquid> = Component::named(l::RENEWABLE);
    pub static DENSITY: Component<i32, Liquid> = Component::named(l::DENSITY);
    pub static HEAT: Component<u32, Liquid> = Component::named(l::HEAT);
    pub static FOG: Component<abi::LiquidFog, Liquid> = Component::named(l::FOG);
    pub static OVERLAY: Component<abi::LiquidOverlay, Liquid> = Component::named(l::OVERLAY);
}

/// What a liquid reaction is made of (`abi::comp::reaction`).
pub mod reaction {
    use crate::abi::comp::reaction as r;
    use crate::kind::Reaction;
    use crate::{Component, Text};
    pub static LIQUID: Component<crate::Matcher, Reaction> = Component::named(r::LIQUID);
    pub static TOUCHING: Component<crate::Matcher, Reaction> = Component::named(r::TOUCHING);
    pub static SOURCE: Component<Text, Reaction> = Component::named(r::SOURCE);
    pub static FLOWING: Component<Text, Reaction> = Component::named(r::FLOWING);
    pub static FALLS_INTO: Component<Text, Reaction> = Component::named(r::FALLS_INTO);
}

/// What a scatter is made of (`abi::comp::scatter`).
pub mod scatter {
    use crate::abi::comp::scatter as s;
    use crate::kind::Scatter;
    use crate::{Component, Text};
    pub static CELL: Component<i32, Scatter> = Component::named(s::CELL);
    pub static REACH: Component<i32, Scatter> = Component::named(s::REACH);
    pub static REACH_UP: Component<i32, Scatter> = Component::named(s::REACH_UP);
    pub static REACH_DOWN: Component<i32, Scatter> = Component::named(s::REACH_DOWN);
    pub static DENSITY: Component<Text, Scatter> = Component::named(s::DENSITY);
    pub static GROUND: Component<crate::Matcher, Scatter> = Component::named(s::GROUND);
    pub static SHAPE: Component<crate::abi::Callback, Scatter> = Component::named(s::SHAPE);
}

/// What a decoration runs.
pub mod decoration {
    use crate::Component;
    use crate::kind::Decoration;
    pub static GENERATE: Component<crate::abi::Callback, Decoration> = Component::named(crate::abi::comp::decoration::GENERATE);
}

/// How often a structure comes (`abi::comp::structure`).
pub mod structure {
    use crate::Component;
    use crate::abi::comp::structure as s;
    use crate::kind::Structure;
    pub static SPACING: Component<u32, Structure> = Component::named(s::SPACING);
    pub static CHANCE: Component<f32, Structure> = Component::named(s::CHANCE);
    pub static BURIED: Component<i32, Structure> = Component::named(s::BURIED);
}

/// A template's blocks.
pub mod template {
    use crate::Component;
    use crate::kind::Template;
    pub static BLOCKS: Component<Vec<crate::abi::TemplateBlock>, Template> = Component::named(crate::abi::comp::template::BLOCKS);
}

/// What a natural spawning rule is made of (`abi::comp::spawn`).
pub mod spawn {
    use crate::abi::comp::spawn as s;
    use crate::kind::SpawnRule;
    use crate::{Component, Text};
    pub static ENTITY: Component<Text, SpawnRule> = Component::named(s::ENTITY);
    pub static PLACE: Component<u32, SpawnRule> = Component::named(s::PLACE);
    pub static GROUP: Component<[u32; 2], SpawnRule> = Component::named(s::GROUP);
    pub static WEIGHT: Component<u32, SpawnRule> = Component::named(s::WEIGHT);
    pub static CHANCE: Component<f32, SpawnRule> = Component::named(s::CHANCE);
    pub static WHEN: Component<Vec<String>, SpawnRule> = Component::named(s::WHEN);
}

/// What a terrain is made of (`abi::comp::terrain`): the lists edit like any (`rule_before` is an
/// `insert_before` on `RULES`).
pub mod terrain {
    use crate::abi::{self, comp::terrain as t};
    use crate::kind::Terrain;
    use crate::{Component, Text};
    pub static STONE: Component<Text, Terrain> = Component::named(t::STONE);
    pub static BASE_HEIGHT: Component<f64, Terrain> = Component::named(t::BASE_HEIGHT);
    pub static SEA: Component<abi::TerrainSea, Terrain> = Component::named(t::SEA);
    pub static CAVES: Component<abi::TerrainCaves, Terrain> = Component::named(t::CAVES);
    pub static FIELDS: Component<Vec<abi::TerrainField>, Terrain> = Component::named(t::FIELDS);
    pub static FEATURES: Component<Vec<abi::TerrainFeature>, Terrain> = Component::named(t::FEATURES);
    pub static MASKS: Component<Vec<abi::FeatureMask>, Terrain> = Component::named(t::MASKS);
    pub static RULES: Component<Vec<abi::BiomeRule>, Terrain> = Component::named(t::RULES);
    pub static RULE_EDITS: Component<Vec<abi::BiomeRule>, Terrain> = Component::named(t::RULE_EDITS);
    pub static CAVE_BIOMES: Component<Vec<abi::CaveBiome>, Terrain> = Component::named(t::CAVE_BIOMES);
}

/// An item's own (`abi::comp::item`), besides the shared ones (`component::MAX_STACK`..).
pub mod item {
    use crate::abi::comp::item as i;
    use crate::kind::Item;
    use crate::{Component, Text};
    pub static TEXTURE: Component<Text, Item> = Component::named(i::TEXTURE);
    pub static BLOCK: Component<Text, Item> = Component::named(i::BLOCK);
    pub static ON_USE: Component<crate::abi::Callback, Item> = Component::named(i::ON_USE);
    pub static CONDITION: Component<crate::abi::Callback, Item> = Component::named(i::CONDITION);
}

/// A biome's own climate and surface (`abi::comp::biome`), what terrain rules placing it start
/// from, and its tints.
pub mod biome {
    use crate::abi::{self, comp::biome as b};
    use crate::kind::Biome;
    use crate::{Component, Text};
    pub static RANGES: Component<Vec<abi::RuleRange>, Biome> = Component::named(b::RANGES);
    pub static HEIGHT: Component<[i32; 2], Biome> = Component::named(b::HEIGHT);
    pub static TOP: Component<Text, Biome> = Component::named(b::TOP);
    pub static FILLER: Component<Text, Biome> = Component::named(b::FILLER);
    pub static DEPTH: Component<i32, Biome> = Component::named(b::DEPTH);
    pub static UNDER: Component<Text, Biome> = Component::named(b::UNDER);
    pub static UNDER_LAYERS: Component<i32, Biome> = Component::named(b::UNDER_LAYERS);
    pub static DEEP_TOP: Component<Text, Biome> = Component::named(b::DEEP_TOP);
    pub static DEEP_BELOW: Component<i32, Biome> = Component::named(b::DEEP_BELOW);
    pub static BARE_ABOVE: Component<i32, Biome> = Component::named(b::BARE_ABOVE);
    pub static ICE: Component<Text, Biome> = Component::named(b::ICE);
    pub static TINTS: Component<Vec<abi::BiomeTint>, Biome> = Component::named(b::TINTS);
}

/// What a dimension is made of: tags for its flags (`abi::comp::dimension`) and these.
pub mod dimension {
    use crate::Component;
    use crate::abi::{self, comp::dimension as d};
    use crate::kind::Dimension;
    pub static GENERATOR: Component<abi::Callback, Dimension> = Component::named(d::GENERATOR);
    pub static SPAWN: Component<abi::Callback, Dimension> = Component::named(d::SPAWN);
    pub static COLUMN: Component<abi::Callback, Dimension> = Component::named(d::COLUMN);
    pub static CUBES: Component<[i32; 2], Dimension> = Component::named(d::CUBES);
    pub static SKY_ABOVE: Component<i32, Dimension> = Component::named(d::SKY_ABOVE);
    pub static GRAVITY: Component<f32, Dimension> = Component::named(d::GRAVITY);
    pub static DAY_LENGTH: Component<u32, Dimension> = Component::named(d::DAY_LENGTH);
    pub static FIXED_TIME: Component<u32, Dimension> = Component::named(d::FIXED_TIME);
    pub static DAYLIGHT: Component<Vec<abi::DaylightPoint>, Dimension> = Component::named(d::DAYLIGHT);
    pub static SKY_COLOR: Component<u32, Dimension> = Component::named(d::SKY_COLOR);
    pub static FOG_COLOR: Component<u32, Dimension> = Component::named(d::FOG_COLOR);
    pub static AMBIENT: Component<u32, Dimension> = Component::named(d::AMBIENT);
    pub static WEATHERS: Component<Vec<String>, Dimension> = Component::named(d::WEATHERS);
}

/// What a block is made of (`abi::comp::block`): its properties and callbacks, and per state its
/// look and shape, tags for its flags (`abi::comp::block::FLAGS`, `NOT_SOLID`). The builder's
/// setters set them; the engine's defaults stand where nothing is set.
pub mod block {
    use crate::abi::{self, Callback, comp::block as b};
    use crate::kind::Block;
    use crate::{Component, Text};
    pub static PROPS: Component<Vec<abi::BlockProp>, Block> = Component::named(b::PROPS);
    pub static ON_USE: Component<Callback, Block> = Component::named(b::ON_USE);
    pub static ON_NEIGHBOR: Component<Callback, Block> = Component::named(b::ON_NEIGHBOR);
    pub static ON_SCHEDULED: Component<Callback, Block> = Component::named(b::ON_SCHEDULED);
    pub static ON_CARRIED: Component<Callback, Block> = Component::named(b::ON_CARRIED);
    pub static ON_GROW: Component<Callback, Block> = Component::named(b::ON_GROW);
    pub static PLACEMENT: Component<Callback, Block> = Component::named(b::PLACEMENT);
    pub static MERGE: Component<Callback, Block> = Component::named(b::MERGE);
    pub static ON_SIGNAL: Component<Callback, Block> = Component::named(b::ON_SIGNAL);
    pub static SIGNAL_GATE: Component<Callback, Block> = Component::named(b::SIGNAL_GATE);
    pub static SIGNAL_READ: Component<Callback, Block> = Component::named(b::SIGNAL_READ);
    pub static OPENS: Component<Text, Block> = Component::named(b::OPENS);
    pub static SHARES_CELL: Component<crate::Matcher, Block> = Component::named(b::SHARES_CELL);
    pub static MODEL_FILE: Component<Text, Block> = Component::named(b::MODEL_FILE);
    pub static MODEL_TEXTURES: Component<Vec<abi::ModelTexture>, Block> = Component::named(b::MODEL_TEXTURES);
    pub static SHAPES_OF: Component<abi::ShapesOf, Block> = Component::named(b::SHAPES_OF);
    pub static RETEXTURE: Component<Vec<abi::Retexture>, Block> = Component::named(b::RETEXTURE);
    pub static LIGHT_FILTER: Component<[u8; 3], Block> = Component::named(b::LIGHT_FILTER);
    pub static LIGHT_OPACITY: Component<u8, Block> = Component::named(b::LIGHT_OPACITY);
    pub static LAYER: Component<u32, Block> = Component::named(b::LAYER);
    pub static MODEL: Component<u32, Block> = Component::named(b::MODEL);
    pub static TEXTURES: Component<[String; 6], Block> = Component::named(b::TEXTURES);
    pub static TURNS: Component<[u8; 6], Block> = Component::named(b::TURNS);
    pub static TURN: Component<u8, Block> = Component::named(b::TURN);
    pub static BOXES: Component<Vec<abi::ModelBox>, Block> = Component::named(b::BOXES);
    pub static HITBOX: Component<Vec<[f32; 6]>, Block> = Component::named(b::HITBOX);
    pub static OUTLINE: Component<Vec<[f32; 6]>, Block> = Component::named(b::OUTLINE);
    pub static SIGNAL: Component<abi::SignalDesc, Block> = Component::named(b::SIGNAL);
    pub static LIQUID_WALLS: Component<u8, Block> = Component::named(b::LIQUID_WALLS);
    pub static PIECES: Component<Vec<abi::Piece>, Block> = Component::named(b::PIECES);
    pub static RULES: Component<Vec<abi::BlockRule>, Block> = Component::named(b::RULES);
    pub static HAZARDS: Component<Vec<abi::Hazard>, Block> = Component::named(b::HAZARDS);
}

/// What an entity kind is made of: tags for its flags (`abi::comp::entity::FLAGS`, `OFF`) and
/// these; the player's inventory layout and menu too.
pub mod entity {
    use crate::Component;
    use crate::Text;
    use crate::abi::{self, comp::entity as e};
    use crate::kind::Entity;
    pub static SIZE: Component<abi::BodySize, Entity> = Component::named(e::SIZE);
    pub static EYE: Component<f32, Entity> = Component::named(e::EYE);
    pub static SPEED: Component<f32, Entity> = Component::named(e::SPEED);
    pub static CARRY_HEIGHT: Component<f32, Entity> = Component::named(e::CARRY_HEIGHT);
    pub static RIDE_CAMERA: Component<f32, Entity> = Component::named(e::RIDE_CAMERA);
    pub static TEXTURE: Component<Text, Entity> = Component::named(e::TEXTURE);
    pub static TEXTURE_SIZE: Component<[u32; 2], Entity> = Component::named(e::TEXTURE_SIZE);
    pub static PARTS: Component<Vec<abi::ModelPart>, Entity> = Component::named(e::PARTS);
    pub static BONE_MODELS: Component<Vec<abi::BoneModel>, Entity> = Component::named(e::BONE_MODELS);
    pub static CATEGORY: Component<Text, Entity> = Component::named(e::CATEGORY);
    pub static TEMPLATE: Component<Text, Entity> = Component::named(e::TEMPLATE);
    pub static BLOCK: Component<Text, Entity> = Component::named(e::BLOCK);
    pub static SEATS: Component<Vec<abi::Seat>, Entity> = Component::named(e::SEATS);
    pub static ATTACH: Component<Vec<abi::Attach>, Entity> = Component::named(e::ATTACH);
    pub static ON_TICK: Component<abi::Callback, Entity> = Component::named(e::ON_TICK);
    pub static ON_INTERACT: Component<abi::Callback, Entity> = Component::named(e::ON_INTERACT);
    pub static STEER: Component<abi::Callback, Entity> = Component::named(e::STEER);
    pub static DISMOUNT: Component<abi::Callback, Entity> = Component::named(e::DISMOUNT);
    pub static FRAMES: Component<Vec<abi::Callback>, Entity> = Component::named(e::FRAMES);
    pub static SLOT_GROUPS: Component<Vec<abi::SlotGroupDef>, Entity> = Component::named(e::SLOT_GROUPS);
    pub static INVENTORY_MENU: Component<Text, Entity> = Component::named(e::INVENTORY_MENU);
}

/// How a bone moves by itself (`abi::comp::bone_motion`).
pub mod bone_motion {
    use crate::Component;
    use crate::abi::comp::bone_motion as b;
    use crate::kind::BoneMotion;
    pub static GRAVITY: Component<f32, BoneMotion> = Component::named(b::GRAVITY);
    pub static SPRING: Component<f32, BoneMotion> = Component::named(b::SPRING);
    pub static DRAG: Component<f32, BoneMotion> = Component::named(b::DRAG);
}

/// What a menu is made of: tags for how its screen behaves (`abi::comp::menu::FLAGS`) and these.
pub mod menu {
    use crate::Component;
    use crate::abi::{self, comp::menu as m};
    use crate::kind::Menu;
    pub static GROUPS: Component<Vec<abi::SlotGroupDef>, Menu> = Component::named(m::GROUPS);
    pub static RECIPES: Component<Vec<String>, Menu> = Component::named(m::RECIPES);
    pub static ON_CHANGE: Component<abi::Callback, Menu> = Component::named(m::ON_CHANGE);
    pub static ON_TAKE: Component<abi::Callback, Menu> = Component::named(m::ON_TAKE);
    pub static ON_TICK: Component<abi::Callback, Menu> = Component::named(m::ON_TICK);
    pub static SCREEN: Component<abi::Callback, Menu> = Component::named(m::SCREEN);
    pub static ON_KEY: Component<abi::Callback, Menu> = Component::named(m::ON_KEY);
}
