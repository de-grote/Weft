//! Light that moves with entities (a torch in hand, a burning zombie, a glowing mob), drawn by the
//! client alone: filled out from each source by the same rule the world's light moves by, kept
//! apart from the light the server sends, and taken with it where cubes are meshed. Nothing of
//! it reaches the server, so it lights what players see, not where mobs spawn or plants grow.

use glam::{DVec3, IVec3};
use rustc_hash::{FxHashMap, FxHashSet};

use super::ClientEntity;
use super::world::ClientWorld;
use crate::content::Content;
use crate::world::{DIRS, cube_of};

/// Sources nearest the camera that are drawn; far ones hardly show.
const MAX_SOURCES: usize = 64;

/// The brighter of two lights, channel by channel: what several sources in one place give.
pub fn brightest(a: [u8; 3], b: [u8; 3]) -> [u8; 3] {
    [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])]
}

/// Light as the world stores it (sky, red, green, blue a byte each) with the entities' `g`
/// where it is brighter.
pub fn with_glow(l: u32, g: [u8; 3]) -> u32 {
    (l & 0xFF) | (1..4).fold(0, |m, k| m | ((l >> (8 * k)) & 0xFF).max(g[k - 1] as u32) << (8 * k))
}

/// Where an entity's light comes from and what it gives off: its own `LIGHT` (`stored`, else its
/// kind's), its effects' (fire's while it burns) and every stack it shows (in hand, worn, a dropped
/// one), the brightest of each channel.
pub fn entity_glow(c: &Content, e: &ClientEntity, now: u64, pos: DVec3, yaw: f32, stored: Option<&[u8]>) -> (DVec3, [u8; 3]) {
    let kind = &c.game.entities[e.kind as usize];
    let own = stored
        .and_then(|b| b.try_into().ok())
        .unwrap_or(c.entity_light(e.kind));
    let effects = e
        .effects
        .iter()
        .filter(|f| f.until > now)
        .map(|f| f.id)
        .chain(e.burning.then_some(weft_abi::entity_effect::BURNING));
    let light = effects
        .filter_map(|f| c.game.entity_effects.get(f as usize))
        .map(|d| d.light)
        .chain(e.displayed().map(|s| c.stack_light(&s)))
        .fold(own, brightest);
    (pos + crate::world::turned(kind.light_at, yaw), light)
}

/// The light sources, and what they light, cell by cell.
#[derive(Default)]
pub struct Glow {
    sources: Vec<(IVec3, [u8; 3])>,
    pub cells: FxHashMap<IVec3, [u8; 3]>,
    /// Cubes with any of it.
    pub cubes: FxHashSet<IVec3>,
    /// A cube it reaches (or could, once loaded) changed: filled out again though no source moved.
    stale: bool,
}

impl Glow {
    /// Cube `cube` arrived or a block in it changed.
    pub fn changed(&mut self, cube: IVec3) {
        self.stale |= self.cubes.contains(&cube)
            || self
                .sources
                .iter()
                .any(|s| (cube_of(s.0) - cube).abs().max_element() <= 1);
    }
}

impl ClientWorld {
    /// Lights from these sources (where, how bright) the ones nearest `eye`; the cubes whose light
    /// that changed are meshed again.
    pub fn set_glow(&mut self, c: &Content, eye: DVec3, mut sources: Vec<(DVec3, [u8; 3])>) {
        sources.retain(|s| s.1 != [0; 3]);
        sources.sort_by(|a, b| {
            (a.0 - eye)
                .length_squared()
                .total_cmp(&(b.0 - eye).length_squared())
        });
        sources.truncate(MAX_SOURCES);
        let mut cells: Vec<(IVec3, [u8; 3])> = sources
            .iter()
            .map(|s| (s.0.floor().as_ivec3(), s.1))
            .collect();
        cells.sort_unstable_by_key(|s| (s.0.x, s.0.y, s.0.z, s.1));
        if cells == self.glow.sources && !self.glow.stale {
            return;
        }
        self.glow.stale = false;
        let mut lit: FxHashMap<IVec3, [u8; 3]> = FxHashMap::default();
        for &(at, l) in &cells {
            self.fill_glow(c, at, l, &mut lit);
        }
        let old = std::mem::take(&mut self.glow.cells);
        let mut changed: FxHashSet<IVec3> = lit
            .iter()
            .filter(|(p, l)| old.get(p) != Some(l))
            .map(|(p, _)| cube_of(*p))
            .collect();
        changed.extend(
            old.keys()
                .filter(|p| !lit.contains_key(p))
                .map(|p| cube_of(*p)),
        );
        self.glow.cubes = lit.keys().map(|p| cube_of(*p)).collect();
        self.glow.cells = lit;
        self.glow.sources = cells;
        self.relit(changed.into_iter().collect());
    }

    /// The world's light at `p` with the entities': what models drawn there are lit by.
    pub fn light_shown(&self, p: IVec3) -> u32 {
        let l = self.light(p);
        self.glow.cells.get(&p).map_or(l, |g| with_glow(l, *g))
    }

    /// Light `l` filled out from `at` as the world's own moves, channel by channel, into `lit`.
    fn fill_glow(&self, c: &Content, at: IVec3, l: [u8; 3], lit: &mut FxHashMap<IVec3, [u8; 3]>) {
        let mut best: FxHashMap<IVec3, [u8; 3]> = FxHashMap::default();
        best.insert(at, l);
        let mut queue = vec![(at, l)];
        while let Some((p, v)) = queue.pop() {
            let from = self.block(p);
            if best.get(&p) != Some(&v) || from == u32::MAX {
                continue;
            }
            // Light leaves the cell it starts in whatever is there: a block entity's own block.
            let from = if p == at { crate::content::AIR } else { from };
            for (dir, d) in DIRS.iter().enumerate() {
                let q = p + *d;
                let to = self.block(q);
                if to == u32::MAX {
                    continue;
                }
                let (tf, tt) = (&c.tight[from as usize], &c.tight[to as usize]);
                let next = [0, 1, 2].map(|i| crate::world::light::passed(tf, tt, dir, v[i], i + 1));
                let was = best.get(&q).copied().unwrap_or([0; 3]);
                if (0..3).any(|i| next[i] > was[i]) {
                    let now = brightest(was, next);
                    best.insert(q, now);
                    queue.push((q, now));
                }
            }
        }
        for (p, v) in best {
            let m = lit.entry(p).or_insert([0; 3]);
            *m = brightest(*m, v);
        }
    }
}
