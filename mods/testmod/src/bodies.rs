//! Moving collision demos: a lift going up and down and a skiff flying back and forth fast
//! enough that jumping on it only works because riders move with it, both moving structures of
//! planks that blocks can be added to and broken off on the way, and a crate that falls and can
//! be stood on.

use weft_sdk::*;

static LIFT: EntityKind = EntityKind::named("lift");
static SKIFF: EntityKind = EntityKind::named("skiff");
static CRATE: EntityKind = EntityKind::named("crate");
static LIFT_DECK: Template = Template::named("lift_deck");
static SKIFF_DECK: Template = Template::named("skiff_deck");

pub fn register(r: &mut Registry) {
    // What the moving structures are built of, checked when the game loads.
    r.template(&LIFT_DECK)
        .fill("base:oak_planks", [0, 0, 0], [1, 0, 1]);
    r.template(&SKIFF_DECK)
        .fill("base:oak_planks", [-1, 0, -1], [1, 0, 1]);
    // 4 blocks up in 4 seconds, then down again.
    r.entity(&LIFT)
        .no_egg()
        .max_health(100.0)
        .invulnerable()
        .solid()
        .kinematic()
        .body(&LIFT_DECK)
        .on_tick(|t| {
            let w = t.world();
            for &id in t.ids() {
                let Some(st) = w.entity(id) else { continue };
                w.entity_modify(id)
                    .velocity([0.0, if (st.age / 80) % 2 == 0 { 1.0 } else { -1.0 }, 0.0]);
            }
        });
    // 6 blocks a second along x for 3 seconds each way; riders stay with it up to 3 blocks up.
    r.entity(&SKIFF)
        .no_egg()
        .max_health(100.0)
        .invulnerable()
        .solid()
        .kinematic()
        .carry_height(3.0)
        .body(&SKIFF_DECK)
        .on_tick(|t| {
            let w = t.world();
            for &id in t.ids() {
                let Some(st) = w.entity(id) else { continue };
                w.entity_modify(id)
                    .velocity([if (st.age / 60) % 2 == 0 { 6.0 } else { -6.0 }, 0.0, 0.0]);
            }
        });
    r.entity(&CRATE)
        .size(1.0, 1.0)
        .max_health(10.0)
        .invulnerable()
        .solid()
        .texture("bodies")
        .model(
            Model::from(vec![ModelPart::new("crate", [0.0; 3]).cuboid([-8.0, 0.0, -8.0], [16.0, 16.0, 16.0], [0.0, 24.0])]).uv_size(64, 64),
        );
}
