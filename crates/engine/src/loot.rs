//! Loot tables: pools of weighted entries with counts and conditions, for blocks (the table named
//! like the block, by default its own item when harvested), entities (named like the kind) and anything a mod
//! names (chests). Each is an entry (`abi::comp::loot`) other mods edit like any.

use rustc_hash::FxHashMap;
use weft_abi::{self as abi, ItemStack};

#[derive(Clone, Debug)]
pub struct Entry {
    /// 0 = nothing.
    pub item: u32,
    pub weight: u32,
    pub min: u32,
    pub max: u32,
    /// A condition (`Content::conds`) it is rolled under.
    pub when: Option<u32>,
    /// Recipe type that cooks it when the victim burns.
    pub smelt: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Pool {
    pub rolls: (u32, u32),
    pub when: Option<u32>,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub pools: Vec<Pool>,
}

/// What a roll knows about the situation.
#[derive(Clone, Default)]
pub struct Ctx {
    pub tool: ItemStack,
    pub by_player: bool,
    pub on_fire: bool,
    /// The block state mined with `tool` (0 = none, or broken by the world).
    pub mined: u32,
    /// The block state the table is rolled for, however it was broken (0 for none).
    pub state: u32,
}

// ---------------------------------------------------------------- registration

/// What a block without a table of its own drops: its item, once, when harvested.
pub fn harvested(item: &str) -> abi::LootPool {
    let entry = abi::LootEntry { item: item.to_string(), weight: 1, min: 1, max: 1, ..Default::default() };
    abi::LootPool { rolls_min: 1, rolls_max: 1, when: vec!["harvested".into()], entries: vec![entry] }
}

pub struct Resolve<'a> {
    pub items: &'a FxHashMap<String, u32>,
    pub conds: &'a mut crate::conditions::Texts,
}

impl Resolve<'_> {
    fn item(&self, s: &str, errors: &mut Vec<String>, table: &str) -> u32 {
        if s.is_empty() {
            return 0;
        }
        crate::refs::one(s, self.items, "item").unwrap_or_else(|e| {
            errors.push(format!("loot table {table}: {e}"));
            0
        })
    }

    fn pool(&mut self, p: &abi::LootPool, errors: &mut Vec<String>, table: &str) -> Pool {
        let entries = p
            .entries
            .iter()
            .map(|e| Entry {
                item: self.item(&e.item, errors, table),
                weight: e.weight,
                min: e.min,
                max: e.max.max(e.min),
                when: self.conds.all(&e.when),
                smelt: (!e.smelt.is_empty()).then(|| e.smelt.clone()),
            })
            .collect();
        Pool { rolls: (p.rolls_min, p.rolls_max.max(p.rolls_min)), when: self.conds.all(&p.when), entries }
    }

    /// A table from its entry: the pools, then the swaps in order.
    pub fn table(&mut self, e: &crate::entries::Entry, errors: &mut Vec<String>) -> Table {
        use abi::comp::loot as l;
        let mut pools: Vec<Pool> = e
            .or::<Vec<abi::LootPool>>(l::POOLS, Vec::new())
            .iter()
            .map(|p| self.pool(p, errors, &e.name))
            .collect();
        for s in e.or::<Vec<abi::LootSwap>>(l::SWAPS, Vec::new()) {
            let from = self.item(&s.name, errors, &e.name);
            let to = (!s.to.is_empty()).then(|| self.item(&s.to, errors, &e.name));
            for p in &mut pools {
                match to {
                    Some(to) => p
                        .entries
                        .iter_mut()
                        .filter(|x| x.item == from)
                        .for_each(|x| x.item = to),
                    None => p.entries.retain(|x| x.item != from),
                }
            }
        }
        Table { pools }
    }
}

// ---------------------------------------------------------------- rolling

/// Rolls a table; `rng` gives uniform numbers in 0..1 from the region's stream, so results are
/// deterministic, and `holds` answers a condition (`Content::conds`).
pub fn roll(
    c: &crate::content::Content,
    t: &Table,
    ctx: &Ctx,
    rng: &mut dyn FnMut() -> f32,
    holds: &mut dyn FnMut(u32) -> bool,
) -> Vec<ItemStack> {
    let mut out: Vec<ItemStack> = Vec::new();
    for p in &t.pools {
        if !p.when.is_none_or(&mut *holds) {
            continue;
        }
        let rolls = p.rolls.0 + ((rng() * (p.rolls.1 - p.rolls.0 + 1) as f32) as u32).min(p.rolls.1 - p.rolls.0);
        for _ in 0..rolls {
            let ok: Vec<&Entry> = p
                .entries
                .iter()
                .filter(|e| e.when.is_none_or(&mut *holds))
                .collect();
            let total: u32 = ok.iter().map(|e| e.weight).sum();
            if total == 0 {
                continue;
            }
            let mut pick = (rng() * total as f32) as u32;
            let Some(e) = ok.iter().find(|e| {
                if pick < e.weight {
                    true
                } else {
                    pick -= e.weight;
                    false
                }
            }) else {
                continue;
            };
            let n = e.min + ((rng() * (e.max - e.min + 1) as f32) as u32).min(e.max - e.min);
            if e.item == 0 || n == 0 {
                continue;
            }
            let mut item = e.item;
            if let (true, Some(kind)) = (ctx.on_fire, &e.smelt) {
                if let Some(r) = c.game.process(kind, item, &c.components) {
                    let made = r.result();
                    if !made.is_empty() {
                        item = made.item;
                    }
                }
            }
            match out
                .iter_mut()
                .find(|s| s.item == item && s.count + n <= c.max_stack(item))
            {
                Some(s) => s.count += n,
                None => out.push(ItemStack::new(item, n)),
            }
        }
    }
    out
}
