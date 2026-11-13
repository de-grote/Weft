//! The client's options as the engine reads them: typed views of its `core:` settings
//! (`config::engine_configs`), which live in `config/core.txt` like every mod's. Setting one
//! saves it; the client applies what changed by itself (`Running::apply_settings`).

use std::path::{Path, PathBuf};

use crate::config;
use crate::input::{Binding, Input, Slots};

#[derive(Clone)]
pub struct Settings {
    pub run_dir: PathBuf,
    /// An automated run: it saves nothing and never takes the real mouse.
    pub read_only: bool,
}

/// The largest GUI scale a window shows whole (vanilla's 320x240 per step); auto uses it.
pub fn max_gui_scale(window: (u32, u32)) -> u32 {
    (window.0 / 320).min(window.1 / 240).max(1)
}

fn id(name: &str) -> u32 {
    config::by_name(name).unwrap_or(u32::MAX)
}

fn number(name: &str) -> f64 {
    config::get(id(name))
}

/// How the chat is shown, as its settings have it.
#[derive(Clone, Copy, PartialEq)]
pub struct ChatLook {
    pub width: f32,
    /// The most it takes up open, and shown over the game.
    pub open_height: f32,
    pub closed_height: f32,
    /// Of the dark behind it, 0..255.
    pub dim: u8,
}

/// Which chat lines a player wants.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChatVisibility {
    All,
    System,
    Hidden,
}

pub fn chat_visibility() -> ChatVisibility {
    match number("core:chat_visibility") as u32 {
        0 => ChatVisibility::All,
        1 => ChatVisibility::System,
        _ => ChatVisibility::Hidden,
    }
}

pub fn chat_look() -> ChatLook {
    ChatLook {
        width: number("core:chat_width") as f32,
        open_height: number("core:chat_height") as f32,
        closed_height: number("core:chat_height_closed") as f32,
        dim: (number("core:chat_opacity") * 2.55) as u8,
    }
}

fn list(name: &str) -> Vec<String> {
    config::text(id(name))
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

impl Settings {
    pub fn new(run_dir: &Path) -> Settings {
        Settings { run_dir: run_dir.to_path_buf(), read_only: false }
    }

    pub fn saves_dir(&self) -> PathBuf {
        self.run_dir.join("saves")
    }
}

pub fn language() -> String {
    config::text(id("core:language"))
}
/// 0 = automatic.
pub fn gui_scale() -> u32 {
    number("core:gui_scale") as u32
}
/// The part of the screen's size the world is drawn at.
pub fn render_scale() -> f32 {
    number("core:render_scale") as f32
}
pub fn view_distance() -> i32 {
    number("core:view_distance") as i32
}
pub fn vsync() -> bool {
    number("core:vsync") != 0.0
}
/// The chosen renderer's name (`core:renderer`).
pub fn renderer() -> String {
    let id = id("core:renderer");
    crate::content::content()
        .game
        .configs
        .get(id as usize)
        .map(|d| d.show(config::get(id)))
        .unwrap_or_default()
}
pub fn fov() -> f32 {
    number("core:fov") as f32
}
pub fn fov_effects() -> f32 {
    number("core:fov_effects") as f32
}
pub fn sensitivity() -> f32 {
    number("core:sensitivity") as f32
}
/// 0 moody .. 1 bright.
pub fn brightness() -> f32 {
    number("core:brightness") as f32
}
/// Bit per debug overlay group shown (F3 + number).
pub fn debug_groups() -> u32 {
    number("core:debug_groups") as u32
}
pub fn advanced_tooltips() -> bool {
    number("core:advanced_tooltips") != 0.0
}
/// Mouse up looks down instead.
pub fn invert_y() -> bool {
    number("core:invert_y") != 0.0
}
/// The HUD's buttons and steering for fingers.
pub fn touch_controls() -> bool {
    number("core:touch_controls") != 0.0
}
pub fn name() -> String {
    config::text(id("core:name"))
}
pub fn server_port() -> u16 {
    number("core:server_port") as u16
}
/// Extra iroh relay URLs; empty uses the public defaults.
pub fn relays() -> Vec<String> {
    list("core:relays")
}
/// Enabled resource packs, highest priority first.
pub fn resource_packs() -> Vec<String> {
    list("core:resource_packs")
}
pub fn shader_pack() -> Option<String> {
    Some(config::text(id("core:shader_pack"))).filter(|p| !p.is_empty())
}

pub fn occlusion_culling() -> bool {
    number("core:occlusion_culling") != 0.0
}
pub fn frustum_culling() -> bool {
    number("core:frustum_culling") != 0.0
}

pub fn show_avatars() -> bool {
    number("core:show_avatars") != 0.0
}

pub fn first_person_model() -> bool {
    number("core:first_person_model") != 0.0
}

pub fn set(name: &str, v: f64) {
    config::set(id(&format!("core:{name}")), v);
}
pub fn set_text(name: &str, v: &str) {
    config::set_text(id(&format!("core:{name}")), v);
}
pub fn set_resource_packs(packs: &[String]) {
    set_text("resource_packs", &packs.join(","));
}

/// Every key as its `core:key.<action>` setting says.
pub fn apply_binds(input: &mut Input) {
    let actions: Vec<String> = input.binds.iter().map(|b| b.action.clone()).collect();
    for action in actions {
        match Binding::parse_slots(&config::text(id(&format!("core:key.{action}")))) {
            Some(k) => input.rebind(&action, k),
            None => log::warn!("key binding {action}: unknown key {}", config::text(id(&format!("core:key.{action}")))),
        }
    }
}

pub fn set_bind(action: &str, keys: &Slots) {
    config::set_text(id(&format!("core:key.{action}")), &Binding::slots_id(keys));
}

/// The system's language as a lang code (`ru_ru`), English when the system doesn't say.
pub fn system_language() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
        .or_else(os_language)
        .and_then(|locale| lang_code(&locale))
        .unwrap_or_else(|| crate::ui::lang::FALLBACK.into())
}

/// `ru_RU.UTF-8` and `ru-RU` are `ru_ru`; `C`, `POSIX` and a language without a region are none.
fn lang_code(locale: &str) -> Option<String> {
    let code = locale
        .split(['.', '@'])
        .next()?
        .replace('-', "_")
        .to_lowercase();
    let (lang, region) = code.split_once('_')?;
    (!lang.is_empty() && !region.is_empty()).then_some(code)
}

#[cfg(windows)]
fn os_language() -> Option<String> {
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
        fn LCIDToLocaleName(lcid: u32, name: *mut u16, len: i32, flags: u32) -> i32;
    }
    const LOCALE_NAME_MAX_LENGTH: usize = 85;
    let mut name = [0u16; LOCALE_NAME_MAX_LENGTH];
    // Characters written with the closing zero, 0 when the language has no name.
    let written = unsafe { LCIDToLocaleName(GetUserDefaultUILanguage().into(), name.as_mut_ptr(), name.len() as i32, 0) };
    (written > 1).then(|| String::from_utf16_lossy(&name[..written as usize - 1]))
}

#[cfg(not(windows))]
fn os_language() -> Option<String> {
    None
}
