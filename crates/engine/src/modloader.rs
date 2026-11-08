//! Finds mod libraries, loads them and runs registration in parallel.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use rayon::prelude::*;
use weft_abi::{self as abi, ModId};

use crate::content::{self, ModMeta, RawRegistrar};

pub struct LoadedMod {
    pub id: String,
    pub assets: PathBuf,
    _lib: libloading::Library,
    decl: &'static abi::ModDecl,
}

static NAMES: OnceLock<Vec<String>> = OnceLock::new();
/// Each loaded mod's assets folder by its id, for `asset_exists` while mods register.
static ASSET_ROOTS: std::sync::RwLock<Vec<(String, std::path::PathBuf)>> = std::sync::RwLock::new(Vec::new());

/// Whether mod asset `name` (`beauty_of_nature:block/log_oak_inner`) is a file `<kind>/...<.ext>`
/// of the mod it names. Mods' own files only: resource packs are each client's, and what a mod
/// registers has to be the same everywhere.
pub fn asset_exists(name: &str, kind: &str, ext: &str) -> bool {
    let Some((ns, rel)) = name.split_once(':') else { return false };
    if rel.is_empty() || rel.contains("..") || rel.contains('\\') || rel.starts_with('/') {
        return false;
    }
    ASSET_ROOTS
        .read()
        .unwrap()
        .iter()
        .any(|(id, dir)| id == ns && dir.join(kind).join(format!("{rel}.{ext}")).is_file())
}

pub fn mod_loaded(id: &str) -> bool {
    ASSET_ROOTS.read().unwrap().iter().any(|(m, _)| m == id)
}

pub fn mod_id_name(m: ModId) -> &'static str {
    NAMES
        .get()
        .and_then(|n| n.get(m.0 as usize))
        .map_or("?", |s| s.as_str())
}

/// Mods found in `mod_dirs`, earlier dirs winning: a folder (`assets/` and the library) or the
/// same packed into `<name>.zip`, unpacked into `cache` once per version of the file. A folder's
/// library may also sit next to the engine executable instead (development builds,
/// `target/<profile>/`).
pub fn discover(mod_dirs: &[PathBuf], cache: &Path) -> Vec<(String, PathBuf, PathBuf)> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let mut found = Vec::new();
    for dir in mod_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let zipped = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
            if !path.is_dir() && !zipped {
                continue;
            }
            let name = if zipped { path.file_stem() } else { path.file_name() }
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            // For a name found twice, the first dir's copy stays. Before unpacking: another
            // version unpacked takes away the one before it.
            if found
                .iter()
                .any(|f: &(String, PathBuf, PathBuf)| f.0 == name)
            {
                continue;
            }
            let root = if zipped {
                match unpack(&path, &cache.join("unpacked"), &name) {
                    Ok(root) => root,
                    Err(e) => {
                        log::error!("mod {}: {e}", path.display());
                        continue;
                    }
                }
            } else {
                path
            };
            let file = libloading::library_filename(&name);
            let lib = [Some(root.join(&file)), exe_dir.as_ref().map(|d| d.join(&file))]
                .into_iter()
                .flatten()
                .find(|p| p.exists());
            match lib {
                Some(lib) => found.push((name, root, lib)),
                // The library is named after the mod (its crate), and so must be the folder or zip.
                None => log::error!(
                    "mod {}: no {} in it; a mod's folder or zip is named like its crate",
                    root.display(),
                    file.to_string_lossy()
                ),
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Unpacks a mod archive into `<cache>/<name>-<size>-<mtime>`, unless that is already there;
/// older unpacked versions of the mod are removed.
fn unpack(archive: &Path, cache: &Path, name: &str) -> Result<PathBuf, String> {
    let meta = std::fs::metadata(archive).map_err(|e| e.to_string())?;
    let stamp = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let dest = cache.join(format!("{name}-{}-{stamp}", meta.len()));
    if dest.join(".done").exists() {
        return Ok(dest);
    }
    if let Ok(old) = std::fs::read_dir(cache) {
        for e in old.flatten() {
            if e.file_name().to_string_lossy().rsplitn(3, '-').nth(2) == Some(name) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    zip.extract(&dest).map_err(|e| e.to_string())?;
    std::fs::write(dest.join(".done"), b"").map_err(|e| e.to_string())?;
    log::info!("unpacked mod {name}");
    Ok(dest)
}

/// The libraries of mod `name` to pack, with their file names: those in its folder, one per
/// system, so that one zip serves them all (`discover` picks the system's own), and this system's
/// from next to the engine executable if the folder has none.
fn libraries(dir: &Path, name: &str) -> Vec<(String, PathBuf)> {
    let mut libs: Vec<(String, PathBuf)> = [format!("{name}.dll"), format!("lib{name}.so"), format!("lib{name}.dylib")]
        .into_iter()
        .map(|f| (dir.join(&f), f))
        .filter(|(p, _)| p.exists())
        .map(|(p, f)| (f, p))
        .collect();
    let own = libloading::library_filename(name)
        .to_string_lossy()
        .into_owned();
    if !libs.iter().any(|l| l.0 == own) {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(&own)));
        libs.extend(beside.filter(|p| p.exists()).map(|p| (own, p)));
    }
    libs
}

/// Packs a mod folder (its `assets/` and `libraries`) into `<out>/<name>.zip`, the single file
/// players drop into `mods/`.
pub fn pack(dir: &Path, out: &Path) -> Result<PathBuf, String> {
    use std::io::Write;
    let name = dir
        .file_name()
        .ok_or("not a mod folder")?
        .to_string_lossy()
        .into_owned();
    let libs = libraries(dir, &name);
    if libs.is_empty() {
        return Err(format!("{} is not built", libloading::library_filename(&name).to_string_lossy()));
    }
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let path = out.join(format!("{name}.zip"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).map_err(|e| e.to_string())?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut add = |rel: String, src: &Path| -> Result<(), String> {
        zip.start_file(rel, opts).map_err(|e| e.to_string())?;
        zip.write_all(&std::fs::read(src).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    };
    for (file, lib) in &libs {
        add(file.clone(), lib)?;
    }
    let mut files = Vec::new();
    collect(&dir.join("assets"), &mut files);
    for f in files {
        let rel = f
            .strip_prefix(dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        add(rel, &f)?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(path)
}

/// `pack`, unless `<out>/<name>.zip` is already newer than the library and every asset. Returns
/// the archive and whether it was written.
pub fn pack_if_stale(dir: &Path, out: &Path) -> Result<(PathBuf, bool), String> {
    let name = dir
        .file_name()
        .ok_or("not a mod folder")?
        .to_string_lossy()
        .into_owned();
    let zip = out.join(format!("{name}.zip"));
    let mut inputs = Vec::new();
    collect(&dir.join("assets"), &mut inputs);
    inputs.extend(libraries(dir, &name).into_iter().map(|l| l.1));
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let newest = inputs.iter().filter_map(|p| modified(p)).max();
    if let (Some(z), Some(n)) = (modified(&zip), newest) {
        if z >= n {
            return Ok((zip, false));
        }
    }
    pack(dir, out).map(|p| (p, true))
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Loads every library, then calls all `init`s in parallel and freezes the registries.
/// Where the mods turned off are listed, ids one a line. Automated runs keep theirs apart.
pub fn off_file(run_dir: &Path, automated: bool) -> PathBuf {
    let dir = if automated { run_dir.join("test-config") } else { crate::paths::config(run_dir) };
    dir.join("disabled_mods.txt")
}

/// What turning mods on and off has come to, and what this run started with.
struct Switches {
    file: PathBuf,
    wanted: Vec<String>,
    at_start: Vec<String>,
    /// Off for this run only: shown off, not written to the file.
    run_only: Vec<String>,
}

static OFF: OnceLock<Vec<ModMeta>> = OnceLock::new();
static SWITCHES: parking_lot::Mutex<Option<Switches>> = parking_lot::Mutex::new(None);

/// Mods found but not running, for the mods screen: turned off, or needing one that is.
pub fn turned_off() -> &'static [ModMeta] {
    OFF.get().map_or(&[], |v| v.as_slice())
}

/// Whether a mod would run on the next start, as things are set now.
pub fn wanted_on(id: &str) -> bool {
    let wanted = SWITCHES
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.wanted.clone());
    let metas: Vec<&ModMeta> = content::try_content()
        .map_or(Vec::new(), |c| c.mods.iter().collect())
        .into_iter()
        .chain(turned_off())
        .collect();
    runs(id, &wanted, &|m| {
        metas
            .iter()
            .find(|o| o.id == m)
            .map_or(Vec::new(), |o| o.deps.clone())
    })
}

/// Not itself turned off, and neither is anything it needs, however far down.
fn runs(id: &str, off: &[String], deps: &dyn Fn(&str) -> Vec<String>) -> bool {
    !off.iter().any(|o| o == id) && deps(id).iter().all(|d| d != id && runs(d, off, deps))
}

/// Turns a mod on or off from the next start.
pub fn toggle(id: &str) {
    let mut guard = SWITCHES.lock();
    let Some(s) = guard.as_mut() else { return };
    match s.wanted.iter().position(|w| w == id) {
        Some(i) => drop(s.wanted.remove(i)),
        None => s.wanted.push(id.to_string()),
    }
    s.wanted.sort();
    if let Some(d) = s.file.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let saved: Vec<&str> = s
        .wanted
        .iter()
        .filter(|w| !s.run_only.contains(w))
        .map(String::as_str)
        .collect();
    if let Err(e) = std::fs::write(&s.file, saved.join("\n")) {
        log::error!("{}: {e}", s.file.display());
    }
}

/// Something was switched that a restart would bring in.
pub fn restart_pending() -> bool {
    SWITCHES
        .lock()
        .as_ref()
        .is_some_and(|s| s.wanted != s.at_start)
}

/// What a mod has to be built against to load (`weft --mod-abi`).
pub const ABI_VERSION: u64 = abi::ABI_VERSION;

/// `also_off`: off for this run only, besides what the player turned off (a launcher's choice).
pub fn load(mod_dirs: &[PathBuf], cache: &Path, off_file: &Path, also_off: &[String]) -> Result<Vec<LoadedMod>, String> {
    let start = Instant::now();
    let mut wanted: Vec<String> = std::fs::read_to_string(off_file)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    wanted.extend(also_off.iter().cloned());
    wanted.sort();
    wanted.dedup();
    *SWITCHES.lock() =
        Some(Switches { file: off_file.to_path_buf(), at_start: wanted.clone(), wanted: wanted.clone(), run_only: also_off.to_vec() });
    let mut off: Vec<ModMeta> = Vec::new();
    let shadow = cache.join("mods");
    std::fs::create_dir_all(&shadow).ok();
    if let Ok(entries) = std::fs::read_dir(&shadow) {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }

    let mut mods = Vec::new();
    for (name, dir, lib_path) in discover(mod_dirs, cache) {
        // Windows locks loaded libraries; loading a copy keeps the original rebuildable.
        let copy = shadow.join(format!("{}-{}", std::process::id(), lib_path.file_name().unwrap().to_string_lossy()));
        let load_from = if std::fs::copy(&lib_path, &copy).is_ok() { copy } else { lib_path.clone() };
        let lib = match unsafe { libloading::Library::new(&load_from) } {
            Ok(lib) => lib,
            // A mod that won't load is a good reason to have turned it off.
            Err(_) if wanted.contains(&name) => {
                off.push(ModMeta { id: name.clone(), version: String::new(), deps: Vec::new(), order: 0, assets: dir.join("assets") });
                continue;
            }
            Err(e) => return Err(format!("loading mod {name} from {}: {e}", lib_path.display())),
        };
        let entry: libloading::Symbol<unsafe extern "C" fn() -> *const abi::ModDecl> =
            unsafe { lib.get(abi::ENTRY_SYMBOL) }.map_err(|_| format!("{} is not a Weft mod", lib_path.display()))?;
        let decl = unsafe { &*entry() };
        if decl.abi_version != abi::ABI_VERSION {
            return Err(format!(
                "mod {name} was built against another mod ABI ({:016x}, this engine has {:016x}); rebuild it against the current weft_sdk",
                decl.abi_version,
                abi::ABI_VERSION
            ));
        }
        let id = unsafe { decl.id.as_str() }.to_string();
        if id != name {
            return Err(format!("mod {name} is the crate {id}: name its folder or zip {id}"));
        }
        mods.push(LoadedMod { id, assets: dir.join("assets"), _lib: lib, decl });
    }
    mods.sort_by(|a, b| a.id.cmp(&b.id));
    // Out go the ones turned off, then what needs them, and what needs that.
    let deps_of = |m: &LoadedMod| -> Vec<String> {
        unsafe {
            m.decl
                .deps
                .as_slice()
                .iter()
                .map(|d| d.as_str().to_string())
                .collect()
        }
    };
    // A mod needing one that isn't there is off too, rather than the game not starting.
    let found: Vec<String> = mods.iter().map(|m| m.id.clone()).collect();
    let missing = |d: &String| !found.contains(d);
    loop {
        let Some(i) = mods.iter().position(|m| {
            wanted.contains(&m.id)
                || deps_of(m)
                    .iter()
                    .any(|d| missing(d) || off.iter().any(|o| &o.id == d))
        }) else {
            break;
        };
        let m = mods.remove(i);
        if wanted.contains(&m.id) {
            log::info!("mod {} is turned off", m.id);
        } else if let Some(d) = deps_of(&m).iter().find(|d| missing(d)) {
            log::warn!("mod {} is off, as it needs {d}, which isn't there", m.id);
        } else {
            log::warn!("mod {} is off, as it needs one that is", m.id);
        }
        off.push(ModMeta {
            id: m.id.clone(),
            version: unsafe { m.decl.version.as_str() }.to_string(),
            deps: deps_of(&m),
            order: 0,
            assets: m.assets.clone(),
        });
    }
    off.sort_by(|a, b| a.id.cmp(&b.id));
    let _ = OFF.set(off);
    let _ = NAMES.set(mods.iter().map(|m| m.id.clone()).collect());

    let metas: Vec<ModMeta> = mods
        .iter()
        .map(|m| unsafe {
            ModMeta {
                id: m.id.clone(),
                version: m.decl.version.as_str().to_string(),
                deps: m
                    .decl
                    .deps
                    .as_slice()
                    .iter()
                    .map(|d| d.as_str().to_string())
                    .collect(),
                order: 0,
                assets: m.assets.clone(),
            }
        })
        .collect();
    for m in &metas {
        for d in &m.deps {
            if !metas.iter().any(|o| &o.id == d) {
                return Err(format!("mod {} requires missing mod {d}", m.id));
            }
        }
    }

    *ASSET_ROOTS.write().unwrap() = metas
        .iter()
        .map(|m| (m.id.clone(), m.assets.clone()))
        .collect();
    let regs: Vec<RawRegistrar> = mods
        .par_iter()
        .enumerate()
        .map(|(i, m)| {
            let mut reg = RawRegistrar::new(ModId(i as u32));
            let t = Instant::now();
            let r = unsafe { (m.decl.init)(&crate::host::HOST, ModId(i as u32), &mut reg as *mut _ as *mut abi::Registrar) };
            if r != 0 {
                reg.errors.push("init panicked".into());
            }
            log::debug!("{} registered in {:.1} ms", m.id, t.elapsed().as_secs_f64() * 1000.0);
            reg
        })
        .collect();

    let content = content::freeze(metas, regs).map_err(|e| e.join("\n"))?;
    log::info!(
        "{} mods, {} blocks ({} states), {} items, {} dimensions in {:.1} ms",
        mods.len(),
        content.blocks.len(),
        content.states.len(),
        content.items.len(),
        content.dims.len(),
        start.elapsed().as_secs_f64() * 1000.0
    );
    // Load order is part of the game's behaviour (later mods win on conflicts), so every mod is logged.
    let mut order: Vec<&ModMeta> = content.mods.iter().collect();
    order.sort_by_key(|m| m.order);
    for m in order {
        log::info!("loaded mod {} {} (load position {})", m.id, m.version, m.order);
    }
    content::set(content);
    Ok(mods)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mod_needs_what_it_depends_on_running() {
        let deps = |m: &str| -> Vec<String> {
            match m {
                "top" => vec!["middle".to_string()],
                "middle" => vec!["base".to_string()],
                _ => Vec::new(),
            }
        };
        let off = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(runs("top", &off(&[]), &deps));
        assert!(!runs("top", &off(&["base"]), &deps), "all the way down");
        assert!(runs("base", &off(&["top"]), &deps), "not the other way");
    }

    /// A player's copy of a mod that also came with the game: theirs is the one, still unpacked.
    #[test]
    fn a_mod_in_two_folders_is_the_first_ones() {
        let tmp = std::env::temp_dir().join(format!("weft-twice-{}", std::process::id()));
        let lib = libloading::library_filename("mymod");
        let mut zips = Vec::new();
        for (dir, text) in [("mine", "mine"), ("shipped", "shipped, a longer one")] {
            let src = tmp.join(dir).join("mymod");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join(&lib), text).unwrap();
            zips.push(
                pack(&src, &tmp.join(format!("{dir}-mods")))
                    .unwrap()
                    .parent()
                    .unwrap()
                    .to_path_buf(),
            );
        }
        let found = discover(&zips, &tmp.join("cache"));
        assert_eq!(found.len(), 1);
        assert_eq!(std::fs::read_to_string(&found[0].2).unwrap(), "mine");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// One zip carries a library per system, and each system finds its own in it.
    #[test]
    fn a_zip_serves_every_system() {
        let tmp = std::env::temp_dir().join(format!("weft-pack-{}", std::process::id()));
        let dir = tmp.join("src").join("mymod");
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets").join("a.txt"), "a").unwrap();
        for lib in ["mymod.dll", "libmymod.so", "libmymod.dylib"] {
            std::fs::write(dir.join(lib), lib).unwrap();
        }
        let mods = tmp.join("mods");
        let zip = pack(&dir, &mods).unwrap();
        let archive = zip::ZipArchive::new(std::fs::File::open(&zip).unwrap()).unwrap();
        let mut names: Vec<&str> = archive.file_names().collect();
        names.sort();
        assert_eq!(names, ["assets/a.txt", "libmymod.dylib", "libmymod.so", "mymod.dll"]);
        let found = discover(&[mods], &tmp.join("cache"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].2.file_name().unwrap(), libloading::library_filename("mymod"));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
