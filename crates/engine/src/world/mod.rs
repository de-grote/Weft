pub mod cube;
pub mod light;
pub mod view;

use glam::{DVec3, IVec3};

pub use cube::Cube;
pub use view::View;

/// Game ticks per second, the unit every duration in ticks is in.
pub const TPS: u32 = 20;
/// One tick in seconds.
pub const TICK: f64 = 1.0 / TPS as f64;

/// A point or vector from the ABI (or an array) as the engine works with it.
pub fn dvec(v: impl Into<[f64; 3]>) -> DVec3 {
    DVec3::from_array(v.into())
}

/// A block position from the ABI (or an array) as the engine works with it.
pub fn ivec(v: impl Into<[i32; 3]>) -> glam::IVec3 {
    glam::IVec3::from_array(v.into())
}

/// How a player holds itself, which decides its box and eyes.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Pose(pub u16);

impl Pose {
    pub const STAND: Pose = Pose(weft_abi::pose::STAND as u16);
    pub const CROUCH: Pose = Pose(weft_abi::pose::CROUCH as u16);
    /// In a liquid, lying along where it looks.
    pub const SWIM: Pose = Pose(weft_abi::pose::SWIM as u16);
    /// Where nothing higher fits: lying flat, with the same low box.
    pub const CRAWL: Pose = Pose(weft_abi::pose::CRAWL as u16);
    /// In bed, on its back.
    pub const SLEEP: Pose = Pose(weft_abi::pose::SLEEP as u16);

    pub fn from_id(v: u32) -> Pose {
        let n = crate::content::try_content().map_or(5, |c| c.game.poses.len());
        if (v as usize) < n { Pose(v as u16) } else { Pose::STAND }
    }

    /// The pose a body takes, a player's and any other's alike: the one held for it (`forced`),
    /// else the first of `pose_order` it `wants` (asked `true`: with its keys); where that doesn't
    /// `fit`, the first of its fallbacks it wants without the keys and that fits, else the last.
    pub fn choose(
        forced: Option<Pose>,
        mut wants: impl FnMut(&crate::registry::PoseDef, bool) -> bool,
        fits: impl Fn(Pose) -> bool,
    ) -> Pose {
        let c = crate::content::content();
        let want = forced.unwrap_or_else(|| {
            c.game
                .pose_order
                .iter()
                .copied()
                .find(|q| wants(q.def(), true))
                .unwrap_or(Pose::STAND)
        });
        if fits(want) {
            return want;
        }
        let back = &want.def().fallback;
        back.iter()
            .copied()
            .find(|&q| wants(q.def(), false) && fits(q))
            .or(back.last().copied())
            .unwrap_or(want)
    }

    /// Before content is frozen (unit tests) only the engine's poses are there.
    pub fn def(self) -> &'static crate::registry::PoseDef {
        let all = match crate::content::try_content() {
            Some(c) => &c.game.poses,
            None => crate::registry::engine_pose_defs(),
        };
        all.get(self.0 as usize).unwrap_or(&all[0])
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct GameMode(pub u16);

impl GameMode {
    pub const SURVIVAL: GameMode = GameMode(weft_abi::game_mode::SURVIVAL as u16);
    pub const CREATIVE: GameMode = GameMode(weft_abi::game_mode::CREATIVE as u16);
    pub const SPECTATOR: GameMode = GameMode(weft_abi::game_mode::SPECTATOR as u16);

    /// Every registered mode, the engine's first.
    pub fn all() -> impl Iterator<Item = GameMode> {
        (0..crate::content::content().game.game_modes.len() as u16).map(GameMode)
    }

    pub fn def(self) -> &'static crate::registry::GameModeDef {
        let all = &crate::content::content().game.game_modes;
        all.get(self.0 as usize).unwrap_or(&all[0])
    }

    /// One of `crate::tag_bits::game_mode`.
    pub fn has(self, flag: u32) -> bool {
        self.def().flags & flag != 0
    }

    /// The name players use: the engine's without `core:`. Before content is frozen (a world's
    /// file written by a unit test), survival's.
    pub fn name(self) -> &'static str {
        let Some(c) = crate::content::try_content() else { return "survival" };
        let n = &c
            .game
            .game_modes
            .get(self.0 as usize)
            .unwrap_or(&c.game.game_modes[0])
            .name;
        n.strip_prefix("core:").unwrap_or(n)
    }

    /// By its name, with or without the engine's `core:`.
    pub fn parse(s: &str) -> Option<GameMode> {
        let modes = &crate::content::try_content()?.game.game_modes;
        modes
            .iter()
            .position(|m| m.name == s || m.name.strip_prefix("core:") == Some(s))
            .map(|i| GameMode(i as u16))
    }

    pub fn from_id(v: u32) -> GameMode {
        let n = crate::content::try_content().map_or(3, |c| c.game.game_modes.len());
        if (v as usize) < n { GameMode(v as u16) } else { GameMode::SURVIVAL }
    }
}

/// `v` in a mount's frame (+Z ahead, +X right) turned into the world's by its yaw.
pub fn turned(v: DVec3, yaw: f32) -> DVec3 {
    let (sy, cy) = (yaw as f64).sin_cos();
    let ahead = DVec3::new(-sy, 0.0, -cy);
    let right = DVec3::new(cy, 0.0, -sy);
    right * v.x + DVec3::Y * v.y + ahead * v.z
}

/// An effect on an entity, as its `EFFECT` component holds it (16 bytes each).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnEffect {
    pub id: u32,
    /// 1 and up.
    pub level: u32,
    /// The tick of its dimension it ends at, `FOREVER` for never.
    pub until: u64,
}

/// `OnEffect::until` of an effect that doesn't end.
pub const FOREVER: u64 = u64::MAX;

/// The entity effects in an `EFFECT` component still on at `now` (`u64::MAX`: all).
pub fn effects_on(bytes: Option<&[u8]>, now: u64) -> impl Iterator<Item = OnEffect> + '_ {
    bytes
        .unwrap_or_default()
        .chunks_exact(16)
        .map(|b| {
            let word = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
            OnEffect { id: word(0), level: word(4).max(1), until: u64::from_le_bytes(b[8..16].try_into().unwrap()) }
        })
        .filter(move |f| now == u64::MAX || f.until > now)
}

pub fn encode_effects(list: &[OnEffect]) -> Box<[u8]> {
    list.iter()
        .flat_map(|f| {
            f.id.to_le_bytes()
                .into_iter()
                .chain(f.level.to_le_bytes())
                .chain(f.until.to_le_bytes())
        })
        .collect()
}

/// An entity's effects going with it to a dimension whose clock is `by` ticks ahead: they end
/// as many ticks from now there as here.
pub fn rebase_effects(comps: &mut [(u32, Box<[u8]>)], effect: u32, by: i64) {
    for (_, v) in comps.iter_mut().filter(|(k, _)| *k == effect) {
        let moved: Vec<OnEffect> = effects_on(Some(v), u64::MAX)
            .filter(|f| f.until != FOREVER)
            .map(|f| OnEffect { until: f.until.saturating_add_signed(by), ..f })
            .chain(effects_on(Some(v), u64::MAX).filter(|f| f.until == FOREVER))
            .collect();
        *v = encode_effects(&moved);
    }
}

#[inline]
pub fn cube_of(p: IVec3) -> IVec3 {
    p >> 4
}

#[inline]
pub fn local_index(p: IVec3) -> usize {
    cube::index((p.x & 15) as usize, (p.y & 15) as usize, (p.z & 15) as usize)
}

pub const DIRS: [IVec3; 6] = {
    let d = weft_abi::FACE_DIRS;
    [
        IVec3::new(d[0][0], d[0][1], d[0][2]),
        IVec3::new(d[1][0], d[1][1], d[1][2]),
        IVec3::new(d[2][0], d[2][1], d[2][2]),
        IVec3::new(d[3][0], d[3][1], d[3][2]),
        IVec3::new(d[4][0], d[4][1], d[4][2]),
        IVec3::new(d[5][0], d[5][1], d[5][2]),
    ]
};

pub use weft_noise::{Rng, mix};

/// The corners of the box around every block that is not air (a structure's body).
pub fn block_bounds<'a>(cubes: impl Iterator<Item = (&'a IVec3, &'a cube::Cube)>) -> Option<(IVec3, IVec3)> {
    let mut out: Option<(IVec3, IVec3)> = None;
    for (&cp, cube) in cubes {
        if cube.uniform_state() == Some(0) {
            continue;
        }
        for i in 0..cube::VOLUME {
            if cube.get(i) != 0 {
                let p = cp * 16 + crate::world::cube::offset(i);
                out = Some(out.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))));
            }
        }
    }
    out
}
