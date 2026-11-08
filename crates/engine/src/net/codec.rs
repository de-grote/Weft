//! Binary encoding of messages for network transports. The in-process transport never uses it.
//! Runtime ids are sent as is: peers verify they run the same content before playing.
//! Every type in a message writes and reads itself (`Wire`); messages and their structs get
//! theirs from their declarations (`messages!`, `wire_struct!` in `net`).

use crate::ui::text::{Arg, Msg};
use std::sync::Arc;

use glam::{DVec3, IVec3};
use weft_abi::{ItemStack, ParticleBurst};

use super::{ToClient, ToServer};
use crate::server::lod_tops::{LodColumn, Span};
use crate::world::{Cube, GameMode, Pose};

#[derive(Default)]
pub struct W(pub Vec<u8>);

pub struct R<'a>(pub &'a [u8]);

impl<'a> R<'a> {
    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    pub fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    pub fn u32(&mut self) -> Option<u32> {
        u32::get(self)
    }
    pub fn u64(&mut self) -> Option<u64> {
        u64::get(self)
    }
    pub fn f32(&mut self) -> Option<f32> {
        f32::get(self)
    }
    pub fn f64(&mut self) -> Option<f64> {
        f64::get(self)
    }
}

/// Lengths of lists; byte strings may be longer.
const MAX_LEN: usize = 1 << 20;

pub trait Wire: Sized {
    fn put(&self, w: &mut W);
    fn get(r: &mut R) -> Option<Self>;
}

/// Lists are written element by element; bytes (`Vec<u8>`) in one copy, so they are not one.
pub trait Item: Wire {}

fn get_len(r: &mut R, max: usize) -> Option<usize> {
    let n = u32::get(r)? as usize;
    (n <= max).then_some(n)
}

macro_rules! numbers {
    ($($t:ty),*) => {$(
        impl Wire for $t {
            fn put(&self, w: &mut W) {
                w.0.extend_from_slice(&self.to_le_bytes());
            }
            fn get(r: &mut R) -> Option<Self> {
                Some(<$t>::from_le_bytes(r.take(size_of::<$t>())?.try_into().ok()?))
            }
        }
    )*};
}
numbers!(u8, u32, i32, u64, i64, f32, f64);

impl Wire for bool {
    fn put(&self, w: &mut W) {
        (*self as u8).put(w);
    }
    fn get(r: &mut R) -> Option<Self> {
        Some(u8::get(r)? != 0)
    }
}

impl Wire for Vec<u8> {
    fn put(&self, w: &mut W) {
        (self.len() as u32).put(w);
        w.0.extend_from_slice(self);
    }
    fn get(r: &mut R) -> Option<Self> {
        let n = get_len(r, super::MAX_MESSAGE)?;
        Some(r.take(n)?.to_vec())
    }
}

impl Wire for String {
    fn put(&self, w: &mut W) {
        (self.len() as u32).put(w);
        w.0.extend_from_slice(self.as_bytes());
    }
    fn get(r: &mut R) -> Option<Self> {
        String::from_utf8(Vec::<u8>::get(r)?).ok()
    }
}

impl<T: Item> Wire for Vec<T> {
    fn put(&self, w: &mut W) {
        (self.len() as u32).put(w);
        for v in self {
            v.put(w);
        }
    }
    fn get(r: &mut R) -> Option<Self> {
        let n = get_len(r, MAX_LEN)?;
        let mut out = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            out.push(T::get(r)?);
        }
        Some(out)
    }
}

impl<T: Wire> Wire for Option<T> {
    fn put(&self, w: &mut W) {
        self.is_some().put(w);
        if let Some(v) = self {
            v.put(w);
        }
    }
    fn get(r: &mut R) -> Option<Self> {
        Some(if bool::get(r)? { Some(T::get(r)?) } else { None })
    }
}

impl<T: Wire + Copy + Default, const N: usize> Wire for [T; N] {
    fn put(&self, w: &mut W) {
        for v in self {
            v.put(w);
        }
    }
    fn get(r: &mut R) -> Option<Self> {
        let mut out = [T::default(); N];
        for v in &mut out {
            *v = T::get(r)?;
        }
        Some(out)
    }
}

macro_rules! tuples {
    ($(($($n:tt $t:ident),*)),*) => {$(
        impl<$($t: Wire),*> Wire for ($($t,)*) {
            fn put(&self, w: &mut W) {
                $(self.$n.put(w);)*
            }
            fn get(r: &mut R) -> Option<Self> {
                Some(($($t::get(r)?,)*))
            }
        }
        impl<$($t: Wire),*> Item for ($($t,)*) {}
    )*};
}
tuples!((0 A, 1 B), (0 A, 1 B, 2 C), (0 A, 1 B, 2 C, 3 D), (0 A, 1 B, 2 C, 3 D, 4 E), (0 A, 1 B, 2 C, 3 D, 4 E, 5 F), (0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G));

impl Wire for Span {
    fn put(&self, w: &mut W) {
        (self.bottom, self.top, self.block, self.light).put(w);
    }
    fn get(r: &mut R) -> Option<Self> {
        let (bottom, top, block, light) = Wire::get(r)?;
        Some(Span { bottom, top, block, light })
    }
}

impl Wire for LodColumn {
    fn put(&self, w: &mut W) {
        (self.x, self.z, self.y, self.top, self.under, self.light).put(w);
        self.spans.put(w);
    }
    fn get(r: &mut R) -> Option<Self> {
        let (x, z, y, top, under, light) = Wire::get(r)?;
        Some(LodColumn { x, z, y, top, under, light, spans: Wire::get(r)? })
    }
}

impl Wire for IVec3 {
    fn put(&self, w: &mut W) {
        self.to_array().put(w);
    }
    fn get(r: &mut R) -> Option<Self> {
        Some(IVec3::from_array(Wire::get(r)?))
    }
}

impl Wire for DVec3 {
    fn put(&self, w: &mut W) {
        self.to_array().put(w);
    }
    fn get(r: &mut R) -> Option<Self> {
        Some(DVec3::from_array(Wire::get(r)?))
    }
}

impl Wire for Pose {
    fn put(&self, w: &mut W) {
        (self.0 as u32).put(w);
    }
    fn get(r: &mut R) -> Option<Self> {
        Some(Pose::from_id(u32::get(r)?))
    }
}

impl Wire for GameMode {
    fn put(&self, w: &mut W) {
        (self.0 as u32).put(w);
    }
    fn get(r: &mut R) -> Option<Self> {
        Some(GameMode::from_id(u32::get(r)?))
    }
}

/// Stack data by component name: interned handles differ between processes.
impl Wire for ItemStack {
    fn put(&self, w: &mut W) {
        (self.item, self.count).put(w);
        crate::stack::put_named(w, self.data);
    }
    fn get(r: &mut R) -> Option<Self> {
        let (item, count): (u32, u32) = Wire::get(r)?;
        Some(ItemStack { item, count, data: crate::stack::get_named(r)? })
    }
}

/// States out of range read as air.
impl Wire for Arc<Cube> {
    fn put(&self, w: &mut W) {
        let mut raw = Vec::new();
        self.write(&mut raw, |s| s);
        raw.put(w);
    }
    fn get(r: &mut R) -> Option<Self> {
        let raw = Vec::<u8>::get(r)?;
        let max = crate::content::content().states.len() as u32;
        Some(Arc::new(Cube::read(&mut &raw[..], |s| if s < max { s } else { 0 })?))
    }
}

impl Wire for Msg {
    fn put(&self, w: &mut W) {
        self.key.put(w);
        (self.args.len() as u32).put(w);
        for a in &self.args {
            match a {
                Arg::Text(t) => (0u8, t.clone()).put(w),
                Arg::Key(k) => (1u8, k.clone()).put(w),
                Arg::Int(v) => (2u8, *v).put(w),
                Arg::Float(v, p) => (3u8, *v, *p).put(w),
                Arg::Item(i) => (4u8, *i).put(w),
                Arg::Name(n) => (5u8, n.spec()).put(w),
                Arg::Msg(m) => {
                    6u8.put(w);
                    m.put(w);
                }
            }
        }
    }
    fn get(r: &mut R) -> Option<Self> {
        let key = String::get(r)?;
        let n = get_len(r, MAX_LEN)?;
        let mut args = Vec::with_capacity(n.min(64));
        for _ in 0..n {
            args.push(match u8::get(r)? {
                0 => Arg::Text(String::get(r)?),
                1 => Arg::Key(String::get(r)?),
                2 => Arg::Int(i64::get(r)?),
                3 => Arg::Float(f64::get(r)?, u8::get(r)?),
                4 => Arg::Item(u32::get(r)?),
                5 => {
                    let spec = String::get(r)?;
                    crate::ui::text::Name::parse(&spec).map_or(Arg::Text(spec), Arg::Name)
                }
                6 => Arg::Msg(Msg::get(r)?),
                _ => return None,
            });
        }
        Some(Msg { key, args })
    }
}

impl Wire for ParticleBurst {
    fn put(&self, w: &mut W) {
        // Spread and speeds go as floats: they need no more.
        let small = |v: weft_abi::Vec3| [v.x as f32, v.y as f32, v.z as f32];
        let tail = (self.count, self.block, self.outward, self.color);
        (self.kind, crate::world::dvec(self.pos), small(self.spread), small(self.velocity), self.speed, tail).put(w);
    }
    fn get(r: &mut R) -> Option<Self> {
        let (kind, pos, spread, velocity, speed, (count, block, outward, color)): (u32, DVec3, [f32; 3], [f32; 3], _, _) = Wire::get(r)?;
        let wide = |v: [f32; 3]| weft_abi::Vec3::new(v[0] as f64, v[1] as f64, v[2] as f64);
        Some(ParticleBurst {
            kind,
            pos: pos.to_array().into(),
            spread: wide(spread),
            velocity: wide(velocity),
            speed,
            count,
            block,
            outward,
            color,
        })
    }
}

macro_rules! items {
    ($($t:ty),*) => {$(impl Item for $t {})*};
}
items!(u32, u64, i32, f32, f64, bool, String, IVec3, DVec3, ItemStack, Arc<Cube>, Msg, ParticleBurst, Span, LodColumn);

pub fn encode_client(m: &ToClient) -> Vec<u8> {
    let mut w = W::default();
    m.put(&mut w);
    w.0
}

pub fn decode_client(data: &[u8]) -> Option<ToClient> {
    ToClient::get(&mut R(data))
}

pub fn encode_server(m: &ToServer) -> Vec<u8> {
    let mut w = W::default();
    m.put(&mut w);
    w.0
}

pub fn decode_server(data: &[u8]) -> Option<ToServer> {
    ToServer::get(&mut R(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Message;

    #[test]
    fn server_messages_roundtrip() {
        let msgs = [
            ToServer::Join { name: "Ann".into(), key: "k".into() },
            ToServer::Use {
                pos: IVec3::new(-5, 70, 3),
                part: 7,
                face: 3,
                hit: [0.5, 1.0, 0.25],
                look: [0.1, -0.5, 0.8],
                sneak: true,
                frame: 0,
            },
            ToServer::Move {
                jumped: false,
                pos: DVec3::new(1.5, 64.0, -3.25),
                yaw: 1.0,
                pitch: -0.5,
                on_ground: true,
                pose: crate::world::Pose::SWIM,
                sprint: true,
                flying: false,
                frame: Some((7, DVec3::ONE)),
            },
            ToServer::UseItem { target: Some((IVec3::new(1, 2, 3), 4)) },
            ToServer::Click { slot: 7, button: 1, shift: true },
            ToServer::Drag { slots: vec![3, 4, 9], button: 0 },
            ToServer::Chat("/give stone 5".into()),
            ToServer::Gizmos(vec!["regions".into(), "mymod:pipes".into()]),
            ToServer::Packet { channel: 3, data: vec![1, 2, 255] },
            ToServer::Leave,
        ];
        for m in msgs {
            let back = decode_server(&encode_server(&m)).unwrap();
            assert_eq!(encode_server(&back), encode_server(&m));
        }
    }

    #[test]
    fn tags_follow_the_message_table() {
        for m in [
            ToClient::Died,
            ToClient::Show(crate::net::Shown::chat(Msg::new(
                "chat.message",
                vec![Arg::Text("me".into()), Arg::Int(-3), Arg::Msg(Msg::new("k", vec![Arg::Float(1.5, 2)]))],
            ))),
            ToClient::Despawn(vec![(4, true)]),
            ToClient::PlayAction(Vec::new()),
        ] {
            let bytes = encode_client(&m);
            assert_eq!(bytes[0] as usize, m.kind());
            assert_eq!(decode_client(&bytes).map(|b| b.kind()), Some(m.kind()));
            assert_eq!(encode_client(&decode_client(&bytes).unwrap()), bytes);
        }
        assert_eq!(encode_server(&ToServer::Leave)[0] as usize, ToServer::Leave.kind());
    }

    #[test]
    fn truncated_messages_fail() {
        let bytes = encode_server(&ToServer::Command { id: 5, line: "/time set 0".into() });
        for n in 0..bytes.len() {
            assert!(decode_server(&bytes[..n]).is_none());
        }
    }

    #[test]
    fn gizmos_roundtrip() {
        let g = crate::net::GizmoShape {
            group: "regions".into(),
            kind: 1,
            a: DVec3::new(1.0, 2.0, 3.0),
            b: DVec3::splat(-4.5),
            color: 0xFF8000FF,
            ticks: 21,
            size: 1.5,
            text: Msg::new("gizmo.value", vec![Arg::Text("r0".into())]),
        };
        let Some(ToClient::Gizmos(back)) = decode_client(&encode_client(&ToClient::Gizmos(vec![g.clone()]))) else { panic!("not decoded") };
        assert_eq!(back, vec![g]);
    }
}

#[cfg(test)]
mod named_tests {
    use super::*;

    /// More than a save's list ever got back whole: what follows it must still read right.
    #[test]
    fn long_lists_keep_what_follows() {
        let list: Vec<(String, Vec<u8>)> = (0..100)
            .map(|i| (format!("c{i}"), vec![i as u8; 3]))
            .collect();
        let mut w = W::default();
        list.put(&mut w);
        7u32.put(&mut w);
        let mut r = R(&w.0);
        let got: Vec<(String, Vec<u8>)> = Wire::get(&mut r).unwrap();
        assert_eq!(got, list);
        assert_eq!(r.u32(), Some(7));
    }
}
