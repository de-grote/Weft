//! Beds: two blocks long, the foot where it is placed and the head away from the player. Using
//! one sets the player's respawn point there (kept through death); at night the player lies
//! down, and once every player of the dimension is asleep the night is skipped to the morning.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::Instant;

use weft_sdk::*;

pub use base_api::{BED_SPAWN, BedSpawn};

pub static BED: Block = Block::named("bed");
static BED_ITEM: Item = Item::named("bed");

value! {
    /// Lying in the bed at `bed` (its foot) since world time `since`, during night number `night`
    /// of its dimension's `Nights`; `asleep` 0 when not. Not saved: a player who leaves wakes up.
    struct Sleep { bed: [i32; 3], since: u64, night: u64, asleep: u32 }
}
static SLEEP: Component<Sleep> = Component::named("sleep");

value! {
    /// Sent to their bed after dying (`on_way` 1), from `from` in dimension `dim`: where a bed far
    /// away stands can only be seen once there, and without one they go back.
    struct Arriving { from: [f64; 3], dim: u32, on_way: u32 }
}
static ARRIVING: Component<Arriving> = Component::named("bed_arriving");

/// Below this daylight it is night enough to sleep (dusk to dawn).
const NIGHT: f32 = 0.2;
/// Ticks every player has to lie in bed before the night is skipped.
const FALL_ASLEEP: u64 = 100;
/// Monsters this close keep the player awake, across and up or down (vanilla's box).
const MONSTERS: [f64; 2] = [8.0, 5.0];
/// Everything that counts as a monster, a mod's too.
static HOSTILE: Tag = Tag::named("hostile");

/// What the regions of a dimension know together, by its ticks: they all run them, whatever each
/// one takes, and each sees only its own players. The night is skipped by whichever finds
/// everybody asleep.
#[derive(Default)]
struct Nights {
    /// Players some region saw lately: when, and whether they have been asleep long enough.
    players: HashMap<u64, (u64, bool)>,
    /// Nights skipped so far: sleepers of an earlier night wake up.
    skipped: u64,
    skipped_at: u64,
}
/// By dimension, for one opening of a world (`World::session`): another starts over.
static NIGHTS: Mutex<(u64, BTreeMap<u32, Nights>)> = Mutex::new((0, BTreeMap::new()));

/// `f` on this world's nights.
fn nights<T>(w: &World, f: impl FnOnce(&mut BTreeMap<u32, Nights>) -> T) -> T {
    let mut all = NIGHTS.lock().unwrap();
    if all.0 != w.session() {
        *all = (w.session(), BTreeMap::new());
    }
    f(&mut all.1)
}

/// Server to client: lying down in bed (1) or getting up (0), for the sleeping screen.
static LYING: Channel<u32> = Channel::named("bed_lying");
/// Client to server: the player got up (left the sleep screen).
static GET_UP: Channel<()> = Channel::named("bed_get_up");

const SPAWN_SET: &str = "bed.base.spawn_set";
const NOT_NIGHT: &str = "bed.base.not_night";
const MONSTERS_NEAR: &str = "bed.base.monsters";
const OCCUPIED: &str = "bed.base.occupied";
const MISSING: &str = "bed.base.missing";
const OBSTRUCTED: &str = "bed.base.obstructed";

/// The head of the bed with its foot at `foot`: away from where the bed faces (the player who
/// placed it).
fn head_of(foot: [i32; 3], state: u32) -> [i32; 3] {
    let d = state_facing(state).opposite().offset();
    [foot[0] + d.x, foot[1] + d.y, foot[2] + d.z]
}

/// Where a player lying on the bed is: over the middle of it, the body's middle on the mattress.
fn lying_on_bed(foot: [i32; 3], state: u32) -> [f64; 3] {
    let head = head_of(foot, state);
    [(foot[0] + head[0]) as f64 / 2.0 + 0.5, foot[1] as f64 + 0.39, (foot[2] + head[2]) as f64 / 2.0 + 0.5]
}

/// The yaw looking from the head of the bed to its foot, as quarter turns.
fn lying_quarters(state: u32) -> u32 {
    match state_prop(state, "facing").and_then(Dir::from_name) {
        Some(Dir::West) => 1,
        Some(Dir::South) => 2,
        Some(Dir::East) => 3,
        _ => 0,
    }
}

/// Where a player stands on the bed with its foot at `foot`.
fn on_bed(foot: [i32; 3]) -> [f64; 3] {
    [foot[0] as f64 + 0.5, foot[1] as f64 + 0.5625, foot[2] as f64 + 0.5]
}

/// No face: transparent pixels of a side picture, where the part of a box meets another.
const NONE: (&str, [f32; 4]) = ("bed_foot_east", [0.0, 0.0, 1.0, 1.0]);
const BODY_UV: [f32; 4] = [0.0, 7.0, 16.0, 13.0];
const BOTTOM: (&str, [f32; 4]) = ("planks_oak", [0.0, 0.0, 16.0, 16.0]);
/// Replaced by the state's picture of the top, turned to where it faces.
const UP: (&str, [f32; 4]) = ("", [0.0, 0.0, 16.0, 16.0]);

/// A box of a bed facing north: from, to, faces in -X, +X, -Y, +Y, -Z, +Z order. The texels are
/// vanilla's layout, so that a pack's pictures fit: mattress in the middle of a side, legs below.
type BedBox = ([f32; 3], [f32; 3], [(&'static str, [f32; 4]); 6]);
const FOOT_BOXES: &[BedBox] = &[
    (
        [0.0, 3.0, 0.0],
        [16.0, 9.0, 16.0],
        [("bed_foot_west", BODY_UV), ("bed_foot_east", BODY_UV), BOTTOM, UP, NONE, ("bed_foot_south", BODY_UV)],
    ),
    (
        [0.0, 0.0, 13.0],
        [3.0, 3.0, 16.0],
        [
            ("bed_foot_west", [13.0, 13.0, 16.0, 16.0]),
            ("bed_foot_south", [3.0, 13.0, 6.0, 16.0]),
            ("bed_foot_west", [7.0, 13.0, 10.0, 16.0]),
            NONE,
            ("bed_foot_west", [10.0, 13.0, 13.0, 16.0]),
            ("bed_foot_south", [0.0, 13.0, 3.0, 16.0]),
        ],
    ),
    (
        [13.0, 0.0, 13.0],
        [16.0, 3.0, 16.0],
        [
            ("bed_foot_south", [10.0, 13.0, 13.0, 16.0]),
            ("bed_foot_east", [0.0, 13.0, 3.0, 16.0]),
            ("bed_foot_east", [6.0, 13.0, 9.0, 16.0]),
            NONE,
            ("bed_foot_east", [3.0, 13.0, 6.0, 16.0]),
            ("bed_foot_south", [13.0, 13.0, 16.0, 16.0]),
        ],
    ),
];
const HEAD_BOXES: &[BedBox] = &[
    (
        [0.0, 3.0, 0.0],
        [16.0, 9.0, 16.0],
        [("bed_head_west", BODY_UV), ("bed_head_east", BODY_UV), BOTTOM, UP, ("bed_head_north", BODY_UV), NONE],
    ),
    (
        [0.0, 0.0, 0.0],
        [3.0, 3.0, 3.0],
        [
            ("bed_head_west", [0.0, 13.0, 3.0, 16.0]),
            ("bed_head_north", [10.0, 13.0, 13.0, 16.0]),
            ("bed_head_west", [6.0, 13.0, 9.0, 16.0]),
            NONE,
            ("bed_head_north", [13.0, 13.0, 16.0, 16.0]),
            ("bed_head_west", [3.0, 13.0, 6.0, 16.0]),
        ],
    ),
    (
        [13.0, 0.0, 0.0],
        [16.0, 3.0, 3.0],
        [
            ("bed_head_north", [3.0, 13.0, 6.0, 16.0]),
            ("bed_head_east", [13.0, 13.0, 16.0, 16.0]),
            ("bed_head_east", [7.0, 13.0, 10.0, 16.0]),
            NONE,
            ("bed_head_north", [0.0, 13.0, 3.0, 16.0]),
            ("bed_head_east", [10.0, 13.0, 13.0, 16.0]),
        ],
    ),
];
/// The picture of the top of the foot and the head.
const TOP: [&str; 2] = ["bed_top_foot", "bed_top_head_0"];

pub fn register(r: &mut Registry) {
    r.block(&BED)
        .sound_group("wood")
        .facing(Facings::HORIZONTAL, Toward::Player)
        // One block on two cells, its own the foot, the head away from the player.
        .cells(&[[0, 0, -1]])
        .states(|p, s| {
            // What the model file of a bed says for one facing south, turned to this one; the head
            // is the next cell north.
            let mut s = s
                .cutout()
                .orientation(Orientation::new(Dir::Down, p.facing().opposite()))
                .hitbox([0.0, 0.0, -16.0], [16.0, 9.0, 16.0]);
            for (head, boxes) in [(false, FOOT_BOXES), (true, HEAD_BOXES)] {
                let z = if head { -16.0 } else { 0.0 };
                for b in boxes {
                    let mut faces = b.2;
                    faces[3].0 = TOP[head as usize];
                    s = s.model_box_faces([b.0[0], b.0[1], b.0[2] + z], [b.1[0], b.1[1], b.1[2] + z], faces);
                }
            }
            s
        })
        .hardness(0.2)
        .mined_by("axe", tool_tier::HAND)
        .drops(BED_ITEM.name(), 1)
        .set(&crate::items::FUEL, 300)
        .on_use(use_bed)
        .tab(&crate::tabs::FUNCTIONAL);
    r.entity_component(
        &BED_SPAWN,
        BedSpawn { bed: [0; 3], set: 0, dim_key: 0 },
        component_flags::SAVE | component_flags::KEEP_ON_RESPAWN | component_flags::SYNC_OWNER,
    );
    r.edit(&PLAYER).attach(&BED_SPAWN);
    r.entity_component(&SLEEP, Sleep { bed: [0; 3], since: 0, night: 0, asleep: 0 }, 0);
    r.edit(&PLAYER).attach(&SLEEP);
    r.entity_component(&ARRIVING, Arriving { from: [0.0; 3], dim: 0, on_way: 0 }, 0);
    r.edit(&PLAYER).attach(&ARRIVING);
    r.on_event(&PLAYER_RESPAWNED, |b| {
        let w = b.world();
        for e in b.iter() {
            let Some(spawn) = w.component_mut(e.player, &BED_SPAWN).filter(|s| s.set != 0) else { continue };
            let (Some(st), Some(arriving)) = (w.entity(e.player), w.component_mut(e.player, &ARRIVING)) else { continue };
            // Its mod may be gone by now.
            let Some(dim) = dimension_from_key(spawn.dim_key).map(|d| name_of(abi::entry_kind::DIMENSION, d)) else {
                spawn.set = 0;
                w.hint(MISSING, &[]).to(e.player);
                continue;
            };
            *arriving = Arriving { from: st.pos.to_array(), dim: w.dimension(), on_way: 1 };
            w.teleport(e.player, Some(Dimension::generated(dim.to_string())), on_bed(spawn.bed));
        }
    });
    r.on_event(&actions::HURT, |b| {
        let w = b.world();
        for e in b.iter() {
            get_up(&w, e.entity);
        }
    });
    r.world_tick(watch_sleepers);
    r.packet(&GET_UP, |p, ()| get_up(&p.world(), p.player()));

    client(r);
}

fn use_bed(u: &mut Use) -> bool {
    let w = u.world();
    let player = u.player();
    let foot = u.pos().to_array();
    // Vanilla's order: nothing is set on a bed that can't be used at all.
    if w.players().into_iter().any(|o| {
        o != player
            && w.component(o, &SLEEP)
                .is_some_and(|s| s.asleep != 0 && s.bed == foot)
    }) {
        w.hint(OCCUPIED, &[]).to(player);
        return true;
    }
    // A player stays standing in bed, so as much room as that takes.
    if [1, 2]
        .iter()
        .any(|dy| w.block_flags([foot[0], foot[1] + dy, foot[2]]) & abi::block_flags::SOLID != 0)
    {
        w.hint(OBSTRUCTED, &[]).to(player);
        return true;
    }
    let dim = w.dimension();
    let dim_key = dimension_key(dim);
    if let Some(spawn) = w.component_mut(player, &BED_SPAWN) {
        if spawn.set == 0 || spawn.bed != foot || spawn.dim_key != dim_key {
            *spawn = BedSpawn { bed: foot, set: 1, dim_key };
            w.hint(SPAWN_SET, &[]).to(player);
        }
    }
    if w.daylight() >= NIGHT {
        w.hint(NOT_NIGHT, &[]).to(player);
    } else if monsters_near(&w, foot) {
        w.hint(MONSTERS_NEAR, &[]).to(player);
    } else if let Some(s) = w.component_mut(player, &SLEEP) {
        let night = nights(&w, |n| n.get(&dim).map_or(0, |n| n.skipped));
        *s = Sleep { bed: foot, since: w.game_time(), night, asleep: 1 };
        let state = w.block(foot);
        w.teleport(player, None, lying_on_bed(foot, state));
        w.set_pose(player, Some(&poses::SLEEP), Some(lying_quarters(state) as f32 * 90.0));
        w.send(player, &LYING, &1);
    }
    true
}

fn monsters_near(w: &World, foot: [i32; 3]) -> bool {
    let c = [foot[0] as f64 + 0.5, foot[1] as f64 + 0.5, foot[2] as f64 + 0.5];
    let reach = (2.0 * MONSTERS[0] * MONSTERS[0] + MONSTERS[1] * MONSTERS[1]).sqrt();
    w.targets_near(c, reach, None)
        .into_iter()
        .filter_map(|id| w.entity(id))
        .any(|e| {
            (e.pos.x - c[0]).abs() <= MONSTERS[0]
                && (e.pos.z - c[2]).abs() <= MONSTERS[0]
                && (e.pos.y - c[1]).abs() <= MONSTERS[1]
                && HOSTILE.on_entity(e.kind)
        })
}

/// A player sent to their bed, once where it should be is loaded: without a bed, back they go.
fn arrive(w: &World, id: u64) {
    let (Some(a), Some(spawn)) = (w.component_mut(id, &ARRIVING).filter(|a| a.on_way != 0), w.component_mut(id, &BED_SPAWN)) else {
        return;
    };
    if dimension_key(w.dimension()) != spawn.dim_key {
        return;
    }
    let Some(state) = w.block_loaded(spawn.bed) else { return };
    a.on_way = 0;
    if !BED.is(state) {
        spawn.set = 0;
        w.hint(MISSING, &[]).to(id);
        w.teleport(id, Some(Dimension::generated(name_of(abi::entry_kind::DIMENSION, a.dim).to_string())), a.from);
    }
}

fn get_up(w: &World, player: u64) {
    if let Some(s) = w.component_mut(player, &SLEEP).filter(|s| s.asleep != 0) {
        s.asleep = 0;
        w.set_pose(player, None, None);
        w.send(player, &LYING, &0);
        if BED.is(w.block(s.bed)) {
            w.teleport(player, None, on_bed(s.bed));
        }
    }
}

/// Wakes sleepers whose night was skipped or who left their bed, notes who is asleep, and skips
/// the night once everybody has been asleep a while. Spectators and the dead aren't asked.
fn watch_sleepers(w: &World) {
    let (now, dim) = (w.game_time(), w.dimension());
    let night = nights(w, |n| n.get(&dim).map_or(0, |n| n.skipped));
    let mut here_asleep = false;
    let mut seen = Vec::new();
    for id in w.players() {
        arrive(w, id);
        let Some(s) = w.component(id, &SLEEP) else { continue };
        let mut ready = false;
        if s.asleep != 0 {
            let left = w.entity(id).is_none_or(|e| {
                let d = [e.pos.x - (s.bed[0] as f64 + 0.5), e.pos.z - (s.bed[2] as f64 + 0.5)];
                d[0] * d[0] + d[1] * d[1] > 2.5 * 2.5
            });
            if s.night < night || left || !BED.is(w.block(s.bed)) {
                get_up(w, id);
            } else {
                ready = w.game_time().saturating_sub(s.since) >= FALL_ASLEEP;
                here_asleep |= ready;
            }
        }
        if w.entity(id)
            .is_some_and(|e| w.targets_near(e.pos, 0.1, None).contains(&id))
        {
            seen.push((id, ready));
        }
    }
    let skips = nights(w, |all| {
        let nights = all.entry(dim).or_default();
        for (id, ready) in seen {
            nights.players.insert(id, (now, ready));
        }
        // Not seen by any region for a couple of ticks: not here.
        nights.players.retain(|_, (at, _)| at.abs_diff(now) <= 2);
        let skips = here_asleep && nights.players.values().all(|&(_, ready)| ready) && nights.skipped_at.abs_diff(now) >= FALL_ASLEEP;
        if skips {
            nights.skipped_at = now;
            nights.skipped += 1;
        }
        skips
    });
    if !skips {
        return;
    }
    // To the next morning; ticks run on as before.
    w.set_day_time((w.day_time() / 24000 + 1) * 24000);
}

static SLEEP_SCREEN: Menu = Menu::named("sleeping");
/// Darkens the picture while lying in bed (`shaders/post/sleep.wgsl`).
static SLEEP_EFFECT: Shader = Shader::named("sleep");

/// The client's side: since when it lies in bed.
static LYING_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

fn client(r: &mut Registry) {
    r.client_packet(&LYING, |c, lying| {
        if lying != 0 {
            *LYING_SINCE.lock().unwrap() = Some(Instant::now());
            c.menu_open(&SLEEP_SCREEN);
        } else if LYING_SINCE.lock().unwrap().take().is_some() {
            c.post_effect(&SLEEP_EFFECT, 0.0);
            if c.screen() == SLEEP_SCREEN.name() {
                c.menu_close();
            }
        }
    });
    // Leaving the screen (Escape, the button) gets the player up.
    r.client_frame(|c| {
        let mut lying = LYING_SINCE.lock().unwrap();
        if let Some(since) = *lying {
            if c.screen() != SLEEP_SCREEN.name() {
                *lying = None;
                c.post_effect(&SLEEP_EFFECT, 0.0);
                c.send(&GET_UP, &());
            } else {
                c.post_effect(&SLEEP_EFFECT, (since.elapsed().as_secs_f32() / 2.5).clamp(0.01, 1.0));
            }
        }
    });
    r.menu(&SLEEP_SCREEN)
        .screen(|ui| {
            ui.column(S::column().gap(12.0).center(), |ui| {
                ui.text_styled(
                    S::column()
                        .class("title")
                        .scale(2.0)
                        .no_wrap()
                        .self_align(abi::ui_align::CENTER),
                    "bed.base.sleeping",
                    &[],
                );
                ui.button("bed.base.leave", S::column().min_w(200.0).self_align(abi::ui_align::CENTER), |c| {
                    c.client().menu_close();
                });
            });
        })
        .closes_on_respawn();
}
