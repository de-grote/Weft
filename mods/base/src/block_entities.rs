//! Furnace and chest: blocks with an entity holding their inventory and state.

use weft_sdk::*;

use crate::blocks::{CHEST, FURNACE};
use crate::items::FUEL;

pub use base_api::{SMELT_TIME, SMELTED, SMELTING_STATE as SMELTING, Smelted, Smelting};
pub static FURNACE_MENU: Menu = Menu::named("furnace");
pub static CHEST_MENU: Menu = Menu::named("chest");

pub fn register(r: &mut Registry) {
    r.entity_component(
        &SMELTING,
        Smelting { burn: 0, burn_max: 0, cook: 0, cook_max: 0, cooking_item: 0 },
        component_flags::SAVE | component_flags::SYNC_MENU,
    );
    r.system(&SMELTING, tick_furnaces);
    // The furnace's state syncs to whoever has it open: the screen binds to it directly.
    r.block(&FURNACE)
        // Front towards the player; unlit and lit.
        .sound_group("stone")
        .facing(Facings::HORIZONTAL, Toward::Player)
        .prop("lit", ["false", "true"])
        .states(|p, s| {
            let lit = p.is("lit", "true");
            let s = s.front_faces("furnace_side", "furnace_top", if lit { "furnace_front_lit" } else { "furnace_front" });
            if lit { s.light_rgb(13, 11, 8) } else { s }
        })
        .hardness(3.5)
        .mined_by("pickaxe", 0)
        .tab(&crate::tabs::FUNCTIONAL)
        .entity(|e| e.attach(&SMELTING).inventory(3))
        .opens(&FURNACE_MENU);
    r.menu(&FURNACE_MENU)
        .group("input", Slots::single().holder())
        .group("fuel", Slots::single().accepts("fuel").holder())
        .group("output", Slots::single().output().holder())
        .recipes(&crate::recipes::SMELTING)
        .player_inventory()
        .screen(|ui| {
            ui.panel("panel", S::column().pad(7.0).gap(4.0).stretch(), |ui| {
                ui.title();
                ui.row(S::row().gap(8.0), |ui| {
                    ui.spacer();
                    ui.column(S::column().gap(2.0), |ui| {
                        ui.group("input");
                        ui.progress_bind_dir("flame", S::default().size(14.0, 14.0), abi::ui_dir::UP, |c| {
                            c.menu_component(&SMELTING)
                                .map_or(0.0, |s| ratio(s.burn, s.burn_max))
                        });
                        ui.group("fuel");
                    });
                    ui.recipe_arrow_bind(|c| {
                        c.menu_component(&SMELTING)
                            .map_or(0.0, |s| ratio(s.cook, s.cook_max))
                    });
                    ui.group("output");
                    ui.spacer();
                });
                ui.player_inventory(true);
            });
        });

    r.block(&CHEST)
        .sound_group("wood")
        .facing(Facings::HORIZONTAL, Toward::Player)
        .states(|_, s| {
            s.front_faces("chest_side", "chest_top", "chest_front")
                .model_box([1.0, 0.0, 1.0], [15.0, 14.0, 15.0])
        })
        .hardness(2.5)
        .mined_by("axe", tool_tier::HAND)
        .tab(&crate::tabs::FUNCTIONAL)
        .entity(|e| e.inventory(27))
        .opens(&CHEST_MENU);
    r.menu(&CHEST_MENU)
        .group("chest", Slots::chest(3).holder())
        .default_screen();
}

/// `ticks` of one furnace at once (its area was unloaded, the time jumped): it smelts while
/// input, room and fuel last, lighting fuel as it goes, then the fire burns down and the
/// progress cools, as tick by tick.
fn catch_up(w: &World, id: u64, s: &mut Smelting, ticks: u32) {
    let inv = w.inventory(id);
    let (input_slot, fuel_slot, output_slot) = (inv.group("input"), inv.group("fuel"), inv.group("output"));
    let (input, fuel, output) = (input_slot.get(0), fuel_slot.get(0), output_slot.get(0));
    let mut idle = ticks as u64;
    if let Some((out, time)) = (!input.is_empty())
        .then(|| recipe_process(&crate::recipes::SMELTING, input.item))
        .flatten()
    {
        let q = abi::Query { entity: id, item: input.item, ..query() };
        s.cook_max = w
            .chain_compute(&SMELT_TIME, time as f64, &q)
            .round()
            .max(1.0) as u32;
        let items = (input.count).min(output.room_for(out)) as u64;
        let per_fuel = FUEL.on_stack_item(&fuel).unwrap_or(0) as u64;
        let fuel_items = if per_fuel > 0 { fuel.count as u64 } else { 0 };
        let work = (items * s.cook_max as u64).saturating_sub(s.cook as u64);
        let worked = idle.min(work).min(s.burn as u64 + fuel_items * per_fuel);
        let lit = if worked > s.burn as u64 { (worked - s.burn as u64).div_ceil(per_fuel.max(1)) } else { 0 };
        if lit > 0 {
            let mut left = fuel;
            for _ in 0..lit {
                left = consume_one(left);
            }
            fuel_slot.set(0, left);
            s.burn_max = per_fuel as u32;
        }
        s.burn = (s.burn as u64 + lit * per_fuel - worked) as u32;
        let total = s.cook as u64 + worked;
        let done = (total / s.cook_max as u64) as u32;
        s.cook = (total % s.cook_max as u64) as u32;
        if done > 0 {
            input_slot.set(0, input.shrink(done));
            output_slot.set(0, output.merged(out, done));
            for _ in 0..done {
                w.emit(&SMELTED, Smelted { furnace: id, input: input.item, output: out });
            }
        }
        idle -= worked;
    }
    s.burn = (s.burn as u64).saturating_sub(idle) as u32;
    s.cook = (s.cook as u64).saturating_sub(idle * 2) as u32;
}

/// Every furnace of a region at once, through its state (a system: no host call to reach it).
fn tick_furnaces(b: &mut SystemBatch<Smelting>) {
    let w = b.world();
    for (id, s, ticks) in b.iter_mut() {
        let Some(st) = w.entity(id) else { continue };
        if ticks > 1 {
            let was_lit = s.burn > 0;
            catch_up(&w, id, s, ticks);
            let pos = st.block_pos();
            if (s.burn > 0) != was_lit && FURNACE.is(w.block(pos)) {
                w.prop_set(pos, "lit", if s.burn > 0 { "true" } else { "false" });
            }
            continue;
        }
        let pos = st.block_pos();
        let inv = w.inventory(id);
        let (input_slot, fuel_slot, output_slot) = (inv.group("input"), inv.group("fuel"), inv.group("output"));
        let (input, fuel, output) = (input_slot.get(0), fuel_slot.get(0), output_slot.get(0));
        let was_lit = s.burn > 0;
        if s.burn > 0 {
            s.burn -= 1;
        }
        // Another item in the input starts over, before this tick counts for it.
        if input.item != s.cooking_item {
            s.cooking_item = input.item;
            s.cook = 0;
        }
        let recipe = (!input.is_empty())
            .then(|| recipe_process(&crate::recipes::SMELTING, input.item))
            .flatten();
        let fits = recipe.is_some_and(|(out, _)| output.room_for(out) > 0);
        if s.burn == 0 && fits {
            if let Some(ticks) = FUEL.on_stack_item(&fuel) {
                s.burn = ticks;
                s.burn_max = ticks;
                fuel_slot.set(0, consume_one(fuel));
            }
        }
        if s.burn > 0 && fits {
            let (out, time) = recipe.unwrap();
            let q = abi::Query { entity: id, item: input.item, ..query() };
            s.cook_max = w
                .chain_compute(&SMELT_TIME, time as f64, &q)
                .round()
                .max(1.0) as u32;
            s.cook += 1;
            if s.cook >= s.cook_max {
                s.cook = 0;
                input_slot.set(0, input.shrink(1));
                output_slot.set(0, output.merged(out, 1));
                w.emit(&SMELTED, Smelted { furnace: id, input: input.item, output: out });
            }
        } else if s.burn > 0 {
            // Modern's: burning with nothing it can do starts over; out of fuel it cools down.
            s.cook = 0;
        } else {
            s.cook = s.cook.saturating_sub(2);
        }
        let lit = s.burn > 0;
        if lit != was_lit && FURNACE.is(w.block(pos)) {
            w.prop_set(pos, "lit", if lit { "true" } else { "false" });
        }
    }
}
