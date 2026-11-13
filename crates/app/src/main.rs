use std::path::PathBuf;

use weft_engine::Options;
use weft_engine::paths::{self, Dirs};
use weft_render::{Backend, BackendDesc, Error};

mod optimize;
mod submit;

#[global_allocator]
static ALLOC: weft_engine::diag::CountingAlloc = weft_engine::diag::CountingAlloc;

fn vulkan(desc: &BackendDesc) -> Result<Box<dyn Backend>, Error> {
    Ok(Box::new(weft_vulkan::VulkanBackend::new(desc)?))
}

fn wgpu(desc: &BackendDesc) -> Result<Box<dyn Backend>, Error> {
    Ok(Box::new(weft_wgpu::WgpuBackend::new(desc)?))
}

const RENDERERS: weft_engine::Renderers = &[("vulkan", vulkan), ("wgpu", wgpu)];

/// Where `run` (and, in the source tree, `mods`) live: the working directory if it has either,
/// otherwise the nearest folder above the executable that does (a development build runs from
/// `target/<profile>/`).
fn game_root() -> PathBuf {
    let has = |d: &std::path::Path| d.join("run").is_dir() || d.join("mods").is_dir();
    let cwd = std::env::current_dir().unwrap_or_default();
    if has(&cwd) {
        return cwd;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    exe.ancestors()
        .skip(1)
        .find(|d| has(d))
        .map(PathBuf::from)
        .unwrap_or(cwd)
}

const HELP: &str = "usage: weft [command] [options]

commands:
  client               play (what runs with no command)
  server               a world with no window or GPU (--world, --port)
  pack-mod <dir>...    zip each mod folder into the run folder's mods
  dist <dir>           a folder to hand to someone: the game and every mod packed
  submit-mod <dir>     send a mod's pushed commit to the mod index (--owner <login>)
  test-all             every test, several processes at once (--jobs N)
  export-models <dir>  entity models made in code, any mod's, as Blockbench .geo.json files
  optimize-assets      the source tree's PNGs smaller as the game reads them the same, its WAVs
                       to Ogg Vorbis (needs ffmpeg)

options:
  --run-dir <dir>      saves, settings, logs and mods go here
  --mods <dir>         load mods from this folder too
  --off <a,b>          start with these mods off
  --world <name>       skip the menu and play this world (created if missing)
  --seed <n>           seed of a world made by --world
  --view <chunks>      view distance
  --size <WxH>         window size
  --screen <name>      open this screen at start
  --renderer <name>    vulkan or wgpu
  --no-vsync           frames as fast as they come
  --gpu-debug          the graphics API's validation
  --dev                engine assets from the source tree
  --verbose            more in the log
  --log <spec>         log levels, `target=level,level`
  --mod-abi            print what mods have to be built against
  --version            print the version

Other `--name [value]` go to mods: `--test <name>`, `--script <file>`, `--port <n>`.
";

fn main() {
    let root = game_root();
    let mut opts = Options {
        run_dir: PathBuf::new(),
        cache_dir: PathBuf::new(),
        // Mod sources of the game itself; players' mods go into `<run dir>/mods`, added below.
        mod_dirs: vec![root.join("mods")],
        mods_off: Vec::new(),
        gpu_debug: false,
        world: None,
        seed: None,
        screen: None,
        size: (1600, 900),
        dev: cfg!(debug_assertions),
        automated: false,
        no_vsync: false,
        view: None,
        renderer: None,
    };
    let mut chosen_run: Option<PathBuf> = None;
    let mut verbose = false;
    let mut log_spec = String::new();
    let mut owner: Option<String> = None;
    // Options the engine does not know go to mods (`--test furnace`, `--script x.txt`).
    let mut extra: Vec<(String, String)> = Vec::new();
    let mut args = std::env::args().skip(1).peekable();
    let command = args
        .next_if(|a| !a.starts_with('-'))
        .unwrap_or_else(|| "client".into());
    let mut places: Vec<PathBuf> = Vec::new();
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_default();
        match a.as_str() {
            "help" | "h" | "--help" => {
                print!("{HELP}");
                return;
            }
            "--version" => {
                println!("weft {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--run-dir" => chosen_run = Some(PathBuf::from(value())),
            "--mods" => opts.mod_dirs.push(PathBuf::from(value())),
            "--off" => opts.mods_off.extend(
                value()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
            ),
            // What a mod has to be built against to load here, for a launcher.
            "--mod-abi" => {
                println!("{:016x}", weft_engine::modloader::ABI_VERSION);
                return;
            }
            "--gpu-debug" => opts.gpu_debug = true,
            "--verbose" => verbose = true,
            "--log" => log_spec = value(),
            "--world" => opts.world = Some(value()),
            "--seed" => opts.seed = value().parse().ok(),
            "--screen" => opts.screen = Some(value()),
            "--no-vsync" => opts.no_vsync = true,
            // Engine shaders and assets live from the source tree in a release build too.
            "--dev" => opts.dev = true,
            "--view" => opts.view = value().parse().ok(),
            "--renderer" => opts.renderer = Some(value()),
            "--owner" => owner = Some(value()),
            "--size" => {
                let v: Vec<u32> = value().split('x').filter_map(|s| s.parse().ok()).collect();
                if v.len() == 2 {
                    opts.size = (v[0], v[1]);
                }
            }
            other => match other.strip_prefix("--") {
                Some(name) => {
                    let v = if args.peek().is_some_and(|n| !n.starts_with("--")) { args.next().unwrap_or_default() } else { "1".into() };
                    extra.push((name.to_string(), v));
                }
                None => places.push(PathBuf::from(other)),
            },
        }
    }
    let (mut server, mut pack, mut dist, mut submit, mut export) = (false, Vec::new(), None, None, None);
    match (command.as_str(), places.len()) {
        ("client", 0) => {}
        ("server", 0) => server = true,
        ("test-all", 0) => extra.push(("test".into(), "all".into())),
        ("pack-mod", 1..) => pack = places,
        ("dist", 1) => dist = places.pop(),
        // Sends a mod's pushed commit to the mod index, see `submit.rs`.
        ("submit-mod", 1) => submit = places.pop(),
        ("export-models", 1) => export = places.pop(),
        ("optimize-assets", 0) => match optimize::run(&root) {
            Ok(done) => {
                println!("{done}");
                return;
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        _ => {
            eprint!("{HELP}");
            std::process::exit(2);
        }
    }
    if let Some(dir) = submit {
        match submit::run(&dir, owner) {
            Ok(done) => println!("{done}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let dirs = chosen_run.map_or_else(|| paths::of_install(&root), |run| Dirs::under(&run));
    (opts.run_dir, opts.cache_dir) = (dirs.data, dirs.cache);
    paths::set_config(dirs.config);
    opts.automated = extra.iter().any(|(k, _)| k == "script" || k == "test");
    if let Some(code) = test_jobs(&extra, &opts.run_dir) {
        std::process::exit(code);
    }
    weft_engine::set_launch_options(extra);
    let player_mods = opts.run_dir.join("mods");
    if !pack.is_empty() {
        for dir in pack {
            match weft_engine::modloader::pack(&dir, &player_mods) {
                Ok(path) => println!("packed {}", path.display()),
                Err(e) => eprintln!("cannot pack {}: {e}", dir.display()),
            }
        }
        return;
    }
    if let Some(out) = dist {
        match make_dist(&root, &out) {
            Ok(()) => println!("game ready in {}", out.display()),
            Err(e) => eprintln!("cannot make {}: {e}", out.display()),
        }
        return;
    }
    weft_engine::log::init(Some(&opts.run_dir.join("logs").join("latest.log")), verbose, &log_spec);
    weft_engine::diag::install_crash_handler(&opts.run_dir);
    log::info!("game folder {}, cache {}", opts.run_dir.display(), opts.cache_dir.display());
    // Release builds load the game's own mods the way players get them: each packed into one
    // file in `run/mods` (again only when it changed).
    if !cfg!(debug_assertions) && root.join("mods").is_dir() {
        for dir in subdirs(&root.join("mods")) {
            match weft_engine::modloader::pack_if_stale(&dir, &player_mods) {
                Ok((path, true)) => log::info!("packed {}", path.display()),
                Ok(_) => {}
                Err(e) => log::warn!("cannot pack {}: {e}", dir.display()),
            }
        }
        let sources = root.join("mods");
        opts.mod_dirs.retain(|d| *d != sources);
    }
    // What came with a shipped game stays with it, and a player's copy of a mod comes first.
    let shipped = root.join("run").join("mods");
    let separate = shipped != player_mods;
    opts.mod_dirs.push(player_mods);
    if separate {
        opts.mod_dirs.push(shipped);
    }
    weft_engine::init_workers();
    if let Some(out) = export {
        match weft_engine::export_models(&opts, &out) {
            Ok(n) => println!("{n} models written to {}", out.display()),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if server {
        if let Err(e) = weft_engine::dedicated::run(opts) {
            log::error!("{e}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(e) = weft_engine::run(RENDERERS, opts) {
        log::error!("{e}");
        std::process::exit(1);
    }
    std::process::exit(weft_engine::exit_code());
}

/// `--test all` runs in several processes at once, each with its own run folder (`--jobs N`, 1 for
/// one after the other, also what naming tests does unless asked). Each takes every Nth test
/// (`testkit`, `--test-shard`); what they leave is put together as a single run would have it.
fn test_jobs(extra: &[(String, String)], run_dir: &std::path::Path) -> Option<i32> {
    let option = |name: &str| {
        extra
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    if option("test-shard").is_some() {
        return None;
    }
    let test = option("test")?;
    let auto = (std::thread::available_parallelism().map_or(1, |n| n.get()) / 4).clamp(1, 6);
    let jobs = option("jobs")
        .and_then(|j| j.parse().ok())
        .unwrap_or(if test == "all" { auto } else { 1 });
    if jobs < 2 || test == "list" {
        return None;
    }
    let started = std::time::Instant::now();
    let exe = std::env::current_exe().ok()?;
    let mut dirs: Vec<PathBuf> = (0..jobs)
        .map(|i| run_dir.join("test-jobs").join(i.to_string()))
        .collect();
    let spawn = |shard: &str, dir: &PathBuf| {
        let _ = std::fs::create_dir_all(dir);
        // Or a process that dies at once passes by the last run's.
        let _ = std::fs::remove_file(dir.join("test-reports").join("report.txt"));
        let log = std::fs::File::create(dir.join("output.log")).ok()?;
        let mut c = std::process::Command::new(&exe);
        c.args(["--test", test, "--test-shard", shard, "--run-dir"])
            .arg(dir)
            .stdout(log)
            .stderr(std::process::Stdio::null());
        for (k, v) in extra.iter().filter(|(k, _)| k != "test" && k != "jobs") {
            c.arg(format!("--{k}"));
            if !v.is_empty() {
                c.arg(v);
            }
        }
        c.spawn().ok()
    };
    let children: Vec<_> = dirs
        .iter()
        .enumerate()
        .map(|(i, dir)| spawn(&format!("{i}/{jobs}"), dir))
        .collect();
    let code = |c: Option<std::process::Child>| {
        c.and_then(|mut c| c.wait().ok())
            .and_then(|s| s.code())
            .unwrap_or(1)
    };
    let mut codes: Vec<i32> = children.into_iter().map(code).collect();
    // What measures how quick the game is can't share the machine: last, on its own.
    dirs.push(run_dir.join("test-jobs").join("alone"));
    codes.push(code(spawn("alone", &dirs[jobs])));
    // A crashed process has no report, and its tests didn't pass.
    let shots = run_dir.join("test-shots");
    let _ = std::fs::remove_dir_all(&shots);
    let _ = std::fs::create_dir_all(&shots);
    let (mut passed, mut failed, mut details, mut crashed) = (0, 0, String::new(), 0);
    for (i, dir) in dirs.iter().enumerate() {
        for e in std::fs::read_dir(dir.join("test-shots"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let _ = std::fs::rename(e.path(), shots.join(e.file_name()));
        }
        let report = std::fs::read_to_string(dir.join("test-reports/report.txt")).unwrap_or_default();
        let counts: Vec<i32> = report
            .lines()
            .next()
            .unwrap_or("")
            .split_whitespace()
            .filter_map(|w| w.parse().ok())
            .collect();
        if let ([p, f], true) = (counts.as_slice(), codes[i] >= 0 && !report.is_empty()) {
            (passed, failed) = (passed + p, failed + f);
            details.push_str(report.split_once('\n').map_or("", |r| r.1));
        } else {
            crashed += 1;
            details.push_str(&format!("\nFAIL process {i} ended without a report, see {}\n", dir.join("output.log").display()));
        }
    }
    let head = format!("testkit report: {passed} passed, {failed} failed, {:.1} s, {jobs} at once\n", started.elapsed().as_secs_f32());
    let reports = run_dir.join("test-reports");
    let _ = std::fs::create_dir_all(&reports);
    let _ = std::fs::write(reports.join("report.txt"), format!("{head}{details}"));
    print!("{head}");
    for l in details
        .lines()
        .scan(false, |in_fail, l| {
            *in_fail = if l.starts_with("FAIL") {
                true
            } else if l.starts_with("PASS") {
                false
            } else {
                *in_fail
            };
            Some(in_fail.then_some(l))
        })
        .flatten()
    {
        println!("{l}");
    }
    Some(failed + crashed)
}

fn subdirs(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// A folder to hand to someone: the executable, the engine's assets and every mod packed.
fn make_dist(root: &std::path::Path, out: &std::path::Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    std::fs::copy(&exe, out.join(exe.file_name().unwrap())).map_err(|e| e.to_string())?;
    // What mods have to be built against, for a launcher to read without starting the game.
    std::fs::write(out.join("mod-abi.txt"), format!("{:016x}", weft_engine::modloader::ABI_VERSION)).map_err(|e| e.to_string())?;
    copy_dir(&root.join("crates").join("engine").join("assets"), &out.join("assets").join("core"))?;
    for dir in subdirs(&root.join("mods")) {
        // Development tools (the test kit) mark themselves `dist = false` in Cargo.toml.
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
        if manifest.lines().any(|l| l.replace(' ', "") == "dist=false") {
            continue;
        }
        weft_engine::modloader::pack(&dir, &out.join("run").join("mods"))?;
    }
    Ok(())
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(from)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .flatten()
    {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()))?;
        } else {
            std::fs::copy(&p, to.join(e.file_name())).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
