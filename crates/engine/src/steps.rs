//! Steps: code that changes a value. A chain (damage, an attribute, a question) runs its steps in
//! the order of their phases, inside a phase by `after`/`before`, else in load order, and a pack can
//! set the rest by hand (`config/step_order.txt`). Steps and phases are entries, so a mod edits,
//! replaces or removes anyone's, the engine's too.

use rustc_hash::FxHashMap;
use weft_abi::{self as abi, Callback, ModId, entry_kind as ek};

use crate::content::Content;
use crate::entries::{Entry, RawEntry};
use crate::server::Dim;

/// One run of a step: what the value began as, where its phase began, the value, the modifier's
/// number.
#[derive(Clone, Copy, Debug)]
pub struct Calc {
    pub base: f64,
    pub start: f64,
    pub value: f64,
    pub arg: f64,
}

/// An engine step: the dimension, what the value is about, the run.
pub type Native = fn(&mut Dim, &'static Content, &abi::Query, &mut Calc);

#[derive(Clone, Copy)]
pub enum Run {
    Native(Native),
    Mod(ModId, Callback),
}

pub struct StepDef {
    pub name: String,
    /// Stable key in saved modifiers.
    pub hash: u64,
    /// Its phase's place in `Steps::phases`.
    pub phase: u32,
    pub run: Run,
}

/// Every step and phase, and the steps each chain runs.
#[derive(Default)]
pub struct Steps {
    /// Phase names in the order they run.
    pub phases: Vec<String>,
    pub phase_names: FxHashMap<String, u32>,
    /// By the hash of its name, what saved modifiers keep.
    pub phase_hashes: FxHashMap<u64, u32>,
    pub defs: Vec<StepDef>,
    pub names: FxHashMap<String, u32>,
    pub hashes: FxHashMap<u64, u32>,
    /// By chain, the steps every computation of it runs, in order.
    pub chains: FxHashMap<String, Vec<u32>>,
    /// The same for each attribute, by id: they are computed every tick by every body.
    pub attributes: Vec<Vec<u32>>,
}

impl Steps {
    pub fn by_hash(&self, hash: u64) -> Option<&StepDef> {
        self.hashes.get(&hash).map(|&i| &self.defs[i as usize])
    }

    pub fn phase_of_hash(&self, hash: u64) -> Option<u32> {
        self.phase_hashes.get(&hash).copied()
    }
}

fn add(_: &mut Dim, _: &'static Content, _: &abi::Query, s: &mut Calc) {
    s.value += s.arg;
}

fn mul(_: &mut Dim, _: &'static Content, _: &abi::Query, s: &mut Calc) {
    s.value *= s.arg;
}

fn set(_: &mut Dim, _: &'static Content, _: &abi::Query, s: &mut Calc) {
    s.value = s.arg;
}

fn percent(_: &mut Dim, _: &'static Content, _: &abi::Query, s: &mut Calc) {
    s.value += s.start * s.arg;
}

/// Nothing is built or broken in a dimension made not to be edited (`DimensionDef::editable`).
fn fixed_dimension(d: &mut Dim, c: &'static Content, _: &abi::Query, s: &mut Calc) {
    if !c.dims[d.id as usize].editable {
        s.value = 0.0;
    }
}

/// What the attacker holds adds to its own `ATTACK_DAMAGE`, as vanilla's: an item's damage is
/// what it hits for in a bare hand of the attribute's default (1), so a 6 sword adds 5. The
/// stack `q.item` says (data and all while it still holds it), else what is in its hand.
fn weapon(d: &mut Dim, c: &'static Content, q: &abi::Query, s: &mut Calc) {
    let held = d.entities.held(q.entity);
    let stack = if q.item == 0 || held.item == q.item { held } else { abi::ItemStack::new(q.item, 1) };
    let from_item = c
        .stack_value::<f32>(&stack, c.ids.attack_damage)
        .filter(|_| !stack.is_empty());
    if let Some(v) = from_item {
        s.value += v as f64 - c.game.attributes[crate::registry::attr::ATTACK_DAMAGE as usize].default;
    }
}

/// Modern's swing: a hit does less the less it was charged.
fn charge(_: &mut Dim, _: &'static Content, q: &abi::Query, s: &mut Calc) {
    s.value *= 0.2 + 0.8 * (q.charge as f64).powi(2);
}

fn crit(d: &mut Dim, c: &'static Content, q: &abi::Query, s: &mut Calc) {
    if q.crit != 0 {
        s.value *= d.attribute_for(c, crate::registry::attr::CRIT_DAMAGE, q, false);
    }
}

/// Vanilla's: a full swing while falling, off the ground, out of liquids and ladders, not
/// sprinting.
fn vanilla_crit(d: &mut Dim, c: &'static Content, q: &abi::Query, s: &mut Calc) {
    let Some(e) = d.entities.get(q.entity) else { return };
    let here = crate::physics::around(c, &*d.space, e.pos, e.height(c));
    if q.charge > 0.9 && e.fall > 0.0 && !e.on_ground && !here.in_liquid && !here.climbing && !e.sprinting {
        s.value = 1.0;
    }
}

/// Vanilla's: a full swing on the ground with what sweeps, not sprinting, not a crit.
fn vanilla_sweep(d: &mut Dim, c: &'static Content, q: &abi::Query, s: &mut Calc) {
    let Some(e) = d.entities.get(q.entity) else { return };
    let sweeps = c
        .ids
        .sweeps
        .is_some_and(|id| q.item != 0 && c.components.has(crate::components::ITEM, q.item, id));
    if q.charge > 0.9 && sweeps && e.on_ground && !e.sprinting && q.crit == 0 {
        s.value = 1.0;
    }
}

fn sweep_damage(d: &mut Dim, c: &'static Content, q: &abi::Query, s: &mut Calc) {
    s.value = 1.0 + d.attribute_for(c, crate::registry::attr::SWEEPING_DAMAGE_RATIO, q, false) * s.value;
}

/// A full swing while sprinting knocks one more.
fn sprint_knockback(d: &mut Dim, _: &'static Content, q: &abi::Query, s: &mut Calc) {
    if q.charge > 0.9 && d.entities.get(q.entity).is_some_and(|e| e.sprinting) {
        s.value += 1.0;
    }
}

/// What can't be hurt so: a kind immune to the cause, a cause the world's rules turned off, and
/// the invulnerable (`attribute::INVULNERABLE`) unless the cause pierces that.
fn immune(d: &mut Dim, c: &'static Content, q: &abi::Query, s: &mut Calc) {
    use crate::tag_bits::damage as df;
    let Some(kind) = d.entities.get(q.entity).map(|e| e.kind) else { return };
    let Some(def) = c.game.damage_causes.get(q.cause as usize) else { return };
    let immune = c
        .damage_immune
        .get(q.cause as usize)
        .copied()
        .flatten()
        .is_some_and(|t| c.components.has(crate::components::ENTITY, kind, t));
    let off = c.game.damage_rules[q.cause as usize].is_some_and(|r| crate::config::get(r) == 0.0);
    let spared = !def.has(df::PIERCES_INVULNERABLE) && d.attribute(c, q.entity, crate::registry::attr::INVULNERABLE, false) >= 1.0;
    if immune || off || spared {
        s.value = 0.0;
    }
}

/// What it resists comes off: `DAMAGE_RESISTANCE`, and `FIRE_RESISTANCE` of burning. What
/// pierces invulnerability gets through.
fn resistance(d: &mut Dim, c: &'static Content, q: &abi::Query, s: &mut Calc) {
    use crate::tag_bits::damage as df;
    let Some(def) = c.game.damage_causes.get(q.cause as usize) else { return };
    if def.has(df::PIERCES_INVULNERABLE) {
        return;
    }
    let fire = if def.has(df::BURNING) { d.attribute(c, q.entity, crate::registry::attr::FIRE_RESISTANCE, false) } else { 0.0 };
    s.value *= (1.0 - d.attribute(c, q.entity, crate::registry::attr::DAMAGE_RESISTANCE, false)) * (1.0 - fire);
}

/// What can't kill leaves half a heart.
fn not_lethal(d: &mut Dim, c: &'static Content, q: &abi::Query, s: &mut Calc) {
    let lethal = c
        .game
        .damage_causes
        .get(q.cause as usize)
        .is_none_or(|def| !def.has(crate::tag_bits::damage::NOT_LETHAL));
    if !lethal {
        s.value = s.value.min(
            d.entities
                .get(q.entity)
                .map_or(0.0, |e| e.health as f64 - 1.0),
        );
    }
}

/// Recovering while its last hurt's `HURT_COOLDOWN` runs, unless the cause ignores that.
fn hurt_cooldown(d: &mut Dim, c: &'static Content, q: &abi::Query, s: &mut Calc) {
    let ignores = c
        .game
        .damage_causes
        .get(q.cause as usize)
        .is_some_and(|def| def.has(crate::tag_bits::damage::IGNORES_COOLDOWN));
    if !ignores
        && d.entities
            .get(q.entity)
            .is_some_and(|e| e.hurt_cooldown > 0)
    {
        s.value = 1.0;
    }
}

/// Past its `SAFE_FALL`, times its `FALL_DAMAGE_MULTIPLIER`; none for a kind that takes none.
fn safe_fall(d: &mut Dim, c: &'static Content, q: &abi::Query, s: &mut Calc) {
    let Some(kind) = d.entities.get(q.entity).map(|e| e.kind) else { return };
    let safe = d.attribute(c, q.entity, crate::registry::attr::SAFE_FALL, false);
    let mul = d.attribute(c, q.entity, crate::registry::attr::FALL_DAMAGE_MULTIPLIER, false);
    let takes = c.game.entities[kind as usize].is(crate::tag_bits::entity::FALL_DAMAGE);
    s.value = if takes { (s.value - safe).max(0.0) * mul } else { 0.0 };
}

/// Vanilla's arrow: its damage times its speed, rounded up.
fn by_speed(_: &mut Dim, _: &'static Content, q: &abi::Query, s: &mut Calc) {
    s.value = (s.value * q.amount).ceil();
}

/// A kind tagged `immune_effect/<effect>` never takes it.
fn effect_immune(d: &mut Dim, c: &'static Content, q: &abi::Query, s: &mut Calc) {
    let Some(kind) = d.entities.get(q.entity).map(|e| e.kind) else { return };
    let tag = c.effect_immune.get(q.cause as usize).copied().flatten();
    if tag.is_some_and(|t| c.components.has(crate::components::ENTITY, kind, t)) {
        s.value = 0.0;
    }
}

/// The engine's steps: name, the chain it is on (none: modifiers use it), phase, code.
pub const NATIVE: &[(&str, Option<&str>, &str, Native)] = &[
    (abi::step::ADD, None, abi::phase::ADD, add),
    (abi::step::MUL, None, abi::phase::MULTIPLY, mul),
    (abi::step::SET, None, abi::phase::OVERRIDE, set),
    (abi::step::PERCENT, None, abi::phase::PERCENT, percent),
    ("core:fixed_dimension", Some(abi::action::BREAK_BLOCK), abi::phase::BASE, fixed_dimension),
    ("core:fixed_dimension_place", Some(abi::action::PLACE_BLOCK), abi::phase::BASE, fixed_dimension),
    ("core:weapon", Some(abi::attribute::ATTACK_DAMAGE), abi::phase::ADD, weapon),
    ("core:charge", Some(abi::chain::MELEE), abi::phase::MULTIPLY, charge),
    ("core:crit", Some(abi::chain::MELEE), abi::phase::MULTIPLY, crit),
    ("core:vanilla_crit", Some(abi::question::CRIT), abi::phase::BASE, vanilla_crit),
    ("core:vanilla_sweep", Some(abi::question::SWEEP), abi::phase::BASE, vanilla_sweep),
    ("core:sweep_damage", Some(abi::chain::SWEEP_DAMAGE), abi::phase::BASE, sweep_damage),
    ("core:sprint_knockback", Some(abi::attribute::ATTACK_KNOCKBACK), abi::phase::ADD, sprint_knockback),
    ("core:immune", Some(abi::action::HURT), abi::phase::BASE, immune),
    ("core:resistance", Some(abi::chain::DAMAGE), abi::phase::FINAL, resistance),
    ("core:not_lethal", Some(abi::chain::DAMAGE), abi::phase::FINAL, not_lethal),
    ("core:hurt_cooldown", Some(abi::question::RECOVERING), abi::phase::BASE, hurt_cooldown),
    ("core:effect_immune", Some(abi::action::EFFECT), abi::phase::BASE, effect_immune),
    ("core:by_speed", Some(abi::chain::PROJECTILE_DAMAGE), abi::phase::BASE, by_speed),
    ("core:safe_fall", Some(abi::chain::FALL_DAMAGE), abi::phase::BASE, safe_fall),
];

/// The engine's phases and steps as entries, before every mod's.
pub fn engine_entries(kind: u32) -> Vec<RawEntry> {
    use abi::comp::{phase as p, step as s};
    match kind {
        ek::PHASE => abi::phase::ALL
            .iter()
            .enumerate()
            .map(|(i, name)| match i {
                0 => RawEntry::engine(kind, name),
                _ => RawEntry::engine(kind, name).set(p::AFTER, abi::phase::ALL[i - 1].to_string()),
            })
            .collect(),
        ek::STEP => NATIVE
            .iter()
            .map(|&(name, chain, phase, _)| {
                let e = RawEntry::engine(kind, name).set(s::PHASE, phase.to_string());
                match chain {
                    Some(chain) => e.set(s::CHAIN, chain.to_string()),
                    None => e,
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Phases in order: each right after the one it names, after the ones that named it before
/// (and theirs), ahead of the engine's next.
pub fn order_phases(entries: &[Entry], errors: &mut Vec<String>) -> Vec<String> {
    fn after(e: &Entry) -> Option<&str> {
        e.text(abi::comp::phase::AFTER).filter(|a| !a.is_empty())
    }
    let known: FxHashMap<&str, ()> = entries.iter().map(|e| (e.name.as_str(), ())).collect();
    let mut children: FxHashMap<&str, Vec<&Entry>> = FxHashMap::default();
    let mut roots = Vec::new();
    for e in entries {
        match after(e) {
            Some(a) if known.contains_key(a) => children.entry(a).or_default().push(e),
            Some(a) => errors.push(format!("phase {}: no phase {a} to come after", e.name)),
            None => roots.push(e),
        }
    }
    // A mod's phase goes right after the one it names, the engine's own next one after it.
    for list in children.values_mut() {
        list.sort_by_key(|e| e.owner == crate::registry::ENGINE);
    }
    let mut out = Vec::new();
    let mut stack: Vec<&Entry> = roots.into_iter().rev().collect();
    while let Some(e) = stack.pop() {
        if out.contains(&e.name) {
            continue;
        }
        out.push(e.name.clone());
        stack.extend(children.get(e.name.as_str()).into_iter().flatten().rev());
    }
    for e in entries.iter().filter(|e| !out.contains(&e.name)) {
        if after(e).is_some_and(|a| known.contains_key(a)) {
            errors.push(format!("phase {} comes after itself through others", e.name));
        }
    }
    out
}

/// What a step entry says, before ordering.
struct Found {
    id: u32,
    chain: Option<String>,
    after: Vec<String>,
    before: Vec<String>,
}

/// Steps baked: each one's code (the engine's by name when no mod gave a callback), then every
/// chain's in order. `order_file` is `config/step_order.txt`.
pub fn freeze(phases: Vec<String>, entries: &[Entry], order_file: &str, errors: &mut Vec<String>) -> Steps {
    use abi::comp::step as s;
    let phase_names: FxHashMap<String, u32> = phases
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), i as u32))
        .collect();
    let phase_hashes = phases
        .iter()
        .enumerate()
        .map(|(i, p)| (crate::registry::name_hash(p), i as u32))
        .collect();
    let mut out = Steps { phases, phase_names, phase_hashes, ..Default::default() };
    let mut found = Vec::new();
    for e in entries {
        let phase = e.text(s::PHASE).unwrap_or(abi::phase::ADD);
        let Some(&rank) = out.phase_names.get(phase) else {
            errors.push(format!("step {}: no phase {phase}", e.name));
            continue;
        };
        let native = NATIVE.iter().find(|n| n.0 == e.name).map(|n| n.3);
        let run = match (e.callback(s::CALL), native) {
            (Some((owner, cb)), _) => Run::Mod(owner, cb),
            (None, Some(f)) => Run::Native(f),
            (None, None) => {
                errors.push(format!("step {} has no code", e.name));
                continue;
            }
        };
        let id = out.defs.len() as u32;
        let hash = crate::registry::name_hash(&e.name);
        out.defs
            .push(StepDef { name: e.name.clone(), hash, phase: rank, run });
        out.names.insert(e.name.clone(), id);
        out.hashes.insert(hash, id);
        let list = |name: &str| e.get::<Vec<String>>(name).unwrap_or_default();
        found.push(Found {
            id,
            chain: e.text(s::CHAIN).filter(|c| !c.is_empty()).map(String::from),
            after: list(s::AFTER),
            before: list(s::BEFORE),
        });
    }
    let mut by_chain: FxHashMap<String, Vec<&Found>> = FxHashMap::default();
    for f in &found {
        if let Some(c) = &f.chain {
            by_chain.entry(c.clone()).or_default().push(f);
        }
    }
    let hand = hand_order(order_file, errors);
    for (chain, steps) in by_chain {
        let mut list = order_chain(&out, &chain, &steps, errors);
        if let Some(wanted) = hand.get(&chain) {
            apply_hand_order(&out, &mut list, wanted, &chain, errors);
            // A hand order reorders inside a phase, never across.
            list.sort_by_key(|&s| out.defs[s as usize].phase);
        }
        out.chains.insert(chain, list);
    }
    out
}

/// A chain's steps by phase; inside one, each after what it says it follows (and before what
/// says it follows it), else in load order.
fn order_chain(steps: &Steps, chain: &str, found: &[&Found], errors: &mut Vec<String>) -> Vec<u32> {
    let name = |id: u32| steps.defs[id as usize].name.as_str();
    let mut out = Vec::new();
    for rank in 0..steps.phases.len() as u32 {
        let here: Vec<&&Found> = found
            .iter()
            .filter(|f| steps.defs[f.id as usize].phase == rank)
            .collect();
        // `a` must run before `b`.
        let mut edges: Vec<(u32, u32)> = Vec::new();
        for f in &here {
            for (list, first) in [(&f.after, false), (&f.before, true)] {
                for other in list {
                    match here.iter().find(|o| name(o.id) == other) {
                        Some(o) if first => edges.push((f.id, o.id)),
                        Some(o) => edges.push((o.id, f.id)),
                        // Another phase, or not loaded: nothing to keep apart from here.
                        None if !steps.names.contains_key(other) => {
                            log::debug!(target: "modifiers", "step {}: no step {other} to order by", name(f.id))
                        }
                        None => {}
                    }
                }
            }
        }
        let mut left: Vec<u32> = here.iter().map(|f| f.id).collect();
        while !left.is_empty() {
            let next = left
                .iter()
                .position(|&s| !edges.iter().any(|&(a, b)| b == s && left.contains(&a)));
            match next {
                Some(i) => out.push(left.remove(i)),
                None => {
                    let names: Vec<&str> = left.iter().map(|&s| name(s)).collect();
                    errors.push(format!("chain {chain}: steps {} each want to run before another", names.join(", ")));
                    out.append(&mut left);
                }
            }
        }
    }
    out
}

/// `chain: step, step, ..` per line.
fn hand_order(src: &str, errors: &mut Vec<String>) -> FxHashMap<String, Vec<String>> {
    let mut out = FxHashMap::default();
    for line in src
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        match line.split_once(": ") {
            Some((chain, steps)) => {
                out.insert(
                    chain.trim().to_string(),
                    steps
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                );
            }
            None => errors.push(format!("step_order.txt: '{line}' is not 'chain: step, step'")),
        }
    }
    out
}

/// The steps named go in the places those steps held, in the order given.
fn apply_hand_order(steps: &Steps, list: &mut [u32], wanted: &[String], chain: &str, errors: &mut Vec<String>) {
    let mut ids = Vec::new();
    for w in wanted {
        match steps.names.get(w).filter(|id| list.contains(id)) {
            Some(&id) => ids.push(id),
            None => errors.push(format!("step_order.txt: chain {chain} has no step {w}")),
        }
    }
    let places: Vec<usize> = (0..list.len())
        .filter(|&i| ids.contains(&list[i]))
        .collect();
    for (place, id) in places.into_iter().zip(ids) {
        list[place] = id;
    }
}
