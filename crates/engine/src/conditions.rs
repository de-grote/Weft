//! Conditions (`entry_kind::CONDITION`): one predicate language for music, `{if:..}` in text,
//! loot tables, natural spawning, poses and camera modes, and for mods (`w.holds`). A condition
//! is a record with maybe an argument, `core:light(..7)`, put together with `&`, `|`, `!` and
//! parentheses; the engine's go without their namespace (`night & light(..7)`). Every distinct
//! text compiles once, at freeze (`Texts`), into an `Expr` its users keep the id of. The
//! engine's records are answered here; a mod's by its callback, with its argument as text.

use glam::DVec3;
use rustc_hash::FxHashMap;
use weft_abi::{self as abi, condition as k};

use crate::content::Content;
use crate::registry::Answer;
use crate::server::Dim;

#[derive(Clone, Debug)]
pub enum Expr {
    All(Vec<Expr>),
    Any(Vec<Expr>),
    Not(Box<Expr>),
    /// A record (`ConditionDef` id) and its argument.
    Is(u32, Arg),
}

#[derive(Clone, Debug)]
pub enum Arg {
    None,
    /// Inclusive.
    Range(f32, f32),
    /// A light range on a side of the cell (`FACE_NAMES`; 6 the cell itself, 7 its brightest side).
    LightAt(f32, f32, u8),
    Num(f32),
    /// A block matcher's states, a bit each (`refs::compile`).
    Blocks(Vec<u64>),
    /// A tag's component id.
    Tag(u32),
    Text(String),
    /// A side of the cell (`FACE_DIRS`, 6 for the cell itself) and maybe the blocks wanted there.
    Side(u8, Option<Vec<u64>>),
    /// An attribute and its range, inclusive.
    Attr(u32, f32, f32),
    /// Equipment slots (`PlayerLayout::equipment` order), a bit each.
    Slots(u32),
}

/// What a condition is asked about.
#[derive(Clone, Copy, Default)]
pub struct Ask<'a> {
    pub pos: DVec3,
    /// Who it is about (the killer or breaker for loot), 0 for nobody.
    pub entity: u64,
    /// The block it is about: the one broken for loot, the one under a spawn.
    pub state: u32,
    pub loot: Option<&'a crate::loot::Ctx>,
    /// What the body it is about is doing, when the one asking knows (a pose).
    pub body: Option<Body>,
    /// The daylight `light` counts with, the world's now unless set (as if it were night).
    pub daylight: Option<f32>,
    /// Sides named (`side(down)`) are of `state`'s own frame, turned as it is (a block's rules).
    pub own_frame: bool,
    /// Whether it may hold some time, not whether it does now: what changes by itself (light,
    /// time, chance, a mod's answer) is not known then.
    pub could: bool,
}

#[derive(Clone, Copy, Default)]
pub struct Body {
    /// In a liquid it swims in.
    pub in_liquid: bool,
    pub flying: bool,
    pub sprinting: bool,
}

/// Condition texts met while freezing, by id, compiled at its end.
#[derive(Default)]
pub struct Texts {
    pub texts: crate::content::Interner,
}

impl Texts {
    pub fn id(&mut self, text: &str) -> u32 {
        self.texts.id(text.trim())
    }

    /// The conditions of a `[str]` component, all of them; `None` for none.
    pub fn all(&mut self, list: &[String]) -> Option<u32> {
        let parts: Vec<String> = list
            .iter()
            .filter(|t| !t.trim().is_empty())
            .map(|t| format!("({})", t.trim()))
            .collect();
        (!parts.is_empty()).then(|| self.id(&parts.join(" & ")))
    }
}

/// Where names and arguments resolve.
pub struct Look<'a> {
    /// Conditions and attributes by name.
    pub game: &'a crate::registry::Game,
    pub block_names: &'a FxHashMap<String, u32>,
    pub blocks: &'a [crate::content::BlockDef],
    pub components: &'a crate::components::Components,
}

pub fn compile(text: &str, look: &Look) -> Result<Expr, String> {
    let mut p = Parser { s: text.as_bytes(), at: 0, look };
    if text.trim().is_empty() {
        return Err("an empty condition".into());
    }
    let e = p.any()?;
    p.space();
    if p.at != p.s.len() {
        return Err(format!("condition {text:?}: unexpected {:?}", &text[p.at..]));
    }
    Ok(e)
}

struct Parser<'a, 'b> {
    s: &'a [u8],
    at: usize,
    look: &'a Look<'b>,
}

impl Parser<'_, '_> {
    fn space(&mut self) {
        while self.s.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.space();
        let hit = self.s.get(self.at) == Some(&c);
        self.at += hit as usize;
        hit
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(self.s).into_owned()
    }

    fn any(&mut self) -> Result<Expr, String> {
        self.joined(b'|', Self::all, Expr::Any)
    }

    fn all(&mut self) -> Result<Expr, String> {
        self.joined(b'&', Self::one, Expr::All)
    }

    /// `part`s with `sep` between them, `join`ed when there are several.
    fn joined(&mut self, sep: u8, part: fn(&mut Self) -> Result<Expr, String>, join: fn(Vec<Expr>) -> Expr) -> Result<Expr, String> {
        let mut v = vec![part(self)?];
        while self.eat(sep) {
            v.push(part(self)?);
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { join(v) })
    }

    fn one(&mut self) -> Result<Expr, String> {
        if self.eat(b'!') {
            return Ok(Expr::Not(Box::new(self.one()?)));
        }
        if self.eat(b'(') {
            let e = self.any()?;
            if !self.eat(b')') {
                return Err(format!("condition {:?}: a ( without its )", self.text()));
            }
            return Ok(e);
        }
        self.space();
        let start = self.at;
        while self
            .s
            .get(self.at)
            .is_some_and(|c| c.is_ascii_alphanumeric() || b"_:/.-".contains(c))
        {
            self.at += 1;
        }
        let name = std::str::from_utf8(&self.s[start..self.at]).unwrap_or_default();
        if name.is_empty() {
            return Err(format!("condition {:?}: a name expected at {}", self.text(), start));
        }
        let mut arg = None;
        if self.s.get(self.at) == Some(&b'(') {
            let (from, mut depth) = (self.at + 1, 0);
            loop {
                match self.s.get(self.at) {
                    None => return Err(format!("condition {:?}: {name}( without its )", self.text())),
                    Some(b'(') => depth += 1,
                    Some(b')') => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                self.at += 1;
            }
            arg = Some(
                std::str::from_utf8(&self.s[from..self.at])
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            );
            self.at += 1;
        }
        let names = &self.look.game.condition_names;
        let id = *names
            .get(name)
            .or_else(|| {
                (!name.contains(':'))
                    .then(|| names.get(&format!("core:{name}")))
                    .flatten()
            })
            .ok_or_else(|| format!("no condition {name}"))?;
        Ok(Expr::Is(id, self.arg(id, name, arg)?))
    }

    /// An engine record's argument as it takes it; a mod's is its text.
    fn arg(&self, id: u32, name: &str, arg: Option<String>) -> Result<Arg, String> {
        // The engine's come first, in `abi::condition` order.
        let engine = (id as usize) < k::NAMES.len();
        let Some(text) = arg else {
            return match id {
                k::LIGHT
                | k::CHANCE
                | k::BLOCK
                | k::STATE
                | k::TOOL
                | k::TOOL_LEVEL
                | k::SIDE
                | k::BESIDE
                | k::OPEN
                | k::ATTRIBUTE
                | k::SKY
                | k::HEIGHT
                | k::WEARS
                    if engine =>
                {
                    Err(format!("condition {name} needs an argument"))
                }
                _ => Ok(Arg::None),
            };
        };
        let num = |t: &str| {
            t.trim()
                .parse::<f32>()
                .map_err(|_| format!("condition {name}({t}): not a number"))
        };
        // `a..b`, `..b`, `a..`, `a`.
        let range = |t: &str| -> Result<Arg, String> {
            let (a, b) = t.split_once("..").unwrap_or((t, t));
            let end = |x: &str, none: f32| if x.trim().is_empty() { Ok(none) } else { num(x) };
            Ok(Arg::Range(end(a, f32::MIN)?, end(b, f32::MAX)?))
        };
        Ok(match id {
            _ if !engine => Arg::Text(text),
            k::SKY | k::HEIGHT => range(&text)?,
            // `light(9.., up)`: on a side of the cell (6: itself, 7: its brightest).
            k::LIGHT => match text.split_once(',') {
                Some((r, side)) => {
                    let Arg::Range(a, b) = range(r)? else { unreachable!() };
                    let side = side.trim();
                    let f = match side {
                        "any" => 7,
                        _ => abi::FACE_NAMES
                            .iter()
                            .position(|n| *n == side)
                            .ok_or_else(|| format!("condition {name}({text}): no side {side}"))? as u8,
                    };
                    Arg::LightAt(a, b, f)
                }
                None => range(&text)?,
            },
            k::ATTRIBUTE => {
                let (attr, r) = text
                    .split_once(',')
                    .ok_or_else(|| format!("condition {name}({text}): an attribute and a range"))?;
                let id = *self
                    .look
                    .game
                    .attr_names
                    .get(attr.trim())
                    .ok_or_else(|| format!("condition {name}({text}): no attribute {attr}"))?;
                let Arg::Range(a, b) = range(r)? else { unreachable!() };
                Arg::Attr(id, a, b)
            }
            k::CHANCE | k::TOOL_LEVEL => Arg::Num(num(&text)?),
            k::WEARS => {
                let what = text.trim();
                let slots = self
                    .look
                    .game
                    .player_layout
                    .equipment
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.name == what || e.bones.iter().any(|b| b == what))
                    .fold(0, |m, (i, _)| m | 1 << i);
                if slots == 0 {
                    return Err(format!("condition {name}({text}): no equipment slot is or covers {what}"));
                }
                Arg::Slots(slots)
            }
            k::BLOCK => Arg::Blocks(crate::refs::compile(&text, self.look.block_names, self.look.blocks, self.look.components)?),
            k::TOOL => Arg::Tag(crate::refs::tag(self.look.components, text.trim_start_matches('#'))?),
            k::STATE => Arg::Text(text),
            k::SIDE | k::BESIDE | k::OPEN => {
                let (first, rest) = text
                    .split_once(',')
                    .map_or((text.as_str(), None), |(a, b)| (a, Some(b.trim())));
                let face = abi::FACE_NAMES.iter().position(|n| *n == first.trim());
                let (face, blocks) = match (face, rest) {
                    (Some(f), m) => (f as u8, m),
                    // `beside(#water)`: around the cell itself.
                    (None, None) if id == k::BESIDE => (6, Some(text.as_str())),
                    _ => return Err(format!("condition {name}({text}): a side (north .. down) first")),
                };
                let blocks = match blocks {
                    Some(m) if id != k::OPEN => {
                        Some(crate::refs::compile(m, self.look.block_names, self.look.blocks, self.look.components)?)
                    }
                    None if id == k::BESIDE => return Err(format!("condition {name}({text}) needs blocks")),
                    None => None,
                    Some(_) => return Err(format!("condition {name}({text}) takes only a side")),
                };
                Arg::Side(face, blocks)
            }
            _ => return Err(format!("condition {name} takes no argument")),
        })
    }
}

/// What can be told without the world: the block, the loot roll, the body.
fn known(c: &Content, id: u32, arg: &Arg, a: &Ask) -> Option<bool> {
    let tool = || a.loot.map_or(abi::ItemStack::EMPTY, |l| l.tool);
    Some(match (id, arg) {
        (k::BLOCK, Arg::Blocks(b)) => crate::refs::has(b, a.state),
        (k::STATE, Arg::Text(t)) => t.split(',').all(|kv| {
            kv.split_once('=')
                .is_some_and(|(p, v)| c.state_prop(a.state, p.trim()) == Some(v.trim()))
        }),
        (k::TOOL, Arg::Tag(t)) => !tool().is_empty() && c.stack_has(&tool(), Some(*t)),
        (k::TOOL_LEVEL, Arg::Num(n)) => !tool().is_empty() && c.stack_value::<u32>(&tool(), c.ids.tool_level).unwrap_or(0) as f32 >= *n,
        (k::KILLED_BY_PLAYER, _) => a.loot.is_some_and(|l| l.by_player),
        (k::ON_FIRE, _) => a.loot.is_some_and(|l| l.on_fire),
        (k::HARVESTED, _) => a
            .loot
            .is_some_and(|l| l.mined == 0 || c.harvests(l.mined, &l.tool)),
        (k::IN_LIQUID, _) => a.body.is_some_and(|b| b.in_liquid),
        (k::FLYING, _) => a.body.is_some_and(|b| b.flying),
        (k::SPRINTING, _) => a.body.is_some_and(|b| b.sprinting),
        _ => return None,
    })
}

/// Three ways: `None` when not known (unloaded, or changing by itself while `could`).
fn eval(e: &Expr, rec: &mut dyn FnMut(u32, &Arg) -> Option<bool>) -> Option<bool> {
    match e {
        Expr::All(v) => v
            .iter()
            .try_fold(Some(true), |all, e| match eval(e, rec) {
                Some(false) => Err(()),
                Some(true) => Ok(all),
                None => Ok(None),
            })
            .unwrap_or(Some(false)),
        Expr::Any(v) => v
            .iter()
            .try_fold(Some(false), |any, e| match eval(e, rec) {
                Some(true) => Err(()),
                Some(false) => Ok(any),
                None => Ok(None),
            })
            .unwrap_or(Some(true)),
        Expr::Not(e) => eval(e, rec).map(|b| !b),
        Expr::Is(id, arg) => rec(*id, arg),
    }
}

fn in_range(arg: &Arg, v: f32) -> bool {
    matches!(arg, Arg::Range(a, b) | Arg::Attr(_, a, b) | Arg::LightAt(a, b, _) if (*a..=*b).contains(&v))
}

/// The light `light(..)` reads, of the cell or a side of it (`Arg::LightAt`), by `light` at a
/// cell and `daylight`.
fn light_for(arg: &Arg, p: glam::IVec3, light: &dyn Fn(glam::IVec3) -> u32, daylight: f32) -> f32 {
    let at = |q: glam::IVec3| abi::light::effective(light(q), daylight) as f32;
    match arg {
        Arg::LightAt(_, _, 7) => crate::world::DIRS
            .iter()
            .map(|&d| at(p + d))
            .fold(0.0, f32::max),
        Arg::LightAt(_, _, f) if *f < 6 => at(p + crate::world::DIRS[*f as usize]),
        _ => at(p),
    }
}

/// Whether a body of `kind` with `inventory` has something in one of the equipment `slots`.
fn wears(kind: &crate::registry::EntityKind, inventory: &[abi::ItemStack], slots: u32) -> bool {
    let worn = &kind.worn;
    (0..32u32).filter(|i| slots & 1 << i != 0).any(|i| {
        // The other hand is the slot after the rest.
        let slot = if i < worn.len() as u32 { Some(worn.start + i) } else { kind.off.filter(|_| i == worn.len() as u32) };
        slot.and_then(|s| inventory.get(s as usize))
            .is_some_and(|s| !s.is_empty())
    })
}

fn attr(arg: &Arg) -> u32 {
    if let Arg::Attr(id, ..) = arg { *id } else { u32::MAX }
}

/// Every text of `texts`, compiled; one that fails matches nothing, said in `errors`.
pub fn compile_all(texts: &Texts, look: &Look, errors: &mut Vec<String>) -> Vec<Expr> {
    texts
        .texts
        .list
        .iter()
        .map(|t| {
            compile(t, look).unwrap_or_else(|e| {
                errors.push(e);
                Expr::Any(Vec::new())
            })
        })
        .collect()
}

/// Condition `id` (`Content::conds`) on the server.
pub fn on_server(d: &mut Dim, c: &'static Content, id: u32, a: &Ask) -> bool {
    c.conds
        .get(id as usize)
        .is_some_and(|e| holds_server(d, c, e, a))
}

/// Whether condition `id` may hold there some time (`Ask::could`).
pub fn may_server(d: &mut Dim, c: &'static Content, id: u32, a: &Ask) -> bool {
    c.conds
        .get(id as usize)
        .is_some_and(|e| server(d, c, e, &Ask { could: true, ..*a }) != Some(false))
}

pub fn holds_server(d: &mut Dim, c: &'static Content, e: &Expr, a: &Ask) -> bool {
    server(d, c, e, a) == Some(true)
}

fn server(d: &mut Dim, c: &'static Content, e: &Expr, a: &Ask) -> Option<bool> {
    let p = d.place.to_local(a.pos).floor().as_ivec3();
    eval(e, &mut |id, arg| {
        if let Some(v) = known(c, id, arg, a) {
            return Some(v);
        }
        let def = c.game.conditions.get(id as usize)?;
        let changes = matches!(def.answer, Answer::Mod(..))
            || matches!(id, k::DAY | k::NIGHT | k::UNDERGROUND | k::SURFACE | k::LIGHT | k::CHANCE | k::SKY | k::SHELTERED);
        if a.could && changes {
            return None;
        }
        let sky = || !crate::world::light::enclosed(&c.dims[d.id as usize], d.light(p));
        // The cell on a side named, the cell itself for 6.
        let toward = |f: u8| {
            let f = if a.own_frame && f < 6 { c.states[a.state as usize].turn.face(f as usize) } else { f as usize };
            p + crate::world::DIRS
                .get(f)
                .copied()
                .unwrap_or(glam::IVec3::ZERO)
        };
        // What is on a side, `None` if it isn't loaded.
        let side = |d: &Dim, f: u8, test: &dyn Fn(u32) -> bool| -> Option<bool> {
            let at = toward(f);
            d.get(at)?;
            Some(d.parts(at).into_iter().any(test))
        };
        Some(match def.answer {
            Answer::Engine => match (id, arg) {
                (k::DAY, _) => d.daylight(c) > 0.5,
                (k::NIGHT, _) => d.daylight(c) <= 0.5,
                (k::UNDERGROUND, _) => !sky(),
                (k::SURFACE, _) => sky(),
                (k::LIGHT, _) => {
                    let daylight = a.daylight.unwrap_or_else(|| d.daylight(c));
                    in_range(arg, light_for(arg, p, &|q| d.light(q), daylight))
                }
                (k::CHANCE, Arg::Num(n)) => d.random_unit() < *n,
                (k::SKY, _) => in_range(arg, abi::light::sky(d.light(p)) as f32),
                (k::SHELTERED, _) => c.game.weathers[d.weather as usize].shelters(),
                (k::HEIGHT, _) => in_range(arg, a.pos.y as f32),
                (k::ATTRIBUTE, _) => in_range(arg, d.attribute(c, a.entity, attr(arg), false) as f32),
                (k::WEARS, Arg::Slots(slots)) => {
                    let Some(e) = d.entities.get(a.entity) else { return Some(false) };
                    let inv = d
                        .entities
                        .inventories
                        .get(&a.entity)
                        .map_or(&[][..], |i| &i[..]);
                    wears(&c.game.entities[e.kind as usize], inv, *slots)
                }
                (k::SIDE, Arg::Side(f, m)) => {
                    return side(d, *f, &|s| m.as_ref().map_or(c.solid(s), |m| crate::refs::has(m, s)));
                }
                (k::OPEN, Arg::Side(f, _)) => {
                    return d
                        .get(toward(*f))
                        .map(|s| s == crate::content::AIR || c.states[s as usize].flags & abi::block_flags::REPLACEABLE != 0);
                }
                (k::BESIDE, Arg::Side(f, Some(m))) => {
                    let at = toward(*f);
                    let mut known = true;
                    for n in [glam::IVec3::X, -glam::IVec3::X, glam::IVec3::Z, -glam::IVec3::Z] {
                        match d.get(at + n) {
                            None => known = false,
                            Some(_) if d.parts(at + n).into_iter().any(|s| crate::refs::has(m, s)) => return Some(true),
                            Some(_) => {}
                        }
                    }
                    return known.then_some(false);
                }
                (k::COVERED, _) => {
                    let above = d.get(p + glam::IVec3::Y)?;
                    c.states[above as usize].flags & abi::block_flags::OPAQUE != 0
                }
                _ => false,
            },
            Answer::Mode(m) => d.entities.get(a.entity).is_some_and(|e| e.mode == m),
            Answer::Mod(owner, cb) => {
                let text = if let Arg::Text(t) = arg { t.as_str() } else { "" };
                d.call(owner, cb, |w| abi::ConditionArgs {
                    world: w,
                    client: std::ptr::null_mut(),
                    pos: a.pos.to_array().into(),
                    entity: a.entity,
                    arg: abi::Str::new(text),
                    result: 0,
                })
                .result
                    != 0
            }
        })
    })
}

/// Condition `id` on a client, about its own player.
pub fn on_client(client: Option<&crate::client::Client>, id: u32, a: &Ask) -> bool {
    let c = crate::content::content();
    c.conds
        .get(id as usize)
        .is_some_and(|e| client_holds(client, e, a))
}

pub fn client_holds(client: Option<&crate::client::Client>, e: &Expr, a: &Ask) -> bool {
    let c = crate::content::content();
    eval(e, &mut |id, arg| {
        if let Some(v) = known(c, id, arg, a) {
            return Some(v);
        }
        let def = c.game.conditions.get(id as usize)?;
        let enclosed = || {
            client.is_some_and(|cl| crate::world::light::enclosed(&c.dims[cl.world.dim as usize], cl.world.light(a.pos.floor().as_ivec3())))
        };
        Some(match def.answer {
            Answer::Engine => match id {
                k::DAY => client.is_some_and(|cl| cl.daylight() > 0.5),
                k::NIGHT => client.is_some_and(|cl| cl.daylight() <= 0.5),
                k::UNDERGROUND => enclosed(),
                k::SURFACE => client.is_some() && !enclosed(),
                k::LIGHT => client.is_some_and(|cl| {
                    let daylight = a.daylight.unwrap_or_else(|| cl.daylight());
                    in_range(arg, light_for(arg, a.pos.floor().as_ivec3(), &|q| cl.world.light(q), daylight))
                }),
                k::ATTRIBUTE => client.is_some_and(|cl| in_range(arg, cl.attribute(attr(arg)))),
                k::WEARS => client.is_some_and(|cl| {
                    let Arg::Slots(slots) = arg else { return false };
                    wears(&c.game.entities[cl.me.kind as usize], &cl.inventory, *slots)
                }),
                k::SKY => client.is_some_and(|cl| in_range(arg, abi::light::sky(cl.world.light(a.pos.floor().as_ivec3())) as f32)),
                k::SHELTERED => client.is_some_and(|cl| c.game.weathers[cl.weather_id as usize].shelters()),
                k::HEIGHT => in_range(arg, a.pos.y as f32),
                // Rolled where the world is.
                _ => false,
            },
            Answer::Mode(m) => client.is_some_and(|cl| cl.mode == m),
            Answer::Mod(owner, cb) => {
                let text = if let Arg::Text(t) = arg { t.as_str() } else { "" };
                let mut v = crate::client::modui::ClientView::new(client);
                let mut x = abi::ConditionArgs {
                    world: std::ptr::null_mut(),
                    client: v.raw(),
                    pos: a.pos.to_array().into(),
                    entity: a.entity,
                    arg: abi::Str::new(text),
                    result: 0,
                };
                crate::host::invoke(owner, cb, &mut x);
                x.result != 0
            }
        })
    }) == Some(true)
}
