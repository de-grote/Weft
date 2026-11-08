//! Authoritative simulation. Runs on its own thread; single player talks to it through the
//! in-process transport. Each dimension is a set of regions ticked in parallel.

pub(crate) mod actions;
pub mod area;
mod builtin;
pub mod dim;
pub mod energy;
pub mod entity;
pub(crate) mod extend;
pub mod forced;
pub mod holder;
mod liquid;
mod load;
pub mod lod_tops;
pub mod path;
mod player_file;
pub mod signal;
pub mod space;
mod sync;
pub use builtin::COMMANDS as BUILTIN_COMMANDS;
pub use extend::{JUMP_STRENGTH, MAX_HEALTH, MOVEMENT_SPEED, explanation};
use load::send_far;
use player_file::save_player;
pub mod menu;
pub mod persist;
pub mod regions;
pub mod riding;
pub mod structure;
pub mod terrain;
pub mod visibility;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use glam::{DVec3, IVec3};
use parking_lot::Mutex;
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use weft_abi::{self as abi, ItemStack, component_flags as cf};

pub use dim::{Dim, DimEvent, mining_rate};

use crate::commands::{Origin, Selector};
use crate::content::{Content, content};
use crate::net::{EntityView, ServerConn, ToClient, ToServer};
use crate::saves::Storage;
use crate::ui::text::{Arg, Msg};
use crate::world::{Cube, GameMode, View, cube_of};
use entity::Entity;
use menu::OpenMenu;
use regions::DimSet;

pub use crate::world::TPS;
const AUTOSAVE_TICKS: u64 = TPS as u64 * 30;
/// What the server accepts past a player's reach: room for a turn on the way there, and for what
/// moved meanwhile.
pub(crate) const BLOCK_SLACK: f64 = 1.5;
const ENTITY_SLACK: f64 = 3.0;
/// Players further away than vanilla's particle distance get no particles.
const PARTICLE_RANGE: f64 = 32.0;

#[derive(Clone)]
pub struct ServerConfig {
    pub seed: u64,
    /// View radius in cubes, sideways and up and down alike.
    pub view_radius: i32,
    /// `--server`: nobody plays on this machine, so nobody owns the world by connecting first.
    pub dedicated: bool,
    /// Who may join: keys, one a line. No file, anyone.
    pub whitelist: Option<std::path::PathBuf>,
}

pub struct Player {
    /// What `/title times` set for the next ones.
    title_times: Option<[f32; 3]>,
    pub name: String,
    /// The skin this player sent, passed on to everyone who joins.
    skin: Option<Arc<Vec<u8>>>,
    /// The same for their own model.
    avatar: Option<Arc<crate::net::AvatarData>>,
    /// Public key: the identity saves are keyed by.
    pub key: String,
    conn: ServerConn,
    pub dim: u32,
    pub entity: u64,
    sent: FxHashSet<IVec3>,
    /// Cubes told about in `ToClient::Saved`, and the ones of them still to send, nearest last.
    announced: FxHashSet<IVec3>,
    to_send_far: Vec<IVec3>,
    known: FxHashSet<u64>,
    joined: bool,
    /// The item it holds the use button on, the server tick that takes effect, the slot it is in.
    using: Option<(u32, u64, u32)>,
    mining: Option<Mining>,
    sent_cracks: Vec<crate::net::Crack>,
    /// The tick rate its client has.
    sent_rate: u32,
    /// Its client has what bodies' lights give the cells of this dimension.
    glow_sent: bool,
    menu: Option<OpenMenu>,
    window: u32,
    /// The frame its last move was in and where in that: walking and falls are measured there.
    last_place: (Option<u64>, DVec3),
    /// Times it died in this world, for `core:lives`.
    pub deaths: u32,

    first_sync: bool,
    /// View (dim, center, radius, height) whose cubes were all present at the last check, and the
    /// same plus the settled version when nothing was left to send. Lets idle ticks skip scanning
    /// the whole view volume.
    gen_done: Option<(u32, View, u64)>,
    /// What the player can see; only these cubes (and the sky column) get loaded.
    vis: Option<visibility::Visibility>,
    /// Settled cubes in range not sent yet (waiting for their neighbors), and the range they
    /// were gathered for: only cubes newly settled or newly in range are looked at.
    unsent: FxHashSet<IVec3>,
    sync_range: Option<View>,
    /// Set by teleports: moves sent before the client saw the teleport are stale and would read
    /// as a long fall, so they are ignored until one arrives near this point.
    teleported: Option<DVec3>,
    /// Gizmo groups the client shows.
    gizmos: Vec<String>,
    /// Walking speed and jump multipliers the client has (attributes).
    sent_attributes: Vec<f32>,
    /// The view distance the client has (`ToServer::View`).
    view_radius: i32,
}

/// What a player is mining, and how far along by what it held and where it was each tick.
#[derive(Clone, Copy)]
pub(crate) struct Mining {
    frame: u64,
    pos: IVec3,
    part: u32,
    progress: f32,
}

pub struct Generated {
    dim: u32,
    pos: IVec3,
    /// `None`: dropped unmade, nobody's view reached it any more when its turn came.
    cube: Option<Cube>,
    entities: Vec<u8>,
    conn: u16,
}

/// Cubes around a player that are loaded whether visible or not, so digging and walking into
/// sealed ground always finds blocks there.
pub const NEAR: i32 = 2;

/// Saved cubes beyond what is loaded sent to a player per tick (each read from disk).
const FAR_PER_TICK: usize = 256;

/// State the client thread may read or poke while the server runs.
#[derive(Default)]
pub struct Shared {
    pub paused: AtomicBool,
    pub hosting: AtomicBool,
    pub tick_us: AtomicU64,
    pub regions: AtomicU64,
    /// Load pipeline counters for profiling: cubes generating or waiting for insertion, cubes a
    /// player should get but has not yet, and work done so far.
    pub pending: AtomicU64,
    pub unsent: AtomicU64,
    pub generated: AtomicU64,
    pub gen_ns: AtomicU64,
    pub inserted: AtomicU64,
    pub insert_ns: AtomicU64,
    /// Last tick split into `PHASES`, microseconds.
    pub phase_us: [AtomicU64; 6],
    /// Longest tick since a reader last swapped it out.
    pub tick_max_us: AtomicU64,
    /// Loaded cubes and live entities over all dimensions, refreshed every 10 ticks.
    pub cubes: AtomicU64,
    pub entities: AtomicU64,
    /// Where players load cubes (dim, center, radius, up, down), for generation jobs to skip
    /// cubes a fast player already left behind.
    pub views: parking_lot::RwLock<Vec<(u32, View)>>,
    /// Boxes of cubes tickets keep loaded (dim, lowest, highest cube), for the same.
    pub forced: parking_lot::RwLock<Vec<(u32, IVec3, IVec3)>>,
}

pub const PHASES: [&str; 6] = ["insert", "messages", "regions", "request", "sync", "rest"];

/// Cubes sent to one player per tick; a client meshes about this many per frame anyway.
const SEND_PER_TICK: usize = 8192;

pub struct Server {
    pub cfg: ServerConfig,
    pub dims: Vec<DimSet>,
    pub players: Vec<Player>,
    pub tick: u64,
    /// `/tick`: ticks per second, the world held still (players still move), ticks left to step
    /// while frozen, and a sprint (ticks left, ticks in all, started) run without waiting.
    pub tick_rate: u32,
    /// The tick mining last went on at: its sync also runs between ticks.
    mining_tick: u64,
    pub frozen: bool,
    pub steps: u32,
    pub sprint: Option<(u32, u32, Instant)>,
    gen_tx: Sender<Generated>,
    gen_rx: Receiver<Generated>,
    incoming: Receiver<ServerConn>,
    /// Generated cubes waiting for insertion (lighting is budgeted per tick).
    backlog: std::collections::VecDeque<Generated>,
    /// Cubes some player still needs, as of the last request: a cube whose upper neighbor is
    /// among them waits for it, or it would be lit dark and relit when the sky arrives.
    wanted: FxHashSet<(u32, IVec3)>,
    storage: Option<Arc<Mutex<Storage>>>,
    shared: Arc<Shared>,
    next_id: Arc<AtomicU64>,
    /// Names last sent to clients as the online list.
    online: Vec<String>,
    /// Generated and inserted totals at the last `load` report.
    load_seen: (u64, u64),
    /// World rule values by rule id.
    /// Synced config values as players last got them.
    configs: Vec<(u32, f64)>,
    /// Per dimension, cubes kept loaded with no player near (`forced`), by ticket key.
    forced: Vec<forced::Tickets>,
}

/// Handle the client side uses to reach a running server thread.
pub struct ServerHandle {
    pub connect: Sender<ServerConn>,
    /// Lines typed into a dedicated server.
    pub console: Sender<String>,
    pub shared: Arc<Shared>,
    stop: Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ServerHandle {
    pub fn stop(&mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    pub fn set_paused(&self, p: bool) {
        self.shared.paused.store(p, Ordering::Relaxed);
    }

    pub fn is_hosting(&self) -> bool {
        self.shared.hosting.load(Ordering::Relaxed)
    }

    pub fn tick_ms(&self) -> f32 {
        self.shared.tick_us.load(Ordering::Relaxed) as f32 / 1000.0
    }

    /// Nothing left to generate, insert or send.
    pub fn idle(&self) -> bool {
        self.shared.pending.load(Ordering::Relaxed) == 0 && self.shared.unsent.load(Ordering::Relaxed) == 0
    }

    /// Cubes generated and their average cost on a worker, cubes inserted and their average
    /// cost (lighting) on the server thread, in microseconds.
    pub fn load_stats(&self) -> (u64, f64, u64, f64) {
        let s = &self.shared;
        let (g, i) = (s.generated.load(Ordering::Relaxed), s.inserted.load(Ordering::Relaxed));
        let avg = |ns: u64, n: u64| ns as f64 / n.max(1) as f64 / 1000.0;
        (g, avg(s.gen_ns.load(Ordering::Relaxed), g), i, avg(s.insert_ns.load(Ordering::Relaxed), i))
    }

    pub fn phases(&self) -> [f32; 6] {
        std::array::from_fn(|i| self.shared.phase_us[i].load(Ordering::Relaxed) as f32 / 1000.0)
    }

    pub fn regions(&self) -> u32 {
        self.shared.regions.load(Ordering::Relaxed) as u32
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn spawn(cfg: ServerConfig, storage: Option<Storage>) -> ServerHandle {
    let (connect, incoming) = unbounded();
    let (stop, stop_rx) = unbounded::<()>();
    let (console, console_rx) = unbounded::<String>();
    let shared = Arc::new(Shared::default());
    let sh = shared.clone();
    let thread = std::thread::Builder::new()
        .name("server".into())
        .spawn(move || {
            let mut server = Server::new(cfg, incoming, storage, sh);
            let mut next = Instant::now();
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                while let Ok(line) = console_rx.try_recv() {
                    server.console(&line);
                }
                if server.shared.paused.load(Ordering::Relaxed) {
                    server.idle();
                } else {
                    server.tick();
                }
                if server.sprint.is_some() {
                    next = Instant::now();
                    continue;
                }
                let step = Duration::from_secs_f64(1.0 / server.tick_rate.max(1) as f64);
                next += step;
                // The rest of the tick goes to loading instead of sleeping.
                server.load_until(next.checked_sub(Duration::from_millis(2)).unwrap_or(next));
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else if now - next > step * 10 {
                    // Far behind (debugger, huge load): skip instead of fast-forwarding.
                    next = now;
                }
            }
            server.shutdown();
        })
        .expect("spawning server thread");
    ServerHandle { connect, console, shared, stop, thread: Some(thread) }
}

impl Server {
    /// A line typed into a dedicated server. The answers go to the log, which is what it shows.
    fn console(&mut self, line: &str) {
        let mut words = line.split_whitespace();
        let command = words.next().unwrap_or("");
        let name = words.next().unwrap_or("");
        let find = |s: &Self| s.players.iter().position(|p| p.joined && p.name == name);
        match command {
            "" => {}
            "list" => {
                let names: Vec<&str> = self
                    .players
                    .iter()
                    .filter(|p| p.joined)
                    .map(|p| p.name.as_str())
                    .collect();
                log::info!("{} online: {}", names.len(), names.join(", "));
            }
            "say" => {
                let text = line
                    .trim_start()
                    .strip_prefix("say")
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let msg = Msg::new("chat.message", vec![Arg::Text("Server".into()), Arg::Text(text)]);
                for p in &mut self.players {
                    p.conn
                        .send(ToClient::Show(crate::net::Shown::chat(msg.clone())));
                }
            }
            "op" | "deop" => match find(self) {
                Some(pi) => {
                    let level = if command == "deop" {
                        abi::permission::EVERYONE
                    } else {
                        words
                            .next()
                            .and_then(|l| l.parse().ok())
                            .unwrap_or(abi::permission::GAME_MASTER)
                            .min(abi::permission::OWNER)
                    };
                    self.set_permission(pi, level);
                    log::info!("{name} now has permission level {level}");
                }
                None => log::info!("{name:?} is not online"),
            },
            _ => log::info!("commands here: stop, list, say <text>, op <player> [level], deop <player>"),
        }
    }

    pub fn new(cfg: ServerConfig, incoming: Receiver<ServerConn>, storage: Option<Storage>, shared: Arc<Shared>) -> Server {
        let c = content();
        let next_id = Arc::new(AtomicU64::new(storage.as_ref().map_or(1, |s| s.meta.next_entity.max(1))));
        let mut dims: Vec<DimSet> = (0..c.dims.len() as u32)
            .map(|i| DimSet::new(i, cfg.seed, next_id.clone()))
            .collect();
        if let Some(s) = &storage {
            for (name, t) in &s.meta.times {
                if let Some(&i) = c.dim_names.get(name) {
                    dims[i as usize].set_time(*t);
                }
            }
            for (name, weather, until) in &s.meta.weather {
                if let (Some(&i), Some(&w)) = (c.dim_names.get(name), c.game.weather_names.get(weather)) {
                    let d = &mut dims[i as usize];
                    d.set_weather(w as u16, 0);
                    d.weather_until = *until;
                }
            }
            for (name, o) in &s.meta.day_offsets {
                if let Some(&i) = c.dim_names.get(name) {
                    dims[i as usize].day_offset = *o;
                }
            }
            for (name, spec) in &s.meta.edits {
                let (Some(&i), Some(fill)) = (c.dim_names.get(name), area::AreaFill::parse(c, spec)) else { continue };
                Arc::make_mut(&mut dims[i as usize].edits).push(fill);
            }
        }
        let (gen_tx, gen_rx) = unbounded();
        crate::config::load_world(storage.as_ref().map_or(&[][..], |s| &s.meta.configs));
        let forced_specs = storage
            .as_ref()
            .map_or(Vec::new(), |s| s.meta.forced.clone());
        let mut server = Server {
            cfg,
            dims,
            players: Vec::new(),
            tick: 0,
            gen_tx,
            gen_rx,
            incoming,
            backlog: Default::default(),
            wanted: FxHashSet::default(),
            storage: storage.map(|s| Arc::new(Mutex::new(s))),
            shared,
            next_id,
            online: Vec::new(),
            tick_rate: TPS,
            mining_tick: 0,
            frozen: false,
            steps: 0,
            sprint: None,
            load_seen: (0, 0),
            configs: crate::config::synced(),
            forced: vec![Default::default(); c.dims.len()],
        };
        server.read_forced(c, &forced_specs);
        server
    }

    /// Synced settings the host changed since the last tick, to everyone.
    fn send_configs(&mut self) {
        let now = crate::config::synced();
        if now == self.configs {
            return;
        }
        let changed: Vec<(u32, f64)> = now
            .iter()
            .filter(|v| !self.configs.contains(v))
            .copied()
            .collect();
        for p in &mut self.players {
            p.conn.send(ToClient::Configs(changed.clone()));
        }
        self.configs = now;
    }

    /// Paused single player: keep talking to the client but do not advance the world.
    fn idle(&mut self) {
        let c = content();
        self.accept();
        self.receive_generated(c, Instant::now() + Duration::from_millis(15));
        self.handle_messages(c);
        self.sync_players(c);
    }

    fn shutdown(&mut self) {
        // What the players did last (the `Leave` of who closed the game), and everyone's cursor
        // and crafting grid back in their inventory before it is saved.
        let c = content();
        self.handle_messages(c);
        let Server { players, dims, .. } = self;
        for p in players.iter_mut().filter(|p| p.joined) {
            if let (Some(m), Some(r)) = (p.menu.take(), dims[p.dim as usize].entity_region(p.entity)) {
                menu::close(m, r, c, p.entity);
            }
        }
        for p in &mut self.players {
            p.conn.send(ToClient::Disconnect("server closed".into()));
        }
        self.save_all(true);
    }

    fn accept(&mut self) {
        while let Ok(conn) = self.incoming.try_recv() {
            self.players.push(Player {
                title_times: None,
                skin: None,
                avatar: None,
                name: String::new(),
                key: String::new(),
                conn,
                dim: 0,
                entity: 0,
                sent: FxHashSet::default(),
                announced: FxHashSet::default(),
                to_send_far: Vec::new(),
                known: FxHashSet::default(),
                joined: false,
                using: None,
                mining: None,
                sent_rate: 0,
                glow_sent: false,
                sent_cracks: Vec::new(),
                menu: None,
                window: 0,
                last_place: (None, DVec3::ZERO),
                deaths: 0,

                first_sync: true,
                gen_done: None,
                unsent: FxHashSet::default(),
                sync_range: None,
                vis: None,
                teleported: None,
                gizmos: Vec::new(),
                sent_attributes: Vec::new(),
                view_radius: self.cfg.view_radius,
            });
        }
    }

    pub fn tick(&mut self) {
        let start = Instant::now();
        let c = content();
        let mut phase = Instant::now();
        let mut lap = |i: usize, shared: &Shared| {
            let now = Instant::now();
            shared.phase_us[i].store((now - phase).as_micros() as u64, Ordering::Relaxed);
            phase = now;
        };
        self.accept();
        self.receive_generated(c, start + Duration::from_millis(15));
        lap(0, &self.shared);
        self.handle_messages(c);
        self.send_configs();
        if self.tick % 20 == 0 {
            self.send_lod_changes();
        }
        lap(1, &self.shared);

        // Every region of every dimension is independent: tick them all in parallel. A frozen
        // world only runs the ticks it is stepped.
        let runs = !self.frozen || self.steps > 0 || self.sprint.is_some();
        self.steps -= (self.frozen && self.steps > 0) as u32;
        for d in self.dims.iter_mut().filter(|_| runs) {
            let mut groups: Vec<String> = self
                .players
                .iter()
                .filter(|p| p.dim == d.id)
                .flat_map(|p| p.gizmos.iter().cloned())
                .collect();
            groups.sort();
            groups.dedup();
            let groups: Arc<[String]> = groups.into();
            let t = d.time;
            for r in &mut d.regions {
                r.time = t;
                r.gizmo_groups = groups.clone();
            }
            d.time += 1;
            if c.dims[d.id as usize].has_weather() {
                d.tick_weather(c);
            }
        }
        if runs {
            self.dims
                .par_iter_mut()
                .flat_map(|d| d.regions.par_iter_mut())
                .for_each(|r| r.tick(c));
        }
        lap(2, &self.shared);
        if let Some((left, all, start)) = &mut self.sprint {
            *left -= 1;
            if *left == 0 {
                let note = Msg::new("command.tick.sprinted", vec![Arg::Int(*all as i64), Arg::Int(start.elapsed().as_millis() as i64)]);
                for p in &mut self.players {
                    p.conn
                        .send(ToClient::Show(crate::net::Shown::chat(note.clone())));
                }
                self.sprint = None;
            }
        }

        self.finish_uses(c);
        self.dim_events(c);
        for pi in 0..self.players.len() {
            let Server { players, dims, .. } = self;
            let p = &mut players[pi];
            let Some(r) = dims[p.dim as usize].entity_region(p.entity) else { continue };
            if p.menu
                .as_ref()
                .is_some_and(|m| menu::out_of_reach(m, r, p.entity))
            {
                if let Some(m) = p.menu.take() {
                    menu::close(m, r, c, p.entity);
                    p.conn.send(ToClient::CloseMenu);
                }
            }
            if let Some(m) = &mut p.menu {
                menu::tick(m, r, c, p.entity);
            }
        }
        self.request_generation(c);
        lap(3, &self.shared);
        self.sync_players(c);
        lap(4, &self.shared);
        self.players.retain(|p| !p.conn.is_closed());
        let online: Vec<String> = self
            .players
            .iter()
            .filter(|p| p.joined)
            .map(|p| p.name.clone())
            .collect();
        if online != self.online {
            for p in self.players.iter_mut().filter(|p| p.joined) {
                p.conn.send(ToClient::Players(online.clone()));
            }
            self.online = online;
        }
        if self.tick % 10 == 0 {
            for d in &mut self.dims {
                if d.dirty || self.tick % 100 == 0 {
                    d.regionize(c);
                }
            }
            let n: usize = self.dims.iter().map(|d| d.regions.len()).sum();
            self.shared.regions.store(n as u64, Ordering::Relaxed);
            let regions = || self.dims.iter().flat_map(|d| d.regions.iter());
            self.shared
                .cubes
                .store(regions().map(|r| r.space.cubes.len() as u64).sum(), Ordering::Relaxed);
            self.shared.entities.store(
                regions()
                    .map(|r| r.entities.list.iter().filter(|e| !e.removed).count() as u64)
                    .sum(),
                Ordering::Relaxed,
            );
        }
        if self.tick % TPS as u64 == 0 && crate::log::on(crate::log::Channel::Load, log::Level::Debug) {
            self.report_load();
        }
        self.tick += 1;
        if self.tick % AUTOSAVE_TICKS == 0 {
            self.save_all(false);
        }
        lap(5, &self.shared);
        let us = start.elapsed().as_micros() as u64;
        self.shared.tick_us.store(us, Ordering::Relaxed);
        self.shared.tick_max_us.fetch_max(us, Ordering::Relaxed);
    }

    /// Whatever a player's or mod's action changed takes effect at once, wherever it reached:
    /// neighbor updates, events to their handlers, what regions report to players.
    fn settle(&mut self, c: &'static Content) {
        for d in &mut self.dims {
            for r in &mut d.regions {
                r.settle(c);
            }
        }
        self.dim_events(c);
    }

    fn dim_events(&mut self, c: &'static Content) {
        let mut events = Vec::new();
        for d in &mut self.dims {
            for r in &mut d.regions {
                events.append(&mut r.events);
            }
        }
        if events.is_empty() {
            return;
        }
        // Particles and sounds of the whole tick go out as one message of each per player.
        let mut bursts: Vec<Vec<abi::ParticleBurst>> = vec![Vec::new(); self.players.len()];
        let mut sounds: Vec<Vec<(u32, DVec3, f32, f32)>> = vec![Vec::new(); self.players.len()];
        for ev in events {
            match ev {
                DimEvent::Show { who, shown } => self.show(who, shown),
                DimEvent::PlayAction { id, name, looping } => {
                    for p in self
                        .players
                        .iter_mut()
                        .filter(|p| p.joined && (p.known.contains(&id) || p.entity == id))
                    {
                        p.conn
                            .send(ToClient::PlayAction(vec![(id, name.clone(), looping)]));
                    }
                }
                DimEvent::Sound { dim, sound, pos: at, volume, pitch } => {
                    let range = c
                        .game
                        .sounds
                        .get(sound as usize)
                        .map_or(16.0, |d| d.range * volume.max(1.0)) as f64;
                    for (pi, p) in self.players.iter().enumerate() {
                        let pos = self.dims[p.dim as usize].entity(p.entity).map(|e| e.pos);
                        if p.joined && p.dim == dim && pos.is_some_and(|pos| (pos - at).length_squared() < range * range) {
                            sounds[pi].push((sound, at, volume, pitch));
                        }
                    }
                }
                DimEvent::Particles { dim, burst } => {
                    let at = crate::world::dvec(burst.pos);
                    for (pi, p) in self.players.iter().enumerate() {
                        let pos = self.dims[p.dim as usize].entity(p.entity).map(|e| e.pos);
                        if p.joined && p.dim == dim && pos.is_some_and(|pos| (pos - at).length_squared() < PARTICLE_RANGE * PARTICLE_RANGE)
                        {
                            bursts[pi].push(burst);
                        }
                    }
                }
                DimEvent::OpenMenu { player, menu, on, block } => {
                    if let Some(pi) = self.players.iter().position(|p| p.entity == player) {
                        self.open_menu(c, pi, menu, on, block);
                    }
                }
                DimEvent::Died { player, cause, killer } => {
                    let Server { players, dims, .. } = self;
                    if let Some(p) = players.iter_mut().find(|p| p.entity == player) {
                        // The menu first: what is on the cursor and in the crafting grid goes
                        // back to the inventory, and falls with the rest.
                        if let Some(r) = dims[p.dim as usize].entity_region(player) {
                            if let Some(m) = p.menu.take() {
                                menu::close(m, r, c, player);
                                p.conn.send(ToClient::CloseMenu);
                            }
                            // check keep_inventory rule
                            // if not enabled, drop inventory on the ground
                            let keep_inventory = crate::config::get(c.game.keep_inventory) != 0.0;
                            if !keep_inventory {
                                r.drop_inventory(c, player);
                            }
                        }
                        p.deaths += 1;
                        p.conn.send(ToClient::Died);
                        log::info!("{} died", p.name);
                        let line = death_message(c, cause, killer, p.name.clone());
                        for q in players.iter_mut().filter(|q| q.joined) {
                            q.conn
                                .send(ToClient::Show(crate::net::Shown::chat(line.clone())));
                        }
                    }
                }
                DimEvent::SetPermission { player, level } => {
                    if let Some(pi) = self
                        .players
                        .iter()
                        .position(|p| p.joined && p.entity == player)
                    {
                        self.set_permission(pi, level);
                    }
                }
                DimEvent::SetWeather { dim, weather, ticks } => self.dims[dim as usize].set_weather(weather, ticks as u64),

                DimEvent::EntitySwing { id, off } => {
                    for p in self
                        .players
                        .iter_mut()
                        .filter(|p| p.joined && p.entity != id && p.known.contains(&id))
                    {
                        p.conn.send(ToClient::EntitySwing { id, off });
                    }
                }
                DimEvent::ForceLoad { dim, key, min, max, mode, ticks } => self.force_load(dim, key, min, max, mode, ticks),
                DimEvent::Camera { player, mode, locked, transition, easing } => {
                    if let Some(p) = self
                        .players
                        .iter_mut()
                        .find(|p| p.joined && p.entity == player)
                    {
                        p.conn
                            .send(ToClient::Camera { mode, locked, transition, easing });
                    }
                }
                DimEvent::StopSprint { player } => {
                    if let Some(p) = self
                        .players
                        .iter_mut()
                        .find(|p| p.joined && p.entity == player)
                    {
                        p.conn.send(ToClient::StopSprint);
                    }
                }
                DimEvent::Packet { player, channel, data } => {
                    if let Some(p) = self
                        .players
                        .iter_mut()
                        .find(|p| p.joined && p.entity == player)
                    {
                        p.conn.send(ToClient::Packet { channel, data });
                    }
                }
                DimEvent::Teleport { from, id, dim, pos } => {
                    let to = if dim == u32::MAX { from } else { dim };
                    if (to as usize) < self.dims.len() && pos.is_finite() {
                        self.teleport(c, from, id, to, pos);
                    }
                }
                DimEvent::SetTime { dim, time } => self.dims[dim as usize].set_day_time(time),
                DimEvent::SkipTicks { dim, ticks } => {
                    let d = &mut self.dims[dim as usize];
                    d.set_time(d.time.saturating_add(ticks));
                }
                DimEvent::AreaFill { dim, fill } => self.area_fill(c, dim, fill),
                DimEvent::SetMode { player, mode } => {
                    if let Some(pi) = self.player_index(player) {
                        self.set_mode(pi, mode);
                    }
                }
            }
        }
        for ((p, list), sounds) in self.players.iter_mut().zip(bursts).zip(sounds) {
            if !list.is_empty() {
                p.conn.send(ToClient::Particles(list));
            }
            if !sounds.is_empty() {
                p.conn.send(ToClient::Sounds(sounds));
            }
        }
    }

    fn open_menu(&mut self, c: &'static Content, pi: usize, kind: u32, on: Option<holder::Inv>, block: u32) {
        if kind as usize >= c.game.menus.len() {
            return;
        }
        let Server { players, dims, .. } = self;
        let p = &mut players[pi];
        let Some(r) = dims[p.dim as usize].entity_region(p.entity) else { return };
        if let Some(m) = p.menu.take() {
            menu::close(m, r, c, p.entity);
        }
        p.window += 1;
        let mut m = menu::open(r, c, p.entity, kind, on);
        m.on_block = block != 0;
        let owner = m.owner();
        // The title: the owner's own name (the held stack's), else the block it stands on.
        let name = match on.map(|inv| inv.at) {
            Some(holder::At::Stack { entity, slot }) => {
                crate::stack::name(&holder::get(r, c, holder::Inv::own(holder::At::Entity(entity)), slot)).unwrap_or_default()
            }
            _ => r
                .entities
                .comps
                .get(&(owner, c.game.name_comp))
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default(),
        };
        let block = if block != 0 {
            block
        } else {
            r.entities
                .get(owner)
                .and_then(|e| e.anchor)
                .and_then(|a| r.get(a))
                .unwrap_or(0)
        };
        p.conn
            .send(ToClient::OpenMenu { kind, window: p.window, slots: m.slots.len() as u32, owner, block, name });
        p.menu = Some(m);
    }

    /// Every column of `dim` distant terrain shows other than the terrain says, to player `pi`.
    pub(super) fn send_lod_columns(&mut self, pi: usize, dim: u32) {
        if let Some(s) = &self.storage {
            let cols = s.lock().lod[dim as usize]
                .columns
                .values()
                .cloned()
                .collect();
            self.players[pi]
                .conn
                .send(ToClient::LodColumns { dim, all: true, cols });
        }
    }

    /// Columns whose distant top changed as cubes were saved, to the players in their dimension.
    fn send_lod_changes(&mut self) {
        let Some(s) = &self.storage else { return };
        let changed: Vec<(u32, Vec<lod_tops::LodColumn>)> = s
            .lock()
            .lod
            .iter_mut()
            .enumerate()
            .map(|(d, t)| (d as u32, t.take_changed()))
            .filter(|(_, c)| !c.is_empty())
            .collect();
        for (dim, cols) in changed {
            for p in self.players.iter_mut().filter(|p| p.joined && p.dim == dim) {
                p.conn
                    .send(ToClient::LodColumns { dim, all: false, cols: cols.clone() });
            }
        }
    }

    fn save_all(&mut self, players: bool) {
        for r in self.dims.iter_mut().flat_map(|d| d.regions.iter_mut()) {
            r.finish_moving(crate::content::content());
        }
        let Some(storage) = self.storage.clone() else { return };
        let c = content();
        let t = Instant::now();
        let mut s = storage.lock();
        let mut n = 0;
        for d in &mut self.dims {
            for r in &mut d.regions {
                // Cubes where something happened, and the ones entities are in (they move and
                // spawn in untouched cubes too).
                let mut cubes: Vec<IVec3> = std::mem::take(&mut r.space.modified).into_iter().collect();
                cubes.extend(r.on_disk.iter().copied());
                cubes.extend(
                    r.entities
                        .list
                        .iter()
                        .filter(|e| e.saved_with_cube())
                        .map(|e| cube_of(e.pos.floor().as_ivec3())),
                );
                cubes.sort_unstable_by_key(|p| (p.x, p.y, p.z));
                cubes.dedup();
                for pos in cubes {
                    if let Some(cube) = r.space.cubes.get(&pos) {
                        let mut bytes = entities_in(c, r, pos);
                        let ticks = r.ticks_in(pos);
                        let holds = r
                            .entities
                            .list
                            .iter()
                            .any(|e| e.saved_with_cube() && cube_of(e.pos.floor().as_ivec3()) == pos);
                        persist::append_ticks(&mut bytes, &ticks);
                        s.save(c, r.id, pos, cube, &r.around(pos, |_| None), &bytes);
                        if holds || !ticks.is_empty() {
                            r.on_disk.insert(pos);
                        } else {
                            r.on_disk.remove(&pos);
                        }
                        n += 1;
                    }
                }
            }
        }
        s.meta.times = self
            .dims
            .iter()
            .map(|d| (c.dims[d.id as usize].name.clone(), d.time))
            .collect();
        s.meta.day_offsets = self
            .dims
            .iter()
            .filter(|d| d.day_offset != 0)
            .map(|d| (c.dims[d.id as usize].name.clone(), d.day_offset))
            .collect();
        s.meta.weather = self
            .dims
            .iter()
            .filter(|d| d.weather_until != 0)
            .map(|d| (c.dims[d.id as usize].name.clone(), c.game.weathers[d.weather as usize].name.clone(), d.weather_until))
            .collect();
        s.meta.edits = self
            .dims
            .iter()
            .flat_map(|d| {
                d.edits
                    .iter()
                    .map(|e| (c.dims[d.id as usize].name.clone(), e.spec(c)))
            })
            .collect();
        s.meta.configs = crate::config::world_values();
        s.meta.forced = self.forced_specs(c);
        s.meta.next_entity = self.next_id.load(Ordering::Relaxed);
        for p in &mut self.players {
            if p.joined || players {
                if let Some(r) = self.dims[p.dim as usize].entity_region_ref(p.entity) {
                    save_player(&s, p, r);
                }
            }
        }
        s.flush();
        if n > 0 {
            log::info!("saved {n} cubes in {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        }
    }

    fn set_mode(&mut self, pi: usize, mode: GameMode) {
        let (dim, entity) = (self.players[pi].dim, self.players[pi].entity);
        if let Some(r) = self.dims[dim as usize].entity_region(entity) {
            if let Some(e) = r.entities.get_mut(entity) {
                e.mode = mode;
            }
            r.keep_sources(content(), entity);
        }
        self.players[pi].conn.send(ToClient::GameMode(mode));
    }

    pub(crate) fn permission_of(&self, pi: usize) -> u32 {
        let p = &self.players[pi];
        self.dims[p.dim as usize]
            .entity(p.entity)
            .map_or(0, |e| e.permission)
    }

    /// One way to change what a player may do: known to its entity (mods ask there), to its
    /// client (what to offer), and to the world's file by the player's key.
    pub(crate) fn set_permission(&mut self, pi: usize, level: u32) {
        let Server { players, dims, storage, .. } = self;
        let p = &mut players[pi];
        if let Some(e) = dims[p.dim as usize]
            .entity_region(p.entity)
            .and_then(|r| r.entities.get_mut(p.entity))
        {
            e.permission = level;
        }
        p.conn.send(ToClient::Permission(level));
        if let Some(st) = storage {
            let mut st = st.lock();
            st.meta.ops.retain(|o| o.0 != p.key);
            if level > 0 {
                st.meta.ops.push((p.key.clone(), level));
            }
        }
    }

    /// Moves a player (with inventory and components) to a position, possibly in another
    /// dimension or region, without reloading anything.
    pub fn change_dim(&mut self, c: &'static Content, pi: usize, to: u32, pos: DVec3) {
        let Server { players, dims, storage, .. } = self;
        let p = &mut players[pi];
        let from = p.dim as usize;
        if let Some(r) = dims[from].entity_region(p.entity) {
            if let Some(m) = p.menu.take() {
                menu::close(m, r, c, p.entity);
                p.conn.send(ToClient::CloseMenu);
            }
        }
        let taken = dims[from]
            .entity_region(p.entity)
            .and_then(|r| r.entities.take(p.entity));
        let mut frame = None;
        if let Some((mut e, inv, comps)) = taken {
            e.pos = pos;
            let id = e.id;
            let r = dims[to as usize].ensure_region(c, pos);
            let hw = e.dims(c).half_width;
            frame = r.body_under(c, pos, hw).map(|(b, at)| (b, pos - at));
            e.frame = frame.map(|f| f.0);
            r.entities.insert_with(c, e, inv, comps);
            // The client hears its inventory again (component changes are found by comparing).
            r.entities.dirty_inv.insert(id);
        }
        if p.dim != to {
            p.dim = to;
            p.glow_sent = false;
            p.sent.clear();
            p.announced.clear();
            p.to_send_far.clear();
            p.gen_done = None;
            p.unsent.clear();
            p.sync_range = None;
            p.vis = None;
            p.known.clear();
        }
        p.last_place = (None, pos);
        p.teleported = Some(pos);
        p.conn.send(ToClient::Teleport { dim: to, pos, frame });
        if let Some(s) = storage {
            let cols = s.lock().lod[to as usize]
                .columns
                .values()
                .cloned()
                .collect();
            players[pi]
                .conn
                .send(ToClient::LodColumns { dim: to, all: true, cols });
        }
        let p = &mut players[pi];
        p.conn
            .send(ToClient::Edits { dim: to, specs: dims[to as usize].edits.iter().map(|e| e.spec(c)).collect(), all: true });
        p.conn
            .send(ToClient::Time { dim: to, time: dims[to as usize].day_time() });
        p.conn
            .send(ToClient::Weather(dims[to as usize].weather as u32));
        if to == c.void_dim && from != to as usize {
            p.conn
                .send(ToClient::Show(crate::net::Shown::chat(void_note(c))));
        }
    }

    fn command(&mut self, c: &'static Content, pi: usize, line: &str) -> Vec<Msg> {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(&name) = tokens.first() else { return Vec::new() };
        let Some(def) = c.commands.iter().find(|d| d.name == name) else {
            return vec![Msg::new("command.unknown", vec![Arg::Text(name.to_string())])];
        };
        if def.client {
            return vec![Msg::new("command.client_only", vec![Arg::Text(name.to_string())])];
        }
        if self.permission_of(pi) < def.level {
            return vec![Msg::new("command.no_permission", vec![Arg::Text(name.to_string()), Arg::Int(def.level as i64)])];
        }
        let (dim, entity) = (self.players[pi].dim, self.players[pi].entity);
        let here = self.dims[dim as usize]
            .entity_region_ref(entity)
            .and_then(|r| r.entities.get(entity))
            .map_or_else(Origin::default, |e| Origin { pos: e.pos, yaw: e.yaw, pitch: e.pitch });
        let players: Vec<String> = self
            .players
            .iter()
            .filter(|p| p.joined)
            .map(|p| p.name.clone())
            .collect();
        let (args, handler) = match def.parse(c, &tokens[1..], here, &players) {
            Ok(a) => a,
            Err(e) => {
                return vec![Msg::new("command.error_usage", vec![Arg::Msg(e), Arg::Text(def.name.clone()), Arg::Text(def.usage.clone())])];
            }
        };
        // Selectors become entity ids before mods see them.
        let selected: Vec<Option<String>> = args
            .values
            .iter()
            .map(|v| match v {
                Some(crate::commands::Value::Targets(s)) => Some(
                    self.select(pi, s, here.pos)
                        .iter()
                        .map(|t| t.1.to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                _ => None,
            })
            .collect();
        let reply = match handler {
            None => self.builtin(c, pi, name, &args, here.pos),
            Some((owner, cb)) => {
                let Some((r, player)) = self.region(pi) else { return Vec::new() };
                let values = crate::commands::abi_values(&args, &selected);
                let mut reply: Vec<Msg> = Vec::new();
                let reply_ptr = &mut reply as *mut Vec<Msg> as *mut core::ffi::c_void;
                r.call(owner, cb, |w| abi::CommandArgs {
                    world: w,
                    player,
                    values: values.as_ptr(),
                    count: values.len() as u32,
                    reply: reply_ptr,
                    client: std::ptr::null_mut(),
                });
                reply
            }
        };
        // Commands also run outside players' messages (the console, scripts).
        self.settle(c);
        reply
    }

    /// Entities a selector picks as (dimension, id), in the order it asks for. `@e` looks in the
    /// runner's dimension, player selectors in all of them.
    fn select(&self, pi: usize, s: &Selector, here: DVec3) -> Vec<(u32, u64)> {
        let me = &self.players[pi];
        let mut found: Vec<(u32, u64, f64)> = Vec::new();
        let mut consider = |dim: u32, e: &Entity| {
            // Block entities and virtual ones are parts of things, not things to point at.
            if !e.in_world() {
                return;
            }
            if let Some((kind, neg)) = s.kind {
                if (e.kind == kind) == neg {
                    return;
                }
            }
            let d = if dim == me.dim { (e.pos - here).length() } else { f64::INFINITY };
            if s.distance.is_some_and(|(lo, hi)| d < lo || d > hi) {
                return;
            }
            found.push((dim, e.id, d));
        };
        if s.base == 'e' {
            for r in &self.dims[me.dim as usize].regions {
                for e in r.entities.list.iter().filter(|e| !e.removed) {
                    if s.name.as_ref().is_none_or(|n| {
                        self.players
                            .iter()
                            .any(|p| p.joined && p.entity == e.id && &p.name == n)
                    }) {
                        consider(me.dim, e);
                    }
                }
            }
        } else {
            for p in self.players.iter().filter(|p| p.joined) {
                let mine = p.entity == me.entity;
                if (s.base == 's' && !mine) || s.name.as_ref().is_some_and(|n| &p.name != n) {
                    continue;
                }
                if let Some(e) = self.dims[p.dim as usize].entity(p.entity) {
                    consider(p.dim, e);
                }
            }
        }
        let sort = s.sort.as_deref().unwrap_or(match s.base {
            'p' => "nearest",
            'r' => "random",
            _ => "arbitrary",
        });
        match sort {
            "nearest" => found.sort_by(|a, b| a.2.total_cmp(&b.2)),
            "furthest" => found.sort_by(|a, b| b.2.total_cmp(&a.2)),
            "random" => {
                let mut rng = weft_noise::Rng(self.tick);
                for i in (1..found.len()).rev() {
                    found.swap(i, rng.below(i as u64 + 1) as usize);
                }
            }
            _ => {}
        }
        let limit = s
            .limit
            .unwrap_or(if matches!(s.base, 'p' | 'r' | 's') { 1 } else { usize::MAX });
        found.into_iter().take(limit).map(|f| (f.0, f.1)).collect()
    }

    /// Puts text on the screens of `who`.
    pub fn show(&mut self, who: weft_abi::Who, shown: crate::net::Shown) {
        use weft_abi::Who;
        for pi in 0..self.players.len() {
            let p = &self.players[pi];
            let to_them = p.joined
                && match who {
                    Who::Player(id) => p.entity == id,
                    Who::All => true,
                    Who::Dimension(d) => p.dim == d,
                    Who::Near(d, at, radius) => {
                        p.dim == d
                            && self.dims[d as usize]
                                .entity(p.entity)
                                .is_some_and(|e| (e.pos - crate::world::dvec(at)).length_squared() < radius * radius)
                    }
                };
            if to_them {
                self.players[pi].conn.send(ToClient::Show(shown.clone()));
            }
        }
    }

    fn player_index(&self, entity: u64) -> Option<usize> {
        self.players
            .iter()
            .position(|p| p.joined && p.entity == entity)
    }

    /// Moves any entity; players go through `change_dim` so their client follows. Changing
    /// dimension asks `enter_dimension` first.
    fn teleport(&mut self, c: &'static Content, dim: u32, id: u64, to_dim: u32, pos: DVec3) {
        if dim != to_dim {
            let Some(r) = self.dims[dim as usize].entity_region(id) else { return };
            let mut q = abi::Query { entity: id, dim: to_dim, ..abi::Query::DEFAULT };
            if !r.ask(c, abi::action::ENTER_DIMENSION, &mut q) {
                return;
            }
        }
        if dim != to_dim {
            if let Some(r) = self.dims[dim as usize].entity_region(id) {
                r.emit_engine(c, abi::action::ENTER_DIMENSION, &abi::DimensionChange { entity: id, from: dim, to: to_dim });
            }
        }
        // Off what it rides, as in vanilla; what rides it comes along.
        let riders = match self.dims[dim as usize].entity_region(id) {
            Some(r) => {
                r.leave(c, id);
                r.riders_of(id)
            }
            None => Vec::new(),
        };
        self.move_entity(c, dim, id, to_dim, pos);
        for rider in riders {
            self.move_entity(c, dim, rider, to_dim, pos);
        }
    }

    /// `id` to `pos` in `to_dim`, a player's client told; riding or carried, it stays so.
    fn move_entity(&mut self, c: &'static Content, dim: u32, id: u64, to_dim: u32, pos: DVec3) {
        if let Some(pi) = self.player_index(id) {
            self.change_dim(c, pi, to_dim, pos);
            return;
        }
        let Some(from) = self.dims[dim as usize].entity_region(id) else { return };
        let Some((mut e, inv, comps)) = from.entities.take(id) else { return };
        // The blocks of a structure travel with its entity.
        let structure = from.bodies.remove(&id);
        let on_it: Vec<_> = from
            .entities
            .on_body(id)
            .into_iter()
            .filter_map(|b| from.entities.take(b))
            .collect();
        e.pos = pos;
        let r = self.dims[to_dim as usize].ensure_region(c, pos);
        r.entities.insert_with(c, e, inv, comps);
        if let Some(mut s) = structure {
            // It says what it gives all over again, where it is now.
            (s.cell, s.in_touch) = (None, false);
            r.bodies.insert(id, s);
        }
        for (b, inv, comps) in on_it {
            r.entities.insert_with(c, b, inv, comps);
        }
    }

    fn position_of(&self, dim: u32, id: u64) -> Option<DVec3> {
        self.dims[dim as usize]
            .entity_region_ref(id)
            .and_then(|r| r.entities.get(id))
            .map(|e| e.pos)
    }

    fn builtin(&mut self, c: &'static Content, pi: usize, name: &str, a: &crate::commands::Args, here: DVec3) -> Vec<Msg> {
        let me = (self.players[pi].dim, self.players[pi].entity);
        match builtin::COMMANDS.iter().find(|b| b.0 == name) {
            Some(b) => (b.2)(self, &builtin::Ctx { c, pi, a, here, me }),
            None => vec![Msg::new("command.unknown", vec![Arg::Text(name.to_string())])],
        }
    }

    /// What a player sees (the client generates it).
    fn full_view(&self, p: &Player) -> View {
        let dim = &self.dims[p.dim as usize];
        let pos = dim.entity(p.entity).map_or(DVec3::ZERO, |e| e.pos);
        let center = (pos.floor().as_ivec3()) >> 4;
        let r = p.view_radius;
        View::around(center, r, dim.air_below(center, View::air_probe(r)))
    }

    /// What the server loads, simulates and sends whole around a player: the view cut to the
    /// simulation distance. Beyond it clients generate the world themselves and get only what
    /// differs from generation (`send_far`).
    fn view(&self, p: &Player) -> View {
        self.full_view(p)
            .cut(crate::config::get(content().game.simulation_distance) as i32)
    }
}

/// The nearest structure `name` of dimension `dim` within `radius` of `pos`: its lowest corner.
pub fn structure_near(c: &Content, dim: u32, seed: u64, name: &str, pos: IVec3, radius: i32) -> Option<IVec3> {
    let def = &c.dims[dim as usize];
    let s = def.structures.iter().find(|s| s.name == name)?;
    let cells = |v: i32| (v - radius).div_euclid(s.spacing)..=(v + radius).div_euclid(s.spacing);
    let mut best: Option<IVec3> = None;
    for cz in cells(pos.z) {
        for cx in cells(pos.x) {
            if let Some(a) = dim::structure_in(def, s, seed, (cx, cz)) {
                let d = |p: IVec3| (p - pos).with_y(0).as_vec3().length();
                if d(a) <= radius as f32 && best.is_none_or(|b| d(a) < d(b)) {
                    best = Some(a);
                }
            }
        }
    }
    best
}

/// Items in an entity's equipment slots that show on the model.
fn worn_of(c: &Content, ents: &entity::Entities, id: u64) -> Vec<u32> {
    let Some(e) = ents.get(id) else { return Vec::new() };
    let worn = &c.game.entities[e.kind as usize].worn;
    let Some(inv) = ents
        .inventories
        .get(&id)
        .and_then(|i| i.get(worn.start as usize..worn.end as usize))
    else {
        return Vec::new();
    };
    inv.iter()
        .filter(|s| !s.is_empty() && c.worn(s.item).is_some())
        .map(|s| s.item)
        .collect()
}

fn serialize(c: &Content, d: &Dim, ids: &[u64]) -> Vec<u8> {
    // What is on a body comes right after it, for it to be there when those are read back.
    let ids: Vec<u64> = ids
        .iter()
        .flat_map(|&id| std::iter::once(id).chain(d.entities.on_body(id)))
        .collect();
    let list: Vec<persist::ToSave> = ids
        .iter()
        .filter_map(|&id| {
            let e = d.entities.get(id)?;
            let comps = d
                .entities
                .comps
                .iter()
                .filter(|((eid, _), _)| *eid == id)
                .map(|((_, k), v)| (*k, &v[..]))
                .collect();
            Some((e, d.entities.inventories.get(&id), comps, d.bodies.get(&id).map(|s| s.structure_blocks(c))))
        })
        .collect();
    let mut out = Vec::new();
    persist::write(c, &mut out, &list);
    out
}

fn entities_in(c: &Content, d: &Dim, cube: IVec3) -> Vec<u8> {
    let ids: Vec<u64> = d
        .entities
        .list
        .iter()
        .filter(|e| e.saved_with_cube() && cube_of(e.pos.floor().as_ivec3()) == cube)
        .map(|e| e.id)
        .collect();
    serialize(c, d, &ids)
}

/// Said to a player arriving in the engine's void.
/// "X was slain by Zombie": `killer` is the kind of whatever did it, if something did.
fn death_message(c: &Content, cause: u32, killer: Option<u32>, name: String) -> Msg {
    let by = killer
        .and_then(|k| c.game.entities.get(k as usize))
        .map(|k| Arg::Key(format!("entity.{}", k.name.replace(':', "."))));
    let def = c.game.damage_causes.get(cause as usize);
    let key = match (def, &by) {
        (Some(d), Some(_)) if !d.death_message_by.is_empty() => d.death_message_by.clone(),
        (Some(d), _) if !d.death_message.is_empty() => d.death_message.clone(),
        _ => "death.generic".to_string(),
    };
    let mut args = vec![Arg::Text(name)];
    args.extend(by);
    Msg::new(&key, args)
}

fn void_note(c: &Content) -> Msg {
    Msg::new(if c.spawn_dim == c.void_dim { "void.no_dimension" } else { "void.here" }, Vec::new())
}
