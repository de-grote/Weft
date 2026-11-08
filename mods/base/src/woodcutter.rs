//! The woodcutter: a block put in is cut into smaller pieces of it with nothing lost, every way at
//! once to pick from. Recipes of `WOODCUTTING`, one input, its first output what is shown (a block
//! cut into stairs also gives the beam left over).

use weft_sdk::*;

pub use base_api::WOODCUTTING;

pub static WOODCUTTER: Block = Block::named("woodcutter");
static MENU: Menu = Menu::named("woodcutter");

pub fn register(r: &mut Registry) {
    r.block(&WOODCUTTER)
        .sound_group("wood")
        .facing(Facings::HORIZONTAL, Toward::Player)
        .states(|_, s| {
            let blade = [("iron_block", abi::UV_FROM_BOX); 6];
            s.texture("planks_oak")
                .model_box([0.0; 3], [16.0, 9.0, 16.0])
                .model_box_faces([1.0, 9.0, 7.5], [15.0, 16.0, 8.5], blade)
        })
        .hitbox([0.0; 3], [16.0, 9.0, 16.0])
        .hardness(2.5)
        .mined_by("axe", tool_tier::HAND)
        .opens(&MENU)
        .tab(&crate::tabs::FUNCTIONAL);
    r.menu(&MENU)
        .group("input", Slots::single())
        .group("results", Slots::grid(3, 2).output())
        .player_inventory()
        .recipes(&WOODCUTTING)
        .on_change(offer)
        .on_take(cut)
        .screen(|ui| {
            ui.panel("panel", S::column().pad(7.0).gap(4.0), |ui| {
                ui.title();
                ui.row(S::row().gap(6.0), |ui| {
                    ui.group("input");
                    ui.recipe_arrow();
                    ui.group("results");
                });
                ui.player_inventory(true);
            });
        });
    r.recipe_kind(&WOODCUTTING).inputs(1, 1).outputs(1, 2);
    // A whole block is 8 eighths: a slab or a vertical slab 4, stairs 6, a beam 2.
    r.shapeless(&WOODCUTTING, "#stairs/{m}", 1, &["#stairs_of/{m}"], 0)
        .output("#beam/{m}", 1);
    let cuts = [
        ("#slab_of/{m}", "#slab/{m}", 2),
        ("#vertical_slab_of/{m}", "#vertical_slab/{m}", 2),
        ("#beam_of/{m}", "#beam/{m}", 4),
        ("#eighth_of/{m}", "#eighth/{m}", 8),
        ("#slab/{m}", "#beam/{m}", 2),
        ("#slab/{m}", "#eighth/{m}", 4),
        ("#vertical_slab/{m}", "#beam/{m}", 2),
        ("#vertical_slab/{m}", "#eighth/{m}", 4),
        ("#stairs/{m}", "#beam/{m}", 3),
        ("#stairs/{m}", "#eighth/{m}", 6),
        ("#beam/{m}", "#eighth/{m}", 2),
    ];
    for (from, into, count) in cuts {
        r.shapeless(&WOODCUTTING, into, count, &[from], 0);
    }
}

/// What the block in the input can be cut into, a slot each.
fn offer(m: &MenuCtx) {
    let input = m.group("input").get(0);
    let results = m.group("results");
    let mut found = recipes_for(&WOODCUTTING, std::slice::from_ref(&input));
    for i in 0..results.len() {
        let out = found
            .next()
            .and_then(|r| r.outputs().first().map(|o| o.0))
            .unwrap_or_default();
        results.set(i, out);
    }
}

/// A result taken: one of the input goes, and what else the cut gives goes to the player.
fn cut(m: &MenuCtx) {
    let Some(i) = m.taken_in("results") else { return };
    let input = m.group("input");
    let mut stacks = [input.get(0)];
    let Some(recipe) = recipes_for(&WOODCUTTING, &stacks).nth(i as usize) else { return };
    if !recipe.consume(&mut stacks) {
        return;
    }
    input.set(0, stacks[0]);
    let w = m.world();
    for (extra, _) in recipe.outputs().into_iter().skip(1) {
        let left = w.inventory(m.player()).insert(extra);
        if !left.is_empty()
            && let Some(e) = w.entity(m.player())
        {
            w.spawn_item(e.pos, left);
        }
    }
}
