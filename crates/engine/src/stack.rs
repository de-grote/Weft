//! Per-stack component data (damage taken, a battery's charge, a tank's contents). Data is
//! immutable and interned: a stack holds a `u32` handle, equal data always gets the same handle,
//! so comparing stacks stays a field compare and merging needs no special cases. Changing a value
//! makes (or finds) another handle. Handles are per process; saves and the network carry the
//! components by name.

use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use weft_abi::ItemStack;

/// Components of one stack, sorted by component id.
pub type Data = Arc<[(u32, Box<[u8]>)]>;

struct Interner {
    list: Vec<Data>,
    index: FxHashMap<Data, u32>,
    projections: FxHashMap<(u32, Projection), u32>,
}

fn table() -> &'static RwLock<Interner> {
    static T: OnceLock<RwLock<Interner>> = OnceLock::new();
    T.get_or_init(|| {
        let empty: Data = Arc::from(Vec::new());
        let mut index = FxHashMap::default();
        index.insert(empty.clone(), 0);
        RwLock::new(Interner { list: vec![empty], index, projections: FxHashMap::default() })
    })
}

pub fn get(data: u32) -> Data {
    let t = table().read();
    t.list
        .get(data as usize)
        .cloned()
        .unwrap_or_else(|| t.list[0].clone())
}

/// Value of `comp` in `data`. The bytes live as long as the process (data is never freed).
pub fn value(data: u32, comp: u32) -> Option<&'static [u8]> {
    if data == 0 {
        return None;
    }
    let d = get(data);
    let i = d.binary_search_by_key(&comp, |e| e.0).ok()?;
    // Interned data is never dropped, so the boxed bytes outlive every caller.
    Some(unsafe { std::slice::from_raw_parts(d[i].1.as_ptr(), d[i].1.len()) })
}

pub fn intern(mut comps: Vec<(u32, Box<[u8]>)>) -> u32 {
    if comps.is_empty() {
        return 0;
    }
    comps.sort_by_key(|e| e.0);
    comps.dedup_by_key(|e| e.0);
    let data: Data = Arc::from(comps);
    if let Some(&id) = table().read().index.get(&data) {
        return id;
    }
    let mut t = table().write();
    if let Some(&id) = t.index.get(&data) {
        return id;
    }
    let id = t.list.len() as u32;
    t.list.push(data.clone());
    t.index.insert(data, id);
    id
}

/// `data` with `comp` set to `bytes`, or removed with `None`.
pub fn with(data: u32, comp: u32, bytes: Option<&[u8]>) -> u32 {
    let mut comps: Vec<(u32, Box<[u8]>)> = get(data).iter().filter(|e| e.0 != comp).cloned().collect();
    if let Some(b) = bytes {
        comps.push((comp, b.into()));
    }
    intern(comps)
}

pub fn u32_of(stack: &ItemStack, comp: Option<u32>) -> u32 {
    comp.and_then(|c| value(stack.data, c))
        .filter(|b| b.len() >= 4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Damage taken (the `damage` stack component), 0 for a fresh stack.
pub fn damage(stack: &ItemStack) -> u32 {
    u32_of(stack, crate::content::try_content().and_then(|c| c.ids.damage))
}

pub fn embedded(stack: &ItemStack) -> Vec<weft_abi::EmbeddedProjectile> {
    crate::content::try_content()
        .and_then(|c| c.components.id(weft_abi::comp::EMBEDDED_PROJECTILES))
        .and_then(|id| value(stack.data, id))
        .and_then(weft_abi::data::from_bytes)
        .unwrap_or_default()
}

/// The pictures a stack shows as (`abi::comp::LAYERS`): its own, else what its item's
/// `condition` gives it, else its item's when that is more than its one untinted icon.
/// `None`: the item's icon.
pub fn layers(stack: &ItemStack) -> Option<&'static str> {
    let c = crate::content::try_content()?;
    c.ids
        .layers
        .filter(|&id| value(stack.data, id).is_some())
        .and_then(|_| text(*stack, weft_abi::comp::LAYERS))
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            let t = look(*stack).texture;
            (!t.is_empty()).then_some(t)
        })
        .or_else(|| c.items.get(stack.item as usize)?.layers.as_deref())
}

/// The data that can change a model, shared by every place a stack is displayed.
pub fn visual_key(stack: ItemStack) -> (u32, u32) {
    (stack.item, projected(stack.data, Projection::Visual))
}

#[derive(Clone, Copy, Default)]
pub struct Look {
    pub model: &'static str,
    pub texture: &'static str,
    pub worn_model: &'static str,
    pub worn_texture: &'static str,
}

/// One conditional result for models, textures and every display context.
pub fn look(stack: ItemStack) -> Look {
    static SEEN: std::sync::LazyLock<RwLock<FxHashMap<(u32, u32), Look>>> = std::sync::LazyLock::new(Default::default);
    let c = crate::content::content();
    let Some(&(owner, cb)) = c.game.item_model_for_stack.get(&stack.item) else { return Look::default() };
    let key = visual_key(stack);
    if let Some(&look) = SEEN.read().get(&key) {
        return look;
    }
    let empty = weft_abi::Str::new("");
    let mut args = weft_abi::ItemModelArgs {
        stack: ItemStack { item: key.0, count: 1, data: key.1 },
        model: empty,
        texture: empty,
        worn_model: empty,
        worn_texture: empty,
    };
    crate::host::invoke(owner, cb, &mut args);
    let keep = |s: weft_abi::Str| -> &'static str {
        let text = unsafe { s.as_str() };
        if text.is_empty() { "" } else { Box::leak(text.to_string().into_boxed_str()) }
    };
    let look = Look {
        model: keep(args.model),
        texture: keep(args.texture),
        worn_model: keep(args.worn_model),
        worn_texture: keep(args.worn_texture),
    };
    SEEN.write().insert(key, look);
    look
}

/// The asset to resolve for a use of a displayed stack. Being shown does not imply being worn.
pub fn model(stack: ItemStack, worn: bool) -> Option<&'static str> {
    if stack.is_empty() {
        return None;
    }
    let look = look(stack);
    let (said, component) = if worn { (look.worn_model, weft_abi::comp::WORN_MODEL) } else { (look.model, weft_abi::comp::MODEL) };
    if !said.is_empty() {
        return Some(said);
    }
    if worn {
        text(stack, component)
    } else {
        let id = crate::content::content().components.id(component)?;
        value(stack.data, id)?;
        text(stack, component)
    }
}

/// Stack text is packed data, while frozen item text is owned by its component table.
pub fn text(stack: ItemStack, name: &str) -> Option<&'static str> {
    let c = crate::content::content();
    let id = c.components.id(name)?;
    if let Some(bytes) = value(stack.data, id) {
        use weft_abi::data::Data;
        let mut rest = bytes;
        let len = u32::read(&mut rest)? as usize;
        return std::str::from_utf8(rest.get(..len)?).ok();
    }
    c.components.text(crate::components::ITEM, stack.item, id)
}

/// `layers` text as (texture name, tint 0xRRGGBB) pairs, white where none is given. A layer
/// tinted by a kind takes the kind's default color: what an item shows away from any biome.
pub fn parse_layers(spec: &str) -> Vec<(&str, u32)> {
    let kind = |k: &str| {
        crate::content::try_content()
            .and_then(|c| c.tint_kinds.iter().find(|t| t.0 == k))
            .map_or(0xFF_FFFF, |t| t.1)
    };
    weft_abi::layers(spec)
        .map(|l| {
            let tint = l.tint.unwrap_or(0xFF_FFFF);
            match l.kind {
                Some(k) => (l.name, multiply(tint, kind(k))),
                None => (l.name, tint),
            }
        })
        .collect()
}

/// Two 0xRRGGBB colors multiplied.
fn multiply(a: u32, b: u32) -> u32 {
    [16, 8, 0]
        .iter()
        .fold(0, |out, s| out | ((a >> s & 255) * (b >> s & 255) / 255) << s)
}

/// The stack's own name, if a player gave it one.
pub fn name(stack: &ItemStack) -> Option<String> {
    let c = crate::content::try_content()?;
    value(stack.data, c.ids.name?).map(|b| String::from_utf8_lossy(b).into_owned())
}

/// `stack` with the components of `data` on top of its own.
pub fn merged_data(stack: ItemStack, data: u32) -> ItemStack {
    let mut comps: Vec<(u32, Box<[u8]>)> = get(stack.data).to_vec();
    comps.extend(get(data).iter().cloned());
    comps.reverse();
    comps.sort_by_key(|e| e.0);
    ItemStack { data: intern(comps), ..stack }
}

pub fn with_damage(stack: ItemStack, damage: u32) -> ItemStack {
    let Some(comp) = crate::content::try_content().and_then(|c| c.ids.damage) else { return stack };
    let bytes = damage.to_le_bytes();
    ItemStack { data: with(stack.data, comp, (damage > 0).then_some(&bytes[..])), ..stack }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Projection {
    Client,
    Save,
    Visual,
}

fn projected(data: u32, projection: Projection) -> u32 {
    use weft_abi::stack_flags as flags;
    if data == 0 {
        return 0;
    }
    if let Some(&known) = table().read().projections.get(&(data, projection)) {
        return known;
    }
    let Some(c) = crate::content::try_content() else { return data };
    let (include, exclude) = match projection {
        Projection::Client => (0, flags::SERVER_ONLY),
        Projection::Save => (0, flags::NO_SAVE),
        Projection::Visual => (flags::VISUAL, flags::SERVER_ONLY),
    };
    let result = intern(
        get(data)
            .iter()
            .filter_map(|(id, bytes)| {
                let flags = c.stack_flags.get(id).copied().unwrap_or(0);
                if flags & exclude != 0 || include != 0 && flags & include == 0 {
                    return None;
                }
                let mut bytes = bytes.clone();
                if holds_stacks(c, *id) {
                    for stack in bytes.chunks_exact_mut(12) {
                        let data = u32::from_le_bytes(stack[8..12].try_into().unwrap());
                        stack[8..12].copy_from_slice(&projected(data, projection).to_le_bytes());
                    }
                }
                Some((*id, bytes))
            })
            .collect(),
    );
    table()
        .write()
        .projections
        .insert((data, projection), result);
    result
}

/// The stack as clients may see it: without `SERVER_ONLY` components.
pub fn for_client(s: ItemStack) -> ItemStack {
    ItemStack { data: projected(s.data, Projection::Client), ..s }
}

pub fn client_bytes(stack: ItemStack) -> Vec<u8> {
    use crate::net::codec::Wire;
    let mut out = crate::net::codec::W::default();
    if !stack.is_empty() {
        for_client(stack).put(&mut out);
    }
    out.0
}

/// Public stacks in slot order, using the same codec as inventories and dropped items.
pub fn shown_bytes(stacks: &[ItemStack]) -> Vec<u8> {
    use crate::net::codec::Wire;
    if stacks.is_empty() {
        return Vec::new();
    }
    let mut out = crate::net::codec::W::default();
    (stacks.len() as u32).put(&mut out);
    for &stack in stacks {
        for_client(stack).put(&mut out);
    }
    out.0
}

/// Stack data to write to saves: without `NO_SAVE` components.
pub fn for_save(data: u32) -> u32 {
    projected(data, Projection::Save)
}

/// Components by name, for saves and the network; names the reader does not know are dropped.
pub fn to_names(data: u32) -> Vec<(String, Vec<u8>)> {
    let Some(c) = crate::content::try_content() else { return Vec::new() };
    get(data)
        .iter()
        .map(|(id, b)| {
            let bytes = if holds_stacks(c, *id) { contents_to_names(c, b) } else { b.to_vec() };
            (c.components.names[*id as usize].clone(), bytes)
        })
        .collect()
}

/// Whether component `id` holds stacks: `contents`, and storages.
pub fn holds_stacks(c: &crate::content::Content, id: u32) -> bool {
    Some(id) == c.ids.contents
        || c.game
            .components
            .get(id as usize)
            .is_some_and(|d| d.flags & weft_abi::component_flags::STORAGE != 0)
}

/// `contents` holds stacks by item id and data handle, both per process: written out by name.
pub fn contents_to_names(c: &crate::content::Content, b: &[u8]) -> Vec<u8> {
    use crate::net::codec::{W, Wire};
    let mut w = W(Vec::new());
    let n = b.len() / 12;
    (n as u32).put(&mut w);
    for i in 0..n {
        let at = |k: usize| u32::from_le_bytes(b[i * 12 + k..i * 12 + k + 4].try_into().unwrap());
        let (item, count, data) = (at(0), at(4), at(8));
        c.items
            .get(item as usize)
            .filter(|_| count > 0 && item != 0)
            .map_or(String::new(), |d| d.name.clone())
            .put(&mut w);
        count.put(&mut w);
        put_named(&mut w, data);
    }
    w.0
}

pub fn contents_from_names(c: &crate::content::Content, b: &[u8]) -> Vec<u8> {
    use crate::net::codec::{R, Wire};
    let mut r = R(b);
    let mut out = Vec::new();
    let Some(n) = u32::get(&mut r) else { return out };
    for _ in 0..n.min(1 << 12) {
        let (Some(name), Some(count), Some(data)) = (String::get(&mut r), u32::get(&mut r), get_named(&mut r)) else { break };
        let item = c.item_names.get(&name).copied().filter(|_| count > 0);
        let (item, count, data) = item.map_or((0, 0, 0), |i| (i, count, data));
        out.extend([item, count, data].iter().flat_map(|v| v.to_le_bytes()));
    }
    out
}

/// `amount` more damage, nothing left when that uses it up; what has no durability stays.
pub fn worn(c: &crate::content::Content, stack: ItemStack, amount: u32) -> ItemStack {
    let most = c.durability(&stack);
    if most == 0 || stack.is_empty() {
        return stack;
    }
    let total = damage(&stack).saturating_add(amount);
    if total >= most { ItemStack::EMPTY } else { with_damage(stack, total) }
}

/// Stack data by component name, for what outlasts the handles of one process: saves and the
/// network alike.
pub fn put_named(w: &mut crate::net::codec::W, data: u32) {
    use crate::net::codec::Wire;
    if data == 0 { Vec::<(String, Vec<u8>)>::new() } else { to_names(data) }.put(w);
}

pub fn get_named(r: &mut crate::net::codec::R) -> Option<u32> {
    use crate::net::codec::Wire;
    Some(from_names(Wire::get(r)?))
}

/// `damage=05000000,mymod:charge=10270000` for text files; empty for plain stacks.
pub fn to_text(data: u32) -> String {
    to_names(data)
        .iter()
        .map(|(n, b)| format!("{n}={}", crate::kv::hex(b)))
        .collect::<Vec<_>>()
        .join(",")
}

pub fn from_text(s: &str) -> u32 {
    from_names(
        s.split(',')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_string(), crate::kv::unhex(v)))
            .collect(),
    )
}

pub fn from_names(list: Vec<(String, Vec<u8>)>) -> u32 {
    let Some(c) = crate::content::try_content() else { return 0 };
    intern(
        list.into_iter()
            .filter_map(|(name, b)| {
                let id = c.components.id(&name)?;
                let b = if holds_stacks(c, id) { contents_from_names(c, &b) } else { b };
                Some((id, b.into_boxed_slice()))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_data_equal_handles() {
        let a = with(0, 7, Some(&5u32.to_le_bytes()));
        let b = with(with(0, 3, Some(&[1])), 7, Some(&5u32.to_le_bytes()));
        let b = with(b, 3, None);
        assert_eq!(a, b);
        assert_ne!(a, 0);
        assert_eq!(value(a, 7), Some(&5u32.to_le_bytes()[..]));
        assert_eq!(with(a, 7, None), 0);
    }
}
