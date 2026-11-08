//! Mobs: zombies and spiders at night or in the dark, pigs and cows on grass by day.

use weft_sdk::abi::MeshVertex;
use weft_sdk::ai;
use weft_sdk::*;

pub static ZOMBIE: EntityKind = EntityKind::named("zombie");
pub static SPIDER: EntityKind = EntityKind::named("spider");
pub static PIG: EntityKind = EntityKind::named("pig");
pub static CREEPER: EntityKind = EntityKind::named("creeper");
pub static SKELETON: EntityKind = EntityKind::named("skeleton");
pub static SHEEP: EntityKind = EntityKind::named("sheep");
pub static CHICKEN: EntityKind = EntityKind::named("chicken");
pub static ARROW: EntityKind = EntityKind::named("arrow");
static EGG: Item = Item::named("egg");
/// Ticks of whatever a mob counts: a creeper's fuse, a skeleton's reload, a chicken's next egg.
static FUSE: Component<u32> = Component::named("fuse");
static SHOOTER: Component<u64> = Component::named("shooter");

/// A part of a model written as vanilla's are, y down from 24, so their textures fit.
fn vanilla_part(name: &str, pivot: [f32; 3], boxes: &[([f32; 3], [f32; 3], [f32; 2])]) -> ModelPart {
    let mut part = ModelPart::new(name, [pivot[0], 24.0 - pivot[1], pivot[2]]);
    for &(at, size, uv) in boxes {
        part = part.cuboid([at[0], -(at[1] + size[1]), at[2]], size, uv);
    }
    part
}

/// Legs as vanilla's, hanging from `y` (y down), back ones at z 7, front ones at z -5.
fn vanilla_legs(x: f32, y: f32) -> [ModelPart; 4] {
    let height = 24.0 - y;
    let leg = |name, px, pz| vanilla_part(name, [px, y, pz], &[([-2.0, 0.0, -2.0], [4.0, height, 4.0], [0.0, 16.0])]);
    [leg("leg0", -x, 7.0), leg("leg1", x, 7.0), leg("leg2", x, -5.0), leg("leg3", -x, -5.0)]
}
fn polygon(part: ModelPart, size: [f32; 2], uv: [f32; 4], two_sided: bool) -> ModelPart {
    let [x, y, z] = part.pivot;
    let [width, height] = size;
    let [u, v, uv_width, uv_height] = uv;
    let a = MeshVertex { pos: [x, y, z], uv: [u, v] };
    let b = MeshVertex { pos: [x + width, y, z], uv: [u + uv_width, v] };
    let c = MeshVertex { pos: [x + width, y + height, z], uv: [u + uv_width, v + uv_height] };
    let d = MeshVertex { pos: [x, y + height, z], uv: [u, v + uv_height] };
    if two_sided {
        part.mesh(vec![a, b, c, a, c, d])
            .mesh(vec![a, c, b, a, d, c])
    } else {
        part.mesh(vec![a, b, c, a, c, d])
    }
}
fn rotate_vertex(v: MeshVertex, pivot: [f32; 3], degrees: [f32; 3]) -> MeshVertex {
    let [rx, ry, rz] = degrees.map(f32::to_radians);
    let (mut x, mut y, mut z) = (v.pos[0] - pivot[0], v.pos[1] - pivot[1], v.pos[2] - pivot[2]);
    let (s, c) = rx.sin_cos();
    let (ny, nz) = (y * c - z * s, y * s + z * c);
    y = ny;
    z = nz;
    let (s, c) = ry.sin_cos();
    let (nx, nz) = (x * c + z * s, -x * s + z * c);
    x = nx;
    z = nz;
    let (s, c) = rz.sin_cos();
    let (nx, ny) = (x * c - y * s, x * s + y * c);
    x = nx;
    y = ny;
    MeshVertex { pos: [x + pivot[0], y + pivot[1], z + pivot[2]], uv: v.uv }
}
fn polygon_at(part: ModelPart, offset: [f32; 3], size: [f32; 2], uv: [f32; 4], rotation: [f32; 3], two_sided: bool) -> ModelPart {
    let anchor = [part.pivot[0] + offset[0], part.pivot[1] + offset[1], part.pivot[2] + offset[2]];
    let [x, y, z] = anchor;
    let [width, height] = size;
    let [u, v, uv_width, uv_height] = uv;
    let make = |pos: [f32; 3], uv: [f32; 2]| rotate_vertex(MeshVertex { pos, uv }, anchor, rotation);
    let a = make([x, y, z], [u, v]);
    let b = make([x + width, y, z], [u + uv_width, v]);
    let c = make([x + width, y + height, z], [u + uv_width, v + uv_height]);
    let d = make([x, y + height, z], [u, v + uv_height]);
    if two_sided {
        part.mesh(vec![a, b, c, a, c, d])
            .mesh(vec![a, c, b, a, d, c])
    } else {
        part.mesh(vec![a, b, c, a, c, d])
    }
}

/// Vanilla's creeper (`CreeperModel`): 64 x 32.
fn creeper_model() -> Model {
    let leg = |name, x, z| vanilla_part(name, [x, 18.0, z], &[([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0])]);
    Model::from(vec![
        vanilla_part("head", [0.0, 6.0, 0.0], &[([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0])]),
        vanilla_part("body", [0.0, 6.0, 0.0], &[([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0])]),
        leg("leg0", -2.0, 4.0),
        leg("leg1", 2.0, 4.0),
        leg("leg2", 2.0, -4.0),
        leg("leg3", -2.0, -4.0),
    ])
    .uv_size(64, 32)
}

/// Vanilla's skeleton (`SkeletonModel`): a thin biped, arms held out like a zombie's while aiming.
fn skeleton_model() -> Model {
    let forward = 90.0;
    Model::from(vec![
        vanilla_part("head", [0.0, 0.0, 0.0], &[([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0])]),
        vanilla_part("body", [0.0, 0.0, 0.0], &[([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0])]),
        vanilla_part("rightArm", [-5.0, 2.0, 0.0], &[([-1.0, -2.0, -1.0], [2.0, 12.0, 2.0], [40.0, 16.0])])
            .mirrored()
            .rotation(forward, 0.0, 0.0),
        vanilla_part("leftArm", [5.0, 2.0, 0.0], &[([-1.0, -2.0, -1.0], [2.0, 12.0, 2.0], [40.0, 16.0])]).rotation(forward, 0.0, 0.0),
        vanilla_part("rightLeg", [-2.0, 12.0, 0.0], &[([-1.0, 0.0, -1.0], [2.0, 12.0, 2.0], [0.0, 16.0])]).mirrored(),
        vanilla_part("leftLeg", [2.0, 12.0, 0.0], &[([-1.0, 0.0, -1.0], [2.0, 12.0, 2.0], [0.0, 16.0])]),
    ])
    .uv_size(64, 32)
}

/// Vanilla's sheep
fn sheep_model() -> Model {
    let wool = |p: [f32; 2]| [p[0], p[1] + 32.0];
    let head = vanilla_part("head", [0.0, 6.0, -8.0], &[([-4.0, -4.0, -6.0], [8.0, 8.0, 8.0], [0.0, 32.0])]);
    let body = vanilla_part("body", [0.0, 5.0, 2.0], &[([-4.0, -10.0, -7.0], [8.0, 16.0, 6.0], [28.0, 8.0])])
        .cuboid([-4.5, -8.0, -9.0], [9.0, 18.0, 8.0], [16.0, 0.0])
        .inflated(1.75)
        .rotation(-90.0, 0.0, 0.0);
    let left_ear = ModelPart::new("leftEar", [-4.0, 12.0, -9.0])
        .cuboid([0.0, 6.5, -3.0], [3.0, 2.0, 1.0], [0.0, 0.0])
        .rotation(0.0, 0.0, 15.0)
        .mirrored()
        .child_of(0);
    let right_ear = ModelPart::new("rightEar", [4.0, 12.0, -9.0])
        .cuboid([-3.0, 6.5, -3.0], [3.0, 2.0, 1.0], [0.0, 0.0])
        .rotation(0.0, 0.0, -15.0)
        .child_of(0);
    let snout = ModelPart::new("snout", [4.0, 12.0, -9.0])
        .cuboid([-5.5, 5.0, -6.0], [3.0, 3.0, 4.0], [0.0, 3.0])
        .rotation(-15.0, 0.0, 0.0)
        .child_of(0);
    let mut parts = vec![head, body, left_ear, right_ear, snout];
    parts.extend(vanilla_legs(3.0, 12.0).map(|l| {
        l.cuboid([-2.0, -6.0, -2.0], [4.0, 6.0, 4.0], wool([0.0, 16.0]))
            .inflated(0.5)
    }));
    Model::from(parts).uv_size(64, 64)
}

/// Vanilla's chicken: 64 x 32.
fn chicken_model() -> Model {
    let leg = |name, x| {
        polygon_at(
            polygon(ModelPart::new(name, [x, 5.0, 0.0]), [1.0, -5.0], [26.0, 3.0, 1.0, 5.0], true),
            [-1.0, -4.9, -3.0],
            [3.0, 3.0],
            [27.0, 3.0, 3.0, 3.0],
            [90.0, 0.0, 0.0],
            true,
        )
    };
    Model::from(vec![
        vanilla_part(
            "head",
            [0.0, 15.0, -4.0],
            &[
                ([-2.0, -6.0, -2.0], [4.0, 6.0, 3.0], [0.0, 0.0]),
                ([-1.5, -4.0, -4.0], [3.0, 2.0, 2.0], [14.0, 0.0]),
                ([-1.0, -2.0, -3.0], [2.0, 2.0, 2.0], [14.0, 4.0]),
            ],
        ),
        vanilla_part("body", [0.0, 16.0, 0.0], &[([-3.0, -4.0, -3.0], [6.0, 8.0, 6.0], [0.0, 9.0])]).rotation(-90.0, 0.0, 0.0),
        vanilla_part("wing0", [-4.0, 13.0, 0.0], &[([0.0, 0.0, -3.0], [1.0, 4.0, 6.0], [24.0, 13.0])]),
        vanilla_part("wing1", [4.0, 13.0, 0.0], &[([-1.0, 0.0, -3.0], [1.0, 4.0, 6.0], [24.0, 13.0])]),
        leg("rightLeg", -2.0),
        leg("leftLeg", 1.0),
        polygon(ModelPart::new("comb", [0.0, 17.0, -7.0]), [4.0, -3.0], [31.0, 3.0, 4.0, 3.0], true)
            .child_of(0)
            .rotation(0.0, -90.0, 0.0),
        polygon(ModelPart::new("tail", [0.0, 13.0, 2.0]), [6.0, -6.0], [36.0, 3.0, 6.0, 6.0], true).rotation(0.0, -90.0, 0.0),
    ])
    .uv_size(64, 32)
}

/// A shot arrow: a thin stick.
fn arrow_model() -> Model {
    Model::from(vec![ModelPart::new("body", [0.0, 2.0, 0.0]).cuboid([-0.5, -0.5, -8.0], [1.0, 1.0, 16.0], [0.0, 0.0])]).uv_size(32, 32)
}

/// Vanilla's cow (`CowModel`): head with muzzle and horns, body lying along z with its udder.
fn cow_model() -> Model {
    let head = vanilla_part(
        "head",
        [0.0, 4.0, -8.0],
        &[
            ([-4.0, -4.0, -6.0], [8.0, 8.0, 6.0], [0.0, 0.0]),
            ([-3.0, 1.0, -7.0], [6.0, 3.0, 1.0], [1.0, 33.0]),
            ([-5.0, -5.0, -5.0], [1.0, 3.0, 1.0], [22.0, 0.0]),
            ([4.0, -5.0, -5.0], [1.0, 3.0, 1.0], [22.0, 0.0]),
        ],
    );
    let body = vanilla_part(
        "body",
        [0.0, 5.0, 2.0],
        &[([-6.0, -10.0, -7.0], [12.0, 18.0, 10.0], [18.0, 4.0]), ([-2.0, 2.0, -8.0], [4.0, 6.0, 1.0], [52.0, 0.0])],
    )
    .rotation(-90.0, 0.0, 0.0);
    let left_ear = ModelPart::new("leftEar", [-4.0, 12.0, -9.0])
        .cuboid([0.0, 8.0, -3.0], [3.0, 2.0, 1.0], [18.0, 32.0])
        .rotation(0.0, 0.0, 15.0)
        .mirrored()
        .child_of(0);
    let right_ear = ModelPart::new("rightEar", [4.0, 12.0, -9.0])
        .cuboid([-3.0, 8.0, -3.0], [3.0, 2.0, 1.0], [18.0, 32.0])
        .rotation(0.0, 0.0, -15.0)
        .child_of(0);
    let mut parts = vec![head, body, left_ear, right_ear];
    parts.extend(vanilla_legs(4.0, 12.0));
    Model::from(parts).uv_size(64, 64)
}

/// Vanilla's pig (`PigModel`).
fn pig_model() -> Model {
    let head = vanilla_part(
        "head",
        [0.0, 12.0, -6.0],
        &[([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [0.0, 0.0]), ([-2.0, 0.0, -9.0], [4.0, 3.0, 1.0], [16.0, 16.0])],
    );
    let body = vanilla_part("body", [0.0, 11.0, 2.0], &[([-5.0, -10.0, -7.0], [10.0, 17.0, 8.0], [28.0, 8.0])]).rotation(-90.0, 0.0, 0.0);
    let left_ear = ModelPart::new("leftEar", [-4.0, 12.0, -9.0])
        .cuboid([-1.0, -2.0, -4.0], [1.0, 5.0, 4.0], [0.0, 26.0])
        .rotation(0.0, 0.0, -15.0)
        .mirrored()
        .child_of(0);
    let right_ear = ModelPart::new("rightEar", [4.0, 12.0, -9.0])
        .cuboid([0.0, -2.0, -4.0], [1.0, 5.0, 4.0], [0.0, 26.0])
        .rotation(0.0, 0.0, 15.0)
        .child_of(0);
    let mut parts = vec![head, body, left_ear, right_ear];
    parts.extend(vanilla_legs(3.0, 18.0));
    Model::from(parts).uv_size(64, 64)
}
pub static COW: EntityKind = EntityKind::named("cow");

pub fn register(r: &mut Registry) {
    // The engine's player kind, drawn as the same humanoid as the zombie.
    r.set_model(&PLAYER, humanoid(false));
    r.entity(&ZOMBIE)
        .tag("hostile")
        .equipment()
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(0.6, 1.95)
        .max_health(20.0)
        .walk_speed(2.6)
        .hostile()
        .tag(abi::comp::entity::BURNS_IN_SUN)
        .model(humanoid(true))
        .attack_damage(3.0)
        .reach(1.3)
        .goals(hostile(ai::Goals::new().aggressive(35.0)));
    r.entity(&SPIDER)
        .tag("hostile")
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(1.3, 0.9)
        .max_health(16.0)
        .walk_speed(3.6)
        .hostile()
        .model(spider())
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
    r.entity_component(&SHOOTER, 0u64, 0);
    r.entity(&CREEPER)
        .tag("hostile")
        .size(0.6, 1.7)
        .max_health(20.0)
        .walk_speed(2.5)
        .hostile()
        .model(creeper_model())
        .attach(&FUSE)
        .goals(creeper());
    r.entity(&SKELETON)
        .tag("hostile")
        .equipment()
        .size(0.6, 1.99)
        .max_health(20.0)
        .walk_speed(2.5)
        .hostile()
        .tag(abi::comp::entity::BURNS_IN_SUN)
        .model(skeleton_model())
        .attach(&FUSE)
        .goals(skeleton());
    r.entity(&ARROW)
        .no_egg()
        .size(0.5, 0.5)
        .unpushable()
        .max_health(1.0)
        .walk_speed(0.0)
        .model(arrow_model())
        .attach(&SHOOTER)
        .goals(ai::Goals::new().then(arrow_flies));
    r.entity(&SHEEP)
        .tag("livestock")
        .size(0.9, 1.3)
        .max_health(8.0)
        .walk_speed(2.3)
        .passive()
        .model(sheep_model())
        .goals(passive());
    r.entity(&CHICKEN)
        .tag("livestock")
        .size(0.4, 0.7)
        .max_health(4.0)
        .walk_speed(2.5)
        .passive()
        .model(chicken_model())
        .attach(&FUSE)
        .goals(passive_chicken());
    r.entity(&PIG)
        .tag("livestock")
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(0.9, 0.9)
        .max_health(10.0)
        .walk_speed(2.5)
        .passive()
        .model(pig_model())
        .goals(passive());
    r.entity(&COW)
        .tag("livestock")
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(0.9, 1.3)
        .max_health(10.0)
        .walk_speed(2.2)
        .passive()
        .model(cow_model())
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
    r.loot(&CREEPER).pool(Pool::new().items("gunpowder", 0, 2));
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
    r.natural_spawn(&CREEPER).light(0, 6).weight(2);
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

/// Vanilla's creeper: comes for the player, and within 3 blocks starts its 30 tick fuse, which
/// runs back down once the player is 7 away. A radius 3 blast at the end.
fn creeper() -> ai::Goals {
    ai::Goals::new()
        .aggressive(16.0)
        .then(|w: &World, id: u64, st: &EntityState| {
            let near = ai::target(w, id, st, 7.0);
            let d2 = near
                .as_ref()
                .map_or(f64::MAX, |(_, p)| (p.pos - st.pos).length_squared());
            let Some(fuse) = w.component_mut(id, &FUSE) else { return false };
            if d2 < 9.0 || (*fuse > 0 && d2 < 49.0) {
                *fuse += 1;
            } else {
                *fuse = fuse.saturating_sub(1);
            }
            let lit = *fuse;
            // Faster as it nears the end.
            if lit > 0 && lit % (if lit > 20 { 3 } else { 8 }) == 0 {
                w.flash(id);
            }
            if lit >= 30 {
                blast(w, id, st.pos.to_array().into(), 3.0);
                return true;
            }
            lit > 0
        })
        .then(ai::watch_player(8.0))
        .then(ai::chase_target(16.0, 1.0))
        .then(ai::wander(0.6, 10.0))
}

/// Vanilla's explosion in short: a hole, and damage falling off to nothing at twice the radius.
fn blast(w: &World, id: u64, at: Vec3, radius: f64) {
    for other in w.entities_near(at, radius * 2.0, None) {
        let Some(s) = w.entity(other).filter(|_| other != id) else { continue };
        let d = (s.pos - at).length_squared().sqrt();
        let impact = (1.0 - d / (radius * 2.0)).max(0.0);
        let damage = (impact * impact + impact) / 2.0 * 7.0 * radius * 2.0 + 1.0;
        w.damage(other, damage as f32, id, damage_cause::EXPLOSION);
    }
    // Vanilla's: one going off in a liquid breaks nothing, and liquids don't break at all.
    if w.block_flags(block_pos_of(at)) & abi::block_flags::LIQUID == 0 {
        w.fill(Shape::blast(at, radius, w.random()), 0)
            .keep("#unbreakable #liquids");
    }
    w.spawn_particles(&Particle::named("core:poof"), at)
        .count(40)
        .spread(radius as f32 * 0.5)
        .speed(2.0);
    w.despawn(id);
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

fn shoot(w: &World, id: u64, from: &EntityState, at: &EntityState) {
    let eye = from.eye().y;
    let d = [at.pos.x - from.pos.x, at.pos.y + at.size.eye as f64 * 0.7 - eye, at.pos.z - from.pos.z];
    let flat = (d[0] * d[0] + d[2] * d[2]).sqrt();
    let len = (flat * flat + d[1] * d[1]).sqrt().max(0.1);
    // A little above the target for the drop on the way.
    const SPEED: f64 = 32.0;
    let arrow = w.spawn(&ARROW, [from.pos.x, eye, from.pos.z]);
    if let Some(shooter) = w.component_mut(arrow, &SHOOTER) {
        *shooter = id;
    }
    w.entity_modify(arrow)
        .velocity([d[0] / len * SPEED, d[1] / len * SPEED + flat * 0.4, d[2] / len * SPEED])
        .yaw(yaw_toward([0.0; 3], d));
}

/// An arrow in flight: gone when it hits something or has flown for five seconds.
fn arrow_flies(w: &World, id: u64, st: &EntityState) -> bool {
    let shooter = w.component(id, &SHOOTER).unwrap_or(0);
    // Its own box against theirs: a tall mob is hit in the head too.
    let tip = Shape::cuboid([st.pos.x - 0.25, st.pos.y, st.pos.z - 0.25], [st.pos.x + 0.25, st.pos.y + 0.5, st.pos.z + 0.25]);
    let hit = w
        .entities_in(&tip, EntityFilter::any().targets().touching())
        .into_iter()
        .find(|&e| e != id && e != shooter && w.entity(e).is_some_and(|s| s.kind != st.kind));
    if let Some(target) = hit {
        w.damage(target, 3.0, shooter, damage_cause::ATTACK);
        w.despawn(id);
    } else if st.flags & entity_state::BLOCKED != 0 || st.on_ground != 0 || st.age > 100 {
        w.despawn(id);
    }
    true
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
