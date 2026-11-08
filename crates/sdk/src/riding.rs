//! Riding: seats on entity kinds, who steers, how keys move a mount, where riders get off and
//! what they may do up there. All of it is a hook a mod can fill; `steer::*`, `Exit::*` and
//! `RiderRules` are presets over those hooks.
//!
//! ```ignore
//! r.entity(&GONDOLA)
//!     .seat(Seat::at([0.0, 4.2, 0.5]).steers())
//!     .seat(Seat::at([1.0, 4.2, -0.5]).exit(Exit::offset([2.0, 0.0, 0.0])))
//!     .steering(steer::fly(1.0))
//!     .rider_rules(RiderRules::new().builds(|_, _, _| false))
//!     .rideable();
//! ```

use crate::abi::{self, Steer, Vec3};
use crate::client::{Keys, keys};
use crate::{EntityState, World, host};

/// A place on a mount: where a rider's feet go, from the mount's position in its own frame
/// (+Z ahead, +X right, turned with it), whether who sits there steers, where they get off.
#[derive(Clone)]
pub struct Seat {
    pub(crate) desc: abi::Seat,
    pub(crate) exit: Option<ExitFn>,
}

pub(crate) type ExitFn = std::sync::Arc<dyn Fn(&World, u64, u64, Vec3) -> Vec3 + Send + Sync>;

impl Seat {
    pub fn at(pos: impl Into<Vec3>) -> Seat {
        let desc = abi::Seat { pos: pos.into(), steers: false, exit: abi::seat_exit::NEAREST, exit_at: Vec3::new(2.0, 0.0, 0.0) };
        Seat { desc, exit: None }
    }
    /// Who sits here steers: a player with its keys (`steering`), a mob with its AI. With nobody
    /// steering a mount moves by its own AI.
    pub fn steers(mut self) -> Seat {
        self.desc.steers = true;
        self
    }
    /// Where a rider of this seat gets off (`Exit::nearest(2.0)` unless told otherwise).
    pub fn exit(mut self, exit: Exit) -> Seat {
        match exit {
            Exit::Nearest(reach) => (self.desc.exit, self.desc.exit_at) = (abi::seat_exit::NEAREST, Vec3::new(reach, 0.0, 0.0)),
            Exit::Offset(at) => (self.desc.exit, self.desc.exit_at) = (abi::seat_exit::OFFSET, at),
            Exit::Stay => self.desc.exit = abi::seat_exit::STAY,
            Exit::Call(f) => {
                self.desc.exit = abi::seat_exit::CALL;
                self.exit = Some(f);
            }
        }
        self
    }
}

/// Where a rider gets off.
pub enum Exit {
    /// The nearest place beside the mount it fits standing, within this many blocks; none:
    /// where it sat.
    Nearest(f64),
    /// At this offset from the mount, in its frame (a gondola's door).
    Offset(Vec3),
    /// Right where it sat, in the air if need be (a parachute's job then).
    Stay,
    /// Yours: `|w, mount, rider, nearest| ..` gives the place, `nearest` the engine's pick.
    Call(ExitFn),
}

impl Exit {
    pub fn nearest(reach: f64) -> Exit {
        Exit::Nearest(reach)
    }
    pub fn offset(at: impl Into<Vec3>) -> Exit {
        Exit::Offset(at.into())
    }
    pub fn stay() -> Exit {
        Exit::Stay
    }
    pub fn call(f: impl Fn(&World, u64, u64, Vec3) -> Vec3 + Send + Sync + 'static) -> Exit {
        Exit::Call(std::sync::Arc::new(f))
    }
}

/// How a player's keys move a mount: called on that player's client each tick with its keys
/// and the mount as it is, giving what the mount does. `steer::*` are the usual ones; `and`
/// puts two together (a walk with a jump).
pub struct Steering(pub(crate) Box<dyn Fn(&Keys, &EntityState) -> Steer + Send + Sync>);

impl Steering {
    /// Yours from scratch.
    pub fn new(f: impl Fn(&Keys, &EntityState) -> Steer + Send + Sync + 'static) -> Steering {
        Steering(Box::new(f))
    }
    /// Both: the first's movement and facing unless it gives none, the higher jump, flying if
    /// either flies.
    pub fn and(self, other: Steering) -> Steering {
        Steering::new(move |i, m| {
            let (a, b) = ((self.0)(i, m), (other.0)(i, m));
            let moves = a.forward != 0.0 || a.strafe != 0.0;
            Steer {
                forward: if moves { a.forward } else { b.forward },
                strafe: if moves { a.strafe } else { b.strafe },
                speed: if moves { a.speed } else { b.speed },
                yaw: if a.yaw.is_finite() { a.yaw } else { b.yaw },
                jump: a.jump.max(b.jump),
                flies: a.flies | b.flies,
                rise: a.rise + b.rise,
            }
        })
    }
}

/// Ready-made steering. Speeds are times the kind's walking speed.
pub mod steer {
    use super::{Keys, Steering, keys};
    use crate::abi::Steer;

    fn ahead(k: &Keys) -> f32 {
        k.axis(&keys::FORWARD, &keys::BACK)
    }

    fn aside(k: &Keys) -> f32 {
        k.axis(&keys::RIGHT, &keys::LEFT)
    }

    /// W and S go ahead and back, A and D turn it at `degrees` a second (a horse).
    pub fn walk_turn(speed: f32, degrees: f32) -> Steering {
        Steering::new(move |k, m| Steer {
            forward: ahead(k),
            speed,
            // A tick is a twentieth of a second.
            yaw: m.yaw - aside(k) * degrees.to_radians() / 20.0,
            ..Steer::DEFAULT
        })
    }
    /// Faces where its rider looks; the keys walk it that way and to the sides (a pig with a
    /// carrot on a stick, a boat).
    pub fn walk_look(speed: f32) -> Steering {
        Steering::new(move |k, _| Steer { forward: ahead(k), strafe: aside(k), speed, yaw: k.yaw, ..Steer::DEFAULT })
    }
    /// `walk_look` that doesn't turn: A and D go sideways (a cart on a track, a crab).
    pub fn strafe(speed: f32) -> Steering {
        Steering::new(move |k, _| Steer { forward: ahead(k), strafe: aside(k), speed, ..Steer::DEFAULT })
    }
    /// Flies where its rider looks, jump and sneak taking it up and down at `climb` blocks a
    /// second (a happy ghast).
    pub fn fly(speed: f32, climb: f32) -> Steering {
        Steering::new(move |k, _| Steer {
            forward: ahead(k),
            strafe: aside(k),
            speed,
            yaw: k.yaw,
            flies: 1,
            rise: k.axis(&keys::JUMP, &keys::SNEAK) * climb,
            ..Steer::DEFAULT
        })
    }
    /// Jumps at `speed` blocks a second up the moment jump is pressed, from the ground.
    pub fn jump(speed: f32) -> Steering {
        Steering::new(move |k, _| Steer { jump: if k.state(&keys::JUMP).held_ticks == 1 { speed } else { 0.0 }, ..Steer::DEFAULT })
    }
    /// Jumps when jump is let go, the higher the longer it was held, full after `ticks` (a horse).
    pub fn charged_jump(most: f32, ticks: u32) -> Steering {
        Steering::new(move |k, _| {
            let held = k.state(&keys::JUMP).released;
            let share = (held as f32 / ticks.max(1) as f32).min(1.0);
            Steer { jump: if held > 0 { most * share.max(0.2) } else { 0.0 }, ..Steer::DEFAULT }
        })
    }
}

/// What riders of a kind may do, each a test of the world, the mount and the rider, so it can
/// hang on anything (only on the ground, only by day, only its owner). A rule not given lets it.
#[derive(Default)]
pub struct RiderRules {
    pub(crate) hits: Option<RuleFn>,
    pub(crate) builds: Option<RuleFn>,
    pub(crate) hurt_share: f32,
}

pub(crate) type RuleFn = std::sync::Arc<dyn Fn(&World, u64, u64) -> bool + Send + Sync>;

impl RiderRules {
    pub fn new() -> RiderRules {
        RiderRules::default()
    }
    /// Whether a rider may hit (`|w, mount, rider| ..`).
    pub fn hits(mut self, f: impl Fn(&World, u64, u64) -> bool + Send + Sync + 'static) -> Self {
        self.hits = Some(std::sync::Arc::new(f));
        self
    }
    /// Whether a rider may place and break blocks.
    pub fn builds(mut self, f: impl Fn(&World, u64, u64) -> bool + Send + Sync + 'static) -> Self {
        self.builds = Some(std::sync::Arc::new(f));
        self
    }
    /// Share of what hurts the mount that hurts each rider too (a blast under a boat), 0 for
    /// none.
    pub fn hurt_reaches_riders(mut self, share: f32) -> Self {
        self.hurt_share = share;
        self
    }
}

impl World {
    /// Puts `rider` on `mount`, in its first free seat; false when it can't (no free seat, a
    /// mod said no, the mount rides the rider).
    pub fn mount(&self, rider: u64, mount: u64) -> bool {
        unsafe { (host().entity_mount)(self.raw(), rider, mount, u32::MAX) != 0 }
    }
    /// `mount` in seat `seat`.
    pub fn mount_seat(&self, rider: u64, mount: u64, seat: u32) -> bool {
        unsafe { (host().entity_mount)(self.raw(), rider, mount, seat) != 0 }
    }
    /// Takes `rider` off what it rides, where its seat's `Exit` says.
    pub fn dismount(&self, rider: u64) {
        unsafe { (host().entity_dismount)(self.raw(), rider) }
    }
    /// What `rider` sits on, and in which seat.
    pub fn vehicle(&self, rider: u64) -> Option<(u64, u32)> {
        let mut seat = 0;
        let m = unsafe { (host().entity_vehicle)(self.raw(), rider, &mut seat) };
        (m != 0).then_some((m, seat))
    }
    /// Who sits in each seat of `mount`.
    pub fn riders(&self, mount: u64) -> Vec<Option<u64>> {
        let mut out = [0u64; 64];
        let n = unsafe { (host().entity_riders)(self.raw(), mount, out.as_mut_ptr(), 64) } as usize;
        out[..n.min(64)]
            .iter()
            .map(|&id| (id != 0).then_some(id))
            .collect()
    }
}

/// `RiderRules` as what they are underneath: refusals of hits and block changes by riders of
/// `kind`, and hurt passed on to riders.
pub(crate) fn register_rules(r: &mut crate::Registry, kind: &'static crate::EntityKind, rules: RiderRules) {
    use crate::{Verdict, actions, question};
    // The mount `who` rides, if it is of `kind`.
    let mount_of = move |w: &World, who: u64| {
        w.vehicle(who)
            .map(|v| v.0)
            .filter(|&m| w.entity(m).is_some_and(|s| kind.is(s.kind)))
    };
    let deny = |rule: Option<RuleFn>| {
        move |q: &crate::Ask| match &rule {
            Some(f) => match mount_of(&q.world(), q.entity()) {
                Some(m) if !f(&q.world(), m, q.entity()) => Verdict::Deny,
                _ => Verdict::Abstain,
            },
            None => Verdict::Abstain,
        }
    };
    if rules.hits.is_some() {
        r.before_event(&question::ATTACK, deny(rules.hits.clone()));
    }
    if rules.builds.is_some() {
        r.before_event(&actions::PLACE_BLOCK, deny(rules.builds.clone()));
        r.before_event(&actions::BREAK_BLOCK, deny(rules.builds.clone()));
    }
    let share = rules.hurt_share;
    if share > 0.0 {
        r.on_event(&actions::HURT, move |b| {
            let w = b.world();
            for h in b
                .iter()
                .filter(|h| w.entity(h.entity).is_some_and(|s| kind.is(s.kind)))
            {
                for rider in w.riders(h.entity).into_iter().flatten() {
                    w.damage(rider, h.amount * share, h.source, h.cause);
                }
            }
        });
    }
}
