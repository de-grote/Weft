//! The mod index (Weft-Mods-Index, where each mod is built from its source) browsed from the
//! game: what the launcher does on a computer, and all a phone has. A build fits this game when
//! its `abi` is this engine's (`modloader::ABI_VERSION`); installing puts its zip, checked against
//! the SHA-256 the index wrote down, into the player's `mods` with what it needs. A build marked
//! verified had its source read by a maintainer; the others show only when asked for.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};

const INDEX_URL: &str = "https://raw.githubusercontent.com/Weft-Rust/Weft-Mods-Index/gh-pages/index.json";

#[derive(Clone, Debug)]
pub struct IndexMod {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub author: String,
    pub source: String,
    /// Where its icon is, if the index says.
    pub icon: String,
    /// Newest first.
    pub builds: Vec<Build>,
}

#[derive(Clone, Debug)]
pub struct Build {
    pub version: String,
    pub abi: String,
    pub url: String,
    pub sha256: String,
    pub deps: Vec<String>,
    pub verified: bool,
}

impl IndexMod {
    /// The newest build that loads in this game.
    pub fn fitting(&self) -> Option<&Build> {
        let abi = format!("{:016x}", crate::modloader::ABI_VERSION);
        self.builds.iter().find(|b| b.abi == abi)
    }
}

fn parse(bytes: &[u8]) -> Option<Vec<IndexMod>> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let text = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
    let mods = v["mods"].as_array()?;
    Some(
        mods.iter()
            .map(|m| IndexMod {
                id: text(m, "id"),
                name: text(m, "name"),
                summary: text(m, "summary"),
                author: text(m, "author"),
                source: text(m, "source"),
                icon: text(m, "icon"),
                builds: m["builds"]
                    .as_array()
                    .map(|bs| {
                        bs.iter()
                            .map(|b| Build {
                                version: text(b, "version"),
                                abi: text(b, "abi"),
                                url: text(b, "url"),
                                sha256: text(b, "sha256"),
                                deps: b["deps"]
                                    .as_array()
                                    .map(|d| {
                                        d.iter()
                                            .filter_map(|d| d.as_str().map(String::from))
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                verified: b["verified"].as_bool().unwrap_or(false),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .filter(|m| valid_id(&m.id))
            .collect(),
    )
}

/// Mod ids name their zip and library: `[a-z0-9_]`.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// What the background work has come to.
#[derive(Clone, Default)]
pub enum Status {
    #[default]
    Idle,
    Loading,
    /// Downloading this mod.
    Busy(String),
    Failed(String),
}

#[derive(Default)]
struct State {
    index: Option<Vec<IndexMod>>,
    status: Status,
    /// Counts changes, for the screen to build itself again.
    generation: u64,
}

static STATE: parking_lot::Mutex<State> = parking_lot::Mutex::new(State { index: None, status: Status::Idle, generation: 0 });
/// Mods were installed or removed: they load on the next start.
static CHANGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn update(f: impl FnOnce(&mut State)) {
    let mut s = STATE.lock();
    f(&mut s);
    s.generation += 1;
}

pub fn generation() -> u64 {
    STATE.lock().generation
}

pub fn status() -> Status {
    STATE.lock().status.clone()
}

pub fn index() -> Option<Vec<IndexMod>> {
    STATE.lock().index.clone()
}

/// Mods were installed or removed since the start.
pub fn changed() -> bool {
    CHANGED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The certificates come with the game: a phone has none Rust can read without Java.
fn client() -> Result<reqwest::blocking::Client, String> {
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    reqwest::blocking::Client::builder()
        .tls_backend_preconfigured(tls)
        .user_agent(concat!("weft/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    let r = client()?
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    r.bytes().map(|b| b.to_vec()).map_err(|e| e.to_string())
}

/// Fetches the index in the background; offline, the last copy that came (`cache`).
pub fn refresh(cache: &Path) {
    if matches!(status(), Status::Loading | Status::Busy(_)) {
        return;
    }
    update(|s| s.status = Status::Loading);
    if fake::on() {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(600));
            update(|s| (s.index, s.status) = (Some(fake::mods()), Status::Idle));
        });
        return;
    }
    let file = cache.join("mod-index.json");
    std::thread::spawn(move || {
        let fetched = get(INDEX_URL).and_then(|b| parse(&b).map(|i| (i, b)).ok_or("not an index".into()));
        match fetched {
            Ok((index, bytes)) => {
                let _ = file.parent().map(std::fs::create_dir_all);
                let _ = std::fs::write(&file, bytes);
                fetch_icons(&index, &file.with_file_name("mod-icons"));
                update(|s| (s.index, s.status) = (Some(index), Status::Idle));
            }
            Err(e) => {
                log::warn!("mod index: {e}");
                let cached = std::fs::read(&file).ok().and_then(|b| parse(&b));
                update(|s| {
                    s.status = if cached.is_some() { Status::Idle } else { Status::Failed(e) };
                    s.index = cached.or(s.index.take());
                });
            }
        }
    });
}

/// Where mod `id`'s icon from `url` is kept.
fn icon_file(dir: &Path, id: &str, url: &str) -> PathBuf {
    dir.join(format!("{id}-{:08x}.png", crate::registry::name_hash(url) as u32))
}

/// Mods' icons the index points at, those not here yet, one after another.
fn fetch_icons(index: &[IndexMod], dir: &Path) {
    let todo: Vec<(String, String)> = index
        .iter()
        .filter(|m| !m.icon.is_empty() && !icon_file(dir, &m.id, &m.icon).exists())
        .map(|m| (m.id.clone(), m.icon.clone()))
        .collect();
    if todo.is_empty() {
        return;
    }
    let dir = dir.to_path_buf();
    std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        for (id, url) in todo {
            // A picture, and a small one: what the index points at is the mod author's.
            if let Some(bytes) = get(&url)
                .ok()
                .filter(|b| b.starts_with(b"\x89PNG") && b.len() < 1 << 18)
            {
                let _ = std::fs::write(icon_file(&dir, &id, &url), bytes);
                update(|_| {});
            }
        }
    });
}

/// Mod `id`'s icon as a GUI texture name, if it came.
pub fn icon(cache: &Path, m: &IndexMod) -> Option<String> {
    let path = icon_file(&cache.join("mod-icons"), &m.id, &m.icon);
    (!m.icon.is_empty() && path.exists()).then(|| format!("{}{}", crate::ui::render::FILE, path.display()))
}

fn sha256_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The mods in `dir` as zips, with each one's SHA-256.
pub fn installed(dir: &Path) -> Vec<(String, String)> {
    if fake::on() {
        fake::seed();
        return fake::INSTALLED.lock().clone();
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")))
        .filter_map(|p| {
            let id = p.file_stem()?.to_string_lossy().into_owned();
            Some((id, sha256_of(&std::fs::read(&p).ok()?)))
        })
        .collect()
}

/// Resolves the complete download list before anything is written or downloaded.
pub fn install_plan(id: &str, dir: &Path) -> Result<Vec<IndexMod>, String> {
    let index = index().ok_or("the catalog is not loaded")?;
    let metas = crate::content::try_content()
        .into_iter()
        .flat_map(|c| &c.mods)
        .chain(crate::modloader::turned_off());
    let mut have: rustc_hash::FxHashMap<String, Vec<String>> = metas
        .filter(|m| m.source.exists() && crate::modloader::load_error(&m.id).is_none())
        .map(|m| (m.id.clone(), m.deps.clone()))
        .collect();
    let abi = format!("{:016x}", crate::modloader::ABI_VERSION);
    for (id, sha) in installed(dir) {
        let build = index.iter().find(|m| m.id == id).and_then(|m| {
            m.builds
                .iter()
                .find(|b| b.sha256.eq_ignore_ascii_case(&sha))
        });
        match build {
            Some(b) if b.abi == abi => {
                have.insert(id, b.deps.clone());
            }
            Some(_) => {
                have.remove(&id);
            }
            None if crate::modloader::load_error(&id).is_some() => {
                have.remove(&id);
            }
            None => {}
        }
    }
    plan(id, &index, &have)
}

fn plan(id: &str, index: &[IndexMod], have: &rustc_hash::FxHashMap<String, Vec<String>>) -> Result<Vec<IndexMod>, String> {
    let mut todo = vec![id.to_string()];
    let mut planned: Vec<IndexMod> = Vec::new();
    let mut needed = rustc_hash::FxHashMap::<String, Vec<String>>::default();
    while let Some(want) = todo.pop() {
        if needed.contains_key(&want) {
            continue;
        }
        let deps = if let Some(deps) = have.get(&want).filter(|_| want != id) {
            deps.clone()
        } else {
            let m = index
                .iter()
                .find(|m| m.id == want)
                .ok_or_else(|| format!("missing dependency {want}"))?;
            let b = m
                .fitting()
                .ok_or_else(|| format!("{} has no build for this game", m.name))?;
            planned.push(m.clone());
            b.deps.clone()
        };
        todo.extend(deps.iter().cloned());
        needed.insert(want, deps);
    }
    let mut names: Vec<_> = needed.keys().collect();
    names.sort();
    let order = crate::content::dependency_order(names.len(), |i, j| needed[names[j]].contains(names[i]));
    planned.sort_by_key(|m| order[names.binary_search(&&m.id).unwrap()]);
    Ok(planned)
}

/// Downloads exactly the builds the player reviewed, with their SHA-256 checked.
pub fn install(plan: Vec<IndexMod>, dir: PathBuf) {
    let Some(id) = plan.last().map(|m| m.id.clone()) else { return };
    update(|s| s.status = Status::Busy(id.clone()));
    if fake::on() {
        fake::install(plan);
        return;
    }
    std::thread::spawn(move || {
        let mut changed = false;
        let result = (|| {
            for m in plan {
                let b = m
                    .fitting()
                    .ok_or(format!("{} has no build for this game", m.name))?;
                let bytes = get(&b.url)?;
                if !sha256_of(&bytes).eq_ignore_ascii_case(&b.sha256) {
                    return Err(format!("{} came different from what the index built", m.name));
                }
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let to = dir.join(format!("{}.zip", m.id));
                let part = to.with_extension("part");
                std::fs::write(&part, &bytes)
                    .and_then(|_| std::fs::rename(&part, &to))
                    .map_err(|e| e.to_string())?;
                changed = true;
            }
            Ok::<_, String>(())
        })();
        if changed {
            CHANGED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        update(|s| {
            s.status = match result {
                Ok(()) => Status::Idle,
                Err(e) => Status::Failed(e),
            }
        });
    });
}

pub fn remove(id: &str, dir: &Path) {
    if fake::on() {
        fake::INSTALLED.lock().retain(|m| m.0 != id);
        CHANGED.store(true, std::sync::atomic::Ordering::Relaxed);
        update(|_| {});
        return;
    }
    if valid_id(id) && std::fs::remove_file(dir.join(format!("{id}.zip"))).is_ok() {
        CHANGED.store(true, std::sync::atomic::Ordering::Relaxed);
        update(|_| {});
    }
}

/// `--fake-catalog`: made-up mods instead of the index, to look at the screen with a full list;
/// installing and removing only pretend, nothing is downloaded or written.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_plan_resolves_graph_before_installing() {
        let entry = |id: &str, deps: &[&str]| IndexMod {
            id: id.into(),
            name: id.into(),
            summary: String::new(),
            author: String::new(),
            source: String::new(),
            icon: String::new(),
            builds: vec![Build {
                version: "1".into(),
                abi: format!("{:016x}", crate::modloader::ABI_VERSION),
                url: String::new(),
                sha256: String::new(),
                deps: deps.iter().map(|s| s.to_string()).collect(),
                verified: true,
            }],
        };
        let mut index = vec![
            entry("top", &["left", "right", "shipped"]),
            entry("left", &["shared"]),
            entry("right", &["shared"]),
            entry("shared", &[]),
        ];
        let have = [("shipped".into(), Vec::new())]
            .into_iter()
            .collect::<rustc_hash::FxHashMap<_, _>>();
        let ids = |p: Vec<IndexMod>| {
            let mut ids: Vec<_> = p.into_iter().map(|m| m.id).collect();
            ids.sort();
            ids
        };
        assert_eq!(ids(plan("top", &index, &have).unwrap()), ["left", "right", "shared", "top"]);
        let mut installed = have.clone();
        for id in ["left", "right"] {
            installed.insert(id.into(), vec!["shared".into()]);
        }
        assert_eq!(
            ids(plan("top", &index, &installed).unwrap()),
            ["shared", "top"],
            "installed mods still need their dependencies resolved"
        );
        assert!(plan("top", &index, &Default::default()).is_err(), "a missing dependency rejects the whole plan");
        index[3].builds[0].abi = "another".into();
        assert!(plan("top", &index, &have).is_err(), "a transitive ABI mismatch rejects the whole plan");
        index[3] = entry("shared", &["top"]);
        assert_eq!(
            ids(plan("top", &index, &have).unwrap()),
            ["left", "right", "shared", "top"],
            "cycles follow the loader's deterministic order"
        );
    }
}

mod fake {
    use super::{Build, CHANGED, IndexMod, Status, update};

    pub fn on() -> bool {
        crate::app::api::launch_options().contains_key("fake-catalog")
    }

    /// (id, the SHA-256 of what is "installed"): one up to date, one a version behind.
    pub static INSTALLED: parking_lot::Mutex<Vec<(String, String)>> = parking_lot::Mutex::new(Vec::new());
    static SEEDED: std::sync::Once = std::sync::Once::new();

    fn sha(id: &str, version: &str) -> String {
        format!("{:064x}", crate::registry::name_hash(&format!("{id}@{version}")) as u128)
    }

    pub fn mods() -> Vec<IndexMod> {
        let abi = format!("{:016x}", crate::modloader::ABI_VERSION);
        // id, name, author, summary, verified, deps, versions (newest first), fits this game.
        let list: &[(&str, &str, &str, &str, bool, &[&str], &[&str], bool)] = &[
            (
                "cogwork",
                "Cogwork",
                "Zorro909",
                "Gears, shafts and belts that carry power from a water wheel to whatever needs turning: mills, saws, presses.",
                true,
                &[],
                &["0.4.1", "0.4.0", "0.3.2"],
                true,
            ),
            (
                "structgen",
                "Structgen",
                "Zorro909",
                "Generates structures and hamlets across the overworld, and lets you save and load your own.",
                true,
                &[],
                &["0.3.0"],
                true,
            ),
            (
                "showcase",
                "Showcase",
                "Zorro909",
                "Films of the game: a camera that flies a path through a world as it generates.",
                false,
                &["structgen"],
                &["0.2.0"],
                true,
            ),
            ("lanterns", "Lanterns", "mira", "Twelve colored lanterns.", true, &[], &["1.0.0"], true),
            (
                "better_farming_and_rural_life_overhaul",
                "Better Farming and Rural Life: a Complete Overhaul of Crops, Animals, Seasons and lots and lots of different other things",
                "the_very_long_name_collective",
                "Crops that grow by season and by how wet the ground is, animals that need feeding and shelter, a farmer's almanac that keeps track of it all, scarecrows, compost, greenhouses, irrigation channels that carry water from a river further than a bucket would, and a market in every village that buys what you grow at prices that change with the weather. Long descriptions wrap, and the panel scrolls when there is more of it than fits.",
                true,
                &["lanterns"],
                &["2.3.0", "2.2.5"],
                true,
            ),
            ("tiny", "T", "a", "", false, &[], &["0.0.1"], true),
            (
                "old_magic",
                "Old Magic",
                "wizard42",
                "Wands, runes and a tower. Made for an older game: no build fits this one yet.",
                true,
                &[],
                &["1.2.0"],
                false,
            ),
            (
                "cartography",
                "Cartography",
                "mira",
                "Maps that fill in as you walk, markers, and a table to copy them on.",
                true,
                &["lanterns"],
                &["0.9.0", "0.8.0"],
                true,
            ),
            (
                "redstone_plus",
                "Redstone+",
                "sparky",
                "Gates, timers, displays and wireless pairs for bigger machines.",
                false,
                &[],
                &["0.5.0"],
                true,
            ),
            (
                "biomes_galore",
                "Biomes Galore",
                "terra",
                "Forty new biomes: salt flats, cherry hills, ash plains, mangroves.",
                true,
                &[],
                &["3.1.0", "3.0.0"],
                true,
            ),
            (
                "deep_dark_caves",
                "Deep Dark Caves",
                "terra",
                "Caves under the caves, with their own light and things living there.",
                true,
                &["biomes_galore"],
                &["1.1.0"],
                true,
            ),
            ("pipes", "Pipes", "flux", "Item and liquid pipes, pumps and filters.", true, &[], &["2.0.0"], true),
            (
                "quests",
                "Quests",
                "storyteller",
                "A quest book: chapters, rewards and tasks a modpack writes in a text file.",
                false,
                &[],
                &["0.3.0"],
                true,
            ),
            ("hats", "Hats", "mira", "Hats. Worn on any model, players' own ones too.", true, &[], &["1.0.2"], true),
            (
                "weather_plus",
                "Weather Plus",
                "storm",
                "Fog banks, hail, heat haze and storms that move across the map.",
                true,
                &[],
                &["0.7.0"],
                true,
            ),
            (
                "tools_of_the_trade",
                "Tools of the Trade",
                "smith",
                "Hammers that mine 3 x 3, axes that fell whole trees, sickles.",
                false,
                &["pipes"],
                &["0.2.1"],
                true,
            ),
        ];
        list.iter()
            .map(|&(id, name, author, summary, verified, deps, versions, fits)| IndexMod {
                id: id.into(),
                name: name.into(),
                summary: summary.into(),
                author: author.into(),
                source: format!("https://github.com/{author}/{id}"),
                icon: String::new(),
                builds: versions
                    .iter()
                    .map(|v| Build {
                        version: v.to_string(),
                        abi: if fits { abi.clone() } else { "0000000000000000".into() },
                        url: String::new(),
                        sha256: sha(id, v),
                        deps: deps.iter().map(|d| d.to_string()).collect(),
                        verified,
                    })
                    .collect(),
            })
            .collect()
    }

    /// What is "installed" at the start: the newest of one, an older one of another.
    pub fn seed() {
        SEEDED.call_once(|| {
            *INSTALLED.lock() = vec![("lanterns".into(), sha("lanterns", "1.0.0")), ("cogwork".into(), sha("cogwork", "0.4.0"))];
        });
    }

    /// Takes a moment, then `id` and what it needs are "installed".
    pub fn install(plan: Vec<IndexMod>) {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1200));
            let mut have = INSTALLED.lock();
            for m in plan {
                let b = m.fitting().unwrap();
                have.retain(|h| h.0 != m.id);
                have.push((m.id.clone(), b.sha256.clone()));
            }
            drop(have);
            CHANGED.store(true, std::sync::atomic::Ordering::Relaxed);
            update(|s| s.status = Status::Idle);
        });
    }
}
