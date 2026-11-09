//! Vanilla's status effects, each a few attribute modifiers or hazards with an icon, and a potion
//! for each: drunk, it gives the effect and leaves the bottle.

use weft_sdk::abi::attr_op as op;
use weft_sdk::attribute as a;
use weft_sdk::*;

pub static SPEED: EntityEffect = EntityEffect::named("speed");
pub static SLOWNESS: EntityEffect = EntityEffect::named("slowness");
pub static HASTE: EntityEffect = EntityEffect::named("haste");
pub static MINING_FATIGUE: EntityEffect = EntityEffect::named("mining_fatigue");
pub static STRENGTH: EntityEffect = EntityEffect::named("strength");
pub static WEAKNESS: EntityEffect = EntityEffect::named("weakness");
pub static JUMP_BOOST: EntityEffect = EntityEffect::named("jump_boost");
pub static SLOW_FALLING: EntityEffect = EntityEffect::named("slow_falling");
pub static LEVITATION: EntityEffect = EntityEffect::named("levitation");
pub static REGENERATION: EntityEffect = EntityEffect::named("regeneration");
pub static POISON: EntityEffect = EntityEffect::named("poison");
pub static WITHER: EntityEffect = EntityEffect::named("wither");
pub static RESISTANCE: EntityEffect = EntityEffect::named("resistance");
pub static FIRE_RESISTANCE: EntityEffect = EntityEffect::named("fire_resistance");
pub static HEALTH_BOOST: EntityEffect = EntityEffect::named("health_boost");
pub static NIGHT_VISION: EntityEffect = EntityEffect::named("night_vision");
pub static BLINDNESS: EntityEffect = EntityEffect::named("blindness");
pub static INVISIBILITY: EntityEffect = EntityEffect::named("invisibility");
pub static INSTANT_HEALTH: EntityEffect = EntityEffect::named("instant_health");
pub static INSTANT_DAMAGE: EntityEffect = EntityEffect::named("instant_damage");
pub static HUNGER: EntityEffect = EntityEffect::named("hunger");
pub static WATER_BREATHING: EntityEffect = EntityEffect::named("water_breathing");
pub static NAUSEA: EntityEffect = EntityEffect::named("nausea");

pub static GLASS_BOTTLE: Item = Item::named("glass_bottle");

static MAGIC: DamageCause = DamageCause::named("magic");
static POISONED: DamageCause = DamageCause::named("poison");
static WITHERED: DamageCause = DamageCause::named("wither");

static BLINDNESS_SCREEN: Shader = Shader::named("blindness");
static NAUSEA_SCREEN: Shader = Shader::named("nausea");

/// Its breath doesn't run out under water (1 or more; the water breathing effect).
pub static BREATHES_UNDER_WATER: Attribute = Attribute::named("water_breathing");
/// Exhaustion a tick whatever it does (0; the hunger effect).
pub static EXHAUSTION: Attribute = Attribute::named("exhaustion");

/// A status effect named like its icon, `textures/mob_effect/<name>.png`, drawn by nothing on
/// the entity itself.
fn status<'a>(r: &'a mut Registry, e: &'static EntityEffect, color: u32) -> EntityEffectBuilder<'a> {
    let name = e.name().rsplit(':').next().unwrap_or("").to_string();
    r.entity_effect(e)
        .unseen()
        .icon(&format!("mob_effect/{name}"))
        .color(color)
}

/// Ticks between a heal or a hurt of vanilla's regeneration, poison and wither at levels I to V:
/// halved with every level.
fn every(first: u32, level: u32) -> u32 {
    (first >> (level - 1)).max(1)
}

pub fn register(r: &mut Registry) {
    r.damage_cause(&MAGIC)
        .bypasses_armor()
        .death_message("death.base.magic");
    r.damage_cause(&POISONED).bypasses_armor().not_lethal();
    r.damage_cause(&WITHERED)
        .bypasses_armor()
        .death_message("death.base.wither");
    r.attribute(&BREATHES_UNDER_WATER, 0.0, 0.0, 1.0);
    r.attribute(&EXHAUSTION, 0.0, 0.0, 10.0);

    status(r, &SPEED, 0x33EBFF)
        .beneficial()
        .modifier(Modifier::new(&a::MOVEMENT_SPEED, op::MULTIPLY_BASE, 0.2));
    status(r, &SLOWNESS, 0x8BAFE0)
        .harmful()
        .modifier(Modifier::new(&a::MOVEMENT_SPEED, op::MULTIPLY_BASE, -0.15));
    status(r, &HASTE, 0xD9C043)
        .beneficial()
        .modifier(Modifier::new(&a::MINING_SPEED, op::MULTIPLY_BASE, 0.2))
        .modifier(Modifier::new(&a::ATTACK_SPEED, op::MULTIPLY_BASE, 0.1));
    // Vanilla's steep steps: a third, a ninth, then next to nothing.
    let mut fatigue = status(r, &MINING_FATIGUE, 0x4A4217)
        .harmful()
        .modifier(Modifier::new(&a::ATTACK_SPEED, op::MULTIPLY_BASE, -0.1));
    for (level, share) in [(1, 0.3), (2, 0.09), (3, 0.0027), (4, 0.00081)] {
        fatigue = fatigue.modifier_at(level, Modifier::new(&a::MINING_SPEED, op::MULTIPLY_TOTAL, share - 1.0));
    }
    // Past IV as at IV.
    drop(fatigue);
    status(r, &STRENGTH, 0xFFC700)
        .beneficial()
        .modifier(Modifier::add(&a::ATTACK_DAMAGE, 3.0));
    status(r, &WEAKNESS, 0x484D48)
        .harmful()
        .modifier(Modifier::add(&a::ATTACK_DAMAGE, -4.0));
    // A tenth of a block a tick more, and a block more fallen without harm, a level.
    status(r, &JUMP_BOOST, 0xFDFF84)
        .beneficial()
        .modifier(Modifier::add(&a::JUMP_STRENGTH, 0.1 / 0.42))
        .modifier(Modifier::add(&a::SAFE_FALL, 1.0));
    status(r, &SLOW_FALLING, 0xF3CFB9)
        .beneficial()
        .modifier(Modifier::new(&a::GRAVITY, op::SET, 0.125))
        .modifier(Modifier::new(&a::FALL_DAMAGE_MULTIPLIER, op::SET, 0.0));
    // Up at a twentieth of a block a tick a level, as vanilla's, what drag leaves of a gravity
    // turned around.
    let mut levitation = status(r, &LEVITATION, 0xCEFFFF).harmful();
    for level in 1..=5 {
        levitation = levitation.modifier_at(level, Modifier::new(&a::GRAVITY, op::SET, -0.0125 * level as f64));
    }
    drop(levitation);
    // Vanilla's: a heal or a hurt every so many ticks, half as many with every level.
    for (e, color, amount, cause, first, harmful) in [
        (&REGENERATION, 0xCD5CAB, -1.0, &damage_causes::HEAL, 50, false),
        (&POISON, 0x87A363, 1.0, &POISONED, 25, true),
        (&WITHER, 0x736156, 1.0, &WITHERED, 40, true),
    ] {
        let mut b = status(r, e, color);
        b = if harmful { b.harmful() } else { b.beneficial() };
        // Past V as at V.
        for level in 1..=5 {
            b = b.hazard_at(level, Hazard::own(amount, cause).every(every(first, level)));
        }
    }
    status(r, &RESISTANCE, 0x9146F0)
        .beneficial()
        .modifier(Modifier::add(&a::DAMAGE_RESISTANCE, 0.2));
    status(r, &FIRE_RESISTANCE, 0xFF9900)
        .beneficial()
        .modifier(Modifier::new(&a::FIRE_RESISTANCE, op::SET, 1.0));
    status(r, &HEALTH_BOOST, 0xF87D23)
        .beneficial()
        .modifier(Modifier::add(&a::MAX_HEALTH, 4.0));
    status(r, &NIGHT_VISION, 0xC2FF66)
        .beneficial()
        .modifier(Modifier::new(&a::NIGHT_VISION, op::SET, 1.0));
    status(r, &BLINDNESS, 0x1F1F23)
        .harmful()
        .screen(&BLINDNESS_SCREEN);
    // Unseen, and mobs notice it from a fourteenth as far.
    status(r, &INVISIBILITY, 0xF6F6F6)
        .beneficial()
        .hides()
        .modifier(Modifier::new(&a::VISIBILITY, op::MULTIPLY_TOTAL, -0.93));
    // Twice as much a level, as vanilla's, up to VIII.
    for (e, color, amount, cause, harmful) in
        [(&INSTANT_HEALTH, 0xF82423, -4.0, &damage_causes::HEAL, false), (&INSTANT_DAMAGE, 0xA9656A, 6.0, &MAGIC, true)]
    {
        let mut b = status(r, e, color).instant();
        b = if harmful { b.harmful() } else { b.beneficial() };
        for level in 1..=8u32 {
            b = b.hazard_at(level, Hazard::own(amount * (1 << (level - 1)) as f32, cause));
        }
    }
    status(r, &HUNGER, 0x587653)
        .harmful()
        .modifier(Modifier::add(&EXHAUSTION, 0.005));
    status(r, &WATER_BREATHING, 0x98DAC0)
        .beneficial()
        .modifier(Modifier::new(&BREATHES_UNDER_WATER, op::SET, 1.0));
    status(r, &NAUSEA, 0x551D4A)
        .harmful()
        .screen(&NAUSEA_SCREEN);
    // The engine's glow becomes a status effect too.
    r.edit(&entity_effects::GLOW)
        .icon("mob_effect/glowing")
        .color(0x94A061)
        // Lights up all around, a torch's light, as if held.
        .light_rgb(15, 13, 10);

    r.item(&GLASS_BOTTLE).tab(&crate::tabs::INGREDIENTS);
    // A potion each: vanilla's three minutes, an instant one at once.
    for e in [
        &SPEED,
        &SLOWNESS,
        &HASTE,
        &MINING_FATIGUE,
        &STRENGTH,
        &WEAKNESS,
        &JUMP_BOOST,
        &SLOW_FALLING,
        &LEVITATION,
        &REGENERATION,
        &POISON,
        &WITHER,
        &RESISTANCE,
        &FIRE_RESISTANCE,
        &HEALTH_BOOST,
        &NIGHT_VISION,
        &BLINDNESS,
        &INVISIBILITY,
        &INSTANT_HEALTH,
        &INSTANT_DAMAGE,
        &HUNGER,
        &WATER_BREATHING,
        &NAUSEA,
    ] {
        let name = e.name().rsplit(':').next().unwrap_or("").to_string();
        let instant = matches!(name.as_str(), "instant_health" | "instant_damage");
        r.item(Item::generated(format!("potion_{name}")))
            .max_stack(1)
            .use_ticks(32)
            .remainder(&GLASS_BOTTLE)
            .gives_effect(e, 1, if instant { 0.0 } else { 180.0 })
            .tag("potions")
            .tab(&crate::tabs::FOOD);
    }
}
