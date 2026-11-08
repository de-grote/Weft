//! What distant terrain needs of the cubes that are saved: the rest generates the same again, so
//! the client's tiles take it from the terrain formula, and only columns whose saved cubes make
//! them differ from it (a town, a crater, a tower) are kept here and sent. Per saved cube, the
//! runs of each column's blocks with the block light by them; a column's ground then comes from
//! those and the formula for the cubes of it that aren't saved, and what stands above the ground
//! is sent as spans.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use glam::{IVec2, IVec3};
use rustc_hash::FxHashMap;

use weft_abi::BlockModel;
use weft_abi::block_flags as bf;

use crate::content::Content;
use crate::world::Cube;
use crate::world::cube::index;

/// One block's vertical run, inclusive, and the block light by it (`abi::light` without the sky,
/// a byte a channel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub bottom: i32,
    pub top: i32,
    pub block: u32,
    pub light: u32,
}

/// A column that differs from the terrain: its ground's height, top block, the one under it and
/// the light on it, and what stands above the ground. `top` `u32::MAX`: as the terrain says
/// again; 0: the terrain's ground, only spans or light of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LodColumn {
    pub x: i32,
    pub z: i32,
    pub y: i32,
    pub top: u32,
    pub under: u32,
    pub light: u32,
    pub spans: Vec<Span>,
}

/// The cubes beside one, -x, +x, -z, +z, as far as they are loaded: the light at its sides.
pub type Around = [Option<Arc<Cube>>; 4];

/// A saved cube's columns: the runs that make each, low to high, local to the cube (0..15).
#[derive(Clone, PartialEq, Eq)]
struct CubeTops {
    /// `at[i]..at[i + 1]` of `runs` is column `i`.
    at: Box<[u16; 257]>,
    runs: Vec<Span>,
}

impl CubeTops {
    fn column(&self, i: usize) -> &[Span] {
        &self.runs[self.at[i] as usize..self.at[i + 1] as usize]
    }
}

/// Runs kept per column of one cube, the highest. A wall is one run; the rest is trim and a roof.
const MAX_RUNS: usize = 6;
/// Spans sent for one column, the highest of them.
const MAX_SPANS: usize = 8;
const MAGIC: &[u8] = b"LOD4";

/// A dimension's saved cubes' runs, by cube column then cube height.
#[derive(Default)]
pub struct DimTops {
    cubes: FxHashMap<IVec2, BTreeMap<i32, CubeTops>>,
    /// Columns that differ from the terrain, as last worked out.
    pub columns: FxHashMap<IVec2, LodColumn>,
    /// Columns that changed since `take_changed`.
    changed: Vec<IVec2>,
    dirty: bool,
}

/// Solid and opaque, or a liquid: the ground from afar. A window or a stair is not.
pub fn shows(c: &Content, s: u32) -> bool {
    let Some(st) = c.states.get(s as usize) else { return false };
    s != 0 && (st.liquid.is_some() || (st.flags & bf::SOLID != 0 && st.flags & bf::OPAQUE != 0))
}

/// A cube or a model box the distance can draw. A cross, a wire, an empty model, or a block a
/// placement replaces is not.
fn drawn(c: &Content, s: u32) -> bool {
    let Some(st) = c.states.get(s as usize) else { return false };
    if st.liquid.is_some() {
        return true;
    }
    s != 0
        && st.flags & bf::REPLACEABLE == 0
        && match st.model {
            BlockModel::Cube => true,
            BlockModel::Boxes => !st.boxes.is_empty(),
            _ => false,
        }
}

/// Each channel the brighter of the two.
fn brighter(a: u32, b: u32) -> u32 {
    (0..24)
        .step_by(8)
        .fold(0, |l, s| l | ((a >> s) & 0xFF).max((b >> s) & 0xFF) << s)
}

/// The block light by the run from `lo` to `hi` of column (`x`, `z`): of its cells and the ones
/// beside them, in the cubes around too.
fn light_by(cube: &Cube, around: &Around, x: usize, z: usize, lo: usize, hi: usize) -> u32 {
    let at = |x: i32, y: usize, z: i32| {
        let (cube, x, z) = match (x, z) {
            (-1, _) => (around[0].as_deref(), 15, z),
            (16, _) => (around[1].as_deref(), 0, z),
            (_, -1) => (around[2].as_deref(), x, 15),
            (_, 16) => (around[3].as_deref(), x, 0),
            _ => (Some(cube), x, z),
        };
        cube.map_or(0, |c| c.light(index(x as usize, y, z as usize)) >> 8)
    };
    let mut l = 0;
    for y in lo.saturating_sub(1)..=(hi + 1).min(15) {
        for (dx, dz) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
            l = brighter(l, at(x as i32 + dx, y, z as i32 + dz));
        }
    }
    l
}

fn tops_of(c: &Content, cube: &Cube, around: &Around) -> CubeTops {
    let mut at = Box::new([0u16; 257]);
    let mut runs = Vec::new();
    for z in 0..16 {
        for x in 0..16 {
            at[z * 16 + x] = runs.len() as u16;
            let mut column: Vec<Span> = Vec::new();
            let mut y = 0;
            while y < 16 {
                let s = cube.get(index(x, y, z));
                if !drawn(c, s) {
                    y += 1;
                    continue;
                }
                let lo = y;
                while y < 16 && cube.get(index(x, y, z)) == s {
                    y += 1;
                }
                column.push(Span { bottom: lo as i32, top: y as i32 - 1, block: s, light: light_by(cube, around, x, z, lo, y - 1) });
            }
            runs.extend_from_slice(&column[column.len().saturating_sub(MAX_RUNS)..]);
        }
    }
    at[256] = runs.len() as u16;
    CubeTops { at, runs }
}

impl DimTops {
    /// A cube was saved as `cube`.
    pub fn saved(&mut self, c: &Content, dim: u32, seed: u64, pos: IVec3, cube: &Cube, around: &Around) {
        let tops = tops_of(c, cube, around);
        let col = IVec2::new(pos.x, pos.z);
        let list = self.cubes.entry(col).or_default();
        if list.get(&pos.y).is_some_and(|t| *t == tops) {
            return;
        }
        list.insert(pos.y, tops);
        self.dirty = true;
        self.refresh(c, dim, seed, col);
    }

    /// Works out the 256 columns of cube column `col` again.
    fn refresh(&mut self, c: &Content, dim: u32, seed: u64, col: IVec2) {
        let def = &c.dims[dim as usize];
        let Some(list) = self.cubes.get(&col) else { return };
        let sea = def.sea_level();
        for i in 0..256 {
            let (x, z) = (col.x * 16 + (i % 16) as i32, col.y * 16 + (i / 16) as i32);
            let Some(t) = def.column(seed, x, z) else { continue };
            // As distant terrain shows it untouched: the ground, or the sea over it.
            let (fy, ftop) = match sea {
                Some(s) if t.height < s => (s, u32::MAX),
                _ => (t.height, def.surface_block(seed, x, z)),
            };
            let ground = t.height.div_euclid(16);
            let lowest = list.keys().next().copied().unwrap_or(ground);
            let highest = list
                .keys()
                .next_back()
                .copied()
                .unwrap_or(ground)
                .max(ground);
            // What rises above the ground, from the ground up; the rest stays the ground.
            let mut spans: Vec<Span> = Vec::new();
            for (&cy, tops) in list.range(lowest..=highest) {
                for r in tops.column(i) {
                    let (bottom, top) = (cy * 16 + r.bottom, cy * 16 + r.top);
                    match spans.last_mut() {
                        Some(prev) if prev.top + 1 == bottom && prev.block == r.block => {
                            (prev.top, prev.light) = (top, brighter(prev.light, r.light));
                        }
                        _ => spans.push(Span { bottom, top, ..*r }),
                    }
                }
            }
            spans.retain(|s| s.top > fy);
            for s in &mut spans {
                s.bottom = s.bottom.max(fy);
            }
            spans.drain(..spans.len().saturating_sub(MAX_SPANS));
            // The ground under them: from the highest saved cube down, what shows in it; where
            // one isn't saved above the terrain's ground, air; at or under it, the terrain.
            let limit = spans.first().map_or(i32::MAX, |s| s.bottom - 1);
            let mut here = None;
            for cy in (lowest.min(ground) - 1..=highest).rev() {
                match list.get(&cy) {
                    Some(tops) => {
                        let runs = tops.column(i);
                        let found = (0..runs.len())
                            .rev()
                            .find(|&k| shows(c, runs[k].block) && cy * 16 + runs[k].bottom <= limit);
                        if let Some(k) = found {
                            let r = runs[k];
                            let y = (cy * 16 + r.top).min(limit);
                            let below = k
                                .checked_sub(1)
                                .map(|b| runs[b])
                                .filter(|b| b.top + 1 == r.bottom);
                            let under = if y > cy * 16 + r.bottom { r.block } else { below.map_or(r.block, |b| b.block) };
                            here = Some((y, r.block, under, r.light));
                            break;
                        }
                    }
                    None if cy == ground => break,
                    None if cy < ground => {
                        let stone = def.terrain.as_ref().map_or(0, |t| t.stone);
                        here = Some((cy * 16 + 15, stone, stone, 0));
                        break;
                    }
                    None => {}
                }
            }
            let terrain = |&(y, top, _, _): &(i32, u32, u32, u32)| {
                y == fy && (top == ftop || ftop == u32::MAX && c.states[top as usize].liquid.is_some())
            };
            let light = here.map_or(0, |h| h.3);
            let now = match here {
                // The terrain's own ground, as it was: kept only for what stands on it or shines.
                Some(h) if terrain(&h) && spans.is_empty() && light == 0 => None,
                Some(h) if terrain(&h) => Some(LodColumn { x, z, y: fy, top: 0, under: 0, light, spans }),
                Some((y, top, under, light)) => Some(LodColumn { x, z, y, top, under, light, spans }),
                // Its ground cube isn't saved: the terrain's ground stands.
                None if spans.is_empty() => None,
                None => Some(LodColumn { x, z, y: fy, top: 0, under: 0, light: 0, spans }),
            };
            let p = IVec2::new(x, z);
            match now {
                Some(now) if self.columns.get(&p) != Some(&now) => {
                    self.columns.insert(p, now);
                    self.changed.push(p);
                }
                Some(_) => {}
                None => {
                    if self.columns.remove(&p).is_some() {
                        self.changed.push(p);
                    }
                }
            }
        }
    }

    /// Columns changed since the last call, as they are now (`top` `u32::MAX` for gone).
    pub fn take_changed(&mut self) -> Vec<LodColumn> {
        let mut ps = std::mem::take(&mut self.changed);
        ps.sort_unstable_by_key(|p| (p.x, p.y));
        ps.dedup();
        ps.into_iter()
            .map(|p| {
                self.columns.get(&p).cloned().unwrap_or(LodColumn {
                    x: p.x,
                    z: p.y,
                    y: 0,
                    top: u32::MAX,
                    under: 0,
                    light: 0,
                    spans: Vec::new(),
                })
            })
            .collect()
    }

    /// Written by world ids, lz4 packed.
    pub fn write(&mut self, path: &Path, world_id: &mut impl FnMut(u32) -> u32) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let mut b: Vec<u8> = MAGIC.to_vec();
        for (col, list) in &self.cubes {
            for (y, tops) in list {
                for v in [col.x, *y, col.y] {
                    b.extend_from_slice(&v.to_le_bytes());
                }
                for i in 0..256 {
                    let rs = tops.column(i);
                    b.push(rs.len() as u8);
                    for r in rs {
                        b.extend_from_slice(&[r.bottom as u8, r.top as u8]);
                        b.extend_from_slice(&world_id(r.block).to_le_bytes());
                        b.extend_from_slice(&r.light.to_le_bytes());
                    }
                }
            }
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(path, lz4_flex::compress_prepend_size(&b)) {
            log::error!("saving {}: {e}", path.display());
        }
    }

    /// What `write` wrote, its columns worked out. A file of another layout is left out: its
    /// columns come back as their cubes are saved again.
    pub fn read(path: &Path, c: &Content, dim: u32, seed: u64, runtime_id: &mut impl FnMut(u32) -> u32) -> DimTops {
        let mut d = DimTops::default();
        let Some(b) = std::fs::read(path)
            .ok()
            .and_then(|raw| lz4_flex::decompress_size_prepended(&raw).ok())
        else {
            return d;
        };
        let Some(mut r) = b.strip_prefix(MAGIC) else { return d };
        let u32_ = |r: &mut &[u8]| -> Option<u32> {
            let (v, rest) = r.split_first_chunk::<4>()?;
            *r = rest;
            Some(u32::from_le_bytes(*v))
        };
        let u8_ = |r: &mut &[u8]| -> Option<u8> {
            let (v, rest) = r.split_first()?;
            *r = rest;
            Some(*v)
        };
        'cubes: while !r.is_empty() {
            let (Some(x), Some(y), Some(z)) = (u32_(&mut r), u32_(&mut r), u32_(&mut r)) else { break };
            let mut at = Box::new([0u16; 257]);
            let mut runs = Vec::new();
            for i in 0..256 {
                at[i] = runs.len() as u16;
                let Some(n) = u8_(&mut r) else { break 'cubes };
                for _ in 0..n {
                    let (Some(lo), Some(hi), Some(block), Some(light)) = (u8_(&mut r), u8_(&mut r), u32_(&mut r), u32_(&mut r)) else {
                        break 'cubes;
                    };
                    runs.push(Span { bottom: lo as i32, top: hi as i32, block: runtime_id(block), light });
                }
            }
            at[256] = runs.len() as u16;
            d.cubes
                .entry(IVec2::new(x as i32, z as i32))
                .or_default()
                .insert(y as i32, CubeTops { at, runs });
        }
        let cols: Vec<IVec2> = d.cubes.keys().copied().collect();
        for col in cols {
            d.refresh(c, dim, seed, col);
        }
        d.changed.clear();
        d
    }
}
