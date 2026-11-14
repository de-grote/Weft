//! Engine side of the mod ABI: the function table mods call, and the wrapper the engine uses to
//! call mod callbacks (profiling and fault isolation live there).

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use glam::{DVec3, IVec3};
use weft_abi::{self as abi, Callback, Host, ItemStack, ModId, Str};

use crate::content::{RawRegistrar, content, try_content};
use crate::registry;
use crate::server::Dim;
use crate::server::menu::MenuCall;

pub static HOST: Host = Host {
    log: host_log,
    log_at,
    log_enabled,
    reg_world_tick,
    timer,
    reg_command,
    find,
    block_of,
    state_prop,
    state_with,
    state_cycle,
    state_turned,
    state_flags,
    find_component,
    matcher_has,
    component_table,
    stack_get,
    stack_with,
    storage_slots,
    component_info,
    block_info,
    item_info,
    block_get,
    world_dimension,
    block_set,
    part_list,
    part_add,
    part_replace,
    game_time,
    random,
    light,
    daylight,
    world_session,
    entity_spawn,
    entity_despawn,
    entity_mount,
    entity_dismount,
    entity_vehicle,
    entity_riders,
    entity_get,
    entity_push,
    entity_set,
    entity_walk,
    entity_damage,
    entity_attack,
    entity_in_reach,
    raycast,
    entities_in,
    in_liquid,
    entity_swing_strength,
    entity_use_item,
    entity_key,
    entity_release_use,
    entity_mine,
    component,
    player_count,
    player_at,
    game_mode,
    inventory_len,
    inventory_get,
    inventory_set,
    inventory_insert,
    selected_slot,

    block_entity,
    open_menu,
    menu_group,
    inventory_group,
    menu_get,
    menu_set,
    recipe_match,
    recipe_process,
    recipe_find,
    recipe_info,
    recipe_output,
    recipe_roll,
    recipe_consume,
    item_block,
    ui_open: crate::client::modui::ui_open,
    ui_close: crate::client::modui::ui_close,
    ui_text: crate::client::modui::ui_text,
    client_player: crate::client::modui::client_player,
    client_attribute: crate::client::modui::client_attribute,
    client_effect_level: crate::client::modui::client_effect_level,
    client_swing_strength: crate::client::modui::client_swing_strength,
    client_use_ticks_left: crate::client::modui::client_using,
    client_day_time: crate::client::modui::client_day_time,
    client_weather: crate::client::modui::client_weather,
    client_component: crate::client::modui::client_component,
    client_entity: crate::client::modui::client_entity,
    client_target: crate::client::modui::client_target,
    energy_move,
    component_get,
    component_set,
    command_reply,
    show,
    client_show: crate::app::api::client_show,
    reg_chat_format,
    chat_format_set,
    gizmo_shown,
    gizmo,
    gizmo_text,
    reg_client_frame,
    client_pose_bone: crate::app::api::client_pose_bone,
    client_bone_position: crate::app::api::client_bone_position,
    client_action_current: crate::app::api::client_entity_action,
    client_clip_length: crate::app::api::client_clip_length,
    environment_set,
    client_environment_set: crate::app::api::client_environment_set,
    client_environment_values: crate::app::api::client_environment_values,
    client_displayed: crate::app::api::client_displayed,
    reg_packet,
    reg_mistake,
    launch_option: crate::app::api::launch_option,
    send_packet,
    entity_teleport,
    set_day_time,
    day_time,
    skip_ticks,
    fill,
    shape_contains,
    shape_bounds,
    set_game_mode,
    client_state: crate::app::api::client_state,
    client_input: crate::app::api::client_input,
    client_key_state: crate::app::api::client_pressed,
    keys_state: crate::input::keys_state,
    client_control: crate::app::api::client_set_player,
    client_op: crate::app::api::client_command,
    client_text: crate::app::api::client_text,
    client_command_reply: crate::app::api::client_command_reply,
    client_block_in: crate::app::api::client_block_in,
    client_cube_shown: crate::app::api::client_cube_shown,
    client_light: crate::app::api::client_light,
    client_slot: crate::app::api::client_slot,
    client_menu: crate::app::api::client_menu,
    client_entities: crate::app::api::client_entities,
    client_ui_rect: crate::app::api::client_ui_rect,
    client_draw: crate::app::api::client_draw,
    client_draw_text: crate::app::api::client_draw_text,
    client_screen_text: crate::app::api::client_screen_text,
    client_send_packet: crate::app::api::client_send_packet,
    image_pixels: crate::app::api::image_get,
    image_load: crate::app::api::image_load,
    image_save: crate::app::api::image_save,
    image_unload: crate::app::api::image_free,
    name_of,
    liquid_of,
    use_container,
    reg_event_handler,
    find_event,
    emit,
    ask,
    chain_compute,
    explanation,
    attribute,
    attribute_add_modifier,
    attribute_remove_modifier,
    reg_entry,
    mod_loaded,
    roll_loot,
    holds,
    break_block,
    client_menu_owner: crate::client::modui::client_menu_owner,
    reg_system,
    set_weather,
    weather_now,
    entity_effect,
    entity_effects,
    entity_set_pose,
    reg_command_permission,
    player_permission,
    set_player_permission,
    reg_command_branch,
    particle_burst,
    signal_at,
    client_particle_burst: crate::app::api::client_particle_burst,
    config_value,
    config_set,
    config_text,
    config_set_text,
    action_play,
    action_override,
    play_sound,
    client_play_sound: crate::app::api::client_play_sound,
    biome_at,
    navigate,
    navigating,
    block_list,
    place,
    unplace,
    plan_op,
    generation_read,
    template_size,
    template_joints,
    world_seed,
    structure_near,
    fall_block,
    player_inventory_group,
    set_entity_size,
    set_entity_appearance,
    entity_kind_size,
    body_from_blocks,
    body_to_blocks,
    body_fall,
    push_blocks,
    world_enter_body,
    world_leave_body,
    body_move_to,
    signal_read_changed,
    dim_column,
    ease,
    force_load,
    asset_exists,
    player_camera,
};

unsafe extern "C" fn fall_block(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u64 {
    unsafe { dim(w).fall_block(content(), ip(pos)) }
}

unsafe extern "C" fn block_list(index: u32) -> abi::Slice<[u32; 2]> {
    try_content()
        .and_then(|c| c.ref_lists.get(index as usize))
        .map_or(abi::Slice::EMPTY, |l| abi::Slice::new(l))
}

unsafe extern "C" fn place(w: *mut abi::WorldCtx, name: Str, pos: *const abi::BlockPos, transform: u32, palette: Str) -> u64 {
    unsafe {
        let c = content();
        let (name, palette) = (name.as_str(), palette.as_str());
        let d = dim(w);
        let at = ip(pos);
        let pal = match palette {
            "" => None,
            p => match c.palette_names.get(p) {
                Some(&p) => Some(p),
                None => {
                    misuse("place", || format!("no palette {p}"));
                    return 0;
                }
            },
        };
        let placed = if let Some(&t) = c.template_names.get(name) {
            d.place_template(c, &c.templates[t as usize], at, crate::template::Transform::new(transform), pal)
        } else if let Some(fi) = c.dims[d.id as usize]
            .features
            .iter()
            .position(|f| f.name == name)
        {
            d.place_feature(c, fi, at)
        } else {
            misuse("place", || format!("no template or feature {name} (here)"));
            return 0;
        };
        placed.unwrap_or(0)
    }
}

unsafe extern "C" fn unplace(w: *mut abi::WorldCtx, placing: u64) -> u32 {
    unsafe { dim(w).unplace(content(), placing) as u32 }
}

unsafe extern "C" fn generation_read(source: *const abi::GenSource, pos: abi::BlockPos) -> u32 {
    let Some(source) = (unsafe { source.as_ref() }) else { return 0 };
    let c = content();
    if source.dim as usize >= c.dims.len() {
        return 0;
    }
    let pos = crate::world::ivec(pos);
    if source.ctx.is_null() {
        if !crate::generation::can_read(source) {
            misuse("generation_read", || {
                "a generation callback cannot read its own or a later stage's output; use its borrowed view".into()
            });
            return 0;
        }
        crate::generation::Snapshot::new(c, source.dim, source.seed, source.before).read(pos)
    } else {
        unsafe { (&*source.ctx.cast::<crate::generation::ReadCtx>()).read(pos) }
    }
}

unsafe extern "C" fn plan_op(ctx: *mut abi::PlanCtx, op: *const abi::PlanOp) -> u32 {
    if ctx.is_null() || op.is_null() {
        return 0;
    }
    unsafe { (*(ctx as *mut crate::features::PlanCtx)).op(&*op) as u32 }
}

unsafe extern "C" fn template_size(name: Str, transform: u32, out: *mut abi::BlockPos) -> u32 {
    let Some(c) = try_content() else { return 0 };
    let Some(&t) = c.template_names.get(unsafe { name.as_str() }) else { return 0 };
    let size = crate::template::Transform::new(transform).size(c.templates[t as usize].size);
    unsafe { *out = size.to_array().into() };
    1
}

unsafe extern "C" fn template_joints(name: Str, transform: u32, out: *mut abi::Joint, cap: u32) -> u32 {
    let Some(c) = try_content() else { return 0 };
    let Some(&t) = c.template_names.get(unsafe { name.as_str() }) else { return 0 };
    let def = &c.templates[t as usize];
    let tr = crate::template::Transform::new(transform);
    for (i, j) in def.joints.iter().enumerate().take(cap as usize) {
        let pos = tr.cell(def.size, IVec3::from_array(j.pos));
        let joint = abi::Joint {
            pos: pos.to_array().into(),
            face: tr.face(j.face as usize) as u32,
            name: Str::new(&j.name),
            target: Str::new(&j.target),
            pool: Str::new(&j.pool),
        };
        unsafe { *out.add(i) = joint };
    }
    def.joints.len() as u32
}

unsafe extern "C" fn world_seed(w: *mut abi::WorldCtx) -> u64 {
    unsafe { dim(w).seed }
}

unsafe extern "C" fn structure_near(
    w: *mut abi::WorldCtx,
    name: Str,
    pos: *const abi::BlockPos,
    radius: i32,
    out: *mut abi::BlockPos,
) -> u32 {
    unsafe {
        let d = dim(w);
        match crate::features::nearest(content(), d.id, d.seed, name.as_str(), ip(pos), radius) {
            Some(p) => {
                *out = p.to_array().into();
                1
            }
            None => 0,
        }
    }
}

unsafe extern "C" fn navigate(w: *mut abi::WorldCtx, id: u64, target: *const abi::Vec3, speed: f32) -> u32 {
    unsafe {
        let c = content();
        let d = dim(w);
        let target = d.place.to_world(crate::world::dvec(*target));
        let mul = d.attribute(c, id, crate::registry::attr::MOVEMENT_SPEED, false) as f32;
        let jump_mul = d.attribute(c, id, crate::registry::attr::JUMP_STRENGTH, false) as f32;
        if let Some(e) = d.entities.get_mut(id) {
            e.jump_mul = jump_mul;
        }
        d.navigate(id, target, speed * mul) as u32
    }
}

unsafe extern "C" fn navigating(w: *mut abi::WorldCtx, id: u64) -> u32 {
    unsafe { dim(w).entities.get(id).is_some_and(|e| e.nav.is_some()) as u32 }
}

unsafe extern "C" fn biome_at(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u32 {
    unsafe {
        let p = ip(pos);
        dim(w)
            .space
            .cubes
            .get(&crate::world::cube_of(p))
            .map_or(0, |c| c.biome((p.x & 15) as usize, (p.z & 15) as usize) as u32)
    }
}

unsafe extern "C" fn action_override(w: *mut abi::WorldCtx, id: u64, from: Str, to: Str) {
    unsafe {
        let d = dim(w);
        if d.entities.get(id).is_none() {
            return;
        }
        let comp = content().game.fields.swaps;
        let (from, to) = (from.as_str(), to.as_str());
        let now = d
            .entities
            .comps
            .get(&(id, comp))
            .map_or(&[][..], |b| &b[..]);
        let text = String::from_utf8_lossy(now);
        let mut pairs = text.lines().filter_map(|l| l.split_once('='));
        // A goal says it every tick: the same as before costs one look.
        if pairs.clone().any(|p| p == (from, to)) || (to.is_empty() && !pairs.any(|p| p.0 == from)) {
            return;
        }
        let mut next = String::new();
        for (a, b) in text
            .lines()
            .filter_map(|l| l.split_once('='))
            .filter(|p| p.0 != from)
        {
            next.push_str(&format!("{a}={b}\n"));
        }
        if !to.is_empty() {
            next.push_str(&format!("{from}={to}\n"));
        }
        d.entities
            .comps
            .insert((id, comp), next.into_bytes().into_boxed_slice());
    }
}

unsafe extern "C" fn action_play(w: *mut abi::WorldCtx, id: u64, name: Str, looping: u32) {
    unsafe {
        let d = dim(w);
        // What loops goes on for whoever comes along later too, as anything else they should see.
        let comp = content().game.fields.looping;
        let now = if looping != 0 { name.as_str().as_bytes() } else { &[] };
        if d.entities.get(id).is_some()
            && d.entities
                .comps
                .get(&(id, comp))
                .map_or(&[][..], |b| &b[..])
                != now
        {
            d.entities
                .comps
                .insert((id, comp), now.to_vec().into_boxed_slice());
        }
        d.events
            .push(crate::server::DimEvent::PlayAction { id, name: name.as_str().to_string(), looping: looping != 0 });
    }
}

unsafe extern "C" fn play_sound(w: *mut abi::WorldCtx, who: abi::Who, sound: u32, pos: *const abi::Vec3, volume: f32, pitch: f32) {
    unsafe {
        if (sound as usize) < content().game.sounds.len() {
            let d = dim(w);
            let pos = pos
                .as_ref()
                .map(|p| d.place.to_world(crate::world::dvec(*p)));
            d.events
                .push(crate::server::DimEvent::Sound { dim: d.id, who, sound, pos, volume, pitch });
        } else {
            misuse("play_sound", || format!("no sound {sound}"));
        }
    }
}

// ---------------------------------------------------------------- profiling per mod

pub const MAX_MODS: usize = 256;

pub struct ModStats {
    pub nanos: AtomicU64,
    pub calls: AtomicU64,
    pub faults: AtomicU32,
}

static STATS: [ModStats; MAX_MODS] =
    [const { ModStats { nanos: AtomicU64::new(0), calls: AtomicU64::new(0), faults: AtomicU32::new(0) } }; MAX_MODS];

pub fn stats(m: ModId) -> &'static ModStats {
    &STATS[(m.0 as usize).min(MAX_MODS - 1)]
}

/// Callback nanoseconds and calls of a mod since start; readers diff two looks.
pub fn mod_totals(m: ModId) -> (u64, u64) {
    let s = stats(m);
    (s.nanos.load(Ordering::Relaxed), s.calls.load(Ordering::Relaxed))
}

/// Calls a mod callback. Time is charged to `owner`; a failing callback is logged with the mod's
/// name and the engine carries on.
pub fn invoke<A>(owner: ModId, cb: Callback, args: &mut A) -> bool {
    let Some(f) = cb.func else { return true };
    let start = Instant::now();
    let prev = crate::diag::enter_mod(owner);
    let r = unsafe { f(cb.data, args as *mut A as *mut c_void) };
    crate::diag::leave_mod(prev);
    let s = stats(owner);
    s.nanos
        .fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
    s.calls.fetch_add(1, Ordering::Relaxed);
    if r != 0 {
        let n = s.faults.fetch_add(1, Ordering::Relaxed);
        if n < 10 {
            let name = try_content().map_or("?".to_string(), |c| c.mod_name(owner).to_string());
            log::error!("callback of mod '{name}' failed (see its panic above), continuing");
        }
        return false;
    }
    true
}

// ---------------------------------------------------------------- world access from callbacks

/// `WorldCtx` pointers handed to mods point at one of these. Only valid during the callback.
pub struct CallCtx {
    pub dim: *mut Dim,
}

impl CallCtx {
    pub fn raw(&mut self) -> *mut abi::WorldCtx {
        self as *mut CallCtx as *mut abi::WorldCtx
    }
}

unsafe fn dim<'a>(w: *mut abi::WorldCtx) -> &'a mut Dim {
    unsafe { &mut *(*(w as *mut CallCtx)).dim }
}

fn ip(p: *const abi::BlockPos) -> IVec3 {
    crate::world::ivec(unsafe { *p })
}

unsafe extern "C" fn block_get(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u32 {
    unsafe { dim(w).seen(ip(pos)).unwrap_or(u32::MAX) }
}

unsafe extern "C" fn part_list(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, out: *mut u32, cap: u32) -> u32 {
    unsafe {
        let parts = dim(w).parts(ip(pos));
        std::ptr::copy_nonoverlapping(parts.as_ptr(), out, parts.len().min(cap as usize));
        parts.len() as u32
    }
}

unsafe extern "C" fn part_add(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, state: u32) -> u32 {
    if !known_state("part_add", state) {
        return 0;
    }
    unsafe { dim(w).add_part(content(), ip(pos), state) as u32 }
}

unsafe extern "C" fn part_replace(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, old: u32, new: u32) -> u32 {
    if !known_state("part_replace", new) {
        return 0;
    }
    unsafe { dim(w).replace_part(content(), ip(pos), old, new) as u32 }
}

unsafe extern "C" fn world_dimension(w: *mut abi::WorldCtx) -> u32 {
    unsafe { dim(w).id }
}

/// Whether block state `state` exists, a `misuse` of `func` if not.
fn known_state(func: &'static str, state: u32) -> bool {
    let known = (state as usize) < content().states.len();
    if !known {
        misuse(func, || format!("no block state {state}"));
    }
    known
}

/// A mod called a host function with something that does not exist: logged, the first 20 times
/// per function (a mod doing it every tick would flood the log).
pub(crate) fn misuse(func: &'static str, detail: impl FnOnce() -> String) {
    static SEEN: std::sync::Mutex<Vec<(&'static str, u32)>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let n = match seen.iter_mut().find(|s| s.0 == func) {
        Some(s) => {
            s.1 += 1;
            s.1
        }
        None => {
            seen.push((func, 1));
            1
        }
    };
    if n <= 20 {
        log::error!("{func}: {}{}", detail(), if n == 20 { " (not reported again)" } else { "" });
    }
}

unsafe extern "C" fn block_set(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, state: u32) -> u32 {
    if !known_state("block_set", state) {
        return 0;
    }
    unsafe { dim(w).set_seen(content(), ip(pos), state).is_some() as u32 }
}

unsafe extern "C" fn body_from_blocks(
    w: *mut abi::WorldCtx,
    center: *const abi::BlockPos,
    cells: *const abi::BlockPos,
    count: usize,
) -> u64 {
    unsafe {
        let cells: Vec<IVec3> = std::slice::from_raw_parts(cells, count)
            .iter()
            .map(|p| crate::world::ivec(*p))
            .collect();
        dim(w).lift_for_mod(content(), ip(center), &cells)
    }
}

unsafe extern "C" fn body_to_blocks(w: *mut abi::WorldCtx, entity: u64) {
    unsafe { dim(w).ask_put_down(entity) }
}

unsafe extern "C" fn world_enter_body(w: *mut abi::WorldCtx, entity: u64) -> u32 {
    unsafe { dim(w).enter_frame(entity) as u32 }
}

unsafe extern "C" fn world_leave_body(w: *mut abi::WorldCtx) {
    unsafe { dim(w).leave_frame() }
}

unsafe extern "C" fn push_blocks(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, dir: u32) -> u32 {
    unsafe { dim(w).push_blocks(content(), ip(pos), dir.min(6) as u8) as u32 }
}

unsafe extern "C" fn body_fall(w: *mut abi::WorldCtx, entity: u64) {
    unsafe { dim(w).body_fall(entity) }
}

unsafe extern "C" fn body_move_to(w: *mut abi::WorldCtx, entity: u64, pos: *const abi::Vec3, ticks: u32) {
    unsafe {
        let d = dim(w);
        let to = d.place.to_world(crate::world::dvec(*pos));
        if let Some(e) = d
            .entities
            .get_mut(entity)
            .filter(|e| e.motion != crate::server::entity::Motion::Parked)
        {
            e.motion = crate::server::entity::Motion::To { to, left: ticks.clamp(1, u16::MAX as u32) as u16 };
        }
    }
}

unsafe extern "C" fn signal_read_changed(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) {
    unsafe { dim(w).space.signals.read_changed(content(), ip(pos)) }
}

unsafe extern "C" fn game_time(w: *mut abi::WorldCtx) -> u64 {
    unsafe { dim(w).time }
}

unsafe extern "C" fn random(w: *mut abi::WorldCtx) -> u64 {
    unsafe { dim(w).random() }
}

unsafe extern "C" fn light(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u32 {
    unsafe { dim(w).light(ip(pos)) }
}

extern "C" fn world_session(_: *mut abi::WorldCtx) -> u64 {
    crate::server::SESSION.load(std::sync::atomic::Ordering::Relaxed)
}

unsafe extern "C" fn daylight(w: *mut abi::WorldCtx) -> f32 {
    unsafe { dim(w).daylight(content()) }
}

unsafe extern "C" fn entity_spawn(w: *mut abi::WorldCtx, desc: *const abi::SpawnEntityDesc) -> u64 {
    let c = content();
    let d = unsafe { &*desc };
    let kind = d.kind;
    if kind as usize >= c.game.entities.len() {
        misuse("entity_spawn", || format!("no entity kind {kind}"));
        return 0;
    }
    if c.game.is_block_entity(kind) {
        misuse("entity_spawn", || format!("{} belongs to a block, set the block instead", c.game.entities[kind as usize].name));
        return 0;
    }
    let is_item = kind == c.game.item_kind;
    if !is_item && !d.stack.is_empty() {
        misuse("entity_spawn", || format!("{} carries no stack", c.game.entities[kind as usize].name));
    }
    if is_item && !known_item("entity_spawn", &d.stack) {
        return 0;
    }
    let dim = unsafe { dim(w) };
    let pos = crate::world::dvec(d.pos);
    let inherited = if d.set & abi::spawn_set::INHERIT_VELOCITY != 0 { dim.world_velocity(d.owner) } else { DVec3::ZERO };
    // Items are their stack: nothing to spawn for an empty one.
    // How it starts, set before anyone is asked about it.
    let init = |e: &mut crate::server::entity::Entity| {
        use abi::spawn_set as s;
        if d.set & s::VELOCITY != 0 && crate::world::dvec(d.velocity).is_finite() {
            e.vel = crate::world::dvec(d.velocity);
        }
        e.vel += inherited;
        if d.set & s::YAW != 0 && d.yaw.is_finite() {
            e.yaw = d.yaw;
        }
        if d.set & s::PICKUP_DELAY != 0 {
            e.pickup_delay = d.pickup_delay;
        }
        e.owner = d.owner;
    };
    if is_item {
        let id = dim.spawn_item(c, pos, d.stack);
        if let Some(e) = dim.entities.get_mut(id) {
            init(e);
        }
        return id;
    }
    dim.spawn_by(c, kind, pos, d.cause, init)
}

unsafe extern "C" fn entity_despawn(w: *mut abi::WorldCtx, id: u64) {
    unsafe {
        let d = dim(w);
        if d.entities.get(id).is_some_and(|e| e.player) {
            misuse("entity_remove", || "a player can't be removed; kill it or teleport it".to_string());
            return;
        }
        d.entities.remove(id, abi::removal::MOD)
    }
}

unsafe extern "C" fn entity_mount(w: *mut abi::WorldCtx, rider: u64, mount: u64, seat: u32) -> u32 {
    unsafe { dim(w).mount(content(), rider, mount, (seat != u32::MAX).then_some(seat.min(255) as u8)) as u32 }
}

unsafe extern "C" fn entity_dismount(w: *mut abi::WorldCtx, rider: u64) {
    unsafe {
        dim(w).dismount(content(), rider);
    }
}

unsafe extern "C" fn entity_vehicle(w: *mut abi::WorldCtx, rider: u64, seat: *mut u32) -> u64 {
    unsafe {
        let Some((mount, s)) = dim(w).entities.get(rider).and_then(|e| e.vehicle) else { return 0 };
        if !seat.is_null() {
            *seat = s as u32;
        }
        mount
    }
}

unsafe extern "C" fn entity_riders(w: *mut abi::WorldCtx, mount: u64, out: *mut u64, cap: u32) -> u32 {
    unsafe {
        let d = dim(w);
        let Some(m) = d.entities.get(mount) else { return 0 };
        let seats = content().game.entities[m.kind as usize].seats.len() as u32;
        for s in 0..seats.min(cap) {
            *out.add(s as usize) = d.rider_in(mount, s as u8).unwrap_or(0);
        }
        seats
    }
}

unsafe extern "C" fn entity_get(w: *mut abi::WorldCtx, id: u64, out: *mut abi::EntityState) -> u32 {
    unsafe {
        let d = dim(w);
        let c = content();
        let max = d.max_health(c, id);
        let kept = d.entities.comps.contains_key(&(id, c.game.persistent_comp));
        match d.entities.get(id) {
            Some(e) => {
                *out = e.state(c, max);
                if kept {
                    (*out).flags |= abi::entity_state::PERSISTENT;
                }
                (*out).pos = (d.place.to_local(e.pos).to_array()).into();
                1
            }
            None => 0,
        }
    }
}

unsafe extern "C" fn entity_set(w: *mut abi::WorldCtx, id: u64, set: u32, s: *const abi::EntityState) {
    use crate::server::entity::Told;
    use abi::entity_set as es;
    unsafe {
        let s = &*s;
        let d = dim(w);
        let c = content();
        let max_health = d.max_health(c, id);
        let Some(e) = d.entities.get_mut(id) else { return };
        let player = e.player;
        let mut told = Vec::new();
        let pos = crate::world::dvec(s.pos);
        let teleport = set & es::POS != 0 && pos.is_finite();
        if teleport && !player {
            e.pos = d.place.to_world(pos);
        }
        let vel = crate::world::dvec(s.vel);
        if set & es::VELOCITY != 0 && vel.is_finite() {
            e.vel = vel;
            told.push(Told::Velocity((vel / crate::world::TPS as f64).as_vec3().to_array()));
        }
        if set & es::YAW != 0 && s.yaw.is_finite() {
            e.yaw = s.yaw;
        }
        if set & es::PITCH != 0 && s.pitch.is_finite() {
            e.pitch = s.pitch;
        }
        if set & (es::YAW | es::PITCH) != 0 {
            told.push(Told::Look([e.yaw, e.pitch]));
        }
        if set & es::FALL_DISTANCE != 0 && s.fall_distance.is_finite() {
            e.fall = s.fall_distance.max(0.0);
        }
        if set & es::FLYING != 0 {
            e.flying = s.flying != 0;
            told.push(Told::Flying(e.flying));
        }
        if set & es::FIRE != 0 {
            e.fire = s.fire;
        }
        let hands = c.game.entities[e.kind as usize].hand_slots;
        if set & es::SELECTED != 0 && hands > 0 {
            e.selected = s.selected.min(hands - 1);
            if player {
                told.push(Told::Selected(e.selected));
            }
        }
        // Only a respawn brings back the dead, and only damage kills: everything that follows
        // a death goes one way.
        let dies = set & es::HEALTH != 0 && e.alive() && {
            let health = s.health.min(max_health);
            if health > 0.0 {
                e.health = health;
            }
            !(health > 0.0)
        };
        if player {
            d.entities.told.extend(told.into_iter().map(|t| (id, t)));
        }
        if teleport && player {
            d.events
                .push(crate::server::DimEvent::Teleport { from: d.id, id, dim: d.id, pos: d.place.to_world(pos) });
        }
        if set & es::PERSISTENT != 0 {
            let key = (id, c.game.persistent_comp);
            if s.flags & abi::entity_state::PERSISTENT != 0 {
                d.entities.comps.insert(key, Box::new([]));
            } else {
                d.entities.comps.remove(&key);
            }
        }
        if dies {
            d.damage(c, id, &abi::Damage::new(f32::MAX, weft_abi::damage_cause::VOID));
        }
    }
}

unsafe extern "C" fn entity_push(w: *mut abi::WorldCtx, id: u64, v: *const abi::Vec3) {
    unsafe {
        let v = crate::world::dvec(*v);
        if !v.is_finite() {
            return;
        }
        dim(w).push(id, v);
    }
}

unsafe extern "C" fn entity_walk(w: *mut abi::WorldCtx, id: u64, dir: *const [f32; 2], speed: f32, keys: u32) {
    unsafe {
        let (d, c) = (dim(w), content());
        let mut attr = |a| d.attribute(c, id, a, false) as f32;
        use crate::registry::attr as a;
        let (mul, jump_mul, can_fly, can_sprint) = (attr(a::MOVEMENT_SPEED), attr(a::JUMP_STRENGTH), attr(a::CAN_FLY), attr(a::CAN_SPRINT));
        if let Some(e) = d.entities.get_mut(id).filter(|e| !e.player) {
            // What it may not do, its keys don't do: as a hungry player's sprint key.
            let keys = if can_sprint < 1.0 { keys & !abi::walk::SPRINT } else { keys };
            e.walk = Some(crate::server::entity::Walk { dir: *dir, speed: speed * mul, keys });
            e.jump_mul = jump_mul;
            // It takes off with the key, and comes down when it may fly no more or lands.
            if keys & abi::walk::FLY != 0 || can_fly < 1.0 {
                e.flying = keys & abi::walk::FLY != 0 && can_fly >= 1.0;
            }
        }
    }
}

unsafe extern "C" fn raycast(
    w: *mut abi::WorldCtx,
    from: *const abi::Vec3,
    dir: *const abi::Vec3,
    max: f64,
    flags: u32,
    skip: u64,
    out: *mut abi::RayHit,
) -> u32 {
    unsafe {
        let hit = dim(w).raycast(content(), crate::world::dvec(*from), crate::world::dvec(*dir), max.min(1024.0), flags, &[skip]);
        if let Some(h) = hit {
            *out = h;
        }
        hit.is_some() as u32
    }
}

unsafe extern "C" fn entity_damage(w: *mut abi::WorldCtx, id: u64, damage: *const abi::Damage) -> u32 {
    unsafe { dim(w).damage(content(), id, &*damage) as u32 }
}

unsafe extern "C" fn entity_in_reach(w: *mut abi::WorldCtx, id: u64, target: u64) -> u32 {
    unsafe { crate::server::actions::in_reach(content(), dim(w), id, target) as u32 }
}

unsafe extern "C" fn entity_attack(w: *mut abi::WorldCtx, attacker: u64, target: u64) -> u32 {
    unsafe { dim(w).melee(content(), attacker, target) as u32 }
}

unsafe extern "C" fn in_liquid(w: *mut abi::WorldCtx, pos: *const abi::Vec3) -> u32 {
    unsafe {
        let d = dim(w);
        crate::physics::liquid_at(content(), &*d.space, crate::world::dvec(*pos)).is_some() as u32
    }
}

unsafe extern "C" fn entity_use_item(w: *mut abi::WorldCtx, id: u64, off: u32, target: *const abi::BlockPos, face: u32) -> u32 {
    unsafe {
        let (d, c) = (dim(w), content());
        let Some(e) = d.entities.get(id) else { return 0 };
        let inv = d.entities.inventories.get(&id).map_or(&[][..], |i| &i[..]);
        // The main hand as `content::in_main_hand` has it: none while both are needed.
        let main = !crate::content::in_main_hand(c, e.kind, inv, e.selected).is_empty();
        let slot = if off != 0 { c.game.entities[e.kind as usize].off } else { main.then_some(e.selected) };
        let target = (!target.is_null()).then(|| (ip(target), face.min(5) as u8));
        slot.is_some_and(|slot| d.use_in_hand(c, id, slot, target, false)) as u32
    }
}

unsafe extern "C" fn entity_release_use(w: *mut abi::WorldCtx, id: u64) {
    unsafe { dim(w).release_use(content(), id) }
}

unsafe extern "C" fn entity_key(w: *mut abi::WorldCtx, player: u64, key: Str, out: *mut abi::KeyState) {
    unsafe {
        let (d, c) = (dim(w), content());
        let id = c.game.keys.iter().position(|k| k.name == key.as_str());
        let since = id.and_then(|id| {
            d.entities
                .get(player)?
                .keys
                .iter()
                .find(|k| k.0 == id as u32)
                .map(|k| k.1)
        });
        *out = abi::KeyState {
            held: since.is_some() as u32,
            held_ticks: since.map_or(0, |s| d.time.saturating_sub(s) as u32),
            ..Default::default()
        };
    }
}

unsafe extern "C" fn entity_mine(w: *mut abi::WorldCtx, id: u64, pos: *const abi::BlockPos) -> u32 {
    unsafe {
        let d = dim(w);
        if pos.is_null() {
            d.stop_mining(id);
            return 1;
        }
        d.start_mining(content(), id, 0, ip(pos), u32::MAX) as u32
    }
}

unsafe extern "C" fn entity_swing_strength(w: *mut abi::WorldCtx, id: u64) -> f32 {
    unsafe {
        let d = dim(w);
        d.swing_strength(content(), id, d.entities.held(id))
    }
}

unsafe extern "C" fn entities_in(
    w: *mut abi::WorldCtx,
    shape: abi::Slice<abi::ShapeStep>,
    kind: u32,
    flags: u32,
    out: *mut u64,
    max: usize,
) -> usize {
    unsafe {
        let Some(shape) = crate::shape::Shape::from_abi(shape.as_slice()) else {
            misuse("entities_in", || "the shape's steps don't make one shape".to_string());
            return 0;
        };
        let d = dim(w);
        let ids = d.entities.within(content(), &d.place, &shape, kind, flags);
        for (i, id) in ids.iter().take(max).enumerate() {
            *out.add(i) = *id;
        }
        ids.len()
    }
}

unsafe extern "C" fn component(w: *mut abi::WorldCtx, id: u64, comp: u32) -> *mut u8 {
    unsafe {
        let d = dim(w);
        // Writes are found by comparing at sync time (`Entities::find_changes`).
        d.entities
            .comps
            .get_mut(&(id, comp))
            .map_or(std::ptr::null_mut(), |b| b.as_mut_ptr())
    }
}

unsafe extern "C" fn player_count(w: *mut abi::WorldCtx) -> u32 {
    unsafe { dim(w).visible_players().len() as u32 }
}

unsafe extern "C" fn player_at(w: *mut abi::WorldCtx, index: u32) -> u64 {
    unsafe {
        dim(w)
            .visible_players()
            .get(index as usize)
            .copied()
            .unwrap_or(0)
    }
}

/// What `holder` points at here, `None` (and nothing done) when it isn't.
unsafe fn held(w: *mut abi::WorldCtx, h: *const abi::Holder) -> Option<crate::server::holder::At> {
    unsafe { crate::server::holder::resolve(dim(w), &*h) }
}

/// The inventory `holder` points at here, `None` (and nothing done) when there is none.
unsafe fn inventory(w: *mut abi::WorldCtx, h: *const abi::Holder) -> Option<crate::server::holder::Inv> {
    unsafe { crate::server::holder::inventory(dim(w), content(), &*h) }
}

unsafe extern "C" fn inventory_len(w: *mut abi::WorldCtx, h: *const abi::Holder) -> u32 {
    unsafe { inventory(w, h).map_or(0, |inv| crate::server::holder::len(dim(w), content(), inv)) }
}

unsafe extern "C" fn inventory_get(w: *mut abi::WorldCtx, h: *const abi::Holder, slot: u32) -> ItemStack {
    unsafe { inventory(w, h).map_or(ItemStack::EMPTY, |inv| crate::server::holder::get(dim(w), content(), inv, slot)) }
}

/// A stack from a mod: anything it says goes into saves and on screen, where a wrong id would
/// bring down the game instead of naming the mod.
pub(crate) fn known_item(what: &'static str, stack: &ItemStack) -> bool {
    let known = stack.is_empty() || (stack.item as usize) < content().items.len();
    if !known {
        misuse(what, || format!("no item with id {}", stack.item));
    }
    known
}

unsafe extern "C" fn inventory_set(w: *mut abi::WorldCtx, h: *const abi::Holder, slot: u32, stack: ItemStack) {
    if known_item("inventory_set", &stack) {
        unsafe {
            if let Some(inv) = inventory(w, h) {
                crate::server::holder::set(dim(w), content(), inv, slot, stack)
            }
        }
    }
}

unsafe extern "C" fn inventory_insert(w: *mut abi::WorldCtx, h: *const abi::Holder, stack: ItemStack) -> ItemStack {
    if !known_item("inventory_insert", &stack) {
        return ItemStack::EMPTY;
    }
    unsafe {
        match inventory(w, h) {
            Some(inv) => crate::server::holder::insert(dim(w), content(), inv, stack),
            None => stack,
        }
    }
}

unsafe extern "C" fn selected_slot(w: *mut abi::WorldCtx, player: u64) -> u32 {
    unsafe { dim(w).entities.get(player).map_or(0, |e| e.selected) }
}

unsafe extern "C" fn block_entity(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u64 {
    unsafe { dim(w).anchored(ip(pos)).unwrap_or(0) }
}

unsafe extern "C" fn open_menu(w: *mut abi::WorldCtx, player: u64, menu: u32, h: *const abi::Holder) -> u32 {
    use crate::server::holder::{self, At};
    unsafe {
        let d = dim(w);
        let c = content();
        let Some(def) = c.game.menus.get(menu as usize) else {
            log::error!("open_menu: there is no menu {menu}");
            return 0;
        };
        let h = &*h;
        let on = match holder::inventory(d, c, h) {
            _ if h.kind == abi::holder::NONE => None,
            Some(inv) if matches!(inv.at, At::Entity(_)) || holder::len(d, c, inv) > 0 => Some(inv),
            found => {
                let what = match h.kind {
                    _ if h.storage != 0 && holder::resolve(d, h).is_some() => {
                        let name = c
                            .game
                            .components
                            .get(h.storage as usize)
                            .map_or("?", |d| d.name.as_str());
                        format!("storage {name}, which isn't one or isn't carried there")
                    }
                    abi::holder::BLOCK => {
                        let p = crate::world::ivec(h.pos);
                        d.get(p)
                            .map_or("an unloaded block".to_string(), |s| format!("{} at {p}, which has no block entity", c.state_name(s)))
                    }
                    abi::holder::HELD if found.is_some() => {
                        format!("the stack in slot {} of entity {}, which holds no items", h.slot, h.entity)
                    }
                    abi::holder::HELD => format!("slot {} of entity {}, which isn't here", h.slot, h.entity),
                    _ => format!("entity {} (removed, or in another region)", h.entity),
                };
                log::error!("cannot open menu {} on {what}", def.name);
                return 0;
            }
        };
        d.events
            .push(crate::server::DimEvent::OpenMenu { player, menu, on, block: 0 });
        1
    }
}

unsafe extern "C" fn day_time(w: *mut abi::WorldCtx) -> u64 {
    unsafe {
        let d = dim(w);
        d.time.saturating_add_signed(d.day_offset)
    }
}

unsafe extern "C" fn skip_ticks(w: *mut abi::WorldCtx, ticks: u64) {
    unsafe {
        let d = dim(w);
        d.events
            .push(crate::server::DimEvent::SkipTicks { dim: d.id, ticks })
    }
}

unsafe extern "C" fn entity_teleport(w: *mut abi::WorldCtx, id: u64, to: u32, pos: *const abi::Vec3) {
    unsafe {
        let d = dim(w);
        d.events
            .push(crate::server::DimEvent::Teleport { from: d.id, id, dim: to, pos: d.place.to_world(crate::world::dvec(*pos)) })
    }
}

unsafe extern "C" fn set_day_time(w: *mut abi::WorldCtx, time: u64) {
    unsafe {
        let d = dim(w);
        d.events
            .push(crate::server::DimEvent::SetTime { dim: d.id, time })
    }
}

unsafe extern "C" fn fill(w: *mut abi::WorldCtx, desc: *const abi::FillDesc) {
    unsafe {
        let d = dim(w);
        let a = &*desc;
        let c = content();
        if d.frame != 0 {
            misuse("fill", || "fills are for the world, not a body's blocks".to_string());
            return;
        }
        let Some(shape) = crate::shape::Shape::from_abi(a.shape.as_slice()) else {
            misuse("fill", || "the shape's steps don't make one shape".to_string());
            return;
        };
        if !known_state("fill", a.state) {
            return;
        }
        let fill = match crate::server::area::AreaFill::new(c, shape, a.state, a.keep.as_str(), a.only.as_str()) {
            Ok(f) => f,
            Err(e) => {
                misuse("fill", || e);
                return;
            }
        };
        d.events
            .push(crate::server::DimEvent::AreaFill { dim: d.id, fill });
    }
}

unsafe extern "C" fn shape_contains(shape: abi::Slice<abi::ShapeStep>, p: *const abi::Vec3) -> u32 {
    unsafe { crate::shape::Shape::from_abi(shape.as_slice()).is_some_and(|s| s.contains(crate::world::dvec(*p))) as u32 }
}

unsafe extern "C" fn shape_bounds(shape: abi::Slice<abi::ShapeStep>, min: *mut abi::Vec3, max: *mut abi::Vec3) -> u32 {
    unsafe {
        let Some(s) = crate::shape::Shape::from_abi(shape.as_slice()) else { return 0 };
        let (lo, hi) = s.bounds();
        (*min, *max) = (lo.to_array().into(), hi.to_array().into());
        1
    }
}

unsafe extern "C" fn force_load(
    w: *mut abi::WorldCtx,
    key: u64,
    from: *const abi::BlockPos,
    to: *const abi::BlockPos,
    mode: u32,
    ticks: u32,
) -> u32 {
    unsafe {
        let d = dim(w);
        let (a, b) = (crate::world::ivec(*from), crate::world::ivec(*to));
        if mode != abi::force_load::OFF {
            let mut q = abi::Query { other: key, pos: *from, amount: mode as f64, ..abi::Query::DEFAULT };
            if !d.ask(content(), abi::action::FORCE_LOAD, &mut q) {
                return 0;
            }
        }
        let (a, b) = (d.place.to_world(a.as_dvec3()).floor().as_ivec3(), d.place.to_world(b.as_dvec3()).floor().as_ivec3());
        d.events.push(crate::server::DimEvent::ForceLoad {
            dim: d.id,
            key,
            min: crate::world::cube_of(a.min(b)),
            max: crate::world::cube_of(a.max(b)),
            mode,
            ticks,
        });
        1
    }
}

unsafe extern "C" fn asset_exists(name: Str, kind: Str, ext: Str) -> u32 {
    unsafe { crate::modloader::asset_exists(name.as_str(), kind.as_str(), ext.as_str()) as u32 }
}

unsafe extern "C" fn ease(kind: u32, t: f32) -> f32 {
    crate::client::camera::ease(kind, t)
}

unsafe extern "C" fn player_camera(w: *mut abi::WorldCtx, player: u64, mode: u32, locked: u32, transition: f32, easing: u32) {
    unsafe {
        dim(w)
            .events
            .push(crate::server::DimEvent::Camera { player, mode, locked: locked != 0, transition, easing })
    }
}

unsafe extern "C" fn set_game_mode(w: *mut abi::WorldCtx, player: u64, mode: u32) {
    let mode = crate::world::GameMode::from_id(mode);
    unsafe {
        dim(w)
            .events
            .push(crate::server::DimEvent::SetMode { player, mode })
    }
}

unsafe extern "C" fn send_packet(w: *mut abi::WorldCtx, who: abi::Who, name: Str, schema: Str, data: abi::Slice<u8>) {
    unsafe {
        if let Some(channel) = channel_for(name.as_str(), schema.as_str()) {
            let d = dim(w);
            d.events
                .push(crate::server::DimEvent::Packet { dim: d.id, who, channel, data: data.as_slice().to_vec() });
        }
    }
}

/// The channel's id if it is registered and carries `schema`; a reason in the log otherwise.
pub fn channel_for(name: &str, schema: &str) -> Option<u32> {
    let g = &content().game;
    let Some(&channel) = g.packet_names.get(name) else {
        misuse("send", || format!("no channel {name} is registered"));
        return None;
    };
    let have = &g.packets[channel as usize].schema;
    if have != schema {
        misuse("send", || format!("channel {name} carries '{have}', not '{schema}'"));
        return None;
    }
    Some(channel)
}

unsafe extern "C" fn find_component(name: Str, schema: Str) -> u32 {
    let (name, schema) = unsafe { (name.as_str(), schema.as_str()) };
    let Some(c) = try_content() else { return u32::MAX };
    let Some(id) = c.components.id(name) else { return u32::MAX };
    let have = &c.components.schemas[id as usize];
    if !schema.is_empty() && have != schema {
        log::error!("component {name} is '{have}' but a mod reads it as '{schema}'");
        return u32::MAX;
    }
    id
}

extern "C" fn matcher_has(matcher: u32, state: u32) -> u32 {
    try_content().is_some_and(|c| c.matchers.has(matcher, state)) as u32
}

unsafe extern "C" fn component_table() -> *const abi::ComponentTable {
    try_content().map_or(std::ptr::null(), |c| c.components.table())
}

unsafe extern "C" fn stack_get(data: u32, comp: u32, len: *mut usize) -> *const u8 {
    match crate::stack::value(data, comp) {
        Some(b) => {
            if !len.is_null() {
                unsafe { *len = b.len() }
            }
            b.as_ptr()
        }
        None => std::ptr::null(),
    }
}

unsafe extern "C" fn stack_with(data: u32, comp: u32, value: abi::Slice<u8>, remove: u32) -> u32 {
    if try_content().is_none_or(|c| comp >= c.components.count()) {
        misuse("stack_with", || "a component nobody declared: r.stack_component(&COMP, 0) makes it one".to_string());
        return data;
    }
    let bytes = unsafe { value.as_slice() };
    crate::stack::with(data, comp, (remove == 0).then_some(bytes))
}

unsafe extern "C" fn storage_slots(comp: u32) -> u32 {
    try_content()
        .and_then(|c| crate::server::holder::storage_slots(c, comp))
        .unwrap_or(0)
}

unsafe extern "C" fn component_info(comp: u32, name: *mut Str, schema: *mut Str) -> u32 {
    let Some(c) = try_content() else { return 0 };
    let Some(n) = c.components.names.get(comp as usize) else { return 0 };
    let s = c
        .components
        .schemas
        .get(comp as usize)
        .map_or("", String::as_str);
    unsafe {
        *name = Str::new(n);
        *schema = Str::new(s);
    }
    1
}

unsafe extern "C" fn block_info(state: u32, out: *mut abi::BlockInfo) -> u32 {
    let Some(c) = try_content() else { return 0 };
    let Some(s) = c.states.get(state as usize) else { return 0 };
    let b = &c.blocks[s.block as usize];
    let tex = |i: u32| {
        c.textures
            .get(i as usize)
            .map_or(Str::EMPTY, |t| Str::new(t))
    };
    let item = c
        .items
        .iter()
        .position(|i| i.block == Some(b.first_state))
        .map_or(u32::MAX, |i| i as u32);
    unsafe {
        *out = abi::BlockInfo {
            block: s.block,
            first_state: b.first_state,
            states: b.states,
            name: Str::new(&b.name),
            owner: Str::new(c.mod_name(b.owner)),
            flags: s.flags,
            opacity: s.opacity as u32,
            light: s.light,
            absorb: s.absorb,
            layer: s.layer,
            model: s.model,
            hardness: s.hardness,
            friction: s.friction,
            drag: s.drag,
            textures: s.textures.map(tex),
            liquid: s.liquid.map_or(u32::MAX, |l| l as u32),
            item,
        };
    }
    1
}

unsafe extern "C" fn item_info(item: u32, out: *mut abi::ItemInfo) -> u32 {
    let Some(c) = try_content() else { return 0 };
    let Some(d) = c.items.get(item as usize) else { return 0 };
    let worn = d
        .equip
        .and_then(|e| c.game.player_layout.equipment.get(e as usize))
        .map_or(Str::EMPTY, |e| Str::new(&e.name));
    unsafe {
        *out = abi::ItemInfo {
            name: Str::new(&d.name),
            owner: Str::new(c.mod_name(d.owner)),
            max_stack: c.max_stack(&abi::ItemStack::new(item, 1)),
            durability: c.item_value(item, c.ids.durability).unwrap_or(0),
            attack: c.item_value(item, c.ids.attack_damage).unwrap_or(1.0),
            block: d.block.unwrap_or(u32::MAX),
            worn,
            handheld: c.item_has(item, c.ids.handheld) as u32,
        };
    }
    1
}

unsafe extern "C" fn energy_move(w: *mut abi::WorldCtx, h: *const abi::Holder, amount: i64, simulate: u32) -> i64 {
    unsafe { held(w, h).map_or(0, |at| crate::server::holder::move_energy(dim(w), content(), at, amount, simulate != 0)) }
}

unsafe extern "C" fn component_get(w: *mut abi::WorldCtx, h: *const abi::Holder, comp: u32, out: *mut u8, cap: u32) -> u32 {
    unsafe {
        let Some(v) = held(w, h).and_then(|at| crate::server::holder::component(dim(w), at, comp)) else { return 0 };
        std::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len().min(cap as usize));
        v.len() as u32
    }
}

unsafe extern "C" fn environment_set(w: *mut abi::WorldCtx, entity: u64, name: abi::Str, data: abi::Slice<u8>) -> u32 {
    unsafe {
        let d = dim(w);
        if d.entities.get(entity).is_none_or(|e| e.removed) {
            return 0;
        }
        let c = content();
        let id = c.components.id(abi::environment::COMPONENT).unwrap();
        let old = d
            .entities
            .comps
            .get(&(entity, id))
            .map_or(&[][..], |b| b.as_ref());
        let mut layers: Vec<abi::environment::EnvironmentLayer> = abi::data::from_bytes(old).unwrap_or_default();
        let key = name.as_str();
        if !data.as_slice().is_empty() {
            let Some(mut layer) = abi::data::from_bytes::<abi::environment::EnvironmentLayer>(data.as_slice()) else { return 0 };
            layer.name = key.to_string();
            layers.retain(|l| l.name != key);
            layers.push(layer);
        } else {
            layers.retain(|l| l.name != key);
        }
        d.entities
            .comps
            .insert((entity, id), abi::data::bytes_of(&layers).into_boxed_slice());
        1
    }
}

unsafe extern "C" fn component_set(w: *mut abi::WorldCtx, h: *const abi::Holder, comp: u32, value: abi::Slice<u8>) -> u32 {
    unsafe {
        let v = value.as_slice();
        held(w, h).is_some_and(|at| crate::server::holder::set_component(dim(w), content(), at, comp, (!v.is_empty()).then_some(v))) as u32
    }
}

unsafe extern "C" fn gizmo_shown(w: *mut abi::WorldCtx, group: Str) -> u32 {
    unsafe { dim(w).gizmo_groups.iter().any(|g| g == group.as_str()) as u32 }
}

unsafe extern "C" fn gizmo(
    w: *mut abi::WorldCtx,
    who: abi::Who,
    group: Str,
    kind: u32,
    a: *const abi::Vec3,
    b: *const abi::Vec3,
    color: u32,
    ticks: u32,
) {
    unsafe {
        let d = dim(w);
        let group = group.as_str();
        if kind > abi::gizmo_kind::FILL || !d.gizmo_seen(group) {
            return;
        }
        let (a, b) = (d.place.to_world(crate::world::dvec(*a)), d.place.to_world(crate::world::dvec(*b)));
        d.gizmos
            .push((who, crate::net::GizmoShape::shape(group, kind as u8, a, b, color, ticks.max(1))));
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn gizmo_text(
    w: *mut abi::WorldCtx,
    who: abi::Who,
    group: Str,
    pos: *const abi::Vec3,
    size: f32,
    color: u32,
    ticks: u32,
    key: Str,
    args: abi::Slice<abi::UiArg>,
) {
    unsafe {
        let d = dim(w);
        let group = group.as_str();
        if !d.gizmo_seen(group) {
            return;
        }
        let at = d.place.to_world(crate::world::dvec(*pos));
        let text = crate::ui::text::Msg::new(key.as_str(), crate::client::modui::args(args));
        d.gizmos
            .push((who, crate::net::GizmoShape::text(group, at, size, color, ticks.max(1), text)));
    }
}

// ---------------------------------------------------------------- menus

unsafe fn menu<'a>(m: *mut abi::MenuCtx) -> &'a mut MenuCall {
    unsafe { &mut *(m as *mut MenuCall) }
}

unsafe extern "C" fn menu_group(m: *mut abi::MenuCtx, name: Str, out: *mut abi::SlotGroup) -> u32 {
    unsafe {
        match menu(m).group(content(), name.as_str()) {
            Some((start, len, cols)) => {
                *out = abi::SlotGroup { start, len, cols };
                1
            }
            None => 0,
        }
    }
}

/// A machine's group in its entity's inventory: the menu built for its kind says where.
unsafe extern "C" fn inventory_group(w: *mut abi::WorldCtx, h: *const abi::Holder, name: Str, out: *mut abi::SlotGroup) -> u32 {
    unsafe {
        let c = content();
        let entity = match held(w, h) {
            Some(crate::server::holder::At::Entity(id)) if (*h).storage == 0 => id,
            _ => return 0,
        };
        let Some(e) = dim(w).entities.get(entity) else { return 0 };
        let layout = &c.game.player_layout;
        // A player's groups are the player inventory's.
        if e.player {
            return match layout.group(name.as_str()) {
                Some(g) => {
                    *out = abi::SlotGroup { start: g.start, len: g.count, cols: g.cols };
                    1
                }
                None => 0,
            };
        }
        // A wearing kind's equipment, each slot named like it.
        let worn = &c.game.entities[e.kind as usize].worn;
        let slot = layout
            .equipment
            .iter()
            .position(|q| q.name == name.as_str())
            .map(|i| i as u32);
        let (start, len) = match slot {
            _ if name.as_str() == registry::EQUIPMENT_GROUP => (worn.start, worn.len() as u32),
            Some(i) if i < worn.len() as u32 => (worn.start + i, 1),
            _ => (0, 0),
        };
        if len > 0 {
            *out = abi::SlotGroup { start, len, cols: 1 };
            return 1;
        }
        // A block entity's are those of the menu its block opens.
        let found = c
            .game
            .block_entities
            .iter()
            .find(|&(_, &k)| k == e.kind)
            .and_then(|(&b, _)| c.blocks[b as usize].opens)
            .and_then(|m| c.game.menus[m as usize].group(layout, name.as_str()))
            .filter(|g| g.0.owner == abi::slot_owner::HOLDER);
        match found {
            Some((g, _, n)) => {
                *out = abi::SlotGroup { start: g.start, len: n, cols: g.cols };
                1
            }
            None => 0,
        }
    }
}

unsafe extern "C" fn player_inventory_group(name: Str, out: *mut abi::SlotGroup) -> u32 {
    unsafe {
        match content().game.player_layout.group(name.as_str()) {
            Some(g) => {
                *out = abi::SlotGroup { start: g.start, len: g.count, cols: g.cols };
                1
            }
            None => 0,
        }
    }
}

unsafe extern "C" fn menu_get(m: *mut abi::MenuCtx, slot: u32) -> ItemStack {
    unsafe { menu(m).get(slot) }
}

unsafe extern "C" fn menu_set(m: *mut abi::MenuCtx, slot: u32, stack: ItemStack) {
    if known_item("menu_set", &stack) {
        unsafe { menu(m).set(slot, stack) }
    }
}

// ---------------------------------------------------------------- lookups

unsafe extern "C" fn recipe_match(kind: Str, grid: *const ItemStack, width: u32, height: u32) -> ItemStack {
    let kind = unsafe { kind.as_str() };
    let cells: Vec<u32> = (0..(width * height) as usize)
        .map(|i| unsafe { (*grid.add(i)).item })
        .collect();
    content()
        .game
        .match_grid(kind, &cells, width, height, &content().components)
        .map_or(ItemStack::EMPTY, |r| r.result())
}

fn recipe_at(kind: Str, recipe: u32) -> Option<&'static registry::Recipe> {
    content()
        .game
        .recipes
        .get(unsafe { kind.as_str() })?
        .get(recipe as usize)
}

unsafe extern "C" fn recipe_find(kind: Str, stacks: *const ItemStack, count: u32, from: u32) -> u32 {
    let c = content();
    let stacks = unsafe { std::slice::from_raw_parts(stacks, count as usize) };
    let Some(list) = c.game.recipes.get(unsafe { kind.as_str() }) else { return 0 };
    (from as usize..list.len())
        .find(|&i| {
            c.game
                .find_inputs(&list[i], stacks, &c.components)
                .is_some()
        })
        .map_or(0, |i| i as u32 + 1)
}

unsafe extern "C" fn recipe_info(kind: Str, recipe: u32, out: *mut abi::RecipeInfo) -> u32 {
    let Some(r) = recipe_at(kind, recipe) else { return 0 };
    unsafe { *out = abi::RecipeInfo { time: r.time, inputs: r.inputs.len() as u32, outputs: r.outputs.len() as u32, entry: r.entry } };
    1
}

unsafe extern "C" fn recipe_output(kind: Str, recipe: u32, index: u32, stack: *mut ItemStack, chance: *mut f32) -> u32 {
    let Some(o) = recipe_at(kind, recipe).and_then(|r| r.outputs.get(index as usize)) else { return 0 };
    unsafe {
        *stack = match o.what {
            registry::Ingredient::Item(i) => ItemStack::new(i, o.count),
            _ => ItemStack::EMPTY,
        };
        *chance = o.chance;
    }
    1
}

unsafe extern "C" fn recipe_roll(w: *mut abi::WorldCtx, kind: Str, recipe: u32, out: *mut ItemStack, max: usize) -> usize {
    let Some(r) = recipe_at(kind, recipe) else { return 0 };
    unsafe {
        let stacks = dim(w).roll_table(content(), &r.extras, &Default::default(), DVec3::ZERO, 0);
        for (i, s) in stacks.iter().take(max).enumerate() {
            *out.add(i) = *s;
        }
        stacks.len()
    }
}

unsafe extern "C" fn recipe_consume(kind: Str, recipe: u32, stacks: *mut ItemStack, count: u32) -> u32 {
    let c = content();
    let Some(r) = recipe_at(kind, recipe) else { return 0 };
    let stacks = unsafe { std::slice::from_raw_parts_mut(stacks, count as usize) };
    let Some(found) = c.game.find_inputs(r, stacks, &c.components) else { return 0 };
    let used = (0..r.inputs.len()).filter(|&k| !matches!(r.inputs[k], registry::Ingredient::Empty));
    for (i, k) in found.into_iter().zip(used) {
        let (s, n) = (stacks[i], r.input_counts[k]);
        stacks[i] = match r.uses.get(k).copied().unwrap_or(registry::Use::Consume) {
            registry::Use::Keep => s,
            registry::Use::Wear(w) => match crate::stack::worn(c, s, w) {
                broken if broken.is_empty() => remainder_of(c, s.item).map_or(ItemStack::EMPTY, |x| ItemStack::new(x, 1)),
                after => after,
            },
            // What is left of the last of a stack takes its place; a machine has nowhere else
            // to put it.
            u if s.count == n => match u {
                registry::Use::Leaves(x) => ItemStack::new(x, n),
                _ => remainder_of(c, s.item).map_or(ItemStack::EMPTY, |x| ItemStack::new(x, n)),
            },
            _ => ItemStack { count: s.count - n, ..s },
        };
    }
    1
}

fn remainder_of(c: &crate::content::Content, item: u32) -> Option<u32> {
    let id = c.components.id(abi::comp::REMAINDER)?;
    c.components
        .get::<u32>(crate::components::ITEM, item, id)
        .filter(|&r| r != 0)
}

unsafe extern "C" fn recipe_process(kind: Str, input: u32, time_out: *mut u32) -> ItemStack {
    let kind = unsafe { kind.as_str() };
    match content().game.process(kind, input, &content().components) {
        Some(r) => {
            if !time_out.is_null() {
                unsafe { *time_out = r.time }
            }
            r.result()
        }
        None => ItemStack::EMPTY,
    }
}

unsafe extern "C" fn item_block(item: u32) -> u32 {
    try_content()
        .and_then(|c| c.items.get(item as usize))
        .and_then(|d| d.block)
        .unwrap_or(0)
}

unsafe extern "C" fn find(kind: u32, name: Str, count: *mut u32) -> u32 {
    let name = unsafe { name.as_str() };
    let Some(c) = try_content() else { return u32::MAX };
    let mut n = 1;
    fn at<'a>(mut names: impl Iterator<Item = &'a str>, name: &str) -> Option<u32> {
        names.position(|n| n == name).map(|i| i as u32)
    }
    let id = match kind {
        abi::entry_kind::BLOCK if name.contains('[') => c.state_by_name(name),
        abi::entry_kind::BLOCK => c.block_names.get(name).map(|&b| {
            n = c.blocks[b as usize].states;
            c.blocks[b as usize].first_state
        }),
        abi::entry_kind::ITEM => c.item_names.get(name).copied(),
        abi::entry_kind::ENTITY => c.game.entity_names.get(name).copied(),
        abi::entry_kind::BIOME => c.biome_names.get(name).copied(),
        abi::entry_kind::DIMENSION => c.dim_names.get(name).copied(),
        abi::entry_kind::MENU => c.game.menu_names.get(name).copied(),
        abi::entry_kind::PARTICLE => c.game.particle_names.get(name).copied(),
        abi::entry_kind::CONFIG => c.game.config_names.get(name).copied(),
        abi::entry_kind::SOUND => c.game.sound_names.get(name).copied(),
        abi::entry_kind::DAMAGE_CAUSE => c.game.damage_cause_names.get(name).copied(),
        abi::entry_kind::ENTITY_EFFECT => c.game.entity_effect_names.get(name).copied(),
        abi::entry_kind::TEXT_PLACE => c.game.text_place_names.get(name).copied(),
        abi::entry_kind::GAME_MODE => c.game.game_mode_names.get(name).copied(),
        abi::entry_kind::POSE => c.game.pose_names.get(name).copied(),
        abi::entry_kind::WEATHER => c.game.weather_names.get(name).copied(),
        abi::entry_kind::PRECIPITATION => c.game.precipitation_names.get(name).copied(),
        abi::entry_kind::CONDITION => c.game.condition_names.get(name).copied(),
        abi::entry_kind::MUSIC => at(c.game.music.iter().map(|m| m.name.as_str()), name),
        abi::entry_kind::CAMERA => c.game.camera_names.get(name).copied(),
        abi::entry_kind::EQUIPMENT_SLOT => at(
            c.game
                .player_layout
                .equipment
                .iter()
                .map(|e| e.name.as_str()),
            name,
        ),
        abi::entry_kind::CREATIVE_TAB => at(c.creative_tabs.iter().map(|t| t.name.as_str()), name),
        abi::entry_kind::SOUND_CATEGORY => at(c.game.sound_categories.iter().map(|k| k.name.as_str()), name),
        abi::entry_kind::ENTITY_CATEGORY => at(c.game.categories.iter().map(|k| k.name.as_str()), name),
        abi::entry_kind::TINT_KIND => at(c.tint_kinds.iter().map(|k| k.0.as_str()), name),
        abi::entry_kind::RECIPE_KIND => c.game.recipe_kinds.get(name).copied(),
        abi::entry_kind::ATTRIBUTE => c.game.attr_names.get(name).copied(),
        abi::entry_kind::KEY => at(c.game.keys.iter().map(|k| k.name.as_str()), name),
        abi::entry_kind::LIQUID => at(c.liquids.iter().map(|l| l.name.as_str()), name),
        abi::entry_kind::FEATURE => c.feature_names.get(name).copied(),
        abi::entry_kind::RECORD => c.record_names.get(name).copied(),
        _ => None,
    };
    if !count.is_null() {
        unsafe { *count = if id.is_some() { n } else { 0 } }
    }
    id.unwrap_or(u32::MAX)
}

unsafe extern "C" fn name_of(kind: u32, id: u32) -> Str {
    static STATE_NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let Some(c) = try_content() else { return Str::EMPTY };
    let name: Option<&str> = match kind {
        abi::entry_kind::BLOCK => {
            let names = STATE_NAMES.get_or_init(|| {
                (0..c.states.len() as u32)
                    .map(|s| c.state_name(s))
                    .collect()
            });
            names.get(id as usize).map(|s| s.as_str())
        }
        abi::entry_kind::ITEM => c.items.get(id as usize).map(|i| i.name.as_str()),
        abi::entry_kind::ENTITY => c.game.entities.get(id as usize).map(|e| e.name.as_str()),
        abi::entry_kind::BIOME => c.biomes.get(id as usize).map(|b| b.name.as_str()),
        abi::entry_kind::DIMENSION => c.dims.get(id as usize).map(|d| d.name.as_str()),
        abi::entry_kind::MENU => c.game.menus.get(id as usize).map(|m| m.name.as_str()),
        abi::entry_kind::PARTICLE => c.game.particles.get(id as usize).map(|p| p.name.as_str()),
        abi::entry_kind::CONFIG => c.game.configs.get(id as usize).map(|d| d.name.as_str()),
        abi::entry_kind::SOUND => c.game.sounds.get(id as usize).map(|d| d.name.as_str()),
        abi::entry_kind::FEATURE => c.features.get(id as usize).map(|d| d.as_str()),
        abi::entry_kind::RECORD => c.records.get(id as usize).map(|d| d.as_str()),
        _ => None,
    };
    name.map_or(Str::EMPTY, Str::new)
}

unsafe extern "C" fn state_prop(state: u32, name: Str) -> Str {
    let c = content();
    if state as usize >= c.states.len() {
        return Str::EMPTY;
    }
    c.state_prop(state, unsafe { name.as_str() })
        .map_or(Str::EMPTY, Str::new)
}

unsafe extern "C" fn state_with(state: u32, name: Str, value: Str) -> u32 {
    let c = content();
    if state as usize >= c.states.len() {
        return u32::MAX;
    }
    unsafe { c.state_with(state, name.as_str(), value.as_str()) }.unwrap_or(u32::MAX)
}

extern "C" fn state_turned(state: u32, turn: u8) -> u32 {
    let c = content();
    match weft_abi::Turn::from_index(turn) {
        Some(t) if (state as usize) < c.states.len() => c.turned_state(state, t),
        _ => u32::MAX,
    }
}

unsafe extern "C" fn state_cycle(state: u32, name: Str) -> u32 {
    let c = content();
    let Some(s) = c.states.get(state as usize) else { return u32::MAX };
    let name = unsafe { name.as_str() };
    let Some(prop) = c.blocks[s.block as usize]
        .props
        .iter()
        .find(|p| p.name == name)
    else {
        return u32::MAX;
    };
    let at = (state - c.blocks[s.block as usize].first_state) / prop.stride.max(1);
    let next = (at as usize % prop.values.len().max(1) + 1) % prop.values.len().max(1);
    c.state_with(state, name, &prop.values[next])
        .unwrap_or(u32::MAX)
}

unsafe extern "C" fn block_of(state: u32) -> u32 {
    let c = content();
    c.states
        .get(state as usize)
        .map_or(0, |s| c.blocks[s.block as usize].first_state)
}

unsafe extern "C" fn state_flags(state: u32) -> u32 {
    content().states.get(state as usize).map_or(0, |s| s.flags)
}

// ---------------------------------------------------------------- registration

fn level(level: u32) -> log::Level {
    match level {
        abi::log_level::ERROR => log::Level::Error,
        abi::log_level::WARN => log::Level::Warn,
        abi::log_level::DEBUG => log::Level::Debug,
        abi::log_level::TRACE => log::Level::Trace,
        _ => log::Level::Info,
    }
}

fn mod_target(me: ModId, sub: &str) -> String {
    let name = crate::modloader::mod_id_name(me);
    if sub.is_empty() { format!("mod:{name}") } else { format!("mod:{name}::{sub}") }
}

unsafe extern "C" fn host_log(me: ModId, lvl: u32, msg: Str) {
    unsafe { log_at(me, lvl, Str::EMPTY, msg) }
}

unsafe extern "C" fn log_at(me: ModId, lvl: u32, target: Str, msg: Str) {
    let (target, msg) = unsafe { (mod_target(me, target.as_str()), msg.as_str()) };
    log::log!(target: &target, level(lvl), "{msg}");
}

unsafe extern "C" fn log_enabled(me: ModId, lvl: u32, target: Str) -> u32 {
    let lvl = level(lvl);
    if lvl > log::max_level() {
        return 0;
    }
    crate::log::enabled(&mod_target(me, unsafe { target.as_str() }), lvl) as u32
}

unsafe fn reg<'a>(r: *mut abi::Registrar) -> &'a mut RawRegistrar {
    unsafe { &mut *(r as *mut RawRegistrar) }
}

unsafe extern "C" fn reg_world_tick(r: *mut abi::Registrar, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game.world_ticks.push((owner, cb));
    }
}

unsafe extern "C" fn timer(w: *mut abi::WorldCtx, h: *const abi::Holder, ticks: u32, event: u32, data: abi::Slice<u8>) {
    unsafe {
        let c = content();
        if c.game.events.get(event as usize).is_none() {
            misuse("timer", || format!("no event {event}"));
            return;
        }
        let (d, h, data) = (dim(w), &*h, data.as_slice());
        let until = d.time + ticks.max(1) as u64;
        match h.kind {
            abi::holder::ENTITY if d.entities.get(h.entity).is_some() => {
                let list = d
                    .entities
                    .comps
                    .entry((h.entity, c.game.timers_comp))
                    .or_insert_with(|| Box::new([]));
                let mut v = list.to_vec();
                crate::server::dim::push_timer(&mut v, until, &c.game.events[event as usize].name, data);
                *list = v.into_boxed_slice();
            }
            abi::holder::ENTITY => {}
            // A block's own tick is the same whoever set it.
            abi::holder::BLOCK if event == c.game.block_tick => d.schedule(ip(&h.pos), ticks),
            abi::holder::BLOCK => d.timer_at(ip(&h.pos), ticks, event, data),
            abi::holder::NONE => d.dim_timers.push((until, event, data.into())),
            _ => misuse("timer", || "a stack doesn't tick, so it can't hold a timer".into()),
        }
    }
}

unsafe extern "C" fn dim_column(dim: u32, seed: u64, x: i32, z: i32, out: *mut abi::TerrainColumn) -> u32 {
    let Some(col) = content()
        .dims
        .get(dim as usize)
        .and_then(|d| d.column(seed, x, z))
    else {
        return 0;
    };
    unsafe { *out = col };
    1
}

unsafe extern "C" fn reg_command(r: *mut abi::Registrar, name: Str, usage: Str, client: u32, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        let list = if client != 0 { &mut r.game.client_commands } else { &mut r.game.commands };
        list.push((owner, name.as_str().to_string(), usage.as_str().to_string(), cb));
    }
}

unsafe extern "C" fn command_reply(args: *mut abi::CommandArgs, key: Str, list: abi::Slice<abi::UiArg>) {
    unsafe {
        let reply = &mut *((*args).reply as *mut Vec<crate::ui::text::Msg>);
        reply.push(crate::ui::text::Msg::new(key.as_str(), crate::client::modui::args(list)));
    }
}

/// The text of a `ShowDesc`, to whoever it goes.
pub(crate) unsafe fn shown(d: &abi::ShowDesc) -> crate::net::Shown {
    unsafe {
        crate::net::Shown {
            id: d.id.as_str().to_string(),
            times: d.times,
            scale: d.scale,
            at: (d.at[0] >= 0.0 && d.at[1] >= 0.0).then_some(d.at),
            color: (d.color >= 0).then_some(d.color as u32),
            ..crate::net::Shown::new(d.place, crate::ui::text::Msg::new(d.key.as_str(), crate::client::modui::args(d.args)))
        }
    }
}

unsafe extern "C" fn show(w: *mut abi::WorldCtx, who: abi::Who, d: *const abi::ShowDesc) {
    unsafe {
        let r = dim(w);
        r.events
            .push(crate::server::DimEvent::Show { dim: r.id, who, shown: shown(&*d) })
    }
}

unsafe extern "C" fn reg_chat_format(r: *mut abi::Registrar, cb: Callback) {
    unsafe {
        let r = reg(r);
        r.game.chat_format = Some((r.owner, cb));
    }
}

unsafe extern "C" fn chat_format_set(args: *mut abi::ChatFormatArgs, key: Str, list: abi::Slice<abi::UiArg>) {
    unsafe {
        let out = &mut *((*args).out as *mut Option<crate::ui::text::Msg>);
        *out = Some(crate::ui::text::Msg::new(key.as_str(), crate::client::modui::args(list)));
    }
}

unsafe extern "C" fn game_mode(w: *mut abi::WorldCtx, entity: u64) -> u32 {
    unsafe { dim(w).entities.get(entity).map_or(0, |e| e.mode.0 as u32) }
}

unsafe extern "C" fn liquid_of(state: u32) -> u32 {
    try_content()
        .and_then(|c| c.states.get(state as usize))
        .and_then(|s| s.liquid)
        .map_or(u32::MAX, |l| l as u32)
}

unsafe extern "C" fn use_container(w: *mut abi::WorldCtx, player: u64, stack: ItemStack, handled: *mut u32) -> ItemStack {
    unsafe {
        let r = dim(w).use_container(content(), player, stack);
        if !handled.is_null() {
            *handled = r.is_some() as u32;
        }
        r.unwrap_or(stack)
    }
}

unsafe extern "C" fn reg_event_handler(r: *mut abi::Registrar, name: Str, schema: Str, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .event_handlers
            .push((owner, name.as_str().to_string(), schema.as_str().to_string(), cb));
    }
}

unsafe extern "C" fn find_event(name: Str, schema: Str) -> u32 {
    let (name, schema) = unsafe { (name.as_str(), schema.as_str()) };
    let Some(c) = try_content() else { return u32::MAX };
    let Some(&id) = c.game.event_names.get(name) else { return u32::MAX };
    let def = &c.game.events[id as usize];
    if !schema.is_empty() && def.schema != schema {
        log::error!("event {name} is '{}' but a mod emits '{schema}'", def.schema);
        return u32::MAX;
    }
    id
}

unsafe extern "C" fn emit(w: *mut abi::WorldCtx, event: u32, value: abi::Slice<u8>) {
    unsafe { dim(w).emit(content(), event, value.as_slice()) }
}

unsafe extern "C" fn ask(w: *mut abi::WorldCtx, name: Str, query: *const abi::Query) -> u32 {
    unsafe { dim(w).ask(content(), name.as_str(), &*query) as u32 }
}

unsafe extern "C" fn chain_compute(w: *mut abi::WorldCtx, name: Str, base: f64, ctx: *const abi::Query, explain: u32) -> f64 {
    unsafe {
        let q = if ctx.is_null() { abi::Query::DEFAULT } else { *ctx };
        dim(w).compute(content(), name.as_str(), base, &q, explain != 0)
    }
}

unsafe extern "C" fn explanation() -> Str {
    thread_local! {
        static TEXT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }
    TEXT.with(|t| {
        let mut t = t.borrow_mut();
        *t = crate::server::explanation();
        Str { ptr: t.as_ptr(), len: t.len() }
    })
}

unsafe extern "C" fn attribute(w: *mut abi::WorldCtx, entity: u64, name: Str, explain: u32) -> f64 {
    unsafe { dim(w).attribute_named(content(), entity, name.as_str(), explain != 0) }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn attribute_add_modifier(
    w: *mut abi::WorldCtx,
    entity: u64,
    name: Str,
    id: Str,
    step: Str,
    phase: Str,
    amount: f64,
    ticks: u64,
) {
    unsafe {
        let c = content();
        let (name, step, phase) = (name.as_str(), step.as_str(), phase.as_str());
        let Some(&a) = c.game.attr_names.get(name) else { return misuse("attribute_add_modifier", || format!("no attribute {name}")) };
        if !c.game.steps.names.contains_key(step) {
            return misuse("attribute_add_modifier", || format!("no step {step}"));
        }
        if !phase.is_empty() && !c.game.steps.phase_names.contains_key(phase) {
            return misuse("attribute_add_modifier", || format!("no phase {phase}"));
        }
        let phase = if phase.is_empty() { 0 } else { crate::registry::name_hash(phase) };
        dim(w).set_attr_modifier(c, entity, a, id.as_str(), crate::registry::name_hash(step), phase, amount, ticks);
    }
}

unsafe extern "C" fn attribute_remove_modifier(w: *mut abi::WorldCtx, entity: u64, name: Str, id: Str) {
    unsafe {
        let c = content();
        match c.game.attr_names.get(name.as_str()) {
            Some(&a) => dim(w).remove_attr_modifier(c, entity, a, id.as_str()),
            None => misuse("attribute_remove", || format!("no attribute {}", name.as_str())),
        }
    }
}

unsafe extern "C" fn reg_entry(r: *mut abi::Registrar, d: *const abi::EntryDesc) {
    unsafe {
        let r = reg(r);
        let e = crate::entries::RawEntry::copy(r.owner, &*d);
        r.game.entries.push(e);
    }
}

unsafe extern "C" fn mod_loaded(id: Str) -> u32 {
    unsafe { crate::modloader::mod_loaded(id.as_str()) as u32 }
}

unsafe extern "C" fn holds(w: *mut abi::WorldCtx, text: Str, pos: abi::Vec3, entity: u64) -> u32 {
    unsafe {
        let c = content();
        let ask = crate::conditions::Ask { pos: crate::world::dvec(pos), entity, ..Default::default() };
        c.condition(text.as_str())
            .is_some_and(|e| crate::conditions::holds_server(dim(w), c, &e, &ask)) as u32
    }
}

unsafe extern "C" fn roll_loot(w: *mut abi::WorldCtx, name: Str, ctx: *const abi::LootCtx, out: *mut ItemStack, max: usize) -> usize {
    unsafe {
        let x = if ctx.is_null() { abi::LootCtx::DEFAULT } else { *ctx };
        let ctx = crate::loot::Ctx { tool: x.tool, by_player: x.by_player != 0, on_fire: x.on_fire != 0, mined: x.mined, state: x.mined };
        let stacks = dim(w).roll_loot(content(), name.as_str(), &ctx, DVec3::ZERO, 0);
        for (i, s) in stacks.iter().take(max).enumerate() {
            *out.add(i) = *s;
        }
        stacks.len()
    }
}

unsafe extern "C" fn break_block(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, tool: ItemStack, player: u64, drops: u32) -> u32 {
    unsafe { dim(w).break_block(content(), ip(pos), tool, player, drops != 0) as u32 }
}

unsafe extern "C" fn set_entity_size(w: *mut abi::WorldCtx, id: u64, size: abi::EntitySize) {
    unsafe {
        let d = dim(w);
        if let Some(e) = d.entities.get_mut(id) {
            let height = size.height.max(0.0);
            e.size = (size.width > 0.0).then_some(abi::EntitySize { width: size.width, height, eye: size.eye.clamp(0.0, height) });
        }
    }
}

unsafe extern "C" fn set_entity_appearance(w: *mut abi::WorldCtx, id: u64, kind: Str) {
    unsafe {
        let name = kind.as_str();
        let look = match name {
            "" => None,
            n => match content().game.entity_names.get(n) {
                Some(&k) => Some(k),
                None => return misuse("set_entity_look", || format!("no entity kind {n}")),
            },
        };
        let d = dim(w);
        if let Some(e) = d.entities.get_mut(id) {
            e.look = look;
        }
    }
}

unsafe extern "C" fn entity_kind_size(kind: Str, out: *mut abi::EntitySize) -> u32 {
    unsafe {
        let g = &content().game;
        let Some(&k) = g.entity_names.get(kind.as_str()) else { return 0 };
        let e = &g.entities[k as usize];
        *out = e.size();
        1
    }
}

unsafe extern "C" fn set_weather(w: *mut abi::WorldCtx, weather: u32, ticks: u32) {
    unsafe {
        let d = dim(w);
        if weather as usize >= content().game.weathers.len() {
            misuse("set_weather", || format!("no weather {weather}"));
            return;
        }
        d.events
            .push(crate::server::DimEvent::SetWeather { dim: d.id, weather: weather as u16, ticks });
    }
}

unsafe extern "C" fn weather_now(w: *mut abi::WorldCtx) -> u32 {
    unsafe { dim(w).weather as u32 }
}

unsafe extern "C" fn entity_effect(w: *mut abi::WorldCtx, id: u64, effect: u32, level: u32, ticks: u32) {
    let c = content();
    if effect as usize >= c.game.entity_effects.len() {
        misuse("entity_effect", || format!("no entity effect {effect}"));
        return;
    }
    unsafe { dim(w).give_effect(c, id, effect, level, ticks) }
}

unsafe extern "C" fn entity_effects(w: *mut abi::WorldCtx, id: u64, out: *mut abi::EffectOn, cap: u32) -> u32 {
    unsafe {
        let d = dim(w);
        let comp = content().game.fields.effect;
        let list: Vec<abi::EffectOn> = crate::world::effects_on(d.entities.comps.get(&(id, comp)).map(|b| &b[..]), d.time)
            .map(|f| abi::EffectOn {
                effect: f.id,
                level: f.level,
                ticks_left: if f.until == crate::world::FOREVER {
                    abi::INFINITE_TICKS
                } else {
                    f.until
                        .saturating_sub(d.time)
                        .min(abi::INFINITE_TICKS as u64 - 1) as u32
                },
            })
            .collect();
        for (i, e) in list.iter().take(cap as usize).enumerate() {
            *out.add(i) = *e;
        }
        list.len() as u32
    }
}

unsafe extern "C" fn entity_set_pose(w: *mut abi::WorldCtx, id: u64, pose: u32, yaw: f32) {
    unsafe {
        let d = dim(w);
        let c = content();
        let comp = c.game.fields.forced;
        let Some(e) = d.entities.get_mut(id) else { return };
        let held = (pose as usize) < c.game.poses.len();
        e.facing = (held && !yaw.is_nan()).then_some(yaw);
        let bytes: Vec<u8> = if held {
            e.pose = crate::world::Pose(pose as u16);
            if let Some(y) = e.facing {
                e.yaw = y;
            }
            [pose.to_le_bytes(), yaw.to_le_bytes()].concat()
        } else {
            Vec::new()
        };
        d.entities
            .comps
            .insert((id, comp), bytes.into_boxed_slice());
    }
}

unsafe extern "C" fn reg_command_permission(r: *mut abi::Registrar, command: Str, level: u32) {
    unsafe {
        reg(r)
            .game
            .command_permissions
            .push((command.as_str().to_string(), level));
    }
}

unsafe extern "C" fn player_permission(w: *mut abi::WorldCtx, player: u64) -> u32 {
    unsafe { dim(w).entities.get(player).map_or(0, |e| e.permission) }
}

unsafe extern "C" fn set_player_permission(w: *mut abi::WorldCtx, player: u64, level: u32) {
    unsafe {
        dim(w)
            .events
            .push(crate::server::DimEvent::SetPermission { player, level: level.min(abi::permission::OWNER) });
    }
}

unsafe extern "C" fn reg_command_branch(r: *mut abi::Registrar, command: Str, usage: Str, client: u32, cb: Callback) {
    unsafe {
        let r = reg(r);
        let ext = registry::CommandExtension::Branch {
            command: command.as_str().to_string(),
            usage: usage.as_str().to_string(),
            client: client != 0,
            cb,
        };
        let owner = r.owner;
        r.game.command_extensions.push((owner, ext));
    }
}

unsafe extern "C" fn reg_system(r: *mut abi::Registrar, name: Str, schema: Str, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .systems
            .push((owner, name.as_str().to_string(), schema.as_str().to_string(), cb));
    }
}

unsafe extern "C" fn reg_client_frame(r: *mut abi::Registrar, cb: Callback) {
    unsafe {
        let r = reg(r);
        r.game.client_frames.push((r.owner, cb));
    }
}

unsafe extern "C" fn reg_packet(r: *mut abi::Registrar, name: Str, schema: Str, side: u32, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .packets
            .push((owner, name.as_str().to_string(), schema.as_str().to_string(), side.min(1), cb));
    }
}

unsafe extern "C" fn reg_mistake(r: *mut abi::Registrar, message: Str) {
    unsafe { reg(r).errors.push(message.as_str().to_string()) }
}

unsafe extern "C" fn particle_burst(w: *mut abi::WorldCtx, who: abi::Who, b: *const abi::ParticleBurst) {
    unsafe {
        let mut b = *b;
        if (b.kind as usize) < content().game.particles.len() {
            let d = dim(w);
            b.pos = d
                .place
                .to_world(crate::world::dvec(b.pos))
                .to_array()
                .into();
            d.events
                .push(crate::server::DimEvent::Particles { dim: d.id, who, burst: b });
        } else {
            misuse("particles", || format!("no particle {}", b.kind));
        }
    }
}

unsafe extern "C" fn signal_at(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u32 {
    unsafe {
        let d = dim(w);
        d.space
            .signals
            .power_at(content(), d.signal_view(), crate::world::ivec(*pos)) as u32
    }
}

unsafe extern "C" fn config_value(id: u32) -> f64 {
    crate::config::get(id)
}

unsafe extern "C" fn config_set(id: u32, value: f64) {
    crate::config::set(id, value);
}

unsafe extern "C" fn config_text(id: u32) -> Str {
    thread_local! {
        static TEXT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }
    TEXT.with(|t| {
        *t.borrow_mut() = crate::config::text(id);
        let t = t.borrow();
        Str { ptr: t.as_ptr(), len: t.len() }
    })
}

unsafe extern "C" fn config_set_text(id: u32, value: Str) {
    unsafe { crate::config::set_text(id, value.as_str()) }
}
