//! Safe Rust API for writing Weft mods. The base game uses exactly this crate.

pub use weft_abi as abi;

pub mod ai;
pub mod camera;
pub mod client;
mod component;
pub mod riding;
pub mod shape;
pub mod shapes;

pub use weft_noise as noise;
mod command;
pub mod comps;
mod config;
mod content;
mod effects;
pub mod environment;
pub use environment::{ENVIRONMENT, Environment, EnvironmentValues};
mod entry;
mod extend;
pub mod facing;
mod feature;
mod game;
mod generation;
mod out;
mod show;
pub mod terrain;
mod ui;
mod view;
mod world;

pub use abi::{
    BlockPos, Blocking, EmbeddedProjectile, EntitySize, Holder, ItemStack, OpenMenu, ProjectileCatch, Rect, UV_FROM_BOX, UV_NO_FACE, Vec3,
    Who, hitbox_part, hitbox_part_id, hud_anchor, tool_tier,
};
pub use camera::{Shot, cameras, easing, shot};
pub use client::{Channel, Client, DisplaySource, Hand, Packet, Payload, keys, launch_option, screens};
pub use command::{ClientCommand, Command, CommandOptions, ServerCommand};
pub use component::*;
pub use config::*;
pub use content::*;
pub use effects::*;
pub use extend::*;
pub use facing::{Dir, Facings, Orientation, Toward};
pub use feature::*;
pub use game::*;
pub use generation::Generation;
pub use out::{Drawing, Output};
pub use riding::{Exit, RiderRules, Seat, Steering, steer};
pub use shape::{EntityFilter, Fill, Shape};
pub use show::{ChatFormat, Show, TextPlaceBuilder, text_places};
pub use ui::*;
pub use view::View;
pub use weft_noise::cache::ColumnCache;
pub use world::*;

use std::ffi::c_void;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use abi::{Callback, Host, ModId, Str};

static HOST: AtomicPtr<Host> = AtomicPtr::new(std::ptr::null_mut());
static ME: AtomicU32 = AtomicU32::new(u32::MAX);
static MOD_ID: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
/// The registrar while `init` runs, so the SDK can report mistakes as load errors.
static REGISTRAR: AtomicPtr<abi::Registrar> = AtomicPtr::new(std::ptr::null_mut());

pub fn host() -> &'static Host {
    let p = HOST.load(Ordering::Relaxed);
    assert!(!p.is_null(), "weft host used before mod init");
    unsafe { &*p }
}

pub fn mod_id() -> &'static str {
    MOD_ID.get().copied().unwrap_or("?")
}

fn mod_handle() -> ModId {
    ModId(ME.load(Ordering::Relaxed))
}

#[doc(hidden)]
pub fn __log(level: u32, target: &str, args: std::fmt::Arguments) {
    let msg = std::fmt::format(args);
    unsafe { (host().log_at)(mod_handle(), level, Str::new(target), Str::new(&msg)) }
}

/// Whether a record at `level` (`abi::log_level::*`) under this mod's `target` (`""` for the mod
/// itself) would be written: players switch these with `--log mod:<id>::<target>=debug`.
pub fn log_enabled(level: u32, target: &str) -> bool {
    unsafe { (host().log_enabled)(mod_handle(), level, Str::new(target)) != 0 }
}

/// `info!("..")` logs as `mod:<id>`, `info!(target: "pipes", "..")` as `mod:<id>::pipes`; the
/// message is only formatted when that target is enabled at that level.
#[macro_export]
macro_rules! __log_macro {
    ($lvl:expr, target: $t:expr, $($a:tt)*) => {
        if $lvl <= $crate::abi::log_level::INFO || $crate::log_enabled($lvl, $t) { $crate::__log($lvl, $t, format_args!($($a)*)) }
    };
    ($lvl:expr, $($a:tt)*) => { $crate::__log_macro!($lvl, target: "", $($a)*) };
}
#[macro_export]
macro_rules! error { ($($t:tt)*) => { $crate::__log_macro!($crate::abi::log_level::ERROR, $($t)*) } }
#[macro_export]
macro_rules! warn { ($($t:tt)*) => { $crate::__log_macro!($crate::abi::log_level::WARN, $($t)*) } }
#[macro_export]
macro_rules! info { ($($t:tt)*) => { $crate::__log_macro!($crate::abi::log_level::INFO, $($t)*) } }
#[macro_export]
macro_rules! debug { ($($t:tt)*) => { $crate::__log_macro!($crate::abi::log_level::DEBUG, $($t)*) } }
#[macro_export]
macro_rules! trace { ($($t:tt)*) => { $crate::__log_macro!($crate::abi::log_level::TRACE, $($t)*) } }

/// Declares the mod entry point. The mod's id is the crate's name (also the library's and the
/// mod folder's), its version the crate's version; players see the name from the language file
/// (`mod.<id>.name`).
///
/// ```ignore
/// weft_sdk::declare_mod!(deps: ["base"], init: init);
/// ```
///
/// `deps` are needed: without one the mod is off. `after` and `before` only order it among mods
/// that are there, the later one's edits winning where two change one thing:
///
/// ```ignore
/// weft_sdk::declare_mod!(deps: ["base"], after: ["othermod"], before: [], init: init);
/// ```
#[macro_export]
macro_rules! declare_mod {
    (deps: [$($dep:literal),* $(,)?], $(after: [$($after:literal),* $(,)?],)? $(before: [$($before:literal),* $(,)?],)? init: $init:path $(,)?) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn weft_mod() -> *const $crate::abi::ModDecl {
            static DEPS: &[$crate::abi::Str] = &[$($crate::abi::Str::new($dep)),*];
            static AFTER: &[$crate::abi::Str] = &[$($($crate::abi::Str::new($after)),*)?];
            static BEFORE: &[$crate::abi::Str] = &[$($($crate::abi::Str::new($before)),*)?];
            unsafe extern "C" fn __weft_init(
                host: *const $crate::abi::Host,
                me: $crate::abi::ModId,
                reg: *mut $crate::abi::Registrar,
            ) -> u32 {
                unsafe { $crate::__init(host, me, reg, env!("CARGO_CRATE_NAME"), $init) }
            }
            static DECL: $crate::abi::ModDecl = $crate::abi::ModDecl {
                abi_version: $crate::abi::ABI_VERSION,
                id: $crate::abi::Str::new(env!("CARGO_CRATE_NAME")),
                version: $crate::abi::Str::new(env!("CARGO_PKG_VERSION")),
                deps: $crate::abi::Slice::new(DEPS),
                after: $crate::abi::Slice::new(AFTER),
                before: $crate::abi::Slice::new(BEFORE),
                init: __weft_init,
            };
            &DECL
        }
    };
}

#[doc(hidden)]
pub unsafe fn __init(host: *const Host, me: ModId, reg: *mut abi::Registrar, id: &'static str, f: fn(&mut Registry)) -> u32 {
    HOST.store(host as *mut Host, Ordering::Relaxed);
    ME.store(me.0, Ordering::Relaxed);
    let _ = MOD_ID.set(id);
    install_panic_hook();
    let mut r = Registry { raw: reg, ns: id, derived: Vec::new() };
    REGISTRAR.store(reg, Ordering::Relaxed);
    let result = guard(|| f(&mut r));
    REGISTRAR.store(std::ptr::null_mut(), Ordering::Relaxed);
    result
}

/// A mistake in a registration: a load error while the mod registers, a logged error later.
pub(crate) fn reg_mistake(message: &str) {
    let reg = REGISTRAR.load(Ordering::Relaxed);
    if reg.is_null() {
        error!("{message}");
    } else {
        unsafe { (host().reg_mistake)(reg, Str::new(message)) }
    }
}

/// Texture folders: an asset name never starts with one of them, the call already picks it.
const TEXTURE_FOLDERS: [&str; 5] = ["block", "item", "entity", "gui", "particle"];

/// Whether texture `name` (`beauty_of_nature:block/log_oak_inner`, or without a namespace this
/// mod's) is in its mod's files: to make something of every mod's logs only where they have the
/// picture for it. Resource packs don't count; they are each player's.
pub fn texture_exists(name: &str) -> bool {
    asset_exists(name, "textures", "png")
}

/// `texture_exists` for any asset: `asset_exists("othermod:hollow_log", "models", "json")`.
pub fn asset_exists(name: &str, kind: &str, ext: &str) -> bool {
    let full = if name.contains(':') { name.to_string() } else { format!("{}:{name}", mod_id()) };
    unsafe { (host().asset_exists)(abi::Str::new(&full), abi::Str::new(kind), abi::Str::new(ext)) != 0 }
}

/// Every asset argument goes through here. The call implies a folder (`block`, `item`, `entity`,
/// `gui`, `particle`, or one of the asset kinds that have their own root: sounds, models,
/// animations, shaders): `"x"` is that folder of this mod, `"other:x"` the same folder of mod
/// `other`, and subfolders are fine (`"armor/iron_1"`). Leaving the folder (`"block/x"` for an
/// item, `..`, an absolute path) is a load error. Gives `ns:folder/path` (or `ns:path` for an
/// empty folder).
pub(crate) fn asset(folder: &str, name: &str) -> String {
    // Said of the source by a block made of others, the engine fills it in (`{source.side}`).
    if name.contains("{source") {
        return name.to_string();
    }
    let (ns, path) = name.split_once(':').unwrap_or((mod_id(), name));
    if let Err(why) = asset_check(folder, ns, path) {
        let shown = if folder.is_empty() { String::new() } else { format!(" in {folder}/") };
        reg_mistake(&format!("asset `{name}`{shown}: {why}"));
    }
    if folder.is_empty() { format!("{ns}:{path}") } else { format!("{ns}:{folder}/{path}") }
}

fn asset_check(folder: &str, ns: &str, path: &str) -> Result<(), &'static str> {
    let bad = if path.is_empty() || ns.is_empty() {
        Some("an empty name")
    } else if path.starts_with('/') || path.contains('\\') || path.contains(':') {
        Some("not a name inside the folder")
    } else if path
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == "..")
    {
        Some("a path leaving the folder")
    } else if TEXTURE_FOLDERS.contains(&folder)
        && path
            .split('/')
            .next()
            .is_some_and(|f| TEXTURE_FOLDERS.contains(&f))
    {
        Some("names start inside the folder, without it")
    } else {
        None
    };
    bad.map_or(Ok(()), Err)
}

fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let loc = info
                .location()
                .map(|l| format!(" at {}:{}", l.file(), l.line()))
                .unwrap_or_default();
            let msg = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            __log(abi::log_level::ERROR, "", format_args!("panicked{loc}: {msg}"));
        }));
    });
}

/// Runs `f`, turning a panic into an error code so it never unwinds into the engine.
pub fn guard(f: impl FnOnce()) -> u32 {
    match std::panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// Wraps a closure into an ABI callback. The closure is leaked: callbacks live as long as the mod.
pub fn callback<A, F>(f: F) -> Callback
where
    F: Fn(&mut A) + Send + Sync + 'static,
{
    unsafe extern "C" fn tramp<A, F: Fn(&mut A)>(data: *const c_void, args: *mut c_void) -> u32 {
        let f = unsafe { &*(data as *const F) };
        let args = unsafe { &mut *(args as *mut A) };
        guard(|| f(args))
    }
    Callback { func: Some(tramp::<A, F>), data: Box::into_raw(Box::new(f)) as *const c_void }
}

/// A thing's full name: one without a namespace is this mod's.
pub(crate) fn full_name(name: &str) -> String {
    qualify(mod_id(), name)
}

/// Prefixes the mod namespace unless the name already has one. Tag references (`#logs`) are
/// vocabulary and stay as written.
pub(crate) fn qualify(ns: &str, name: &str) -> String {
    let base = name.split('[').next().unwrap_or(name);
    if name.starts_with('#') || base.contains(':') || name.is_empty() { name.to_string() } else { format!("{ns}:{name}") }
}

/// A block matcher with each name qualified (`qualify`), `!` kept: what every call taking
/// "which blocks" sends.
pub(crate) fn qualify_matcher(ns: &str, text: &str) -> String {
    abi::matcher_terms(text)
        .map(|t| match t.strip_prefix('!') {
            Some(t) => format!("!{}", qualify(ns, t)),
            None => qualify(ns, t),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn limbs_at_minus_x_are_mirrored() {
        let model = crate::humanoid(false);
        let mirror = |name: &str| {
            model
                .parts
                .iter()
                .find(|p| p.name == name)
                .map(|p| p.boxes[0].mirror)
        };
        // Else a sleeve's long side, on the outside of the arm, is on the inside of one of them.
        assert_eq!((mirror("rightArm"), mirror("rightLeg")), (Some(1), Some(1)));
        assert_eq!((mirror("leftArm"), mirror("leftLeg")), (Some(0), Some(0)));
    }

    #[test]
    fn assets_stay_in_their_folder() {
        use super::asset_check as ok;
        assert!(ok("block", "base", "stone").is_ok());
        assert!(ok("entity", "base", "armor/iron_1").is_ok());
        assert!(ok("", "base", "dig/stone1").is_ok());
        assert!(ok("item", "base", "block/torch").is_err());
        assert!(ok("block", "base", "block/torch").is_err());
        assert!(ok("block", "base", "../item/x").is_err());
        assert!(ok("block", "base", "/x").is_err());
        assert!(ok("block", "base", "").is_err());
    }
}
