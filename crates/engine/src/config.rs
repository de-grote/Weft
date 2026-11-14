//! Settings the engine (namespace `core`) and mods declare: typed, ranged values read with one
//! atomic load (text ones under a lock), edited on screens built from the declarations. The scope
//! says where a value lives: `CLIENT` in each player's `config/<ns>.txt`; `SERVER` in the
//! server's file, sent to its players whenever it changes; `WORLD` saved with the world (game
//! rules, `/rule`) and sent the same way. The engine's own settings and key bindings are `core:`
//! ones like any other, in `config/core.txt`.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};

use weft_abi::{config_kind as ck, config_scope as scope};

use crate::content::content;
use crate::entries::{Entry, RawEntry};

#[derive(Clone, Debug)]
pub struct ConfigDef {
    pub name: String,
    pub kind: u32,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub choices: Vec<String>,
    pub flags: u32,
    pub scope: u32,
    /// The default of a `TEXT` setting.
    pub text: String,
    /// The settings page it shows on, empty for its mod's; and its place there.
    pub page: String,
    pub order: i32,
}

impl ConfigDef {
    /// The server's value counts for its players.
    pub fn synced(&self) -> bool {
        self.scope != scope::CLIENT
    }

    /// `v` made valid for this setting.
    pub fn fit(&self, v: f64) -> f64 {
        if !v.is_finite() {
            return self.default;
        }
        match self.kind {
            ck::BOOL => (v != 0.0) as u8 as f64,
            ck::CHOICE => v
                .round()
                .clamp(0.0, self.choices.len().saturating_sub(1) as f64),
            ck::TEXT => 0.0,
            _ => {
                let mut v = v.clamp(self.min.min(self.max), self.max.max(self.min));
                if self.step > 0.0 {
                    v = (self.min + ((v - self.min) / self.step).round() * self.step).clamp(self.min.min(self.max), self.max.max(self.min));
                }
                if self.kind == ck::INT { v.round() } else { v }
            }
        }
    }

    /// How many steps a slider of it goes through: its own, or `unit` (what one shown digit
    /// is worth) where it has none or a finer one.
    pub fn steps(&self, unit: f64) -> u32 {
        ((self.max - self.min).abs() / self.step.max(unit))
            .round()
            .clamp(1.0, u32::MAX as f64) as u32
    }

    fn parse(&self, s: &str) -> Option<f64> {
        let s = s.trim();
        match self.kind {
            ck::BOOL => match s {
                "true" | "on" | "1" => Some(1.0),
                "false" | "off" | "0" => Some(0.0),
                _ => None,
            },
            ck::CHOICE => self.choices.iter().position(|c| c == s).map(|i| i as f64),
            _ => s.parse().ok(),
        }
    }

    pub fn show(&self, v: f64) -> String {
        match self.kind {
            ck::BOOL => (v != 0.0).to_string(),
            ck::CHOICE => self.choices.get(v as usize).cloned().unwrap_or_default(),
            ck::INT => format!("{}", v as i64),
            _ => {
                let s = format!("{v:.3}");
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
        }
    }
}

/// Value bits by config id.
static VALUES: OnceLock<Vec<AtomicU64>> = OnceLock::new();
/// Values of text settings by config id (empty for the others).
static TEXTS: RwLock<Vec<String>> = RwLock::new(Vec::new());
/// Counts changes, so whoever applies settings (the client) sees that something moved.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static RUN_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
/// Automated runs change settings for themselves only: they read the player's files and write
/// to `test-config/` beside them.
static AUTOMATED: AtomicBool = AtomicBool::new(false);
/// Values given on the command line for this run: (id, value in the file). Saving keeps the
/// file's value unless the setting was changed since.
static OVERRIDES: Mutex<Vec<(u32, String, String)>> = Mutex::new(Vec::new());
/// Values of the remote server this player is on, kept the same way until leaving it.
static FROM_SERVER: Mutex<Vec<(u32, String, String)>> = Mutex::new(Vec::new());

fn values() -> &'static [AtomicU64] {
    VALUES.get_or_init(|| {
        content()
            .game
            .configs
            .iter()
            .map(|d| AtomicU64::new(d.default.to_bits()))
            .collect()
    })
}

fn def(id: u32) -> Option<&'static ConfigDef> {
    content().game.configs.get(id as usize)
}

pub fn get(id: u32) -> f64 {
    values()
        .get(id as usize)
        .map_or(0.0, |v| f64::from_bits(v.load(Ordering::Relaxed)))
}

pub fn text(id: u32) -> String {
    TEXTS
        .read()
        .unwrap()
        .get(id as usize)
        .cloned()
        .unwrap_or_default()
}

/// Changes so far; compare with an earlier value to see whether anything was set.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Sets a value without saving (server values arriving, loading files).
pub fn put(id: u32, v: f64) {
    if let (Some(slot), Some(d)) = (values().get(id as usize), def(id)) {
        let bits = d.fit(v).to_bits();
        if slot.swap(bits, Ordering::Relaxed) != bits {
            GENERATION.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn put_text(id: u32, v: &str) {
    let mut t = TEXTS.write().unwrap();
    if let Some(slot) = t.get_mut(id as usize).filter(|s| s.as_str() != v) {
        *slot = v.to_string();
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// Sets a value and saves it where its scope keeps it (a world's are saved with the world).
pub fn set(id: u32, v: f64) {
    put(id, v);
    save_of(id);
}

pub fn set_text(id: u32, v: &str) {
    put_text(id, v);
    save_of(id);
}

fn save_of(id: u32) {
    let (Some(dir), Some(d)) = (RUN_DIR.get(), def(id)) else { return };
    if let (Some((ns, _)), false) = (d.name.split_once(':'), d.scope == scope::WORLD) {
        save(dir, ns);
    }
}

pub fn by_name(name: &str) -> Option<u32> {
    content().game.config_names.get(name).copied()
}

pub fn automated() -> bool {
    AUTOMATED.load(Ordering::Relaxed)
}

pub fn set_automated(on: bool) {
    AUTOMATED.store(on, Ordering::Relaxed);
}

/// A value for this run only (`--view`, `--no-vsync`), as the file writes it.
pub fn override_for_run(name: &str, value: &str) {
    let Some(id) = by_name(name) else { return };
    let saved = shown(id);
    put_value(id, value);
    let now = shown(id);
    OVERRIDES.lock().unwrap().push((id, now, saved));
}

/// A value the server decides, in force while connected and not written to this player's file.
pub fn put_from_server(id: u32, v: f64) {
    let saved = shown(id);
    put(id, v);
    let now = shown(id);
    if now != saved {
        let mut server = FROM_SERVER.lock().unwrap();
        // A later value from the server: what the file should keep is still the first one's.
        let saved = server
            .iter()
            .position(|o| o.0 == id)
            .map_or(saved, |i| server.remove(i).2);
        server.push((id, now, saved));
    }
}

/// Off a remote server: what it decided is over, the player's own settings are back.
pub fn left_server(run_dir: &Path) {
    FROM_SERVER.lock().unwrap().clear();
    load(run_dir);
}

/// A value as its file writes it.
fn shown(id: u32) -> String {
    match def(id) {
        Some(d) if d.kind == ck::TEXT => text(id),
        Some(d) => d.show(get(id)),
        None => String::new(),
    }
}

/// Puts a value written as text; false if it does not fit the setting.
fn put_value(id: u32, v: &str) -> bool {
    let Some(d) = def(id) else { return false };
    if d.kind == ck::TEXT {
        put_text(id, v.trim());
        return true;
    }
    match d.parse(v) {
        Some(n) => {
            put(id, n);
            true
        }
        None => false,
    }
}

/// Every setting back to its default, then each namespace's `config/<ns>.txt` (not the world's);
/// missing lines keep their defaults, unknown or bad ones are reported.
pub fn load(run_dir: &Path) {
    let _ = RUN_DIR.set(run_dir.to_path_buf());
    let c = content();
    {
        let mut t = TEXTS.write().unwrap();
        *t = c.game.configs.iter().map(|d| d.text.clone()).collect();
    }
    for (id, d) in c.game.configs.iter().enumerate() {
        put(id as u32, d.default);
    }
    let mut namespaces: Vec<&str> = c
        .game
        .configs
        .iter()
        .filter_map(|d| d.name.split_once(':').map(|x| x.0))
        .collect();
    namespaces.dedup();
    for ns in namespaces {
        let Ok(src) = std::fs::read_to_string(file(run_dir, false, ns)) else { continue };
        for (_, k, v) in crate::kv::pairs(&src) {
            let name = format!("{ns}:{k}");
            match by_name(&name).filter(|&id| c.game.configs[id as usize].scope != scope::WORLD) {
                Some(id) => {
                    if !put_value(id, v) {
                        let d = &c.game.configs[id as usize];
                        log::warn!("config {name}: cannot use '{}', keeping {}", v.trim(), d.show(d.default));
                    }
                }
                None => log::warn!("config/{ns}.txt: no setting {}", k.trim()),
            }
        }
    }
    for (id, value, _) in OVERRIDES.lock().unwrap().iter() {
        put_value(*id, value);
    }
}

fn file(run_dir: &Path, automated: bool, ns: &str) -> std::path::PathBuf {
    let dir = if automated { run_dir.join("test-config") } else { crate::paths::config(run_dir) };
    dir.join(format!("{ns}.txt"))
}

/// Writes a namespace's settings, every one of them so players see what exists.
pub fn save(run_dir: &Path, ns: &str) {
    let c = content();
    let (overrides, server) = (OVERRIDES.lock().unwrap().clone(), FROM_SERVER.lock().unwrap().clone());
    let mut out = String::new();
    for (id, d) in c
        .game
        .configs
        .iter()
        .enumerate()
        .filter(|(_, d)| d.scope != scope::WORLD)
    {
        let Some(key) = d.name.strip_prefix(ns).and_then(|k| k.strip_prefix(':')) else { continue };
        let kept = |list: &[(u32, String, String)], now: String| {
            list.iter()
                .find(|o| o.0 == id as u32 && o.1 == now)
                .map_or(now.clone(), |o| o.2.clone())
        };
        let value = kept(&overrides, kept(&server, shown(id as u32)));
        out.push_str(&format!("{key} = {value}\n"));
    }
    let path = file(run_dir, AUTOMATED.load(Ordering::Relaxed), ns);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Err(e) = std::fs::write(&path, out) {
        log::error!("saving {}: {e}", path.display());
    }
}

/// Values of the settings the server decides, for its players.
pub fn synced() -> Vec<(u32, f64)> {
    content()
        .game
        .configs
        .iter()
        .enumerate()
        .filter(|(_, d)| d.synced() && d.kind != ck::TEXT)
        .map(|(i, _)| (i as u32, get(i as u32)))
        .collect()
}

/// A world's settings as it saves them.
pub fn world_values() -> Vec<(String, f64)> {
    content()
        .game
        .configs
        .iter()
        .enumerate()
        .filter(|(_, d)| d.scope == scope::WORLD)
        .map(|(i, d)| (d.name.clone(), get(i as u32)))
        .collect()
}

/// Opening a world: its settings as it saved them, defaults for the rest.
pub fn load_world(saved: &[(String, f64)]) {
    for (id, d) in content()
        .game
        .configs
        .iter()
        .enumerate()
        .filter(|(_, d)| d.scope == scope::WORLD)
    {
        put(
            id as u32,
            saved
                .iter()
                .find(|s| s.0 == d.name)
                .map_or(d.default, |s| s.1),
        );
    }
}

/// Settings the engine itself reads: the client's options, `core:mob_spawning` for worlds, and
/// a binding per key (`core:key.<action>`, from `keys`: action and default key); each sound
/// category adds its volume (`volume`).
pub fn engine_configs(keys: &[(String, String)]) -> Vec<RawEntry> {
    use weft_abi::comp::config as c;
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "Player".into());
    // A phone's GPU spends most of its time on distant terrain's triangles and on pixels.
    let phone = cfg!(target_os = "android");
    let mut out = vec![
        volume("core:volume_master").set(c::ORDER, -1),
        string("core:language", &crate::settings::system_language()),
        number("core:gui_scale", ck::INT, 0.0, 0.0, 16.0, 1.0),
        // The world drawn at this part of the screen's size and stretched over it; the interface
        // stays sharp. What a phone's GPU needs, most of its time going to pixels.
        number("core:render_scale", ck::FLOAT, if phone { 0.6 } else { 1.0 }, 0.25, 1.0, 0.05),
        number("core:view_distance", ck::INT, 12.0, 2.0, 64.0, 1.0),
        // Distant terrain beyond the view (`client::lod`), in cubes; 0 off.
        number("core:lod_distance", ck::INT, if phone { 32.0 } else { 256.0 }, 0.0, 2048.0, 16.0),
        // How near distant terrain's tiles split into finer ones, in tile widths (`client::lod`).
        number("core:lod_detail", ck::FLOAT, if phone { 2.0 } else { 4.0 }, 1.0, 4.0, 0.5),
        // Blocks clouds float above where their dimension puts them.
        number("core:cloud_height", ck::INT, 0.0, 0.0, 512.0, 16.0),
        number("core:cloud_distance", ck::INT, if phone { 2048.0 } else { 6144.0 }, 0.0, 8192.0, 16.0),
        // Vanilla's: in GUI pixels, and percent of the dark behind the text.
        number("core:chat_width", ck::INT, 320.0, 40.0, 1000.0, 1.0),
        number("core:chat_height", ck::INT, 180.0, 20.0, 1000.0, 1.0),
        number("core:chat_height_closed", ck::INT, 110.0, 20.0, 1000.0, 1.0),
        number("core:chat_opacity", ck::INT, 50.0, 0.0, 100.0, 1.0),
        number("core:chat_visibility", ck::CHOICE, 0.0, 0.0, 2.0, 0.0)
            .set(c::CHOICES, ["all", "system", "hidden"].map(String::from).to_vec()),
        number("core:vsync", ck::BOOL, 1.0, 0.0, 1.0, 0.0),
        // Taken at the next start; the others are tried if it can't run.
        number("core:renderer", ck::CHOICE, 0.0, 0.0, 1.0, 0.0).set(c::CHOICES, ["vulkan", "wgpu"].map(String::from).to_vec()),
        number("core:fullscreen", ck::BOOL, 0.0, 0.0, 1.0, 0.0),
        number("core:terrain_gpu", ck::BOOL, 1.0, 0.0, 1.0, 0.0),
        number("core:terrain_check", ck::BOOL, 0.0, 0.0, 1.0, 0.0),
        number("core:client_visibility", ck::INT, 0.0, 0.0, 2.0, 1.0),
        number("core:fov", ck::FLOAT, 70.0, 30.0, 110.0, 1.0),
        // How much sprinting, flying and speed widen the view, as vanilla's FOV effects.
        number("core:fov_effects", ck::FLOAT, 1.0, 0.0, 1.0, 0.0),
        number("core:sensitivity", ck::FLOAT, 1.0, 0.01, 3.0, 0.01),
        number("core:brightness", ck::FLOAT, 0.5, 0.0, 1.0, 0.0),
        number("core:debug_groups", ck::INT, u32::MAX as f64, 0.0, u32::MAX as f64, 1.0),
        number("core:advanced_tooltips", ck::BOOL, 0.0, 0.0, 1.0, 0.0),
        number("core:invert_y", ck::BOOL, 0.0, 0.0, 1.0, 0.0),
        // Buttons and steering for fingers (`touch`), on where the screen is the only input.
        number("core:touch_controls", ck::BOOL, cfg!(target_os = "android") as u8 as f64, 0.0, 1.0, 0.0),
        string("core:debug_lines", ""),
        string("core:name", &user),
        string("core:relays", ""),
        // Where others reach this game by address; forward it, TCP and UDP.
        number("core:server_port", ck::INT, 27182.0, 1.0, 65535.0, 1.0),
        string("core:resource_packs", ""),
        string("core:shader_pack", ""),
        number("core:mob_spawning", ck::BOOL, 1.0, 0.0, 1.0, 0.0).set(c::SCOPE, scope::WORLD),
        number("core:simulation_distance", ck::INT, 8.0, 2.0, 64.0, 1.0).set(c::SCOPE, scope::WORLD),
        // Most cubes mods' tickets keep loaded with no player near (`Host::force_load`).
        number("core:max_forced_cubes", ck::INT, 4096.0, 0.0, 1048576.0, 1.0).set(c::SCOPE, scope::WORLD),
        number("core:first_person_model", ck::BOOL, 1.0, 0.0, 1.0, 0.0),
        number("core:show_avatars", ck::BOOL, 1.0, 0.0, 1.0, 0.0),
        number("core:occlusion_culling", ck::BOOL, 1.0, 0.0, 1.0, 0.0),
        // Off, what is behind the camera is drawn too: for shaders that bend the world around.
        number("core:frustum_culling", ck::BOOL, 1.0, 0.0, 1.0, 0.0),
        number("core:allow_avatars", ck::BOOL, 1.0, 0.0, 1.0, 0.0).set(c::SCOPE, scope::WORLD),
        number("core:keep_inventory", ck::BOOL, 0.0, 0.0, 1.0, 0.0).set(c::SCOPE, scope::WORLD),
        // Vanilla's: more than this many pushing on one hurt it (0: never).
        rule("core:max_entity_cramming", ck::INT, 24.0, 0.0, 1000.0),
        // Deaths a player has before staying a spectator (0: no end); 1 is hardcore.
        rule("core:lives", ck::INT, 0.0, 0.0, 100.0),
        // Set when the world is made, nobody changes the rules after.
        rule("core:rules_locked", ck::BOOL, 0.0, 0.0, 1.0),
    ];
    out.extend(
        keys.iter()
            .map(|(action, key)| string(&format!("core:key.{action}"), key)),
    );
    // The settings pages, in the order they show.
    let pages: [(&str, &[&str]); 6] = [
        // What anyone changes now and then; the rest is a page away.
        ("core:settings", &["fov", "view_distance"]),
        (
            "core:video",
            &[
                "gui_scale",
                "render_scale",
                "brightness",
                "lod_distance",
                "lod_detail",
                "cloud_height",
                "cloud_distance",
                "vsync",
                "fullscreen",
                "renderer",
            ],
        ),
        ("core:controls", &["sensitivity", "invert_y", "touch_controls"]),
        ("core:accessibility", &["fov_effects"]),
        ("core:profile", &["first_person_model", "show_avatars"]),
        ("core:chat_settings", &["chat_width", "chat_height", "chat_height_closed", "chat_opacity", "chat_visibility"]),
    ];
    let percent = ["core:sensitivity", "core:brightness", "core:fov_effects", "core:render_scale"];
    // Applied on the way, the scale would move the slider under the cursor.
    let on_release = ["core:gui_scale"];
    out.into_iter()
        .map(|e| {
            let at = pages.iter().find_map(|(page, names)| {
                names
                    .iter()
                    .position(|n| e.name.strip_prefix("core:") == Some(*n))
                    .map(|i| (*page, i as i32))
            });
            let e = if percent.contains(&e.name.as_str()) { e.tag(c::PERCENT) } else { e };
            let e = if on_release.contains(&e.name.as_str()) { e.tag(c::ON_RELEASE) } else { e };
            match at {
                Some((page, order)) => e.set(c::PAGE, page.to_string()).set(c::ORDER, order),
                None => e,
            }
        })
        .collect()
}

/// A world rule (`/rule`).
pub fn rule(name: &str, kind: u32, default: f64, min: f64, max: f64) -> RawEntry {
    let step = if kind == ck::FLOAT { 0.0 } else { 1.0 };
    number(name, kind, default, min, max, step).set(weft_abi::comp::config::SCOPE, scope::WORLD)
}

/// A volume setting in percent, on the sound page.
pub fn volume(name: &str) -> RawEntry {
    number(name, ck::INT, 100.0, 0.0, 100.0, 1.0).set(weft_abi::comp::config::PAGE, "core:sound".to_string())
}

fn number(name: &str, kind: u32, default: f64, min: f64, max: f64, step: f64) -> RawEntry {
    use weft_abi::comp::config as c;
    RawEntry::engine(weft_abi::entry_kind::CONFIG, name)
        .set(c::KIND, kind)
        .set(c::DEFAULT, default)
        .set(c::RANGE, [min, max])
        .set(c::STEP, step)
}

fn string(name: &str, default: &str) -> RawEntry {
    number(name, ck::TEXT, 0.0, 0.0, 0.0, 0.0).set(weft_abi::comp::config::TEXT, default.to_string())
}

/// A setting from its entry. The engine's defaults stand as written; a mod's are made to fit.
pub fn bake(e: &Entry, errors: &mut Vec<String>) -> ConfigDef {
    use weft_abi::comp::config as c;
    let [min, max] = e.or(c::RANGE, [0.0, 1.0]);
    let mut d = ConfigDef {
        name: e.name.clone(),
        kind: e.or(c::KIND, ck::BOOL),
        default: e.or(c::DEFAULT, 0.0),
        min,
        max,
        step: e.or(c::STEP, 0.0),
        choices: e.or(c::CHOICES, Vec::new()),
        flags: e.flags(crate::tag_bits::config::TAGS),
        scope: e.or(c::SCOPE, scope::CLIENT),
        text: e.text(c::TEXT).unwrap_or_default().to_string(),
        page: e.text(c::PAGE).unwrap_or_default().to_string(),
        order: e.or(c::ORDER, 0),
    };
    if d.kind == ck::TEXT && d.scope != scope::CLIENT {
        errors.push(format!("setting {}: text settings are the client's own", d.name));
        d.scope = scope::CLIENT;
    }
    if e.owner != crate::registry::ENGINE {
        d.default = d.fit(d.default);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_fit_their_setting() {
        let f = crate::entries::fold("setting", vec![number("t:x", ck::INT, 5.0, 0.0, 10.0, 2.0)], &|_| String::new(), &mut Vec::new());
        let d = bake(&f.entries[0], &mut Vec::new());
        assert_eq!((d.fit(3.2), d.fit(99.0), d.fit(f64::NAN)), (4.0, 10.0, 5.0));
        let c = ConfigDef { kind: ck::CHOICE, choices: vec!["low".into(), "high".into()], ..d.clone() };
        assert_eq!((c.parse("high"), c.parse("mid"), c.show(0.0)), (Some(1.0), None, "low".into()));
        let b = ConfigDef { kind: ck::BOOL, ..d };
        assert_eq!((b.parse("on"), b.show(1.0)), (Some(1.0), "true".into()));
    }
}
