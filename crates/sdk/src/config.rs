//! Settings: declared once, read with one call, edited by players in Settings > Mod Settings (a
//! screen built from the declarations). Where a value lives is its scope: the player's own
//! (`config/<ns>.txt`, the default), the server's (`.server()`, its players get it and every
//! change), or the world's (`.world()`, saved with it, changed with `/rule`: game rules).
//! The engine's own settings are the same thing, in `settings::*`.
//!
//! ```ignore
//! static MAX_WISPS: Config<i32> = Config::named("max_wisps");
//! r.config(&MAX_WISPS).int(8, 0, 64).server();
//! if count < MAX_WISPS.get() { .. }
//! ```
//! Names show as `config.<ns>.<key>` (tooltip `.tooltip`, choices `.<choice>`) from the language
//! files.

use std::marker::PhantomData;

use crate::abi::{self, Str, config_kind as ck, config_scope as scope};
use crate::comps::config as c;
use crate::content::{Handle, kind};
use crate::{Registry, WithComponents, host};

/// Types a setting can be read as.
pub trait ConfigValue: Copy {
    fn from_f64(v: f64) -> Self;
    fn to_f64(self) -> f64;
}

macro_rules! numbers {
    ($($t:ty),*) => {$(
        impl ConfigValue for $t {
            fn from_f64(v: f64) -> Self { v as $t }
            fn to_f64(self) -> f64 { self as f64 }
        }
    )*};
}
numbers!(i32, i64, u32, f32, f64);

impl ConfigValue for bool {
    fn from_f64(v: f64) -> Self {
        v != 0.0
    }
    fn to_f64(self) -> f64 {
        self as u8 as f64
    }
}

pub struct Config<T> {
    h: Handle<kind::Config>,
    _t: PhantomData<fn() -> T>,
}

impl<T: ConfigValue> Config<T> {
    /// This mod's setting (`"sparks"`) or another's (`"othermod:sparks"`).
    pub const fn named(name: &'static str) -> Self {
        Config { h: Handle::named(name), _t: PhantomData }
    }

    pub fn id(&self) -> u32 {
        self.h.id()
    }

    pub fn get(&self) -> T {
        T::from_f64(unsafe { (host().config_value)(self.h.id()) })
    }

    /// Changes and saves it (clamped to its range); whoever reads it sees the change.
    pub fn set(&self, v: T) {
        unsafe { (host().config_set)(self.h.id(), v.to_f64()) }
    }
}

/// A text setting (`ConfigBuilder::text`), the player's own.
pub struct TextConfig {
    h: Handle<kind::Config>,
}

impl TextConfig {
    pub const fn named(name: &'static str) -> Self {
        TextConfig { h: Handle::named(name) }
    }
    pub fn get(&self) -> String {
        unsafe { (host().config_text)(self.h.id()).as_str().to_string() }
    }
    pub fn set(&self, v: &str) {
        unsafe { (host().config_set_text)(self.h.id(), Str::new(v)) }
    }
}

/// The engine's settings, read and changed like a mod's; the client puts changes into effect
/// by itself.
pub mod settings {
    use super::{Config, TextConfig};
    pub static LANGUAGE: TextConfig = TextConfig::named("core:language");
    /// 0 = automatic.
    pub static GUI_SCALE: Config<u32> = Config::named("core:gui_scale");
    /// Which chat lines show: 0 all, 1 no players' messages, 2 none but replies to one's commands.
    pub static CHAT_VISIBILITY: Config<u32> = Config::named("core:chat_visibility");
    pub static VIEW_DISTANCE: Config<i32> = Config::named("core:view_distance");
    /// Native cloud fog distance in blocks; environment sources can override it. Zero hides clouds.
    pub static CLOUD_DISTANCE: Config<i32> = Config::named("core:cloud_distance");
    pub static VSYNC: Config<bool> = Config::named("core:vsync");
    pub static FOV: Config<f32> = Config::named("core:fov");
    pub static SENSITIVITY: Config<f32> = Config::named("core:sensitivity");
    pub static BRIGHTNESS: Config<f32> = Config::named("core:brightness");
    /// Bit per F3 overlay group shown.
    pub static DEBUG_GROUPS: Config<u32> = Config::named("core:debug_groups");
    pub static ADVANCED_TOOLTIPS: Config<bool> = Config::named("core:advanced_tooltips");
    pub static NAME: TextConfig = TextConfig::named("core:name");
    /// Enabled resource packs, highest priority first, comma separated.
    /// When each F3 line shows: `fps=always,cube=never`.
    pub static DEBUG_LINES: TextConfig = TextConfig::named("core:debug_lines");
    pub static RESOURCE_PACKS: TextConfig = TextConfig::named("core:resource_packs");
    pub static SHADER_PACK: TextConfig = TextConfig::named("core:shader_pack");
    pub static VOLUME_MASTER: Config<u32> = Config::named("core:volume_master");
    /// A world's: mobs appear by themselves.
    pub static MOB_SPAWNING: Config<bool> = Config::named("core:mob_spawning");
    /// Deaths a player has before staying a spectator (0: no end); 1 is hardcore. A world rule,
    /// the server's on its clients.
    pub static LIVES: Config<u32> = Config::named("core:lives");
    pub static KEEP_INVENTORY: Config<bool> = Config::named("core:keep_inventory");
}

pub struct ConfigBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(Config => ConfigBuilder, config_builder;);

impl ConfigBuilder<'_> {
    pub fn bool(self, default: bool) -> Self {
        self.set(&c::KIND, ck::BOOL)
            .set(&c::DEFAULT, default as u8 as f64)
    }
    pub fn int(self, default: i64, min: i64, max: i64) -> Self {
        self.set(&c::KIND, ck::INT)
            .set(&c::DEFAULT, default as f64)
            .set(&c::RANGE, [min as f64, max as f64])
            .set(&c::STEP, 1.0)
    }
    pub fn float(self, default: f64, min: f64, max: f64) -> Self {
        self.set(&c::KIND, ck::FLOAT)
            .set(&c::DEFAULT, default)
            .set(&c::RANGE, [min, max])
    }
    /// Values the slider snaps to, and how far the arrow keys move it (`int` has 1).
    pub fn step(self, step: f64) -> Self {
        self.set(&c::STEP, step)
    }
    /// One of `choices`, read as its index.
    pub fn choice(self, default: u32, choices: &[&str]) -> Self {
        self.set(&c::KIND, ck::CHOICE)
            .set(&c::DEFAULT, default as f64)
            .set(&c::CHOICES, choices.iter().map(|c| c.to_string()).collect())
    }
    /// Text, read with a `TextConfig`; the player's own.
    pub fn text(self, default: &str) -> Self {
        self.set(&c::KIND, ck::TEXT).set_text(&c::TEXT, default)
    }
    /// The server's value counts for everyone on it, and its changes reach them.
    pub fn server(self) -> Self {
        self.set(&c::SCOPE, scope::SERVER)
    }
    /// Saved with the world and changed there with `/rule` (vanilla's game rules); its players
    /// get it like the server's.
    pub fn world(self) -> Self {
        self.set(&c::SCOPE, scope::WORLD)
    }
    /// Only read at start: the screen says a restart is needed.
    pub fn restart_required(self) -> Self {
        self.tag(abi::comp::config::RESTART_REQUIRED)
    }
    /// Its slider takes the value when let go, not on its way there (a value that changes what
    /// the slider is drawn by, as the GUI scale).
    pub fn on_release(self) -> Self {
        self.tag(abi::comp::config::ON_RELEASE)
    }
}

impl Registry {
    /// A setting, see `ConfigBuilder` (a bool `false` unless told otherwise).
    pub fn config<'a, T: ConfigValue>(&'a mut self, config: &'static Config<T>) -> ConfigBuilder<'a> {
        self.config_builder(&config.h)
    }

    /// A text setting, see `ConfigBuilder::text`.
    pub fn text_config<'a>(&'a mut self, config: &'static TextConfig, default: &str) -> ConfigBuilder<'a> {
        self.config_builder(&config.h).text(default)
    }

    fn config_builder(&mut self, h: &'static Handle<kind::Config>) -> ConfigBuilder<'_> {
        ConfigBuilder { reg: self, e: crate::entry::Entry::of(h) }
    }
}
