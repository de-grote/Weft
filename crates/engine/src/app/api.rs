//! Client side of the mod ABI: what client callbacks see of the running game, and what frame
//! hooks may do with it (input, the local player, worlds, screens, screenshots).

use std::cell::RefCell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use glam::Vec2;
use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use weft_abi::{self as abi, ItemStack, Slice, Str, client_flags as cf};

use super::{Request, Running};
use crate::client::Client;
use crate::client::modui::ClientView;
use crate::input::Key;
use crate::ui::Kind;

static LAUNCH: OnceLock<FxHashMap<String, String>> = OnceLock::new();
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

/// `--name value` pairs the engine itself does not know, for mods (`Host::launch_option`).
pub fn set_launch_options(o: Vec<(String, String)>) {
    let _ = LAUNCH.set(o.into_iter().collect());
}

pub fn launch_options() -> &'static FxHashMap<String, String> {
    LAUNCH.get_or_init(FxHashMap::default)
}

/// Exit code a mod asked for with `client_op::QUIT`.
pub fn exit_code() -> i32 {
    EXIT_CODE.load(Ordering::Relaxed)
}

pub unsafe extern "C" fn launch_option(name: Str) -> Str {
    let name = unsafe { name.as_str() };
    launch_options()
        .get(name)
        .map_or(Str::EMPTY, |v| Str::new(v))
}

// ---------------------------------------------------------------- images

static IMAGES: Mutex<Option<FxHashMap<u64, (u32, u32, Vec<u8>)>>> = Mutex::new(None);
static NEXT_IMAGE: AtomicU64 = AtomicU64::new(1);

pub fn new_image_id() -> u64 {
    NEXT_IMAGE.fetch_add(1, Ordering::Relaxed)
}

pub fn store_image(id: u64, w: u32, h: u32, rgba: Vec<u8>) {
    IMAGES
        .lock()
        .get_or_insert_with(FxHashMap::default)
        .insert(id, (w, h, rgba));
}

pub unsafe extern "C" fn image_get(id: u64, w: *mut u32, h: *mut u32) -> *const u8 {
    let g = IMAGES.lock();
    match g.as_ref().and_then(|m| m.get(&id)) {
        Some((iw, ih, px)) => {
            unsafe {
                *w = *iw;
                *h = *ih;
            }
            // The pixel buffer stays put until the image is freed, whatever the map does.
            px.as_ptr()
        }
        None => std::ptr::null(),
    }
}

pub unsafe extern "C" fn image_load(path: Str) -> u64 {
    let path = unsafe { path.as_str() };
    match crate::assets::decode_png_file(std::path::Path::new(path)) {
        Some((w, h, px)) => {
            let id = new_image_id();
            store_image(id, w, h, px);
            id
        }
        None => 0,
    }
}

pub unsafe extern "C" fn image_save(path: Str, rgba: Slice<u8>, w: u32, h: u32) -> u32 {
    let (path, px) = unsafe { (path.as_str(), rgba.as_slice()) };
    if px.len() != (w * h * 4) as usize {
        return 0;
    }
    crate::assets::encode_png(std::path::Path::new(path), w, h, px).is_ok() as u32
}

pub unsafe extern "C" fn image_free(id: u64) {
    if let Some(m) = IMAGES.lock().as_mut() {
        m.remove(&id);
    }
}

// ---------------------------------------------------------------- context

/// Frame hooks get a `ClientView` whose `run` points at the whole client: these functions only
/// act when it does (UI bindings see a view without it).
unsafe fn view<'a>(c: *mut abi::ClientCtx) -> Option<&'a ClientView> {
    unsafe { (c as *const ClientView).as_ref() }
}

unsafe fn client<'a>(c: *mut abi::ClientCtx) -> Option<&'a Client> {
    unsafe { view(c).and_then(|v| v.client.as_ref()) }
}

pub(crate) unsafe fn run<'a>(c: *mut abi::ClientCtx) -> Option<&'a mut Running> {
    unsafe { view(c).and_then(|v| (v.run as *mut Running).as_mut()) }
}

pub unsafe extern "C" fn client_show(c: *mut abi::ClientCtx, d: *const abi::ShowDesc) {
    unsafe {
        if let Some(g) = run(c).and_then(|r| r.game.as_mut()) {
            g.client.show(crate::host::shown(&*d));
        }
    }
}

/// The answer to a command run with `client_op::RUN_COMMAND`, once it came: its lines in the
/// client's language, one per line.
pub unsafe extern "C" fn client_command_reply(c: *mut abi::ClientCtx, id: u32, out: *mut Str) -> u32 {
    unsafe {
        let Some(r) = run(c) else { return 0 };
        let Some(g) = r.game.as_mut() else { return 0 };
        let Some(lines) = g.client.command_replies.remove(&id) else { return 0 };
        let text: Vec<String> = lines.iter().map(|m| m.text(&r.lang)).collect();
        *out = text_out(&text.join("\n"));
        1
    }
}

thread_local! {
    static TEXT: RefCell<String> = const { RefCell::new(String::new()) };
}

fn text_out(s: &str) -> Str {
    TEXT.with(|t| {
        let mut t = t.borrow_mut();
        t.clear();
        t.push_str(s);
        Str { ptr: t.as_ptr(), len: t.len() }
    })
}

impl Running {
    /// Whether the world around the player is loaded and meshed, and whether everything is idle.
    pub(super) fn load_state(&self) -> (bool, bool) {
        let loaded = self.game.as_ref().is_some_and(|g| {
            let terrain = g
                .client
                .terrain
                .pending
                .load(std::sync::atomic::Ordering::Relaxed);
            g.client.joined && g.client.world.len() > 50 && g.client.pending_meshes() == 0 && terrain == 0
        });
        let idle = loaded
            && self
                .game
                .as_ref()
                .is_some_and(|g| g.server.as_ref().is_none_or(|s| s.idle()));
        (loaded, idle)
    }

    fn hold(&mut self, k: Key, secs: f32) {
        // A key still held from the last tap would not count as pressed again.
        self.input.key_up(k);
        self.input.key_down(k);
        let until = Instant::now() + Duration::from_secs_f32(secs.max(0.05));
        self.mod_held.retain(|(h, _)| *h != k);
        self.mod_held.push((k, until));
    }

    /// Releases keys mods held down once their time is up.
    pub(super) fn release_mod_keys(&mut self) {
        let now = Instant::now();
        let input = &mut self.input;
        self.mod_held.retain(|(k, at)| {
            if now >= *at {
                input.key_up(*k);
                false
            } else {
                true
            }
        });
    }
}

pub unsafe extern "C" fn client_state(c: *mut abi::ClientCtx, out: *mut abi::ClientState) {
    unsafe {
        let mut s = abi::ClientState::DEFAULT;
        if let Some(r) = run(c) {
            let (loaded, idle) = r.load_state();
            let mut flags = 0;
            if loaded {
                flags |= cf::LOADED;
            }
            if idle {
                flags |= cf::IDLE;
            }
            if let Some((_, ui)) = r.screens.last() {
                if r.screen_frames >= 2 && !ui.dirty {
                    flags |= cf::SCREEN_READY;
                }
            }
            if crate::net::current_ticket().is_some() {
                flags |= cf::HOSTING;
            }
            if r.debug {
                flags |= cf::DEBUG;
            }
            if r.audio.as_ref().is_some_and(|a| a.music_playing()) {
                flags |= cf::MUSIC;
            }
            let single = r
                .game
                .as_ref()
                .and_then(|g| g.server.as_ref())
                .is_some_and(|s| !s.is_hosting());
            if single && r.screens.iter().any(|(s, _)| s.pauses()) {
                flags |= cf::PAUSED;
            }
            s.sounds = r.audio.as_ref().map_or(0, |a| a.played);
            s.flags = flags;
            s.frame = r.frame_no;
            s.dt = r.frame_dt;
            s.fps = r.fps;
            s.frame_ms = r.frame_ms;
            s.gpu_ms = r.gpu.gpu_times().iter().sum();
            s.window = r.gpu.surface_size().into();
            s.gui_scale = r.gui_scale();
            s.screen = r
                .screens
                .last()
                .map_or(Str::EMPTY, |(sc, _)| Str::new(sc.name()));
            s.screen_frames = r.screen_frames;
            s.bob = r.game.as_ref().map_or(0.0, |g| g.client.bob());
            if let Some(g) = &r.game {
                s.camera = g.client.cameras.mode;
                s.lod_columns = g.client.lod_columns() as u32;
                s.camera_pos = g.client.cameras.shown.pos.to_array().into();
            }
            (s.benches, [s.bench_fps, s.bench_avg_ms, s.bench_p99_ms, s.bench_gpu_ms, s.bench_server_ms]) = (r.benches, r.bench_last);
            if let Some(srv) = r.game.as_ref().and_then(|g| g.server.as_ref()) {
                s.server_ms = srv.tick_ms();
                let (g, gus, i, ius) = srv.load_stats();
                (s.generated, s.gen_us, s.inserted, s.insert_us) = (g, gus as f32, i, ius as f32);
            }
        }
        if let Some(cl) = client(c) {
            s.flags |= cf::IN_WORLD;
            let p = &cl.player;
            let bit = |on: bool, f: u32| if on { f } else { 0 };
            s.flags |= bit(cl.joined, cf::JOINED)
                | bit(cl.dead, cf::DEAD)
                | bit(p.flying, cf::FLYING)
                | bit(p.on_ground, cf::ON_GROUND)
                | bit(cl.menu.is_some(), cf::MENU_OPEN);
            s.dim = cl.world.dim;
            s.player = cl.player_id;
            s.pos = p.pos.to_array().into();
            s.shown = cl.smooth_pos().to_array().into();
            if let Some(h) = cl.target {
                (s.target, s.target_body) = (h.block.to_array().into(), h.frame);
            }
            s.vel = p.vel.to_array().into();
            s.yaw = p.yaw;
            s.pitch = p.pitch;
            s.selected = cl.selected as u32;
            s.mode = cl.mode.0 as u32;
            s.health = cl.health();
            s.cubes = cl.world.len() as u32;
            s.pending_meshes = cl.pending_meshes() as u32;
            s.gpu_cubes = crate::client::terrain_gpu::GPU_CUBES.load(std::sync::atomic::Ordering::Relaxed);
            s.gpu_mismatches = crate::client::terrain_gpu::GPU_MISMATCHES.load(std::sync::atomic::Ordering::Relaxed);
            let (shown, _, _, pending) = cl.lod_stats();
            (s.lod_tiles, s.lod_pending) = (shown as u32, pending as u32);
            s.entities = cl.entities.len() as u32;
            s.particles = cl.particles.len() as u32;
        }
        *out = s;
    }
}

pub unsafe extern "C" fn client_input(c: *mut abi::ClientCtx, op: u32, text: Str, x: f32, y: f32) -> u32 {
    use abi::client_input as ci;
    unsafe {
        let Some(r) = run(c) else { return 0 };
        let text = text.as_str();
        match op {
            ci::ACTION => match r.input.binding_of(text).map(|b| b.chord) {
                Some(c) => {
                    for (bit, _, k, _) in crate::input::MODIFIERS {
                        if c.mods & bit != 0 {
                            r.hold(Key::Code(k), x);
                        }
                    }
                    if let Some(w) = c.with {
                        r.hold(w, x);
                    }
                    r.hold(c.key, x)
                }
                None => return 0,
            },
            ci::KEY => match crate::input::Key::parse(text) {
                Some(k) => r.hold(k, x),
                None => return 0,
            },
            ci::CLICK => r.hold(Key::Mouse(y.clamp(0.0, 255.0) as u8), x),
            ci::CURSOR => {
                let (w, h) = r.gpu.surface_size();
                r.input.cursor = Vec2::new(x * w as f32, y * h as f32);
            }
            ci::SCROLL => r.input.wheel(x),
            ci::TYPE => r.input.typed.push_str(text),
            ci::RELEASE_ALL => {
                r.input.release_all();
                r.mod_held.clear();
            }
            _ => return 0,
        }
        1
    }
}

pub unsafe extern "C" fn client_pressed(c: *mut abi::ClientCtx, action: Str, out: *mut abi::KeyState) {
    unsafe {
        let Some(r) = run(c) else { return };
        // Mod keys belong to the world: while a screen (chat, inventory) has the input they stay up.
        *out = match &r.game {
            Some(g) if r.screens.is_empty() => {
                crate::input::Keys { input: &r.input, ticks: &g.client.key_ticks, raw: false }.state(action.as_str())
            }
            _ => abi::KeyState::default(),
        };
    }
}

pub unsafe extern "C" fn client_set_player(c: *mut abi::ClientCtx, control: *const abi::PlayerControl) {
    use abi::control_mask as m;
    unsafe {
        let Some(r) = run(c) else { return };
        let Some(g) = r.game.as_mut() else { return };
        let ctl = *control;
        let p = &mut g.client.player;
        if ctl.mask & m::POS != 0 && crate::world::dvec(ctl.pos).is_finite() {
            p.pos = crate::world::dvec(ctl.pos);
            p.prev = p.pos;
        }
        if ctl.mask & m::VEL != 0 && crate::world::dvec(ctl.vel).is_finite() {
            p.vel = crate::world::dvec(ctl.vel);
        }
        if ctl.mask & m::LOOK != 0 {
            p.yaw = ctl.yaw;
            p.pitch = ctl
                .pitch
                .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
        }
        if ctl.mask & m::FLYING != 0 {
            p.flying = ctl.flying != 0;
        }
        if ctl.mask & m::FLY_SPEED != 0 {
            p.fly_speed = ctl.fly_speed.max(0.0);
        }
        if ctl.mask & m::SELECTED != 0 && ctl.selected < crate::content::content().game.player_layout.hotbar {
            g.client.selected = ctl.selected as usize;
            g.client.send(crate::net::ToServer::Select(ctl.selected));
        }
    }
}

pub unsafe extern "C" fn client_command(c: *mut abi::ClientCtx, op: u32, text: Str, num: f64) -> u64 {
    use abi::client_op as op_;
    unsafe {
        let Some(r) = run(c) else { return 0 };
        let text = text.as_str().to_string();
        match op {
            op_::CHAT => r.handle(vec![Request::SendChat(text)]),
            op_::SCREENSHOT => {
                let dir = r.settings.run_dir.join("screenshots");
                let path = if text.is_empty() {
                    dir.join(format!("{}.png", crate::saves::now_secs()))
                } else {
                    let p = std::path::PathBuf::from(&text);
                    if p.is_absolute() || p.components().count() > 1 { p } else { dir.join(p) }
                };
                let id = new_image_id();
                r.shot_path = Some(path);
                r.shot_image = Some(id);
                r.gpu.request_capture();
                r.shot_pending = true;
                return id;
            }
            op_::QUIT => {
                EXIT_CODE.store(num as i32, Ordering::Relaxed);
                r.quit = true;
            }
            op_::OPEN_WORLD | op_::NEW_WORLD => {
                r.leave_world_silent();
                let dir = r.settings.saves_dir();
                let mut named: Vec<_> = crate::saves::list(&dir)
                    .into_iter()
                    .filter(|w| w.name == text)
                    .collect();
                named.sort_by_key(|w| !crate::saves::in_own_folder(w));
                let meta = match named.into_iter().next() {
                    Some(w) if op == op_::OPEN_WORLD => w,
                    Some(w) => {
                        if crate::saves::in_own_folder(&w) {
                            let _ = std::fs::remove_dir_all(&w.dir);
                        }
                        crate::saves::create(&dir, &text, num as u64)
                    }
                    None => crate::saves::create(&dir, &text, num as u64),
                };
                r.start_world(meta);
            }
            op_::LEAVE_WORLD => r.leave_world(),
            op_::HOST => r.handle(vec![Request::Host]),
            op_::JOIN => r.handle(vec![Request::Join(text)]),
            op_::OPEN_SCREEN => {
                if text == "core:inventory" {
                    if let Some(g) = r.game.as_mut() {
                        g.client.send(crate::net::ToServer::OpenInventory);
                    }
                    return 1;
                }
                let Some(s) = crate::screens::named(&text) else {
                    log::warn!("client_op::OPEN_SCREEN: no screen {text}");
                    return 0;
                };
                r.open(s);
            }
            op_::CLOSE_SCREEN => {
                if !r.screens.is_empty() {
                    r.handle(vec![Request::Close]);
                }
            }
            op_::RELOAD_ASSETS => r.reload_requested = true,
            op_::BENCH => r.bench = Some(crate::bench::Bench::new(&text, num.max(0.1) as f32)),
            op_::RESPAWN => r.handle(vec![Request::Respawn]),
            op_::RUN_COMMAND => return r.run_command(&text) as u64,
            op_::MENU_SORT => {
                let Some(g) = r.game.as_mut().filter(|g| g.client.menu.is_some()) else { return 0 };
                let slot = if num < 0.0 { u32::MAX } else { num as u32 };
                g.client
                    .send(crate::net::ToServer::Click { slot, button: crate::server::menu::SORT, shift: false });
            }
            _ => return 0,
        }
        1
    }
}

pub unsafe extern "C" fn client_text(c: *mut abi::ClientCtx, which: u32, index: u32) -> Str {
    use abi::client_text as ct;
    unsafe {
        match which {
            ct::TICKET => text_out(&crate::net::current_ticket().unwrap_or_default()),
            ct::RUN_DIR => match run(c) {
                Some(r) => text_out(&r.settings.run_dir.to_string_lossy()),
                None => Str::EMPTY,
            },
            // In the client's language, as the player reads it.
            ct::CHAT => match (client(c), run(c)) {
                (Some(cl), Some(r)) => cl
                    .chat
                    .iter()
                    .rev()
                    .nth(index as usize)
                    .map_or(Str::EMPTY, |(m, _)| text_out(&m.msg.text(&r.lang))),
                _ => Str::EMPTY,
            },
            ct::BENCH => match run(c) {
                Some(r) => text_out(&r.bench_line),
                None => Str::EMPTY,
            },
            ct::INPUT => {
                let field = run(c).and_then(|r| r.screens.last()).and_then(|(_, ui)| {
                    let n = ui.focused.or_else(|| {
                        ui.nodes
                            .iter()
                            .position(|n| matches!(n.kind, Kind::Input { .. }))
                    })?;
                    match &ui.nodes[n].kind {
                        Kind::Input { value, .. } => Some(value.clone()),
                        _ => None,
                    }
                });
                field.map_or(Str::EMPTY, |v| text_out(&v))
            }
            _ => Str::EMPTY,
        }
    }
}

pub unsafe extern "C" fn client_block_in(c: *mut abi::ClientCtx, frame: u64, pos: *const abi::BlockPos) -> u32 {
    unsafe {
        let Some(cl) = client(c) else { return u32::MAX };
        let p = crate::world::ivec(*pos);
        if frame == 0 {
            return cl.world.block(p);
        }
        cl.entities
            .get(&frame)
            .and_then(|e| e.structure.as_ref())
            .map_or(u32::MAX, |s| s.block(p))
    }
}

pub unsafe extern "C" fn client_cube_shown(c: *mut abi::ClientCtx, pos: *const abi::BlockPos) -> u32 {
    unsafe {
        client(c).is_some_and(|cl| {
            cl.world
                .cube_shown(crate::world::cube_of(crate::world::ivec(*pos)))
        }) as u32
    }
}

pub unsafe extern "C" fn client_light(c: *mut abi::ClientCtx, pos: *const abi::BlockPos) -> u32 {
    unsafe {
        let p = crate::world::ivec(*pos);
        client(c).map_or(u32::MAX, |cl| if cl.world.block(p) == u32::MAX { u32::MAX } else { cl.world.light(p) })
    }
}

pub unsafe extern "C" fn client_slot(c: *mut abi::ClientCtx, container: u32, slot: u32) -> ItemStack {
    unsafe {
        let Some(cl) = client(c) else { return ItemStack::EMPTY };
        use abi::slot_container as sc;
        let s = match container {
            sc::INVENTORY => cl.inventory.get(slot as usize).copied(),
            sc::MENU => cl
                .menu
                .as_ref()
                .and_then(|m| m.slots.get(slot as usize))
                .copied(),
            sc::CURSOR => cl.menu.as_ref().map(|m| m.cursor),
            _ => None,
        };
        s.unwrap_or_default()
    }
}

pub unsafe extern "C" fn client_menu(c: *mut abi::ClientCtx, out: *mut abi::OpenMenu) -> u32 {
    unsafe {
        match client(c).and_then(|cl| cl.menu.as_ref()) {
            Some(m) => {
                *out = abi::OpenMenu { kind: m.kind, window: m.window, slots: m.slots.len() as u32 };
                1
            }
            None => 0,
        }
    }
}

pub unsafe extern "C" fn client_entities(c: *mut abi::ClientCtx, out: *mut u64, max: usize) -> usize {
    unsafe {
        let Some(cl) = client(c) else { return 0 };
        let mut ids: Vec<u64> = cl.entities.keys().copied().collect();
        ids.sort_unstable();
        for (i, id) in ids.iter().take(max).enumerate() {
            *out.add(i) = *id;
        }
        ids.len()
    }
}

pub unsafe extern "C" fn client_ui_rect(c: *mut abi::ClientCtx, query: Str, out: *mut abi::Rect) -> u32 {
    unsafe {
        let Some(r) = run(c) else { return 0 };
        let q = query.as_str();
        // `id:hud:<element>` is on the overlay whatever screen is open (the HUD editor's).
        let ui = if q.starts_with("id:hud:") { &r.hud.ui } else { r.screens.last().map_or(&r.hud.ui, |s| &s.1) };
        let found = if let Some(n) = q.strip_prefix("slot:").and_then(|n| n.parse::<u64>().ok()) {
            ui.nodes
                .iter()
                .position(|node| matches!(node.kind, Kind::Custom { tag: crate::client::modui::TAG_SLOT, data } if data == n))
        } else if let Some(n) = q.strip_prefix("item:").and_then(|n| n.parse::<u64>().ok()) {
            ui.nodes
                .iter()
                .position(|node| matches!(node.kind, Kind::Custom { tag: crate::client::modui::TAG_CREATIVE, data } if data == n))
        } else if let Some(n) = q.strip_prefix("icon:").and_then(|n| n.parse::<u64>().ok()) {
            ui.nodes.iter().position(|node| match node.kind {
                Kind::Custom { tag: crate::client::modui::TAG_ICON, data } => data == n,
                Kind::Item { last, .. } => last.0 as u64 == n,
                _ => false,
            })
        } else if q == "slider" {
            ui.nodes
                .iter()
                .position(|node| matches!(node.kind, Kind::Slider { .. }))
        } else if q == "entity" {
            ui.nodes
                .iter()
                .position(|node| matches!(node.kind, Kind::Custom { tag: crate::client::modui::TAG_ENTITY, .. }))
        } else if let Some(id) = q.strip_prefix("id:") {
            ui.find(id)
        } else if let Some(class) = q.strip_prefix("class:") {
            ui.nodes.iter().position(|node| match &node.kind {
                Kind::Container { class: Some(c) }
                | Kind::Button { class: c, .. }
                | Kind::Progress { class: c, .. }
                | Kind::Icons { class: c, .. } => c == class,
                _ => false,
            })
        } else if let Some(words) = q.strip_prefix("text:") {
            let words = words.to_lowercase();
            ui.nodes.iter().position(|node| match &node.kind {
                Kind::Text(t) => {
                    let s: String = t
                        .spans
                        .iter()
                        .filter_map(|s| if let crate::ui::text::Piece::Text(x) = &s.piece { Some(x.as_str()) } else { None })
                        .collect();
                    s.to_lowercase().contains(&words)
                }
                _ => false,
            })
        } else {
            None
        };
        // `scrollbar`: the track of the first scroll bar shown.
        let bar = (q == "scrollbar")
            .then(|| ui.nodes.iter().find_map(|n| n.scrollbar.map(|b| b.track)))
            .flatten();
        let Some(rect) = bar.or_else(|| found.map(|i| ui.nodes[i].rect)) else { return 0 };
        let scale = r.gui_scale();
        let (w, h) = r.gpu.surface_size();
        let (w, h) = (w as f32 / scale, h as f32 / scale);
        *out = abi::Rect { x: rect.x / w, y: rect.y / h, w: rect.w / w, h: rect.h / h };
        1
    }
}

/// Whether a mod's drawing this frame shows: no group, or one the player shows.
fn drawn(r: &Running, group: &str) -> bool {
    group.is_empty() || r.game.as_ref().is_some_and(|g| g.client.gizmos.on(group))
}

pub unsafe extern "C" fn client_draw(c: *mut abi::ClientCtx, group: Str, kind: u32, a: *const abi::Vec3, b: *const abi::Vec3, color: u32) {
    unsafe {
        let Some(r) = run(c) else { return };
        let group = group.as_str();
        if kind > abi::gizmo_kind::FILL || !drawn(r, group) {
            return;
        }
        let (a, b) = (crate::world::dvec(*a), crate::world::dvec(*b));
        r.mod_gizmos
            .push(crate::net::GizmoShape::shape(group, kind as u8, a, b, color, 0));
    }
}

pub unsafe extern "C" fn client_draw_text(
    c: *mut abi::ClientCtx,
    group: Str,
    pos: *const abi::Vec3,
    size: f32,
    color: u32,
    key: Str,
    args: Slice<abi::UiArg>,
) {
    unsafe {
        let Some(r) = run(c) else { return };
        let group = group.as_str();
        if !drawn(r, group) {
            return;
        }
        let text = crate::ui::text::Msg::new(key.as_str(), crate::client::modui::args(args));
        r.mod_gizmos
            .push(crate::net::GizmoShape::text(group, crate::world::dvec(*pos), size, color, 0, text));
    }
}

pub unsafe extern "C" fn client_screen_text(
    c: *mut abi::ClientCtx,
    x: f32,
    y: f32,
    size: f32,
    color: u32,
    key: Str,
    args: Slice<abi::UiArg>,
) {
    unsafe {
        let Some(r) = run(c) else { return };
        let text = crate::ui::text::Msg::new(key.as_str(), crate::client::modui::args(args));
        r.mod_screen_text
            .push((x, y, text, color, if size > 0.0 { size } else { 1.0 }));
    }
}

pub unsafe extern "C" fn client_send_packet(c: *mut abi::ClientCtx, name: Str, schema: Str, data: Slice<u8>) {
    unsafe {
        let Some(r) = run(c) else { return };
        let Some(channel) = crate::host::channel_for(name.as_str(), schema.as_str()) else { return };
        if let Some(g) = r.game.as_mut() {
            g.client
                .send(crate::net::ToServer::Packet { channel, data: data.as_slice().to_vec() });
        }
    }
}

pub unsafe extern "C" fn client_particle_burst(c: *mut abi::ClientCtx, burst: *const abi::ParticleBurst) {
    unsafe {
        let Some(r) = run(c) else { return };
        if let Some(g) = r.game.as_mut() {
            g.client.particles.spawn(&*burst, &g.client.world);
        }
    }
}

pub unsafe extern "C" fn client_pose_bone(
    c: *mut abi::ClientCtx,
    entity: u64,
    bone: Str,
    turn: *const [f32; 3],
    by: *const [f32; 3],
    scale: *const [f32; 3],
) {
    unsafe {
        let Some(g) = run(c).and_then(|r| r.game.as_mut()) else { return };
        let pose = crate::client::models::BonePose {
            turn: glam::Vec3::from_array(*turn),
            by: glam::Vec3::from_array(*by),
            scale: glam::Vec3::from_array(*scale),
        };
        g.client
            .entity_render
            .models
            .turned
            .entry(entity)
            .or_default()
            .push((bone.as_str().to_string(), pose));
    }
}

/// Runs `f` on an entity as the renderer sees it, the local player too.
unsafe fn with_entity<T>(
    c: *mut abi::ClientCtx,
    entity: u64,
    f: impl FnOnce(&crate::client::Client, &crate::client::ClientEntity) -> T,
) -> Option<T> {
    unsafe {
        let g = run(c)?.game.as_ref()?;
        if entity == g.client.player_id {
            return Some(f(&g.client, &g.client.me));
        }
        g.client
            .entities
            .get(&entity)
            .or_else(|| g.client.showing_blocks.get(&entity))
            .map(|e| f(&g.client, e))
    }
}

pub unsafe extern "C" fn client_entity_action(c: *mut abi::ClientCtx, entity: u64, out: *mut Str) -> u32 {
    unsafe {
        let name = with_entity(c, entity, |g, e| g.entity_render.action_of(entity, e).map(str::to_string));
        match name.flatten() {
            Some(n) => {
                *out = text_out(&n);
                1
            }
            None => 0,
        }
    }
}

pub unsafe extern "C" fn client_clip_length(c: *mut abi::ClientCtx, entity: u64, name: Str) -> f32 {
    unsafe {
        with_entity(c, entity, |g, e| g.entity_render.clip_length(entity, e, name.as_str()))
            .flatten()
            .unwrap_or(-1.0)
    }
}

pub unsafe extern "C" fn client_environment_set(c: *mut abi::ClientCtx, name: Str, data: Slice<u8>) -> u32 {
    unsafe {
        let Some(r) = run(c) else { return 0 };
        let name = name.as_str();
        if data.len == 0 {
            if let Some(g) = r.game.as_mut() {
                g.client.environment.local.remove(name);
                return 1;
            }
            return 0;
        }
        let Some(mut layer) = abi::data::from_bytes::<abi::environment::EnvironmentLayer>(data.as_slice()) else { return 0 };
        if layer.post.iter().any(|effect| !r.post.has(&effect.name)) {
            return 0;
        }
        let Some(g) = r.game.as_mut() else { return 0 };
        layer.name = name.to_string();
        g.client.environment.local.insert(name.to_string(), layer);
        1
    }
}

pub unsafe extern "C" fn client_environment_values(c: *mut abi::ClientCtx, out: *mut abi::environment::EnvironmentValues) {
    unsafe {
        if let Some(client) = client(c) {
            *out = client.environment.values;
        }
    }
}

pub unsafe extern "C" fn client_displayed(c: *mut abi::ClientCtx, entity: u64, source: u32, out: *mut ItemStack, max: usize) -> usize {
    unsafe {
        with_entity(c, entity, |_, e| {
            let hands = [e.held, e.off_held];
            let stacks = match source {
                abi::display_source::HANDS => &hands[..],
                abi::display_source::EQUIPMENT => &e.worn,
                abi::display_source::SHOWN => &e.shown,
                abi::display_source::GROUND => std::slice::from_ref(&e.item),
                _ => &[],
            };
            for (i, stack) in stacks.iter().take(max).enumerate() {
                *out.add(i) = *stack;
            }
            stacks.len()
        })
        .unwrap_or(0)
    }
}

pub unsafe extern "C" fn client_bone_position(c: *mut abi::ClientCtx, entity: u64, bone: Str, out: *mut abi::Vec3) -> u32 {
    unsafe {
        let Some(g) = run(c).and_then(|r| r.game.as_ref()) else { return 0 };
        match g
            .client
            .entity_render
            .models
            .bone_position(entity, bone.as_str())
        {
            Some(p) => {
                *out = p.to_array().into();
                1
            }
            None => 0,
        }
    }
}

pub unsafe extern "C" fn client_play_sound(c: *mut abi::ClientCtx, sound: u32, pos: *const abi::Vec3, volume: f32, pitch: f32) {
    unsafe {
        let Some(r) = run(c) else { return };
        let pos = (!pos.is_null()).then(|| crate::world::dvec(*pos));
        r.ui_sounds
            .push(crate::audio::Play { sound, pos, volume, pitch });
    }
}
