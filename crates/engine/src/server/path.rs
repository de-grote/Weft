//! Paths for walking mobs: A* over block cells with vanilla-like rules (room for the whole body,
//! a step up of one block by jumping, drops of up to three, no hot liquids), and the
//! navigation that walks an entity along one.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use glam::{DVec3, IVec3};
use rustc_hash::FxHashMap;

use crate::content::Content;
use crate::physics::{self, Aabb, Dims};

/// Where an entity is walking to.
pub struct Nav {
    pub target: DVec3,
    pub speed: f32,
    pub path: Vec<IVec3>,
    pub next: usize,
    /// Earliest age the path may be looked for again.
    pub repath: u64,
    /// The target the path was found for; `None` looks again at `repath`.
    pub found_for: Option<DVec3>,
    /// Ticks without getting anywhere.
    pub stuck: u32,
    /// The body it walks on, whose coordinates the target and the path are in.
    pub frame: Option<u64>,
}

impl Nav {
    /// Worth a new search: never searched, the target moved off, or the walk got stuck. A path
    /// to a target that stays put is kept (searching every mob's path every second costs most of
    /// a tick with thousands of mobs).
    pub fn stale(&self, age: u64) -> bool {
        age >= self.repath
            && self
                .found_for
                .is_none_or(|f| (f - self.target).length_squared() > 4.0 || self.stuck > 10)
    }
}

const MAX_NODES: usize = 600;

type Scratch = (FxHashMap<IVec3, (IVec3, f32)>, BinaryHeap<Reverse<(u32, i32, i32, i32)>>);

thread_local! {
    /// The search's map and queue, kept between searches (thousands of mobs walk about).
    static SCRATCH: std::cell::RefCell<Scratch> = std::cell::RefCell::new((FxHashMap::default(), BinaryHeap::new()));
}

// Center even-sized footprints between cells so they fit even-width passages.
fn offset(dims: Dims) -> DVec3 {
    let half = (dims.half_width * 2.0).ceil() * 0.5;
    DVec3::new(half, 0.0, half)
}

/// Positions a body can stand in, from `from` towards `to`: the whole way, or as far as
/// it gets when the target is out of reach or more than `MAX_NODES` nodes away.
/// `drop`: the most blocks it goes down at once (what it falls without harm).
pub fn find(c: &Content, block: &impl physics::Cells, from: DVec3, to: DVec3, dims: Dims, drop: i32) -> Vec<IVec3> {
    let offset = offset(dims);
    let cell = |p: DVec3| (p - offset + DVec3::new(0.5, 0.0, 0.5)).floor().as_ivec3();
    let target = to;
    let (from, to) = (cell(from), cell(to));
    let area = |p: IVec3| Aabb::around(p.as_dvec3() + offset, dims.half_width, dims.height);
    let top = |s: u32| physics::collision_top(c, s);
    let burns = |s: u32| {
        c.states[s as usize]
            .liquid
            .is_some_and(|l| c.liquids[l as usize].burns())
    };
    let room = |p: IVec3| {
        let area = area(p);
        if physics::collides(c, block, area) {
            return false;
        }
        let lo = area.min.floor().as_ivec3();
        let hi = area.max.ceil().as_ivec3() - IVec3::ONE;
        for y in lo.y..=hi.y {
            for z in lo.z..=hi.z {
                for x in lo.x..=hi.x {
                    let p = IVec3::new(x, y, z);
                    if block.main(p).is_none_or(burns) || block.extras(p).iter().any(|&s| burns(s)) {
                        return false;
                    }
                }
            }
        }
        true
    };
    let ground = |p: IVec3| {
        let area = area(p);
        let lo = area.min.floor().as_ivec3();
        let hi = area.max.ceil().as_ivec3() - IVec3::ONE;
        let holds = |s: u32| c.states[s as usize].liquid.is_some() || top(s).is_some_and(|t| t >= 0.5);
        let mut supported = false;
        for z in lo.z..=hi.z {
            for x in lo.x..=hi.x {
                let below = IVec3::new(x, p.y - 1, z);
                if let Some(s) = block.main(below) {
                    let extras = block.extras(below);
                    if burns(s) || extras.iter().any(|&s| burns(s)) {
                        return false;
                    }
                    supported |= holds(s) || extras.iter().any(|&s| holds(s));
                }
            }
        }
        supported
    };
    let standable = |p: IVec3| room(p) && ground(p);
    let wet = |p: IVec3| block.liquid(c, p).is_some();
    let h = |p: IVec3| (p.as_dvec3() + offset - target).length() as f32;

    let (mut came, mut queue) = SCRATCH.with_borrow_mut(std::mem::take);
    came.clear();
    queue.clear();
    came.insert(from, (from, 0.0));
    queue.push(Reverse(((h(from) * 1000.0) as u32, from.x, from.y, from.z)));
    let mut best = (h(from), from);
    let mut visited = 0;
    while let Some(Reverse((_, x, y, z))) = queue.pop() {
        let p = IVec3::new(x, y, z);
        let g = came[&p].1;
        let d = h(p);
        if d < best.0 {
            best = (d, p);
        }
        if p == to || (d < 1.0 && (p.y - to.y).abs() <= 1) {
            best = (0.0, p);
            break;
        }
        visited += 1;
        if visited > MAX_NODES {
            break;
        }
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let side = IVec3::new(dx, 0, dz);
            // Diagonals only where both straight neighbors leave room (no corner cutting).
            if dx != 0 && dz != 0 && !(room(p + IVec3::new(dx, 0, 0)) && room(p + IVec3::new(0, 0, dz))) {
                continue;
            }
            let base = if dx != 0 && dz != 0 { 1.414 } else { 1.0 };
            let q = p + side;
            let mut next = None;
            if standable(q) {
                next = Some((q, base));
            } else if !room(q) {
                // A step up: jump, with room above the head first.
                let up = q + IVec3::Y;
                if standable(up) && room(p + IVec3::Y) {
                    next = Some((up, base + 1.0));
                }
            } else {
                // Open but nothing to stand on: drop down to the first ground.
                for k in 1..=drop {
                    let down = q - IVec3::Y * k;
                    if !room(down) {
                        break;
                    }
                    if ground(down) {
                        next = Some((down, base + 0.5 * k as f32));
                        break;
                    }
                }
            }
            if let Some((n, cost)) = next {
                let g2 = g + cost + if wet(n) { 2.0 } else { 0.0 };
                if came.get(&n).is_none_or(|&(_, old)| g2 < old) {
                    came.insert(n, (p, g2));
                    queue.push(Reverse((((g2 + h(n)) * 1000.0) as u32, n.x, n.y, n.z)));
                }
            }
        }
    }
    let mut path = vec![best.1];
    while let Some(&(prev, _)) = came.get(path.last().unwrap()) {
        if prev == *path.last().unwrap() {
            break;
        }
        path.push(prev);
    }
    path.reverse();
    SCRATCH.with_borrow_mut(|s| *s = (came, queue));
    path
}

/// Steering for this tick: walk direction, jump, and whether the target is reached.
pub fn steer(nav: &mut Nav, pos: DVec3, dims: Dims, blocked: bool) -> Option<([f32; 2], bool)> {
    let feet = pos.floor().as_ivec3();
    while let Some(&w) = nav.path.get(nav.next) {
        let center = w.as_dvec3() + offset(dims);
        let flat = (center - pos).with_y(0.0);
        if flat.length() < 0.35 && (w.y - feet.y).abs() <= 1 {
            nav.next += 1;
            continue;
        }
        let dir = flat.normalize_or_zero();
        return Some(([dir.x as f32, dir.z as f32], w.y > feet.y || blocked));
    }
    None
}
