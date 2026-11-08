//! The base game. It is an ordinary mod: everything here goes through `weft_sdk`, nothing in the
//! engine knows about stone, water or zombies.

mod advancements;
mod beds;
mod block_entities;
mod blocks;
mod commands;
mod items;
mod mobs;
mod player;
mod recipes;
mod redstone;
mod sounds;
mod woodcutter;
mod worldgen;

use weft_sdk::*;

pub use base_api::{CLOUDS, FLATLANDS, MOON, OVERWORLD, STARS, SUN, tabs};

declare_mod!(deps: [], init: init);

fn init(r: &mut Registry) {
    // Vanilla's creative tabs; blocks and items join them with `.tab(..)`.
    for (tab, icon) in [
        (&tabs::BUILDING, &Item::named("bricks")),
        (&tabs::COLORED, &Item::named("red_stained_glass")),
        (&tabs::NATURAL, &Item::named("grass")),
        (&tabs::FUNCTIONAL, &Item::named("crafting_table")),
        (&tabs::TOOLS, &Item::named("iron_pickaxe")),
        (&tabs::COMBAT, &Item::named("iron_sword")),
        (&tabs::FOOD, &Item::named("apple")),
        (&tabs::INGREDIENTS, &items::IRON_INGOT),
        (&tabs::REDSTONE, &Item::named("redstone")),
    ] {
        r.creative_tab(tab, icon);
    }
    blocks::register(r);
    commands::register(r);
    items::register(r);
    player::register(r);
    block_entities::register(r);
    recipes::register(r);
    sounds::register(r);
    mobs::register(r);
    worldgen::register(r);
    advancements::register(r);
    beds::register(r);
    redstone::register(r);
    woodcutter::register(r);
    // Its terrain is described in `worldgen`.
    r.dimension(&OVERWORLD)
        .player_spawn()
        .sky_above(16)
        .day_length(24000)
        .weathers(&[&weathers::RAIN, &weathers::THUNDER]);
    // Second dimension on purpose: nothing in the engine may assume there is only one world.
    r.dimension(&FLATLANDS)
        .generator(worldgen::flat)
        .bare()
        .spawn_point(|_| [0, 3, 0].into())
        .height_cubes(-2, 8)
        .sky_above(1)
        .fixed_time(6000)
        .gravity(20.0)
        .sky_color(0xb0c8ff, 0xe0e8ff);
    for dim in [&OVERWORLD, &FLATLANDS] {
        r.sky_stars(dim, &STARS).count(1500);
        r.sky_clouds(dim, &CLOUDS).texture("clouds");
        r.sky_body(dim, &SUN)
            .texture("sun")
            .blend_add()
            .size(33.0)
            .glow(0xFFE633);
        r.sky_body(dim, &MOON)
            .texture("moon_phases")
            .blend_add()
            .frames(4, 2)
            .size(23.0)
            .phase(0.5);
    }
}
