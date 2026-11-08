//! Cubes kept loaded with no player near (`Host::force_load`): a mod's tickets, each a box
//! of cubes kept for so many ticks more unless renewed, as block entities only or all of it. Saved
//! with the world, so a factory goes on after a restart.

use glam::IVec3;
use rustc_hash::{FxHashMap, FxHashSet};
use weft_abi::force_load as fl;

use super::Server;
use crate::content::Content;

#[derive(Clone, Copy, Debug)]
pub struct Ticket {
    pub min: IVec3,
    pub max: IVec3,
    pub mode: u32,
    /// The server tick it runs out at.
    pub until: u64,
}

impl Ticket {
    pub fn contains(&self, p: IVec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    fn cubes(&self) -> i64 {
        let s = (self.max - self.min + IVec3::ONE).as_i64vec3();
        s.x * s.y * s.z
    }
}

/// Most cubes all tickets together keep (`core:max_forced_cubes`).
fn limit() -> i64 {
    crate::config::by_name("core:max_forced_cubes").map_or(4096, |id| crate::config::get(id) as i64)
}

impl Server {
    /// `DimEvent::ForceLoad`.
    pub(super) fn force_load(&mut self, dim: u32, key: u64, min: IVec3, max: IVec3, mode: u32, ticks: u32) {
        let tickets = &mut self.forced[dim as usize];
        if mode == fl::OFF {
            tickets.remove(&key);
        } else {
            let t = Ticket { min, max, mode, until: self.tick + ticks as u64 };
            let others: i64 = self
                .forced
                .iter()
                .enumerate()
                .flat_map(|(d, m)| {
                    m.iter()
                        .filter(move |(k, _)| !(d as u32 == dim && **k == key))
                })
                .map(|(_, t)| t.cubes())
                .sum();
            if others + t.cubes() > limit() {
                log::warn!("force load of {} cubes refused: over core:max_forced_cubes", t.cubes());
                return;
            }
            self.forced[dim as usize].insert(key, t);
        }
        self.publish_forced();
    }

    /// Where generation jobs look to know a cube is still wanted.
    fn publish_forced(&self) {
        *self.shared.forced.write() = self
            .forced
            .iter()
            .enumerate()
            .flat_map(|(d, m)| m.values().map(move |t| (d as u32, t.min, t.max)))
            .collect();
    }

    /// Drops tickets run out; whether any did.
    pub(super) fn expire_forced(&mut self) {
        let now = self.tick;
        let mut gone = false;
        for m in &mut self.forced {
            let before = m.len();
            m.retain(|_, t| t.until > now);
            gone |= m.len() != before;
        }
        if gone {
            self.publish_forced();
        }
    }

    /// Cubes of tickets not loaded yet, to ask generation for.
    pub(super) fn forced_missing(&self, dim: u32, has: impl Fn(IVec3) -> bool) -> Vec<IVec3> {
        let mut out = Vec::new();
        for t in self.forced[dim as usize].values() {
            for y in (t.min.y..=t.max.y).rev() {
                for z in t.min.z..=t.max.z {
                    for x in t.min.x..=t.max.x {
                        let p = IVec3::new(x, y, z);
                        if !has(p) {
                            out.push(p);
                        }
                    }
                }
            }
        }
        out
    }

    /// The cubes of `dim` only block entity tickets keep: their mobs stand still and nothing grows.
    pub(super) fn idle_cubes(&self, dim: u32, near_player: impl Fn(IVec3) -> bool) -> FxHashSet<IVec3> {
        let tickets = &self.forced[dim as usize];
        let mut out = FxHashSet::default();
        for t in tickets.values().filter(|t| t.mode == fl::BLOCK_ENTITIES) {
            for y in t.min.y..=t.max.y {
                for z in t.min.z..=t.max.z {
                    for x in t.min.x..=t.max.x {
                        let p = IVec3::new(x, y, z);
                        if !near_player(p) && !tickets.values().any(|o| o.mode == fl::ALL && o.contains(p)) {
                            out.insert(p);
                        }
                    }
                }
            }
        }
        out
    }

    /// For the world's meta: `<dim name>`, `key mode x0 y0 z0 x1 y1 z1 ticks_left`.
    pub(super) fn forced_specs(&self, c: &Content) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (d, m) in self.forced.iter().enumerate() {
            for (k, t) in m {
                let (a, b) = (t.min, t.max);
                out.push((
                    c.dims[d].name.clone(),
                    format!("{k} {} {} {} {} {} {} {} {}", t.mode, a.x, a.y, a.z, b.x, b.y, b.z, t.until.saturating_sub(self.tick)),
                ));
            }
        }
        out
    }

    /// What `forced_specs` wrote.
    pub(super) fn read_forced(&mut self, c: &Content, specs: &[(String, String)]) {
        for (name, spec) in specs {
            let Some(&d) = c.dim_names.get(name) else { continue };
            let mut words = spec.split_whitespace();
            let Some(k) = words.next().and_then(|w| w.parse::<u64>().ok()) else { continue };
            let v: Vec<i64> = words.filter_map(|w| w.parse().ok()).collect();
            if let [mode, x0, y0, z0, x1, y1, z1, left] = v[..] {
                let t = Ticket {
                    min: IVec3::new(x0 as i32, y0 as i32, z0 as i32),
                    max: IVec3::new(x1 as i32, y1 as i32, z1 as i32),
                    mode: mode as u32,
                    until: self.tick + left.max(0) as u64,
                };
                self.forced[d as usize].insert(k, t);
            }
        }
        self.publish_forced();
    }
}

pub type Tickets = FxHashMap<u64, Ticket>;
