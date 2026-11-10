//! Survival rules on top of the engine's player: hunger, regeneration, drowning, armor, eating,
//! the HUD and the inventory screen with its 2x2 crafting grid.

use weft_sdk::*;

use crate::items::{ARMOR, ARMOR_PIECE, ARMOR_TOUGHNESS, FOOD};

pub use base_api::{FEET, HEAD, HUNGER, Hunger, LEGS};

/// Every 4 seconds a player with a full belly heals and a starving one hurts, like vanilla's.
const PULSE_TICKS: u32 = 80;
pub static INVENTORY: Menu = Menu::named("inventory");
pub static CRAFTING: Menu = Menu::named("crafting_table");
static HUD_HEALTH: HudElement = HudElement::named("health");
static HUD_ARMOR: HudElement = HudElement::named("armor");
static HUNGRY: Condition = Condition::named("hungry");
/// How fast the food goes (1 vanilla's, 0 never): a world rule, and one player's by `/attribute`.
static HUNGER_RATE: Attribute = Attribute::named("hunger_rate");
static HUD_FOOD: HudElement = HudElement::named("food");
static HUD_AIR: HudElement = HudElement::named("air");
static HUD_ATTACK: HudElement = HudElement::named("attack_indicator");
static WATER_LIKE: Tag = Tag::named("water");
static ALWAYS_EATABLE: Tag = Tag::named("can_always_eat");

const MAX_AIR: f32 = 300.0;

/// Head to feet.
pub const EQUIPMENT: [&EquipmentSlot; 4] = [&HEAD, &base_api::CHEST, &LEGS, &FEET];

pub fn register(r: &mut Registry) {
    r.attribute(&ARMOR, 0.0, 0.0, 30.0);
    r.attribute(&ARMOR_TOUGHNESS, 0.0, 0.0, 20.0);
    r.player_slots()
        .group("hotbar", Slots::row(9).hotbar())
        .group("main", Slots::grid(9, 3));
    for (slot, covers, piece) in [
        (&HEAD, worn::HEAD, "helmet"),
        (&base_api::CHEST, worn::CHEST, "chestplate"),
        (&LEGS, worn::LEGS, "leggings"),
        (&FEET, worn::FEET, "boots"),
    ] {
        r.equipment_slot(slot, covers, &format!("slot_{piece}"));
    }
    r.equipment_slot(&OFF_HAND, &[], "slot_offhand");
    r.entity_component(
        &HUNGER,
        Hunger { food: 20.0, saturation: 5.0, exhaustion: 0.0, air: MAX_AIR, food_timer: 0 },
        component_flags::SAVE | component_flags::SYNC_OWNER,
    );
    // Too hungry to sprint: modern's 3 drumsticks.
    r.condition(&HUNGRY, |c| {
        let w = c.world();
        w.is_some_and(|w| {
            w.component(c.entity(), &HUNGER)
                .is_some_and(|h| h.food <= 6.0)
        })
    });
    r.attribute(&HUNGER_RATE, 1.0, 0.0, 10.0).rule();
    r.edit(&PLAYER)
        .attach(&HUNGER)
        .modifier(Modifier::add(&attribute::CAN_SPRINT, -1.0).when(&HUNGRY));
    r.world_tick(tick_players);
    // Not hungry, it doesn't start eating; the other way round it would only find out at the end.
    r.before_event(&actions::USE_ITEM, |q| {
        if ALWAYS_EATABLE.on_item(q.item()) {
            return Verdict::Abstain;
        }
        let full = FOOD.on_item(q.item()).is_some()
            && q.world()
                .component(q.entity(), &HUNGER)
                .is_some_and(|h| h.food >= 20.0);
        if full { Verdict::Deny } else { Verdict::Abstain }
    });
    // Modern's exhaustion, by what the player does.
    r.on_event(&actions::BREAK_BLOCK, |b| {
        let w = b.world();
        for e in b.iter() {
            tire(&w, e.player, 0.005);
        }
    });
    r.on_event(&actions::HURT, |b| {
        let w = b.world();
        for e in b.iter() {
            tire(&w, e.source, 0.1);
            if armor_applies(e.cause) {
                tire(&w, e.entity, 0.1);
            }
        }
    });
    // Armor is a step of the damage chain (modern's CombatRules: big hits go through more of it,
    // unless it is tough), and wears when the hit actually lands, a quarter of the damage per
    // piece (at least 1) like vanilla.
    r.modify(&chain::DAMAGE, &phase::MULTIPLY, |m| {
        let q = *m.query();
        if !armor_applies(q.cause) {
            return;
        }
        let w = m.world();
        let (points, toughness) = (w.attribute(q.entity, &ARMOR), w.attribute(q.entity, &ARMOR_TOUGHNESS));
        let through = m.value / (2.0 + toughness / 4.0);
        m.value *= 1.0 - (points - through).clamp(points * 0.2, 20.0) / 25.0;
    });
    r.on_event(&actions::HURT, |b| {
        let w = b.world();
        for h in b.iter().filter(|h| armor_applies(h.cause)) {
            let Some(worn) = w.inventory(h.entity).find_group(EQUIPMENT_GROUP) else { continue };
            let wear = ((h.amount / 4.0) as u32).max(1);
            for slot in 0..worn.len() {
                let s = worn.get(slot);
                if ARMOR_PIECE.on_item(s.item) {
                    worn.set(slot, s.add_wear(wear));
                }
            }
        }
    });

    // Names the held stack, like an anvil: a chest placed from it is called so.
    r.command("rename", "[name:text]", |cmd| {
        let w = cmd.world();
        let hotbar = w.inventory(cmd.player()).group("hotbar");
        let slot = w.selected_slot(cmd.player());
        let s = hotbar.get(slot);
        if !s.is_empty() {
            hotbar.set(slot, stack_with_name(s, cmd.text("name")));
        }
    })
    .permission(permission::EVERYONE);
    r.command("feed", "[food:int]", |cmd| {
        let food = cmd.int("food").unwrap_or(20).clamp(0, 20) as f32;
        if let Some(h) = cmd.world().component_mut(cmd.player(), &HUNGER) {
            h.food = food;
            h.saturation = h.saturation.min(food);
        }
        cmd.reply("command.base.feed", &[Arg::Int(food as i64)]);
    });

    // Survival bars; a negative value hides them (creative).
    r.hud_element(&HUD_HEALTH, hud_anchor::HOTBAR_LEFT, 0, |ui| {
        ui.icons("heart", 10, |c| if c.outside_survival() { -1.0 } else { c.player_state().map_or(20.0, |s| s.health.ceil()) })
            .max_by(|c| c.player_state().map_or(20.0, |s| s.max_health.ceil()))
            // Wither's black hearts over poison's green ones, as vanilla's.
            .look_while("heart_withered", |c| c.effect_level(&crate::effects::WITHER) as f32)
            .look_while("heart_poisoned", |c| c.effect_level(&crate::effects::POISON) as f32)
            // One life: hearts that say so, as vanilla's hardcore ones.
            .look_while("heart_hardcore", |_| (weft_sdk::settings::LIVES.get() == 1) as u32 as f32)
            .shaking()
            .flash_on_gain(0.5)
            .flash_on_loss(1.0);
    });
    r.hud_element(&HUD_ARMOR, hud_anchor::HOTBAR_LEFT, 1, |ui| {
        ui.icons("armor", 10, |c| {
            let points = c.attribute(&ARMOR).floor() as f32;
            if points > 0.0 && !c.outside_survival() { points } else { -1.0 }
        });
    });
    r.hud_element(&HUD_FOOD, hud_anchor::HOTBAR_RIGHT, 0, |ui| {
        ui.icons("food", 10, |c| if c.outside_survival() { -1.0 } else { c.component(&HUNGER).map_or(20.0, |h| h.food.ceil()) })
            .from_right()
            // Green while something drains it (the hunger effect), as vanilla's.
            .look_while("food_hunger", |c| c.attribute(&crate::effects::EXHAUSTION) as f32)
            // With no saturation left the food shakes, harder when it is nearly gone.
            .shaking_by(|c| match c.component(&HUNGER) {
                Some(h) if h.saturation <= 0.0 => {
                    if h.food <= 6.0 {
                        2.0
                    } else {
                        1.0
                    }
                }
                _ => 0.0,
            })
            .overlay(|c| c.component(&HUNGER).map_or(0.0, |h| h.saturation.ceil()))
            // What the food in hand would fill, fading in and out.
            .preview(1.6, |c| {
                let held = c
                    .player_state()
                    .map_or(ItemStack::EMPTY, |s| c.inventory_slot(s.selected));
                match (c.component(&HUNGER), FOOD.on_item(held.item)) {
                    (Some(h), Some(f)) => (h.food + f.hunger as f32).min(20.0),
                    _ => 0.0,
                }
            });
    });
    r.hud_element(&HUD_AIR, hud_anchor::HOTBAR_RIGHT, 1, |ui| {
        ui.icons("air", 10, |c| if c.outside_survival() { -1.0 } else { c.component(&HUNGER).map_or(-1.0, |h| air_bubbles(h.air)) })
            .from_right();
    });

    // While a swing charges.
    r.hud_element(&HUD_ATTACK, hud_anchor::UNDER_CROSSHAIR, 0, |ui| {
        ui.progress_bind("attack_bar", S::default().size(16.0, 4.0), |c| {
            let charge = c.swing_strength();
            if charge < 1.0 { charge } else { -1.0 }
        })
    });

    // The equipment slots take what is worn there and show its silhouette while empty.
    r.menu(&INVENTORY)
        .group("grid", Slots::grid(2, 2))
        .group("result", Slots::single().output().large())
        .equipment()
        .player_inventory()
        // Last, so the slots before keep their numbers.
        .group(
            "trash",
            Slots::single()
                .trash()
                .background("slot_trash")
                .tooltip("storage.base.trash"),
        )
        .recipes(&crate::recipes::CRAFTING)
        .on_change(craft_result)
        .on_take(consume_grid)
        .screen(|ui| {
            ui.panel("panel", S::column().pad(7.0).gap(5.0), |ui| {
                ui.row(S::row().gap(1.0), |ui| {
                    ui.equipment();
                    ui.player_model(51.0, 72.0);
                    ui.spacer();
                    ui.column(S::column().gap(2.0).stretch(), |ui| {
                        ui.title();
                        ui.row(S::row().gap(4.0), |ui| {
                            ui.group("grid");
                            ui.recipe_arrow();
                            ui.group("result");
                        });
                        ui.spacer();
                        ui.row(S::row(), |ui| {
                            ui.group(OFF_HAND.name());
                            ui.group("trash");
                        });
                    });
                });
                ui.player_inventory(false);
            });
        });

    r.menu(&CRAFTING)
        .group("grid", Slots::grid(3, 3))
        .group("result", Slots::single().output().large())
        .player_inventory()
        .recipes(&crate::recipes::CRAFTING)
        .on_change(craft_result)
        .on_take(consume_grid)
        .screen(|ui| {
            ui.panel("panel", S::column().pad(7.0).gap(4.0).stretch(), |ui| {
                ui.title();
                ui.row(S::row().gap(8.0), |ui| {
                    ui.spacer();
                    ui.group("grid");
                    ui.recipe_arrow();
                    ui.group("result");
                    ui.spacer();
                });
                ui.player_inventory(true);
            });
        });
    r.player_menu(&INVENTORY);
}

fn armor_applies(cause: u32) -> bool {
    !comps::damage_cause::BYPASSES_ARMOR.on_id(cause)
}

/// The `result` slot shows what the `grid` makes.
fn craft_result(m: &MenuCtx) {
    let grid = m.group("grid");
    let cells: Vec<ItemStack> = (0..grid.len()).map(|i| grid.get(i)).collect();
    m.group("result")
        .set(0, recipe_grid_output(&crate::recipes::CRAFTING, &cells, grid.cols(), grid.rows()));
}

/// Takes one of each ingredient; the last of a stack leaves its `remainder` (a bucket).
fn consume_grid(m: &MenuCtx) {
    let grid = m.group("grid");
    for i in 0..grid.len() {
        let s = grid.get(i);
        if !s.is_empty() {
            grid.set(i, consume_one(s));
        }
    }
}

fn tire(w: &World, player: u64, amount: f32) {
    if let Some(h) = w
        .component_mut(player, &HUNGER)
        .filter(|_| !w.outside_survival(player))
    {
        h.exhaustion += amount;
    }
}

/// Modern's FoodData: full and saturated heals fast and burns the saturation, 18 and up heals
/// slowly, empty starves.
fn heal_or_starve(w: &World, id: u64, st: &mut EntityState) {
    let Some(h) = w.component_mut(id, &HUNGER) else { return };
    let hurt = st.health < st.max_health;
    let every = if h.food >= 20.0 && h.saturation > 0.0 && hurt {
        10
    } else if (h.food >= 18.0 && hurt) || h.food <= 0.0 {
        PULSE_TICKS
    } else {
        h.food_timer = 0;
        return;
    };
    h.food_timer += 1;
    if h.food_timer < every {
        return;
    }
    h.food_timer = 0;
    if h.food <= 0.0 {
        if st.health > 1.0 {
            w.damage(id, Damage::new(1.0, damage_cause::STARVE));
        }
    } else {
        let spent = if every == 10 { h.saturation.min(6.0) } else { 6.0 };
        h.exhaustion += spent;
        w.heal(id, spent / 6.0);
    }
}

/// What the air bar shows: whole bubbles like vanilla's, hidden with a full breath.
fn air_bubbles(air: f32) -> f32 {
    if air < MAX_AIR { (air.max(0.0) / MAX_AIR * 10.0).ceil() * 2.0 } else { -1.0 }
}

fn tick_players(w: &World) {
    for id in w.players() {
        let Some(mut st) = w.entity(id) else { continue };
        if w.outside_survival(id) {
            // Nothing takes the breath here, and what was used comes back.
            if w.component(id, &HUNGER).is_some_and(|h| h.air < MAX_AIR) {
                if let Some(h) = w.component_mut(id, &HUNGER) {
                    h.air = (h.air + 4.0).min(MAX_AIR);
                }
            }
            continue;
        }
        heal_or_starve(w, id, &mut st);
        // Jumping tires by a landing's worth of pulling yourself up.
        if st.jumped != 0 {
            tire(w, id, if st.sprinting != 0 { 0.2 } else { 0.05 });
        }
        let per_point = 4.0 / w.attribute(id, &HUNGER_RATE) as f32;
        let drain = w.attribute(id, &crate::effects::EXHAUSTION) as f32;
        let breathes = w.attribute(id, &crate::effects::BREATHES_UNDER_WATER) >= 1.0;
        let Some(h) = w.component_mut(id, &HUNGER) else { continue };
        h.exhaustion += drain;
        // Modern's: 0.1 a block sprinting, 0.01 swimming, 0 walking; jumps are counted from `Move`.
        h.exhaustion += st.moved
            * if st.sprinting != 0 {
                0.1
            } else if st.in_liquid != 0 {
                0.01
            } else {
                0.0
            };
        if h.exhaustion > per_point {
            h.exhaustion -= per_point;
            if h.saturation > 0.0 {
                h.saturation = (h.saturation - 1.0).max(0.0);
            } else {
                h.food = (h.food - 1.0).max(0.0);
            }
        }
        // Only water takes the breath, whatever else burns or drags.
        let underwater = !breathes && WATER_LIKE.on_block(w.block(block_pos_of(st.eye()))) && w.in_liquid(st.eye());
        let Some(h) = w.component_mut(id, &HUNGER) else { continue };
        if underwater {
            h.air -= 1.0;
            if h.air <= -20.0 {
                h.air = 0.0;
                w.damage(id, Damage::new(2.0, damage_cause::DROWN));
            }
        } else if h.air < MAX_AIR {
            h.air = (h.air + 4.0).min(MAX_AIR);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_shows_whole_bubbles() {
        assert_eq!(air_bubbles(MAX_AIR), -1.0, "hidden with a full breath");
        assert_eq!(air_bubbles(299.0), 20.0);
        assert_eq!(air_bubbles(270.0), 18.0);
        assert_eq!(air_bubbles(1.0), 2.0);
        assert_eq!(air_bubbles(-5.0), 0.0);
    }
}
