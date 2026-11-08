//! Shapes as values: what `World::fill`, `World::entities_in`, `draw_shape` and particle
//! bursts take. Built from a few primitives and combined; plain data, so a fill can be saved
//! and replayed where cubes load later.
//!
//! ```ignore
//! w.fill(Shape::blast(at, 4.0, w.random()), 0).keep("#unbreakable");
//! let geode = Shape::ball(c, 4.0).subtract(Shape::ball(c, 3.0));
//! let on_belt = w.entities_in(&Shape::cuboid(lo, hi), EntityFilter::any());
//! ```

use crate::abi::Vec3;
use crate::abi::{self, ShapeStep, shape_kind as k};
use crate::{Dir, host};

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    steps: Vec<ShapeStep>,
}

fn prim(kind: u32, v: &[f64]) -> Shape {
    let mut s = ShapeStep { kind, ..ShapeStep::DEFAULT };
    s.v[..v.len()].copy_from_slice(v);
    Shape { steps: vec![s] }
}

impl Shape {
    pub fn ball(center: impl Into<Vec3>, radius: f64) -> Shape {
        let center = center.into();
        prim(k::BALL, &[center.x, center.y, center.z, radius])
    }
    /// The box between two corners.
    pub fn cuboid(min: impl Into<Vec3>, max: impl Into<Vec3>) -> Shape {
        let min = min.into();
        let max = max.into();
        prim(k::BOX, &[min.x, min.y, min.z, max.x, max.y, max.z])
    }
    /// From `base` along `dir` for `height` blocks (a mine shaft: `Dir::Down`).
    pub fn cylinder(base: impl Into<Vec3>, radius: f64, height: f64, dir: Dir) -> Shape {
        let base = base.into();
        let mut s = prim(k::CYLINDER, &[base.x, base.y, base.z, radius, height]);
        s.steps[0].axis = dir.face();
        s
    }
    pub fn ellipsoid(center: impl Into<Vec3>, radii: impl Into<Vec3>) -> Shape {
        let center = center.into();
        let radii = radii.into();
        prim(k::ELLIPSOID, &[center.x, center.y, center.z, radii.x, radii.y, radii.z])
    }
    /// A ball whose edge wanders in and out by up to `amount` of the radius (0..1), with
    /// `bumps` of them across; the same `seed`, the same edge.
    pub fn noisy(center: impl Into<Vec3>, radius: f64, amount: f64, bumps: f64, seed: u64) -> Shape {
        let center = center.into();
        let mut s = prim(k::NOISY, &[center.x, center.y, center.z, radius, amount, bumps]);
        s.steps[0].seed = seed;
        s
    }
    /// A crater like vanilla's explosions leave, without their rays: a ball of radius `power`
    /// with a ragged edge.
    pub fn blast(center: impl Into<Vec3>, power: f64, seed: u64) -> Shape {
        Shape::noisy(center.into(), power, 0.25, 1.5, seed)
    }

    fn join(mut self, other: Shape, op: u32) -> Shape {
        self.steps.extend(other.steps);
        self.steps
            .push(ShapeStep { kind: op, ..ShapeStep::DEFAULT });
        self
    }
    /// Inside either.
    pub fn union(self, other: Shape) -> Shape {
        self.join(other, k::UNION)
    }
    /// Inside both.
    pub fn intersect(self, other: Shape) -> Shape {
        self.join(other, k::INTERSECT)
    }
    /// Inside this and not in `other`: a shell, a tunnel's hollow.
    pub fn subtract(self, other: Shape) -> Shape {
        self.join(other, k::SUBTRACT)
    }

    pub fn contains(&self, p: impl Into<Vec3>) -> bool {
        let p = p.into();
        unsafe { (host().shape_contains)(abi::Slice::new(&self.steps), &p) != 0 }
    }
    /// A box around everything inside (min, max).
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let (mut lo, mut hi) = (Vec3::ZERO, Vec3::ZERO);
        unsafe { (host().shape_bounds)(abi::Slice::new(&self.steps), &mut lo, &mut hi) };
        (lo, hi)
    }

    pub fn steps(&self) -> &[ShapeStep] {
        &self.steps
    }

    /// Lines tracing each primitive (circles around balls and cylinders, a box's edges), for
    /// drawing it.
    pub fn outline(&self) -> Vec<(Vec3, Vec3)> {
        let mut out = Vec::new();
        for s in &self.steps {
            let v = s.v;
            let c = [v[0], v[1], v[2]];
            match s.kind {
                k::BALL => ellipse_rings(&mut out, c.into(), [v[3]; 3].into()),
                k::NOISY => ellipse_rings(&mut out, c.into(), [v[3]; 3].into()),
                k::ELLIPSOID => ellipse_rings(&mut out, c.into(), [v[3], v[4], v[5]].into()),
                k::BOX => box_edges(&mut out, c.into(), [v[3], v[4], v[5]].into()),
                k::CYLINDER => {
                    let d = abi::FACE_DIRS[(s.axis as usize).min(5)].map(|x| x as f64);
                    let top = [c[0] + d[0] * v[4], c[1] + d[1] * v[4], c[2] + d[2] * v[4]];
                    let a = d.iter().position(|x| *x != 0.0).unwrap_or(1);
                    let (u, w) = ((a + 1) % 3, (a + 2) % 3);
                    for end in [c, top] {
                        ring(&mut out, end.into(), u, w, v[3], v[3]);
                    }
                    for i in 0..4 {
                        let t = i as f64 * std::f64::consts::FRAC_PI_2;
                        let mut p = c;
                        p[u] += t.cos() * v[3];
                        p[w] += t.sin() * v[3];
                        let q = [p[0] + top[0] - c[0], p[1] + top[1] - c[1], p[2] + top[2] - c[2]];
                        out.push((p.into(), q.into()));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

fn ring(out: &mut Vec<(Vec3, Vec3)>, c: Vec3, u: usize, w: usize, ru: f64, rw: f64) {
    const N: usize = 24;
    let at = |i: usize| {
        let t = i as f64 / N as f64 * std::f64::consts::TAU;
        let mut p = c.to_array();
        p[u] += t.cos() * ru;
        p[w] += t.sin() * rw;
        Vec3::from(p)
    };
    for i in 0..N {
        out.push((at(i), at(i + 1)));
    }
}

fn ellipse_rings(out: &mut Vec<(Vec3, Vec3)>, c: Vec3, r: Vec3) {
    ring(out, c, 0, 2, r.x, r.z);
    ring(out, c, 0, 1, r.x, r.y);
    ring(out, c, 2, 1, r.z, r.y);
}

fn box_edges(out: &mut Vec<(Vec3, Vec3)>, lo: Vec3, hi: Vec3) {
    let corner =
        |i: usize| [if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z }];
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                out.push((corner(i).into(), corner(i | bit).into()));
            }
        }
    }
}

/// A fill in the making (`World::fill`); it happens as this goes.
pub struct Fill {
    pub(crate) world: crate::World,
    pub(crate) shape: Shape,
    pub(crate) state: u32,
    pub(crate) keep: String,
    pub(crate) only: String,
}

impl Fill {
    /// Blocks `what` takes stay (a block matcher: `"#unbreakable #liquids"`), on top of what an
    /// earlier `keep` said. One naming a block nobody has is a mistake the fill logs instead of
    /// happening.
    pub fn keep(mut self, what: &str) -> Self {
        add_terms(&mut self.keep, what);
        self
    }
    /// Only blocks `what` (or an earlier `only`) takes change (a block matcher): ore veins in
    /// stone only.
    pub fn only(mut self, what: &str) -> Self {
        add_terms(&mut self.only, what);
        self
    }
}

fn add_terms(to: &mut String, what: &str) {
    if !to.is_empty() {
        to.push(' ');
    }
    to.push_str(&crate::qualify_matcher(crate::mod_id(), what));
}

impl Drop for Fill {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let desc = abi::FillDesc {
                shape: abi::Slice::new(&self.shape.steps),
                state: self.state,
                keep: abi::Str::new(&self.keep),
                only: abi::Str::new(&self.only),
            };
            unsafe { (host().fill)(self.world.raw(), &desc) }
        }
    }
}

/// What `World::entities_in` picks.
#[derive(Clone, Copy, Debug)]
pub struct EntityFilter {
    kind: u32,
    flags: u32,
}

impl EntityFilter {
    pub fn any() -> EntityFilter {
        EntityFilter { kind: u32::MAX, flags: 0 }
    }
    pub fn kind(kind: &crate::EntityKind) -> EntityFilter {
        EntityFilter { kind: kind.id(), flags: 0 }
    }
    /// Only what can be hit: no dropped items, block entities, spectators or the dead.
    pub fn targets(mut self) -> Self {
        self.flags |= abi::entity_filter::TARGETS;
        self
    }
    /// Any part of its box in the shape, not just its feet (an arrow catching a tall mob's head).
    pub fn touching(mut self) -> Self {
        self.flags |= abi::entity_filter::TOUCHING;
        self
    }
}

impl crate::World {
    /// Every block whose middle is inside `shape` becomes `state` (air for a crater), at the
    /// end of the tick and across the whole dimension: loaded cubes at once on every core, the
    /// rest when they load. Far faster than `set_block` per block for anything big. It doesn't ask
    /// `BREAK_BLOCK` or drop anything.
    pub fn fill(&self, shape: Shape, state: u32) -> Fill {
        Fill { world: *self, shape, state, keep: String::new(), only: String::new() }
    }
    /// Entities in `shape` that `filter` lets through.
    pub fn entities_in(&self, shape: &Shape, filter: EntityFilter) -> Vec<u64> {
        let mut buf = vec![0u64; 64];
        let call = |buf: &mut Vec<u64>| unsafe {
            (host().entities_in)(self.raw(), abi::Slice::new(&shape.steps), filter.kind, filter.flags, buf.as_mut_ptr(), buf.len())
        };
        let n = call(&mut buf);
        if n > buf.len() {
            buf.resize(n, 0);
            call(&mut buf);
        }
        buf.truncate(n);
        buf
    }
    /// Entity ids whose feet are within `radius`; `kind` None = any kind.
    pub fn entities_near(&self, pos: impl Into<Vec3>, radius: f64, kind: Option<&crate::EntityKind>) -> Vec<u64> {
        let f = kind.map_or(EntityFilter::any(), EntityFilter::kind);
        self.entities_in(&Shape::ball(pos.into(), radius), f)
    }
    /// Like `entities_near`, only what can be hit.
    pub fn targets_near(&self, pos: impl Into<Vec3>, radius: f64, kind: Option<&crate::EntityKind>) -> Vec<u64> {
        let f = kind.map_or(EntityFilter::any(), EntityFilter::kind);
        self.entities_in(&Shape::ball(pos.into(), radius), f.targets())
    }
}
