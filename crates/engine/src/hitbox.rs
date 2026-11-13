//! What hits land on: an entity's hitbox parts (`abi::Hitboxes`) where they are at a moment.
//! One function of what the server and every client know alike (its place, body and look, its
//! stride by the tick, its pose), so a hit is found the same everywhere without sending poses.
//! Models play no part: a model is only how it looks.

use glam::{DAffine3, DQuat, DVec3};
use weft_abi as abi;

use crate::physics::{self, Aabb};
use crate::world::Pose;

/// Where an entity is and how it holds itself, as hits see it.
#[derive(Clone, Copy)]
pub struct Body {
    pub pos: DVec3,
    pub body_yaw: f32,
    /// The look: yaw and pitch.
    pub yaw: f32,
    pub pitch: f32,
    /// How far it has walked and how wide its stride is now (`model::walk_limbs`, by the tick).
    pub walk: f32,
    pub stride: f32,
    pub size: abi::EntitySize,
    /// Its own height for its kind's: one made bigger has bigger parts.
    pub scale: f64,
    pub pose: Pose,
}

/// One part placed: its id (`abi::hitbox_part`), its box in its own pixels, and from those to
/// the world.
#[derive(Clone, Copy)]
pub struct Placed {
    pub id: u32,
    pub min: DVec3,
    pub max: DVec3,
    pub to_world: DAffine3,
}

/// The stride's beat at `walk`: the stock `walk` clip's, a full step each 1.417 / 0.6 of it.
fn beat(walk: f32) -> f64 {
    walk as f64 * 0.6 / 1.41726 * std::f64::consts::TAU
}

/// Where each part of `h` is on `b`. Standing it fits the box's height; crouching squeezes it
/// down to the lower box; lying, it tips over as the model does.
pub fn place(h: &abi::Hitboxes, b: &Body, out: &mut Vec<Placed>) {
    out.clear();
    let stand = physics::posed(b.size, Pose::STAND).height;
    let now = physics::posed(b.size, b.pose).height;
    let def = b.pose.def();
    let tilt = match def.tilt {
        abi::pose_tilt::ALONG_LOOK => Some(b.pitch as f64 - std::f64::consts::FRAC_PI_2),
        abi::pose_tilt::FACE_DOWN => Some(-std::f64::consts::FRAC_PI_2),
        abi::pose_tilt::ON_BACK => Some(std::f64::consts::FRAC_PI_2),
        _ => None,
    };
    let per_px = b.scale / 16.0;
    let squeeze = if tilt.is_some() { 1.0 } else { now / stand.max(1e-6) };
    // On its back it lies the way it looks, as the model does.
    let yaw = if def.tilt == abi::pose_tilt::ON_BACK { b.yaw } else { b.body_yaw };
    let lying = tilt.map_or(DAffine3::IDENTITY, |a| {
        DAffine3::from_translation(DVec3::Y * now / 2.0)
            * DAffine3::from_rotation_x(a)
            * DAffine3::from_translation(-DVec3::Y * stand / 2.0)
    });
    let body = DAffine3::from_translation(b.pos)
        * DAffine3::from_quat(DQuat::from_rotation_y(yaw as f64))
        * lying
        * DAffine3::from_scale(DVec3::new(per_px, per_px * squeeze, per_px));
    for p in &h.parts {
        let turn = match p.follows {
            abi::hitbox_follows::LOOK => {
                DQuat::from_rotation_y(physics::angle_diff(b.yaw, yaw) as f64) * DQuat::from_rotation_x(b.pitch as f64)
            }
            abi::hitbox_follows::STRIDE => DQuat::from_rotation_x((beat(b.walk) + p.phase as f64).cos() * p.swing as f64 * b.stride as f64),
            _ => DQuat::IDENTITY,
        };
        let pivot = DVec3::from_array(p.pivot.map(f64::from));
        let to_world = body * DAffine3::from_translation(pivot) * DAffine3::from_quat(turn) * DAffine3::from_translation(-pivot);
        out.push(Placed {
            id: abi::hitbox_part_id(&p.name),
            min: DVec3::from_array(p.min.map(f64::from)),
            max: DVec3::from_array(p.max.map(f64::from)),
            to_world,
        });
    }
}

/// The part a ray from `o` along `d` (a unit vector) enters first within `max`: its id, how far,
/// the face it went in by in the part's own frame (-Z its front), and where on the part from its
/// low corner, in blocks.
pub fn ray(parts: &[Placed], o: DVec3, d: DVec3, max: f64) -> Option<(u32, f64, u8, DVec3)> {
    let mut best: Option<(u32, f64, u8, DVec3)> = None;
    for p in parts {
        // In the part's pixels the ray goes by the same steps: its length stays the world's.
        let back = p.to_world.inverse();
        let (lo, ld) = (back.transform_point3(o), back.transform_vector3(d));
        let Some((t, face)) = physics::ray_aabb(lo, ld, &Aabb { min: p.min, max: p.max }, max) else { continue };
        if best.is_none_or(|b| t < b.1) {
            // From its low corner in blocks: the part's pixels times their size.
            let px = p.to_world.matrix3.x_axis.length();
            best = Some((p.id, t, face, (lo + ld * t - p.min) * px));
        }
    }
    best
}

/// What a ray from `o` along `d` (a unit vector) hits of an entity within `max`: one of its
/// parts (`h`), else its box (part 0). The part's id, how far, the face it went in by and where on
/// it from its low corner. Its box, grown by how far parts may reach (arms held out, a stride, a
/// head turned), is looked at first.
pub fn ray_entity(
    h: Option<&abi::Hitboxes>,
    b: &Body,
    o: DVec3,
    d: DVec3,
    max: f64,
    scratch: &mut Vec<Placed>,
) -> Option<(u32, f64, u8, DVec3)> {
    let dims = physics::posed(b.size, b.pose);
    let bx = Aabb::around(b.pos, dims.half_width, dims.height);
    let Some(h) = h else {
        let (t, face) = physics::ray_aabb(o, d, &bx, max)?;
        return Some((0, t, face, o + d * t - bx.min));
    };
    let reach = DVec3::splat(b.size.width.max(b.size.height) as f64 * 0.5 + 0.25);
    physics::ray_aabb(o, d, &Aabb { min: bx.min - reach, max: bx.max + reach }, max)?;
    place(h, b, scratch);
    ray(scratch, o, d, max)
}
