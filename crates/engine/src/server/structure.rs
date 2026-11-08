//! Moving structures: blocks that move together, a `Space` in its own coordinates owned by an
//! entity whose position places it: block (0, 0, 0) is centered on the entity. Whatever the
//! world does with blocks works on it as it is (placement, support, callbacks, loot, events),
//! in `Dim::in_frame`; what comes out (dropped items, sounds, particles) is in the world, through
//! `Dim::place`, and block entities on it are entities of the region like any.

use glam::{DVec3, IVec3};

use weft_abi as abi;

use super::dim::Dim;
use super::entity::Motion;
use super::signal::{Moved, RiderMeets};
use super::space::Space;
use crate::content::{AIR, Content};
use crate::physics::Transform;
use crate::world::{cube_of, local_index};
use rustc_hash::{FxHashMap, FxHashSet};

/// What has landed is there for circuits at once, and for clients this much later, once their view
/// of the body has caught up with the server's (they show bodies 1.5 ticks behind).
const REST_TICKS: u8 = 2;
/// And the body goes only after the block has reached the client, or whoever stands on it has
/// nothing under them for a moment.
const LINGER_TICKS: u8 = 2;

/// A floater's speed: blocks a second per level of power.
const SPEED_PER_LEVEL: f64 = 0.5;
/// However many floaters, on each axis: it looks one block ahead for each it comes to, so less
/// than one a tick.
const MOST_SPEED: f64 = 15.0;

/// A block out of its place on its way to another: where in the body and where it ends up,
/// with the entity it has, which stays the same one.
#[derive(Clone, Copy)]
pub struct Carried {
    pub local: IVec3,
    pub to: IVec3,
    pub state: u32,
    pub entity: Option<u64>,
}

/// Floaters going together, and what that takes.
pub struct FloaterPlan {
    pub floaters: Vec<IVec3>,
    pub cells: FxHashSet<IVec3>,
    pub broken: Vec<IVec3>,
}

/// What `Dim::lift` takes: from where, to where in the body and where it is meant to end up, and
/// what of it (a cell of wire may have pieces that stay), if not all.
pub struct Lifted {
    pub from: IVec3,
    pub local: IVec3,
    pub to: IVec3,
    pub part: Option<u32>,
}

/// How a body's circuits go. They always go; only that a piston on a piston's blocks can't act.
#[derive(Clone, Copy, PartialEq)]
pub enum Liveness {
    Free,
    OnTheWay,
}

/// A body whose blocks have landed, waiting for clients to catch up with them.
pub struct Landing {
    body: u64,
    rests: u8,
    blocks: Vec<Carried>,
    /// Cells clients aren't told of yet.
    cells: Vec<IVec3>,
    /// A block in the body that turns into another a tick in: (where, into).
    turns: Option<(IVec3, u32)>,
}

/// The box a structure's blocks fill around its entity: (width, height). Block (0, 0, 0) is
/// centered on the entity, so the box reaches as far as the farthest block.
/// Each step a body going `sign` has to clear: along each axis it goes and across the corners
/// between them, once.
fn ways(sign: IVec3) -> impl Iterator<Item = IVec3> {
    (1..8)
        .filter(move |m| (m & 1 == 0 || sign.x != 0) && (m & 2 == 0 || sign.y != 0) && (m & 4 == 0 || sign.z != 0))
        .map(move |m| IVec3::new(sign.x * (m & 1), sign.y * ((m >> 1) & 1), sign.z * ((m >> 2) & 1)))
}

pub fn extent(blocks: &[(IVec3, u32)]) -> Option<(f32, f32)> {
    let mut hw: f32 = 0.0;
    let mut top = i32::MIN;
    for (p, _) in blocks {
        hw = hw
            .max((p.x as f32 - 0.5).abs())
            .max((p.x as f32 + 0.5).abs())
            .max((p.z as f32 - 0.5).abs())
            .max((p.z as f32 + 0.5).abs());
        top = top.max(p.y + 1);
    }
    (top > i32::MIN).then_some((hw * 2.0, top as f32))
}

/// The blocks a kind is built of (its template).
pub fn template(c: &Content, kind: u32) -> Option<&[(IVec3, u32)]> {
    let t = c.game.entities.get(kind as usize)?.template?;
    Some(&c.templates[t as usize].blocks)
}

impl Dim {
    /// Takes the block at `p` out: nothing of it is broken, what is next to it just hears it is gone.
    /// What it has as an entity comes along, not attached to any place for now.
    pub fn take_block(&mut self, c: &'static Content, p: IVec3) -> Option<(u32, Option<u64>, Moved)> {
        let state = self.get(p)?;
        let entity = self.anchored(p);
        if let Some(id) = entity {
            self.entities.unanchor((self.frame, p), id);
        }
        let signal = self.space.signals.carry_out(p);
        self.write_block(c, p, AIR, true);
        Some((state, entity, signal))
    }

    /// Puts a block taken out with `take_block` at `p`, its entity with it, its circuit going on.
    pub fn put_block(&mut self, c: &'static Content, p: IVec3, state: u32, entity: Option<u64>, signal: Moved) {
        if !signal.is_empty() {
            self.space.signals.arriving.insert(p, signal);
        }
        self.write_block(c, p, state, true);
        if let Some(id) = entity {
            self.entities
                .attach(id, self.frame, p, self.place.to_world(p.as_dvec3() + DVec3::splat(0.5)));
        }
    }

    /// Takes `cells` out of here into a new body of `kind` with its entity at `at`.
    pub fn lift(
        &mut self,
        c: &'static Content,
        kind: u32,
        at: DVec3,
        cells: impl IntoIterator<Item = Lifted>,
        also: &[(IVec3, u32)],
        live: Liveness,
    ) -> (u64, Vec<Carried>) {
        let mut carried = Vec::new();
        let mut blocks: Vec<(IVec3, u32)> = also.to_vec();
        let mut arriving = FxHashMap::default();
        let mut extras: Vec<(IVec3, Vec<u32>)> = Vec::new();
        for l in cells {
            let whole = self.get(l.from);
            if !self.space.extras(l.from).is_empty() {
                extras.push((l.local, self.space.extras(l.from).to_vec()));
            }
            let Some((state, entity, signal)) = self.take_block(c, l.from) else { continue };
            let state = l.part.unwrap_or(state);
            // Pieces of wire on what stays, stay.
            if let (Some(part), Some(whole)) = (l.part, whole) {
                if part != whole {
                    let first = c.block_of(whole).first_state;
                    self.write_block(c, l.from, first + ((whole - first) & !(part - first)), true);
                }
            }
            blocks.push((l.local, state));
            if !signal.is_empty() {
                arriving.insert(l.local, signal);
            }
            carried.push(Carried { local: l.local, to: l.to, state, entity });
        }
        let body = self.spawn(c, kind, at);
        let mut space = Space::default();
        space.signals.arriving = arriving;
        space.signals.now = self.time;
        space.signals.on_the_way = live == Liveness::OnTheWay;
        // Where it starts: it moves before anything looks, and half a block on it could count as
        // either cell.
        let cell = Transform::of(self.place.to_world(at)).cell(None);
        space.cell = Some(cell);
        space.build(c, &blocks);
        for (at, parts) in extras {
            space.put_extras(at, parts);
        }
        self.bodies.insert(body, Box::new(space));
        // What it lit with doesn't go off for a moment. Its light is in the world's, from there on
        // a body too; that gets to it when it looks.
        if self.frame == 0 {
            self.light_from_body(c, body, cell);
            self.see_world_light(body);
        }
        // On a body itself, it goes with that.
        let on = (self.frame != 0).then_some(self.frame);
        if let Some(e) = self.entities.get_mut(body) {
            e.frame = on;
        }
        for b in &carried {
            if let Some(id) = b.entity {
                let pos = crate::physics::Transform::of(self.place.to_world(at)).to_world(b.local.as_dvec3() + DVec3::splat(0.5));
                self.entities.attach(id, body, b.local, pos);
            }
        }
        (body, carried)
    }

    /// `cells` out of here into a platform, for a mod.
    pub fn lift_for_mod(&mut self, c: &'static Content, center: IVec3, cells: &[IVec3]) -> u64 {
        let cells: Vec<IVec3> = cells
            .iter()
            .copied()
            .filter(|p| self.get(*p).is_some_and(|s| s != AIR))
            .collect();
        if cells.is_empty() {
            return 0;
        }
        let lifted = self.with_riders(c, center, cells);
        self.lift(c, c.game.platform_kind, center.as_dvec3() + DVec3::new(0.5, 0.0, 0.5), lifted, &[], Liveness::Free)
            .0
    }

    /// Floaters with power in the world, to go together: those stuck to each other are one body, and
    /// can't go if their ways add up to nothing or something is in that way.
    pub(super) fn floater_plans(&self, c: &Content, floaters: Vec<IVec3>) -> (Vec<FloaterPlan>, Vec<IVec3>) {
        let mut plans: Vec<FloaterPlan> = Vec::new();
        let mut held = Vec::new();
        for p in floaters {
            let Some(cells) = self.stuck_to_floater(c, p) else {
                held.push(p);
                continue;
            };
            let mut plan = FloaterPlan { floaters: vec![p], cells: cells.into_iter().collect(), broken: Vec::new() };
            // It and those it shares a block with are one, whichever came first.
            let (joined, others): (Vec<FloaterPlan>, Vec<FloaterPlan>) = plans
                .into_iter()
                .partition(|o| !o.cells.is_disjoint(&plan.cells));
            for o in joined {
                plan.floaters.extend(o.floaters);
                plan.cells.extend(o.cells);
            }
            plans = others;
            plans.push(plan);
        }
        let mut going = Vec::new();
        for mut plan in plans {
            let steps = self.floaters_way(c, &plan.floaters);
            let ahead = (steps.0 != IVec3::ZERO)
                .then(|| self.ahead_every_way(c, plan.cells.iter().copied(), &plan.cells, steps.0, steps.1))
                .flatten();
            match ahead {
                Some(mut broken) => {
                    // What is fixed to them comes along, unless it can't.
                    let list: Vec<IVec3> = plan.cells.iter().copied().collect();
                    let riders: FxHashSet<IVec3> = self
                        .who_rides(c, &list, None, IVec3::ZERO)
                        .0
                        .into_iter()
                        .map(|r| r.0)
                        .collect();
                    broken.retain(|b| !riders.contains(b));
                    broken.extend(ways(steps.0).flat_map(|step| self.who_rides(c, &list, None, step).1));
                    broken.sort_unstable_by_key(|b| b.to_array());
                    broken.dedup();
                    plan.broken = broken;
                    going.push(plan);
                }
                None => {
                    log::debug!("floaters at {:?} can't go", plan.floaters);
                    held.extend(plan.floaters);
                }
            }
        }
        (going, held)
    }

    /// Which way these floaters in the world add up to, by axis, and whether they break through.
    fn floaters_way(&self, c: &Content, floaters: &[IVec3]) -> (IVec3, bool) {
        let mut sum = IVec3::ZERO;
        let mut breaks = false;
        for p in floaters {
            let Some(s) = self.get(*p).map(|s| c.states[s as usize].signal) else { continue };
            let Some(level) = self.space.signals.level_into(*p) else { continue };
            sum += crate::world::DIRS
                .get(s.front as usize)
                .copied()
                .unwrap_or(IVec3::ZERO)
                * level as i32;
            breaks |= s.flags as u32 & weft_abi::signal_flags::BREAKS != 0;
        }
        (sum.signum(), breaks)
    }

    /// Off with them.
    pub(super) fn run_floater_plan(&mut self, c: &'static Content, plan: FloaterPlan) {
        self.break_for_body(c, plan.broken);
        let center = plan
            .floaters
            .iter()
            .copied()
            .min_by_key(|p| p.to_array())
            .unwrap_or(IVec3::ZERO);
        log::debug!("floaters at {:?} set off with {} blocks", plan.floaters, plan.cells.len());
        let cells = self.with_riders(c, center, plan.cells.into_iter().collect());
        let (id, _) = self.lift(c, c.game.platform_kind, center.as_dvec3() + DVec3::new(0.5, 0.0, 0.5), cells, &[], Liveness::Free);
        if let Some(b) = self.bodies.get_mut(&id) {
            b.floated = true;
        }
    }

    /// `what_is_ahead` along each axis `sign` has and across the corners between them.
    fn ahead_every_way(
        &self,
        c: &Content,
        cells: impl Iterator<Item = IVec3> + Clone,
        own: &FxHashSet<IVec3>,
        sign: IVec3,
        breaks: bool,
    ) -> Option<Vec<IVec3>> {
        let mut out = Vec::new();
        for step in ways(sign) {
            out.extend(self.what_is_ahead(c, cells.clone(), own, step, breaks)?);
        }
        Some(out)
    }

    /// What is in the way of `cells`, whichever of them `own` has not one step along `step`:
    /// what they break going on. `None`: it can't go on, or there is no telling.
    fn what_is_ahead(
        &self,
        c: &Content,
        cells: impl Iterator<Item = IVec3>,
        own: &FxHashSet<IVec3>,
        step: IVec3,
        breaks: bool,
    ) -> Option<Vec<IVec3>> {
        let mut out = Vec::new();
        for at in cells {
            let n = at + step;
            if own.contains(&n) {
                continue;
            }
            let s = self.get(n)?;
            let info = &c.states[s as usize];
            if s == AIR || info.liquid.is_some() {
                continue;
            }
            if info.flags & abi::block_flags::SOLID != 0 && (!breaks || info.hardness < 0.0) {
                return None;
            }
            out.push(n);
        }
        Some(out)
    }

    pub(super) fn break_for_body(&mut self, c: &'static Content, cells: Vec<IVec3>) {
        for p in cells {
            if let Some(s) = self.get(p).filter(|s| *s != AIR) {
                self.drop_block_loot(c, p, s, abi::ItemStack::EMPTY, false, 0);
                self.set(c, p, AIR);
            }
        }
    }

    /// Platforms: their floaters say how they go. Coming to a stop, they go on to a whole block.
    pub(super) fn drive_bodies(&mut self, c: &'static Content) {
        let bodies = &self.bodies;
        let ids: Vec<u64> = self
            .entities
            .list
            .iter()
            .filter(|e| {
                (e.kind == c.game.platform_kind || bodies.get(&e.id).is_some_and(|b| b.stopping))
                    && !e.removed
                    && e.motion != Motion::Parked
            })
            .map(|e| e.id)
            .collect();
        for id in ids {
            self.drive(c, id);
        }
    }

    fn drive(&mut self, c: &'static Content, id: u64) {
        let Some(b) = self.bodies.get(&id) else { return };
        let Some(e) = self.entities.get(id) else { return };
        if b.stopping {
            if !matches!(e.motion, Motion::To { .. }) {
                self.put_down_whole(c, id);
            }
            return;
        }
        let Some(cell) = b.cell else { return };
        let Some(floaters) = b.signals.floaters(c) else { return };
        // Without any it is a mod's, unless they were broken off it.
        if floaters.is_empty() {
            if b.floated {
                log::debug!("a platform at {cell} has no floaters left");
                let (pos, vel) = (e.pos, e.vel);
                self.bring_to_stop(id, pos, vel, None);
            }
            return;
        }
        let v: DVec3 = floaters
            .iter()
            .filter_map(|&(f, level, _)| {
                crate::world::DIRS
                    .get(f as usize)
                    .map(|d| d.as_dvec3() * level as f64 * SPEED_PER_LEVEL)
            })
            .sum();
        let v = v.clamp(DVec3::splat(-MOST_SPEED), DVec3::splat(MOST_SPEED));
        let breaks = floaters.iter().any(|m| m.2);
        let pos = e.pos;
        let vel = e.vel;
        if v == DVec3::ZERO {
            log::debug!("a platform at {cell} has no power: {floaters:?}");
            self.bring_to_stop(id, pos, vel, None);
            return;
        }
        // On coming to a new cell, a look at the next one.
        let sign = IVec3::from_array([0, 1, 2].map(|a| if v[a].abs() > 1e-9 { v[a].signum() as i32 } else { 0 }));
        // What it has that can't be moved now, with more of it pushing that along: it stops as it
        // would have stayed. What nothing pushes rides on, a body doesn't come apart on its way.
        let solid = |p: IVec3| {
            b.get(p)
                .is_some_and(|s| c.states[s as usize].flags & abi::block_flags::SOLID != 0)
        };
        if b.signals
            .fixed(c)
            .any(|p| ways(sign).any(|step| solid(p - step)))
        {
            log::debug!("a platform at {cell} going {sign} is held by what it carries");
            self.bring_to_stop(id, pos, vel, None);
            return;
        }
        if b.looked_from != Some((cell, sign)) {
            let all = b.all_blocks();
            let own: FxHashSet<IVec3> = all.iter().map(|(p, _)| *p + cell).collect();
            let solid = |s: u32| c.states[s as usize].flags & abi::block_flags::SOLID != 0;
            let at: Vec<IVec3> = all
                .iter()
                .filter(|(_, s)| solid(*s))
                .map(|(p, _)| *p + cell)
                .collect();
            let Some(mut broken) = self.ahead_every_way(c, at.iter().copied(), &own, sign, breaks) else {
                log::debug!("a platform at {cell} going {sign} has something in its way");
                self.bring_to_stop(id, pos, vel, Some(cell));
                return;
            };
            // What it carries that isn't solid doesn't hold it up.
            let mut off = Vec::new();
            for (p, _) in all.iter().filter(|(_, s)| !solid(*s)) {
                for to in ways(sign)
                    .map(|step| *p + cell + step)
                    .filter(|to| !own.contains(to))
                {
                    match self
                        .get(to)
                        .map(|there| super::signal::rider_meets(c, there))
                    {
                        Some(RiderMeets::Wall) => off.push(*p),
                        Some(RiderMeets::Breaks) => broken.push(to),
                        _ => {}
                    }
                }
            }
            off.dedup();
            if !off.is_empty() {
                self.with_space(c, id, |d, _| {
                    for p in off {
                        if let Some(s) = d.get(p).filter(|s| *s != AIR) {
                            log::debug!("{} riding on a platform at {p} came off", c.state_name(s));
                            d.drop_block_loot(c, p, s, abi::ItemStack::EMPTY, false, 0);
                            d.set(c, p, AIR);
                        }
                    }
                });
            }
            self.break_for_body(c, broken);
            if let Some(b) = self.bodies.get_mut(&id) {
                b.looked_from = Some((cell, sign));
            }
        }
        if let Some(e) = self.entities.get_mut(id) {
            e.vel = v;
        }
    }

    /// Goes on to the next whole block along the way it was going, or to `at` if it is stopped.
    fn bring_to_stop(&mut self, id: u64, pos: DVec3, vel: DVec3, at: Option<IVec3>) {
        let o = Transform::of(pos).origin();
        let cell = at.unwrap_or_else(|| {
            IVec3::from_array([0, 1, 2].map(|a| {
                (if vel[a] > 1e-9 {
                    (o[a] - 1e-6).ceil()
                } else if vel[a] < -1e-9 {
                    (o[a] + 1e-6).floor()
                } else {
                    o[a].round()
                }) as i32
            }))
        });
        let to = cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        let speed = vel.length().max(2.0);
        let ticks = ((to - pos).length() / speed * crate::world::TPS as f64)
            .ceil()
            .max(1.0) as u16;
        if let Some(e) = self.entities.get_mut(id) {
            e.vel = DVec3::ZERO;
            e.motion = Motion::To { to, left: ticks };
        }
        if let Some(b) = self.bodies.get_mut(&id) {
            b.stopping = true;
        }
    }

    /// A mod wants a body back as blocks, which happens when its tick comes: it may be the one
    /// whose block is asking.
    pub fn ask_put_down(&mut self, id: u64) {
        if id == self.frame {
            self.space.stopping = true;
        } else if let Some(b) = self.bodies.get_mut(&id) {
            b.stopping = true;
        }
    }

    /// All a body has goes back to where it is, as blocks of what it is on: at the whole block it
    /// is at, which it may have been on its way to. Anything else moving on it lands first.
    pub(super) fn put_down_whole(&mut self, c: &'static Content, id: u64) {
        self.in_frame(id, |d| d.finish_moving_here(c));
        let Some(e) = self.entities.get(id).filter(|e| e.motion != Motion::Parked) else { return };
        let (pos, on) = (e.pos, e.frame);
        let Some(b) = self.bodies.get(&id) else { return };
        let (all, was) = (b.all_blocks(), b.cell);
        let blocks: Vec<(IVec3, u32, Option<u64>)> = all
            .into_iter()
            .map(|(local, state)| (local, state, self.entities.anchored(id, local)))
            .collect();
        self.in_frame(on.unwrap_or(0), |d| {
            let place = Transform::of(d.place.to_local(pos));
            let cell = place.cell(if d.frame == 0 { was } else { None });
            d.put_down(
                c,
                id,
                blocks
                    .into_iter()
                    .map(|(local, state, entity)| Carried { local, to: local + cell, state, entity })
                    .collect(),
                Vec::new(),
                None,
            );
        });
    }

    /// Puts what a body carried where it is going: there for circuits now, and for clients when
    /// they see the body there. It stays until then and a little more, not moving. What found
    /// its place taken drops as it would broken. `also`: other cells that changed with it, and
    /// what they were.
    pub fn put_down(&mut self, c: &'static Content, body: u64, blocks: Vec<Carried>, also: Vec<(IVec3, u32)>, turns: Option<(IVec3, u32)>) {
        if let Some(e) = self.entities.get_mut(body) {
            e.motion = Motion::Parked;
            e.vel = DVec3::ZERO;
        }
        let mark = self.space.changes.len();
        let mut cells = also;
        let mut landed = Vec::with_capacity(blocks.len());
        for mut b in blocks {
            let there = self.bodies.get(&body).and_then(|s| s.get(b.local));
            // Broken off on its way.
            if there == Some(AIR) {
                continue;
            }
            // As it is now, having gone on on the way; a head is what it turns into.
            if let Some(now) = there.filter(|_| c.states[b.state as usize].signal.kind != abi::signal_kind::PISTON_HEAD) {
                b.state = now;
            }
            let was = self.get(b.to);
            let signal = self
                .bodies
                .get(&body)
                .map_or_else(Moved::default, |s| s.signals.carry_out(b.local));
            let extras = self
                .bodies
                .get(&body)
                .map_or(Vec::new(), |s| s.extras(b.local).to_vec());
            self.land_block(c, b.to, b.state, b.entity, signal);
            if self.get(b.to) == Some(b.state) {
                self.write_cell(c, b.to, b.state, Some(extras), true);
            }
            if let Some(was) = was.filter(|was| self.get(b.to) != Some(*was)) {
                cells.push((b.to, was));
            }
            landed.push(b);
        }
        // Its circuits and light are the world's again: at once here, in a moment from on a body.
        if self.frame == 0 {
            self.space.signals.set_outside(body, Default::default());
            self.set_body_lights(c, body, Vec::new());
        }
        if let Some(b) = self.bodies.get_mut(&body) {
            b.signals = Default::default();
        }
        let tail = self.space.changes.split_off(mark);
        self.space.changes.extend(
            tail.into_iter()
                .filter(|(p, _)| !cells.iter().any(|(at, _)| at == p)),
        );
        for &(at, was) in &cells {
            self.space.not_shown.insert(at, (was, body));
        }
        let cells = cells.into_iter().map(|(at, _)| at).collect();
        self.space
            .landings
            .push(Landing { body, rests: REST_TICKS, blocks: landed, cells, turns });
    }

    /// Goes where it was going, if that is free: otherwise it breaks there, what it holds with it.
    fn land_block(&mut self, c: &'static Content, at: IVec3, state: u32, entity: Option<u64>, signal: Moved) {
        match self.get(at) {
            // Sides join the sides there, each holding on by itself.
            Some(there) if c.states[there as usize].block == c.states[state as usize].block && c.states[state as usize].sides != 0 => {
                let sides = c.states[there as usize].sides | c.states[state as usize].sides;
                self.write_block(c, at, c.block_of(state).with_sides(state, sides), true);
            }
            Some(there) if there == AIR || c.states[there as usize].flags & abi::block_flags::REPLACEABLE != 0 => {
                if c.states[state as usize].signal.kind == abi::signal_kind::OBSERVER {
                    self.space.signals.moved_observers.insert(at);
                }
                self.put_block(c, at, state, entity, signal);
            }
            _ => self.broke_landing(c, at, state, entity),
        }
    }

    /// A block that couldn't land drops what it does, and what it held.
    pub(super) fn broke_landing(&mut self, c: &'static Content, at: IVec3, state: u32, entity: Option<u64>) {
        if let Some(id) = entity {
            self.drop_inventory(c, id);
            self.entities.remove(id);
        }
        self.drop_block_loot(c, at, state, abi::ItemStack::EMPTY, false, 0);
    }

    /// Landed bodies, a tick on: they get shown, and go.
    pub(super) fn tick_landings(&mut self, c: &'static Content) {
        if self.space.landings.is_empty() && self.space.lingering.is_empty() {
            return;
        }
        let mut lingering = std::mem::take(&mut self.space.lingering);
        lingering.retain_mut(|(body, ticks)| {
            *ticks -= 1;
            if *ticks == 0 {
                self.entities.remove(*body);
                self.bodies.remove(body);
            }
            *ticks > 0
        });
        self.space.lingering = lingering;
        let mut turning = Vec::new();
        let mut shown = Vec::new();
        for s in &mut self.space.landings {
            if s.rests > 1 {
                s.rests -= 1;
                turning.extend(s.turns.take().map(|t| (s.body, t)));
            } else {
                shown.push(s.body);
            }
        }
        for (body, (at, state)) in turning {
            self.set_in_body(c, body, at, state);
        }
        for body in shown {
            if let Some(i) = self.space.landings.iter().position(|s| s.body == body) {
                let s = self.space.landings.swap_remove(i);
                self.show_landed(c, s);
            }
        }
    }

    /// Bodies' places against the world's cells: one moved past the middle of a cell, by more than
    /// half a block from where it counted as being (exactly half is no more, or which way it goes
    /// would show at speeds that land there), gets to the next. Then their circuits and the
    /// world's hear what is next to what. Only the world's blocks, not other bodies'.
    pub(super) fn cross_cells(&mut self, c: &'static Content) {
        let ids: Vec<u64> = self.bodies.keys().copied().collect();
        for id in ids {
            let Some(e) = self.entities.get(id).filter(|e| e.motion != Motion::Parked) else { continue };
            let place = Transform::of(e.pos);
            let Some(b) = self.bodies.get_mut(&id) else { continue };
            let was = b.cell;
            let now = place.cell(was);
            let in_touch = std::mem::replace(&mut b.in_touch, true);
            if in_touch && was == Some(now) {
                continue;
            }
            b.cell = Some(now);
            if was != Some(now) {
                self.crossed(c, id, was, now);
            }
            self.light_from_body(c, id, now);
            self.see_world_light(id);
            self.exchange_signals(c, id);
        }
    }

    /// Body `id` came to be `now` from the world's cells (and was `was`).
    fn crossed(&mut self, c: &'static Content, id: u64, was: Option<IVec3>, now: IVec3) {
        let Some(b) = self.bodies.get(&id) else { return };
        let blocks: Vec<(IVec3, u32)> = b.all_blocks();
        let watched = b.signals.watched();
        for &(at, _) in &blocks {
            self.space.signals.passing(at + now);
            if let Some(w) = was {
                self.space.signals.passing(at + w);
            }
        }
        // Its own observers, looking at what isn't its own: the world moves past them.
        if let (Some(w), Some(b)) = (was, self.bodies.get_mut(&id)) {
            for at in watched {
                if b.get(at).is_none_or(|s| s == AIR) && self.space.get(at + w) != self.space.get(at + now) {
                    b.signals.passing(at);
                }
            }
        }
        for (at, state) in blocks {
            if let Some((owner, cb)) = c.block_of(state).cb.on_carried {
                let cell = at + now;
                self.call(owner, cb, |w| abi::BlockEventArgs {
                    world: w,
                    pos: cell.to_array().into(),
                    state,
                    other: at.to_array().into(),
                    entity: id,
                });
            }
        }
    }

    /// What the world's light is where a body's blocks are, worked out here and now, not left to
    /// whatever clients have heard so far: it is shown with the body from the first. The most
    /// over each of its cubes, per channel.
    fn see_world_light(&mut self, id: u64) {
        let Some(b) = self.bodies.get(&id) else { return };
        let Some(cell) = b.cell else { return };
        let mut lights: FxHashMap<IVec3, u32> = FxHashMap::default();
        for (at, _) in b.all_blocks() {
            let w = at + cell;
            let here = self
                .space
                .cubes
                .get(&cube_of(w))
                .map_or(crate::world::light::SKY_FULL as u32, |cube| cube.light(local_index(w)));
            let most = lights.entry(at >> 4).or_insert(0);
            *most = u32::from_le_bytes(std::array::from_fn(|i| here.to_le_bytes()[i].max(most.to_le_bytes()[i])));
        }
        let Some(b) = self.bodies.get_mut(&id) else { return };
        if b.world_light != lights {
            (b.world_light, b.world_light_changed) = (lights, true);
        }
    }

    /// The light changed somewhere: bodies there look again. Ones landed are the world's blocks and
    /// show as they were.
    pub(super) fn bodies_see_light(&mut self) {
        let ids: Vec<u64> = self
            .bodies
            .iter()
            .filter(|(id, b)| {
                let (Some(cell), Some((lo, hi))) = (b.cell, b.bounds) else { return false };
                if self
                    .entities
                    .get(**id)
                    .is_none_or(|e| e.motion == Motion::Parked)
                {
                    return false;
                }
                let (lo, hi) = (cube_of(lo + cell), cube_of(hi + cell));
                self.space
                    .light_dirty
                    .iter()
                    .any(|d| d.cmpge(lo).all() && d.cmple(hi).all())
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.see_world_light(id);
        }
    }

    /// The world's light from body `id`, which is `cell` from the world's cells now.
    fn light_from_body(&mut self, c: &'static Content, id: u64, cell: IVec3) {
        let lights: Vec<(IVec3, [u8; 3])> = self.bodies.get(&id).map_or(Vec::new(), |b| {
            b.all_blocks()
                .into_iter()
                .filter(|(_, s)| c.states[*s as usize].light != [0; 3])
                .map(|(at, s)| (at + cell, c.states[s as usize].light))
                .collect()
        });
        self.set_body_lights(c, id, lights);
    }

    fn set_body_lights(&mut self, c: &'static Content, id: u64, lights: Vec<(IVec3, [u8; 3])>) {
        let was = self.space.body_lights.remove(&id).unwrap_or_default();
        if was == lights {
            if !was.is_empty() {
                self.space.body_lights.insert(id, was);
            }
            return;
        }
        let touched: Vec<IVec3> = was.iter().chain(lights.iter()).map(|l| l.0).collect();
        if !lights.is_empty() {
            self.space.body_lights.insert(id, lights);
        }
        let Space { cubes, body_light, body_lights, body_light_changes, light_dirty, .. } = &mut *self.space;
        let mut changed = Vec::new();
        for &cell in &touched {
            let all = body_lights
                .values()
                .flatten()
                .filter(|l| l.0 == cell)
                .fold([0u8; 3], |m, l| [m[0].max(l.1[0]), m[1].max(l.1[1]), m[2].max(l.1[2])]);
            let now = (all != [0; 3]).then_some(all);
            if now != body_light.get(&cell).copied() {
                match now {
                    Some(v) => body_light.insert(cell, v),
                    None => body_light.remove(&cell),
                };
                changed.push(cell);
                body_light_changes.push((cell, all));
            }
        }
        let def = &c.dims[self.id as usize];
        let mut map = super::dim::Cubes(cubes);
        let mut lighter = crate::world::light::Lighter::new(&mut map, c, def.sky_light).with_extra(body_light);
        for cell in changed {
            lighter.source_changed(cell);
        }
        let changed = std::mem::take(&mut lighter.changed);
        light_dirty.extend(changed);
    }

    /// What bodies gone gave the world goes with them, however they went; ones whose lamps changed
    /// say so.
    pub(super) fn forget_gone_bodies(&mut self, c: &'static Content) {
        let gone: Vec<u64> = self
            .space
            .body_lights
            .keys()
            .copied()
            .filter(|id| !self.bodies.contains_key(id))
            .collect();
        for id in gone {
            self.set_body_lights(c, id, Vec::new());
        }
        for id in self
            .space
            .signals
            .outside_from()
            .into_iter()
            .filter(|id| !self.bodies.contains_key(id))
        {
            self.space.signals.set_outside(id, Default::default());
        }
        let changed: Vec<(u64, IVec3)> = self
            .bodies
            .iter_mut()
            .filter_map(|(id, b)| {
                std::mem::take(&mut b.emitters_changed)
                    .then_some(())
                    .and(b.cell)
                    .map(|cell| (*id, cell))
            })
            .collect();
        for (id, cell) in changed {
            self.light_from_body(c, id, cell);
        }
    }

    /// What body `id` and the world give each other, as they are next to each other now.
    pub(super) fn exchange_signals(&mut self, c: &'static Content, id: u64) {
        let Some(b) = self.bodies.get_mut(&id) else { return };
        let Some(cell) = b.cell else { return };
        let (giving, around) = {
            let view = super::signal::View { cubes: &b.cubes, on_body: true };
            let around: FxHashSet<IVec3> = b
                .signals
                .cells()
                .flat_map(|p| crate::world::DIRS.map(|d| p + d))
                .filter(|p| b.get(*p).is_none_or(|s| s == AIR))
                .collect();
            (b.signals.giving(c, view, cell), around)
        };
        let world = super::signal::View { cubes: &self.space.cubes, on_body: false };
        let got = self
            .space
            .signals
            .giving_at(c, world, around.into_iter(), cell);
        b.signals.set_outside(0, got);
        self.space.signals.set_outside(id, giving);
    }

    /// Those that have anything to say, after circuits have gone.
    pub(super) fn exchange_all_signals(&mut self, c: &'static Content) {
        let world_changed = std::mem::take(&mut self.space.signals.outputs_changed);
        let ids: Vec<u64> = self
            .bodies
            .iter()
            .filter(|(_, b)| b.signals.has_cells() && (world_changed || b.signals.outputs_changed))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.exchange_signals(c, id);
            if let Some(b) = self.bodies.get_mut(&id) {
                b.signals.outputs_changed = false;
            }
        }
    }

    /// Cubes and bodies are about to go elsewhere or be written: light from bodies comes out of
    /// them, and bodies say it all again, wherever they turn out to be.
    pub(super) fn out_of_touch(&mut self, c: &'static Content) {
        let lit: Vec<u64> = self.space.body_lights.keys().copied().collect();
        for id in lit {
            self.set_body_lights(c, id, Vec::new());
        }
        for id in self.space.signals.outside_from() {
            self.space.signals.set_outside(id, Default::default());
        }
        for b in self.bodies.values_mut() {
            b.in_touch = false;
        }
    }

    pub(super) fn set_in_body(&mut self, c: &'static Content, body: u64, at: IVec3, state: u32) {
        self.in_frame(body, |d| {
            d.write_block(c, at, state, true);
            d.space.relight();
        });
    }

    fn show_landed(&mut self, c: &'static Content, s: Landing) {
        for at in s.cells {
            if self.space.not_shown.get(&at).is_some_and(|n| n.1 == s.body) {
                self.space.not_shown.remove(&at);
                self.resend_cell(at);
            }
        }
        // What has already moved on doesn't stay behind in the body.
        let gone: Vec<IVec3> = s
            .blocks
            .iter()
            .filter(|b| self.get(b.to) != Some(b.state))
            .map(|b| b.local)
            .collect();
        if !gone.is_empty() {
            self.in_frame(s.body, |d| {
                for at in gone {
                    d.write_block(c, at, AIR, true);
                }
                d.space.relight();
            });
        }
        self.space.lingering.push((s.body, LINGER_TICKS));
    }

    /// Bodies waiting to be shown go now.
    pub(super) fn finish_landings(&mut self) {
        for s in std::mem::take(&mut self.space.landings) {
            self.entities.remove(s.body);
            self.bodies.remove(&s.body);
        }
        for (at, _) in std::mem::take(&mut self.space.not_shown) {
            self.resend_cell(at);
        }
        for (body, _) in std::mem::take(&mut self.space.lingering) {
            self.entities.remove(body);
            self.bodies.remove(&body);
        }
    }

    /// Where blocks a client saw on body `frame` really are: one that has landed only looks like
    /// it is still there, and its blocks are in what it landed on.
    pub fn landed_as(&self, frame: u64, pos: IVec3) -> (u64, IVec3) {
        let landed = self
            .entities
            .get(frame)
            .filter(|e| e.motion == Motion::Parked);
        match (landed, self.bodies.get(&frame).and_then(|b| b.cell)) {
            (Some(e), Some(cell)) if e.frame.is_none() => (0, pos + cell),
            _ => (frame, pos),
        }
    }

    /// Runs `f` on the space blocks at `frame` are in: this region (0) or the blocks of body
    /// `frame`, with where that is in the world. It settles after; the world's with the rest of
    /// the action (`Server::settle`).
    pub fn with_space<R>(&mut self, c: &'static Content, frame: u64, f: impl FnOnce(&mut Dim, Transform) -> R) -> Option<R> {
        if frame == 0 {
            return Some(f(self, Transform::default()));
        }
        if self
            .entities
            .get(frame)
            .is_some_and(|e| e.motion == Motion::Parked)
        {
            return None;
        }
        self.in_frame(frame, |d| {
            let place = d.place;
            let out = f(d, place);
            d.settle(c);
            out
        })
    }
}
