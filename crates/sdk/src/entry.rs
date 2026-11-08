//! Registration as entries (`abi::EntryDesc`): a kind, a name, and what to do with its
//! components. Every builder is one, so new, edit, replace and remove work for every kind alike.

use std::sync::atomic::AtomicU32;

use crate::abi::{self, Slice, Str, entry_mode as em};
use crate::content::{Pending, raw_values};
use crate::{Handle, Registry, host, kind};

pub struct Entry {
    pub(crate) kind: u32,
    pub(crate) name: String,
    pub(crate) cell: *const AtomicU32,
    pub(crate) mode: u32,
    pub(crate) ops: Vec<Pending>,
    /// Blocks: operations on the states a key (`prop=value,..`) picks, in order.
    pub(crate) parts: Vec<(String, Vec<Pending>)>,
}

impl Entry {
    pub(crate) fn of<K: kind::Kind>(h: &Handle<K>) -> Entry {
        Entry { cell: h.id_cell(), ..Entry::new(K::KIND, h.name().to_string()) }
    }

    /// One no handle stands for (a sky object of a dimension, a block's loot).
    pub(crate) fn new(kind: u32, name: String) -> Entry {
        Entry { kind, name, cell: std::ptr::null(), mode: em::NEW, ops: Vec::new(), parts: Vec::new() }
    }

    pub(crate) fn send(&mut self, reg: &Registry) {
        if std::thread::panicking() {
            return;
        }
        let mut scratch = Vec::new();
        let ops = value_ops(&self.ops, &mut scratch);
        let mut part_scratch: Vec<Vec<Vec<u8>>> = self.parts.iter().map(|_| Vec::new()).collect();
        let part_ops: Vec<Vec<abi::ValueOp>> = self
            .parts
            .iter()
            .zip(part_scratch.iter_mut())
            .map(|((_, ops), s)| value_ops(ops, s))
            .collect();
        let parts: Vec<abi::PartDesc> = self
            .parts
            .iter()
            .zip(&part_ops)
            .map(|((key, _), ops)| abi::PartDesc { key: Str::new(key), ops: Slice::new(ops) })
            .collect();
        let d = abi::EntryDesc {
            kind: self.kind,
            name: Str::new(&self.name),
            cell: self.cell,
            mode: self.mode,
            ops: Slice::new(&ops),
            parts: Slice::new(&parts),
        };
        unsafe { (host().reg_entry)(reg.raw, &d) }
    }
}

fn value_ops<'a>(list: &'a [Pending], scratch: &'a mut Vec<Vec<u8>>) -> Vec<abi::ValueOp> {
    raw_values(list, scratch)
        .into_iter()
        .zip(list)
        .map(|(value, p)| abi::ValueOp { op: p.op, value, anchor: Str::new(&p.anchor) })
        .collect()
}

/// Declares a builder as an entry of a kind: `Registry` makes it (`$make`), `edit` and `replace`
/// start it in that mode, and dropping it sends it, after its `$finish` if it has one.
macro_rules! entry_builder {
    ($($k:ident => $b:ident, $make:ident $(, $finish:ident)?;)*) => {
        $(impl $crate::WithComponents for $b<'_> {
            type Kind = $crate::kind::$k;
            fn pending(&mut self) -> &mut Vec<$crate::Pending> {
                &mut self.e.ops
            }
        }
        impl Drop for $b<'_> {
            fn drop(&mut self) {
                $(self.$finish();)?
                self.e.send(&*self.reg);
            }
        }
        impl $crate::kind::Builds for $crate::kind::$k {
            type Builder<'a> = $b<'a>;
            fn builder<'a>(r: &'a mut $crate::Registry, h: &'static $crate::Handle<Self>, mode: u32) -> $b<'a> {
                let mut b = r.$make(h);
                b.e.mode = mode;
                b
            }
        })*
    };
}
pub(crate) use entry_builder;
