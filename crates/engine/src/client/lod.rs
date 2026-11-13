//! Distant terrain beyond the view: made on the GPU straight from the dimension's terrain at a
//! coarser step (`shaders/terrain.wgsl`'s `lod`), so it reaches as far as wanted for a few
//! kilobytes a tile. A tile is 32x32 cells of 2^level blocks; each cell is the column at its
//! middle: its top block, the one under it, its biome, and tree canopies over it. Where saved
//! cubes made a column's top other than the terrain's (a town, a crater), the server says so
//! (`server::lod_tops`) and that column is taken instead. What stands above the ground is drawn
//! as its own spans, so a building keeps its roof and its walls.
//! Tiles are picked by distance, nearer ones split into the four of the level below; one is
//! shown once made, its parent or children standing in until then. Where a column of real cubes
//! shows its ground the tiles' pixels are dropped (`fs_lod`, `Lod::mask`), so they fill in for
//! cubes not made or meshed yet (flying fast), and nowhere stand through them.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use glam::{DVec3, IVec2};
use rustc_hash::{FxHashMap, FxHashSet};
use weft_abi::{BlockModel, FACE_DIRS};

use crate::background;
use crate::config;
use crate::content::{Content, StateInfo, content};
use crate::gpu_layout::terrain::lod as lod_gpu;
use crate::gpu_layout::{CORNERS, face, lod_box, lod_face, lod_record};
use crate::server::lod_tops::{LodColumn, Span, shows};

/// Columns that differ from the terrain, as the server said.
pub type Columns = FxHashMap<IVec2, LodColumn>;

/// Cells a tile side.
pub const CELLS: i32 = 32;
/// Columns a tile is made from: its cells and one around them (for the walls at its edges).
pub const SAMPLES: usize = lod_gpu::SAMPLES as usize;
const SIDE: i32 = lod_gpu::SIDE as i32;
const GRID: usize = (CELLS + 2) as usize;
/// Words per sample and per tile coming back from the GPU.
const WORDS: usize = lod_gpu::WORDS as usize;
pub const LOD_WORDS_PER_TILE: usize = SAMPLES * WORDS;
pub const MAX_LEVEL: u32 = 5;
/// Blocks under the land from where distant terrain is left out.
const UNDERGROUND: i32 = 32;
/// Tiles asked for and not back yet, at most.
const IN_FLIGHT: usize = 256;
/// Quads a tile may hold. Past this the index buffer does not cover the draw.
const MAX_QUADS: usize = 48_000;
/// More model boxes than this become one box around all of them.
const MAX_MODEL_BOXES: usize = 6;
/// Time taken to dither between a parent tile and its four children.
const LOD_FADE: Duration = Duration::from_millis(250);

/// A tile: its level and place in the grid of its level's tiles.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TileKey {
    pub level: u32,
    pub x: i32,
    pub z: i32,
}

impl TileKey {
    /// Width in blocks.
    pub fn size(self) -> i32 {
        CELLS << self.level
    }

    /// Lowest block corner (x, z).
    pub fn origin(self) -> IVec2 {
        IVec2::new(self.x, self.z) * self.size()
    }

    fn parent(self) -> TileKey {
        TileKey { level: self.level + 1, x: self.x.div_euclid(2), z: self.z.div_euclid(2) }
    }

    fn children(self) -> [TileKey; 4] {
        std::array::from_fn(|i| TileKey { level: self.level - 1, x: self.x * 2 + (i as i32 & 1), z: self.z * 2 + (i as i32 >> 1) })
    }

    /// Nearest and farthest horizontal distance from `p` (x, z) to it.
    fn distance(self, p: glam::DVec2) -> (f64, f64) {
        let lo = self.origin().as_dvec2();
        let hi = lo + glam::DVec2::splat(self.size() as f64);
        let near = (lo - p).max(p - hi).max(glam::DVec2::ZERO).length();
        let far = (p - lo).abs().max((p - hi).abs()).length();
        (near, far)
    }
}

/// A tile for the GPU to make.
pub struct LodJob {
    pub dim: u32,
    pub seed: u64,
    pub key: TileKey,
    pub done: Sender<LodMesh>,
    /// What the server said of columns, taken over the terrain's where the tile samples them.
    pub columns: Arc<Columns>,
}

/// A made tile's quads (`mesh`), how many of each part and direction, and the heights they span.
pub struct LodMesh {
    pub dim: u32,
    pub seed: u64,
    pub key: TileKey,
    pub quads: Vec<[u32; 4]>,
    pub counts: Counts,
    pub y: (i32, i32),
}

/// How many quads a tile has, per part (`ALWAYS`, `BESIDE_CUBES`, `SURFACE`) and per direction
/// rank (`mesher::DIR_RANK`), in that order.
pub type Counts = [[u32; 6]; PARTS];
/// All a tile shows that is opaque.
pub const ALWAYS: usize = 0;
/// Skirts beside a neighbor as high: only seen where that neighbor's pixels give way to cubes.
pub const BESIDE_CUBES: usize = 1;
/// A see-through surface over the ground (a sea), drawn after everything opaque.
pub const SURFACE: usize = 2;
const PARTS: usize = 3;

/// What came back for `job`, meshed on a worker.
pub fn made(job: LodJob, mut words: Vec<u32>) {
    background().spawn(move || {
        if !job.columns.is_empty() {
            let c = content();
            let def = &c.dims[job.dim as usize];
            let (s, o) = (1 << job.key.level, job.key.origin());
            // Cells something stands in: the tree cover the terrain puts there is gone.
            let built: FxHashSet<(i32, i32)> = job
                .columns
                .iter()
                .filter(|(_, col)| !col.spans.is_empty())
                .map(|(p, _)| ((p.x - o.x).div_euclid(s), (p.y - o.y).div_euclid(s)))
                .collect();
            for k in 0..SAMPLES {
                let (i, j) = ((k as i32 % SIDE) - 1, (k as i32 / SIDE) - 1);
                let at = o + IVec2::new(i * s + s / 2, j * s + s / 2);
                let w = k * WORDS;
                let (y, top, under) = ((words[w] & 0xFFFF) as u16 as i16 as i32, words[w] >> 16, words[w + 1] >> 16);
                let ground = match job.columns.get(&at) {
                    // One saved column must not repaint a coarse cell: a patch of dirt the width
                    // of the cell. Up close a cell is a dug column's width.
                    Some(col) if s <= 2 && col.top != 0 => Some((col.y, col.top, col.under)),
                    _ if built.contains(&(i, j)) && top == under && !shows(c, top) => def
                        .column(job.seed, at.x, at.y)
                        .filter(|t| t.height < y && def.sea_level().is_none_or(|sea| t.height >= sea))
                        .map(|t| (t.height, def.surface_block(job.seed, at.x, at.y)))
                        .filter(|g| g.1 != 0)
                        .map(|(y, top)| (y, top, top)),
                    _ => None,
                };
                if let Some((y, top, under)) = ground {
                    words[w] = (y as u32 & 0xFFFF) | (top << 16);
                    words[w + 1] = (words[w + 1] & 0xFFFF) | (under << 16);
                }
            }
        }
        let (quads, counts, y) = mesh(content(), job.key, &words, &job.columns);
        let _ = job
            .done
            .send(LodMesh { dim: job.dim, seed: job.seed, key: job.key, quads, counts, y });
    });
}

/// A record of `world.wgsl`'s `vs_lod`: the `dir` face of a box at (x, y, z) in the tile, size
/// (sx, sy, sz) blocks, drawn with texture `tex`, tinted for `biome` when `tint`, corners in `ao`,
/// lit by block light `light` (`lod_tops::Span::light`) with how much of the sky's it lacks in
/// the top byte.
fn quad(dir: u32, x: i32, y: i32, z: i32, size: (i32, i32, i32), tex: u32, tint: u32, biome: u32, ao: u32, light: u32) -> [u32; 4] {
    let rgb = [0, 8, 16].map(|s| (light >> s) as u8);
    let sky = 15u8.saturating_sub((light >> 24) as u8);
    let mut r = lod_record(dir, [x, y, z], [size.0, size.1, size.2], tex, tint, biome, sky, rgb);
    r[3] |= (ao & 0xFF) << 24;
    r
}

fn corner_y(words: &[u32], gi: i32, gj: i32) -> i32 {
    let gi = gi.clamp(-1, CELLS);
    let gj = gj.clamp(-1, CELLS);
    let k = ((gj + 1) * SIDE + gi + 1) as usize * WORDS;
    (words[k] & 0xFFFF) as u16 as i16 as i32
}

fn sample_y(words: &[u32], gx: f32, gz: f32) -> i32 {
    let i0 = gx.floor().clamp(-1.0, (CELLS - 1) as f32) as i32;
    let j0 = gz.floor().clamp(-1.0, (CELLS - 1) as f32) as i32;
    let fx = gx - i0 as f32;
    let fz = gz - j0 as f32;
    let h = |x: i32, z: i32| corner_y(words, x, z) as f32;
    let x = |z: i32| h(i0, z) * (1.0 - fx) + h(i0 + 1, z) * fx;
    (x(j0) * (1.0 - fz) + x(j0 + 1) * fz).round() as i32
}

/// The tile's top faces and the walls down to lower neighbors: the top block for its own
/// height, then the block under it for the rest, lit as the column the cell is sampled at; then
/// what stands on the ground (`add_structures`). Sorted by part and direction (`Counts`).
pub fn mesh(c: &Content, key: TileKey, words: &[u32], columns: &Columns) -> (Vec<[u32; 4]>, Counts, (i32, i32)) {
    let s = 1 << key.level;
    let origin = key.origin();
    let at = |i: i32, j: i32| {
        let k = ((j + 1) * SIDE + i + 1) as usize * WORDS;
        let (a, b) = (words[k], words[k + 1]);
        let col = IVec2::new(origin.x + i * s + s / 2, origin.y + j * s + s / 2);
        (a >> 16, b & 0xFFFF, b >> 16, columns.get(&col).map_or(0, |c| c.light))
    };
    // What stands over a cell's ground: its top and block.
    let over = |i: i32, j: i32| {
        let w = words[((j + 1) * SIDE + i + 1) as usize * WORDS + 2];
        ((w & 0xFFFF) as u16 as i16 as i32, w >> 16)
    };
    // Under a surface (a sea) the sky's light on the ground after `depth` blocks of it, as in the
    // cubes.
    let sky_under = |surface: u32, depth: i32| {
        (0..depth.min(16)).fold(15, |v, _| crate::world::light::step(v, c.tight[surface as usize].opacity, true))
    };
    let light_of = |i: i32, j: i32, y: i32, light: u32| match over(i, j) {
        (top_y, surface) if surface != 0 => light | ((15 - sky_under(surface, top_y - y)) as u32) << 24,
        _ => light,
    };
    let mut h = height_grid(words);
    // Ground deeper than the sky's light reaches is as dark at any depth: it is laid as high as
    // that, a flat floor of few quads and no walls.
    let deepest = h.iter().flatten().copied().min().unwrap_or(0);
    for j in -1..=CELLS {
        for i in -1..=CELLS {
            let (top_y, surface) = over(i, j);
            if let Some(dark) = (surface != 0)
                .then(|| (1..=16).find(|&d| sky_under(surface, d) == 0))
                .flatten()
            {
                let y = &mut h[(i + 1) as usize][(j + 1) as usize];
                *y = (*y).max(top_y - dark);
            }
        }
    }
    let looks = super::looks::current();
    let ctm = super::ctm::current();
    // Far off a face is its first layer.
    let face_of = |state: u32, dir: usize| {
        let info = &c.states[state as usize];
        let tex = looks.drawn(state, info).textures[dir] & face::TEX_MASK;
        let first = super::mesher::face_layers(c, info, tex, dir)[0];
        (first.tex, first.tint as u32)
    };
    let mut faces = Vec::new();
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    for j in 0..CELLS {
        for i in 0..CELLS {
            let y = column(&h, i, j);
            let (top, biome, _, light) = at(i, j);
            let lit = light_of(i, j, y, light);
            let (tex, tint) = face_of(top, 3);
            faces.push(Face {
                dir: 3,
                at: [i * s, y, j * s],
                size: [s, 1, s],
                tex,
                tint,
                biome,
                ao: ao_bits(&h, i, j, 3, y),
                light: lit,
                part: ALWAYS,
            });
            (lo, hi) = (lo.min(y), hi.max(y + 1));
            // A sea's surface, unshaded as a liquid's.
            let (top_y, surface) = over(i, j);
            if surface != 0 && top_y > y {
                let (tex, tint) = face_of(surface, 3);
                let part = if c.states[surface as usize].layer == weft_abi::RenderLayer::Translucent { SURFACE } else { ALWAYS };
                faces.push(Face { dir: 3, at: [i * s, top_y, j * s], size: [s, 1, s], tex, tint, biome, ao: 0xFF, light, part });
                hi = hi.max(top_y + 1);
            }
        }
    }
    for j in 0..CELLS {
        for i in 0..CELLS {
            let (top, biome, under, light) = at(i, j);
            let y = column(&h, i, j);
            let light = light_of(i, j, y, light);
            for (dir, (di, dj)) in [(0usize, (-1, 0)), (1, (1, 0)), (4, (0, -1)), (5, (0, 1))] {
                // At the tile's edge the tile beside may be of another level, its ground not
                // where this one's sample says: the wall goes on down below it (a skirt), or
                // the sky shows through the crack.
                let (ni, nj) = (i + di, j + dj);
                let ny = if (-1..=CELLS).contains(&ni) && (-1..=CELLS).contains(&nj) { column(&h, ni, nj) } else { y.min(y - 1) - 2 * s };
                // Past a neighbor as high, real cubes may stand in for it lower than its sample
                // says (it is interpolated): a skirt down below that, or the sky shows through
                // between them.
                if ny > y {
                    continue;
                }
                let mut skirt = ny.min(y - 1) - s - 2;
                // Under a sea the wall goes on down to where the ground really is, the tile's
                // deepest for all (their rows stay alike): seen from the cubes' water at the edge
                // of the view, a look goes under the floor laid higher.
                if over(i, j).1 != 0 {
                    skirt = skirt.min(deepest - s - 2);
                }
                // Beside one as high inside the tile it is only ever seen where that one's
                // pixels give way to cubes.
                let part = if ny == y && (0..CELLS).contains(&ni) && (0..CELLS).contains(&nj) { BESIDE_CUBES } else { ALWAYS };
                let dir = dir as u32;
                if ny < y {
                    let (tex, tint) = face_of(top, dir as usize);
                    let ao = ao_bits(&h, i, j, dir as usize, y);
                    faces.push(Face { dir, at: [i * s, y, j * s], size: [s, 1, s], tex, tint, biome, ao, light, part });
                }
                let (tex, tint) = face_of(under, dir as usize);
                let top_y = if ny < y { y - 1 } else { y };
                let ao = ao_span(&h, i, j, dir as usize, top_y, (ny + 1).min(top_y));
                faces.push(Face { dir, at: [i * s, skirt + 1, j * s], size: [s, y - 1 - skirt, s], tex, tint, biome, ao, light, part });
                lo = lo.min(skirt + 1);
            }
        }
    }
    let record =
        |f: &Face| quad(f.dir, f.at[0], f.at[1], f.at[2], (f.size[0], f.size[1], f.size[2]), f.tex, f.tint, f.biome, f.ao, f.light);
    let faces = merged(faces);
    let mut parts: [Vec<[u32; 4]>; PARTS] = std::array::from_fn(|p| faces.iter().filter(|f| f.part == p).map(record).collect());
    add_structures(&mut parts[ALWAYS], &mut lo, &mut hi, c, key, words, columns, &h, &face_of, &ctm);
    let rank = |q: &[u32; 4]| super::mesher::DIR_RANK[((q[0] >> lod_face::DIR_SHIFT) & face::DIR_MASK) as usize] as usize;
    let mut counts = [[0; 6]; PARTS];
    for (part, n) in parts.iter_mut().zip(&mut counts) {
        part.sort_by_key(rank);
        for q in part.iter() {
            n[rank(q)] += 1;
        }
    }
    (parts.concat(), counts, (lo, hi))
}

/// A face of a tile before it is packed.
#[derive(Clone, Copy, PartialEq)]
struct Face {
    dir: u32,
    at: [i32; 3],
    size: [i32; 3],
    tex: u32,
    tint: u32,
    biome: u32,
    ao: u32,
    light: u32,
    /// Which of `Counts`' parts.
    part: usize,
}

/// `faces` with neighbors alike made one face: a flat stretch of ground is a few quads, a straight
/// cliff a few strips, not one a cell. Runs along a row first (walls facing x along z, the others
/// along x), then tops' runs alike side by side into rectangles; walls stop at strips, their
/// rows being of other heights. A face stretches only where its shading would (the corners at
/// both ends shaded alike).
fn merged(faces: Vec<Face>) -> Vec<Face> {
    let rows = merged_along(faces, |f| Some(if f.dir <= 1 { 2 } else { 0 }));
    merged_along(rows, |f| (f.dir == 2 || f.dir == 3).then_some(2))
}

/// Runs of faces alike but for where they are along the axis `along` gives (none: kept as they
/// are) made one face.
fn merged_along(mut faces: Vec<Face>, along: impl Fn(&Face) -> Option<usize>) -> Vec<Face> {
    // Alike but for where along the row: sorted together, in order along it.
    let key = |f: &Face| {
        let a = along(f);
        let mut pos = f.at;
        if let Some(a) = a {
            pos[a] = 0;
        }
        (f.dir, f.part, pos, f.size, f.tex, f.tint, f.biome, f.ao, f.light, a.map_or(0, |a| f.at[a]))
    };
    faces.sort_unstable_by_key(key);
    let most = 1 << crate::gpu_layout::lod_face::SIZE_BITS;
    let mut out: Vec<Face> = Vec::with_capacity(faces.len());
    for f in faces {
        if let Some(last) = out.last_mut()
            && let Some(a) = along(&f)
        {
            let (mut a_pos, mut f_pos) = (last.at, f.at);
            (a_pos[a], f_pos[a]) = (0, 0);
            let mut a_size = last.size;
            a_size[a] = f.size[a];
            let alike = Face { at: f_pos, ..f } == Face { at: a_pos, size: a_size, ..*last };
            if alike && last.at[a] + last.size[a] == f.at[a] && last.size[a] + f.size[a] <= most && stretches(f.dir, f.ao, a) {
                last.size[a] += f.size[a];
                continue;
            }
        }
        out.push(f);
    }
    out
}

/// Whether a face's shading holds when it is stretched along axis `along`: each corner shaded
/// like the one at the other end of the face.
fn stretches(dir: u32, ao: u32, along: usize) -> bool {
    let corners = &CORNERS[dir as usize];
    let shade = |k: usize| (ao >> (2 * k)) & 3;
    (0..4).all(|k| {
        let other = (0..4).find(|&m| m != k && (0..3).all(|ax| ax == along || corners[m][ax] == corners[k][ax]));
        other.is_none_or(|m| shade(m) == shade(k))
    })
}

fn height_grid(words: &[u32]) -> [[i32; GRID]; GRID] {
    let mut h = [[0i32; GRID]; GRID];
    for j in -1..=CELLS {
        for i in -1..=CELLS {
            let y = if i < CELLS && j < CELLS {
                sample_y(words, i as f32 + 0.5, j as f32 + 0.5)
            } else {
                let (i0, fx) = if i < CELLS { (i, 0.5) } else { (CELLS, 0.0) };
                let (j0, fz) = if j < CELLS { (j, 0.5) } else { (CELLS, 0.0) };
                let s = |x: i32, z: i32| corner_y(words, x, z) as f32;
                let a = s(i0, j0) * (1.0 - fx) + s((i0 + 1).min(CELLS), j0) * fx;
                let b = s(i0, (j0 + 1).min(CELLS)) * (1.0 - fx) + s((i0 + 1).min(CELLS), (j0 + 1).min(CELLS)) * fx;
                (a * (1.0 - fz) + b * fz).round() as i32
            };
            h[(i + 1) as usize][(j + 1) as usize] = y;
        }
    }
    h
}

fn column(h: &[[i32; GRID]; GRID], i: i32, j: i32) -> i32 {
    h[(i + 1).clamp(0, CELLS + 1) as usize][(j + 1).clamp(0, CELLS + 1) as usize]
}

fn ao_bits(h: &[[i32; GRID]; GRID], i: i32, j: i32, dir: usize, by: i32) -> u32 {
    let nrm = FACE_DIRS[dir];
    let axis = dir / 2;
    let (ta, tb) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let base = [i + nrm[0], by + nrm[1], j + nrm[2]];
    let mut bits = 0u32;
    for (k, corner) in CORNERS[dir].iter().enumerate() {
        let sa = corner[ta] * 2 - 1;
        let sb = corner[tb] * 2 - 1;
        let mut p1 = base;
        let mut p2 = base;
        let mut p3 = base;
        p1[ta] += sa;
        p2[tb] += sb;
        p3[ta] += sa;
        p3[tb] += sb;
        let solid = |p: [i32; 3]| column(h, p[0], p[2]) >= p[1];
        let (s1, s2, s3) = (solid(p1), solid(p2), solid(p3));
        let ao = if s1 && s2 { 0 } else { 3 - u32::from(s1) - u32::from(s2) - u32::from(s3) };
        bits |= ao << (2 * k);
    }
    bits
}

fn ao_span(h: &[[i32; GRID]; GRID], i: i32, j: i32, dir: usize, top: i32, bot: i32) -> u32 {
    let hi = ao_bits(h, i, j, dir, top);
    let lo = ao_bits(h, i, j, dir, bot);
    let mut bits = 0u32;
    for (k, c) in CORNERS[dir].iter().enumerate() {
        let src = if c[1] == 1 { hi } else { lo };
        bits |= ((src >> (2 * k)) & 3) << (2 * k);
    }
    bits
}

/// `lo..=hi` with the ranges in `cover` taken out.
fn exposed(lo: i32, hi: i32, cover: &[Span]) -> Vec<(i32, i32)> {
    let mut segs = vec![(lo, hi)];
    for span in cover {
        let (a, b) = (span.bottom, span.top);
        let mut next = Vec::new();
        for (s, e) in segs {
            if b < s || a > e {
                next.push((s, e));
            } else {
                if s < a {
                    next.push((s, a - 1));
                }
                if e > b {
                    next.push((b + 1, e));
                }
            }
        }
        segs = next;
    }
    segs
}

/// A model's boxes in sixteenths, or one box around them when there are many. Tilt is dropped:
/// the distant shape is the box as it was placed.
fn model_boxes(st: &StateInfo) -> Vec<([u32; 3], [u32; 3], [u32; 6])> {
    let mut raw: Vec<([u32; 3], [u32; 3], [u32; 6])> = st
        .boxes
        .iter()
        .map(|b| {
            let edge = |v: f32| (v * 16.0).round().clamp(0.0, 16.0) as u32;
            (b.min.map(edge), b.max.map(edge), b.textures)
        })
        .filter(|(mn, mx, _)| mn.iter().zip(mx).all(|(a, c)| a < c))
        .collect();
    if raw.len() > MAX_MODEL_BOXES {
        let mut mn = [16u32; 3];
        let mut mx = [0u32; 3];
        for (a, c, _) in &raw {
            for i in 0..3 {
                mn[i] = mn[i].min(a[i]);
                mx[i] = mx[i].max(c[i]);
            }
        }
        let tex = raw[0].2;
        raw = vec![(mn, mx, tex)];
    }
    raw
}

/// Saved spans in this tile, one box per run of block, at the column itself. The ground is left
/// where the tile has it, so a roof is not smoothed into the neighbors.
#[allow(clippy::too_many_arguments)]
fn add_structures(
    out: &mut Vec<[u32; 4]>,
    lo: &mut i32,
    hi: &mut i32,
    c: &Content,
    key: TileKey,
    words: &[u32],
    columns: &Columns,
    h: &[[i32; GRID]; GRID],
    face_of: &impl Fn(u32, usize) -> (u32, u32),
    ctm: &super::ctm::Ctm,
) {
    let s = 1 << key.level;
    let origin = key.origin();
    let (x0, z0) = (origin.x, origin.y);
    let (x1, z1) = (x0 + CELLS * s, z0 + CELLS * s);
    let mut here: Vec<IVec2> = columns
        .iter()
        .filter(|(p, col)| !col.spans.is_empty() && p.x >= x0 && p.x < x1 && p.y >= z0 && p.y < z1)
        .map(|(p, _)| *p)
        .collect();
    let (cx, cz) = (x0 + CELLS * s / 2, z0 + CELLS * s / 2);
    here.sort_by_key(|p| (p.x - cx).abs() + (p.y - cz).abs());
    let hides = |block: u32, by: &Span| super::mesher::face_hidden(c, ctm, block, by.block);
    for p in here {
        if out.len() >= MAX_QUADS {
            return;
        }
        let Some(col) = columns.get(&p) else { continue };
        let (i, j) = ((p.x - x0) / s, (p.y - z0) / s);
        let k = ((j + 1) * SIDE + i + 1) as usize * WORDS;
        let biome = words[k + 1] & 0xFFFF;
        let ground = column(h, i, j);
        // How high the ground beside covers this column's sides.
        let skirt_hi = |ni: i32, nj: i32| {
            let n = column(h, ni, nj);
            if ni == i && nj == j || n < ground {
                i32::MIN
            } else if n > ground {
                n
            } else {
                n - 1
            }
        };
        let (lx, lz) = (p.x - x0, p.y - z0);
        for span in &col.spans {
            let (bottom, span_hi, block, light) = (span.bottom, span.top, span.block, span.light);
            if c.states[block as usize].model == BlockModel::Boxes {
                for (mn, mx, textures) in model_boxes(&c.states[block as usize]) {
                    let height = span_hi - bottom + 1;
                    let extrude = mn[1] == 0 && mx[1] == 16 && height > 1;
                    let mut y = bottom;
                    while y <= span_hi {
                        let top = if extrude { span_hi } else { y };
                        for dir in 0..6 {
                            let at_edge = [mn[0] == 0, mx[0] == 16, mn[1] == 0, mx[1] == 16, mn[2] == 0, mx[2] == 16][dir];
                            let hidden = at_edge
                                && match dir {
                                    2 => {
                                        y <= ground + 1
                                            || col
                                                .spans
                                                .iter()
                                                .any(|s| s.bottom <= y - 1 && s.top >= y - 1 && hides(block, s))
                                    }
                                    3 => {
                                        (y <= ground && mx[1] == 16)
                                            || col
                                                .spans
                                                .iter()
                                                .any(|s| s.bottom <= top + 1 && s.top >= top + 1 && hides(block, s))
                                    }
                                    _ => {
                                        let (dx, dz) = [(-1, 0), (1, 0), (0, 0), (0, 0), (0, -1), (0, 1)][dir];
                                        let np = IVec2::new(p.x + dx, p.y + dz);
                                        let hide = skirt_hi((np.x - x0).div_euclid(s), (np.y - z0).div_euclid(s));
                                        let beside = columns.get(&np).map_or(&[][..], |n| n.spans.as_slice());
                                        (hide != i32::MIN && top <= hide)
                                            || beside
                                                .iter()
                                                .any(|s| s.bottom <= y && s.top >= top && hides(block, s))
                                    }
                                };
                            if hidden {
                                continue;
                            }
                            let (fb, tint) = face_of(block, dir);
                            let own = textures[dir] & face::TEX_MASK;
                            let tex = if own == 0 { fb } else { own };
                            let r = quad(
                                dir as u32,
                                lx,
                                y,
                                lz,
                                (1, if extrude { height } else { 1 }, 1),
                                tex,
                                tint,
                                biome,
                                ao_bits(h, i, j, dir, top),
                                light,
                            );
                            out.push(lod_box(r, mn, mx, extrude));
                        }
                        if extrude {
                            break;
                        }
                        y += 1;
                    }
                }
            } else {
                if !col
                    .spans
                    .iter()
                    .any(|s| s.bottom == span_hi + 1 && hides(block, s))
                {
                    let (tex, tint) = face_of(block, 3);
                    out.push(quad(3, lx, span_hi, lz, (1, 1, 1), tex, tint, biome, ao_bits(h, i, j, 3, span_hi), light));
                }
                let held = col
                    .spans
                    .iter()
                    .any(|s| s.top + 1 == bottom && hides(block, s));
                if !held && bottom > ground + 1 {
                    let (tex, tint) = face_of(block, 2);
                    out.push(quad(2, lx, bottom, lz, (1, 1, 1), tex, tint, biome, ao_bits(h, i, j, 2, bottom), light));
                }
                for (dir, dx, dz) in [(0usize, -1, 0), (1, 1, 0), (4, 0, -1), (5, 0, 1)] {
                    let (tex, tint) = face_of(block, dir);
                    let np = IVec2::new(p.x + dx, p.y + dz);
                    let beside = columns.get(&np).map_or(&[][..], |n| n.spans.as_slice());
                    let mut cover: Vec<Span> = beside.iter().copied().filter(|s| hides(block, s)).collect();
                    let n_top = skirt_hi((np.x - x0).div_euclid(s), (np.y - z0).div_euclid(s));
                    if n_top != i32::MIN {
                        cover.push(Span { bottom: n_top.saturating_sub(4096), top: n_top, block: 0, light: 0 });
                    }
                    for (s0, e0) in exposed(bottom, span_hi, &cover) {
                        let ao = ao_span(h, i, j, dir, e0 + 1, s0);
                        out.push(quad(dir as u32, lx, s0, lz, (1, e0 - s0 + 1, 1), tex, tint, biome, ao, light));
                    }
                }
            }
            *lo = (*lo).min(bottom);
            *hi = (*hi).max(span_hi + 1);
            if out.len() >= MAX_QUADS {
                return;
            }
        }
    }
}

/// The client's distant terrain: which tiles to show, asking for missing ones, taking made ones.
pub struct Lod {
    /// The land's height at the camera's column, kept while it stays in it.
    ground: Option<((u32, IVec2), i32)>,
    jobs: Sender<LodJob>,
    done: (Sender<LodMesh>, Receiver<LodMesh>),
    dim: u32,
    seed: u64,
    /// Whether the GPU makes this dimension's terrain (`terrain_gpu`); if not, there is no
    /// distant terrain here.
    able: bool,
    ready: FxHashSet<TileKey>,
    pending: FxHashSet<TileKey>,
    /// Tiles wanted, nearest first, and where the camera was then.
    wanted: Vec<TileKey>,
    wanted_at: Option<(glam::DVec2, i32, i32)>,
    /// A bit per cube column around the camera's (`MASK_MARGIN` past the view, addressed
    /// modulo the window's width so it does not move with the camera): its cubes show its
    /// ground. Its half width and center, and whether it changed since uploaded.
    mask: Vec<u32>,
    half: i32,
    center: IVec2,
    mask_changed: bool,
    /// What the server said of columns of `columns_dim`.
    columns: Arc<Columns>,
    columns_dim: u32,
    /// Made tiles to make again for columns that changed, shown as they are till then.
    stale: FxHashSet<TileKey>,
    /// Parent tile and transition start; true means parent -> children.
    transitions: FxHashMap<TileKey, (bool, Instant)>,
}

/// Columns the mask reaches past the view radius (cubes are kept that far).
const MASK_MARGIN: i32 = 3;

impl Lod {
    pub fn new(jobs: Sender<LodJob>) -> Lod {
        Lod {
            ground: None,
            jobs,
            done: unbounded(),
            dim: u32::MAX,
            seed: 0,
            able: false,
            ready: FxHashSet::default(),
            pending: FxHashSet::default(),
            wanted: Vec::new(),
            wanted_at: None,
            mask: Vec::new(),
            half: 0,
            center: IVec2::ZERO,
            mask_changed: false,
            columns: Arc::default(),
            columns_dim: u32::MAX,
            stale: FxHashSet::default(),
            transitions: FxHashMap::default(),
        }
    }

    /// `ToClient::LodColumns`: tiles with those columns are made again.
    pub fn set_columns(&mut self, dim: u32, all: bool, cols: &[LodColumn]) {
        if all || dim != self.columns_dim {
            self.columns = Arc::default();
            self.columns_dim = dim;
            // One still being made has the columns from before this, so it is made again too.
            self.stale.extend(self.ready.iter().copied());
            self.stale.extend(self.pending.iter().copied());
        }
        let map = Arc::make_mut(&mut self.columns);
        for col in cols {
            let (x, z) = (col.x, col.z);
            let p = IVec2::new(x, z);
            if col.top == u32::MAX {
                map.remove(&p);
            } else {
                map.insert(p, col.clone());
            }
            if !all {
                // Every level's tile with it, and the ones beside that sample it for their walls.
                for level in 0..=MAX_LEVEL {
                    let size = CELLS << level;
                    for dz in -1..=1 {
                        for dx in -1..=1 {
                            let key = TileKey {
                                level,
                                x: (x + dx * (size / CELLS)).div_euclid(size),
                                z: (z + dz * (size / CELLS)).div_euclid(size),
                            };
                            if self.ready.contains(&key) || self.pending.contains(&key) {
                                self.stale.insert(key);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn columns_len(&self) -> usize {
        self.columns.len()
    }

    /// Tiles asked for and not back yet.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Distance in blocks it reaches (`core:lod_distance` cubes), 0 when off.
    pub fn distance() -> i32 {
        let c = content();
        c.game
            .config_names
            .get("core:lod_distance")
            .map_or(0, |&id| config::get(id) as i32)
            * 16
    }

    /// Picks, asks for and hands the renderer this frame's tiles. `view`: the view radius in
    /// cubes (inside it the real cubes are drawn).
    pub fn update(
        &mut self,
        gpu: &mut dyn weft_render::Backend,
        render: &mut super::world_render::WorldRenderer,
        world: &mut super::world::ClientWorld,
        textures: &super::textures::GameTextures,
        seed: u64,
        cam: DVec3,
        view: i32,
    ) {
        let c = content();
        let dim = world.dim;
        self.update_mask(gpu, render, world, seed, cam, view);
        let reach = Self::distance();
        if (dim, seed) != (self.dim, self.seed) || reach == 0 {
            for key in self.ready.drain() {
                render.remove_lod(key);
            }
            self.pending.clear();
            self.wanted.clear();
            self.wanted_at = None;
            self.transitions.clear();
        }
        if (dim, seed) != (self.dim, self.seed) {
            (self.dim, self.seed) = (dim, seed);
            self.able = super::terrain_gpu::describe_lod(c, dim, seed).is_some();
        }
        // Tiles stand in for cubes not shown yet; the inner half of the view is shown long
        // before the edge, where flying outruns the cubes.
        let mut hide = (view / 2 * 16).max(0);
        // High up, the view's cubes don't reach the ground: tiles have to fill that in too (where
        // cubes do show, the mask leaves them out).
        if self
            .land_height(c, cam)
            .is_some_and(|land| cam.y > (land + view * 8) as f64)
        {
            hide = 0;
        }
        while let Ok(m) = self.done.1.try_recv() {
            if (m.dim, m.seed) != (self.dim, self.seed) || !self.pending.remove(&m.key) {
                continue;
            }
            render.upload_lod(gpu, textures, m.key, &m.quads, m.counts, m.y);
            self.ready.insert(m.key);
        }
        if reach == 0 || !self.able {
            render.lod_draw.clear();
            return;
        }
        let p = glam::DVec2::new(cam.x, cam.z);
        if self
            .wanted_at
            .is_none_or(|(at, h, r)| (at - p).length() > 16.0 || h != hide || r != reach)
        {
            self.wanted_at = Some((p, hide, reach));
            self.pick(p, hide as f64, reach as f64);
        }
        // Find ready parent/children pairs.
        let wanted: FxHashSet<TileKey> = self.wanted.iter().copied().collect();
        let mut candidates = FxHashMap::default();
        for &key in &self.wanted {
            if key.level > 0
                && self.ready.contains(&key)
                && key
                    .children()
                    .iter()
                    .all(|child| self.ready.contains(child) && !wanted.contains(child))
            {
                candidates.insert(key, false);
            }
            if key.level < MAX_LEVEL {
                let parent = key.parent();
                if self.ready.contains(&parent)
                    && parent
                        .children()
                        .iter()
                        .all(|child| wanted.contains(child) && self.ready.contains(child))
                {
                    candidates.insert(parent, true);
                }
            }
        }

        self.transitions.retain(|key, (to_fine, _)| {
            candidates
                .get(key)
                .is_some_and(|candidate| *candidate == *to_fine)
        });
        let now = Instant::now();
        for (&key, &to_fine) in &candidates {
            self.transitions.entry(key).or_insert((to_fine, now));
        }

        let mut active = FxHashMap::default();
        for (&key, &(to_fine, start)) in &self.transitions {
            let t = (now - start).as_secs_f32() / LOD_FADE.as_secs_f32();
            if t < 1.0 {
                active.insert(key, (to_fine, t.clamp(0.0, 1.0)));
            }
        }

        let mut shown = FxHashMap::default();
        let mut keep = FxHashSet::default();
        let mut transitioning = FxHashSet::default();
        for (&parent, &(to_fine, t)) in &active {
            let children = parent.children();
            keep.insert(parent);
            keep.extend(children);
            transitioning.insert(parent);
            transitioning.extend(children);
            if to_fine {
                shown.insert(parent, t - 1.0);
                for child in children {
                    shown.insert(child, t);
                }
            } else {
                shown.insert(parent, t);
                for child in children {
                    shown.insert(child, t - 1.0);
                }
            }
        }

        for &key in &self.wanted {
            if transitioning.contains(&key) {
                keep.insert(key);
                continue;
            }
            keep.insert(key);
            if self.ready.contains(&key) {
                shown.insert(key, 2.0);
                continue;
            }
            if key.level > 0 && key.children().iter().all(|k| self.ready.contains(k)) {
                for child in key.children() {
                    shown.insert(child, 2.0);
                    keep.insert(child);
                }
                continue;
            }
            let mut a = key;
            while a.level < MAX_LEVEL {
                a = a.parent();
                if self.ready.contains(&a) {
                    if !transitioning.contains(&a) {
                        shown.insert(a, 2.0);
                    }
                    keep.insert(a);
                    break;
                }
            }
        }
        render.lod_draw.clear();
        // It is a shell over the land with nothing under it: from deep below, its walls would hang
        // in the sky. Only rock is that way anyway.
        if !self.deep_underground(c, cam) {
            render.lod_draw.extend(shown);
        }
        let gone: Vec<TileKey> = self
            .ready
            .iter()
            .copied()
            .filter(|k| !keep.contains(k))
            .collect();
        for key in gone {
            self.ready.remove(&key);
            render.remove_lod(key);
        }
        self.request();
    }

    /// Well under the land at the camera's column, so its sky can't be seen. Near the surface a cave
    /// mouth or a ravine still looks out over the distance.
    fn deep_underground(&mut self, c: &Content, cam: DVec3) -> bool {
        self.land_height(c, cam)
            .is_some_and(|land| cam.y < (land - UNDERGROUND) as f64)
    }

    /// How high the land is at the camera's column.
    fn land_height(&mut self, c: &Content, cam: DVec3) -> Option<i32> {
        let at = IVec2::new(cam.x.floor() as i32, cam.z.floor() as i32);
        if self.ground.is_none_or(|g| g.0 != (self.dim, at)) {
            let height = c.dims[self.dim as usize]
                .column(self.seed, at.x, at.y)
                .map_or(i32::MIN, |col| col.height);
            self.ground = Some(((self.dim, at), height));
        }
        self.ground.map(|g| g.1).filter(|&h| h != i32::MIN)
    }

    /// Keeps the mask of columns shown by cubes current: columns that changed, and the ones
    /// coming into the window when the camera moved to another column. A bit per column for
    /// its ground (seen through what is clear), then a bit per column for its top (a sea's
    /// surface): a sea with its floor not drawn yet shows the tile's floor under the cubes' water.
    fn update_mask(
        &mut self,
        gpu: &mut dyn weft_render::Backend,
        render: &mut super::world_render::WorldRenderer,
        world: &mut super::world::ClientWorld,
        seed: u64,
        cam: DVec3,
        view: i32,
    ) {
        let half = view + MASK_MARGIN;
        let w = 2 * half + 1;
        let center = IVec2::new((cam.x / 16.0).floor() as i32, (cam.z / 16.0).floor() as i32);
        let slot = |col: IVec2| (col.y.rem_euclid(w) * w + col.x.rem_euclid(w)) as usize;
        let words = ((w * w) as usize).div_ceil(32);
        let changed = world.take_changed_columns();
        let world = &*world;
        let set = |mask: &mut Vec<u32>, col: IVec2| {
            let i = slot(col);
            let bit = 1u32 << (i % 32);
            let mut changed = false;
            for (part, through) in [(0, true), (1, false)] {
                let word = part * words + i / 32;
                let old = mask[word];
                mask[word] = if world.column_shown(col, seed, through) { old | bit } else { old & !bit };
                changed |= old != mask[word];
            }
            changed
        };
        let inside = |col: IVec2, at: IVec2| (col - at).abs().max_element() <= half;
        if half != self.half || self.mask.is_empty() {
            self.half = half;
            self.mask = vec![0; 2 * words];
            self.center = center;
            for z in -half..=half {
                for x in -half..=half {
                    let col = center + IVec2::new(x, z);
                    set(&mut self.mask, col);
                }
            }
            self.mask_changed = true;
        } else if center != self.center {
            let old = self.center;
            self.center = center;
            for z in -half..=half {
                for x in -half..=half {
                    let col = center + IVec2::new(x, z);
                    if !inside(col, old) {
                        self.mask_changed |= set(&mut self.mask, col);
                    }
                }
            }
        }
        for col in changed {
            if inside(col, center) {
                self.mask_changed |= set(&mut self.mask, col);
            }
        }
        render.lod_half = half as u32;
        if self.mask_changed {
            self.mask_changed = false;
            render.set_lod_mask(gpu, &self.mask);
        }
    }

    /// The tiles to show: level 5 tiles within `reach`, split while near, leaving out what
    /// lies inside `hide` (where the view's cubes are always shown).
    fn pick(&mut self, p: glam::DVec2, hide: f64, reach: f64) {
        // A tile splits into the level below while nearer than this many of its widths
        // (`core:lod_detail`): a cell stays about the same size on screen, at 4 a 32-block cell
        // at 4096 blocks. Half of it has a fourth of the tiles, what a phone's GPU can draw.
        let split = config::by_name("core:lod_detail").map_or(4.0, config::get);
        let top = CELLS << MAX_LEVEL;
        let (lo, hi) = (((p - reach) / top as f64).floor().as_ivec2(), ((p + reach) / top as f64).floor().as_ivec2());
        let mut out = Vec::new();
        let mut stack: Vec<TileKey> = Vec::new();
        for z in lo.y..=hi.y {
            for x in lo.x..=hi.x {
                stack.push(TileKey { level: MAX_LEVEL, x, z });
            }
        }
        while let Some(key) = stack.pop() {
            let (near, far) = key.distance(p);
            if near > reach || far < hide {
                continue;
            }
            if key.level > 1 && near < split * key.size() as f64 {
                stack.extend(key.children());
            } else {
                out.push((near, key));
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.wanted = out.into_iter().map(|t| t.1).collect();
    }

    /// Asks for the nearest wanted tiles not made yet, or made before columns changed.
    fn request(&mut self) {
        let columns = if self.columns_dim == self.dim { self.columns.clone() } else { Arc::default() };
        for &key in &self.wanted {
            if self.pending.len() >= IN_FLIGHT {
                break;
            }
            // Leave the mark: the one in flight still has the old columns.
            if self.pending.contains(&key) {
                continue;
            }
            let again = self.stale.remove(&key);
            if self.ready.contains(&key) && !again {
                continue;
            }
            self.pending.insert(key);
            let _ = self
                .jobs
                .send(LodJob { dim: self.dim, seed: self.seed, key, done: self.done.0.clone(), columns: columns.clone() });
        }
        self.stale
            .retain(|k| self.ready.contains(k) || self.pending.contains(k));
    }
}
