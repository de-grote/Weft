//! The engine's own chat commands: name, usage (parsed and completed by `commands`) and handler
//! side by side, so a listed command always has one.

use glam::{DVec3, IVec3};
use weft_abi::{ItemStack, Who, damage_cause as damage};

use super::Server;
use crate::commands::Args;
use crate::content::Content;
use crate::net::Shown;
use crate::ui::text::{Arg, Msg, Name};
use crate::world::GameMode;

/// What a handler gets besides the server.
pub struct Ctx<'a> {
    pub c: &'static Content,
    /// Index of the player who typed it.
    pub pi: usize,
    pub a: &'a Args,
    /// Where the player stands.
    pub here: DVec3,
    /// The player's dimension and entity.
    pub me: (u32, u64),
}

impl Ctx<'_> {
    /// Entities a `targets` argument selects, the player without it.
    fn targets(&self, s: &Server, key: &str) -> Vec<(u32, u64)> {
        match self.a.get(key) {
            Some(crate::commands::Value::Targets(sel)) => s.select(self.pi, sel, self.here),
            _ => vec![self.me],
        }
    }
}

fn block_at(a: &Args) -> Option<IVec3> {
    a.pos("pos").map(|p| p.floor().as_ivec3())
}

type Handler = fn(&mut Server, &Ctx) -> Vec<Msg>;

/// One reply line: `command.<key>` with its arguments.
fn say(key: &str, args: Vec<Arg>) -> Vec<Msg> {
    vec![Msg::new(&format!("command.{key}"), args)]
}

fn text(s: &str) -> Arg {
    Arg::Text(s.to_string())
}

fn int(v: impl TryInto<i64>) -> Arg {
    Arg::Int(v.try_into().unwrap_or(i64::MAX))
}

use weft_abi::permission as p;

/// Name, usage, what it does, the permission level it asks for.
pub const COMMANDS: &[(&str, &str, Handler, u32)] = &[
    ("help", "[command:word]", help, p::EVERYONE),
    ("tp", "<pos:pos> | <to:targets> | <who:targets> <pos:pos> | <who:targets> <to:targets>", tp, p::GAME_MASTER),
    ("dim", "<dim:dim> [who:targets]", dim, p::GAME_MASTER),
    ("give", "<who:targets> <item:item> [count:int] | <item:item> [count:int]", give, p::GAME_MASTER),
    ("time", "set <time:time> | add <ticks:int> | query", time, p::GAME_MASTER),
    ("summon", "<entity:entity> [pos:pos] [count:int]", summon, p::GAME_MASTER),
    ("clear", "[who:targets] | [item:item]", clear, p::GAME_MASTER),
    ("setblock", "<pos:pos> <block:block>", set_block, p::GAME_MASTER),
    ("insert", "<pos:pos> <item:item> [count:int]", insert, p::GAME_MASTER),
    ("block", "<pos:pos>", block, p::GAME_MASTER),
    ("gamemode", "<mode:word(#core:game_modes)> [who:targets]", game_mode, p::GAME_MASTER),
    ("heal", "[who:targets]", heal, p::GAME_MASTER),
    ("tick", "query | rate <tps:int> | freeze | unfreeze | step [ticks:int] | sprint <ticks:int>", tick, p::ADMIN),
    ("regions", "", regions, p::GAME_MASTER),
    ("seed", "", seed, p::GAME_MASTER),
    ("weather", "<weather:word(#core:weathers)> [ticks:int]", weather, p::GAME_MASTER),
    ("op", "<who:targets> [level:int]", op, p::ADMIN),
    ("deop", "<who:targets>", deop, p::ADMIN),
    ("rule", "[name:word(#core:rules)] [value:float]", rule, p::ADMIN),
    (
        "title",
        "<who:targets> title <text:text> | <who:targets> subtitle <text:text> | <who:targets> hint <text:text> | <who:targets> show <x:float> <y:float> <size:float> <text:text> | <who:targets> times <fade_in:float> <stay:float> <fade_out:float> | <who:targets> clear | <who:targets> reset",
        title,
        p::GAME_MASTER,
    ),
    ("say", "<text:text>", say_all, p::GAME_MASTER),
    ("msg", "<who:player> <text:text>", whisper, p::EVERYONE),
    ("tellraw", "<who:targets> <text:text>", tellraw, p::GAME_MASTER),
];

fn title(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    use weft_abi::show_place as place;
    let a = x.a;
    let players: Vec<usize> = x
        .targets(s, "who")
        .iter()
        .filter_map(|t| s.player_index(t.1))
        .collect();
    let text = a.text("text").unwrap_or("");
    let shown = |p: u32| Shown::new(p, Msg::template(text));
    let to_all = |s: &mut Server, shown: Shown| {
        for &pi in &players {
            let times = s.players[pi].title_times;
            let timed = if shown.place == place::FREE || shown.place == place::HINT { None } else { times };
            let one = Shown { times: timed.unwrap_or(shown.times), ..shown.clone() };
            s.show(Who::Player(s.players[pi].entity), one);
        }
    };
    if a.get("times").is_some() {
        let t = [a.float("fade_in").unwrap_or(0.0), a.float("stay").unwrap_or(0.0), a.float("fade_out").unwrap_or(0.0)];
        for &pi in &players {
            s.players[pi].title_times = Some(t.map(|v| v.max(0.0) as f32));
        }
    } else if a.get("reset").is_some() {
        for &pi in &players {
            s.players[pi].title_times = None;
        }
    } else if a.get("clear").is_some() {
        let empty = Msg::new("", vec![]);
        for p in [place::TITLE, place::SUBTITLE] {
            to_all(s, Shown::new(p, empty.clone()));
        }
    } else if a.get("show").is_some() {
        let at = [a.float("x").unwrap_or(0.5) as f32, a.float("y").unwrap_or(0.5) as f32];
        let scale = a.float("size").unwrap_or(1.0) as f32;
        to_all(s, Shown { at: Some(at), scale, ..shown(place::FREE) });
    } else {
        let named = [("subtitle", place::SUBTITLE), ("hint", place::HINT), ("title", place::TITLE)];
        for (_, p) in named.into_iter().filter(|(name, _)| a.get(name).is_some()) {
            to_all(s, shown(p));
        }
    }
    say("title.done", vec![int(players.len())])
}

fn say_all(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let name = s.players[x.pi].name.clone();
    let line = Msg::new("chat.say", vec![Arg::Text(name), text(x.a.text("text").unwrap_or(""))]);
    s.show(Who::All, Shown::chat(line));
    Vec::new()
}

fn whisper(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let said = text(x.a.text("text").unwrap_or(""));
    let to = x.a.text("who").unwrap_or("");
    let Some(target) = s.players.iter().find(|p| p.name == to).map(|p| p.entity) else { return Vec::new() };
    let name = s.players[x.pi].name.clone();
    let heard = Msg::new("chat.whisper", vec![Arg::Text(name), said.clone()]);
    s.show(Who::Player(target), Shown { from: Shown::PLAYER, ..Shown::chat(heard) });
    vec![Msg::new("chat.whisper_to", vec![text(to), said])]
}

fn tellraw(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let line = Msg::template(x.a.text("text").unwrap_or(""));
    for (_, id) in x.targets(s, "who") {
        s.show(Who::Player(id), Shown::chat(line.clone()));
    }
    Vec::new()
}

fn help(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let (c, a) = (x.c, x.a);
    match a.text("command") {
        Some(n) => c
            .commands
            .iter()
            .find(|d| d.name == n)
            .map_or_else(|| say("unknown", vec![text(n)]), |d| say("usage_line", vec![text(&d.name), text(&d.usage)])),
        None => c
            .commands
            .iter()
            .filter(|d| d.level <= s.permission_of(x.pi))
            .flat_map(|d| say("usage_line", vec![text(&d.name), text(&short_usage(&d.usage))]))
            .collect(),
    }
}

/// A command's usage for the list of them: a line. `/help <command>` has it all.
fn short_usage(usage: &str) -> String {
    const ROOM: usize = 80;
    let mut out = String::new();
    for alt in usage.split(" | ") {
        if !out.is_empty() && out.len() + alt.len() + 3 > ROOM {
            out.push_str(" | ...");
            break;
        }
        if !out.is_empty() {
            out.push_str(" | ");
        }
        out.push_str(alt);
    }
    out
}

fn seed(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    say("seed.done", vec![text(&crate::saves::shown_seed(s.dims[x.me.0 as usize].seed()))])
}

fn tp(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let (c, a, pi, here, me) = (x.c, x.a, x.pi, x.here, x.me);
    let who = x.targets(s, "who");
    let dest = match (a.pos("pos"), a.get("to")) {
        (Some(p), _) => Some((me.0, p)),
        (None, Some(crate::commands::Value::Targets(sel))) => s
            .select(pi, sel, here)
            .first()
            .and_then(|&(d, id)| s.position_of(d, id).map(|p| (d, p))),
        _ => None,
    };
    let Some((to_dim, pos)) = dest else { return say("tp.nowhere", Vec::new()) };
    for &(d, id) in &who {
        s.teleport(c, d, id, to_dim, pos);
    }
    say("tp.done", vec![int(who.len()), Arg::Float(pos.x, 1), Arg::Float(pos.y, 1), Arg::Float(pos.z, 1)])
}

fn dim(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let (c, a) = (x.c, x.a);
    let d = a.id("dim").unwrap_or(0);
    let spawn = s.dims[d as usize].spawn_point(c);
    let who = x.targets(s, "who");
    for &(from, id) in &who {
        s.teleport(c, from, id, d, spawn);
    }
    say("dim.done", vec![text(a.text("dim").unwrap_or_default())])
}

fn give(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let (c, a) = (x.c, x.a);
    let count = a.int("count").unwrap_or(1).clamp(1, 1 << 20) as u32;
    let item = a.id("item").unwrap_or(0);
    let mut given = 0;
    for (d, id) in x.targets(s, "who") {
        if let Some(r) = s.dims[d as usize].entity_region(id) {
            given += count
                - r.entities
                    .insert_item(c, id, ItemStack::new(item, count))
                    .count;
        }
    }
    say("give.done", vec![int(given), Arg::Name(Name::Item(item))])
}

fn time(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let (c, a, me) = (x.c, x.a, x.me);
    let d = &mut s.dims[me.0 as usize];
    let day_length = c.dims[d.id as usize].day_length;
    let len = day_length.max(1) as u64;
    let day = d.day_time();
    if a.get("query").is_some() {
        return say("time.query", vec![int(day % len), int(day / len), int(d.time)]);
    }
    if day_length == 0 {
        return say("time.fixed", Vec::new());
    }
    let now = match (a.int("time"), a.int("ticks")) {
        (Some(t), _) => day - day % len + t.max(0) as u64,
        (None, Some(add)) => day.saturating_add_signed(add),
        _ => day,
    };
    d.set_day_time(now);
    say("time.set", vec![int(now % len)])
}

fn summon(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let (c, a, here, me) = (x.c, x.a, x.here, x.me);
    let count = a.int("count").unwrap_or(1).clamp(1, 100_000);
    let pos = a.pos("pos").unwrap_or(here);
    let (kind, kname) = (a.id("entity").unwrap_or(0), a.text("entity").unwrap_or_default().to_string());
    if c.game.is_block_entity(kind) {
        return say("summon.block_entity", vec![text(&kname)]);
    }
    let r = s.dims[me.0 as usize].ensure_region(c, pos);
    for _ in 0..count {
        r.spawn(c, kind, pos);
    }
    say("summon.done", vec![int(count), text(&kname)])
}

/// `/clear <targets>`: kills them. `/clear [item]`: empties the player's own inventory, or only
/// the stacks of `item`.
fn clear(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let c = x.c;
    if x.a.get("who").is_some() {
        let who = x.targets(s, "who");
        for &(d, id) in &who {
            if let Some(r) = s.dims[d as usize].entity_region(id) {
                r.damage(c, id, f32::MAX, 0, damage::VOID);
            }
        }
        return say("clear.killed", vec![int(who.len())]);
    }
    let (d, id) = x.me;
    let filter = x.a.id("item");
    let mut total: u64 = 0;
    if let Some(r) = s.dims[d as usize].entity_region(id) {
        let slots = r.entities.inventories.get(&id).map_or(0, Vec::len) as u32;
        for slot in 0..slots {
            let stack = r.entities.inventory_slot(id, slot);
            if stack.is_empty() || filter.is_some_and(|item| item != stack.item) {
                continue;
            }
            total += u64::from(stack.count);
            r.entities.set_slot(id, slot, ItemStack::EMPTY);
        }
    }
    say("clear.done", vec![int(total)])
}

fn set_block(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let (c, a, dim) = (x.c, x.a, x.me.0);
    let (Some(p), Some(state)) = (block_at(a), a.id("block")) else { return Vec::new() };
    if !c.dims[dim as usize].editable {
        return say("locked", vec![text(&c.dims[dim as usize].name)]);
    }
    match s.dims[dim as usize].region_at(p) {
        Some(r) => {
            r.set(c, p, state);
            say("setblock.done", vec![Arg::Name(Name::Block(state))])
        }
        None => say("not_loaded", Vec::new()),
    }
}

fn insert(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let (c, a, dim) = (x.c, x.a, x.me.0);
    let (Some(p), Some(item)) = (block_at(a), a.id("item")) else { return Vec::new() };
    let count = a.int("count").unwrap_or(1).clamp(1, 1 << 20) as u32;
    match s.dims[dim as usize].region_at(p) {
        Some(r) => match r.anchored(p) {
            Some(owner) => {
                let left = r
                    .entities
                    .insert_item(c, owner, ItemStack::new(item, count));
                say("insert.done", vec![int(count - left.count), Arg::Name(Name::Item(item))])
            }
            None => say("insert.no_entity", Vec::new()),
        },
        None => say("not_loaded", Vec::new()),
    }
}

fn block(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let (c, a, dim) = (x.c, x.a, x.me.0);
    let Some(p) = block_at(a) else { return Vec::new() };
    let d = &s.dims[dim as usize];
    let dim_name = text(&c.dims[d.id as usize].name);
    match d.get(p) {
        Some(state) => say("block.done", vec![text(&c.state_name(state)), dim_name]),
        None => say("block.unloaded", vec![dim_name]),
    }
}

fn game_mode(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let a = x.a;
    let named = a.text("mode").unwrap_or("");
    let Some(mode) = GameMode::parse(named) else {
        return say("gamemode.unknown", vec![text(named)]);
    };
    let who: Vec<usize> = x
        .targets(s, "who")
        .iter()
        .filter_map(|t| s.player_index(t.1))
        .collect();
    for &p in &who {
        s.set_mode(p, mode);
    }
    say("gamemode.done", vec![Arg::Key(format!("gamemode.{}", mode.name())), int(who.len())])
}

fn weather(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let d = &mut s.dims[x.me.0 as usize];
    if !x.c.dims[d.id as usize].has_weather() {
        return say("weather.none", Vec::new());
    }
    let named = x.a.text("weather").unwrap_or("");
    let names = &x.c.game.weather_names;
    let Some(&w) = names
        .get(named)
        .or_else(|| names.get(&format!("core:{named}")))
    else {
        return say("weather.unknown", vec![text(named)]);
    };
    let ticks = x.a.int("ticks").map_or(12000, |t| t.max(1) as u64);
    d.set_weather(w as u16, ticks);
    say("weather.done", Vec::new())
}

/// `/op`: nobody gives out more than it has itself.
fn set_level(s: &mut Server, x: &Ctx, level: u32) -> Vec<Msg> {
    let own = s.permission_of(x.pi);
    if level > own {
        return say("op.too_high", vec![int(own)]);
    }
    let mut lines = Vec::new();
    for (_, id) in x.targets(s, "who") {
        if let Some(pi) = s.players.iter().position(|p| p.joined && p.entity == id) {
            s.set_permission(pi, level);
            lines.extend(say("op.done", vec![text(&s.players[pi].name), int(level)]));
        }
    }
    lines
}

fn op(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let level =
        x.a.int("level")
            .map_or(weft_abi::permission::OWNER, |l| l.clamp(0, weft_abi::permission::OWNER as i64) as u32);
    set_level(s, x, level)
}

fn deop(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    set_level(s, x, weft_abi::permission::EVERYONE)
}

fn heal(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let c = x.c;
    for (d, id) in x.targets(s, "who") {
        if let Some(r) = s.dims[d as usize].entity_region(id) {
            let max = r.max_health(c, id);
            if let Some(e) = r.entities.get_mut(id) {
                if e.alive() {
                    e.health = max;
                }
            }
        }
    }
    say("heal.done", Vec::new())
}

/// Vanilla's `/tick`: the world's speed, frozen with steps, or a sprint through many ticks as fast
/// as they run (for tests that need hours of world time).
fn tick(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let a = x.a;
    if let Some(tps) = a.int("tps") {
        s.tick_rate = tps.clamp(1, 1000) as u32;
        return say("tick.rate", vec![int(s.tick_rate)]);
    }
    if a.get("freeze").is_some() {
        s.frozen = true;
        return say("tick.frozen", Vec::new());
    }
    if a.get("unfreeze").is_some() {
        (s.frozen, s.steps) = (false, 0);
        return say("tick.runs", Vec::new());
    }
    if a.get("step").is_some() {
        if !s.frozen {
            return say("tick.not_frozen", Vec::new());
        }
        let n = a.int("ticks").unwrap_or(1).clamp(1, 1 << 20) as u32;
        s.steps += n;
        return say("tick.stepping", vec![int(n)]);
    }
    if a.get("sprint").is_some() {
        let n = a.int("ticks").unwrap_or(1).clamp(1, 1 << 24) as u32;
        s.sprint = Some((n, n, std::time::Instant::now()));
        return say("tick.sprinting", vec![int(n)]);
    }
    say(if s.frozen { "tick.query_frozen" } else { "tick.rate" }, vec![int(s.tick_rate)])
}

fn regions(s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let c = x.c;
    let parts: Vec<String> = s
        .dims
        .iter()
        .filter(|d| !d.regions.is_empty())
        .map(|d| {
            let sizes: Vec<String> = d
                .regions
                .iter()
                .map(|r| format!("{}/{}", r.space.cubes.len(), r.entities.list.len()))
                .collect();
            format!("{}: {} ({})", c.dims[d.id as usize].name, d.regions.len(), sizes.join(" "))
        })
        .collect();
    say("regions.done", vec![text(&parts.join(", "))])
}

/// The world's settings (`config_scope::WORLD`), like vanilla's game rules.
fn rule(_s: &mut Server, x: &Ctx) -> Vec<Msg> {
    let (c, a) = (x.c, x.a);
    let rules: Vec<(u32, &crate::config::ConfigDef)> = c
        .game
        .configs
        .iter()
        .enumerate()
        .filter(|(_, d)| d.scope == weft_abi::config_scope::WORLD)
        .map(|(i, d)| (i as u32, d))
        .collect();
    let value = |id: u32, d: &crate::config::ConfigDef| say("rule.value", vec![text(&d.name), text(&d.show(crate::config::get(id)))]);
    let Some(name) = a.text("name") else {
        return rules.iter().flat_map(|&(id, d)| value(id, d)).collect();
    };
    let Some(&(id, d)) = rules
        .iter()
        .find(|r| r.1.name == name || r.1.name.rsplit(':').next() == Some(name))
    else {
        return say("rule.unknown", vec![text(name)]);
    };
    if let Some(v) = a.float("value") {
        // A world made with its rules locked keeps them.
        if crate::config::get(c.game.rules_locked) != 0.0 {
            return say("rule.locked", Vec::new());
        }
        crate::config::set(id, v);
    }
    value(id, d)
}
