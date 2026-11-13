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
    source: PathBuf,
    _lib: libloading::Library,
    decl: &'static abi::ModDecl,
}

static NAMES: OnceLock<Vec<String>> = OnceLock::new();
/// Each loaded mod's assets folder by its id, for `asset_exists` while mods register.
static ASSET_ROOTS: std::sync::RwLock<Vec<(String, std::path::PathBuf)>> = std::sync::RwLock::new(Vec::new());

/// Whether mod asset `name` (`beauty_of_nature:block/log_oak_inner`) is a file `<kind>/...<.ext>`
/// of the mod it names, or for a picture a folder of connected tiles (`Assets::ctm_rules`).
/// Mods' own files only: resource packs are each client's, and what a mod registers has to be the
/// same everywhere.
pub fn asset_exists(name: &str, kind: &str, ext: &str) -> bool {
    let Some((ns, rel)) = name.split_once(':') else { return false };
    if rel.is_empty() || rel.contains("..") || rel.contains('\\') || rel.starts_with('/') {
        return false;
    }
    ASSET_ROOTS
        .read()
        .unwrap()
        .iter()
        .any(|(id, dir)| id == ns && file_or_tiles(&dir.join(kind), rel, kind, ext))
}

/// `<rel>.<ext>` in `dir`, or for a texture its folder of connected tiles.
pub fn file_or_tiles(dir: &Path, rel: &str, kind: &str, ext: &str) -> bool {
    let last = rel.rsplit('/').next().unwrap_or(rel);
    dir.join(format!("{rel}.{ext}")).is_file() || (kind == "textures" && ext == "png" && dir.join(format!("{rel}/{last}.ctm")).is_file())
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
/// library may also sit beside the engine's instead (development builds, `target/<profile>/`;
/// an Android package's).
pub struct DiscoveredMod {
    name: String,
    root: PathBuf,
    library: PathBuf,
    source: PathBuf,
}

pub fn discover(mod_dirs: &[PathBuf], cache: &Path) -> Vec<DiscoveredMod> {
    let exe_dir = crate::paths::libraries();
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
            if found.iter().any(|f: &DiscoveredMod| f.name == name) {
                continue;
            }
            let source = path.clone();
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
            // Android's is named as Linux's, so it has a folder of its own (`ANDROID`).
            let android = cfg!(target_os = "android").then(|| root.join(ANDROID).join(&file));
            let lib = [android, Some(root.join(&file)), exe_dir.as_ref().map(|d| d.join(&file))]
                .into_iter()
                .flatten()
                .find(|p| p.exists());
            match lib {
                Some(library) => found.push(DiscoveredMod { name, root, library, source }),
                // The library is named after the mod (its crate), and so must be the folder or zip.
                None => log::error!(
                    "mod {}: no {} in it; a mod's folder or zip is named like its crate",
                    root.display(),
                    file.to_string_lossy()
                ),
            }
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
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

/// Where a mod keeps its library for Android phones: the same file name as Linux's.
const ANDROID: &str = "android";

/// The libraries of mod `name` to pack, with their file names: those in its folder, one per
/// system, so that one zip serves them all (`discover` picks the system's own), and this system's
/// from next to the engine executable if the folder has none.
fn libraries(dir: &Path, name: &str) -> Vec<(String, PathBuf)> {
    let mut libs: Vec<(String, PathBuf)> =
        [format!("{name}.dll"), format!("lib{name}.so"), format!("lib{name}.dylib"), format!("{ANDROID}/lib{name}.so")]
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
    for f in &files {
        let rel = f
            .strip_prefix(dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        add(rel, f)?;
    }
    zip.set_comment(fingerprint(dir, &name));
    zip.finish().map_err(|e| e.to_string())?;
    Ok(path)
}

/// What a mod folder packs, as a text the archive keeps (its comment): every file's path, size
/// and time. A file added, taken away or renamed changes it, also one copied in with an older
/// time than the archive's.
fn fingerprint(dir: &Path, name: &str) -> String {
    let mut files = Vec::new();
    collect(&dir.join("assets"), &mut files);
    files.extend(libraries(dir, name).into_iter().map(|l| l.1));
    files.sort();
    // The std hasher may change with the compiler: then everything is packed again once.
    let mut h = std::hash::DefaultHasher::new();
    for f in &files {
        let meta = std::fs::metadata(f).ok();
        std::hash::Hash::hash(&(f, meta.as_ref().map(|m| m.len()), meta.and_then(|m| m.modified().ok())), &mut h);
    }
    format!("weft {:016x}", std::hash::Hasher::finish(&h))
}

/// `pack`, unless `<out>/<name>.zip` was packed from the folder as it is (`fingerprint`).
/// Returns the archive and whether it was written.
pub fn pack_if_stale(dir: &Path, out: &Path) -> Result<(PathBuf, bool), String> {
    let name = dir
        .file_name()
        .ok_or("not a mod folder")?
        .to_string_lossy()
        .into_owned();
    let zip = out.join(format!("{name}.zip"));
    let packed = std::fs::File::open(&zip)
        .ok()
        .and_then(|f| zip::ZipArchive::new(f).ok())
        .map(|a| a.comment().to_vec());
    if packed.is_some_and(|c| c == fingerprint(dir, &name).as_bytes()) {
        return Ok((zip, false));
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

impl Switches {
    fn save(&self) {
        let saved: Vec<&str> = self
            .wanted
            .iter()
            .filter(|w| !self.run_only.contains(w))
            .map(String::as_str)
            .collect();
        let result = self
            .file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&self.file, saved.join("\n")));
        if let Err(e) = result {
            log::error!("{}: {e}", self.file.display());
        }
    }
}

static OFF: OnceLock<Vec<ModMeta>> = OnceLock::new();
static NEWLY_DISABLED: OnceLock<Vec<String>> = OnceLock::new();
static LOAD_ERRORS: OnceLock<rustc_hash::FxHashMap<String, String>> = OnceLock::new();
static SWITCHES: parking_lot::Mutex<Option<Switches>> = parking_lot::Mutex::new(None);

/// Mods found but not running, for the mods screen: turned off, or needing one that is.
pub fn turned_off() -> &'static [ModMeta] {
    OFF.get().map_or(&[], |v| v.as_slice())
}

/// Mods automatically disabled on this start, excluding ones already disabled through dependencies.
pub fn newly_disabled() -> &'static [String] {
    NEWLY_DISABLED.get().map_or(&[], |v| v.as_slice())
}

pub fn load_error(id: &str) -> Option<&'static str> {
    LOAD_ERRORS.get()?.get(id).map(String::as_str)
}

fn dependencies(id: &str) -> Option<Vec<String>> {
    if load_error(id).is_some() {
        return None;
    }
    content::try_content()
        .into_iter()
        .flat_map(|c| &c.mods)
        .chain(turned_off())
        .find(|m| m.id == id)
        .map(|m| m.deps.clone())
        .or_else(|| {
            crate::mod_index::index()?
                .iter()
                .find(|m| m.id == id)?
                .fitting()
                .map(|b| b.deps.clone())
        })
}

/// Whether a mod would run on the next start, as things are set now.
pub fn wanted_on(id: &str) -> bool {
    let wanted = SWITCHES
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.wanted.clone());
    runs(id, &wanted, &dependencies)
}

/// Not itself turned off, and neither is anything it needs, however far down.
fn runs(id: &str, off: &[String], deps: &dyn Fn(&str) -> Option<Vec<String>>) -> bool {
    let mut todo = vec![id.to_string()];
    let mut seen = rustc_hash::FxHashSet::default();
    while let Some(id) = todo.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if off.contains(&id) {
            return false;
        }
        let Some(needs) = deps(&id) else { return false };
        todo.extend(needs);
    }
    true
}

fn enable(id: &str, off: &mut Vec<String>, deps: &dyn Fn(&str) -> Option<Vec<String>>) {
    let mut todo = vec![id.to_string()];
    let mut seen = rustc_hash::FxHashSet::default();
    while let Some(id) = todo.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        off.retain(|m| m != &id);
        todo.extend(deps(&id).unwrap_or_default());
    }
}

/// Turns a mod on or off from the next start.
pub fn toggle(id: &str) {
    let mut guard = SWITCHES.lock();
    let Some(s) = guard.as_mut() else { return };
    if runs(id, &s.wanted, &dependencies) {
        s.wanted.push(id.to_string());
    } else {
        enable(id, &mut s.wanted, &dependencies);
    }
    s.wanted.sort();
    s.save();
}

fn automatically_disabled(off: &[ModMeta], wanted: &[String]) -> Vec<String> {
    // Missing dependencies cause automatic disabling; only explicit switches suppress the notice.
    let deps = |id: &str| {
        Some(
            off.iter()
                .find(|m| m.id == id)
                .map_or(Vec::new(), |m| m.deps.clone()),
        )
    };
    off.iter()
        .filter(|m| runs(&m.id, wanted, &deps))
        .map(|m| m.id.clone())
        .collect()
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
    let run_only = also_off
        .iter()
        .filter(|id| !wanted.contains(id))
        .cloned()
        .collect();
    wanted.extend(also_off.iter().cloned());
    wanted.sort();
    wanted.dedup();
    let mut off: Vec<ModMeta> = Vec::new();
    let mut errors = rustc_hash::FxHashMap::default();
    let shadow = cache.join("mods");
    std::fs::create_dir_all(&shadow).ok();
    if let Ok(entries) = std::fs::read_dir(&shadow) {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }

    let mut mods = Vec::new();
    for DiscoveredMod { name, root: dir, library: lib_path, source } in discover(mod_dirs, cache) {
        let rejected = |reason: String, off: &mut Vec<ModMeta>, errors: &mut rustc_hash::FxHashMap<String, String>| {
            log::warn!("mod {name} is off: {reason}");
            errors.insert(name.clone(), reason);
            off.push(ModMeta {
                id: name.clone(),
                version: String::new(),
                deps: Vec::new(),
                after: Vec::new(),
                before: Vec::new(),
                order: 0,
                assets: dir.join("assets"),
                source: source.clone(),
            });
        };
        // Windows locks loaded libraries; loading a copy keeps the original rebuildable.
        let copy = shadow.join(format!("{}-{}", std::process::id(), lib_path.file_name().unwrap().to_string_lossy()));
        let load_from = if cfg!(windows) && std::fs::copy(&lib_path, &copy).is_ok() { copy } else { lib_path.clone() };
        let lib = match unsafe { libloading::Library::new(&load_from) } {
            Ok(lib) => lib,
            Err(e) => {
                rejected(format!("could not load library: {e}"), &mut off, &mut errors);
                continue;
            }
        };
        let entry: libloading::Symbol<unsafe extern "C" fn() -> *const abi::ModDecl> = match unsafe { lib.get(abi::ENTRY_SYMBOL) } {
            Ok(entry) => entry,
            Err(_) => {
                rejected("not a Weft mod".into(), &mut off, &mut errors);
                continue;
            }
        };
        let decl = unsafe { entry() };
        if decl.is_null() {
            rejected("empty mod declaration".into(), &mut off, &mut errors);
            continue;
        }
        // Only the version prefix is safe to read before the ABI agrees.
        let version = unsafe { std::ptr::read(decl.cast::<u64>()) };
        if version != abi::ABI_VERSION {
            rejected(format!("built for mod ABI {version:016x}; this engine needs {:016x}", abi::ABI_VERSION), &mut off, &mut errors);
            continue;
        }
        let decl = unsafe { &*decl };
        let id = unsafe { decl.id.as_str() }.to_string();
        if id != name {
            rejected(format!("the library declares mod {id}; its folder or zip must use that name"), &mut off, &mut errors);
            continue;
        }
        mods.push(LoadedMod { id, assets: dir.join("assets"), source, _lib: lib, decl });
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
            after: Vec::new(),
            before: Vec::new(),
            order: 0,
            assets: m.assets.clone(),
            source: m.source.clone(),
        });
    }
    off.sort_by(|a, b| a.id.cmp(&b.id));
    let newly_disabled = automatically_disabled(&off, &wanted);
    wanted.extend(newly_disabled.iter().cloned());
    wanted.sort();
    wanted.dedup();
    let switches = Switches { file: off_file.to_path_buf(), at_start: wanted.clone(), wanted, run_only };
    if !newly_disabled.is_empty() {
        log::info!("automatically disabled mods: {}", newly_disabled.join(", "));
        switches.save();
    }
    *SWITCHES.lock() = Some(switches);
    let _ = NEWLY_DISABLED.set(newly_disabled);
    let _ = OFF.set(off);
    let _ = LOAD_ERRORS.set(errors);
    let _ = NAMES.set(mods.iter().map(|m| m.id.clone()).collect());

    let metas: Vec<ModMeta> = mods
        .iter()
        .map(|m| unsafe {
            let names = |list: &abi::Slice<abi::Str>| {
                list.as_slice()
                    .iter()
                    .map(|d| d.as_str().to_string())
                    .collect()
            };
            ModMeta {
                id: m.id.clone(),
                version: m.decl.version.as_str().to_string(),
                deps: names(&m.decl.deps),
                after: names(&m.decl.after),
                before: names(&m.decl.before),
                order: 0,
                assets: m.assets.clone(),
                source: m.source.clone(),
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

    let step_order = std::fs::read_to_string(off_file.with_file_name("step_order.txt")).unwrap_or_default();
    let content = content::freeze(metas, regs, &step_order).map_err(|e| e.join("\n"))?;
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
        let deps = |m: &str| -> Option<Vec<String>> {
            Some(match m {
                "top" => vec!["middle".to_string(), "base".to_string()],
                "middle" => vec!["base".to_string()],
                "cycle" => vec!["cycle".to_string()],
                "base" => Vec::new(),
                _ => return None,
            })
        };
        let off = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(runs("top", &off(&[]), &deps));
        assert!(!runs("top", &off(&["base"]), &deps), "all the way down");
        assert!(runs("base", &off(&["top"]), &deps), "not the other way");
        assert!(!runs("missing", &off(&[]), &deps));
        assert!(runs("cycle", &off(&[]), &deps), "a cycle terminates");
        let mut disabled = off(&["middle", "base", "unrelated"]);
        enable("top", &mut disabled, &deps);
        assert_eq!(disabled, off(&["unrelated"]));
        assert!(runs("top", &disabled, &deps));

        let off_mods: Vec<ModMeta> = [
            ("base", vec![]),
            ("middle", vec!["base"]),
            ("top", vec!["middle", "base"]),
            ("broken", vec![]),
            ("addon", vec!["broken"]),
            ("absent_dep", vec!["absent"]),
            ("cycle", vec!["cycle"]),
        ]
        .into_iter()
        .map(|(id, deps)| ModMeta {
            id: id.to_string(),
            version: String::new(),
            deps: off(&deps),
            after: Vec::new(),
            before: Vec::new(),
            order: 0,
            assets: PathBuf::new(),
            source: PathBuf::new(),
        })
        .collect();
        let wanted = off(&["base", "cycle"]);
        let automatic = automatically_disabled(&off_mods, &wanted);
        assert_eq!(automatic, off(&["broken", "addon", "absent_dep"]));
        let mut saved = wanted.clone();
        saved.extend(automatic);
        assert!(automatically_disabled(&off_mods, &saved).is_empty());
        enable("top", &mut saved, &deps);
        assert!(runs("top", &saved, &deps));
        assert_eq!(saved, off(&["cycle", "broken", "addon", "absent_dep"]));
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
        assert_eq!(std::fs::read_to_string(&found[0].library).unwrap(), "mine");
        assert_eq!(found[0].source.parent(), Some(zips[0].as_path()));
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
        assert_eq!(found[0].library.file_name().unwrap(), libloading::library_filename("mymod"));
        assert_eq!(found[0].source, zip);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
