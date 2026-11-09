//! How mods hook into what happens without replacing engine code: notification events
//! (batched, delivered at the end of the tick), interceptors that may forbid an action (any deny
//! wins), value chains of modifiers in stage order, and attributes with timed modifiers.

use std::cell::RefCell;
use std::sync::atomic::Ordering;

use weft_abi::{self as abi, verdict};

use super::dim::Dim;
use crate::content::{Content, SourceMod};

thread_local! {
    static EXPLAIN: RefCell<String> = const { RefCell::new(String::new()) };
}

/// The text of the last explained chain or attribute on this thread.
pub fn explanation() -> String {
    EXPLAIN.with(|e| e.borrow().clone())
}

fn set_explanation(s: String) {
    EXPLAIN.with(|e| *e.borrow_mut() = s);
}

/// Rounds of event delivery per call: handlers may emit more, a loop must end.
const MAX_ROUNDS: usize = 8;

/// One saved attribute modifier.
#[derive(Clone, Debug, PartialEq)]
pub struct AttrMod {
    pub attr: u64,
    pub op: u8,
    pub amount: f64,
    /// World time it expires at, 0 = never.
    pub until: u64,
    pub id: String,
}

/// Visits the encoded modifiers without decoding them into a list (attributes are read every
/// tick by every walking mob).
fn each_mod(b: &[u8], mut f: impl FnMut(u64, u8, f64, u64, &[u8])) {
    let mut i = 0;
    while i + 27 <= b.len() {
        let attr = u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        let op = b[i + 8];
        let amount = f64::from_le_bytes(b[i + 9..i + 17].try_into().unwrap());
        let until = u64::from_le_bytes(b[i + 17..i + 25].try_into().unwrap());
        let n = u16::from_le_bytes([b[i + 25], b[i + 26]]) as usize;
        i += 27;
        let Some(id) = b.get(i..i + n) else { break };
        f(attr, op, amount, until, id);
        i += n;
    }
}

pub fn decode_mods(b: &[u8]) -> Vec<AttrMod> {
    let mut out = Vec::new();
    each_mod(b, |attr, op, amount, until, id| out.push(AttrMod { attr, op, amount, until, id: String::from_utf8_lossy(id).into_owned() }));
    out
}

pub fn encode_mods(m: &[AttrMod]) -> Box<[u8]> {
    let mut b = Vec::new();
    for x in m {
        b.extend_from_slice(&x.attr.to_le_bytes());
        b.push(x.op);
        b.extend_from_slice(&x.amount.to_le_bytes());
        b.extend_from_slice(&x.until.to_le_bytes());
        b.extend_from_slice(&(x.id.len().min(u16::MAX as usize) as u16).to_le_bytes());
        b.extend_from_slice(&x.id.as_bytes()[..x.id.len().min(u16::MAX as usize)]);
    }
    b.into_boxed_slice()
}

impl Dim {
    /// Queues an event value for the end of the tick; values of the wrong size are refused.
    pub fn emit(&mut self, c: &Content, event: u32, bytes: &[u8]) {
        let Some(def) = c.game.events.get(event as usize) else { return };
        if def.handlers.is_empty() {
            return;
        }
        let size = def.size.load(Ordering::Relaxed);
        if size == 0 {
            def.size.store(bytes.len() as u32, Ordering::Relaxed);
        } else if size as usize != bytes.len() {
            log::error!("event {}: value of {} bytes, others had {size}", def.name, bytes.len());
            return;
        }
        self.space
            .event_queue
            .entry(event)
            .or_default()
            .extend_from_slice(bytes);
    }

    /// Engine events by name, with their `abi` struct as the value.
    pub fn emit_engine<T: Copy>(&mut self, c: &Content, name: &str, v: &T) {
        if let Some(&e) = c.game.event_names.get(name) {
            let bytes = unsafe { std::slice::from_raw_parts(v as *const T as *const u8, size_of::<T>()) };
            self.emit(c, e, bytes);
        }
    }

    /// Hands every queued value to the handlers: one call per event and handler with the batch.
    pub fn deliver_events(&mut self, c: &'static Content) {
        for _ in 0..MAX_ROUNDS {
            if self.space.event_queue.is_empty() {
                return;
            }
            let queue = std::mem::take(&mut self.space.event_queue);
            for (event, bytes) in queue {
                let def = &c.game.events[event as usize];
                let stride = def.size.load(Ordering::Relaxed).max(1);
                let count = bytes.len() as u32 / stride;
                for &(owner, cb) in &def.handlers {
                    let data = bytes.as_ptr();
                    self.call(owner, cb, |w| abi::EventArgs { world: w, event, data, count, stride });
                }
            }
        }
        if !self.space.event_queue.is_empty() {
            log::warn!("events still queued after {MAX_ROUNDS} rounds in dimension {}; dropping them", self.id);
            self.space.event_queue.clear();
        }
    }

    /// Whether the interceptors of `question` let it happen: any deny wins.
    pub fn ask(&mut self, c: &'static Content, question: &str, q: &mut abi::Query) -> bool {
        if !c.dims[self.id as usize].editable && (question == abi::action::BREAK_BLOCK || question == abi::action::PLACE_BLOCK) {
            q.verdict = verdict::DENY;
            return false;
        }
        let Some(list) = c.game.interceptors.get(question) else { return true };
        let mut allowed = true;
        for &(owner, cb) in list {
            q.verdict = verdict::ABSTAIN;
            let qp = q as *mut abi::Query;
            self.call(owner, cb, |w| abi::InterceptArgs { world: w, question: abi::Str::new(question), query: qp });
            if q.verdict == verdict::DENY {
                allowed = false;
                break;
            }
        }
        q.verdict = if allowed { verdict::ALLOW } else { verdict::DENY };
        allowed
    }

    /// `ask` where an interceptor may also allow outright: with every one abstaining, `default`.
    pub fn ask_or(&mut self, c: &'static Content, question: &str, q: &mut abi::Query, default: bool) -> bool {
        let Some(list) = c.game.interceptors.get(question) else { return default };
        let mut allowed = None;
        for &(owner, cb) in list {
            q.verdict = verdict::ABSTAIN;
            let qp = q as *mut abi::Query;
            self.call(owner, cb, |w| abi::InterceptArgs { world: w, question: abi::Str::new(question), query: qp });
            match q.verdict {
                verdict::DENY => {
                    allowed = Some(false);
                    break;
                }
                verdict::ALLOW => allowed = Some(true),
                _ => {}
            }
        }
        let allowed = allowed.unwrap_or(default);
        q.verdict = if allowed { verdict::ALLOW } else { verdict::DENY };
        allowed
    }

    /// Runs value chain `chain` from `base`; with `explain` the steps go to `explanation`.
    pub fn compute(&mut self, c: &'static Content, chain: &str, base: f64, q: &abi::Query, explain: bool) -> f64 {
        let Some(list) = c.game.modifiers.get(chain) else {
            if explain {
                set_explanation(format!("{chain}: {base} (no modifiers)"));
            }
            return base;
        };
        let mut value = base;
        let mut text = explain.then(|| format!("{chain}: {base}"));
        for &(stage, owner, cb) in list {
            let before = value;
            let qp = q as *const abi::Query;
            let a = self.call(owner, cb, |w| abi::ModifyArgs { world: w, chain: abi::Str::new(chain), ctx: qp, base, value });
            if a.value.is_finite() {
                value = a.value;
            }
            if let Some(t) = &mut text {
                if value != before {
                    t.push_str(&format!(", {} at stage {stage}: {before} -> {value}", c.mod_name(owner)));
                }
            }
        }
        if let Some(t) = text {
            set_explanation(format!("{t} = {value}"));
        }
        value
    }
}

impl Dim {
    /// Its kind's own base of an attribute, else the world's rule if the attribute is one
    /// (`comp::attribute::RULE`), else the attribute's default.
    fn base_attribute(&self, c: &Content, entity: u64, attr: u32) -> Option<f64> {
        let e = self.entities.get(entity)?;
        let def = &c.game.attributes[attr as usize];
        let own = def.comp.and_then(|id| {
            c.components
                .get::<f64>(crate::components::ENTITY, e.kind, id)
        });
        let rule = c.game.attr_rules[attr as usize].map(crate::config::get);
        Some(own.or(rule).unwrap_or(def.default))
    }

    fn modifiers_of(&self, c: &Content, entity: u64) -> Vec<AttrMod> {
        let mut out = self
            .entities
            .comps
            .get(&(entity, c.game.modifiers_comp))
            .map_or_else(Vec::new, |b| decode_mods(b));
        let kind = self.entities.get(entity).map_or(u32::MAX, |e| e.kind);
        out.extend(
            c.source_mods
                .kinds
                .get(&kind)
                .into_iter()
                .flatten()
                .filter(|m| m.when.is_none())
                .map(|m| AttrMod {
                    attr: c.game.attributes[m.attr as usize].hash,
                    op: m.op as u8,
                    amount: m.amount,
                    until: 0,
                    id: "core:kind".into(),
                }),
        );
        out
    }

    /// Value of attribute `attr` for an entity; with `explain` the steps go to `explanation`.
    pub fn attribute(&self, c: &Content, entity: u64, attr: u32, explain: bool) -> f64 {
        let def = &c.game.attributes[attr as usize];
        let Some(base) = self.base_attribute(c, entity, attr) else { return def.default };
        if !explain {
            let (mut add, mut mul_base, mut mul_total, mut set) = (0.0, 0.0, 1.0, None);
            // A kind's own modifiers hold whatever it is (a dropped stack is invulnerable from
            // its first tick); `keep_sources` keeps the ones under conditions.
            let kind = self.entities.get(entity).map_or(u32::MAX, |e| e.kind);
            for m in c
                .source_mods
                .kinds
                .get(&kind)
                .into_iter()
                .flatten()
                .filter(|m| m.when.is_none() && m.attr == attr)
            {
                match m.op {
                    abi::attr_op::ADD => add += m.amount,
                    abi::attr_op::MULTIPLY_BASE => mul_base += m.amount,
                    abi::attr_op::MULTIPLY_TOTAL => mul_total *= 1.0 + m.amount,
                    _ => set = Some(m.amount),
                }
            }
            if let Some(b) = self.entities.comps.get(&(entity, c.game.modifiers_comp)) {
                each_mod(b, |a, op, amount, until, _| {
                    if a == def.hash && (until == 0 || until > self.time) {
                        match op as u32 {
                            abi::attr_op::ADD => add += amount,
                            abi::attr_op::MULTIPLY_BASE => mul_base += amount,
                            abi::attr_op::MULTIPLY_TOTAL => mul_total *= 1.0 + amount,
                            _ => set = Some(amount),
                        }
                    }
                });
            }
            return set
                .unwrap_or((base + add) * (1.0 + mul_base) * mul_total)
                .clamp(def.min, def.max);
        }
        let mods: Vec<AttrMod> = self
            .modifiers_of(c, entity)
            .into_iter()
            .filter(|m| m.attr == def.hash && (m.until == 0 || m.until > self.time))
            .collect();
        let add: f64 = mods
            .iter()
            .filter(|m| m.op as u32 == abi::attr_op::ADD)
            .map(|m| m.amount)
            .sum();
        let mul_base: f64 = mods
            .iter()
            .filter(|m| m.op as u32 == abi::attr_op::MULTIPLY_BASE)
            .map(|m| m.amount)
            .sum();
        let mut v = (base + add) * (1.0 + mul_base);
        for m in mods
            .iter()
            .filter(|m| m.op as u32 == abi::attr_op::MULTIPLY_TOTAL)
        {
            v *= 1.0 + m.amount;
        }
        let set = mods.iter().rev().find(|m| m.op as u32 == abi::attr_op::SET);
        let v = set.map_or(v, |m| m.amount).clamp(def.min, def.max);
        if explain {
            let parts: Vec<String> = mods
                .iter()
                .map(|m| {
                    let op = ["+", "x base +", "x", "= "][m.op.min(3) as usize];
                    let left = if m.until == 0 { String::new() } else { format!(" ({} ticks left)", m.until - self.time) };
                    format!("{} {op}{}{left}", m.id, m.amount)
                })
                .collect();
            set_explanation(format!("{}: base {base}{}{} = {v}", def.name, if parts.is_empty() { "" } else { ", " }, parts.join(", ")));
        }
        v
    }

    pub fn set_attr_modifier(&mut self, c: &Content, entity: u64, attr: u32, id: &str, op: u32, amount: f64, ticks: u64) {
        if self.entities.get(entity).is_none() {
            return;
        }
        let hash = c.game.attributes[attr as usize].hash;
        let mut mods = self.modifiers_of(c, entity);
        let now = self.time;
        mods.retain(|m| !(m.attr == hash && m.id == id) && (m.until == 0 || m.until > now));
        mods.push(AttrMod { attr: hash, op: op.min(3) as u8, amount, until: if ticks == 0 { 0 } else { now + ticks }, id: id.to_string() });
        self.entities
            .comps
            .insert((entity, c.game.modifiers_comp), encode_mods(&mods));
    }

    pub fn remove_attr_modifier(&mut self, c: &Content, entity: u64, attr: u32, id: &str) {
        let hash = c.game.attributes[attr as usize].hash;
        let mut mods = self.modifiers_of(c, entity);
        mods.retain(|m| !(m.attr == hash && m.id == id));
        if mods.is_empty() {
            self.entities.comps.remove(&(entity, c.game.modifiers_comp));
        } else {
            self.entities
                .comps
                .insert((entity, c.game.modifiers_comp), encode_mods(&mods));
        }
    }

    /// The modifiers what entities are in, hold, wear, do and are give them
    /// (`Content::source_mods`), kept up each tick (`keep_sources`).
    pub(super) fn source_modifiers(&mut self, c: &'static Content) {
        if c.source_mods.is_empty() {
            return;
        }
        let ids: Vec<u64> = self
            .entities
            .list
            .iter()
            .filter(|e| !e.removed && e.is_body())
            .map(|e| e.id)
            .collect();
        for id in ids {
            self.keep_sources(c, id);
        }
    }

    /// Entity `id`'s modifiers from its game mode, its main hand's item, its worn items, its pose
    /// and its kind while their conditions hold: one per attribute and operation,
    /// `core:source/<op>`.
    pub fn keep_sources(&mut self, c: &'static Content, id: u64) {
        let s = &c.source_mods;
        let Some(e) = self.entities.get(id) else { return };
        let (kind, pose, pos, mode, body) = (e.kind, e.pose, e.pos, e.mode, e.body());
        let mut from: Vec<SourceMod> = s
            .kinds
            .get(&kind)
            .into_iter()
            .flatten()
            .filter(|m| m.when.is_some())
            .cloned()
            .collect();
        let comp = c.game.fields.effect;
        let on: Vec<_> = crate::world::effects_on(self.entities.comps.get(&(id, comp)).map(|b| &b[..]), self.time).collect();
        if on.len() * 16 != self.entities.comps.get(&(id, comp)).map_or(0, |b| b.len()) {
            // Emptied, not removed: a removed component never reaches clients.
            self.entities
                .comps
                .insert((id, comp), crate::world::encode_effects(&on));
            self.entities.dirty_comp.insert((id, comp));
        }
        // An effect's at its level: for each attribute those given for the highest level up to
        // it, else the others times the level (what is set is the value at every level).
        for f in &on {
            let all = s.effects.get(&f.id).map_or(&[][..], |v| &v[..]);
            let given = |attr: u32| {
                all.iter()
                    .filter(|o| o.attr == attr && o.level > 0 && o.level <= f.level)
                    .map(|o| o.level)
                    .max()
            };
            from.extend(all.iter().filter_map(|m| match (m.level, given(m.attr)) {
                (l, Some(at)) if l == at => Some(*m),
                (0, None) => Some(SourceMod { amount: if m.op == abi::attr_op::SET { m.amount } else { m.amount * f.level as f64 }, ..*m }),
                _ => None,
            }));
        }
        from.extend(s.modes.get(&(mode.0 as u32)).into_iter().flatten());
        from.extend(s.poses.get(&(pose.0 as u32)).into_iter().flatten());
        let held = self.entities.held(id).item;
        from.extend(s.items.get(&held).into_iter().flatten().filter(|m| !m.worn));
        let worn = c.game.entities[kind as usize].worn.clone();
        let inv = self.entities.inventories.get(&id);
        for st in inv
            .and_then(|i| i.get(worn.start as usize..worn.end as usize))
            .into_iter()
            .flatten()
        {
            from.extend(
                s.items
                    .get(&st.item)
                    .into_iter()
                    .flatten()
                    .filter(|m| m.worn),
            );
        }
        if from.is_empty()
            && !self
                .entities
                .comps
                .contains_key(&(id, c.game.modifiers_comp))
        {
            return;
        }
        let ask = crate::conditions::Ask { pos, entity: id, body: Some(body), ..Default::default() };
        let mut want: Vec<(u32, u32, f64)> = Vec::new();
        for m in from {
            if m.when
                .is_some_and(|w| !crate::conditions::on_server(self, c, w, &ask))
            {
                continue;
            }
            match want.iter_mut().find(|w| (w.0, w.1) == (m.attr, m.op)) {
                Some(w) if m.op == abi::attr_op::MULTIPLY_TOTAL => w.2 = (1.0 + w.2) * (1.0 + m.amount) - 1.0,
                Some(w) if m.op == abi::attr_op::SET => w.2 = m.amount,
                Some(w) => w.2 += m.amount,
                None => want.push((m.attr, m.op, m.amount)),
            }
        }
        let mut mods = self.modifiers_of(c, id);
        let ours = |m: &AttrMod| m.id.starts_with("core:source/");
        let had: Vec<(u64, u8, f64)> = mods
            .iter()
            .filter(|m| ours(m))
            .map(|m| (m.attr, m.op, m.amount))
            .collect();
        let now: Vec<(u64, u8, f64)> = want
            .iter()
            .map(|&(a, op, x)| (c.game.attributes[a as usize].hash, op.min(3) as u8, x))
            .collect();
        if had.len() == now.len() && now.iter().all(|w| had.contains(w)) {
            return;
        }
        mods.retain(|m| !ours(m));
        mods.extend(
            now.into_iter()
                .map(|(attr, op, amount)| AttrMod { attr, op, amount, until: 0, id: format!("core:source/{op}") }),
        );
        if mods.is_empty() {
            self.entities.comps.remove(&(id, c.game.modifiers_comp));
        } else {
            self.entities
                .comps
                .insert((id, c.game.modifiers_comp), encode_mods(&mods));
        }
    }

    /// The only maximum health there is: the attribute (the kind's value plus modifiers).
    pub fn max_health(&self, c: &Content, entity: u64) -> f32 {
        self.attribute(c, entity, crate::registry::attr::MAX_HEALTH, false) as f32
    }

    /// Attribute by name, the default for unknown entities, NaN for unknown names.
    pub fn attribute_named(&self, c: &Content, entity: u64, name: &str, explain: bool) -> f64 {
        match c.game.attr_names.get(name) {
            Some(&a) => self.attribute(c, entity, a, explain),
            None => f64::NAN,
        }
    }

    /// Rolls a loot table with this region's random stream.
    /// Rolls `table`; conditions it asks are answered about `pos` and `entity` first.
    pub fn roll_loot(
        &mut self,
        c: &'static Content,
        table: &str,
        ctx: &crate::loot::Ctx,
        pos: glam::DVec3,
        entity: u64,
    ) -> Vec<weft_abi::ItemStack> {
        let Some(t) = c.loot.get(table) else { return Vec::new() };
        self.roll_table(c, t, ctx, pos, entity)
    }

    pub fn roll_table(
        &mut self,
        c: &'static Content,
        t: &crate::loot::Table,
        ctx: &crate::loot::Ctx,
        pos: glam::DVec3,
        entity: u64,
    ) -> Vec<weft_abi::ItemStack> {
        let ask = crate::conditions::Ask { pos, entity, state: ctx.state, loot: Some(ctx), ..Default::default() };
        let me = std::cell::RefCell::new(self);
        let mut rng = || me.borrow_mut().random_unit();
        let mut holds = |id| crate::conditions::on_server(&mut me.borrow_mut(), c, id, &ask);
        crate::loot::roll(c, t, ctx, &mut rng, &mut holds)
    }
}
