//! How mods hook into what happens without replacing engine code: notification events
//! (batched, delivered at the end of the tick) and chains of steps (`crate::steps`), which are
//! also how questions are answered and how attributes come about.

use std::cell::RefCell;
use std::sync::atomic::Ordering;

use weft_abi as abi;

use super::dim::Dim;
use crate::content::{Content, SourceMod};
use crate::steps::{Calc, Run};

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

/// One step saved on an entity's attribute (`set_attr_modifier`, and what its sources give).
#[derive(Clone, Debug, PartialEq)]
pub struct AttrMod {
    pub attr: u64,
    pub step: u64,
    /// 0: the step's own phase.
    pub phase: u64,
    pub amount: f64,
    /// World time it expires at, 0 = never.
    pub until: u64,
    pub id: String,
}

/// What a step adds onto a run beside its chain's own: (step, phase, its number), and when
/// explaining, whose it is.
struct Extra {
    step: u32,
    phase: u32,
    arg: f64,
    from: String,
}

/// The id of what an entity's sources give it (`keep_sources`).
const SOURCE: &str = "core:source";
/// Levels of an effect whose modifier runs once a level, at most.
const MAX_LEVELS: u32 = 255;

const HEAD: usize = 42;

/// Visits the encoded modifiers without decoding them into a list (attributes are read every
/// tick by every walking mob); `id` is left empty, its bytes come beside.
fn each_mod(b: &[u8], mut f: impl FnMut(&AttrMod, &[u8])) {
    let mut i = 0;
    let word = |at: usize| u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
    let mut m = AttrMod { attr: 0, step: 0, phase: 0, amount: 0.0, until: 0, id: String::new() };
    while i + HEAD <= b.len() {
        (m.attr, m.step, m.phase, m.amount, m.until) = (word(i), word(i + 8), word(i + 16), f64::from_bits(word(i + 24)), word(i + 32));
        let n = u16::from_le_bytes([b[i + 40], b[i + 41]]) as usize;
        let Some(id) = b.get(i + HEAD..i + HEAD + n) else { break };
        f(&m, id);
        i += HEAD + n;
    }
}

pub fn decode_mods(b: &[u8]) -> Vec<AttrMod> {
    let mut out = Vec::new();
    each_mod(b, |m, id| out.push(AttrMod { id: String::from_utf8_lossy(id).into_owned(), ..m.clone() }));
    out
}

pub fn encode_mods(m: &[AttrMod]) -> Box<[u8]> {
    let mut b = Vec::new();
    for x in m {
        for w in [x.attr, x.step, x.phase, x.amount.to_bits(), x.until] {
            b.extend_from_slice(&w.to_le_bytes());
        }
        let n = x.id.len().min(u16::MAX as usize);
        b.extend_from_slice(&(n as u16).to_le_bytes());
        b.extend_from_slice(&x.id.as_bytes()[..n]);
    }
    b.into_boxed_slice()
}

/// An entity going to a dimension whose clock is `by` ticks ahead: what it keeps by world time
/// (its effects' ends, its modifiers' ends, when its keys went down) is as far from now there.
pub(super) fn rebase_clock(c: &Content, e: &mut super::entity::Entity, comps: &mut [(u32, Box<[u8]>)], by: i64) {
    crate::world::rebase_effects(comps, c.game.fields.effect, by);
    for (_, v) in comps
        .iter_mut()
        .filter(|(k, _)| *k == c.game.modifiers_comp)
    {
        let mods: Vec<AttrMod> = decode_mods(v)
            .into_iter()
            .map(|m| AttrMod { until: if m.until == 0 { 0 } else { m.until.saturating_add_signed(by).max(1) }, ..m })
            .collect();
        *v = encode_mods(&mods);
    }
    for k in &mut e.keys {
        k.1 = k.1.saturating_add_signed(by);
    }
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

    /// Question `question` about `q`: its chain from yes (1). Every step answers no, keeps or
    /// yes; the last that doesn't keep wins.
    pub fn ask(&mut self, c: &'static Content, question: &str, q: &abi::Query) -> bool {
        self.ask_or(c, question, q, true)
    }

    /// `ask` starting from `default` where nobody says either way.
    pub fn ask_or(&mut self, c: &'static Content, question: &str, q: &abi::Query, default: bool) -> bool {
        let list = Self::chain_steps(c, question);
        self.run(c, question, list, if default { 1.0 } else { 0.0 }, q, &[], false) != 0.0
    }

    /// Runs value chain `chain` from `base`; with `explain` the steps go to `explanation`.
    pub fn compute(&mut self, c: &'static Content, chain: &str, base: f64, q: &abi::Query, explain: bool) -> f64 {
        self.run(c, chain, Self::chain_steps(c, chain), base, q, &[], explain)
    }

    fn chain_steps<'a>(c: &'a Content, chain: &str) -> &'a [u32] {
        c.game.steps.chains.get(chain).map_or(&[], |v| &v[..])
    }

    /// A chain's own steps (`list`) and `extra` ones (what an entity has on it), phase by phase:
    /// in each, the chain's first, then the extra in their order.
    #[allow(clippy::too_many_arguments)]
    fn run(&mut self, c: &'static Content, chain: &str, list: &[u32], base: f64, q: &abi::Query, extra: &[Extra], explain: bool) -> f64 {
        let steps = &c.game.steps;
        let mut text = explain.then(|| format!("{chain}: {base}"));
        let mut value = base;
        if !list.is_empty() || !extra.is_empty() {
            let mut next = 0;
            for rank in 0..steps.phases.len() as u32 {
                let start = value;
                let mine = list[next..]
                    .iter()
                    .take_while(|&&s| steps.defs[s as usize].phase == rank)
                    .count();
                let theirs = extra.iter().filter(|x| x.phase == rank);
                let runs = list[next..next + mine]
                    .iter()
                    .map(|&s| (s, 0.0, ""))
                    .chain(theirs.map(|x| (x.step, x.arg, x.from.as_str())));
                for (s, arg, from) in runs {
                    let before = value;
                    let mut calc = Calc { base, start, value, arg };
                    match steps.defs[s as usize].run {
                        Run::Native(f) => f(self, c, q, &mut calc),
                        Run::Mod(owner, cb) => {
                            let a = self.call(owner, cb, |w| abi::StepArgs {
                                world: w,
                                chain: abi::Str::new(chain),
                                ctx: q,
                                base,
                                start,
                                value,
                                arg,
                            });
                            calc.value = a.value;
                        }
                    }
                    if calc.value.is_finite() {
                        value = calc.value;
                    }
                    if let Some(t) = &mut text {
                        if value != before {
                            let def = &steps.defs[s as usize];
                            let from = if from.is_empty() { String::new() } else { format!(" of {from}") };
                            t.push_str(&format!(", {} {}{from}: {before} -> {value}", steps.phases[rank as usize], def.name));
                        }
                    }
                }
                next += mine;
            }
        }
        if let Some(t) = text {
            set_explanation(format!("{t} = {value}"));
        }
        value
    }

    /// Its kind's own base of an attribute, else the world's rule if the attribute is one
    /// (`comp::attribute::RULE`), else the attribute's default.
    fn base_attribute(c: &Content, kind: u32, attr: u32) -> f64 {
        let def = &c.game.attributes[attr as usize];
        let own = def
            .comp
            .and_then(|id| c.components.get::<f64>(crate::components::ENTITY, kind, id));
        let rule = c.game.attr_rules[attr as usize].map(crate::config::get);
        own.or(rule).unwrap_or(def.default)
    }

    /// What is saved on an entity: what its sources give first (`keep_sources`), then its own.
    fn modifiers_of(&self, c: &Content, entity: u64) -> Vec<AttrMod> {
        self.entities
            .comps
            .get(&(entity, c.game.modifiers_comp))
            .map_or_else(Vec::new, |b| decode_mods(b))
    }

    fn put_modifiers(&mut self, c: &Content, entity: u64, mods: &[AttrMod]) {
        let key = (entity, c.game.modifiers_comp);
        if mods.is_empty() {
            self.entities.comps.remove(&key);
        } else {
            self.entities.comps.insert(key, encode_mods(mods));
        }
    }

    /// The steps an entity has on attribute `attr`: its kind's, then what is saved on it; with
    /// `explain` whose each is.
    fn attribute_steps(&self, c: &Content, kind: u32, entity: u64, attr: u32, explain: bool) -> Vec<Extra> {
        let steps = &c.game.steps;
        let def = &c.game.attributes[attr as usize];
        let mut out: Vec<Extra> = c
            .source_mods
            .kinds
            .get(&kind)
            .into_iter()
            .flatten()
            .filter(|m| m.when.is_none() && m.attr == attr)
            .map(|m| Extra { step: m.step, phase: m.phase, arg: m.amount, from: if explain { "its kind".into() } else { String::new() } })
            .collect();
        if let Some(b) = self.entities.comps.get(&(entity, c.game.modifiers_comp)) {
            each_mod(b, |m, id| {
                if m.attr != def.hash || (m.until != 0 && m.until <= self.time) {
                    return;
                }
                let Some(&step) = steps.hashes.get(&m.step) else { return };
                let phase = match m.phase {
                    0 => Some(steps.defs[step as usize].phase),
                    p => steps.phase_of_hash(p),
                };
                if let Some(phase) = phase {
                    let from = if explain { String::from_utf8_lossy(id).into_owned() } else { String::new() };
                    out.push(Extra { step, phase, arg: m.amount, from });
                }
            });
        }
        out
    }

    /// Value of attribute `attr` for an entity; with `explain` the steps go to `explanation`.
    pub fn attribute(&mut self, c: &'static Content, entity: u64, attr: u32, explain: bool) -> f64 {
        self.attribute_for(c, attr, &abi::Query { entity, ..abi::Query::DEFAULT }, explain)
    }

    /// `attribute` of `q.entity` with more said about what it is for (whom a hit goes at).
    pub fn attribute_for(&mut self, c: &'static Content, attr: u32, q: &abi::Query, explain: bool) -> f64 {
        let def = &c.game.attributes[attr as usize];
        let Some(kind) = self.entities.get(q.entity).map(|e| e.kind) else { return def.default };
        let base = Self::base_attribute(c, kind, attr);
        let list = &c.game.steps.attributes[attr as usize];
        // Most bodies have nothing on most attributes, read every tick: just the base.
        let saved = self
            .entities
            .comps
            .contains_key(&(q.entity, c.game.modifiers_comp));
        let of_kind = || {
            c.source_mods
                .kinds
                .get(&kind)
                .is_some_and(|l| l.iter().any(|m| m.when.is_none() && m.attr == attr))
        };
        if !explain && list.is_empty() && !saved && !of_kind() {
            return base.clamp(def.min, def.max);
        }
        let extra = self.attribute_steps(c, kind, q.entity, attr, explain);
        let v = self.run(c, &def.name, list, base, q, &extra, explain);
        let clamped = v.clamp(def.min, def.max);
        if explain && clamped != v {
            set_explanation(format!("{}, kept to {clamped}", explanation()));
        }
        clamped
    }

    pub fn set_attr_modifier(&mut self, c: &Content, entity: u64, attr: u32, id: &str, step: u64, phase: u64, amount: f64, ticks: u64) {
        if self.entities.get(entity).is_none() {
            return;
        }
        let hash = c.game.attributes[attr as usize].hash;
        let now = self.time;
        let mut mods = self.modifiers_of(c, entity);
        mods.retain(|m| !(m.attr == hash && m.id == id) && (m.until == 0 || m.until > now));
        mods.push(AttrMod { attr: hash, step, phase, amount, until: if ticks == 0 { 0 } else { now + ticks }, id: id.to_string() });
        self.put_modifiers(c, entity, &mods);
    }

    pub fn remove_attr_modifier(&mut self, c: &Content, entity: u64, attr: u32, id: &str) {
        let hash = c.game.attributes[attr as usize].hash;
        let mut mods = self.modifiers_of(c, entity);
        mods.retain(|m| !(m.attr == hash && m.id == id));
        self.put_modifiers(c, entity, &mods);
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

    /// Entity `id`'s modifiers from its game mode, its main hand's stack, its worn stacks, its
    /// effects, its pose and its kind while their conditions hold, saved on it as `core:source`
    /// ahead of its own. An effect's modifier for every level runs once per level.
    pub fn keep_sources(&mut self, c: &'static Content, id: u64) {
        let s = &c.source_mods;
        let Some(e) = self.entities.get(id) else { return };
        let (kind, pose, pos, mode, body) = (e.kind, e.pose, e.pos, e.mode, e.body());
        type Cond = Option<std::sync::Arc<crate::conditions::Expr>>;
        let mut from: Vec<(SourceMod, u32, Cond)> = s
            .kinds
            .get(&kind)
            .into_iter()
            .flatten()
            .filter(|m| m.when.is_some())
            .map(|m| (*m, 1, None))
            .collect();
        let comp = c.game.fields.effect;
        let on: Vec<_> = crate::world::effects_on(self.entities.comps.get(&(id, comp)).map(|b| &b[..]), self.time).collect();
        // An effect's at its level: for each attribute those given for the highest level up to
        // it, else the others once a level.
        for f in &on {
            let all = s.effects.get(&f.id).map_or(&[][..], |v| &v[..]);
            let given = |attr: u32| {
                all.iter()
                    .filter(|o| o.attr == attr && o.level > 0 && o.level <= f.level)
                    .map(|o| o.level)
                    .max()
            };
            from.extend(all.iter().filter_map(|m| match (m.level, given(m.attr)) {
                (l, Some(at)) if l == at => Some((*m, 1, None)),
                (0, None) => Some((*m, f.level, None)),
                _ => None,
            }));
        }
        from.extend(
            s.modes
                .get(&(mode.0 as u32))
                .into_iter()
                .flatten()
                .map(|m| (*m, 1, None)),
        );
        from.extend(
            s.poses
                .get(&(pose.0 as u32))
                .into_iter()
                .flatten()
                .map(|m| (*m, 1, None)),
        );
        let held = self.entities.held(id);
        c.each_stack_modifier(&held, |m, when| {
            if !m.worn {
                from.push((*m, 1, when.cloned()));
            }
        });
        let worn = c.game.entities[kind as usize].worn.clone();
        let inv = self.entities.inventories.get(&id);
        for st in inv
            .and_then(|i| i.get(worn.start as usize..worn.end as usize))
            .into_iter()
            .flatten()
        {
            c.each_stack_modifier(st, |m, when| {
                if m.worn {
                    from.push((*m, 1, when.cloned()));
                }
            });
        }
        let key = (id, c.game.modifiers_comp);
        if from.is_empty() && !self.entities.comps.contains_key(&key) {
            return;
        }
        let ask = crate::conditions::Ask { pos, entity: id, body: Some(body), ..Default::default() };
        // (attribute, step, phase, number) of each, by hash as saved.
        let mut now: Vec<(u64, u64, u64, f64)> = Vec::new();
        for (m, times, when) in from {
            let holds = match (m.when, when) {
                (Some(w), _) => crate::conditions::on_server(self, c, w, &ask),
                (None, Some(e)) => crate::conditions::holds_server(self, c, &e, &ask),
                (None, None) => true,
            };
            if !holds {
                continue;
            }
            let step = &c.game.steps.defs[m.step as usize];
            let phase = if m.phase == step.phase { 0 } else { crate::registry::name_hash(&c.game.steps.phases[m.phase as usize]) };
            let one = (c.game.attributes[m.attr as usize].hash, step.hash, phase, m.amount);
            now.extend(std::iter::repeat_n(one, times.min(MAX_LEVELS) as usize));
        }
        // Unchanged, as on nearly every tick: nothing decoded, nothing written.
        let (mut at, mut same) = (0, true);
        if let Some(b) = self.entities.comps.get(&key) {
            each_mod(b, |m, by| {
                if by == SOURCE.as_bytes() {
                    same &= m.until == 0 && now.get(at) == Some(&(m.attr, m.step, m.phase, m.amount));
                    at += 1;
                }
            });
        }
        if same && at == now.len() {
            return;
        }
        let mut mods: Vec<AttrMod> = now
            .into_iter()
            .map(|(attr, step, phase, amount)| AttrMod { attr, step, phase, amount, until: 0, id: SOURCE.into() })
            .collect();
        mods.extend(
            self.modifiers_of(c, id)
                .into_iter()
                .filter(|m| m.id != SOURCE),
        );
        self.put_modifiers(c, id, &mods);
    }

    /// The only maximum health there is: the attribute (the kind's value plus modifiers).
    pub fn max_health(&mut self, c: &'static Content, entity: u64) -> f32 {
        self.attribute(c, entity, crate::registry::attr::MAX_HEALTH, false) as f32
    }

    /// Attribute by name, the default for unknown entities, NaN for unknown names.
    pub fn attribute_named(&mut self, c: &'static Content, entity: u64, name: &str, explain: bool) -> f64 {
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
