//! Command syntax shared by the server (parsing, dispatch) and the client (completion). A command
//! describes its arguments with one usage string: literal words, `<name:type>` required and
//! `[name:type]` optional parameters, alternatives separated by `|`, for example
//! `<pos:pos> | <target:player>` or `<item:item> [count:int]`.
//!
//! Types: int (`int(a,b)`: a whole number or one of those words, completing them, like
//! `int(infinite)`), float, word (`word(a,b)`: any word, completing a and b; `word(#set)` completes
//! from a word set mods add to, like `#core:rules`), text (rest of the line), item, block,
//! entity, dim, attribute, player, time
//! (`day`, `noon`, `night`, `midnight` or ticks), pos (three coordinates; `~` relative to the player,
//! `^left ^up ^forward` relative to where they look) and targets (a player name or a selector:
//! `@p` nearest player, `@a` all players, `@e` entities, `@s` yourself, `@r` a random player, with
//! filters like `@e[type=zombie,distance=..10,limit=3,sort=nearest]`).

use crate::ui::text::{Arg, Msg};
use glam::DVec3;
use weft_abi::{Callback, ModId};

use crate::content::Content;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Literal(String),
    /// A whole number, or one of the listed words (taken as text).
    Int(Vec<String>),
    Float,
    /// Any word; the listed ones are suggested.
    Word(Vec<String>),
    Text,
    Item,
    Block,
    Entity,
    Dim,
    Attribute,
    Player,
    Pos,
    Targets,
    Time,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub kind: Kind,
    pub optional: bool,
}

pub type Alt = Vec<Param>;

pub fn parse_usage(usage: &str) -> Result<Vec<Alt>, String> {
    let mut alts = Vec::new();
    for alt in usage.split('|') {
        let mut params = Vec::new();
        for tok in alt.split_whitespace() {
            let (open, close, optional) = match tok.chars().next() {
                Some('<') => ('<', '>', false),
                Some('[') => ('[', ']', true),
                _ => {
                    params.push(Param { name: tok.to_string(), kind: Kind::Literal(tok.to_string()), optional: false });
                    continue;
                }
            };
            let inner = tok
                .strip_prefix(open)
                .and_then(|t| t.strip_suffix(close))
                .ok_or_else(|| format!("bad parameter {tok}"))?;
            let (name, ty) = inner
                .split_once(':')
                .ok_or_else(|| format!("parameter {tok} needs a type"))?;
            let kind = match ty {
                "int" => Kind::Int(Vec::new()),
                "float" => Kind::Float,
                "word" => Kind::Word(Vec::new()),
                w if w.starts_with("int(") && w.ends_with(')') => Kind::Int(words(&w[4..w.len() - 1])),
                w if w.starts_with("word(") && w.ends_with(')') => Kind::Word(words(&w[5..w.len() - 1])),
                "text" => Kind::Text,
                "item" => Kind::Item,
                "block" => Kind::Block,
                "entity" => Kind::Entity,
                "dim" => Kind::Dim,
                "attribute" => Kind::Attribute,
                "player" => Kind::Player,
                "pos" => Kind::Pos,
                "targets" => Kind::Targets,
                "time" => Kind::Time,
                _ => return Err(format!("unknown parameter type {ty}")),
            };
            params.push(Param { name: name.to_string(), kind, optional });
        }
        alts.push(params);
    }
    Ok(alts)
}

fn words(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Parameter names (and literal words) in order of first appearance across alternatives: the
/// slots of `Args` and of the values handed to mods.
pub fn slots(alts: &[Alt]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for p in alts.iter().flatten() {
        if !names.contains(&p.name) {
            names.push(p.name.clone());
        }
    }
    names
}

#[derive(Clone, Debug)]
pub enum Value {
    Int(i64),
    Float(f64),
    Text(String),
    /// Registry id and full name (items, block states, entity kinds, dimensions).
    Id(u32, String),
    Pos(DVec3),
    Targets(Selector),
}

/// Who a targets parameter means; the server turns it into entities.
#[derive(Clone, Debug, PartialEq)]
pub struct Selector {
    /// `p`, `a`, `e`, `s`, `r`, or `n` for a plain player name (in `name`).
    pub base: char,
    pub name: Option<String>,
    /// Entity kind, and whether it is negated (`type=!zombie`).
    pub kind: Option<(u32, bool)>,
    pub distance: Option<(f64, f64)>,
    pub limit: Option<usize>,
    pub sort: Option<String>,
}

/// Where and which way the command runs from, for `~` and `^` coordinates and selectors.
#[derive(Clone, Copy, Debug, Default)]
pub struct Origin {
    pub pos: DVec3,
    pub yaw: f32,
    pub pitch: f32,
}

fn parse_selector(c: &Content, tok: &str, players: &[String]) -> Result<Selector, Msg> {
    let mut s = Selector { base: 'n', name: None, kind: None, distance: None, limit: None, sort: None };
    let Some(rest) = tok.strip_prefix('@') else {
        if !players.iter().any(|n| n == tok) {
            return Err(err("no_player", tok));
        }
        s.name = Some(tok.to_string());
        return Ok(s);
    };
    let mut chars = rest.chars();
    s.base = chars
        .next()
        .filter(|b| "paesr".contains(*b))
        .ok_or_else(|| err("unknown_selector", tok))?;
    let filters = chars.as_str();
    if filters.is_empty() {
        return Ok(s);
    }
    let inner = filters
        .strip_prefix('[')
        .and_then(|f| f.strip_suffix(']'))
        .ok_or_else(|| err("bad_selector", tok))?;
    for f in inner.split(',').filter(|f| !f.is_empty()) {
        let (k, v) = f.split_once('=').ok_or_else(|| err("bad_filter", f))?;
        match k.trim() {
            "type" => {
                let (neg, name) = v.strip_prefix('!').map_or((false, v), |n| (true, n));
                let full = resolve(c.game.entity_names.keys(), name).ok_or_else(|| err("unknown_entity", name))?;
                s.kind = Some((c.game.entity_names[&full], neg));
            }
            "distance" => {
                let num = |t: &str, d: f64| if t.is_empty() { Ok(d) } else { t.parse::<f64>().map_err(|_| err("bad_distance", v)) };
                s.distance = Some(match v.split_once("..") {
                    Some((a, b)) => (num(a, 0.0)?, num(b, f64::INFINITY)?),
                    None => (num(v, 0.0)?, num(v, 0.0)?),
                });
            }
            "limit" => s.limit = Some(v.parse().map_err(|_| err("bad_limit", v))?),
            "sort" if ["nearest", "furthest", "random", "arbitrary"].contains(&v) => s.sort = Some(v.to_string()),
            "name" => s.name = Some(v.to_string()),
            _ => return Err(err("unknown_filter", f)),
        }
    }
    Ok(s)
}

/// Named times of day in ticks, like vanilla's `/time set`.
pub const TIMES: [(&str, i64); 4] = [("day", 1000), ("noon", 6000), ("night", 13000), ("midnight", 18000)];

pub struct Args {
    pub names: Vec<String>,
    pub values: Vec<Option<Value>>,
}

impl Args {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.names
            .iter()
            .position(|n| n == name)
            .and_then(|i| self.values[i].as_ref())
    }
    pub fn int(&self, name: &str) -> Option<i64> {
        match self.get(name)? {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }
    pub fn float(&self, name: &str) -> Option<f64> {
        match self.get(name)? {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f64),
            _ => None,
        }
    }
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Value::Text(t) | Value::Id(_, t) => Some(t),
            _ => None,
        }
    }
    pub fn id(&self, name: &str) -> Option<u32> {
        match self.get(name)? {
            Value::Id(i, _) => Some(*i),
            _ => None,
        }
    }
    pub fn pos(&self, name: &str) -> Option<DVec3> {
        match self.get(name)? {
            Value::Pos(p) => Some(*p),
            _ => None,
        }
    }
}

/// A full registry name from what the player typed: exact `ns:name`, or a bare name matching an
/// entry's path, the first loaded mod's when several do.
pub fn resolve<'a>(names: impl Iterator<Item = &'a String> + Clone, typed: &str) -> Option<String> {
    if typed.contains(':') {
        return names.into_iter().find(|n| n.as_str() == typed).cloned();
    }
    let matches: Vec<&String> = names
        .filter(|n| n.split_once(':').is_some_and(|(_, p)| p == typed))
        .collect();
    match matches.as_slice() {
        [one] => Some((*one).clone()),
        [] => None,
        many => {
            let order = |n: &&&String| {
                crate::content::content()
                    .mods
                    .iter()
                    .position(|m| n.split_once(':').is_some_and(|(ns, _)| ns == m.id))
            };
            many.iter()
                .min_by_key(|n| order(n).unwrap_or(usize::MAX))
                .map(|n| (**n).clone())
        }
    }
}

fn coordinate(tok: &str, base: f64) -> Option<f64> {
    match tok.strip_prefix('~') {
        Some("") => Some(base),
        Some(rest) => rest.parse::<f64>().ok().map(|v| base + v),
        None => tok.parse().ok(),
    }
    .filter(|v: &f64| v.is_finite())
}

/// Three coordinate tokens: absolute and `~` mixed per axis, or all `^` (left, up, forward from
/// the view).
fn position(t: &[&str], o: Origin) -> Option<DVec3> {
    if t.iter().all(|s| s.starts_with('^')) {
        let num = |s: &str| if s.len() == 1 { Some(0.0) } else { s[1..].parse::<f64>().ok() };
        let forward = crate::physics::look_dir(o.yaw, o.pitch);
        let (sy, cy) = (o.yaw as f64).sin_cos();
        let right = DVec3::new(cy, 0.0, -sy);
        let up = right.cross(forward);
        return Some(o.pos - right * num(t[0])? + up * num(t[1])? + forward * num(t[2])?);
    }
    if t.iter().any(|s| s.starts_with('^')) {
        return None;
    }
    Some(DVec3::new(coordinate(t[0], o.pos.x)?, coordinate(t[1], o.pos.y)?, coordinate(t[2], o.pos.z)?))
}

fn parse_alt(c: &Content, alt: &Alt, tokens: &[&str], here: Origin, players: &[String], slots: &[String]) -> Result<Args, Msg> {
    let mut values: Vec<Option<Value>> = vec![None; slots.len()];
    let mut i = 0;
    for p in alt {
        if i >= tokens.len() {
            if p.optional {
                continue;
            }
            return Err(err("missing", &p.name));
        }
        let slot = slots.iter().position(|n| n == &p.name);
        let tok = tokens[i];
        let value = match &p.kind {
            // A matched literal shows up as a value under its own name, so alternatives such
            // as `add <x> | remove <x>` can be told apart.
            Kind::Literal(l) => {
                if tok != l {
                    return Err(err("expected", l));
                }
                Value::Text(l.clone())
            }
            Kind::Int(named) if named.iter().any(|n| n == tok) => Value::Text(tok.to_string()),
            Kind::Int(_) => Value::Int(tok.parse().map_err(|_| err("whole_number", &p.name))?),
            Kind::Float => Value::Float(tok.parse().map_err(|_| err("number", &p.name))?),
            Kind::Word(_) => Value::Text(tok.to_string()),
            Kind::Text => {
                let v = Value::Text(tokens[i..].join(" "));
                i = tokens.len();
                if let Some(s) = slot {
                    values[s] = Some(v);
                }
                continue;
            }
            Kind::Item => {
                let n = resolve(c.item_names.keys(), tok).ok_or_else(|| err("unknown_item", tok))?;
                Value::Id(c.item_names[&n], n)
            }
            Kind::Block => {
                // `base:furnace[lit=true,facing=east]`: the name completes, the properties are checked.
                let (name, props) = tok.split_once('[').map_or((tok, ""), |(n, p)| (n, p));
                let n = resolve(c.block_names.keys(), name).ok_or_else(|| err("unknown_block", name))?;
                let full = if props.is_empty() { n.clone() } else { format!("{n}[{props}") };
                Value::Id(c.parse_state(&full).map_err(|_| err("bad_state", &full))?, full)
            }
            Kind::Entity => {
                let n = resolve(c.game.entity_names.keys(), tok).ok_or_else(|| err("unknown_entity", tok))?;
                Value::Id(c.game.entity_names[&n], n)
            }
            Kind::Dim => {
                let n = resolve(c.dim_names.keys(), tok).ok_or_else(|| err("unknown_dimension", tok))?;
                Value::Id(c.dim_names[&n], n)
            }
            Kind::Attribute => {
                let n = resolve(c.game.attr_names.keys(), tok).ok_or_else(|| err("unknown_attribute", tok))?;
                Value::Id(c.game.attr_names[&n], n)
            }
            Kind::Player => {
                if !players.iter().any(|n| n == tok) {
                    return Err(err("no_player", tok));
                }
                Value::Text(tok.to_string())
            }
            Kind::Targets => match parse_selector(c, tok, players) {
                Ok(s) => Value::Targets(s),
                // An optional parameter that does not fit leaves the word to the next one.
                Err(_) if p.optional => continue,
                Err(e) => return Err(e),
            },
            Kind::Time => match TIMES.iter().find(|t| t.0 == tok) {
                Some(t) => Value::Int(t.1),
                None => Value::Int(tok.parse().map_err(|_| err("time", &p.name))?),
            },
            Kind::Pos => {
                let parsed = tokens.get(i..i + 3).and_then(|t| position(t, here));
                match parsed {
                    Some(v) => {
                        i += 2;
                        Value::Pos(v)
                    }
                    None if p.optional => continue,
                    None if i + 3 > tokens.len() => return Err(err("needs_xyz", &p.name)),
                    None => return Err(err("bad_position", &tokens[i..i + 3].join(" "))),
                }
            }
        };
        if let Some(s) = slot {
            values[s] = Some(value);
        }
        i += 1;
    }
    if i < tokens.len() {
        return Err(err("unexpected", tokens[i]));
    }
    Ok(Args { names: slots.to_vec(), values })
}

#[derive(Clone)]
pub struct CommandDef {
    pub name: String,
    pub usage: String,
    pub alts: Vec<Alt>,
    /// `None`: an engine command.
    pub handler: Option<(ModId, Callback)>,
    /// Runs on the client that typed it (logging, gizmos, mods' client commands), never sent.
    pub client: bool,
    /// Per alternative: 0 = the command's own, i = `branches[i - 1]` (added by another mod).
    pub owners: Vec<usize>,
    pub branches: Vec<(ModId, Callback)>,
    /// The `permission` level it asks for.
    pub level: u32,
}

impl CommandDef {
    pub fn new(name: &str, usage: &str, alts: Vec<Alt>, handler: Option<(ModId, Callback)>, client: bool) -> CommandDef {
        let owners = vec![0; alts.len()];
        // Commands on a player's own client are its own business.
        let level = if client { weft_abi::permission::EVERYONE } else { weft_abi::permission::GAME_MASTER };
        CommandDef { name: name.to_string(), usage: usage.to_string(), alts, handler, client, owners, branches: Vec::new(), level }
    }

    /// Parses `tokens` and says who handles them: the command's own handler (`None` for the
    /// engine's) or the mod whose branch matched. Values are laid out by that owner's own
    /// alternatives, as its usage string declares them.
    pub fn parse(&self, c: &Content, tokens: &[&str], here: Origin, players: &[String]) -> Result<(Args, Option<(ModId, Callback)>), Msg> {
        let mut first_err = None;
        for (i, alt) in self.alts.iter().enumerate() {
            let owner = self.owners[i];
            let own: Vec<Alt> = self
                .alts
                .iter()
                .zip(&self.owners)
                .filter(|(_, o)| **o == owner)
                .map(|(a, _)| a.clone())
                .collect();
            match parse_alt(c, alt, tokens, here, players, &slots(&own)) {
                Ok(a) => return Ok((a, if owner == 0 { self.handler } else { Some(self.branches[owner - 1]) })),
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        Err(first_err.unwrap_or_else(|| Msg::new("command.error.bad_arguments", Vec::new())))
    }

    /// Another mod's alternatives with its own handler.
    pub fn add_branch(&mut self, alts: Vec<Alt>, handler: (ModId, Callback)) {
        self.branches.push(handler);
        let owner = self.branches.len();
        self.owners.extend(std::iter::repeat_n(owner, alts.len()));
        self.alts.extend(alts);
        self.usage = usage_text(&self.alts);
    }
}

fn type_name(k: &Kind) -> String {
    match k {
        Kind::Literal(l) => l.clone(),
        Kind::Int(list) if list.is_empty() => "int".into(),
        Kind::Int(list) => format!("int({})", list.join(",")),
        Kind::Float => "float".into(),
        Kind::Word(list) if list.is_empty() => "word".into(),
        Kind::Word(list) => format!("word({})", list.join(",")),
        Kind::Text => "text".into(),
        Kind::Item => "item".into(),
        Kind::Block => "block".into(),
        Kind::Entity => "entity".into(),
        Kind::Dim => "dim".into(),
        Kind::Attribute => "attribute".into(),
        Kind::Player => "player".into(),
        Kind::Pos => "pos".into(),
        Kind::Targets => "targets".into(),
        Kind::Time => "time".into(),
    }
}

/// Usage string of alternatives, the inverse of `parse_usage`.
pub fn usage_text(alts: &[Alt]) -> String {
    alts.iter()
        .map(|alt| {
            alt.iter()
                .map(|p| match &p.kind {
                    Kind::Literal(l) => l.clone(),
                    k if p.optional => format!("[{}:{}]", p.name, type_name(k)),
                    k => format!("<{}:{}>", p.name, type_name(k)),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Command values as mods receive them; `selected` holds resolved targets per slot. The values
/// point into `args` and `selected`.
pub fn abi_values(args: &Args, selected: &[Option<String>]) -> Vec<weft_abi::CommandValue> {
    use weft_abi::{Str, command_value as cv};
    args.values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let mut out = weft_abi::CommandValue {
                kind: cv::NONE,
                int: 0,
                float: 0.0,
                text: Str::new(""),
                pos: [0.0; 3].into(),
                name: Str::new(&args.names[i]),
            };
            match v {
                None => {}
                Some(Value::Int(n)) => (out.kind, out.int, out.float) = (cv::INT, *n, *n as f64),
                Some(Value::Float(f)) => (out.kind, out.float) = (cv::FLOAT, *f),
                Some(Value::Text(t)) => (out.kind, out.text) = (cv::TEXT, Str::new(t)),
                Some(Value::Id(n, name)) => (out.kind, out.int, out.text) = (cv::ID, *n as i64, Str::new(name)),
                Some(Value::Pos(p)) => (out.kind, out.pos) = (cv::POS, p.to_array().into()),
                Some(Value::Targets(_)) => {
                    let ids = selected.get(i).and_then(|s| s.as_deref()).unwrap_or("");
                    (out.kind, out.int, out.text) = (cv::TARGETS, ids.split_whitespace().count() as i64, Str::new(ids));
                }
            }
            out
        })
        .collect()
}

/// `/log`: shows or changes the log filters of this process (see `crate::log`).
pub fn log_command(spec: &str) -> Vec<Msg> {
    if spec.is_empty() {
        let channels: Vec<&str> = crate::log::CHANNELS.iter().map(|c| c.0).collect();
        return vec![
            Msg::new("command.log.filters", vec![Arg::Text(crate::log::describe())]),
            Msg::new("command.log.channels", vec![Arg::Text(channels.join(", "))]),
            Msg::new("command.log.example", Vec::new()),
        ];
    }
    vec![match crate::log::set(spec) {
        Ok(now) => Msg::new("command.log.filters", vec![Arg::Text(now)]),
        Err(e) => Msg::new("command.log.bad", vec![Arg::Text(e)]),
    }]
}

/// A parse error: `command.error.<key>` about `what` (a word typed, or a parameter's name).
fn err(key: &str, what: &str) -> Msg {
    Msg::new(&format!("command.error.{key}"), vec![Arg::Text(what.to_string())])
}

/// What the chat input can complete to: suggestions for the word being typed, the byte offset
/// where that word starts, and the usage of what is left to type.
pub struct Completion {
    pub suggestions: Vec<String>,
    pub from: usize,
    pub hint: String,
}

pub fn complete(c: &Content, line: &str, players: &[String], permission: u32) -> Completion {
    let from = line.rfind(' ').map_or(0, |i| i + 1);
    let partial = &line[from..];
    let done: Vec<&str> = line[..from].split_whitespace().collect();
    let mut out = Completion { suggestions: Vec::new(), from, hint: String::new() };
    if done.is_empty() {
        let mut names: Vec<String> = c
            .commands
            .iter()
            .filter(|d| d.level <= permission)
            .map(|d| d.name.clone())
            .filter(|n| n.starts_with(partial))
            .collect();
        names.sort();
        out.suggestions = names;
        return out;
    }
    let Some(cmd) = c.commands.iter().find(|d| d.name == done[0]) else { return out };
    let typed = &done[1..];
    let mut seen = Vec::new();
    for alt in &cmd.alts {
        // Find the parameter the current word belongs to.
        let mut at = 0;
        let mut found = None;
        for (pi, p) in alt.iter().enumerate() {
            let width = if p.kind == Kind::Pos {
                3
            } else if p.kind == Kind::Text {
                usize::MAX / 2
            } else {
                1
            };
            // An optional selector or position the typed word cannot be skips to what follows.
            let fits = |w: &str| match p.kind {
                Kind::Targets => w.starts_with('@') || players.iter().any(|n| n == w),
                Kind::Pos => w.starts_with('~') || w.starts_with('^') || w.parse::<f64>().is_ok(),
                _ => true,
            };
            if p.optional && typed.len() > at && !fits(typed[at]) {
                continue;
            }
            if typed.len() < at + width {
                found = Some((pi, typed.len() - at));
                break;
            }
            if let Kind::Literal(l) = &p.kind {
                if typed[at] != l {
                    found = None;
                    break;
                }
            }
            at += width;
        }
        let Some((pi, within)) = found else { continue };
        let p = &alt[pi];
        let names_from = |keys: Vec<&String>| -> Vec<String> {
            let mut v: Vec<String> = keys
                .into_iter()
                .filter(|n| {
                    n.starts_with(partial)
                        || n.split_once(':')
                            .is_some_and(|(_, path)| path.starts_with(partial))
                })
                .cloned()
                .collect();
            v.sort();
            v
        };
        let list = match &p.kind {
            Kind::Literal(l) if l.starts_with(partial) => vec![l.clone()],
            Kind::Word(options) => {
                let set = |o: &String| {
                    o.strip_prefix('#')
                        .map(|s| c.word_sets.get(s).cloned().unwrap_or_default())
                        .unwrap_or_else(|| vec![o.clone()])
                };
                names_from(
                    options
                        .iter()
                        .flat_map(set)
                        .collect::<Vec<_>>()
                        .iter()
                        .collect(),
                )
            }
            Kind::Item => names_from(c.item_names.keys().collect()),
            Kind::Block => names_from(c.block_names.keys().collect()),
            Kind::Entity => names_from(
                c.game
                    .entity_names
                    .iter()
                    .filter(|(_, k)| !c.game.is_block_entity(**k))
                    .map(|(n, _)| n)
                    .collect(),
            ),
            Kind::Dim => names_from(c.dim_names.keys().collect()),
            Kind::Attribute => names_from(c.game.attr_names.keys().collect()),
            Kind::Player => players
                .iter()
                .filter(|n| n.starts_with(partial))
                .cloned()
                .collect(),
            Kind::Targets => ["@p", "@a", "@e", "@s", "@r"]
                .iter()
                .map(|s| s.to_string())
                .chain(players.iter().cloned())
                .filter(|n| n.starts_with(partial))
                .collect(),
            Kind::Int(named) => named
                .iter()
                .filter(|n| n.starts_with(partial))
                .cloned()
                .collect(),
            Kind::Time => TIMES
                .iter()
                .map(|t| t.0.to_string())
                .filter(|n| n.starts_with(partial))
                .collect(),
            Kind::Pos if partial.is_empty() || partial == "~" => vec![if within == 0 { "~ ~ ~".to_string() } else { "~".to_string() }],
            Kind::Pos if partial == "^" => vec![if within == 0 { "^ ^ ^".to_string() } else { "^".to_string() }],
            _ => Vec::new(),
        };
        for s in list {
            if !seen.contains(&s) {
                seen.push(s);
            }
        }
        if out.hint.is_empty() {
            out.hint = alt[pi..]
                .iter()
                .map(|p| match &p.kind {
                    Kind::Literal(l) => l.clone(),
                    k if p.optional => format!("[{}]", hint_name(&p.name, k)),
                    k => format!("<{}>", hint_name(&p.name, k)),
                })
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
    out.suggestions = seen;
    out
}

fn hint_name(name: &str, k: &Kind) -> String {
    if *k == Kind::Pos { "x y z".into() } else { name.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mods_command_asks_for_game_master_whatever_its_name() {
        // The engine's table says what its own /rule and /help ask for; a mod's isn't in it.
        for name in ["rule", "help", "anything"] {
            let d = CommandDef::new(name, "", parse_usage("").unwrap(), None, false);
            assert_eq!(d.level, weft_abi::permission::GAME_MASTER);
        }
        let d = CommandDef::new("x", "", parse_usage("").unwrap(), None, true);
        assert_eq!(d.level, weft_abi::permission::EVERYONE);
    }
}
