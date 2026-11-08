//! Which way a block faces. Any block may face any of the six directions or only some of them;
//! `.facing(..)` makes a `facing` property whose values are the allowed directions.
//!
//! ```ignore
//! // A furnace: four directions, each lit or not, front towards the player.
//! r.block(&FURNACE).facing(Facings::HORIZONTAL, Toward::Player).prop("lit", ["false", "true"])
//!     .states(|p, s| s.faces(..p.facing()..p.is("lit", "true")..));
//! ```

use crate::abi::{self, BlockPos};

/// A direction, in face order (`-X +X -Y +Y -Z +Z`), so `face()` is the face index blocks and
/// support rules use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    West,
    East,
    Down,
    Up,
    North,
    South,
}

impl Dir {
    pub const ALL: [Dir; 6] = [Dir::West, Dir::East, Dir::Down, Dir::Up, Dir::North, Dir::South];

    pub fn face(self) -> u32 {
        self as u32
    }
    pub fn from_face(face: u32) -> Dir {
        Dir::ALL[(face % 6) as usize]
    }
    pub fn opposite(self) -> Dir {
        Dir::from_face(self.face() ^ 1)
    }
    pub fn offset(self) -> BlockPos {
        crate::abi::FACE_DIRS[self as usize].into()
    }
    /// The next way around, seen from above: north, east, south, west. Up and down stay.
    pub fn clockwise(self) -> Dir {
        match self {
            Dir::North => Dir::East,
            Dir::East => Dir::South,
            Dir::South => Dir::West,
            Dir::West => Dir::North,
            d => d,
        }
    }
    pub fn counter_clockwise(self) -> Dir {
        self.clockwise().opposite()
    }
    pub fn horizontal(self) -> bool {
        !matches!(self, Dir::Down | Dir::Up)
    }
    /// The value of a `facing` property.
    pub fn name(self) -> &'static str {
        crate::abi::FACE_NAMES[self as usize]
    }
    pub fn from_name(name: &str) -> Option<Dir> {
        Dir::ALL.into_iter().find(|d| d.name() == name)
    }
    /// The direction closest to `v` among `allowed`.
    pub fn nearest(v: [f32; 3], allowed: Facings) -> Dir {
        let score = |d: Dir| {
            let o = d.offset();
            o.x as f32 * v[0] + o.y as f32 * v[1] + o.z as f32 * v[2]
        };
        allowed
            .dirs()
            .max_by(|a, b| score(*a).total_cmp(&score(*b)))
            .unwrap_or(Dir::Up)
    }
}

/// The directions a block may face (a bit per face index).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Facings(pub u8);

impl Facings {
    pub const ALL: Facings = Facings(0b11_1111);
    pub const HORIZONTAL: Facings = Facings(0b11_0011);
    pub const VERTICAL: Facings = Facings(0b00_1100);

    pub const fn only(dirs: &[Dir]) -> Facings {
        let mut bits = 0;
        let mut i = 0;
        while i < dirs.len() {
            bits |= 1 << dirs[i] as u8;
            i += 1;
        }
        Facings(bits)
    }
    pub fn allows(self, d: Dir) -> bool {
        self.0 & (1 << d as u8) != 0
    }
    pub fn count(self) -> u32 {
        self.0.count_ones()
    }
    pub fn dirs(self) -> impl Iterator<Item = Dir> {
        Dir::ALL.into_iter().filter(move |&d| self.allows(d))
    }
}

/// How placing chooses the facing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Toward {
    /// The front looks at the player (furnaces, chests, dispensers).
    Player,
    /// Out of the face that was clicked (torches and ladders on walls, logs along it).
    Face,
    /// Where the player looks (an observer watching what it was placed against, stairs).
    Away,
    /// Away from the side of its cell nearest where it was clicked, along the clicked face if
    /// that is a wall (a vertical slab against a wall, on a floor against its near edge).
    Hit,
}

impl Toward {
    /// The facing for a placement: clicked face (0..6), look direction and where it was clicked
    /// (0..1 in its cell). A clicked face the block cannot face (a ladder on a floor) falls back
    /// to the player's look, front towards the player.
    pub fn choose(self, allowed: Facings, face: u32, look: [f32; 3], hit: [f32; 3]) -> Dir {
        if self == Toward::Face && allowed.allows(Dir::from_face(face)) {
            return Dir::from_face(face);
        }
        let at = match self {
            Toward::Away => look,
            Toward::Hit => {
                let axis = face as usize / 2;
                std::array::from_fn(|i| if axis == 1 || i == axis { 0.5 - hit[i] } else { 0.0 })
            }
            _ => [-look[0], -look[1], -look[2]],
        };
        // Horizontal-only blocks go by the horizontal part of the look alone.
        if allowed.0 & Facings::VERTICAL.0 == 0 { Dir::nearest([at[0], 0.0, at[2]], allowed) } else { Dir::nearest(at, allowed) }
    }
}

/// How a block's model is turned (`State::orientation`, one of `abi::Turn`'s 24). A model is
/// written once facing north, its top up; boxes, model files, hitboxes, a cube's faces, liquid
/// walls and support all turn with it. `.facing(..)`, `.axis()` and redstone's `attach` and `front`
/// turn their states by themselves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Orientation(abi::Turn);

impl Orientation {
    pub const NONE: Orientation = Orientation(abi::Turn::NONE);

    /// Its bottom against `attach`, its front to `front`; a front along `attach` takes another way.
    pub fn new(attach: Dir, front: Dir) -> Orientation {
        let front = if attach.face() / 2 == front.face() / 2 { if attach.horizontal() { Dir::Up } else { Dir::North } } else { front };
        let (n, up) = (front.offset(), attach.opposite().offset());
        let east = [n.y * up.z - n.z * up.y, n.z * up.x - n.x * up.z, n.x * up.y - n.y * up.x];
        let east = Dir::ALL
            .into_iter()
            .find(|d| d.offset().to_array() == east)
            .unwrap_or(Dir::East);
        Orientation(abi::Turn { east: east.face() as u8, up: attach.opposite().face() as u8 })
    }

    /// Its front to `facing`, its top up unless the front is (then to the north, under it to the south).
    pub fn front(facing: Dir) -> Orientation {
        let attach = match facing {
            Dir::Up => Dir::North,
            Dir::Down => Dir::South,
            _ => Dir::Down,
        };
        Orientation::new(attach, facing)
    }

    /// Its top to `facing` (a piston's head).
    pub fn up(facing: Dir) -> Orientation {
        Orientation::new(facing.opposite(), if facing.horizontal() { Dir::Up } else { Dir::North })
    }

    /// Upright, its own west against `attach` (a torch on a wall).
    pub fn leaning(attach: Dir) -> Orientation {
        Orientation::new(Dir::Down, attach.clockwise())
    }

    /// Its top along axis `x`, `y` or `z` (a log).
    pub fn axis(axis: &str) -> Orientation {
        match axis {
            "x" => Orientation::new(Dir::West, Dir::North),
            "z" => Orientation::new(Dir::North, Dir::Down),
            _ => Orientation::NONE,
        }
    }

    /// Where its own `d` goes.
    pub fn dir(self, d: Dir) -> Dir {
        Dir::from_face(self.0.face(d.face() as usize) as u32)
    }

    /// Its own direction that goes to `d`.
    pub fn own(self, d: Dir) -> Dir {
        Dir::from_face(self.0.inverse().face(d.face() as usize) as u32)
    }

    pub fn turn(self) -> abi::Turn {
        self.0
    }
}

impl From<abi::Turn> for Orientation {
    fn from(t: abi::Turn) -> Orientation {
        Orientation(t)
    }
}

/// The direction along a face closest to where a player looks.
pub(crate) fn dir_along_face(attach: Dir, look: [f32; 3]) -> Dir {
    let score = |d: Dir| {
        let o = d.offset();
        o.x as f32 * look[0] + o.y as f32 * look[1] + o.z as f32 * look[2]
    };
    Dir::ALL
        .into_iter()
        .filter(|d| d.face() / 2 != attach.face() / 2)
        .max_by(|a, b| score(*a).total_cmp(&score(*b)))
        .unwrap_or(Dir::North)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allowed_directions() {
        let f = Facings::only(&[Dir::Up, Dir::North, Dir::East]);
        assert_eq!(f.count(), 3);
        assert_eq!(f.dirs().collect::<Vec<_>>(), [Dir::East, Dir::Up, Dir::North]);
        assert_eq!(Dir::from_name("north"), Some(Dir::North));
        assert_eq!(Toward::Player.choose(Facings::HORIZONTAL, 3, [0.0, -0.9, -0.4], [0.5; 3]), Dir::South);
        assert_eq!(Toward::Face.choose(f, 3, [0.0, -1.0, 0.0], [0.5; 3]), Dir::Up);
        assert_eq!(Toward::Hit.choose(Facings::HORIZONTAL, 3, [0.0, -1.0, 0.0], [0.9, 0.0, 0.6]), Dir::West);
    }
}

#[cfg(test)]
mod turning_tests {
    use super::*;

    #[test]
    fn turning_keeps_a_box_a_box_and_the_top_away_from_what_holds_it() {
        for attach in Dir::ALL {
            for front in Dir::ALL
                .into_iter()
                .filter(|f| f.face() / 2 != attach.face() / 2)
            {
                let o = Orientation::new(attach, front);
                let (lo, hi) = o.turn().bounds([0.0; 3], [16.0, 2.0, 16.0], 16.0);
                let axis = attach.face() as usize / 2;
                assert_eq!(hi[axis] - lo[axis], 2.0, "{attach:?} {front:?}");
                assert_eq!((o.dir(Dir::Down), o.dir(Dir::North)), (attach, front));
                assert_eq!(o.own(attach), Dir::Down);
            }
        }
        assert_eq!(Orientation::leaning(Dir::South).dir(Dir::West), Dir::South);
        assert_eq!(Orientation::axis("x").dir(Dir::Up).face() / 2, Dir::East.face() / 2);
        assert_eq!(Orientation::axis("z").dir(Dir::Up).face() / 2, Dir::North.face() / 2);
    }

    #[test]
    fn a_box_out_of_its_cell_keeps_its_faces() {
        // Three texels out: as a texel rectangle that must not read as "no face".
        for face in 0..6 {
            let uv = abi::box_face_uv(face, [-3.0, 0.0, -3.0], [2.0, 19.0, 2.0]);
            assert!(uv.iter().all(|v| (0.0..=16.0).contains(v)), "{face}: {uv:?}");
            assert!(uv != abi::UV_NO_FACE);
        }
    }

    #[test]
    fn along_stays_on_the_face() {
        assert_eq!(dir_along_face(Dir::Down, [0.0, -1.0, 0.1]), Dir::South);
        assert_eq!(dir_along_face(Dir::West, [-1.0, 0.9, 0.1]), Dir::Up);
    }

    #[test]
    fn around_and_back() {
        assert_eq!(Dir::North.clockwise(), Dir::East);
        assert_eq!(Dir::West.clockwise(), Dir::North);
        assert_eq!(Dir::Up.clockwise(), Dir::Up);
        assert_eq!(Dir::East.counter_clockwise(), Dir::North);
    }

    #[test]
    fn away_looks_where_the_player_does() {
        let look = [0.0, 0.0, -1.0];
        assert_eq!(Toward::Away.choose(Facings::HORIZONTAL, 0, look, [0.5; 3]), Dir::North);
        assert_eq!(Toward::Player.choose(Facings::HORIZONTAL, 0, look, [0.5; 3]), Dir::South);
    }
}
