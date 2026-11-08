//! How component values travel and are kept. Fixed values (numbers, `[T; N]`, `{..}` structs of
//! fixed fields, `callback`) are their `repr(C)` bytes, so frozen tables are read in place.
//! Values with text or lists are packed: text is a `u32` length and UTF-8, a list `[T]` a `u32`
//! count and its elements, a packed struct `<a:T,..>` its fields in order without padding (fixed
//! fields still `repr(C)`). Packed bytes hold no pointers, so they are copied as they are.

use crate::Callback;

/// A value with a schema both sides compare.
pub trait Data: Sized + 'static {
    /// `repr(C)` bytes, read in place from tables (no text or list inside).
    const FIXED: bool;
    fn schema() -> String;
    fn write(&self, out: &mut Vec<u8>);
    /// Reads one value off the front of `r`, moving it past.
    fn read(r: &mut &[u8]) -> Option<Self>;
}

/// The bytes of `v`.
pub fn bytes_of<T: Data>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.write(&mut out);
    out
}

/// One value of `T` from `bytes`, all of them.
pub fn from_bytes<T: Data>(mut bytes: &[u8]) -> Option<T> {
    T::read(&mut bytes)
}

fn take<'a>(r: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    let (head, rest) = (r.get(..n)?, r.get(n..)?);
    *r = rest;
    Some(head)
}

macro_rules! raw {
    ($($t:ty => $s:literal),*) => {
        $(impl Data for $t {
            const FIXED: bool = true;
            fn schema() -> String {
                $s.into()
            }
            fn write(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
            fn read(r: &mut &[u8]) -> Option<Self> {
                Some(<$t>::from_le_bytes(take(r, size_of::<$t>())?.try_into().ok()?))
            }
        })*
    };
}
raw!(u8 => "u8", i8 => "i8", u16 => "u16", i16 => "i16", u32 => "u32", i32 => "i32", u64 => "u64", i64 => "i64", f32 => "f32", f64 => "f64");

impl Data for bool {
    const FIXED: bool = true;
    fn schema() -> String {
        "bool".into()
    }
    fn write(&self, out: &mut Vec<u8>) {
        out.push(*self as u8);
    }
    fn read(r: &mut &[u8]) -> Option<Self> {
        Some(take(r, 1)?[0] != 0)
    }
}

impl<T: Data, const N: usize> Data for [T; N] {
    const FIXED: bool = T::FIXED;
    fn schema() -> String {
        alloc::format!("[{};{N}]", T::schema())
    }
    fn write(&self, out: &mut Vec<u8>) {
        self.iter().for_each(|v| v.write(out));
    }
    fn read(r: &mut &[u8]) -> Option<Self> {
        let mut v = Vec::with_capacity(N);
        for _ in 0..N {
            v.push(T::read(r)?);
        }
        v.try_into().ok()
    }
}

impl Data for String {
    const FIXED: bool = false;
    fn schema() -> String {
        "str".into()
    }
    fn write(&self, out: &mut Vec<u8>) {
        write_text(self, out);
    }
    fn read(r: &mut &[u8]) -> Option<Self> {
        let n = u32::read(r)? as usize;
        String::from_utf8(take(r, n)?.to_vec()).ok()
    }
}

pub fn write_text(s: &str, out: &mut Vec<u8>) {
    (s.len() as u32).write(out);
    out.extend_from_slice(s.as_bytes());
}

impl<T: Data> Data for Vec<T> {
    const FIXED: bool = false;
    fn schema() -> String {
        alloc::format!("[{}]", T::schema())
    }
    fn write(&self, out: &mut Vec<u8>) {
        (self.len() as u32).write(out);
        self.iter().for_each(|v| v.write(out));
    }
    fn read(r: &mut &[u8]) -> Option<Self> {
        let n = u32::read(r)? as usize;
        (0..n).map(|_| T::read(r)).collect()
    }
}

impl Data for Callback {
    const FIXED: bool = true;
    fn schema() -> String {
        "callback".into()
    }
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&(self.func.map_or(0, |f| f as usize) as u64).to_le_bytes());
        out.extend_from_slice(&(self.data as usize as u64).to_le_bytes());
    }
    fn read(r: &mut &[u8]) -> Option<Self> {
        let (f, d) = (u64::read(r)? as usize, u64::read(r)? as usize);
        Some(Callback {
            func: (f != 0).then(|| unsafe { core::mem::transmute::<usize, crate::CallbackFn>(f) }),
            data: d as *const core::ffi::c_void,
        })
    }
}

/// A packed struct (`<field:schema,..>`), its fields written in order.
#[macro_export]
macro_rules! data {
    ($(#[$m:meta])* $vis:vis struct $name:ident { $($(#[$fm:meta])* $fvis:vis $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq, Default)]
        $vis struct $name { $($(#[$fm])* $fvis $field: $ty),* }
        impl $crate::data::Data for $name {
            const FIXED: bool = false;
            fn schema() -> $crate::data::String {
                let fields: $crate::data::Vec<$crate::data::String> =
                    $crate::data::vec![$($crate::data::format!("{}:{}", stringify!($field), <$ty as $crate::data::Data>::schema())),*];
                $crate::data::format!("<{}>", fields.join(","))
            }
            fn write(&self, out: &mut $crate::data::Vec<u8>) {
                $($crate::data::Data::write(&self.$field, out);)*
            }
            fn read(r: &mut &[u8]) -> Option<Self> {
                Some($name { $($field: $crate::data::Data::read(r)?),* })
            }
        }
    };
}
pub use alloc::{format, string::String, vec, vec::Vec};

/// `repr(C)` size and alignment of a fixed schema.
pub fn layout(schema: &str) -> Option<(usize, usize)> {
    let s = schema.trim();
    match s {
        "" => return Some((0, 1)),
        "u8" | "i8" | "bool" => return Some((1, 1)),
        "u16" | "i16" => return Some((2, 2)),
        "u32" | "i32" | "f32" => return Some((4, 4)),
        "u64" | "i64" | "f64" => return Some((8, 8)),
        "callback" => return Some((16, 8)),
        _ => {}
    }
    if let Some((t, n)) = s
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .and_then(|r| r.rsplit_once(';'))
    {
        let (size, align) = layout(t)?;
        return Some((size * n.trim().parse::<usize>().ok()?, align));
    }
    let (mut at, mut most) = (0usize, 1usize);
    for (_, t) in fields(s.strip_prefix('{')?.strip_suffix('}')?)? {
        let (size, align) = layout(t)?;
        at = at.next_multiple_of(align) + size;
        most = most.max(align);
    }
    Some((at.next_multiple_of(most), most))
}

/// The `name:schema` fields inside a struct schema's brackets.
pub fn fields(inner: &str) -> Option<Vec<(&str, &str)>> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0, 0);
    for (i, ch) in inner.char_indices() {
        match ch {
            '{' | '[' | '<' => depth += 1,
            '}' | ']' | '>' => depth -= 1,
            ',' if depth == 0 => {
                let (n, t) = inner[start..i].split_once(':')?;
                out.push((n.trim(), t.trim()));
                start = i + 1;
            }
            _ => {}
        }
    }
    if !inner[start..].trim().is_empty() {
        let (n, t) = inner[start..].split_once(':')?;
        out.push((n.trim(), t.trim()));
    }
    Some(out)
}

/// Schemas written as text: `str` and the references the engine resolves at freeze.
pub fn is_text(schema: &str) -> bool {
    matches!(schema, "str" | "block" | "blocks" | "item" | "match" | "tint")
}

/// Bytes of one value of `schema` at the front of `bytes`.
pub fn value_len(schema: &str, bytes: &[u8]) -> Option<usize> {
    let s = schema.trim();
    if is_text(s) {
        return Some(4 + u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize);
    }
    if let Some((size, _)) = layout(s) {
        return Some(size);
    }
    if let Some(inner) = s.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
        let mut at = 0;
        for (_, t) in fields(inner)? {
            at += value_len(t, bytes.get(at..)?)?;
        }
        return Some(at);
    }
    if let Some(t) = list_of(s) {
        let n = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
        let mut at = 4;
        for _ in 0..n {
            at += value_len(t, bytes.get(at..)?)?;
        }
        return Some(at);
    }
    // An array of packed values.
    let (t, n) = s.strip_prefix('[')?.strip_suffix(']')?.rsplit_once(';')?;
    let mut at = 0;
    for _ in 0..n.trim().parse::<usize>().ok()? {
        at += value_len(t, bytes.get(at..)?)?;
    }
    Some(at)
}

/// The element schema of a list schema `[T]` (not an array `[T;N]`).
pub fn list_of(schema: &str) -> Option<&str> {
    let t = schema.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut depth = 0;
    for c in t.chars() {
        match c {
            '{' | '[' | '<' => depth += 1,
            '}' | ']' | '>' => depth -= 1,
            ';' if depth == 0 => return None,
            _ => {}
        }
    }
    Some(t)
}

/// A list value's elements.
pub fn items<'a>(schema: &str, bytes: &'a [u8]) -> Option<Vec<&'a [u8]>> {
    let t = list_of(schema)?;
    let n = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let (mut at, mut out) = (4, Vec::with_capacity(n));
    for _ in 0..n {
        let len = value_len(t, bytes.get(at..)?)?;
        out.push(bytes.get(at..at + len)?);
        at += len;
    }
    Some(out)
}

/// A list value made of `items`.
pub fn list(items: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    (items.len() as u32).write(&mut out);
    items.iter().for_each(|i| out.extend_from_slice(i));
    out
}

/// The name of a list element, how list edits find it: a text element itself, else the first field
/// when it is `name:str`.
pub fn item_name<'a>(elem_schema: &str, item: &'a [u8]) -> Option<&'a str> {
    if !is_text(elem_schema.trim()) {
        let inner = elem_schema.trim().strip_prefix('<')?.strip_suffix('>')?;
        if fields(inner)?.into_iter().next()? != ("name", "str") {
            return None;
        }
    }
    let n = u32::from_le_bytes(item.get(..4)?.try_into().ok()?) as usize;
    core::str::from_utf8(item.get(4..4 + n)?).ok()
}

/// The engine's own `repr(C)` values: their bytes, by the schema they declare.
macro_rules! schema_data {
    ($($t:ident),*) => {
        $(impl Data for crate::$t {
            const FIXED: bool = true;
            fn schema() -> String {
                <crate::$t as crate::Schema>::SCHEMA.into()
            }
            fn write(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, size_of::<Self>()) });
            }
            fn read(r: &mut &[u8]) -> Option<Self> {
                let b = take(r, size_of::<Self>())?;
                Some(unsafe { core::ptr::read_unaligned(b.as_ptr() as *const Self) })
            }
        })*
    };
}
schema_data!(
    ItemStack,
    Mounting,
    Worn,
    WeatherTick,
    Interaction,
    DimensionChange,
    ItemUsed,
    Hurt,
    BlockChange,
    Death,
    PlayerEvent,
    ChatMessage,
    Pickup,
    Energy,
    Vec3,
    ModelCuboid,
    MeshVertex,
    BoxTilt
);
