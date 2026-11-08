//! Riding: entities in the seats of others (`EntityKind::seats`), in chains as long as wanted.
//! A rider goes where its seat is after everything moved; a mob in a seat that steers walks
//! its mount with its own AI, a player in one moves it from its client (`ToServer::MountMove`).

use glam::DVec3;
use weft_abi as abi;

use super::dim::Dim;
use crate::content::Content;

/// How deep a chain of riders goes at most; deeper ones are taken off.
const MOST_RIDING: usize = 32;

/// `v` in a mount's frame (+Z ahead, +X right) turned into the world's by its yaw.
pub fn turned(v: DVec3, yaw: f32) -> DVec3 {
    let (sy, cy) = (yaw as f64).sin_cos();
    let ahead = DVec3::new(-sy, 0.0, -cy);
    let right = DVec3::new(cy, 0.0, -sy);
    right * v.x + DVec3::Y * v.y + ahead * v.z
}

impl Dim {
    /// Where a rider's feet go in seat `seat` of `mount`, if it has that seat.
    pub fn seat_pos(&self, c: &Content, mount: u64, seat: u8) -> Option<DVec3> {
        let m = self.entities.get(mount)?;
        let s = c.game.entities[m.kind as usize].seats.get(seat as usize)?;
        Some(m.pos + turned(s.pos, m.yaw))
    }

    /// Who sits in `seat` of `mount`.
    pub fn rider_in(&self, mount: u64, seat: u8) -> Option<u64> {
        self.entities
            .list
            .iter()
            .find(|e| !e.removed && e.vehicle == Some((mount, seat)))
            .map(|e| e.id)
    }

    /// `rider` is `mount` or somewhere under it: sitting there would make a ring.
    fn carries(&self, rider: u64, mount: u64) -> bool {
        let mut at = Some(mount);
        for _ in 0..MOST_RIDING {
            match at {
                Some(id) if id == rider => return true,
                Some(id) => at = self.entities.get(id).and_then(|e| e.vehicle).map(|v| v.0),
                None => return false,
            }
        }
        true
    }

    /// Puts `rider` in `seat` of `mount` (`None`: the first free one), asking mods first.
    pub fn mount(&mut self, c: &'static Content, rider: u64, mount: u64, seat: Option<u8>) -> bool {
        let (Some(r), Some(m)) = (self.entities.get(rider), self.entities.get(mount)) else { return false };
        if r.removed || m.removed || !r.alive() || self.carries(rider, mount) {
            return false;
        }
        let seats = &c.game.entities[m.kind as usize].seats;
        let free = |s: u8| (s as usize) < seats.len() && self.rider_in(mount, s).is_none_or(|o| o == rider);
        let Some(seat) = (match seat {
            Some(s) => free(s).then_some(s),
            None => (0..seats.len().min(255) as u8).find(|&s| free(s)),
        }) else {
            return false;
        };
        let mut q = abi::Query { entity: rider, other: mount, amount: seat as f64, ..abi::Query::DEFAULT };
        if !self.ask(c, abi::action::MOUNT, &mut q) {
            return false;
        }
        if self.entities.get(rider).and_then(|e| e.vehicle).is_some() {
            self.leave(c, rider);
        }
        let steers = seats[seat as usize].steers;
        let player = self.entities.get(rider).is_some_and(|e| e.player);
        if let Some(e) = self.entities.get_mut(rider) {
            (e.vehicle, e.nav, e.fall, e.vel) = (Some((mount, seat)), None, 0.0, DVec3::ZERO);
        }
        if steers && player {
            if let Some(m) = self.entities.get_mut(mount) {
                (m.driver, m.nav) = (Some(rider), None);
            }
        }
        self.seat_riders(c);
        self.emit_engine(c, abi::action::MOUNT, &abi::Mounting { rider, mount, seat: seat as u32, pad: 0 });
        true
    }

    /// Takes `rider` off what it rides, asking mods first, and puts it where its seat says.
    pub fn dismount(&mut self, c: &'static Content, rider: u64) -> bool {
        let Some((mount, seat)) = self.entities.get(rider).and_then(|e| e.vehicle) else { return false };
        let mut q = abi::Query { entity: rider, other: mount, amount: seat as f64, ..abi::Query::DEFAULT };
        if !self.ask(c, abi::action::DISMOUNT, &mut q) {
            return false;
        }
        let at = self.exit_pos(c, rider, mount, seat);
        self.leave(c, rider);
        if let Some(e) = self.entities.get_mut(rider) {
            e.pos = at;
        }
        // A player's client moves it: it is told where it went.
        if self.entities.get(rider).is_some_and(|e| e.player) {
            self.events
                .push(crate::server::DimEvent::Teleport { from: self.id, id: rider, dim: self.id, pos: at });
        }
        self.emit_engine(c, abi::action::DISMOUNT, &abi::Mounting { rider, mount, seat: seat as u32, pad: 0 });
        true
    }

    /// Everything riding `mount`, the riders of its riders too, nearest first.
    pub fn riders_of(&self, mount: u64) -> Vec<u64> {
        let mut out = vec![mount];
        let mut i = 0;
        while i < out.len() && out.len() <= MOST_RIDING {
            let m = out[i];
            out.extend(
                self.entities
                    .list
                    .iter()
                    .filter(|e| !e.removed && e.vehicle.is_some_and(|v| v.0 == m))
                    .map(|e| e.id),
            );
            i += 1;
        }
        out.remove(0);
        out
    }

    /// Off its seat, without asking or moving it (its mount went, it died, it got on another,
    /// it was teleported away).
    pub fn leave(&mut self, c: &Content, rider: u64) {
        let Some((mount, _)) = self.entities.get_mut(rider).and_then(|e| e.vehicle.take()) else { return };
        // What it is saved and moved between regions with says so too, at once.
        self.entities.comps.remove(&(rider, c.game.field_comps[11]));
        if let Some(m) = self
            .entities
            .get_mut(mount)
            .filter(|m| m.driver == Some(rider))
        {
            m.driver = None;
        }
    }

    /// Where `rider` gets off seat `seat` of `mount`.
    fn exit_pos(&mut self, c: &'static Content, rider: u64, mount: u64, seat: u8) -> DVec3 {
        let sat = self
            .seat_pos(c, mount, seat)
            .or_else(|| self.entities.get(rider).map(|e| e.pos))
            .unwrap_or(DVec3::ZERO);
        let Some(m) = self.entities.get(mount) else { return sat };
        let k = &c.game.entities[m.kind as usize];
        let Some(s) = k.seats.get(seat as usize).copied() else { return sat };
        let (mpos, myaw) = (m.pos, m.yaw);
        let nearest = |d: &Dim, reach: f64| {
            d.free_spot(c, rider, mpos, k.width as f64 / 2.0, reach)
                .unwrap_or(sat)
        };
        match s.exit {
            abi::seat_exit::OFFSET => mpos + turned(s.exit_at, myaw),
            abi::seat_exit::STAY => sat,
            abi::seat_exit::CALL => {
                let first = nearest(self, 2.0);
                let Some((owner, cb)) = k.dismount else { return first };
                let out = first.to_array().into();
                let a = self.call(owner, cb, |w| abi::DismountArgs { world: w, mount, rider, seat: seat as u32, out });
                let p = crate::world::dvec(a.out);
                if p.is_finite() { p } else { first }
            }
            _ => nearest(self, s.exit_at.x.max(1.0)),
        }
    }

    /// The nearest place beside a mount at `center` (half width `half`) where `rider` fits
    /// standing, within `reach` blocks.
    fn free_spot(&self, c: &Content, rider: u64, center: DVec3, half: f64, reach: f64) -> Option<DVec3> {
        let r = self.entities.get(rider)?;
        let d = r.dims(c);
        let block = super::space::CubeCells(&self.space.cubes);
        let fits = |p: DVec3| {
            !crate::physics::collides(c, &block, crate::physics::Aabb::around(p, d.half_width, d.height))
                && crate::physics::collides(c, &block, crate::physics::Aabb::around(p - DVec3::Y * 0.5, d.half_width, 0.4))
        };
        let out = half + d.half_width + 0.1;
        let mut best: Option<(f64, DVec3)> = None;
        for ring in 0..=(reach.ceil() as i32) {
            let r = out + ring as f64;
            for k in 0..16 {
                let a = k as f64 * std::f64::consts::TAU / 16.0;
                for dy in [0.0, 1.0, -1.0, 2.0] {
                    let p = DVec3::new(center.x + a.cos() * r, center.y.floor() + dy, center.z + a.sin() * r);
                    let dist = (p - center).length_squared();
                    if best.is_none_or(|b| dist < b.0) && fits(p) {
                        best = Some((dist, p));
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        best.map(|b| b.1)
    }

    /// Before physics: a mob in a seat that steers walks its mount the way its AI walks it.
    pub fn steer_mounts(&mut self, c: &Content) {
        let mut steered: Vec<(u64, ([f32; 2], f32, bool))> = Vec::new();
        for e in self
            .entities
            .list
            .iter_mut()
            .filter(|e| !e.player && !e.removed)
        {
            let Some((mount, _)) = e.vehicle else { continue };
            if let Some(w) = e.walk.take() {
                steered.push((mount, w));
            }
        }
        for (mount, walk) in steered {
            let steers = self
                .entities
                .get(mount)
                .is_some_and(|m| self.rider_seat_steers(c, m.kind, mount));
            if let Some(m) = self.entities.get_mut(mount).filter(|_| steers) {
                m.walk = Some(walk);
            }
        }
    }

    /// Whether a mob steering `mount` sits in a seat that steers.
    fn rider_seat_steers(&self, c: &Content, kind: u32, mount: u64) -> bool {
        let seats = &c.game.entities[kind as usize].seats;
        self.entities.list.iter().any(|e| {
            !e.player
                && e.vehicle
                    .is_some_and(|(m, s)| m == mount && seats.get(s as usize).is_some_and(|s| s.steers))
        })
    }

    /// After physics: every rider where its seat is now, mounts before what rides them; those
    /// whose mount went (or who died) are off.
    pub fn seat_riders(&mut self, c: &Content) {
        let riders: Vec<u64> = self
            .entities
            .list
            .iter()
            .filter(|e| e.vehicle.is_some() && !e.removed)
            .map(|e| e.id)
            .collect();
        if riders.is_empty() {
            return;
        }
        let mut done: rustc_hash::FxHashSet<u64> = Default::default();
        for &r in &riders {
            // Up to the bottom of its chain, then down again.
            let mut chain = vec![r];
            while let Some((m, _)) = self
                .entities
                .get(*chain.last().unwrap())
                .and_then(|e| e.vehicle)
            {
                if chain.len() > MOST_RIDING || chain.contains(&m) {
                    break;
                }
                chain.push(m);
            }
            for &id in chain.iter().rev() {
                if !done.insert(id) {
                    continue;
                }
                let Some((mount, seat)) = self.entities.get(id).and_then(|e| e.vehicle) else { continue };
                let gone = self
                    .entities
                    .get(mount)
                    .is_none_or(|m| m.removed || !m.alive())
                    || self.entities.get(id).is_some_and(|e| !e.alive());
                let at = (!gone).then(|| self.seat_pos(c, mount, seat)).flatten();
                let Some(at) = at else {
                    self.leave(c, id);
                    continue;
                };
                let (vel, frame) = self
                    .entities
                    .get(mount)
                    .map_or((DVec3::ZERO, None), |m| (m.vel, m.frame));
                if let Some(e) = self.entities.get_mut(id) {
                    (e.pos, e.vel, e.on_ground, e.fall, e.frame) = (at, vel, false, 0.0, frame);
                }
            }
        }
    }

    /// The client of the player steering `mount` moved it.
    pub fn mount_moved(&mut self, c: &'static Content, player: u64, pos: DVec3, yaw: f32, vel: DVec3, on_ground: bool) {
        let Some(mount) = self
            .entities
            .get(player)
            .and_then(|e| e.vehicle)
            .map(|v| v.0)
        else {
            return;
        };
        let Some(m) = self
            .entities
            .get_mut(mount)
            .filter(|m| m.driver == Some(player))
        else {
            return;
        };
        let dy = pos.y - m.pos.y;
        m.moved = (pos - m.pos).with_y(0.0).length() as f32;
        (m.pos, m.yaw, m.vel, m.on_ground) = (pos, yaw, vel, on_ground);
        let k = &c.game.entities[m.kind as usize];
        let fell = super::entity::track_fall(&mut m.fall, dy, on_ground, m.in_liquid);
        let land = fell.and_then(|f| super::entity::fall_damage(k, f));
        if let Some(amount) = land {
            self.damage(c, mount, amount, 0, abi::damage_cause::FALL);
            self.riders_fall(c, mount, amount);
        }
        self.seat_riders(c);
    }

    /// A mount's fall hurts its riders too, when its kind says so (`RIDERS_FALL`).
    pub fn riders_fall(&mut self, c: &'static Content, mount: u64, amount: f32) {
        let Some(m) = self.entities.get(mount) else { return };
        if !c.game.entities[m.kind as usize].is(crate::tag_bits::entity::RIDERS_FALL) {
            return;
        }
        let riders: Vec<u64> = self
            .entities
            .list
            .iter()
            .filter(|e| e.vehicle.is_some_and(|v| v.0 == mount))
            .map(|e| e.id)
            .collect();
        for r in riders {
            self.damage(c, r, amount, 0, abi::damage_cause::FALL);
        }
    }
}
