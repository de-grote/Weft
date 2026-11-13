//! Mobs: zombies and spiders at night or in the dark, pigs and cows on grass by day.

use weft_sdk::ai;
use weft_sdk::*;

pub static ZOMBIE: EntityKind = EntityKind::named("zombie");
pub static SPIDER: EntityKind = EntityKind::named("spider");
pub static PIG: EntityKind = EntityKind::named("pig");
pub static SKELETON: EntityKind = EntityKind::named("skeleton");
pub static SHEEP: EntityKind = EntityKind::named("sheep");
pub static CHICKEN: EntityKind = EntityKind::named("chicken");
pub static ARROW: EntityKind = EntityKind::named("arrow");
pub static BOW: Item = Item::named("bow");
static ARROW_ITEM: Item = Item::named("arrow");
static EGG: Item = Item::named("egg");
/// Ticks of whatever a mob counts: a skeleton's reload, a chicken's next egg.
static FUSE: Component<u32> = Component::named("fuse");

pub static COW: EntityKind = EntityKind::named("cow");

pub fn register(r: &mut Registry) {
    // The engine's player kind, drawn as the same humanoid as the zombie.
    r.set_model(&PLAYER, humanoid(false));
    r.edit(&PLAYER).hitboxes(Hitboxes::humanoid());
    r.entity(&ZOMBIE)
        .hitboxes(Hitboxes::humanoid().reaching())
        .tag("hostile")
        // Undead, as vanilla's: poison and regeneration take no hold.
        .tag("immune_effect/base:poison")
        .tag("immune_effect/base:regeneration")
        .equipment()
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(0.6, 1.95)
        .max_health(20.0)
        .walk_speed(2.6)
        .hostile()
        .burns_in_sun()
        .attack_damage(3.0)
        .reach(1.3)
        .goals(hostile(ai::Goals::new().aggressive(35.0)));
    r.entity(&SPIDER)
        .hitboxes(spider_parts())
        .tag("hostile")
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(1.3, 0.9)
        .max_health(16.0)
        .walk_speed(3.6)
        .hostile()
        .attack_damage(2.0)
        // Vanilla's reach goes by a mob's width: a body this wide stops further off.
        .reach(2.4)
        // In daylight it only fights back.
        .goals(hostile(
            ai::Goals::new()
                .react(ai::Trigger::sight(35.0).only_if(|w, st| !in_daylight(w, st)), ai::react::anger(2.0, 1))
                .neutral(),
        ));
    r.entity_component(&FUSE, 0u32, 0);
    r.entity(&SKELETON)
        .hitboxes(Hitboxes::humanoid().slim(2.0).reaching())
        .tag("hostile")
        .tag("immune_effect/base:poison")
        .tag("immune_effect/base:regeneration")
        .equipment()
        .size(0.6, 1.99)
        .max_health(20.0)
        .walk_speed(2.5)
        .hostile()
        .burns_in_sun()
        .attach(&FUSE)
        .goals(skeleton());
    r.on_event(&actions::SPAWN, |batch| {
        for spawned in batch.iter() {
            if spawned.kind == SKELETON.id() {
                batch
                    .world()
                    .inventory(spawned.entity)
                    .set(0, ItemStack::new(BOW.id(), 1));
            }
        }
    });
    // Drawn as long as the use button is held, shot when it is let go.
    r.item(&BOW)
        .durability(384)
        .handheld()
        .use_until_released()
        .on_release(shoot_bow)
        .tab(&crate::tabs::COMBAT);
    // An arrow in the head hurts half as much again, a bow's critical hit: `part` is the part
    // of the hitbox it went into.
    r.modify(&chain::PROJECTILE_DAMAGE, &phase::MULTIPLY, |m| {
        let q = *m.query();
        let arrow = m
            .world()
            .entity(q.entity)
            .is_some_and(|e| e.kind == ARROW.id());
        if arrow && q.part == hitbox_part::HEAD {
            m.value *= 2.0;
        }
    });
    // Drawing it narrows the view, more the further it is drawn (vanilla's 15% at full draw).
    r.modify(&attribute::FOV, &phase::MULTIPLY, |m| {
        let Some(s) = m.world().entity(m.query().entity) else { return };
        if s.using == BOW.id() {
            let drawn = (s.using_ticks as f64 / 20.0).min(1.0);
            m.value *= 1.0 - drawn * drawn * 0.15;
        }
    });
    r.shaped(&crate::recipes::CRAFTING, "bow", 1, &[" s#", "s #", " s#"], &[('s', "#stick"), ('#', "#string")]);
    // Vanilla's arrow: 0.05 a tick down, stuck a minute where it hits a block.
    r.entity(&ARROW)
        .no_egg()
        .size(0.5, 0.5)
        .unpushable()
        .max_health(1.0)
        .walk_speed(0.0)
        .attribute(&attribute::GRAVITY, 0.625)
        .projectile(Projectile::new().sticks(1200));
    r.entity(&SHEEP)
        .hitboxes(sheep_parts())
        .tag("livestock")
        .size(0.9, 1.3)
        .max_health(8.0)
        .walk_speed(2.3)
        .passive()
        .goals(passive());
    r.entity(&CHICKEN)
        .hitboxes(chicken_parts())
        .tag("livestock")
        .size(0.4, 0.7)
        .max_health(4.0)
        .walk_speed(2.5)
        .passive()
        .attach(&FUSE)
        .goals(passive_chicken());
    r.entity(&PIG)
        .hitboxes(pig_parts())
        .tag("livestock")
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(0.9, 0.9)
        .max_health(10.0)
        .walk_speed(2.5)
        .passive()
        .goals(passive());
    r.entity(&COW)
        .hitboxes(Hitboxes::quadruped())
        .tag("livestock")
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(0.9, 1.3)
        .max_health(10.0)
        .walk_speed(2.2)
        .passive()
        .goals(passive());
    // Vanilla amounts; meat comes out cooked when the animal burns.
    r.loot(&ZOMBIE)
        .pool(Pool::new().items("rotten_flesh", 0, 2));
    r.loot(&SPIDER)
        .pool(Pool::new().items("string", 0, 2))
        .pool(
            Pool::new()
                .when(Cond::killed_by_player())
                .when(Cond::chance(0.33))
                .item("spider_eye"),
        );
    r.loot(&SKELETON)
        .pool(Pool::new().items("arrow", 0, 2))
        .pool(Pool::new().items("bone", 0, 2));
    r.loot(&SHEEP).pool(Pool::new().item("wool")).pool(
        Pool::new().entry(
            Entry::item("mutton")
                .count(1, 2)
                .smelted(&crate::recipes::SMELTING),
        ),
    );
    r.loot(&CHICKEN)
        .pool(Pool::new().items("feather", 0, 2))
        .pool(
            Pool::new().entry(
                Entry::item("chicken")
                    .count(1, 1)
                    .smelted(&crate::recipes::SMELTING),
            ),
        );
    r.loot(&PIG).pool(
        Pool::new().entry(
            Entry::item("porkchop")
                .count(1, 3)
                .smelted(&crate::recipes::SMELTING),
        ),
    );
    r.loot(&COW)
        .pool(
            Pool::new().entry(
                Entry::item("beef")
                    .count(1, 3)
                    .smelted(&crate::recipes::SMELTING),
            ),
        )
        .pool(Pool::new().items("leather", 0, 2));
    // Natural spawning, as rules the engine runs: monsters in the dark (spiders more in deep
    // caves), animals in small herds on grass by day, rarely once the world is loaded.
    r.natural_spawn(&ZOMBIE).light(0, 6).weight(2);
    r.natural_spawn(&SKELETON).light(0, 6).weight(2);
    r.natural_spawn(&SPIDER).light(0, 6);
    r.natural_spawn(&SPIDER)
        .light(0, 6)
        .biome("#cave")
        .weight(4);
    for animal in [&PIG, &COW, &SHEEP, &CHICKEN] {
        r.natural_spawn(animal)
            .biome("#animals")
            .ground("#animal_spawn")
            .light(7, 15)
            .pack_size(2, 3)
            .chance(0.05);
    }
}

/// Vanilla's skeleton: keeps to about 10 blocks and shoots once in two seconds.
fn skeleton() -> ai::Goals {
    ai::Goals::new()
        .aggressive(16.0)
        .then(ai::watch_player(16.0))
        .then(|w: &World, id: u64, st: &EntityState| {
            let Some((_, p)) = ai::target(w, id, st, 15.0) else { return false };
            let Some(reload) = w.component_mut(id, &FUSE) else { return false };
            *reload = (*reload + 1) % 40;
            if *reload == 0 {
                shoot(w, id, st, &p);
            }
            let d2 = (p.pos.x - st.pos.x).powi(2) + (p.pos.z - st.pos.z).powi(2);
            if d2 > 100.0 {
                w.navigate(id, p.pos, 1.0);
            }
            true
        })
        .then(ai::chase_target(16.0, 1.0))
        .then(ai::wander(0.6, 10.0))
}

/// Vanilla's bow: full strength after a second's draw, the curve easing in; an arrow from the
/// inventory, none taken in creative.
fn shoot_bow(u: &mut ItemUse) -> bool {
    let (w, id) = (u.world(), u.entity());
    let t = u.held_ticks() as f32 / 20.0;
    let power = ((t * t + t * 2.0) / 3.0).min(1.0) as f64;
    if power < 0.1 {
        return false;
    }
    let creative = w.creative(id);
    let inv = w.inventory(id);
    let arrow = (0..inv.len()).find(|&s| inv.get(s).item == ARROW_ITEM.id());
    if arrow.is_none() && !creative {
        return false;
    }
    let Some(s) = w.entity(id) else { return false };
    // Three blocks a tick at full draw.
    let v = look_dir(s.yaw, s.pitch) * (power * 3.0 * 20.0);
    w.spawn_with(
        &ARROW,
        s.eye(),
        &Spawn::new()
            .owner(id)
            .velocity(v)
            .inherit_velocity()
            .yaw(s.yaw),
    );
    if !creative {
        if let Some(slot) = arrow {
            inv.set(slot, consume_one(inv.get(slot)));
        }
        u.set_stack(u.stack().add_wear(1));
    }
    true
}

fn shoot(w: &World, id: u64, from: &EntityState, at: &EntityState) {
    let eye = from.eye().y;
    let d = [at.pos.x - from.pos.x, at.pos.y + at.size.eye as f64 * 0.7 - eye, at.pos.z - from.pos.z];
    let flat = (d[0] * d[0] + d[2] * d[2]).sqrt();
    let len = (flat * flat + d[1] * d[1]).sqrt().max(0.1);
    // A little above the target for the drop on the way.
    const SPEED: f64 = 32.0;
    let v = [d[0] / len * SPEED, d[1] / len * SPEED + flat * 0.4, d[2] / len * SPEED];
    w.spawn_with(
        &ARROW,
        [from.pos.x, eye, from.pos.z],
        &Spawn::new()
            .owner(id)
            .velocity(v)
            .inherit_velocity()
            .yaw(yaw_toward([0.0; 3], d)),
    );
}

/// A chicken: flutters down instead of falling, and lays an egg now and then.
fn passive_chicken() -> ai::Goals {
    let goals = ai::Goals::new().then(|w: &World, id: u64, st: &EntityState| {
        if st.on_ground == 0 && st.vel.y < 0.0 {
            w.entity_modify(id)
                .velocity(Vec3 { y: st.vel.y * 0.6, ..st.vel });
        }
        if let Some(next) = w.component_mut(id, &FUSE) {
            if *next == 0 {
                *next = 6000 + w.random_below(6000) as u32;
            } else {
                *next -= 1;
                if *next == 0 {
                    w.spawn_item(st.pos, ItemStack::new(EGG.id(), 1));
                }
            }
        }
        false
    });
    with_passive(goals)
}

/// Monsters chase the player they are after (what `goals` sets them off by) and hit it when in
/// reach (`ai::melee`, once a second). Otherwise they wander.
fn hostile(goals: ai::Goals) -> ai::Goals {
    goals
        .then(ai::watch_player(8.0))
        .then(ai::melee())
        .then(ai::chase_target(35.0, 1.0))
        .then(ai::wander(0.6, 10.0))
}

/// A spider's peace: out in daylight.
fn in_daylight(w: &World, st: &EntityState) -> bool {
    let feet = [st.pos.x.floor() as i32, (st.pos.y + 0.5).floor() as i32, st.pos.z.floor() as i32];
    w.daylight() >= 0.5 && w.sky_light(feet) >= 10
}

/// Animals run about for three seconds after being hurt, otherwise wander.
fn passive() -> ai::Goals {
    with_passive(ai::Goals::new())
}

fn with_passive(goals: ai::Goals) -> ai::Goals {
    goals
        .timid(3.0)
        .then(ai::watch_player(6.0))
        .then(ai::panic(1.6))
        .then(ai::wander(0.5, 10.0))
}

// Where each of base's mobs is hit, part by part, in the pixels of its model: what its body
// looks like, not the model itself (`Hitboxes`).
const PI: f32 = std::f32::consts::PI;

fn four_legs(h: Hitboxes, x: [f32; 2], tall: f32, front: [f32; 2], back: [f32; 2]) -> Hitboxes {
    let mid = |a: [f32; 2]| (a[0] + a[1]) / 2.0;
    let (xm, fz, bz) = (mid(x), mid(front), mid(back));
    h.limb("front_right_leg", [-x[1], 0.0, front[0]], [-x[0], tall, front[1]], [-xm, tall, fz], 0.8, PI)
        .limb("front_left_leg", [x[0], 0.0, front[0]], [x[1], tall, front[1]], [xm, tall, fz], 0.8, 0.0)
        .limb("back_right_leg", [-x[1], 0.0, back[0]], [-x[0], tall, back[1]], [-xm, tall, bz], 0.8, 0.0)
        .limb("back_left_leg", [x[0], 0.0, back[0]], [x[1], tall, back[1]], [xm, tall, bz], 0.8, PI)
}

fn sheep_parts() -> Hitboxes {
    let h = Hitboxes::new()
        .head("head", [-4.0, 14.0, -14.0], [4.0, 22.0, -6.0], [0.0, 18.0, -8.0])
        .part("body", [-4.5, 10.0, -8.0], [4.5, 18.0, 10.0]);
    four_legs(h, [1.0, 5.0], 12.0, [-7.0, -3.0], [5.0, 9.0])
}

fn pig_parts() -> Hitboxes {
    let h = Hitboxes::new()
        .head("head", [-4.0, 8.0, -15.0], [4.0, 16.0, -6.0], [0.0, 12.0, -6.0])
        .part("body", [-5.0, 6.0, -8.0], [5.0, 14.0, 9.0]);
    four_legs(h, [1.0, 5.0], 6.0, [-7.0, -3.0], [5.0, 9.0])
}

fn chicken_parts() -> Hitboxes {
    Hitboxes::new()
        .head("head", [-2.0, 9.0, -8.0], [2.0, 15.0, -3.0], [0.0, 9.0, -4.0])
        .part("body", [-3.0, 5.0, -4.0], [3.0, 11.0, 4.0])
        .part("right_wing", [-4.0, 7.0, -3.0], [-3.0, 11.0, 3.0])
        .part("left_wing", [3.0, 7.0, -3.0], [4.0, 11.0, 3.0])
        .limb("right_leg", [-2.5, 0.0, -1.0], [-1.5, 5.0, 1.0], [-2.0, 5.0, 0.0], 0.8, 0.0)
        .limb("left_leg", [0.5, 0.0, -1.0], [1.5, 5.0, 1.0], [1.0, 5.0, 0.0], 0.8, PI)
}

fn spider_parts() -> Hitboxes {
    Hitboxes::new()
        .head("head", [-4.0, 5.0, -11.0], [4.0, 13.0, -3.0], [0.0, 9.0, -3.0])
        .part("body", [-3.0, 6.0, -3.0], [3.0, 12.0, 3.0])
        .part("abdomen", [-5.0, 5.0, 3.0], [5.0, 13.0, 15.0])
        .part("right_legs", [-15.0, 0.0, -4.0], [-3.0, 9.0, 4.0])
        .part("left_legs", [3.0, 0.0, -4.0], [15.0, 9.0, 4.0])
}
