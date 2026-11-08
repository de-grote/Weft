//! What other mods use of the base game, as types: its events with their values, components,
//! value chains, recipe kinds, equipment slots, creative tabs and dimensions. A mod adds
//! `base_api` to its `Cargo.toml` and gets these checked when it compiles, without depending on
//! the game's code; the engine still compares the value layouts when the mods load.
//!
//! Names here are full (`base:..`): the statics compile into whichever mod uses them.

use weft_sdk::*;

value! {
    /// Fired when a furnace finishes one item.
    pub struct Smelted { pub furnace: u64, pub input: u32, pub output: ItemStack }
}
pub static SMELTED: Event<Smelted> = Event::named("base:smelted");

value! {
    /// A furnace's state, synced to whoever has it open.
    pub struct Smelting { pub burn: u32, pub burn_max: u32, pub cook: u32, pub cook_max: u32, pub cooking_item: u32 }
}
pub static SMELTING_STATE: Component<Smelting> = Component::named("base:smelting");

value! {
    /// A player's hunger and air.
    pub struct Hunger { pub food: f32, pub saturation: f32, pub exhaustion: f32, pub air: f32, pub food_timer: u32 }
}
pub static HUNGER: Component<Hunger> = Component::named("base:hunger");

value! {
    /// A player's advancements, a bit each in the order of the base game's list.
    pub struct Advancements { pub done: u64 }
}
pub static ADVANCEMENTS: Component<Advancements> = Component::named("base:advancements");

value! {
    /// Where a player respawns: the foot of the bed they last used and its dimension
    /// (`dimension_key`), when `set` is 1. Kept through death.
    pub struct BedSpawn { pub bed: [i32; 3], pub set: u32, pub dim_key: u64 }
}
pub static BED_SPAWN: Component<BedSpawn> = Component::named("base:bed_spawn");

value! {
    /// Read by the hunger system; other mods set it on their own food.
    pub struct Food { pub hunger: u32, pub saturation: f32 }
}
value! {
    /// Protection while worn: about 4% less damage per point, less against big hits unless
    /// `toughness` holds them back, the way modern's armor works.
    pub struct Armor { pub points: u32, pub toughness: f32 }
}

// The common vocabulary the base game reads; any mod may set these on its own content.
pub static FOOD: Component<Food> = Component::named("food");

/// Eating for `.on_use`: hunger and saturation from its `FOOD`, one item less. With `.use_ticks(32)`
/// it takes as long as the base game's food.
pub fn eat(u: &mut ItemUse) -> bool {
    let stack = u.stack();
    let Some(food) = FOOD.on_stack(&stack) else { return false };
    let w = u.world();
    let Some(h) = w.component_mut(u.player(), &HUNGER) else { return false };
    if h.food >= 20.0 {
        return false;
    }
    h.food = (h.food + food.hunger as f32).min(20.0);
    h.saturation = (h.saturation + food.saturation).min(h.food);
    u.set_stack(consume_one(stack));
    true
}
/// Burn time in ticks.
pub static FUEL: Component<u32> = Component::named("fuel");
pub static ARMOR: Component<Armor> = Component::named("armor");
/// How readily fire takes it.
pub static FLAMMABLE: Component<u32> = Component::named("flammable");

/// A furnace's cooking time: base is the recipe's time, `entity` the furnace, `item` the input.
pub static SMELT_TIME: Chain = Chain::named("base:smelt_time");

pub static CRAFTING: RecipeKind = RecipeKind::named("base:crafting");
pub static SMELTING: RecipeKind = RecipeKind::named("base:smelting");
/// What the woodcutter cuts: one input into smaller pieces of it.
pub static WOODCUTTING: RecipeKind = RecipeKind::named("base:woodcutting");

/// The player's equipment slots, head to feet.
pub static HEAD: EquipmentSlot = EquipmentSlot::named("base:head");
pub static CHEST: EquipmentSlot = EquipmentSlot::named("base:chest");
pub static LEGS: EquipmentSlot = EquipmentSlot::named("base:legs");
pub static FEET: EquipmentSlot = EquipmentSlot::named("base:feet");

pub static OVERWORLD: Dimension = Dimension::named("base:overworld");
pub static FLATLANDS: Dimension = Dimension::named("base:flatlands");

/// What is in the sky of the game's dimensions. Any of them can be changed or taken out by a mod:
/// `r.edit(OVERWORLD.sky(&MOON)).size(40.0)`, `r.remove(OVERWORLD.sky(&STARS))`.
pub static SUN: SkyObject = SkyObject::named("base:sun");
pub static MOON: SkyObject = SkyObject::named("base:moon");
pub static STARS: SkyObject = SkyObject::named("base:stars");
pub static CLOUDS: SkyObject = SkyObject::named("base:clouds");

/// Vanilla's creative tabs.
pub mod tabs {
    use weft_sdk::CreativeTab;
    pub static BUILDING: CreativeTab = CreativeTab::named("base:building");
    pub static COLORED: CreativeTab = CreativeTab::named("base:colored");
    pub static NATURAL: CreativeTab = CreativeTab::named("base:natural");
    pub static FUNCTIONAL: CreativeTab = CreativeTab::named("base:functional");
    pub static TOOLS: CreativeTab = CreativeTab::named("base:tools");
    pub static COMBAT: CreativeTab = CreativeTab::named("base:combat");
    pub static FOOD: CreativeTab = CreativeTab::named("base:food");
    pub static INGREDIENTS: CreativeTab = CreativeTab::named("base:ingredients");
    pub static REDSTONE: CreativeTab = CreativeTab::named("base:redstone");
}
