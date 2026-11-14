//! Entity models on the GPU: every kind's boxes and triangles in two static tables; per frame
//! the bones of every visible entity are posed on the CPU (parents, keyframes, the look) and each
//! layer (skin or something worn) is one instance, drawn instanced per kind.
//! Models and animations come from code or from files (`model.rs`); files win, so resource packs
//! can change any entity's model.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use weft_abi::{ItemStack, comp};
use weft_render::*;

use crate::assets::Assets;
use crate::block_model::{self, Display};
use crate::components::{ENTITY, ITEM};
use crate::content::content;
pub use crate::gpu_layout::Instance;
use crate::gpu_layout::{GpuCorner, GpuCuboid, rows};
use crate::model::{self, Animation, BoneMotion, Bound, Drive, Motion, Playing, role};
use crate::net::AvatarData;
use crate::registry::{Cuboid, Part};

/// A kind's model and animations.
pub struct KindModel {
    pub parts: Vec<Part>,
    cuboids: (u32, u32),
    corners: (u32, u32),
    /// What worn items are drawn on (`add_worn_geometry`), first and count as above.
    worn_cuboids: (u32, u32),
    worn_corners: (u32, u32),
    /// Per bone, the bone it is worn on if it is worn geometry (`model::worn_targets`).
    worn_on: Vec<Option<usize>>,
    pub anims: Vec<Bound>,
    /// Per action, its clips: this kind's own and what is left of the stock one under them.
    actions: FxHashMap<String, Vec<u32>>,
    /// The same for what an item in hand brings along.
    items: FxHashMap<u32, FxHashMap<String, Vec<u32>>>,
    /// The actions the engine plays, by `Act`, resolved once: mobs by the thousand go by these.
    engine: [Vec<u32>; ACTS.len()],
    /// Texture size the model file was made for (it overrides the kind's).
    pub texture: Option<[u32; 2]>,
    /// A model that isn't an entity kind's: the texture it goes with.
    pub skin: Option<String>,
    /// Bones that move by themselves, parents first.
    live: Vec<LiveBone>,
    /// Its holders' clips that name its bones: they play when those play on a holder.
    brought: Vec<u32>,
    /// What it always has on: (model, bone it goes on).
    pub carries: Vec<(u32, usize)>,
    /// Where a file says its eyes are: an empty bone called `eyes`.
    eye_bone: Option<usize>,
    /// Where it holds things, main hand and other, if a file says.
    pub item_bones: [Option<usize>; 2],
    /// Corners of its box at rest, in pixels.
    pub bounds: (Vec3, Vec3),
    /// How far from its feet it reaches at rest, in blocks.
    pub reach: f32,
}

/// A bone that follows its `BoneMotion`.
struct LiveBone {
    bone: usize,
    /// It and all that hang on it.
    under: Vec<usize>,
    /// From the pivot to the middle of what hangs on it, in the model's pixels.
    reach: Vec3,
    motion: BoneMotion,
}

/// Where the end of a dangling bone is, relative to the camera, and was the step before.
#[derive(Clone, Copy)]
struct Swing {
    tip: Vec3,
    last: Vec3,
}

impl Swing {
    /// `dt` further on, with its bone's pivot at `pivot` and where its clips have the end at `pivot +
    /// rest`: what pulls it moves it, what it hangs from drags it along at its length.
    fn step(&mut self, pivot: Vec3, rest: Vec3, motion: BoneMotion, dt: f32) {
        let length = rest.length().max(1e-3);
        let steps = ((dt * 120.0).ceil() as u32).clamp(1, 8);
        let h = dt.min(0.1) / steps as f32;
        let keep = (-motion.drag * h).exp();
        for _ in 0..steps {
            let pull = Vec3::NEG_Y * motion.gravity * length + (pivot + rest - self.tip) * motion.spring;
            let next = self.tip + (self.tip - self.last) * keep + pull * h * h;
            self.last = self.tip;
            self.tip = pivot + (next - pivot).normalize_or(rest / length) * length;
        }
    }
}

/// What a mod did to a bone for the next frame.
#[derive(Clone, Copy)]
pub struct BonePose {
    /// Radians about x, y, z.
    pub turn: Vec3,
    /// Pixels.
    pub by: Vec3,
    pub scale: Vec3,
}

/// One model's dangling bones on one entity.
struct Swings {
    list: Vec<Swing>,
    seen: u32,
}

/// The actions the engine asks for on every entity.
#[derive(Clone, Copy)]
pub enum Act {
    Idle,
    Walk,
    Sneak,
    Swim,
    Crawl,
    Swing,
    Death,
    Ground,
}

const ACTS: [&str; 8] = ["idle", "walk", "sneak", "swim", "crawl", "swing", "death", "ground"];

impl Act {
    pub fn name(self) -> &'static str {
        ACTS[self as usize]
    }
}

impl KindModel {
    /// Its boxes and triangles in the tables: the skin's, or those worn items are drawn on.
    fn ranges(&self, worn: bool) -> ((u32, u32), (u32, u32)) {
        if worn { (self.worn_cuboids, self.worn_corners) } else { (self.cuboids, self.corners) }
    }

    /// What plays for an engine action, nothing in hand or swapped.
    pub fn act(&self, a: Act) -> &[u32] {
        &self.engine[a as usize]
    }

    /// What plays for action `name`: the item's clips if it has them, else the kind's.
    pub fn action(&self, item: u32, name: &str) -> &[u32] {
        if item != 0 {
            if let Some(c) = self.items.get(&item).and_then(|t| t.get(name)) {
                return c;
            }
        }
        self.actions.get(name).map_or(&[], Vec::as_slice)
    }

    /// The item brings clips of its own for this action.
    pub fn has_own(&self, item: u32, name: &str) -> bool {
        self.items.get(&item).is_some_and(|t| t.contains_key(name))
    }

    /// The swing of what is in the hand: from a held pose a whole arm's swing goes over the head.
    pub fn swing_of(&self, item: u32) -> &'static str {
        if item != 0 && !self.has_own(item, "swing") && self.actions.contains_key("swing_held") { "swing_held" } else { "swing" }
    }

    /// `name.first_person` from the first person, when it exists.
    pub fn action_seen(&self, item: u32, name: &str, first_person: bool) -> &[u32] {
        if first_person {
            let own = self.action(item, &format!("{name}.first_person"));
            if !own.is_empty() {
                return own;
            }
        }
        self.action(item, name)
    }

    /// Adds `clips` to what plays this frame.
    pub fn play(&self, out: &mut Clips, clips: &[u32], how: &Play) {
        for &i in clips {
            let Some(b) = self.anims.get(i as usize) else { continue };
            let t = match b.anim.drive.unwrap_or(how.drive) {
                Drive::Clock => b.at(how.clock, how.looping),
                Drive::Distance => b.at(how.distance, true),
                Drive::Progress => how.progress.clamp(0.0, 1.0) * b.anim.length,
            };
            let speed = if how.by_speed || b.anim.by_speed { how.speed } else { 1.0 };
            let w = how.weight * b.anim.weight * speed;
            if w > 0.0 {
                out.push(PlayClip {
                    anim: i,
                    t,
                    w,
                    mirror: how.mirror,
                    aim: how.aim,
                    hand: how.hand,
                    by_progress: how.drive == Drive::Progress,
                });
            }
        }
    }
}

/// How the clips of an action play this frame.
#[derive(Clone, Copy)]
pub struct Play {
    pub drive: Drive,
    pub clock: f32,
    pub distance: f32,
    pub progress: f32,
    pub looping: bool,
    pub weight: f32,
    pub speed: f32,
    pub by_speed: bool,
    pub mirror: bool,
    pub aim: bool,
    /// One hand's and what it holds, not the whole body's.
    pub hand: bool,
}

impl Default for Play {
    fn default() -> Play {
        Play {
            drive: Drive::Clock,
            clock: 0.0,
            distance: 0.0,
            progress: 0.0,
            looping: false,
            weight: 1.0,
            speed: 1.0,
            by_speed: false,
            mirror: false,
            aim: false,
            hand: false,
        }
    }
}

/// How many clips play on one entity at most: more than come at once, for those on their way out.
pub const MAX_CLIPS: usize = 16;

#[derive(Clone, Copy, Default)]
pub struct PlayClip {
    pub anim: u32,
    pub t: f32,
    pub w: f32,
    pub mirror: bool,
    pub aim: bool,
    pub hand: bool,
    pub by_progress: bool,
}

/// A clip coming in or going out on one entity: how far in it was when it last turned (`from`,
/// 0..1, at its holder's clock `since`) and how fast it goes now, so it is a function of the
/// holder's clock and every client has it alike.
#[derive(Clone, Copy)]
pub struct Fade {
    clip: PlayClip,
    since: f32,
    from: f32,
    rate: f32,
}

impl Fade {
    fn on(&self, now: f32) -> f32 {
        (self.from + self.rate * (now - self.since).max(0.0)).clamp(0.0, 1.0)
    }

    /// From `now` on at `rate` a second, going on from where it is.
    fn turn(&mut self, now: f32, rate: f32) {
        if self.rate != rate {
            (self.from, self.since, self.rate) = (self.on(now), now, rate);
        }
    }
}

/// Seconds a clip takes to come and go unless its file says. What goes by an action's progress
/// starts at once: its first frames are the action's.
const BLEND_SECS: f32 = 0.12;

/// Eases what plays over the standing clips in and out by the holder's clock `now` (seconds):
/// what is wanted comes in, what no longer is goes out where it stopped. What is below keeps its
/// order: over it, later clips win.
pub fn fade_clips(fades: &mut Vec<Fade>, m: &KindModel, clips: &mut Clips, standing: usize, now: f32) {
    let wanted = *clips;
    clips.n = standing;
    let anim = |c: &PlayClip| m.anims.get(c.anim as usize).map(|b| &b.anim);
    fades.retain_mut(|f| {
        if wanted.all()[standing..]
            .iter()
            .any(|c| (c.anim, c.mirror) == (f.clip.anim, f.clip.mirror))
        {
            return true;
        }
        let secs = anim(&f.clip)
            .and_then(|a| a.blend_out)
            .unwrap_or(BLEND_SECS);
        f.turn(now, -1.0 / secs.max(1e-3));
        let on = f.on(now);
        if on <= 0.0 {
            return false;
        }
        clips.push(PlayClip { w: f.clip.w * smooth(on), ..f.clip });
        true
    });
    for c in &wanted.all()[standing..] {
        let secs = anim(c)
            .and_then(|a| a.blend_in)
            .unwrap_or(if c.by_progress { 0.0 } else { BLEND_SECS });
        let found = fades
            .iter()
            .position(|f| (f.clip.anim, f.clip.mirror) == (c.anim, c.mirror));
        // What has no time to come in is there at once.
        let i = found.unwrap_or_else(|| {
            fades.push(Fade { clip: *c, since: now, from: if secs <= 0.0 { 1.0 } else { 0.0 }, rate: 0.0 });
            fades.len() - 1
        });
        let f = &mut fades[i];
        f.turn(now, 1.0 / secs.max(1e-3));
        f.clip = *c;
        clips.push(PlayClip { w: c.w * smooth(f.on(now)), ..*c });
    }
}

fn smooth(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

#[derive(Clone, Copy, Default)]
pub struct Clips {
    pub list: [PlayClip; MAX_CLIPS],
    pub n: usize,
}

impl Clips {
    fn push(&mut self, c: PlayClip) {
        if self.n < MAX_CLIPS {
            self.list[self.n] = c;
            self.n += 1;
        }
    }

    pub fn all(&self) -> &[PlayClip] {
        &self.list[..self.n]
    }
}

/// Layers one entity draws at most: its skin and what it wears.
pub const MAX_LAYERS: usize = 8;

/// One entity to pose this frame: kind, root transform, its look, where in `Models::clips` what
/// plays on it is, and its layers.
#[derive(Clone, Copy)]
pub struct Posed {
    pub id: u64,
    pub kind: u32,
    /// Far away it isn't posed every frame: `Direct` for the usual, `Fresh` posed and kept, `Kept`
    /// going by what was.
    pub lod: PoseLod,
    pub root: Mat4,
    pub pitch: f32,
    pub head_yaw: f32,
    pub clips: (u32, u32),
    pub layers: [Instance; MAX_LAYERS],
    pub layer_count: usize,
}

#[derive(Clone, Copy, PartialEq)]
pub enum PoseLod {
    Direct,
    Fresh,
    Kept,
}

#[derive(Clone, Copy)]
pub enum Batch {
    World,
    Preview,
    /// The first person's arms, after everything else.
    Hand,
    /// Items' pictures for slots.
    Icon,
}

pub struct Models {
    pub kinds: Vec<KindModel>,
    /// Items with a model of bones: their place in `kinds`, past the entities'.
    pub item_kinds: FxHashMap<u32, u32>,
    /// Models read from files, by name, also those that aren't there.
    files: FxHashMap<String, Option<u32>>,
    stock: FxHashMap<&'static str, Vec<Arc<Animation>>>,
    /// What items bring along: holders of models added later need it.
    item_clips: Vec<(u32, Vec<Arc<Animation>>)>,
    /// Everyone's boxes and triangles; the GPU has them as of the last `sync`.
    tables: (Vec<GpuCuboid>, Vec<GpuCorner>),
    stale: bool,
    carriers: FxHashSet<(u32, u32)>,
    /// Textures of new models, for whoever loads them.
    pub new_skins: Vec<String>,
    swings: FxHashMap<(u64, u32, u32), Swings>,
    /// What mods did to bones for the next frame, by entity.
    pub turned: FxHashMap<u64, Vec<(String, BonePose)>>,
    /// Who was posed where, the last frame: (entity, model, first bone), and from where.
    located: Vec<(u64, u32, u32)>,
    located_from: glam::DVec3,
    frame: u32,
    /// What plays on the entities of this frame (`Posed::clips`).
    pub clips: Vec<PlayClip>,
    /// Items whose model file says where they go in the hand.
    pub displays: FxHashMap<u32, Display>,
    cuboids: BufferId,
    corners: BufferId,
    /// Per frame slot, one set per [`Batch`]: the GPU reads them only when the frame's list
    /// runs, so two batches drawn in one frame can't share them. Bones can where they are the
    /// same bones (`bone_set`), and `uploaded` says they are in place.
    instances: [Vec<(BufferId, u32)>; 4],
    bones: [Vec<(BufferId, u32)>; 3],
    uploaded: Option<BufferId>,
    /// This frame: (kind, instance) and bone rows.
    queued: Vec<(u32, Instance)>,
    rows: Vec<[[f32; 4]; 3]>,
    scratch: Vec<Mat4>,
    /// Bones per entity in `add_many`.
    counts: Vec<usize>,
    /// Far entities' bones as of when they were last posed, from their feet: by entity, the model.
    kept: FxHashMap<u64, (u32, Vec<Mat4>)>,
}

/// The items whose slots show a model's picture, and those pictures once drawn. Slots ask from
/// anywhere in the interface, and it is cached there.
#[derive(Default)]
pub struct Icons {
    items: FxHashSet<u32>,
    /// By item and stack data: bindless texture and its part, `None` for no picture to be had.
    ready: FxHashMap<(u32, u32), Option<(u32, [f32; 4])>>,
    pub wanted: Vec<ItemStack>,
}

pub static ICONS: std::sync::LazyLock<std::sync::Mutex<Icons>> = std::sync::LazyLock::new(Default::default);

/// The picture of a model that shows `stack` in a slot, if it has one by now.
pub fn icon_of(stack: ItemStack) -> Option<(u32, [f32; 4])> {
    let mut i = ICONS.lock().unwrap();
    if !i.items.contains(&stack.item) && crate::stack::model(stack, false).is_none() {
        return None;
    }
    let key = crate::stack::visual_key(stack);
    match i.ready.get(&key) {
        Some(r) => *r,
        None => {
            if !i.wanted.iter().any(|w| crate::stack::visual_key(*w) == key) {
                i.wanted.push(stack);
            }
            None
        }
    }
}

pub fn icon_done(stack: ItemStack, icon: Option<(u32, [f32; 4])>) {
    ICONS
        .lock()
        .unwrap()
        .ready
        .insert(crate::stack::visual_key(stack), icon);
}

/// The text of a component of an item or an entity, with `{name}` as the entry's path.
fn file_named(kind: usize, entry: u32, key: &str, name: &str) -> Option<String> {
    let c = content();
    let t = c
        .components
        .id(key)
        .and_then(|id| c.components.text(kind, entry, id))?;
    Some(t.replace("{name}", name.split_once(':').map_or(name, |n| n.1)))
}

/// The kind's model: the file named by its `model` component, else `<ns>:<name>` if a model
/// file of that name exists, else its parts from code.
fn load_kind(assets: &Assets, k: u32) -> (Vec<Part>, Option<[u32; 2]>, Vec<Animation>) {
    let c = content();
    let kind = &c.game.entities[k as usize];
    let named = |key: &str| file_named(ENTITY, k, key, &kind.name);
    let (mut parts, mut texture) = (kind.parts.clone(), None);
    if let Some(file) = named(comp::ANIMATIONS) {
        warn_no_clips(assets, &kind.name, &file);
    }
    let model_name = named(comp::MODEL).unwrap_or_else(|| kind.name.clone());
    if let Some(src) = read(assets, &model_name, "models", "geo.json") {
        match model::load_geo(&src) {
            Ok(g) => (parts, texture) = (g.parts, g.texture),
            Err(e) => log::error!("model {model_name}: {e}"),
        }
    }
    let anim_name = named(comp::ANIMATIONS).unwrap_or_else(|| kind.name.clone());
    (parts, texture, animations(assets, &anim_name))
}

/// A file somebody named on purpose, not there.
fn warn_no_clips(assets: &Assets, of: &str, file: &str) {
    if read(assets, file, "animations", "animation.json").is_none() {
        log::warn!("{of}: no animations {file}");
    }
}

fn read(assets: &Assets, name: &str, kind: &str, ext: &str) -> Option<String> {
    assets
        .read(name, kind, ext)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn animations(assets: &Assets, name: &str) -> Vec<Animation> {
    match read(assets, name, "animations", "animation.json") {
        Some(src) => model::load_animations(&src).unwrap_or_else(|e| {
            log::error!("animations {name}: {e}");
            Vec::new()
        }),
        None => Vec::new(),
    }
}

/// The engine's own animations for a model of this shape, which any of its bones go by unless
/// the kind has clips of its own for them.
fn stock_file(parts: &[Part]) -> &'static str {
    let has = |r: u32| parts.iter().any(|p| p.role == r);
    let arms = parts
        .iter()
        .filter(|p| p.role == role::ARM_LEFT || p.role == role::ARM_RIGHT);
    if has(role::LEG_SWEEP) {
        "core:stock_spider"
    } else if has(role::LEG_FRONT_LEFT) || has(role::LEG_FRONT_RIGHT) {
        "core:stock_quadruped"
    } else if arms.clone().any(|p| p.rest[0].abs() > 1.0) {
        "core:stock_biped_arms_out"
    } else if arms.count() > 0 || has(role::LEG_LEFT) || has(role::LEG_RIGHT) {
        "core:stock_biped"
    } else {
        "core:stock_other"
    }
}

const STOCK_FILES: [&str; 5] =
    ["core:stock_biped", "core:stock_biped_arms_out", "core:stock_quadruped", "core:stock_spider", "core:stock_other"];

/// A box of bone `bone` for the table, drawn when bone `cover` (of role `role`) is.
fn gpu_cuboid(cu: &Cuboid, p: &Part, bone: usize, cover: usize, role: u32) -> GpuCuboid {
    GpuCuboid {
        origin: [p.pivot[0] + cu.origin[0], p.pivot[1] + cu.origin[1], p.pivot[2] + cu.origin[2]],
        bone: bone as u32,
        size: cu.size,
        inflate: cu.inflate,
        faces: cu.face_rects(),
        role,
        cover: cover as u32,
        pad1: 0,
        pad2: 0,
    }
}

/// Bone `bone`'s triangles for the table, drawn when bone `cover` (of role `role`) is.
fn gpu_corners(p: &Part, bone: usize, cover: usize, role: u32) -> impl Iterator<Item = GpuCorner> + '_ {
    p.mesh
        .iter()
        .map(move |v| GpuCorner { pos: v.pos, bone: bone as u32, uv: v.uv, role, cover: cover as u32 })
}

/// A model's boxes and triangles into the two tables, but for its worn geometry.
fn add_geometry(parts: &[Part], worn_on: &[Option<usize>], cuboids: &mut Vec<GpuCuboid>, corners: &mut Vec<GpuCorner>) {
    for (b, p) in parts
        .iter()
        .enumerate()
        .filter(|(b, _)| worn_on[*b].is_none())
    {
        cuboids.extend(p.cuboids.iter().map(|cu| gpu_cuboid(cu, p, b, b, p.role)));
        corners.extend(gpu_corners(p, b, b, p.role));
    }
}

/// What worn items are drawn on, per bone of a model: its `armor_` bones if it has any; else
/// vanilla armor's piece in vanilla's unwrap fitted to its biggest box, where there is one
/// (`model::armor_unwrap`); else its own boxes and triangles, for a texture in its skin's layout.
/// Every model has it, players' own ones too, so armor goes on whatever wears it.
fn add_worn_geometry(parts: &[Part], worn_on: &[Option<usize>], cuboids: &mut Vec<GpuCuboid>, corners: &mut Vec<GpuCorner>) {
    let volume = |c: &&Cuboid| c.size[0] * c.size[1] * c.size[2];
    for (t, p) in parts
        .iter()
        .enumerate()
        .filter(|(t, _)| worn_on[*t].is_none())
    {
        let own: Vec<usize> = (0..parts.len())
            .filter(|&b| worn_on[b] == Some(t))
            .collect();
        if !own.is_empty() {
            for b in own {
                let q = &parts[b];
                cuboids.extend(q.cuboids.iter().map(|cu| gpu_cuboid(cu, q, b, t, p.role)));
                corners.extend(gpu_corners(q, b, t, p.role));
            }
        } else if let (Some((uv, size)), Some(main)) = (
            model::armor_unwrap(p),
            p.cuboids
                .iter()
                .max_by(|a, b| volume(a).total_cmp(&volume(b))),
        ) {
            // Limbs at -x have their unwrap mirrored, as vanilla's.
            let mirror = p.pivot[0] + main.origin[0] + main.size[0] / 2.0 < 0.0;
            let piece = Cuboid { origin: [0.0; 3], size, uv, inflate: 0.0, mirror, faces: None };
            cuboids.push(GpuCuboid { faces: piece.face_rects(), ..gpu_cuboid(main, p, t, t, p.role) });
        } else {
            cuboids.extend(p.cuboids.iter().map(|cu| gpu_cuboid(cu, p, t, t, p.role)));
            corners.extend(gpu_corners(p, t, t, p.role));
        }
    }
}

/// The middle of what a bone and those under it have, in the model's pixels.
fn middle_of(parts: &[Part], under: &[usize]) -> Option<Vec3> {
    let (mut sum, mut n) = (Vec3::ZERO, 0.0);
    for &i in under {
        let p = &parts[i];
        for c in &p.cuboids {
            sum += Vec3::from_array(p.pivot) + Vec3::from_array(c.origin) + Vec3::from_array(c.size) / 2.0;
            n += 1.0;
        }
        for v in &p.mesh {
            sum += Vec3::from_array(v.pos);
            n += 1.0;
        }
    }
    (n > 0.0).then(|| sum / n)
}

/// What a model's bones do by what is said of it: its sidecar, then mods.
fn motions_of(assets: &Assets, model: &str) -> Vec<(String, BoneMotion)> {
    let mut out = read(assets, model, "models", "bones").map_or_else(Vec::new, |s| model::load_bones(&s));
    out.extend(
        content()
            .game
            .bone_motions
            .iter()
            .filter(|m| m.0 == model)
            .map(|m| (m.1.clone(), m.2)),
    );
    out
}

/// Bone `i` and all that hang on it.
fn subtree(parts: &[Part], i: usize) -> Vec<usize> {
    let mut out = vec![i];
    for (k, p) in parts.iter().enumerate().skip(i + 1) {
        if p.parent.is_some_and(|q| out.contains(&(q as usize))) {
            out.push(k);
        }
    }
    out
}

/// The corners of a model's box at rest.
fn bounds_of(parts: &[Part]) -> (Vec3, Vec3) {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in parts {
        model::part_points(p, |v| {
            lo = lo.min(v);
            hi = hi.max(v);
        });
    }
    if lo.x > hi.x { (Vec3::ZERO, Vec3::ZERO) } else { (lo, hi) }
}

/// Its bones that move by themselves: by name, by a sidecar, by a mod, the later over the earlier.
fn live_bones(model_name: &str, parts: &[Part], said: &[(String, BoneMotion)]) -> Vec<LiveBone> {
    for (bone, _) in said {
        if model::bone_named(parts, bone).is_none() {
            log::warn!("model {model_name} has no bone {bone} to move by itself");
        }
    }
    parts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let by_word = said
                .iter()
                .rev()
                .find(|s| model::bone_named(parts, &s.0) == Some(i))
                .map(|s| s.1);
            let motion = by_word.or_else(|| model::motion_by_name(&p.name))?;
            let under = subtree(parts, i);
            let reach = middle_of(parts, &under)
                .map(|m| m - Vec3::from_array(p.pivot))
                .filter(|r| r.length_squared() > 1e-3)
                .unwrap_or(Vec3::NEG_Y * 8.0);
            Some(LiveBone { bone: i, under, reach, motion })
        })
        .collect()
}

/// Binds one action's clips: `over` wins on the bones it names, `under` plays the rest.
fn bind(anims: &mut Vec<Bound>, parts: &[Part], over: &Arc<Animation>, under: &[u32]) -> Vec<u32> {
    let top = Bound::new(over.clone(), parts);
    let mut out = Vec::new();
    for &i in under {
        let rest = anims[i as usize].without(&top);
        out.push(anims.len() as u32);
        anims.push(rest);
    }
    out.push(anims.len() as u32);
    anims.push(top);
    out
}

/// A kind's clips, by action, from its own file over the stock one, then what items bring.
fn bind_kind(
    parts: &[Part],
    own: &[Arc<Animation>],
    stock: &[Arc<Animation>],
    items: &[(u32, Vec<Arc<Animation>>)],
) -> (Vec<Bound>, FxHashMap<String, Vec<u32>>, FxHashMap<u32, FxHashMap<String, Vec<u32>>>) {
    let mut anims: Vec<Bound> = Vec::new();
    let mut actions: FxHashMap<String, Vec<u32>> = FxHashMap::default();
    for a in stock {
        actions.insert(a.name.clone(), vec![anims.len() as u32]);
        anims.push(Bound::new(a.clone(), parts));
    }
    for a in own {
        let under = actions.remove(&a.name).unwrap_or_default();
        let top = bind(&mut anims, parts, a, &under);
        actions.insert(a.name.clone(), top);
    }
    let mut by_item = FxHashMap::default();
    if parts.iter().any(|p| p.role == role::ARM_RIGHT) {
        for (item, clips) in items {
            let mut table: FxHashMap<String, Vec<u32>> = FxHashMap::default();
            for a in clips {
                let under = actions.get(&a.name).cloned().unwrap_or_default();
                table.insert(a.name.clone(), bind(&mut anims, parts, a, &under));
            }
            by_item.insert(*item, table);
        }
    }
    (anims, actions, by_item)
}

/// Adds a model to the tables and the list of kinds; its place there.
fn push_kind(
    kinds: &mut Vec<KindModel>,
    tables: &mut (Vec<GpuCuboid>, Vec<GpuCorner>),
    parts: Vec<Part>,
    texture: Option<[u32; 2]>,
    clips: (&[Arc<Animation>], &[Arc<Animation>], &[(u32, Vec<Arc<Animation>>)]),
) -> u32 {
    let worn_on = model::worn_targets(&parts);
    let (c0, v0) = (tables.0.len() as u32, tables.1.len() as u32);
    add_geometry(&parts, &worn_on, &mut tables.0, &mut tables.1);
    let (c1, v1) = (tables.0.len() as u32, tables.1.len() as u32);
    add_worn_geometry(&parts, &worn_on, &mut tables.0, &mut tables.1);
    let (anims, actions, items) = bind_kind(&parts, clips.0, clips.1, clips.2);
    let engine = ACTS.map(|n| actions.get(n).cloned().unwrap_or_default());
    kinds.push(KindModel {
        skin: None,
        live: Vec::new(),
        brought: Vec::new(),
        carries: Vec::new(),
        eye_bone: model::bone_named(&parts, "eyes"),
        item_bones: [model::item_bone(&parts, false), model::item_bone(&parts, true)],
        bounds: bounds_of(&parts),
        reach: {
            let (lo, hi) = bounds_of(&parts);
            lo.abs().max(hi.abs()).length() / 16.0
        },
        cuboids: (c0, c1 - c0),
        corners: (v0, v1 - v0),
        worn_cuboids: (c1, tables.0.len() as u32 - c1),
        worn_corners: (v1, tables.1.len() as u32 - v1),
        worn_on,
        parts,
        anims,
        actions,
        items,
        engine,
        texture,
    });
    kinds.len() as u32 - 1
}

/// Which set of bone buffers a batch uses.
fn bone_set(batch: Batch) -> usize {
    match batch {
        Batch::World | Batch::Hand => 0,
        Batch::Preview => 1,
        Batch::Icon => 2,
    }
}

/// The head of a model and where its eyes are in its pixels, looking ahead: `eye` above the
/// ground, at the front of the head.
fn eyes(m: &KindModel, eye: f32) -> Option<(usize, Vec3)> {
    if let Some(i) = m.eye_bone {
        return Some((i, Vec3::from_array(m.parts[i].pivot)));
    }
    let i = m.parts.iter().position(|p| p.role == role::HEAD)?;
    let head = &m.parts[i];
    let front = head
        .cuboids
        .iter()
        .map(|c| c.origin[2])
        .fold(0.0f32, f32::min);
    Some((i, Vec3::new(head.pivot[0], eye * 16.0, head.pivot[2] + front)))
}

fn playing_of<'a, 'b>(m: &'a KindModel, clips: &'b [PlayClip]) -> impl Iterator<Item = Playing<'a>> + use<'a, 'b> {
    clips.iter().filter_map(|c| {
        m.anims
            .get(c.anim as usize)
            .map(|b| Playing { bound: b, t: c.t, w: c.w, mirror: c.mirror, aim: c.aim })
    })
}

/// How much of a model a stranger may send.
const AVATAR_BONES: usize = 128;
const AVATAR_BOXES: usize = 1024;
const AVATAR_CORNERS: usize = 8192;
const AVATAR_CLIPS: usize = 64;
const AVATAR_CHANNELS: usize = 1024;
const AVATAR_INFLATE: f32 = 8.0;
const AVATAR_KINDS: usize = 64;
/// Pixels from its feet, as far as the biggest mob goes and a little over.
const AVATAR_REACH: f32 = 64.0;
const AVATAR_SCALE: f32 = 4.0;

/// A stranger's model and clips, if nothing in them is past what is allowed.
fn checked_avatar(f: &AvatarData) -> Result<(Vec<Part>, Option<[u32; 2]>, Vec<Animation>), String> {
    let geo = model::load_geo(&String::from_utf8_lossy(&f.geo))?;
    let clips = if f.animations.is_empty() { Vec::new() } else { model::load_animations(&String::from_utf8_lossy(&f.animations))? };
    let boxes: usize = geo.parts.iter().map(|p| p.cuboids.len()).sum();
    let corners: usize = geo.parts.iter().map(|p| p.mesh.len()).sum();
    let channels: usize = clips.iter().map(|a| a.channels.len()).sum();
    if geo.parts.len() > AVATAR_BONES
        || boxes > AVATAR_BOXES
        || corners > AVATAR_CORNERS
        || clips.len() > AVATAR_CLIPS
        || channels > AVATAR_CHANNELS
    {
        return Err(format!(
            "too big: {} bones, {boxes} boxes, {corners} corners, {} clips, {channels} channels",
            geo.parts.len(),
            clips.len()
        ));
    }
    let (lo, hi) = bounds_of(&geo.parts);
    let pivots = geo
        .parts
        .iter()
        .flat_map(|p| p.pivot)
        .fold(0.0f32, |a, v| a.max(v.abs()));
    if lo
        .abs()
        .max_element()
        .max(hi.abs().max_element())
        .max(pivots)
        > AVATAR_REACH
    {
        return Err("reaches too far".into());
    }
    if geo
        .parts
        .iter()
        .flat_map(|p| &p.cuboids)
        .any(|c| c.inflate.abs() > AVATAR_INFLATE)
    {
        return Err("a box inflated too far".into());
    }
    if !clips.iter().all(|a| a.within(AVATAR_REACH, AVATAR_SCALE)) {
        return Err("a clip moves it too far".into());
    }
    Ok((geo.parts, geo.texture, clips))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(s: &mut Swing, pivot: Vec3, rest: Vec3, d: BoneMotion, secs: f32) {
        for _ in 0..(secs * 60.0) as u32 {
            s.step(pivot, rest, d, 1.0 / 60.0);
        }
    }

    fn part(name: &str, parent: Option<u32>, pivot: [f32; 3], boxes: &[([f32; 3], [f32; 3], [f32; 2], bool)]) -> Part {
        let cuboids = boxes
            .iter()
            .map(|&(origin, size, uv, mirror)| Cuboid { origin, size, uv, inflate: 0.0, mirror, faces: None })
            .collect();
        Part { name: name.into(), parent, pivot, role: 0, cuboids, rest: [0.0; 3], mesh: Vec::new() }
    }

    /// A model's skin and worn geometry, its roles assigned.
    fn geometry(mut parts: Vec<Part>) -> (Vec<GpuCuboid>, Vec<GpuCuboid>) {
        model::assign_roles(&mut parts);
        let worn_on = model::worn_targets(&parts);
        let (mut skin, mut worn, mut corners) = (Vec::new(), Vec::new(), Vec::new());
        add_geometry(&parts, &worn_on, &mut skin, &mut corners);
        add_worn_geometry(&parts, &worn_on, &mut worn, &mut corners);
        (skin, worn)
    }

    fn same(a: &GpuCuboid, b: &GpuCuboid) -> bool {
        bytemuck::bytes_of(a) == bytemuck::bytes_of(b)
    }

    #[test]
    fn armor_on_the_classic_humanoid_is_drawn_on_its_own_boxes() {
        // The SDK's `humanoid`: its unwrap is vanilla armor's.
        let limb = |uv: [f32; 2], mirror| ([-2.0, -10.0, -2.0], [4.0, 12.0, 4.0], uv, mirror);
        let leg = |mirror| ([-2.0, -12.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0], mirror);
        let (skin, worn) = geometry(vec![
            part("head", None, [0.0, 24.0, 0.0], &[([-4.0, 0.0, -4.0], [8.0; 3], [0.0, 0.0], false)]),
            part("body", None, [0.0, 24.0, 0.0], &[([-4.0, -12.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0], false)]),
            part("rightArm", None, [-6.0, 22.0, 0.0], &[limb([40.0, 16.0], true)]),
            part("leftArm", None, [6.0, 22.0, 0.0], &[limb([40.0, 16.0], false)]),
            part("rightLeg", None, [-2.0, 12.0, 0.0], &[leg(true)]),
            part("leftLeg", None, [2.0, 12.0, 0.0], &[leg(false)]),
        ]);
        assert_eq!(worn.len(), skin.len());
        for (i, (w, s)) in worn.iter().zip(&skin).enumerate() {
            assert!(same(w, s), "box {i} worn as it is");
        }
    }

    #[test]
    fn armor_fits_a_bigger_head_and_leaves_its_ears_and_tail() {
        let (skin, worn) = geometry(vec![
            part(
                "head",
                None,
                [0.0, 24.0, 0.0],
                &[([-5.0, 0.0, -6.0], [10.0, 10.0, 10.0], [0.0, 0.0], false), ([5.0, 8.0, -1.0], [2.0, 4.0, 1.0], [40.0, 0.0], false)],
            ),
            part("tail", None, [0.0, 12.0, 2.0], &[([-1.0, -1.0, 0.0], [2.0, 2.0, 8.0], [24.0, 32.0], false)]),
        ]);
        assert_eq!(worn.len(), 2, "a helmet and the tail's own box");
        let helmet = &worn[0];
        assert_eq!((helmet.origin, helmet.size), ([-5.0, 24.0, -6.0], [10.0; 3]), "on the head, the ears left out");
        let vanilla = Cuboid { origin: [0.0; 3], size: [8.0; 3], uv: [0.0, 0.0], inflate: 0.0, mirror: false, faces: None };
        assert_eq!(helmet.faces, vanilla.face_rects(), "painted from vanilla's helmet");
        assert_eq!((helmet.role, helmet.cover), (role::HEAD, 0));
        assert!(same(&worn[1], &skin[2]), "what has no armor piece is worn in its skin's layout");
    }

    #[test]
    fn armor_bones_are_worn_and_never_skin() {
        let (skin, worn) = geometry(vec![
            part("body", None, [0.0, 24.0, 0.0], &[([-4.0, -12.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0], false)]),
            part("head", None, [0.0, 24.0, 0.0], &[([-4.0, 0.0, -4.0], [8.0; 3], [0.0, 0.0], false)]),
            part("armor_head", Some(1), [0.0, 24.0, 0.0], &[([-5.0, 0.0, -5.0], [10.0, 9.0, 10.0], [0.0, 0.0], false)]),
            part("visor", Some(2), [0.0, 28.0, -5.0], &[([-4.0, -1.0, -1.0], [8.0, 2.0, 1.0], [32.0, 0.0], false)]),
        ]);
        assert_eq!(skin.len(), 2, "the body and the head only");
        assert_eq!(worn.len(), 3, "the body's piece and the helmet the file has, visor and all");
        for (w, bone) in worn[1..].iter().zip([2, 3]) {
            assert_eq!((w.bone, w.cover, w.role), (bone, 1, role::HEAD), "moves with its bone, worn when the head is covered");
        }
    }

    #[test]
    fn a_lantern_hangs_lags_and_settles() {
        let rest = Vec3::new(0.0, 0.0, -0.5);
        let mut s = Swing { tip: rest, last: rest };
        settle(&mut s, Vec3::ZERO, rest, BoneMotion::HANGING, 8.0);
        assert!((s.tip - Vec3::new(0.0, -0.5, 0.0)).length() < 0.02, "it hangs whichever way its bone was: {}", s.tip);
        // The hand jumps aside: the lamp stays behind, stays on its chain, swings back to rest.
        let pivot = Vec3::new(0.4, 0.0, 0.0);
        s.step(pivot, rest, BoneMotion::HANGING, 1.0 / 60.0);
        assert!(s.tip.x < pivot.x - 0.05, "it lags: {}", s.tip);
        assert!(((s.tip - pivot).length() - 0.5).abs() < 1e-3, "its chain doesn't stretch");
        settle(&mut s, pivot, rest, BoneMotion::HANGING, 8.0);
        assert!((s.tip - (pivot + Vec3::NEG_Y * 0.5)).length() < 0.02, "and hangs again: {}", s.tip);
    }

    #[test]
    fn a_long_frame_does_not_throw_it() {
        let rest = Vec3::NEG_Y * 0.5;
        let mut s = Swing { tip: rest, last: rest };
        for dt in [0.5, 0.0, 2.0, 1e-5] {
            s.step(Vec3::X, rest, BoneMotion::HANGING, dt);
            assert!(s.tip.is_finite() && ((s.tip - Vec3::X).length() - 0.5).abs() < 1e-3, "{}", s.tip);
        }
    }

    fn geo(cubes: usize, origin_x: i32) -> Vec<u8> {
        let cube = format!(r#"{{"origin":[{origin_x},0,0],"size":[1,1,1],"uv":[0,0]}}"#);
        let list = vec![cube; cubes].join(",");
        format!(r#"{{"minecraft:geometry":[{{"description":{{"texture_width":16,"texture_height":16}},"bones":[{{"name":"body","pivot":[0,0,0],"cubes":[{list}]}}]}}]}}"#)
            .into_bytes()
    }

    #[test]
    fn a_strangers_avatar_stays_within_limits() {
        let ok = AvatarData { geo: geo(4, 0), ..AvatarData::default() };
        assert!(checked_avatar(&ok).is_ok());
        let too_many = AvatarData { geo: geo(AVATAR_BOXES + 1, 0), ..AvatarData::default() };
        assert!(checked_avatar(&too_many).is_err());
        let too_far = AvatarData { geo: geo(1, 500), ..AvatarData::default() };
        assert!(checked_avatar(&too_far).is_err());
        let garbage = AvatarData { geo: b"\xff not json".to_vec(), ..AvatarData::default() };
        assert!(checked_avatar(&garbage).is_err());
        let flung = br#"{"animations":{"animation.a.idle":{"bones":{"body":{"position":[9000,0,0]}}}}}"#;
        let moves = AvatarData { geo: geo(1, 0), animations: flung.to_vec(), ..AvatarData::default() };
        assert!(checked_avatar(&moves).is_err());
    }
}

impl Models {
    pub fn new(gpu: &mut dyn Backend, assets: &Assets) -> Models {
        let mut m = Models {
            kinds: Vec::new(),
            item_kinds: FxHashMap::default(),
            files: FxHashMap::default(),
            stock: FxHashMap::default(),
            item_clips: Vec::new(),
            tables: (Vec::new(), Vec::new()),
            stale: false,
            carriers: FxHashSet::default(),
            new_skins: Vec::new(),
            swings: FxHashMap::default(),
            turned: FxHashMap::default(),
            located: Vec::new(),
            located_from: glam::DVec3::ZERO,
            frame: 0,
            displays: FxHashMap::default(),
            cuboids: gpu.create_buffer(16, usage::STORAGE, "model cuboids"),
            corners: gpu.create_buffer(16, usage::STORAGE, "model corners"),
            instances: Default::default(),
            bones: Default::default(),
            uploaded: None,
            clips: Vec::new(),
            queued: Vec::new(),
            rows: Vec::new(),
            scratch: Vec::new(),
            counts: Vec::new(),
            kept: FxHashMap::default(),
        };
        m.reload(gpu, assets);
        m
    }

    /// Reads every kind's model and animations again (files may have changed).
    pub fn reload(&mut self, gpu: &mut dyn Backend, assets: &Assets) {
        // Buffers may not be empty: there is always this one of each, unused.
        self.tables = (vec![GpuCuboid::default()], vec![GpuCorner::default()]);
        self.kinds.clear();
        self.files.clear();
        self.carriers.clear();
        self.swings.clear();
        self.kept.clear();
        self.new_skins.clear();
        let c = content();
        self.stock = STOCK_FILES
            .into_iter()
            .map(|n| (n, animations(assets, n).into_iter().map(Arc::new).collect()))
            .collect();

        // An item's own file: named by its `animations` component, else after the item.
        self.item_clips = c
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let said = file_named(ITEM, i as u32, comp::ANIMATIONS, &d.name);
                if let Some(file) = &said {
                    warn_no_clips(assets, &d.name, file);
                }
                let clips = animations(assets, said.as_deref().unwrap_or(&d.name));
                (!clips.is_empty()).then(|| (i as u32, clips.into_iter().map(Arc::new).collect()))
            })
            .collect();
        for k in 0..c.game.entities.len() as u32 {
            let (parts, texture, own) = load_kind(assets, k);
            let own: Vec<Arc<Animation>> = own.into_iter().map(Arc::new).collect();
            let at = self.push_with_stock(parts, texture, &own);
            let kind = &c.game.entities[k as usize];
            let file = file_named(ENTITY, k, comp::MODEL, &kind.name).unwrap_or_else(|| kind.name.clone());
            self.kinds[at as usize].live = live_bones(&file, &self.kinds[at as usize].parts, &motions_of(assets, &file));
        }
        // Items: where they sit in the hand, and those made of bones like an entity. Such a model
        // is posed by the holder's clips of the same names (a gun's slide in its `swing`).
        self.item_kinds.clear();
        self.displays.clear();
        let model_comp = c.components.id(comp::MODEL);
        for (i, d) in c.items.iter().enumerate().skip(1) {
            let Some((ns, path)) = d.name.split_once(':') else { continue };
            let default = format!("{ns}:item/{path}");
            let source = |name: &str| read(assets, name, "models", "json");
            if let Some(display) = block_model::load(&default, &source)
                .ok()
                .flatten()
                .and_then(|m| m.display)
            {
                self.displays.insert(i as u32, display);
            }
            let said = model_comp.and_then(|id| c.components.text(ITEM, i as u32, id));
            let file = said.map_or(default, |t| t.replace("{name}", path));
            let Some(kind) = self.load_file(assets, &file, Some(i as u32)) else {
                // A pattern for a whole tag may well name a file that isn't there: that item stays as it was.
                if said.is_some_and(|t| !t.contains("{name}")) {
                    log::error!("item {}: no model {file}", d.name);
                }
                continue;
            };
            self.item_kinds.insert(i as u32, kind);
        }
        for (holder, entity) in c.game.entities.iter().enumerate() {
            for (file, bone) in &entity.bone_models {
                let Some(kind) = self.load_file(assets, file, None) else {
                    log::error!("on a bone of {}: no model {file}", entity.name);
                    continue;
                };
                self.carry(holder as u32, kind, bone);
            }
        }

        {
            let mut i = ICONS.lock().unwrap();
            *i = Icons::default();
            i.items.extend(self.item_kinds.keys());
            i.items.extend(c.game.item_model_for_stack.keys());
        }
        self.stale = true;
        self.sync(gpu);
    }

    fn push_with_stock(&mut self, parts: Vec<Part>, texture: Option<[u32; 2]>, own: &[Arc<Animation>]) -> u32 {
        let stocked = self
            .stock
            .get(stock_file(&parts))
            .map_or(&[][..], Vec::as_slice);
        push_kind(&mut self.kinds, &mut self.tables, parts, texture, (own, stocked, &self.item_clips))
    }

    // Items that bring clips along have a model each: the same boxes, not the same clips.
    fn file_key<'a>(&self, name: &'a str, item: Option<u32>) -> std::borrow::Cow<'a, str> {
        match item.filter(|i| self.item_clips.iter().any(|t| t.0 == *i)) {
            Some(i) => format!("{name}#{i}").into(),
            None => name.into(),
        }
    }

    /// `name`'s model from its file, read once: `models/<name>.geo.json`, its clips, what its bones
    /// do. An item adds the clips it brings along.
    pub fn load_file(&mut self, assets: &Assets, name: &str, item: Option<u32>) -> Option<u32> {
        let key = self.file_key(name, item).into_owned();
        if let Some(&k) = self.files.get(&key) {
            return k;
        }
        let geo = read(assets, name, "models", "geo.json").and_then(|src| {
            model::load_geo(&src)
                .map_err(|e| log::error!("model {name}: {e}"))
                .ok()
        });
        let kind = geo.map(|g| {
            let mut own: Vec<Arc<Animation>> = animations(assets, name).into_iter().map(Arc::new).collect();
            let brought = item.and_then(|i| self.item_clips.iter().find(|t| t.0 == i));
            own.extend(brought.iter().flat_map(|t| t.1.iter().cloned()));
            let k = push_kind(&mut self.kinds, &mut self.tables, g.parts, g.texture, (&own, &[], &[]));
            let m = &mut self.kinds[k as usize];
            m.live = live_bones(name, &m.parts, &motions_of(assets, name));
            m.skin = Some(name.to_string());
            self.new_skins.push(name.to_string());
            self.stale = true;
            k
        });
        self.files.insert(key, kind);
        kind
    }

    /// `holder` always has `model` on its bone `bone`.
    fn carry(&mut self, holder: u32, model: u32, bone: &str) {
        let Some(at) = model::bone_named(&self.kinds[holder as usize].parts, bone) else {
            log::warn!("{} has no bone {bone} to carry a model on", content().game.entities[holder as usize].name);
            return;
        };
        self.bring_clips(holder, model);
        self.kinds[holder as usize].carries.push((model, at));
    }

    /// `model` is on `holder` from now on: what plays on the holder moves those of its bones it names.
    fn bring_clips(&mut self, holder: u32, model: u32) {
        if !self.carriers.insert((holder, model)) {
            return;
        }
        let mut clips: Vec<Arc<Animation>> = Vec::new();
        for b in &self.kinds[holder as usize].anims {
            if !clips.iter().any(|a| Arc::ptr_eq(a, &b.anim)) {
                clips.push(b.anim.clone());
            }
        }
        let m = &mut self.kinds[model as usize];
        for a in clips {
            if m.anims.iter().any(|b| Arc::ptr_eq(&b.anim, &a)) {
                continue;
            }
            if let Some(b) = Bound::carried(a.clone(), &m.parts) {
                m.brought.push(m.anims.len() as u32);
                m.anims.push(b);
            }
        }
    }

    /// A player's avatar as a model of its own, past the kinds: the same files are the one model.
    pub fn add_avatar(&mut self, f: &AvatarData) -> Result<u32, String> {
        let mut hash = rustc_hash::FxHasher::default();
        (&f.geo, &f.animations, &f.bones).hash(&mut hash);
        let key = format!("avatar:{:x}", hash.finish());
        if let Some(&Some(k)) = self.files.get(&key) {
            return Ok(k);
        }
        if self
            .files
            .keys()
            .filter(|k| k.starts_with("avatar:"))
            .count()
            >= AVATAR_KINDS
        {
            return Err("too many avatars".into());
        }
        let (parts, texture, clips) = checked_avatar(f)?;
        let clips: Vec<Arc<Animation>> = clips.into_iter().map(Arc::new).collect();
        let k = self.push_with_stock(parts, texture, &clips);
        let c = content();
        let mut said = model::load_bones(&String::from_utf8_lossy(&f.bones));
        said.truncate(AVATAR_BONES);
        let m = &mut self.kinds[k as usize];
        m.live = live_bones("of an avatar", &m.parts, &said);
        for (file, bone) in &c.game.entities[c.game.player_kind as usize].bone_models {
            if let (Some(Some(model)), true) =
                (self.files.get(file).copied(), model::bone_named(&self.kinds[k as usize].parts, bone).is_some())
            {
                self.carry(k, model, bone);
            }
        }
        self.files.insert(key, Some(k));
        self.stale = true;
        Ok(k)
    }

    /// New models' boxes and triangles to the GPU. Before anything is drawn this frame: lists
    /// recorded earlier name the old buffers.
    pub fn sync(&mut self, gpu: &mut dyn Backend) {
        if !std::mem::take(&mut self.stale) {
            return;
        }
        gpu.destroy_buffer(self.cuboids);
        gpu.destroy_buffer(self.corners);
        let (cuboids, corners) = &self.tables;
        self.cuboids = gpu.create_buffer((cuboids.len() * size_of::<GpuCuboid>()) as u64, usage::STORAGE, "model cuboids");
        gpu.write_buffer(self.cuboids, 0, bytemuck::cast_slice(cuboids));
        self.corners = gpu.create_buffer((corners.len() * size_of::<GpuCorner>()) as u64, usage::STORAGE, "model corners");
        gpu.write_buffer(self.corners, 0, bytemuck::cast_slice(corners));
    }

    /// Resolves a stack's model through the shared look, then the item's default.
    pub fn model_for(&mut self, assets: &Assets, stack: ItemStack) -> Option<u32> {
        if stack.is_empty() {
            return None;
        }
        match crate::stack::model(stack, false) {
            Some(file) => self
                .load_file(assets, file, Some(stack.item))
                .or_else(|| self.item_kinds.get(&stack.item).copied()),
            None => self.item_kinds.get(&stack.item).copied(),
        }
    }

    /// A model already loaded for a displayed stack.
    pub fn model_seen(&self, stack: ItemStack) -> Option<u32> {
        if stack.is_empty() {
            return None;
        }
        match crate::stack::model(stack, false) {
            Some(file) => self
                .files
                .get(self.file_key(file, Some(stack.item)).as_ref())
                .copied()
                .flatten()
                .or_else(|| self.item_kinds.get(&stack.item).copied()),
            None => self.item_kinds.get(&stack.item).copied(),
        }
    }

    pub fn prepare_worn(&mut self, assets: &Assets, holder: u32, stack: ItemStack) {
        let Some((file, bone)) = crate::stack::model(stack, true).and_then(|s| s.split_once('@')) else { return };
        let Some(model) = self.load_file(assets, file, None) else { return };
        if model::bone_named(&self.kinds[holder as usize].parts, bone).is_some() {
            self.bring_clips(holder, model);
        }
    }

    /// What `kind`, wearing `worn`, has on: (model, bone).
    pub fn carried_by(&self, kind: u32, worn: &[ItemStack], mut each: impl FnMut(u32, usize)) {
        let Some(m) = self.kinds.get(kind as usize) else { return };
        for &(model, bone) in &m.carries {
            each(model, bone);
        }
        for &stack in worn {
            let Some((file, bone)) = crate::stack::model(stack, true).and_then(|s| s.split_once('@')) else { continue };
            if let (Some(Some(model)), Some(b)) = (self.files.get(file), model::bone_named(&m.parts, bone)) {
                each(*model, b);
            }
        }
    }

    /// Moves a posed bone and all that hang on it by `by`.
    pub fn move_subtree(&mut self, kind: u32, first: usize, bone: usize, by: Mat4) {
        let Some(m) = self.kinds.get(kind as usize) else { return };
        for i in subtree(&m.parts, bone) {
            self.rows[first + i] = rows(by * self.bone(first + i));
        }
    }

    /// What moves bones of a model posed at `first` after its clips: dangling, and mods turning them.
    /// `moved`: how far the camera went since the last frame, which everything is relative to.
    /// `slot` tells apart two of a model on one entity: a lantern in each hand.
    pub fn move_live(&mut self, id: u64, kind: u32, slot: u32, first: usize, dt: f32, moved: Vec3) {
        let Some(m) = self.kinds.get(kind as usize) else { return };
        if !m.live.is_empty() {
            let key = (id, kind, slot);
            let live_len = m.live.len();
            let mut state = self
                .swings
                .remove(&key)
                .unwrap_or_else(|| Swings { list: Vec::new(), seen: 0 });
            let fresh = state.list.len() != live_len;
            if fresh {
                state.list = vec![Swing { tip: Vec3::ZERO, last: Vec3::ZERO }; live_len];
            }
            state.seen = self.frame;
            for k in 0..live_len {
                let m = &self.kinds[kind as usize];
                let (bone, reach, motion) = (m.live[k].bone, m.live[k].reach, m.live[k].motion);
                let pivot_px = Vec3::from_array(m.parts[bone].pivot);
                let at = self.bone(first + bone);
                let pivot = at.transform_point3(pivot_px);
                let rest = at.transform_vector3(reach);
                let length = rest.length().max(1e-3);
                let s = &mut state.list[k];
                if fresh || (s.tip - moved - pivot).length() > length * 4.0 + 4.0 {
                    *s = Swing { tip: pivot + rest, last: pivot + rest };
                } else {
                    s.tip -= moved;
                    s.last -= moved;
                }
                s.step(pivot, rest, motion, dt);
                let turn = Quat::from_rotation_arc(rest / length, (s.tip - pivot) / length);
                let by = Mat4::from_translation(pivot) * Mat4::from_quat(turn) * Mat4::from_translation(-pivot);
                for n in 0..self.kinds[kind as usize].live[k].under.len() {
                    let i = first + self.kinds[kind as usize].live[k].under[n];
                    self.rows[i] = rows(by * self.bone(i));
                }
            }
            self.swings.insert(key, state);
        }
        if self.turned.is_empty() {
            return;
        }
        let Some(list) = self.turned.get(&id).cloned() else { return };
        for (name, pose) in list {
            let m = &self.kinds[kind as usize];
            let Some(i) = model::bone_named(&m.parts, &name) else { continue };
            let pivot = Vec3::from_array(m.parts[i].pivot);
            let at = self.bone(first + i);
            let turn = Quat::from_euler(glam::EulerRot::ZYX, pose.turn.z, pose.turn.y, pose.turn.x);
            let local = Mat4::from_translation(pivot + pose.by)
                * Mat4::from_quat(turn)
                * Mat4::from_scale(pose.scale)
                * Mat4::from_translation(-pivot);
            self.move_subtree(kind, first, i, at * local * at.inverse());
        }
    }

    /// Who is posed where, for those asking what became of a bone.
    pub fn located_at(&mut self, id: u64, kind: u32, first: usize) {
        self.located.push((id, kind, first as u32));
    }

    pub fn frame_number(&self) -> u32 {
        self.frame
    }

    pub fn end_located(&mut self, from: glam::DVec3) {
        self.located_from = from;
        self.frame = self.frame.wrapping_add(1);
        self.turned.clear();
        if self.frame % 256 == 0 {
            let frame = self.frame;
            self.swings.retain(|_, s| frame.wrapping_sub(s.seen) < 256);
        }
    }

    /// Where a bone of `entity` was in the world, the last frame, in whatever it has on too.
    pub fn bone_position(&self, entity: u64, name: &str) -> Option<glam::DVec3> {
        self.located
            .iter()
            .filter(|l| l.0 == entity)
            .find_map(|&(_, kind, first)| {
                let m = self.kinds.get(kind as usize)?;
                let i = model::bone_named(&m.parts, name)?;
                let at = self.bone(first as usize + i);
                let p = at.transform_point3(Vec3::from_array(m.parts[i].pivot));
                Some(self.located_from + p.as_dvec3())
            })
    }

    /// The first box of what `entity` showed last frame (its model, what it holds and has on) that
    /// a ray from `from` along `dir` (the world, a unit vector) enters within `max`: that model,
    /// the bone and how far along.
    pub fn ray_bones(&self, entity: u64, from: glam::DVec3, dir: glam::DVec3, max: f64) -> Option<(u32, usize, f64)> {
        let mut best: Option<(u32, usize, f64)> = None;
        for &(_, kind, first) in self.located.iter().filter(|l| l.0 == entity) {
            let Some(m) = self.kinds.get(kind as usize) else { continue };
            for (i, p) in m
                .parts
                .iter()
                .enumerate()
                .filter(|(i, _)| m.worn_on[*i].is_none())
            {
                // In the bone's pixels the ray goes by the same steps: its own length stays the world's.
                let back = self.bone(first as usize + i).inverse();
                let o = back
                    .transform_point3((from - self.located_from).as_vec3())
                    .as_dvec3();
                let d = back.transform_vector3(dir.as_vec3()).as_dvec3();
                for cu in &p.cuboids {
                    let lo = Vec3::from_array(p.pivot) + Vec3::from_array(cu.origin);
                    let b = crate::physics::Aabb { min: lo.as_dvec3(), max: (lo + Vec3::from_array(cu.size)).as_dvec3() };
                    if let Some((t, _)) = crate::physics::ray_aabb(o, d, &b, max) {
                        if best.is_none_or(|b| t < b.2) {
                            best = Some((kind, i, t));
                        }
                    }
                }
            }
        }
        best
    }

    /// Whether `entity` was shown last frame: its bones are known.
    pub fn posed(&self, entity: u64) -> bool {
        self.located.iter().any(|l| l.0 == entity)
    }

    /// The bone the whole of `entity`'s own model goes with (its `body`, else its first), as last shown.
    pub fn main_bone(&self, entity: u64) -> Option<(u32, usize)> {
        let &(_, kind, _) = self.located.iter().find(|l| l.0 == entity)?;
        let parts = &self.kinds.get(kind as usize)?.parts;
        Some((kind, model::bone_named(parts, "body").unwrap_or(0)))
    }

    /// A point and a direction in the world as they were in bone `part` of `entity`'s model
    /// `kind` last frame, in its pixels.
    pub fn to_bone(&self, entity: u64, kind: u32, part: usize, p: glam::DVec3, d: glam::DVec3) -> Option<(Vec3, Vec3)> {
        let &(.., first) = self.located.iter().find(|l| l.0 == entity && l.1 == kind)?;
        let back = self.bone(first as usize + part).inverse();
        Some((back.transform_point3((p - self.located_from).as_vec3()), back.transform_vector3(d.as_vec3())))
    }

    /// Where a point and a direction in a bone's pixels were in the world last frame (`to_bone`
    /// the other way).
    pub fn from_bone(&self, entity: u64, kind: u32, part: usize, p: Vec3, d: Vec3) -> Option<(glam::DVec3, glam::DVec3)> {
        let &(.., first) = self.located.iter().find(|l| l.0 == entity && l.1 == kind)?;
        let m = self.bone(first as usize + part);
        Some((self.located_from + m.transform_point3(p).as_dvec3(), m.transform_vector3(d).as_dvec3().normalize_or_zero()))
    }

    pub fn destroy(&mut self, gpu: &mut dyn Backend) {
        for b in [self.cuboids, self.corners].into_iter().chain(
            self.instances
                .iter_mut()
                .chain(&mut self.bones)
                .flat_map(|v| v.drain(..))
                .map(|b| b.0),
        ) {
            gpu.destroy_buffer(b);
        }
    }

    pub fn begin(&mut self) {
        self.queued.clear();
        self.rows.clear();
        self.clips.clear();
        self.located.clear();
        self.uploaded = None;
    }

    /// Poses a model into `scratch` by clips and a look.
    fn pose_scratch(&mut self, kind: u32, root: Mat4, look: (f32, f32), clips: &[PlayClip]) -> bool {
        let Some(m) = self.kinds.get(kind as usize) else { return false };
        let playing: Vec<Playing> = playing_of(m, clips).collect();
        let motion = Motion { pitch: look.0, head_yaw: look.1, aim_root: None, clips: &playing };
        model::pose(&m.parts, root, &motion, &mut self.scratch);
        true
    }

    /// Where a model's bones would be with these clips, without keeping them.
    pub fn pose_only(&mut self, kind: u32, root: Mat4, clips: &[PlayClip]) -> Vec<Mat4> {
        if self.pose_scratch(kind, root, (0.0, 0.0), clips) { self.scratch.clone() } else { Vec::new() }
    }

    /// Poses a model by clips alone, looking ahead; where its bones start.
    pub fn pose_clips(&mut self, kind: u32, root: Mat4, clips: &[PlayClip]) -> u32 {
        let first = self.rows.len() as u32;
        if self.pose_scratch(kind, root, (0.0, 0.0), clips) {
            self.rows.extend(self.scratch.iter().map(|b| rows(*b)));
        }
        first
    }

    /// Where the eyes of a model are with these clips, looking straight ahead.
    pub fn eye_point(&mut self, kind: u32, root: Mat4, clips: &[PlayClip], eye: f32) -> Option<Vec3> {
        let (i, at) = eyes(self.kinds.get(kind as usize)?, eye)?;
        self.pose_scratch(kind, root, (0.0, 0.0), clips);
        Some(self.scratch[i].transform_point3(at))
    }

    /// Where a standing model's arms are seen from through its own eyes, in its pixels: above and
    /// behind the eyes, or the hands are under the view with a shoulder at its edge. Vanilla has
    /// its hand as far out for the same reason, with no shoulder to answer for.
    pub fn view_point(&self, kind: u32, eye: f32) -> Option<Vec3> {
        eyes(self.kinds.get(kind as usize)?, eye).map(|e| e.1 + Vec3::new(-3.0, 5.0, 10.0))
    }

    /// Poses a model by clips and a look and queues its layers; where its bones start.
    pub fn add_clips(
        &mut self,
        kind: u32,
        root: Mat4,
        look: (f32, f32),
        clips: &[PlayClip],
        layers: impl IntoIterator<Item = Instance>,
    ) -> u32 {
        let first = self.rows.len() as u32;
        if self.pose_scratch(kind, root, look, clips) {
            self.rows.extend(self.scratch.iter().map(|b| rows(*b)));
        }
        self.queue(kind, first, layers);
        first
    }

    /// How many bones are posed so far: where what comes next starts in `bone`.
    pub fn bones_so_far(&self) -> usize {
        self.rows.len()
    }

    /// A posed bone's transform.
    pub fn bone(&self, at: usize) -> Mat4 {
        let [a, b, c] = self.rows[at];
        Mat4::from_cols_array_2d(&[a, b, c, [0.0, 0.0, 0.0, 1.0]]).transpose()
    }

    /// The clips of model `to` that go with what plays on `holder` for the hand it is in, by
    /// name (a first person version counts as the action): same place in them.
    pub fn clips_along(&self, to: u32, holder: u32, clips: &[PlayClip], clock: f32, off: bool) -> Clips {
        let mut out = Clips::default();
        let (Some(m), Some(h)) = (self.kinds.get(to as usize), self.kinds.get(holder as usize)) else { return out };
        m.play(&mut out, m.act(Act::Idle), &Play { clock, looping: true, mirror: off, ..Play::default() });
        for c in clips.iter().filter(|c| !c.hand || c.mirror == off) {
            let Some(b) = h.anims.get(c.anim as usize) else { continue };
            let name = b
                .anim
                .name
                .strip_suffix(".first_person")
                .unwrap_or(&b.anim.name);
            // Its own idle plays above, on its own clock.
            let own = if name == "idle" { &[][..] } else { m.action(0, name) };
            for &i in own {
                let Some(own) = m.anims.get(i as usize) else { continue };
                out.push(PlayClip { anim: i, t: c.t / b.anim.length * own.anim.length, w: c.w, mirror: off, ..PlayClip::default() });
            }
            // A holder's own clip, not whoever else's of that name, on this one's bones.
            for &i in &m.brought {
                if m.anims
                    .get(i as usize)
                    .is_some_and(|own| Arc::ptr_eq(&own.anim, &b.anim))
                {
                    out.push(PlayClip { anim: i, t: c.t, w: c.w, mirror: off, ..PlayClip::default() });
                }
            }
        }
        out
    }

    /// Starts another batch over the bones posed so far.
    pub fn begin_batch(&mut self) {
        self.queued.clear();
    }

    /// Queues layers of a model whose bones start at `first`.
    pub fn queue(&mut self, kind: u32, first: u32, layers: impl IntoIterator<Item = Instance>) {
        let Some(m) = self.kinds.get(kind as usize) else { return };
        for mut inst in layers {
            let (cuboids, corners) = m.ranges(inst.worn != 0);
            (inst.bones, inst.cuboids, inst.cuboid_count, inst.vertices) = (first, cuboids.0, cuboids.1, corners.0);
            self.queued.push((kind, inst));
        }
    }

    /// Whether `id` has bones kept for this model to go by.
    pub fn has_kept(&self, id: u64, kind: u32) -> bool {
        !self.kept.is_empty() && self.kept.get(&id).is_some_and(|k| k.0 == kind)
    }

    /// Forgets the bones of those `alive` says are gone.
    pub fn retain_kept(&mut self, alive: impl Fn(u64) -> bool) {
        self.kept.retain(|id, _| alive(*id));
    }

    /// Poses many models at once, in parallel, straight into the bone table: per entity its kind,
    /// root, look, clips and layers. Nothing is allocated per entity (a thousand mobs are a
    /// thousand of each every frame).
    /// `own`: the one entity whose arms are in the view (`Motion::aim_root`).
    pub fn add_many(&mut self, entities: &[Posed], own: Option<(usize, Mat4)>) {
        let kinds = &self.kinds;
        let clips = &self.clips;
        let base = self.rows.len();
        let mut counts = std::mem::take(&mut self.counts);
        counts.clear();
        counts.extend(
            entities
                .iter()
                .map(|e| kinds.get(e.kind as usize).map_or(0, |m| m.parts.len())),
        );
        self.rows
            .resize(base + counts.iter().sum::<usize>(), [[0.0; 4]; 3]);
        let mut slices: Vec<&mut [[[f32; 4]; 3]]> = Vec::with_capacity(entities.len());
        let mut rest = &mut self.rows[base..];
        for &n in &counts {
            let (a, b) = rest.split_at_mut(n);
            slices.push(a);
            rest = b;
        }
        // Taken out of the map for the parallel part, so each is only one worker's, and back after.
        let mut taken: Vec<Option<(u32, Vec<Mat4>)>> = Vec::with_capacity(entities.len());
        taken.extend(entities.iter().map(|e| {
            if e.lod == PoseLod::Direct {
                return None;
            }
            Some(
                self.kept
                    .remove(&e.id)
                    .unwrap_or_else(|| (e.kind, Vec::new())),
            )
        }));
        entities
            .par_iter()
            .enumerate()
            .zip(slices)
            .zip(taken.par_iter_mut())
            .with_min_len(64)
            .for_each_init(
                || (Vec::new(), Vec::new()),
                |(scratch, list), (((i, e), out), keep)| {
                    let Some(m) = kinds.get(e.kind as usize) else { return };
                    if let Some((kept_kind, bones)) = keep {
                        if e.lod == PoseLod::Fresh || bones.len() != m.parts.len() {
                            list.clear();
                            list.extend(playing_of(m, &clips[e.clips.0 as usize..e.clips.1 as usize]));
                            model::pose(
                                &m.parts,
                                Mat4::IDENTITY,
                                &Motion { pitch: e.pitch, head_yaw: e.head_yaw, aim_root: None, clips: list },
                                bones,
                            );
                            *kept_kind = e.kind;
                        }
                        for (o, b) in out.iter_mut().zip(bones.iter()) {
                            *o = rows(e.root * *b);
                        }
                        return;
                    }
                    list.clear();
                    list.extend(playing_of(m, &clips[e.clips.0 as usize..e.clips.1 as usize]));
                    let aim_root = own.filter(|o| o.0 == i).map(|o| o.1);
                    model::pose(&m.parts, e.root, &Motion { pitch: e.pitch, head_yaw: e.head_yaw, aim_root, clips: list }, scratch);
                    for (o, b) in out.iter_mut().zip(scratch.iter()) {
                        *o = rows(*b);
                    }
                },
            );
        for (e, keep) in entities.iter().zip(taken) {
            if let Some(k) = keep {
                self.kept.insert(e.id, k);
            }
        }
        let mut first = base as u32;
        for (e, &n) in entities.iter().zip(&counts) {
            self.queue(e.kind, first, e.layers[..e.layer_count].iter().copied());
            first += n as u32;
        }
        self.counts = counts;
    }

    /// Draws what was queued since `begin`, one instanced draw per kind, with `pipe` and the
    /// view in `view`.
    pub fn draw(
        &mut self,
        gpu: &mut dyn Backend,
        list: &mut CommandList,
        frame: &FrameInfo,
        batch: Batch,
        pipe: PipelineId,
        view: BufferId,
    ) {
        if self.queued.is_empty() {
            return;
        }
        self.queued.sort_by_key(|q| (q.0, q.1.worn));
        let grow = crate::gpu_layout::frame_buffer;
        let ib = grow(
            gpu,
            &mut self.instances[batch as usize],
            frame.slot,
            self.queued.len() as u32,
            size_of::<Instance>() as u64,
            usage::STORAGE,
            "model instances",
        );
        let insts: Vec<Instance> = self.queued.iter().map(|q| q.1).collect();
        gpu.mapped(ib)[..insts.len() * size_of::<Instance>()].copy_from_slice(bytemuck::cast_slice(&insts));
        let bb = match self.uploaded {
            Some(b) => b,
            None => {
                let b = grow(
                    gpu,
                    &mut self.bones[bone_set(batch)],
                    frame.slot,
                    self.rows.len().max(1) as u32,
                    48,
                    usage::STORAGE,
                    "model bones",
                );
                gpu.mapped(b)[..self.rows.len() * 48].copy_from_slice(bytemuck::cast_slice(&self.rows));
                self.uploaded = Some(b);
                b
            }
        };
        list.pipeline(pipe);
        list.bind(crate::gpu_layout::bind::model::INSTANCES, ib);
        list.bind(crate::gpu_layout::bind::model::SCENE, view);
        list.bind(crate::gpu_layout::bind::model::CUBOIDS, self.cuboids);
        list.bind(crate::gpu_layout::bind::model::CORNERS, self.corners);
        list.bind(crate::gpu_layout::bind::model::BONES, bb);
        let mut i = 0;
        while i < self.queued.len() {
            let (kind, worn) = (self.queued[i].0, self.queued[i].1.worn);
            let end = i + self.queued[i..]
                .iter()
                .take_while(|q| (q.0, q.1.worn) == (kind, worn))
                .count();
            let (cuboids, corners) = self.kinds[kind as usize].ranges(worn != 0);
            let vertices = cuboids.1 * 36 + corners.1;
            list.push(Cmd::Draw { vertices, instances: (end - i) as u32, first_vertex: 0, first_instance: i as u32 });
            i = end;
        }
    }
}
