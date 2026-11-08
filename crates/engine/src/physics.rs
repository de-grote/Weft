//! Axis-separated AABB movement against block collision boxes and solid bodies (entities others
//! stand on and bump into), riding bodies, plus voxel raycasts. Shared by the client (local
//! player prediction) and the server (entities), so both agree.

use glam::{DVec3, IVec3};
use weft_abi::{EntitySize, block_flags as bf};

use crate::content::Content;

/// The box of an entity in a pose (modern vanilla), on client and server
/// alike, from its width, height and eyes standing: crouching lowers them like the player's 1.8 to
/// 1.5 and 1.62 to 1.27, swimming to 0.6 and 0.4, whatever size it has.
pub fn posed(size: EntitySize, pose: crate::world::Pose) -> Dims {
    let r = |v: f64| (v * 1e6).round() / 1e6;
    let (half_width, height, eye) = (size.width as f64 / 2.0, size.height as f64, size.eye as f64);
    let d = pose.def();
    if d.height == 1.0 && d.eye == 1.0 {
        return Dims { half_width, height, eye };
    }
    Dims { half_width, height: r(height * d.height), eye: r(eye * d.eye) }
}

/// An entity's box as physics uses it, in blocks: half its width (from its middle to a side),
/// its height and where its eyes are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dims {
    pub half_width: f64,
    pub height: f64,
    pub eye: f64,
}

impl Dims {
    /// The same box as mods see sizes.
    pub fn size(&self) -> EntitySize {
        EntitySize { width: self.half_width as f32 * 2.0, height: self.height as f32, eye: self.eye as f32 }
    }
}

/// Where a look goes, radians: yaw 0 is north (-Z), pitch up.
pub fn look_dir(yaw: f32, pitch: f32) -> DVec3 {
    let (sy, cy) = (yaw as f64).sin_cos();
    let (sp, cp) = (pitch as f64).sin_cos();
    DVec3::new(-sy * cp, sp, -cy * cp)
}

/// A walker's jump, blocks per tick.
pub const JUMP: f64 = 0.42;
/// Blocks a walker goes up without jumping.
pub const STEP_UP: f64 = 0.6;

/// A dimension's gravity, in blocks per second squared, per tick squared.
pub fn gravity_per_tick(gravity: f64) -> f64 {
    gravity * crate::world::TICK * crate::world::TICK
}

/// Vanilla's knockback, in blocks per tick: half the speed, 0.4 away (`away` is flat and of length
/// 1) and on the ground up, never more than that up.
pub fn knock(vel: DVec3, away: DVec3, on_ground: bool) -> DVec3 {
    const STRENGTH: f64 = 0.4;
    let y = if on_ground { (vel.y / 2.0 + STRENGTH).min(STRENGTH) } else { vel.y };
    DVec3::new(vel.x / 2.0 + away.x * STRENGTH, y, vel.z / 2.0 + away.z * STRENGTH)
}

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    pub fn around(feet: DVec3, half_width: f64, height: f64) -> Aabb {
        Aabb {
            min: DVec3::new(feet.x - half_width, feet.y, feet.z - half_width),
            max: DVec3::new(feet.x + half_width, feet.y + height, feet.z + half_width),
        }
    }

    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min.cmplt(o.max).all() && self.max.cmpgt(o.min).all()
    }

    pub fn shifted(&self, by: DVec3) -> Aabb {
        Aabb { min: self.min + by, max: self.max + by }
    }
}

/// Where a body's blocks are: from its own coordinates to the world's and back. All of that goes
/// through here, which is where turning would go.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Transform {
    origin: DVec3,
}

impl Transform {
    /// Of a body whose entity is at `pos`: its block (0, 0, 0) is centered on it.
    pub fn of(pos: DVec3) -> Transform {
        Transform { origin: pos - DVec3::new(0.5, 0.0, 0.5) }
    }
    /// Where its block (0, 0, 0) starts, for what can only move: drawing.
    pub fn origin(self) -> DVec3 {
        self.origin
    }
    pub fn to_world(self, p: DVec3) -> DVec3 {
        p + self.origin
    }
    pub fn to_local(self, p: DVec3) -> DVec3 {
        p - self.origin
    }
    pub fn box_to_world(self, b: Aabb) -> Aabb {
        b.shifted(self.origin)
    }
    pub fn box_to_local(self, b: Aabb) -> Aabb {
        b.shifted(-self.origin)
    }
    /// The whole-block place of its coordinates among a world's, which was `was`. On each axis it
    /// gets to the next only strictly past the middle: exactly half is no more, or which way
    /// something goes shows at speeds that land there.
    pub fn cell(self, was: Option<IVec3>) -> IVec3 {
        IVec3::from_array([0, 1, 2].map(|a| match was {
            Some(w) if (self.origin[a] - w[a] as f64).abs() <= 0.5 + 1e-6 => w[a],
            _ => self.origin[a].round() as i32,
        }))
    }
}

/// Blocks a moving structure is built of, in its own coordinates (outside it: air).
pub trait Blocks: Sync {
    fn block(&self, p: IVec3) -> u32;
    /// The corners of its blocks, inclusive; `None` when it has none.
    fn bounds(&self) -> Option<(IVec3, IVec3)>;
    /// What else is in that cell.
    fn extras(&self, _p: IVec3) -> &[u32] {
        &[]
    }
}

/// Blocks by position to physics: the one a cell stands for, `None` for what isn't loaded (to
/// count as solid), and what else is in it. A plain closure has nothing else.
pub trait Cells {
    fn main(&self, p: IVec3) -> Option<u32>;
    fn extras(&self, _p: IVec3) -> &[u32] {
        &[]
    }
    /// Both at once, for what reads every cell around every mob every tick.
    #[inline]
    fn cell(&self, p: IVec3) -> Option<(u32, &[u32])> {
        Some((self.main(p)?, self.extras(p)))
    }
    /// The liquid in a cell, on its own or in with something.
    fn liquid(&self, c: &Content, p: IVec3) -> Option<u32> {
        self.main(p)
            .into_iter()
            .chain(self.extras(p).iter().copied())
            .find(|&s| c.states[s as usize].liquid.is_some())
    }
}

impl<F: Fn(IVec3) -> Option<u32>> Cells for F {
    fn main(&self, p: IVec3) -> Option<u32> {
        self(p)
    }
}

/// A moving structure's blocks as `Cells`: never anything unloaded.
struct BodyCells<'a>(&'a dyn Blocks);

impl Cells for BodyCells<'_> {
    fn main(&self, p: IVec3) -> Option<u32> {
        Some(self.0.block(p))
    }
    fn extras(&self, p: IVec3) -> &[u32] {
        self.0.extras(p)
    }
}

/// Collision boxes of a state in block-local coordinates.
pub fn boxes(c: &Content, state: u32, out: &mut Vec<[f64; 6]>) {
    out.extend(
        c.states[state as usize]
            .collision
            .iter()
            .map(|b| b.map(|v| v as f64)),
    );
}

/// A solid body this step: its box after its move (around all its blocks for a structure), how
/// far it moved, how high above it riders still move with it, whether it crushes what it
/// squeezes, and the blocks it is built of with where they are.
#[derive(Clone, Copy)]
pub struct Body<'a> {
    pub id: u64,
    pub aabb: Aabb,
    pub delta: DVec3,
    pub carry: f64,
    pub crushes: bool,
    pub blocks: Option<(&'a dyn Blocks, Transform)>,
}

impl<'a> Body<'a> {
    /// A solid entity at `pos` with the box `(half width, height)`: a structure collides as its
    /// blocks, an empty one not at all.
    pub fn of(
        id: u64,
        kind: &crate::registry::EntityKind,
        pos: DVec3,
        size: (f64, f64),
        delta: DVec3,
        built: Option<&'a dyn Blocks>,
    ) -> Option<Body<'a>> {
        let place = Transform::of(pos);
        let aabb = match built {
            Some(s) => {
                let (lo, hi) = s.bounds()?;
                place.box_to_world(Aabb { min: lo.as_dvec3(), max: (hi + IVec3::ONE).as_dvec3() })
            }
            None => Aabb::around(pos, size.0, size.1),
        };
        let blocks = built.map(|s| (s, place));
        Some(Body { id, aabb, delta, carry: kind.carry_height as f64, crushes: kind.is(crate::tag_bits::entity::CRUSHES), blocks })
    }

    /// Its solid boxes touching `area`, in world space (until `f` returns true).
    pub fn solids(&self, c: &Content, area: Aabb, mut f: impl FnMut(Aabb) -> bool) -> bool {
        if !self.aabb.intersects(&area) {
            return false;
        }
        match self.blocks {
            None => f(self.aabb),
            Some((blocks, place)) => each_solid(c, &BodyCells(blocks), place.box_to_local(area), |b| f(place.box_to_world(b))),
        }
    }

    /// Its solid boxes something with feet at `pos` could stand on, at most `reach` below the feet.
    fn under(&self, c: &Content, pos: DVec3, half_width: f64, reach: f64, mut f: impl FnMut(Aabb)) {
        let area = Aabb {
            min: DVec3::new(pos.x - half_width, pos.y - reach - 1e-3, pos.z - half_width),
            max: DVec3::new(pos.x + half_width, pos.y + 1e-3, pos.z + half_width),
        };
        let mut look = |b: Aabb| {
            let inside = b.min.x < area.max.x && b.max.x > area.min.x && b.min.z < area.max.z && b.max.z > area.min.z;
            if inside && b.max.y <= pos.y + 1e-3 && b.max.y >= pos.y - reach - 1e-3 {
                f(b);
            }
            false
        };
        match self.blocks {
            // Some only shove, and aren't to stand on and be taken away by.
            Some((blocks, place)) if self.aabb.intersects(&area) => {
                let solid = |p: IVec3| {
                    Some(blocks.block(p))
                        .filter(|s| c.states[*s as usize].flags & bf::NOT_STOOD_ON == 0)
                        .or(Some(crate::content::AIR))
                };
                each_solid(c, &solid, place.box_to_local(area), |b| look(place.box_to_world(b)));
            }
            _ => {
                self.solids(c, area, look);
            }
        }
    }

    /// How much of it a box with feet at `pos` stands on.
    fn contact(&self, c: &Content, pos: DVec3, half_width: f64, reach: f64) -> f64 {
        let mut area = 0.0;
        self.under(c, pos, half_width, reach, |b| {
            let width = |lo: f64, hi: f64, at: f64| (hi.min(at + half_width) - lo.max(at - half_width)).max(0.0);
            area += width(b.min.x, b.max.x, pos.x) * width(b.min.z, b.max.z, pos.z);
        });
        area
    }
}

/// What a box stands on, given where its feet are against each body: the one it has most under it.
/// Two moving differently with as much under it carry it nowhere, for no list order to decide.
fn most_under<'a>(
    c: &Content,
    bodies: &'a [Body<'a>],
    skip: u64,
    half_width: f64,
    reach: f64,
    feet: impl Fn(&Body) -> DVec3,
) -> Option<&'a Body<'a>> {
    let mut best: Option<(&Body, f64)> = None;
    let mut tied = false;
    for b in bodies.iter().filter(|b| b.id != skip) {
        let area = b.contact(c, feet(b), half_width, reach);
        if area <= 1e-9 {
            continue;
        }
        match best {
            Some((_, most)) if area > most + 1e-9 => (best, tied) = (Some((b, area)), false),
            Some((other, most)) if area >= most - 1e-9 => tied |= other.delta != b.delta,
            Some(_) => {}
            None => best = Some((b, area)),
        }
    }
    best.filter(|_| !tied).map(|b| b.0)
}

/// What a rider's support does to it before its own move: carries it by `carry`, or lets go
/// (`released`: the support's last move, for keeping its momentum).
#[derive(Default)]
pub struct Ride {
    pub carry: DVec3,
    pub released: Option<DVec3>,
}

/// A rider moves with the body it stands on, and keeps doing so while in the air above it up to
/// the body's carry height (its own motion stays relative to the body, like inside a moving
/// ship); further away it lets go. What stood on a body before its move rides from that move.
pub fn ride(c: &Content, pos: DVec3, half_width: f64, on_ground: bool, support: &mut Option<u64>, bodies: &[Body<'_>], skip: u64) -> Ride {
    if support.is_none() && on_ground {
        *support = most_under(c, bodies, skip, half_width, 0.0, |b| pos + b.delta)
            .filter(|b| b.delta != DVec3::ZERO)
            .map(|b| b.id);
    }
    let Some(id) = *support else { return Ride::default() };
    let Some(b) = bodies.iter().find(|b| b.id == id) else {
        *support = None;
        return Ride::default();
    };
    // Where the body was before its move, which is where the rider stood on it.
    let (lo, hi) = (b.aabb.min - b.delta, b.aabb.max - b.delta);
    // A rider right at the edge stays on (exact edges come and go with rounding).
    let e = half_width + 1e-6;
    let over = pos.x + e > lo.x && pos.x - e < hi.x && pos.z + e > lo.z && pos.z - e < hi.z;
    // Anywhere from its bottom (inside a structure's cabin, say) up to its carry height.
    if over && pos.y > lo.y - 0.25 && pos.y - hi.y <= b.carry.max(0.01) {
        Ride { carry: b.delta, released: None }
    } else {
        *support = None;
        Ride { carry: DVec3::ZERO, released: Some(b.delta) }
    }
}

/// The body under a box with feet at `pos`, its top at most `reach` below the feet: the frame
/// a position put there belongs to.
pub fn body_below(c: &Content, pos: DVec3, half_width: f64, bodies: &[Body<'_>], reach: f64) -> Option<u64> {
    most_under(c, bodies, 0, half_width, reach, |_| pos).map(|b| b.id)
}

/// What bodies did to a box before its own move: the last move of one it let go of (for keeping
/// its momentum), and whether a crushing one squeezed it into blocks.
pub struct Carried {
    pub released: Option<DVec3>,
    pub crushed: bool,
}

/// The body a box rides moves it first (blocks still stop it), then bodies that ran into it push.
/// The order and the rules are the same for a player's prediction and for the server.
#[allow(clippy::too_many_arguments)]
pub fn carried(
    c: &Content,
    block: &impl Cells,
    pos: &mut DVec3,
    half_width: f64,
    height: f64,
    on_ground: bool,
    support: &mut Option<u64>,
    bodies: &[Body<'_>],
    skip: u64,
) -> Carried {
    let rode = ride(c, *pos, half_width, on_ground, support, bodies, skip);
    if rode.carry != DVec3::ZERO {
        *pos = move_box(c, block, *pos, half_width, height, rode.carry, 0.0).pos;
    }
    let (shift, crushes) = push(c, *pos, half_width, height, bodies, skip, *support);
    let mut crushed = false;
    if shift != DVec3::ZERO {
        let to = move_box(c, block, *pos, half_width, height, shift, 0.0).pos;
        // It hurts what it cannot push away.
        crushed = crushes && (to - *pos - shift).length_squared() > 1e-6;
        *pos = to;
    }
    Carried { released: rode.released, crushed }
}

/// The body whose top a box with feet at `pos` rests on.
pub fn standing_on(c: &Content, pos: DVec3, half_width: f64, bodies: &[Body<'_>], skip: u64) -> Option<u64> {
    most_under(c, bodies, skip, half_width, 0.0, |_| pos).map(|b| b.id)
}

/// How far bodies that moved this step and overlap a box push it out of them, along each body's
/// main direction of travel, and whether a crushing one did. A box already inside goes out too
/// (like vanilla's shulker boxes): a body that showed up around a rider, or a rider put on a
/// deck the client draws ahead of the server, must not stay stuck in it. The body it rides and
/// `skip` (itself) do not push.
pub fn push(c: &Content, pos: DVec3, half_width: f64, height: f64, bodies: &[Body<'_>], skip: u64, support: Option<u64>) -> (DVec3, bool) {
    let me = Aabb::around(pos, half_width, height);
    let (mut shift, mut crushes) = (DVec3::ZERO, false);
    for b in bodies
        .iter()
        .filter(|b| b.id != skip && Some(b.id) != support && b.delta != DVec3::ZERO)
    {
        let d = b.delta.abs();
        let axis = if d.y >= d.x && d.y >= d.z {
            1
        } else if d.x >= d.z {
            0
        } else {
            2
        };
        let up = b.delta[axis] > 0.0;
        b.solids(c, me, |s| {
            if s.intersects(&me) {
                let need = if up { s.max[axis] - me.min[axis] } else { s.min[axis] - me.max[axis] };
                if need.abs() > shift[axis].abs() {
                    shift[axis] = need;
                }
                crushes |= b.crushes;
            }
            false
        });
    }
    (shift, crushes)
}

/// Calls `f` with the solid collision boxes of the blocks around `area` (unloaded positions count
/// as full blocks) until it returns true; true if it did. Nothing is collected: thousands of mobs
/// move every tick.
fn each_solid(c: &Content, block: &impl Cells, area: Aabb, mut f: impl FnMut(Aabb) -> bool) -> bool {
    // One extra layer below for collision boxes taller than a block (fences); everything else
    // stays inside its own block.
    let lo = area.min.floor().as_ivec3() - IVec3::Y;
    let hi = area.max.floor().as_ivec3();
    for y in lo.y..=hi.y {
        for z in lo.z..=hi.z {
            for x in lo.x..=hi.x {
                let p = IVec3::new(x, y, z);
                let b = p.as_dvec3();
                let full = Aabb { min: b, max: b + DVec3::ONE };
                let Some((s, extras)) = block.cell(p) else {
                    if f(full) {
                        return true;
                    }
                    continue;
                };
                for s in std::iter::once(s).chain(extras.iter().copied()) {
                    for bx in &c.states[s as usize].collision {
                        let bx = bx.map(|v| v as f64);
                        if f(Aabb { min: b + DVec3::new(bx[0], bx[1], bx[2]), max: b + DVec3::new(bx[3], bx[4], bx[5]) }) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// How far along `from`..`to` (0..1) a cube `half` each way around `from` goes before something
/// solid stops it; what it starts inside of doesn't.
pub fn sweep(c: &Content, block: &impl Cells, from: DVec3, to: DVec3, half: f64) -> f64 {
    let d = to - from;
    if d.length_squared() < 1e-12 {
        return 1.0;
    }
    let h = DVec3::splat(half);
    let area = Aabb { min: from.min(to) - h, max: from.max(to) + h };
    let mut t = 1.0f64;
    each_solid(c, block, area, |b| {
        let grown = Aabb { min: b.min - h, max: b.max + h };
        let inside = from.cmpgt(grown.min).all() && from.cmplt(grown.max).all();
        if !inside {
            if let Some(hit) = ray_aabb(from, d, &grown, 1.0) {
                t = t.min(hit);
            }
        }
        false
    });
    t
}

/// Whether `area` overlaps anything solid.
pub fn collides(c: &Content, block: &impl Cells, area: Aabb) -> bool {
    each_solid(c, block, area, |s| s.intersects(&area))
}

thread_local! {
    /// `move_box_with`'s obstacles, kept between calls.
    static SOLIDS: std::cell::RefCell<Vec<Aabb>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub struct MoveResult {
    pub pos: DVec3,
    pub hit: [bool; 3],
    pub on_ground: bool,
}

/// Moves a box with feet at `pos` by `delta`. `block(p)` returns the state at p, or `None` for
/// unloaded positions which are treated as solid so nothing falls out of the world while loading.
pub fn move_box(c: &Content, block: &impl Cells, pos: DVec3, half_width: f64, height: f64, delta: DVec3, step_height: f64) -> MoveResult {
    move_box_with(c, block, &[], 0, pos, half_width, height, delta, step_height)
}

/// `move_box` that also bumps into solid bodies (all but `skip`).
#[allow(clippy::too_many_arguments)]
pub fn move_box_with(
    c: &Content,
    block: &impl Cells,
    bodies: &[Body<'_>],
    skip: u64,
    pos: DVec3,
    half_width: f64,
    height: f64,
    delta: DVec3,
    step_height: f64,
) -> MoveResult {
    let start = Aabb::around(pos, half_width, height);
    let mut sweep = start;
    for i in 0..3 {
        if delta[i] < 0.0 {
            sweep.min[i] += delta[i];
        } else {
            sweep.max[i] += delta[i];
        }
    }
    sweep.max.y += step_height;
    let mut solids = SOLIDS.with_borrow_mut(std::mem::take);
    solids.clear();
    each_solid(c, block, sweep, |b| {
        solids.push(b);
        false
    });
    for b in bodies.iter().filter(|b| b.id != skip) {
        b.solids(c, sweep, |s| {
            solids.push(s);
            false
        });
    }

    let try_move = |from: Aabb, delta: DVec3| -> (Aabb, [bool; 3], DVec3) {
        let mut b = from;
        let mut hit = [false; 3];
        let mut moved = DVec3::ZERO;
        for axis in [1usize, 0, 2] {
            let mut d = delta[axis];
            if d == 0.0 {
                continue;
            }
            for s in &solids {
                let overlap = (0..3)
                    .filter(|&a| a != axis)
                    .all(|a| b.min[a] < s.max[a] - 1e-7 && b.max[a] > s.min[a] + 1e-7);
                if !overlap {
                    continue;
                }
                if d > 0.0 && b.max[axis] <= s.min[axis] + 1e-7 {
                    d = d.min(s.min[axis] - b.max[axis]);
                } else if d < 0.0 && b.min[axis] >= s.max[axis] - 1e-7 {
                    d = d.max(s.max[axis] - b.min[axis]);
                }
            }
            if d != delta[axis] {
                hit[axis] = true;
            }
            b.min[axis] += d;
            b.max[axis] += d;
            moved[axis] = d;
        }
        (b, hit, moved)
    };

    let (mut b, mut hit, _) = try_move(start, delta);
    let falling_or_grounded = delta.y <= 0.0;
    if step_height > 0.0 && (hit[0] || hit[2]) && falling_or_grounded {
        // Retry lifted by the step height, keep it if it gets further horizontally.
        let (up, _, lifted) = try_move(start, DVec3::new(0.0, step_height, 0.0));
        let (side, side_hit, _) = try_move(up, DVec3::new(delta.x, 0.0, delta.z));
        let (down, _, _) = try_move(side, DVec3::new(0.0, -lifted.y + delta.y.min(0.0), 0.0));
        let gained = |a: &Aabb| (a.min.x - start.min.x).powi(2) + (a.min.z - start.min.z).powi(2);
        if gained(&down) > gained(&b) + 1e-6 {
            b = down;
            hit = [side_hit[0], false, side_hit[2]];
        }
    }
    let on_ground = hit[1] && delta.y < 0.0;
    let pos = DVec3::new(b.min.x + half_width, b.min.y, b.min.z + half_width);
    SOLIDS.with_borrow_mut(|s| *s = solids);
    MoveResult { pos, hit, on_ground }
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    /// In the coordinates of `frame`: the moving structure (entity) the block is part of, 0 for
    /// the world.
    pub block: IVec3,
    pub frame: u64,
    /// Face index (-X, +X, -Y, +Y, -Z, +Z) that was entered.
    pub face: u8,
    pub state: u32,
    /// From the ray's start to where it entered the block's selection.
    pub dist: f64,
}

/// The one way to find what a ray from `eye` hits within `reach`: the nearest selection box of
/// the world's blocks and of moving structures (`(entity, where its blocks are, its blocks)`), by
/// where the ray enters it. The client's crosshair, the server's and mods'
/// `look_at` all ask this.
pub fn pick(
    c: &Content,
    world: &impl Cells,
    structures: &[(u64, Transform, &dyn Blocks)],
    eye: DVec3,
    dir: DVec3,
    reach: f64,
    liquids: bool,
) -> Option<Hit> {
    let mut best = raycast(c, world, eye, dir, reach, liquids);
    for &(id, place, blocks) in structures {
        if let Some(mut h) = raycast(c, &BodyCells(blocks), place.to_local(eye), dir, reach, liquids) {
            if best.is_none_or(|b| h.dist < b.dist) {
                h.frame = id;
                best = Some(h);
            }
        }
    }
    best
}

/// Where a ray enters box `b` (0 when it starts inside), within `max`: entities picked by the
/// crosshair.
pub fn ray_aabb(o: DVec3, d: DVec3, b: &Aabb, max: f64) -> Option<f64> {
    let (mut t0, mut t1) = (0.0f64, max);
    for i in 0..3 {
        if d[i].abs() < 1e-12 {
            if o[i] < b.min[i] || o[i] > b.max[i] {
                return None;
            }
            continue;
        }
        let a = (b.min[i] - o[i]) / d[i];
        let c = (b.max[i] - o[i]) / d[i];
        t0 = t0.max(a.min(c));
        t1 = t1.min(a.max(c));
    }
    (t0 <= t1).then_some(t0)
}

/// Amanatides-Woo traversal. Blocks are hit where the ray crosses their selection boxes; of
/// blocks the ray starts inside, only what is solid where it starts (a head stuck in a block).
/// First targetable block along the ray; with `liquids` liquids stop it too.
pub fn raycast(c: &Content, block: &impl Cells, origin: DVec3, dir: DVec3, max: f64, liquids: bool) -> Option<Hit> {
    let dir = dir.normalize();
    let mut p = origin.floor().as_ivec3();
    let step = dir.signum().as_ivec3();
    let inv = DVec3::ONE / dir.abs().max(DVec3::splat(1e-12));
    let mut t_max = DVec3::ZERO;
    for i in 0..3 {
        let o = origin[i] - p[i] as f64;
        t_max[i] = if dir[i] > 0.0 { (1.0 - o) * inv[i] } else { o * inv[i] };
    }
    let mut t = 0.0;
    while t <= max {
        if let Some((main, extras)) = block.cell(p) {
            let local = origin - p.as_dvec3();
            // The nearest of what is in the cell; when asking for liquids, one in it comes first.
            let mut best: Option<Hit> = None;
            for state in std::iter::once(main).chain(extras.iter().copied()) {
                let s = &c.states[state as usize];
                let wanted = liquids && s.liquid.is_some();
                if s.flags & bf::NOT_TARGETABLE != 0 && !wanted {
                    continue;
                }
                let stuck = t == 0.0
                    && s.collision
                        .iter()
                        .any(|b| (0..3).all(|i| (b[i] as f64..b[i + 3] as f64).contains(&local[i])));
                let near = if stuck {
                    // Entered, as it were, through the face it looks most toward.
                    let axis = (0..3)
                        .max_by(|&a, &b| dir[a].abs().total_cmp(&dir[b].abs()))
                        .unwrap_or(0);
                    Some((0.0, (axis * 2) as u8 + (dir[axis] < 0.0) as u8))
                } else {
                    s.selection
                        .iter()
                        .filter_map(|b| ray_box(local, dir, b, max))
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                };
                if let Some((dist, face)) = near {
                    if wanted {
                        return Some(Hit { frame: 0, block: p, face, state, dist });
                    }
                    if best.is_none_or(|b| dist < b.dist) {
                        best = Some(Hit { frame: 0, block: p, face, state, dist });
                    }
                }
            }
            if best.is_some() {
                return best;
            }
        }
        let axis = if t_max.x < t_max.y && t_max.x < t_max.z {
            0
        } else if t_max.y < t_max.z {
            1
        } else {
            2
        };
        t = t_max[axis];
        t_max[axis] += inv[axis];
        p[axis] += step[axis];
    }
    None
}

/// Entry distance and entered face, `None` if missed or if the ray starts inside.
fn ray_box(o: DVec3, d: DVec3, b: &[f32; 6], max: f64) -> Option<(f64, u8)> {
    let mut t0 = f64::NEG_INFINITY;
    let mut t1 = max;
    let mut face = 0u8;
    for i in 0..3 {
        let (lo, hi) = (b[i] as f64, b[i + 3] as f64);
        if d[i].abs() < 1e-12 {
            if o[i] < lo || o[i] > hi {
                return None;
            }
            continue;
        }
        let a = (lo - o[i]) / d[i];
        let c = (hi - o[i]) / d[i];
        let (near, far) = if a < c { (a, c) } else { (c, a) };
        if near > t0 {
            t0 = near;
            // Entering through the low side means the -axis face.
            face = (i * 2) as u8 + if d[i] > 0.0 { 0 } else { 1 };
        }
        t1 = t1.min(far);
    }
    (t0 <= t1 && t0 >= 0.0).then_some((t0, face))
}

/// Whether liquid fills the block `level` above the feet at `pos` (0 the feet, the eye height for
/// eyes): the one question of "in water", asked at the height the caller means.
pub fn in_liquid(c: &Content, block: &impl Cells, pos: DVec3, level: f64) -> bool {
    block
        .liquid(c, (pos + DVec3::Y * level).floor().as_ivec3())
        .is_some()
}

/// What the cells a box of that height stands in do to it, by all that is in them: the one answer
/// for whoever asks about a player or a mob, here or on a client.
#[derive(Clone, Copy, Default)]
pub struct Around {
    /// Liquid at its feet or around the middle of its body.
    pub in_liquid: bool,
    /// Something at its feet holds it up, like a ladder.
    pub climbing: bool,
}

/// How far above its feet "the middle of its body" is, for liquids: a giant's isn't wading.
pub fn body_level(height: f64) -> f64 {
    (height * 0.5).min(1.0)
}

/// Whether `around` is the same at both, the cells it reads being the same: most of what moves
/// a tick stays in them, and thousands of items lie still.
pub fn same_around(a: DVec3, b: DVec3, height: f64) -> bool {
    let mid = DVec3::Y * body_level(height);
    a.floor() == b.floor() && (a + mid).floor() == (b + mid).floor()
}

pub fn around(c: &Content, block: &impl Cells, pos: DVec3, height: f64) -> Around {
    let climbs = |s: u32| c.states[s as usize].flags & bf::CLIMBABLE != 0;
    Around {
        in_liquid: in_liquid(c, block, pos, 0.0) || in_liquid(c, block, pos, body_level(height)),
        climbing: block
            .cell(pos.floor().as_ivec3())
            .is_some_and(|(main, extras)| climbs(main) || extras.iter().any(|&s| climbs(s))),
    }
}

/// Which liquid a point is under, its surface considered: water in a block only fills it up to
/// its level, unless more of the same is on top.
pub fn liquid_at(c: &Content, block: &impl Cells, p: DVec3) -> Option<u16> {
    let at = p.floor().as_ivec3();
    let st = &c.states[block.liquid(c, at)? as usize];
    let liquid = st.liquid?;
    let covered = block
        .liquid(c, at + IVec3::Y)
        .is_some_and(|s| c.states[s as usize].liquid == Some(liquid));
    let surface = if covered { 1.0 } else { (8.0 - st.level as f64) / 9.0 };
    (p.y - at.y as f64 <= surface).then_some(liquid)
}

/// How a walker moves itself this tick, in blocks per tick like vanilla.
#[derive(Clone, Copy, Default)]
pub struct Control {
    /// Where it wants to go, horizontal, length up to 1.
    pub wish: DVec3,
    /// Acceleration on ground of normal friction (a player 0.1, 0.13 sprinting); slippery ground
    /// changes it the way vanilla does, so ice is fast and slides.
    pub ground: f64,
    /// Acceleration in the air.
    pub air: f64,
    /// Jump speed when it jumps off the ground, 0 = not jumping.
    pub jump: f64,
    /// Added to the velocity with a jump (a sprinting player's leap).
    pub jump_boost: DVec3,
    /// Holds itself up in liquids (a swimming mob, a player holding jump).
    pub swim: bool,
    /// Faster and without sinking in liquids (a sprinting player).
    pub sprint: bool,
    /// Swimming: goes where it looks, this being the look's up component.
    pub swim_look: Option<f64>,
    /// Never steps off an edge, and holds on to a ladder (sneaking).
    pub careful: bool,
}

/// The walker's box and the world's pull.
#[derive(Clone, Copy)]
pub struct Frame {
    pub half_width: f64,
    pub height: f64,
    /// Blocks it walks up without jumping (0.6).
    pub step: f64,
    /// Blocks per tick squared.
    pub gravity: f64,
}

pub struct Walked {
    pub hit: [bool; 3],
    pub on_ground: bool,
    pub jumped: bool,
    /// `around` where it started.
    pub started_in: Around,
}

/// One tick of walking, the same for players and mobs: friction (ice), drag (lava, oil),
/// ladders, liquids and steps from the blocks it is in, with vanilla's numbers. `vel` is blocks
/// per tick; a player differs only in where `ctl` comes from.
#[allow(clippy::too_many_arguments)]
pub fn walk(
    c: &Content,
    block: &impl Cells,
    bodies: &[Body<'_>],
    skip: u64,
    pos: &mut DVec3,
    vel: &mut DVec3,
    on_ground: bool,
    ctl: &Control,
    f: &Frame,
) -> Walked {
    let feet = pos.floor().as_ivec3();
    let started_in = around(c, block, *pos, f.height);
    let Around { in_liquid: liquid, climbing } = started_in;
    let drag = block
        .liquid(c, feet)
        .or(block.main(feet))
        .map_or(1.0, |s| c.states[s as usize].drag as f64);
    let slip_at = |y: f64| {
        block
            .main(IVec3::new(feet.x, (y - 0.5).floor() as i32, feet.z))
            .map_or(0.6, |s| c.states[s as usize].friction as f64)
    };
    let accelerate = |vel: &mut DVec3, k: f64| {
        let len2 = ctl.wish.length_squared();
        if len2 > 1e-7 {
            *vel += ctl.wish * (k / len2.sqrt().max(1.0));
        }
    };
    let mut jumped = false;
    if liquid {
        accelerate(vel, 0.02 * drag);
        if ctl.swim {
            vel.y += 0.04;
        }
        if ctl.careful {
            vel.y -= 0.04;
        }
        if let Some(look) = ctl.swim_look {
            let head_under = in_liquid(c, block, *pos, 0.9);
            if look <= 0.0 || ctl.swim || head_under {
                vel.y += (look - vel.y) * if look < -0.2 { 0.085 } else { 0.06 };
            }
        }
    } else {
        let slip = slip_at(pos.y);
        let accel = if on_ground { ctl.ground * (0.216 / (slip * slip * slip)) } else { ctl.air };
        if ctl.jump > 0.0 && on_ground {
            vel.y = ctl.jump;
            *vel += ctl.jump_boost;
            jumped = true;
        }
        accelerate(vel, accel * drag);
        if climbing {
            vel.x = vel.x.clamp(-0.15, 0.15);
            vel.z = vel.z.clamp(-0.15, 0.15);
            vel.y = vel.y.max(if ctl.careful { 0.0 } else { -0.15 });
        }
    }
    // Careful at an edge (vanilla's sneaking): each horizontal component is cut back in 0.05
    // steps while the moved box would have nothing under it, so it slides along the edge.
    if ctl.careful && on_ground {
        let ground = |dx: f64, dz: f64| {
            let area = Aabb::around(*pos + DVec3::new(dx, -0.6, dz), f.half_width, f.height);
            collides(c, block, area)
                || bodies
                    .iter()
                    .any(|b| b.solids(c, area, |s| s.intersects(&area)))
        };
        let shrink = |v: f64| if v.abs() < 0.05 { 0.0 } else { v - 0.05 * v.signum() };
        while vel.x != 0.0 && !ground(vel.x, 0.0) {
            vel.x = shrink(vel.x);
        }
        while vel.z != 0.0 && !ground(0.0, vel.z) {
            vel.z = shrink(vel.z);
        }
        while vel.x != 0.0 && vel.z != 0.0 && !ground(vel.x, vel.z) {
            vel.x = shrink(vel.x);
            vel.z = shrink(vel.z);
        }
    }
    let before = *pos;
    let r = move_box_with(c, block, bodies, skip, *pos, f.half_width, f.height, *vel, if on_ground { f.step } else { 0.0 });
    *pos = r.pos;
    for i in 0..3 {
        if r.hit[i] {
            vel[i] = 0.0;
        }
    }
    if (r.hit[0] || r.hit[2] || ctl.jump > 0.0) && climbing {
        vel.y = 0.2;
    }
    if liquid {
        let keep = if ctl.sprint { 0.9 } else { 0.8 };
        vel.x *= keep;
        vel.z *= keep;
        vel.y *= 0.8;
        if !ctl.sprint {
            vel.y -= f.gravity / 16.0;
        }
        // Up a ledge out of the water, vanilla's little boost at a wall with room above it.
        if r.hit[0] || r.hit[2] {
            let above = Aabb::around(*pos + DVec3::new(vel.x, vel.y + 0.6, vel.z), f.half_width - 1e-3, f.height);
            if !collides(c, block, above) {
                vel.y = 0.3;
            }
        }
    } else {
        let slip = if on_ground { slip_at(before.y) } else { 1.0 };
        vel.y = (vel.y - f.gravity) * 0.98;
        vel.x *= slip * 0.91;
        vel.z *= slip * 0.91;
    }
    Walked { hit: r.hit, on_ground: r.on_ground, jumped, started_in }
}

/// The push two overlapping soft bodies give each other per tick (vanilla's
/// applyEntityCollision), for an offset `d` from the other one: along the larger axis, 0.05.
pub fn nudge(d: DVec3) -> DVec3 {
    let dist = d.x.abs().max(d.z.abs()).max(0.01).sqrt();
    DVec3::new(d.x, 0.0, d.z) * ((1.0 / dist).min(1.0) / dist * 0.05)
}

/// The top of a state's collision boxes in the block (a slab 0.5, a fence 1.5), `None` when
/// nothing there stops a walker.
pub fn collision_top(c: &Content, state: u32) -> Option<f64> {
    c.states[state as usize]
        .collision
        .iter()
        .map(|b| b[4] as f64)
        .reduce(f64::max)
}

#[cfg(test)]
mod knock_tests {
    use super::*;

    #[test]
    fn knock_is_vanillas() {
        let away = DVec3::X;
        // On the ground: half the speed, away and up, never more than that up.
        assert_eq!(knock(DVec3::new(0.2, 0.0, 0.1), away, true), DVec3::new(0.5, 0.4, 0.05));
        assert_eq!(knock(DVec3::new(0.0, 0.3, 0.0), away, true).y, 0.4);
        // In the air it goes on up or down as it was.
        assert_eq!(knock(DVec3::new(0.0, -0.5, 0.0), away, false).y, -0.5);
    }

    #[test]
    fn looks_where_vanilla_does() {
        assert!((look_dir(0.0, 0.0) - DVec3::NEG_Z).length() < 1e-9);
        assert!((look_dir(std::f32::consts::FRAC_PI_2, 0.0) - DVec3::NEG_X).length() < 1e-6);
        assert!((look_dir(0.0, std::f32::consts::FRAC_PI_2) - DVec3::Y).length() < 1e-6);
    }

    #[test]
    fn a_pose_scales_with_the_size() {
        use crate::world::Pose;
        let player = EntitySize { width: 0.6, height: 1.8, eye: 1.62 };
        let d = posed(player, Pose::STAND);
        assert!((d.half_width - 0.3).abs() < 1e-5 && (d.height - 1.8).abs() < 1e-5 && (d.eye - 1.62).abs() < 1e-5);
        let d = posed(player, Pose::CROUCH);
        assert!((d.height - 1.5).abs() < 1e-5 && (d.eye - 1.27).abs() < 1e-5);
        let d = posed(EntitySize { width: 1.2, height: 3.6, eye: 3.24 }, Pose::CROUCH);
        assert!((d.height - 3.0).abs() < 1e-5);
    }
}
