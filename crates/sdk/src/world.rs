use crate::abi::{self, Str};
use crate::abi::{BlockPos, Vec3};
use crate::host;

/// A dimension as seen from a callback. Only valid for the duration of that callback.
#[derive(Clone, Copy)]
pub struct World {
    raw: *mut abi::WorldCtx,
}

impl World {
    pub(crate) fn from_raw(raw: *mut abi::WorldCtx) -> World {
        World { raw }
    }

    pub(crate) fn raw(&self) -> *mut abi::WorldCtx {
        self.raw
    }

    pub fn block(&self, pos: impl Into<BlockPos>) -> u32 {
        self.block_loaded(pos.into()).unwrap_or(0)
    }

    /// `block` that tells air from what isn't loaded here: `None` for that. A callback sees the
    /// region it runs in, so that is also anything far from the players around it.
    pub fn block_loaded(&self, pos: impl Into<BlockPos>) -> Option<u32> {
        let pos = pos.into();
        let s = unsafe { (host().block_get)(self.raw, &pos) };
        (s != u32::MAX).then_some(s)
    }

    /// The id of this dimension (`Dimension::id`).
    pub fn dimension(&self) -> u32 {
        unsafe { (host().world_dimension)(self.raw) }
    }

    /// False if the position is not loaded.
    pub fn set_block(&self, pos: impl Into<BlockPos>, state: u32) -> bool {
        let pos = pos.into();
        unsafe { (host().block_set)(self.raw, &pos, state) != 0 }
    }

    /// The same world for the blocks of moving body `body`, in its own coordinates (block
    /// (0, 0, 0) centered on its entity): everything about blocks through it (`block`, `set_block`,
    /// `prop_set`, parts, `schedule_tick`, light, signals) reaches the body's, as in its own blocks'
    /// callbacks. Meant for one statement, `w.on_body(ship).prop_set(pos, "lit", "true")`, or a
    /// short scope: while it lives, `w` itself is the body's too. `None` if `body` isn't one.
    pub fn on_body(&self, body: u64) -> Option<OnBody<'_>> {
        (unsafe { (host().world_enter_body)(self.raw, body) } != 0).then(|| OnBody { world: *self, _w: std::marker::PhantomData })
    }

    /// Takes `cells` out of the world into a new moving body, whose block (0, 0, 0) is at
    /// `center` and which they keep their places around. Chests and the like go along with
    /// what is in them, and it stands there till told otherwise: `entity_modify(id).velocity(..)` for a speed,
    /// `body_move_to` to go somewhere, `body_to_blocks` to be blocks again. Its entity, or 0 if nothing came.
    pub fn body_from_blocks(&self, center: impl Into<BlockPos>, cells: &[BlockPos]) -> u64 {
        let center = center.into();
        unsafe { (host().body_from_blocks)(self.raw, &center, cells.as_ptr(), cells.len()) }
    }

    /// A moving body goes back into the world at the nearest whole block. What found its
    /// place taken drops.
    pub fn body_to_blocks(&self, entity: u64) {
        unsafe { (host().body_to_blocks)(self.raw, entity) }
    }

    /// A moving body goes to `pos` in exactly `ticks`.
    pub fn body_move_to(&self, entity: u64, pos: impl Into<Vec3>, ticks: u32) {
        let pos = pos.into();
        unsafe { (host().body_move_to)(self.raw, entity, &pos, ticks) }
    }

    /// Moves what a piston behind `pos` facing `dir` would push one block that way, the way a
    /// piston does (sticky blocks bring their neighbors, immovable ones stop it): a drawbridge
    /// turned by a machine, a conveyor of blocks. Pulling is pushing the other way from the far
    /// side. False if it can't go.
    pub fn push_blocks(&self, pos: impl Into<BlockPos>, dir: crate::Dir) -> bool {
        let pos = pos.into();
        unsafe { (host().push_blocks)(self.raw, &pos, dir.face()) != 0 }
    }

    /// A moving body falls under its weight and lands as blocks, each where it comes down
    /// if it can stand there, else as its drops: a felled tree, a collapsing roof
    /// (`let id = w.body_from_blocks(base, &cells); w.body_fall(id);`).
    pub fn body_fall(&self, entity: u64) {
        unsafe { (host().body_fall)(self.raw, entity) }
    }

    /// What a comparator reads of the block at `pos` (`signal_read`) may be different now.
    pub fn signal_read_changed(&self, pos: impl Into<BlockPos>) {
        let pos = pos.into();
        unsafe { (host().signal_read_changed)(self.raw, &pos) }
    }

    pub fn schedule_tick(&self, pos: impl Into<BlockPos>, delay_ticks: u32) {
        let pos = pos.into();
        unsafe { (host().schedule_tick)(self.raw, &pos, delay_ticks) }
    }

    pub fn game_time(&self) -> u64 {
        unsafe { (host().game_time)(self.raw) }
    }

    pub fn random(&self) -> u64 {
        unsafe { (host().random)(self.raw) }
    }

    /// Uniform in `0..n`.
    pub fn random_below(&self, n: u32) -> u32 {
        ((self.random() >> 32) * n as u64 >> 32) as u32
    }

    pub fn block_flags(&self, pos: impl Into<BlockPos>) -> u32 {
        state_flags(self.block(pos.into()))
    }

    /// The strongest redstone power coming into a cell, 0..15.
    pub fn signal(&self, pos: impl Into<BlockPos>) -> u8 {
        let pos = pos.into();
        unsafe { (host().signal_at)(self.raw, &pos) as u8 }
    }

    /// The value of property `name` of the block at `pos`: `w.prop(pos, "lit")`.
    pub fn prop(&self, pos: impl Into<BlockPos>, name: &str) -> Option<&'static str> {
        let pos = pos.into();
        state_prop(self.block(pos), name).or_else(|| {
            self.parts(pos)
                .into_iter()
                .find_map(|s| state_prop(s, name))
        })
    }

    /// A number property of the block at `pos` (`age`, `fuse`).
    pub fn prop_int(&self, pos: impl Into<BlockPos>, name: &str) -> Option<u32> {
        self.prop(pos.into(), name)?.parse().ok()
    }

    /// The block at `pos` goes on to the next value of a property, the first after the last: a
    /// lever's power, a repeater's delay. False if it has no such property.
    pub fn prop_cycle(&self, pos: impl Into<BlockPos>, name: &str) -> bool {
        let pos = pos.into();
        let Some(old) = self.part_with_prop(pos, name) else { return false };
        let next = unsafe { (host().state_cycle)(old, Str::new(name)) };
        next != u32::MAX && state_prop(next, name).is_some_and(|v| self.prop_set(pos, name, v))
    }

    /// Of the block `block` gives, else of the first in the cell that has the property.
    fn part_with_prop(&self, pos: BlockPos, name: &str) -> Option<u32> {
        let own = self.block(pos);
        if state_prop(own, name).is_some() {
            return Some(own);
        }
        self.parts(pos)
            .into_iter()
            .find(|&s| state_prop(s, name).is_some())
    }

    /// Changes one property of the block at `pos`, keeping the others (its facing stays):
    /// `w.prop_set(pos, "lit", "true")`. False if the block has no such property or value.
    pub fn prop_set(&self, pos: impl Into<BlockPos>, name: &str, value: &str) -> bool {
        let pos = pos.into();
        let part = self.part_with_prop(pos, name);
        match part.and_then(|old| Some((old, state_with(old, name, value)?))) {
            Some((old, new)) if old == self.block(pos) => self.set_block(pos, new),
            Some((old, new)) => self.part_replace(pos, old, new),
            None => false,
        }
    }

    /// All that is in a cell, which may hold any number of blocks: the one `block` gives first, a
    /// slab and the water in it, a flower and the snow on it. Air has none.
    pub fn parts(&self, pos: impl Into<BlockPos>) -> Vec<u32> {
        let pos = pos.into();
        let mut out = [0u32; 16];
        let n = unsafe { (host().part_list)(self.raw, &pos, out.as_mut_ptr(), out.len() as u32) } as usize;
        if n <= out.len() {
            return out[..n].to_vec();
        }
        let mut all = vec![0u32; n];
        unsafe { (host().part_list)(self.raw, &pos, all.as_mut_ptr(), n as u32) };
        all
    }

    /// Puts another block in a cell with what is there, whatever its shape; `set_block` would replace
    /// all of it. Only one thing in a cell can have a block entity, a circuit, light of its own or
    /// growth: false if there would be two, or the cell isn't loaded.
    pub fn part_add(&self, pos: impl Into<BlockPos>, state: u32) -> bool {
        let pos = pos.into();
        unsafe { (host().part_add)(self.raw, &pos, state) != 0 }
    }

    /// Takes one block out of a cell, the others staying. False if it isn't there.
    pub fn part_remove(&self, pos: impl Into<BlockPos>, state: u32) -> bool {
        self.part_replace(pos.into(), state, 0)
    }

    /// One block of a cell becomes another state, the others staying. False if it isn't there.
    pub fn part_replace(&self, pos: impl Into<BlockPos>, old: u32, new: u32) -> bool {
        let pos = pos.into();
        unsafe { (host().part_replace)(self.raw, &pos, old, new) != 0 }
    }
}

/// The value of property `name` in `state` (`None` if its block has no such property).
pub fn state_prop(state: u32, name: &str) -> Option<&'static str> {
    let v = unsafe { (host().state_prop)(state, Str::new(name)).as_str() };
    (!v.is_empty()).then_some(v)
}

/// `state` with property `name` set to `value`, its other properties kept.
/// The `facing` property of a state, up if it has none.
pub fn state_facing(state: u32) -> crate::Dir {
    state_prop(state, "facing")
        .and_then(crate::Dir::from_name)
        .unwrap_or(crate::Dir::Up)
}

/// Where the other part of a `two_part` block is, from the part `state` at `pos`.
pub fn other_part(pos: impl Into<BlockPos>, state: u32, prop: &str, second: &str, way: impl Fn(u32) -> crate::Dir) -> BlockPos {
    let pos = pos.into();
    let (d, s) = (way(state).offset(), if state_prop(state, prop) == Some(second) { -1 } else { 1 });
    [pos.x + d.x * s, pos.y + d.y * s, pos.z + d.z * s].into()
}

/// The middle of a block.
pub fn block_center(p: impl Into<BlockPos>) -> Vec3 {
    p.into().center()
}

/// The middle of a block's floor: where something stands on it.
pub fn block_floor(p: impl Into<BlockPos>) -> Vec3 {
    let p = p.into();
    [p.x as f64 + 0.5, p.y as f64, p.z as f64 + 0.5].into()
}

/// The block a point is in.
pub fn block_pos_of(p: impl Into<Vec3>) -> BlockPos {
    p.into().block()
}

/// The yaw (radians, as `EntityState` has it) that faces from one point to another.
pub fn yaw_toward(from: impl Into<Vec3>, to: impl Into<Vec3>) -> f32 {
    let from = from.into();
    let to = to.into();
    (from.x - to.x).atan2(from.z - to.z) as f32
}

/// Where a yaw faces: x, z, of length 1.
pub fn yaw_dir(yaw: f32) -> [f32; 2] {
    [-yaw.sin(), -yaw.cos()]
}

/// Where a look goes (radians, as `EntityState` has them), of length 1.
pub fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    let (sy, cy) = (yaw as f64).sin_cos();
    let (sp, cp) = (pitch as f64).sin_cos();
    [-sy * cp, sp, -cy * cp].into()
}

/// `a` of `b`, 0..1, 0 of nothing: a progress bar's value.
pub fn ratio(a: u32, b: u32) -> f32 {
    if b == 0 { 0.0 } else { a as f32 / b as f32 }
}

pub fn state_with(state: u32, name: &str, value: &str) -> Option<u32> {
    let s = unsafe { (host().state_with)(state, Str::new(name), Str::new(value)) };
    (s != u32::MAX).then_some(s)
}

/// `state` turned by `o` with the world around it (a structure placed turned, a wrench): its
/// `facing`, `axis` or sides turned; itself where its block can't turn so.
pub fn state_turned(state: u32, o: crate::Orientation) -> u32 {
    match unsafe { (host().state_turned)(state, o.turn().index()) } {
        u32::MAX => state,
        s => s,
    }
}

pub fn state_flags(state: u32) -> u32 {
    unsafe { (host().state_flags)(state) }
}

pub fn block_of(state: u32) -> u32 {
    unsafe { (host().block_of)(state) }
}

/// Full name of a block state (`base:furnace[facing=north,lit=false]`), item, entity kind ...
/// by its id; `kind` is `abi::entry_kind`. Empty if unknown.
pub fn name_of(kind: u32, id: u32) -> &'static str {
    unsafe { (host().name_of)(kind, id).as_str() }
}

/// What to save instead of a dimension's id: ids are numbers in the order mods registered them, and
/// shift when a mod comes or goes. This goes by its name.
pub fn dimension_key(id: u32) -> u64 {
    abi::fnv(name_of(abi::entry_kind::DIMENSION, id).as_bytes())
}

/// The dimension a saved `dimension_key` means now, if it is still there.
pub fn dimension_from_key(key: u64) -> Option<u32> {
    (0..)
        .map_while(|id| Some((id, name_of(abi::entry_kind::DIMENSION, id))).filter(|n| !n.1.is_empty()))
        .find(|n| abi::fnv(n.1.as_bytes()) == key)
        .map(|n| n.0)
}

/// A world turned to a body's blocks (`World::on_body`); back to what it was when this goes.
pub struct OnBody<'a> {
    world: World,
    _w: std::marker::PhantomData<&'a World>,
}

impl std::ops::Deref for OnBody<'_> {
    type Target = World;
    fn deref(&self) -> &World {
        &self.world
    }
}

impl Drop for OnBody<'_> {
    fn drop(&mut self) {
        unsafe { (host().world_leave_body)(self.world.raw) }
    }
}

/// What a block state is besides its components (those: `components_of`): its block, the mod
/// it is from, its flags, light, look and feel. `None` for a state that doesn't exist.
#[derive(Clone, Copy, Debug)]
pub struct BlockInfo {
    pub block: u32,
    pub first_state: u32,
    pub states: u32,
    pub name: &'static str,
    pub owner: &'static str,
    /// `block_flags`: solid, opaque, replaceable ...
    pub flags: u32,
    pub opacity: u32,
    pub light: [u8; 3],
    pub absorb: [u8; 3],
    pub layer: abi::RenderLayer,
    pub model: abi::BlockModel,
    pub hardness: f32,
    pub friction: f32,
    pub drag: f32,
    /// Texture names per face, -X, +X, -Y, +Y, -Z, +Z.
    pub textures: [&'static str; 6],
    pub liquid: Option<u32>,
    /// The item that places it.
    pub item: Option<u32>,
}

impl BlockInfo {
    pub fn solid(&self) -> bool {
        self.flags & abi::block_flags::SOLID != 0
    }
    pub fn opaque(&self) -> bool {
        self.flags & abi::block_flags::OPAQUE != 0
    }
}

pub fn block_info(state: u32) -> Option<BlockInfo> {
    let mut d = std::mem::MaybeUninit::<abi::BlockInfo>::uninit();
    if unsafe { (host().block_info)(state, d.as_mut_ptr()) } == 0 {
        return None;
    }
    let d = unsafe { d.assume_init() };
    let some = |v: u32| (v != u32::MAX).then_some(v);
    Some(BlockInfo {
        block: d.block,
        first_state: d.first_state,
        states: d.states,
        name: unsafe { d.name.as_str() },
        owner: unsafe { d.owner.as_str() },
        flags: d.flags,
        opacity: d.opacity,
        light: d.light,
        absorb: d.absorb,
        layer: d.layer,
        model: d.model,
        hardness: d.hardness,
        friction: d.friction,
        drag: d.drag,
        textures: d.textures.map(|t| unsafe { t.as_str() }),
        liquid: some(d.liquid),
        item: some(d.item),
    })
}

/// What an item is besides its components (`components_of`): the mod it is from, its stack
/// size, durability and attack (a stack may carry its own: `Component::on_stack`), what it
/// places and where it is worn.
#[derive(Clone, Copy, Debug)]
pub struct ItemInfo {
    pub name: &'static str,
    pub owner: &'static str,
    pub max_stack: u32,
    pub durability: u32,
    pub attack: f32,
    pub block: Option<u32>,
    /// Its equipment slot's name, empty for none.
    pub worn: &'static str,
    pub handheld: bool,
}

pub fn item_info(item: u32) -> Option<ItemInfo> {
    let mut d = std::mem::MaybeUninit::<abi::ItemInfo>::uninit();
    if unsafe { (host().item_info)(item, d.as_mut_ptr()) } == 0 {
        return None;
    }
    let d = unsafe { d.assume_init() };
    Some(ItemInfo {
        name: unsafe { d.name.as_str() },
        owner: unsafe { d.owner.as_str() },
        max_stack: d.max_stack,
        durability: d.durability,
        attack: d.attack,
        block: (d.block != u32::MAX).then_some(d.block),
        worn: unsafe { d.worn.as_str() },
        handheld: d.handheld != 0,
    })
}

pub fn block_name(state: u32) -> &'static str {
    name_of(abi::entry_kind::BLOCK, state)
}

pub fn item_name(item: u32) -> &'static str {
    name_of(abi::entry_kind::ITEM, item)
}

/// How cubes kept loaded go on with no player near (`World::keep_loaded`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Loading {
    /// Block entities, systems, scheduled ticks and redstone; mobs stand still and
    /// nothing grows.
    BlockEntities,
    /// All of it, as near a player.
    All,
}

impl World {
    /// Keeps the cubes from block `from` to `to` loaded for `ticks` more under ticket `key` (a
    /// block entity's id, say): call again before it runs out to keep them, with other blocks
    /// to move it. Kept across restarts. False when a mod refused (`question::FORCE_LOAD`); one
    /// past `core:max_forced_cubes` in all is dropped.
    pub fn keep_loaded(&self, key: u64, from: impl Into<BlockPos>, to: impl Into<BlockPos>, loading: Loading, ticks: u32) -> bool {
        let mode = match loading {
            Loading::BlockEntities => abi::force_load::BLOCK_ENTITIES,
            Loading::All => abi::force_load::ALL,
        };
        let (a, b) = (from.into(), to.into());
        unsafe { (host().force_load)(self.raw(), key, &a, &b, mode, ticks) != 0 }
    }
    /// Lets ticket `key`'s cubes go.
    pub fn release_loaded(&self, key: u64) {
        let p = BlockPos::ZERO;
        unsafe { (host().force_load)(self.raw(), key, &p, &p, abi::force_load::OFF, 0) };
    }
}
