//! A starting point for your own mod. Copy this folder, rename it, and replace the
//! examples with your stuff.
//!
//! Rename checklist:
//! - `Cargo.toml`: `name` is your mod id (and the library's file name and the mod folder's),
//!   `version` the version players see.
//! - `declare_mod!`: `deps`, the mods yours needs.
//! - `assets/lang/*.lang`: the keys start with your mod id; `mod.<id>.name` is the name players
//!   see (without it, the id).
//!
//! Build: `cargo build --release`. Then make `run/mods/<your id>/` with the library from
//! `target/release/` and the `assets` folder next to it.
//! To share it, run `weft --pack-mod run/mods/<your id>`,
//! it packs that folder (library included) into a zip.
//!
//! This example adds one item, a setting, and a gift for joined players.
//! Everything else is commented examples. The full cheat sheet is at `docs/modding.md`.
//! `mods/base` and `mods/testmod` use exactly this API, so for more examples check those.

use weft_sdk::*;

declare_mod!(deps: ["base"], init: init);

// Everything your mod adds is named in a `static` handle and registered by it. A name without
// `mod:` is yours (`examplemod:example_item`), `Item::named("base:coal")` is another mod's.
// Once the game is loaded `EXAMPLE_ITEM.id()` is the numeric id hot code works with.
pub static EXAMPLE_ITEM: Item = Item::named("example_item");
pub static GIFT_ON_JOIN: Config<bool> = Config::named("gift_on_join");
// The base game's tab, by name.
static INGREDIENTS: CreativeTab = CreativeTab::named("base:ingredients");

/// Called once at startup, in parallel with the other mods: register everything here.
/// Nothing exists in the world yet, you only describe what your mod adds.
fn init(r: &mut Registry) {
    config(r);
    blocks(r);
    items(r);
    recipes(r);
    worldgen(r);
    entities(r);
    events(r);
    commands(r);
    client(r);
}

/// Settings show up in Settings > Mod Settings by themselves and are saved to
/// `config/examplemod.txt`. Name and tooltip come from `config.examplemod.<key>` in the lang file.
fn config(r: &mut Registry) {
    r.config(&GIFT_ON_JOIN).bool(true);
    // More examples:
    //      static POWER: Config<i32> = Config::named("power");
    //      static STYLE: Config<u32> = Config::named("style");
    //      r.config(&POWER).int(10, 0, 100);
    //      r.config(&STYLE).choice(0, &["warm", "cold"]);
    // Where the value lives: the player's own by default, `.server()` the server's (its players
    // get it and every change, for anything gameplay depends on), `.world()` saved with the
    // world and changed with `/rule` (game rules):
    //      static HARD_MODE: Config<bool> = Config::named("hard_mode");
    //      r.config(&HARD_MODE).bool(false).world();
    // The engine's own settings are the same: `settings::FOV.get()`, `settings::LANGUAGE.set("ru_ru")`.
}

/// Your blocks, texture is `block/<name>.png`, translation is `block.examplemod.<name>`.
/// More at `mods/base/src/blocks.rs` (support rules, plants, falling sand, torches, axis, etc.)
/// And `mods/testmod/src/lib.rs` (block entities, facing, editing a block etc.)
fn blocks(_r: &mut Registry) {
    // static RUBY_ORE: Block = Block::named("ruby_ore");
    // static NATURAL: CreativeTab = CreativeTab::named("base:natural");
    // r.block(&RUBY_ORE)
    //     .hardness(3.0)
    //     .mined_by("pickaxe", tool_tier::IRON)   // drops nothing without at least an iron pickaxe
    //     .drops("ruby", 1)
    //     .tab(&NATURAL);
    //
    // Light and glass-like looks plus a custom box:
    // static LAMP: Block = Block::named("lamp");
    // static PANE: Block = Block::named("pane");
    // r.block(&LAMP).light_rgb(15, 10, 4).hardness(0.3);
    // r.block(&PANE).model_box([0.0, 0.0, 7.0], [16.0, 16.0, 9.0]).cutout();   // faces show block/pane
    // A shape made in Blockbench needs no code: `assets/models/block/<name>.json` (Java block model).
    //
    // Behavior (all optional, all get a `World`):
    // static BUTTON: Block = Block::named("button");
    // r.block(&BUTTON).on_use(|u| { u.world(); u.pos(); u.player(); true });
    //
    // Redstone: say what it is and the engine runs it (docs/redstone.md):
    // static CRYSTAL: Block = Block::named("crystal");
    // static ALARM: Block = Block::named("alarm");
    // static AND_GATE: Block = Block::named("and_gate");
    // r.block(&CRYSTAL).signal_constant();                        // always on
    // r.block(&ALARM).signal_sink().on_signal(|e| { e.rising(); e.world(); });
    // r.block(&AND_GATE)
    //     .signal_gate(Gate::new(), |s| if s.side(Side::Rear) > 0 && (s.side(Side::Left) > 0 || s.side(Side::Right) > 0) { 15 } else { 0 })
    //     .model_box([0.0, 0.0, 0.0], [16.0, 2.0, 16.0])
    //     .not_solid();
    //
    // A family of blocks made in a loop gets its handles from the names:
    // for color in ["red", "blue"] {
    //     r.block(Block::generated(format!("{color}_brick"))).hardness(2.0);
    // }
    //
    // Another mod's block changed: what you set goes on every state, `state_where` on some.
    // `r.replace(&X)` registers it again from scratch, `r.remove(&X)` takes it out.
    // static FURNACE: Block = Block::named("base:furnace");
    // r.edit(&FURNACE).hardness(5.0).state_where("lit=true", |s| s.light(15));
    // The same edit on everything a pattern names (`#tag`, a name with `*`, `base:furnace[lit=true]`):
    // r.edit_pattern::<kind::Block>("#logs").hardness(3.0);
}

/// Your items, texture is `item/<name>.png`, translation is `item.examplemod.<name>`
/// Gray tooltip with `item.examplemod.<name>.tooltip`, for more tooltip options check `mods/base/assets/lang/en_us.lang`
fn items(r: &mut Registry) {
    r.item(&EXAMPLE_ITEM).tab(&INGREDIENTS).tag("gems/example");
    // Tools, weapons, food, fuel are all components or tags, so other mods can use them:
    // static RUBY_PICKAXE: Item = Item::named("ruby_pickaxe");
    // r.item(&RUBY_PICKAXE).tool("pickaxe", tool_tier::DIAMOND).mining_speed(8.0).durability(1200).attack_damage(4.0);
    // r.item(&RUBY_APPLE).set(&FOOD, ...)     // see `mods/base/src/items.rs` for food and armor
    // static FUEL: Component<u32> = Component::named("fuel");   // the common burn time, in ticks
    // r.set("examplemod:example_item", &FUEL, 1600);
}

/// Recipes reference items by name or `#tag`, so a recipe made with `#ingots/iron` tag takes any mod's
/// item with the same tag. `base:crafting` and `base:smelting` belong to the base game.
fn recipes(_r: &mut Registry) {
    // static CRAFTING: RecipeKind = RecipeKind::named("base:crafting");
    // static SMELTING: RecipeKind = RecipeKind::named("base:smelting");
    // r.shaped(&CRAFTING, "ruby_pickaxe", 1, &["rrr", " s ", " s "], &[('r', "#gems/ruby"), ('s', "#stick")]);
    // r.shapeless(&CRAFTING, "ruby", 9, &["ruby_block"], 0);
    // r.shapeless(&SMELTING, "ruby", 1, &["ruby_ore"], 200);    // 200 ticks in a furnace
    // Each is named `<mod>:<kind>/<output>` (this one `examplemod:smelting/ruby`), so any is removed by it:
    // r.remove(&Recipe::named("base:crafting/torch"));
}

/// Ores, biomes, structures and dimensions. Ores go into every dimension whose stone they replace.
/// Mars as a whole planet from terrain presets is at `mods/testmod/src/lib.rs`
/// More at `mods/base/src/worldgen.rs`
fn worldgen(_r: &mut Registry) {
    // static OVERWORLD: Dimension = Dimension::named("base:overworld");
    // static RUBY_ORE: Block = Block::named("ruby_ore");
    // r.ore(&RUBY_ORE).y(-64, 16).veins(2).vein_size(5).replaces("#stone_ore_replaceables");
    // static SHRINE: Template = Template::named("shrine");
    // r.template(&SHRINE).layer(&["ccc", "c c", "ccc"]).key('c', "base:cobblestone");
    // r.structure(&SHRINE).dimension(&OVERWORLD).spacing(256).chance(0.3);
    // Something new in the sky, on an orbit of its own (textures/sky/planet.png):
    // static PLANET: SkyObject = SkyObject::named("planet");
    // r.sky_body(&OVERWORLD, &PLANET).texture("planet").size(6.0).orbit(96000.0).phase(0.1).tilt(20.0);
    // A weather of your own in base's overworld: lists take `push`, `insert_before`, `remove_item`..
    // static ACID_RAIN: Weather = Weather::named("acid_rain");
    // r.weather(&ACID_RAIN).darkness(0.5).duration(2400, 6000).weight(1);
    // r.edit(&OVERWORLD).push(&comps::dimension::WEATHERS, ACID_RAIN.name().to_string());
    // Base's iron, twice as common (an ore is named like its block):
    // static IRON_ORE: Block = Block::named("base:iron_ore");
    // r.edit(IRON_ORE.ore()).veins(12);
}

/// Mobs and other entities
/// Models in code (`humanoid`, `quadruped`, `spider`) or Blockbench files in `assets/models`
/// More at `mods/base/src/mobs.rs` (AI goals, spawn rules, etc.).
fn entities(_r: &mut Registry) {
    // static GHOST: EntityKind = EntityKind::named("ghost");
    // r.entity(&GHOST).size(0.6, 1.8).max_health(10.0).walk_speed(3.0).hostile().model(humanoid(false)).sound("hurt");
    // r.natural_spawn(&GHOST).light(0, 4).weight(1);
}

/// Reacting to the game, events arrive batched once per tick
/// for "may this happen" use `r.before_event`
/// to change a number use `r.modify`
/// For more info check "Hooking into the game" in `docs/modding.md`
fn events(r: &mut Registry) {
    // Players joining get an example item if the setting allows it.
    r.on_event(&PLAYER_JOINED, |batch| {
        if !GIFT_ON_JOIN.get() {
            return;
        }
        let w = batch.world();
        for joined in batch.iter() {
            w.inventory(joined.player)
                .insert(ItemStack::new(EXAMPLE_ITEM.id(), 1));
            info!("gave player {} an example item", joined.player);
        }
    });
    // r.on_event(&actions::BREAK_BLOCK, |b| for e in b.iter() { /* e.player, e.pos, e.state */ });
    // r.before_event(&actions::BREAK_BLOCK, |q| if q.pos().y < -60 { Verdict::Deny } else { Verdict::Abstain });
    // r.modify(&chain::DAMAGE, stage::MULTIPLY, |m| m.value *= 0.9);
}

/// Chat commands
/// The usage string is parsed, checked, completed with Tab and shown in `/help` by the engine
/// you only read the values by name.
fn commands(_r: &mut Registry) {
    // Text on a player's screen, all one call with different defaults (docs: Text on the screen).
    // r.on_event(&PLAYER_JOINED, |b| for e in b.iter() {
    //     b.world().hint(e.player, "examplemod.hello", &[]);
    //     b.world().show(e.player, "examplemod.hello", &[]).at(0.5, 0.1).scale(2.0).times(0.5, 2.0, 0.5);
    // });
    // r.command("example", "[times:int]", |cmd| {
    //     let n = cmd.int("times").unwrap_or(1);
    //     cmd.reply("examplemod.hello", &[Arg::Int(n)]);    // `examplemod.hello = hello {0} times` in lang
    // });
}

/// Client side: screens, HUD, key bindings, packets.
/// More at:
/// `mods/base/src/player.rs` (HUD),
/// `mods/testmod/src/lib.rs` (screens, block entity panels),
/// `mods/testmod/src/looks.rs` (particles, shaders, sounds).
fn client(_r: &mut Registry) {
    // static DASH: Key = Key::named("dash");
    // static ABOUT: Menu = Menu::named("about");
    // static MANA: HudElement = HudElement::named("mana");
    // r.key(&DASH, "R").on_server(|p| { let w = p.world(); info!("{} dashes", p.player()); w.game_time(); });
    // r.menu(&ABOUT).screen(|ui| ui.panel("background", S::column().pad(8.0), |ui| ui.text("examplemod.about")));
    // r.hud_element(&MANA, hud_anchor::HOTBAR_RIGHT, 2, |ui| ui.text_bind(|c, out| { /* ... */ }));
    //
    // In the hands: both or none, one with its own action, first on a right click. An item's clips
    // go in `animations/<item>.animation.json`, `models/player_template.geo.json` is the skeleton
    // to animate on in Blockbench. `mainArm` and `offArm` in a clip are the two arms.
    // static GREATSWORD: Item = Item::named("greatsword");
    // static SCROLL: Item = Item::named("scroll");
    // static WAND: Item = Item::named("wand");
    // r.item(&GREATSWORD).two_handed();
    // r.item(&SCROLL).one_handed_action("hold_one");
    // r.item(&WAND).use_priority(10);
    //
    // A mob's goal that plays its own run instead of walking:
    // static SCARED: EntityKind = EntityKind::named("scared");
    // r.entity(&SCARED).goals(ai::Goals::new().then(ai::with_action_override("walk", "flee", ai::wander(4.0, 12.0))));
    //
    // Any item as a model, in the hand, on the ground and in slots: models/apple.geo.json and
    // textures/apple.png. All food at once, what has no file stays as it was:
    // r.set_model("base:apple", "apple");
    // r.set_model("#food", "food/{name}");
    //
    // A tail on who wears it, drawn in the player's pixels; swinging by itself, and wagged by code:
    // static TAIL: Item = Item::named("tail");
    // r.item(&TAIL).wearable_model(&EquipmentSlot::named("base:chest"), "tail", "body");
    // r.bone_motion("tail", "tail2", BoneMotion::SPRINGY);
    // r.client_entity_frame(&PLAYER, |c, e| {
    //     c.bone_turn(e.id(), "tail1", [0.0, (e.age() * 6.0).sin() * 20.0, 0.0]);
    // });
    //
    // A stack with data of its own, what a custom workbench might make:
    // static GRADE: Component<u32> = Component::named("grade");
    // static CHISEL: Item = Item::named("chisel");
    // r.stack_component(&GRADE, 0);
    // fn made(grade: u32) -> ItemStack { CHISEL.stack(1).with(&GRADE, grade).with_wear(5) }
    //
    // Sparks around a mob while it lives, the engine's steps and idle noises kept (`.own_effects()`
    // on the kind would turn those off):
    // static WISP: EntityKind = EntityKind::named("wisp");
    // static SPARK: Particle = Particle::named("spark");
    // r.client_entity_frame(&WISP, |c, e| {
    //     if e.dying().is_none() {
    //         c.spawn_particles(&SPARK, e.pos());
    //     }
    // });
}
