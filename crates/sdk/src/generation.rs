//! Read generation's input in world coordinates, independently of loaded cubes.
use crate::terrain::Column;
use crate::{BlockPos, Dir, abi, host};
use std::marker::PhantomData;

/// The completed stages before this callback, plus its own earlier writes. Reads in the
/// same stage never depend on another start's execution order. This view cannot outlive
/// the callback it was borrowed from.
pub struct Generation<'a> {
    source: abi::GenSource,
    lifetime: PhantomData<&'a ()>,
}
impl Generation<'_> {
    pub(crate) fn new(source: abi::GenSource) -> Self {
        Self { source, lifetime: PhantomData }
    }
    /// A state at any world coordinate, including across cube boundaries; air outside the dimension.
    pub fn read(&self, pos: impl Into<BlockPos>) -> u32 {
        unsafe { (host().generation_read)(&self.source, pos.into()) }
    }
    /// The terrain's reference height and biome. This is metadata, before features change blocks.
    pub fn column(&self, x: i32, z: i32) -> Column {
        let mut out = Column { height: i32::MIN, biome: 0 };
        unsafe { (host().dim_column)(self.source.dim, self.source.seed, x, z, &mut out) };
        out
    }
    /// First cell matching `test`, starting at `from`, then at most `distance` steps in `dir`.
    pub fn find(&self, from: impl Into<BlockPos>, dir: Dir, distance: u32, mut test: impl FnMut(u32) -> bool) -> Option<BlockPos> {
        let mut p = from.into();
        let d = dir.offset();
        for step in 0..=distance {
            if test(self.read(p)) {
                return Some(p);
            }
            if step == distance {
                break;
            }
            p = BlockPos { x: p.x.checked_add(d.x)?, y: p.y.checked_add(d.y)?, z: p.z.checked_add(d.z)? };
        }
        None
    }
    /// Every cell in the box between two opposite corners matches `test`.
    pub fn all(&self, from: impl Into<BlockPos>, to: impl Into<BlockPos>, mut test: impl FnMut(u32) -> bool) -> bool {
        let (a, b) = (from.into(), to.into());
        for y in a.y.min(b.y)..=a.y.max(b.y) {
            for z in a.z.min(b.z)..=a.z.max(b.z) {
                for x in a.x.min(b.x)..=a.x.max(b.x) {
                    if !test(self.read([x, y, z])) {
                        return false;
                    }
                }
            }
        }
        true
    }
    /// What is now at the terrain's reference surface; air where it was carved away.
    pub fn surface_block(&self, x: i32, z: i32) -> u32 {
        let h = self.column(x, z).height;
        if h == i32::MIN { 0 } else { self.read([x, h, z]) }
    }
    /// Whether the terrain's reference surface is now empty.
    pub fn surface_carved(&self, x: i32, z: i32) -> bool {
        self.column(x, z).height != i32::MIN && self.surface_block(x, z) == 0
    }
    /// Fits a footprint at the terrain's reference height, rejecting missing ground and steep slopes.
    pub fn fit(&self, x: i32, z: i32, size: impl Into<BlockPos>, max_slope: i32) -> Option<BlockPos> {
        let s = size.into();
        if s.x <= 0 || s.z <= 0 {
            return None;
        }
        let mid = self.column(x + s.x / 2, z + s.z / 2).height;
        if mid == i32::MIN {
            return None;
        }
        let (mut lo, mut hi) = (mid, mid);
        for (cx, cz) in [(x, z), (x + s.x - 1, z), (x, z + s.z - 1), (x + s.x - 1, z + s.z - 1)] {
            let h = self.column(cx, cz).height;
            if h == i32::MIN || self.surface_block(cx, cz) == 0 {
                return None;
            }
            lo = lo.min(h);
            hi = hi.max(h);
        }
        (i64::from(hi) - i64::from(lo) <= i64::from(max_slope)).then_some(BlockPos { x, y: mid.checked_add(1)?, z })
    }
}
