pub mod cube;
pub mod light;
pub mod view;

use glam::IVec3;

pub use cube::Cube;
pub use view::View;

/// Game ticks per second, the unit every duration in ticks is in.
pub const TPS: u32 = 20;
/// One tick in seconds.
pub const TICK: f64 = 1.0 / TPS as f64;

/// A point or vector from the ABI (or an array) as the engine works with it.
pub fn dvec(v: impl Into<[f64; 3]>) -> glam::DVec3 {
    glam::DVec3::from_array(v.into())
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

    /// Takes no damage and needs no food.
    pub fn invulnerable(self) -> bool {
        self.has(crate::tag_bits::game_mode::INVULNERABLE)
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
