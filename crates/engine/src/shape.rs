//! Shapes as data (`abi::ShapeStep` in postfix order): what area fills, entity searches and
//! particles take. Plain data, so a fill is saved as text and replayed on cubes that load
//! later, on the server and on clients alike.

use glam::DVec3;
use weft_abi::{self as abi, shape_kind as k};

/// Most results a shape keeps pending at once: plenty for shapes built by hand.
const MAX_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prim {
    Ball {
        c: DVec3,
        r: f64,
    },
    Box {
        min: DVec3,
        max: DVec3,
    },
    /// From `base` along face direction `axis` for `h` blocks.
    Cylinder {
        base: DVec3,
        r: f64,
        h: f64,
        axis: usize,
    },
    Ellipsoid {
        c: DVec3,
        radii: DVec3,
    },
    /// A ball whose radius wanders by up to `amount` of it with direction, `scale` bumps across.
    Noisy {
        c: DVec3,
        r: f64,
        amount: f64,
        scale: f64,
        seed: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Prim(Prim),
    Union,
    Intersect,
    Subtract,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Shape {
    pub steps: Vec<Step>,
}

/// Value noise in -1..1, smooth across `p`.
fn noise3(seed: u64, p: DVec3) -> f64 {
    let f = p.floor();
    let t = p - f;
    let t = t * t * (DVec3::splat(3.0) - 2.0 * t);
    let at =
        |dx: i64, dy: i64, dz: i64| weft_noise::unit(weft_noise::hash(seed, f.x as i64 + dx, f.y as i64 + dy, f.z as i64 + dz)) * 2.0 - 1.0;
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let x00 = lerp(at(0, 0, 0), at(1, 0, 0), t.x);
    let x10 = lerp(at(0, 1, 0), at(1, 1, 0), t.x);
    let x01 = lerp(at(0, 0, 1), at(1, 0, 1), t.x);
    let x11 = lerp(at(0, 1, 1), at(1, 1, 1), t.x);
    lerp(lerp(x00, x10, t.y), lerp(x01, x11, t.y), t.z)
}

fn corners(lo: DVec3, hi: DVec3) -> [DVec3; 8] {
    std::array::from_fn(|i| {
        DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z })
    })
}

impl Prim {
    fn axis_dir(axis: usize) -> DVec3 {
        let d = abi::FACE_DIRS[axis.min(5)];
        DVec3::new(d[0] as f64, d[1] as f64, d[2] as f64)
    }

    pub fn contains(&self, p: DVec3) -> bool {
        match *self {
            Prim::Ball { c, r } => (p - c).length_squared() <= r * r,
            Prim::Box { min, max } => p.cmpge(min).all() && p.cmple(max).all(),
            Prim::Cylinder { base, r, h, axis } => {
                let d = Self::axis_dir(axis);
                let v = p - base;
                let along = v.dot(d);
                (0.0..=h).contains(&along) && (v - d * along).length_squared() <= r * r
            }
            Prim::Ellipsoid { c, radii } => ((p - c) / radii.max(DVec3::splat(1e-9))).length_squared() <= 1.0,
            Prim::Noisy { c, r, amount, scale, seed } => {
                let v = p - c;
                let len = v.length();
                if len <= r * (1.0 - amount) {
                    return true;
                }
                if len > r * (1.0 + amount) {
                    return false;
                }
                let dir = if len > 0.0 { v / len } else { DVec3::Y };
                len <= r * (1.0 + amount * noise3(seed, dir * scale))
            }
        }
    }

    pub fn bounds(&self) -> (DVec3, DVec3) {
        match *self {
            Prim::Ball { c, r } => (c - r, c + r),
            Prim::Box { min, max } => (min, max),
            Prim::Cylinder { base, r, h, axis } => {
                let d = Self::axis_dir(axis);
                let side = DVec3::splat(r) * (DVec3::ONE - d.abs());
                let end = base + d * h;
                (base.min(end) - side, base.max(end) + side)
            }
            Prim::Ellipsoid { c, radii } => (c - radii, c + radii),
            Prim::Noisy { c, r, amount, .. } => (c - r * (1.0 + amount), c + r * (1.0 + amount)),
        }
    }

    /// The whole box is inside (may say no for some that are).
    fn covers(&self, lo: DVec3, hi: DVec3) -> bool {
        match *self {
            Prim::Ball { c, r } => corners(lo, hi)
                .iter()
                .all(|&p| (p - c).length_squared() <= r * r),
            Prim::Noisy { c, r, amount, .. } => Prim::Ball { c, r: r * (1.0 - amount) }.covers(lo, hi),
            // Convex: the corners inside, the box inside.
            Prim::Box { .. } | Prim::Cylinder { .. } | Prim::Ellipsoid { .. } => corners(lo, hi).iter().all(|&p| self.contains(p)),
        }
    }

    /// The box and the shape may share a point (may say yes for some that don't).
    fn meets(&self, lo: DVec3, hi: DVec3) -> bool {
        let (a, b) = self.bounds();
        if (a.cmpgt(hi) | b.cmplt(lo)).any() {
            return false;
        }
        match *self {
            Prim::Ball { c, r } => (c.clamp(lo, hi) - c).length_squared() <= r * r,
            Prim::Noisy { c, r, amount, .. } => (c.clamp(lo, hi) - c).length_squared() <= (r * (1.0 + amount)).powi(2),
            Prim::Ellipsoid { c, radii } => {
                let s = radii.max(DVec3::splat(1e-9));
                let (l, h) = ((lo - c) / s, (hi - c) / s);
                DVec3::ZERO.clamp(l, h).length_squared() <= 1.0
            }
            Prim::Box { .. } | Prim::Cylinder { .. } => true,
        }
    }
}

impl Shape {
    pub fn from_abi(steps: &[abi::ShapeStep]) -> Option<Shape> {
        let mut out = Vec::with_capacity(steps.len());
        let mut depth = 0i32;
        for s in steps {
            let v = s.v.map(|x| if x.is_finite() { x } else { 0.0 });
            let p3 = |i: usize| DVec3::new(v[i], v[i + 1], v[i + 2]);
            let step = match s.kind {
                k::BALL => Step::Prim(Prim::Ball { c: p3(0), r: v[3].max(0.0) }),
                k::BOX => Step::Prim(Prim::Box { min: p3(0).min(p3(3)), max: p3(0).max(p3(3)) }),
                k::CYLINDER => {
                    Step::Prim(Prim::Cylinder { base: p3(0), r: v[3].max(0.0), h: v[4].max(0.0), axis: (s.axis as usize).min(5) })
                }
                k::ELLIPSOID => Step::Prim(Prim::Ellipsoid { c: p3(0), radii: p3(3).abs() }),
                k::NOISY => {
                    Step::Prim(Prim::Noisy { c: p3(0), r: v[3].max(0.0), amount: v[4].clamp(0.0, 1.0), scale: v[5].max(0.0), seed: s.seed })
                }
                k::UNION => Step::Union,
                k::INTERSECT => Step::Intersect,
                k::SUBTRACT => Step::Subtract,
                _ => return None,
            };
            depth += if matches!(step, Step::Prim(_)) { 1 } else { -1 };
            if depth < 1 || depth > MAX_DEPTH as i32 {
                return None;
            }
            out.push(step);
        }
        (depth == 1).then_some(Shape { steps: out })
    }

    /// Runs the steps with `prim` answering for each primitive and `op` combining two answers.
    fn eval<T: Copy + Default>(&self, prim: impl Fn(&Prim) -> T, op: impl Fn(Step, T, T) -> T) -> Option<T> {
        // Asked for every block of a fill: no allocation, and `from_abi` keeps it this shallow.
        let mut stack = [T::default(); MAX_DEPTH];
        let mut n = 0;
        for s in &self.steps {
            match s {
                Step::Prim(p) => {
                    *stack.get_mut(n)? = prim(p);
                    n += 1;
                }
                _ if n >= 2 => {
                    stack[n - 2] = op(*s, stack[n - 2], stack[n - 1]);
                    n -= 1;
                }
                _ => return None,
            }
        }
        (n == 1).then(|| stack[0])
    }

    pub fn contains(&self, p: DVec3) -> bool {
        self.eval(
            |s| s.contains(p),
            |op, a, b| match op {
                Step::Union => a || b,
                Step::Intersect => a && b,
                _ => a && !b,
            },
        )
        .unwrap_or(false)
    }

    /// A box around everything inside.
    pub fn bounds(&self) -> (DVec3, DVec3) {
        self.eval(
            |s| s.bounds(),
            |op, a, b| match op {
                Step::Union => (a.0.min(b.0), a.1.max(b.1)),
                Step::Intersect => (a.0.max(b.0), a.1.min(b.1).max(a.0.max(b.0))),
                _ => a,
            },
        )
        .unwrap_or((DVec3::ZERO, DVec3::ZERO))
    }

    /// (all of the box inside, part of it maybe inside): the first never says yes wrongly, the
    /// second never says no wrongly.
    pub fn test_box(&self, lo: DVec3, hi: DVec3) -> (bool, bool) {
        self.eval(
            |s| (s.covers(lo, hi), s.meets(lo, hi)),
            |op, a, b| match op {
                Step::Union => (a.0 || b.0, a.1 || b.1),
                Step::Intersect => (a.0 && b.0, a.1 && b.1),
                _ => (a.0 && !b.1, a.1 && !b.0),
            },
        )
        .unwrap_or((false, false))
    }

    /// As text for `world.txt`: steps separated by spaces, `ball:x,y,z,r` and so on.
    pub fn spec(&self) -> String {
        let n = |v: &[f64]| {
            v.iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        self.steps
            .iter()
            .map(|s| match *s {
                Step::Prim(Prim::Ball { c, r }) => format!("ball:{}", n(&[c.x, c.y, c.z, r])),
                Step::Prim(Prim::Box { min, max }) => format!("box:{}", n(&[min.x, min.y, min.z, max.x, max.y, max.z])),
                Step::Prim(Prim::Cylinder { base, r, h, axis }) => format!("cylinder:{}", n(&[base.x, base.y, base.z, r, h, axis as f64])),
                Step::Prim(Prim::Ellipsoid { c, radii }) => format!("ellipsoid:{}", n(&[c.x, c.y, c.z, radii.x, radii.y, radii.z])),
                Step::Prim(Prim::Noisy { c, r, amount, scale, seed }) => {
                    format!("noisy:{},{seed}", n(&[c.x, c.y, c.z, r, amount, scale]))
                }
                Step::Union => "union".into(),
                Step::Intersect => "intersect".into(),
                Step::Subtract => "subtract".into(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn parse<'a>(words: impl Iterator<Item = &'a str>) -> Option<Shape> {
        let mut steps = Vec::new();
        for w in words {
            let (kind, args) = w.split_once(':').unwrap_or((w, ""));
            let v: Vec<f64> = args
                .split(',')
                .filter(|a| !a.is_empty())
                .map(|a| a.parse().ok())
                .collect::<Option<_>>()?;
            let mut s = abi::ShapeStep::DEFAULT;
            s.kind = match kind {
                "ball" => k::BALL,
                "box" => k::BOX,
                "cylinder" => k::CYLINDER,
                "ellipsoid" => k::ELLIPSOID,
                "noisy" => k::NOISY,
                "union" => k::UNION,
                "intersect" => k::INTERSECT,
                "subtract" => k::SUBTRACT,
                _ => return None,
            };
            for (i, x) in v.iter().take(6).enumerate() {
                s.v[i] = *x;
            }
            if s.kind == k::CYLINDER {
                s.axis = v.get(5).copied().unwrap_or(3.0) as u32;
            }
            if s.kind == k::NOISY {
                s.seed = args.rsplit(',').next()?.parse().ok()?;
            }
            steps.push(s);
        }
        Shape::from_abi(&steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_tests_agree_with_points() {
        let shell = Shape::from_abi(&[
            abi::ShapeStep { kind: k::BALL, v: [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 0.0], ..abi::ShapeStep::DEFAULT },
            abi::ShapeStep { kind: k::NOISY, v: [2.0, 0.0, 0.0, 6.0, 0.4, 1.5, 0.0, 0.0], seed: 7, ..abi::ShapeStep::DEFAULT },
            abi::ShapeStep { kind: k::SUBTRACT, ..abi::ShapeStep::DEFAULT },
            abi::ShapeStep { kind: k::CYLINDER, v: [0.0, -12.0, 0.0, 3.0, 24.0, 0.0, 0.0, 0.0], axis: 3, ..abi::ShapeStep::DEFAULT },
            abi::ShapeStep { kind: k::UNION, ..abi::ShapeStep::DEFAULT },
        ])
        .unwrap();
        assert_eq!(Shape::parse(shell.spec().split(' ')).as_ref(), Some(&shell));
        for x in -14..14 {
            for y in -14..14 {
                for z in -14..14 {
                    let lo = DVec3::new(x as f64, y as f64, z as f64) * 1.5;
                    let hi = lo + 1.5;
                    let (all, some) = shell.test_box(lo, hi);
                    let inside: Vec<bool> = (0..27)
                        .map(|i| shell.contains(lo + DVec3::new((i % 3) as f64, (i / 3 % 3) as f64, (i / 9) as f64) * 0.75))
                        .collect();
                    assert!(!all || inside.iter().all(|&b| b), "covers wrongly at {lo}");
                    assert!(some || !inside.iter().any(|&b| b), "misses wrongly at {lo}");
                }
            }
        }
    }
}
