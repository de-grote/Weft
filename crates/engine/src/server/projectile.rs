//! What rays hit, blocks and entities alike, and projectiles: each tick a ray along a
//! projectile's speed decides what it hits before it moves, so nothing is flown through however
//! fast, and where it sticks is where the ray met the block.

use glam::{DQuat, DVec3, IVec3};
use weft_abi as abi;

use super::dim::Dim;
use super::entity::{Stuck, StuckIn};
use crate::content::Content;
use crate::physics::{Cells, Transform};

/// At most this many hits refused in one tick before a projectile just flies on.
const MAX_REFUSED: usize = 4;

impl Dim {
    /// What a ray from `from` (this dimension's frame) along `dir` hits first within `max`, of
    /// what `flags` say (`abi::ray`), past the entities in `skip`.
    pub fn raycast(&self, c: &Content, from: DVec3, dir: DVec3, max: f64, flags: u32, skip: &[u64]) -> Option<abi::RayHit> {
        let dir = dir.normalize_or_zero();
        if dir == DVec3::ZERO || !max.is_finite() {
            return None;
        }
        let mut best: Option<abi::RayHit> = None;
        if flags & (abi::ray::BLOCKS | abi::ray::LIQUIDS) != 0 {
            if let Some(h) = self.ray(c, from, dir, max, flags) {
                let point = from + dir * h.dist;
                // On a body's block, where in that body's blocks.
                let on = match self.entities.get(h.frame).filter(|_| h.frame != 0) {
                    Some(b) => Transform::of(b.pos).to_local(self.place.to_world(point)),
                    None => point,
                };
                best = Some(abi::RayHit {
                    entity: 0,
                    body: h.frame,
                    pos: h.origin.to_array().into(),
                    state: h.state,
                    face: h.face as u32,
                    part: 0,
                    point: point.to_array().into(),
                    local: (on - h.origin.as_dvec3()).to_array().into(),
                    distance: h.dist,
                });
            }
        }
        if flags & abi::ray::ENTITIES != 0 {
            let reach = best.map_or(max, |b| b.distance);
            // Only what stands near the line's box is looked at closely: no mob is wider than this.
            const WIDEST: f64 = 4.0;
            let (to, world_from) = (self.place.to_world(from + dir * reach), self.place.to_world(from));
            let (lo, hi) = (world_from.min(to) - DVec3::splat(WIDEST), world_from.max(to) + DVec3::splat(WIDEST));
            let mut near: Option<(f64, u8, u64, DVec3, u32)> = None;
            let mut scratch = Vec::new();
            for e in &self.entities.list {
                if e.pos.cmplt(lo).any() || e.pos.cmpgt(hi).any() {
                    continue;
                }
                // A body with blocks is hit by those; the dead are hit no more.
                if e.removed || !e.is_body() || !e.alive() || skip.contains(&e.id) || self.bodies.contains_key(&e.id) {
                    continue;
                }
                // The parts of its hitbox where they are this tick, else its box.
                let body = e.hit_body(c, self.place.to_local(e.pos));
                let parts = c.game.entities[e.kind as usize].hitboxes.as_ref();
                if let Some((part, t, face, on)) = crate::hitbox::ray_entity(parts, &body, from, dir, reach, &mut scratch) {
                    if near.is_none_or(|n| t < n.0) {
                        near = Some((t, face, e.id, on, part));
                    }
                }
            }
            if let Some((t, face, id, on, part)) = near {
                let point = from + dir * t;
                best = Some(abi::RayHit {
                    entity: id,
                    body: 0,
                    pos: abi::Vec3::from(point.to_array()).block(),
                    state: 0,
                    face: face as u32,
                    part,
                    point: point.to_array().into(),
                    local: on.to_array().into(),
                    distance: t,
                });
            }
        }
        best
    }

    /// Projectiles (`comp::entity::PROJECTILE`) move: a ray along this tick's speed first, what
    /// it hits asked (`action::PROJECTILE_HIT`) and told; an entity hit takes the damage
    /// (`chain::PROJECTILE_DAMAGE`) and the projectile goes, a block stops it there, stuck or
    /// gone. Then drag and gravity, as vanilla's arrow.
    pub(super) fn fly_projectiles(&mut self, c: &'static Content) {
        let flying: Vec<(u64, abi::Projectile)> = self
            .entities
            .list
            .iter()
            .filter(|e| !e.removed && e.item.is_empty())
            .filter_map(|e| {
                c.game.entities[e.kind as usize]
                    .projectile
                    .clone()
                    .map(|p| (e.id, p))
            })
            .collect();
        let gravity = crate::physics::gravity_per_tick(c.dims[self.id as usize].gravity as f64);
        for (id, p) in flying {
            let Some(e) = self.entities.get(id) else { continue };
            if let Some(st) = e.stuck {
                self.stay_stuck(c, id, st, &p);
                continue;
            }
            let gravity = gravity * self.attribute(c, id, crate::registry::attr::GRAVITY, false);
            let Some(e) = self.entities.get(id) else { continue };
            let v = e.vel * crate::world::TICK;
            let speed = v.length();
            let (pos, owner) = (self.place.to_local(e.pos), e.owner);
            let mut skip = vec![id];
            // Its shooter can't be hit until it flew clear of them.
            if owner != 0 && !e.left_owner {
                let clear = self.entities.get(owner).is_none_or(|o| {
                    !o.aabb(c)
                        .intersects(&self.entities.get(id).map_or(o.aabb(c), |e| e.aabb(c)))
                });
                if clear {
                    if let Some(e) = self.entities.get_mut(id) {
                        e.left_owner = true;
                    }
                } else {
                    skip.push(owner);
                }
            }
            let mut to = pos + v;
            // What it goes on with: `None` stopped (stuck or gone), else its speed.
            let mut after = Some(v);
            for _ in 0..MAX_REFUSED {
                let flags = abi::ray::BLOCKS | abi::ray::SOLID | abi::ray::ENTITIES;
                let Some(h) = self.raycast(c, pos, v, speed, flags, &skip) else { break };
                let q = abi::Query {
                    entity: id,
                    other: h.entity,
                    pos: h.pos,
                    state: h.state,
                    amount: speed,
                    part: h.part,
                    ..abi::Query::DEFAULT
                };
                if !self.ask(c, abi::action::PROJECTILE_HIT, &q) {
                    if h.entity == 0 {
                        break;
                    }
                    skip.push(h.entity);
                    continue;
                }
                let point = DVec3::from_array(h.point.to_array());
                let hit =
                    abi::ProjectileHit { projectile: id, target: h.entity, owner, pos: h.pos, face: h.face, part: h.part, point: h.point };
                self.emit_engine(c, abi::action::PROJECTILE_HIT, &hit);
                if h.entity != 0 {
                    let amount = self.compute(c, abi::chain::PROJECTILE_DAMAGE, p.damage as f64, &q, false) as f32;
                    let d = abi::Damage {
                        amount,
                        cause: abi::damage_cause::PROJECTILE,
                        source: owner,
                        direct: id,
                        knock: v.to_array().into(),
                        part: h.part,
                        ..abi::Damage::DEFAULT
                    };
                    let embedded = point + v.normalize_or_zero() * 0.15;
                    let yaw = self.entities.get(id).map_or(0.0, |e| e.yaw);
                    // A lethal hit removes its target; keep the target's frame while it exists.
                    let stuck = p
                        .sticks
                        .then(|| self.stuck_at(StuckIn::Entity(h.entity), embedded, yaw));
                    let outcome = self.damage_outcome(c, h.entity, &d);
                    if let super::guard::DamageOutcome::Blocked(slot) = outcome {
                        if p.sticks {
                            self.catch_projectile(c, h.entity, slot, id);
                        }
                        if let Some(e) = self.entities.get_mut(id) {
                            e.gone(abi::removal::HIT);
                        }
                        after = None;
                        to = point;
                    } else if matches!(outcome, super::guard::DamageOutcome::Hurt) {
                        // In their box a little way, going about with them, as vanilla's arrows.
                        to = embedded;
                        if let Some(e) = self.entities.get_mut(id) {
                            match stuck {
                                Some(st) => e.stuck = Some(st),
                                None => e.gone(abi::removal::HIT),
                            }
                        }
                        after = None;
                    } else {
                        // Refused or not landing, it bounces back off them as vanilla's does.
                        to = point;
                        after = Some(-v * 0.1);
                    }
                } else {
                    // Just short of the face it went in through, as vanilla's arrow.
                    to = point - v.normalize_or_zero() * 0.05;
                    let block = StuckIn::Block { body: h.body, block: IVec3::from_array(h.pos.to_array()) };
                    let yaw = self.entities.get(id).map_or(0.0, |e| e.yaw);
                    let stuck = p.sticks.then(|| self.stuck_at(block, to, yaw));
                    if let Some(e) = self.entities.get_mut(id) {
                        match stuck {
                            Some(st) => e.stuck = Some(st),
                            None => e.gone(abi::removal::HIT),
                        }
                    }
                    after = None;
                }
                break;
            }
            let wet = crate::physics::Cells::liquid(&super::space::CubeCells(&self.space.cubes), c, to.floor().as_ivec3()).is_some();
            let Some(e) = self.entities.get_mut(id) else { continue };
            e.pos = self.place.to_world(to);
            let v = after.map_or(DVec3::ZERO, |v| v * if wet { p.liquid_drag } else { p.drag } as f64 - DVec3::Y * gravity);
            e.vel = v * crate::world::TPS as f64;
            e.in_liquid = wet;
            // Pointing where it flies.
            if v.length_squared() > 1e-8 {
                e.yaw = (-v.x).atan2(-v.z) as f32;
                e.pitch = v.y.atan2(v.with_y(0.0).length()) as f32;
            }
        }
    }

    /// Stuck at `to` (this dimension's frame) in what `into` says, pointing `yaw`.
    fn stuck_at(&self, into: StuckIn, to: DVec3, yaw: f32) -> Stuck {
        let world = self.place.to_world(to);
        let (at, yaw) = match into {
            StuckIn::Block { body, .. } => match self.entities.get(body).filter(|_| body != 0) {
                Some(b) => (Transform::of(b.pos).to_local(world), yaw),
                None => (to, yaw),
            },
            StuckIn::Entity(id) => match self.entities.get(id) {
                Some(t) => (turn(world - t.pos, -t.body_yaw), yaw - t.body_yaw),
                None => (to, yaw),
            },
        };
        Stuck { into, at, yaw, ticks: 0 }
    }

    /// A stuck projectile rides what holds it: the body its block is on, falling once that block
    /// is gone, or the entity it went into, going with it. It goes after its `stuck_ticks`.
    fn stay_stuck(&mut self, c: &Content, id: u64, st: Stuck, p: &abi::Projectile) {
        let solid = |s: u32| !c.states[s as usize].collision.is_empty();
        let held = |cells: &dyn Fn(IVec3) -> (Option<u32>, Vec<u32>), block: IVec3| {
            let (main, extras) = cells(block);
            main.is_some_and(solid) || extras.iter().any(|&s| solid(s))
        };
        // Where it is now, or `None` once nothing holds it.
        let now: Option<(Option<DVec3>, f32)> = match st.into {
            StuckIn::Block { body: 0, block } => {
                let cells = super::space::CubeCells(&self.space.cubes);
                held(&|b| (cells.main(b), cells.extras(b).to_vec()), block).then_some((None, st.yaw))
            }
            StuckIn::Block { body, block } => {
                let holds = self.bodies.get(&body).is_some_and(|d| {
                    let cells = super::space::DeckCells(d);
                    held(&|b| (cells.main(b), cells.extras(b).to_vec()), block)
                });
                let place = self.entities.get(body).map(|b| Transform::of(b.pos));
                holds.then(|| (place.map(|t| t.to_world(st.at)), st.yaw))
            }
            StuckIn::Entity(target) => self
                .entities
                .get(target)
                .filter(|t| t.alive())
                .map(|t| (Some(t.pos + turn(st.at, t.body_yaw)), st.yaw + t.body_yaw)),
        };
        let Some(e) = self.entities.get_mut(id) else { return };
        let Some((pos, yaw)) = now else {
            match st.into {
                // Its block went: it falls from where it was.
                StuckIn::Block { .. } => (e.stuck, e.vel) = (None, DVec3::ZERO),
                // Who it was in is gone.
                StuckIn::Entity(_) => e.gone(abi::removal::HIT),
            }
            return;
        };
        if let Some(pos) = pos {
            e.pos = pos;
        }
        e.yaw = yaw;
        let ticks = st.ticks + 1;
        e.stuck = Some(Stuck { ticks, ..st });
        if ticks > p.stuck_ticks {
            e.gone(abi::removal::EXPIRED);
        }
    }
}

/// `v` turned about the up axis by `yaw` radians, as models turn with their yaw.
fn turn(v: DVec3, yaw: f32) -> DVec3 {
    DQuat::from_rotation_y(yaw as f64) * v
}
