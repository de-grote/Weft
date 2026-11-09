//! Entity serialization for saves. Kinds, items and components are stored by name so a save
//! survives mods being added, removed or reordered.

use glam::{DVec3, IVec3};
use weft_abi::{ItemStack, component_flags as cf};

use super::entity::Entity;
use crate::content::Content;
use crate::net::codec::{self, R};

/// An entity read back: itself, its inventory, components and, for a moving structure, its
/// blocks.
pub type Saved = (Entity, Option<Vec<ItemStack>>, Vec<(u32, Box<[u8]>)>, Option<Vec<(IVec3, u32)>>);
pub type ToSave<'a> = (&'a Entity, Option<&'a Vec<ItemStack>>, Vec<(u32, &'a [u8])>, Option<Vec<(IVec3, String)>>);

struct W<'a>(&'a mut Vec<u8>);

impl W<'_> {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u16(s.len() as u16);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn stack(&mut self, c: &Content, s: ItemStack) {
        if s.is_empty() {
            self.str("");
            return;
        }
        self.str(&c.items[s.item as usize].name);
        self.u32(s.count);
        let mut named = codec::W::default();
        crate::stack::put_named(&mut named, crate::stack::for_save(s.data));
        self.0.extend_from_slice(&named.0);
    }
}

impl R<'_> {
    fn str(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        Some(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn stack(&mut self, c: &Content) -> Option<ItemStack> {
        let name = self.str()?;
        if name.is_empty() {
            return Some(ItemStack::EMPTY);
        }
        let count = self.u32()?;
        let data = crate::stack::get_named(self)?;
        // Items of removed mods are dropped rather than turned into something else.
        Some(
            c.item_names
                .get(&name)
                .map_or(ItemStack::EMPTY, |&i| ItemStack { item: i, count, data }),
        )
    }
}

/// `now`: the tick of the entities' dimension, what their effects' ends count from.
pub fn write(c: &Content, out: &mut Vec<u8>, list: &[ToSave<'_>], now: u64) {
    let mut w = W(out);
    w.u32(list.len() as u32);
    for (e, inv, comps, blocks) in list {
        w.str(&c.game.entities[e.kind as usize].name);
        w.u64(e.id);
        for v in e.pos.to_array().into_iter().chain(e.vel.to_array()) {
            w.f64(v);
        }
        w.f32(e.yaw);
        w.f32(e.pitch);
        w.f32(e.health);
        w.u64(e.age);
        w.stack(c, e.item);
        // 2: on a body, which it says.
        match (e.anchor, e.block_frame()) {
            (Some(a), frame) => {
                w.u8(if frame != 0 { 2 } else { 1 });
                for v in a.to_array() {
                    w.u32(v as u32);
                }
                if frame != 0 {
                    w.u64(frame);
                }
            }
            (None, _) => w.u8(0),
        }
        match inv {
            Some(inv) => {
                w.u16(inv.len() as u16);
                for s in inv.iter() {
                    w.stack(c, *s);
                }
            }
            None => w.u16(u16::MAX),
        }
        let saved: Vec<&(u32, &[u8])> = comps
            .iter()
            .filter(|(id, _)| c.game.components[*id as usize].flags & cf::SAVE != 0)
            .collect();
        w.u16(saved.len() as u16);
        for (id, bytes) in saved {
            w.str(&c.game.components[*id as usize].name);
            let named = if crate::stack::holds_stacks(c, *id) {
                Some(crate::stack::contents_to_names(c, bytes))
            } else {
                (*id == c.game.fields.effect).then(|| effects_to_save(c, bytes, now))
            };
            let bytes = named.as_deref().unwrap_or(bytes);
            // Component values over 64 KB say so with a marker, older saves never have it.
            if bytes.len() >= u16::MAX as usize {
                w.u16(u16::MAX);
                w.u32(bytes.len() as u32);
            } else {
                w.u16(bytes.len() as u16);
            }
            w.0.extend_from_slice(bytes);
        }
        match blocks {
            Some(list) => {
                w.u32(list.len() as u32);
                for (p, name) in list {
                    for v in p.to_array() {
                        w.u32(v as u32);
                    }
                    w.str(name);
                }
            }
            None => w.u32(u32::MAX),
        }
    }
}

/// Marks the trailer of a cube's saved data that holds the timers on its blocks: each the block,
/// then as `dim::push_timer` writes it; their length in bytes, this.
const TIMERS_MAGIC: u32 = 0x5352_4D54;

pub fn append_timers(out: &mut Vec<u8>, c: &Content, timers: &[super::space::BlockTimer]) {
    let start = out.len();
    for (at, p, event, data) in timers {
        p.iter()
            .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        super::dim::push_timer(out, *at, &c.game.events[*event as usize].name, data);
    }
    out.extend_from_slice(&((out.len() - start) as u32).to_le_bytes());
    out.extend_from_slice(&TIMERS_MAGIC.to_le_bytes());
}

/// The timers `append_timers` wrote, but those of events no mod has now.
pub fn read_timers(c: &Content, data: &[u8]) -> Vec<super::space::BlockTimer> {
    let word = |at: usize| {
        data.get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let Some(end) = data.len().checked_sub(8) else { return Vec::new() };
    if word(end + 4) != Some(TIMERS_MAGIC) {
        return Vec::new();
    }
    let Some(mut at) = end.checked_sub(word(end).unwrap_or(0) as usize) else { return Vec::new() };
    let mut out = Vec::new();
    while at + 12 < end {
        let p = [0, 1, 2].map(|i| word(at + i * 4).unwrap_or(0) as i32);
        let Some((len, (until, name, value))) = super::dim::read_timer(&data[at + 12..end]) else { break };
        at += 12 + len;
        match c.game.event_names.get(&name) {
            Some(&e) => out.push((until, p, e, value.into())),
            None => log::warn!("a timer at {p:?} for event {name}, which no mod has now"),
        }
    }
    out
}

/// `now`: the tick of the dimension they come back to, what their effects' ends count from.
pub fn read(c: &Content, data: &[u8], now: u64) -> Vec<Saved> {
    let mut r = R(data);
    let mut out = Vec::new();
    let Some(n) = r.u32() else { return out };
    for _ in 0..n {
        let Some(e) = read_one(c, &mut r, now) else { break };
        if let Some(e) = e {
            out.push(e);
        }
    }
    out
}

fn read_one(c: &Content, r: &mut R, now: u64) -> Option<Option<Saved>> {
    let kind_name = r.str()?;
    let id = r.u64()?;
    let mut v = [0.0f64; 6];
    for x in v.iter_mut() {
        *x = r.f64()?;
    }
    let yaw = r.f32()?;
    let pitch = r.f32()?;
    let health = r.f32()?;
    let age = r.u64()?;
    let item = r.stack(c)?;
    let anchor_kind = r.u8()?;
    let anchor = if anchor_kind != 0 { Some(IVec3::new(r.u32()? as i32, r.u32()? as i32, r.u32()? as i32)) } else { None };
    let frame = if anchor_kind == 2 { Some(r.u64()?) } else { None };
    let inv_len = r.u16()?;
    let inv = if inv_len == u16::MAX {
        None
    } else {
        let mut v = Vec::with_capacity(inv_len as usize);
        for _ in 0..inv_len {
            v.push(r.stack(c)?);
        }
        Some(v)
    };
    let nc = r.u16()?;
    let mut comps = Vec::new();
    for _ in 0..nc {
        let name = r.str()?;
        let len = r.u16()? as usize;
        let len = if len == u16::MAX as usize { r.u32()? as usize } else { len };
        let bytes = r.take(len)?.to_vec();
        if let Some(id) = c.game.component(&name) {
            let bytes = if crate::stack::holds_stacks(c, id) {
                crate::stack::contents_from_names(c, &bytes)
            } else if id == c.game.fields.effect {
                effects_from_save(c, &bytes, now)
            } else {
                bytes
            };
            comps.push((id, c.game.fit_component(id, bytes)));
        }
    }
    let nb = r.u32()?;
    let blocks = if nb == u32::MAX {
        None
    } else {
        let mut v = Vec::with_capacity(nb as usize);
        for _ in 0..nb {
            let p = IVec3::new(r.u32()? as i32, r.u32()? as i32, r.u32()? as i32);
            // Blocks of removed mods are left out.
            if let Some(s) = c.state_by_name(&r.str()?) {
                v.push((p, s));
            }
        }
        Some(v)
    };
    let Some(&kind) = c.game.entity_names.get(&kind_name) else { return Some(None) };
    let mut e = Entity::new(id, kind, DVec3::new(v[0], v[1], v[2]), health);
    e.vel = DVec3::new(v[3], v[4], v[5]);
    e.yaw = yaw;
    e.pitch = pitch;
    e.age = age;
    e.item = item;
    e.anchor = anchor;
    e.frame = frame;
    if kind == c.game.item_kind && item.is_empty() {
        return Some(None);
    }
    Some(Some((e, inv, comps, blocks)))
}

/// An `EFFECT` component as saves keep it: each effect by name, with its level and the ticks it has
/// left, so a mod coming or going and time passing elsewhere change nothing.
pub fn effects_to_save(c: &Content, bytes: &[u8], now: u64) -> Vec<u8> {
    let mut out = Vec::new();
    for f in crate::world::effects_on(Some(bytes), now) {
        let Some(d) = c.game.entity_effects.get(f.id as usize) else { continue };
        out.extend((d.name.len() as u16).to_le_bytes());
        out.extend(d.name.as_bytes());
        out.extend(f.level.to_le_bytes());
        out.extend((f.until - now).min(u32::MAX as u64).to_le_bytes()[..4].iter());
    }
    out
}

/// `effects_to_save` read back for a dimension at tick `now`, but those no mod has now.
pub fn effects_from_save(c: &Content, bytes: &[u8], now: u64) -> Vec<u8> {
    let mut r = R(bytes);
    let mut list = Vec::new();
    while let Some(len) = r.u16() {
        let (Some(name), Some(level), Some(left)) = (r.take(len as usize), r.u32(), r.u32()) else { break };
        let name = String::from_utf8_lossy(name);
        if let Some(&id) = c.game.entity_effect_names.get(name.as_ref()) {
            let until = if left == weft_abi::INFINITE_TICKS { crate::world::FOREVER } else { now + left as u64 };
            list.push(crate::world::OnEffect { id, level, until });
        }
    }
    crate::world::encode_effects(&list).into_vec()
}
