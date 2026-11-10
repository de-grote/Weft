//! Every texture the game world draws: the content's one list (blocks, items, particles) and the
//! connected texture tiles after it, each animation
//! frame and glow mask its own image in the bindless table, at its own size. A texture loads the
//! first time something asks for it (the world's before a mesh using them is drawn, the rest one
//! frame later); a reload reads again only the files that changed.

use std::cell::RefCell;

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use weft_render::*;

use crate::assets::{self, Anim, Assets, LoadedTexture};
use crate::content::Content;

#[derive(Clone, PartialEq)]
enum Source {
    File,
    /// Quarters of four other entries (connected textures' compact form).
    Composite([u32; 4]),
    /// Pictures one over the other, each tinted (a stack's `layers`).
    Layers(Vec<(String, u32)>),
    /// Another entry, every frame multiplied by a color (`0xRRGGBB`): a block face's layer of a
    /// fixed tint, or a connected tile of one.
    Tinted(u32, u32),
}

struct Entry {
    name: String,
    source: Source,
    /// Bindless index per frame and of its glow mask (`u32::MAX` none); empty until loaded.
    frames: Vec<u32>,
    glow: Vec<u32>,
    anim: Option<Anim>,
    images: Vec<TextureId>,
    stamp: u64,
    requested: bool,
    size: u32,
}

struct Inner {
    entries: Vec<Entry>,
    /// Entry per texture id: the content's, then connected texture tiles and composites.
    ids: Vec<u32>,
    requests: Vec<u32>,
    /// Glow mask index per bindless color index (`u32::MAX` none), on the GPU too.
    glow_map: Vec<u32>,
    glow_buffer: BufferId,
    glow_capacity: usize,
    glow_dirty: bool,
    generation: u64,
    assets: Assets,
    /// Texture id of each stack's `layers` made so far, by its text.
    layered: FxHashMap<String, u32>,
}

pub struct GameTextures {
    inner: RefCell<Inner>,
    /// Fully transparent, drawn for a texture until it has loaded.
    blank: (TextureId, u32),
}

/// What a load produced for one entry, before it goes to the GPU.
struct Loaded {
    entry: u32,
    tex: LoadedTexture,
    stamp: u64,
}

/// Generations count up across reloads, so a table built from an older set never looks current.
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_generation() -> u64 {
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

fn composite_name(parts: &[String; 4]) -> String {
    format!("composite:{}", parts.join("+"))
}

impl GameTextures {
    /// Also loads the resource packs' models and the connected texture rules, whose textures and
    /// tiles become block textures.
    pub fn new(gpu: &mut dyn Backend, c: &Content, assets: &Assets) -> GameTextures {
        let (looks, names) = super::looks::load(assets, c);
        let (ctm, extra) = super::ctm::load(assets, c, &looks, names);
        super::ctm::set(ctm);
        let mut entries: Vec<Entry> = Vec::new();
        let mut by_name: FxHashMap<String, u32> = FxHashMap::default();
        let mut add = |name: String, source: Source, entries: &mut Vec<Entry>| -> u32 {
            *by_name.entry(name.clone()).or_insert_with(|| {
                entries.push(Entry {
                    name,
                    source,
                    frames: Vec::new(),
                    glow: Vec::new(),
                    anim: None,
                    images: Vec::new(),
                    stamp: 0,
                    requested: false,
                    size: 16,
                });
                entries.len() as u32 - 1
            })
        };
        let mut ids: Vec<u32> = c
            .textures
            .iter()
            .chain(&extra.names)
            .map(|n| match crate::content::tinted_copy(n) {
                Some((of, rgb)) => {
                    let of = add(of.to_string(), Source::File, &mut entries);
                    add(n.clone(), Source::Tinted(of, rgb), &mut entries)
                }
                None => add(n.clone(), Source::File, &mut entries),
            })
            .collect();
        // A face of several layers, drawn one by one, is its first anywhere it is one picture.
        for (t, layers) in c.face_layers.iter().enumerate() {
            if let Some(first) = layers.first().filter(|l| l.tex as usize != t) {
                ids[t] = ids[first.tex as usize];
            }
        }
        for parts in &extra.composites {
            let parts = parts.map(|p| ids.get(p as usize).copied().unwrap_or(0));
            let name = composite_name(&parts.map(|i| entries[i as usize].name.clone()));
            ids.push(add(name, Source::Composite(parts), &mut entries));
        }
        for &(tile, rgb) in &extra.tinted {
            let of = ids.get(tile as usize).copied().unwrap_or(0);
            let name = format!("{}#{rgb:06X}", entries[of as usize].name);
            ids.push(add(name, Source::Tinted(of, rgb), &mut entries));
        }
        super::looks::set(looks);
        let blank = gpu.create_texture(
            &TextureDesc { width: 1, height: 1, mips: 1, format: Format::Rgba8Srgb, render_target: false },
            "blank texture",
        );
        gpu.write_texture(blank, 0, &[0, 0, 0, 0]);
        let blank = (blank, gpu.texture_index(blank));
        let glow_buffer = gpu.create_buffer(4096 * 4, usage::STORAGE, "glow map");
        log::info!("textures: {} known ({} of the content), loaded when first used", entries.len(), c.textures.len());
        let t = GameTextures {
            inner: RefCell::new(Inner {
                entries,
                ids,
                requests: Vec::new(),
                glow_map: Vec::new(),
                glow_buffer,
                glow_capacity: 4096,
                glow_dirty: true,
                generation: next_generation(),
                assets: assets.clone(),
                layered: FxHashMap::default(),
            }),
            blank,
        };
        t.flush_glow(gpu);
        t
    }

    /// Bindless index of an entry's frame `n` (wrapping), or the blank texture while it loads.
    fn frame(&self, entry: Option<u32>, n: u32) -> u32 {
        let Some(e) = entry else { return self.blank.1 };
        let mut i = self.inner.borrow_mut();
        let Some(en) = i.entries.get_mut(e as usize) else { return self.blank.1 };
        if en.frames.is_empty() {
            if !en.requested {
                en.requested = true;
                i.requests.push(e);
            }
            return self.blank.1;
        }
        en.frames[n as usize % en.frames.len()]
    }

    /// The texture id of a stack's `layers` text, made the first time it is asked for; loaded like
    /// any texture once drawn.
    pub fn layered(&self, spec: &str) -> u32 {
        let mut i = self.inner.borrow_mut();
        if let Some(&id) = i.layered.get(spec) {
            return id;
        }
        let parts = crate::stack::parse_layers(spec)
            .into_iter()
            .map(|(n, t)| (n.to_string(), t))
            .collect();
        i.entries.push(Entry {
            name: format!("layers:{spec}"),
            source: Source::Layers(parts),
            frames: Vec::new(),
            glow: Vec::new(),
            anim: None,
            images: Vec::new(),
            stamp: 0,
            requested: false,
            size: 16,
        });
        let entry = i.entries.len() as u32 - 1;
        i.ids.push(entry);
        let id = i.ids.len() as u32 - 1;
        i.layered.insert(spec.to_string(), id);
        id
    }

    /// First frame of texture `id` (a block's face, an item's icon, a particle's frame).
    pub fn texture(&self, id: u32) -> u32 {
        let e = self.inner.borrow().ids.get(id as usize).copied();
        self.frame(e, 0)
    }

    /// The frame of texture `id` at `ticks`, as its `.anim` plays it.
    pub fn frame_now(&self, id: u32, ticks: f32) -> u32 {
        let e = self.inner.borrow().ids.get(id as usize).copied();
        self.frame(e, self.strip_frame(id, ticks))
    }

    /// Which frame of its strip that is, for what draws the file's own picture. Nothing while it
    /// isn't loaded.
    pub fn strip_frame(&self, id: u32, ticks: f32) -> u32 {
        let i = self.inner.borrow();
        let en = i
            .ids
            .get(id as usize)
            .and_then(|&e| i.entries.get(e as usize));
        en.and_then(|en| en.anim.as_ref())
            .map_or(0, |a| a.frame_at(ticks).0)
    }

    /// Texture ids there are: the content's and the connected texture tiles.
    pub fn count(&self) -> usize {
        self.inner.borrow().ids.len()
    }

    /// The glow map on the GPU, indexed by bindless color index.
    pub fn glow_buffer(&self) -> BufferId {
        self.inner.borrow().glow_buffer
    }

    /// Changes whenever textures were loaded, so tables built from them can be redone.
    pub fn generation(&self) -> u64 {
        self.inner.borrow().generation
    }

    /// Block textures `ids` loaded now: a mesh about to be drawn uses them.
    pub fn ensure_blocks(&self, gpu: &mut dyn Backend, ids: impl IntoIterator<Item = u32>) {
        {
            let mut i = self.inner.borrow_mut();
            for id in ids {
                let Some(&e) = i.ids.get(id as usize) else { continue };
                let en = &mut i.entries[e as usize];
                if en.frames.is_empty() && !en.requested {
                    en.requested = true;
                    i.requests.push(e);
                }
            }
        }
        self.load_requested(gpu);
    }

    /// Loads everything asked for since the last call: files read and decoded in parallel, then
    /// uploaded.
    pub fn load_requested(&self, gpu: &mut dyn Backend) {
        let mut i = self.inner.borrow_mut();
        if i.requests.is_empty() {
            return;
        }
        let ids = std::mem::take(&mut i.requests);
        let t = std::time::Instant::now();
        let loaded = load(&i.entries, &i.assets, &ids);
        let n = loaded.len();
        for l in loaded {
            upload(gpu, &mut i, l);
        }
        i.generation = next_generation();
        i.glow_dirty = true;
        drop(i);
        self.flush_glow(gpu);
        log::debug!(target: "load", "loaded {n} textures in {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    }

    fn flush_glow(&self, gpu: &mut dyn Backend) {
        let mut i = self.inner.borrow_mut();
        if !std::mem::take(&mut i.glow_dirty) {
            return;
        }
        let need = i.glow_map.len().max(1);
        if need > i.glow_capacity {
            gpu.destroy_buffer(i.glow_buffer);
            i.glow_capacity = need.next_power_of_two();
            i.glow_buffer = gpu.create_buffer(i.glow_capacity as u64 * 4, usage::STORAGE, "glow map");
        }
        let mut words = i.glow_map.clone();
        words.resize(i.glow_capacity, u32::MAX);
        gpu.write_buffer(i.glow_buffer, 0, bytemuck::cast_slice(&words));
    }

    /// The world shader's tables, per texture id. `info`: four words, nothing read past them or
    /// worked out from the table's length (on macOS, MoltenVK, `arrayLength` came back longer
    /// than it, and every block drew in one flat color): static -> (index, 0, size, 0), animated
    /// -> (offset into `frames`, count | total ticks << 16 | interpolate << 31, size, 0).
    /// `frames`: animation frames (index | ticks << 16).
    pub fn world_tables(&self) -> (Vec<u32>, Vec<u32>) {
        let i = self.inner.borrow();
        let mut info: Vec<u32> = Vec::with_capacity(i.ids.len() * 4);
        let mut frames: Vec<u32> = Vec::new();
        for &e in &i.ids {
            let en = &i.entries[e as usize];
            let size = if en.frames.is_empty() { 16 } else { en.size.max(1) };
            match (&en.anim, en.frames.is_empty()) {
                (Some(a), false) if !a.frames.is_empty() => {
                    let total: u32 = a.frames.iter().map(|f| f.1).sum::<u32>().min(0x7FFF);
                    info.extend([frames.len() as u32, a.frames.len() as u32 | total << 16 | (a.interpolate as u32) << 31, size, 0]);
                    frames.extend(
                        a.frames
                            .iter()
                            .map(|&(f, ticks)| en.frames[f as usize % en.frames.len()] | ticks.min(0xFFFF) << 16),
                    );
                }
                (_, false) => info.extend([en.frames[0], 0, size, 0]),
                (_, true) => info.extend([self.blank.1, 0, size, 0]),
            }
        }
        // An empty storage buffer can't be bound.
        if frames.is_empty() {
            frames.push(0);
        }
        (info, frames)
    }

    /// Takes assets that may have changed on disk: block, item and particle lists and connected
    /// textures are built again, but a texture whose files did not change keeps its images, so
    /// even a huge pack reloads at once. Loaded ones stay loaded.
    pub fn reload(&mut self, gpu: &mut dyn Backend, c: &Content, assets: &Assets) {
        let next = GameTextures::new(gpu, c, assets);
        let old = std::mem::replace(self, next);
        let blank = old.blank.0;
        let mut old = old.inner.into_inner();
        let loaded: FxHashMap<String, u32> = old
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.frames.is_empty())
            .map(|(k, e)| (e.name.clone(), k as u32))
            .collect();
        let (mut kept, mut redo) = (0, 0);
        {
            let mut n = self.inner.borrow_mut();
            // Only what was loaded is compared; the rest loads fresh when first used anyway.
            let unchanged = |name: &str| {
                loaded
                    .get(name)
                    .is_some_and(|&o| assets::texture_stamp(assets, name) == old.entries[o as usize].stamp)
            };
            let entries = &n.entries;
            let same: Vec<bool> = entries
                .par_iter()
                .map(|e| match &e.source {
                    Source::File => unchanged(&e.name),
                    Source::Composite(parts) => loaded
                        .get(&e.name)
                        .is_some_and(|&o| composite_stamp(assets, entries, *parts) == old.entries[o as usize].stamp),
                    // Made again when next asked for.
                    Source::Layers(_) | Source::Tinted(..) => false,
                })
                .collect();
            for k in 0..n.entries.len() {
                let Some(&o) = loaded.get(&n.entries[k].name) else { continue };
                if same[k] {
                    let (oe, ne) = (&mut old.entries[o as usize], &mut n.entries[k]);
                    (ne.frames, ne.glow, ne.images, ne.anim, ne.stamp, ne.size) = (
                        std::mem::take(&mut oe.frames),
                        std::mem::take(&mut oe.glow),
                        std::mem::take(&mut oe.images),
                        oe.anim.take(),
                        oe.stamp,
                        oe.size,
                    );
                    let pairs: Vec<(u32, u32)> = ne
                        .frames
                        .iter()
                        .copied()
                        .zip(ne.glow.iter().copied())
                        .collect();
                    for (f, g) in pairs {
                        set_glow(&mut n.glow_map, f, g);
                    }
                    kept += 1;
                } else {
                    n.entries[k].requested = true;
                    n.requests.push(k as u32);
                    redo += 1;
                }
            }
            n.glow_dirty = true;
        }
        for e in &mut old.entries {
            for t in e.images.drain(..) {
                gpu.destroy_texture(t);
            }
        }
        gpu.destroy_buffer(old.glow_buffer);
        gpu.destroy_texture(blank);
        self.load_requested(gpu);
        self.flush_glow(gpu);
        log::info!("textures reloaded: {redo} changed, {kept} kept");
    }
}

/// A composite changes when any of its parts does.
fn composite_stamp(assets: &Assets, entries: &[Entry], parts: [u32; 4]) -> u64 {
    parts
        .iter()
        .fold(weft_noise::FNV, |h, &p| weft_noise::fnv_step(h, assets::texture_stamp(assets, &entries[p as usize].name)))
}

/// A stack's `layers` from their files: the same picture for its icon, its sprite in the hand
/// and on the ground.
pub fn layered(assets: &Assets, parts: &[(String, u32)]) -> assets::LoadedTexture {
    let loaded: Vec<assets::LoadedTexture> = parts
        .iter()
        .map(|(n, _)| assets::load_texture(assets, n))
        .collect();
    let refs: Vec<(&assets::LoadedTexture, u32)> = loaded.iter().zip(parts).map(|(l, p)| (l, p.1)).collect();
    assets::layered_texture(&refs)
}

fn set_glow(map: &mut Vec<u32>, color: u32, glow: u32) {
    if color as usize >= map.len() {
        map.resize(color as usize + 1, u32::MAX);
    }
    map[color as usize] = glow;
}

/// Reads and prepares entries in parallel (composites and tinted copies read their sources again).
fn load(entries: &[Entry], assets: &Assets, ids: &[u32]) -> Vec<Loaded> {
    ids.par_iter()
        .map(|&e| {
            let (tex, stamp) = load_one(entries, assets, e);
            Loaded { entry: e, tex, stamp }
        })
        .collect()
}

fn load_one(entries: &[Entry], assets: &Assets, e: u32) -> (LoadedTexture, u64) {
    let en = &entries[e as usize];
    match &en.source {
        Source::File => (assets::load_texture(assets, &en.name), assets::texture_stamp(assets, &en.name)),
        Source::Layers(parts) => (layered(assets, parts), 0),
        &Source::Composite(ids) => {
            let parts = ids.map(|p| assets::load_texture(assets, &entries[p as usize].name));
            (assets::composite_texture([&parts[0], &parts[1], &parts[2], &parts[3]]), composite_stamp(assets, entries, ids))
        }
        &Source::Tinted(of, rgb) => {
            let (mut tex, stamp) = load_one(entries, assets, of);
            let t = [(rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255];
            for px in tex
                .frames
                .iter_mut()
                .flatten()
                .flat_map(|m| m.chunks_exact_mut(4))
            {
                for k in 0..3 {
                    px[k] = (px[k] as u32 * t[k] / 255) as u8;
                }
            }
            (tex, stamp)
        }
    }
}

fn upload(gpu: &mut dyn Backend, i: &mut Inner, l: Loaded) {
    let size = l.tex.size;
    let label = &i.entries[l.entry as usize].name;
    let mut images = Vec::new();
    let mut frames = Vec::new();
    let mut glows = Vec::new();
    for (mips, glow) in l.tex.frames.iter().zip(&l.tex.glow) {
        let t = gpu.create_texture(
            &TextureDesc { width: size, height: size, mips: mips.len() as u32, format: Format::Rgba8Srgb, render_target: false },
            label,
        );
        for (m, data) in mips.iter().enumerate() {
            gpu.write_texture(t, m as u32, data);
        }
        let index = gpu.texture_index(t);
        images.push(t);
        let g = glow.as_ref().map_or(u32::MAX, |mask| {
            let g = gpu
                .create_texture(&TextureDesc { width: size, height: size, mips: 1, format: Format::R8Unorm, render_target: false }, label);
            gpu.write_texture(g, 0, mask);
            images.push(g);
            gpu.texture_index(g)
        });
        set_glow(&mut i.glow_map, index, g);
        frames.push(index);
        glows.push(g);
    }
    let en = &mut i.entries[l.entry as usize];
    for t in std::mem::replace(&mut en.images, images) {
        gpu.destroy_texture(t);
    }
    (en.frames, en.glow, en.anim, en.stamp, en.requested, en.size) = (frames, glows, l.tex.anim, l.stamp, false, size);
}
