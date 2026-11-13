//! The game as an Android app. NativeActivity loads this library and calls `android_main`; the
//! package holds the engine's assets and the game's mods as `assets/game.zip`, unpacked once per
//! version, and the mods' libraries beside this one.

#![cfg(target_os = "android")]

use std::io::Read;
use std::path::Path;

use weft_engine::{AndroidApp, Options};
use weft_render::{Backend, BackendDesc, Error};

#[global_allocator]
static ALLOC: weft_engine::diag::CountingAlloc = weft_engine::diag::CountingAlloc;

fn vulkan(desc: &BackendDesc) -> Result<Box<dyn Backend>, Error> {
    Ok(Box::new(weft_vulkan::VulkanBackend::new(desc)?))
}

const RENDERERS: weft_engine::Renderers = &[("vulkan", vulkan)];

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    // Saves, settings, logs and the player's mods where a file manager or a computer over USB
    // finds them; what the game unpacks stays private, where libraries may load from.
    let private = app.internal_data_path().unwrap_or_default();
    let data = app.external_data_path().unwrap_or_else(|| private.clone());
    let game = private.join("game");
    weft_engine::paths::set_shipped(game.clone());
    weft_engine::paths::set_config(data.join("config"));
    let options = Launch::read(&data.join("options.txt"));
    weft_engine::log::init(Some(&data.join("logs").join("latest.log")), false, &options.log);
    weft_engine::diag::install_crash_handler(&data);
    log::info!("game folder {}, unpacked into {}", data.display(), game.display());
    // There from the start, for whoever opens the folder to see where mods and packs go.
    for dir in ["mods", "resourcepacks", "shaderpacks"] {
        let _ = std::fs::create_dir_all(data.join(dir));
    }
    if let Err(e) = unpack(&app, &game) {
        log::error!("unpacking the game: {e}");
        return;
    }
    let mut opts = Options {
        run_dir: data.clone(),
        cache_dir: private.join("cache"),
        // A player's copy of a mod comes first, as on a computer.
        mod_dirs: vec![data.join("mods"), game.join("mods")],
        mods_off: Vec::new(),
        // A debug package carries the validation layer (`build.py --validation`).
        gpu_debug: weft_engine::paths::libraries().is_some_and(|d| d.join("libVkLayer_khronos_validation.so").is_file()),
        world: options.world,
        seed: options.seed,
        screen: None,
        size: (0, 0),
        dev: false,
        automated: false,
        no_vsync: false,
        view: options.view,
        renderer: None,
    };
    opts.automated = options
        .extra
        .iter()
        .any(|(k, _)| k == "script" || k == "test");
    weft_engine::set_launch_options(options.extra);
    weft_engine::init_workers();
    if let Err(e) = weft_engine::run_android(app, RENDERERS, opts) {
        log::error!("{e}");
    }
}

/// `assets/game.zip` into `dir`, unless this very archive was unpacked there already.
fn unpack(app: &AndroidApp, dir: &Path) -> Result<(), String> {
    let mut asset = app
        .asset_manager()
        .open(c"game.zip")
        .ok_or("the package has no game.zip")?;
    let mut bytes = Vec::new();
    asset.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let stamp = format!("{:016x}", weft_noise::fnv(&bytes));
    let marker = dir.join(".unpacked");
    if std::fs::read_to_string(&marker).is_ok_and(|s| s == stamp) {
        return Ok(());
    }
    let _ = std::fs::remove_dir_all(dir);
    zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .and_then(|mut z| z.extract(dir))
        .map_err(|e| e.to_string())?;
    std::fs::write(&marker, stamp).map_err(|e| e.to_string())
}

/// What a computer gets on the command line, from `options.txt` in the game folder, as the same
/// words (`--world test --view 6 --log frame=debug`): a phone has no command line.
#[derive(Default)]
struct Launch {
    log: String,
    world: Option<String>,
    seed: Option<u64>,
    view: Option<i32>,
    /// What the engine doesn't know goes to mods (`--script x.txt`).
    extra: Vec<(String, String)>,
}

impl Launch {
    fn read(file: &Path) -> Launch {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let mut words = text.split_whitespace().peekable();
        let mut l = Launch::default();
        while let Some(w) = words.next() {
            let Some(name) = w.strip_prefix("--") else { continue };
            let value = words
                .next_if(|v| !v.starts_with("--"))
                .unwrap_or("1")
                .to_string();
            match name {
                "log" => l.log = value,
                "world" => l.world = Some(value),
                "seed" => l.seed = value.parse().ok(),
                "view" => l.view = value.parse().ok(),
                _ => l.extra.push((name.to_string(), value)),
            }
        }
        l
    }
}
