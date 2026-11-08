//! Camera modes (`registry::CameraMode`): which one is on, what it says each frame, the box or
//! the pull-in that keeps it out of blocks, and the eased change from one to the next.

use crate::tag_bits::camera as cf;
use glam::DVec3;
use weft_abi::{self as abi};

use super::Client;
use super::world_render::Camera;
use crate::content::content;
use crate::input::Input;
use crate::net::ToServer;
use crate::physics;
use crate::registry::{CameraMode, camera};

/// Half the size of the box a camera is: what keeps it a little off the faces it touches.
const HALF: f64 = 0.1;

/// Third person's distance behind the eyes, as vanilla's.
const BACK: f32 = 4.0;

/// A spectator's flying speed, blocks a second; sprint doubles it.
const FREE_SPEED: f64 = 10.0;

pub struct Cameras {
    /// The one on now.
    pub mode: u32,
    /// The one the player chose (F5): what a key or the server goes back to.
    pub own: u32,
    /// The server put it in `mode` and holds it there.
    pub locked: bool,
    /// What the mode said last frame, before blocks and the change moved it.
    pose: abi::CameraPose,
    /// Where its box is, for `camera_flags::SOLID`.
    body: DVec3,
    /// Its own look, for `camera_flags::DETACHED`.
    look: (f32, f32),
    vel: DVec3,
    since: f32,
    first: bool,
    /// The change under way: from where, how far along, how long, how eased.
    blend: Option<(Camera, f32, f32, u32)>,
    /// What this frame shows.
    pub shown: Camera,
    /// An ASK mode waiting for the server.
    asked: Option<u32>,
}

impl Default for Cameras {
    fn default() -> Self {
        Cameras {
            mode: camera::FIRST_PERSON,
            own: camera::FIRST_PERSON,
            locked: false,
            pose: abi::CameraPose::DEFAULT,
            body: DVec3::ZERO,
            look: (0.0, 0.0),
            vel: DVec3::ZERO,
            since: 0.0,
            first: true,
            blend: None,
            shown: Camera { pos: DVec3::ZERO, yaw: 0.0, pitch: 0.0, fov: 70.0, bob: (glam::Vec3::ZERO, 0.0, 0.0) },
            asked: None,
        }
    }
}

/// `t` (0..1) eased by `kind` (`abi::easing`).
pub fn ease(kind: u32, t: f32) -> f32 {
    use abi::easing::*;
    use std::f32::consts::PI;
    let t = t.clamp(0.0, 1.0);
    match kind {
        IN => t * t * t,
        OUT => 1.0 - (1.0 - t).powi(3),
        IN_OUT if t < 0.5 => 4.0 * t * t * t,
        IN_OUT => 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0,
        SMOOTH => t * t * (3.0 - 2.0 * t),
        SMOOTHER => t * t * t * (t * (t * 6.0 - 15.0) + 10.0),
        SINE => -((PI * t).cos() - 1.0) / 2.0,
        EXPO_OUT if t >= 1.0 => 1.0,
        EXPO_OUT => 1.0 - 2f32.powf(-10.0 * t),
        BACK_OUT => 1.0 + 2.70158 * (t - 1.0).powi(3) + 1.70158 * (t - 1.0).powi(2),
        ELASTIC_OUT if t <= 0.0 || t >= 1.0 => t,
        ELASTIC_OUT => 2f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * (2.0 * PI / 3.0)).sin() + 1.0,
        BOUNCE_OUT => {
            let (n, d) = (7.5625, 2.75);
            let (t, add) = if t < 1.0 / d {
                (t, 0.0)
            } else if t < 2.0 / d {
                (t - 1.5 / d, 0.75)
            } else if t < 2.5 / d {
                (t - 2.25 / d, 0.9375)
            } else {
                (t - 2.625 / d, 0.984375)
            };
            n * t * t + add
        }
        _ => t,
    }
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    a + super::angle_diff(b, a) * t
}

impl Client {
    pub fn camera_mode(&self) -> &'static CameraMode {
        let c = content();
        &c.game.cameras[(self.cameras.mode as usize).min(c.game.cameras.len() - 1)]
    }

    pub fn camera_has(&self, f: u32) -> bool {
        self.camera_mode().has(f)
    }

    /// Turns `mode` on, the change over `over` (seconds, easing) or as the mode says.
    pub fn set_camera(&mut self, mode: u32, over: Option<(f32, u32)>) {
        let c = content();
        let Some(m) = c.game.cameras.get(mode as usize) else { return };
        if mode == self.cameras.mode {
            return;
        }
        let (secs, easing) = over.unwrap_or((m.transition, m.easing));
        let cams = &mut self.cameras;
        cams.blend = (secs > 0.0).then_some((cams.shown, 0.0, secs, easing));
        (cams.mode, cams.since, cams.first) = (mode, 0.0, true);
    }

    /// The player asks for `mode`: at once, or the server first when it has conditions.
    fn want_camera(&mut self, mode: u32) {
        if self.cameras.locked {
            return;
        }
        if content().game.cameras[mode as usize].when.is_some() {
            self.cameras.asked = Some(mode);
            self.conn.send(ToServer::Camera(mode));
        } else {
            self.set_camera(mode, None);
            // The player's own choice is what keys and the server come back to.
            if content().game.cameras[mode as usize].has(cf::CYCLE) {
                self.cameras.own = mode;
            }
        }
    }

    /// `ToClient::Camera`.
    pub(super) fn camera_from_server(&mut self, mode: u32, locked: bool, transition: f32, easing: u32) {
        let over = transition.is_finite().then_some((transition, easing));
        if mode == u32::MAX {
            self.cameras.locked = false;
            self.set_camera(self.cameras.own, over);
            return;
        }
        let asked = self.cameras.asked.take() == Some(mode);
        if asked && self.cameras.locked {
            return;
        }
        self.cameras.locked = locked;
        self.set_camera(mode, over);
    }

    /// F5 goes through the `CYCLE` modes; a mode's key turns it on and back off.
    pub(super) fn camera_keys(&mut self, input: &Input) {
        if !input.captured || self.cameras.locked {
            return;
        }
        let c = content();
        if input.pressed("core:perspective") {
            let n = c.game.cameras.len() as u32;
            let from = if self.camera_has(cf::CYCLE) { self.cameras.mode } else { self.cameras.own };
            if let Some(next) = (1..=n)
                .map(|i| (from + i) % n)
                .find(|&i| c.game.cameras[i as usize].has(cf::CYCLE))
            {
                self.want_camera(next);
            }
        }
        for (i, m) in c.game.cameras.iter().enumerate() {
            if m.key.as_deref().is_some_and(|k| input.pressed_raw(k)) {
                let i = i as u32;
                if self.cameras.mode == i {
                    let back = self.cameras.own;
                    self.set_camera(back, None);
                } else {
                    self.want_camera(i);
                }
            }
        }
    }

    /// How far back third person sits: further on a mount, by `ride_camera` or its size.
    fn camera_back(&self) -> (u64, f32) {
        let c = content();
        let Some((id, _)) = self.me.vehicle else { return (0, BACK) };
        let extra = self.entities.get(&id).map_or(0.0, |m| {
            let k = &c.game.entities[m.kind as usize];
            if k.ride_camera.is_finite() {
                k.ride_camera
            } else {
                let d = m.dims();
                (d.half_width * 2.0).max(d.height) as f32
            }
        });
        (id, BACK + extra.max(0.0))
    }

    /// The camera for this frame (`cameras.shown`), from the eyes and the mode.
    pub(super) fn update_camera(&mut self, input: &Input, dt: f32) {
        let c = content();
        let m = self.camera_mode();
        let eye = self.camera();
        let cams = &mut self.cameras;
        cams.since += dt;
        let first = std::mem::take(&mut cams.first);
        if first {
            cams.pose = abi::CameraPose {
                pos: cams.shown.pos.to_array().into(),
                yaw: cams.shown.yaw,
                pitch: cams.shown.pitch,
                roll: 0.0,
                fov: 1.0,
            };
            cams.look = (cams.shown.yaw, cams.shown.pitch);
            cams.vel = DVec3::ZERO;
            // Coming from somewhere unset (the first frame of the game), from the eyes.
            if cams.shown.pos == DVec3::ZERO {
                cams.pose.pos = eye.pos.to_array().into();
                cams.look = (eye.yaw, eye.pitch);
            }
            cams.body = DVec3::from_array([cams.pose.pos.x, cams.pose.pos.y, cams.pose.pos.z]);
        }
        let detached = m.has(cf::DETACHED);
        if detached && input.captured {
            let sens = 0.0022 * input.sensitivity;
            cams.look.0 -= input.mouse_delta.x * sens;
            let look = input.mouse_delta.y * sens * if input.invert_y { -1.0 } else { 1.0 };
            cams.look.1 = (cams.look.1 - look).clamp(-1.55, 1.55);
        }
        let (yaw, pitch) = if detached { cams.look } else { (eye.yaw, eye.pitch) };
        let keys = crate::input::Keys { input, ticks: &self.key_ticks, raw: true };
        let eye_v: abi::Vec3 = eye.pos.to_array().into();
        let mut pose = cams.pose;
        let mut anchor = eye_v;
        let cams_since = cams.since;
        let (vehicle, back) = self.camera_back();
        let cams = &mut self.cameras;
        match (m.call, cams.mode) {
            (Some((owner, cb)), _) => {
                let mut args = abi::CameraArgs {
                    keys: keys.ctx(),
                    yaw,
                    pitch,
                    eye: eye_v,
                    eye_yaw: eye.yaw,
                    eye_pitch: eye.pitch,
                    vehicle,
                    back,
                    dt,
                    time: cams_since,
                    first: first as u32,
                    anchor,
                    out: pose,
                };
                crate::host::invoke(owner, cb, &mut args);
                (pose, anchor) = (args.out, args.anchor);
            }
            (None, camera::FREE) => {
                let ahead = physics::look_dir(yaw, pitch);
                let right = DVec3::new(yaw.cos() as f64, 0.0, -yaw.sin() as f64);
                let up = keys.axis("core:jump", "core:sneak");
                let wish = ahead * keys.axis("core:forward", "core:back") as f64
                    + right * keys.axis("core:right", "core:left") as f64
                    + DVec3::Y * up as f64;
                let speed = FREE_SPEED * if keys.state("core:sprint").held != 0 { 2.0 } else { 1.0 };
                let target = wish.normalize_or_zero() * speed;
                // Eases into a speed and out of it, as a spectator's flight.
                cams.vel += (target - cams.vel) * (dt as f64 * 8.0).min(1.0);
                let p = DVec3::new(pose.pos.x, pose.pos.y, pose.pos.z) + cams.vel * dt as f64;
                pose = abi::CameraPose { pos: p.to_array().into(), yaw, pitch, roll: 0.0, fov: 1.0 };
            }
            (None, mode) => {
                let mut cam = abi::CameraPose { pos: eye_v, yaw: eye.yaw, pitch: eye.pitch, roll: 0.0, fov: 1.0 };
                if mode == camera::BEHIND || mode == camera::FRONT {
                    let front = mode == camera::FRONT;
                    if front {
                        cam.yaw += std::f32::consts::PI;
                        cam.pitch = -cam.pitch;
                    }
                    let p = eye.pos - physics::look_dir(cam.yaw, cam.pitch) * back as f64;
                    cam.pos = p.to_array().into();
                }
                pose = cam;
            }
        }
        cams.pose = pose;
        let want = DVec3::new(pose.pos.x, pose.pos.y, pose.pos.z);
        let world = &self.world;
        let pos = if m.has(cf::SOLID) {
            let cells = super::world::SeenCells { world, air_if_missing: false };
            let from = cams.body;
            let from =
                if physics::collides(c, &cells, physics::Aabb::around(from - DVec3::Y * HALF, HALF, HALF * 2.0)) { eye.pos } else { from };
            let r = physics::move_box(c, &cells, from - DVec3::Y * HALF, HALF, HALF * 2.0, want - from, 0.0);
            r.pos + DVec3::Y * HALF
        } else if m.has(cf::CLIP) {
            let cells = super::world::SeenCells { world, air_if_missing: true };
            let from = DVec3::new(anchor.x, anchor.y, anchor.z);
            from + (want - from) * physics::sweep(c, &cells, from, want, HALF)
        } else {
            want
        };
        let cams = &mut self.cameras;
        cams.body = pos;
        // A SOLID camera's mode goes on from where the box stopped.
        if m.has(cf::SOLID) {
            cams.pose.pos = pos.to_array().into();
        }
        let effects = m.has(cf::EFFECTS);
        let mut cam = Camera {
            pos,
            yaw: pose.yaw,
            pitch: pose.pitch,
            fov: if effects { eye.fov } else { eye.fov / self.player.fov_mul / self.inside_fov() } * pose.fov,
            bob: (glam::Vec3::ZERO, pose.roll, 0.0),
        };
        if effects {
            let bob = self.bobbed(cam).bob;
            cam.bob = (bob.0, bob.1 + pose.roll, bob.2);
        }
        let cams = &mut self.cameras;
        if let Some((from, t, secs, easing)) = &mut cams.blend {
            *t += dt;
            let k = ease(*easing, *t / *secs);
            let f = *from;
            if *t >= *secs {
                cams.blend = None;
            } else {
                cam = Camera {
                    pos: f.pos.lerp(cam.pos, k as f64),
                    yaw: lerp_angle(f.yaw, cam.yaw, k),
                    pitch: f.pitch + (cam.pitch - f.pitch) * k,
                    fov: f.fov + (cam.fov - f.fov) * k,
                    bob: (f.bob.0.lerp(cam.bob.0, k), f.bob.1 + (cam.bob.1 - f.bob.1) * k, f.bob.2 + (cam.bob.2 - f.bob.2) * k),
                };
            }
        }
        cams.shown = cam;
    }
}
