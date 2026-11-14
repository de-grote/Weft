use crate::abi::{self, Holder, ItemStack, Slice, Str};
use crate::abi::{BlockPos, Vec3};
use crate::component::{Component, Storage, Value};
use crate::content::{
    Block, Declaration, EntityCategory, EntityKind, EquipmentSlot, GizmoGroup, HudElement, Menu, Pending, RecipeKind, TintKind, WordSet,
};
use crate::ui::{Ui, ui_cb};
use crate::{Id, Registry, WithComponents, World, callback, comps, host, qualify};

pub use abi::{EYE_SHARE, Energy, EntityState, component_flags, damage_cause, entity_state, slot_flags};

/// The engine's player kind, for hooks and components on players.
pub static PLAYER: EntityKind = EntityKind::named("core:player");
/// The engine's kind for dropped stacks (`World::spawn_item`).
pub static ITEM: EntityKind = EntityKind::named("core:item");

/// How a spawned entity starts (`World::spawn_with`); what isn't set is the kind's default.
#[derive(Clone, Copy, Debug)]
pub struct Spawn {
    desc: abi::SpawnEntityDesc,
}

impl Default for Spawn {
    fn default() -> Self {
        Spawn::new()
    }
}

impl Spawn {
    pub fn new() -> Spawn {
        Spawn {
            desc: abi::SpawnEntityDesc {
                kind: 0,
                pos: [0.0; 3].into(),
                set: 0,
                stack: ItemStack::EMPTY,
                velocity: [0.0; 3].into(),
                yaw: 0.0,
                pickup_delay: 0,
                cause: abi::spawn_cause::MOD,
                owner: 0,
            },
        }
    }
    /// Who it belongs to: a projectile's shooter, which it doesn't hit as it leaves them and
    /// whose hits its hits are.
    pub fn owner(mut self, id: u64) -> Self {
        self.desc.owner = id;
        self
    }
    /// Why it comes (`spawn_cause`, `MOD` unless said), what `actions::SPAWN` is told.
    pub fn cause(mut self, cause: u32) -> Self {
        self.desc.cause = cause;
        self
    }
    /// What an item entity is.
    pub fn stack(mut self, s: ItemStack) -> Self {
        self.desc.stack = s;
        self
    }
    /// Blocks per second.
    pub fn velocity(mut self, v: impl Into<Vec3>) -> Self {
        self.desc.velocity = v.into();
        self.desc.set |= abi::spawn_set::VELOCITY;
        self
    }
    /// Adds its owner's current velocity once at spawn, including a mount or moving platform.
    /// Combine with `owner(id)` and `velocity(v)` to launch relative to the shooter.
    pub fn inherit_velocity(mut self) -> Self {
        self.desc.set |= abi::spawn_set::INHERIT_VELOCITY;
        self
    }
    /// Radians, like `EntityState::yaw`.
    pub fn yaw(mut self, radians: f32) -> Self {
        self.desc.yaw = radians;
        self.desc.set |= abi::spawn_set::YAW;
        self
    }
    /// Ticks before an item can be picked up (10 by default, 40 for one a player threw).
    pub fn pickup_delay(mut self, ticks: u32) -> Self {
        self.desc.pickup_delay = ticks;
        self.desc.set |= abi::spawn_set::PICKUP_DELAY;
        self
    }
}

/// A hurt described (`World::damage`): `Damage::new(4.0, &damage_causes::ATTACK).by(shooter)
/// .through(arrow)`. Steps see all of it (`Query::other`, `direct`, `item`, `crit`).
#[derive(Clone, Copy, Debug)]
pub struct Damage(pub(crate) abi::Damage);

impl Damage {
    pub fn new(amount: f32, cause: impl Into<Id<crate::DamageCause>>) -> Damage {
        Damage(abi::Damage::new(amount, cause.into().raw()))
    }
    /// Who did it.
    pub fn by(mut self, source: u64) -> Self {
        self.0.source = source;
        self
    }
    /// What hit, when not `by` itself (the arrow): knockback goes away from it.
    pub fn through(mut self, direct: u64) -> Self {
        self.0.direct = direct;
        self
    }
    /// What it was done with.
    pub fn with(mut self, item: ItemStack) -> Self {
        self.0.item = item;
        self
    }
    /// The way it pushes, instead of away from what hit.
    pub fn knock(mut self, way: impl Into<Vec3>) -> Self {
        self.0.knock = way.into();
        self
    }
    pub fn crit(mut self) -> Self {
        self.0.crit = 1;
        self
    }
    /// The part of the hitbox it lands on (`hitbox_part::HEAD`, `hitbox_part_id("tail")`).
    pub fn part(mut self, part: u32) -> Self {
        self.0.part = part;
        self
    }
}

// ---------------------------------------------------------------- world helpers

impl World {
    /// Entity `id` plays keyframe animation `name` for everyone who sees it (looping until
    /// another starts); an empty name goes back to idle and walk.
    pub fn action_play(&self, id: u64, name: &str, looping: bool) {
        unsafe { (host().action_play)(self.raw(), id, Str::new(name), looping as u32) }
    }

    /// Stops the action `action_play` started.
    pub fn action_stop(&self, id: u64) {
        self.action_play(id, "", false);
    }

    /// While this holds, entity `id` plays action `to` wherever it would play `from`: a mob
    /// running from a cat plays its own run instead of `walk`. Everyone who sees it sees it, and
    /// without a clip for `to` it goes on playing `from`.
    pub fn action_override(&self, id: u64, from: &str, to: &str) {
        unsafe { (host().action_override)(self.raw(), id, Str::new(from), Str::new(to)) }
    }

    /// Ends `action_override`.
    pub fn action_override_clear(&self, id: u64, from: &str) {
        unsafe { (host().action_override)(self.raw(), id, Str::new(from), Str::new("")) }
    }

    pub fn light(&self, pos: impl Into<BlockPos>) -> u32 {
        let pos = pos.into();
        unsafe { (host().light)(self.raw(), &pos) }
    }
    /// Biome id at `pos` (`Biome::id`, `Tag::on_biome`); underground biomes differ from the
    /// surface's.
    pub fn biome(&self, pos: impl Into<BlockPos>) -> u32 {
        let pos = pos.into();
        unsafe { (host().biome_at)(self.raw(), &pos) }
    }
    /// Sky light level at `pos` (0..15).
    pub fn sky_light(&self, pos: impl Into<BlockPos>) -> u32 {
        abi::light::sky(self.light(pos.into()))
    }
    /// Strongest block light channel at `pos`.
    pub fn block_light(&self, pos: impl Into<BlockPos>) -> u32 {
        abi::light::block(self.light(pos.into()))
    }
    pub fn daylight(&self) -> f32 {
        unsafe { (host().daylight)(self.raw()) }
    }
    /// An entity of a kind (a handle, or its id); 0 if there is no such kind.
    pub fn spawn(&self, kind: impl Into<Id<EntityKind>>, pos: impl Into<Vec3>) -> u64 {
        let pos = pos.into();
        self.spawn_with(kind, pos, &Spawn::new())
    }
    /// `spawn` with what it starts with: `Spawn::new().velocity(v).yaw(1.57)`, and for an item
    /// entity its stack.
    pub fn spawn_with(&self, kind: impl Into<Id<EntityKind>>, pos: impl Into<Vec3>, how: &Spawn) -> u64 {
        let pos = pos.into();
        let desc = abi::SpawnEntityDesc { kind: kind.into().raw(), pos, ..how.desc };
        unsafe { (host().entity_spawn)(self.raw(), &desc) }
    }
    pub fn despawn(&self, id: u64) {
        unsafe { (host().entity_despawn)(self.raw(), id) }
    }
    pub fn entity(&self, id: u64) -> Option<EntityState> {
        let mut s = EntityState::default();
        (unsafe { (host().entity_get)(self.raw(), id, &mut s) } != 0).then_some(s)
    }
    /// Places `template` with its lowest corner at `pos` once placed with `transform`, its slots
    /// filled from `palette` (else its defaults). What undoes it (`unplace`), `None` (and nothing
    /// placed) when part of it isn't loaded.
    pub fn place_template(
        &self,
        template: &crate::Template,
        pos: impl Into<BlockPos>,
        transform: crate::Transform,
        palette: Option<&crate::Palette>,
    ) -> Option<u64> {
        let pos = pos.into();
        let palette = Str::new(palette.map_or("", |p| p.name()));
        let id = unsafe { (host().place)(self.raw(), Str::new(template.name()), &pos, transform.0 as u32, palette) };
        (id != 0).then_some(id)
    }
    /// Puts feature `feature` into the world now as if it started at `pos` (its plan or template,
    /// with what they bring). What undoes it, `None` when part of it isn't loaded.
    pub fn place_feature(&self, feature: &crate::Feature, pos: impl Into<BlockPos>) -> Option<u64> {
        let pos = pos.into();
        let id = unsafe { (host().place)(self.raw(), Str::new(feature.name()), &pos, 0, Str::new("")) };
        (id != 0).then_some(id)
    }
    /// Puts back what a placing replaced (the last 16 of a dimension are kept); whether it could.
    pub fn unplace(&self, placing: u64) -> bool {
        unsafe { (host().unplace)(self.raw(), placing) != 0 }
    }
    /// Changes what the entity is now, only what is named, when the edit is dropped:
    /// `w.entity_modify(id).velocity(v).yaw(y);`. Players' own clients follow.
    pub fn entity_modify(&self, id: u64) -> EntityModify {
        EntityModify { world: *self, id, set: 0, state: EntityState::DEFAULT }
    }
    /// Adds `v` (blocks per second) to its velocity, players included: a trampoline, a blast.
    pub fn push(&self, id: u64, v: impl Into<Vec3>) {
        let v = v.into();
        unsafe { (host().entity_push)(self.raw(), id, &v) }
    }
    /// The entity's own box; `None` goes back to its kind's. Crouching still shrinks it.
    pub fn set_size(&self, id: u64, size: Option<abi::EntitySize>) {
        unsafe { (host().set_entity_size)(self.raw(), id, size.unwrap_or_default()) }
    }
    /// Draws the entity as another kind (model, texture, animations); `None` as itself. It stays
    /// the same entity with its own behavior.
    pub fn set_appearance(&self, id: u64, kind: Option<&str>) {
        unsafe { (host().set_entity_appearance)(self.raw(), id, Str::new(kind.unwrap_or(""))) }
    }
    /// The box entities of a kind have.
    pub fn kind_size(&self, kind: &str) -> Option<abi::EntitySize> {
        let mut out = abi::EntitySize::default();
        (unsafe { (host().entity_kind_size)(Str::new(kind), &mut out) } != 0).then_some(out)
    }
    /// `id` walks to `target` by itself, around obstacles, at `speed` times its walking speed
    /// (0 stops it). Call it again as the target moves. The path is found later in the tick;
    /// `navigating` turns false when the walk ends (arrived, or stuck for two seconds).
    pub fn navigate(&self, id: u64, target: impl Into<Vec3>, speed: f32) -> bool {
        let target = target.into();
        unsafe { (host().navigate)(self.raw(), id, &target, speed) != 0 }
    }
    /// A number of its own for each time a world is opened. Entity ids and time start over in
    /// another world: state a mod keeps by them in its own memory goes by this too.
    pub fn session(&self) -> u64 {
        unsafe { (host().world_session)(self.raw()) }
    }
    /// The world's seed, the one its terrain comes from. The same in every dimension.
    pub fn seed(&self) -> u64 {
        unsafe { (host().world_seed)(self.raw()) }
    }
    /// A random number that belongs to this block of this world and never changes: what a chest
    /// should hold when first opened, a villager's trades. Several from one spot:
    /// `noise::hash(w.seed_at(pos), 1, 0, 0)`, and so on. On a body `pos` is in the body's
    /// coordinates, like anything there.
    pub fn seed_at(&self, pos: impl Into<BlockPos>) -> u64 {
        let pos = pos.into();
        crate::noise::hash(self.seed(), pos.x as i64, pos.y as i64, pos.z as i64)
    }
    /// The same for the whole cube (16 blocks each way) `pos` is in.
    pub fn seed_of_cube(&self, pos: impl Into<BlockPos>) -> u64 {
        let pos = pos.into();
        crate::noise::hash(self.seed() ^ 0x5EED_C0BE, (pos.x >> 4) as i64, (pos.y >> 4) as i64, (pos.z >> 4) as i64)
    }
    /// Lowest corner of the nearest structure `name` (full name) within `radius` blocks of
    /// `pos`, generated yet or not.
    pub fn structure_near(&self, name: &str, pos: impl Into<BlockPos>, radius: i32) -> Option<BlockPos> {
        let pos = pos.into();
        let mut out = BlockPos::ZERO;
        (unsafe { (host().structure_near)(self.raw(), Str::new(name), &pos, radius, &mut out) } != 0).then_some(out)
    }
    /// The block at `pos` falls as an entity (sand, gravel) and is placed again where it lands:
    /// `body_fall` of a body of that one block.
    pub fn fall_block(&self, pos: impl Into<BlockPos>) -> u64 {
        let pos = pos.into();
        unsafe { (host().fall_block)(self.raw(), &pos) }
    }
    /// Whether `id` is still walking where `navigate` sent it.
    pub fn navigating(&self, id: u64) -> bool {
        unsafe { (host().navigating)(self.raw(), id) != 0 }
    }
    /// Moves `id` this tick as a player's keys would (`Walk`): the same physics and poses.
    pub fn walk(&self, id: u64, w: Walk) {
        unsafe { (host().entity_walk)(self.raw(), id, &w.dir, w.speed, w.keys) }
    }
    /// What a ray from `from` along `dir` hits first within `max` blocks, of what `ray` looks for
    /// (`Ray::blocks()`, `.liquids()`, `.entities()`, `.skip(id)`): a block or an entity, the face
    /// it went in by and where on it (`RayHit::local`). A turret's line, a mob checking whether it
    /// sees a player, which part of a block a player clicked.
    pub fn raycast(&self, from: impl Into<Vec3>, dir: impl Into<Vec3>, max: f64, ray: Ray) -> Option<abi::RayHit> {
        let (from, dir) = (from.into(), dir.into());
        let mut out = abi::RayHit::default();
        let hit = unsafe { (host().raycast)(self.raw(), &from, &dir, max, ray.flags, ray.skip, &mut out) };
        (hit != 0).then_some(out)
    }
    /// What `id` looks at within `max` blocks: `raycast` from its eyes along its look, past itself.
    pub fn looked_at(&self, id: u64, max: f64, ray: Ray) -> Option<abi::RayHit> {
        let s = self.entity(id)?;
        self.raycast(s.eye(), crate::look_dir(s.yaw, s.pitch), max, ray.skip(id))
    }
    /// Up to its maximum: `damage` with a negative amount.
    pub fn heal(&self, id: u64, amount: f32) -> bool {
        self.damage(id, Damage::new(-amount.max(0.0), &crate::damage_causes::HEAL))
    }
    /// Hurts `id` as `d` says: the `HURT` question, `question::RECOVERING`, the `DAMAGE` chain,
    /// knockback, the `actions::HURT` event, death as the cause says. A negative amount heals
    /// instead, through the `HEAL` chain and the `ENTITY_HEALED` event, with none of damage's
    /// consequences. True if health changed.
    pub fn damage(&self, id: u64, d: Damage) -> bool {
        unsafe { (host().entity_damage)(self.raw(), id, &d.0) != 0 }
    }
    /// `id` hits `target` with what it holds, which wears, or bare handed with its kind's
    /// `.attack_damage(..)`, as strong as its swing is charged (`swing_strength`, by its
    /// `.attack_speed(..)` when bare handed). Other mods can forbid it (`question::ATTACK`) or
    /// change it (`attribute::ATTACK_DAMAGE`, `chain::MELEE`, `question::CRIT`). True if it landed.
    pub fn attack(&self, id: u64, target: u64) -> bool {
        unsafe { (host().entity_attack)(self.raw(), id, target) != 0 }
    }
    /// Whether `target` is within `id`'s reach (the `ENTITY_REACH` attribute, from its eyes to
    /// the nearest point of the target's box): the same test for players and mobs.
    pub fn in_reach(&self, id: u64, target: u64) -> bool {
        unsafe { (host().entity_in_reach)(self.raw(), id, target) != 0 }
    }

    /// Whether that point is under a liquid's surface, which a block that isn't full lies below
    /// its top: for what the block alone can't tell, like a head above shallow water.
    pub fn in_liquid(&self, pos: impl Into<Vec3>) -> bool {
        let pos = pos.into();
        unsafe { (host().in_liquid)(self.raw(), &pos) != 0 }
    }
    /// `id` uses what is in its main hand (or the other one, `off_hand`), on nothing, as a
    /// player's right click in the air does: a witch drinking, a skeleton drawing its bow. True if
    /// it did something or started to; what takes time goes on while it holds the item.
    pub fn use_item(&self, id: u64, off_hand: bool) -> bool {
        unsafe { (host().entity_use_item)(self.raw(), id, off_hand as u32, std::ptr::null(), 0) != 0 }
    }
    /// `id` lets go of what it is using, as a player's use button going up: a drawn bow shoots
    /// (`on_release`, after how long it was held).
    pub fn release_item(&self, id: u64) {
        unsafe { (host().entity_release_use)(self.raw(), id) }
    }
    /// `use_item` on the block at `pos`, from `face` (-X, +X, -Y, +Y, -Z, +Z): a bucket filled.
    pub fn use_item_on(&self, id: u64, off_hand: bool, pos: impl Into<BlockPos>, face: u32) -> bool {
        let pos = pos.into();
        unsafe { (host().entity_use_item)(self.raw(), id, off_hand as u32, &pos, face) != 0 }
    }
    /// `id` starts breaking the block at `pos` as a player holding the button does: as fast as
    /// what it holds mines it (`attribute::MINING_SPEED`), cracks showing, broken when done if
    /// still within its reach, the tool wearing. False when it may not (`actions::BREAK_BLOCK`).
    pub fn start_mining(&self, id: u64, pos: impl Into<BlockPos>) -> bool {
        let pos = pos.into();
        unsafe { (host().entity_mine)(self.raw(), id, &pos) != 0 }
    }
    /// `id` stops breaking what it was.
    pub fn stop_mining(&self, id: u64) {
        unsafe { (host().entity_mine)(self.raw(), id, std::ptr::null()) };
    }
    /// What player `player` holds of action `key` (yours from `Registry::key`, or `keys::*`, the
    /// mouse buttons' `keys::USE` and `keys::ATTACK` too): `held` and `held_ticks`, as its client
    /// last said. Presses and releases come as `KEY` events.
    pub fn key(&self, player: u64, key: &crate::Key) -> abi::KeyState {
        let mut out = abi::KeyState::default();
        unsafe { (host().entity_key)(self.raw(), player, Str::new(key.name()), &mut out) };
        out
    }
    /// How charged `id`'s next swing is, 0..1, with what it holds now.
    pub fn swing_strength(&self, id: u64) -> f32 {
        unsafe { (host().entity_swing_strength)(self.raw(), id) }
    }
    /// The value of `c` on any holder: an entity, a block's entity, a stack in a slot.
    pub fn component<T: Value>(&self, h: impl Into<Holder>, c: &Component<T>) -> Option<T> {
        let mut buf = vec![0u8; size_of::<T>()];
        let n = unsafe { (host().component_get)(self.raw(), &h.into(), c.id(), buf.as_mut_ptr(), buf.len() as u32) };
        (n as usize == buf.len()).then(|| unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const T) })
    }
    /// Sets `c` on any holder. An entity has it only if its kind attaches it; a stack takes any.
    pub fn set_component<T: Value>(&self, h: impl Into<Holder>, c: &Component<T>, v: T) -> bool {
        let bytes = crate::component::value_bytes(&v);
        unsafe { (host().component_set)(self.raw(), &h.into(), c.id(), Slice::new(&bytes)) != 0 }
    }
    /// An entity's component changed in place (a stack's is a value: `set_component`); changes
    /// are saved and synced like any other.
    pub fn component_mut<T: Value>(&self, id: u64, c: &Component<T>) -> Option<&mut T> {
        let p = unsafe { (host().component)(self.raw(), id, c.id()) };
        (!p.is_null()).then(|| unsafe { &mut *(p as *mut T) })
    }
    /// Holds `id` in `pose` (a pet sitting, a zombie crawling, a player in a chair; its client
    /// follows), or with `None` gives it back to what its movement makes.
    /// With `facing` (degrees, feet that way when lying) it also stays still and turned so: in a bed.
    pub fn set_pose(&self, id: u64, pose: Option<&crate::Pose>, facing: Option<f32>) {
        let yaw = facing.map_or(f32::NAN, f32::to_radians);
        unsafe { (host().entity_set_pose)(self.raw(), id, pose.map_or(u32::MAX, |p| p.id()), yaw) }
    }
    /// A player in creative mode.
    pub fn creative(&self, id: u64) -> bool {
        self.game_mode(id) == abi::game_mode::CREATIVE
    }
    /// What is spared damage, hunger and air (creative, spectator, `attribute::INVULNERABLE`).
    pub fn outside_survival(&self, id: u64) -> bool {
        self.attribute(id, &crate::attribute::INVULNERABLE) >= 1.0
    }
    /// The id of its game mode (`game_modes::CREATIVE.id()`): survival for what isn't a player.
    pub fn game_mode(&self, id: u64) -> u32 {
        unsafe { (host().game_mode)(self.raw(), id) }
    }
    /// Energy (the `ENERGY` component, read and set like any other) moves `amount` in (positive) or out (negative) within capacity and rate limits; returns
    /// what moved (or would, with `simulate`), with the same sign.
    pub fn energy_move(&self, h: impl Into<Holder>, amount: i64, simulate: bool) -> i64 {
        unsafe { (host().energy_move)(self.raw(), &h.into(), amount, simulate as u32) }
    }

    pub fn players(&self) -> Vec<u64> {
        self.players_iter().collect()
    }
    /// `players` without collecting them (mob AI asks every tick).
    pub fn players_iter(&self) -> impl Iterator<Item = u64> + '_ {
        let n = unsafe { (host().player_count)(self.raw()) };
        (0..n).map(move |i| unsafe { (host().player_at)(self.raw(), i) })
    }
    /// The inventory of an entity, a block (`[x, y, z]`, through its block entity) or a held
    /// stack (`Holder::held(player, slot)`, a backpack).
    pub fn inventory(&self, h: impl Into<Holder>) -> Inventory {
        Inventory { world: *self, holder: h.into() }
    }
    pub fn selected_slot(&self, player: u64) -> u32 {
        unsafe { (host().selected_slot)(self.raw(), player) }
    }
    /// A dropped stack, tossed a little; 0 for an empty one. `spawn_with(&ITEM, ..)` to throw
    /// it somewhere.
    pub fn spawn_item(&self, pos: impl Into<Vec3>, stack: ItemStack) -> u64 {
        let pos = pos.into();
        self.spawn_with(&ITEM, pos, &Spawn::new().stack(stack))
    }
    /// Entity attached to the block at `pos` (furnace, chest), 0 if none.
    pub fn block_entity(&self, pos: impl Into<BlockPos>) -> u64 {
        let pos = pos.into();
        unsafe { (host().block_entity)(self.raw(), &pos) }
    }
    /// Opens a menu (a handle, or its id) on a holder: an entity, a block, a held stack, or
    /// `Holder::NONE` for menus without one. False (and a logged reason) when the holder isn't
    /// there or holds no items.
    pub fn open_menu(&self, player: u64, menu: impl Into<Id<Menu>>, h: impl Into<Holder>) -> bool {
        unsafe { (host().open_menu)(self.raw(), player, menu.into().raw(), &h.into()) != 0 }
    }

    /// Whether a player here shows the gizmo group `group` (`/gizmo mymod:pipes`; the mod's
    /// namespace is added to bare names). Check before computing what to draw.
    pub fn gizmo_shown(&self, group: &GizmoGroup) -> bool {
        unsafe { (host().gizmo_shown)(self.raw(), Str::new(group.name())) != 0 }
    }
    /// Moves an entity (players included) at the end of the tick, to `dim` or the same dimension.
    pub fn teleport(&self, id: u64, dim: Option<&crate::Dimension>, pos: impl Into<Vec3>) {
        let pos = pos.into();
        let d = dim.map_or(u32::MAX, |d| d.id());
        unsafe { (host().entity_teleport)(self.raw(), id, d, &pos) }
    }

    /// Rain or thunder in this dimension for `ticks`, after which it changes by itself.
    pub fn set_weather(&self, weather: impl Into<crate::Id<crate::Weather>>, ticks: u32) {
        unsafe { (host().set_weather)(self.raw(), weather.into().raw(), ticks) }
    }

    /// The weather's id now (`weathers::RAIN.id()`).
    pub fn weather(&self) -> u32 {
        unsafe { (host().weather_now)(self.raw()) }
    }

    /// Effect `effect` on an entity at `level` (1 and up) for `ticks` (0 ends it,
    /// `abi::INFINITE_TICKS` never):
    /// `w.entity_effect(id, &entity_effects::GLOW, 1, 200)`, or one of your own
    /// (`Registry::entity_effect`). Several at once, one of each: giving one again replaces it.
    /// An instant one acts once and doesn't stay.
    pub fn entity_effect(&self, id: u64, effect: impl Into<crate::Id<crate::EntityEffect>>, level: u32, ticks: u32) {
        unsafe { (host().entity_effect)(self.raw(), id, effect.into().raw(), level.max(1), ticks) }
    }
    /// The effects on `id`: which, at what level, for how many ticks yet.
    pub fn effects(&self, id: u64) -> Vec<abi::EffectOn> {
        let mut out = vec![abi::EffectOn::default(); 16];
        let n = unsafe { (host().entity_effects)(self.raw(), id, out.as_mut_ptr(), out.len() as u32) } as usize;
        if n > out.len() {
            out.resize(n, abi::EffectOn::default());
            unsafe { (host().entity_effects)(self.raw(), id, out.as_mut_ptr(), out.len() as u32) };
        }
        out.truncate(n);
        out
    }

    /// A white flash on an entity for everyone who sees it (something about to go off).
    pub fn flash(&self, id: u64) {
        self.entity_effect(id, &crate::entity_effects::FLASH, 1, 2)
    }

    /// A player's `abi::permission` level: ask it before anything only some may do.
    pub fn permission(&self, player: u64) -> u32 {
        unsafe { (host().player_permission)(self.raw(), player) }
    }

    /// Gives a player a level, remembered with the world.
    pub fn set_permission(&self, player: u64, level: u32) {
        unsafe { (host().set_player_permission)(self.raw(), player, level) }
    }

    /// Sets this dimension's time of day (0 sunrise, 6000 noon, 18000 midnight). Only the sky and
    /// what follows the day move: what waits on ticks keeps its pace.
    pub fn set_day_time(&self, time: u64) {
        unsafe { (host().set_day_time)(self.raw(), time) }
    }

    /// This dimension's time of day, counting on past a day. `w.time()` is another clock: ticks
    /// the world has run, which nothing moves.
    pub fn day_time(&self) -> u64 {
        unsafe { (host().day_time)(self.raw()) }
    }

    /// As if `ticks` passed at once: the day moves on, and growth, machines and timers catch up.
    pub fn skip_ticks(&self, ticks: u64) {
        unsafe { (host().skip_ticks)(self.raw(), ticks) }
    }

    /// `w.set_game_mode(player, &game_modes::CREATIVE)`, or a mod's own mode.
    pub fn set_game_mode(&self, player: u64, mode: impl Into<crate::Id<crate::GameMode>>) {
        unsafe { (host().set_game_mode)(self.raw(), player, mode.into().raw()) }
    }
}

/// What `World::entity_modify` changes, sent when dropped.
pub struct EntityModify {
    world: World,
    id: u64,
    set: u32,
    state: EntityState,
}

impl EntityModify {
    fn with(mut self, field: u32, f: impl FnOnce(&mut EntityState)) -> Self {
        self.set |= field;
        f(&mut self.state);
        self
    }
    /// A player is teleported there.
    pub fn pos(self, pos: impl Into<Vec3>) -> Self {
        let pos = pos.into();
        self.with(abi::entity_set::POS, |s| s.pos = pos)
    }
    /// Blocks per second.
    pub fn velocity(self, v: impl Into<Vec3>) -> Self {
        let v = v.into();
        self.with(abi::entity_set::VELOCITY, |s| s.vel = v)
    }
    /// Radians, like `EntityState::yaw`.
    pub fn yaw(self, yaw: f32) -> Self {
        self.with(abi::entity_set::YAW, |s| s.yaw = yaw)
    }
    pub fn pitch(self, pitch: f32) -> Self {
        self.with(abi::entity_set::PITCH, |s| s.pitch = pitch)
    }
    /// Up to its max health; 0 or less kills it (`damage_cause::VOID`). Only a respawn brings
    /// back the dead.
    pub fn health(self, health: f32) -> Self {
        self.with(abi::entity_set::HEALTH, |s| s.health = health)
    }
    /// What the next landing hurts by: 0 after a mace's smash, a parachute opening.
    pub fn fall_distance(self, blocks: f32) -> Self {
        self.with(abi::entity_set::FALL_DISTANCE, |s| s.fall_distance = blocks)
    }
    /// Players: flying or not, whatever the game mode allows (landing still stops it).
    pub fn flying(self, on: bool) -> Self {
        self.with(abi::entity_set::FLYING, |s| s.flying = on as u32)
    }
    /// Ticks it burns for, 0 puts it out.
    pub fn fire(self, ticks: u32) -> Self {
        self.with(abi::entity_set::FIRE, |s| s.fire = ticks)
    }
    /// Players: the hotbar slot in hand.
    pub fn selected(self, slot: u32) -> Self {
        self.with(abi::entity_set::SELECTED, |s| s.selected = slot)
    }
    /// Never despawns however far from players (a boss, a pet, a prisoner), or does again as its
    /// category says. Naming it or it picking something up keeps it too.
    pub fn persistent(self, on: bool) -> Self {
        self.with(abi::entity_set::PERSISTENT, |s| {
            s.flags = if on { s.flags | abi::entity_state::PERSISTENT } else { s.flags & !abi::entity_state::PERSISTENT }
        })
    }
}

impl Drop for EntityModify {
    fn drop(&mut self) {
        if self.set != 0 && !std::thread::panicking() {
            unsafe { (host().entity_set)(self.world.raw(), self.id, self.set, &self.state) }
        }
    }
}

#[derive(Clone, Copy)]
pub struct Inventory {
    world: World,
    holder: Holder,
}

impl Inventory {
    /// Slots, 0 when the holder has no inventory.
    pub fn len(&self) -> u32 {
        unsafe { (host().inventory_len)(self.world.raw(), &self.holder) }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, slot: u32) -> ItemStack {
        unsafe { (host().inventory_get)(self.world.raw(), &self.holder, slot) }
    }
    pub fn set(&self, slot: u32, stack: ItemStack) {
        unsafe { (host().inventory_set)(self.world.raw(), &self.holder, slot, stack) }
    }
    /// Returns what did not fit.
    pub fn insert(&self, stack: ItemStack) -> ItemStack {
        unsafe { (host().inventory_insert)(self.world.raw(), &self.holder, stack) }
    }
    /// A slot group of this inventory (a block's: the `holder` groups of the menu it opens); a name it doesn't
    /// have is logged as a mistake and gives no slots.
    pub fn group(&self, name: &str) -> SlotGroup<'static> {
        self.find_group(name).unwrap_or_else(|| {
            crate::error!("the inventory of {:?} has no group {name}", self.holder);
            SlotGroup::empty(GroupIn::Inventory(*self))
        })
    }

    /// The group if this inventory has it: a player's equipment, which mobs don't have.
    pub fn find_group(&self, name: &str) -> Option<SlotGroup<'static>> {
        let mut out = abi::SlotGroup::default();
        let found = unsafe { (host().inventory_group)(self.world.raw(), &self.holder, Str::new(name), &mut out) } != 0;
        found.then(|| SlotGroup::of(GroupIn::Inventory(*self), out))
    }
}

#[derive(Clone, Copy)]
enum GroupIn {
    Inventory(Inventory),
    Menu(*mut abi::MenuCtx),
}

/// Slots of one group, numbered from 0: of an inventory (`w.inventory(h).group(..)`) or of an
/// open menu (`m.group(..)`, whoever's inventory they show), valid while the menu callback runs.
pub struct SlotGroup<'a> {
    of: GroupIn,
    start: u32,
    len: u32,
    cols: u32,
    _p: std::marker::PhantomData<&'a MenuCtx>,
}

impl SlotGroup<'_> {
    fn of(of: GroupIn, g: abi::SlotGroup) -> Self {
        SlotGroup { of, start: g.start, len: g.len, cols: g.cols, _p: std::marker::PhantomData }
    }
    fn empty(of: GroupIn) -> Self {
        SlotGroup { of, start: 0, len: 0, cols: 0, _p: std::marker::PhantomData }
    }
    pub fn len(&self) -> u32 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Its width as laid out (a 3 x 3 grid has 3).
    pub fn cols(&self) -> u32 {
        self.cols
    }
    pub fn rows(&self) -> u32 {
        self.len.div_ceil(self.cols.max(1))
    }
    pub fn get(&self, i: u32) -> ItemStack {
        if i >= self.len {
            return ItemStack::EMPTY;
        }
        match self.of {
            GroupIn::Inventory(inv) => inv.get(self.start + i),
            GroupIn::Menu(m) => unsafe { (host().menu_get)(m, self.start + i) },
        }
    }
    pub fn set(&self, i: u32, stack: ItemStack) {
        if i >= self.len {
            return;
        }
        match self.of {
            GroupIn::Inventory(inv) => inv.set(self.start + i, stack),
            GroupIn::Menu(m) => unsafe { (host().menu_set)(m, self.start + i, stack) },
        }
    }
}

// ---------------------------------------------------------------- recipes

pub struct PlayerInventoryBuilder<'a> {
    reg: &'a mut Registry,
    groups: Vec<abi::SlotGroupDef>,
}

impl PlayerInventoryBuilder<'_> {
    /// A group of the player's inventory: its size from `slots` (`Slots::row(9).hotbar()`).
    pub fn group(mut self, name: &str, slots: Slots) -> Self {
        self.groups.push(slots.def(name, abi::slot_owner::PLAYER));
        self
    }
}

impl Drop for PlayerInventoryBuilder<'_> {
    fn drop(&mut self) {
        let groups = std::mem::take(&mut self.groups);
        drop(
            self.reg
                .edit(&PLAYER)
                .set(&comps::entity::SLOT_GROUPS, groups),
        );
    }
}

pub fn recipe_grid_output(kind: &RecipeKind, grid: &[ItemStack], width: u32, height: u32) -> ItemStack {
    unsafe { (host().recipe_match)(Str::new(kind.name()), grid.as_ptr(), width, height) }
}

/// Output and time of a processing recipe (smelting) for `input`.
pub fn recipe_process(kind: &RecipeKind, input: u32) -> Option<(ItemStack, u32)> {
    let mut t = 0;
    let s = unsafe { (host().recipe_process)(Str::new(kind.name()), input, &mut t) };
    (!s.is_empty()).then_some((s, t))
}

// ---------------------------------------------------------------- registration

pub struct EntityTick {
    args: abi::EntityTickArgs,
}

impl EntityTick {
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn ids(&self) -> &[u64] {
        unsafe { std::slice::from_raw_parts(self.args.ids, self.args.count) }
    }
}

pub struct EntityEvent {
    args: abi::EntityEventArgs,
}

impl EntityEvent {
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn entity(&self) -> u64 {
        self.args.entity
    }
    pub fn other(&self) -> u64 {
        self.args.other
    }
}

/// An effect's tick (`EntityEffectBuilder::on_tick`): every entity that has it, with its level.
pub struct EffectTick {
    args: abi::EffectTickArgs,
}

impl EffectTick {
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn effect(&self) -> u32 {
        self.args.effect
    }
    /// (entity, level).
    pub fn iter(&self) -> impl Iterator<Item = (u64, u32)> + '_ {
        let ids = unsafe { std::slice::from_raw_parts(self.args.ids, self.args.count) };
        let levels = unsafe { std::slice::from_raw_parts(self.args.levels, self.args.count) };
        ids.iter().copied().zip(levels.iter().copied())
    }
}

pub(crate) fn effect_tick_cb(f: impl Fn(&EffectTick) + Send + Sync + 'static) -> abi::Callback {
    callback(move |a: &mut abi::EffectTickArgs| f(unsafe { &*(a as *const abi::EffectTickArgs as *const EffectTick) }))
}

fn tick_cb(f: impl Fn(&EntityTick) + Send + Sync + 'static) -> abi::Callback {
    callback(move |a: &mut abi::EntityTickArgs| f(unsafe { &*(a as *const abi::EntityTickArgs as *const EntityTick) }))
}

fn event_cb(f: impl Fn(&mut EntityEvent) -> bool + Send + Sync + 'static) -> abi::Callback {
    callback(move |a: &mut abi::EntityEventArgs| {
        let handled = f(unsafe { &mut *(a as *mut abi::EntityEventArgs as *mut EntityEvent) });
        a.handled = handled as u32;
    })
}

/// A model part (a bone): its name, pivot, boxes, free-form triangles, and the part it hangs
/// from. The name says how it moves by itself, like in Blockbench's Bedrock models: `head` looks
/// around, `rightArm`, `leftArm`, `rightLeg`, `leftLeg` swing while walking (arms resting held
/// out, like a zombie's, only sway), `body` leans when sneaking, `leg0`..`leg3` are a
/// quadruped's and `leg0`..`leg7` a spider's sweeping legs; other names keep still. Keyframe
/// animations address bones by the same names.
pub struct ModelPart {
    pub pivot: [f32; 3],
    pub boxes: Vec<abi::ModelCuboid>,
    pub rest: [f32; 3],
    pub name: String,
    pub parent: i32,
    pub mesh: Vec<abi::MeshVertex>,
}

impl ModelPart {
    pub fn new(name: &str, pivot: [f32; 3]) -> ModelPart {
        ModelPart { pivot, boxes: Vec::new(), rest: [0.0; 3], name: name.to_string(), parent: -1, mesh: Vec::new() }
    }
    /// Resting rotation in degrees about x, y, z (applied in that order).
    pub fn rotation(mut self, x: f32, y: f32, z: f32) -> ModelPart {
        self.rest = [x, y, z].map(f32::to_radians);
        self
    }
    /// Box relative to the pivot in model pixels, `uv` = top-left texel of its unwrap.
    pub fn cuboid(mut self, origin: [f32; 3], size: [f32; 3], uv: [f32; 2]) -> ModelPart {
        self.boxes
            .push(abi::ModelCuboid { origin, extent: size, uv, ..abi::ModelCuboid::DEFAULT });
        self
    }
    /// The last box grown by this much on every side, its texture staying the same (wool, a skin's
    /// outer layer).
    pub fn inflated(mut self, by: f32) -> ModelPart {
        if let Some(b) = self.boxes.last_mut() {
            b.inflate = by;
        }
        self
    }
    /// The last box's unwrap mirrored left to right.
    pub fn mirrored(mut self) -> ModelPart {
        if let Some(b) = self.boxes.last_mut() {
            b.mirror = 1;
        }
        self
    }
    /// Hangs from the part at `index` in the same model: moves and turns with it.
    pub fn child_of(mut self, index: usize) -> ModelPart {
        self.parent = index as i32;
        self
    }
    /// Free-form triangles (three corners each), in model pixels like the pivot.
    pub fn mesh(mut self, triangles: Vec<abi::MeshVertex>) -> ModelPart {
        self.mesh.extend(triangles);
        self
    }
}

/// A sphere of triangles around `center` (model pixels), its texture stretched over the texel
/// rectangle `uv` (x, y, width, height) like a globe's map.
pub fn sphere_mesh(center: [f32; 3], radius: f32, rings: u32, segments: u32, uv: [f32; 4]) -> Vec<abi::MeshVertex> {
    use std::f32::consts::{PI, TAU};
    let (rings, segments) = (rings.max(2), segments.max(3));
    let at = |i: u32, j: u32| {
        let (theta, phi) = (PI * i as f32 / rings as f32, TAU * j as f32 / segments as f32);
        let pos =
            [center[0] + radius * theta.sin() * phi.cos(), center[1] + radius * theta.cos(), center[2] + radius * theta.sin() * phi.sin()];
        abi::MeshVertex { pos, uv: [uv[0] + uv[2] * j as f32 / segments as f32, uv[1] + uv[3] * i as f32 / rings as f32] }
    };
    let mut out = Vec::new();
    for i in 0..rings {
        for j in 0..segments {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            // Wound so the outside faces the camera (back faces are culled).
            out.extend([a, d, c, a, c, b]);
        }
    }
    out
}

/// A model built in code: its parts and the size of the skin its UVs are laid out on (the
/// picture itself may be bigger, a pack's high resolution skin keeps lining up).
pub struct Model {
    pub parts: Vec<ModelPart>,
    pub uv: [u32; 2],
}

impl Model {
    /// The UV size, if the layout isn't on a 64 x 32 skin.
    pub fn uv_size(mut self, width: u32, height: u32) -> Self {
        self.uv = [width, height];
        self
    }
}

impl From<Vec<ModelPart>> for Model {
    fn from(parts: Vec<ModelPart>) -> Model {
        Model { parts, uv: [64, 32] }
    }
}

/// Classic humanoid on a 64x32 skin, the player's too; `arms_forward` for zombies.
/// The limbs at -x have their unwrap mirrored: looking at the front +x is on the left, where the
/// limb drawn as it lies in the unwrap goes (`rightArm` is the one at -x all the same).
pub fn humanoid(arms_forward: bool) -> Model {
    let arms = if arms_forward { 90.0 } else { 0.0 };
    Model::from(vec![
        ModelPart::new("head", [0.0, 24.0, 0.0]).cuboid([-4.0, 0.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0]),
        ModelPart::new("body", [0.0, 24.0, 0.0]).cuboid([-4.0, -12.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0]),
        ModelPart::new("rightArm", [-6.0, 22.0, 0.0])
            .cuboid([-2.0, -10.0, -2.0], [4.0, 12.0, 4.0], [40.0, 16.0])
            .mirrored()
            .rotation(arms, 0.0, 0.0),
        ModelPart::new("leftArm", [6.0, 22.0, 0.0])
            .cuboid([-2.0, -10.0, -2.0], [4.0, 12.0, 4.0], [40.0, 16.0])
            .rotation(arms, 0.0, 0.0),
        ModelPart::new("rightLeg", [-2.0, 12.0, 0.0])
            .cuboid([-2.0, -12.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0])
            .mirrored(),
        ModelPart::new("leftLeg", [2.0, 12.0, 0.0]).cuboid([-2.0, -12.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0]),
    ])
}

/// Vanilla's spider on a 64x32 skin: head, thorax, abdomen and eight splayed legs.
pub fn spider() -> Model {
    use std::f32::consts::FRAC_PI_4 as Q;
    let mut parts = vec![
        ModelPart::new("head", [0.0, 9.0, -3.0]).cuboid([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [32.0, 4.0]),
        ModelPart::new("neck", [0.0, 9.0, 0.0]).cuboid([-3.0, -3.0, -3.0], [6.0, 6.0, 6.0], [0.0, 0.0]),
        ModelPart::new("body", [0.0, 9.0, 9.0]).cuboid([-5.0, -4.0, -6.0], [10.0, 8.0, 12.0], [0.0, 12.0]),
    ];
    // (z, sideways splay, fan) back to front, a left leg then its right twin: leg0, leg1 ...
    for (i, (z, splay, fan)) in [(2.0, 1.0, 2.0), (1.0, 0.74, 1.0), (0.0, 0.74, -1.0), (-1.0, 1.0, -2.0)]
        .into_iter()
        .enumerate()
    {
        parts.push(
            ModelPart::new(&format!("leg{}", i * 2), [-4.0, 9.0, z])
                .cuboid([-15.0, -1.0, -1.0], [16.0, 2.0, 2.0], [18.0, 0.0])
                .rotation(0.0, (fan * Q / 2.0).to_degrees(), (splay * Q).to_degrees()),
        );
        parts.push(
            ModelPart::new(&format!("leg{}", i * 2 + 1), [4.0, 9.0, z])
                .cuboid([-1.0, -1.0, -1.0], [16.0, 2.0, 2.0], [18.0, 0.0])
                .rotation(0.0, (-fan * Q / 2.0).to_degrees(), (-splay * Q).to_degrees()),
        );
    }
    Model::from(parts)
}

/// Four-legged animal: body (width, height, length), leg height and head size in model pixels.
/// Skin is 64x64: head at (0,0), legs at (0,16), body at (0,32).
pub fn quadruped(body: [f32; 3], leg: f32, head: [f32; 3]) -> Model {
    let [bw, bh, bl] = body;
    let y = leg;
    // Bedrock's quadruped names: leg0 and leg1 at the back, leg2 and leg3 at the front.
    let legs = [
        ("leg0", [-bw / 2.0 + 2.0, y, bl / 2.0 - 2.0]),
        ("leg1", [bw / 2.0 - 2.0, y, bl / 2.0 - 2.0]),
        ("leg2", [bw / 2.0 - 2.0, y, -bl / 2.0 + 2.0]),
        ("leg3", [-bw / 2.0 + 2.0, y, -bl / 2.0 + 2.0]),
    ];
    let mut parts = vec![
        ModelPart::new("body", [0.0, y, 0.0]).cuboid([-bw / 2.0, 0.0, -bl / 2.0], [bw, bh, bl], [0.0, 32.0]),
        ModelPart::new("head", [0.0, y + bh * 0.75, -bl / 2.0]).cuboid([-head[0] / 2.0, -head[1] / 2.0, -head[2]], head, [0.0, 0.0]),
    ];
    for (name, pivot) in legs {
        parts.push(ModelPart::new(name, pivot).cuboid([-2.0, -leg, -2.0], [4.0, leg, 4.0], [0.0, 16.0]));
    }
    Model::from(parts).uv_size(64, 64)
}

pub struct EntityBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
    kind: &'static EntityKind,
    no_egg: bool,
    /// Where riders of the seats set here get off, those that say.
    exits: Vec<Option<crate::riding::ExitFn>>,
}

crate::entry::entry_builder!(Entity => EntityBuilder, entity, finish;);

impl EntityBuilder<'_> {
    /// An attribute modifier its entities have, `.when(..)` while conditions hold (`Modifier`).
    pub fn modifier(self, m: crate::Modifier) -> Self {
        self.push(&crate::component::MODIFIERS, m.0)
    }
    /// Every entity of this kind carries `component` (declared with `r.entity_component`, by any
    /// mod), starting at the kind's own value of it (`.set`), else its declared one; another mod's
    /// kind with `r.edit(&PLAYER).attach(..)`.
    /// Not `.set`: that is one value of the kind itself, `.attach` gives each entity its own,
    /// which changes.
    pub fn attach<T: Value>(self, component: &Component<T>) -> Self {
        self.attached(component.name(), Vec::new())
    }
    /// `attach` with another starting value.
    pub fn attach_with<T: Value>(self, component: &Component<T>, value: T) -> Self {
        self.attached(component.name(), crate::component::value_bytes(&value))
    }
    /// Every entity of the kind carries `storage` (declared with `r.storage`), empty at first.
    pub fn attach_storage(self, storage: &Storage) -> Self {
        self.attached(storage.name(), Vec::new())
    }
    fn attached(self, name: &str, start: Vec<u8>) -> Self {
        self.push(&comps::entity::ATTACH, abi::Attach { name: name.to_string(), start })
    }
    /// A solid body: others stand on it and ride along, bump into it and get pushed by it
    /// (platforms, boats, lifts). Not pushed around itself.
    pub fn solid(self) -> Self {
        self.tag(abi::comp::entity::SOLID).unpushable()
    }
    /// Takes no damage (no hurt flash, health stays); `/clear` and the void still remove it.
    pub fn invulnerable(self) -> Self {
        self.modifier(crate::Modifier::set(&crate::attribute::INVULNERABLE, 1.0))
    }
    /// Neither pushes nor is pushed by other entities (ghosts, projectiles).
    pub fn unpushable(self) -> Self {
        self.tag(abi::comp::entity::UNPUSHABLE)
    }
    /// Moved exactly by the velocity mod logic gives it (`entity_modify`), through blocks, no
    /// gravity: lifts, airships, contraptions.
    pub fn kinematic(self) -> Self {
        self.tag(abi::comp::entity::KINEMATIC)
    }
    /// What it does to entities: itself (`Hazard::own`) or what it squeezes (`Hazard::crushed`).
    pub fn hazard(self, h: crate::Hazard) -> Self {
        self.push(&crate::comps::entity::HAZARDS, h.0)
    }
    /// A solid body that hurts what it squeezes into blocks instead of only pushing it.
    pub fn crushing(self) -> Self {
        self.hazard(crate::Hazard::crushed(1.0, &crate::damage_causes::CRUSH))
    }
    /// Daylight from an open sky sets it burning for 8 seconds, as the undead in vanilla, unless
    /// it wears something on its head.
    pub fn burns_in_sun(self) -> Self {
        let sun = crate::Hazard::own(0.0, &crate::damage_causes::FIRE)
            .harmless()
            .fire(160)
            .every(20)
            .when(crate::Cond::new("day & sky(15) & !sheltered & !in_liquid & !wears(head)"));
        self.hazard(sun)
    }
    /// Riders up to `blocks` above the body (jumping, flying around inside it) keep moving with
    /// it and their own motion stays relative to it, so a jump on a fast ship lands on the ship.
    pub fn carry_height(self, blocks: f32) -> Self {
        self.set(&comps::entity::CARRY_HEIGHT, blocks)
    }
    /// Where the light it gives comes from: blocks from its feet, +Z ahead, turned with it (a
    /// lantern on its head). Its middle when not said, a block entity's block.
    pub fn light_at(self, x: f32, y: f32, z: f32) -> Self {
        self.set(&comps::entity::LIGHT_AT, [x, y, z])
    }
    /// Shares these inventory slots as public stacks, preserving order and empty slots.
    /// Clients read them with `Client::displayed` (`DisplaySource::Shown`); their light is
    /// independent of whether a model draws them.
    pub fn shown_slots(self, slots: &[u32]) -> Self {
        self.set(&comps::entity::SHOWN_SLOTS, slots.to_vec())
    }
    pub fn size(self, width: f32, height: f32) -> Self {
        self.set(&comps::entity::SIZE, abi::BodySize { width, height })
    }
    /// Eye height in blocks, `EYE_SHARE` of its height when not said.
    pub fn eye_height(self, blocks: f32) -> Self {
        self.set(&comps::entity::EYE, blocks)
    }
    /// No spawn egg. Every entity has one otherwise, `<name>_spawn_egg`, called after it and looking
    /// generic until `item/<name>_spawn_egg.png` exists: for what isn't a creature (a projectile, a
    /// moving platform).
    pub fn no_egg(mut self) -> Self {
        self.no_egg = true;
        self
    }
    /// The client makes no steps, idle noises or death smoke for it: `r.client_entity_frame`
    /// does its effects instead.
    pub fn own_effects(self) -> Self {
        self.effect("step", "none", "none")
            .effect("ambient", "none", "none")
            .effect("death", "", "none")
    }
    /// What `event` (`hurt`, `death`, `ambient`, `step`) sounds and shows instead of its
    /// `.sound(..)` and the engine's (smoke when it dies): a sound event and a particle kind,
    /// `""` for what it would be anyway, `"none"` for nothing.
    pub fn effect(self, event: &str, sound: &str, particle: &str) -> Self {
        self.push(&crate::component::EFFECTS, crate::component::effect(event, sound, particle))
    }
    /// It has the player's equipment slots (`Registry::equipment_slot`) after its own: what it
    /// wears shows on its model and counts like a player's armor; `group(EQUIPMENT_GROUP)` or a
    /// slot's name in `w.inventory(id)` reaches them.
    pub fn equipment(self) -> Self {
        self.tag(abi::comp::entity::WEARS)
    }
    /// It takes the poses the player does, by the keys its walk holds (`Walk`) and the poses'
    /// conditions: crouches, swims lying down, crawls where nothing higher fits.
    pub fn poses(self) -> Self {
        self.tag(abi::comp::entity::POSES)
    }
    /// It picks up the dropped items it touches into its inventory, as the player does.
    pub fn picks_up_items(self) -> Self {
        self.tag(abi::comp::entity::PICKS_UP)
    }
    /// Its own melee damage (`World::attack`), the base of its `attribute::ATTACK_DAMAGE`; what it
    /// holds adds to it, as vanilla's.
    pub fn attack_damage(self, damage: f32) -> Self {
        self.attribute(&crate::attribute::ATTACK_DAMAGE, damage as f64)
    }
    /// Its melee hits per second (1), like an item's; `attribute::ATTACK_SPEED` multiplies it.
    pub fn attack_speed(self, per_second: f32) -> Self {
        self.set(&crate::component::ATTACK_SPEED, per_second)
    }
    /// How far it hits (the `ENTITY_REACH` attribute, from its eyes to the target's box); a
    /// modifier changes it like any attribute.
    pub fn reach(self, blocks: f64) -> Self {
        self.attribute(&crate::attribute::ENTITY_REACH, blocks)
    }
    pub fn max_health(self, h: f32) -> Self {
        self.attribute(&crate::attribute::MAX_HEALTH, h as f64)
    }
    /// The kind's own base of an attribute: `.attribute(&attribute::GRAVITY, 0.625)` falls
    /// like vanilla's arrow.
    pub fn attribute(mut self, a: &crate::Attribute, v: f64) -> Self {
        self.e
            .ops
            .push(Pending::value(a.name(), "f64", crate::component::value_bytes(&v)));
        self
    }
    /// It flies as a projectile (`Projectile`): the engine moves it, a ray along its speed each
    /// tick decides what it hits (`actions::PROJECTILE_HIT`), however fast.
    pub fn projectile(self, p: Projectile) -> Self {
        self.set(&comps::entity::PROJECTILE, p.0)
    }
    /// What hits land on, part by part (`Hitboxes::humanoid()` ...); without, its whole box.
    pub fn hitboxes(self, h: Hitboxes) -> Self {
        self.set(&comps::entity::HITBOXES, h.0)
    }
    pub fn walk_speed(self, s: f32) -> Self {
        self.set(&comps::entity::SPEED, s)
    }
    /// A category registered with `Registry::entity_category` (any mod's), for spawn caps and
    /// despawning.
    pub fn category(self, category: &EntityCategory) -> Self {
        self.set_text(&comps::entity::CATEGORY, category.name())
    }
    pub fn hostile(self) -> Self {
        self.set_text(&comps::entity::CATEGORY, "core:hostile")
    }
    pub fn passive(self) -> Self {
        self.set_text(&comps::entity::CATEGORY, "core:passive")
    }
    /// Skin texture under `entity/`; without this, the kind's name (`entity/zombie`).
    pub fn texture(self, t: &str) -> Self {
        let t = crate::asset("entity", t);
        self.set_text(&comps::entity::TEXTURE, &t)
    }
    /// A model built in code (`humanoid(..)`, `quadruped(..)`, `spider()`, or parts of your own).
    pub fn model(self, model: impl Into<Model>) -> Self {
        let m = model.into();
        let parts: Vec<abi::ModelPart> = m.parts.iter().map(raw_part).collect();
        self.set(&comps::entity::PARTS, parts)
            .set(&comps::entity::TEXTURE_SIZE, m.uv)
    }
    /// Sound event `event` (`hurt`, `death`, `ambient`, `step`): one of the files
    /// `sounds/entity/<kind>/<event>*.ogg|wav` at random.
    pub fn sound(self, event: &str) -> Self {
        let file = format!("entity/{}/{event}*", self.kind.path());
        self.sound_files(event, &[&file])
    }
    /// Sound event `event` from these files of `sounds/entity/<kind>/`, one at random.
    pub fn sound_variants(self, event: &str, files: &[&str]) -> Self {
        let path = self.kind.path();
        let files: Vec<String> = files.iter().map(|f| format!("entity/{path}/{f}")).collect();
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        self.sound_files(event, &files)
    }
    fn sound_files(self, event: &str, files: &[&str]) -> Self {
        let name = self.kind.name();
        let sound = crate::Sound::generated(format!("{name}.{event}"));
        self.reg
            .sound(sound)
            .files(files)
            .category(&crate::sound_categories::CREATURES)
            .pitch(0.8, 1.2);
        self.set_text(&crate::component::SOUND, name)
    }
    /// Sounds from `<ns>:entity.<group>` (`.hurt`, `.death`, `.ambient`, `.step` events); a name
    /// with a namespace is used as it is.
    pub fn sound_group(self, group: &str) -> Self {
        let prefix = if group.contains(':') { group.to_string() } else { format!("{}:entity.{group}", self.reg.ns) };
        self.set_text(&crate::component::SOUND, &prefix)
    }

    /// Keyframe animations from `animations/<name>.animation.json`: `idle` and `walk` play by
    /// themselves, others with `World::action_play`. A file named like the kind is used even
    /// without this.
    pub fn animations(self, name: &str) -> Self {
        let name = crate::asset("", name);
        self.set_text(&crate::component::ANIMATIONS, &name)
    }
    /// A moving body built of `template` (`Registry::template`; block (0, 0, 0) is
    /// centered on the entity's position, its box is what the blocks fill). Blocks are placed and
    /// broken on it like in the world, and it carries what stands on it; usually `.solid()`.
    pub fn body(self, template: &crate::Template) -> Self {
        self.set_text(&comps::entity::TEMPLATE, template.name())
    }
    /// Shades the model with `shaders/entity/<name>.wgsl`.
    pub fn shader(self, shader: &crate::Shader) -> Self {
        self.set_text(&crate::component::SHADER, shader.name())
    }
    /// Called once per tick with every entity of this kind.
    pub fn on_tick(self, f: impl Fn(&EntityTick) + Send + Sync + 'static) -> Self {
        self.set(&comps::entity::ON_TICK, tick_cb(f))
    }
    /// Behavior from goals and what sets it off (`ai::Goals`), run every tick; replaces `on_tick`.
    pub fn goals(self, goals: crate::ai::Goals) -> Self {
        let hurt = goals.reactions.clone();
        self.on_hurt(move |w, h| crate::ai::Goals::hurt(&hurt, w, h))
            .on_tick(move |t| goals.tick(t))
    }
    /// One of these was hurt: `actions::HURT` for this kind, after the damage, with the other
    /// events at the end of the tick. Changing the damage is `r.modify(&chain::DAMAGE, ..)`,
    /// preventing it `r.before_event(&actions::HURT, ..)`.
    pub fn on_hurt(self, f: impl Fn(&World, &crate::Hurt) + Send + Sync + 'static) -> Self {
        let kind = self.kind;
        self.reg.on_event(&crate::actions::HURT, move |b| {
            let w = b.world();
            for h in b
                .iter()
                .filter(|h| w.entity(h.entity).is_some_and(|s| s.kind == kind.id()))
            {
                f(&w, h);
            }
        });
        self
    }
    /// One of these died: `actions::DIE` for this kind.
    pub fn on_died(self, f: impl Fn(&World, &crate::Death) + Send + Sync + 'static) -> Self {
        let kind = self.kind;
        self.reg.on_event(&crate::actions::DIE, move |b| {
            let w = b.world();
            for d in b.iter().filter(|d| d.kind == kind.id()) {
                f(&w, d);
            }
        });
        self
    }
    /// A player right clicked one, and `actions::INTERACT_ENTITY` let it; true if it did something,
    /// which swings the player's arm.
    pub fn on_interact(self, f: impl Fn(&mut EntityEvent) -> bool + Send + Sync + 'static) -> Self {
        self.set(&comps::entity::ON_INTERACT, event_cb(f))
    }
    /// A place to ride it, in the order seats are numbered (`crate::riding::Seat`).
    pub fn seat(mut self, seat: crate::riding::Seat) -> Self {
        self.exits.push(seat.exit);
        self.push(&comps::entity::SEATS, seat.desc)
    }
    /// How the keys of a player in a seat that steers move it (`steer::*`, or your own).
    pub fn steering(self, s: crate::riding::Steering) -> Self {
        let cb = callback(move |a: &mut abi::SteerArgs| a.out = (s.0)(&crate::client::Keys::of(a.keys, a.yaw, a.pitch), &a.mount));
        self.set(&comps::entity::STEER, cb)
    }
    /// What its riders may do (`RiderRules`).
    pub fn rider_rules(self, rules: crate::riding::RiderRules) -> Self {
        crate::riding::register_rules(self.reg, self.kind, rules);
        self
    }
    /// Its falls hurt its riders too, as a horse's.
    pub fn riders_fall(self) -> Self {
        self.tag(abi::comp::entity::RIDERS_FALL)
    }
    /// Blocks the third-person camera stays further back while riding it, 0 for none; without
    /// this, by its size.
    pub fn ride_camera(self, blocks: f32) -> Self {
        self.set(&comps::entity::RIDE_CAMERA, blocks)
    }
    /// A right click gets on it, in its first free seat (`on_interact` doing `w.mount`).
    pub fn rideable(self) -> Self {
        self.on_interact(|e| e.world().mount(e.other(), e.entity()))
    }

    fn finish(&mut self) {
        if self.e.mode == abi::entry_mode::NEW && !self.no_egg {
            let name = self.kind.name().to_string();
            self.reg
                .item(crate::Item::generated(format!("{name}_spawn_egg")))
                .spawns(EntityKind::generated(name))
                .tab(&crate::SPAWN_EGGS);
        }
        let exits = std::mem::take(&mut self.exits);
        if exits.iter().any(Option::is_some) {
            let cb = callback(move |a: &mut abi::DismountArgs| {
                if let Some(Some(f)) = exits.get(a.seat as usize) {
                    a.out = f(&World::from_raw(a.world), a.mount, a.rider, a.out);
                }
            });
            self.e.ops.push(comps::entity::DISMOUNT.raw_value(&cb));
        }
    }
}

fn raw_part(p: &ModelPart) -> abi::ModelPart {
    abi::ModelPart { name: p.name.clone(), parent: p.parent, pivot: p.pivot, rest: p.rest, cuboids: p.boxes.clone(), mesh: p.mesh.clone() }
}

impl crate::content::ModelSource for Model {
    /// A kind's model, another mod's or the engine's (`core:player` is drawn with `humanoid(false)`
    /// by base): an edit of that kind.
    fn set_on(self, r: &mut Registry, what: &dyn crate::content::ModelTarget) {
        drop(r.edit_entity(what.model_name()).model(self));
    }
}

pub struct MenuCtx {
    args: abi::MenuArgs,
}

impl MenuCtx {
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    /// In `on_take`, which slot of group `name` was taken from; `None` for another group's.
    pub fn taken_in(&self, name: &str) -> Option<u32> {
        let g = self.find_group(name)?;
        (g.start..g.start + g.len)
            .contains(&self.args.slot)
            .then(|| self.args.slot - g.start)
    }
    pub fn player(&self) -> u64 {
        self.args.player
    }
    /// In `on_take` of a crafting grid's result (`Slots::crafting`), how many runs of its recipe
    /// that was (a shift-click crafts all that fit at once); 1 otherwise.
    pub fn times(&self) -> u32 {
        self.args.times.max(1)
    }
    /// What the menu was opened on (a block entity, a mob, a pouch in a slot); `None` for
    /// the player's own and a crafting table's.
    pub fn owner(&self) -> Option<Holder> {
        (self.args.owner.kind != abi::holder::NONE).then_some(self.args.owner)
    }
    /// Slot group `name` of this menu; asking for one it doesn't have is a mistake that gets
    /// logged, and the group is empty.
    pub fn group(&self, name: &str) -> SlotGroup<'_> {
        self.find_group(name).unwrap_or_else(|| {
            crate::reg_mistake(&format!("the menu has no group {name}"));
            SlotGroup::empty(GroupIn::Menu(self.args.menu))
        })
    }
    /// `group`, when not having it is fine.
    pub fn find_group(&self, name: &str) -> Option<SlotGroup<'_>> {
        let mut out = abi::SlotGroup::default();
        let found = unsafe { (host().menu_group)(self.args.menu, Str::new(name), &mut out) } != 0;
        found.then(|| SlotGroup::of(GroupIn::Menu(self.args.menu), out))
    }
}

/// The groups `player_inventory()` adds, for `Ui::group` and `MenuCtx::group`.
pub const INVENTORY_GROUP: &str = "core:inventory";
pub const HOTBAR_GROUP: &str = "core:hotbar";
/// Every equipment slot of a player's inventory (`w.inventory(player).group(EQUIPMENT_GROUP)`).
pub const EQUIPMENT_GROUP: &str = "core:equipment";

/// A group of slots: a grid, row or column of one inventory sharing its rules. Shown with
/// `ui.group(name)`, read and written with `MenuCtx::group(name)`.
///
/// ```ignore
/// r.menu(&MENU)
///     .group("input", Slots::row(2).holder())
///     .group("fuel", Slots::single().accepts("#fuel").background("flame_slot"))
///     .group("output", Slots::single().output().large());
/// ```
#[derive(Clone)]
pub struct Slots {
    owner: Option<u32>,
    start: u32,
    cols: u32,
    rows: u32,
    flags: u32,
    accepts: String,
    limit: u32,
    size: f32,
    background: String,
    tooltip: String,
    priority: i32,
}

impl Slots {
    pub fn grid(cols: u32, rows: u32) -> Slots {
        Slots {
            owner: None,
            start: u32::MAX,
            cols,
            rows,
            flags: 0,
            accepts: String::new(),
            limit: 0,
            size: 0.0,
            background: String::new(),
            tooltip: String::new(),
            priority: 0,
        }
    }
    pub fn row(n: u32) -> Slots {
        Slots::grid(n, 1)
    }
    pub fn column(n: u32) -> Slots {
        Slots::grid(1, n)
    }
    pub fn single() -> Slots {
        Slots::grid(1, 1)
    }
    /// Rows of nine, like a chest.
    pub fn chest(rows: u32) -> Slots {
        Slots::grid(9, rows)
    }
    /// The menu's own slots, given back to the player when it closes (crafting grids). The
    /// default for `MenuBuilder::group`.
    pub fn menu(mut self) -> Self {
        self.owner = Some(abi::slot_owner::MENU);
        self
    }
    /// The inventory of what the menu opened on: a block entity's (`.entity(|e| e.inventory(n))`), an
    /// entity's, a storage's.
    pub fn holder(mut self) -> Self {
        self.owner = Some(abi::slot_owner::HOLDER);
        self
    }
    /// The player's hotbar, in `Registry::player_slots`.
    pub fn hotbar(mut self) -> Self {
        self.flags |= slot_flags::HOTBAR;
        self
    }
    /// From this index of the owner's inventory instead of right after the group before.
    pub fn at(mut self, start: u32) -> Self {
        self.start = start;
        self
    }
    /// The player's trash (`slot_flags::TRASH`), one slot: it keeps the last thing put in until the
    /// next takes its place or the player leaves, and the trash key (Delete) over any slot sends
    /// that stack there.
    pub fn trash(mut self) -> Self {
        self.flags |= abi::slot_flags::TRASH;
        self
    }
    /// Items only go in (a machine uses them up).
    pub fn input(mut self) -> Self {
        self.flags |= abi::slot_flags::INPUT;
        self
    }
    /// Holds `capacity` millibuckets of one liquid: its item (`Item::named` the liquid's
    /// name), poured in and taken out with buckets by hand, read and set like any slot.
    pub fn liquid(mut self, capacity: u32) -> Self {
        self.flags |= abi::slot_flags::LIQUID;
        self.limit = capacity;
        self
    }
    /// A crafting grid of the menu's first `.recipes(..)` kind: the engine shows what it makes in
    /// the menu's `output()` group and uses it up when that is taken, as the recipe says
    /// (`RecipeBuilder::keep`, `wear`, `leaves`); a shift-click crafts all that fits at once.
    pub fn crafting(mut self) -> Self {
        self.flags |= abi::slot_flags::GRID;
        self
    }
    /// Items only come out (results).
    pub fn output(mut self) -> Self {
        self.flags |= abi::slot_flags::OUTPUT;
        self
    }
    /// Only these items, a reference like everywhere else: `#fuel` (anything with the component),
    /// `#fuel #mymod:fire_starter` (either), `#fuel !base:lava_bucket` (but that one), names.
    pub fn accepts(mut self, items: &str) -> Self {
        self.accepts = items.to_string();
        self
    }
    /// At most this many items per slot.
    pub fn limit(mut self, n: u32) -> Self {
        self.limit = n;
        self
    }
    /// Side of a slot in GUI pixels (18 by default).
    pub fn size(mut self, px: f32) -> Self {
        self.size = px;
        self
    }
    /// The big slot vanilla uses for results.
    pub fn large(self) -> Self {
        self.size(26.0)
    }
    /// A texture from `gui/` shown while a slot is empty (the armor silhouettes).
    pub fn background(mut self, texture: &str) -> Self {
        self.background = crate::asset("gui", texture);
        self
    }
    /// A translation shown over an empty slot.
    pub fn tooltip(mut self, key: &str) -> Self {
        self.tooltip = key.to_string();
        self
    }
    /// Shift-clicks from the player's inventory fill groups with a higher priority first.
    pub fn priority(mut self, p: i32) -> Self {
        self.priority = p;
        self
    }

    /// As a menu or the player's inventory has it, owned by `default` unless it says.
    fn def(self, name: &str, default: u32) -> abi::SlotGroupDef {
        abi::SlotGroupDef {
            name: name.to_string(),
            owner: self.owner.unwrap_or(default),
            start: self.start,
            cols: self.cols,
            rows: self.rows,
            flags: self.flags,
            accepts: self.accepts,
            limit: self.limit,
            size: self.size,
            background: self.background,
            tooltip: self.tooltip,
            priority: self.priority,
        }
    }
}

fn menu_cb(f: impl Fn(&MenuCtx) + Send + Sync + 'static) -> abi::Callback {
    callback(move |a: &mut abi::MenuArgs| f(unsafe { &*(a as *const abi::MenuArgs as *const MenuCtx) }))
}

pub struct MenuBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
    /// Its own groups (the menu's and the holder's), for `default_screen`.
    own: Vec<String>,
    plain_screen: bool,
}

crate::entry::entry_builder!(Menu => MenuBuilder, menu, finish;);

impl MenuBuilder<'_> {
    /// A group of slots, by default the menu's own (`Slots::menu`). Groups come in this order.
    pub fn group(mut self, name: &str, slots: Slots) -> Self {
        let g = slots.def(name, abi::slot_owner::MENU);
        if g.owner < abi::slot_owner::PLAYER {
            self.own.push(name.to_string());
        }
        self.push(&comps::menu::GROUPS, g)
    }
    /// The player's inventory and hotbar as two groups (`INVENTORY_GROUP`, `HOTBAR_GROUP`), shown
    /// with `ui.player_inventory(..)`.
    pub fn player_inventory(self) -> Self {
        let group = |name, owner| Slots::single().def(name, owner);
        self.push(&comps::menu::GROUPS, group(INVENTORY_GROUP, abi::slot_owner::PLAYER_MAIN))
            .push(&comps::menu::GROUPS, group(HOTBAR_GROUP, abi::slot_owner::PLAYER_HOTBAR))
    }
    /// The equipment slots, here among the groups, each named like its slot: of the entity the
    /// menu is open on when it wears (`EntityBuilder::equipment`, a mob's), else the player's.
    /// Only the slots: `ui.equipment()` draws them where the screen wants.
    pub fn equipment(self) -> Self {
        let g = Slots::single().def(EQUIPMENT_GROUP, abi::slot_owner::EQUIPMENT);
        self.push(&comps::menu::GROUPS, g)
    }
    /// Recipes of this kind are made here: clicking an `arrow` bar of the screen shows them.
    pub fn recipes(self, kind: &RecipeKind) -> Self {
        self.push(&comps::menu::RECIPES, kind.name().to_string())
    }
    pub fn on_change(self, f: impl Fn(&MenuCtx) + Send + Sync + 'static) -> Self {
        self.set(&comps::menu::ON_CHANGE, menu_cb(f))
    }
    pub fn on_take(self, f: impl Fn(&MenuCtx) + Send + Sync + 'static) -> Self {
        self.set(&comps::menu::ON_TAKE, menu_cb(f))
    }
    pub fn on_tick(self, f: impl Fn(&MenuCtx) + Send + Sync + 'static) -> Self {
        self.set(&comps::menu::ON_TICK, menu_cb(f))
    }
    pub fn screen(self, f: impl Fn(&mut Ui) + Send + Sync + 'static) -> Self {
        self.set(&comps::menu::SCREEN, ui_cb(f))
    }
    /// Single player stops while it is open (the pause and settings screens).
    pub fn pauses(self) -> Self {
        self.tag(abi::comp::menu::PAUSES)
    }
    /// Escape doesn't close it (the death screen).
    pub fn keep_on_escape(self) -> Self {
        self.tag(abi::comp::menu::KEEP_ON_ESCAPE)
    }
    /// It closes when the player respawns.
    pub fn closes_on_respawn(self) -> Self {
        self.tag(abi::comp::menu::CLOSES_ON_RESPAWN)
    }
    /// Like the chat screen: the HUD leaves the messages to it and Enter sends the text of its
    /// input of class `chat`.
    pub fn shows_chat(self) -> Self {
        self.tag(abi::comp::menu::SHOWS_CHAT)
    }
    /// Keys pressed while it is on top, by their settings name (`Enter`, `KeyE`, `mouse3`), before
    /// Escape and the other global keys.
    pub fn on_key(self, f: impl Fn(&crate::ClientView, &str) + Send + Sync + 'static) -> Self {
        let cb = crate::callback(move |a: &mut abi::ScreenKeyArgs| f(&crate::ClientView::from_raw(a.client), unsafe { a.key.as_str() }));
        self.set(&comps::menu::ON_KEY, cb)
    }
    /// The usual screen: its title, its own groups in order, the player's inventory.
    pub fn default_screen(mut self) -> Self {
        self.plain_screen = true;
        self.player_inventory()
    }

    fn finish(&mut self) {
        if !self.plain_screen {
            return;
        }
        let names = std::mem::take(&mut self.own);
        let screen = ui_cb(move |ui| {
            ui.panel("panel", crate::ui::S::column().pad(7.0).gap(4.0), |ui| {
                ui.title();
                for name in &names {
                    ui.group(name);
                }
                ui.player_inventory(true);
            });
        });
        self.e.ops.push(comps::menu::SCREEN.raw_value(&screen));
    }
}

impl Registry {
    pub fn entity<'a>(&'a mut self, kind: &'static EntityKind) -> EntityBuilder<'a> {
        EntityBuilder { reg: self, e: crate::entry::Entry::of(kind), kind, no_egg: false, exits: Vec::new() }
    }

    /// An edit of entity kind `name`, any mod's or the engine's.
    pub(crate) fn edit_entity(&mut self, name: &str) -> EntityBuilder<'_> {
        let kind = EntityKind::generated(qualify(self.ns, name));
        self.edit(kind)
    }

    /// Declares `comp` for anything that holds a value of its own: entities (as
    /// `entity_component` does) and stacks (as `stack_component`). With
    /// `component_flags::CARRIED` the value goes from a broken block onto its item and back
    /// when placed, as a renamed chest's name does.
    pub fn instance_component<T: Value>(&mut self, comp: &Component<T>, default: T, flags: u32) -> DeclarationBuilder<'_> {
        let no_save = if flags & component_flags::SAVE == 0 { abi::stack_flags::NO_SAVE } else { 0 };
        self.declare::<T>(comp.name())
            .entity(&default, flags)
            .stack(no_save)
    }

    /// Declares per-entity storage for `comp`: the value an entity starts with and the
    /// `component_flags` (saved, synced to whom). Kinds carry it with `.attach(&COMP)` (another
    /// mod's kind with `r.edit(&KIND).attach(&COMP)`).
    pub fn entity_component<T: Value>(&mut self, comp: &Component<T>, default: T, flags: u32) -> DeclarationBuilder<'_> {
        self.declare::<T>(comp.name()).entity(&default, flags)
    }

    /// A component's declaration with its schema, the rest up to the caller.
    pub(crate) fn declare<T: crate::component::Data>(&mut self, name: &str) -> DeclarationBuilder<'_> {
        let e = crate::entry::Entry::new(abi::entry_kind::COMPONENT, name.to_string());
        DeclarationBuilder { reg: self, e }.set_text(&comps::component::SCHEMA, &T::schema())
    }

    fn declaration_entry(&mut self, declaration: &'static Declaration) -> DeclarationBuilder<'_> {
        DeclarationBuilder { reg: self, e: crate::entry::Entry::of(declaration) }
    }

    /// Logic that is nobody's own (players' hunger, a mod's weather): once per tick in every
    /// region of every dimension, where `w.players()` are the players in that region. Another
    /// mod's entities are reached through events (`r.on_event(&actions::HURT, ..)`) and questions.
    pub fn world_tick(&mut self, f: impl Fn(&World) + Send + Sync + 'static) {
        let cb = crate::callback(move |a: &mut abi::WorldTickArgs| f(&World::from_raw(a.world)));
        unsafe { (host().reg_world_tick)(self.raw, cb) }
    }

    pub fn menu<'a>(&'a mut self, menu: &'static Menu) -> MenuBuilder<'a> {
        MenuBuilder { reg: self, e: crate::entry::Entry::of(menu), own: Vec::new(), plain_screen: false }
    }

    /// The player inventory as named groups, one of them `.hotbar()`:
    /// `r.player_slots().group("hotbar", Slots::row(9).hotbar()).group("main", Slots::grid(9, 3))`.
    /// The engine lays them out, hotbar first, the equipment slots (`equipment_slot`) after
    /// everything; `w.inventory(player).group(name)` reads a group, `core:equipment` all of the
    /// equipment. The mod loaded last wins.
    pub fn player_slots(&mut self) -> PlayerInventoryBuilder<'_> {
        PlayerInventoryBuilder { reg: self, groups: Vec::new() }
    }

    /// An equipment slot of players (head, chest...): what is worn there covers the bones named
    /// in `covers` (`worn::CHEST`, `&["leftWing", "rightWing"]`); a name with a role (`head`,
    /// `leftArm`, `leg0`) covers every bone of that role. `background` (from `gui/`) shows while
    /// it is empty. Items go there with `.wearable(&SLOT, ..)`; menus show the slots with
    /// `.equipment()`.
    pub fn equipment_slot(&mut self, slot: &'static EquipmentSlot, covers: &[&str], background: &str) -> EquipmentSlotBuilder<'_> {
        let bg = crate::asset("gui", background);
        let covers = covers.iter().map(|c| c.to_string()).collect();
        self.equipment_slot_entry(slot)
            .set(&crate::comps::equipment_slot::COVERS, covers)
            .set_text(&crate::comps::equipment_slot::BACKGROUND, &bg)
    }

    fn equipment_slot_entry(&mut self, slot: &'static EquipmentSlot) -> EquipmentSlotBuilder<'_> {
        EquipmentSlotBuilder { reg: self, e: crate::entry::Entry::of(slot) }
    }

    /// The menu the inventory key opens. The mod loaded last wins.
    pub fn player_menu(&mut self, menu: &Menu) {
        drop(
            self.edit(&PLAYER)
                .set_text(&comps::entity::INVENTORY_MENU, menu.name()),
        );
    }

    /// Gives `block` a block entity of a kind of its own, named like the block
    /// (`BlockBuilder::entity`), registered as `mode` says (`entry_mode`).
    pub(crate) fn block_entity(&mut self, block: &Block, mode: u32, f: impl for<'r> FnOnce(EntityBuilder<'r>) -> EntityBuilder<'r>) {
        let kind = EntityKind::generated(block.name().to_string());
        let mut b = self.entity(kind);
        b.e.mode = mode;
        drop(f(b
            .no_egg()
            .tag(abi::comp::entity::NO_GRAVITY)
            .invulnerable()
            .set_text(&comps::entity::BLOCK, block.name())));
    }

    /// An inventory of `slots` a holder can keep besides its own (`Storage`).
    pub fn storage(&mut self, storage: &Storage, slots: u32) -> DeclarationBuilder<'_> {
        let e = crate::entry::Entry::new(abi::entry_kind::COMPONENT, storage.name().to_string());
        DeclarationBuilder { reg: self, e }.set(&comps::component::SLOTS, slots)
    }

    /// Chat command `/name` run on the server; `usage` declares its arguments (`<name:type>`,
    /// `[name:type]`, literal words, `|` alternatives), which the engine parses, checks and
    /// completes. An engine command of that name (`/give`, `/tp` ...) is replaced by it.
    ///
    /// It asks for `permission::GAME_MASTER` unless told otherwise, and players below that are
    /// told so and not offered it: `r.command("rename", "<name:text>", f).permission(permission::EVERYONE)`.
    pub fn command(
        &mut self,
        name: &str,
        usage: &str,
        f: impl Fn(&mut crate::ServerCommand) + Send + Sync + 'static,
    ) -> crate::CommandOptions {
        let cb = crate::command::server_cb(f);
        unsafe { (host().reg_command)(self.raw, Str::new(name), Str::new(usage), 0, cb) };
        crate::CommandOptions::new(self.raw, name)
    }

    /// Like `command`, but runs on the client that types it (`cmd.client()`), never on the
    /// server: for what only the player's own game has (views, local settings, debug output).
    pub fn client_command(&mut self, name: &str, usage: &str, f: impl Fn(&mut crate::ClientCommand) + Send + Sync + 'static) {
        let cb = crate::command::client_cb(f);
        unsafe { (host().reg_command)(self.raw, Str::new(name), Str::new(usage), 1, cb) }
    }

    /// A word set: command parameters `word(#set)` complete from its words, which other mods
    /// add to with `r.edit(&SET).words(..)` (the engine's sets too, `word_sets::GIZMOS`).
    pub fn word_set<'a>(&'a mut self, set: &'static WordSet) -> WordSetBuilder<'a> {
        WordSetBuilder { reg: self, e: crate::entry::Entry::of(set) }
    }

    /// A gizmo group of this mod, offered by `/gizmo`'s completion.
    pub fn gizmo_group(&mut self, group: &GizmoGroup) {
        drop(self.edit(&crate::word_sets::GIZMOS).words(&[group.name()]));
    }

    /// A new branch of an existing server command, handled here: `r.command_branch("time",
    /// "dawn", |cmd| ..)` makes `/time dawn` this mod's. Earlier alternatives win when both fit.
    pub fn command_branch(&mut self, command: &str, usage: &str, f: impl Fn(&mut crate::ServerCommand) + Send + Sync + 'static) {
        let cb = crate::command::server_cb(f);
        unsafe { (host().reg_command_branch)(self.raw, Str::new(command), Str::new(usage), 0, cb) }
    }

    /// The same for a client command (`/gizmo`, `/log`, a mod's `client_command`): `r.
    /// client_command_branch("gizmo", "clear", |cmd| ..)`.
    pub fn client_command_branch(&mut self, command: &str, usage: &str, f: impl Fn(&mut crate::ClientCommand) + Send + Sync + 'static) {
        let cb = crate::command::client_cb(f);
        unsafe { (host().reg_command_branch)(self.raw, Str::new(command), Str::new(usage), 1, cb) }
    }

    /// Adds to place `area` of a screen or HUD element without replacing it: a main menu button is
    /// `r.screen_part(&screens::TITLE, "buttons", 0, |ui| ..)`. Lower `order` comes first. The
    /// engine's places: `TITLE` `buttons` and `side`, `PAUSE` `buttons`, `SETTINGS` `buttons`, and
    /// beside menus: any menu's own `side` (`r.screen_part(&base::CHEST, "side", ..)`, another
    /// mod's menu too) and `MENUS` `side` beside every one (a sort button). Named
    /// `<mod>:<screen>/<area>` (`base:pause/buttons`, the next `/2`..), other mods edit it by that.
    pub fn screen_part<K: crate::kind::Kind>(
        &mut self,
        screen: &crate::Handle<K>,
        area: &str,
        order: i32,
        f: impl Fn(&mut Ui) + Send + Sync + 'static,
    ) -> ScreenPartBuilder<'_> {
        use comps::screen_part as s;
        let name = self.derive_name(&format!("{}/{area}", screen.path()));
        ScreenPartBuilder { reg: self, e: crate::entry::Entry::new(abi::entry_kind::SCREEN_PART, name) }
            .set_text(&s::SCREEN, screen.name())
            .set_text(&s::AREA, area)
            .set(&s::ORDER, order)
            .set(&s::CALL, ui_cb(f))
    }

    fn screen_part_entry(&mut self, part: &'static crate::ScreenPart) -> ScreenPartBuilder<'_> {
        ScreenPartBuilder { reg: self, e: crate::entry::Entry::of(part) }
    }

    /// HUD element at `anchor` (`hud_anchor::*`); `order` 0 sits nearest the anchor's edge.
    pub fn hud_element(
        &mut self,
        element: &'static HudElement,
        anchor: &str,
        order: i32,
        f: impl Fn(&mut Ui) + Send + Sync + 'static,
    ) -> HudElementBuilder<'_> {
        self.hud_element_entry(element)
            .anchor(anchor)
            .order(order)
            .ui(f)
    }

    fn hud_element_entry(&mut self, element: &'static HudElement) -> HudElementBuilder<'_> {
        HudElementBuilder { reg: self, e: crate::entry::Entry::of(element) }
    }

    /// A tint kind: blocks with `.tint(&KIND)` take its color from the biome they stand in
    /// (`BiomeBuilder::tint`, `r.edit(&BIOME).tint(..)` for another mod's), `default` (0xRRGGBB)
    /// elsewhere and on items.
    pub fn tint_kind(&mut self, kind: &'static TintKind, default: u32) -> TintKindBuilder<'_> {
        self.tint_kind_entry(kind)
            .set(&crate::comps::tint_kind::DEFAULT, default)
    }

    fn tint_kind_entry(&mut self, kind: &'static TintKind) -> TintKindBuilder<'_> {
        TintKindBuilder { reg: self, e: crate::entry::Entry::of(kind) }
    }

    /// An entity category: at most `cap` of it spawn naturally near a player; with `despawn` > 0
    /// they vanish when no player is within that many blocks.
    pub fn entity_category(&mut self, category: &'static EntityCategory, cap: u32, despawn: f32) -> CategoryBuilder<'_> {
        self.entity_category_entry(category)
            .set(&crate::comps::entity_category::CAP, cap)
            .set(&crate::comps::entity_category::DESPAWN, despawn)
    }

    fn entity_category_entry(&mut self, category: &'static EntityCategory) -> CategoryBuilder<'_> {
        CategoryBuilder { reg: self, e: crate::entry::Entry::of(category) }
    }

    /// Registers a recipe kind this mod owns, and how the recipe viewer shows it.
    pub fn recipe_kind<'a>(&'a mut self, kind: &'static RecipeKind) -> RecipeKindBuilder<'a> {
        RecipeKindBuilder { reg: self, e: crate::entry::Entry::of(kind) }
    }

    /// Shaped recipe: `pattern` rows use the characters in `keys`; ingredients are item names or
    /// `#tag`. More results and data with the builder it returns, named as `RecipeBuilder` says.
    pub fn shaped<'a>(
        &'a mut self,
        kind: &RecipeKind,
        output: &str,
        count: u32,
        pattern: &[&str],
        keys: &[(char, &str)],
    ) -> RecipeBuilder<'a> {
        let keys = keys
            .iter()
            .map(|(c, n)| abi::RecipeKey { name: c.to_string(), item: self.ingredient(n) })
            .collect();
        let pattern = pattern.iter().map(|p| p.to_string()).collect();
        self.recipe(kind, output, count)
            .set(&comps::recipe::PATTERN, pattern)
            .set(&comps::recipe::KEYS, keys)
    }

    /// Shapeless recipe or a processing recipe (inputs used up over `time` ticks). Inputs are
    /// items or `#tags`; a liquid is its item, named like it, counted in millibuckets.
    pub fn shapeless<'a>(&'a mut self, kind: &RecipeKind, output: &str, count: u32, inputs: &[&str], time: u32) -> RecipeBuilder<'a> {
        let inputs = inputs.iter().map(|n| self.ingredient(n)).collect();
        self.recipe(kind, output, count)
            .set(&comps::recipe::INPUTS, inputs)
            .set(&comps::recipe::TIME, time)
    }

    fn recipe(&mut self, kind: &RecipeKind, output: &str, count: u32) -> RecipeBuilder<'_> {
        let output = self.ingredient(output);
        let name = self.derive_name(&format!("{}/{}", kind.path(), output.split_once(':').map_or(output.as_str(), |o| o.1)));
        RecipeBuilder { reg: self, e: crate::entry::Entry::new(abi::entry_kind::RECIPE, name) }
            .set_text(&comps::recipe::KIND, kind.name())
            .set_text(&comps::recipe::OUTPUT, &output)
            .set(&comps::recipe::COUNT, count)
    }

    fn recipe_entry(&mut self, recipe: &'static crate::Recipe) -> RecipeBuilder<'_> {
        RecipeBuilder { reg: self, e: crate::entry::Entry::of(recipe) }
    }

    fn ingredient(&self, n: &str) -> String {
        if n.starts_with('#') { n.to_string() } else { qualify(self.ns, n) }
    }
}

/// A recipe being registered; sent when dropped. It is named `<mod>:<kind>/<output>`
/// (`base:crafting/torch`), `/2`, `/3`.. after it for more of this mod's with that kind and output,
/// unless `named`: other mods edit and remove it by that name (`r.remove(&Recipe::named(..))`).
/// Any other component is data it carries for what makes it (`.set(&ENERGY_PER_TICK, 32)`), which
/// the kind's info line can show.
/// An ingredient of a recipe by its pattern character (`'h'`), or for one without a pattern by
/// its place among the inputs (`0`).
pub trait RecipeSlot {
    fn key(&self) -> String;
}

impl RecipeSlot for char {
    fn key(&self) -> String {
        self.to_string()
    }
}

impl RecipeSlot for usize {
    fn key(&self) -> String {
        self.to_string()
    }
}

pub struct RecipeBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl RecipeBuilder<'_> {
    /// Its name instead of `<mod>:<kind>/<output>`.
    pub fn named(mut self, name: &str) -> Self {
        let derived = std::mem::replace(&mut self.e.name, qualify(self.reg.ns, name));
        self.reg.derived.retain(|n| *n != derived);
        self
    }
    /// How many of each input it takes, in the order of the inputs (millibuckets for liquids).
    pub fn input_amounts(self, counts: &[u32]) -> Self {
        self.set(&comps::recipe::AMOUNTS, counts.to_vec())
    }
    /// Another result (an item, or a liquid by its name and millibuckets).
    pub fn output(self, item: &str, count: u32) -> Self {
        self.extra(crate::Pool::new().items(item, count, count))
    }
    /// A result that comes out with chance `chance` (0..1) each time: a pool of it and nothing.
    pub fn output_chance(self, item: &str, count: u32, chance: f32) -> Self {
        let w = (chance.clamp(0.0, 1.0) * 1000.0).round() as u32;
        let pool = crate::Pool::new()
            .entry(crate::Entry::item(item).count(count, count).weight(w))
            .entry(crate::Entry::nothing().weight(1000 - w));
        self.extra(pool)
    }
    /// Rolls of more results, as a loot table's (`r.loot(..).pool(..)`).
    pub fn extra(self, pool: crate::Pool) -> Self {
        let pool = pool.raw(self.reg.ns);
        self.push(&comps::recipe::OUTPUTS, pool)
    }
    /// Works, but the recipe viewer doesn't list it.
    /// Ingredient `key` (its pattern character, or for a recipe without a pattern its place
    /// among the inputs) stays as it is: a mold, a catalyst.
    pub fn keep(self, key: impl RecipeSlot) -> Self {
        self.uses(key, abi::recipe_use::KEEP, 0, "")
    }
    /// Ingredient `key` stays and wears by `amount` a run, breaking when it is worn out: a hammer.
    pub fn wear(self, key: impl RecipeSlot, amount: u32) -> Self {
        self.uses(key, abi::recipe_use::WEAR, amount, "")
    }
    /// Ingredient `key` is used up and `item` takes its place (any mod's): a lava bucket leaving
    /// its bucket. Without this one leaves its item's `remainder`.
    pub fn leaves(self, key: impl RecipeSlot, item: &str) -> Self {
        let item = crate::full_name(item);
        self.uses(key, abi::recipe_use::LEAVES, 0, &item)
    }
    fn uses(self, key: impl RecipeSlot, how: u32, amount: u32, item: &str) -> Self {
        let u = abi::RecipeUse { key: key.key(), how, amount, item: item.to_string() };
        self.push(&comps::recipe::USES, u)
    }
    pub fn hidden(self) -> Self {
        self.tag(abi::comp::recipe::HIDDEN)
    }
}

/// A recipe kind being registered; sent when dropped.
pub struct RecipeKindBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(
    Recipe => RecipeBuilder, recipe_entry;
    HudElement => HudElementBuilder, hud_element_entry;
    ScreenPart => ScreenPartBuilder, screen_part_entry;
    WordSet => WordSetBuilder, word_set;
    Declaration => DeclarationBuilder, declaration_entry;
    RecipeKind => RecipeKindBuilder, recipe_kind;
    EntityCategory => CategoryBuilder, entity_category_entry;
    EquipmentSlot => EquipmentSlotBuilder, equipment_slot_entry;
    TintKind => TintKindBuilder, tint_kind_entry;
);

impl RecipeKindBuilder<'_> {
    /// The recipe viewer shows inputs as a grid this big, whatever a recipe uses (a crafting
    /// table's 3 x 3).
    pub fn inputs(self, cols: u32, rows: u32) -> Self {
        self.set(&crate::comps::recipe_kind::INPUTS, [cols, rows])
    }
    pub fn outputs(self, cols: u32, rows: u32) -> Self {
        self.set(&crate::comps::recipe_kind::OUTPUTS, [cols, rows])
    }
    /// A line under each recipe: translation `key`, its `{0}`.. filled with `values`: `time`
    /// (seconds) or components the recipe carries (`mymod:energy_per_tick`).
    pub fn info_line(self, key: &str, values: &[&str]) -> Self {
        let values = values.iter().map(|v| v.to_string()).collect();
        self.set_text(&crate::comps::recipe_kind::INFO, key)
            .set(&crate::comps::recipe_kind::INFO_VALUES, values)
    }
    /// Builds each recipe yourself instead of the usual inputs, arrow, outputs:
    /// `ui.recipe_inputs()`, `ui.recipe_outputs()` and `ui.recipe_input(i)` place its slots.
    pub fn layout(self, f: impl Fn(&mut Ui, &RecipeView) + Send + Sync + 'static) -> Self {
        let cb = callback(move |a: &mut abi::RecipeUiArgs| {
            let mut ui = Ui::from_raw(a.ui, a.client);
            let r = RecipeView { kind: unsafe { a.kind.as_str() }.to_string(), index: a.recipe };
            f(&mut ui, &r);
        });
        self.set(&crate::comps::recipe_kind::LAYOUT, cb)
    }
}

/// An equipment slot being registered (`Registry::equipment_slot`).
pub struct EquipmentSlotBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

/// A tint kind being registered (`Registry::tint_kind`).
pub struct TintKindBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

/// A HUD element being registered (`Registry::hud_element`).
pub struct HudElementBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl HudElementBuilder<'_> {
    /// Rebuilds the layout when this revision changes. Keep changing text and numbers in
    /// bindings; use this for a different set of widgets, such as a new inspected block.
    pub fn rebuild_when(self, f: impl Fn(&crate::ClientView) -> u64 + Send + Sync + 'static) -> Self {
        self.set(
            &comps::hud_element::REBUILD,
            callback(move |a: &mut abi::UiRevisionArgs| {
                a.revision = f(&crate::ClientView::from_raw(a.client));
            }),
        )
    }
    /// Where it sits (`hud_anchor::*`).
    pub fn anchor(self, anchor: &str) -> Self {
        self.set_text(&comps::hud_element::ANCHOR, anchor)
    }
    /// 0 sits nearest the anchor's edge.
    pub fn order(self, order: i32) -> Self {
        self.set(&comps::hud_element::ORDER, order)
    }
    /// What it shows.
    pub fn ui(self, f: impl Fn(&mut Ui) + Send + Sync + 'static) -> Self {
        self.set(&comps::hud_element::CALL, ui_cb(f))
    }
}

/// A screen part being registered (`Registry::screen_part`).
pub struct ScreenPartBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

/// A word set being registered or added to (`Registry::word_set`).
pub struct WordSetBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl WordSetBuilder<'_> {
    pub fn words(self, words: &[&str]) -> Self {
        words
            .iter()
            .fold(self, |b, w| b.push(&comps::words::WORDS, w.to_string()))
    }
    /// Takes a word out (another mod's, in an edit).
    pub fn remove_word(self, word: &str) -> Self {
        self.remove_item(&comps::words::WORDS, word)
    }
}

/// A component's declaration being registered (`Registry::entity_component`, `stack_component`,
/// `storage`); another mod's is `r.edit(COMP.declaration())`.
pub struct DeclarationBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl DeclarationBuilder<'_> {
    fn flags(self, all: &[(&str, u32)], flags: u32) -> Self {
        all.iter()
            .filter(|f| flags & f.1 != 0)
            .fold(self, |b, f| b.tag(f.0))
    }
    /// Entities carry it, starting at `default`, with `component_flags`.
    fn entity<T: Value>(self, default: &T, flags: u32) -> Self {
        self.set(&comps::component::DEFAULT, crate::component::value_bytes(default))
            .flags(&abi::comp::component::FLAGS, flags)
    }
    /// Stacks carry it, with `stack_flags`.
    pub fn stack(self, flags: u32) -> Self {
        self.tag(abi::comp::component::STACK)
            .flags(&abi::comp::component::STACK_FLAGS, flags)
    }
}

/// One recipe of a kind, found by `recipe_find` (or shown by a kind's `layout`).
pub struct RecipeView {
    kind: String,
    index: u32,
}

/// The first recipe of `kind` whose inputs, with their counts, are all among `stacks`.
pub fn recipe_find(kind: &RecipeKind, stacks: &[ItemStack]) -> Option<RecipeView> {
    recipes_for(kind, stacks).next()
}

/// Every recipe of `kind` whose inputs are all among `stacks`, in order (what a stonecutter offers).
pub fn recipes_for<'a>(kind: &'a RecipeKind, stacks: &'a [ItemStack]) -> impl Iterator<Item = RecipeView> + 'a {
    let mut from = Some(0);
    std::iter::from_fn(move || {
        let n = unsafe { (host().recipe_find)(Str::new(kind.name()), stacks.as_ptr(), stacks.len() as u32, from?) };
        from = (n > 0).then_some(n);
        Some(RecipeView { kind: kind.name().to_string(), index: from? - 1 })
    })
}

impl RecipeView {
    fn info(&self) -> abi::RecipeInfo {
        let mut out = abi::RecipeInfo::default();
        unsafe { (host().recipe_info)(Str::new(&self.kind), self.index, &mut out) };
        out
    }
    /// Its time in ticks.
    pub fn time(&self) -> u32 {
        self.info().time
    }
    /// Its results with their chances; a liquid's is its item by the millibucket.
    pub fn outputs(&self) -> Vec<(ItemStack, f32)> {
        (0..self.info().outputs)
            .map(|i| {
                let (mut s, mut chance) = (ItemStack::EMPTY, 1.0);
                unsafe { (host().recipe_output)(Str::new(&self.kind), self.index, i, &mut s, &mut chance) };
                (s, chance)
            })
            .collect()
    }
    /// What else one run of it gives this time, its pools rolled.
    pub fn roll(&self, w: &crate::World) -> Vec<ItemStack> {
        let mut buf = vec![ItemStack::EMPTY; 256];
        let n = unsafe { (host().recipe_roll)(w.raw(), Str::new(&self.kind), self.index, buf.as_mut_ptr(), buf.len()) };
        buf.truncate(n.min(buf.len()));
        buf
    }
    /// A value the recipe carries (`r.shaped(..).set(&COMP, v)`): its entry's.
    pub fn value<T: Value, K>(&self, comp: &Component<T, K>) -> Option<T> {
        comp.on(abi::entry_kind::RECIPE, self.info().entry)
    }
    /// Takes its inputs out of `stacks` (the ones `recipe_find` was given); false, and nothing
    /// taken, if they are not all there.
    pub fn consume(&self, stacks: &mut [ItemStack]) -> bool {
        unsafe { (host().recipe_consume)(Str::new(&self.kind), self.index, stacks.as_mut_ptr(), stacks.len() as u32) != 0 }
    }
}

/// An entity category being registered (`Registry::entity_category`).
pub struct CategoryBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl CategoryBuilder<'_> {
    /// How close to a player its mobs count for the cap (64 by default).
    pub fn cap_radius(self, blocks: f32) -> Self {
        self.set(&crate::comps::entity_category::CAP_RADIUS, blocks)
    }
}

/// What the base game's armor slots cover (`Registry::equipment_slot`): bones by role, so they fit
/// any model with bones of those names.
pub mod worn {
    pub const HEAD: &[&str] = &["head"];
    pub const CHEST: &[&str] = &["body", "leftArm", "rightArm"];
    pub const LEGS: &[&str] = &["body", "leftLeg", "rightLeg", "leg2", "leg3"];
    pub const FEET: &[&str] = &["leftLeg", "rightLeg", "leg2", "leg3"];
}

/// Where a natural spawn stands (`SpawnBuilder::place`).
#[derive(Clone, Copy, Debug)]
pub struct SpawnPlace {
    kind: u32,
    tag: Option<&'static str>,
}

impl SpawnPlace {
    /// On a solid block, in air.
    pub fn ground() -> SpawnPlace {
        SpawnPlace { kind: abi::spawn_place::GROUND, tag: None }
    }
    /// Inside a liquid with this tag (`"#water"`): fish, squid.
    pub fn in_liquid(tag: &'static str) -> SpawnPlace {
        SpawnPlace { kind: abi::spawn_place::IN_LIQUID, tag: Some(tag) }
    }
    /// Hanging under a solid block: bats.
    pub fn ceiling() -> SpawnPlace {
        SpawnPlace { kind: abi::spawn_place::CEILING, tag: None }
    }
}

/// A natural spawning rule (see `abi::comp::spawn`), named `<mod>:<entity>`, `/2`.. unless `named`:
/// `r.natural_spawn(&PIG).biome("#animals").ground("#animal_spawn").light(9, 15).pack_size(2, 3).chance(0.05)`.
pub struct SpawnBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl Registry {
    pub fn natural_spawn(&mut self, entity: &EntityKind) -> SpawnBuilder<'_> {
        let name = self.derive_name(entity.path());
        SpawnBuilder { reg: self, e: crate::entry::Entry::new(abi::entry_kind::SPAWN, name) }.set_text(&comps::spawn::ENTITY, entity.name())
    }

    fn spawn_entry(&mut self, rule: &'static crate::SpawnRule) -> SpawnBuilder<'_> {
        SpawnBuilder { reg: self, e: crate::entry::Entry::of(rule) }
    }
}

crate::entry::entry_builder!(SpawnRule => SpawnBuilder, spawn_entry;);

impl SpawnBuilder<'_> {
    /// Its name instead of `<mod>:<entity>`.
    pub fn named(mut self, name: &str) -> Self {
        let derived = std::mem::replace(&mut self.e.name, qualify(self.reg.ns, name));
        self.reg.derived.retain(|n| *n != derived);
        self
    }
    /// Only on blocks `what` takes (a block matcher; under the spot, what `place` says it rests
    /// on): `when(Cond::block(what))`.
    pub fn ground(self, what: &str) -> Self {
        self.when(crate::Cond::block(what))
    }
    /// What kind of spot it takes: on the ground (the default), in a liquid, under a ceiling.
    /// The mob's height decides how much room it needs.
    pub fn place(self, place: SpawnPlace) -> Self {
        let b = self.set(&comps::spawn::PLACE, place.kind);
        match place.tag {
            Some(t) => b.ground(t),
            None => b,
        }
    }
    /// Only where this holds too (asked about the spot and the block it rests on).
    pub fn when(self, c: impl Into<crate::Cond>) -> Self {
        self.push(&comps::spawn::WHEN, c.into().0)
    }
    /// Light at the spot, inclusive (0..15): `when(Cond::light(min, max))`.
    pub fn light(self, min: u32, max: u32) -> Self {
        self.when(crate::Cond::light(min, max))
    }
    pub fn pack_size(self, min: u32, max: u32) -> Self {
        self.set(&comps::spawn::GROUP, [min, max])
    }
    pub fn weight(self, w: u32) -> Self {
        self.set(&comps::spawn::WEIGHT, w)
    }
    pub fn chance(self, c: f32) -> Self {
        self.set(&comps::spawn::CHANCE, c)
    }
}

/// The engine's entity categories (`.hostile()` and `.passive()` pick the first two).
pub mod entity_categories {
    use crate::EntityCategory;
    pub static HOSTILE: EntityCategory = EntityCategory::named("core:hostile");
    pub static PASSIVE: EntityCategory = EntityCategory::named("core:passive");
    pub static MISC: EntityCategory = EntityCategory::named("core:misc");
}

/// How a projectile flies (`EntityBuilder::projectile`): vanilla's arrow unless said otherwise,
/// 2 damage per block a tick of speed, 0.99 of its speed kept a tick in the air and 0.6 in a
/// liquid, gone where it hits a block.
#[derive(Clone, Debug)]
pub struct Projectile(abi::Projectile);

impl Default for Projectile {
    fn default() -> Self {
        Projectile::new()
    }
}

impl Projectile {
    pub fn new() -> Projectile {
        Projectile(abi::Projectile { damage: 2.0, drag: 0.99, liquid_drag: 0.6, sticks: false, stuck_ticks: 1200 })
    }
    /// Damage per block a tick of its speed, rounded up (`chain::PROJECTILE_DAMAGE`).
    pub fn damage(mut self, per_speed: f32) -> Self {
        self.0.damage = per_speed;
        self
    }
    /// Shares of its speed kept each tick in the air and in a liquid.
    pub fn drag(mut self, air: f32, liquid: f32) -> Self {
        (self.0.drag, self.0.liquid_drag) = (air, liquid);
        self
    }
    /// It stays stuck where it hits a block, for `ticks` (vanilla's arrow a minute).
    pub fn sticks(mut self, ticks: u32) -> Self {
        (self.0.sticks, self.0.stuck_ticks) = (true, ticks);
        self
    }
}

/// What hits land on, part by part (`EntityBuilder::hitboxes`): boxes in pixels (1/16 block, its
/// feet at 0, -Z its front, -X its right) at the kind's size; one made bigger is hit bigger. Rays, arrows and blows tell the part they hit (`RayHit::part`, `Damage::part`, the
/// `part` a chain step reads in its query). They go with the body, a head with the look, arms
/// and legs swing as it walks: run and an arrow may pass between them. Not the model's boxes:
/// how a mob looks never changes where it can be hit.
#[derive(Clone)]
pub struct Hitboxes(abi::Hitboxes);

impl Hitboxes {
    pub fn new() -> Hitboxes {
        Hitboxes(abi::Hitboxes { parts: Vec::new() })
    }
    fn add(mut self, name: &str, min: [f32; 3], max: [f32; 3], pivot: [f32; 3], follows: u32, swing: f32, phase: f32) -> Self {
        self.0
            .parts
            .push(abi::HitboxPart { name: name.into(), min, max, pivot, follows, swing, phase });
        self
    }
    /// A part going with the body.
    pub fn part(self, name: &str, min: [f32; 3], max: [f32; 3]) -> Self {
        self.add(name, min, max, [0.0; 3], abi::hitbox_follows::BODY, 0.0, 0.0)
    }
    /// A head: it turns with the look about `pivot`, its neck.
    pub fn head(self, name: &str, min: [f32; 3], max: [f32; 3], pivot: [f32; 3]) -> Self {
        self.add(name, min, max, pivot, abi::hitbox_follows::LOOK, 0.0, 0.0)
    }
    /// A leg or an arm: it swings about `pivot` as it walks, `swing` radians at a full stride,
    /// `phase` radians after the beat (`PI` the other half of the step).
    pub fn limb(self, name: &str, min: [f32; 3], max: [f32; 3], pivot: [f32; 3], swing: f32, phase: f32) -> Self {
        self.add(name, min, max, pivot, abi::hitbox_follows::STRIDE, swing, phase)
    }
    /// Two legs, two arms, a body and a head, 32 pixels tall: the player's.
    pub fn humanoid() -> Hitboxes {
        let pi = std::f32::consts::PI;
        Hitboxes::new()
            .head("head", [-4.0, 24.0, -4.0], [4.0, 32.0, 4.0], [0.0, 24.0, 0.0])
            .part("body", [-4.0, 12.0, -2.0], [4.0, 24.0, 2.0])
            .limb("right_arm", [-8.0, 12.0, -2.0], [-4.0, 24.0, 2.0], [-5.0, 22.0, 0.0], 0.8, pi)
            .limb("left_arm", [4.0, 12.0, -2.0], [8.0, 24.0, 2.0], [5.0, 22.0, 0.0], 0.8, 0.0)
            .limb("right_leg", [-4.0, 0.0, -2.0], [0.0, 12.0, 2.0], [-2.0, 12.0, 0.0], 0.8, 0.0)
            .limb("left_leg", [0.0, 0.0, -2.0], [4.0, 12.0, 2.0], [2.0, 12.0, 0.0], 0.8, pi)
    }
    /// A body on four legs with a head ahead, 25 pixels tall: a cow's.
    pub fn quadruped() -> Hitboxes {
        let pi = std::f32::consts::PI;
        Hitboxes::new()
            .head("head", [-5.0, 16.0, -15.0], [5.0, 25.0, -8.0], [0.0, 20.0, -8.0])
            .part("body", [-6.0, 11.0, -8.0], [6.0, 22.0, 10.0])
            .limb("front_right_leg", [-6.0, 0.0, -7.0], [-2.0, 12.0, -3.0], [-4.0, 12.0, -5.0], 0.8, pi)
            .limb("front_left_leg", [2.0, 0.0, -7.0], [6.0, 12.0, -3.0], [4.0, 12.0, -5.0], 0.8, 0.0)
            .limb("back_right_leg", [-6.0, 0.0, 5.0], [-2.0, 12.0, 9.0], [-4.0, 12.0, 7.0], 0.8, 0.0)
            .limb("back_left_leg", [2.0, 0.0, 5.0], [6.0, 12.0, 9.0], [4.0, 12.0, 7.0], 0.8, pi)
    }
    /// Its arms held out ahead, not swinging, as a zombie's.
    pub fn reaching(mut self) -> Self {
        for p in self
            .0
            .parts
            .iter_mut()
            .filter(|p| p.name == "right_arm" || p.name == "left_arm")
        {
            let (half, below, above) = ((p.max[2] - p.min[2]) / 2.0, p.pivot[1] - p.min[1], p.max[1] - p.pivot[1]);
            p.min = [p.min[0], p.pivot[1] - half, p.pivot[2] - below];
            p.max = [p.max[0], p.pivot[1] + half, p.pivot[2] + above];
            p.follows = abi::hitbox_follows::BODY;
        }
        self
    }
    /// Arms and legs `px` pixels thick, as a skeleton's.
    pub fn slim(mut self, px: f32) -> Self {
        for p in self
            .0
            .parts
            .iter_mut()
            .filter(|p| p.name.ends_with("_arm") || p.name.ends_with("_leg"))
        {
            for a in [0, 2] {
                let mid = (p.min[a] + p.max[a]) / 2.0;
                (p.min[a], p.max[a]) = (mid - px / 2.0, mid + px / 2.0);
            }
        }
        self
    }
}

/// What a ray looks for (`World::raycast`): blocks unless said otherwise.
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    flags: u32,
    skip: u64,
}

impl Ray {
    pub fn blocks() -> Ray {
        Ray { flags: abi::ray::BLOCKS, skip: 0 }
    }
    /// Only what can be hit: bodies, not dropped stacks, the dead or spectators.
    pub fn entities_only() -> Ray {
        Ray { flags: abi::ray::ENTITIES, skip: 0 }
    }
    /// Liquids stop it too.
    pub fn liquids(mut self) -> Ray {
        self.flags |= abi::ray::LIQUIDS;
        self
    }
    pub fn entities(mut self) -> Ray {
        self.flags |= abi::ray::ENTITIES;
        self
    }
    /// Blocks by what stops a body, not what is aimed at: through grass, as an arrow flies.
    pub fn solid(mut self) -> Ray {
        self.flags |= abi::ray::SOLID;
        self
    }
    /// It goes through `id` (the one looking, the shooter).
    pub fn skip(mut self, id: u64) -> Ray {
        self.skip = id;
        self
    }
}

/// One tick of a mob's walking for `World::walk`: a direction and the keys a player would hold.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Walk {
    dir: [f32; 2],
    speed: f32,
    keys: u32,
}

impl Walk {
    /// Toward `dir` (x, z, length up to 1) at `speed` times its kind's walking speed.
    pub fn toward(dir: [f32; 2], speed: f32) -> Walk {
        Walk { dir, speed, keys: 0 }
    }
    /// Jumps; swims and flies up.
    pub fn jump(self) -> Self {
        Walk { keys: self.keys | abi::walk::JUMP, ..self }
    }
    /// Never steps off an edge, holds on to ladders, crouches; flies down.
    pub fn sneak(self) -> Self {
        Walk { keys: self.keys | abi::walk::SNEAK, ..self }
    }
    /// Faster, a longer jump, swims lying down in a liquid it swims in.
    pub fn sprint(self) -> Self {
        Walk { keys: self.keys | abi::walk::SPRINT, ..self }
    }
    /// Takes off and flies, if its `attribute::CAN_FLY` is 1 or more (`.attribute(..)` on its
    /// kind, a modifier); it comes down when it lands without `jump`, as a player does.
    pub fn fly(self) -> Self {
        Walk { keys: self.keys | abi::walk::FLY, ..self }
    }
}
