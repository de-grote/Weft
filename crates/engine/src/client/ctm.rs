//! Connected and varied block textures. A `<texture>.ctm` file next to a block texture (in a mod
//! or a resource pack) holds rules for every cube face showing that texture, on any block and any
//! face (a log side is just its side texture); a pack that replaces the picture without a file of
//! its own leaves it with none. Methods and keys follow OptiFine/MCPatcher CTM so existing packs
//! port with little more than renaming files:
//!
//! ```text
//! method = compact       # ctm (47 tiles), compact (5), horizontal, vertical, h+v, v+h (4/4/7/7),
//!                        # top (1), random, repeat, fixed (1)
//! tiles = glass/0-4      # relative to this file; ranges expand, <default> is the plain texture
//! connect = tile         # tile (default: neighbors showing this texture on that face, whatever
//!                        # block they are), block (same block), state (same block state)
//! faces = sides          # all, top, bottom, sides, north, south, east, west
//! inner_seams = true     # covered neighbors do not connect
//! hides_neighbors = false  # the block no longer hides the faces next to it
//! weights = 8 1 1        # random
//! symmetry = opposite    # random: none, opposite (pairs opposite faces), all
//! width = 2              # repeat
//! height = 2
//! blocks = base:stone #ores/*
//! heights = 0-40 100-    # world y ranges
//! biomes = !base:desert #cold
//!
//! [another rule]         # starts a further rule; the first one whose filters pass wins
//! ```
//!
//! Resolving is a texture id swap at mesh time, so records and shaders are unchanged. Compact sets
//! are expanded to all 47 tiles when the block textures are listed.
//!
//! Every method works on every block: what a method looks at are the neighbors in the plane of a
//! face, whatever is drawn there (a plant counts as face 0, a side). Only compact doesn't go with
//! custom models, its tiles being cut for a cube's face. Such a rule stops the game.
//!
//! A tile with a model file of its name (`models/block/...`) is that model: the method picks it
//! for the whole block, and its faces then resolve as any others. `hides_neighbors = false` says
//! a block this applies to has holes, so what is next to it is drawn.

use std::sync::{Arc, RwLock};

use rustc_hash::{FxHashMap, FxHashSet};

use super::looks::{Drawn, Looks, Names};
use super::mesher::{NORMALS, Neighborhood, pidx};
use crate::assets::Assets;
use crate::content::{Content, ModelBox};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Method {
    Ctm,
    Horizontal,
    Vertical,
    HorizontalVertical,
    VerticalHorizontal,
    Top,
    Random,
    Repeat,
    Fixed,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Connect {
    Block,
    Tile,
    State,
}

#[derive(Debug)]
struct Rule {
    method: Method,
    connect: Connect,
    /// Bit per face direction.
    faces: u8,
    inner_seams: bool,
    blocks: Vec<u32>,
    heights: Vec<(i32, i32)>,
    biomes: Option<(bool, Vec<u64>)>,
    tiles: Vec<u32>,
    /// Per tile the model it is in `Ctm::models`, or `NO_MODEL`. Empty: only textures.
    models: Vec<u32>,
    /// `hides_neighbors = false`.
    open: bool,
    /// Random: tile index for the top byte of the position hash.
    pick: Vec<u8>,
    /// Random: face groups sharing a hash (1 none, 2 opposite, 6 all).
    symmetry: u8,
    size: (i32, i32),
}

#[derive(Default)]
pub struct Ctm {
    /// Per texture id: range in `rules`.
    by_texture: Vec<(u32, u32)>,
    rules: Vec<Rule>,
    /// Per texture id: its `<name>_overlay` texture id + 1, or 0. The overlay is drawn over the
    /// face and takes the block's tint instead of the face (vanilla's grass sides).
    pub overlay: Vec<u32>,
    /// What tiles draw that are models.
    models: Vec<Vec<ModelBox>>,
    /// Per state, if its faces aren't simply its `textures`: what they show. Neighbors connect
    /// by what shows.
    shown: Vec<Option<Box<[Vec<u32>; 6]>>>,
    /// Per state: the face and texture whose rules may pick it a model, or `NO_MODEL` as the
    /// texture. Empty if no rule has models.
    model_from: Vec<(u8, u32)>,
    /// Per state: drawn with holes, so it hides nothing. Empty if no rule says so.
    open: Vec<bool>,
}

const NO_MODEL: u32 = u32::MAX;

/// Textures needed on top of the content's block textures.
#[derive(Default)]
pub struct Extra {
    /// Tile textures, ids continue after the content's block textures.
    pub names: Vec<String>,
    /// Compact sets expanded to 47 tiles: per tile, the source texture id of each quadrant
    /// (top-left, top-right, bottom-left, bottom-right). Ids continue after `names`.
    pub composites: Vec<[u32; 4]>,
}

static CURRENT: RwLock<Option<Arc<Ctm>>> = RwLock::new(None);

pub fn current() -> Arc<Ctm> {
    CURRENT.read().unwrap().clone().unwrap_or_default()
}

pub fn set(ctm: Ctm) {
    *CURRENT.write().unwrap() = Some(Arc::new(ctm));
}

/// MCPatcher's neighbor bits (L 1, DL 2, D 4, DR 8, R 16, UR 32, U 64, UL 128) to the 47-tile
/// template index.
const CTM47: [u8; 256] = [
    0, 3, 0, 3, 12, 5, 12, 15, 0, 3, 0, 3, 12, 5, 12, 15, 1, 2, 1, 2, 4, 7, 4, 29, 1, 2, 1, 2, 13, 31, 13, 14, //
    0, 3, 0, 3, 12, 5, 12, 15, 0, 3, 0, 3, 12, 5, 12, 15, 1, 2, 1, 2, 4, 7, 4, 29, 1, 2, 1, 2, 13, 31, 13, 14, //
    36, 17, 36, 17, 24, 19, 24, 43, 36, 17, 36, 17, 24, 19, 24, 43, 16, 18, 16, 18, 6, 46, 6, 21, 16, 18, 16, 18, 28, 9, 28, 22, //
    36, 17, 36, 17, 24, 19, 24, 43, 36, 17, 36, 17, 24, 19, 24, 43, 37, 40, 37, 40, 30, 8, 30, 34, 37, 40, 37, 40, 25, 23, 25, 45, //
    0, 3, 0, 3, 12, 5, 12, 15, 0, 3, 0, 3, 12, 5, 12, 15, 1, 2, 1, 2, 4, 7, 4, 29, 1, 2, 1, 2, 13, 31, 13, 14, //
    0, 3, 0, 3, 12, 5, 12, 15, 0, 3, 0, 3, 12, 5, 12, 15, 1, 2, 1, 2, 4, 7, 4, 29, 1, 2, 1, 2, 13, 31, 13, 14, //
    36, 39, 36, 39, 24, 41, 24, 27, 36, 39, 36, 39, 24, 41, 24, 27, 16, 42, 16, 42, 6, 20, 6, 10, 16, 42, 16, 42, 28, 35, 28, 44, //
    36, 39, 36, 39, 24, 41, 24, 27, 36, 39, 36, 39, 24, 41, 24, 27, 37, 38, 37, 38, 30, 11, 30, 32, 37, 38, 37, 38, 25, 33, 25, 26,
];

/// Compact tile per quadrant (TL, TR, BL, BR) of each 47 template tile: 0 isolated, 1 connected
/// all round, 2 connected up/down, 3 connected left/right, 4 inner corner. Tile 41 differs from
/// Angelica's generator, which repeats tile 27 there and loses the inner corner.
const COMPACT: [[u8; 4]; 47] = [
    [0, 0, 0, 0],
    [0, 3, 0, 3],
    [3, 3, 3, 3],
    [3, 0, 3, 0],
    [0, 3, 2, 4],
    [3, 0, 4, 2],
    [2, 4, 2, 4],
    [3, 3, 4, 4],
    [4, 1, 4, 4],
    [4, 4, 4, 1],
    [1, 4, 1, 4],
    [1, 1, 4, 4],
    [0, 0, 2, 2],
    [0, 3, 2, 1],
    [3, 3, 1, 1],
    [3, 0, 1, 2],
    [2, 4, 0, 3],
    [4, 2, 3, 0],
    [4, 4, 3, 3],
    [4, 2, 4, 2],
    [1, 4, 4, 4],
    [4, 4, 1, 4],
    [4, 4, 1, 1],
    [4, 1, 4, 1],
    [2, 2, 2, 2],
    [2, 1, 2, 1],
    [1, 1, 1, 1],
    [1, 2, 1, 2],
    [2, 4, 2, 1],
    [3, 3, 1, 4],
    [2, 1, 2, 4],
    [3, 3, 4, 1],
    [1, 1, 1, 4],
    [1, 1, 4, 1],
    [4, 1, 1, 4],
    [1, 4, 4, 1],
    [2, 2, 0, 0],
    [2, 1, 0, 3],
    [1, 1, 3, 3],
    [1, 2, 3, 0],
    [4, 1, 3, 3],
    [1, 2, 4, 2],
    [1, 4, 3, 3],
    [4, 2, 1, 2],
    [1, 4, 1, 1],
    [4, 1, 1, 1],
    [4, 4, 4, 4],
];

/// Texture u (right) and up directions of each face, matching the corner order in the mesher.
const AXES: [([i32; 3], [i32; 3]); 6] = [
    ([0, 0, 1], [0, 1, 0]),
    ([0, 0, -1], [0, 1, 0]),
    ([1, 0, 0], [0, 0, 1]),
    ([1, 0, 0], [0, 0, -1]),
    ([-1, 0, 0], [0, 1, 0]),
    ([1, 0, 0], [0, 1, 0]),
];

fn faces(v: &str) -> u8 {
    v.split_whitespace().fold(0, |m, f| {
        m | match f {
            "all" => 0b111111,
            "bottom" => 1 << 2,
            "top" => 1 << 3,
            "sides" => 0b110011,
            "west" => 1 << 0,
            "east" => 1 << 1,
            "north" => 1 << 4,
            "south" => 1 << 5,
            _ => 0,
        }
    })
}

fn ranges(v: &str) -> Vec<(i32, i32)> {
    v.split_whitespace()
        .filter_map(|r| match r.split_once('-') {
            Some((a, b)) => Some((a.parse().unwrap_or(i32::MIN), b.parse().unwrap_or(i32::MAX))),
            None => r.parse().ok().map(|y| (y, y)),
        })
        .collect()
}

/// Tile names relative to the folder of `texture`; `a-b` at the end of an entry expands to numbers.
fn tile_names(v: &str, texture: &str) -> Vec<Option<String>> {
    let dir = match texture.rfind('/') {
        Some(i) => texture[..=i].to_string(),
        None => texture
            .split_once(':')
            .map_or(String::new(), |(ns, _)| format!("{ns}:")),
    };
    let full = |rel: &str| if rel.contains(':') { rel.to_string() } else { format!("{dir}{rel}") };
    let mut out = Vec::new();
    for t in v.split_whitespace() {
        if t == "<default>" {
            out.push(None);
            continue;
        }
        let (prefix, last) = t.rsplit_once('/').map_or(("", t), |(p, l)| (p, l));
        let prefix = if prefix.is_empty() { String::new() } else { format!("{prefix}/") };
        match last
            .split_once('-')
            .and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?)))
        {
            Some((a, b)) => out.extend((a..=b).map(|i| Some(full(&format!("{prefix}{i}"))))),
            None => out.push(Some(full(t))),
        }
    }
    out
}

/// Whether something drawn other than as a cube shows `tex`, on a face in `faces`.
fn off_cube(d: &Drawn, tex: u32, faces: u8) -> bool {
    d.model != weft_abi::BlockModel::Cube && (0..6).any(|f| faces & (1 << f) != 0 && d.on_face(f).contains(&tex))
}

/// The tile `name` as a model, if it has a file. A face the file names no texture for shows the
/// picture of the same name if there is one, else `plain`.
fn tile_model(assets: &Assets, name: &str, plain: u32, names: &mut Names) -> Option<Vec<ModelBox>> {
    assets.path(name, "models", "json")?;
    let read = |n: &str| {
        assets
            .read(n, "models", "json")
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    let m = crate::block_model::load(name, &read).unwrap_or_else(|e| {
        log::error!("tile {e}");
        None
    })?;
    let own = if assets.path(name, "textures", "png").is_some() { names.id(name) } else { plain };
    let boxes = super::looks::file_boxes(&m, &|_| own, &mut |t| names.id(t));
    (!boxes.is_empty()).then_some(boxes)
}

/// Reads the `.ctm` files of all block textures, those `looks` brought included (all in `names`)
/// and those of tiles that are models. Other textures get ids after those.
pub fn load(assets: &Assets, c: &Content, looks: &Looks, mut names: Names) -> (Ctm, Extra) {
    let base = c.textures.len();
    let mut extra = Extra::default();
    let mut ctm = Ctm::default();
    let mut overlays: Vec<(u32, u32)> = Vec::new();
    let mut todo: Vec<u32> = (0..names.list.len() as u32).collect();
    let mut queued: FxHashSet<u32> = todo.iter().copied().collect();
    let mut tile_models: FxHashMap<(String, u32), u32> = FxHashMap::default();
    // Compact sets waiting for their composite ids, which come after every tile name.
    let mut compact: Vec<(usize, u32, Vec<u32>)> = Vec::new();
    let mut next = 0;
    while let Some(&tex) = todo.get(next) {
        next += 1;
        let name = names.list[tex as usize].clone();
        let over = format!("{name}_overlay");
        if assets.path(&over, "textures", "png").is_some() {
            overlays.push((tex, names.id(&over) + 1));
        }
        let Some(src) = assets
            .sidecar(&name, "textures", "png", "ctm")
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
        else {
            continue;
        };
        let start = ctm.rules.len() as u32;
        for section in crate::kv::sections(&src) {
            let kv: FxHashMap<&str, &str> = section.iter().map(|p| (p.1, p.2)).collect();
            if kv.is_empty() {
                continue;
            }
            let method = match kv.get("method").copied().unwrap_or("ctm") {
                "ctm" | "glass" | "default" => Method::Ctm,
                "compact" | "ctm_compact" => Method::Ctm,
                "horizontal" | "bookshelf" => Method::Horizontal,
                "vertical" => Method::Vertical,
                "h+v" | "horizontal+vertical" => Method::HorizontalVertical,
                "v+h" | "vertical+horizontal" => Method::VerticalHorizontal,
                "top" | "sandstone" => Method::Top,
                "random" => Method::Random,
                "repeat" | "pattern" => Method::Repeat,
                "fixed" | "static" => Method::Fixed,
                m => {
                    log::warn!("{name}.ctm: unknown method {m}");
                    continue;
                }
            };
            let is_compact = matches!(kv.get("method").copied(), Some("compact" | "ctm_compact"));
            let (mut tiles, mut models) = (Vec::new(), Vec::new());
            for t in tile_names(kv.get("tiles").copied().unwrap_or(""), &name) {
                let Some(n) = t else {
                    tiles.push(tex);
                    models.push(NO_MODEL);
                    continue;
                };
                let known = tile_models.get(&(n.clone(), tex)).copied();
                let model = known.or_else(|| {
                    let boxes = tile_model(assets, &n, tex, &mut names)?;
                    for t in boxes.iter().flat_map(|b| b.textures) {
                        if queued.insert(t) {
                            todo.push(t);
                        }
                    }
                    ctm.models.push(boxes);
                    let m = ctm.models.len() as u32 - 1;
                    tile_models.insert((n.clone(), tex), m);
                    Some(m)
                });
                // A model's tile shown on a single face is the plain texture: the face is the
                // model's now.
                tiles.push(if model.is_some() { tex } else { names.id(&n) });
                models.push(model.unwrap_or(NO_MODEL));
            }
            let needed = match method {
                Method::Ctm if is_compact => 5,
                Method::Ctm => 47,
                Method::Horizontal | Method::Vertical => 4,
                Method::HorizontalVertical | Method::VerticalHorizontal => 7,
                Method::Repeat => {
                    let w = kv
                        .get("width")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1)
                        .max(1);
                    let h = kv
                        .get("height")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1)
                        .max(1);
                    w * h
                }
                _ => 1,
            };
            if tiles.len() < needed {
                log::warn!("{name}.ctm: {:?} needs {needed} tiles, has {}", method, tiles.len());
                continue;
            }
            let has_models = models.iter().any(|m| *m != NO_MODEL);
            if !has_models {
                models.clear();
            }
            if is_compact && has_models {
                let why = format!(
                    "{name}.ctm: compact ctm doesn't support custom models, and its tiles have model files: use ctm with all 47 tiles"
                );
                log::error!("{why}");
                panic!("{why}");
            }
            let rule_index = ctm.rules.len();
            if is_compact {
                compact.push((rule_index, tex, tiles[..5].to_vec()));
                tiles.clear();
            }
            let pick = if method == Method::Random {
                let given: Vec<u32> = kv.get("weights").map_or(Vec::new(), |w| {
                    w.split_whitespace()
                        .filter_map(|x| x.parse().ok())
                        .collect()
                });
                let avg = if given.is_empty() { 1 } else { (given.iter().sum::<u32>() / given.len() as u32).max(1) };
                let weights: Vec<u32> = (0..tiles.len())
                    .map(|i| given.get(i).copied().unwrap_or(avg))
                    .collect();
                let total: u32 = weights.iter().sum::<u32>().max(1);
                let mut lut = Vec::with_capacity(256);
                let (mut acc, mut i) = (0u32, 0usize);
                for slot in 0..256u32 {
                    while i + 1 < weights.len() && (slot * total) / 256 >= acc + weights[i] {
                        acc += weights[i];
                        i += 1;
                    }
                    lut.push(i as u8);
                }
                lut
            } else {
                Vec::new()
            };
            // References like anywhere else: names, `name[prop=value]`, `#tag`, `#pattern/*`.
            let mut blocks = Vec::new();
            for b in kv.get("blocks").map_or("", |v| *v).split_whitespace() {
                match crate::refs::blocks(b, &c.block_names, &c.blocks, &c.components) {
                    Ok(list) => blocks.extend(list),
                    Err(e) => log::error!("{name}.ctm: blocks: {e}"),
                }
            }
            let biomes = kv.get("biomes").map(|v| {
                let (invert, list) = v.strip_prefix('!').map_or((false, *v), |r| (true, r));
                let mut set = vec![0u64; c.biomes.len() / 64 + 1];
                for b in list.split_whitespace() {
                    match crate::refs::set(b, crate::components::BIOME, &c.biome_names, c.biomes.len() as u32, &c.components, "biome") {
                        Ok(list) => list
                            .into_iter()
                            .for_each(|i| set[i as usize / 64] |= 1 << (i % 64)),
                        Err(e) => log::error!("{name}.ctm: biomes: {e}"),
                    }
                }
                (invert, set)
            });
            ctm.rules.push(Rule {
                method,
                connect: match kv.get("connect").copied() {
                    Some("block") => Connect::Block,
                    Some("state") => Connect::State,
                    _ => Connect::Tile,
                },
                faces: kv.get("faces").map_or(0b111111, |v| faces(v)),
                inner_seams: kv.get("inner_seams").is_some_and(|v| *v == "true"),
                blocks,
                heights: kv.get("heights").map_or(Vec::new(), |v| ranges(v)),
                biomes,
                tiles,
                models,
                open: kv.get("hides_neighbors").is_some_and(|v| *v == "false"),
                pick,
                // A model is one for the block, so every face's look at the rule must agree.
                symmetry: match kv.get("symmetry").copied() {
                    _ if has_models => 6,
                    Some("opposite") => 2,
                    Some("all") => 6,
                    _ => 1,
                },
                size: (
                    kv.get("width")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1)
                        .max(1),
                    kv.get("height")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1)
                        .max(1),
                ),
            });
        }
        if ctm.by_texture.len() <= tex as usize {
            ctm.by_texture.resize(tex as usize + 1, (0, 0));
        }
        ctm.by_texture[tex as usize] = (start, ctm.rules.len() as u32 - start);
    }
    let first_composite = names.list.len() as u32;
    for (rule, tex, src) in compact {
        let r = &ctm.rules[rule];
        let name = &names.list[tex as usize];
        let on = c.states.iter().enumerate().find_map(|(st, s)| {
            let hit = (r.blocks.is_empty() || r.blocks.contains(&s.block)) && off_cube(&looks.drawn(st as u32, s), tex, r.faces);
            hit.then(|| {
                c.block_names
                    .iter()
                    .find(|n| *n.1 == s.block)
                    .map_or("a block", |n| n.0.as_str())
            })
        });
        if let Some(on) = on {
            let why = format!(
                "{name}.ctm: compact ctm doesn't support custom models, and {on} shows this texture on one: use ctm with all 47 tiles (or leave it out with blocks =)"
            );
            log::error!("{why}");
            panic!("{why}");
        }
        if ctm
            .models
            .iter()
            .flatten()
            .any(|b| (0..6).any(|d| r.faces & (1 << d) != 0 && super::mesher::box_face(b, d).is_some() && b.textures[d] == tex))
        {
            let why = format!(
                "{name}.ctm: compact ctm doesn't support custom models, and a tile's model shows this texture: use ctm with all 47 tiles"
            );
            log::error!("{why}");
            panic!("{why}");
        }
        let start = first_composite + extra.composites.len() as u32;
        extra
            .composites
            .extend(COMPACT.iter().map(|q| q.map(|code| src[code as usize])));
        ctm.rules[rule].tiles = (start..start + 47).collect();
    }
    ctm.overlay = vec![0; names.list.len()];
    for (tex, over) in overlays {
        ctm.overlay[tex as usize] = over;
    }
    extra.names = names.list.split_off(base);
    if !ctm.rules.is_empty() {
        ctm.see_states(c, looks);
        log::info!(
            "connected textures: {} rules, {} models, {} textures, {} generated",
            ctm.rules.len(),
            ctm.models.len(),
            extra.names.len(),
            extra.composites.len()
        );
    }
    (ctm, extra)
}

impl Ctm {
    /// Works out what the rules mean for each block state.
    fn see_states(&mut self, c: &Content, looks: &Looks) {
        let (any_models, any_open) = (self.rules.iter().any(|r| !r.models.is_empty()), self.rules.iter().any(|r| r.open));
        self.shown = c
            .states
            .iter()
            .enumerate()
            .map(|(st, s)| {
                let d = looks.drawn(st as u32, s);
                (d.model != weft_abi::BlockModel::Cube || d.textures != s.textures).then(|| Box::new(std::array::from_fn(|f| d.on_face(f))))
            })
            .collect();
        if any_models {
            self.model_from = vec![(0, NO_MODEL); c.states.len()];
        }
        if any_open {
            self.open = vec![false; c.states.len()];
        }
        if !any_models && !any_open {
            return;
        }
        for (st, s) in c
            .states
            .iter()
            .enumerate()
            .filter(|(_, s)| s.liquid.is_none())
        {
            let d = looks.drawn(st as u32, s);
            for dir in 0..6 {
                for tex in d.on_face(dir) {
                    let &(start, len) = self.by_texture.get(tex as usize).unwrap_or(&(0, 0));
                    for rule in &self.rules[start as usize..(start + len) as usize] {
                        if !rule.blocks.is_empty() && !rule.blocks.contains(&s.block) {
                            continue;
                        }
                        if rule.open {
                            self.open[st] = true;
                        }
                        if !rule.models.is_empty() && rule.faces & (1 << dir) != 0 && self.model_from[st].1 == NO_MODEL {
                            self.model_from[st] = (dir as u8, tex);
                        }
                    }
                }
            }
        }
    }
}

fn hash(x: i32, y: i32, z: i32, f: u32) -> u64 {
    weft_noise::hash(f as u64, x as i64, y as i64, z as i64)
}

impl Ctm {
    #[inline]
    pub fn has_overlay(&self, tex: u32) -> bool {
        self.overlay.get(tex as usize).is_some_and(|&o| o != 0)
    }

    /// Whether a block of these flags hides the faces next to it.
    #[inline]
    pub fn hides(&self, flags: u32, state: u32) -> bool {
        flags & weft_abi::block_flags::OPAQUE != 0 && !self.open.get(state as usize).is_some_and(|o| *o)
    }

    /// Whether a rule says this state is drawn with holes.
    #[inline]
    pub fn is_open(&self, state: u32) -> bool {
        self.open.get(state as usize).is_some_and(|o| *o)
    }

    /// The model a rule picks for the block at `(x, y, z)` in the cube, if any.
    #[inline]
    pub fn model(&self, c: &Content, n: &Neighborhood, x: i32, y: i32, z: i32, state: u32) -> Option<&[ModelBox]> {
        let &(dir, tex) = self
            .model_from
            .get(state as usize)
            .filter(|f| f.1 != NO_MODEL)?;
        let &(start, len) = self.by_texture.get(tex as usize)?;
        let (rule, tile) = self.pick(c, n, x, y, z, dir as usize, tex, start as usize..(start + len) as usize)?;
        self.models
            .get(*rule.models.get(tile)? as usize)
            .map(Vec::as_slice)
    }

    /// Texture for face `dir` of the block at `(x, y, z)` in the cube, `tex` being its plain one.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn resolve(&self, c: &Content, n: &Neighborhood, x: i32, y: i32, z: i32, dir: usize, tex: u32) -> u32 {
        match self.by_texture.get(tex as usize) {
            Some(&(start, len)) if len > 0 => self
                .pick(c, n, x, y, z, dir, tex, start as usize..(start + len) as usize)
                .map_or(tex, |(rule, tile)| rule.tiles.get(tile).copied().unwrap_or(tex)),
            _ => tex,
        }
    }

    /// Whether `state` shows `tex` on face `dir`.
    fn shows(&self, c: &Content, state: u32, dir: usize, tex: u32) -> bool {
        match self.shown.get(state as usize) {
            Some(Some(faces)) => faces[dir].contains(&tex),
            _ => c.states[state as usize].textures[dir] == tex,
        }
    }

    /// The first rule that goes for this face, and which of its tiles it wants.
    #[allow(clippy::too_many_arguments)]
    fn pick(
        &self,
        c: &Content,
        n: &Neighborhood,
        x: i32,
        y: i32,
        z: i32,
        dir: usize,
        tex: u32,
        rules: std::ops::Range<usize>,
    ) -> Option<(&Rule, usize)> {
        let s = n.states[pidx(x, y, z)];
        let info = &c.states[s as usize];
        let world = [n.origin[0] + x, n.origin[1] + y, n.origin[2] + z];
        let (r, u) = AXES[dir];
        let nrm = NORMALS[dir];
        let at = |a: i32, b: i32, extra: [i32; 3]| {
            pidx(x + r[0] * a + u[0] * b + extra[0], y + r[1] * a + u[1] * b + extra[1], z + r[2] * a + u[2] * b + extra[2])
        };
        for rule in &self.rules[rules] {
            if rule.faces & (1 << dir) == 0 || (!rule.blocks.is_empty() && !rule.blocks.contains(&info.block)) {
                continue;
            }
            if !rule.heights.is_empty()
                && !rule
                    .heights
                    .iter()
                    .any(|&(lo, hi)| world[1] >= lo && world[1] <= hi)
            {
                continue;
            }
            if let Some((invert, set)) = &rule.biomes {
                let b = n.biomes[(z * 16 + x) as usize] as usize;
                if set.get(b / 64).is_some_and(|w| w >> (b % 64) & 1 == 1) == *invert {
                    continue;
                }
            }
            let connects = |a: i32, b: i32| -> bool {
                let ns = n.states[at(a, b, [0; 3])];
                let same = match rule.connect {
                    Connect::Block => c.states[ns as usize].block == info.block,
                    Connect::State => ns == s,
                    Connect::Tile => self.shows(c, ns, dir, tex),
                };
                // With inner seams a neighbor whose own face here is covered breaks the run.
                same && !(rule.inner_seams && {
                    let front = n.states[at(a, b, nrm)];
                    front != 0 && self.hides(c.states[front as usize].flags, front)
                })
            };
            let tile = match rule.method {
                Method::Ctm => {
                    let mut bits = 0usize;
                    for (bit, (a, b)) in [(-1, 0), (-1, -1), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1)]
                        .into_iter()
                        .enumerate()
                    {
                        if connects(a, b) {
                            bits |= 1 << bit;
                        }
                    }
                    CTM47[bits] as usize
                }
                Method::Horizontal | Method::HorizontalVertical => {
                    let h = [3, 2, 0, 1][connects(-1, 0) as usize | (connects(1, 0) as usize) << 1];
                    if h != 3 || rule.method == Method::Horizontal {
                        h
                    } else {
                        let down = connects(0, -1) && !connects(-1, -1) && !connects(1, -1);
                        let up = connects(0, 1) && !connects(-1, 1) && !connects(1, 1);
                        match (down, up) {
                            (false, true) => 4,
                            (true, true) => 5,
                            (true, false) => 6,
                            _ => 3,
                        }
                    }
                }
                Method::Vertical | Method::VerticalHorizontal => {
                    let v = [3, 2, 0, 1][connects(0, -1) as usize | (connects(0, 1) as usize) << 1];
                    if v != 3 || rule.method == Method::Vertical {
                        v
                    } else {
                        let left = connects(-1, 0) && !connects(-1, 1) && !connects(-1, -1);
                        let right = connects(1, 0) && !connects(1, 1) && !connects(1, -1);
                        match (left, right) {
                            (false, true) => 4,
                            (true, true) => 5,
                            (true, false) => 6,
                            _ => 3,
                        }
                    }
                }
                Method::Top => {
                    let above = n.states[pidx(x, y + 1, z)];
                    if dir == 2 || dir == 3 || c.states[above as usize].block != info.block {
                        continue;
                    }
                    0
                }
                Method::Random => {
                    let group = (dir as u32) / rule.symmetry as u32;
                    rule.pick[(hash(world[0], world[1], world[2], group) >> 56) as usize] as usize
                }
                Method::Repeat => {
                    let dot = |a: [i32; 3]| world[0] * a[0] + world[1] * a[1] + world[2] * a[2];
                    let (w, h) = rule.size;
                    (w * (-dot(u)).rem_euclid(h) + dot(r).rem_euclid(w)) as usize
                }
                Method::Fixed => 0,
            };
            return Some((rule, tile));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_shows_a_texture_off_a_cube_counts() {
        use weft_abi::BlockModel;
        let (t, sides_only) = ([7; 6], 0b110011);
        let plain = |model| Drawn { model, boxes: &[], textures: t };
        assert!(off_cube(&plain(BlockModel::Cross), 7, 0b111111));
        assert!(!off_cube(&plain(BlockModel::Cross), 8, 0b111111));
        assert!(!off_cube(&plain(BlockModel::Cube), 7, 0b111111));
        let top = [ModelBox::whole([0, 0, 0, 7, 0, 0])];
        let boxes = Drawn { model: BlockModel::Boxes, boxes: &top, textures: [0; 6] };
        assert!(off_cube(&boxes, 7, 0b111111));
        assert!(!off_cube(&boxes, 7, sides_only), "not on the faces the rule is for");
        // A face the file leaves out holds a texture only to fill the field.
        let mut gone = ModelBox::whole([7; 6]);
        gone.uv = [weft_abi::UV_NO_FACE; 6];
        let gone = [gone];
        assert!(!off_cube(&Drawn { model: BlockModel::Boxes, boxes: &gone, textures: [0; 6] }, 7, 0b111111));
    }

    #[test]
    fn axes_match_mesher_corners() {
        // u runs from corner 0 to 1 and up from corner 0 to 3 (see mesher::CORNERS).
        for dir in 0..6 {
            let c = super::super::mesher::corners(dir);
            let sub = |a: [i32; 3], b: [i32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
            assert_eq!(sub(c[1], c[0]), AXES[dir].0, "u of face {dir}");
            assert_eq!(sub(c[3], c[0]), AXES[dir].1, "up of face {dir}");
        }
    }

    #[test]
    fn compact_table_agrees_with_quadrant_rule() {
        // Each quadrant of a 47 tile follows from its two edges and diagonal.
        for bits in 0..256usize {
            let has = |b: usize| bits & b != 0;
            let quad = |side_h: usize, side_v: usize, diag: usize| match (has(side_h), has(side_v)) {
                (true, true) if has(diag) => 1,
                (true, true) => 4,
                (true, false) => 3,
                (false, true) => 2,
                _ => 0,
            };
            let expect = [quad(1, 64, 128), quad(16, 64, 32), quad(1, 4, 2), quad(16, 4, 8)];
            assert_eq!(COMPACT[CTM47[bits] as usize], expect, "bits {bits:08b}");
        }
    }

    #[test]
    fn tile_ranges_expand() {
        let t = tile_names("glass/0-2 <default> other:x/y", "base:block/glass");
        assert_eq!(
            t,
            vec![
                Some("base:block/glass/0".into()),
                Some("base:block/glass/1".into()),
                Some("base:block/glass/2".into()),
                None,
                Some("other:x/y".into())
            ]
        );
    }
}
