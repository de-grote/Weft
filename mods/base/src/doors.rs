//! Doors: one block two cells tall, opened by a click on either or by power.

use weft_sdk::*;

pub static OAK_DOOR: Block = Block::named("oak_door");

pub fn register(r: &mut Registry) {
    r.block(&OAK_DOOR)
        .sound_group("wood")
        .facing(Facings::HORIZONTAL, Toward::Player)
        .prop("open", ["false", "true"])
        .cells(&[[0, 1, 0]])
        .states(|p, s| {
            // Shut across the side toward the player, open along its left. One picture for the
            // whole door, 16 by 32 (`door_oak`), which is the left half of the square it is put in.
            let (min, max) = if p.is("open", "true") { ([0.0; 3], [3.0, 32.0, 16.0]) } else { ([0.0; 3], [16.0, 32.0, 3.0]) };
            let size = [max[0] - min[0], max[1], max[2] - min[2]];
            // Across and up of a face by its axis (x faces: z by y, y faces: x by z, z faces: x by
            // y), half a unit of the square a pixel.
            let face = |f: usize| {
                let (w, h) = [(size[2], size[1]), (size[0], size[2]), (size[0], size[1])][f / 2];
                ("door_oak", [0.0, 0.0, w / 2.0, h / 2.0])
            };
            s.cutout()
                .model_box_faces(min, max, std::array::from_fn(face))
        })
        .needs_below("")
        .hardness(3.0)
        .mined_by("axe", tool_tier::HAND)
        .on_use(|u| {
            u.world().prop_cycle(u.pos(), "open");
            true
        })
        .signal_sink()
        .on_signal(|e| {
            let open = if e.level() > 0 { "true" } else { "false" };
            e.world().prop_set(e.pos(), "open", open);
        })
        .tab(&crate::tabs::REDSTONE);
    r.shaped(&base_api::CRAFTING, "oak_door", 3, &["pp", "pp", "pp"], &[('p', "oak_planks")]);
}
