//! Server half: the operations tests run, as the command `/tk` (see `OPS`). The engine parses
//! their arguments like any command's, and the answer goes back to the test that ran it.

use std::sync::atomic::{AtomicU32, Ordering};

use weft_sdk::abi::entry_kind as ek;
use weft_sdk::terrain::{BiomeRule, Expr};
use weft_sdk::*;

/// How many of each event arrived since `events reset`, in `EVENT_NAMES` order.
static COUNTS: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
const EVENT_NAMES: [&str; 8] =
    ["block_broken", "block_placed", "item_picked_up", "entity_died", "entity_hurt", "player_joined", "item_used", "smelted"];

use base_api::SMELTED;
static GOLD: Block = Block::named("base:gold_block");
static GOLD_HELMET: Item = Item::named("base:gold_helmet");
static RED_LAMP: Item = Item::named("base:red_lamp");
static BLUE_LAMP: Item = Item::named("base:blue_lamp");
/// A value and a question of steps in several phases (`value` op).
static VALUE: Chain = Chain::named("value");
static LATE: Step = Step::named("late");
static EARLY: Step = Step::named("early");
static MAY: Question = Question::named("may");
static HALF_STEP: Phase = Phase::named("half_step");
/// Shared with everyone who sees the entity, for the sync test.
pub static MARK: Component<u32> = Component::named("testkit:mark");
/// Server-side stack data that is not saved either.
pub static SECRET: Component<u32> = Component::named("testkit:secret");
static DISPLAY_PHASE: Component<u32> = Component::named("testmod:display_phase");
static DISPLAY_PARTS: Storage = Storage::named("testmod:display_parts");
/// A stash every player carries, for menus on an inventory that is no block's or entity's own.
static STASH: Storage = Storage::named("testkit:stash");
static PIG: EntityKind = EntityKind::named("base:pig");
/// Green light and nothing drawn, for the dynamic light test.
static LIT: EntityEffect = EntityEffect::named("lit");
/// Shows what is in its one slot.
static PEDESTAL: Block = Block::named("pedestal");
pub static COUNT_MARKS: Config<bool> = Config::named("count_marks");
pub static PROTECT_GOLD: Config<bool> = Config::named("protect_gold");
pub static HALF_DAMAGE: Config<bool> = Config::named("half_damage");
/// Mobs may not attack while it is on.
pub static PEACE: Config<bool> = Config::named("peace");
/// Nobody places blocks while it is on: what the client can't know before the server says.
pub static NO_PLACING: Config<bool> = Config::named("no_placing");
static DRILL: Block = Block::named("drill");
/// Registered, then changed by `r.edit` (the `edits` test).
/// Testmod's, edited here: an edit wins whichever mod loads first.
static EDITED: Item = Item::named("testmod:edited");
static EDITED_BIOME: Biome = Biome::named("edited");
static EDITED_LOOT: LootTable = LootTable::named("edited");
static EDITED_KIND: EntityKind = EntityKind::named("edited");
/// Testmod's records' tag and weight, by name: the vocabulary is shared.
static STYLE: Tag = Tag::named("testmod:style");
static STYLE_WEIGHT: Component<f32> = Component::named("testmod:weight");
static WARMTH: Component<f32> = Component::named("testkit:warmth");
static STORM: Weather = Weather::named("storm");
static GRIT: Attribute = Attribute::named("grit");
static TIN_ORE: Block = Block::named("testmod:tin_ore");
static OIL: Block = Block::named("testmod:oil");
static MARS: Dimension = Dimension::named("testmod:mars");
/// Lit by `.states`, then edited: its light on every state, its opacity on the lit one.
static LAMP: Block = Block::named("lamp");
/// Made by base's stairs template, edited like any block.
static OAK_STAIRS: Block = Block::named("base:oak_planks_stairs");
/// A column of Mars, far from what other tests look at, that an edit of its terrain gives base's
/// plains.
const PROBE: [i32; 2] = [-20000, 20000];
/// Only the equipment of what it opens on (the `armor` test opens it on a zombie).
static GEAR: Menu = Menu::named("gear");
/// How many cells of the world a drill on a moving body has come to.
static PASSES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Players can't be hurt (`actions::HURT`) or use items (`actions::USE_ITEM`) while on.
pub static NO_HURT: Config<bool> = Config::named("no_hurt");
pub static NO_USE: Config<bool> = Config::named("no_use");
/// `chat_hooks`: "forbidden" stays unsaid, lines are written by this mod.
pub static CHAT_HOOKS: Config<bool> = Config::named("chat_hooks");
/// What the last `actions::CHAT` said.
static LAST_SAID: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
/// Features placed in generated cubes: (feature, start).
static PLACED: std::sync::Mutex<Vec<(u32, BlockPos)>> = std::sync::Mutex::new(Vec::new());
/// Fired by `after` timers; counted.
static RANG: Event<u64> = Event::named("rang");
static RINGS: AtomicU32 = AtomicU32::new(0);
/// Mobs walked by tests (`drive`): (entity, direction, ticks left).
static DRIVEN: std::sync::Mutex<Vec<(u64, [f32; 2], u32)>> = std::sync::Mutex::new(Vec::new());
/// A box watched tick by tick (`trace`): dimension, corners, ticks left, what each cell last was,
/// and what changed: `tick x,y,z block signal`, as the tick before left it (signals go later).
struct Trace {
    dim: u32,
    a: [i32; 3],
    b: [i32; 3],
    left: u32,
    last: std::collections::HashMap<[i32; 3], (u32, u8)>,
    lines: Vec<String>,
}
static TRACE: std::sync::Mutex<Option<Trace>> = std::sync::Mutex::new(None);

fn trace_tick(w: &World) {
    let mut guard = TRACE.lock().unwrap();
    let Some(t) = guard
        .as_mut()
        .filter(|t| t.dim == w.dimension() && t.left > 0)
    else {
        return;
    };
    t.left -= 1;
    let now = w.game_time();
    for x in t.a[0]..=t.b[0] {
        for y in t.a[1]..=t.b[1] {
            for z in t.a[2]..=t.b[2] {
                let p = [x, y, z];
                let here = (w.block(p), w.signal(p));
                let was = t.last.insert(p, here);
                if was != Some(here) && (was.is_some() || here.0 != 0) {
                    t.lines
                        .push(format!("{now} {x},{y},{z} {} {}", block_name(here.0), here.1));
                }
            }
        }
    }
}

fn count(i: usize, n: usize) {
    COUNTS[i].fetch_add(n as u32, Ordering::Relaxed);
}

pub fn register(r: &mut Registry) {
    crate::generation::register(r);
    // A random `.ctm` with tiles of two colors, on what isn't a cube.
    static VARIED_PLANT: Block = Block::named("varied_plant");
    static VARIED_BOX: Block = Block::named("varied_box");
    r.block(&VARIED_PLANT).texture("varied").plant().no_item();
    r.block(&VARIED_BOX)
        .texture("varied")
        .model_box([2.0, 0.0, 2.0], [14.0, 16.0, 14.0])
        .no_item();
    static VARIED_CUBE: Block = Block::named("varied_cube");
    r.block(&VARIED_CUBE).texture("varied_cube").no_item();
    r.block(&DRILL)
        .on_carried(|_| {
            PASSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        })
        .texture("varied")
        .no_item();
    r.entity_component(&MARK, 0, component_flags::SYNC_WATCHERS);
    // A suit to swim in lava with (`swimming`).
    r.edit(Item::generated("base:diamond_chestplate".into()))
        .set_text(&SWIMS_IN, "#lava");
    r.edit(&PIG)
        .attach(&MARK)
        .attach(&LIGHT)
        .light_at(0.0, 0.5, 1.0);
    r.edit(&PLAYER).attach(&LIGHT);
    r.entity_effect(&LIT).unseen().light_rgb(0, 15, 0);
    r.block(&PEDESTAL)
        .texture("varied")
        .entity(|e| e.inventory(1).shown_slots(&[0]))
        .no_item();
    r.storage(&STASH, 54);
    // What an edit sets wins, the rest stays; the testmod ones load after or before this mod
    // depending on order, which edits don't care about.
    r.edit(&EDITED).max_stack(8);
    r.biome(&EDITED_BIOME).set(&WARMTH, 1.0).tag("edited_a");
    r.edit(&EDITED_BIOME).set(&WARMTH, 2.0);
    r.weather(&STORM).no_falling().weight(0);
    r.edit(&STORM).lightning().untag(abi::comp::weather::DRY);
    r.attribute(&GRIT, 1.0, 0.0, 10.0);
    r.edit(&GRIT).set(&comps::attribute::DEFAULT, 4.0);
    r.entity(&EDITED_KIND)
        .size(0.6, 1.8)
        .eye_height(1.0)
        .set(&WARMTH, 1.0)
        .no_egg();
    r.edit(&EDITED_KIND)
        .size(0.9, 2.5)
        .set(&WARMTH, 3.0)
        .attach(&MARK);
    r.edit(TIN_ORE.ore()).per_cube(7);
    // Testmod's records: one of its changed, one more of the same kind.
    static STONE_STYLE: Record = Record::named("testmod:stone_style");
    static BRICK_STYLE: Record = Record::named("brick_style");
    r.edit(&STONE_STYLE).set(&STYLE_WEIGHT, 5.0);
    r.record(&BRICK_STYLE)
        .tag(STYLE.name())
        .set(&STYLE_WEIGHT, 1.0);
    r.block(&LAMP)
        .prop("lit", ["false", "true"])
        .states(|p, s| if p.is("lit", "true") { s.light(13) } else { s })
        .tag("testkit:lamps")
        .no_item();
    r.edit(&LAMP)
        .light(4)
        .state_where("lit=true", |s| s.light_opacity(3));
    r.edit(&OAK_STAIRS).light(7);
    r.set("#testkit:lamp*", &WARMTH, 1.5);
    r.tag("testkit:lit", &["lamp[lit=true]"]);
    r.shapeless(&base_api::SMELTING, "base:stick", 1, &["testmod:edited"], 200)
        .named("smelt_edited");
    r.remove(&Recipe::named("smelt_edited"));
    r.loot(&EDITED_LOOT)
        .pool(Pool::new().item("base:stick"))
        .pool(Pool::new().item("base:dirt"));
    r.edit(&EDITED_LOOT)
        .remove_item(&comps::loot::POOLS, "#1")
        .replace_drop("base:stick", "testmod:edited")
        .pool(Pool::new().items("base:cobblestone", 2, 2));
    r.edit(OIL.liquid())
        .flow_delay(12)
        .block(|b| b.tag("testkit:edited"));
    let off = |at: i32, of: Expr| (of - Expr::blocks(at as f64)).abs();
    r.edit(&MARS).tag("testkit:edited");
    r.edit(MARS.terrain())
        .field(
            "testkit:probe",
            off(PROBE[0], Expr::x())
                .max(off(PROBE[1], Expr::z()))
                .ramp_blocks(1.0, 1.0)
                .not(),
        )
        .rule_before(
            "#0",
            BiomeRule::new("base:plains")
                .named("probe")
                .field("testkit:probe", 0.5..),
        );
    r.menu(&GEAR).equipment().player_inventory().screen(|ui| {
        ui.panel("panel", S::column().pad(7.0).gap(4.0), |ui| {
            ui.title();
            ui.equipment();
            ui.player_inventory(true);
        });
    });
    r.edit(&GEAR).group("extra", Slots::row(2));
    r.edit(&PLAYER).attach_storage(&STASH);
    r.config(&COUNT_MARKS).bool(false).world();
    // While the rule is on, every marked pig counts up once a tick.
    r.system(&MARK, |b| {
        if !COUNT_MARKS.get() {
            return;
        }
        for m in b.values().iter_mut().filter(|m| **m > 0) {
            *m += 1;
        }
    });
    r.stack_component(&SECRET, abi::stack_flags::SERVER_ONLY | abi::stack_flags::NO_SAVE);
    // Block properties are components: another mod's block glows blue (testkit is never shipped).
    r.set("base:mossy_cobblestone", &LIGHT, [0, 0, 9]);
    // Switches for the tests of the extension mechanisms, off unless a test turns them on.
    r.config(&PROTECT_GOLD).bool(false).world();
    r.config(&PEACE).bool(false).world();
    r.before_event(&question::ATTACK, |q| {
        let w = q.world();
        let mob = w
            .entity(q.entity())
            .is_some_and(|e| !w.players_iter().any(|p| p == q.entity()) && e.health > 0.0);
        if mob && PEACE.get() { Verdict::Deny } else { Verdict::Abstain }
    });
    r.config(&NO_HURT).bool(false).world();
    r.config(&NO_USE).bool(false).world();
    r.config(&CHAT_HOOKS).bool(false).world();
    r.config(&NO_PLACING).bool(false).world();
    r.before_event(&actions::PLACE_BLOCK, |_| if NO_PLACING.get() { Verdict::Deny } else { Verdict::Abstain });
    r.before_event(&actions::CHAT, |q| if CHAT_HOOKS.get() && q.text() == "forbidden" { Verdict::Deny } else { Verdict::Abstain });
    r.chat_format(|f| {
        if CHAT_HOOKS.get() {
            f.set("{0} from a mod", &[Arg::Text(f.text())]);
        }
    });
    r.on_event(&actions::CHAT, |b| {
        for m in b.iter() {
            *LAST_SAID.lock().unwrap() = m.text().to_string();
        }
    });
    let player = |w: &World, id: u64| w.players_iter().any(|p| p == id);
    r.before_event(&actions::HURT, move |q| if player(&q.world(), q.entity()) && NO_HURT.get() { Verdict::Deny } else { Verdict::Abstain });
    r.before_event(&actions::USE_ITEM, |_| if NO_USE.get() { Verdict::Deny } else { Verdict::Abstain });
    r.on_event(&RANG, |b| {
        RINGS.fetch_add(b.len() as u32, Ordering::Relaxed);
    });
    r.on_event(&FEATURE_PLACED, |b| {
        PLACED
            .lock()
            .unwrap()
            .extend(b.iter().map(|v| (v.feature, v.pos)));
    });
    r.config(&HALF_DAMAGE).bool(false).world();
    r.before_event(&actions::BREAK_BLOCK, |q| if GOLD.is(q.state()) && PROTECT_GOLD.get() { Verdict::Deny } else { Verdict::Abstain });
    r.modify(&chain::DAMAGE, &phase::FINAL, |m| {
        if HALF_DAMAGE.get() {
            m.value *= 0.5;
        }
    });
    // Registered out of order: phases and `before` decide, not who came first.
    r.modify(&VALUE, &phase::MULTIPLY, |m| m.value *= 2.0);
    r.modify(&VALUE, &phase::ADD, |m| m.value += 3.0);
    // A phase of its own goes right after multiply, before rules.
    r.modify(&VALUE, &phase::RULES, |m| m.value *= 3.0);
    r.phase(&HALF_STEP, &phase::MULTIPLY);
    r.modify(&VALUE, &HALF_STEP, |m| m.value += 0.5);
    r.step(&LATE)
        .on(&VALUE)
        .phase(&phase::OVERRIDE)
        .run(|m| m.value -= 1.0);
    r.step(&EARLY)
        .on(&VALUE)
        .phase(&phase::OVERRIDE)
        .before("late")
        .run(|m| m.value *= 10.0);
    r.before_event(&MAY, |_| Verdict::Deny);
    r.before_event(&MAY, |_| Verdict::Allow)
        .phase(&phase::OVERRIDE);
    r.before_event(&MAY, |_| Verdict::Abstain)
        .phase(&phase::FINAL);
    r.on_event(&actions::BREAK_BLOCK, |b| count(0, b.len()));
    r.on_event(&actions::PLACE_BLOCK, |b| count(1, b.len()));
    r.on_event(&actions::PICKUP_ITEM, |b| count(2, b.len()));
    r.on_event(&actions::DIE, |b| count(3, b.len()));
    r.on_event(&actions::HURT, |b| count(4, b.len()));
    r.on_event(&PLAYER_JOINED, |b| count(5, b.len()));
    r.on_event(&actions::USE_ITEM, |b| count(6, b.len()));
    r.on_event(&SMELTED, |b| count(7, b.len()));
    // A mob walks where a test says for a while, after its own AI had its say.
    r.world_tick(|w| {
        let mut driven = DRIVEN.lock().unwrap();
        for (id, dir, left) in driven.iter_mut() {
            if w.entity(*id).is_some() && *left > 0 {
                w.walk(*id, weft_sdk::Walk::toward(*dir, 1.0));
                *left -= 1;
            }
        }
        driven.retain(|d| d.2 > 0);
    });
    r.world_tick(trace_tick);
    register_ops(r);
}

/// The usage of `/tk`: every operation, an owner (`@owner`) written as `me`, an entity id or a
/// block entity's position, an entity (`@entity`) as `me` or an id.
fn usage() -> String {
    let mut alts = Vec::new();
    for (op, args, _) in OPS {
        for args in args.split(" | ") {
            let forms: &[&str] = if args.contains("@owner") {
                &["me", "<id:int>", "<at:pos>"]
            } else if args.contains("@entity") {
                &["me", "<id:int>"]
            } else {
                &[""]
            };
            for f in forms {
                let a = args.replace("@owner", f).replace("@entity", f);
                alts.push(format!("{op} {a}").trim().to_string());
            }
        }
    }
    alts.join(" | ")
}

pub fn register_ops(r: &mut Registry) {
    r.command("tk", &usage(), |cmd| {
        let w = cmd.world();
        let answer = match run(&w, cmd) {
            Ok(s) => ("testkit.ok", s),
            Err(e) => ("testkit.err", e),
        };
        cmd.reply(answer.0, &[Arg::Text(&answer.1)]);
    });
}

fn pos(cmd: &Command, name: &str) -> Result<[i32; 3], String> {
    Ok((block_pos_of(cmd.pos(name).ok_or_else(|| format!("missing {name}"))?)).to_array())
}

fn int<T: TryFrom<i64>>(cmd: &Command, name: &str) -> Result<T, String> {
    cmd.int(name)
        .and_then(|v| T::try_from(v).ok())
        .ok_or_else(|| format!("missing {name}"))
}

fn word<'a>(cmd: &'a Command, name: &str) -> Result<&'a str, String> {
    cmd.text(name).ok_or_else(|| format!("missing {name}"))
}

fn id(cmd: &Command, name: &str) -> Result<u32, String> {
    cmd.id(name).ok_or_else(|| format!("missing {name}"))
}

/// `me`, an entity id or the block entity at a position.
fn owner(w: &World, cmd: &Command) -> Result<u64, String> {
    if let Some(id) = cmd.int("id") {
        return Ok(id as u64);
    }
    if cmd.pos("at").is_none() {
        return Ok(cmd.player());
    }
    let p = pos(cmd, "at")?;
    match w.block_entity(p) {
        0 => Err(format!("no block entity at {p:?} ({})", block_name(w.block(p)))),
        id => Ok(id),
    }
}

/// An item by name, base's when no namespace is given (lists inside one argument).
fn item(name: &str) -> Result<u32, String> {
    let n = if name.contains(':') { name.to_string() } else { format!("base:{name}") };
    Item::lookup(&n).ok_or_else(|| format!("no item {n}"))
}

fn stack_text(s: ItemStack) -> String {
    if s.is_empty() { "- 0 0".into() } else { format!("{} {} {}", item_name(s.item), s.count, s.wear()) }
}

fn run(w: &World, cmd: &Command) -> Result<String, String> {
    let player = cmd.player();
    let me = w.entity(player).map(|e| e.block_pos()).unwrap_or_default();
    // The operation is the literal that came first; a parameter may share its name (`slot`).
    let op = OPS
        .iter()
        .find(|o| cmd.text(o.0) == Some(o.0))
        .ok_or("no operation")?;
    (op.2)(w, cmd, player, me.to_array())
}

/// A repeater with a delay of four into a block with a torch on it, the torch feeding the
/// repeater through wire that goes clear of that block, which would hold it on: flips slowly
/// enough for a torch not to burn out.
fn clock(w: &World, p: [i32; 3]) -> Result<String, String> {
    let state = |name: &str, props: &[(&str, &str)]| -> Result<u32, String> {
        let mut s = Block::lookup(name).ok_or(format!("no {name}"))?;
        for (k, v) in props {
            s = state_with(s, k, v).ok_or(format!("{name} has no {k}={v}"))?;
        }
        Ok(s)
    };
    let wire = state_with(Block::lookup("base:redstone").ok_or("no redstone")?, "down", "true").ok_or("no redstone on a floor")?;
    let at = |x: i32, z: i32| [p[0] + x, p[1], p[2] + z];
    w.set_block(at(0, 0), state("base:stone", &[])?);
    w.set_block(at(1, 0), state("base:redstone_torch", &[("attach", "west")])?);
    for (x, z) in [(2, 0), (2, 1), (2, 2), (1, 2), (0, 2), (-1, 2), (-2, 2), (-2, 1), (-2, 0)] {
        w.set_block(at(x, z), wire);
    }
    w.set_block(at(-1, 0), state("base:repeater", &[("attach", "down"), ("front", "east"), ("delay", "4")])?);
    Ok(String::new())
}

/// `(name, usage, what it does)`, the running player and where it stands given.
type Run = fn(&World, &Command, u64, [i32; 3]) -> Result<String, String>;

/// The operations tests run on the server, as one command (`/tk set ~ ~ ~2 stone`): the engine
/// parses positions (`~dx`), block, item and entity names, and the answer goes back to the test
/// that ran it (`Client::run_command`), as `ok ..` or `err ..`.
const OPS: &[(&str, &str, Run)] = &[
    ("last_said", "", |_, _, _, _| Ok(LAST_SAID.lock().unwrap().clone())),
    // Every change in a box for some ticks; `traced` gives what it saw.
    ("traced", "", |_, _, _, _| {
        Ok(TRACE
            .lock()
            .unwrap()
            .take()
            .map_or(String::new(), |t| t.lines.join("\n")))
    }),
    ("trace", "<a:pos> <b:pos> <ticks:int>", |w, cmd, _, _| {
        let (a, b) = (pos(cmd, "a")?, pos(cmd, "b")?);
        let (lo, hi) = (std::array::from_fn(|i| a[i].min(b[i])), std::array::from_fn(|i| a[i].max(b[i])));
        *TRACE.lock().unwrap() =
            Some(Trace { dim: w.dimension(), a: lo, b: hi, left: int(cmd, "ticks")?, last: Default::default(), lines: Vec::new() });
        Ok(String::new())
    }),
    ("show", "<who:word> <place:word> <text:text>", |w, cmd, player, _| {
        let pos = w.entity(player).map_or([0.0; 3], |e| e.pos.to_array());
        let who = match word(cmd, "who")? {
            "all" => Who::All,
            "dim" => Who::Dimension(w.dimension()),
            "near" => Who::Near(w.dimension(), pos.into(), 8.0),
            "far" => Who::Near(w.dimension(), [pos[0] + 100.0, pos[1], pos[2]].into(), 8.0),
            _ => Who::Player(player),
        };
        let text = word(cmd, "text")?;
        match word(cmd, "place")? {
            "chat" => drop(w.chat(text, &[]).to(who)),
            "hint" => drop(w.hint(text, &[]).to(who)),
            "title" => drop(w.title(text, &[]).to(who)),
            "subtitle" => drop(w.subtitle(text, &[]).to(who)),
            "corner" => drop(w.show(text, &[]).to(who).at(1.0, 0.0).scale(2.0)),
            p => return Err(format!("no place {p}")),
        }
        Ok(String::new())
    }),
    ("lift", "<at:pos> <r:int>", |w, cmd, _, _| {
        // All in a cube of that reach around it.
        let (p, r) = (pos(cmd, "at")?, int::<i32>(cmd, "r")?);
        let mut cells = Vec::new();
        for x in -r..=r {
            for y in -r..=r {
                for z in -r..=r {
                    cells.push(BlockPos::new(p[0] + x, p[1] + y, p[2] + z));
                }
            }
        }
        Ok(w.body_from_blocks(p, &cells).to_string())
    }),
    ("bodyfall", "<id:int>", |w, cmd, _, _| {
        w.body_fall(int(cmd, "id")?);
        Ok(String::new())
    }),
    ("passes", "", |_, _, _, _| {
        Ok(PASSES
            .load(std::sync::atomic::Ordering::Relaxed)
            .to_string())
    }),
    ("structureset", "<id:int> <x:int> <y:int> <z:int> <block:block>", |w, cmd, _, _| {
        let (id, p) = (int::<u64>(cmd, "id")?, [int::<i32>(cmd, "x")?, int::<i32>(cmd, "y")?, int::<i32>(cmd, "z")?]);
        let block = self::id(cmd, "block")?;
        match w.on_body(id) {
            Some(f) if f.set_block(p, block) => Ok(String::new()),
            _ => Err(format!("no structure {id}")),
        }
    }),
    ("rsbuild", "<at:pos> <seed:int>", |w, cmd, _, _| crate::tests::redstone::build_turned(w, int::<u64>(cmd, "seed")?, pos(cmd, "at")?)),
    ("rsfill", "<at:pos> <half:int>", |w, cmd, _, _| {
        let (p, half) = (pos(cmd, "at")?, int::<i32>(cmd, "half")?);
        let s = state_with(Block::lookup("base:redstone").ok_or("no redstone")?, "down", "true").ok_or("no redstone on a floor")?;
        for x in -half..half {
            for z in -half..half {
                w.set_block([p[0] + x, p[1], p[2] + z], s);
            }
        }
        Ok(String::new())
    }),
    ("rsclear", "<at:pos> <half:int>", |w, cmd, _, _| {
        let (p, half) = (pos(cmd, "at")?, int::<i32>(cmd, "half")?);
        let air = 0;
        for x in -half - 3..half + 3 {
            for z in -half - 3..half + 3 {
                w.set_block([p[0] + x, p[1], p[2] + z], air);
            }
        }
        Ok(String::new())
    }),
    // A torch on a block with a repeater feeding it back: a clock that never burns out.
    ("rsclock", "<at:pos> <n:int>", |w, cmd, _, _| {
        let p = pos(cmd, "at")?;
        clock(w, p)
    }),
    ("rsclocks", "<at:pos> <n:int>", |w, cmd, _, _| {
        let (p, n) = (pos(cmd, "at")?, int::<i32>(cmd, "n")?);
        let side = (n as f32).sqrt().ceil() as i32;
        for i in 0..n {
            let q = [p[0] + (i % side) * 6 - side * 3, p[1], p[2] + (i / side) * 6 - side * 3];
            clock(w, q)?;
        }
        Ok(n.to_string())
    }),
    ("set", "<pos:pos> <block:block>", |w, cmd, _, _| {
        let p = pos(cmd, "pos")?;
        if !w.set_block(p, id(cmd, "block")?) {
            return Err(format!("{p:?} is not loaded"));
        }
        Ok(String::new())
    }),
    ("fill", "<from:pos> <to:pos> <block:block>", |w, cmd, _, _| {
        let (p, q) = (pos(cmd, "from")?, pos(cmd, "to")?);
        let s = id(cmd, "block")?;
        let (lo, hi) = ([p[0].min(q[0]), p[1].min(q[1]), p[2].min(q[2])], [p[0].max(q[0]), p[1].max(q[1]), p[2].max(q[2])]);
        let volume = (0..3).map(|i| (hi[i] - lo[i] + 1) as i64).product::<i64>();
        if volume > 1 << 18 {
            return Err(format!("fill of {volume} blocks is too big"));
        }
        let mut missed = 0;
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                for x in lo[0]..=hi[0] {
                    missed += !w.set_block([x, y, z], s) as u32;
                }
            }
        }
        if missed > 0 { Err(format!("{missed} blocks not loaded")) } else { Ok(volume.to_string()) }
    }),
    ("get", "<pos:pos>", |w, cmd, _, _| Ok(block_name(w.block(pos(cmd, "pos")?)).to_string())),
    ("count", "<from:pos> <to:pos> <block:block>", |w, cmd, _, _| {
        // How many of a block lie in a box.
        let (a, b, want) = (pos(cmd, "from")?, pos(cmd, "to")?, block_of(id(cmd, "block")?));
        let mut n = 0;
        for y in a[1].min(b[1])..=a[1].max(b[1]) {
            for z in a[2].min(b[2])..=a[2].max(b[2]) {
                for x in a[0].min(b[0])..=a[0].max(b[0]) {
                    n += (block_of(w.block([x, y, z])) == want) as u32;
                }
            }
        }
        Ok(n.to_string())
    }),
    ("prop", "<pos:pos> <name:word>", |w, cmd, _, _| {
        Ok(w.prop(pos(cmd, "pos")?, word(cmd, "name")?)
            .unwrap_or("none")
            .to_string())
    }),
    ("parts", "<pos:pos>", |w, cmd, _, _| {
        Ok(w.parts(pos(cmd, "pos")?)
            .into_iter()
            .map(block_name)
            .collect::<Vec<_>>()
            .join(" "))
    }),
    ("addpart", "<pos:pos> <block:block>", |w, cmd, _, _| Ok((w.part_add(pos(cmd, "pos")?, id(cmd, "block")?) as u32).to_string())),
    ("crater", "<r:int>", |w, cmd, _, me| {
        // Every loaded block within `r` of the player turned to air, one `w.set` each, the
        // way a mod would do it today: changed, already air, not loaded, milliseconds.
        let r: i32 = int(cmd, "r")?;
        let start = std::time::Instant::now();
        let (mut changed, mut air, mut missed) = (0u64, 0u64, 0u64);
        for y in -r..=r {
            for z in -r..=r {
                for x in -r..=r {
                    if x * x + y * y + z * z > r * r {
                        continue;
                    }
                    let p = [me[0] + x, me[1] + y, me[2] + z];
                    let s = w.block(p);
                    if s == 0 {
                        air += 1;
                    } else if w.set_block(p, 0) {
                        changed += 1;
                    } else {
                        missed += 1;
                    }
                }
            }
        }
        Ok(format!("{changed} {air} {missed} {:.1}", start.elapsed().as_secs_f64() * 1000.0))
    }),
    ("crater_fast", "<r:int>", |w, cmd, _, me| {
        // The same through the area edit: one call, done at the end of the tick.
        let r: i32 = int(cmd, "r")?;
        w.fill(Shape::ball([me[0] as f64 + 0.5, me[1] as f64 + 0.5, me[2] as f64 + 0.5], r as f64), 0)
            .keep("#unbreakable");
        Ok(String::new())
    }),
    ("group", "@owner <name:word>", |w, cmd, _, _| {
        let g = w.inventory(owner(w, cmd)?).group(word(cmd, "name")?);
        Ok(format!("{} {}", g.len(), (0..g.len()).filter(|&i| !g.get(i).is_empty()).count()))
    }),
    ("sblock", "<id:int> <pos:pos> [block:block]", |w, cmd, _, _| {
        // A block of a moving structure, or set it.
        let (id_, p) = (int::<u64>(cmd, "id")?, pos(cmd, "pos")?);
        let f = w.on_body(id_).ok_or("not a structure")?;
        match cmd.id("block") {
            Some(b) => Ok((f.set_block(p, b) as u32).to_string()),
            None => f
                .block_loaded(p)
                .map(|s| block_name(s).to_string())
                .ok_or_else(|| "not a structure".to_string()),
        }
    }),
    ("light", "<pos:pos>", |w, cmd, _, _| Ok(w.light(pos(cmd, "pos")?).to_string())),
    // What a grid `w` by `h` of these crafts into.
    ("craft", "<w:int> <h:int> <items:text>", |_, cmd, _, _| {
        let (width, height): (u32, u32) = (int(cmd, "w")?, int(cmd, "h")?);
        let grid: Vec<ItemStack> = cmd
            .text("items")
            .unwrap_or("")
            .split_whitespace()
            .map(|n| if n == "air" { ItemStack::EMPTY } else { ItemStack::new(Item::lookup(n).unwrap_or(0), 1) })
            .collect();
        let made = recipe_grid_output(&base_api::CRAFTING, &grid, width, height);
        Ok(format!("{} {}", name_of(abi::entry_kind::ITEM, made.item), made.count))
    }),
    ("seed", "<pos:pos>", |w, cmd, _, _| {
        let p = pos(cmd, "pos")?;
        Ok(format!("{} {} {}", w.seed(), w.seed_at(p), w.seed_of_cube(p)))
    }),
    ("skip", "<ticks:int>", |w, cmd, _, _| {
        w.skip_ticks(int(cmd, "ticks")?);
        Ok(String::new())
    }),
    ("slot", "@owner <index:int>", |w, cmd, _, _| Ok(stack_text(w.inventory(owner(w, cmd)?).get(int(cmd, "index")?)))),
    ("put", "@owner <index:int> <item:item> [count:int]", |w, cmd, _, _| {
        let o = owner(w, cmd)?;
        w.inventory(o)
            .set(int(cmd, "index")?, ItemStack::new(id(cmd, "item")?, int(cmd, "count").unwrap_or(1)));
        Ok(String::new())
    }),
    ("display_data", "@owner <index:int> <phase:int> <counter:int> <secret:int>", |w, cmd, _, _| {
        let entity = owner(w, cmd)?;
        let (inv, index) = (w.inventory(entity), int(cmd, "index")?);
        let secret = int(cmd, "secret")?;
        let phase = int(cmd, "phase")?;
        let inner = Item::named("base:stick")
            .stack(1)
            .with(&SECRET, secret)
            .with(&MARK, int(cmd, "counter")?);
        let mut stack = inv
            .get(index)
            .with(&DISPLAY_PHASE, phase)
            .with(&MARK, int(cmd, "counter")?)
            .with(&SECRET, secret)
            .with_stacks(&DISPLAY_PARTS, &[inner]);
        if phase == 2 {
            stack = stack
                .with(&MODEL, Text::new("testmod:katana"))
                .with(&WORN_MODEL, Text::new("testmod:fox_tail@body"))
                .with(&WORN_TEXTURE, Text::new("base:armor/diamond_1"));
        }
        inv.set(index, stack);
        Ok(entity.to_string())
    }),
    ("stack_light", "@owner <index:int> <r:int> <g:int> <b:int>", |w, cmd, _, _| {
        let entity = owner(w, cmd)?;
        let (inv, index) = (w.inventory(entity), int(cmd, "index")?);
        inv.set(
            index,
            inv.get(index)
                .with(&LIGHT, [int(cmd, "r")?, int(cmd, "g")?, int(cmd, "b")?]),
        );
        Ok(entity.to_string())
    }),
    ("drop_stack", "@owner <index:int> <pos:pos>", |w, cmd, _, _| {
        let stack = w.inventory(owner(w, cmd)?).get(int(cmd, "index")?);
        let p = pos(cmd, "pos")?;
        Ok(w.spawn_item([p[0] as f64 + 0.5, p[1] as f64, p[2] as f64 + 0.5], stack)
            .to_string())
    }),
    ("wear", "@owner <index:int> <amount:int>", |w, cmd, _, _| {
        let (inv, i) = (w.inventory(owner(w, cmd)?), int(cmd, "index")?);
        inv.set(i, inv.get(i).with_wear(int(cmd, "amount")?));
        Ok(String::new())
    }),
    ("insert", "<pos:pos> <item:item> [count:int]", |w, cmd, _, _| {
        let p = pos(cmd, "pos")?;
        let o = match w.block_entity(p) {
            0 => return Err(format!("no block entity at {p:?} ({})", block_name(w.block(p)))),
            id => id,
        };
        let count: u32 = int(cmd, "count").unwrap_or(1);
        let left = w
            .inventory(o)
            .insert(ItemStack::new(id(cmd, "item")?, count));
        Ok((count - left.count).to_string())
    }),
    ("inv", "@owner", |w, cmd, _, _| {
        let inv = w.inventory(owner(w, cmd)?);
        let parts: Vec<String> = (0..inv.len())
            .map(|i| (i, inv.get(i)))
            .filter(|(_, s)| !s.is_empty())
            .map(|(i, s)| format!("{i}:{}:{}", item_name(s.item), s.count))
            .collect();
        Ok(parts.join(" "))
    }),
    ("spawn", "<kind:entity> <pos:pos>", |w, cmd, _, _| {
        let p = pos(cmd, "pos")?;
        Ok(w.spawn(id(cmd, "kind")?, [p[0] as f64 + 0.5, p[1] as f64, p[2] as f64 + 0.5])
            .to_string())
    }),
    // `VALUE` from 1 and the answer to `MAY`.
    ("value", "", |w, _, _, _| Ok(format!("{} {}", w.chain_compute(&VALUE, 1.0, &query()), w.ask(&MAY, &query()) as u32))),
    // What the server knows the player holds of an action: held, ticks.
    ("key", "<action:word>", |w, cmd, player, _| {
        let k = w.key(player, Key::generated(word(cmd, "action")?.to_string()));
        Ok(format!("{} {}", k.held, k.held_ticks))
    }),
    // A projectile from `x y z` flying `vx vy vz` blocks a second, shot by the player.
    ("shoot", "<kind:entity> <x:float> <y:float> <z:float> <vx:float> <vy:float> <vz:float>", |w, cmd, player, _| {
        let f = |n: &str| cmd.float(n).ok_or(format!("missing {n}"));
        let how = Spawn::new()
            .owner(player)
            .velocity([f("vx")?, f("vy")?, f("vz")?]);
        Ok(w.spawn_with(id(cmd, "kind")?, [f("x")?, f("y")?, f("z")?], &how)
            .to_string())
    }),
    ("spawnlit", "<kind:entity> <pos:pos>", |w, cmd, _, _| {
        let p = pos(cmd, "pos")?;
        let e = w.spawn(id(cmd, "kind")?, [p[0] as f64 + 0.5, p[1] as f64, p[2] as f64 + 0.5]);
        let inv = w.inventory(e);
        inv.set(w.selected_slot(e), RED_LAMP.stack(1));
        if let Some(offhand) = inv.find_group(OFF_HAND.name()) {
            offhand.set(0, BLUE_LAMP.stack(1));
        }
        if let Some(worn) = w.inventory(e).find_group(EQUIPMENT_GROUP) {
            worn.set(0, GOLD_HELMET.stack(1));
        }
        Ok(e.to_string())
    }),
    ("entity", "@entity", |w, cmd, _, _| {
        let e = w.entity(owner(w, cmd)?).ok_or("no such entity")?;
        Ok(format!("{} {:.2} {:.2} {:.2} {}", name_of(ek::ENTITY, e.kind), e.pos.x, e.pos.y, e.pos.z, e.health))
    }),
    ("size", "@entity [width:float] [height:float]", |w, cmd, _, _| {
        let id = owner(w, cmd)?;
        let s = w.entity(id).ok_or("no such entity")?.size;
        if let (Some(width), Some(height)) = (cmd.float("width"), cmd.float("height")) {
            w.set_size(id, Some(abi::EntitySize { width: width as f32, height: height as f32, eye: s.eye * height as f32 / s.height }));
        }
        let s = w.entity(id).ok_or("no such entity")?.size;
        Ok(format!("{:.3} {:.3} {:.3}", s.width, s.height, s.eye))
    }),
    ("near", "<pos:pos> <r:float>", |w, cmd, player, _| {
        let p = pos(cmd, "pos")?;
        let r = cmd.float("r").ok_or("missing r")?;
        let ids = w.entities_near([p[0] as f64 + 0.5, p[1] as f64, p[2] as f64 + 0.5], r, None);
        let parts: Vec<String> = ids
            .iter()
            .filter(|&&id| id != player)
            .filter_map(|&id| {
                w.entity(id)
                    .map(|e| format!("{id}:{}", name_of(ek::ENTITY, e.kind)))
            })
            .collect();
        Ok(parts.join(" "))
    }),
    ("action", "@owner [name:word]", |w, cmd, _, _| {
        w.action_play(owner(w, cmd)?, cmd.text("name").unwrap_or(""), false);
        Ok(String::new())
    }),
    ("survey", "<r:int>", |w, cmd, _, me| {
        // Biomes of every block within `r` of the player, and where glow crystals are.
        let r: i32 = int(cmd, "r")?;
        let crystal = Block::lookup("testmod:glow_crystal").ok_or("no crystal block")?;
        let cave: Tag = Tag::named("cave");
        let mut biomes: std::collections::BTreeMap<&str, u32> = Default::default();
        let (mut in_cave, mut elsewhere) = (0, 0);
        for y in me[1] - r..=me[1] + r {
            for z in me[2] - r..=me[2] + r {
                for x in me[0] - r..=me[0] + r {
                    let b = w.biome([x, y, z]);
                    *biomes.entry(name_of(ek::BIOME, b)).or_default() += 1;
                    if block_of(w.block([x, y, z])) == block_of(crystal) {
                        if cave.on_biome(b) { in_cave += 1 } else { elsewhere += 1 }
                    }
                }
            }
        }
        let list: Vec<String> = biomes.iter().map(|(n, k)| format!("{n}={k}")).collect();
        Ok(format!("{} | crystals {in_cave} {elsewhere}", list.join(" ")))
    }),
    ("drops", "<n:int>", |w, cmd, _, me| {
        // `n` dropped stacks of a few kinds (blocks and sprites) in a grid north of the
        // player, 1.1 apart so they do not merge.
        let n: u32 = int(cmd, "n")?;
        let kinds: Vec<u32> = ["base:dirt", "base:coal", "base:iron_pickaxe", "base:dandelion", "base:stick", "base:torch"]
            .iter()
            .filter_map(|k| Item::lookup(k))
            .collect();
        let side = (n as f64).sqrt().ceil() as u32;
        for i in 0..n {
            let (x, z) = ((i % side) as f64 * 1.1 - side as f64 * 0.55, -3.0 - (i / side) as f64 * 1.1);
            w.spawn_item([me[0] as f64 + x, me[1] as f64 + 0.5, me[2] as f64 + z], ItemStack::new(kinds[i as usize % kinds.len()], 1));
        }
        Ok(format!("{n} drops"))
    }),
    ("crowd", "<kind:word> <n:int>", |w, cmd, _, me| {
        // `n` mobs in a grid north of the player, walking on the spot if they have room to.
        let n: u32 = int(cmd, "n")?;
        let kind = EntityKind::lookup(word(cmd, "kind")?).ok_or("no such entity")?;
        let side = (n as f64).sqrt().ceil() as u32;
        for i in 0..n {
            let (x, z) = ((i % side) as f64 * 1.5 - side as f64 * 0.75, -8.0 - (i / side) as f64 * 1.5);
            w.spawn(kind, [me[0] as f64 + x, me[1] as f64, me[2] as f64 + z]);
        }
        Ok(format!("{n} mobs"))
    }),
    ("swap", "<id:int> <from:word> [to:word]", |w, cmd, _, _| {
        match cmd.text("to") {
            Some(to) => w.action_override(int(cmd, "id")?, word(cmd, "from")?, to),
            None => w.action_override_clear(int(cmd, "id")?, word(cmd, "from")?),
        }
        Ok(String::new())
    }),
    ("find", "<r:int> <block:block>", |w, cmd, _, me| {
        // The first block of that kind within `r` of the player, not far above or below.
        let r: i32 = int(cmd, "r")?;
        let want = block_of(id(cmd, "block")?);
        let up = r.min(40);
        for y in me[1] - up..=me[1] + up {
            for z in me[2] - r..=me[2] + r {
                for x in me[0] - r..=me[0] + r {
                    if block_of(w.block([x, y, z])) == want {
                        return Ok(format!("{x} {y} {z}"));
                    }
                }
            }
        }
        Err("not found".into())
    }),
    ("census", "<r:int> <blocks:word>", |w, cmd, _, me| {
        // How many of each block (`a,b`) lie within `r` of the player.
        let r: i32 = int(cmd, "r")?;
        let names: Vec<&str> = word(cmd, "blocks")?.split(',').collect();
        let ids: Vec<u32> = names
            .iter()
            .map(|n| {
                Block::lookup(n)
                    .map(block_of)
                    .ok_or(format!("no block {n}"))
            })
            .collect::<Result<_, _>>()?;
        let mut counts = vec![0u32; ids.len()];
        for y in me[1] - r..=me[1] + r {
            for z in me[2] - r..=me[2] + r {
                for x in me[0] - r..=me[0] + r {
                    let b = block_of(w.block([x, y, z]));
                    if let Some(i) = ids.iter().position(|&id| id == b) {
                        counts[i] += 1;
                    }
                }
            }
        }
        Ok(names
            .iter()
            .zip(&counts)
            .map(|(n, k)| format!("{n}={k}"))
            .collect::<Vec<_>>()
            .join(" "))
    }),
    ("cube_top_plants", "<r:int>", |w, cmd, _, me| {
        // Land columns within `r` of the player, split by whether the ground is the top layer
        // of its cube, and how many of each carry a plant: `tops plants others plants`.
        let r: i32 = int(cmd, "r")?;
        let plants: Vec<u32> = ["base:tall_grass", "base:dandelion", "base:rose"]
            .iter()
            .map(|n| {
                Block::lookup(n)
                    .map(block_of)
                    .ok_or(format!("no block {n}"))
            })
            .collect::<Result<_, _>>()?;
        let mut n = [0u32; 4];
        for z in me[2] - r..=me[2] + r {
            for x in me[0] - r..=me[0] + r {
                let h = base_api::OVERWORLD.column(5, x, z).height;
                if h < 63 {
                    continue;
                }
                let k = if (h + 1) & 15 == 0 { 0 } else { 2 };
                n[k] += 1;
                n[k + 1] += plants.contains(&block_of(w.block([x, h + 1, z]))) as u32;
            }
        }
        Ok(format!("{} {} {} {}", n[0], n[1], n[2], n[3]))
    }),
    ("onto", "<id:int> <h:float> [rider:int]", |w, cmd, player, _| {
        // The player (or the rider) on top of an entity `h` high, in the same tick (moving
        // platforms move on while a teleport by command makes its way).
        let e = w.entity(int(cmd, "id")?).ok_or("no such entity")?;
        let rider = cmd.int("rider").map_or(player, |r| r as u64);
        w.teleport(rider, None, [e.pos.x, e.pos.y + cmd.float("h").unwrap_or(0.0) + 0.05, e.pos.z]);
        Ok(String::new())
    }),
    ("navigate", "@owner <to:pos>", |w, cmd, _, _| {
        let p = pos(cmd, "to")?;
        Ok((w.navigate(owner(w, cmd)?, [p[0] as f64 + 0.5, p[1] as f64, p[2] as f64 + 0.5], 1.0) as u32).to_string())
    }),
    ("structure", "<name:word> <r:int>", |w, cmd, _, me| {
        Ok(w.structure_near(word(cmd, "name")?, me, int(cmd, "r")?)
            .map_or("none".into(), |a| format!("{} {} {}", a.x, a.y, a.z)))
    }),
    // A timer on the player, `dim` or `block` under them: `after 60`, then `rings` counts the
    // ones that fired.
    ("after", "<ticks:int> [on:word]", |w, cmd, player, me| {
        let on = match cmd.text("on") {
            Some("dim") => Holder::NONE,
            Some("block") => Holder::from([me[0], me[1] - 1, me[2]]),
            _ => Holder::entity(player),
        };
        w.timer(on, int(cmd, "ticks")?, &RANG, player);
        Ok(String::new())
    }),
    ("rings", "", |_, _, _, _| Ok(RINGS.load(Ordering::Relaxed).to_string())),
    ("edited", "", |w, _, player, _| {
        let i = EDITED.id();
        let b = EDITED_BIOME.id();
        let mut drops: Vec<String> = w
            .roll_loot(&EDITED_LOOT, &LootCtx::default())
            .iter()
            .map(|s| format!("{}x{}", item_name(s.item), s.count))
            .collect();
        drops.sort();
        let oil: Tag = Tag::named("testkit:edited");
        let probe = |dx: i32| name_of(ek::BIOME, MARS.column(w.seed(), PROBE[0] + dx, PROBE[1]).biome) == "base:plains";
        let size = w
            .kind_size(EDITED_KIND.name())
            .map_or([0.0; 3], |s| [s.width, s.height, s.eye]);
        // Each state of the lamp: light, opacity, warmth, lit tag.
        let lit: Tag = Tag::named("testkit:lit");
        let lamp: Vec<String> = (LAMP.id()..LAMP.id() + 2)
            .map(|s| {
                let i = block_info(s).map_or(([0; 3], 0), |i| (i.light, i.opacity));
                format!("{}/{}/{}/{}", i.0[0], i.1, WARMTH.on_block(s).unwrap_or(0.0), lit.on_block(s))
            })
            .collect();
        let stairs = block_info(OAK_STAIRS.id()).map_or(0, |i| i.states);
        let stairs_lit = (OAK_STAIRS.id()..OAK_STAIRS.id() + stairs).all(|s| block_info(s).is_some_and(|i| i.light == [7; 3]));
        Ok(format!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {:?} {} {} {} {}",
            MAX_STACK.on_item(i).unwrap_or(0),
            ATTACK_DAMAGE.on_item(i).unwrap_or(0.0),
            WARMTH.on_biome(b).unwrap_or(0.0),
            Tag::<kind::Biome>::named("edited_a").on_id(b),
            format!(
                "{}/{}/{}",
                !comps::weather::DRY.on_id(STORM.id()),
                comps::weather::LIGHTNING.on_id(STORM.id()),
                comps::weather::UNSHELTERED.on_id(STORM.id())
            ),
            w.attribute(player, &GRIT),
            recipe_process(&base_api::SMELTING, i).is_none(),
            drops.join(","),
            liquid_of(OIL.id()) == Some(OIL.liquid().id()),
            oil.on_block(OIL.id()) && oil.on_item(Item::generated(OIL.name().to_string()).id()),
            (probe(0), probe(4)) == (true, false),
            oil.on_dimension(MARS.id()),
            size,
            WARMTH.on_entity(EDITED_KIND.id()).unwrap_or(0.0),
            lamp.join(","),
            stairs,
            stairs_lit,
        ))
    }),
    ("look", "", |w, _, player, _| {
        Ok(w.looked_at(player, 5.0, Ray::blocks())
            .map_or("none".into(), |l| format!("{} {} {} {} {}", l.body, l.pos.x, l.pos.y, l.pos.z, block_name(l.state))))
    }),
    // Records with testmod's style tag: `name=weight` each, by name.
    ("records", "", |_, _, _, _| {
        let mut out: Vec<String> = entries_with(ek::RECORD, STYLE.name())
            .into_iter()
            .map(|id| format!("{}={}", name_of(ek::RECORD, id), STYLE_WEIGHT.on_record(id).unwrap_or(-1.0)))
            .collect();
        out.sort();
        Ok(out.join(" "))
    }),
    // Features placed in generated cubes so far: `name x y z` each.
    ("placed", "", |_, _, _, _| {
        let placed = PLACED.lock().unwrap();
        let parts: Vec<String> = placed
            .iter()
            .map(|(f, p)| format!("{} {} {} {}", name_of(ek::FEATURE, *f), p.x, p.y, p.z))
            .collect();
        Ok(parts.join(", "))
    }),
    ("place", "<template:word> <pos:pos>", |w, cmd, _, _| {
        let t = Template::generated(word(cmd, "template")?.to_string());
        Ok((w
            .place_template(&t, pos(cmd, "pos")?, Transform::NONE, None)
            .is_some() as u32)
            .to_string())
    }),
    ("heal", "", |w, _, player, _| {
        w.heal(player, f32::MAX);
        Ok(String::new())
    }),
    ("push", "<x:float> <y:float> <z:float> [id:int]", |w, cmd, player, _| {
        let v = ["x", "y", "z"].map(|a| cmd.float(a).unwrap_or(0.0));
        w.push(cmd.int("id").map_or(player, |i| i as u64), v);
        Ok(String::new())
    }),
    ("drive", "<id:int> <dx:float> <dz:float> <ticks:int>", |_, cmd, _, _| {
        let dir = [cmd.float("dx").unwrap_or(0.0) as f32, cmd.float("dz").unwrap_or(0.0) as f32];
        DRIVEN
            .lock()
            .unwrap()
            .push((int(cmd, "id")?, dir, int(cmd, "ticks")?));
        Ok(String::new())
    }),
    ("health", "[id:int]", |w, cmd, player, _| {
        let id = cmd.int("id").map_or(player, |i| i as u64);
        Ok(w.entity(id)
            .map_or("none".into(), |e| format!("{}", e.health)))
    }),
    ("time", "<ticks:int>", |w, cmd, _, _| {
        w.set_day_time(int(cmd, "ticks")?);
        Ok(String::new())
    }),
    ("mode", "survival | creative | spectator", |w, cmd, player, _| {
        let m = ["survival", "creative", "spectator"]
            .iter()
            .position(|m| cmd.given(m))
            .ok_or("no game mode")?;
        w.set_game_mode(player, m as u32);
        Ok(String::new())
    }),
    ("menu", "<which:word> <pos:pos>", |w, cmd, player, _| {
        let name = word(cmd, "which")?;
        let m = Menu::lookup(name).ok_or_else(|| format!("no menu {name}"))?;
        let p = pos(cmd, "pos")?;
        if !w.open_menu(player, m, p) {
            return Err(format!("no block entity at {p:?} ({})", block_name(w.block(p))));
        }
        Ok(String::new())
    }),
    ("emenu", "<which:word> <id:int>", |w, cmd, player, _| {
        let name = word(cmd, "which")?;
        let m = Menu::lookup(name).ok_or_else(|| format!("no menu {name}"))?;
        let id: u64 = int(cmd, "id")?;
        if !w.open_menu(player, m, id) {
            return Err(format!("can't open {name} on {id}"));
        }
        Ok(String::new())
    }),
    ("hmenu", "<which:word> <slot:int>", |w, cmd, player, _| {
        let name = word(cmd, "which")?;
        let m = Menu::lookup(name).ok_or_else(|| format!("no menu {name}"))?;
        let slot = int(cmd, "slot")?;
        if !w.open_menu(player, m, Holder::held(player, slot)) {
            return Err(format!("nothing to open in slot {slot}"));
        }
        Ok(String::new())
    }),
    ("hput", "<slot:int> <index:int> <item:item> [count:int]", |w, cmd, player, _| {
        let held = w.inventory(Holder::held(player, int(cmd, "slot")?));
        held.set(int(cmd, "index")?, ItemStack::new(id(cmd, "item")?, int(cmd, "count").unwrap_or(1)));
        Ok(held.len().to_string())
    }),
    ("stash", "<which:word> [items:text]", |w, cmd, player, _| {
        let name = word(cmd, "which")?;
        let m = Menu::lookup(name).ok_or_else(|| format!("no menu {name}"))?;
        let inv = w.inventory(STASH.of(player));
        // Pairs of item and count, the rest emptied.
        let words: Vec<&str> = cmd.text("items").unwrap_or("").split_whitespace().collect();
        for i in 0..inv.len() {
            let stack = match words.chunks(2).nth(i as usize) {
                Some([n, c]) => ItemStack::new(item(n)?, c.parse().map_err(|_| format!("bad count {c}"))?),
                _ => ItemStack::EMPTY,
            };
            inv.set(i, stack);
        }
        if !w.open_menu(player, m, STASH.of(player)) {
            return Err(format!("cannot open {name} on the stash"));
        }
        Ok(inv.len().to_string())
    }),
    // A stick with a diamond over it, tinted red: a stack's own layered picture.
    ("layered", "<index:int>", |w, cmd, player, _| {
        let s = ItemStack::new(item("base:stick")?, 1).with_layers(
            Layers::new()
                .layer("base:stick")
                .layer("base:diamond")
                .tint(0xFF2020),
        );
        w.inventory(player).set(int(cmd, "index")?, s);
        Ok(String::new())
    }),
    // `rider` (the player by default) onto `mount`: 1 if it got on.
    ("mount", "<onto:int> [rider:int]", |w, cmd, player, _| {
        let rider = cmd.int("rider").map_or(player, |r| r as u64);
        Ok((w.mount(rider, int(cmd, "onto")?) as u32).to_string())
    }),
    // Entity `id` to dimension `dim` (`here` for the same one) at x y z.
    ("send", "<id:int> <dim:word> <x:float> <y:float> <z:float>", |w, cmd, _, _| {
        let p = ["x", "y", "z"].map(|a| cmd.float(a).unwrap_or(0.0));
        let name = word(cmd, "dim")?;
        let dim = (name != "here").then(|| Dimension::generated(name.to_string()));
        w.teleport(int(cmd, "id")?, dim, p);
        Ok(String::new())
    }),
    // What `id` (the player by default) rides and its seat, "0 0" for nothing.
    // The player's camera: a mode by name, held there with `locked` 1; `release` gives it back.
    ("camera", "<which:word> [locked:int]", |w, cmd, player, _| {
        let name = word(cmd, "which")?;
        if name == "release" {
            w.release_camera(player, None);
        } else {
            if CameraMode::lookup(name).is_none() {
                return Err(format!("no camera {name}"));
            }
            let m = CameraMode::generated(name.to_string());
            if cmd.int("locked") == Some(1) {
                w.force_camera(player, m, None);
            } else {
                w.set_camera(player, m, None);
            }
        }
        Ok(String::new())
    }),
    ("vehicle", "[id:int]", |w, cmd, player, _| {
        let id = cmd.int("id").map_or(player, |r| r as u64);
        Ok(w.vehicle(id)
            .map_or("0 0".to_string(), |(m, s)| format!("{m} {s}")))
    }),
    ("stashed", "<index:int>", |w, cmd, player, _| Ok(stack_text(w.inventory(STASH.of(player)).get(int(cmd, "index")?)))),
    ("remove", "<id:int>", |w, cmd, _, _| {
        w.despawn(int(cmd, "id")?);
        Ok(String::new())
    }),
    ("glow", "@entity <r:int> <g:int> <b:int>", |w, cmd, _, _| {
        let rgb = [int(cmd, "r")?, int(cmd, "g")?, int(cmd, "b")?];
        Ok(w.set_component(owner(w, cmd)?, &LIGHT, rgb).to_string())
    }),
    ("lit", "@entity <ticks:int>", |w, cmd, _, _| {
        w.entity_effect(owner(w, cmd)?, &LIT, 1, int(cmd, "ticks")?);
        Ok(String::new())
    }),
    ("mark", "<id:int> <value:int>", |w, cmd, _, _| {
        *w.component_mut(int(cmd, "id")?, &MARK)
            .ok_or("no mark component")? = int(cmd, "value")?;
        Ok(String::new())
    }),
    ("secret", "@owner <index:int> [value:int]", |w, cmd, _, _| {
        let inv = w.inventory(owner(w, cmd)?);
        let slot: u32 = int(cmd, "index")?;
        let s = inv.get(slot);
        match cmd.int("value") {
            Some(v) => {
                inv.set(slot, s.with(&SECRET, v as u32));
                Ok(String::new())
            }
            None => Ok(SECRET
                .on_stack_own(&s)
                .map_or("none".into(), |v| v.to_string())),
        }
    }),
    ("events", " | reset", |_, cmd, _, _| {
        if cmd.given("reset") {
            COUNTS.iter().for_each(|c| c.store(0, Ordering::Relaxed));
        }
        Ok(EVENT_NAMES
            .iter()
            .zip(&COUNTS)
            .map(|(n, c)| format!("{n}={}", c.load(Ordering::Relaxed)))
            .collect::<Vec<_>>()
            .join(" "))
    }),
    ("attr", "@owner <attribute:word>", |w, cmd, _, _| {
        let (v, text) = w.attribute_explain(owner(w, cmd)?, Attribute::generated(word(cmd, "attribute")?.to_string()));
        Ok(format!("{v} | {text}"))
    }),
    ("attr_mod", "@owner <attribute:word> <mod:word> <step:word> <amount:float> <ticks:int>", |w, cmd, _, _| {
        let o = owner(w, cmd)?;
        let amount = cmd.float("amount").ok_or("missing amount")?;
        let attr = Attribute::generated(word(cmd, "attribute")?.to_string());
        let m = Modifier::step(attr, Step::generated(word(cmd, "step")?.to_string()), amount);
        w.attribute_add_modifier(o, word(cmd, "mod")?, &m, int(cmd, "ticks")?);
        Ok(String::new())
    }),
    ("attr_rm", "@owner <attribute:word> <mod:word>", |w, cmd, _, _| {
        w.attribute_remove_modifier(owner(w, cmd)?, Attribute::generated(word(cmd, "attribute")?.to_string()), word(cmd, "mod")?);
        Ok(String::new())
    }),
    ("loot", "<table:word> <n:int> [ctx:text]", |w, cmd, _, _| {
        let table = LootTable::generated(word(cmd, "table")?.to_string());
        let n: u32 = int(cmd, "n")?;
        let mut ctx = LootCtx::default();
        for word in cmd.text("ctx").unwrap_or("").split_whitespace() {
            match word {
                "fire" => ctx.on_fire = true,
                "player" => ctx.by_player = true,
                // Mined by the tool, from the block the table is named after.
                "mined" => ctx.mined = Block::lookup(table.name()).ok_or("not a block's table")?,
                tool => ctx.tool = ItemStack::new(item(tool)?, 1),
            }
        }
        let mut totals: std::collections::BTreeMap<String, u32> = Default::default();
        for _ in 0..n {
            for s in w.roll_loot(table, &ctx) {
                *totals.entry(item_name(s.item).to_string()).or_default() += s.count;
            }
        }
        Ok(totals
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" "))
    }),
    ("break", "<pos:pos> [tool:item]", |w, cmd, player, _| {
        let tool = cmd
            .id("tool")
            .map_or(ItemStack::EMPTY, |t| ItemStack::new(t, 1));
        Ok((w.break_block(pos(cmd, "pos")?, tool, player, true) as u32).to_string())
    }),
    // A melee hit's damage with what `item` makes of the player having fallen `fall` blocks.
    ("attack_damage", "<item:item> <fall:float>", |w, cmd, player, _| {
        let fall = cmd.float("fall").ok_or("missing fall")?;
        w.entity_modify(player).fall_distance(fall as f32);
        let base = 1.0;
        let q = abi::Query { entity: player, item: id(cmd, "item")?, amount: base, ..query() };
        let fell = w.entity(player).map_or(-1.0, |s| s.fall_distance);
        Ok(format!("{} {fell}", w.chain_compute(&attribute::ATTACK_DAMAGE, base, &q)))
    }),
    // The player hits `id` as if having fallen `fall` blocks first.
    ("hit_falling", "<id:int> <fall:float>", |w, cmd, player, _| {
        w.entity_modify(player)
            .fall_distance(cmd.float("fall").unwrap_or(0.0) as f32);
        w.damage(int(cmd, "id")?, Damage::new(1.0, damage_cause::ATTACK).by(player));
        Ok(String::new())
    }),
    // Three blocks up, flying there: landing would end the flight.
    ("edit_me", "<yaw:float> <slot:int> <flying:int> <fire:int>", |w, cmd, player, _| {
        let up = w
            .entity(player)
            .ok_or("no player")?
            .pos
            .offset(0.0, 3.0, 0.0);
        w.entity_modify(player)
            .pos(up)
            .yaw(cmd.float("yaw").unwrap_or(0.0) as f32)
            .selected(int(cmd, "slot")?)
            .flying(int::<u32>(cmd, "flying")? != 0)
            .fire(int(cmd, "fire")?);
        let s = w.entity(player).ok_or("no player")?;
        Ok(format!("{} {}", s.selected, s.fire))
    }),
    ("damage", "<amount:float> [cause:int]", |w, cmd, player, _| {
        let amount = cmd.float("amount").ok_or("missing amount")?;
        let cause: u32 = int(cmd, "cause").unwrap_or(abi::damage_cause::GENERIC);
        let q = abi::Query { entity: player, cause, amount, ..query() };
        let (v, text) = w.chain_explain(&chain::DAMAGE, amount, &q);
        Ok(format!("{v} | {text}"))
    }),
    ("in_liquid", "<x:float> <y:float> <z:float>", |w, cmd, _, _| {
        let p = ["x", "y", "z"].map(|a| cmd.float(a).unwrap_or(0.0));
        Ok((w.in_liquid(p) as u32).to_string())
    }),
    // A hit from the player who runs it.
    ("effect", "<id:int> <which:word> <level:int> <ticks:int>", |w, cmd, _, _| {
        let name = word(cmd, "which")?;
        let id = EntityEffect::lookup(name).ok_or_else(|| format!("no entity effect {name}"))?;
        w.entity_effect(int(cmd, "id")?, id, int(cmd, "level")?, int(cmd, "ticks")?);
        Ok(String::new())
    }),
    ("hurt", "<id:int> <amount:float>", |w, cmd, player, _| {
        let amount = cmd.float("amount").ok_or("missing amount")? as f32;
        Ok((w.damage(int(cmd, "id")?, Damage::new(amount, abi::damage_cause::ATTACK).by(player)) as u32).to_string())
    }),
    ("pose", "", |w, _, player, _| Ok(w.entity(player).map_or(0, |e| e.pose).to_string())),
    ("hit_from", "<source:int> <amount:float> <weapon:item> [cause:int] [hand:word(main,off)] [knock:pos]", |w, cmd, player, _| {
        let amount = cmd.float("amount").ok_or("missing amount")? as f32;
        let cause = int(cmd, "cause").unwrap_or(abi::damage_cause::ATTACK);
        let weapon = ItemStack::new(id(cmd, "weapon")?, 1);
        if let Some(hand) = cmd.text("hand") {
            w.use_item(player, hand == "off");
        }
        let mut damage = Damage::new(amount, cause)
            .by(int(cmd, "source")?)
            .with(weapon);
        if let Some(knock) = cmd.pos("knock") {
            damage = damage.knock(knock);
        }
        Ok((w.damage(player, damage) as u32).to_string())
    }),
    ("hurt_me", "<amount:float>", |w, cmd, player, _| {
        let amount = cmd.float("amount").ok_or("missing amount")? as f32;
        Ok((w.damage(player, Damage::new(amount, abi::damage_cause::ATTACK)) as u32).to_string())
    }),
    ("snapshot", "<from:pos> <to:pos>", |w, cmd, _, _| {
        let (p, q) = (pos(cmd, "from")?, pos(cmd, "to")?);
        let mut out = Vec::new();
        for y in p[1].min(q[1])..=p[1].max(q[1]) {
            for z in p[2].min(q[2])..=p[2].max(q[2]) {
                for x in p[0].min(q[0])..=p[0].max(q[0]) {
                    out.push(w.block([x, y, z]).to_string());
                }
            }
        }
        Ok(out.join(" "))
    }),
];
