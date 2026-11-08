//! Redstone. The engine runs it like liquids, blocks only say what they are (`abi::SignalDesc`).
//!
//! A connected wire is one network, its power worked out from what feeds it when that changes, so
//! a wire costs nothing per block and a circuit nothing while it isn't changing. Gates act after a
//! delay, from a timing wheel. A tick reads everything as the last one left it and then applies
//! what changed, so no order of going through blocks shows: a build behaves the same however it is
//! turned. Every loop goes through a gate, so a tick always settles. See `wire.rs` for the shape.
//!
//! Power reaches a block two ways. A gate or lever into it powers it strongly: wire touching it
//! takes that power, and so do devices. Wire on it powers it weakly, for devices only, or all
//! wires on one wall would be one wire. What is cut off by unloaded blocks keeps its state.

use std::collections::VecDeque;
use std::sync::Arc;

use glam::{DVec3, IVec3};
use rustc_hash::{FxHashMap, FxHashSet};
use weft_abi as abi;
use weft_abi::block_flags as bf;
use weft_abi::signal_kind as sk;

use super::dim::Dim;
use super::entity::{Entities, Motion};
use super::structure::{Carried, Lifted, Liveness};
use crate::content::{AIR, Content, Signal};
use crate::wire::{self, Link};
use crate::world::{Cube, DIRS, cube_of, local_index};

const NO_NET: u32 = u32::MAX;
/// Ticks ahead the wheel reaches. A gate's delay is below it.
const WHEEL: usize = 32;
const MAX_DELAY: u8 = (WHEEL - 1) as u8;
/// Vanilla's: this many flips in this many ticks burn a torch out, for this long.
const BURN_FLIPS: usize = 8;
const BURN_WINDOW: u64 = 60;
const BURN_TIME: u64 = 160;
const PUSH_LIMIT: usize = 12;
const FLOATER_IDLE: u8 = 0;
const FLOATER_GOING: u8 = 1;
const FLOATER_STUCK: u8 = 2;
/// More than that stuck to a floater and it doesn't go.
const BODY_LIMIT: usize = 4096;
/// Vanilla's: a piston's blocks take two ticks to get there.
const MOVE_TICKS: u8 = 2;

#[derive(Clone, Copy)]
struct WireCell {
    mask: u8,
    level: [u8; 6],
    net: [u32; 6],
}

#[derive(Clone, Default)]
struct Device {
    state: u32,
    /// The level it is going to, and the tick it gets there: the wheel may also hold an entry
    /// for what stood here before.
    pending: Option<(u8, u64)>,
    /// Power into a sink as it was last told.
    input: u8,
    /// An observer saw something, the tick it changed: its delay counts from then.
    triggered: Option<u64>,
    /// When a piston last started pushing something out.
    pushed_at: u64,
    flips: VecDeque<u64>,
    burnt_until: u64,
}

#[derive(Clone, Copy)]
enum Port {
    /// A device next to the piece, its direction toward the piece.
    Direct(IVec3, u8),
    /// A block the piece touches that a device may power strongly.
    Strong(IVec3),
    /// A cell with something of another space in it, its direction toward the piece.
    Outside(IVec3, u8),
}

/// What a cell of one space gives another, per direction out of it: to what reads power, and to
/// wire, which isn't fed by other wire.
#[derive(Clone, Copy, PartialEq, Default)]
pub struct Gives {
    all: [u8; 6],
    to_wire: [u8; 6],
}

/// A device or wire's state on its way with its block to another place, there or in another space.
#[derive(Clone, Default)]
pub struct Moved {
    device: Option<Device>,
    levels: [u8; 6],
}

impl Moved {
    pub(super) fn is_empty(&self) -> bool {
        self.device.is_none() && self.levels == [0; 6]
    }
}

#[derive(Default)]
struct Net {
    pieces: Vec<(IVec3, u8)>,
    /// `adj[start[i]..start[i + 1]]` are the pieces next to piece `i`.
    start: Vec<u32>,
    adj: Vec<u32>,
    ports: Vec<(u32, Port)>,
    level: Vec<u8>,
    next: Vec<u8>,
    /// Pieces with power.
    lit: Vec<u32>,
    /// Part of it may lie in what isn't loaded.
    cut: bool,
}

/// One tick's outcome of a sink or piston, applied once everything has been read.
enum Effect {
    Sink {
        p: IVec3,
        level: u8,
    },
    Piston {
        p: IVec3,
        extend: bool,
    },
    /// Mods run with the rest of the world in place: what came in at each of its own faces.
    Gate {
        p: IVec3,
        inputs: [u8; 6],
    },
    /// A comparator reading a block that has its own idea of it, and what is at its sides.
    Compare {
        p: IVec3,
        behind: IVec3,
        side: u8,
    },
    /// What power a floater has.
    Floater {
        p: IVec3,
        level: u8,
    },
}

#[derive(Default)]
pub struct Signals {
    wires: FxHashMap<IVec3, WireCell>,
    devices: FxHashMap<IVec3, Device>,
    nets: Vec<Option<Net>>,
    free: Vec<u32>,
    /// Block changes not yet looked at: (where, old, new).
    events: Vec<(IVec3, u32, u32)>,
    topology: FxHashSet<IVec3>,
    stale: FxHashSet<u32>,
    dirty_nets: FxHashSet<u32>,
    dirty_devices: FxHashSet<IVec3>,
    wheel: [Vec<IVec3>; WHEEL],
    /// Torches burnt out, looked at until they recover, and pistons that gave way to another,
    /// which may stop wanting the block without anything changing here.
    polled: FxHashSet<IVec3>,
    /// Comparators reading a container: looked at when an inventory changes.
    readers: FxHashSet<IVec3>,
    /// Devices that couldn't act or be sure of what they read, for a cube loading or a piston's
    /// blocks landing to give them another go.
    held: FxHashSet<IVec3>,
    /// Wire and devices per cube, so a cube without any goes quickly.
    per_cube: FxHashMap<IVec3, u32>,
    /// Networks cut off, by the cubes they wait for.
    cut_by_cube: FxHashMap<IVec3, Vec<u32>>,
    /// Observers by the cell they watch.
    watchers: FxHashMap<IVec3, Vec<IVec3>>,
    buckets: [Vec<u32>; 16],
    /// Wire power that changed this tick, for clients: (cell, face, level).
    pub level_changes: Vec<(IVec3, u8, u8)>,
    /// Stands in during a tick, keeping what mods change.
    in_tick: bool,
    /// Pistons at work: their blocks are on their way as a body.
    moving: Vec<Moving>,
    /// Where those blocks are going, which nothing else may take.
    reserved: FxHashSet<IVec3>,
    /// What comes with blocks about to be here.
    pub(super) arriving: FxHashMap<IVec3, Moved>,
    /// Now, for pulses that came along.
    pub(super) now: u64,
    /// Blocks on a piston: circuits go on, but pistons wait to see where they end up.
    pub(super) on_the_way: bool,
    /// What other spaces give at cells of this one: by the space, then the cell, per direction out
    /// of it. A body's in the world, and the world's on a body.
    outside: FxHashMap<u64, FxHashMap<IVec3, Gives>>,
    /// Some output here changed since the other side last looked.
    pub(super) outputs_changed: bool,
    /// Where a piston just put an observer: being moved sets it off, which is what makes a
    /// flying machine go.
    pub(super) moved_observers: FxHashSet<IVec3>,
}

/// A piston's blocks on their way.
struct Moving {
    body: u64,
    piston: IVec3,
    head: IVec3,
    blocks: Vec<Carried>,
    /// It goes back: it is out until the head is in.
    retracting: bool,
    /// The head in the body and what it turns into on the way: the short one while it is inside
    /// the piston, or its rod would stick out of the back.
    head_turns: Option<(IVec3, u32)>,
}

/// The blocks signals look at. What isn't there yet holds things as they are, in the world; a
/// body's blocks are all there, and what it has no cube for is air.
#[derive(Clone, Copy)]
pub struct View<'a> {
    pub cubes: &'a FxHashMap<IVec3, Arc<Cube>>,
    pub on_body: bool,
}

impl View<'_> {
    fn state(&self, p: IVec3) -> Option<u32> {
        match self.cubes.get(&cube_of(p)) {
            Some(cube) => Some(cube.get(local_index(p))),
            None => self.on_body.then_some(AIR),
        }
    }

    fn has(&self, cube: IVec3) -> bool {
        self.on_body || self.cubes.contains_key(&cube)
    }
}

/// What a device gives what is next to it in direction `dir`, wire and devices.
fn direct(s: &Signal, dir: u8) -> u8 {
    if s.gives & (1 << dir) != 0 { s.level } else { 0 }
}

/// What a device gives a block in direction `dir`, strongly: its power goes on through that block.
fn strong(s: &Signal, dir: u8) -> u8 {
    if s.strong & (1 << dir) != 0 { s.level } else { 0 }
}

fn can_give(s: &Signal) -> bool {
    s.gives != 0
}

/// Where a comparator reads what is behind it: opposite its output.
fn rear(s: &Signal) -> u8 {
    s.gives.trailing_zeros() as u8 ^ 1
}

use wire::is_device;

/// What a comparator with `mode` makes of what is behind it and at its sides.
fn compared(flags: u8, rear: u8, side: u8) -> u8 {
    if flags as u32 & abi::signal_flags::SUBTRACT != 0 {
        rear.saturating_sub(side)
    } else if rear >= side {
        rear
    } else {
        0
    }
}

/// What a block riding on a moving one does on coming to a cell with `there` in it.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum RiderMeets {
    Nothing,
    /// That breaks, as under the moving block itself.
    Breaks,
    /// It comes off, which is a way to stop what it was feeding.
    Wall,
}

pub(super) fn rider_meets(c: &Content, there: u32) -> RiderMeets {
    let s = &c.states[there as usize];
    if there == AIR || s.liquid.is_some() || s.flags & bf::REPLACEABLE != 0 || s.signal.kind == sk::WIRE {
        RiderMeets::Nothing
    } else if s.flags & bf::SOLID != 0 {
        RiderMeets::Wall
    } else {
        RiderMeets::Breaks
    }
}

/// The state of a block with its `power` property at `level`.
pub fn with_level(s: &Signal, state: u32, level: u8) -> u32 {
    if s.levels == 0 {
        return state;
    }
    let index = |l: u8| if s.levels == 2 { (l > 0) as u32 } else { l.min(s.levels - 1) as u32 };
    state - index(s.level) * s.stride + index(level) * s.stride
}

impl Signals {
    fn device_signal<'c>(&self, c: &'c Content, p: IVec3) -> Option<&'c Signal> {
        self.devices
            .get(&p)
            .map(|d| &c.states[d.state as usize].signal)
    }

    /// Power a block `b` was given strongly.
    fn strong_in(&self, c: &Content, b: IVec3) -> u8 {
        let mut most = 0;
        for d in 0..6u8 {
            if let Some(s) = self.device_signal(c, b + DIRS[d as usize]) {
                most = most.max(strong(s, d ^ 1));
            }
        }
        most
    }

    /// Power of the wire lying on block `b`; `None`: part of it can't be told.
    fn weak_in(&self, b: IVec3) -> Option<u8> {
        let mut most = 0;
        for d in 0..6u8 {
            if let Some(w) = self.wires.get(&(b + DIRS[d as usize])) {
                let f = d ^ 1;
                if w.mask & (1 << f) != 0 {
                    if self.net_cut(w.net[f as usize]) {
                        return None;
                    }
                    most = most.max(w.level[f as usize]);
                }
            }
        }
        Some(most)
    }

    fn net_cut(&self, id: u32) -> bool {
        id != NO_NET
            && self
                .nets
                .get(id as usize)
                .is_some_and(|n| n.as_ref().is_some_and(|n| n.cut))
    }

    fn outside_gives(&self, n: IVec3, dir: u8) -> u8 {
        self.outside
            .values()
            .filter_map(|m| m.get(&n))
            .map(|g| g.all[dir as usize])
            .max()
            .unwrap_or(0)
    }

    fn outside_gives_wire(&self, n: IVec3, dir: u8) -> u8 {
        self.outside
            .values()
            .filter_map(|m| m.get(&n))
            .map(|g| g.to_wire[dir as usize])
            .max()
            .unwrap_or(0)
    }

    /// Who gives anything here from outside.
    pub(super) fn outside_from(&self) -> Vec<u64> {
        self.outside.keys().copied().collect()
    }

    /// The power a device was last found to have coming in.
    pub(super) fn level_into(&self, p: IVec3) -> Option<u8> {
        self.devices
            .get(&p)
            .map(|d| d.input)
            .filter(|i| *i != u8::MAX)
    }

    /// Power coming into `p` from direction `dir`; `None`: that can't be told, it is cut off.
    fn read(&self, c: &Content, cubes: View, p: IVec3, dir: u8) -> Option<u8> {
        let from_outside = self.outside_gives(p + DIRS[dir as usize], dir ^ 1);
        self.read_here(c, cubes, p, dir)
            .map(|v| v.max(from_outside))
    }

    fn read_here(&self, c: &Content, cubes: View, p: IVec3, dir: u8) -> Option<u8> {
        let n = p + DIRS[dir as usize];
        let toward = dir ^ 1;
        if let Some(w) = self.wires.get(&n) {
            let mut most = 0;
            for f in 0..6u8 {
                if w.mask & (1 << f) != 0 && wire::touches(f, toward) {
                    if self.net_cut(w.net[f as usize]) {
                        return None;
                    }
                    most = most.max(w.level[f as usize]);
                }
            }
            return Some(most);
        }
        let given = self.device_signal(c, n).map_or(0, |s| direct(s, toward));
        let state = cubes.state(n)?;
        if wire::conducts(c, state) {
            // What feeds a block may be on any side of it.
            if DIRS.iter().any(|d| cubes.state(n + *d).is_none()) {
                return None;
            }
            return Some(given.max(self.strong_in(c, n)).max(self.weak_in(n)?));
        }
        Some(given)
    }

    fn mark_cell_nets(&mut self, p: IVec3) {
        if let Some(w) = self.wires.get(&p) {
            for f in 0..6 {
                if w.mask & (1 << f) != 0 && w.net[f] != NO_NET {
                    self.dirty_nets.insert(w.net[f]);
                }
            }
        }
    }

    /// What `cell` gives in each direction out of it, for another space.
    fn gives(&self, c: &Content, cubes: View, cell: IVec3) -> Gives {
        let mut out = Gives::default();
        if let Some(w) = self.wires.get(&cell) {
            for f in 0..6u8 {
                if w.mask & (1 << f) != 0 {
                    for d in (0..6u8).filter(|d| wire::touches(f, *d)) {
                        out.all[d as usize] = out.all[d as usize].max(w.level[f as usize]);
                    }
                }
            }
            return out;
        }
        if let Some(s) = self.device_signal(c, cell) {
            for d in 0..6u8 {
                out.all[d as usize] = direct(s, d);
                out.to_wire[d as usize] = direct(s, d);
            }
        }
        if cubes.state(cell).is_some_and(|s| wire::conducts(c, s)) {
            let strong = self.strong_in(c, cell);
            let all = strong.max(self.weak_in(cell).unwrap_or(0));
            for d in 0..6 {
                out.to_wire[d] = out.to_wire[d].max(strong);
                out.all[d] = out.all[d].max(all);
            }
        }
        out
    }

    /// Everything of this space's signals: wire and devices.
    pub(super) fn cells(&self) -> impl Iterator<Item = IVec3> + '_ {
        self.wires.keys().chain(self.devices.keys()).copied()
    }

    pub(super) fn has_cells(&self) -> bool {
        !self.wires.is_empty() || !self.devices.is_empty()
    }

    /// What this space gives other spaces, where it is `offset` from them.
    pub(super) fn giving(&self, c: &Content, cubes: View, offset: IVec3) -> FxHashMap<IVec3, Gives> {
        let mut out = FxHashMap::default();
        let mut look = |cell: IVec3| {
            let g = self.gives(c, cubes, cell);
            if g != Gives::default() {
                out.insert(cell + offset, g);
            }
        };
        for cell in self.cells() {
            look(cell);
            for d in DIRS {
                look(cell + d);
            }
        }
        out
    }

    /// What this space gives at `cells`, with `offset` taking them from there to here; the
    /// result is by those.
    pub(super) fn giving_at(&self, c: &Content, cubes: View, cells: impl Iterator<Item = IVec3>, offset: IVec3) -> FxHashMap<IVec3, Gives> {
        let mut out = FxHashMap::default();
        for cell in cells {
            let g = self.gives(c, cubes, cell + offset);
            if g != Gives::default() {
                out.insert(cell, g);
            }
        }
        out
    }

    /// The word from space `from` on what it gives at cells here now. Only what differs wakes
    /// anything; a cell starting or stopping to give may join or leave a network.
    pub(super) fn set_outside(&mut self, from: u64, now: FxHashMap<IVec3, Gives>) {
        let news = self.outputs_changed;
        let was = self.outside.remove(&from).unwrap_or_default();
        for (cell, g) in &now {
            match was.get(cell) {
                None => {
                    self.topology.insert(*cell);
                    self.output_changed(*cell);
                }
                Some(old) if old != g => self.output_changed(*cell),
                _ => {}
            }
        }
        for cell in was.keys().filter(|cell| !now.contains_key(cell)) {
            self.topology.insert(*cell);
            self.output_changed(*cell);
        }
        // Not news for the other side, which said it.
        self.outputs_changed = news;
        if !now.is_empty() {
            self.outside.insert(from, now);
        }
    }

    /// Something of another space came to `cell`, or left it.
    pub(super) fn passing(&mut self, cell: IVec3) {
        self.seen(cell, self.now);
    }

    /// What is at `cell` changed at tick `when`, for observers watching it.
    fn seen(&mut self, cell: IVec3, when: u64) {
        if let Some(list) = self.watchers.get(&cell) {
            for o in list.clone() {
                if let Some(d) = self.devices.get_mut(&o) {
                    d.triggered.get_or_insert(when);
                    self.dirty_devices.insert(o);
                }
            }
        }
    }

    /// Observers here watching cells that are not of these blocks, which the world may show
    /// something else at now.
    pub(super) fn watched(&self) -> Vec<IVec3> {
        self.watchers.keys().copied().collect()
    }

    /// What comparators read of the block at `p` may be different now.
    pub fn read_changed(&mut self, c: &Content, p: IVec3) {
        if self.readers.is_empty() {
            return;
        }
        let behind = |s: &Self, r: IVec3| {
            s.device_signal(c, r)
                .and_then(|g| DIRS.get(rear(g) as usize))
                .map(|d| r + *d)
        };
        let those: Vec<IVec3> = self
            .readers
            .iter()
            .copied()
            .filter(|r| behind(self, *r) == Some(p))
            .collect();
        self.dirty_devices.extend(those);
    }

    /// The floaters here, by the way each goes and how fast, and whether it breaks through. `None`
    /// until they have all been told what power they have.
    pub(super) fn floaters(&self, c: &Content) -> Option<Vec<(u8, u8, bool)>> {
        let mut out = Vec::new();
        for d in self.devices.values() {
            let s = &c.states[d.state as usize].signal;
            if s.kind == sk::FLOATER {
                if d.input == u8::MAX {
                    return None;
                }
                out.push((s.front, d.input, s.flags as u32 & abi::signal_flags::BREAKS != 0));
            }
        }
        Some(out)
    }

    /// Its devices that can't be moved just now: an anchor with power.
    pub(super) fn fixed<'a>(&'a self, c: &'a Content) -> impl Iterator<Item = IVec3> + 'a {
        self.devices
            .iter()
            .filter(|(_, d)| c.states[d.state as usize].flags & bf::IMMOVABLE != 0)
            .map(|(p, _)| *p)
    }

    /// What goes with the block at `p` if it moves.
    pub(super) fn carry_out(&self, p: IVec3) -> Moved {
        Moved { device: self.devices.get(&p).cloned(), levels: self.wires.get(&p).map_or([0; 6], |w| w.level) }
    }

    /// A device's output changed: whatever could be reading it looks again.
    fn output_changed(&mut self, p: IVec3) {
        self.outputs_changed = true;
        for d in DIRS {
            let n = p + d;
            if self.devices.contains_key(&n) {
                self.dirty_devices.insert(n);
            }
            self.mark_cell_nets(n);
            // On through a block, to anything touching that.
            for e in DIRS {
                let m = n + e;
                if m != p {
                    if self.devices.contains_key(&m) {
                        self.dirty_devices.insert(m);
                    }
                    self.mark_cell_nets(m);
                }
            }
        }
    }

    /// A piece's power changed: the devices it touches and those around the block it lies on.
    fn piece_changed(&mut self, cell: IVec3, face: u8) {
        self.outputs_changed = true;
        for e in 0..6u8 {
            if wire::touches(face, e) {
                let n = cell + DIRS[e as usize];
                if self.devices.contains_key(&n) {
                    self.dirty_devices.insert(n);
                }
            }
        }
        let on = cell + DIRS[face as usize];
        for d in DIRS {
            if self.devices.contains_key(&(on + d)) {
                self.dirty_devices.insert(on + d);
            }
        }
    }

    fn add_device(&mut self, c: &Content, p: IVec3, state: u32) {
        let s = &c.states[state as usize].signal;
        for d in wire::dirs(s.watches) {
            self.watchers
                .entry(p + DIRS[d as usize])
                .or_default()
                .push(p);
        }
        let triggered = self.moved_observers.remove(&p).then_some(self.now);
        let device = match self.arriving.remove(&p).and_then(|m| m.device) {
            Some(mut d) => {
                d.state = state;
                d.triggered = d.triggered.or(triggered);
                // On its way, and that goes on here.
                if let Some((_, due)) = d.pending.as_mut() {
                    *due = (*due).max(self.now + 1);
                    self.wheel[*due as usize % WHEEL].push(p);
                }
                d
            }
            None => Device { state, input: u8::MAX, triggered, ..Device::default() },
        };
        if self.devices.insert(p, device).is_none() {
            *self.per_cube.entry(p >> 4).or_default() += 1;
        }
        self.dirty_devices.insert(p);
    }

    fn remove_device(&mut self, c: &Content, p: IVec3) {
        if let Some(d) = self.devices.remove(&p) {
            self.count_out(p);
            let s = &c.states[d.state as usize].signal;
            for d in wire::dirs(s.watches) {
                let at = p + DIRS[d as usize];
                if let Some(list) = self.watchers.get_mut(&at) {
                    list.retain(|o| *o != p);
                    if list.is_empty() {
                        self.watchers.remove(&at);
                    }
                }
            }
        }
        self.polled.remove(&p);
        self.readers.remove(&p);
        self.held.remove(&p);
        self.dirty_devices.remove(&p);
    }

    fn count_out(&mut self, p: IVec3) {
        if let Some(n) = self.per_cube.get_mut(&(p >> 4)) {
            *n -= 1;
            if *n == 0 {
                self.per_cube.remove(&(p >> 4));
            }
        }
    }

    fn set_wire(&mut self, p: IVec3, mask: u8) {
        if mask == 0 {
            if let Some(w) = self.wires.remove(&p) {
                self.count_out(p);
                for f in 0..6 {
                    if w.net[f] != NO_NET {
                        self.stale.insert(w.net[f]);
                    }
                    // Or a client keeps it, for when wire is back in that cell.
                    if w.level[f] > 0 {
                        self.level_changes.push((p, f as u8, 0));
                    }
                }
            }
            return;
        }
        if !self.wires.contains_key(&p) {
            *self.per_cube.entry(p >> 4).or_default() += 1;
        }
        // What came with it shows at once, and clients here hear of it.
        let came = if self.wires.contains_key(&p) { None } else { self.arriving.remove(&p) };
        if let Some(m) = &came {
            for f in wire::dirs(mask).filter(|f| m.levels[*f as usize] > 0) {
                self.level_changes.push((p, f, m.levels[f as usize]));
            }
        }
        let w = self
            .wires
            .entry(p)
            .or_insert(WireCell { mask: 0, level: came.map_or([0; 6], |m| m.levels), net: [NO_NET; 6] });
        let gone = w.mask & !mask;
        for f in 0..6 {
            if gone & (1 << f) != 0 {
                if w.net[f] != NO_NET {
                    self.stale.insert(w.net[f]);
                }
                w.net[f] = NO_NET;
                if w.level[f] > 0 {
                    self.level_changes.push((p, f as u8, 0));
                }
                w.level[f] = 0;
            }
        }
        w.mask = mask;
    }

    /// A block changed. Looked at when the tick comes.
    pub fn block_changed(&mut self, c: &Content, p: IVec3, old: u32, new: u32) {
        let relevant = c.states[old as usize].signal.kind != sk::NONE || c.states[new as usize].signal.kind != sk::NONE;
        if relevant || self.in_tick || !self.wires.is_empty() || !self.devices.is_empty() {
            self.events.push((p, old, new));
        }
    }

    fn apply_events(&mut self, c: &Content) {
        for (p, old, new) in std::mem::take(&mut self.events) {
            self.seen(p, self.now);
            let (a, b) = (&c.states[old as usize], &c.states[new as usize]);
            // It may be what a piston couldn't move.
            if a.block != b.block || a.flags != b.flags {
                self.give_another_go();
            }
            self.read_changed(c, p);
            let (ka, kb) = (a.signal.kind, b.signal.kind);
            if ka == sk::WIRE || kb == sk::WIRE {
                self.set_wire(p, wire::mask(c, new));
                self.topology.insert(p);
                if ka == kb {
                    continue;
                }
            }
            if wire::same_device(a, b) {
                if let Some(d) = self.devices.get_mut(&p) {
                    let was = c.states[d.state as usize].signal.level;
                    d.state = new;
                    if was != b.signal.level {
                        self.output_changed(p);
                    }
                    self.dirty_devices.insert(p);
                    continue;
                }
            }
            if is_device(ka) {
                self.remove_device(c, p);
            }
            if is_device(kb) {
                self.add_device(c, p, new);
            }
            if is_device(ka) || is_device(kb) {
                self.topology.insert(p);
                self.output_changed(p);
            } else if wire::conducts(c, old) != wire::conducts(c, new) || wire::in_the_way(c, old) != wire::in_the_way(c, new) {
                self.topology.insert(p);
            }
        }
    }

    pub fn cube_loaded(&mut self, c: &Content, pos: IVec3, cube: &Cube) {
        // Blocks here may be what something next door was waiting to see.
        self.give_another_go();
        self.stale
            .extend(self.cut_by_cube.remove(&pos).into_iter().flatten());
        if !cube
            .palette()
            .iter()
            .any(|&s| c.states[s as usize].signal.kind != sk::NONE)
        {
            return;
        }
        for i in 0..4096usize {
            let state = cube.get(i);
            let kind = c.states[state as usize].signal.kind;
            if kind == sk::NONE {
                continue;
            }
            let p = pos * 16 + crate::world::cube::offset(i);
            if kind == sk::WIRE {
                self.set_wire(p, wire::mask(c, state));
            } else {
                self.add_device(c, p, state);
            }
            self.topology.insert(p);
        }
    }

    fn give_another_go(&mut self) {
        self.dirty_devices.extend(self.held.drain());
    }

    pub fn cube_unloaded(&mut self, c: &Content, pos: IVec3) {
        // The events are for blocks that are gone now, or come again with the cube.
        self.events.retain(|e| e.0 >> 4 != pos);
        if !self.per_cube.contains_key(&pos) {
            return;
        }
        let cells: Vec<IVec3> = self
            .wires
            .keys()
            .filter(|p| **p >> 4 == pos)
            .copied()
            .collect();
        for p in cells {
            self.set_wire(p, 0);
            self.topology.insert(p);
        }
        let gone: Vec<IVec3> = self
            .devices
            .keys()
            .filter(|p| **p >> 4 == pos)
            .copied()
            .collect();
        for p in gone {
            self.remove_device(c, p);
            self.topology.insert(p);
        }
    }

    /// Networks near what changed are worked out again from their pieces.
    fn rebuild(&mut self, c: &Content, cubes: View) {
        if self.topology.is_empty() && self.stale.is_empty() {
            return;
        }
        let mut again: Vec<(IVec3, u8)> = Vec::new();
        for cell in std::mem::take(&mut self.topology) {
            // A device's power reaches wire two blocks away, through the block between.
            for dx in -2i32..=2 {
                for dy in -2i32..=2 {
                    for dz in -2i32..=2 {
                        if dx.abs() + dy.abs() + dz.abs() > 2 {
                            continue;
                        }
                        let q = cell + IVec3::new(dx, dy, dz);
                        if let Some(w) = self.wires.get(&q) {
                            for f in 0..6u8 {
                                if w.mask & (1 << f) != 0 {
                                    if w.net[f as usize] == NO_NET {
                                        again.push((q, f));
                                    } else {
                                        self.stale.insert(w.net[f as usize]);
                                    }
                                }
                            }
                        }
                        if self.devices.contains_key(&q) {
                            self.dirty_devices.insert(q);
                        }
                    }
                }
            }
        }
        for id in std::mem::take(&mut self.stale) {
            let Some(net) = self.nets.get_mut(id as usize).and_then(|n| n.take()) else { continue };
            self.free.push(id);
            self.dirty_nets.remove(&id);
            for (cell, f) in net.pieces {
                if let Some(w) = self.wires.get_mut(&cell) {
                    if w.mask & (1 << f) != 0 {
                        w.net[f as usize] = NO_NET;
                        again.push((cell, f));
                    }
                }
            }
        }
        for (cell, f) in again {
            if self
                .wires
                .get(&cell)
                .is_some_and(|w| w.net[f as usize] == NO_NET && w.mask & (1 << f) != 0)
            {
                self.flood(c, cubes, cell, f);
            }
        }
    }

    fn flood(&mut self, c: &Content, cubes: View, cell: IVec3, face: u8) {
        let id = self.free.pop().unwrap_or_else(|| {
            self.nets.push(None);
            self.nets.len() as u32 - 1
        });
        let get = |p: IVec3| cubes.state(p);
        let mut net = Net::default();
        let mut index: FxHashMap<(IVec3, u8), u32> = FxHashMap::default();
        index.insert((cell, face), 0);
        net.pieces.push((cell, face));
        let mut links: Vec<Vec<u32>> = Vec::new();
        let mut at = 0;
        while at < net.pieces.len() {
            let (p, f) = net.pieces[at];
            at += 1;
            let mut mine = Vec::with_capacity(4);
            for e in wire::edges(f) {
                match wire::link(c, &get, p, f, e) {
                    Link::Piece(q, g) => {
                        let next = net.pieces.len() as u32;
                        let i = *index.entry((q, g)).or_insert_with(|| {
                            net.pieces.push((q, g));
                            next
                        });
                        mine.push(i);
                    }
                    Link::Unknown => net.cut = true,
                    Link::End => {}
                }
            }
            links.push(mine);
        }
        net.start.reserve(links.len() + 1);
        net.start.push(0);
        for l in &links {
            net.adj.extend_from_slice(l);
            net.start.push(net.adj.len() as u32);
        }
        let n = net.pieces.len();
        net.level = vec![0; n];
        net.next = vec![0; n];
        for (i, &(p, f)) in net.pieces.iter().enumerate() {
            // Blocks written without telling this (a structure being built) show up by events.
            let Some(w) = self.wires.get_mut(&p) else { continue };
            w.net[f as usize] = id;
            let level = w.level[f as usize];
            // What it shows until worked out again.
            net.level[i] = level;
            if level > 0 {
                net.lit.push(i as u32);
            }
            for e in 0..6u8 {
                if !wire::touches(f, e) {
                    continue;
                }
                let at = p + DIRS[e as usize];
                if self.outside.values().any(|m| m.contains_key(&at)) {
                    net.ports.push((i as u32, Port::Outside(at, e ^ 1)));
                }
                if self.device_signal(c, at).is_some_and(can_give) {
                    net.ports.push((i as u32, Port::Direct(at, e ^ 1)));
                } else if !self.wires.contains_key(&at) {
                    match cubes.state(at) {
                        Some(state) if wire::conducts(c, state) => {
                            if DIRS.iter().any(|d| cubes.state(at + *d).is_none()) {
                                net.cut = true;
                            }
                            if DIRS
                                .iter()
                                .any(|d| self.device_signal(c, at + *d).is_some_and(can_give))
                            {
                                net.ports.push((i as u32, Port::Strong(at)));
                            }
                        }
                        Some(_) => {}
                        None => net.cut = true,
                    }
                }
            }
        }
        if net.cut {
            // What it waits for: any cube around a piece that isn't there.
            let mut waits: FxHashSet<IVec3> = FxHashSet::default();
            for &(p, _) in &net.pieces {
                for dx in -2..=2 {
                    for dy in -2..=2 {
                        for dz in -2..=2 {
                            let q = p + IVec3::new(dx, dy, dz);
                            if !cubes.has(q >> 4) {
                                waits.insert(q >> 4);
                            }
                        }
                    }
                }
            }
            for cube in waits {
                self.cut_by_cube.entry(cube).or_default().push(id);
            }
        }
        self.nets[id as usize] = Some(net);
        self.dirty_nets.insert(id);
    }

    /// Works out a network's power from what feeds it: every piece has the most any feed leaves
    /// it after the distance, so the order of going through them doesn't show.
    fn recompute(&mut self, c: &Content, id: u32) {
        // Part of what feeds a cut one can't be seen: as it is until it can.
        if self.net_cut(id) {
            return;
        }
        let Some(mut net) = self.nets.get_mut(id as usize).and_then(|n| n.take()) else { return };
        let mut buckets = std::mem::take(&mut self.buckets);
        for &(i, port) in &net.ports {
            let v = match port {
                Port::Direct(at, dir) => self.device_signal(c, at).map_or(0, |s| direct(s, dir)),
                Port::Strong(at) => self.strong_in(c, at),
                Port::Outside(at, dir) => self.outside_gives_wire(at, dir),
            };
            if v > net.next[i as usize] {
                net.next[i as usize] = v;
                buckets[v.min(15) as usize].push(i);
            }
        }
        let mut lit: Vec<u32> = Vec::new();
        for level in (1..=15u8).rev() {
            while let Some(i) = buckets[level as usize].pop() {
                let iu = i as usize;
                if net.next[iu] != level {
                    continue;
                }
                lit.push(i);
                if level > 1 {
                    for k in net.start[iu]..net.start[iu + 1] {
                        let j = net.adj[k as usize] as usize;
                        if net.next[j] < level - 1 {
                            net.next[j] = level - 1;
                            buckets[(level - 1) as usize].push(j as u32);
                        }
                    }
                }
            }
        }
        self.buckets = buckets;
        // Only the pieces that had power and that have it now, however long the wire.
        let old = std::mem::replace(&mut net.lit, lit);
        let changed: Vec<u32> = old
            .iter()
            .copied()
            .filter(|&i| net.next[i as usize] == 0)
            .chain(
                net.lit
                    .iter()
                    .copied()
                    .filter(|&i| net.next[i as usize] != net.level[i as usize]),
            )
            .collect();
        for i in changed {
            let iu = i as usize;
            let (cell, face) = net.pieces[iu];
            let level = net.next[iu];
            net.level[iu] = level;
            if let Some(w) = self.wires.get_mut(&cell) {
                w.level[face as usize] = level;
            }
            self.level_changes.push((cell, face, level));
            self.piece_changed(cell, face);
            // Wire's power is part of what it is, as an observer sees it.
            self.seen(cell, self.now);
        }
        for &i in &net.lit {
            net.next[i as usize] = 0;
        }
        self.nets[id as usize] = Some(net);
    }

    fn schedule(&mut self, p: IVec3, level: u8, delay: u8, now: u64) {
        let Some(d) = self.devices.get_mut(&p) else { return };
        if d.pending.is_none() {
            let due = now + delay.clamp(1, MAX_DELAY) as u64;
            d.pending = Some((level, due));
            self.wheel[due as usize % WHEEL].push(p);
        }
    }

    fn fullness(&self, c: &Content, entities: &Entities, frame: u64, at: IVec3) -> Option<u8> {
        let id = entities.anchored(frame, at)?;
        let items = entities.inventories.get(&id)?;
        if items.is_empty() {
            return Some(0);
        }
        let mut sum = 0.0f32;
        let mut any = false;
        for s in items.iter().filter(|s| !s.is_empty()) {
            any = true;
            sum += s.count as f32
                / c.items
                    .get(s.item as usize)
                    .map_or(64, |i| i.max_stack)
                    .max(1) as f32;
        }
        Some(if any { 1 + (sum / items.len() as f32 * 14.0) as u8 } else { 0 })
    }

    /// What a device should do, from what is around it now.
    fn evaluate(&mut self, c: &'static Content, d: &Dim, p: IVec3, now: u64, effects: &mut Vec<Effect>) {
        let Some(dev) = self.devices.get(&p) else { return };
        let state = dev.state;
        let s = c.states[state as usize].signal;
        // What can't be told keeps things as they are.
        macro_rules! read {
            ($dir:expr) => {
                match self.read(c, d.signal_view(), p, $dir) {
                    Some(v) => v,
                    None => {
                        self.held.insert(p);
                        return;
                    }
                }
            };
        }
        // The most of what comes in at these faces.
        macro_rules! most {
            ($mask:expr) => {{
                let mut most = 0;
                for dir in wire::dirs($mask) {
                    most = most.max(read!(dir));
                }
                most
            }};
        }
        match s.kind {
            sk::TORCH => {
                let input = most!(s.inputs);
                let burnt = self.devices[&p].burnt_until > now;
                if burnt {
                    self.polled.insert(p);
                } else if self.devices[&p].burnt_until != 0 {
                    self.polled.remove(&p);
                    if let Some(dev) = self.devices.get_mut(&p) {
                        dev.burnt_until = 0;
                        dev.flips.clear();
                    }
                }
                let want = if input == 0 && !burnt { 15 } else { 0 };
                if want != s.level {
                    self.schedule(p, want, s.delay, now);
                }
            }
            sk::REPEATER => {
                // A gate pointing into a lock holds it where it is.
                if wire::dirs(s.locks).any(|side| d.signal_view().state(p + DIRS[side as usize]).is_none()) {
                    self.held.insert(p);
                    return;
                }
                let locked = wire::dirs(s.locks).any(|side| {
                    self.device_signal(c, p + DIRS[side as usize])
                        .is_some_and(|g| matches!(g.kind, sk::REPEATER | sk::COMPARATOR) && direct(g, side ^ 1) > 0)
                });
                if !locked {
                    let want = if most!(s.inputs) > 0 { 15 } else { 0 };
                    if want != s.level {
                        self.schedule(p, want, s.delay, now);
                    }
                }
            }
            sk::COMPARATOR => {
                let behind = p + DIRS[rear(&s) as usize];
                let reads_itself = d
                    .get(behind)
                    .is_some_and(|b| c.block_of(b).cb.signal_read.is_some());
                let container = self.fullness(c, &d.entities, d.frame, behind);
                if reads_itself || container.is_some() {
                    self.readers.insert(p);
                } else {
                    self.readers.remove(&p);
                }
                let side = most!(s.inputs & !(1 << rear(&s)));
                if reads_itself {
                    effects.push(Effect::Compare { p, behind, side });
                    return;
                }
                let rear = match container {
                    Some(v) => v,
                    None => read!(rear(&s)),
                };
                let want = compared(s.flags, rear, side);
                if want != s.level {
                    self.schedule(p, want, s.delay, now);
                }
            }
            sk::OBSERVER => {
                let triggered = std::mem::take(&mut self.devices.get_mut(&p).expect("just read").triggered);
                if s.level > 0 {
                    self.schedule(p, 0, s.delay, now);
                } else if let Some(when) = triggered {
                    let due = (when + s.delay as u64).max(now + 1);
                    self.schedule(p, 15, (due - now) as u8, now);
                }
            }
            sk::GATE => {
                // By its own faces, as its model is written.
                let back = c.states[state as usize].turn.inverse();
                let mut inputs = [0u8; 6];
                for dir in wire::dirs(s.inputs) {
                    inputs[back.face(dir as usize)] = read!(dir);
                }
                effects.push(Effect::Gate { p, inputs });
            }
            sk::SINK => {
                let most = most!(s.inputs);
                if self.devices[&p].input != most {
                    effects.push(Effect::Sink { p, level: most });
                }
            }
            sk::FLOATER => {
                self.polled.remove(&p);
                effects.push(Effect::Floater { p, level: most!(s.inputs) });
            }
            sk::PISTON => {
                self.polled.remove(&p);
                let most = most!(s.inputs);
                if (most > 0) != (s.level > 0) {
                    effects.push(Effect::Piston { p, extend: most > 0 });
                }
            }
            _ => {}
        }
    }

    /// Blocks are on their way to `p`.
    pub fn reserved(&self, p: IVec3) -> bool {
        self.reserved.contains(&p)
    }

    fn idle(&self, now: u64) -> bool {
        self.events.is_empty()
            && self.topology.is_empty()
            && self.stale.is_empty()
            && self.dirty_nets.is_empty()
            && self.dirty_devices.is_empty()
            && self.polled.is_empty()
            && self.wheel[now as usize % WHEEL].is_empty()
    }

    /// The strongest power coming into a cell from around it, for mods.
    pub fn power_at(&self, c: &Content, cubes: View, p: IVec3) -> u8 {
        let mut most = self
            .wires
            .get(&p)
            .map_or(0, |w| w.level.iter().copied().max().unwrap_or(0));
        for dir in 0..6u8 {
            most = most.max(self.read(c, cubes, p, dir).unwrap_or(0));
        }
        most
    }

    /// Wire power in a cube, for a client getting it: (cell, face, level).
    pub fn levels_in(&self, c: &Content, pos: IVec3, cube: &Cube) -> Vec<(IVec3, u8, u8)> {
        let mut out = Vec::new();
        for p in wire::cells(c, pos, cube) {
            if let Some(w) = self.wires.get(&p) {
                out.extend(
                    (0..6u8)
                        .filter(|f| w.level[*f as usize] > 0)
                        .map(|f| (p, f, w.level[f as usize])),
                );
            }
        }
        out
    }
}

/// A piston's move, worked out before any of them happens.
struct PushPlan {
    p: IVec3,
    extend: bool,
    dir: u8,
    /// The blocks that move, one step along `step`.
    moved: Vec<IVec3>,
    step: IVec3,
    /// What is in the way and gets broken.
    broken: Vec<IVec3>,
    /// What stands on those and comes along: where, and what of it (a cell of wire may have
    /// pieces on what stays).
    riding: Vec<(IVec3, u32)>,
}

impl Dim {
    pub fn signal_view(&self) -> View<'_> {
        View { cubes: &self.space.cubes, on_body: self.frame != 0 }
    }

    /// A change of a gate's look that nothing around it needs to hear: no lighting, no neighbors.
    fn set_signal_state(&mut self, c: &'static Content, p: IVec3, old: u32, new: u32) {
        if c.states[old as usize].light != c.states[new as usize].light {
            self.set(c, p, new);
            return;
        }
        let cp = cube_of(p);
        let Some(cube) = self.space.cubes.get_mut(&cp) else { return };
        Arc::make_mut(cube).set(local_index(p), new);
        self.space.changes.push((p, new));
        self.space.modified.insert(cp);
        self.space.edited.insert(cp);
    }

    pub fn signal_tick(&mut self, c: &'static Content) {
        let now = self.time;
        self.space.signals.now = now;
        if !self.space.signals.readers.is_empty() && !self.entities.dirty_inv.is_empty() {
            let frame = self.frame;
            let changed: Vec<IVec3> = self
                .entities
                .dirty_inv
                .iter()
                .filter_map(|id| {
                    self.entities
                        .get(*id)
                        .filter(|e| e.block_frame() == frame)
                        .and_then(|e| e.anchor)
                })
                .collect();
            for p in changed {
                self.space.signals.read_changed(c, p);
            }
        }
        if self.space.signals.idle(now) {
            return;
        }
        let mut s = std::mem::take(&mut self.space.signals);
        self.space.signals.in_tick = true;
        s.apply_events(c);
        s.rebuild(c, self.signal_view());
        // What was on its way comes out now.
        for p in std::mem::take(&mut s.wheel[now as usize % WHEEL]) {
            let Some(dev) = s.devices.get_mut(&p) else { continue };
            let Some((level, _)) = dev.pending.filter(|(_, due)| *due == now) else { continue };
            dev.pending = None;
            let old = dev.state;
            let info = c.states[old as usize].signal;
            let new = with_level(&info, old, level);
            dev.state = new;
            if info.kind == sk::TORCH {
                dev.flips.push_back(now);
                while dev
                    .flips
                    .front()
                    .is_some_and(|t| now.saturating_sub(*t) > BURN_WINDOW)
                {
                    dev.flips.pop_front();
                }
                if dev.flips.len() >= BURN_FLIPS {
                    dev.burnt_until = now + BURN_TIME;
                }
            }
            if new != old {
                self.set_signal_state(c, p, old, new);
                s.events.push((p, old, new));
                s.output_changed(p);
            }
            s.dirty_devices.insert(p);
        }
        if now % 2 == 0 {
            s.dirty_devices.extend(s.polled.iter().copied());
        }
        // Wires, from gates as they are now. Nothing here acts back on them in this tick.
        let nets: Vec<u32> = s.dirty_nets.drain().collect();
        for id in nets {
            s.recompute(c, id);
        }
        // Then everything reads what that came to. Sorted for mods' callbacks to run the same way
        // each time; no outcome depends on it.
        let mut dirty: Vec<IVec3> = s.dirty_devices.drain().collect();
        dirty.sort_unstable_by_key(|p| p.to_array());
        let mut effects = Vec::new();
        for p in dirty {
            s.evaluate(c, self, p, now, &mut effects);
        }
        // What mods changed while the tick was going.
        let during = std::mem::replace(&mut self.space.signals, s);
        self.space.signals.events.extend(during.events);
        self.signal_effects(c, effects);
        // What changed in this tick is looked at in the next, but observers count from now.
        let s = &mut self.space.signals;
        for i in 0..s.events.len() {
            s.seen(s.events[i].0, now);
        }
    }

    fn signal_effects(&mut self, c: &'static Content, effects: Vec<Effect>) {
        let mut plans = Vec::new();
        let mut floaters = Vec::new();
        for e in effects {
            match e {
                Effect::Sink { p, level } => {
                    let Some(dev) = self.space.signals.devices.get_mut(&p) else { continue };
                    // One just placed or loaded has nothing to tell yet.
                    let was = std::mem::replace(&mut dev.input, level);
                    let state = dev.state;
                    let info = c.states[state as usize].signal;
                    let new = with_level(&info, state, level);
                    if new != state {
                        self.set(c, p, new);
                    }
                    let block = c.block_of(state);
                    if let (Some((owner, cb)), true) = (block.cb.on_signal, was != u8::MAX) {
                        self.call(owner, cb, |w| abi::SignalArgs {
                            world: w,
                            pos: p.to_array().into(),
                            state: new,
                            level: level as u32,
                            before: was as u32,
                        });
                    }
                }
                Effect::Piston { p, extend } => {
                    // One at it finishes first, and looks again, and so does one being moved.
                    if self.space.signals.on_the_way || self.space.signals.moving.iter().any(|m| m.piston == p) {
                        continue;
                    }
                    match self.push_plan(c, p, extend) {
                        Some(plan) => plans.push(plan),
                        None => {
                            log::debug!("piston at {p} can't move");
                            self.space.signals.held.insert(p);
                        }
                    }
                }
                Effect::Compare { p, behind, side } => {
                    let (Some(state), Some(there)) = (self.space.signals.devices.get(&p).map(|d| d.state), self.get(behind)) else {
                        continue;
                    };
                    let info = c.states[state as usize].signal;
                    let block = c.block_of(there);
                    let Some((owner, cb)) = block.cb.signal_read else { continue };
                    let out =
                        self.call(owner, cb, |w| abi::SignalReadArgs { world: w, pos: behind.to_array().into(), state: there, output: 0 });
                    let want = compared(info.flags, out.output.min(15) as u8, side);
                    if want != info.level {
                        self.space.signals.schedule(p, want, info.delay, self.time);
                    }
                }
                Effect::Floater { p, level } => {
                    if let Some(dev) = self.space.signals.devices.get_mut(&p) {
                        dev.input = level;
                    }
                    if self.frame != 0 {
                        self.floater_shows(c, p, FLOATER_GOING * (level > 0) as u8);
                    } else if level > 0 {
                        floaters.push(p);
                    } else {
                        self.floater_shows(c, p, FLOATER_IDLE);
                    }
                }
                Effect::Gate { p, inputs } => {
                    let Some(state) = self.space.signals.devices.get(&p).map(|d| d.state) else { continue };
                    let info = c.states[state as usize].signal;
                    let block = c.block_of(state);
                    let Some((owner, cb)) = block.cb.signal_gate else { continue };
                    let out = self.call(owner, cb, |w| abi::GateArgs {
                        world: w,
                        pos: p.to_array().into(),
                        state,
                        inputs: inputs.map(|v| v as u32),
                        output: info.level as u32,
                    });
                    let want = out.output.min(15) as u8;
                    if want != info.level {
                        self.space.signals.schedule(p, want, info.delay, self.time);
                    }
                }
            }
        }
        // Worked out from who moves whom and who wants what, never from where they are, for
        // a build to behave the same whichever way it is turned. A floater moving a piston is
        // like a piston moving one; what floaters and pistons both want, neither gets.
        floaters.sort_unstable_by_key(|p| p.to_array());
        let (mut floater_plans, held) = self.floater_plans(c, floaters);
        for p in held {
            self.space.signals.held.insert(p);
            self.floater_shows(c, p, FLOATER_STUCK);
        }
        plans.retain(|plan: &PushPlan| {
            let moved = floater_plans.iter().any(|m| m.cells.contains(&plan.p));
            if moved {
                self.space.signals.held.insert(plan.p);
            }
            !moved
        });
        let mut lost = vec![false; plans.len()];
        let mut lost_floaters = vec![false; floater_plans.len()];
        let waiting = loop {
            let waiting = being_moved(&plans, &lost);
            let mut claims: FxHashMap<IVec3, u32> = FxHashMap::default();
            let going = |i: &usize| !lost[*i] && !waiting[*i];
            for i in (0..plans.len()).filter(going) {
                let cells: FxHashSet<IVec3> = plan_cells(&plans[i]).collect();
                for cell in cells {
                    *claims.entry(cell).or_default() += 1;
                }
            }
            for (m, _) in floater_plans
                .iter()
                .enumerate()
                .filter(|(m, _)| !lost_floaters[*m])
            {
                for cell in floater_plans[m]
                    .cells
                    .iter()
                    .chain(floater_plans[m].broken.iter())
                {
                    *claims.entry(*cell).or_default() += 1;
                }
            }
            let losers: Vec<usize> = (0..plans.len())
                .filter(going)
                .filter(|i| plan_cells(&plans[*i]).any(|cell| claims[&cell] > 1))
                .collect();
            let floater_losers: Vec<usize> = (0..floater_plans.len())
                .filter(|m| !lost_floaters[*m])
                .filter(|m| {
                    floater_plans[*m]
                        .cells
                        .iter()
                        .chain(floater_plans[*m].broken.iter())
                        .any(|cell| claims[cell] > 1)
                })
                .collect();
            if losers.is_empty() && floater_losers.is_empty() {
                break waiting;
            }
            for i in losers {
                // Two bringing back one block both come back without it.
                if !plans[i].extend && !plans[i].moved.is_empty() {
                    let plan = &mut plans[i];
                    (plan.moved, plan.riding, plan.broken) = Default::default();
                    continue;
                }
                log::debug!(
                    "piston at {} gives way: {:?}",
                    plans[i].p,
                    plan_cells(&plans[i])
                        .filter(|cell| claims[cell] > 1)
                        .collect::<Vec<_>>()
                );
                lost[i] = true;
                self.space.signals.polled.insert(plans[i].p);
            }
            for m in floater_losers {
                lost_floaters[m] = true;
                self.space
                    .signals
                    .polled
                    .extend(floater_plans[m].floaters.iter().copied());
                for &p in &floater_plans[m].floaters {
                    self.floater_shows(c, p, FLOATER_STUCK);
                }
            }
        };
        for (i, plan) in plans.into_iter().enumerate() {
            if waiting[i] {
                self.space.signals.held.insert(plan.p);
            } else if !lost[i] {
                self.run_plan(c, plan);
            }
        }
        for (m, plan) in std::mem::take(&mut floater_plans).into_iter().enumerate() {
            if !lost_floaters[m] {
                self.run_floater_plan(c, plan);
            }
        }
    }

    /// What a floater shows and a comparator reads of it (`power`: 0 idle, 1 going, 2 stuck).
    fn floater_shows(&mut self, c: &'static Content, p: IVec3, level: u8) {
        let Some(state) = self.get(p) else { return };
        let info = c.states[state as usize].signal;
        if info.kind == sk::FLOATER && info.level != level {
            self.set(c, p, with_level(&info, state, level));
        }
    }

    pub(super) fn movable(&self, c: &Content, state: u32) -> bool {
        let s = &c.states[state as usize];
        let extended = s.signal.kind == sk::PISTON && s.signal.level > 0;
        s.hardness >= 0.0 && s.flags & bf::IMMOVABLE == 0 && s.signal.kind != sk::PISTON_HEAD && !extended
    }

    /// What moving the block at `start` along `step` takes: blocks ahead of what moves go too, and
    /// what touches a sticky block. `None` if something can't.
    fn gather_move(&self, c: &Content, base: IVec3, free: Option<IVec3>, start: IVec3, step: IVec3) -> Option<(Vec<IVec3>, Vec<IVec3>)> {
        let (mut moved, mut broken): (Vec<IVec3>, Vec<IVec3>) = (Vec::new(), Vec::new());
        let mut todo = vec![start];
        while let Some(at) = todo.pop() {
            if moved.contains(&at) || broken.contains(&at) || Some(at) == free {
                continue;
            }
            if self.space.signals.reserved(at) {
                log::debug!("blocks are on their way to {at}");
                return None;
            }
            let here = self.get(at)?;
            if here == AIR {
                continue;
            }
            let info = &c.states[here as usize];
            if info.liquid.is_some() {
                broken.push(at);
                continue;
            }
            if at == base || !self.movable(c, here) {
                log::debug!("in the way of a piston: {} at {at}", c.state_name(here));
                return None;
            }
            if info.flags & bf::SOLID == 0 {
                broken.push(at);
                continue;
            }
            if moved.len() == PUSH_LIMIT {
                log::debug!("too much for a piston at {base}");
                return None;
            }
            moved.push(at);
            todo.push(at + step);
            if info.flags & bf::STICKY != 0 {
                for d in DIRS {
                    let n = at + d;
                    if n == at + step || n == base {
                        continue;
                    }
                    // What can't come along stays behind, unlike what is in the way.
                    let s = self.get(n)?;
                    if s != AIR && c.states[s as usize].flags & bf::SOLID != 0 && self.movable(c, s) {
                        todo.push(n);
                    }
                }
            }
        }
        Some((moved, broken))
    }

    /// What a piston in front of `from` facing `dir` would push, moved one block that way as a
    /// body over a piston's time, then put down: for a mod's own movers. False if it can't go.
    pub fn push_blocks(&mut self, c: &'static Content, from: IVec3, dir: u8) -> bool {
        let Some(&step) = DIRS.get(dir as usize) else { return false };
        let Some((moved, broken)) = self.gather_move(c, from - step, None, from, step) else { return false };
        if moved.is_empty() {
            return false;
        }
        self.break_for_body(c, broken);
        let id = self.lift_for_mod(c, from, &moved);
        let Some(e) = self.entities.get_mut(id) else { return false };
        e.motion = super::entity::Motion::To { to: e.pos + step.as_dvec3(), left: MOVE_TICKS as u16 };
        if let Some(b) = self.bodies.get_mut(&id) {
            b.stopping = true;
        }
        true
    }

    fn push_plan(&mut self, c: &Content, p: IVec3, extend: bool) -> Option<PushPlan> {
        let state = self.get(p)?;
        let s = c.states[state as usize].signal;
        let dir = s.front;
        let step = *DIRS.get(dir as usize)?;
        let head = p + step;
        let mut plan = PushPlan { p, extend, dir, moved: Vec::new(), step, broken: Vec::new(), riding: Vec::new() };
        if extend {
            (plan.moved, plan.broken) = self.gather_move(c, p, None, head, step)?;
        } else {
            // A sticky one brings back what is against its head, if it can; if not it comes back
            // without it. So it does when the power is gone before what it pushed has got there:
            // vanilla's way to leave a block behind. With nothing pushed, it brings back what it
            // reached however short the power was, which is how a flying machine's back half comes.
            plan.step = -step;
            let held_on = self
                .space
                .signals
                .devices
                .get(&p)
                .is_some_and(|d| self.time > d.pushed_at + MOVE_TICKS as u64);
            if s.flags as u32 & abi::signal_flags::STICKY != 0 && held_on {
                let beyond = head + step;
                let here = self.get(beyond)?;
                if here != AIR && c.states[here as usize].flags & bf::SOLID != 0 && self.movable(c, here) {
                    if let Some((moved, broken)) = self.gather_move(c, p, Some(head), beyond, -step) {
                        (plan.moved, plan.broken) = (moved, broken);
                    }
                }
            }
        }
        let (riding, broken) = self.who_rides(c, &plan.moved, Some(p), plan.step);
        plan.broken.retain(|b| !riding.iter().any(|r| r.0 == *b));
        plan.broken.extend(
            broken
                .into_iter()
                .filter(|b| !plan.broken.contains(b))
                .collect::<Vec<_>>(),
        );
        plan.riding = riding;
        Some(plan)
    }

    /// What a floater takes along: what is in front of it, and on from that what touches anything
    /// sticky. Nothing else it touches comes into it. `None`: it can't.
    pub(super) fn stuck_to_floater(&self, c: &Content, p: IVec3) -> Option<Vec<IVec3>> {
        let info = c.states[self.get(p)? as usize].signal;
        let grip = p + *DIRS.get(info.front as usize)?;
        let mut moved = vec![p];
        let mut seen: FxHashSet<IVec3> = [p, grip].into_iter().collect();
        let mut todo = Vec::new();
        let held = self.get(grip)?;
        if held != AIR && c.states[held as usize].flags & bf::SOLID != 0 {
            if !self.movable(c, held) {
                log::debug!("a floater at {p} has in front of it what can't be moved");
                return None;
            }
            moved.push(grip);
            todo.push(grip);
        }
        while let Some(at) = todo.pop() {
            let here = self.get(at)?;
            if c.states[here as usize].flags & bf::STICKY == 0 {
                continue;
            }
            for d in DIRS {
                let n = at + d;
                if !seen.insert(n) {
                    continue;
                }
                let s = self.get(n)?;
                if s != AIR && c.states[s as usize].flags & bf::SOLID != 0 && self.movable(c, s) {
                    if moved.len() == BODY_LIMIT {
                        log::debug!("too much for a floater at {p}");
                        return None;
                    }
                    moved.push(n);
                    todo.push(n);
                }
            }
        }
        Some(moved)
    }

    /// `moved` with what stands on it, as cells of a new body around `center`.
    pub(super) fn with_riders(&self, c: &Content, center: IVec3, moved: Vec<IVec3>) -> Vec<Lifted> {
        let (riding, _) = self.who_rides(c, &moved, None, IVec3::ZERO);
        moved
            .iter()
            .map(|&m| (m, None))
            .chain(riding.into_iter().map(|(n, part)| (n, Some(part))))
            .map(|(m, part)| Lifted { from: m, local: m - center, to: m, part })
            .collect()
    }

    /// What stands on `moved` goes with them along `step`, unless it would run into something:
    /// what rides, and what breaks instead, in its way or knocked off. Not what merely lies in the
    /// way of `moved`, which `gather_move` has broken. `not`: a cell that doesn't come into it.
    pub(super) fn who_rides(&self, c: &Content, list: &[IVec3], not: Option<IVec3>, step: IVec3) -> (Vec<(IVec3, u32)>, Vec<IVec3>) {
        let moved: FxHashSet<IVec3> = list.iter().copied().collect();
        let mut riding: FxHashMap<IVec3, u32> = FxHashMap::default();
        let mut broken = Vec::new();
        for &at in list {
            for d in DIRS {
                let n = at + d;
                if moved.contains(&n) || riding.contains_key(&n) || Some(n) == not {
                    continue;
                }
                let Some(state) = self.get(n) else { continue };
                let info = &c.states[state as usize];
                if state == AIR || info.flags & bf::SOLID != 0 || info.liquid.is_some() {
                    continue;
                }
                let held_by = |f: u8| moved.contains(&(n + DIRS[f as usize]));
                let part = if info.signal.kind == sk::WIRE {
                    let mask = wire::mask(c, state);
                    let going = (0..6u8)
                        .filter(|f| mask & (1 << f) != 0 && held_by(*f))
                        .fold(0u8, |m, f| m | 1 << f);
                    (going != 0).then(|| c.block_of(state).with_sides(state, going))
                } else {
                    c.held_by(state).filter(|f| held_by(*f)).map(|_| state)
                };
                if let Some(part) = part {
                    riding.insert(n, part);
                }
            }
        }
        // Those running into what doesn't move or come along don't ride: they break, and then
        // what would have followed them may find it free, or run into something too. As many
        // rounds as that takes, none of it in any order.
        let free = |s: &Dim, to: IVec3| {
            s.get(to)
                .is_some_and(|there| rider_meets(c, there) != RiderMeets::Wall)
        };
        loop {
            let stuck: Vec<IVec3> = riding
                .keys()
                .copied()
                .filter(|n| {
                    let to = *n + step;
                    !moved.contains(&to) && !riding.contains_key(&to) && !free(self, to) || self.space.signals.reserved(to)
                })
                .collect();
            if stuck.is_empty() {
                break;
            }
            for n in stuck {
                riding.remove(&n);
                broken.push(n);
            }
        }
        let mut riding: Vec<(IVec3, u32)> = riding.into_iter().collect();
        riding.sort_unstable_by_key(|r| r.0.to_array());
        let in_their_way = riding
            .iter()
            .map(|r| r.0 + step)
            .filter(|to| !moved.contains(to) && !riding.iter().any(|r| r.0 == *to));
        let in_their_way: Vec<IVec3> = in_their_way
            .filter(|to| {
                self.get(*to)
                    .is_some_and(|there| rider_meets(c, there) == RiderMeets::Breaks)
            })
            .collect();
        broken.extend(in_their_way);
        (riding, broken)
    }

    /// Takes the plan's blocks out of the world and sends them on their way as one body, which
    /// carries and shoves what is around it like any other. They land as blocks.
    fn run_plan(&mut self, c: &'static Content, plan: PushPlan) {
        let Some(state) = self.get(plan.p) else { return };
        let info = c.states[state as usize].signal;
        let head = plan.p + DIRS[plan.dir as usize];
        for &b in &plan.broken {
            if let Some(there) = self.get(b) {
                if c.states[there as usize].liquid.is_none() {
                    self.drop_block_loot(c, b, there, abi::ItemStack::EMPTY, false, 0);
                }
                self.set(c, b, AIR);
            }
        }
        let short = |h: u32| c.state_with(h, "short", "true").unwrap_or(h);
        // The head, which isn't a block anywhere as it moves out, or comes off to move in.
        let mut also = Vec::new();
        let mut carried_head = None;
        let mut head_turns = None;
        if plan.extend {
            let h = info
                .head
                .and_then(|b| c.state_with(c.blocks[b as usize].first_state, "facing", wire::dir_name(plan.dir)));
            if let Some(h) = h {
                // From inside the piston, out of its short base.
                also.push((IVec3::ZERO, short(h)));
                carried_head = Some(Carried { local: IVec3::ZERO, to: head, state: h, entity: None });
                head_turns = Some((IVec3::ZERO, h));
            }
        } else if let Some(h) = self
            .get(head)
            .filter(|h| c.states[*h as usize].signal.kind == sk::PISTON_HEAD)
        {
            also.push((head - plan.p, h));
            head_turns = Some((head - plan.p, short(h)));
            self.write_block(c, head, AIR, true);
        }
        // Where they are going is taken from now on.
        self.space.signals.reserved.insert(head);
        for &m in &plan.moved {
            self.space.signals.reserved.insert(m + plan.step);
        }
        if plan.extend {
            self.set(c, plan.p, with_level(&info, state, 15));
            if let Some(d) = self
                .space
                .signals
                .devices
                .get_mut(&plan.p)
                .filter(|_| !plan.moved.is_empty())
            {
                d.pushed_at = self.time;
            }
        }
        // In the body, relative to the piston, as they start; and where they end up.
        for &(n, _) in &plan.riding {
            self.space.signals.reserved.insert(n + plan.step);
        }
        let cells = plan
            .moved
            .iter()
            .map(|&m| (m, None))
            .chain(plan.riding.iter().map(|&(n, part)| (n, Some(part))))
            .map(|(m, part)| Lifted { from: m, local: m - plan.p, to: m + plan.step, part });
        let (body, mut blocks) =
            self.lift(c, c.game.moving_kind, plan.p.as_dvec3() + DVec3::new(0.5, 0.0, 0.5), cells, &also, Liveness::OnTheWay);
        blocks.extend(carried_head);
        if let Some(e) = self.entities.get_mut(body) {
            e.motion = Motion::To { to: e.pos + plan.step.as_dvec3(), left: MOVE_TICKS as u16 };
        }
        self.space
            .signals
            .moving
            .push(Moving { body, piston: plan.p, head, blocks, retracting: !plan.extend, head_turns });
        self.sound(
            c.states[state as usize].effects[crate::content::block_event::PLACE].sound,
            plan.p.as_dvec3() + DVec3::splat(0.5),
            0.5,
            0.0,
        );
    }

    /// Bodies of pistons that got there turn back into blocks.
    pub(super) fn land_moving(&mut self, c: &'static Content) {
        self.tick_landings(c);
        if self.space.signals.moving.is_empty() {
            return;
        }
        let entities = &self.entities;
        let (done, going): (Vec<Moving>, Vec<Moving>) = std::mem::take(&mut self.space.signals.moving)
            .into_iter()
            .partition(|m| {
                entities
                    .get(m.body)
                    .is_none_or(|e| !matches!(e.motion, Motion::To { .. }))
            });
        self.space.signals.moving = going;
        for mut m in done {
            // Clients show this a tick and a half later, a quarter of the way out: past that a
            // long rod no longer shows behind the piston.
            if !m.retracting {
                if let Some((at, state)) = m.head_turns.take() {
                    self.set_in_body(c, m.body, at, state);
                }
            }
            self.land_piston(c, m);
        }
    }

    fn land_piston(&mut self, c: &'static Content, m: Moving) {
        let base = self.get(m.piston);
        let out = base.is_some_and(|s| c.states[s as usize].signal.kind == sk::PISTON && c.states[s as usize].signal.level > 0);
        self.space.signals.reserved.remove(&m.head);
        for b in &m.blocks {
            self.space.signals.reserved.remove(&b.to);
        }
        // A head without its piston goes with it.
        let blocks: Vec<Carried> = m
            .blocks
            .iter()
            .copied()
            .filter(|b| c.states[b.state as usize].signal.kind != sk::PISTON_HEAD || out)
            .collect();
        let mut also = Vec::new();
        if m.retracting {
            if let Some(s) = base.filter(|_| out) {
                self.set(c, m.piston, with_level(&c.states[s as usize].signal, s, 0));
                also.push((m.piston, s));
            }
        }
        self.space.signals.dirty_devices.insert(m.piston);
        self.space.signals.give_another_go();
        self.put_down(c, m.body, blocks, also, m.head_turns);
    }

    /// All that is on its way gets there now: before a save, or before the blocks around it go
    /// to another region or out of memory. It isn't kept anywhere else.
    pub fn finish_moving(&mut self, c: &'static Content) {
        self.finish_moving_here(c);
        self.out_of_touch(c);
        let ids: Vec<u64> = self.bodies.keys().copied().collect();
        for id in ids {
            self.in_frame(id, |d| d.finish_moving_here(c));
        }
    }

    pub(super) fn finish_moving_here(&mut self, c: &'static Content) {
        for m in std::mem::take(&mut self.space.signals.moving) {
            self.land_piston(c, m);
        }
        self.finish_landings();
    }

    /// The piston whose head this is, if it is one.
    pub(super) fn head_of_piston(&self, c: &Content, p: IVec3, state: u32) -> Option<IVec3> {
        let s = c.states[state as usize].signal;
        if s.kind != sk::PISTON_HEAD || s.front >= 6 {
            return None;
        }
        let base = p - DIRS[s.front as usize];
        let there = c.states[self.get(base)? as usize].signal;
        (there.kind == sk::PISTON && there.level > 0 && there.front == s.front).then_some(base)
    }

    /// A piston and its head go together.
    pub(super) fn signal_block_gone(&mut self, c: &'static Content, p: IVec3, old: u32, new: u32) {
        let s = c.states[old as usize].signal;
        if c.states[old as usize].block == c.states[new as usize].block || s.front >= 6 {
            return;
        }
        let step = DIRS[s.front as usize];
        if s.kind == sk::PISTON && s.level > 0 {
            let head = p + step;
            if self
                .get(head)
                .is_some_and(|h| c.states[h as usize].signal.kind == sk::PISTON_HEAD)
            {
                self.set(c, head, AIR);
            }
        } else if s.kind == sk::PISTON_HEAD {
            // Blown up or set from outside; a player's break goes for the piston first.
            if let Some(base) = self.head_of_piston(c, p, old) {
                if let Some(state) = self.get(base) {
                    self.break_off(c, base, state);
                }
            }
        }
    }
}

/// Which plans are of a piston that another piston is moving: it waits to see where it ends up,
/// and goes from there if it still has power. Plans that lost don't move anything, and pistons
/// moving each other in a ring all wait.
fn being_moved(plans: &[PushPlan], lost: &[bool]) -> Vec<bool> {
    let pistons: FxHashMap<IVec3, usize> = plans
        .iter()
        .enumerate()
        .filter(|(i, _)| !lost[*i])
        .map(|(i, plan)| (plan.p, i))
        .collect();
    let mut movers = vec![0u32; plans.len()];
    let mut moves: Vec<Vec<usize>> = vec![Vec::new(); plans.len()];
    for (i, plan) in plans.iter().enumerate().filter(|(i, _)| !lost[*i]) {
        for other in plan.moved.iter().filter_map(|m| pistons.get(m)) {
            movers[*other] += 1;
            moves[i].push(*other);
        }
    }
    let mut goes = vec![false; plans.len()];
    let mut waits = vec![false; plans.len()];
    let mut todo: Vec<usize> = (0..plans.len())
        .filter(|i| !lost[*i] && movers[*i] == 0)
        .collect();
    while let Some(i) = todo.pop() {
        goes[i] = true;
        for &moved in &moves[i] {
            if std::mem::replace(&mut waits[moved], true) {
                continue;
            }
            // Whatever only this one would have moved stays free to go.
            for &next in &moves[moved] {
                movers[next] -= 1;
                if movers[next] == 0 && !waits[next] {
                    todo.push(next);
                }
            }
        }
    }
    (0..plans.len()).map(|i| !lost[i] && !goes[i]).collect()
}

fn plan_cells(plan: &PushPlan) -> impl Iterator<Item = IVec3> + '_ {
    let head = plan.p + DIRS[plan.dir as usize];
    let riding = plan.riding.iter().map(|r| r.0);
    plan.moved
        .iter()
        .copied()
        .chain(riding.clone())
        .chain(
            plan.moved
                .iter()
                .copied()
                .chain(riding)
                .map(|m| m + plan.step),
        )
        .chain(plan.broken.iter().copied())
        .chain([head, plan.p])
}
