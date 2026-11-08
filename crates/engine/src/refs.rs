//! What mods, files, commands and rich text name entries of any kind with, parsed here and
//! nowhere else:
//! - `base:stone`: that thing (a block with all its states),
//! - `base:furnace[lit=true]`: the block's states with those property values,
//! - `#logs`: everything carrying the tag,
//! - `#ingots/*`: everything carrying any tag the pattern matches (`*` is any text).
//!
//! A name nothing has and a tag pattern no tag matches are errors at load, said with the mod and
//! the call. Recipes have one more form, the bound variable `#ores/{m}` -> `#ingots/{m}`
//! (`registry::expand_variables`).
//!
//! Where a rule asks which blocks (support, spreading, ores, liquid reactions, fills...) it takes
//! a matcher: such references apart by spaces, `!` before one to leave what it names out. A state
//! matches if one of the others names it and none of the `!` ones does; only `!` ones: anything
//! but those. Each text is compiled once into a bit per state (`Matchers`).

use rustc_hash::FxHashMap;

use crate::components::{self as comps, Components};
use crate::content::{BlockDef, split_state};

pub enum Ref<'a> {
    Name(&'a str, Vec<(&'a str, &'a str)>),
    Tags(&'a str),
}

impl<'a> Ref<'a> {
    pub fn parse(text: &'a str) -> Result<Ref<'a>, String> {
        let text = text.trim();
        if let Some(pattern) = text.strip_prefix('#') {
            if pattern.is_empty() {
                return Err("`#` without a tag".into());
            }
            return Ok(Ref::Tags(pattern));
        }
        if text.is_empty() {
            return Err("an empty name".into());
        }
        let (name, props) = split_state(text)?;
        Ok(Ref::Name(name, props))
    }
}

/// Ids of the tags `pattern` matches (`*` any text), an error when there is none.
pub fn tags(c: &Components, pattern: &str) -> Result<Vec<u32>, String> {
    let found: Vec<u32> = c
        .names
        .iter()
        .enumerate()
        .filter(|(i, n)| c.is_tag(*i as u32) && comps::matches(pattern, n))
        .map(|(i, _)| i as u32)
        .collect();
    if found.is_empty() { Err(format!("no tag matches #{pattern}")) } else { Ok(found) }
}

/// One tag (no pattern): conditions and filters that test a single tag.
pub fn tag(c: &Components, text: &str) -> Result<u32, String> {
    let name = tag_name(text)?;
    c.id(name)
        .filter(|&t| c.is_tag(t))
        .ok_or_else(|| format!("no tag #{name}"))
}

/// The name in `#name`, for calls that only take a tag; a bare name is an error since it would
/// read as a block or item anywhere else.
pub fn tag_name(text: &str) -> Result<&str, String> {
    match text.trim().strip_prefix('#') {
        Some(t) if !t.is_empty() => Ok(t),
        _ => Err(format!("`{text}` is not a tag: tags are written #{text}")),
    }
}

use weft_abi::matcher_terms as terms;

/// A bit per block state: what a matcher stands for. A plain `#tag` no block carries takes
/// nothing: tags are a vocabulary, and a rule may name one only another mod's blocks will have.
pub fn compile(text: &str, names: &FxHashMap<String, u32>, blocks: &[BlockDef], c: &Components) -> Result<Vec<u64>, String> {
    let count = blocks.last().map_or(0, |b| b.first_state + b.states);
    let name = |n: &str, props: &[(&str, &str)]| -> Result<Vec<u32>, String> {
        blocks[*names.get(n).ok_or_else(|| format!("unknown block {n}"))? as usize].matching(props)
    };
    compile_in(text, comps::BLOCK, count, c, &name)
}

/// What `text` stands for among the `count` entries of `kind` (a block's are its states), a bit
/// each, read as a matcher. `name` says what a name, with the properties after it, stands for.
pub fn compile_in(
    text: &str,
    kind: usize,
    count: u32,
    c: &Components,
    name: &dyn Fn(&str, &[(&str, &str)]) -> Result<Vec<u32>, String>,
) -> Result<Vec<u64>, String> {
    if terms(text).next().is_none() {
        return Err("an empty reference".into());
    }
    let words = (count as usize).div_ceil(64);
    let (mut yes, mut no, mut positive) = (vec![0u64; words], vec![0u64; words], false);
    for term in terms(text) {
        let (out, t) = match term.strip_prefix('!') {
            Some(t) => (&mut no, t),
            None => {
                positive = true;
                (&mut yes, term)
            }
        };
        let ids = match Ref::parse(t)? {
            Ref::Tags(p) if !p.contains('*') && c.id(p).is_none() => continue,
            Ref::Tags(p) => {
                let t = tags(c, p)?;
                (0..count)
                    .filter(|&i| t.iter().any(|&t| c.has(kind, i, t)))
                    .collect()
            }
            Ref::Name(n, props) => name(n, &props)?,
        };
        for i in ids {
            out[i as usize / 64] |= 1 << (i % 64);
        }
    }
    if !positive {
        yes.fill(u64::MAX);
        if count % 64 != 0 {
            yes[words - 1] = (1 << (count % 64)) - 1;
        }
    }
    yes.iter_mut().zip(&no).for_each(|(y, n)| *y &= !n);
    Ok(yes)
}

/// The entries of `kind` (`count` of them, `names` by name) a reference text stands for: what
/// any place taking several of a kind reads, names and tags alike. `what` names the kind in
/// errors.
pub fn set(text: &str, kind: usize, names: &FxHashMap<String, u32>, count: u32, c: &Components, what: &str) -> Result<Vec<u32>, String> {
    let bits = bits(text, kind, names, count, c, what)?;
    Ok((0..count).filter(|&i| has(&bits, i)).collect())
}

/// `set` as a bit per entry, for what tests it often (a biome per column).
pub fn bits(text: &str, kind: usize, names: &FxHashMap<String, u32>, count: u32, c: &Components, what: &str) -> Result<Vec<u64>, String> {
    let name = |n: &str, props: &[(&str, &str)]| -> Result<Vec<u32>, String> {
        if !props.is_empty() {
            return Err(format!("{n}: a {what} has no properties"));
        }
        names
            .get(n)
            .map(|&i| vec![i])
            .ok_or_else(|| format!("unknown {what} {n}"))
    };
    compile_in(text, kind, count, c, &name)
}

/// The one entry a name stands for, where only one fits (a recipe's result, a loot line): a tag
/// stands for every entry with it, so it is an error here.
pub fn one(text: &str, names: &FxHashMap<String, u32>, what: &str) -> Result<u32, String> {
    match Ref::parse(text)? {
        Ref::Name(n, props) if props.is_empty() => names
            .get(n)
            .copied()
            .ok_or_else(|| format!("unknown {what} {n}")),
        Ref::Name(n, _) => Err(format!("{n}: a {what} has no properties")),
        Ref::Tags(_) => Err(format!("{}: one {what} goes here, name it (a tag stands for several)", text.trim())),
    }
}

#[inline]
pub fn has(set: &[u64], state: u32) -> bool {
    set.get(state as usize / 64)
        .is_some_and(|w| w >> (state % 64) & 1 != 0)
}

/// The states of a set as runs: (first, count).
pub fn runs(set: &[u64], states: u32) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for s in (0..states).filter(|&s| has(set, s)) {
        match out.last_mut() {
            Some((first, n)) if *first + *n == s => *n += 1,
            _ => out.push((s, 1)),
        }
    }
    out
}

/// Every matcher the content names, each text compiled once (`compile`): a check in hot code is
/// one bit test.
#[derive(Default)]
pub struct Matchers {
    by_text: FxHashMap<String, u32>,
    words: usize,
    bits: Vec<u64>,
    /// What fills keep (`keep`, `only`), made at run time: every fill of a world stays in its
    /// list, and most say the same.
    kept: std::sync::Mutex<FxHashMap<(String, String), std::sync::Arc<[u64]>>>,
}

impl Matchers {
    pub fn new(states: usize) -> Matchers {
        Matchers { words: states.div_ceil(64), ..Matchers::default() }
    }

    /// The id of `text`'s matcher, compiled now if it is new. One that fails still gets an id
    /// (matching nothing), so ids handed out before stay in step; the error is said once.
    pub fn add(&mut self, text: &str, names: &FxHashMap<String, u32>, blocks: &[BlockDef], c: &Components) -> Result<u32, String> {
        if let Some(&id) = self.by_text.get(text) {
            return Ok(id);
        }
        let id = self.by_text.len() as u32;
        self.by_text.insert(text.to_string(), id);
        let set = compile(text, names, blocks, c);
        let at = self.bits.len();
        self.bits.resize(at + self.words, 0);
        let set = set?;
        self.bits[at..at + self.words].copy_from_slice(&set[..self.words]);
        Ok(id)
    }

    /// The states a fill leaves as they are: those `keep` takes, and with `only` given those it
    /// doesn't (an empty text: none).
    pub fn kept(
        &self,
        keep: &str,
        only: &str,
        names: &FxHashMap<String, u32>,
        blocks: &[BlockDef],
        c: &Components,
    ) -> Result<std::sync::Arc<[u64]>, String> {
        let key = (keep.to_string(), only.to_string());
        if let Some(k) = self.kept.lock().unwrap().get(&key) {
            return Ok(k.clone());
        }
        let set = |text: &str| match self.by_text.get(text) {
            Some(&m) => Ok(self.set(m).to_vec()),
            None => compile(text, names, blocks, c),
        };
        let mut kept = if keep.is_empty() { vec![0; self.words] } else { set(keep)? };
        if !only.is_empty() {
            kept.iter_mut().zip(set(only)?).for_each(|(k, o)| *k |= !o);
        }
        let kept: std::sync::Arc<[u64]> = kept.into();
        self.kept.lock().unwrap().insert(key, kept.clone());
        Ok(kept)
    }

    pub fn set(&self, m: u32) -> &[u64] {
        let at = m as usize * self.words;
        self.bits.get(at..at + self.words).unwrap_or(&[])
    }

    #[inline]
    pub fn has(&self, m: u32, state: u32) -> bool {
        self.bits
            .get(m as usize * self.words + state as usize / 64)
            .is_some_and(|w| w >> (state % 64) & 1 != 0)
    }
}

/// The blocks (definition index) a reference stands for.
pub fn blocks(text: &str, names: &FxHashMap<String, u32>, blocks: &[BlockDef], c: &Components) -> Result<Vec<u32>, String> {
    let set = compile(text, names, blocks, c)?;
    Ok(blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| (b.first_state..b.first_state + b.states).any(|s| has(&set, s)))
        .map(|(i, _)| i as u32)
        .collect())
}

/// One block state: a name with properties picks that state, the rest at their defaults.
pub fn state(text: &str, names: &FxHashMap<String, u32>, blocks: &[BlockDef]) -> Result<u32, String> {
    match Ref::parse(text)? {
        Ref::Name(name, props) => blocks[*names
            .get(name)
            .ok_or_else(|| format!("unknown block {name}"))? as usize]
            .state_of(&props),
        Ref::Tags(_) => Err(format!("{}: one block goes here, name it (a tag stands for several)", text.trim())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_syntax() {
        assert!(matches!(Ref::parse("base:stone"), Ok(Ref::Name("base:stone", ref p)) if p.is_empty()));
        assert!(
            matches!(Ref::parse("base:furnace[lit=true, facing=east]"), Ok(Ref::Name("base:furnace", ref p)) if p == &[("lit", "true"), ("facing", "east")])
        );
        assert!(matches!(Ref::parse("#ingots/*"), Ok(Ref::Tags("ingots/*"))));
        assert!(Ref::parse("#").is_err());
        assert!(Ref::parse("base:furnace[lit").is_err());
        assert!(Ref::parse("base:furnace[lit]").is_err());
        // A matcher's terms: a state's properties may have spaces between them.
        let t: Vec<&str> = terms(" base:furnace[lit=true, facing=east]  !#logs ").collect();
        assert_eq!(t, ["base:furnace[lit=true, facing=east]", "!#logs"]);
        assert_eq!(tag_name("#plant_soil"), Ok("plant_soil"));
        assert!(tag_name("plant_soil").is_err());
    }
}
