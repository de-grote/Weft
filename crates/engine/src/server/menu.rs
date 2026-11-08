//! Container menus: a list of slots pointing into inventories (the player's, the one it opened
//! on or the menu's own temporary grid), the cursor stack, and synced integers. The slots come from the
//! menu's groups; click handling is generic, mods only describe groups and react to changes
//! (crafting).

use weft_abi::{self as abi, ItemStack, slot_flags as sf};

use super::dim::Dim;
use super::holder::{self, At, Inv};
use crate::content::Content;
use crate::host;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    Temp,
    Player,
    /// The inventory the menu opened on.
    On(Inv),
}

#[derive(Clone, Copy, Debug)]
pub struct SlotRef {
    pub owner: Owner,
    pub index: u32,
    pub flags: u32,
    /// Only items with this tag or component (`u32::MAX`: any).
    pub accepts: u32,
    /// Most items it holds, 0: the item's stack size.
    pub limit: u32,
    pub priority: i32,
}

pub struct OpenMenu {
    /// Opened by using a block, so it closes when the player walks away. A mod opening it from
    /// code (a remote terminal) means it to work from anywhere.
    pub on_block: bool,
    pub kind: u32,
    pub on: Option<Inv>,
    /// Opened on a stack: the item it was, the menu closes once that stack is gone.
    pub held_item: u32,
    pub slots: Vec<SlotRef>,
    pub temp: Vec<ItemStack>,
    pub cursor: ItemStack,
    pub sent_slots: Vec<ItemStack>,
    pub sent_cursor: ItemStack,
    /// The owner's menu-synced components went out once.
    pub comps_sent: bool,
}

/// What `MenuCtx` pointers handed to mods point at.
pub struct MenuCall {
    pub menu: *mut OpenMenu,
    pub dim: *mut Dim,
    pub player: u64,
}

impl MenuCall {
    fn m(&self) -> &mut OpenMenu {
        unsafe { &mut *self.menu }
    }

    fn d(&self) -> &mut Dim {
        unsafe { &mut *self.dim }
    }

    /// First menu slot and slot count of group `name`.
    /// (first menu slot, slot count, columns) of group `name`.
    pub fn group(&self, c: &Content, name: &str) -> Option<(u32, u32, u32)> {
        let layout = &c.game.player_layout;
        c.game.menus[self.m().kind as usize]
            .group(layout, name)
            .map(|g| (g.1, g.2, g.0.span(layout).2))
    }

    pub fn get(&self, slot: u32) -> ItemStack {
        get(self.m(), self.d(), self.player, slot)
    }

    pub fn set(&mut self, slot: u32, stack: ItemStack) {
        set(self.m(), self.d(), self.player, slot, stack)
    }
}

impl OpenMenu {
    /// The entity it opened on (holding the stack it opened on), 0 for none.
    pub fn owner(&self) -> u64 {
        match self.on.map(|inv| inv.at) {
            Some(At::Entity(id) | At::Stack { entity: id, .. }) => id,
            None => 0,
        }
    }
}

fn get(m: &OpenMenu, dim: &Dim, player: u64, slot: u32) -> ItemStack {
    let Some(r) = m.slots.get(slot as usize) else { return ItemStack::EMPTY };
    let c = crate::content::content();
    match r.owner {
        Owner::Temp => m.temp.get(r.index as usize).copied().unwrap_or_default(),
        Owner::Player => holder::get(dim, c, Inv::own(At::Entity(player)), r.index),
        Owner::On(inv) => holder::get(dim, c, inv, r.index),
    }
}

fn set(m: &mut OpenMenu, dim: &mut Dim, player: u64, slot: u32, stack: ItemStack) {
    let Some(r) = m.slots.get(slot as usize).copied() else { return };
    let stack = if stack.count == 0 { ItemStack::EMPTY } else { stack };
    match r.owner {
        Owner::Temp => {
            if let Some(s) = m.temp.get_mut(r.index as usize) {
                *s = stack;
            }
        }
        Owner::Player => dim.entities.set_slot(player, r.index, stack),
        Owner::On(inv) => holder::set(dim, crate::content::content(), inv, r.index, stack),
    }
}

fn call(
    m: &mut OpenMenu,
    dim: &mut Dim,
    c: &Content,
    player: u64,
    slot: u32,
    pick: impl Fn(&crate::registry::MenuDef) -> Option<(abi::ModId, abi::Callback)>,
) {
    let def = &c.game.menus[m.kind as usize];
    let Some((by, cb)) = pick(def) else { return };
    let owner = match m.on {
        None => abi::Holder::NONE,
        Some(Inv { at: At::Entity(id), storage }) => abi::Holder { storage, ..abi::Holder::entity(id) },
        Some(Inv { at: At::Stack { entity, slot }, storage }) => abi::Holder { storage, ..abi::Holder::held(entity, slot) },
    };
    let mut call = MenuCall { menu: m as *mut OpenMenu, dim: dim as *mut Dim, player };
    let mut ctx = host::CallCtx { dim: dim as *mut Dim };
    let mut args = abi::MenuArgs { world: ctx.raw(), menu: &mut call as *mut MenuCall as *mut abi::MenuCtx, player, owner, slot };
    host::invoke(by, cb, &mut args);
}

/// The slots of menu `kind` opened on `on`, from its groups. `worn`: the equipment slots of the
/// entity it is on, if that wears.
pub fn slots(c: &Content, kind: u32, on: Option<Inv>, worn: Option<u32>, player: u64) -> Vec<SlotRef> {
    use abi::slot_owner as so;
    let def = &c.game.menus[kind as usize];
    let layout = &c.game.player_layout;
    let mut out = Vec::new();
    for (g, _, n) in def.slot_groups(layout) {
        let accepts = match g.accepts.as_str() {
            "" => u32::MAX,
            name => c.components.id(name).unwrap_or_else(|| {
                host::misuse("menu", || format!("{}: group {} accepts {name}, which no item has", def.name, g.name));
                u32::MAX - 1
            }),
        };
        let (o, first) = match (g.owner, worn) {
            (so::MENU, _) => (Owner::Temp, None),
            (so::HOLDER, _) => (Owner::On(on.unwrap_or(Inv::own(At::Entity(0)))), None),
            (so::WORN, Some(w)) => (Owner::On(on.unwrap_or(Inv::own(At::Entity(0)))), Some(w + g.start)),
            (so::WORN, None) => (Owner::Player, Some(layout.worn.start + g.start)),
            _ => (Owner::Player, None),
        };
        for i in 0..n {
            let index = first.map_or_else(|| g.index(layout, i), |f| f + i);
            // The stack the menu is open on can't be moved while it is.
            let locked = o == Owner::Player && on.is_some_and(|inv| inv.at == At::Stack { entity: player, slot: index });
            let flags = if locked { g.flags | sf::LOCKED } else { g.flags };
            out.push(SlotRef { owner: o, index, flags, accepts, limit: g.limit, priority: g.priority });
        }
    }
    out
}

/// Where the equipment of the entity `on` is starts in its inventory, if it wears.
fn worn_on(dim: &Dim, c: &Content, on: Option<Inv>) -> Option<u32> {
    let Some(Inv { at: At::Entity(id), storage: 0 }) = on else { return None };
    let w = &c.game.entities[dim.entities.get(id)?.kind as usize].worn;
    (!w.is_empty()).then_some(w.start)
}

pub fn open(dim: &mut Dim, c: &Content, player: u64, kind: u32, on: Option<Inv>) -> OpenMenu {
    let def = &c.game.menus[kind as usize];
    let held_item = match on.map(|inv| inv.at) {
        Some(At::Stack { entity, slot }) => holder::get(dim, c, Inv::own(At::Entity(entity)), slot).item,
        _ => 0,
    };
    let mut m = OpenMenu {
        on_block: false,
        kind,
        on,
        held_item,
        slots: slots(c, kind, on, worn_on(dim, c, on), player),
        temp: vec![ItemStack::EMPTY; def.temp_slots as usize],
        cursor: ItemStack::EMPTY,
        sent_slots: Vec::new(),
        sent_cursor: ItemStack::EMPTY,
        comps_sent: false,
    };
    call(&mut m, dim, c, player, 0, |d| d.on_change);
    m
}

/// Whether what the menu is on is gone or too far away: modern's reach to a block plus 4. Virtual
/// inventories have no place to walk away from.
pub fn out_of_reach(m: &OpenMenu, dim: &Dim, player: u64) -> bool {
    let Some(on) = m.on else { return false };
    let Some(owner) = dim.entities.get(m.owner()).filter(|o| !o.removed) else { return true };
    if let At::Stack { entity, slot } = on.at {
        let now = holder::get(dim, crate::content::content(), Inv::own(At::Entity(entity)), slot);
        return now.item != m.held_item || now.is_empty();
    }
    if owner.anchor.is_none() || !m.on_block {
        return false;
    }
    dim.entities
        .get(player)
        .is_none_or(|p| (p.pos - owner.pos).length_squared() > 8.5 * 8.5)
}

pub fn tick(m: &mut OpenMenu, dim: &mut Dim, c: &Content, player: u64) {
    call(m, dim, c, player, 0, |d| d.on_tick);
}

/// Returns temp slots and the cursor to the player (dropping what does not fit). Output slots are
/// previews, not real items, and are discarded.
pub fn close(m: OpenMenu, dim: &mut Dim, c: &Content, player: u64) {
    let outputs: Vec<u32> = m
        .slots
        .iter()
        .filter(|s| s.owner == Owner::Temp && s.flags & sf::OUTPUT != 0)
        .map(|s| s.index)
        .collect();
    let mut back: Vec<ItemStack> = m
        .temp
        .iter()
        .enumerate()
        .filter(|(i, s)| !s.is_empty() && !outputs.contains(&(*i as u32)))
        .map(|(_, s)| *s)
        .collect();
    back.push(m.cursor);
    for s in back.into_iter().filter(|s| !s.is_empty()) {
        let rest = dim.entities.insert_item(c, player, s);
        if !rest.is_empty() {
            if let Some(p) = dim.entities.get(player).map(|e| e.pos) {
                dim.spawn_item(c, p + glam::DVec3::Y, rest);
            }
        }
    }
}

fn fits(c: &Content, s: &SlotRef, item: u32) -> bool {
    if s.flags & sf::LOCKED != 0 || (s.flags & sf::OFF_HAND != 0 && !crate::content::fits_one_hand(c, item)) {
        return false;
    }
    // Liquids by the millibucket go into liquid slots only, and nothing else does.
    if item != 0 && (s.flags & sf::LIQUID != 0) != c.item_fluid.contains_key(&item) {
        return false;
    }
    // A stack holding items doesn't go into another one.
    if matches!(s.owner, Owner::On(Inv { at: At::Stack { .. }, .. })) && holder::item_slots(c, item) > 0 {
        return false;
    }
    s.accepts == u32::MAX || item == 0 || c.components.has(crate::components::ITEM, item, s.accepts)
}

/// Most of `item` slot `s` holds.
fn cap(c: &Content, s: &SlotRef, item: u32) -> u32 {
    let max = c.max_stack(item);
    if s.limit > 0 { max.min(s.limit) } else { max }
}

/// Whether a player may take items out of `s` by hand.
fn takeable(s: &SlotRef) -> bool {
    s.flags & (sf::INPUT | sf::LOCKED | sf::LIQUID) == 0
}

/// What a bucket holds.
const CONTAINER_MB: u32 = 1000;

/// A click on a liquid slot with a container on the cursor: a full one pours into it, an empty
/// one fills from it, a bucket at a time.
fn liquid_click(m: &mut OpenMenu, dim: &mut Dim, c: &'static Content, player: u64, slot: u32) {
    let (Some(s), cursor) = (m.slots.get(slot as usize).copied(), m.cursor) else { return };
    if cursor.is_empty() || s.flags & sf::LOCKED != 0 {
        return;
    }
    let here = get(m, dim, player, slot);
    let capacity = if s.limit > 0 { s.limit } else { CONTAINER_MB };
    let (new_slot, new_cursor) = if let Some(&(l, empty)) = c.item_liquid.get(&cursor.item) {
        // Full containers don't stack; one at a time.
        if cursor.count != 1 || s.flags & sf::OUTPUT != 0 {
            return;
        }
        let fluid = c.fluid_items.get(l as usize).copied().unwrap_or(0);
        if fluid == 0 || !fits(c, &s, fluid) || (!here.is_empty() && here.item != fluid) || here.count + CONTAINER_MB > capacity {
            return;
        }
        let poured = ItemStack { item: fluid, count: here.count + CONTAINER_MB, ..here };
        (poured, if empty == 0 { ItemStack::EMPTY } else { ItemStack::new(empty, 1) })
    } else {
        let Some(&l) = c.item_fluid.get(&here.item) else { return };
        let Some(&filled) = c.containers.get(&(cursor.item, l)) else { return };
        if here.count < CONTAINER_MB {
            return;
        }
        let left = here.count - CONTAINER_MB;
        let rest = if left == 0 { ItemStack::EMPTY } else { ItemStack { count: left, ..here } };
        if cursor.count == 1 {
            (rest, ItemStack::new(filled, 1))
        } else {
            // One of a stack of empty ones: the full one goes into the inventory.
            if !dim
                .entities
                .insert_item(c, player, ItemStack::new(filled, 1))
                .is_empty()
            {
                return;
            }
            (rest, ItemStack { count: cursor.count - 1, ..cursor })
        }
    };
    let taken = new_slot.count < here.count;
    set(m, dim, player, slot, new_slot);
    m.cursor = new_cursor;
    if taken && s.flags & sf::OUTPUT != 0 {
        call(m, dim, c, player, slot, |d| d.on_take);
    }
    call(m, dim, c, player, slot, |d| d.on_change);
}

/// `button` 2..=10 over a slot: number key 1..9, swaps it with that hotbar slot (vanilla).
pub const HOTBAR_SWAP: u8 = 2;
/// Double click: gathers matching items from every slot onto the cursor, up to a full stack.
pub const GATHER: u8 = 11;
/// Middle click in creative: a full stack of what is in the slot onto an empty cursor.
pub const CLONE: u8 = 12;

/// `button`: 0 left, 1 right, or `HOTBAR_SWAP + n`, `GATHER`, `CLONE`. `slot == u32::MAX` means outside
/// the window (drop the cursor).
pub fn click(m: &mut OpenMenu, dim: &mut Dim, c: &'static Content, player: u64, slot: u32, button: u8, shift: bool) {
    if m.slots
        .get(slot as usize)
        .is_some_and(|s| s.flags & sf::LIQUID != 0)
    {
        if button <= 1 && !shift {
            liquid_click(m, dim, c, player, slot);
        }
        return;
    }
    if (HOTBAR_SWAP..HOTBAR_SWAP + 9).contains(&button) {
        let hot = (button - HOTBAR_SWAP) as u32;
        if hot >= c.game.player_layout.hotbar {
            return;
        }
        let Some(h) = m
            .slots
            .iter()
            .position(|s| s.owner == Owner::Player && s.index == hot)
            .map(|i| i as u32)
        else {
            return;
        };
        let (Some(a), Some(b)) = (m.slots.get(slot as usize).copied(), m.slots.get(h as usize).copied()) else { return };
        if h == slot || !m.cursor.is_empty() {
            return;
        }
        let (sa, sb) = (get(m, dim, player, slot), get(m, dim, player, h));
        // Results only come out (into an empty hotbar slot), inputs only take in, and fitted
        // slots take what fits.
        let output = a.flags & sf::OUTPUT != 0;
        if (output && !sb.is_empty())
            || (!takeable(&a) && !sa.is_empty())
            || !fits(c, &a, sb.item)
            || !fits(c, &b, sa.item)
            || sb.count > cap(c, &a, sb.item)
        {
            return;
        }
        set(m, dim, player, slot, sb);
        set(m, dim, player, h, sa);
        if output {
            call(m, dim, c, player, slot, |d| d.on_take);
        }
        call(m, dim, c, player, slot, |d| d.on_change);
        call(m, dim, c, player, h, |d| d.on_change);
        return;
    }
    if button == CLONE {
        let creative = dim
            .entities
            .get(player)
            .is_some_and(|e| e.mode.has(crate::tag_bits::game_mode::NO_COSTS));
        let there = if (slot as usize) < m.slots.len() { get(m, dim, player, slot) } else { ItemStack::EMPTY };
        if creative && m.cursor.is_empty() && !there.is_empty() {
            m.cursor = ItemStack { count: c.max_stack(there.item), ..there };
        }
        return;
    }
    if button == GATHER {
        let want = m.cursor;
        if want.is_empty() {
            return;
        }
        let max = c.max_stack(want.item);
        // Partial stacks first, like vanilla, so full ones stay put when they are not needed.
        for full in [false, true] {
            for i in 0..m.slots.len() as u32 {
                if m.cursor.count >= max {
                    break;
                }
                let s = get(m, dim, player, i);
                if m.slots[i as usize].flags & sf::OUTPUT != 0
                    || !takeable(&m.slots[i as usize])
                    || s.is_empty()
                    || s.item != want.item
                    || s.data != want.data
                    || (s.count >= max) != full
                {
                    continue;
                }
                let n = s.count.min(max - m.cursor.count);
                m.cursor.count += n;
                set(m, dim, player, i, if n == s.count { ItemStack::EMPTY } else { ItemStack { count: s.count - n, ..s } });
                call(m, dim, c, player, i, |d| d.on_change);
            }
        }
        return;
    }
    if slot == u32::MAX {
        if !m.cursor.is_empty() {
            let drop = if button == 1 { ItemStack { count: 1, ..m.cursor } } else { m.cursor };
            m.cursor.count -= drop.count;
            if m.cursor.count == 0 {
                m.cursor = ItemStack::EMPTY;
            }
            dim.throw(c, player, drop);
        }
        return;
    }
    let Some(r) = m.slots.get(slot as usize).copied() else { return };
    if r.flags & sf::LOCKED != 0 {
        return;
    }
    let cur = get(m, dim, player, slot);
    let output = r.flags & sf::OUTPUT != 0;
    let max = |item: u32| c.max_stack(item);

    if output {
        if cur.is_empty() {
            return;
        }
        if shift {
            // Craft into the inventory as long as the result stays the same and fits entirely;
            // a result that does not fit is not taken (no partial inserts, no lost ingredients).
            // A furnace's output is real items, and as many as fit go.
            if r.owner != Owner::Temp {
                let rest = dim.entities.insert_item(c, player, cur);
                if rest.count != cur.count {
                    dim.entities.dirty_inv.insert(player);
                    set(m, dim, player, slot, rest);
                    call(m, dim, c, player, slot, |d| d.on_take);
                    call(m, dim, c, player, slot, |d| d.on_change);
                }
                return;
            }
            for _ in 0..max(cur.item).max(64) {
                let now = get(m, dim, player, slot);
                if now.is_empty() || now.item != cur.item || now.data != cur.data {
                    break;
                }
                let backup = dim.entities.inventories.get(&player).cloned();
                if !dim.entities.insert_item(c, player, now).is_empty() {
                    if let Some(b) = backup {
                        dim.entities.inventories.insert(player, b);
                    }
                    break;
                }
                dim.entities.dirty_inv.insert(player);
                set(m, dim, player, slot, ItemStack::EMPTY);
                call(m, dim, c, player, slot, |d| d.on_take);
                call(m, dim, c, player, slot, |d| d.on_change);
            }
            return;
        } else if m.cursor.is_empty() {
            m.cursor = cur;
        } else if m.cursor.item == cur.item && m.cursor.data == cur.data && m.cursor.count + cur.count <= max(cur.item) {
            m.cursor.count += cur.count;
        } else {
            return;
        }
        set(m, dim, player, slot, ItemStack::EMPTY);
        call(m, dim, c, player, slot, |d| d.on_take);
        call(m, dim, c, player, slot, |d| d.on_change);
        return;
    }

    if shift {
        if cur.is_empty() || !takeable(&r) {
            return;
        }
        // Player slots move to the container part (groups with a higher priority first), container
        // slots move to the player part.
        let from_player = r.owner == Owner::Player;
        let mut rest = cur;
        let mut targets: Vec<u32> = (0..m.slots.len() as u32)
            .filter(|&i| {
                let s = m.slots[i as usize];
                // Not into a crafting grid: temporary slots are filled by hand.
                i != slot
                    && s.flags & sf::OUTPUT == 0
                    && s.owner != Owner::Temp
                    && (s.owner == Owner::Player) != from_player
                    && fits(c, &s, cur.item)
            })
            .collect();
        targets.sort_by_key(|&i| -m.slots[i as usize].priority);
        // Slots made for this item (armor) win over generic ones.
        let fitted: Vec<u32> = (0..m.slots.len() as u32)
            .filter(|&i| {
                let s = m.slots[i as usize];
                s.accepts != u32::MAX && fits(c, &s, cur.item) && get(m, dim, player, i).is_empty()
            })
            .collect();
        let layout = &c.game.player_layout;
        let targets = if r.accepts == u32::MAX && !fitted.is_empty() {
            fitted
        } else if targets.is_empty() && from_player {
            // Inventory screen: hotbar <-> main.
            let hotbar = r.index < layout.hotbar;
            (0..m.slots.len() as u32)
                .filter(|&i| {
                    let s = m.slots[i as usize];
                    s.owner == Owner::Player
                        && s.index < layout.slots
                        && !layout.worn.contains(&s.index)
                        && s.flags & (sf::OFF_HAND | sf::LOCKED) == 0
                        && (s.index < layout.hotbar) != hotbar
                })
                .collect()
        } else {
            targets
        };
        for pass in 0..2 {
            for &t in &targets {
                if rest.count == 0 {
                    break;
                }
                let ts = get(m, dim, player, t);
                let room = cap(c, &m.slots[t as usize], rest.item);
                if pass == 0 && !ts.is_empty() && ts.item == rest.item && ts.data == rest.data && ts.count < room {
                    let n = (room - ts.count).min(rest.count);
                    set(m, dim, player, t, ItemStack { count: ts.count + n, ..ts });
                    rest.count -= n;
                } else if pass == 1 && ts.is_empty() {
                    let n = rest.count.min(room);
                    set(m, dim, player, t, ItemStack { count: n, ..rest });
                    rest.count -= n;
                }
            }
        }
        set(m, dim, player, slot, rest);
        call(m, dim, c, player, slot, |d| d.on_change);
        return;
    }

    if !fits(c, &r, m.cursor.item) {
        return;
    }
    let mut cursor = m.cursor;
    let mut slot_stack = cur;
    let same = !cursor.is_empty() && !slot_stack.is_empty() && cursor.item == slot_stack.item && cursor.data == slot_stack.data;
    let room = cap(c, &r, cursor.item);
    // An input slot only takes: anything that would hand items back is refused.
    if !takeable(&r) && !slot_stack.is_empty() && (cursor.is_empty() || !same) {
        return;
    }
    match (button, cursor.is_empty(), slot_stack.is_empty()) {
        (_, true, true) => return,
        (0, true, false) => {
            cursor = slot_stack;
            slot_stack = ItemStack::EMPTY;
        }
        (_, true, false) => {
            let half = slot_stack.count.div_ceil(2);
            cursor = ItemStack { count: half, ..slot_stack };
            slot_stack.count -= half;
        }
        (0, false, true) => {
            let n = cursor.count.min(room);
            slot_stack = ItemStack { count: n, ..cursor };
            cursor.count -= n;
        }
        (_, false, true) => {
            slot_stack = ItemStack { count: 1, ..cursor };
            cursor.count -= 1;
        }
        (0, false, false) if same => {
            let n = room.saturating_sub(slot_stack.count).min(cursor.count);
            slot_stack.count += n;
            cursor.count -= n;
        }
        (_, false, false) if same => {
            if slot_stack.count < room {
                slot_stack.count += 1;
                cursor.count -= 1;
            }
        }
        (_, false, false) if cursor.count <= room => std::mem::swap(&mut cursor, &mut slot_stack),
        (_, false, false) => return,
    }
    if cursor.count == 0 {
        cursor = ItemStack::EMPTY;
    }
    m.cursor = cursor;
    set(m, dim, player, slot, slot_stack);
    call(m, dim, c, player, slot, |d| d.on_change);
}

/// Dragging the cursor stack over `slots`: button 0 splits it evenly, button 1 leaves one item in
/// each. Slots that cannot take the item are skipped; whatever is left stays on the cursor.
pub fn drag(m: &mut OpenMenu, dim: &mut Dim, c: &'static Content, player: u64, slots: &[u32], button: u8) {
    let cur = m.cursor;
    if cur.is_empty() {
        return;
    }
    let mut seen = rustc_hash::FxHashSet::default();
    let valid: Vec<u32> = slots
        .iter()
        .copied()
        .filter(|&s| seen.insert(s))
        .filter(|&s| {
            let Some(r) = m.slots.get(s as usize) else { return false };
            let t = get(m, dim, player, s);
            r.flags & sf::OUTPUT == 0
                && fits(c, r, cur.item)
                && (t.is_empty() || (t.item == cur.item && t.data == cur.data && t.count < cap(c, r, cur.item)))
        })
        .take(cur.count as usize)
        .collect();
    if valid.is_empty() {
        return;
    }
    let per = if button == 1 { 1 } else { cur.count / valid.len() as u32 };
    let mut left = cur.count;
    for &s in &valid {
        let t = get(m, dim, player, s);
        let max = cap(c, &m.slots[s as usize], cur.item);
        let n = per.min(max - t.count.min(max)).min(left);
        if n > 0 {
            set(m, dim, player, s, ItemStack { count: t.count + n, ..cur });
            left -= n;
        }
    }
    m.cursor = if left == 0 { ItemStack::EMPTY } else { ItemStack { count: left, ..cur } };
    for &s in &valid {
        call(m, dim, c, player, s, |d| d.on_change);
    }
}

/// Slots and cursor that changed since the last call.
pub fn changes(m: &mut OpenMenu, dim: &Dim, player: u64) -> (Vec<(u32, ItemStack)>, Option<ItemStack>) {
    let n = m.slots.len();
    m.sent_slots
        .resize(n, ItemStack { count: u32::MAX, ..ItemStack::EMPTY });
    let mut slots = Vec::new();
    for i in 0..n as u32 {
        let s = get(m, dim, player, i);
        if m.sent_slots[i as usize] != s {
            m.sent_slots[i as usize] = s;
            slots.push((i, crate::stack::for_client(s)));
        }
    }
    let cursor = (m.sent_cursor != m.cursor).then(|| {
        m.sent_cursor = m.cursor;
        crate::stack::for_client(m.cursor)
    });
    (slots, cursor)
}
