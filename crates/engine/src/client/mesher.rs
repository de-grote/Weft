//! Turns a cube plus a one-block border into GPU records.
//!
//! Full block faces: 4 words (pos/dir/ao, texture/biome/tint, light for 4 corners).
//! Everything else (plants, boxes, liquids, whatever is translucent): 8-word quads with explicit
//! corners.

use crate::gpu_layout::{face, quad};
use weft_abi::{BlockModel, RenderLayer, block_flags as bf};

use crate::content::Content;

pub const PAD: usize = 18;
pub const PAD_VOLUME: usize = PAD * PAD * PAD;
pub const ABSENT: u32 = u32::MAX;

#[inline]
pub const fn pidx(x: i32, y: i32, z: i32) -> usize {
    (((y + 1) as usize * PAD) + (z + 1) as usize) * PAD + (x + 1) as usize
}

/// Input of one meshing job, copied out of the client world so it can run on any thread.
pub struct Neighborhood {
    pub states: Box<[u32; PAD_VOLUME]>,
    /// What else is in cells, by padded index.
    pub extras: rustc_hash::FxHashMap<usize, Vec<u32>>,
    pub light: Box<[u32; PAD_VOLUME]>,
    /// Biome id per column of the center cube.
    pub biomes: [u16; 256],
    /// World position of the cube's first block.
    pub origin: [i32; 3],
}

#[derive(Default)]
pub struct Mesh {
    /// Per layer (opaque, cutout, translucent).
    pub faces: [Vec<[u32; 4]>; 3],
    pub quads: [Vec<[u32; 8]>; 3],
}

impl Neighborhood {
    /// The state a cell counts as to a liquid: the liquid in it, if any.
    fn liquid_view(&self, c: &Content, i: usize) -> u32 {
        if !self.extras.is_empty()
            && let Some(parts) = self.extras.get(&i)
            && let Some(&l) = parts
                .iter()
                .find(|&&s| c.states[s as usize].liquid.is_some())
        {
            return l;
        }
        self.states[i]
    }
}

impl Mesh {
    pub fn is_empty(&self) -> bool {
        self.faces.iter().all(Vec::is_empty) && self.quads.iter().all(Vec::is_empty)
    }
}

use crate::gpu_layout::CORNERS;

pub const NORMALS: [[i32; 3]; 6] = [[-1, 0, 0], [1, 0, 0], [0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1]];

#[cfg(test)]
pub fn corners(dir: usize) -> [[i32; 3]; 4] {
    CORNERS[dir]
}

#[inline]
fn light16(l: u32) -> u32 {
    crate::gpu_layout::corner_light([l & 0xFF, (l >> 8) & 0xFF, (l >> 16) & 0xFF, l >> 24])
}

pub fn build(c: &Content, n: &Neighborhood, ctm: &super::ctm::Ctm) -> Mesh {
    let mut mesh = Mesh::default();
    let states = &n.states;
    let looks = super::looks::current();
    // Rows along x of the padded volume (bit x + 1): blocks, and opaque ones. A block with an
    // opaque block on every side shows nothing, and underground that is nearly all of them.
    let mut filled = [0u32; PAD * PAD];
    let mut solid = [0u32; PAD * PAD];
    for (row, (f, o)) in filled.iter_mut().zip(solid.iter_mut()).enumerate() {
        for (x, &s) in states[row * PAD..(row + 1) * PAD].iter().enumerate() {
            if s != 0 {
                *f |= 1 << x;
                if n.light[row * PAD + x] != ABSENT && ctm.hides(c.tight[s as usize].flags, s) {
                    *o |= 1 << x;
                }
            }
        }
    }
    let row = |y: i32, z: i32| ((y + 1) as usize) * PAD + (z + 1) as usize;
    // Padded index to its bit in `solid`.
    let opaque = |i: usize| solid[i / PAD] >> (i % PAD) & 1 != 0;

    for y in 0..16i32 {
        for z in 0..16i32 {
            let o = solid[row(y, z)];
            let enclosed = (o << 1) & (o >> 1) & solid[row(y - 1, z)] & solid[row(y + 1, z)] & solid[row(y, z - 1)] & solid[row(y, z + 1)];
            let mut todo = filled[row(y, z)] & !enclosed & 0x1FFFE;
            while todo != 0 {
                let x = todo.trailing_zeros() as i32 - 1;
                todo &= todo - 1;
                let i = pidx(x, y, z);
                draw_block(c, n, ctm, &looks, &opaque, &mut mesh, x, y, z, states[i]);
                // What else is in the cell.
                if !n.extras.is_empty()
                    && let Some(parts) = n.extras.get(&i)
                {
                    for &part in parts {
                        draw_block(c, n, ctm, &looks, &opaque, &mut mesh, x, y, z, part);
                    }
                }
            }
        }
    }
    // Grouped by direction so the renderer can skip faces turned away from the camera.
    for faces in &mut mesh.faces {
        faces.sort_by_key(|f| DIR_RANK[crate::gpu_layout::face_dir(f)]);
    }
    mesh
}

/// One block of a cell: all there is in a mesh for it.
#[allow(clippy::too_many_arguments)]
fn draw_block(
    c: &Content,
    n: &Neighborhood,
    ctm: &super::ctm::Ctm,
    looks: &super::looks::Looks,
    opaque: &impl Fn(usize) -> bool,
    mesh: &mut Mesh,
    x: i32,
    y: i32,
    z: i32,
    s: u32,
) {
    let i = pidx(x, y, z);
    let info = &c.states[s as usize];
    let biome = n.biomes[(z * 16 + x) as usize] as u32;
    if info.liquid.is_some() {
        liquid(c, n, ctm, mesh, x, y, z, s, biome);
        return;
    }
    // A resource pack may draw it differently (its shape and light stay the mod's).
    let drawn = looks.drawn(s, info);
    let tile = ctm.model(c, n, x, y, z, s);
    let boxes = tile.unwrap_or(drawn.boxes);
    let textures = drawn.textures;
    let model = if tile.is_some() { BlockModel::Boxes } else { drawn.model };
    match model {
        // Drawn apart, for its power to change without a new mesh (`wires.rs`).
        BlockModel::None | BlockModel::Wire => {}
        BlockModel::Cube => {
            for dir in 0..6 {
                let [dx, dy, dz] = NORMALS[dir];
                let ni = pidx(x + dx, y + dy, z + dz);
                if face_hidden(c, ctm, s, n.states[ni]) {
                    continue;
                }
                let (ao, light) = corner_data(c, n, ctm, opaque, x, y, z, dir);
                let tex = ctm.resolve(c, n, x, y, z, dir, textures[dir] & face::TEX_MASK);
                let tint = face_tint(info, ctm, dir, tex);
                // All that is translucent is kept far to near as one list of quads.
                if info.layer == RenderLayer::Translucent {
                    let corners = CORNERS[dir].map(|k| [(x + k[0]) as f32, (y + k[1]) as f32, (z + k[2]) as f32]);
                    let light = std::array::from_fn(|k| darker_by_ao(light[k], (ao >> (k * 2)) & 3));
                    mesh.quads[RenderLayer::Translucent as usize].push(quad(corners, WHOLE, tex, biome, tint, light));
                } else {
                    mesh.faces[info.layer as usize].push(crate::gpu_layout::face_record(
                        [x, y, z],
                        dir as u32,
                        ao,
                        tint as u32,
                        info.turns[dir] as u32,
                        tex,
                        biome,
                        light,
                    ));
                }
            }
        }
        BlockModel::Cross => {
            let l = light16(n.light[i]);
            let t = ctm.resolve(c, n, x, y, z, 0, textures[0] & face::TEX_MASK);
            let b = [x as f32, y as f32, z as f32];
            let e = 0.15;
            let (a0, a1) = ([b[0] + e, b[1], b[2] + e], [b[0] + 1.0 - e, b[1] + 1.0, b[2] + 1.0 - e]);
            let (c0, c1) = ([b[0] + e, b[1], b[2] + 1.0 - e], [b[0] + 1.0 - e, b[1] + 1.0, b[2] + e]);
            for (p0, p1) in [(a0, a1), (c0, c1)] {
                let corners = [[p0[0], p0[1], p0[2]], [p1[0], p0[1], p1[2]], [p1[0], p1[1], p1[2]], [p0[0], p1[1], p0[2]]];
                let back = [corners[1], corners[0], corners[3], corners[2]];
                for q in [corners, back] {
                    mesh.quads[RenderLayer::Cutout as usize].push(quad(q, WHOLE, t, biome, info.tint | FLAT, [l; 4]));
                }
            }
        }
        BlockModel::Boxes => {
            let l = light16(n.light[i]);
            // A file's model on a block with no holes may have them, which a texture
            // of a block with none wouldn't get to show otherwise.
            let layer = if info.layer == RenderLayer::Opaque { RenderLayer::Cutout } else { info.layer };
            // No light is kept inside an opaque block, whatever a pack drew there,
            // nor in others like it next to it: the brightest around goes for all.
            let dark_inside = info.flags & bf::OPAQUE != 0;
            let l = if dark_inside { light16(brightest_around(n, x, y, z)) } else { l };
            for bx in boxes {
                for dir in 0..6 {
                    let [dx, dy, dz] = NORMALS[dir];
                    let axis = dir / 2;
                    let at_edge = bx.tilt.degrees == 0.0 && if dir % 2 == 0 { bx.min[axis] <= 0.0 } else { bx.max[axis] >= 1.0 };
                    let (ns, nl) = (n.states[pidx(x + dx, y + dy, z + dz)], n.light[pidx(x + dx, y + dy, z + dz)]);
                    if at_edge && ctm.hides(c.states[ns as usize].flags, ns) {
                        continue;
                    }
                    let fl = if at_edge && !dark_inside { light16(nl) } else { l };
                    let Some((corners, uv)) = box_face(bx, dir) else { continue };
                    let corners = corners.map(|p| [p[0] + x as f32, p[1] + y as f32, p[2] + z as f32]);
                    let tex = ctm.resolve(c, n, x, y, z, dir, bx.textures[dir] & face::TEX_MASK);
                    mesh.quads[layer as usize].push(quad(corners, uv, tex, biome, face_tint(info, ctm, dir, tex), [fl; 4]));
                }
            }
        }
    }
}

/// Order of face directions in a mesh: all positive, then all negative, so the directions seen
/// from one octant are mostly one contiguous range.
pub const DIR_RANK: [u8; 6] = [3, 0, 4, 1, 5, 2];

/// A face of a model's box as it is drawn, in the world and as an item alike: its corners in the
/// block, turned and tilted, and the part of its picture (16 across). `None`: it has no such face.
/// Whether a cube's face toward `neighbor` doesn't show: it is opaque (a pack's rule doesn't open
/// it), or the same block, which culls its own (glass).
pub fn face_hidden(c: &Content, ctm: &super::ctm::Ctm, state: u32, neighbor: u32) -> bool {
    let (info, n) = (&c.states[state as usize], &c.states[neighbor as usize]);
    ctm.hides(n.flags, neighbor) || n.block == info.block && info.layer != RenderLayer::Opaque && info.flags & bf::CULL_SAME != 0
}

pub fn box_face(bx: &crate::content::ModelBox, dir: usize) -> Option<([[f32; 3]; 4], [f32; 4])> {
    if bx.uv[dir][0] <= weft_abi::UV_NO_FACE[0] {
        return None;
    }
    let corners = CORNERS[dir].map(|k| std::array::from_fn(|a| if k[a] == 1 { bx.max[a] } else { bx.min[a] }));
    let uv = match bx.uv[dir] {
        r if r[0] >= 0.0 => r,
        _ => weft_abi::box_face_uv(dir, bx.min.map(|v| v * 16.0), bx.max.map(|v| v * 16.0)),
    };
    let corners: [[f32; 3]; 4] = std::array::from_fn(|i| corners[(i + bx.turn[dir] as usize) % 4]);
    Some((if bx.tilt.degrees != 0.0 { corners.map(|p| tilted(p, &bx.tilt)) } else { corners }, uv))
}

/// Which way a face `box_face` gave looks, and how bright that makes it: as the world shader has
/// it for such faces.
pub fn face_look(corners: &[[f32; 3]; 4]) -> (glam::Vec3, f32) {
    let [p0, p1, p2, _] = corners.map(glam::Vec3::from_array);
    let n = (p1 - p0).cross(p2 - p1).normalize_or_zero();
    (n, n.x * n.x * 0.6 + n.z * n.z * 0.8 + n.y * n.y * if n.y > 0.0 { 1.0 } else { 0.5 })
}

/// A corner of a box tilted about its axis.
fn tilted(p: [f32; 3], t: &weft_abi::BoxTilt) -> [f32; 3] {
    let o = glam::Vec3::from_array(t.origin);
    let turn = glam::Quat::from_axis_angle(glam::Vec3::from_array(t.axis).normalize(), t.degrees.to_radians());
    (o + turn * (glam::Vec3::from_array(p) - o)).to_array()
}

fn corner_data(
    c: &Content,
    n: &Neighborhood,
    ctm: &super::ctm::Ctm,
    opaque: &impl Fn(usize) -> bool,
    x: i32,
    y: i32,
    z: i32,
    dir: usize,
) -> (u32, [u32; 4]) {
    let nrm = NORMALS[dir];
    let axis = dir / 2;
    let (ta, tb) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let base = [x + nrm[0], y + nrm[1], z + nrm[2]];
    let mut ao_bits = 0u32;
    let mut lights = [0u32; 4];
    for (k, corner) in CORNERS[dir].iter().enumerate() {
        let sa = corner[ta] * 2 - 1;
        let sb = corner[tb] * 2 - 1;
        let mut p1 = base;
        p1[ta] += sa;
        let mut p2 = base;
        p2[tb] += sb;
        let mut p3 = base;
        p3[ta] += sa;
        p3[tb] += sb;
        let i0 = pidx(base[0], base[1], base[2]);
        let i1 = pidx(p1[0], p1[1], p1[2]);
        let i2 = pidx(p2[0], p2[1], p2[2]);
        let i3 = pidx(p3[0], p3[1], p3[2]);
        let (s1, s2, s3) = (opaque(i1), opaque(i2), opaque(i3));
        let ao = if s1 && s2 { 0 } else { 3 - (s1 as u32 + s2 as u32 + s3 as u32) };
        ao_bits |= ao << (k * 2);

        // Only what light could come from into the cell before the face: a slab's closed side
        // keeps its neighbor's light off the face as it keeps the light itself.
        let t = |i: usize| &c.tight[n.states[i] as usize];
        let along = |axis: usize, s: i32| axis * 2 + (s > 0) as usize;
        let (to_a, to_b) = (along(ta, -sa), along(tb, -sb));
        let r1 = t(i1).lets_into(t(i0), to_a);
        let r2 = t(i2).lets_into(t(i0), to_b);
        let r3 = (t(i3).lets_into(t(i1), to_b) && r1) || (t(i3).lets_into(t(i2), to_a) && r2);
        let mut sum = [0u32; 4];
        let mut count = 0;
        for (idx, solid) in [(i0, false), (i1, s1 || !r1), (i2, s2 || !r2), (i3, s3 || (s1 && s2) || !r3)] {
            // A block with holes is still opaque to light, there is none in it to go by.
            if solid || n.light[idx] == ABSENT || ctm.is_open(n.states[idx]) {
                continue;
            }
            let l = n.light[idx];
            for (ch, s) in sum.iter_mut().enumerate() {
                *s += (l >> (ch * 8)) & 0xFF;
            }
            count += 1;
        }
        lights[k] = crate::gpu_layout::corner_light(sum.map(|v| (v + count / 2) / count.max(1)));
    }
    (ao_bits, lights)
}

const FLAT: u8 = quad::FLAT as u8;

/// Per channel, the most light in the cells around a block, corners too: the ones beside it may
/// all be as dark inside as it is.
fn brightest_around(n: &Neighborhood, x: i32, y: i32, z: i32) -> u32 {
    let mut most = [0u32; 4];
    for dy in -1..=1 {
        for dz in -1..=1 {
            for dx in -1..=1 {
                let l = n.light[pidx(x + dx, y + dy, z + dz)];
                if l == ABSENT {
                    continue;
                }
                for (ch, m) in most.iter_mut().enumerate() {
                    *m = (*m).max((l >> (ch * 8)) & 0xFF);
                }
            }
        }
    }
    most[0] | most[1] << 8 | most[2] << 16 | most[3] << 24
}

/// A corner's light with its ambient occlusion in it, a quad having no place for it: about what
/// the world shader's factors do to a bright corner, in light levels.
fn darker_by_ao(light: u32, ao: u32) -> u32 {
    let down = [4u32, 2, 1, 0][ao as usize];
    (0..4).fold(0, |w, ch| w | ((light >> (ch * 4)) & face::LIGHT_MASK).saturating_sub(down) << (ch * 4))
}

/// A whole picture on a quad.
const WHOLE: [f32; 4] = [0.0, 0.0, 16.0, 16.0];

/// The tint the word of a face says; the shader gives it to the overlay when there is one.
fn face_tint(info: &crate::content::StateInfo, ctm: &super::ctm::Ctm, dir: usize, tex: u32) -> u8 {
    let paint = crate::content::face_paint(info, dir, ctm.has_overlay(tex));
    if paint.base_tinted || paint.overlay_tinted { info.tint } else { 0 }
}

fn quad(corners: [[f32; 3]; 4], uv: [f32; 4], texture: u32, biome: u32, tint: u8, light: [u32; 4]) -> [u32; 8] {
    use crate::gpu_layout::{quad_pos, quad_uv};
    let (uv, on) = quad_uv(uv, tint as u32);
    [
        quad_pos(corners[0]) | on[0],
        quad_pos(corners[1]) | on[1],
        quad_pos(corners[2]) | on[2],
        quad_pos(corners[3]) | on[3],
        texture & face::TEX_MASK | biome << face::BIOME_SHIFT,
        uv,
        light[0] | light[1] << face::CORNER_SHIFT,
        light[2] | light[3] << face::CORNER_SHIFT,
    ]
}

#[allow(clippy::too_many_arguments)]
fn liquid(c: &Content, n: &Neighborhood, ctm: &super::ctm::Ctm, mesh: &mut Mesh, x: i32, y: i32, z: i32, s: u32, biome: u32) {
    let i = pidx(x, y, z);
    let info = &c.states[s as usize];
    let block = info.block;
    let same = |xx: i32, yy: i32, zz: i32| c.states[n.liquid_view(c, pidx(xx, yy, zz)) as usize].block == block;
    let height = |xx: i32, zz: i32| -> Option<f32> {
        if !same(xx, y, zz) {
            return None;
        }
        if same(xx, y + 1, zz) {
            return Some(1.0);
        }
        let lvl = c.states[n.liquid_view(c, pidx(xx, y, zz)) as usize]
            .level
            .min(7) as f32;
        Some((8.0 - lvl) / 9.0)
    };
    let corner_h = |cx: i32, cz: i32| -> f32 {
        // Corner (cx, cz) in {0,1}: average over the up to four columns sharing it.
        let mut sum = 0.0;
        let mut cnt = 0.0;
        for (ox, oz) in [(cx - 1, cz - 1), (cx, cz - 1), (cx - 1, cz), (cx, cz)] {
            if let Some(h) = height(x + ox, z + oz) {
                if h >= 1.0 {
                    return 1.0;
                }
                sum += h;
                cnt += 1.0;
            }
        }
        sum / cnt
    };
    let h = [corner_h(0, 0), corner_h(1, 0), corner_h(1, 1), corner_h(0, 1)];
    let hat = |cx: i32, cz: i32| match (cx, cz) {
        (0, 0) => h[0],
        (1, 0) => h[1],
        (1, 1) => h[2],
        _ => h[3],
    };
    let l = light16(n.light[i]);
    let t = info.textures[3];
    let ts = info.textures[0];
    let (fx, fy, fz) = (x as f32, y as f32, z as f32);
    let layer = info.layer as usize;
    // In a block (waterlogged): its sides it covers whole stand where the liquid's would, which
    // would fight them.
    let parts = std::iter::once(n.states[i]).chain(n.extras.get(&i).into_iter().flatten().copied());
    let walls = parts
        .filter(|&p| c.states[p as usize].liquid.is_none())
        .fold(0u8, |m, p| m | c.liquid_walls[p as usize]);
    if !same(x, y + 1, z) && walls & 1 << 3 == 0 {
        let corners = [
            [fx, fy + hat(0, 1), fz + 1.0],
            [fx + 1.0, fy + hat(1, 1), fz + 1.0],
            [fx + 1.0, fy + hat(1, 0), fz],
            [fx, fy + hat(0, 0), fz],
        ];
        let lu = light16(n.light[pidx(x, y + 1, z)]).max(l);
        mesh.quads[layer].push(quad(corners, WHOLE, t, biome, info.tint, [lu; 4]));
        let back = [corners[1], corners[0], corners[3], corners[2]];
        mesh.quads[layer].push(quad(back, WHOLE, t, biome, info.tint, [lu; 4]));
    }
    for dir in [0usize, 1, 4, 5] {
        let [dx, _, dz] = NORMALS[dir];
        if same(x + dx, y, z + dz) || walls & 1 << dir != 0 {
            continue;
        }
        let ni = pidx(x + dx, y, z + dz);
        if ctm.hides(c.states[n.states[ni] as usize].flags, n.states[ni]) {
            continue;
        }
        let k = CORNERS[dir];
        let corners = k.map(|p| {
            let top = if p[1] == 1 { hat(p[0], p[2]) } else { 0.0 };
            [fx + p[0] as f32, fy + top, fz + p[2] as f32]
        });
        let nl = light16(n.light[ni]);
        mesh.quads[layer].push(quad(corners, WHOLE, ts, biome, info.tint, [nl; 4]));
    }
    let below = pidx(x, y - 1, z);
    if !same(x, y - 1, z) && walls & 1 << 2 == 0 && !ctm.hides(c.states[n.states[below] as usize].flags, n.states[below]) {
        let corners = CORNERS[2].map(|p| [fx + p[0] as f32, fy, fz + p[2] as f32]);
        mesh.quads[layer].push(quad(corners, WHOLE, t, biome, info.tint, [light16(n.light[below]); 4]));
    }
}
