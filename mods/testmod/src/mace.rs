//! A mace like modern vanilla's: the farther its wielder fell, the harder it lands, the fall
//! hurts them not at all, and what stands around is thrown back.

use weft_sdk::*;

use base_api::tabs::TOOLS;

pub static MACE: Item = Item::named("mace");
static DUST: Particle = Particle::named(abi::particle::BLOCK);

/// Vanilla's: falls up to this far are no smash.
const SMASH_FROM: f64 = 1.5;
/// How far the smash throws what is around the target, and how hard at its middle.
const SMASH_RADIUS: f64 = 3.5;
const SMASH_PUSH: f64 = 8.0;

/// What the fall adds to a hit: 4 a block for the first three, 2 for the next five, 1 beyond.
pub fn smash_bonus(fall: f64) -> f64 {
    if fall <= SMASH_FROM {
        return 0.0;
    }
    4.0 * fall.min(3.0) + 2.0 * (fall - 3.0).clamp(0.0, 5.0) + (fall - 8.0).max(0.0)
}

fn holds_mace(w: &World, id: u64) -> bool {
    w.inventory(id).get(w.selected_slot(id)).item == MACE.id()
}

pub fn register(r: &mut Registry) {
    r.item(&MACE)
        .attack_damage(6.0)
        .attack_speed(0.6)
        .durability(500)
        .handheld()
        .tab(&TOOLS);
    // The damage: only what the numbers are, so asking the chain is no smash.
    r.modify(&chain::ATTACK_DAMAGE, stage::ADD, |m| {
        let q = *m.query();
        if q.item != MACE.id() {
            return;
        }
        if let Some(me) = m.world().entity(q.entity) {
            m.value += smash_bonus(me.fall_distance as f64);
        }
    });
    // The smash itself, once a hit landed.
    r.on_event(&actions::HURT, |b| {
        let w = b.world();
        for h in b.iter().filter(|h| h.cause == damage_cause::ATTACK) {
            let Some(me) = w.entity(h.source).filter(|_| holds_mace(&w, h.source)) else { continue };
            if (me.fall_distance as f64) <= SMASH_FROM {
                continue;
            }
            w.entity_modify(h.source)
                .fall_distance(0.0)
                .velocity(Vec3 { y: 0.0, ..me.vel });
            let Some(target) = w.entity(h.entity) else { continue };
            for id in w.targets_near(target.pos, SMASH_RADIUS, None) {
                let Some(them) = w.entity(id).filter(|_| id != h.source && id != h.entity) else { continue };
                let (dx, dz) = (them.pos.x - target.pos.x, them.pos.z - target.pos.z);
                let d = (dx * dx + dz * dz).sqrt().max(0.1);
                let k = SMASH_PUSH * (1.0 - d / SMASH_RADIUS).max(0.0);
                w.push(id, [dx / d * k, k * 0.6, dz / d * k]);
            }
            w.spawn_particles(&DUST, target.pos)
                .block(w.block(target.pos.offset(0.0, -0.5, 0.0).block()))
                .count(60)
                .velocity([0.0, 3.0, 0.0])
                .speed(4.0)
                .outward();
        }
    });
}
