//! Something to ride: a steed with a saddle that steers and a pillion seat that doesn't. It
//! walks where its rider looks and jumps higher the longer jump is held; its falls hurt its
//! riders, and nobody builds from its back.

use weft_sdk::*;

pub static STEED: EntityKind = EntityKind::named("steed");

pub fn register(r: &mut Registry) {
    r.entity(&STEED)
        .size(0.9, 1.0)
        .walk_speed(4.0)
        .max_health(30.0)
        .seat(Seat::at([0.0, 1.0, 0.1]).steers())
        .seat(Seat::at([0.0, 1.0, -0.5]).exit(Exit::offset([0.0, 0.0, -1.5])))
        .steering(steer::walk_look(1.6).and(steer::charged_jump(12.0, 10)))
        .rider_rules(RiderRules::new().builds(|_, _, _| false))
        .riders_fall()
        .rideable()
        .model(quadruped([10.0, 8.0, 18.0], 8.0, [6.0, 6.0, 7.0]));
    // Base's leather; testmod adds its tin to this table from outside (`r.edit(STEED.loot())`).
    r.loot(&STEED).pool(Pool::new().items("base:leather", 0, 2));
}
