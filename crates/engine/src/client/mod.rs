pub mod camera;
pub mod ctm;
pub mod entity_render;
pub mod environment;
pub mod gizmo;
pub mod glow;
pub mod lod;
pub mod looks;
pub mod mesher;
pub mod models;
pub mod modui;
pub mod particles;
mod receive;
pub mod sky;
pub mod terrain;
pub mod terrain_gpu;
pub mod textures;
pub mod weather;
pub mod wires;
pub mod world;
pub mod world_render;

use std::time::Instant;

use crate::gpu_layout::Scene;
use glam::{DVec3, IVec3, Vec3};
use rustc_hash::{FxHashMap, FxHashSet};
use weft_abi::{ItemStack, block_flags as bf};
use weft_render::{Backend, BufferId, CommandList, FrameInfo};

use crate::content::content;
use crate::input::Input;
use crate::net::{ClientConn, ToClient, ToServer};
use crate::physics::{self, Hit};
use crate::ui::text::{Arg, Msg};
use crate::world::{GameMode, Pose};
use entity_render::EntityRenderer;
use textures::GameTextures;
use world::ClientWorld;
use world_render::{Camera, WorldRenderer};

pub struct LocalPlayer {
    pub pos: DVec3,
    pub vel: DVec3,
    /// Position at the previous physics tick.
    pub prev: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    /// Touched the ground at any physics step since the last move sent: a quick re-jump can
    /// leave the ground before the next send, and the server would add up the falls.
    pub landed: bool,
    /// Jumped at any physics step since the last move sent.
    pub jumped: bool,
    pub flying: bool,
    /// Spectator: moves through blocks.
    pub noclip: bool,
    /// Flying speed multiplier, changed with the scroll wheel in spectator mode.
    pub fly_speed: f32,
    pub in_liquid: bool,
    /// On a ladder.
    pub climbing: bool,
    /// Physics steps until the next jump while the key stays held (vanilla's 10 ticks), so a
    /// two-block gap cannot be used to bounce at full speed.
    pub jump_cooldown: u32,
    /// Smoothed eye height (lower while sneaking) and field of view multiplier (wider while
    /// sprinting or flying).
    pub eye: f64,
    pub fov_mul: f32,
    /// The `fov` attribute (a bow drawn), eased the same way.
    pub zoom: f32,
    /// Lower box and eyes. Stays on under a low ceiling until there is room for more.
    pub pose: Pose,
    /// A pose a mod holds it in, where it fits, and with a facing it stays put that way (a bed).
    pub forced: Option<(Pose, Option<f32>)>,
    /// Actually sprinting, after the conditions: moving forward, not sneaking, not wading.
    pub sprinting: bool,
    /// Holding the use button on an item that takes time.
    pub using: bool,
    /// By liquid: whether sprinting swims in it (`comp::SWIMS_IN` of its kind and what it wears).
    pub swims_in: Vec<bool>,
    pub can_sprint: bool,
    /// The solid body it rides.
    pub support: Option<u64>,
    /// Its box standing (width, height, eye): the player kind's, or what a mod gave it.
    pub size: weft_abi::EntitySize,
    /// Riding a body: its id and the position relative to it at the previous and the last
    /// step. Drawn on the body as shown, so a rider and its deck never drift apart.
    pub riding: Option<(u64, DVec3, DVec3)>,
    /// Walking speed and jump multipliers, and what the rest of its attributes say of how it
    /// moves, from the server.
    pub speed_mul: f64,
    pub jump_mul: f64,
    pub moves: physics::Moves,
}

pub struct ClientEntity {
    pub kind: u32,
    /// What it rides and in which seat: it is drawn there.
    pub vehicle: Option<(u64, u8)>,
    pub pos: DVec3,
    pub target: DVec3,
    pub yaw: f32,
    pub target_yaw: f32,
    pub pitch: f32,
    pub target_pitch: f32,
    pub item: ItemStack,
    pub health: f32,
    pub hurt: f32,
    pub worn: Vec<ItemStack>,
    pub held: ItemStack,
    pub off_held: ItemStack,
    /// Into using what it holds, 0..1 (seconds for what is used until let go, `endless_use`),
    /// and whether with the other hand.
    pub using: Option<(f32, bool)>,
    /// Ticks using it takes.
    pub use_ticks: f32,
    /// A projectile stuck in an entity (`Client::hold_stuck`).
    pub stuck: Option<Stuck>,
    /// How far into sneaking, 0..1: the pose fades in and out.
    pub crouched: f32,
    /// Actions a mod has play as another for now: which, as what.
    pub swaps: Vec<(String, String)>,
    /// Its swing is the other hand's.
    pub swing_off: bool,
    /// Accumulated walk distance and current swing amplitude, for limb animation.
    pub walk: f32,
    /// Blocks actually walked, which limbs at full swing fall behind of.
    pub walked: f32,
    pub swing: f32,
    pub age: f32,
    pub burning: bool,
    /// Seconds since it died: it falls over and turns red, then disappears in a puff.
    pub dying: Option<f32>,
    /// Walk distance at which its next step sounds.
    pub next_step: f32,
    /// Seconds until what it mines knocks again.
    pub knock: f32,
    /// Action the server started: name, age it started at, looping.
    pub action: Option<(String, f32, bool)>,
    /// How a player holds itself: its box and how the model stands.
    pub pose: Pose,
    pub sprinting: bool,
    /// Where the body faces as drawn, easing to `body_target`: where it faces by the server's
    /// ticks (`physics::turn_body`), as the server has it.
    pub body_yaw: f32,
    pub body_target: f32,
    /// Its clocks in the server's ticks (`Client::anim_tick`), so every client poses it alike:
    /// the tick it came, its swing, its use and its death started, and its crouching turned
    /// (tick, how far into the pose then, crouching).
    pub born: f64,
    pub swing_at: Option<f64>,
    pub use_at: f64,
    pub died_at: f64,
    pub crouch_at: (f64, f32, bool),
    /// Its walk distance and limb swing by the server's ticks, which `walk` and `swing` ease to.
    pub walk_target: f32,
    pub swing_target: f32,
    /// Arm swing progress 0..1 while swinging.
    pub arm_swing: Option<f32>,
    /// Every effect on it, its level and the tick it ends (`ClientWorld::ticks`).
    pub effects: Vec<crate::world::OnEffect>,
    /// The items its kind shows (`registry::SHOWN`), 0 empty.
    pub shown: Vec<ItemStack>,
    /// Solid bodies: where the server had them at its last ticks (tick, position), oldest
    /// first; they are shown `INTERP_TICKS` behind the server (`pose`).
    pub track: std::collections::VecDeque<(f64, DVec3)>,
    /// A picked up item flying to who took it: where it started, the taker, seconds so far.
    pub pulled: Option<(DVec3, u64, f32)>,
    /// Its own box (width, height, eye) and the kind it looks like, when a mod set them.
    pub size: Option<weft_abi::EntitySize>,
    pub look: Option<u32>,
    /// The solid body it stands on (0: the world), and its position relative to that body:
    /// shown and as the server last had it. Riders are drawn where the body is drawn.
    pub frame: u64,
    pub rel: DVec3,
    pub rel_target: Option<DVec3>,
    /// A moving structure's blocks, in its own coordinates (meshed like the world's).
    pub structure: Option<Box<ClientWorld>>,
    /// The world's light on a moving structure, by its cubes, as the server worked it out.
    pub world_light: rustc_hash::FxHashMap<IVec3, u32>,
}

/// Where a projectile sits in an entity: in its box turned with its body as the server has it,
/// and on the bone of its model the arrow's line went into as this client shows it.
#[derive(Clone, Copy)]
pub struct Stuck {
    pub host: u64,
    pub at: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    /// The model, its bone, and its place and way in that bone's pixels, looked for once the
    /// host is shown at the tick it went in (`seen_at`): every client finds the same bone.
    pub bone: Option<(u32, usize, glam::Vec3, glam::Vec3)>,
    pub looked: bool,
    pub seen_at: f64,
    /// Its place and way in the host's frame as last placed, and the host then (place, body yaw):
    /// last frame's bones were posed there.
    pub placed: Option<(DVec3, DVec3)>,
    pub last: Option<(DVec3, f32)>,
    /// The server is done with it as its host died: it stays for the host's death animation.
    pub leaving: bool,
}

/// A picked up item reaches the taker's feet in three ticks (vanilla's pickup animation).
const PULL_SECS: f32 = 0.15;

/// Seconds an entity flashes red after a hit (vanilla's 10 ticks).
const HURT_FLASH: f32 = 0.5;

impl LocalPlayer {
    /// Its box now (crouching lowers it).
    pub fn dims(&self) -> physics::Dims {
        physics::posed(self.size, self.pose)
    }
}

/// One step on a mount: steering it when the seat says so and the kind knows how
/// (`EntityKind::steer`), then sitting where the seat is.
#[allow(clippy::too_many_arguments)]
fn ride_step(
    entities: &FxHashMap<u64, ClientEntity>,
    player: &mut LocalPlayer,
    drive: &mut Drive,
    conn: &mut ClientConn,
    keys: &crate::input::Keys,
    block: &impl physics::Cells,
    gravity: f64,
    bodies: &[physics::Body],
    mount: u64,
    seat: u8,
) -> Option<(DVec3, f32)> {
    let c = content();
    let m = entities.get(&mount)?;
    let k = &c.game.entities[m.kind as usize];
    let steers = k.seats.get(seat as usize).is_some_and(|s| s.steers);
    let mut moved = None;
    if let (true, Some((owner, cb))) = (steers, k.steer) {
        let d = m.dims();
        let mut args = weft_abi::SteerArgs {
            keys: keys.ctx(),
            yaw: player.yaw,
            pitch: player.pitch,
            mount: weft_abi::EntityState {
                kind: m.kind,
                flags: k.flags,
                pos: m.pos.to_array().into(),
                vel: (drive.vel * crate::world::TPS as f64).to_array().into(),
                yaw: m.yaw,
                pitch: m.pitch,
                on_ground: drive.on_ground as u32,
                size: weft_abi::EntitySize { width: d.half_width as f32 * 2.0, height: d.height as f32, eye: d.eye as f32 },
                ..weft_abi::EntityState::DEFAULT
            },
            out: weft_abi::Steer::DEFAULT,
        };
        crate::host::invoke(owner, cb, &mut args);
        let s = args.out;
        let yaw = if s.yaw.is_finite() { s.yaw } else { m.yaw };
        let tick = crate::world::TICK;
        let target = k.speed as f64 * s.speed.max(0.0) as f64 * tick;
        let wish = crate::world::turned(DVec3::new(s.strafe as f64, 0.0, s.forward as f64), yaw);
        let ctl = physics::Control {
            wish,
            // As a mob walks: this much a tick keeps it at its speed on plain ground.
            ground: target * (1.0 - 0.6 * 0.91),
            air: 0.02,
            liquid: physics::SWIM,
            jump: if s.jump > 0.0 { s.jump as f64 * tick } else { 0.0 },
            swim: k.is(crate::tag_bits::entity::SWIMS),
            ..Default::default()
        };
        let flies = s.flies != 0;
        if flies {
            drive.vel.y = s.rise as f64 * tick;
        }
        let frame = physics::Frame {
            half_width: d.half_width,
            height: d.height,
            gravity: if flies || k.is(crate::tag_bits::entity::NO_GRAVITY) { 0.0 } else { physics::gravity_per_tick(gravity) },
            moves: physics::Moves::default(),
        };
        let mut pos = m.pos;
        let r = physics::walk(c, block, bodies, mount, &mut pos, &mut drive.vel, drive.on_ground, &ctl, &frame);
        drive.on_ground = r.on_ground;
        conn.send(ToServer::MountMove { pos, yaw, vel: drive.vel * crate::world::TPS as f64, on_ground: r.on_ground });
        moved = Some((pos, yaw));
    }
    let (pos, yaw) = moved.unwrap_or((m.pos, m.yaw));
    if let Some(at) = seat_at(c, m.kind, pos, yaw, seat) {
        (player.pos, player.vel, player.on_ground) = (at, DVec3::ZERO, false);
    }
    moved
}

/// The mount the local player steers, as its client moves it.
#[derive(Default)]
struct Drive {
    /// Blocks per tick.
    vel: DVec3,
    on_ground: bool,
}

/// Where a rider's feet go in seat `seat` of a mount of `kind` at `pos` facing `yaw`.
fn seat_at(c: &crate::content::Content, kind: u32, pos: DVec3, yaw: f32, seat: u8) -> Option<DVec3> {
    let s = c.game.entities[kind as usize].seats.get(seat as usize)?;
    Some(pos + crate::world::turned(s.pos, yaw))
}

/// `u32` and `f32` values of an engine field component (`registry::HEALTH` ...).
fn words(bytes: &[u8]) -> impl Iterator<Item = [u8; 4]> + '_ {
    bytes.chunks_exact(4).map(|w| [w[0], w[1], w[2], w[3]])
}

fn parse_swaps(bytes: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

/// A pose comes and goes in about a tenth of a second.
fn fade_pose(now: f32, on: bool, dt: f32) -> f32 {
    (now + if on { dt } else { -dt } / 0.1).clamp(0.0, 1.0)
}

impl ClientEntity {
    pub fn displayed(&self) -> impl Iterator<Item = ItemStack> + '_ {
        [self.held, self.off_held, self.item]
            .into_iter()
            .chain(self.worn.iter().copied())
            .chain(self.shown.iter().copied())
            .filter(|s| !s.is_empty())
    }

    /// The shader effect drawn on it at tick `now`: of several, the latest that `drawn` says has a
    /// shader.
    pub fn shader_effect(&self, now: u64, drawn: impl Fn(u32) -> bool) -> Option<(u32, f32)> {
        self.effects
            .iter()
            .filter(|f| f.until > now && drawn(f.id))
            .last()
            .map(|f| (f.id, 1.0))
    }

    pub fn new(kind: u32, pos: DVec3, yaw: f32, pitch: f32) -> ClientEntity {
        ClientEntity {
            kind,
            pos,
            target: pos,
            yaw,
            target_yaw: yaw,
            body_yaw: yaw,
            body_target: yaw,
            born: 0.0,
            swing_at: None,
            use_at: 0.0,
            died_at: 0.0,
            crouch_at: (0.0, 0.0, false),
            walk_target: 0.0,
            swing_target: 0.0,
            arm_swing: None,
            effects: Vec::new(),
            shown: Vec::new(),
            pitch,
            target_pitch: pitch,
            item: ItemStack::EMPTY,
            // Arrives as a component right after: no hurt flash for the first.
            health: f32::NAN,
            hurt: 0.0,
            worn: Vec::new(),
            held: ItemStack::EMPTY,
            off_held: ItemStack::EMPTY,
            using: None,
            use_ticks: 0.0,
            stuck: None,
            crouched: 0.0,
            swaps: Vec::new(),
            swing_off: false,
            walk: 0.0,
            walked: 0.0,
            swing: 0.0,
            age: 0.0,
            burning: false,
            dying: None,
            vehicle: None,
            next_step: 0.0,
            knock: 0.0,
            action: None,
            pose: Pose::STAND,
            sprinting: false,
            track: Default::default(),
            pulled: None,
            size: None,
            look: None,
            frame: 0,
            rel: DVec3::ZERO,
            rel_target: None,
            structure: None,
            world_light: Default::default(),
        }
    }

    /// Something that can be bumped into or hit, like the server's `Entity::is_body`: what isn't
    /// a thing to hit never gets here, and the dead lie there only to be seen.
    pub fn is_body(&self) -> bool {
        let projectile = content()
            .game
            .entities
            .get(self.kind as usize)
            .is_some_and(|k| k.projectile.is_some());
        self.item.is_empty() && self.dying.is_none() && !projectile
    }

    /// What its kind's client tick gets (`client` filled in by the caller).
    pub fn args(&self, id: u64, dt: f32) -> weft_abi::ClientEntityArgs {
        weft_abi::ClientEntityArgs {
            client: std::ptr::null_mut(),
            entity: id,
            kind: self.kind,
            pos: self.pos.to_array().into(),
            yaw: self.yaw,
            walked: self.walked,
            dying: self.dying.unwrap_or(-1.0),
            age: self.age,
            dt,
        }
    }

    /// Its box, like the server's `Entity::dims`.
    pub fn dims(&self) -> physics::Dims {
        physics::posed(
            self.size
                .unwrap_or(content().game.entities[self.kind as usize].size()),
            self.pose,
        )
    }

    /// How it stands for what hits it (`hitbox`), as drawn.
    pub fn hit_body(&self) -> crate::hitbox::Body {
        crate::hitbox::Body {
            pos: self.pos,
            body_yaw: self.body_yaw,
            yaw: self.yaw,
            pitch: self.pitch,
            walk: self.walk,
            stride: self.swing,
            size: self
                .size
                .unwrap_or(content().game.entities[self.kind as usize].size()),
            scale: self.size.map_or(1.0, |s| {
                (s.height
                    / content().game.entities[self.kind as usize]
                        .size()
                        .height
                        .max(1e-3)) as f64
            }),
            pose: self.pose,
        }
    }

    /// The kind whose model, texture and animations draw it.
    pub fn look(&self) -> u32 {
        self.look.unwrap_or(self.kind)
    }

    /// Where a solid body was at server tick `tick` (fractional): between the two positions
    /// around it; past the newest one it goes on as it last moved for at most a tick (the
    /// server sends bodies every tick, so a stop shows as two equal positions, no overshoot).
    pub fn pose(&self, tick: f64) -> DVec3 {
        let n = self.track.len();
        let Some(&(last_t, last)) = self.track.back() else { return self.target };
        if tick >= last_t {
            return match n {
                1 => last,
                _ => {
                    let (pt, prev) = self.track[n - 2];
                    last + (last - prev) / (last_t - pt).max(1.0) * (tick - last_t).min(1.0)
                }
            };
        }
        let mut before = self.track[0];
        for &(t, p) in &self.track {
            if tick < t {
                return if t > before.0 {
                    before
                        .1
                        .lerp(p, ((tick - before.0) / (t - before.0)).max(0.0))
                } else {
                    p
                };
            }
            before = (t, p);
        }
        last
    }

    /// `pose` for physics, which takes only what the server has said. It runs a step ahead of what
    /// is drawn, past the newest position half the time; going on as last moved from there, it
    /// overshoots every stop, and shoves whoever is next to where the body stops.
    pub fn known(&self, tick: f64) -> DVec3 {
        self.pose(self.track.back().map_or(tick, |t| tick.min(t.0)))
    }
}

pub struct ClientMenu {
    pub kind: u32,
    pub window: u32,
    /// The entity it belongs to, 0 for none.
    pub owner: u64,
    /// The block state it was opened on (0: none) and the own name of what it belongs to.
    pub block: u32,
    pub name: String,
    pub slots: Vec<ItemStack>,
    pub cursor: ItemStack,
}

pub enum ClientEvent {
    OpenMenu,
    CloseMenu,
}

/// Player physics runs at vanilla's tick rate; the camera is interpolated between ticks.
const STEP: f64 = crate::world::TICK;
/// How far behind the newest server tick bodies are shown (Sable, under Create: Aeronautics,
/// uses 1.5): late moves still find two positions to show between.
const INTERP_TICKS: f64 = 1.5;

/// While loading, a frame takes finished terrain and uploads meshes for what is left of this
/// after the rest of the last frame (half each), at least `MIN_BUDGET` each: loading goes as
/// fast as the frame allows, down to about 60 frames a second.
const FRAME_TARGET: f64 = 0.016;
const MIN_BUDGET: f64 = 0.002;
const MAX_BUDGET: f64 = 0.005;

pub struct Client {
    conn: ClientConn,
    pub world: ClientWorld,
    pub render: WorldRenderer,
    pub entity_render: EntityRenderer,
    pub player: LocalPlayer,
    pub joined: bool,
    pub player_id: u64,
    /// The local player as an entity: what the server says of it (health, actions, look) and
    /// what its own animation works out, brought up to date each frame (`sync_me`).
    pub me: ClientEntity,
    /// The mount the player steers: its velocity and footing.
    drive: Drive,
    /// Per action: ticks held, and held before being let go this tick (`input::Keys`).
    pub key_ticks: Vec<(u32, u32)>,
    accum: f64,
    tick_accum: f64,
    /// Seconds of physics steps taken, the clock solid bodies are shown by.
    body_clock: f64,
    /// Client clock (`body_clock`) less server time (ticks x `STEP`): maps the client's clock
    /// onto the server's timeline bodies are shown on. It follows `offset_target` (arrival
    /// times, averaged) by stretching time at most 5%, so that timeline only ever goes on
    /// evenly: following each arrival's jitter would step bodies back and forth.
    tick_offset: Option<f64>,
    offset_target: f64,
    /// Structures whose cubes are uploaded for drawing.
    drawn_frames: FxHashSet<u64>,
    start: Instant,
    /// Ticks counted since the server last said the time (`Scene` ticks).
    since_time: f64,
    /// Ticks a second the server runs at.
    tick_rate: f64,
    /// Particle kind per block state shown around the player (`ambient`), `u32::MAX` none.
    ambient_kinds: Option<Vec<(Vec<u32>, Option<u32>)>>,
    /// The frame's `Scene`, one buffer per frame in flight.
    scene_bufs: Vec<BufferId>,
    pub target: Option<Hit>,
    pub target_entity: Option<u64>,
    pub selected: usize,
    pub disconnected: Option<String>,
    /// Seconds until holding a button repeats its action. A new press never waits.
    break_delay: f32,
    use_delay: f32,
    /// Each action as the server last heard of it (`send_keys`).
    keys_down: Vec<bool>,
    pub view_radius: i32,
    /// Makes the world in view from the seed (`terrain`); the server's cubes go through it.
    pub terrain: terrain::TerrainHandle,
    pub gpu_terrain: terrain_gpu::GpuTerrain,
    /// Distant terrain beyond the view.
    lod: lod::Lod,
    seed: u64,
    /// Resets sent to the terrain thread: batches of an older one are dropped.
    terrain_resets: u64,
    terrain_view: (IVec3, i32),
    /// The rest of a batch from the terrain thread cut at the frame's budget.
    terrain_left: Option<terrain::Batch>,
    /// Time each of taking terrain and uploading meshes may take this frame (`FRAME_TARGET`),
    /// and how long they took in the last one.
    budget: std::time::Duration,
    spent: f64,
    /// The rest of a frame (seconds), smoothed.
    rest: f64,
    /// Finished meshes not uploaded yet (the frame's budget ran out).
    meshes: std::collections::VecDeque<world::MeshResult>,
    pub fov: f32,
    /// Share of the widening by sprinting and flying that shows (`core:fov_effects`).
    pub fov_effects: f32,
    /// Brightness setting, 0 moody .. 1 bright.
    pub brightness: f32,
    pub entities: FxHashMap<u64, ClientEntity>,
    /// Block entities that show items (`registry::SHOWN`): not drawn or hit, only lit by.
    pub showing_blocks: FxHashMap<u64, ClientEntity>,
    pub inventory: Vec<ItemStack>,
    pub max_health: f32,
    pub mode: GameMode,
    /// When jump was last pressed, for double-tap flight toggling.
    /// How sneak, sprint and flight are being asked for.
    /// The server ended the sprint; `update` switches the key off.
    sprint_stopped: bool,
    /// 0 first person, 1 third person from behind, 2 from the front.
    pub cameras: camera::Cameras,
    /// F1: no hand and no block outline.
    pub hide_gui: bool,
    /// Show your own body in first person instead of just the hand.
    pub first_person_model: bool,
    /// Arm swing progress 0..1 while swinging.
    hand_swing: Option<f32>,
    /// That swing is the other hand's.
    hand_swing_off: bool,
    using_off: bool,
    /// The stack the hand shows: a new one only comes up after the old one went down.
    hand_stack: ItemStack,
    /// How far the hand is up, 0..1.
    hand_height: f32,
    /// Seconds since the last swing or change of items, and what was in hand then, as the server
    /// judges a hit by.
    swing_since: f32,
    swing_held: ItemStack,
    /// This player's attributes, by id, as the server says.
    /// The night vision attribute, eased over half a second (`Scene::light`).
    night_vision: f32,
    pub(crate) attributes: Vec<f32>,
    /// What the server refused to have mined; the button let go or another block lifts it.
    refused_break: Option<(u64, IVec3)>,
    /// Where the hand's view is, easing after the camera: pitch, yaw.
    hand_view: (f32, f32),
    /// Vanilla's bobbing state: distance walked (x0.6) and the smoothed amplitude.
    walk_dist: f32,
    bob: f32,
    last_smooth: DVec3,
    /// What that was relative to: the body it rode, if any.
    last_frame: Option<u64>,
    /// How long the move since `last_smooth` took.
    moved_over: f32,
    /// Held item and when it became held: its name shows above the hotbar for a moment.
    pub held_name: (ItemStack, Instant),
    /// Names of the players online, for completion.
    pub players: Vec<String>,
    /// Lines this player sent, oldest first (chat history).
    pub sent: Vec<String>,
    /// Mouse button and slots of a drag in progress in a menu.
    pub drag: Option<(u8, Vec<u32>)>,
    /// Slots shift-clicked by sweeping over them with shift and the left button held.
    pub sweep: Vec<u32>,
    /// Slots swept over with the left button still held after picking a stack up: what they
    /// have of it joins the cursor.
    pub picking: Vec<u32>,
    /// The last slot pressed and when, for double clicks.
    pub last_click: Option<(u32, u8, Instant)>,
    pub particles: particles::Particles,
    pub wires: wires::Wires,
    pub weather_render: weather::Weather,
    pub sky: sky::Sky,
    /// Scratch: taken items and where they fly to this frame.
    pulls: Vec<(u64, DVec3)>,
    /// Scratch: solid bodies and where they are drawn this frame.
    frames: Vec<(u64, DVec3)>,
    /// Commands recorded early but drawn after the opaque ones: translucent blocks and blended
    /// particles, then cracks, gizmo fills and the hand.
    late: CommandList,
    hand_late: CommandList,
    /// Seconds until the next dust piece from the block being hit.
    hit_dust: f32,
    /// Sounds to play this frame (from the server and from here).
    pub sounds: Vec<crate::audio::Play>,
    rng: weft_noise::Rng,
    pub dead: bool,
    /// The weather the server says it is (`registry::WeatherDef`).
    pub weather_id: u16,
    /// How much of it there is by now, 0..1 over about five seconds.
    /// The weather's look as it is by now, going over to the weather's own (`weather_effects`).
    look: weft_abi::WeatherLook,
    pub environment: environment::Environment,
    frame_dt: f32,
    /// Seconds left of a lightning's flash.
    lightning: f32,
    /// Holding the use button on an item that takes time: ticks held, ticks in all.
    using: Option<(f32, f32)>,
    /// Who else is mining what, and how far along.
    others_mining: Vec<crate::net::Crack>,
    /// What the server was last told this player mines.
    told_mining: Option<(u64, IVec3, u32)>,
    /// What the server says this player may do (`abi::permission`).
    pub permission: u32,
    /// Seconds since it died, while it is.
    dead_for: Option<f32>,
    pub components: FxHashMap<u32, Vec<u8>>,
    pub menu: Option<ClientMenu>,
    pub events: Vec<ClientEvent>,
    /// Mod channel messages waiting for their client handler.
    pub packets: Vec<(u32, Vec<u8>)>,
    /// Components of other entities the server shares.
    pub entity_components: FxHashMap<(u64, u32), Vec<u8>>,
    /// Formatted when drawn, in the language shown then.
    pub chat: Vec<(crate::net::Shown, Instant)>,
    /// Text on the screen other than the chat, some of it possibly over by now.
    pub shown: Vec<(crate::net::Shown, Instant)>,
    /// Goes up with every change to `chat` and `shown`, for the HUD to notice.
    pub shown_changes: u32,
    /// Answers to commands run from code, by their id, until read.
    pub command_replies: rustc_hash::FxHashMap<u32, Vec<Msg>>,
    pub next_command: u32,
    pub gizmos: gizmo::Gizmos,
    /// This frame's gizmo shapes, from `collect_gizmos`.
    pub gizmo_lines: Vec<gizmo::Line>,
    pub gizmo_fills: Vec<gizmo::Fill>,
    pub gizmo_labels: Vec<gizmo::Label>,
    /// The block being broken (frame, block as in `Hit`) and how far.
    pub breaking: Option<(u64, IVec3, f32)>,
    /// Which of what is in that cell.
    breaking_part: u32,
    /// Hotbar contents last frame, so the HUD knows when to redraw.
    pub hotbar: Vec<u32>,
}

/// The player's position, sent at most once a frame ahead of an action: the server judges actions
/// by where the player looks, which it otherwise hears only 20 times a second.
struct LookOnce(Option<ToServer>);

impl LookOnce {
    fn send(&mut self, conn: &mut ClientConn) {
        if let Some(m) = self.0.take() {
            conn.send(m);
        }
    }
}

impl Client {
    pub fn toggle_gizmo(&mut self, group: &str) -> bool {
        let on = self.gizmos.toggle(group);
        self.send(ToServer::Gizmos(self.gizmos.enabled.clone()));
        on
    }

    /// `/gizmo [group]`: lists groups or switches one; mods' groups are `modid:name`.
    pub fn gizmo_command(&mut self, group: &str) -> Vec<Msg> {
        let text = |s: &str| Arg::Text(s.to_string());
        if group.is_empty() {
            let mut out = vec![Msg::new("command.gizmo.groups", Vec::new())];
            out.extend(
                gizmo::BUILTIN
                    .iter()
                    .map(|g| Msg::new("command.gizmo.group", vec![text(g), Arg::Key(format!("gizmo.{g}"))])),
            );
            out.push(match self.gizmos.enabled.is_empty() {
                true => Msg::new("command.gizmo.none_on", Vec::new()),
                false => Msg::new("command.gizmo.on_list", vec![text(&self.gizmos.enabled.join(", "))]),
            });
            return out;
        }
        let on = self.toggle_gizmo(group);
        vec![Msg::new(if on { "command.gizmo.on" } else { "command.gizmo.off" }, vec![text(group)])]
    }

    /// Builds the gizmo lines and labels for this frame; labels behind opaque blocks are left out.
    pub fn collect_gizmos(&mut self) {
        let mut g = std::mem::take(&mut self.gizmos);
        let (lines, fills, mut labels) = g.collect(self);
        self.gizmos = g;
        let c = content();
        let eye = self.render_camera().pos;
        let opaque = |p: IVec3| {
            self.world
                .block_or_air(p)
                .map(|s| if c.states[s as usize].flags & bf::OPAQUE != 0 { s } else { crate::content::AIR })
        };
        labels.retain(|(pos, ..)| {
            let d = *pos - eye;
            let len = d.length();
            len < 24.0 && (len < 0.2 || physics::raycast(c, &opaque, eye, d / len, len - 0.1, weft_abi::ray::BLOCKS).is_none())
        });
        (self.gizmo_lines, self.gizmo_fills, self.gizmo_labels) = (lines, fills, labels);
    }

    /// Shows a line in the chat as if the server had sent it.
    pub fn chat_line(&mut self, line: Msg) {
        self.show(crate::net::Shown::chat(line));
    }

    /// Puts text on the screen. The chat keeps what is over as its history.
    pub fn show(&mut self, s: crate::net::Shown) {
        let now = Instant::now();
        let s = s.settled();
        self.shown_changes += 1;
        let in_chat = s.place == weft_abi::show_place::CHAT && s.at.is_none();
        let list = if in_chat { &mut self.chat } else { &mut self.shown };
        let replaced = (!s.look().stacks || !s.id.is_empty())
            .then(|| {
                list.iter()
                    .position(|(o, _)| o.place == s.place && o.id == s.id)
            })
            .flatten();
        match replaced {
            Some(i) if s.clears() => drop(list.remove(i)),
            Some(i) => list[i] = (s, now),
            None if s.clears() => {}
            None => list.push((s, now)),
        }
        if in_chat {
            let extra = list.len().saturating_sub(100);
            list.drain(..extra);
        } else {
            // Not by alpha: that is 0 too at the start of a fade in.
            list.retain(|(o, at)| now.duration_since(*at).as_secs_f32() < o.times.iter().sum::<f32>());
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        gpu: &mut dyn Backend,
        assets: &crate::assets::Assets,
        textures: &GameTextures,
        mut conn: ClientConn,
        name: &str,
        key: &str,
        view_radius: i32,
        skin: Option<Vec<u8>>,
        avatar: Option<crate::net::AvatarData>,
    ) -> Result<Client, String> {
        let c = content();
        let render = WorldRenderer::new(gpu, c, textures, assets)?;
        let entity_render = EntityRenderer::new(gpu, assets)?;
        let particles = particles::Particles::new(gpu, assets);
        let wires = wires::Wires::new(gpu, assets);
        let weather_render = weather::Weather::new(gpu, assets);
        let sky = sky::Sky::new(gpu, assets);
        conn.send(ToServer::Join { name: name.to_string(), key: key.to_string() });
        conn.send(ToServer::View(view_radius as u32));
        if let Some(png) = skin {
            conn.send(ToServer::Skin(png));
        }
        if let Some(a) = avatar {
            conn.send(ToServer::Avatar(a));
        }
        let (gpu_tx, gpu_rx) = crossbeam_channel::unbounded();
        let gpu_on = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let gpu_terrain = terrain_gpu::GpuTerrain::new(gpu, assets, gpu_rx, gpu_on.clone());
        Ok(Client {
            weather_render,
            frame_dt: 0.0,
            conn,
            world: ClientWorld::new(0),
            render,
            entity_render,
            player: LocalPlayer {
                pos: DVec3::new(0.0, 80.0, 0.0),
                vel: DVec3::ZERO,
                prev: DVec3::new(0.0, 80.0, 0.0),
                yaw: 0.0,
                pitch: 0.0,
                on_ground: false,
                landed: false,
                jumped: false,
                jump_cooldown: 0,
                eye: 1.62,
                fov_mul: 1.0,
                zoom: 1.0,
                pose: Pose::STAND,
                forced: None,
                sprinting: false,
                using: false,
                swims_in: Vec::new(),
                can_sprint: true,
                support: None,
                size: c.game.entities[c.game.player_kind as usize].size(),
                riding: None,
                flying: false,
                noclip: false,
                fly_speed: 1.0,
                in_liquid: false,
                climbing: false,
                speed_mul: 1.0,
                jump_mul: 1.0,
                moves: physics::Moves::default(),
            },
            joined: false,
            player_id: 0,
            sprint_stopped: false,
            me: ClientEntity::new(content().game.player_kind, DVec3::ZERO, 0.0, 0.0),
            drive: Drive::default(),
            key_ticks: Vec::new(),
            accum: 0.0,
            tick_accum: 0.0,
            body_clock: 0.0,
            tick_offset: None,
            offset_target: 0.0,
            drawn_frames: FxHashSet::default(),
            start: Instant::now(),
            since_time: 0.0,
            tick_rate: crate::world::TPS as f64,
            ambient_kinds: None,
            scene_bufs: Vec::new(),
            target: None,
            target_entity: None,
            selected: 0,
            disconnected: None,
            break_delay: 0.0,
            use_delay: 0.0,
            keys_down: Vec::new(),
            view_radius,
            terrain: terrain::spawn(gpu_tx, gpu_on.clone()),
            lod: lod::Lod::new(gpu_terrain.lod_tx.clone()),
            gpu_terrain,
            seed: 0,
            terrain_resets: 0,
            terrain_view: (IVec3::MAX, 0),
            meshes: std::collections::VecDeque::new(),
            terrain_left: None,
            budget: std::time::Duration::from_secs_f64(MIN_BUDGET),
            spent: 0.0,
            rest: 0.0,
            fov: 70.0,
            fov_effects: 1.0,
            brightness: 0.5,
            entities: FxHashMap::default(),
            showing_blocks: FxHashMap::default(),
            inventory: vec![ItemStack::EMPTY; content().game.player_layout.slots as usize],
            max_health: 20.0,
            particles,
            wires,
            sky,
            pulls: Vec::new(),
            frames: Vec::new(),
            late: CommandList::default(),
            hand_late: CommandList::default(),
            hit_dust: 0.0,
            sounds: Vec::new(),
            rng: weft_noise::Rng(0x9E37_79B9_7F4A_7C15),
            drag: None,
            sweep: Vec::new(),
            picking: Vec::new(),
            last_click: None,
            players: Vec::new(),
            sent: Vec::new(),
            held_name: (ItemStack::EMPTY, Instant::now()),
            mode: GameMode::SURVIVAL,
            cameras: camera::Cameras::default(),
            hide_gui: false,
            first_person_model: true,
            hand_swing: None,
            hand_swing_off: false,
            using_off: false,
            hand_stack: ItemStack::default(),
            hand_height: 1.0,
            swing_since: 1000.0,
            swing_held: ItemStack::default(),
            night_vision: 0.0,
            attributes: content()
                .game
                .attributes
                .iter()
                .map(|a| a.default as f32)
                .collect(),
            refused_break: None,
            hand_view: (0.0, 0.0),
            walk_dist: 0.0,
            bob: 0.0,
            last_smooth: DVec3::ZERO,
            last_frame: None,
            moved_over: 0.0,
            dead: false,
            permission: 0,
            weather_id: 0,
            look: Default::default(),
            environment: Default::default(),
            lightning: 0.0,
            using: None,
            others_mining: Vec::new(),
            told_mining: None,
            dead_for: None,
            components: FxHashMap::default(),
            menu: None,
            events: Vec::new(),
            packets: Vec::new(),
            entity_components: FxHashMap::default(),
            chat: Vec::new(),
            shown: Vec::new(),
            shown_changes: 0,
            command_replies: Default::default(),
            next_command: 0,
            gizmos: Default::default(),
            gizmo_lines: Vec::new(),
            gizmo_fills: Vec::new(),
            gizmo_labels: Vec::new(),
            breaking: None,
            breaking_part: 0,
            hotbar: vec![0; 9],
        })
    }

    pub fn send(&mut self, m: ToServer) {
        self.conn.send(m);
    }

    pub fn leave(&mut self) {
        self.conn.send(ToServer::Leave);
    }

    /// The player's health, full until the server has said.
    pub fn health(&self) -> f32 {
        if self.me.health.is_nan() { self.max_health } else { self.me.health }
    }

    pub fn attribute(&self, id: u32) -> f32 {
        self.attributes.get(id as usize).copied().unwrap_or(1.0)
    }

    /// The attribute says it may fly.
    pub fn can_fly(&self) -> bool {
        self.attributes
            .get(crate::registry::attr::CAN_FLY as usize)
            .is_some_and(|&v| v >= 1.0)
    }

    /// How much walking shakes the view now.
    pub fn bob(&self) -> f32 {
        self.bob
    }

    /// How charged the next swing is, 0..1, as the server will judge it.
    pub fn swing_strength(&self) -> f32 {
        crate::content::swing_charge(
            content(),
            self.me.kind,
            &self.swing_held,
            self.swing_since * self.tick_rate as f32,
            self.attribute(crate::registry::attr::ATTACK_SPEED),
        )
    }

    pub fn held(&self) -> ItemStack {
        crate::content::in_main_hand(content(), self.me.kind, &self.inventory, self.selected as u32)
    }

    /// What the main hand shows: from the eyes the old stack, until the hand has gone down with it.
    pub fn shown_held(&self) -> ItemStack {
        if self.camera_has(crate::tag_bits::camera::HAND) { self.hand_stack } else { self.held() }
    }

    pub fn off_held(&self) -> ItemStack {
        crate::content::in_off_hand(content(), self.me.kind, &self.inventory)
    }

    /// Starts the terrain over for dimension `dim` (joined, or moved to another dimension).
    fn reset_terrain(&mut self, dim: u32, fwd: &mut Vec<terrain::In>) {
        self.world.clear();
        self.wires.clear();
        self.meshes.clear();
        self.world.dim = dim;
        self.terrain_resets += 1;
        self.terrain_view = (IVec3::MAX, 0);
        fwd.push(terrain::In::Reset { dim, seed: self.seed });
    }

    /// Distant terrain: tiles shown, made, their quads, asked for and not back.
    pub fn lod_stats(&self) -> (usize, usize, usize, usize) {
        let (made, quads) = self.render.lod_counts();
        (self.render.lod_draw.len(), made, quads, self.lod.pending())
    }

    /// Light around the player made again from nothing, as far as the view reaches.
    pub fn relight(&mut self) {
        let center = crate::world::cube_of(self.player.pos.floor().as_ivec3());
        let _ = self
            .terrain
            .tx
            .send(terrain::In::Relight { center, reach: self.view_radius + 2 });
    }

    /// Meshes still to come or to upload.
    pub fn pending_meshes(&self) -> usize {
        self.world.pending_meshes() + self.meshes.len()
    }

    /// The key that swaps what is in the two hands.
    fn swap_hands(&mut self, input: &Input) {
        if input.captured && !self.mode.has(crate::tag_bits::game_mode::NO_INTERACT) && input.pressed("core:swap_hands") {
            self.conn.send(ToServer::SwapHands);
        }
    }

    pub fn close_menu(&mut self) {
        self.drag = None;
        self.sweep.clear();
        self.picking.clear();
        if self.menu.take().is_some() {
            self.conn.send(ToServer::CloseMenu);
        }
    }

    pub fn update(&mut self, gpu: &mut dyn Backend, textures: &GameTextures, input: &mut Input, dt: f64) {
        // The server ended the sprint (no food, say): a toggle or a double tap has to be pressed again.
        if std::mem::take(&mut self.sprint_stopped) {
            input.stop("core:sprint");
        }
        let m = self.camera_mode();
        use crate::input::context as k;
        input.muted = if m.has(crate::tag_bits::camera::DETACHED) { k::MOVEMENT | k::RIDING } else { 0 }
            | if m.has(crate::tag_bits::camera::NO_INTERACT) { k::HANDS } else { 0 };
        let input = &*input;
        self.take_messages(dt);
        self.read_look(input);
        self.read_controls(input);
        self.fly_speed(input);
        self.select_slot(input);
        self.ease_camera(input, dt);
        let moved = self.animate_player(input, dt);
        self.frame_dt = dt as f32;
        self.weather_effects(dt as f32);
        // Night vision comes and goes over about half a second, not at once.
        let night = self
            .attribute(crate::registry::attr::NIGHT_VISION)
            .clamp(0.0, 1.0);
        self.night_vision += (night - self.night_vision) * (dt as f32 * 4.0).min(1.0);
        self.sync_me();
        let all = input.pressed("core:drop_all");
        if (all || input.pressed("core:drop")) && !self.mode.has(crate::tag_bits::game_mode::NO_INTERACT) {
            self.conn.send(ToServer::Drop { all });
        }
        if input.pressed("core:inventory") && input.captured {
            self.conn.send(ToServer::OpenInventory);
        }
        self.step_physics(input, dt);
        // Where the camera goes by: placed before the physics step, the body would trail it by
        // part of a tick and catch up on each tick.
        let pos = self.smooth_pos();
        (self.me.pos, self.me.target) = (pos, pos);
        self.update_camera(input, dt as f32);
        let look = self.aim(input);
        self.act(input, dt, look);
        self.send_move(dt);
        self.animate_entities(dt);
        self.chip_block(dt);
        self.particles.update(dt as f32);
        // Its steps go by how far it walks on the ground (`entity_effects`).
        if self.player.on_ground && !self.player.flying && !self.mode.has(crate::tag_bits::game_mode::HIDDEN) {
            self.me.walked += moved;
        }
        self.me.hurt = (self.me.hurt - dt as f32).max(0.0);
        self.dead_for = self.dead.then(|| self.dead_for.unwrap_or(0.0) + dt as f32);
        self.hotbar = self.inventory[..content().game.player_layout.hotbar as usize]
            .iter()
            .map(|s| if s.is_empty() { 0 } else { s.item | (s.count.min(255) << 24) })
            .collect();

        self.upload_meshes(gpu, textures);
    }

    fn take_messages(&mut self, dt: f64) {
        // Smoothed: from one frame alone, a quick frame gave the next a long budget, which made
        // it slow, and so on.
        self.rest += (dt - self.spent - self.rest) * 0.1;
        self.budget = std::time::Duration::from_secs_f64(((FRAME_TARGET - self.rest) / 2.0).clamp(MIN_BUDGET, MAX_BUDGET));
        let t = Instant::now();
        self.receive();
        self.spent = t.elapsed().as_secs_f64();
        if let Some(o) = &mut self.tick_offset {
            *o += (self.offset_target - *o).clamp(-0.05 * dt, 0.05 * dt);
        }
    }

    fn read_look(&mut self, input: &Input) {
        if input.captured && input.muted & crate::input::context::MOVEMENT == 0 {
            let sens = 0.0022 * input.sensitivity;
            self.player.yaw -= input.mouse_delta.x * sens;
            let look = input.mouse_delta.y * sens * if input.invert_y { -1.0 } else { 1.0 };
            self.player.pitch = (self.player.pitch - look).clamp(-1.55, 1.55);
        }
    }

    /// Flying goes on and off where the game mode lets it; on a mount the dismount key gets off.
    fn read_controls(&mut self, input: &Input) {
        if self.me.vehicle.is_some() && input.captured && input.pressed("core:dismount") {
            self.conn.send(ToServer::Dismount);
        }
        if self.can_fly() && input.captured && input.pressed("core:fly") {
            self.player.flying = !self.player.flying;
        }
    }

    /// How fast a spectator flies.
    fn fly_speed(&mut self, input: &Input) {
        let by = if input.pressed("core:fly_faster") {
            1.25
        } else if input.pressed("core:fly_slower") {
            0.8
        } else {
            return;
        };
        if input.captured && self.mode.has(crate::tag_bits::game_mode::NO_INTERACT) {
            self.player.fly_speed = (self.player.fly_speed * by).clamp(0.1, 10.0);
        }
    }

    fn select_slot(&mut self, input: &Input) {
        if self.mode.has(crate::tag_bits::game_mode::NO_INTERACT) {
            return;
        }
        let old = self.selected;
        // In a screen the number keys belong to it (swapping slots with the hotbar).
        if input.captured {
            for (i, action) in crate::input::HOTBAR
                .into_iter()
                .enumerate()
                .take(content().game.player_layout.hotbar as usize)
            {
                if input.pressed(action) {
                    self.selected = i;
                }
            }
        }
        let step = input.pressed("core:hotbar_next") as i32 - input.pressed("core:hotbar_prev") as i32;
        if input.captured && step != 0 {
            self.selected = (self.selected as i32 + step).rem_euclid(content().game.player_layout.hotbar.max(1) as i32) as usize;
        }
        if old != self.selected {
            self.conn.send(ToServer::Select(self.selected as u32));
        }
        let held = self
            .inventory
            .get(self.selected)
            .copied()
            .unwrap_or_default();
        if (held.item, held.data) != (self.held_name.0.item, self.held_name.0.data) || old != self.selected {
            self.held_name = (held, Instant::now());
        }
    }

    /// Eye and view widen or drop over a few frames like vanilla, not in one jump.
    fn ease_camera(&mut self, input: &Input, dt: f64) {
        let k = (dt * 12.0).min(1.0);
        let eye_target = self.player.dims().eye;
        self.player.eye += (eye_target - self.player.eye) * k;
        // Running as it does, not the key held: no wider view too hungry to sprint.
        let wider = if self.player.flying { 0.1 } else { 0.0 } + if self.player.sprinting { 0.15 } else { 0.0 };
        let fov_target = 1.0 + wider * self.fov_effects;
        self.player.fov_mul += (fov_target - self.player.fov_mul) * k as f32;
        self.player.zoom += (self.attribute(crate::registry::attr::FOV) - self.player.zoom) * k as f32;
        self.camera_keys(input);
    }

    /// The player's body, the camera's bob and the hand in first person. Returns how far it
    /// walked.
    fn animate_player(&mut self, input: &Input, dt: f64) -> f32 {
        let (moved, speed) = self.animate_walk(dt);
        self.animate_bob(moved, speed, dt);
        self.animate_hand(input, dt);
        moved
    }

    /// Walking (limbs, turning, steps) is motion in the frame of what the player stands on.
    /// Returns how far it walked and how fast.
    fn animate_walk(&mut self, dt: f64) -> (f32, f32) {
        let t = (self.accum / STEP).clamp(0.0, 1.0);
        // Walking is how its place changes on what it stands on, not how that goes.
        let (now, frame) = match self.player.riding {
            Some((b, before, rel)) => (before.lerp(rel, t), Some(b)),
            None => (self.smooth_pos(), None),
        };
        // Getting on or off isn't a step.
        let walked_by = if frame == self.last_frame { now - self.last_smooth } else { DVec3::ZERO };
        let moved = walked_by.with_y(0.0).length().min(2.0) as f32;
        (self.last_smooth, self.last_frame) = (now, frame);
        let k = (dt * 15.0).min(1.0) as f32;
        let body = self.me.body_yaw + angle_diff(self.me.body_target, self.me.body_yaw) * k;
        self.me.body_yaw = self.player.yaw - angle_diff(self.player.yaw, body);
        // That move is the last frame's: physics runs after this. By this frame's time, uneven frames
        // made the speed jump up and down and the bob settle far too low.
        let speed = moved / std::mem::replace(&mut self.moved_over, dt as f32).max(1e-4);
        // Limbs swing with horizontal speed in the air too, like vanilla.
        crate::model::walk_limbs(&mut self.me.walk, &mut self.me.swing, speed, dt as f32);
        self.me.crouched = fade_pose(self.me.crouched, self.player.pose == Pose::CROUCH, dt as f32);
        (moved, speed)
    }

    fn animate_bob(&mut self, moved: f32, speed: f32, dt: f64) {
        // Vanilla's: swimming doesn't bob, even touching the bottom.
        let walking = self.player.on_ground && !self.player.flying && self.player.pose.def().bob && !self.dead;
        // Vanilla: amplitude follows the per-tick speed capped at 0.1, eased by 0.4 per tick.
        self.walk_dist += moved * 0.6;
        let target = if walking { (speed * crate::world::TICK as f32).min(0.1) } else { 0.0 };
        self.bob += (target - self.bob) * (1.0 - 0.6f32.powf(dt as f32 * crate::world::TPS as f32));
        // It eases forever otherwise, and a still picture should be still.
        if target == 0.0 && self.bob < 1e-5 {
            self.bob = 0.0;
        }
    }

    /// Swings, the held item going down and coming up when it changes, and the hand lagging
    /// behind the view.
    fn animate_hand(&mut self, input: &Input, dt: f64) {
        let spectator = self.mode.has(crate::tag_bits::game_mode::NO_INTERACT);
        advance_swing(&mut self.hand_swing, dt as f32);
        self.swap_hands(input);
        // A use swings only when the server says it did something (`ToClient::Swing`).
        if input.captured && !spectator && (input.pressed("core:attack") || (input.held("core:attack") && self.breaking.is_some())) {
            self.swing_arm();
        }
        // Vanilla's 0.4 per tick: the old item goes down, the new one comes up. More of the same
        // item (a count or damage change) is not a change.
        let held = self.held();
        let same = crate::content::same_in_hand(held, self.hand_stack);
        self.swing_since += dt as f32;
        if !crate::content::same_in_hand(held, self.swing_held) {
            self.swing_since = 0.0;
        }
        self.swing_held = held;
        self.player.using = self.using.is_some();
        let step = dt as f32 * 0.4 * crate::world::TPS as f32;
        // The hand dips after a swing and comes back up as it charges (vanilla's strength cubed).
        let up = if same { self.swing_strength().powi(3) } else { 0.0 };
        self.hand_height += (up - self.hand_height).clamp(-step, step);
        if same || self.hand_height < 0.1 {
            self.hand_stack = held;
        }
        let k_lag = 1.0 - 0.5f32.powf(dt as f32 * crate::world::TPS as f32);
        self.hand_view.0 += (self.player.pitch - self.hand_view.0) * k_lag;
        self.hand_view.1 += angle_diff(self.player.yaw, self.hand_view.1) * k_lag;
        // Both ease forever otherwise, and a still picture should be still.
        if (self.player.pitch - self.hand_view.0).abs() < 1e-5 {
            self.hand_view.0 = self.player.pitch;
        }
        if angle_diff(self.player.yaw, self.hand_view.1).abs() < 1e-5 {
            self.hand_view.1 = self.player.yaw;
        }
    }

    fn step_physics(&mut self, input: &Input, dt: f64) {
        let c = content();
        let world = &self.world;
        let block = world::SeenCells { world, air_if_missing: false };
        // Walking waits for the ground under the player to arrive; flying goes on, taking cubes
        // that are not here yet for air (the next ones are usually on their way).
        let flying = self.player.flying || self.player.noclip;
        let ready = self.joined
            && !self.dead
            && (flying
                || world
                    .block_or_air(self.player.pos.floor().as_ivec3())
                    .is_some());
        // The clock bodies are predicted by runs on while the player waits, or they would stop.
        let mut ambient_steps = 0;
        if self.joined {
            let air_if_missing = world::SeenCells { world, air_if_missing: true };
            self.accum += dt.min(0.25);
            while self.accum >= STEP {
                self.accum -= STEP;
                self.since_time = (self.since_time + STEP * self.tick_rate).min(crate::world::TPS as f64);
                ambient_steps += 1;
                // A rider is where it is on its body: if moves arrived late and the body's past
                // changed, the rider moves with it, not over the deck.
                if let Some(e) = self.player.riding.and_then(|(b, _, now)| {
                    self.entities
                        .get(&b)
                        .map(|e| e.known(self.body_tick(self.body_clock)) + now)
                }) {
                    self.player.pos = e;
                }
                self.player.prev = self.player.pos;
                input.count_ticks(&mut self.key_ticks);
                let flying = self.player.flying || self.player.noclip;
                let gravity = c.dims[world.dim as usize].gravity as f64;
                // Solid bodies at this step (predicted like the server moves them) and soft
                // ones nudging the player aside.
                self.body_clock += STEP;
                let at = self.body_tick(self.body_clock);
                // Bodies move by the server's ticks, this step by its own clock.
                let step_ticks = STEP * self.tick_rate;
                let mut bodies = Vec::new();
                let mut push = DVec3::ZERO;
                for (&id, e) in &self.entities {
                    let k = &c.game.entities[e.kind as usize];
                    if !e.is_body() {
                        continue;
                    }
                    if k.is(crate::tag_bits::entity::SOLID) {
                        let (now, delta) = (e.known(at), e.known(at) - e.known(at - step_ticks));
                        let physics::Dims { half_width: hw, height: h, .. } = e.dims();
                        let built = e.structure.as_deref().map(|s| s as &dyn physics::Blocks);
                        bodies.extend(physics::Body::of(id, k, now, (hw, h), delta, built));
                    } else if k.is(crate::tag_bits::entity::PUSHABLE) {
                        let d = self.player.pos - e.pos;
                        let (theirs, mine) = (e.dims(), self.player.dims());
                        let reach = theirs.half_width + mine.half_width;
                        if d.x.abs() < reach && d.z.abs() < reach && d.y < theirs.height && -d.y < mine.height {
                            push += physics::nudge(d);
                        }
                    }
                }
                if !ready {
                    continue;
                }
                // On a mount the seat carries it; in one that steers its keys move the mount.
                if let Some((mount, seat)) = self.me.vehicle {
                    let moved = ride_step(
                        &self.entities,
                        &mut self.player,
                        &mut self.drive,
                        &mut self.conn,
                        &crate::input::Keys { input, ticks: &self.key_ticks, raw: false },
                        &block,
                        gravity,
                        &bodies,
                        mount,
                        seat,
                    );
                    drop(bodies);
                    if let (Some((pos, yaw)), Some(e)) = (moved, self.entities.get_mut(&mount)) {
                        (e.pos, e.target, e.yaw, e.target_yaw) = (pos, pos, yaw, yaw);
                    }
                    self.me.body_target = physics::turn_body(self.me.body_target, self.player.yaw, DVec3::ZERO);
                    continue;
                }
                if !flying {
                    self.player.vel += push;
                }
                if flying {
                    step_player(&mut self.player, input, &air_if_missing, gravity, &bodies);
                } else {
                    step_player(&mut self.player, input, &block, gravity, &bodies);
                }
                let entities = &self.entities;
                let p = &self.player;
                self.player.riding = p.support.and_then(|b| {
                    entities
                        .get(&b)
                        .map(|e| (b, p.prev - e.known(at - step_ticks), p.pos - e.known(at)))
                });
                // The body turns by the tick, as the server turns it from these moves.
                let walked = self
                    .player
                    .riding
                    .map_or(self.player.pos - self.player.prev, |(_, before, now)| now - before);
                self.me.body_target = physics::turn_body(self.me.body_target, self.player.yaw, walked);
                if crate::log::on(crate::log::Channel::Physics, log::Level::Trace) {
                    let p = &self.player;
                    let feet = p.pos.floor().as_ivec3() - glam::IVec3::Y;
                    log::trace!(
                        target: "physics",
                        "pos {:.4} {:.4} {:.4} vel {:.4} {:.4} {:.4} ground {} liquid {} crouch {} fly {} below {}",
                        p.pos.x, p.pos.y, p.pos.z, p.vel.x, p.vel.y, p.vel.z, p.on_ground, p.in_liquid, p.pose == Pose::CROUCH, p.flying,
                        world.block_or_air(feet).map_or("unloaded".to_string(), |s| c.state_name(s))
                    );
                }
            }
        }
        for _ in 0..ambient_steps {
            self.ambient();
        }
    }

    /// What the crosshair is on. Returns where the eye looks.
    fn aim(&mut self, input: &Input) -> DVec3 {
        let c = content();
        let spectator = self.mode.has(crate::tag_bits::game_mode::NO_INTERACT);
        // From the eye as drawn, the moment structures are drawn at too, so a block on a
        // structure the player rides stays the one aimed at.
        let from = if self.camera_has(crate::tag_bits::camera::AIM) { self.cameras.shown } else { self.camera() };
        let (eye, look) = (from.pos, from.forward().as_dvec3());
        use crate::registry::attr::{BLOCK_REACH, ENTITY_REACH};
        let reach = self.attribute(BLOCK_REACH) as f64;
        self.target = if spectator { None } else { self.raycast(eye, look, reach) };
        // Vanilla's: whichever comes first along the look is all that is aimed at.
        let block_dist = self.target.map_or(f64::INFINITY, |h| h.dist);
        self.target_entity =
            if spectator { None } else { self.pick_entity(eye, look, block_dist.min(self.attribute(ENTITY_REACH) as f64)) };
        if self.target_entity.is_some() {
            self.target = None;
        }
        if input.captured && !spectator && input.pressed("core:pick") {
            // In creative a mob gives its egg, there being no other way to get what stands there.
            let egg = || {
                self.target_entity
                    .filter(|_| self.mode.has(crate::tag_bits::game_mode::NO_COSTS))
                    .and_then(|e| c.spawn_egg_of(self.entities.get(&e)?.kind))
            };
            if let Some(item) = self.target.and_then(|h| c.pick_item(h.state)).or_else(egg) {
                let hotbar = &self.inventory[..content().game.player_layout.hotbar as usize];
                // Already in the hotbar: select it. Otherwise creative fills an empty slot first.
                let slot = hotbar
                    .iter()
                    .position(|s| s.item == item && !s.is_empty())
                    .or_else(|| {
                        (self.mode.has(crate::tag_bits::game_mode::NO_COSTS) && !hotbar[self.selected].is_empty())
                            .then(|| hotbar.iter().position(|s| s.is_empty()))
                            .flatten()
                    });
                if let Some(i) = slot.filter(|&i| i != self.selected) {
                    self.selected = i;
                    self.conn.send(ToServer::Select(i as u32));
                }
                if !hotbar.iter().any(|s| s.item == item && !s.is_empty()) {
                    self.conn.send(ToServer::Pick { item });
                }
            }
        }
        look
    }

    fn move_message(&self, landed: bool, jumped: bool) -> ToServer {
        ToServer::Move {
            jumped,
            frame: self.player.riding.map(|(b, _, now)| (b, now)),
            pos: self.player.pos,
            yaw: self.player.yaw,
            pitch: self.player.pitch,
            on_ground: landed || self.player.on_ground || self.player.flying,
            pose: self.player.pose,
            sprint: self.player.sprinting,
            flying: self.player.flying,
        }
    }

    /// The server hears every action go down and up (`event::KEY`), what the world has of the
    /// input: none while a screen has it.
    fn send_keys(&mut self, input: &Input) {
        let keys = &content().game.keys;
        self.keys_down.resize(keys.len(), false);
        let (mut down, mut up) = (Vec::new(), Vec::new());
        let world = input.captured && !self.dead;
        for (i, k) in keys.iter().enumerate() {
            let now = world && input.held(&k.name);
            if now != self.keys_down[i] {
                self.keys_down[i] = now;
                if now { down.push(i as u32) } else { up.push(i as u32) }
            } else if !now && world && input.pressed(&k.name) {
                // Down and up within a frame (a wheel notch, a tap): both still count.
                down.push(i as u32);
                up.push(i as u32);
            }
        }
        if self.joined && !(down.is_empty() && up.is_empty()) {
            self.conn.send(ToServer::Keys { down, up });
        }
    }

    fn act(&mut self, input: &Input, dt: f64, look: DVec3) {
        self.send_keys(input);
        self.break_delay -= dt as f32;
        self.use_delay -= dt as f32;
        // The server judges actions by where the player looks, which it only hears 20 times a
        // second: a turn made just before a click must arrive first.
        let mut send_look = LookOnce(
            self.joined
                .then(|| self.move_message(self.player.landed, false)),
        );
        if !input.captured || self.dead {
            self.breaking = None;
            return;
        }
        self.attack(input, dt, &mut send_look);
        self.use_held(input, dt, look, &mut send_look);
    }

    fn attack(&mut self, input: &Input, dt: f64, send_look: &mut LookOnce) {
        let c = content();
        let target = self.target;
        self.refused_break = self
            .refused_break
            .filter(|&r| input.held("core:attack") && target.is_some_and(|h| (h.frame, h.origin) == r));
        let keeps = (self.mode.has(crate::tag_bits::game_mode::INSTANT_BREAK) && c.no_creative_break(self.held().item))
            || self.mode.has(crate::tag_bits::game_mode::NO_BREAK);
        let refused = keeps || self.refused_break.is_some();
        if input.held("core:attack") {
            if let Some(id) = self.target_entity {
                // A hit is a press, as often as you like; holding hits nothing. How much it
                // does depends on how charged the swing was.
                if input.pressed("core:attack") {
                    send_look.send(&mut self.conn);
                    self.conn.send(ToServer::Attack(id));
                    self.conn.send(ToServer::Swing);
                    self.swing_since = 0.0;
                }
                self.breaking = None;
            } else if refused {
                self.breaking = None;
            } else if let Some(hit) = self.target {
                use crate::registry::attr as a;
                let speed = self.attribute(a::MINING_SPEED)
                    * if self.eye_liquid().is_some() { self.attribute(a::SUBMERGED_MINING_SPEED) } else { 1.0 }
                    * if self.player.on_ground { 1.0 } else { self.attribute(a::AIRBORNE_MINING_SPEED) };
                let rate = c.mining_rate(hit.state, &self.held(), speed, self.mode.has(crate::tag_bits::game_mode::INSTANT_BREAK));
                let done = match self.breaking {
                    Some((f, p, v)) if (f, p) == (hit.frame, hit.origin) && self.breaking_part == hit.state => v,
                    _ => 0.0,
                };
                self.breaking_part = hit.state;
                let progress = if rate >= 1.0 { 1.0 } else { done + dt as f32 * self.tick_rate as f32 * rate };
                if progress >= 1.0 && (input.pressed("core:attack") || self.break_delay <= 0.0) {
                    send_look.send(&mut self.conn);
                    let at = self.hit_point(&hit, hit.origin);
                    self.conn
                        .send(ToServer::Break { pos: hit.origin, part: hit.state, face: hit.face, hit: at, frame: hit.frame });
                    let after = c.states[hit.state as usize]
                        .piece_hit(at, hit.face)
                        .map_or(crate::content::AIR, |p| p.left);
                    match hit.frame {
                        0 => self.world.replace_part(hit.origin, hit.state, after),
                        f => {
                            if let Some(s) = self.entities.get_mut(&f).and_then(|e| e.structure.as_mut()) {
                                s.replace_part(hit.origin, hit.state, after);
                            }
                        }
                    }
                    self.breaking = None;
                    self.break_delay = 0.25;
                } else if rate < 1.0 {
                    self.breaking = Some((hit.frame, hit.origin, progress.min(1.0)));
                } else {
                    // Instant breaks (creative, grass) show no cracks while the next waits.
                    self.breaking = None;
                }
            } else {
                self.breaking = None;
            }
        } else {
            self.breaking = None;
        }
        // Hitting nothing still spends the swing.
        if input.pressed("core:attack") && self.target_entity.is_none() && self.target.is_none() {
            self.conn.send(ToServer::Swing);
            self.swing_since = 0.0;
        }
        // The server times mining too, and shows the cracks to the others.
        let mining = self.breaking.map(|(f, p, _)| (f, p, self.breaking_part));
        if mining != self.told_mining {
            self.told_mining = mining;
            self.conn.send(match mining {
                Some((frame, pos, part)) => ToServer::StartBreak { pos, part, frame },
                None => ToServer::StopBreak,
            });
        }
    }

    /// Where the crosshair is on what it hit, from the corner of `cell`.
    fn hit_point(&self, hit: &crate::physics::Hit, cell: IVec3) -> [f32; 3] {
        let at = self
            .frame_place(hit.frame)
            .to_local(self.camera().pos + self.camera().forward().as_dvec3() * hit.dist);
        (at - cell.as_dvec3()).as_vec3().to_array()
    }

    /// Whether the ground around the player is drawn, columns `reach` cubes about: what a
    /// picture of the world is needed for no longer (the rest comes in from far away, as it does
    /// anyway).
    pub fn near_shown(&self, reach: i32) -> bool {
        let at = crate::world::cube_of(self.player.pos.floor().as_ivec3());
        self.joined
            && (-reach..=reach).all(|x| {
                (-reach..=reach).all(|z| {
                    self.world
                        .column_shown(glam::IVec2::new(at.x + x, at.z + z), self.seed, false)
                })
            })
    }

    /// The held block used on `hit`, shown placed at once where the server will put it
    /// (`Dim::place_clicked`, `Dim::place`), as vanilla's client does: placing doesn't wait for the
    /// server, nor does the player fall into the cell meanwhile. Only what is sure here: a block
    /// alone in its cell, in one cell, held up by nothing, into air or what it replaces, not into
    /// the player. The server sends the cell back as it is either way. Where it went.
    fn predict_place(&mut self, hit: &Hit, at: [f32; 3], look: [f32; 3], sneak: bool) -> Option<IVec3> {
        use crate::content::{AIR, Then, When};
        let c = content();
        let held = self.held();
        let state = c
            .items
            .get(held.item as usize)
            .and_then(|i| i.block)
            .filter(|_| hit.frame == 0 && !held.is_empty() && !self.mode.has(crate::tag_bits::game_mode::NO_PLACE))?;
        // A block that takes the click itself goes first.
        let clicked = c.block_of(hit.state);
        if clicked.usable() && (!sneak || clicked.sneak_use == weft_abi::sneak_use::ALWAYS) {
            return None;
        }
        let replaceable = |s: u32| c.states[s as usize].flags & bf::REPLACEABLE != 0;
        let cell = |p: IVec3| -> Option<Vec<u32>> {
            let main = self.world.block_or_air(p)?;
            Some(
                std::iter::once(main)
                    .chain(self.world.extras(p).iter().copied())
                    .filter(|&s| s != AIR)
                    .collect(),
            )
        };
        let replacing = replaceable(self.world.block(hit.block)) && cell(hit.block)?.iter().all(|&s| replaceable(s));
        let (axis, up) = ((hit.face / 2) as usize, hit.face % 2 == 1);
        let mut point = at;
        point[axis] = if replacing { if up { 1e-3 } else { 1.0 - 1e-3 } } else { at[axis] + if up { 1e-3 } else { -1e-3 } };
        // Into the clicked cell's own room (a slab's other half): the server's to work out.
        if !replacing && (0.0..1.0).contains(&point[axis]) {
            return None;
        }
        let normal = crate::world::DIRS[hit.face as usize];
        let (p, local) =
            if replacing { (hit.block, point) } else { (hit.block + normal, std::array::from_fn(|a| point[a] - normal[a] as f32)) };
        let def = c.block_of(state);
        let mut chosen = def.first_state;
        if let Some((owner, cb)) = def.cb.placement {
            let mut args = weft_abi::PlaceArgs {
                pos: p.to_array().into(),
                face: hit.face as u32,
                look,
                state: def.first_state,
                first: def.first_state,
                hit: local,
                placed: 0,
            };
            crate::host::invoke(owner, cb, &mut args);
            chosen = args.state;
            if !(def.first_state..def.first_state + def.states).contains(&chosen) {
                return None;
            }
        }
        let st = &c.states[chosen as usize];
        let held_up = st
            .rules
            .iter()
            .any(|r| matches!(r.when, When::Neighbor(_)) && matches!(r.then, Then::Break | Then::Shed(_)));
        if !st.cells.is_empty() || held_up {
            return None;
        }
        if cell(p)?
            .iter()
            .any(|&s| !replaceable(s) || c.states[s as usize].liquid.is_some() || c.states[s as usize].block == st.block)
        {
            return None;
        }
        let mut shape = Vec::new();
        physics::boxes(c, chosen, &mut shape);
        let d = self.player.dims();
        let me = physics::Aabb::around(self.player.pos, d.half_width, d.height);
        let at = p.as_dvec3();
        if shape
            .iter()
            .any(|b| physics::Aabb { min: at + DVec3::new(b[0], b[1], b[2]), max: at + DVec3::new(b[3], b[4], b[5]) }.intersects(&me))
        {
            return None;
        }
        self.world.set_block(p, chosen);
        self.world.set_extras(p, Vec::new());
        Some(p)
    }

    fn use_held(&mut self, input: &Input, dt: f64, look: DVec3, send_look: &mut LookOnce) {
        if let Some((held, _)) = &mut self.using {
            *held += dt as f32 * self.tick_rate as f32;
            // The server hears the button go up with the other keys (`send_keys`).
            if !input.held("core:use") {
                self.using = None;
            }
        } else if input.pressed("core:use") || (input.held("core:use") && self.use_delay <= 0.0) {
            send_look.send(&mut self.conn);
            if let Some(id) = self.target_entity {
                self.conn.send(ToServer::Interact(id));
            } else if let Some(hit) = self.target {
                let sneak = input.held("core:sneak");
                let at = self.hit_point(&hit, hit.block);
                let look = look.as_vec3().to_array();
                let placed = self.predict_place(&hit, at, look, sneak);
                self.conn.send(ToServer::Use {
                    pos: hit.block,
                    part: hit.state,
                    face: hit.face,
                    look,
                    hit: at,
                    sneak,
                    frame: hit.frame,
                    placed,
                });
            } else {
                self.conn.send(ToServer::UseItem { target: None });
            }
            self.use_delay = 0.2;
        }
    }

    fn send_move(&mut self, dt: f64) {
        self.tick_accum += dt;
        if self.tick_accum >= STEP {
            // Keeps the fraction, so moves go out 20 times a second at any frame rate.
            self.tick_accum = (self.tick_accum - STEP).min(STEP);
            if self.joined && !self.dead {
                let landed = std::mem::replace(&mut self.player.landed, false);
                let jumped = std::mem::replace(&mut self.player.jumped, false);
                self.conn.send(self.move_message(landed, jumped));
            }
        }
    }

    fn animate_entities(&mut self, dt: f64) {
        let c = content();
        let k = (dt * 15.0).min(1.0);
        // Solid bodies show where the physics has them, one step behind like the player.
        let shown = self.body_clock - STEP + self.accum;
        let mut pulls = std::mem::take(&mut self.pulls);
        pulls.clear();
        pulls.extend(self.entities.iter().filter_map(|(&id, e)| {
            let (_, by, _) = e.pulled?;
            let to = if by == self.player_id { self.smooth_pos() } else { self.entities.get(&by).map_or(e.pos, |t| t.pos) };
            Some((id, to))
        }));
        for &(id, to) in &pulls {
            let Some(e) = self.entities.get_mut(&id) else { continue };
            let Some((from, _, t)) = &mut e.pulled else { continue };
            *t += dt as f32;
            e.pos = from.lerp(to, (*t / PULL_SECS).min(1.0) as f64);
            e.target = e.pos;
            if *t >= PULL_SECS {
                self.entities.remove(&id);
            }
        }
        self.pulls = pulls;
        // Bodies first: riders are drawn relative to where their body is drawn.
        let mut frames = std::mem::take(&mut self.frames);
        frames.clear();
        let at = self.body_tick(shown);
        frames.extend(
            self.entities
                .iter()
                .filter(|(_, e)| c.game.entities[e.kind as usize].is(crate::tag_bits::entity::SOLID))
                .map(|(&id, e)| (id, e.pose(at))),
        );
        // A crowd's worth on all cores: nothing here depends on another entity.
        let mut moving: Vec<&mut ClientEntity> = self
            .entities
            .values_mut()
            .filter(|e| e.pulled.is_none())
            .collect();
        let frames_ref = &frames;
        let step = |e: &mut &mut ClientEntity| {
            let frames = frames_ref;
            let before = e.pos;
            let body = e
                .rel_target
                .and_then(|t| frames.iter().find(|f| f.0 == e.frame).map(|f| (f.1, t)));
            let mut walked = None;
            e.pos = if c.game.entities[e.kind as usize].is(crate::tag_bits::entity::SOLID) {
                e.pose(at)
            } else if let Some((at, rel_target)) = body {
                let rel_before = e.rel;
                e.rel = e.rel.lerp(rel_target, k);
                walked = Some(e.rel - rel_before);
                at + e.rel
            } else {
                e.pos.lerp(e.target, k)
            };
            let mut dyaw = e.target_yaw - e.yaw;
            if dyaw.abs() > std::f32::consts::PI {
                dyaw = (dyaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            }
            e.yaw += dyaw * k as f32;
            e.pitch += (e.target_pitch - e.pitch) * k as f32;
            // A projectile is all one piece pointing where it flies.
            e.body_yaw = if c.game.entities[e.kind as usize].projectile.is_some() {
                e.yaw
            } else {
                let body = e.body_yaw + angle_diff(e.body_target, e.body_yaw) * k as f32;
                e.yaw - angle_diff(e.yaw, body)
            };
            // Clips go by the server's ticks as shown: every client poses it alike at a tick.
            let since = |t: f64| ((at - t).max(0.0) / crate::world::TPS as f64) as f32;
            e.age = since(e.born);
            e.arm_swing = e.swing_at.map(|t| since(t) / 0.3).filter(|&p| p < 1.0);
            let speed = (walked.unwrap_or(e.pos - before).with_y(0.0).length() / dt.max(1e-4)) as f32;
            e.walked += speed * dt as f32;
            e.walk += (e.walk_target - e.walk) * k as f32;
            e.swing += (e.swing_target - e.swing) * k as f32;
            let (turned, from, crouching) = e.crouch_at;
            e.crouched = (from + if crouching { 1.0 } else { -1.0 } * since(turned) / 0.1).clamp(0.0, 1.0);
            if let Some((p, _)) = &mut e.using {
                let secs = since(e.use_at);
                *p = if endless_use(e.use_ticks) { secs } else { (secs * crate::world::TPS as f32 / e.use_ticks.max(1.0)).min(1.0) };
            }
            e.hurt = (e.hurt - dt as f32).max(0.0);
            if let Some(d) = &mut e.dying {
                *d = since(e.died_at);
                e.hurt = HURT_FLASH;
                e.burning = false;
            }
        };
        if moving.len() >= 512 {
            use rayon::prelude::*;
            moving.par_iter_mut().with_min_len(128).for_each(step);
        } else {
            moving.iter_mut().for_each(step);
        }
        self.frames = frames;
        self.seat_riders();
        self.hold_stuck();
    }

    /// Projectiles stuck in an entity, on the bone of its model they went into as drawn: they
    /// go with the arm or the body, keep up however fast it goes, fall over with it when it dies
    /// and go when it does. Bones are last frame's, taken over by where the host is now.
    fn hold_stuck(&mut self) {
        let stuck: Vec<u64> = self
            .entities
            .iter()
            .filter(|e| e.1.stuck.is_some())
            .map(|e| *e.0)
            .collect();
        let now = self.anim_tick();
        let models = &self.entity_render.models;
        let dir = |yaw: f32, pitch: f32| {
            let ((sy, cy), (sp, cp)) = ((yaw as f64).sin_cos(), (pitch as f64).sin_cos());
            DVec3::new(-sy * cp, sp, -cy * cp)
        };
        for id in stuck {
            let Some(mut st) = self.entities.get(&id).and_then(|e| e.stuck) else { continue };
            let host = if st.host == self.player_id {
                Some((self.me.pos, self.me.body_yaw))
            } else {
                self.entities.get(&st.host).map(|h| (h.pos, h.body_yaw))
            };
            let Some((pos, body)) = host else {
                if st.leaving {
                    self.entities.remove(&id);
                }
                continue;
            };
            if let Some((last_pos, last_body)) = st.last {
                // Once: the bone its line goes into first, else the one the whole body goes with.
                if !st.looked && now >= st.seen_at && models.posed(st.host) {
                    let (p, d) = (last_pos + turn_y(st.at, last_body), dir(last_body + st.yaw, st.pitch));
                    st.bone = match models.ray_bones(st.host, p - d, d, 2.0) {
                        // Its tip a little way in: it is drawn about its middle, the tip 0.225 ahead.
                        Some((kind, part, t)) => models
                            .to_bone(st.host, kind, part, p - d + d * (t - 0.125), d)
                            .map(|b| (kind, part, b.0, b.1)),
                        None => models.main_bone(st.host).and_then(|(kind, part)| {
                            models
                                .to_bone(st.host, kind, part, p, d)
                                .map(|b| (kind, part, b.0, b.1))
                        }),
                    };
                    st.looked = true;
                }
                if let Some((wp, wd)) = st
                    .bone
                    .and_then(|(kind, part, p, d)| models.from_bone(st.host, kind, part, p, d))
                {
                    st.placed = Some((turn_y(wp - last_pos, -last_body), turn_y(wd, -last_body)));
                }
            }
            st.last = Some((pos, body));
            let Some(e) = self.entities.get_mut(&id) else { continue };
            if st.pitch.is_nan() {
                st.pitch = e.target_pitch;
            }
            let (p, d) = st.placed.unwrap_or((st.at, dir(st.yaw, st.pitch)));
            e.stuck = Some(st);
            let (p, d) = (pos + turn_y(p, body), turn_y(d, body));
            (e.pos, e.target) = (p, p);
            let yaw = (-d.x).atan2(-d.z) as f32;
            (e.yaw, e.target_yaw, e.body_yaw) = (yaw, yaw, yaw);
            (e.pitch, e.target_pitch) = (d.y.clamp(-1.0, 1.0).asin() as f32, e.target_pitch);
        }
    }

    /// Riders where their seats are on their mounts as drawn this frame, mounts first.
    fn seat_riders(&mut self) {
        let c = content();
        let riders: Vec<u64> = self
            .entities
            .iter()
            .filter(|e| e.1.vehicle.is_some())
            .map(|e| *e.0)
            .collect();
        for _ in 0..4 {
            for &id in &riders {
                let Some((mount, seat)) = self.entities.get(&id).and_then(|e| e.vehicle) else { continue };
                let Some(at) = self
                    .entities
                    .get(&mount)
                    .and_then(|m| seat_at(c, m.kind, m.pos, m.yaw, seat))
                else {
                    continue;
                };
                if let Some(e) = self.entities.get_mut(&id) {
                    (e.pos, e.target) = (at, at);
                }
            }
        }
    }

    /// Hitting a block chips pieces off the face being hit, one per tick like vanilla.
    fn chip_block(&mut self, dt: f64) {
        let c = content();
        self.hit_dust -= dt as f32;
        if let (Some((frame, pos, _)), Some(hit)) = (self.breaking, self.target) {
            if self.hit_dust <= 0.0 && (hit.frame, hit.origin) == (frame, pos) {
                self.hit_dust = 0.05;
                let kind = c.states[hit.state as usize].effects[crate::content::block_event::HIT].particle;
                if kind != u32::MAX {
                    let normal = crate::world::DIRS[hit.face as usize].as_vec3();
                    let at = self
                        .frame_place(frame)
                        .to_world(pos.as_dvec3() + DVec3::splat(0.5) + (normal * 0.55).as_dvec3());
                    let spread = (glam::Vec3::splat(0.45) * (glam::Vec3::ONE - normal.abs())).to_array();
                    let velocity = [0.0, 2.0, 0.0];
                    self.particles.spawn(
                        &weft_abi::ParticleBurst {
                            kind,
                            pos: at.to_array().into(),
                            spread: spread.into(),
                            velocity: velocity.into(),
                            speed: 0.72,
                            count: 1,
                            block: hit.state,
                            ..weft_abi::ParticleBurst::DEFAULT
                        },
                        &self.world,
                    );
                }
            }
        }
    }

    fn upload_meshes(&mut self, gpu: &mut dyn Backend, textures: &GameTextures) {
        let center: IVec3 = (self.player.pos.floor().as_ivec3()) >> 4;
        // Wide enough for every column the server keeps sent (view radius + 2).
        let reach = self.view_radius + 3;
        if self.render.set_tint_tiles(gpu, (reach * 2 + 1) as u32) {
            self.world.retint_all();
        }
        for (col, tints) in self
            .world
            .take_tints(glam::IVec2::new(center.x, center.z), reach)
        {
            self.render.upload_tints(gpu, col, &tints);
        }
        if self.joined && self.terrain_view != (center, self.view_radius) {
            self.terrain_view = (center, self.view_radius);
            let _ = self
                .terrain
                .tx
                .send(terrain::In::View { center, radius: self.view_radius });
        }
        self.update_glow(crate::content::content());
        // Enough jobs that meshing keeps up at 50 frames per second too (results are taken once
        // a frame), not only at hundreds.
        self.world
            .schedule_meshing(center, crate::meshing().current_num_threads() * 512);
        self.render.update_solid(gpu, &self.world, center);
        self.world.set_eye(center.y);
        // Uploads too, oldest first; the rest wait for the next frame (dropped if their cube
        // changed or left meanwhile). Edits and what is around the player go right away.
        let (now, later) = self.world.take_meshes(center);
        for r in now {
            self.render.upload(gpu, textures, (0, r.pos), &r.mesh);
            self.world.mark_shown(r.pos);
        }
        self.meshes.extend(later);
        let start = Instant::now();
        while start.elapsed() < self.budget {
            let Some(r) = self.meshes.pop_front() else { break };
            if self.world.current(&r) {
                self.render.upload(gpu, textures, (0, r.pos), &r.mesh);
                self.world.mark_shown(r.pos);
            }
        }
        self.spent += start.elapsed().as_secs_f64();
        // Structures: meshed in their own coordinates, a changed cube at a time.
        for (&id, e) in self.entities.iter_mut() {
            let Some(s) = e.structure.as_mut() else { continue };
            s.schedule_meshing(IVec3::ZERO, 64);
            let (now, later) = s.take_meshes(IVec3::ZERO);
            for r in now.into_iter().chain(later) {
                self.render.upload(gpu, textures, (id, r.pos), &r.mesh);
            }
            for pos in std::mem::take(&mut s.removed) {
                self.render.remove((id, pos));
            }
            self.drawn_frames.insert(id);
        }
        let entities = &self.entities;
        let gone: Vec<u64> = self
            .drawn_frames
            .iter()
            .copied()
            .filter(|id| !entities.get(id).is_some_and(|e| e.structure.is_some()))
            .collect();
        for id in gone {
            self.render.remove_frame(id);
            self.drawn_frames.remove(&id);
        }
        if self.joined {
            let cam = self.render_camera().pos;
            self.lod
                .update(gpu, &mut self.render, &mut self.world, textures, self.seed, cam, self.view_radius);
        }
        self.render.set_textures(gpu, textures);
    }

    fn chance(&mut self) -> f32 {
        self.rng.float()
    }

    /// Swirls of its effects' colors rising around an entity, a few a second as vanilla's; the
    /// local player's only where its body shows.
    fn effect_swirls(&mut self, id: u64, pos: DVec3, dims: physics::Dims, dt: f32) {
        let c = content();
        let me = id == self.player_id;
        if me && !self.camera_has(crate::tag_bits::camera::BODY) {
            return;
        }
        let now = self.ticks();
        let e = if me { Some(&self.me) } else { self.entities.get(&id) };
        let colors: Vec<u32> = e
            .into_iter()
            .flat_map(|e| &e.effects)
            .filter(|f| f.until > now)
            .filter_map(|f| {
                c.game
                    .entity_effects
                    .get(f.id as usize)
                    .and_then(|d| d.color)
            })
            .collect();
        let Some(kind) = c
            .game
            .particle_names
            .get(weft_abi::particle::EFFECT)
            .copied()
        else {
            return;
        };
        if colors.is_empty() || self.chance() >= dt * 8.0 {
            return;
        }
        let rgb = colors[(self.chance() * colors.len() as f32) as usize % colors.len()];
        let (hw, h) = (dims.half_width as f32, dims.height as f32);
        self.particles.spawn(
            &weft_abi::ParticleBurst {
                kind,
                pos: (pos + DVec3::Y * (dims.height * 0.5)).to_array().into(),
                count: 1,
                spread: [hw, h * 0.5, hw].into(),
                velocity: [0.0, 0.3, 0.0].into(),
                color: rgb,
                ..weft_abi::ParticleBurst::DEFAULT
            },
            &self.world,
        );
    }

    fn sprint_dust(&mut self, pos: DVec3, vel: DVec3, half_width: f32, dt: f32) {
        let c = content();
        let under = self
            .world
            .block_or_air((pos - DVec3::Y * 0.2).floor().as_ivec3())
            .unwrap_or(0);
        let kind = c.states[under as usize].effects[crate::content::block_event::SPRINT].particle;
        if kind == u32::MAX || under == 0 || c.states[under as usize].liquid.is_some() || self.chance() >= dt * crate::world::TPS as f32 {
            return;
        }
        let mut noise = || self.rng.float() * 0.8 - 0.4;
        let aim = glam::Vec3::new(-4.0 * vel.x as f32 + noise(), 1.5 + noise(), -4.0 * vel.z as f32 + noise());
        let speed = (self.chance() + self.chance() + 1.0) / 3.0 * 3.6;
        let velocity = (aim.normalize_or_zero() * speed + glam::Vec3::Y * 2.0).to_array();
        self.particles.spawn(
            &weft_abi::ParticleBurst {
                kind,
                block: under,
                pos: (pos + DVec3::Y * 0.1).to_array().into(),
                count: 1,
                spread: [half_width, 0.0, half_width].into(),
                velocity: velocity.into(),
                ..weft_abi::ParticleBurst::DEFAULT
            },
            &self.world,
        );
    }

    fn overlays(&self, _textures: &GameTextures) -> Vec<sky::Overlay> {
        if !self.camera_has(crate::tag_bits::camera::EFFECTS) || self.hide_gui { Vec::new() } else { self.environment.overlays.clone() }
    }

    /// Like vanilla's block display ticks: random blocks around the player that name a particle
    /// (`abi::comp::AMBIENT_PARTICLE`) show one, each about every three seconds, and liquids
    /// with `abi::comp::DRIP` drip through the ceiling under them. The server has no part in it.
    fn ambient(&mut self) {
        const TRIES: usize = 667;
        let c = content();
        let kinds = self.ambient_kinds.get_or_insert_with(|| {
            let (ambient, drip) = (c.components.id(weft_abi::comp::AMBIENT_PARTICLE), c.components.id(weft_abi::comp::DRIP));
            let named = |s: u32, comp: Option<u32>| {
                comp.and_then(|id| c.components.text(crate::components::BLOCK, s, id))
                    .map_or(Vec::new(), |names| {
                        names
                            .split(',')
                            .filter_map(|n| c.game.particle_names.get(n).copied())
                            .collect()
                    })
            };
            (0..c.states.len() as u32)
                .map(|s| (named(s, ambient), named(s, drip).first().copied()))
                .collect()
        });
        let center = self.player.pos.floor().as_ivec3();
        let mut bursts: Vec<weft_abi::ParticleBurst> = Vec::new();
        for _ in 0..TRIES {
            let r = self.rng.next();
            let p = center + IVec3::new((r % 33) as i32 - 16, ((r >> 8) % 33) as i32 - 16, ((r >> 16) % 33) as i32 - 16);
            let state = self.world.block(p);
            let Some((own, drip)) = kinds.get(state as usize) else { continue };
            for &kind in own {
                // Vanilla's: over a torch, and out from the wall where it leans.
                let wall = c
                    .held_by(state)
                    .map(|f| f as usize)
                    .filter(|f| f / 2 != 1)
                    .map(|f| crate::world::DIRS[f].as_dvec3());
                let pos = (p.as_dvec3() + wall.map_or(DVec3::new(0.5, 0.7, 0.5), |d| DVec3::new(0.5, 0.92, 0.5) - d * 0.27)).to_array();
                bursts.push(weft_abi::ParticleBurst {
                    kind,
                    pos: pos.into(),
                    spread: [0.1, 0.1, 0.1].into(),
                    speed: 0.1,
                    count: 1,
                    ..weft_abi::ParticleBurst::DEFAULT
                });
            }
            // Through a ceiling: a solid block under the liquid, air under that.
            if let Some(kind) = drip.filter(|_| (r >> 24) % 8 == 0) {
                let (below, air) = (self.world.block(p - IVec3::Y), self.world.block(p - IVec3::Y * 2));
                if c.solid(below) && air == 0 {
                    let pos = [p.x as f64 + 0.5, p.y as f64 - 1.05, p.z as f64 + 0.5];
                    bursts.push(weft_abi::ParticleBurst {
                        kind,
                        pos: pos.into(),
                        spread: [0.4, 0.0, 0.4].into(),
                        count: 1,
                        ..weft_abi::ParticleBurst::DEFAULT
                    });
                }
            }
        }
        for b in bursts {
            self.particles.spawn(&b, &self.world);
        }
    }

    /// The engine's cosmetics of one entity, the local player's too: steps by distance walked,
    /// the knock of what it mines, a sprint's dust, bubbles from its eyes under a liquid, idle
    /// noises about every twelve seconds (vanilla's ambient timer), and a puff of smoke when its
    /// death animation ends (vanilla's 20 poof particles).
    pub fn entity_effects(&mut self, a: &weft_abi::ClientEntityArgs) {
        use crate::audio::Play;
        use crate::content::block_event;
        use crate::registry::entity_event;
        let c = content();
        let k = &c.game.entities[a.kind as usize];
        let pos = crate::world::dvec(a.pos);
        if a.dying >= 1.0 {
            let kind = k.effects[entity_event::DEATH].particle;
            if kind != u32::MAX {
                let crate::physics::Dims { half_width: hw, height: h, .. } = physics::posed(k.size(), Pose::STAND);
                let (w, h) = (hw as f32, h as f32 * 0.5);
                let at = (pos + DVec3::Y * h as f64).to_array();
                self.particles.spawn(
                    &weft_abi::ParticleBurst {
                        kind,
                        pos: at.into(),
                        spread: [w, h, w].into(),
                        velocity: [0.0, 0.4, 0.0].into(),
                        speed: 0.8,
                        count: 20,
                        ..weft_abi::ParticleBurst::DEFAULT
                    },
                    &self.world,
                );
            }
            return;
        }
        if a.dying >= 0.0 {
            return;
        }
        let me = a.entity == self.player_id;
        // What it mines knocks every four ticks, as vanilla's does.
        let mining = if me {
            self.target
                .filter(|h| {
                    self.breaking
                        .is_some_and(|(f, p, _)| (f, p) == (h.frame, h.origin))
                })
                .map(|h| {
                    (
                        self.frame_place(h.frame)
                            .to_world(h.origin.as_dvec3() + DVec3::splat(0.5)),
                        h.state,
                    )
                })
        } else {
            self.others_mining
                .iter()
                .find(|m| m.0 == a.entity)
                .map(|m| (m.1.as_dvec3() + DVec3::splat(0.5), self.world.block_or_air(m.1).unwrap_or(0)))
        };
        let Some(e) = (if me { Some(&mut self.me) } else { self.entities.get_mut(&a.entity) }) else { return };
        let mut sounds = Vec::new();
        if a.walked > e.next_step {
            // Vanilla: a step every 1/0.6 blocks walked, the first as it sets off.
            e.next_step = a.walked + 1.6;
            let sound = if k.effects[entity_event::STEP].sound != u32::MAX {
                k.effects[entity_event::STEP].sound
            } else {
                self.world
                    .block_or_air((pos - DVec3::Y * 0.2).floor().as_ivec3())
                    .map_or(u32::MAX, |s| c.states[s as usize].effects[block_event::STEP].sound)
            };
            sounds.push(Play { sound, pos: Some(pos), volume: 0.25, pitch: 0.0 });
        }
        e.knock -= a.dt;
        if let Some((at, state)) = mining.filter(|_| e.knock <= 0.0) {
            e.knock = 0.2;
            sounds.push(Play { sound: c.states[state as usize].effects[block_event::HIT].sound, pos: Some(at), volume: 0.4, pitch: 0.0 });
        }
        let (sprinting, body_yaw, dims) = (e.sprinting, e.body_yaw, e.dims());
        self.sounds
            .extend(sounds.into_iter().filter(|p| p.sound != u32::MAX));
        if sprinting {
            // Vanilla's sprint, about 0.28 blocks a tick, the way the body faces.
            let (sin, cos) = (body_yaw as f64).sin_cos();
            self.sprint_dust(pos, DVec3::new(-sin, 0.0, -cos) * 0.28, dims.half_width as f32, a.dt);
        }
        self.effect_swirls(a.entity, pos, dims, a.dt);
        // Bubbles from the eyes under a liquid that has them.
        let eye = pos + DVec3::Y * dims.eye;
        let bubble = physics::liquid_at(c, &world::SeenCells { world: &self.world, air_if_missing: false }, eye).and_then(|l| {
            let state = c.liquids[l as usize].first_state;
            let id = c.components.id(weft_abi::comp::BUBBLE)?;
            c.game
                .particle_names
                .get(c.components.text(crate::components::BLOCK, state, id)?)
                .copied()
        });
        if let Some(kind) = bubble.filter(|_| self.chance() < a.dt * 1.5) {
            self.particles.spawn(
                &weft_abi::ParticleBurst {
                    kind,
                    pos: eye.to_array().into(),
                    count: 2,
                    spread: [0.2, 0.1, 0.2].into(),
                    speed: 0.5,
                    ..weft_abi::ParticleBurst::DEFAULT
                },
                &self.world,
            );
        }
        if k.effects[entity_event::AMBIENT].sound != u32::MAX && self.chance() < a.dt / 12.0 {
            self.sounds
                .push(Play { sound: k.effects[entity_event::AMBIENT].sound, pos: Some(eye), volume: 1.0, pitch: 0.0 });
        }
    }

    /// What the ears need from the camera this frame.
    pub fn listener(&self) -> crate::audio::Listener {
        let c = content();
        let cam = self.render_camera();
        crate::audio::Listener {
            pos: cam.pos,
            yaw: cam.yaw,
            pitch: cam.pitch,
            acoustics: crate::audio::Acoustics {
                cutoff: self.environment.values.lowpass,
                reverb: self.environment.values.reverb,
                volume: self.environment.values.volume,
            },
            dim: self.world.dim,
            holds: c
                .game
                .music
                .iter()
                .filter_map(|m| m.when)
                .filter(|&id| {
                    crate::conditions::on_client(Some(self), id, &crate::conditions::Ask { pos: self.player.pos, ..Default::default() })
                })
                .collect(),
        }
    }

    /// No sky light at the camera: a cave, or under a roof.
    pub fn enclosed(&self) -> bool {
        let c = content();
        let eye = self.camera().pos.floor().as_ivec3();
        crate::world::light::enclosed(&c.dims[self.world.dim as usize], self.world.light(eye))
    }

    fn pick_entity(&self, eye: DVec3, dir: DVec3, max: f64) -> Option<u64> {
        let mut best: Option<(f64, u64)> = None;
        let mut scratch = Vec::new();
        for (&id, e) in &self.entities {
            // Structures are aimed at by their blocks.
            if !e.is_body() || id == self.player_id || e.structure.is_some() {
                continue;
            }
            // The parts of its hitbox, what the server hits too.
            let parts = content().game.entities[e.kind as usize].hitboxes.as_ref();
            if let Some((_, t, ..)) = crate::hitbox::ray_entity(parts, &e.hit_body(), eye, dir, max, &mut scratch) {
                if best.is_none_or(|(bt, _)| t < bt) {
                    best = Some((t, id));
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Copies an engine field component into the entity (or the own player) that shows it.
    fn engine_field(&mut self, id: u64, comp: u32, bytes: &[u8]) {
        use crate::net::codec::Wire;
        let c = content();
        let crate::registry::FieldComps {
            health,
            burning,
            worn,
            shape,
            held,
            off_held,
            using,
            swaps,
            looping,
            effect,
            forced,
            riding,
            stuck,
            game_mode,
            permission,
            attributes,
            shown,
            ground,
            // Only in its slot.
            trash: _,
        } = c.game.fields;
        if Some(comp) == c.components.id(weft_abi::environment::COMPONENT) {
            let layers: Vec<weft_abi::environment::EnvironmentLayer> = weft_abi::data::from_bytes(bytes).unwrap_or_default();
            if layers.is_empty() {
                self.environment.remote.remove(&id);
            } else {
                self.environment.remote.insert(id, layers);
            }
        }
        let now = self.ticks();
        let arriving = self.arriving_tick();
        let f: Vec<f32> = words(bytes).map(f32::from_le_bytes).collect();
        let shaped = || {
            let size = (f.len() == 4 && f[0] > 0.0).then(|| weft_abi::EntitySize { width: f[0], height: f[1], eye: f[2] });
            let look = words(bytes)
                .nth(3)
                .map(u32::from_le_bytes)
                .filter(|&l| l != u32::MAX);
            (size, look)
        };
        if [held, off_held, worn, shown, ground, shape].contains(&comp) {
            self.entity_render.changed_items.insert(id);
        }
        let mine = id == self.player_id;
        let other = self
            .entities
            .get_mut(&id)
            .or(self.showing_blocks.get_mut(&id));
        let Some(e) = (if mine { Some(&mut self.me) } else { other }) else { return };
        if comp == health && f.len() == 2 {
            // Lower health out of a lower maximum, or on joining, isn't a hit.
            if f[0] < e.health && (!mine || (f[1] >= self.max_health && e.health > 0.0)) {
                e.hurt = HURT_FLASH;
            }
            e.health = f[0];
        } else if comp == riding {
            e.vehicle = (bytes.len() >= 12).then(|| {
                let mount = u64::from_le_bytes(bytes[..8].try_into().unwrap());
                (mount, u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as u8)
            });
        } else if comp == stuck {
            // Its pitch is the one it flew in with, as it arrives with its moves.
            e.stuck = (bytes.len() >= 24).then(|| Stuck {
                host: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
                at: DVec3::new(f[2] as f64, f[3] as f64, f[4] as f64),
                yaw: f[5],
                pitch: f32::NAN,
                bone: None,
                looked: false,
                seen_at: arriving,
                placed: None,
                last: None,
                leaving: false,
            });
        } else if comp == burning {
            e.burning = bytes.first().is_some_and(|&b| b != 0);
        } else if comp == worn {
            e.worn = Vec::<ItemStack>::get(&mut crate::net::codec::R(bytes)).unwrap_or_default();
        } else if comp == shown {
            e.shown = Vec::<ItemStack>::get(&mut crate::net::codec::R(bytes)).unwrap_or_default();
        } else if comp == ground {
            e.item = ItemStack::get(&mut crate::net::codec::R(bytes)).unwrap_or_default();
        } else if comp == held {
            e.held = ItemStack::get(&mut crate::net::codec::R(bytes)).unwrap_or_default();
        } else if comp == swaps {
            e.swaps = parse_swaps(bytes);
        } else if comp == looping {
            // For who wasn't around when it started; those who were have it from `PlayAction`.
            let name = String::from_utf8_lossy(bytes);
            if !name.is_empty() && e.action.as_ref().is_none_or(|a| a.0 != name) {
                e.action = Some((name.into_owned(), ((arriving - e.born).max(0.0) / crate::world::TPS as f64) as f32, true));
            }
        } else if comp == off_held {
            e.off_held = ItemStack::get(&mut crate::net::codec::R(bytes)).unwrap_or_default();
        } else if comp == using {
            let v = words(bytes).next().map_or(0, u32::from_le_bytes);
            let ticks = (v & !(1 << 31)) as f32;
            if v != 0 && (e.using.is_none() || e.use_ticks != ticks) {
                e.use_at = arriving;
            }
            e.use_ticks = ticks;
            e.using = (v != 0).then(|| (e.using.map_or(0.0, |u| u.0), v >> 31 != 0));
        } else if comp == shape {
            (e.size, e.look) = shaped();
        } else if comp == effect {
            e.effects = crate::world::effects_on(Some(bytes), now).collect();
        }
        if !mine {
            return;
        }
        // What the local player's own movement and screens go by.
        if comp == forced {
            self.player.forced = (bytes.len() >= 8).then(|| {
                let yaw = f32::from_le_bytes(bytes[4..8].try_into().unwrap());
                (Pose::from_id(u32::from_le_bytes(bytes[..4].try_into().unwrap())), (!yaw.is_nan()).then_some(yaw))
            });
        } else if comp == health && f.len() == 2 {
            self.max_health = f[1];
            if f[0] > 0.0 {
                self.dead = false;
            }
        } else if comp == shape {
            self.player.size = shaped()
                .0
                .unwrap_or(c.game.entities[c.game.player_kind as usize].size());
        } else if comp == game_mode {
            let mode = crate::world::GameMode(words(bytes).next().map_or(0, u32::from_le_bytes) as u16);
            self.mode = mode;
            self.player.flying = mode.has(crate::tag_bits::game_mode::ALWAYS_FLY) || (self.can_fly() && self.player.flying);
            self.player.noclip = mode.has(crate::tag_bits::game_mode::NOCLIP);
            self.player.fly_speed = 1.0;
        } else if comp == permission {
            self.permission = words(bytes).next().map_or(0, u32::from_le_bytes);
        } else if comp == attributes {
            use crate::registry::attr as a;
            let could = self.can_fly();
            for (dst, v) in self.attributes.iter_mut().zip(&f) {
                *dst = *v;
            }
            self.player.speed_mul = self.attributes[a::MOVEMENT_SPEED as usize] as f64;
            self.player.jump_mul = self.attributes[a::JUMP_STRENGTH as usize] as f64;
            self.player.can_sprint = self.attributes[a::CAN_SPRINT as usize] >= 1.0;
            self.player.moves = physics::Moves::of(|a| self.attributes[a as usize] as f64);
            // Flight goes with what allowed it (a mode, a ring), which comes a tick after the mode;
            // a mod's flight without it stays.
            if could && !self.can_fly() && !self.mode.has(crate::tag_bits::game_mode::ALWAYS_FLY) {
                self.player.flying = false;
            }
        }
    }

    /// The light every entity gives off (`glow`): the player's from what it holds and wears,
    /// others' from what they show.
    fn update_glow(&mut self, c: &crate::content::Content) {
        let (now, light) = (self.ticks(), c.ids.light.unwrap_or(u32::MAX));
        let mut sources = Vec::new();
        let mine = (!self.mode.has(crate::tag_bits::game_mode::HIDDEN) && !self.dead).then_some((&self.player_id, &self.me));
        let all = mine
            .into_iter()
            .chain(self.entities.iter())
            .chain(&self.showing_blocks);
        let hidden = |e: &ClientEntity| {
            e.effects.iter().any(|f| {
                f.until > now
                    && c.game
                        .entity_effects
                        .get(f.id as usize)
                        .is_some_and(|d| d.hides)
            })
        };
        for (id, e) in all.filter(|e| e.1.dying.is_none() && !hidden(e.1)) {
            let stored = if *id == self.player_id { self.components.get(&light) } else { self.entity_components.get(&(*id, light)) }
                .map(Vec::as_slice);
            let pos = if *id == self.player_id { self.player.pos } else { e.pos };
            sources.push(glow::entity_glow(c, e, now, pos, e.body_yaw, stored));
        }
        let eye = self.player.pos;
        self.world.set_glow(c, eye, sources);
    }

    /// Wire of the world and of bodies, before the frame's passes.
    pub fn update_wires(&mut self, gpu: &mut dyn weft_render::Backend, textures: &textures::GameTextures) {
        self.wires.update(gpu, 0, &mut self.world, textures);
        for (&id, e) in &mut self.entities {
            if let Some(s) = e.structure.as_deref_mut() {
                self.wires.update(gpu, id, s, textures);
            }
        }
        let entities = &self.entities;
        self.wires
            .keep_frames(gpu, |id| entities.get(&id).is_some_and(|e| e.structure.is_some()));
    }

    /// Where `frame`'s space is drawn: a structure's pose, the world as it is at 0.
    pub fn frame_place(&self, frame: u64) -> physics::Transform {
        self.entities
            .get(&frame)
            .filter(|_| frame != 0)
            .map_or(physics::Transform::default(), |e| physics::Transform::of(e.pos))
    }

    /// The block looked at: the world's or a structure's, whichever the ray enters first.
    fn raycast(&self, eye: DVec3, look: DVec3, reach: f64) -> Option<Hit> {
        let world = &self.world;
        let now = self.body_tick(self.body_clock - STEP + self.accum);
        let structures: Vec<(u64, physics::Transform, &dyn physics::Blocks)> = self
            .entities
            .iter()
            .filter_map(|(&id, e)| {
                e.structure
                    .as_deref()
                    .map(|s| (id, physics::Transform::of(e.pose(now)), s as &dyn physics::Blocks))
            })
            .collect();
        physics::pick(content(), &world::SeenCells { world, air_if_missing: false }, &structures, eye, look, reach, weft_abi::ray::BLOCKS)
    }

    /// Server tick (fractional) bodies are shown at when the client clock reads `clock`.
    /// The server's tick its entities are shown at now, with its fraction: their clips' clock.
    pub fn anim_tick(&self) -> f64 {
        self.body_tick(self.body_clock - STEP + self.accum)
    }

    /// The server's tick of what arrives now: it is shown `INTERP_TICKS` later.
    pub fn arriving_tick(&self) -> f64 {
        self.anim_tick() + INTERP_TICKS
    }

    fn body_tick(&self, clock: f64) -> f64 {
        (clock - self.tick_offset.unwrap_or(clock)) * self.tick_rate - INTERP_TICKS
    }

    /// Position drawn this frame, between the last two physics ticks; on a body, on the body as
    /// it is shown now.
    pub fn smooth_pos(&self) -> DVec3 {
        let t = (self.accum / STEP).clamp(0.0, 1.0);
        if let Some((b, before, now)) = self.player.riding {
            if let Some(e) = self.entities.get(&b) {
                return e.pose(self.body_tick(self.body_clock - STEP + self.accum)) + before.lerp(now, t);
            }
        }
        self.player.prev.lerp(self.player.pos, t)
    }

    /// From inside a liquid everything looks closer, vanilla's 60 degrees for 70.
    fn inside_fov(&self) -> f32 {
        if self.eye_liquid().is_some() { 6.0 / 7.0 } else { 1.0 }
    }

    /// Where the player's eyes are and look; what the camera mode starts from.
    pub fn camera(&self) -> Camera {
        // From inside a liquid everything looks closer, vanilla's 60 degrees for 70.
        let inside = self.inside_fov();
        // Lying on its back the head is behind the middle of the body (on a bed's pillow).
        let pillow = match self.player.forced {
            Some((pose, Some(yaw))) if pose.def().tilt == weft_abi::pose_tilt::ON_BACK => {
                DVec3::new(yaw.sin() as f64, 0.0, yaw.cos() as f64) * 0.75
            }
            _ => DVec3::ZERO,
        };
        Camera {
            pos: self.smooth_pos() + DVec3::Y * self.player.eye + pillow,
            yaw: self.player.yaw,
            pitch: self.player.pitch,
            fov: self.fov * self.player.fov_mul * self.player.zoom * inside,
            bob: (Vec3::ZERO, 0.0, 0.0),
        }
    }

    /// The liquid the eyes are in.
    fn eye_liquid(&self) -> Option<u16> {
        let world = &self.world;
        physics::liquid_at(content(), &world::SeenCells { world, air_if_missing: false }, self.smooth_pos() + DVec3::Y * self.player.eye)
    }

    /// The camera with view bobbing, for drawing (aiming uses the steady one).
    fn bobbed(&self, mut cam: Camera) -> Camera {
        use std::f32::consts::PI;
        let (f, g) = (-self.walk_dist * PI, self.bob);
        // Vanilla's bobHurt: a jolt to the side when hit, and falling over when dead.
        let hit = (self.me.hurt / HURT_FLASH).clamp(0.0, 1.0);
        let jolt = (hit.powi(4) * PI).sin() * 14.0;
        let fallen = self
            .dead_for
            .map_or(0.0, |t| 40.0 - 8000.0 / ((t * crate::world::TPS as f32).min(20.0) + 200.0));
        let (kick, roll, pitch) = self.entity_render.camera_kick;
        cam.bob = (
            Vec3::new(f.sin() * g * 0.5, -(f.cos() * g).abs(), 0.0) + kick,
            (f.sin() * g * 3.0 - jolt + fallen).to_radians() + roll,
            ((f - 0.2).cos() * g).abs().to_radians() * 5.0 + pitch,
        );
        cam
    }

    /// Vanilla's swing: starts over once past halfway, which is the quick arm of holding a block
    /// being mined.
    fn swing_arm(&mut self) {
        restart_swing(&mut self.hand_swing);
        self.hand_swing_off = false;
    }

    /// The camera drawn from, as its mode put it this frame (`camera::Cameras`).
    pub fn render_camera(&self) -> Camera {
        self.cameras.shown
    }

    /// The local player as an entity.
    /// Brings `me` up to what prediction, the inventory and the hand say this frame.
    fn sync_me(&mut self) {
        let worn = crate::content::equipped(content(), self.me.kind, &self.inventory).to_vec();
        let (held, off_held) = (self.shown_held(), self.off_held());
        let into = |(held, all): (f32, f32)| if endless_use(all) { held / self.tick_rate as f32 } else { held / all.max(1.0) };
        let using = self.using.map(|u| (into(u), self.using_off));
        let me = &mut self.me;
        me.use_ticks = self.using.map_or(0.0, |u| u.1);
        (me.yaw, me.target_yaw, me.pitch) = (self.player.yaw, self.player.yaw, self.player.pitch);
        (me.worn, me.held, me.off_held, me.using) = (worn, held, off_held, using);
        (me.arm_swing, me.swing_off, me.pose) = (self.hand_swing, self.hand_swing_off, self.player.pose);
        me.sprinting = self.player.sprinting && self.player.on_ground;
        me.age = self.start.elapsed().as_secs_f32();
        self.player.swims_in = crate::content::swims_in(content(), self.me.kind, &self.me.worn);
    }

    /// Columns distant terrain takes from saved cubes.
    pub fn lod_columns(&self) -> usize {
        self.lod.columns_len()
    }

    pub fn set_view_distance(&mut self, r: i32) {
        self.view_radius = r;
        self.send(ToServer::View(r as u32));
    }

    pub fn destroy(&mut self, gpu: &mut dyn Backend) {
        for b in self.scene_bufs.drain(..) {
            gpu.destroy_buffer(b);
        }
        self.render.destroy(gpu);
        self.entity_render.destroy(gpu);
        self.particles.destroy(gpu);
        self.wires.destroy(gpu);
        self.weather_render.destroy(gpu);
        self.sky.destroy(gpu);
        self.gpu_terrain.destroy(gpu);
    }

    /// Simulation daylight used by conditions and prediction, independent of perception overrides.
    pub fn daylight(&self) -> f32 {
        let day = content().dims[self.world.dim as usize].daylight(self.world.time) * (1.0 - self.look.darkness);
        if self.lightning > 0.0 { (day + 0.5).min(1.0) } else { day }
    }

    /// The weather's look going over to the current one's, its sound now and then, and a
    /// lightning's flash (`weather` draws the precipitation).
    fn weather_effects(&mut self, dt: f32) {
        let weather = &content().game.weathers[self.weather_id as usize];
        let (to, step) = (weather.look, dt / 5.0);
        let go = |v: &mut f32, t: f32| *v += (t - *v).clamp(-step, step);
        let l = &mut self.look;
        go(&mut l.falls, if weather.falls() { to.falls } else { 0.0 });
        go(&mut l.sky_gray, to.sky_gray);
        go(&mut l.sky_cover, to.sky_cover);
        go(&mut l.fog_gray, to.fog_gray);
        go(&mut l.cloud_gray, to.cloud_gray);
        go(&mut l.cloud_dark, to.cloud_dark);
        go(&mut l.darkness, to.darkness);
        self.lightning = (self.lightning - dt).max(0.0);
        if weather.lightning() && self.chance() < dt / 12.0 {
            self.lightning = 0.15;
        }
        if let Some(sound) = weather
            .sound
            .filter(|_| self.look.falls > 0.5 && !self.enclosed())
        {
            if self.chance() < dt {
                let volume = self.look.falls;
                self.sounds
                    .push(crate::audio::Play { sound, pos: None, volume, pitch: 0.0 });
            }
        }
    }

    /// Ticks the dimension ran by now, what entity effects end by.
    pub fn ticks(&self) -> u64 {
        self.world.ticks + self.since_time as u64
    }

    /// Game ticks now, with the fraction of the current one.
    fn smooth_time(&self) -> f64 {
        let fraction = if self.since_time < crate::world::TPS as f64 { self.accum.clamp(0.0, STEP) * self.tick_rate } else { 0.0 };
        self.world.time as f64 + self.since_time + fraction
    }

    /// The frame as every world pass sees it (`gpu_layout::Scene`).
    fn scene(&self, cam: &Camera, frame: &FrameInfo) -> Scene {
        let environment = &self.environment.values;
        let (fog_start, fog_end) = (environment.fog_start, environment.fog_end);
        let aspect = frame.width as f32 / frame.height.max(1) as f32;
        let view = cam.view();
        let inv = view.inverse();
        // Animated textures follow the game's ticks: between two from the server the client
        // counts on for a second at most, so they stop with the game.
        let ticks = (self.smooth_time() % (1 << 20) as f64) as f32;
        Scene {
            view_proj: (cam.proj(aspect) * view).to_cols_array_2d(),
            right: inv.transform_vector3(Vec3::X).extend(0.0).to_array(),
            up: inv.transform_vector3(Vec3::Y).extend(0.0).to_array(),
            fog_color: self.fog_color(),
            params: [fog_start, fog_end, environment.daylight, crate::shaders::time()],
            light: [environment.ambient, self.brightness, ticks, environment.night_vision],
            environment: [environment.sunlight, environment.cloud_fog_start, environment.cloud_fog_end, 0.0],
            sun: self.sun(),
        }
    }

    /// Toward the sun: it rises in +x, is highest at noon and leans a little south so that
    /// shadows are never straight down.
    fn sun(&self) -> [f32; 4] {
        let d = &content().dims[self.world.dim as usize];
        if !d.sky_light {
            return [0.0, 1.0, 0.0, 0.0];
        }
        let a = d.day_turn(self.world.time as f64) * std::f64::consts::TAU;
        let v = Vec3::new(a.cos() as f32, a.sin() as f32, 0.35).normalize();
        [v.x, v.y, v.z, v.y.max(0.0) * self.environment.values.sunlight]
    }

    /// `list` goes in the world's pass, `early` before it.
    /// The world into `list`; into `after` what is drawn over it once the depth of the solid world
    /// has been taken for the next frame's occlusion culling: the sky, clouds, translucent
    /// things, particles, weather and the hand, none of which hide what is behind them for good.
    pub fn render(
        &mut self,
        gpu: &mut dyn Backend,
        list: &mut CommandList,
        after: &mut CommandList,
        early: &mut CommandList,
        frame: &FrameInfo,
        textures: &GameTextures,
    ) {
        self.entity_render.local = self.player_id;
        for (entity, png) in std::mem::take(&mut self.entity_render.skin_pending) {
            self.entity_render.set_skin(gpu, entity, &png);
        }
        let cam = self.render_camera();
        let scene = self.scene(&cam, frame);
        while self.scene_bufs.len() < gpu.frames_in_flight() {
            self.scene_bufs.push(gpu.create_buffer(
                size_of::<Scene>() as u64,
                weft_render::usage::STORAGE | weft_render::usage::HOST,
                "scene",
            ));
        }
        let scene_buf = self.scene_bufs[frame.slot];
        gpu.mapped(scene_buf)[..size_of::<Scene>()].copy_from_slice(bytemuck::bytes_of(&scene));
        // Opaque blocks, then entities and cutout particles, then what is seen through:
        // translucent blocks, cracks and gizmo fills, blended particles, and the hand.
        let frames: Vec<(u64, DVec3)> = self
            .entities
            .iter()
            .filter(|(_, e)| e.structure.is_some())
            .map(|(&id, e)| (id, physics::Transform::of(e.pos).origin()))
            .collect();
        // The world's light on each, as the server worked it out when it got there.
        let lights: rustc_hash::FxHashMap<(u64, IVec3), u32> = frames
            .iter()
            .filter_map(|(id, _)| self.entities.get(id).map(|e| (*id, e)))
            .flat_map(|(id, e)| e.world_light.iter().map(move |(cp, l)| ((id, *cp), *l)))
            .collect();
        self.render
            .draw(gpu, list, early, frame, &cam, scene_buf, &frames, &lights);
        let lit: Vec<u32> = frames
            .iter()
            .map(|(id, _)| {
                self.entities
                    .get(id)
                    .map_or(crate::world::light::SKY_FULL as u32, |e| {
                        e.world_light
                            .values()
                            .copied()
                            .max()
                            .unwrap_or(crate::world::light::SKY_FULL as u32)
                    })
            })
            .collect();
        self.wires.draw(list, &cam, scene_buf, &frames, &lit);
        let mut late = std::mem::take(&mut self.late);
        late.clear();
        // The sky fills what the opaque world left, and translucent things are seen against it.
        let overlays = self.overlays(textures);
        self.sky.draw(
            gpu,
            &mut late,
            frame,
            &cam,
            scene_buf,
            self.world.dim as usize,
            self.smooth_time(),
            self.camera().pos.y,
            &self.environment.values,
            &self.environment.sky,
            &overlays,
        );
        self.render.draw_translucent(&mut late);
        let alive = !self.dead && !self.mode.has(crate::tag_bits::game_mode::NO_INTERACT);
        let hand = (alive && self.camera_has(crate::tag_bits::camera::HAND) && !self.hide_gui).then(|| entity_render::Hand {
            lowered: 1.0 - self.hand_height,
            lag: (self.player.pitch - self.hand_view.0, angle_diff(self.player.yaw, self.hand_view.1)),
            body: self.first_person_model,
        });
        let me = (alive && (self.camera_has(crate::tag_bits::camera::BODY) || hand.is_some())).then_some(&self.me);
        let world = &self.world;
        let light = |p: IVec3| world.light_shown(p);
        let mut hand_late = std::mem::take(&mut self.hand_late);
        hand_late.clear();
        let mut cracks: Vec<entity_render::Crack> = self
            .others_mining
            .iter()
            .map(|&(_, pos, state, progress)| entity_render::Crack { pos, progress, at: DVec3::ZERO, state })
            .collect();
        cracks.extend(
            self.breaking
                .map(|(f, pos, progress)| entity_render::Crack {
                    pos,
                    progress,
                    at: self.frame_place(f).origin(),
                    state: self.breaking_part,
                }),
        );
        self.entity_render.draw(
            gpu,
            list,
            frame,
            &cam,
            self.ticks(),
            &self.entities,
            me,
            hand,
            textures,
            &light,
            &|p: IVec3| self.world.biome_at(p),
            &scene,
            scene_buf,
            self.target
                .filter(|_| !self.hide_gui)
                .map(|h| (h, self.frame_place(h.frame).origin())),
            &cracks,
            &self.gizmo_lines,
            &self.gizmo_fills,
            &mut hand_late,
        );
        self.particles.draw(list, &mut late, scene_buf);
        let ticks = self.smooth_time();
        self.weather_render
            .draw(gpu, &mut late, frame, &cam, scene_buf, &self.world, self.look.falls, ticks, self.frame_dt);
        self.sky.draw_overlay(&mut hand_late, scene_buf);
        after.append(&mut late);
        after.append(&mut hand_late);
        (self.late, self.hand_late) = (late, hand_late);
        after.push(weft_render::Cmd::Mark);
    }

    pub fn fog_color(&self) -> [f32; 4] {
        let values = &self.environment.values;
        sky::rained(Vec3::from_array(values.fog_color).powf(1.0 / 2.2), 1.0, values.fog_gray)
            .powf(2.2)
            .extend(1.0)
            .to_array()
    }
}

/// Ticks of an item used until it is let go (`abi::USE_HOLD`), as the server sends them.
pub(crate) fn endless_use(ticks: f32) -> bool {
    ticks >= (weft_abi::USE_HOLD >> 1) as f32
}

/// `v` turned about the up axis by `yaw` radians, as models turn with their yaw.
fn turn_y(v: DVec3, yaw: f32) -> DVec3 {
    glam::DQuat::from_rotation_y(yaw as f64) * v
}

/// Vanilla's swing starts over once past halfway.
fn restart_swing(swing: &mut Option<f32>) {
    if swing.is_none_or(|p| p >= 0.5) {
        *swing = Some(0.0);
    }
}

/// An arm's swing goes on, 0.3 seconds from start to end.
fn advance_swing(swing: &mut Option<f32>, dt: f32) {
    *swing = swing.map(|p| p + dt / 0.3).filter(|&p| p < 1.0);
}

use crate::physics::angle_diff;

/// One 20 Hz tick of player movement with vanilla's numbers (blocks per tick): acceleration
/// scaled by ground slipperiness, 0.91 air drag, 0.98 vertical drag, 0.08 gravity, 0.42 jump,
/// so jumps, sprint-jumps and parkur distances match.
fn step_player(p: &mut LocalPlayer, input: &Input, block: &impl physics::Cells, gravity: f64, bodies: &[physics::Body]) {
    let c = content();
    // Held facing one way it stays put: in a bed.
    if let Some((pose, Some(yaw))) = p.forced {
        (p.yaw, p.pitch, p.vel, p.pose) = (yaw, 0.0, DVec3::ZERO, pose);
        p.eye = p.dims().eye;
        return;
    }
    // The body the player rides moves it first, like on the server.
    let hw = p.dims().half_width;
    if !p.noclip {
        let h = p.dims().height;
        let moved_by = physics::carried(c, block, &mut p.pos, hw, h, p.on_ground, &mut p.support, bodies, 0);
        if let Some(d) = moved_by.released {
            p.vel += d;
        }
    } else {
        p.support = None;
    }
    let mut fwd_in = 0.0f64;
    let mut side_in = 0.0f64;
    if input.held("core:forward") {
        fwd_in += 1.0;
    }
    if input.held("core:back") {
        fwd_in -= 1.0;
    }
    if input.held("core:left") {
        side_in -= 1.0;
    }
    if input.held("core:right") {
        side_in += 1.0;
    }
    let sneak = input.held("core:sneak");
    let crawl = input.held("core:crawl");
    let jump = input.held("core:jump");
    // Sprinting needs forward movement and stops when walking backwards, sneaking or crawling; in
    // flight the sneak key goes down and keeps it.
    let sprint = input.held("core:sprint") && fwd_in > 0.0 && (!sneak || p.flying) && !crawl && !p.using && (p.can_sprint || p.flying);
    // A liquid it swims in, at the eyes, or at the feet once it lies down.
    let low = p.pose.def().tilt != weft_abi::pose_tilt::UPRIGHT;
    let swim_at = p.pos + DVec3::Y * if low { 0.1 } else { p.dims().eye };
    let in_swim_liquid = physics::liquid_at(c, block, swim_at).is_some_and(|l| p.swims_in.get(l as usize) == Some(&true));
    let feet_wet = physics::in_liquid(c, block, p.pos, 0.0);
    // `input`: the action held and sprinting count, which a pose fallen back to goes without.
    let holds = |d: &crate::registry::PoseDef, input_too: bool| {
        let body = crate::conditions::Body { in_liquid: in_swim_liquid, flying: p.flying || p.noclip, sprinting: sprint || !input_too };
        let ask = crate::conditions::Ask { pos: p.pos, body: Some(body), ..Default::default() };
        (!input_too || d.key.is_empty() || input.held(&d.key))
            && d.when
                .is_none_or(|w| crate::conditions::on_client(None, w, &ask))
    };
    let fits =
        |pose: Pose| p.noclip || !physics::collides(c, block, physics::Aabb::around(p.pos, hw - 1e-3, physics::posed(p.size, pose).height));
    p.pose = Pose::choose(p.forced.map(|f| f.0), holds, fits);
    // Sprinting swims in a pose for it, lying down; in a liquid it is no use otherwise.
    let swimming = sprint && in_swim_liquid && p.pose.def().tilt != weft_abi::pose_tilt::UPRIGHT;
    let sprint = sprint && (swimming || !feet_wet);
    p.sprinting = sprint;
    if !p.flying {
        let speed = p.pose.def().speed as f64;
        fwd_in *= speed;
        side_in *= speed;
    }
    // Eating and the like: modern's fifth of the speed.
    if p.using {
        fwd_in *= 0.2;
        side_in *= 0.2;
    }
    let (fwd_in, side_in) = (fwd_in * 0.98, side_in * 0.98);
    let (sy, cy) = (p.yaw as f64).sin_cos();
    let forward = DVec3::new(-sy, 0.0, -cy);
    let right = DVec3::new(cy, 0.0, -sy);
    // Vanilla's moveRelative input: normalized only when longer than 1.
    let wish = forward * fwd_in + right * side_in;
    let g = physics::gravity_per_tick(gravity) * p.moves.gravity;

    let height = p.dims().height;
    let before = p.pos;

    if p.flying || p.noclip {
        let f = physics::Flight { wish, up: jump, down: sneak, speed: p.fly_speed as f64 * p.moves.fly * if sprint { 2.0 } else { 1.0 } };
        // Flying creative players stop flying when they come down onto the ground.
        if physics::fly(c, block, bodies, 0, &mut p.pos, &mut p.vel, &mut p.on_ground, hw, height, &f, p.noclip) {
            p.flying = false;
        }
        p.landed |= p.on_ground;
        let here = physics::around(c, block, p.pos, height);
        (p.in_liquid, p.climbing) = (here.in_liquid, here.climbing);
        return;
    }

    // Walking is the same step mobs take; the player only brings its input.
    p.jump_cooldown = p.jump_cooldown.saturating_sub(1);
    if !jump {
        p.jump_cooldown = 0;
    }
    let ctl = physics::Control {
        wish,
        ground: if sprint { 0.13 } else { 0.1 } * p.speed_mul,
        air: if sprint { 0.026 } else { 0.02 },
        liquid: physics::SWIM * p.moves.swim,
        jump: if jump && p.jump_cooldown == 0 { physics::JUMP * p.jump_mul } else { 0.0 },
        jump_boost: if sprint { forward * 0.2 } else { DVec3::ZERO },
        swim: jump,
        careful: sneak,
        sprint: swimming,
        swim_look: swimming.then(|| (p.pitch as f64).sin()),
    };
    let frame = physics::Frame { half_width: hw, height, gravity: g, moves: p.moves };
    let w = physics::walk(c, block, bodies, 0, &mut p.pos, &mut p.vel, p.on_ground, &ctl, &frame);
    if w.jumped {
        p.jump_cooldown = 10;
        p.jumped = true;
    }
    let here = physics::around(c, block, p.pos, height);
    (p.in_liquid, p.climbing) = (here.in_liquid, here.climbing);
    if w.on_ground {
        if let Some(b) = physics::standing_on(c, p.pos, hw, bodies, 0) {
            p.support = Some(b);
        }
    }
    p.on_ground = w.on_ground || (p.pos == before && p.on_ground);
    p.landed |= p.on_ground;
}
