//! The shape of redstone wire, the same for the server's simulation and the client's picture. A
//! wire block lies on any of a cell's six faces, a face being named by the direction of the block
//! holding it up. Nothing here knows which way is up, so a build works the same whichever way it
//! is turned.

use glam::IVec3;
use weft_abi::{block_flags as bf, signal_kind as sk};

use crate::content::{Content, StateInfo};
use crate::world::{Cube, DIRS};

/// A direction's name as `facing` properties have it.
pub fn dir_name(d: u8) -> &'static str {
    weft_abi::FACE_NAMES[d as usize % 6]
}

/// The faces a wire state lies on.
pub fn mask(c: &Content, state: u32) -> u8 {
    c.states[state as usize].sides
}

/// Whether wire can't pass through a block on its way past an edge.
pub fn in_the_way(c: &Content, state: u32) -> bool {
    let s = &c.states[state as usize];
    s.flags & bf::SOLID != 0 || !matches!(s.signal.kind, sk::NONE | sk::WIRE)
}

/// Whether wire can lie on a block.
pub fn holds_wire(c: &Content, state: u32) -> bool {
    c.states[state as usize].flags & bf::SOLID != 0
}

/// Whether a block carries power from what feeds it to what touches it. A sink (a lamp) does
/// while it takes that power itself.
pub fn conducts(c: &Content, state: u32) -> bool {
    let s = &c.states[state as usize];
    s.flags & bf::SOLID != 0 && s.flags & bf::SIGNAL_BLOCKER == 0 && matches!(s.signal.kind, sk::NONE | sk::SINK)
}

/// Whether a kind of signal block is a device, something other than wire with a circuit.
pub fn is_device(kind: u32) -> bool {
    !matches!(kind, sk::NONE | sk::WIRE)
}

/// Whether going from one state to the other leaves wire around it as it is: the same block turned
/// the same way (a gate going on or off, water falling a level). Only wire itself changes.
pub fn same_shape(a: &StateInfo, b: &StateInfo) -> bool {
    a.block == b.block && a.signal.kind != sk::WIRE && a.turn == b.turn
}

/// `same_shape` of a device: it keeps its circuit, changing its level.
pub fn same_device(a: &StateInfo, b: &StateInfo) -> bool {
    is_device(a.signal.kind) && is_device(b.signal.kind) && same_shape(a, b)
}

/// The directions in a face mask.
pub fn dirs(mask: u8) -> impl Iterator<Item = u8> {
    (0..6u8).filter(move |d| mask & (1 << d) != 0)
}

/// Whether a cube has wire.
pub fn has_wire(c: &Content, cube: &Cube) -> bool {
    cube.palette()
        .iter()
        .any(|&s| c.states[s as usize].signal.kind == sk::WIRE)
}

/// The cells with wire in the cube at `pos`.
pub fn cells(c: &Content, pos: IVec3, cube: &Cube) -> Vec<IVec3> {
    if !has_wire(c, cube) {
        return Vec::new();
    }
    (0..4096usize)
        .filter(|&i| c.states[cube.get(i) as usize].signal.kind == sk::WIRE)
        .map(|i| pos * 16 + crate::world::cube::offset(i))
        .collect()
}

/// The four directions along a face, which are its edges.
pub fn edges(face: u8) -> impl Iterator<Item = u8> {
    (0..6u8).filter(move |d| d / 2 != face / 2)
}

/// What lies past an edge of a piece of wire.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Link {
    /// Another piece of the same wire: `(cell, face)`.
    Piece(IVec3, u8),
    /// No wire goes on from here.
    End,
    /// Past it isn't loaded.
    Unknown,
}

/// Follows edge `e` of the piece on `face` of `cell`: into the corner on the wall at that edge,
/// on along the same plane, or around the edge of the block it lies on, in that order.
pub fn link(c: &Content, get: &impl Fn(IVec3) -> Option<u32>, cell: IVec3, face: u8, e: u8) -> Link {
    let Some(here) = get(cell) else { return Link::Unknown };
    let block = c.states[here as usize].block;
    if mask(c, here) & (1 << e) != 0 {
        return Link::Piece(cell, e);
    }
    let same = |s: u32, f: u8| c.states[s as usize].block == block && mask(c, s) & (1 << f) != 0;
    let next = cell + DIRS[e as usize];
    let Some(ahead) = get(next) else { return Link::Unknown };
    if same(ahead, face) {
        return Link::Piece(next, face);
    }
    if in_the_way(c, ahead) {
        return Link::End;
    }
    let around = next + DIRS[face as usize];
    let Some(below) = get(around) else { return Link::Unknown };
    if same(below, e ^ 1) {
        return Link::Piece(around, e ^ 1);
    }
    Link::End
}

/// Whether the piece on `face` of a cell touches what is next to that cell in direction `e`: not
/// from the far side of the cell.
pub fn touches(face: u8, e: u8) -> bool {
    e != face ^ 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_are_along_the_face() {
        for f in 0..6u8 {
            let e: Vec<u8> = edges(f).collect();
            assert_eq!(e.len(), 4);
            assert!(!e.contains(&f) && !e.contains(&(f ^ 1)));
        }
    }

    #[test]
    fn a_piece_reaches_all_but_the_far_side() {
        assert!(touches(2, 2), "what holds it up");
        assert!(touches(2, 0), "at an edge");
        assert!(!touches(2, 3), "the cell's far side");
    }
}
