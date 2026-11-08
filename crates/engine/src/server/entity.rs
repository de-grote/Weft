//! Entities of one dimension: core state in a dense list, mod components as raw bytes, inventories
//! as an engine contract every mod can read and write.

use crate::tag_bits::entity as ef;
use glam::{DVec3, IVec3};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use weft_abi::ItemStack;

use crate::content::Content;
use crate::physics;

pub struct Entity {
    pub id: u64,
    pub kind: u32,
    pub pos: DVec3,
    pub vel: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub in_liquid: bool,
    /// The last move hit something sideways.
    pub blocked: bool,
    pub fall: f32,
    pub moved: f32,
    pub health: f32,
    pub age: u64,
    pub hurt_cooldown: u32,
    /// Ticks until it may attack again (mobs).
    pub attack_cooldown: u32,
    /// AI steering requested this tick: direction, speed factor, jump.
    pub walk: Option<([f32; 2], f32, bool)>,
    /// Jump strength attribute when its walk was set (1 = vanilla's).
    pub jump_mul: f32,
    pub removed: bool,
    pub item: ItemStack,
    pub pickup_delay: u32,
    pub player: bool,
    pub mode: crate::world::GameMode,
    /// Ticks left burning.
    pub fire: u32,
    /// Hotbar slot the player holds.
    pub selected: u32,
    /// Using what it holds: the ticks that takes, and whether in the other hand. Others see it.
    pub using: (u32, bool),
    pub anchor: Option<IVec3>,
    pub sent: Option<(DVec3, f32, f32, crate::world::Pose, bool)>,
    /// Worn items as last sent to clients.
    pub worn: Vec<u32>,
    /// The solid body it rides (stands on, or flies above within its carry height).
    /// The body it is on: what it rides, or whose blocks hold it if it has an `anchor`.
    pub frame: Option<u64>,
    pub motion: Motion,
    /// A body that falls under its weight and is put down as blocks where it lands, whatever
    /// its kind would do otherwise (`World::body_fall`, every falling block).
    pub falls: bool,
    /// Its own box (width, height, eye) and another kind it looks like, set by mods.
    pub size: Option<weft_abi::EntitySize>,
    pub look: Option<u32>,
    /// Crouching players: a 1.5 high box with lower eyes.
    pub pose: crate::world::Pose,
    /// Held facing this way with its pose (`entity_set_pose`), not turning.
    pub facing: Option<f32>,
    /// Sprinting, as its client says (players).
    pub sprinting: bool,
    /// What a player may do (`abi::permission`).
    pub permission: u32,
    /// Flying, as its client says.
    pub flying: bool,
    /// Left the ground by jumping this tick.
    pub jumped: bool,
    /// Ticks since it last swung or changed items (players): what a hit's strength is judged by.
    pub swing_ticks: u32,
    /// What it held when that was last looked at (`notice_hand`).
    pub in_hand: ItemStack,
    /// What its last hit did while `hurt_cooldown` runs: a stronger one only adds the difference.
    pub last_hurt: f32,
    /// Where it walks to by itself (`Host::navigate`).
    pub nav: Option<super::path::Nav>,
    /// What it rides and in which seat (`riding.rs`).
    pub vehicle: Option<(u64, u8)>,
    /// The player whose client moves it, sitting in a seat that steers.
    pub driver: Option<u64>,
}

/// How a body goes, besides its speed, which is what `Free` goes by.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Motion {
    Free,
    /// To there, in that many ticks.
    To {
        to: DVec3,
        left: u16,
    },
    /// It has landed and is waiting to go: its blocks are the world's now.
    Parked,
}

impl Entity {
    /// The frame the block it belongs to is in: 0 for the world and what isn't a block's.
    pub fn block_frame(&self) -> u64 {
        self.anchor.and(self.frame).unwrap_or(0)
    }

    pub fn new(id: u64, kind: u32, pos: DVec3, health: f32) -> Entity {
        Entity {
            id,
            kind,
            pos,
            vel: DVec3::ZERO,
            blocked: false,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            in_liquid: false,
            fall: 0.0,
            moved: 0.0,
            health,
            age: 0,
            hurt_cooldown: 0,
            attack_cooldown: 0,
            walk: None,
            jump_mul: 1.0,
            removed: false,
            item: ItemStack::EMPTY,
            pickup_delay: 0,
            player: false,
            mode: crate::world::GameMode::SURVIVAL,
            fire: 0,
            selected: 0,
            using: (0, false),
            anchor: None,
            sent: None,
            worn: Vec::new(),
            frame: None,
            motion: Motion::Free,
            falls: false,
            size: None,
            look: None,
            pose: Default::default(),
            facing: None,
            sprinting: false,
            permission: 0,
            flying: false,
            jumped: false,
            swing_ticks: u32::MAX / 2,
            in_hand: ItemStack::EMPTY,
            last_hurt: 0.0,
            nav: None,
            vehicle: None,
            driver: None,
        }
    }

    /// Health never goes below 0 and nothing raises it from there but a respawn, so this is the
    /// one question of being dead.
    pub fn alive(&self) -> bool {
        self.health > 0.0
    }

    /// Its box: its own size if a mod gave it one, else its kind's; crouching lowers either.
    pub fn dims(&self, c: &Content) -> physics::Dims {
        physics::posed(
            self.size
                .unwrap_or(c.game.entities[self.kind as usize].size()),
            self.pose,
        )
    }

    /// Another thing in hand starts the swing over; the same item again (a count, damage or slot
    /// change) doesn't.
    pub fn notice_hand(&mut self, held: ItemStack) {
        if !crate::content::same_in_hand(held, self.in_hand) {
            self.swing_ticks = 0;
        }
        self.in_hand = held;
    }

    pub fn height(&self, c: &Content) -> f64 {
        self.dims(c).height
    }

    pub fn eye(&self, c: &Content) -> f64 {
        self.dims(c).eye
    }

    /// Its collision box now.
    /// What it is doing, as conditions ask (`conditions::Body`).
    pub fn body(&self) -> crate::conditions::Body {
        crate::conditions::Body { in_liquid: self.in_liquid, flying: self.flying, sprinting: self.sprinting }
    }

    pub fn aabb(&self, c: &Content) -> physics::Aabb {
        let physics::Dims { half_width: hw, height: h, .. } = self.dims(c);
        physics::Aabb::around(self.pos, hw, h)
    }

    /// Where items it throws start: just below its eyes (vanilla).
    pub fn drop_point(&self, c: &Content) -> DVec3 {
        self.pos + DVec3::Y * (self.eye(c) - 0.3)
    }

    /// A thing of its own in the world, for others to see: not a block's entity.
    pub fn in_world(&self) -> bool {
        !self.removed && self.anchor.is_none()
    }

    /// Something that stands in the world and can be hit: not a dropped item or a spectator either.
    pub fn is_body(&self) -> bool {
        self.in_world() && self.item.is_empty() && !self.mode.has(crate::tag_bits::game_mode::HIDDEN)
    }

    /// It comes and goes with the world's cube it is in: players have a file of their own, what is
    /// on a body goes with the body.
    pub fn saved_with_cube(&self) -> bool {
        !self.removed && !self.player && self.block_frame() == 0
    }

    pub fn state(&self, c: &Content, max_health: f32) -> weft_abi::EntityState {
        weft_abi::EntityState {
            kind: self.kind,
            flags: if self.blocked { weft_abi::entity_state::BLOCKED } else { 0 },
            pose: self.pose.0 as u32,
            flying: self.flying as u32,
            pos: self.pos.to_array().into(),
            vel: self.vel.to_array().into(),
            yaw: self.yaw,
            pitch: self.pitch,
            on_ground: self.on_ground as u32,
            in_liquid: self.in_liquid as u32,
            sprinting: self.sprinting as u32,
            jumped: self.jumped as u32,
            health: self.health,
            max_health,
            age: self.age,
            fall_distance: self.fall,
            fire: self.fire,
            selected: self.selected,
            moved: self.moved,
            size: self.dims(c).size(),
        }
    }
}

#[derive(Default)]
pub struct Entities {
    pub list: Vec<Entity>,
    index: FxHashMap<u64, usize>,
    pub comps: FxHashMap<(u64, u32), Box<[u8]>>,
    pub inventories: FxHashMap<u64, Vec<ItemStack>>,
    pub dirty_inv: FxHashSet<u64>,
    pub dirty_comp: FxHashSet<(u64, u32)>,
    /// Synced components as last sent, to find what changed (`find_changes`).
    sent: FxHashMap<(u64, u32), Box<[u8]>>,
    /// Block entities by the frame their block is in (0: the world) and where in it, one per
    /// block of the cell that has one, the cell's block first.
    pub anchors: FxHashMap<(u64, IVec3), Vec<u64>>,
    /// Killed this tick: clients play the death animation when they go.
    pub killed: Vec<u64>,
    /// Item entities picked up whole this tick, and by whom (clients pull them in).
    pub taken: Vec<(u64, u64)>,
    /// What changes players whose clients run them (their movement, look, flight, hand), so
    /// they get told.
    pub told: Vec<(u64, Told)>,
}

/// Velocities in blocks per tick, like the client's own.
#[derive(Clone, Copy)]
pub enum Told {
    Knock([f32; 3]),
    Add([f32; 3]),
    Velocity([f32; 3]),
    Look([f32; 2]),
    Flying(bool),
    Selected(u32),
}

/// Falls up to this many blocks do no damage.
pub const SAFE_FALL: f32 = 3.0;

/// What a component of an entity of `kind` starts as: the kind's own value, or the component's.
fn starting_value(c: &Content, kind: &crate::registry::EntityKind, comp: u32) -> Box<[u8]> {
    match kind.values.iter().find(|v| v.0 == comp) {
        Some(v) => v.1.clone(),
        None => c.game.components[comp as usize]
            .default
            .clone()
            .into_boxed_slice(),
    }
}

impl Entities {
    /// What is in its selected slot, whether it can do anything with it or not.
    pub fn in_selected_slot(&self, id: u64) -> weft_abi::ItemStack {
        self.get(id)
            .map_or_else(Default::default, |e| self.inventory_slot(id, e.selected))
    }

    /// What it holds and can use (`content::in_main_hand`).
    pub fn held(&self, id: u64) -> weft_abi::ItemStack {
        let c = crate::content::content();
        self.get(id)
            .zip(self.inventories.get(&id))
            .map_or_else(Default::default, |(e, i)| crate::content::in_main_hand(c, i, e.selected))
    }

    pub fn get(&self, id: u64) -> Option<&Entity> {
        self.index
            .get(&id)
            .map(|&i| &self.list[i])
            .filter(|e| !e.removed)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Entity> {
        self.index
            .get(&id)
            .map(|&i| &mut self.list[i])
            .filter(|e| !e.removed)
    }

    pub fn insert(&mut self, c: &Content, mut e: Entity) {
        let id = e.id;
        // Saved mid-fall, it still lands.
        e.falls |= e.kind == c.game.falling_kind;
        let kind = &c.game.entities[e.kind as usize];
        for &comp in &kind.components {
            self.comps
                .entry((id, comp))
                .or_insert_with(|| starting_value(c, kind, comp));
        }
        if kind.slots > 0 {
            self.inventories
                .entry(id)
                .or_insert_with(|| vec![ItemStack::EMPTY; kind.slots as usize]);
        }
        if e.player {
            self.inventories
                .entry(id)
                .or_insert_with(|| vec![ItemStack::EMPTY; c.game.player_layout.slots as usize]);
        }
        if let Some(a) = e.anchor {
            self.anchors
                .entry((e.block_frame(), a))
                .or_default()
                .push(id);
        }
        self.index.insert(id, self.list.len());
        self.list.push(e);
    }

    /// A new life for an entity that stays, what its kind attaches starts
    /// over, as on `insert`, and modifiers end. The rest goes on: timers are mods' loops (hunger's
    /// pulse is armed once, on joining) and `SIMULATED` at zero would replay all of world time.
    pub fn reset_components(&mut self, c: &Content, id: u64) {
        let Some(kind) = self.get(id).map(|e| &c.game.entities[e.kind as usize]) else { return };
        for &comp in &kind.components {
            if c.game.components[comp as usize].flags & weft_abi::component_flags::KEEP_ON_RESPAWN == 0 {
                self.comps.insert((id, comp), starting_value(c, kind, comp));
            }
        }
        self.comps.remove(&(id, c.game.modifiers_comp));
        if let Some(e) = self.get_mut(id) {
            e.facing = None;
        }
    }

    /// `insert` with the inventory and components it had (loaded, or moved from another region).
    pub fn insert_with(&mut self, c: &Content, e: Entity, inv: Option<Vec<ItemStack>>, comps: Vec<(u32, Box<[u8]>)>) {
        let id = e.id;
        self.insert(c, e);
        if let Some(inv) = inv {
            self.inventories.insert(id, inv);
        }
        let riding = c.game.field_comps[11];
        for (k, v) in comps {
            // A loaded rider sits where it sat.
            if k == riding && v.len() >= 12 {
                let mount = u64::from_le_bytes(v[..8].try_into().unwrap());
                let seat = u32::from_le_bytes(v[8..12].try_into().unwrap()) as u8;
                if let Some(e) = self.get_mut(id).filter(|e| e.vehicle.is_none()) {
                    e.vehicle = Some((mount, seat));
                }
            }
            self.comps.insert((id, k), v);
        }
    }

    /// `id`'s block is at `at` in `frame` now, as it comes to be somewhere else.
    pub fn attach(&mut self, id: u64, frame: u64, at: IVec3, pos: DVec3) {
        let Some(e) = self.get_mut(id) else { return };
        let was = e.anchor.map(|a| (e.block_frame(), a));
        e.anchor = Some(at);
        e.frame = (frame != 0).then_some(frame);
        e.pos = pos;
        if let Some(was) = was {
            self.unanchor(was, id);
        }
        self.anchors.entry((frame, at)).or_default().push(id);
    }

    /// The block entity of the cell at `at` in `frame`: of its block, else of another in it.
    pub fn anchored(&self, frame: u64, at: IVec3) -> Option<u64> {
        self.anchors.get(&(frame, at))?.first().copied()
    }

    /// The block entity of kind `kind` in the cell at `at` in `frame`.
    pub fn anchored_kind(&self, frame: u64, at: IVec3, kind: u32) -> Option<u64> {
        self.anchors
            .get(&(frame, at))?
            .iter()
            .copied()
            .find(|&id| self.get(id).is_some_and(|e| e.kind == kind))
    }

    pub fn unanchor(&mut self, at: (u64, IVec3), id: u64) {
        if let Some(ids) = self.anchors.get_mut(&at) {
            ids.retain(|&x| x != id);
            if ids.is_empty() {
                self.anchors.remove(&at);
            }
        }
    }

    /// Who decides which region `e` is in: the body holding it, if it is a block on one.
    pub fn follows<'a>(&'a self, e: &'a Entity) -> &'a Entity {
        e.frame
            .filter(|_| e.anchor.is_some())
            .and_then(|f| self.get(f))
            .unwrap_or(e)
    }

    /// Block entities on body `id`.
    pub fn on_body(&self, id: u64) -> Vec<u64> {
        self.list
            .iter()
            .filter(|e| !e.removed && e.anchor.is_some() && e.frame == Some(id))
            .map(|e| e.id)
            .collect()
    }

    pub fn remove(&mut self, id: u64) {
        if let Some(e) = self.get_mut(id) {
            e.removed = true;
            if let Some(a) = e.anchor {
                let frame = e.block_frame();
                self.unanchor((frame, a), id);
            }
        }
    }

    /// Drops removed entities and their data. Keeps list order (it is the tick order).
    pub fn compact(&mut self) {
        if !self.list.iter().any(|e| e.removed) {
            return;
        }
        let gone: FxHashSet<u64> = self
            .list
            .iter()
            .filter(|e| e.removed)
            .map(|e| e.id)
            .collect();
        self.list.retain(|e| !e.removed);
        self.index = self
            .list
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        self.comps.retain(|(e, _), _| !gone.contains(e));
        self.sent.retain(|(e, _), _| !gone.contains(e));
        for id in &gone {
            self.inventories.remove(id);
            self.dirty_inv.remove(id);
        }
    }

    /// Marks synced components whose bytes differ from what was last sent, however they changed
    /// (mods write through pointers, systems like energy write directly).
    pub fn find_changes(&mut self, synced: &[bool]) {
        for (k, v) in &self.comps {
            if synced.get(k.1 as usize).copied().unwrap_or(false) && self.sent.get(k) != Some(v) {
                self.dirty_comp.insert(*k);
                self.sent.insert(*k, v.clone());
            }
        }
    }

    /// Takes an entity out entirely (moving it to another dimension).
    /// Whether any of `stack` would fit in its inventory.
    pub fn can_insert(&self, c: &Content, id: u64, stack: ItemStack) -> bool {
        self.inventories.get(&id).is_some_and(|inv| {
            inv.iter().any(|s| {
                s.is_empty()
                    || (s.item == stack.item
                        && s.data == stack.data
                        && s.count < c.items.get(stack.item as usize).map_or(64, |i| i.max_stack))
            })
        })
    }

    pub fn take(&mut self, id: u64) -> Option<(Entity, Option<Vec<ItemStack>>, Vec<(u32, Box<[u8]>)>)> {
        let i = *self.index.get(&id)?;
        let e = self.list.remove(i);
        self.index = self
            .list
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        // Its block is the new region's now; here, the place would still count as taken.
        if let Some(a) = e.anchor {
            self.unanchor((e.block_frame(), a), id);
        }
        let inv = self.inventories.remove(&id);
        let keys: Vec<(u64, u32)> = self.comps.keys().filter(|k| k.0 == id).copied().collect();
        let comps = keys
            .into_iter()
            .filter_map(|k| self.comps.remove(&k).map(|v| (k.1, v)))
            .collect();
        self.sent.retain(|k, _| k.0 != id);
        Some((e, inv, comps))
    }

    /// Canonical tick order after regions merge.
    pub fn sort_by_id(&mut self) {
        self.list.sort_by_key(|e| e.id);
        self.index = self
            .list
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
    }

    pub fn inventory_slot(&self, owner: u64, slot: u32) -> ItemStack {
        self.inventories
            .get(&owner)
            .and_then(|i| i.get(slot as usize))
            .copied()
            .unwrap_or_default()
    }

    pub fn set_slot(&mut self, owner: u64, slot: u32, stack: ItemStack) {
        if let Some(inv) = self.inventories.get_mut(&owner) {
            if let Some(s) = inv.get_mut(slot as usize) {
                *s = if stack.count == 0 { ItemStack::EMPTY } else { stack };
                self.dirty_inv.insert(owner);
            }
        }
    }

    /// Merges into matching stacks, then empty slots. Returns the rest.
    pub fn insert_item(&mut self, c: &Content, owner: u64, mut stack: ItemStack) -> ItemStack {
        let max = c.max_stack(stack.item);
        // Pickups never go into equipment.
        let Some(e) = self.get(owner) else { return stack };
        let off = c.game.player_layout.off.filter(|_| e.player);
        let worn = c.game.entities[e.kind as usize].worn.clone();
        let Some(inv) = self.inventories.get_mut(&owner) else { return stack };
        for (_, s) in inv
            .iter_mut()
            .enumerate()
            .filter(|(i, _)| !worn.contains(&(*i as u32)))
        {
            if stack.count == 0 {
                break;
            }
            if s.item == stack.item && s.data == stack.data && s.count < max && !s.is_empty() {
                let n = (max - s.count).min(stack.count);
                s.count += n;
                stack.count -= n;
            }
        }
        // Nor into an empty other hand.
        for (_, s) in inv
            .iter_mut()
            .enumerate()
            .filter(|(i, _)| !worn.contains(&(*i as u32)) && off != Some(*i as u32))
        {
            if stack.count == 0 {
                break;
            }
            if s.is_empty() {
                let n = max.min(stack.count);
                *s = ItemStack { count: n, ..stack };
                stack.count -= n;
            }
        }
        self.dirty_inv.insert(owner);
        if stack.count == 0 { ItemStack::EMPTY } else { stack }
    }

    /// Within `radius`, of `kind` (`u32::MAX`: any), `abi::near` saying what else.
    /// In `shape` (in the frame `place` turns world positions into), of `kind` (`u32::MAX`:
    /// any), `abi::entity_filter` saying what else.
    pub fn within(&self, c: &Content, place: &crate::physics::Transform, shape: &crate::shape::Shape, kind: u32, flags: u32) -> Vec<u64> {
        use weft_abi::entity_filter as f;
        let (targets, touching) = (flags & f::TARGETS != 0, flags & f::TOUCHING != 0);
        self.list
            .iter()
            .filter(|e| !e.removed && (kind == u32::MAX || e.kind == kind))
            .filter(|e| !targets || (e.is_body() && e.alive()))
            .filter(|e| {
                if touching {
                    let b = e.aabb(c);
                    let (a, z) = (place.to_local(b.min), place.to_local(b.max));
                    shape.test_box(a.min(z), a.max(z)).1
                } else {
                    shape.contains(place.to_local(e.pos))
                }
            })
            .map(|e| e.id)
            .collect()
    }
}

/// Whether a move into a liquid makes a splash: from out of it, falling faster than a tenth of a
/// block a tick.
pub fn splashes(was_in_liquid: bool, in_liquid: bool, falling: f64) -> bool {
    !was_in_liquid && in_liquid && falling > 0.1
}

/// What a fall from that high does to a kind, the same for players and mobs.
pub fn fall_damage(kind: &crate::registry::EntityKind, fell: f32) -> Option<f32> {
    (kind.is(ef::FALL_DAMAGE) && fell > SAFE_FALL).then(|| fell - SAFE_FALL)
}

/// What a step did besides moving: the fall it landed from, and whether a crushing body
/// squeezed it into blocks.
pub struct Stepped {
    pub land: Option<f32>,
    pub crushed: bool,
}

/// One physics step (1/20 s) for a non-player entity. `block` gives states, `None` = unloaded;
/// `bodies` are the solid bodies that moved this tick before it.
/// Distance fallen, the same for players and mobs: a fall ends on the ground, and liquids and
/// ladders take it away. Returns how far it fell when it just landed.
pub fn track_fall(fall: &mut f32, dy: f64, on_ground: bool, safe: bool) -> Option<f32> {
    if safe {
        *fall = 0.0;
        return None;
    }
    if on_ground {
        return Some(std::mem::take(fall)).filter(|&f| f > 0.0);
    }
    if dy < 0.0 {
        *fall += (-dy) as f32;
    }
    None
}

pub fn step(c: &Content, e: &mut Entity, gravity: f64, block: &impl physics::Cells, bodies: &[physics::Body]) -> Stepped {
    if let Some(y) = e.facing {
        e.yaw = y;
    }
    const DT: f64 = crate::world::TICK;
    let k = &c.game.entities[e.kind as usize];
    let mut out = Stepped { land: None, crushed: false };
    // A rider goes where its seat is, a mount a player steers where that player's client says.
    if e.anchor.is_some() || e.vehicle.is_some() || e.driver.is_some() {
        return out;
    }
    if (k.is(ef::KINEMATIC) && !e.falls) || e.motion == Motion::Parked {
        // On another body, it goes with that too (a piston on a platform moving blocks).
        let carried = e
            .frame
            .and_then(|f| bodies.iter().find(|b| b.id == f))
            .map_or(DVec3::ZERO, |b| b.delta);
        e.pos += carried;
        match &mut e.motion {
            Motion::To { to, left } => {
                *to += carried;
                let arrived = *left <= 1;
                let at = *to;
                e.vel = (at - e.pos) / (*left as f64 * DT);
                *left = left.saturating_sub(1);
                if arrived {
                    (e.pos, e.vel, e.motion) = (at, DVec3::ZERO, Motion::Free);
                    return out;
                }
            }
            Motion::Parked => return out,
            Motion::Free => {}
        }
        e.pos += e.vel * DT;
        e.moved = (e.vel.with_y(0.0) * DT).length() as f32;
        return out;
    }
    let physics::Dims { half_width: hw, height: h, .. } = e.dims(c);
    let moved_by = physics::carried(c, block, &mut e.pos, hw, h, e.on_ground, &mut e.frame, bodies, e.id);
    if let Some(d) = moved_by.released {
        e.vel += d / DT;
    }
    out.crushed = moved_by.crushed;
    // After riding: falls and walking are measured relative to the body.
    let start = e.pos;
    // The step players take, in blocks per tick: a mob walks at its target speed on plain
    // ground, faster on ice, slower in lava, and climbs ladders it walks into.
    let mut ctl = physics::Control { swim: k.is(ef::SWIMS), ..Default::default() };
    if let Some((dir, speed, jump)) = e.walk.take() {
        let target = k.speed as f64 * speed as f64 * DT;
        ctl.wish = DVec3::new(dir[0] as f64, 0.0, dir[1] as f64);
        // Vanilla's ground friction keeps 0.546 of the speed a tick; this much a tick holds it.
        ctl.ground = target * (1.0 - 0.6 * 0.91);
        ctl.air = 0.02;
        if jump {
            ctl.jump = physics::JUMP * e.jump_mul as f64;
            ctl.swim = true;
        }
    }
    let frame = physics::Frame {
        half_width: hw,
        height: h,
        step: if e.item.is_empty() { physics::STEP_UP } else { 0.0 },
        gravity: if k.is(ef::NO_GRAVITY) && !e.falls { 0.0 } else { physics::gravity_per_tick(gravity) },
    };
    let mut v = e.vel * DT;
    let r = physics::walk(c, block, bodies, e.id, &mut e.pos, &mut v, e.on_ground, &ctl, &frame);
    e.vel = v / DT;
    e.jumped = r.jumped;
    e.blocked = r.hit[0] || r.hit[2];
    e.on_ground = r.on_ground;
    if r.on_ground {
        if let Some(b) = physics::standing_on(c, e.pos, hw, bodies, e.id) {
            e.frame = Some(b);
        }
    }
    let dy = e.pos.y - start.y;
    e.moved = ((e.pos.x - start.x).powi(2) + (e.pos.z - start.z).powi(2)).sqrt() as f32;
    let here = if physics::same_around(start, e.pos, h) { r.started_in } else { physics::around(c, block, e.pos, h) };
    e.in_liquid = here.in_liquid;
    if let Some(land) = track_fall(&mut e.fall, dy, e.on_ground, here.in_liquid || here.climbing) {
        out.land = Some(land);
    }
    out
}

/// Soft bodies (`PUSHABLE`) overlapping each other drift apart like vanilla mobs: a push of up
/// to 1 block/s per neighbor. `pushers` may push without being pushed (players move on their
/// clients). Returns the velocity change per entity.
/// Vanilla's `maxEntityCramming`.
const MAX_CRAMMING: u32 = 24;

pub fn soft_push(c: &Content, list: &[Entity]) -> Vec<(usize, DVec3)> {
    let soft = |e: &Entity| {
        let k = &c.game.entities[e.kind as usize];
        k.is(ef::PUSHABLE) && !k.is(ef::SOLID) && e.is_body() && e.alive()
    };
    let mut cells: FxHashMap<(i32, i32), Vec<usize>> = FxHashMap::default();
    for (i, e) in list.iter().enumerate().filter(|(_, e)| soft(e)) {
        cells
            .entry(((e.pos.x / 2.0).floor() as i32, (e.pos.z / 2.0).floor() as i32))
            .or_default()
            .push(i);
    }
    list.par_iter()
        .with_min_len(256)
        .enumerate()
        .filter(|(_, e)| soft(e) && !e.player)
        .filter_map(|(i, e)| {
            let (cx, cz) = ((e.pos.x / 2.0).floor() as i32, (e.pos.z / 2.0).floor() as i32);
            let mut push = DVec3::ZERO;
            // Each mob feels at most this many others (vanilla's cramming limit), so a thousand in
            // one spot cost a thousand times that, not a million pairs.
            let (mut felt, mut looked) = (0, 0);
            'near: for dz in -1..=1 {
                for dx in -1..=1 {
                    for &j in cells.get(&(cx + dx, cz + dz)).into_iter().flatten() {
                        looked += 1;
                        if felt == MAX_CRAMMING || looked > 4 * MAX_CRAMMING {
                            break 'near;
                        }
                        let o = &list[j];
                        let reach = e.dims(c).half_width + o.dims(c).half_width;
                        let d = e.pos - o.pos;
                        if j == i || d.x.abs() >= reach || d.z.abs() >= reach || d.y >= o.height(c) || -d.y >= e.height(c) {
                            continue;
                        }
                        // Exactly on top of each other: split by id so they still drift apart.
                        let d = if d.x.abs() + d.z.abs() < 1e-4 { DVec3::new(if e.id < o.id { -0.01 } else { 0.01 }, d.y, 0.0) } else { d };
                        push += physics::nudge(d);
                        felt += 1;
                    }
                }
            }
            (push != DVec3::ZERO).then_some((i, push * crate::world::TPS as f64))
        })
        .collect()
}
