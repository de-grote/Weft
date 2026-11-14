//! The test list. Each test is an `async fn(T) -> R`; failed checks are collected, `?` stops it.

mod audit;
mod bodies;
mod extend;
mod gameplay;
mod looks;
mod probe;
pub mod redstone;
mod scene;
mod shields;
mod sync;
mod ui;
mod visual;
mod world;

use std::time::Duration;

use crate::runner::{R, T, TestDef};

pub fn add(p: [i32; 3], d: [i32; 3]) -> [i32; 3] {
    [p[0] + d[0], p[1] + d[1], p[2] + d[2]]
}

pub fn at(p: [i32; 3]) -> String {
    format!("{} {} {}", p[0], p[1], p[2])
}

pub fn all() -> Vec<TestDef> {
    crate::tests![
        scene::scene,
        ui::screens,
        ui::rebind,
        ui::f3,
        ui::hud_editor,
        ui::debug_lines,
        ui::inventory_crafting,
        ui::target_info,
        ui::creative_inventory,
        ui::chat_overflow,
        ui::text_on_screen,
        ui::mod_settings,
        ui::recipe_viewer,
        ui::inventory_hotkeys,
        world::persistence,
        world::missing_dimension,
        world::dimensions,
        world::world_seed,
        world::camera,
        world::big_blocks,
        world::lod_columns,
        world::entity_modify,
        world::edits,
        world::virtual_menus,
        world::worldgen,
        world::generation_reads,
        world::plants_on_cube_tops,
        world::foreign_terrain,
        world::tick_control,
        world::growth,
        world::catch_up,
        world::crater_saved,
        world::edit_while_loading,
        world::gpu_terrain,
        gameplay::sprint_and_sneak_modes,
        gameplay::mining_and_wear,
        gameplay::creative_and_invulnerable,
        gameplay::block_shapes,
        gameplay::cell_parts,
        gameplay::hitboxes,
        gameplay::replaced_plants_drop,
        gameplay::placing_ahead,
        gameplay::hoe_tills,
        gameplay::inventory_tools,
        gameplay::swimming,
        gameplay::attack_charge,
        gameplay::projectiles,
        extend::steps_in_phases,
        gameplay::bow_and_keys,
        shields::shields,
        gameplay::permissions,
        gameplay::armor,
        gameplay::slot_accepts,
        gameplay::liquid_slot,
        gameplay::slot_limit,
        gameplay::bucket,
        gameplay::advancements,
        gameplay::beds,
        gameplay::second_hand,
        gameplay::model_bones,
        gameplay::beds_far,
        gameplay::dedicated_server,
        gameplay::base_alone,
        gameplay::names_translated,
        gameplay::burning,
        gameplay::entity_fields,
        gameplay::fall_damage,
        gameplay::explosions_and_water,
        gameplay::sneak_edge,
        gameplay::movement_attributes,
        gameplay::mob_physics,
        gameplay::ladder_sides,
        gameplay::player_box,
        gameplay::food,
        gameplay::drop_and_pickup,
        extend::events_batched,
        extend::vetoes_and_timers,
        extend::status_effects,
        extend::block_properties,
        sync::menu_sync,
        sync::displayed_stacks,
        visual::liquid_layers,
        visual::break_overlay,
        visual::tooltips,
        visual::night_light,
        visual::moving_light,
        visual::multiplayer,
        visual::client_hooks,
        audit::saved_entities_stay_gone,
        audit::death_is_a_state,
        audit::reach_and_mining_attributes,
        looks::environment_sources,
        looks::sky,
        looks::clouds,
        looks::light_by_day,
        looks::ctm_any_model,
        redstone::redstone_wire,
        redstone::redstone_on_walls,
        redstone::redstone_conduction,
        redstone::redstone_torch,
        redstone::redstone_repeater,
        redstone::redstone_comparator,
        redstone::redstone_gate,
        redstone::redstone_piston,
        redstone::redstone_piston_moves,
        redstone::redstone_door,
        redstone::redstone_contest,
        redstone::redstone_rides,
        redstone::redstone_body_edges,
        redstone::redstone_floaters_together,
        redstone::redstone_floater_riders,
        redstone::redstone_floater,
        redstone::redstone_body_light,
        redstone::redstone_body_and_world,
        redstone::redstone_moves_chest,
        redstone::redstone_timing,
        redstone::redstone_anchor,
        redstone::redstone_flying_machine,
        redstone::redstone_by_hand,
        redstone::redstone_looks,
        redstone::redstone_clock,
        redstone::redstone_saved,
        redstone::redstone_on_a_structure,
        redstone::redstone_turned,
        looks::shaders_and_glow,
        looks::texture_reload,
        looks::pack_lang,
        looks::rich_text,
        looks::models,
        bodies::lift,
        bodies::ride_frames,
        bodies::skiff_smooth,
        gameplay::respawn,
        bodies::skiff_blocks,
        bodies::body_chest,
        bodies::body_sneak_edge,
        bodies::body_place,
        redstone::body_walking,
        bodies::zombie_chases,
        bodies::neutral_mobs,
    ]
}

/// Measurements without a verdict worth failing a run over; `--test bench`.
pub fn bench() -> Vec<TestDef> {
    crate::tests![
        redstone::redstone_bench,
        world::fly_load,
        world::load_benchmark,
        world::item_farm,
        world::mob_crowd,
        looks::particle_crowd,
        looks::layered_icon,
        looks::particle_rules,
        world::crater
    ]
}

/// A process's first world is slow to open and to settle, which timings shouldn't be measuring.
/// Left out of the report unless it fails.
pub async fn warm_up(t: T) -> R {
    t.flat().await?;
    t.secs(0.5).await
}

impl T {
    /// A fresh world moved to the flat test dimension, survival, at noon, loaded and idle.
    pub async fn flat(&self) -> R {
        self.world(1).await?;
        self.cmd("/dim base:flatlands").await?;
        self.until("flatlands", Duration::from_secs(20), |t| t.state().dim != 0 || t.state().pos.y < 10.0)
            .await?;
        self.loaded().await?;
        self.op("mode survival").await?;
        self.idle().await
    }

    /// Turns the local player to look at the middle of block `p` from its eyes.
    pub fn look_at(&self, p: [i32; 3]) {
        let s = self.state();
        let eye = [s.pos.x, s.pos.y + self.c().entity(0).map_or(1.62, |e| e.size.eye as f64), s.pos.z];
        let d = [p[0] as f64 + 0.5 - eye[0], p[1] as f64 + 0.5 - eye[1], p[2] as f64 + 0.5 - eye[2]];
        let yaw = (-d[0]).atan2(-d[2]).to_degrees() as f32;
        let pitch = d[1].atan2((d[0] * d[0] + d[2] * d[2]).sqrt()).to_degrees() as f32;
        self.c().set_look(yaw, pitch);
    }

    /// Counts entities of a kind near the player as the server has them.
    pub async fn count_near(&self, kind: &str, r: f64) -> Result<usize, String> {
        let list = self.op(&format!("near ~ ~ ~ {r}")).await?;
        Ok(list
            .split_whitespace()
            .filter(|e| e.ends_with(&format!(":{kind}")))
            .count())
    }
}
