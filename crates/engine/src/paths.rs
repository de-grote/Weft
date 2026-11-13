//! Where the game keeps what it writes. The folder it is installed in may not be writable (a
//! system package), and is gone with an update, so a shipped game writes to the player's own
//! folders. One played from the source tree or one that has already been played in its own
//! `run/` stays there.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq)]
pub struct Dirs {
    /// Saves, mods, packs, logs, screenshots.
    pub data: PathBuf,
    pub config: PathBuf,
    /// Unpacked mods: nothing lost when it goes.
    pub cache: PathBuf,
}

impl Dirs {
    /// All in one folder.
    pub fn under(run: &Path) -> Dirs {
        Dirs { data: run.to_path_buf(), config: run.join("config"), cache: run.join("cache") }
    }
}

/// What to start to run the game again. In an AppImage the executable is in a mount that goes
/// away with this process, so it is the image.
pub fn own_program() -> std::io::Result<PathBuf> {
    match std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|p| p.has_root())
    {
        Some(image) => Ok(image),
        None => std::env::current_exe(),
    }
}

/// The folders of a game installed in `root`.
pub fn of_install(root: &Path) -> Dirs {
    let run = root.join("run");
    // A shipped `run/` only holds its mods until the game has been played there.
    if root.join("Cargo.toml").is_file() || run.join("config").is_dir() {
        return Dirs::under(&run);
    }
    per_user(std::env::consts::OS, &|k| std::env::var_os(k).map(PathBuf::from)).unwrap_or_else(|| Dirs::under(&run))
}

fn per_user(os: &str, env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<Dirs> {
    // The XDG spec has relative values ignored, and nothing else here means one.
    let var = |k: &str| env(k).filter(|p| p.has_root());
    let home = var("HOME");
    match os {
        "windows" => {
            let data = var("APPDATA")?.join("Weft");
            let cache = var("LOCALAPPDATA").map_or_else(|| data.join("cache"), |d| d.join("Weft"));
            Some(Dirs { config: data.join("config"), data, cache })
        }
        "macos" => {
            let library = home?.join("Library");
            let data = library.join("Application Support").join("Weft");
            Some(Dirs { config: data.join("config"), data, cache: library.join("Caches").join("Weft") })
        }
        _ => {
            let xdg = |k: &str, fallback: &[&str]| {
                var(k)
                    .or_else(|| Some(fallback.iter().fold(home.clone()?, |p, s| p.join(s))))
                    .map(|p| p.join("weft"))
            };
            Some(Dirs {
                data: xdg("XDG_DATA_HOME", &[".local", "share"])?,
                config: xdg("XDG_CONFIG_HOME", &[".config"])?,
                cache: xdg("XDG_CACHE_HOME", &[".cache"])?,
            })
        }
    }
}

static SHIPPED: OnceLock<PathBuf> = OnceLock::new();

/// Where a shipped game keeps its own files (`assets/core`): next to the executable, or on Android
/// where the package's were unpacked (`set_shipped`).
pub fn shipped() -> Option<PathBuf> {
    SHIPPED.get().cloned().or_else(|| {
        std::env::current_exe()
            .ok()?
            .parent()
            .map(Path::to_path_buf)
    })
}

/// Once, at start, where an Android package's files were unpacked.
pub fn set_shipped(dir: PathBuf) {
    let _ = SHIPPED.set(dir);
}

/// Where the engine's own code is, which a shipped game's mod libraries sit beside: the
/// executable's folder, or on Android the package's native libraries, the only place a library
/// may load from there.
pub fn libraries() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
        if unsafe { libc::dladdr(libraries as *const libc::c_void, &mut info) } == 0 || info.dli_fname.is_null() {
            return None;
        }
        let path = unsafe { std::ffi::CStr::from_ptr(info.dli_fname) };
        Path::new(&*path.to_string_lossy())
            .parent()
            .map(Path::to_path_buf)
    }
    #[cfg(not(target_os = "android"))]
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

static CONFIG: OnceLock<PathBuf> = OnceLock::new();

/// Once, at start, with what `config` should answer.
pub fn set_config(dir: PathBuf) {
    let _ = CONFIG.set(dir);
}

/// The player's settings, keys and lists; `<run_dir>/config` unless told otherwise.
pub fn config(run_dir: &Path) -> PathBuf {
    CONFIG
        .get()
        .cloned()
        .unwrap_or_else(|| run_dir.join("config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<PathBuf> + 'a {
        |k| vars.iter().find(|v| v.0 == k).map(|v| PathBuf::from(v.1))
    }

    #[test]
    fn linux_follows_xdg_with_its_fallbacks() {
        let home = PathBuf::from("/home/a");
        let d = per_user("linux", &env(&[("HOME", "/home/a")])).unwrap();
        assert_eq!(d.data, home.join(".local").join("share").join("weft"));
        assert_eq!(d.config, home.join(".config").join("weft"));
        assert_eq!(d.cache, home.join(".cache").join("weft"));
        let d =
            per_user("linux", &env(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "/d"), ("XDG_CONFIG_HOME", "/c"), ("XDG_CACHE_HOME", "/k")]))
                .unwrap();
        assert_eq!((d.data, d.config, d.cache), (PathBuf::from("/d/weft"), PathBuf::from("/c/weft"), PathBuf::from("/k/weft")));
    }

    #[test]
    fn linux_ignores_relative_xdg_values() {
        let d = per_user("linux", &env(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "here")])).unwrap();
        assert_eq!(
            d.data,
            PathBuf::from("/home/a")
                .join(".local")
                .join("share")
                .join("weft")
        );
    }

    #[test]
    fn linux_without_home_has_no_folders() {
        assert_eq!(per_user("linux", &env(&[("XDG_DATA_HOME", "/d")])), None);
    }

    #[test]
    fn windows_splits_roaming_and_local() {
        let d = per_user("windows", &env(&[("APPDATA", "/roaming"), ("LOCALAPPDATA", "/local")])).unwrap();
        assert_eq!(d.data, PathBuf::from("/roaming").join("Weft"));
        assert_eq!(d.config, PathBuf::from("/roaming").join("Weft").join("config"));
        assert_eq!(d.cache, PathBuf::from("/local").join("Weft"));
    }

    #[test]
    fn macos_uses_the_library() {
        let d = per_user("macos", &env(&[("HOME", "/Users/a")])).unwrap();
        assert_eq!(
            d.data,
            PathBuf::from("/Users/a")
                .join("Library")
                .join("Application Support")
                .join("Weft")
        );
        assert_eq!(
            d.cache,
            PathBuf::from("/Users/a")
                .join("Library")
                .join("Caches")
                .join("Weft")
        );
    }

    #[test]
    fn a_game_played_in_its_own_folder_stays_there() {
        let root = std::env::temp_dir().join(format!("weft-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("run").join("config")).unwrap();
        assert_eq!(of_install(&root), Dirs::under(&root.join("run")));
        std::fs::remove_dir_all(root.join("run").join("config")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        assert_eq!(of_install(&root), Dirs::under(&root.join("run")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
