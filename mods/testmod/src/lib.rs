//! Proves the engine is moddable without touching the core. Everything here uses only
//! `weft_sdk` and the base game's API crate `base_api` (types, no game code):
//! - a generator, a cable and an electric furnace talking through the engine's energy contract,
//! - a block with its own screen (energy bar bound to synced menu data),
//! - reads the base game's data (fuel values, smelting recipes) through `base_api`, checked when
//!   it compiles and again by the engine when the mods load,
//! - leaves the base game's own things as they are: what it shows off on base's systems it does
//!   on things of its own,
//! - replaces a block the way another mod would (its own torch, burning blue),
//! - adds its own dimensions, one of them a planet built only from terrain presets and edited
//!   as from outside (a biome, a rock needle, boulders, obelisks, ores with variants),
//! - adds a liquid (oil) whose bucket is the base game's bucket filled, without bucket code,
//! - makes its apple heal: eaten the base game's way, healing by its own component and the
//!   engine's `item_used` event, and lets its steed drop tin through a loot modifier,
//! - shows off particles, shaders, text effects and glow masks (`looks`).

mod bodies;
mod camera;
mod looks;
mod mace;
mod riding;

use weft_sdk::terrain::*;
use weft_sdk::*;

declare_mod!(deps: ["base"], init: init);

static GENERATOR: Block = Block::named("generator");
static HUM: Sound = Sound::named("machine.hum");
static CABLE: Block = Block::named("cable");
static PIPE: Block = Block::named("pipe");
static AND_GATE: Block = Block::named("and_gate");
/// Its shape is `models/block/pedestal.json`, a Blockbench Java block model.
static PEDESTAL: Block = Block::named("pedestal");
static DYNAMITE: Block = Block::named("dynamite");
static BOOM: Sound = Sound::named("boom");
/// How far testmod's dynamite blows (a server setting).
static DYNAMITE_RADIUS: Config<i32> = Config::named("dynamite_radius");
static ELECTRIC_FURNACE: Block = Block::named("electric_furnace");
static BATTERY: Block = Block::named("battery");
static TANK: Block = Block::named("tank");
static RED_SAND: Block = Block::named("red_sand");
static VENT: Block = Block::named("vent");
static REINFORCED_STONE: Block = Block::named("reinforced_stone");
static STEEL_PICKAXE: Item = Item::named("steel_pickaxe");
static OIL: Block = Block::named("oil");
static OIL_BUCKET: Item = Item::named("oil_bucket");
static MACHINES: SoundCategory = SoundCategory::named("machines");
static ENERGY_GIZMOS: GizmoGroup = GizmoGroup::named("energy");
// The base game's things, by name: no code dependency on it.
static BUCKET: Item = Item::named("base:bucket");
use base_api::tabs::{BUILDING as BASE_BUILDING, TOOLS as BASE_TOOLS};
static TORCH: Block = Block::named("torch");
static GENERATOR_MENU: Menu = Menu::named("generator");
static FURNACE_MENU: Menu = Menu::named("electric_furnace");
static BATTERY_MENU: Menu = Menu::named("battery");
static TANK_MENU: Menu = Menu::named("tank");
use base_api::SMELTING as SMELTING_RECIPES;
static VOID: Dimension = Dimension::named("void");
pub static MARS: Dimension = Dimension::named("mars");
static MARS_SUN: SkyObject = SkyObject::named("mars_sun");
static PHOBOS: SkyObject = SkyObject::named("phobos");
static DEIMOS: SkyObject = SkyObject::named("deimos");
static DUST: Biome = Biome::named("dust");
static SCORCHED: Biome = Biome::named("scorched");
static CRYSTAL_CAVES: Biome = Biome::named("crystal_caves");
/// An apple of its own, eaten the base game's way, with a model of its own and healing.
pub static APPLE: Item = Item::named("apple");
static REGOLITH: Block = Block::named("regolith");
static BASALT: Block = Block::named("basalt");
static TIN_ORE: Block = Block::named("tin_ore");
static BASALT_TIN_ORE: Block = Block::named("basalt_tin_ore");
static TIN_INGOT: Item = Item::named("tin_ingot");
static ANOTHER_STICK: Item = Item::named("another_stick");
static POUCH: Item = Item::named("pouch");
static POUCH_MENU: Menu = Menu::named("pouch");
static NOTES: TextPlace = TextPlace::named("notes");

value! {
    /// A generator's fire.
    #[derive(Default)]
    struct Burner { burn: u32, burn_max: u32 }
}
value! {
    /// An electric furnace's work on the item in it.
    #[derive(Default)]
    struct Work { progress: u32, progress_max: u32 }
}

static BURNER: Component<Burner> = Component::named("testmod:burner");
static WORK: Component<Work> = Component::named("testmod:work");
/// The common fuel value; the base game sets it, this mod only reads it.
use base_api::FUEL;
static FOOD_HEAL: Component<f32> = Component::named("testmod:food_heal");

const PER_TICK: i64 = 40;
const FURNACE_USE: i64 = 20;

fn init(r: &mut Registry) {
    looks::register(r);
    mace::register(r);
    riding::register(r);
    camera::register(r);
    bodies::register(r);
    debug_panel(r);
    about_screen(r);
    morph(r);
    dynamite(r);
    r.block(&CABLE)
        .model_box([5.0, 5.0, 5.0], [11.0, 11.0, 11.0])
        .tag("conductor")
        .tag("cables")
        .hardness(0.5);
    // A gate of its own faces: both sides, not the rear.
    r.block(&AND_GATE)
        .texture("machine_top")
        .model_box([0.0, 0.0, 0.0], [16.0, 2.0, 16.0])
        .signal_gate(Gate::new().inputs(&[Side::Left, Side::Right]), |g| {
            if g.side(Side::Left) > 0 && g.side(Side::Right) > 0 { 15 } else { 0 }
        });
    // Joins whatever pipe is next to it, any mod's, by the shared tag.
    r.block(&PIPE)
        .tag("pipes")
        .joins(Facings::ALL, "#pipes")
        .states(|p, s| {
            let mut s = s
                .texture("machine_top")
                .model_box([5.0, 5.0, 5.0], [11.0, 11.0, 11.0]);
            for d in Dir::ALL.into_iter().filter(|d| p.is(d.name(), "true")) {
                let o = d.offset().to_array().map(|v| v as f32);
                let lo = [0, 1, 2].map(|a| {
                    if o[a] < 0.0 {
                        0.0
                    } else if o[a] > 0.0 {
                        11.0
                    } else {
                        5.0
                    }
                });
                let hi = [0, 1, 2].map(|a| {
                    if o[a] < 0.0 {
                        5.0
                    } else if o[a] > 0.0 {
                        16.0
                    } else {
                        11.0
                    }
                });
                s = s.model_box(lo, hi);
            }
            s
        })
        .hardness(0.5)
        .tab(&BASE_TOOLS);
    // Neutral as long as you keep your distance.
    static SENTRY: EntityKind = EntityKind::named("sentry");
    r.entity(&SENTRY)
        .size(0.6, 1.95)
        .max_health(20.0)
        .walk_speed(2.6)
        .hostile()
        .texture("base:zombie")
        .model(humanoid(true))
        .attack_damage(1.0)
        .goals(
            ai::Goals::new()
                .aggressive(5.0)
                .then(ai::melee_within(1.3))
                .then(ai::chase_target(20.0, 1.0)),
        );
    // One block in 2x2x2 cells, its model and boxes spanning them from its own.
    static BIG: Block = Block::named("big");
    let others = [[1, 0, 0], [0, 1, 0], [1, 1, 0], [0, 0, 1], [1, 0, 1], [0, 1, 1], [1, 1, 1]];
    r.block(&BIG)
        .texture("base:mossy_cobblestone")
        .model_box([0.0; 3], [32.0; 3])
        .cells(&others);
    // Edited by testkit (`max_stack` 8 there): an edit wins whichever mod loads first.
    static EDITED: Item = Item::named("edited");
    r.item(&EDITED).max_stack(16).attack_damage(3.0);
    // Faces the file leaves to the block (`#top`, `#side`) show these.
    r.block(&PEDESTAL)
        .texture("generator")
        .ends("machine_top")
        // Without its crystal, a state on a model of its own with a texture of its own.
        .prop("crystal", ["true", "false"])
        .states(|p, s| {
            if p.is("crystal", "false") {
                s.model_file("pedestal_empty")
                    .model_texture("socket", "base:gold_block")
            } else {
                s
            }
        })
        .hardness(1.0)
        .tab(&BASE_BUILDING);

    // A torch of its own, shaped like base's; then the same block replaced, as another mod would
    // replace it: the handle, item and recipes give the replacement, which keeps the states
    // (floor and walls) and placement and has only its texture and light of its own.
    r.block(&TORCH)
        .sound_group("base:block.wood")
        .ambient_particles(&["core:flame", "core:smoke"])
        .facing(shapes::TORCH_FACINGS, Toward::Face)
        .states(|p, s| shapes::torch_state(p.facing(), s, "base:torch"))
        .item_texture("base:torch")
        .not_solid()
        .light_rgb(15, 13, 10)
        .hardness(0.0)
        .tab(&BASE_BUILDING);
    r.edit(&TORCH)
        .retexture("base:torch", "soul_torch")
        .light_rgb(3, 9, 35)
        .hardness(0.0);

    // Machines: a block whose block entity has the state and energy components and the slots,
    // opening a menu.
    // The components sync to open menus; screens bind to them.
    // Each kind ticks as a system over its own state: every machine of a region in one call.
    r.entity_component(&BURNER, Burner::default(), component_flags::SAVE | component_flags::SYNC_MENU);
    r.entity_component(&WORK, Work::default(), component_flags::SAVE | component_flags::SYNC_MENU);
    r.system(&BURNER, tick_generators);
    r.system(&WORK, tick_furnaces);
    // Machines' sounds have their own category (and volume setting `testmod:volume_machines`).
    r.sound_category(&MACHINES, true);
    r.sound(&HUM)
        .files(&["test/thump"])
        .category(&MACHINES)
        .pitch(0.45, 0.5);
    r.block(&GENERATOR)
        .texture("generator")
        .ends("machine_top")
        .hardness(3.0)
        .mined_by("pickaxe", 0)
        .entity(|e| {
            e.attach(&BURNER)
                .attach_with(&ENERGY, Energy { stored: 0, capacity: 20_000, max_in: 0, max_out: 200 })
                .inventory(1)
        })
        .opens(&GENERATOR_MENU);
    r.menu(&GENERATOR_MENU)
        // Only fuel goes in (the base game's `fuel` component), and stays in until burnt.
        .group(
            "fuel",
            Slots::single()
                .accepts("#fuel")
                .input()
                .background("base:slot_fuel")
                .tooltip("block.testmod.fuel_slot")
                .holder(),
        )
        .player_inventory()
        .screen(|ui| {
            machine_screen(ui, |ui| {
                ui.column(S::column().gap(2.0).center(), |ui| {
                    ui.group("fuel");
                    ui.progress_bind_dir("flame", S::default().size(14.0, 14.0), abi::ui_dir::UP, |c| {
                        c.menu_component(&BURNER)
                            .map_or(0.0, |m| ratio(m.burn, m.burn_max))
                    });
                });
            })
        });
    // Eight buckets of one liquid, in and out by bucket.
    r.block(&TANK)
        .texture("machine_top")
        .hardness(3.0)
        .entity(|e| e.inventory(1))
        .opens(&TANK_MENU);
    r.menu(&TANK_MENU)
        .group("liquid", Slots::single().liquid(8000).holder())
        .player_inventory()
        .screen(|ui| machine_screen(ui, |ui| ui.group("liquid")));
    r.block(&ELECTRIC_FURNACE)
        .texture("electric_furnace")
        .ends("machine_top")
        .hardness(3.0)
        .mined_by("pickaxe", 0)
        .entity(|e| {
            e.attach(&WORK)
                .attach_with(&ENERGY, Energy { stored: 0, capacity: 40_000, max_in: 400, max_out: 0 })
                .inventory(2)
        })
        .opens(&FURNACE_MENU);
    r.menu(&FURNACE_MENU)
        // At most 16 to smelt at a time.
        .group("input", Slots::single().limit(16).holder())
        .group("output", Slots::single().output().holder())
        .recipes(&SMELTING_RECIPES)
        .player_inventory()
        .screen(|ui| {
            machine_screen(ui, |ui| {
                ui.group("input");
                ui.recipe_arrow_bind(|c| {
                    c.menu_component(&WORK)
                        .map_or(0.0, |m| ratio(m.progress, m.progress_max))
                });
                ui.group("output");
            })
        });

    // No tick of its own: only the grid fills and drains it, so its bar shows that components
    // reach open menus whoever changed them.
    r.block(&BATTERY)
        .texture("battery")
        .ends("machine_top")
        .hardness(3.0)
        .mined_by("pickaxe", 0)
        .entity(|e| e.attach_with(&ENERGY, Energy { stored: 0, capacity: 100_000, max_in: 400, max_out: 400 }))
        .opens(&BATTERY_MENU);
    r.menu(&BATTERY_MENU)
        .player_inventory()
        .screen(|ui| machine_screen(ui, |_| {}));

    // The base game's kinds, by name: no code dependency on it.
    use base_api::CRAFTING;
    recipe_showcase(r);
    r.shaped(&CRAFTING, "generator", 1, &["iii", "ifi", "iii"], &[('i', "#ingots/iron"), ('f', "#furnaces")]);
    r.shaped(&CRAFTING, "electric_furnace", 1, &["iii", "iFi", "ici"], &[('i', "#ingots/iron"), ('F', "#furnaces"), ('c', "#cables")]);
    r.shaped(&CRAFTING, "cable", 8, &["iii"], &[('i', "#ingots/iron")]);
    // What a recipe does to its ingredients besides using them up: a hammer stays and wears a
    // point a run (a shift-click crushes as much as it lasts for at once), the water bucket
    // leaves its bucket in its slot (the lava one does by its own `remainder`).
    static HAMMER: Item = Item::named("hammer");
    r.item(&HAMMER)
        .texture("base:iron_pickaxe")
        .tint(0xC08060)
        .durability(16)
        .max_stack(1)
        .handheld()
        .tab(&BASE_TOOLS);
    r.shapeless(&CRAFTING, "base:gravel", 1, &["hammer", "base:cobblestone"], 0)
        .wear(0, 1);
    r.shapeless(&CRAFTING, "base:obsidian", 1, &["base:lava_bucket", "base:water_bucket"], 0)
        .leaves(1, "base:bucket");

    r.block(&REGOLITH)
        .hardness(0.5)
        .mined_by("shovel", tool_tier::HAND);
    // Only the common `sand` tag: cactus and sugar cane accept it without knowing it.
    r.block(&RED_SAND)
        .hardness(0.5)
        .mined_by("shovel", tool_tier::HAND)
        .tag("sand");
    r.block(&BASALT)
        .hardness(1.5)
        .mined_by("pickaxe", tool_tier::HAND)
        .tag("stone_ore_replaceables");
    // Only tags: the base game's `#ores/*` smelting recipe and tools pick these up.
    r.block(&TIN_ORE)
        .hardness(3.0)
        .mined_by("pickaxe", 0)
        .tag("ores/tin");
    r.block(&BASALT_TIN_ORE)
        .texture("tin_ore")
        .hardness(3.5)
        .mined_by("pickaxe", 0)
        .tag("ores/tin");
    r.item(&TIN_INGOT).tag("ingots/tin");

    r.item(&ANOTHER_STICK).tag("stick");

    // A place of its own for text, in the top right corner; players move it like any element.
    r.text_place(&NOTES)
        .hud(hud_anchor::TOP_RIGHT, 5)
        .stacks()
        .sample("testmod.notes.sample");
    // Each pouch keeps its own nine stacks.
    r.item(&POUCH)
        .inventory(9)
        .max_stack(1)
        .tab(&BASE_TOOLS)
        .on_use(|u| {
            u.world().open_menu(u.entity(), &POUCH_MENU, u.holder());
            true
        });
    r.menu(&POUCH_MENU)
        .group("items", Slots::row(9).holder())
        .player_inventory()
        .screen(|ui| {
            ui.panel("panel", S::column().pad(7.0).gap(4.0), |ui| {
                ui.title();
                ui.group("items");
                ui.player_inventory(true);
            });
        });

    // A tier between vanilla's iron (200) and diamond (300): mines everything iron does plus
    // reinforced stone, which iron cannot.
    r.item(&STEEL_PICKAXE)
        .tool("pickaxe", 250)
        .mining_speed(7.0)
        .durability(800)
        .attack_damage(4.0)
        .set(&WEAR_PER_HIT, 2)
        .tab(&BASE_TOOLS);
    // May face up, north or east only: three states, not six.
    r.block(&VENT)
        .facing(Facings::only(&[Dir::Up, Dir::North, Dir::East]), Toward::Player)
        .states(|_, s| s.texture("vent_side").front("vent_front"))
        .hardness(2.0)
        .mined_by("pickaxe", tool_tier::HAND)
        .tab(&BASE_BUILDING);
    r.block(&REINFORCED_STONE)
        .tag("has_shapes")
        .shader(&Shader::named("stripes"))
        .hardness(4.0)
        .mined_by("pickaxe", 250)
        .tab(&BASE_BUILDING);
    r.biome(&DUST)
        .surface("regolith", "regolith", 3)
        .tint(&tints::GRASS, 0xb5764a)
        .tint(&tints::FOLIAGE, 0x9a6040)
        .tint(&tints::WATER, 0x8a5a40)
        .tag("dry");
    // Deep under Mars, where its glow crystals grow.
    r.biome(&CRYSTAL_CAVES).tag("cave").tag("dark");
    r.item(&APPLE)
        .set(&base_api::FOOD, base_api::Food { hunger: 4, saturation: 2.4 })
        .use_ticks(32)
        .on_use(base_api::eat)
        .set(&FOOD_HEAL, 2.0)
        .tab(&BASE_TOOLS);
    // The hottest, driest part of Mars.
    r.biome(&SCORCHED)
        .temperature(0.4..)
        .humidity(..-0.1)
        .surface("regolith", "regolith", 2)
        .tint(&tints::GRASS, 0xbf8a50)
        .tint(&tints::FOLIAGE, 0xae7a3a)
        .tag("dry")
        .tag("hot");
    // Mars' tin: where a vein meets basalt it goes on as basalt tin ore. Half of what would touch
    // a cave is left out.
    r.ore(&TIN_ORE)
        .dimension(&MARS)
        .y(-64, 90)
        .per_cube(5)
        .vein_size(8)
        .replaces("#stone_ore_replaceables")
        .replaces_with("basalt", &BASALT_TIN_ORE)
        .exposed(0.5);
    r.terrain(&MARS)
        .stone("basalt")
        .base_height(60.0)
        .landform(preset::hills(8.0, 220.0))
        .landform(preset::plateaus(24.0, 320.0, 6.0).mask(Expr::noise(900.0).ramp(0.1, 0.4)))
        .landform(preset::canyons(26.0, 9.0).mask(Expr::noise(1200.0).ramp(-0.2, 0.1)))
        .landform(preset::craters(32.0, 16.0, 150.0))
        .landform(preset::dunes(3.0, 22.0).mask(Expr::noise(500.0).ramp(0.15, 0.45)))
        // Base's forest in the cold parts of Mars (any humidity, unlike at home): it brings its
        // grass, and the engine grows its trees here like in base's overworld.
        .rule(
            BiomeRule::new("base:forest")
                .temperature(..-0.3)
                .humidity(..),
        )
        .rule(BiomeRule::new("dust"))
        .cave_biome("crystal_caves", 24, 96.0, 0.0);
    // Mars edited as from outside: a biome tried before its dust, a rock needle from a feature
    // of this mod's own, and boulders on the new biome.
    r.edit(MARS.terrain())
        .rule_before("testmod:dust", BiomeRule::new("scorched"))
        .landform(
            // 90 blocks up within 3 of the spot: a step where the distance passes 3.
            Landform::new(
                ((Expr::x() - Expr::blocks(NEEDLE[0] as f64)).abs())
                    .max((Expr::z() - Expr::blocks(NEEDLE[1] as f64)).abs())
                    .ramp_blocks(3.0, 3.0)
                    .not()
                    * Expr::blocks(90.0),
            )
            .named("needle"),
        );
    r.decoration(boulders).dimension(&MARS);
    r.dimension(&MARS)
        .spawn_point(|seed| [8, MARS.column(seed, 8, 8).height + 1, 8].into())
        .height_cubes(-4, 12)
        .sky_above(11)
        .gravity(12.0)
        .fixed_time(5000)
        .sky_color(0xd8a080, 0xe0b090);
    // Two moons of their own, unrelated to the fixed time of day, and a sun that stands still.
    r.sky_body(&MARS, &MARS_SUN)
        .texture("base:sun")
        .size(18.0)
        .orbit_still()
        .phase(0.2);
    r.sky_body(&MARS, &PHOBOS)
        .texture("phobos")
        .size(10.0)
        .orbit(600.0)
        .tilt(30.0);
    r.sky_body(&MARS, &DEIMOS)
        .texture("phobos")
        .size(5.0)
        .orbit(-2400.0)
        .phase(0.6)
        .tilt(-15.0)
        .color(0xFFD0B0);

    // A liquid of its own, flowing by the engine's rules; its bucket fills from the base game's.
    r.liquid(&OIL)
        .block(|b| b.light_opacity(4).drag(0.4).tag("oil"))
        .flow_delay(15)
        .drop_off(2)
        .slope_find(3)
        .bucket(&OIL_BUCKET, &BUCKET, &BASE_TOOLS);

    // Healing food without touching the base game's eating code: any used item that heals (its
    // apple).
    r.on_event(&actions::USE_ITEM, |b| {
        let w = b.world();
        for e in b.iter() {
            if let Some(heal) = FOOD_HEAL.on_stack(&e.stack) {
                w.heal(e.entity, heal);
            }
        }
    });
    r.edit(crate::riding::STEED.loot())
        .pool(Pool::new().when(Cond::chance(0.05)).item("tin_ingot"));

    r.command("tagbench", "[millions:int]", tag_bench);
    // The engine's /gizmo suggests this mod's group too.
    r.gizmo_group(&ENERGY_GIZMOS);

    r.dimension(&VOID)
        .generator(island)
        .spawn_point(|_| [0, 64, 0].into())
        .height_cubes(2, 6)
        .sky_above(5)
        .gravity(14.0)
        .fixed_time(18000)
        .ambient_light(5)
        .sky_color(0x2a1040, 0x40206a);
}

/// `/morph base:pig 0.9 0.9`: the player looks like another kind and gets a box of that size
/// (eyes at 85%), still the same player; `/morph` alone turns back.
fn morph(r: &mut Registry) {
    r.command("morph", "[kind:entity] [width:float] [height:float]", |cmd| {
        let (w, me) = (cmd.world(), cmd.player());
        match cmd.text("kind").map(str::to_string) {
            Some(kind) => {
                // The size of what it turns into, unless told otherwise.
                let own = w
                    .kind_size(&kind)
                    .unwrap_or(EntitySize { width: 0.9, height: 0.9, eye: 0.765 });
                let (width, height) =
                    (cmd.float("width").map_or(own.width, |v| v as f32), cmd.float("height").map_or(own.height, |v| v as f32));
                let eye = if cmd.float("height").is_some() { height * 0.85 } else { own.eye };
                w.set_appearance(me, Some(&kind));
                w.set_size(me, Some(EntitySize { width, height, eye }));
                cmd.reply("command.testmod.morph", &[Arg::Text(&kind), Arg::Float(width as f64, 2), Arg::Float(height as f64, 2)]);
            }
            None => {
                w.set_appearance(me, None);
                w.set_size(me, None);
                cmd.reply("command.testmod.unmorph", &[]);
            }
        }
    });
}

static ABOUT: Menu = Menu::named("about");
static DEBUG_PANEL: HudElement = HudElement::named("debug");

/// A screen of the mod's own (`/about` here, `--screen testmod:about`).
fn about_screen(r: &mut Registry) {
    // Everything the engine's own screens use: a large title, a text field, a slider, an item,
    // a tooltip with arguments, and pausing like the engine's pause menu.
    static NAME: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    static VOLUME: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    r.menu(&ABOUT)
        .screen(|ui| {
            ui.panel("testmod_panel", S::column().pad(8.0).gap(4.0).min_w(220.0), |ui| {
                ui.text_styled(S::column().class("title").scale(2.0).no_wrap(), "testmod.about.title", &[]);
                ui.text("testmod.about.text");
                ui.panel(
                    "well",
                    S::column()
                        .pad(4.0)
                        .tooltip_args("testmod.about.tip", &[Arg::Int(3)]),
                    |ui| ui.text("testmod.about.hover"),
                );
                ui.input("", 16, S::column().class("about_name").w(200.0), |_, text| *NAME.lock().unwrap() = text.to_string());
                ui.text_bind(|_, out| out.set("testmod.about.name", &[Arg::Text(&NAME.lock().unwrap())]));
                ui.slider(
                    S::column().w(200.0).h(20.0),
                    |_| VOLUME.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.0,
                    |_, v| VOLUME.store((v * 100.0).round() as u32, std::sync::atomic::Ordering::Relaxed),
                );
                ui.text_bind(|_, out| out.set("testmod.about.fov", &[Arg::Float(weft_sdk::settings::FOV.get() as f64, 0)]));
                ui.text_bind_styled(S::column().mono(), |_, out| {
                    out.set("testmod.about.volume", &[Arg::Int(VOLUME.load(std::sync::atomic::Ordering::Relaxed) as i64)])
                });
                ui.row(S::row().gap(4.0), |ui| {
                    ui.item(Item::lookup("base:coal").unwrap_or(0), 3, S::default());
                    ui.button("testmod.about.closed", S::column().disabled(true), |_| {});
                });
            });
        })
        .pauses();
    // A button of its own in the main menu, next to the engine's and other mods'.
    r.screen_part(&screens::TITLE, "buttons", 0, |ui| {
        ui.button("testmod.about.button", S::default(), |c| {
            c.client().menu_open(&ABOUT);
        });
    });
    r.client_command("about", "", |c| {
        c.client().menu_open(&ABOUT);
    });
}

/// An F3 panel of the mod's own (F3+6 toggles it).
fn debug_panel(r: &mut Registry) {
    r.hud_element(&DEBUG_PANEL, hud_anchor::DEBUG_LEFT, 0, |ui| {
        ui.text_bind(|c, out| {
            let Some(me) = c.player_state() else { return };
            let speed = (me.vel.x * me.vel.x + me.vel.z * me.vel.z).sqrt() * 20.0;
            out.set("testmod.debug.speed", &[Arg::Float(speed, 2), Arg::Float(me.fall_distance as f64, 1)]);
        });
    });
}

fn machine_screen(ui: &mut Ui, slots: impl FnOnce(&mut Ui)) {
    // Drawn by the mod's GUI shader (`shader` in `gui/testmod_panel.gui`).
    ui.panel("testmod_panel", S::column().pad(7.0).gap(4.0), |ui| {
        ui.title();
        ui.row(S::row().gap(8.0), |ui| {
            ui.column(S::column().gap(2.0), |ui| {
                ui.text_bind(|c, out| {
                    let e = c.menu_component(&ENERGY).unwrap_or_default();
                    out.set("testmod.energy", &[Arg::Int(e.stored), Arg::Int(e.capacity)]);
                });
                ui.progress_bind("energy", S::default().size(80.0, 8.0), |c| {
                    c.menu_component(&ENERGY)
                        .map_or(0.0, |e| ratio(e.stored as u32, e.capacity as u32))
                });
            });
            ui.spacer();
            ui.row(S::row().gap(6.0), slots);
        });
        ui.player_inventory(true);
    });
}

/// `/gizmo testmod:energy`: stored energy above every machine, the mod-side use of gizmos.
fn energy_gizmos(w: &World, ids: &[u64]) {
    if w.game_time() % 10 != 0 || !w.gizmo_shown(&ENERGY_GIZMOS) {
        return;
    }
    for &id in ids {
        if let (Some(st), Some(e)) = (w.entity(id), w.component(id, &ENERGY)) {
            let args = [Arg::Int(e.stored as i64), Arg::Int(e.capacity as i64)];
            w.draw_text(Some(&ENERGY_GIZMOS), [st.pos.x, st.pos.y + 1.3, st.pos.z], "testmod.energy_label", &args, 0x60FF60FF, 1.0)
                .ticks(11);
        }
    }
}

/// Dynamite: lit by a right click, five seconds of fuse throwing sparks, then a crater of
/// `dynamite_radius` blocks through the engine's area edit: one call however big, the part not
/// loaded gets it when it loads.
fn dynamite(r: &mut Registry) {
    r.config(&DYNAMITE_RADIUS).int(500, 4, 2000).server();
    r.sound(&BOOM).files(&["test/thump"]).pitch(0.2, 0.3);
    r.block(&DYNAMITE)
        .prop("fuse", 0..=10)
        .states(|p, s| {
            let lit = !p.is("fuse", "0");
            let side = "dynamite_side";
            let s = s
                .texture(side)
                .bottom("dynamite_top")
                .top(if lit { "dynamite_top_lit" } else { "dynamite_top" });
            if lit { s.light_rgb(12, 7, 2) } else { s }
        })
        .hardness(0.0)
        .tab(&BASE_TOOLS)
        .on_use(|u| {
            let (w, pos) = (u.world(), u.pos());
            if w.prop(pos, "fuse") == Some("0") {
                w.prop_set(pos, "fuse", "10");
                w.schedule_tick(pos, 10);
            }
            true
        })
        .on_scheduled(|e| {
            let (w, pos) = (e.world(), e.pos());
            let Some(left) = w.prop_int(pos, "fuse").filter(|&f| f > 0) else {
                return;
            };
            let center = block_center(pos);
            if left > 1 {
                w.prop_set(pos, "fuse", &(left - 1).to_string());
                w.spawn_particles(&looks::SPARK, [center.x, center.y + 0.6, center.z])
                    .count(6)
                    .spread(0.2)
                    .speed(0.5);
                w.schedule_tick(pos, 10);
            } else {
                w.play_sound(&BOOM, center, 4.0);
                w.fill(Shape::ball(center, DYNAMITE_RADIUS.get() as f64), 0)
                    .keep("#unbreakable");
            }
        });
}

fn tick_generators(b: &mut SystemBatch<Burner>) {
    let w = b.world();
    energy_gizmos(&w, b.ids());
    for (id, m, ticks) in b.iter_mut() {
        let Some(e) = w.component(id, &ENERGY) else { continue };
        let room = e.capacity - e.stored;
        if ticks > 1 {
            // Catching up: it runs while there is room for what it makes, lighting fuel as it
            // goes (the network takes from it between parts of the catch-up).
            let mut want = (ticks as i64).min(room / PER_TICK).max(0) as u32;
            let mut ran = want.min(m.burn);
            m.burn -= ran;
            want -= ran;
            let slot = w.inventory(id).group("fuel");
            while want > 0 {
                let fuel = slot.get(0);
                let Some(t) = FUEL.on_stack_item(&fuel) else { break };
                (m.burn, m.burn_max) = (t, t);
                slot.set(0, consume_one(fuel));
                let r = want.min(m.burn);
                m.burn -= r;
                ran += r;
                want -= r;
            }
            if let Some(mut e) = w.component(id, &ENERGY) {
                e.stored = (e.stored + ran as i64 * PER_TICK).min(e.capacity);
                w.set_component(id, &ENERGY, e);
            }
            continue;
        }
        if m.burn == 0 && room >= PER_TICK {
            let slot = w.inventory(id).group("fuel");
            let fuel = slot.get(0);
            // Any mod's fuel works: the value is the common `fuel` component.
            if let Some(ticks) = FUEL.on_item(fuel.item).filter(|_| !fuel.is_empty()) {
                m.burn = ticks;
                m.burn_max = ticks;
                // A lava bucket leaves its bucket, like in the furnace.
                slot.set(0, consume_one(fuel));
            }
        }
        if m.burn > 0 {
            m.burn -= 1;
            if let Some(mut e) = w.component(id, &ENERGY) {
                e.stored = (e.stored + PER_TICK).min(e.capacity);
                w.set_component(id, &ENERGY, e);
            }
            // A low hum every two seconds, under the test mod's own volume setting.
            if w.game_time() % 40 == 0 && looks::GENERATOR_HUM.get() {
                if let Some(st) = w.entity(id) {
                    w.play_sound(&HUM, st.pos, 0.4);
                }
            }
        }
    }
}

fn tick_furnaces(b: &mut SystemBatch<Work>) {
    let w = b.world();
    energy_gizmos(&w, b.ids());
    for (id, m, ticks) in b.iter_mut() {
        let Some(e) = w.component(id, &ENERGY) else { continue };
        let stored = e.stored;
        let inv = w.inventory(id);
        let (input_slot, output_slot) = (inv.group("input"), inv.group("output"));
        let (input, output) = (input_slot.get(0), output_slot.get(0));
        let recipe = (!input.is_empty())
            .then(|| recipe_process(&SMELTING_RECIPES, input.item))
            .flatten();
        let fits = recipe.is_some_and(|(out, _)| output.room_for(out) > 0);
        if !fits || stored < FURNACE_USE {
            m.progress = 0;
            continue;
        }
        let (out, time) = recipe.unwrap();
        // Twice as fast as the base furnace, paid for in energy.
        m.progress_max = (time / 2).max(1);
        if ticks > 1 {
            // Catching up: as many steps as the energy, the input and the room allow.
            let work = (input.count.min(output.room_for(out)) * m.progress_max).saturating_sub(m.progress) as i64;
            let steps = (ticks as i64).min(stored / FURNACE_USE).min(work);
            let total = m.progress + steps as u32;
            let done = total / m.progress_max;
            m.progress = total % m.progress_max;
            if let Some(mut e) = w.component(id, &ENERGY) {
                e.stored -= steps * FURNACE_USE;
                w.set_component(id, &ENERGY, e);
            }
            if done > 0 {
                input_slot.set(0, input.shrink(done));
                output_slot.set(0, output.merged(out, done));
            }
            continue;
        }
        m.progress += 1;
        let done = m.progress >= m.progress_max;
        if done {
            m.progress = 0;
        }
        if let Some(mut e) = w.component(id, &ENERGY) {
            e.stored -= FURNACE_USE;
            w.set_component(id, &ENERGY, e);
        }
        if done {
            input_slot.set(0, input.shrink(1));
            output_slot.set(0, output.merged(out, 1));
        }
    }
}

/// Another mod's blocks, found by name the first time they are used.
static STONE: Block = Block::named("base:stone");
static GRASS: Block = Block::named("base:grass");
static DIRT: Block = Block::named("base:dirt");
static CHEST: Block = Block::named("base:chest");
/// Where the island's chest stands: a block entity's block placed by a generator.
pub const ISLAND_CHEST: [i32; 3] = [4, 64, 0];

/// A floating island of stone and grass in an empty void, with a chest on it.
static ALLOYING: RecipeKind = RecipeKind::named("alloying");
static INFUSING: RecipeKind = RecipeKind::named("infusing");
static ENERGY_PER_TICK: Component<u32> = Component::named("testmod:energy_per_tick");

/// Recipe kinds of this mod, only for the recipe viewer: one shown the usual way with an info line
/// from the recipe's values and a result that comes by chance, one laid out by hand with a fluid.
fn recipe_showcase(r: &mut Registry) {
    r.recipe_kind(&ALLOYING)
        .inputs(2, 1)
        .outputs(2, 1)
        .info_line("recipe.testmod.alloying.info", &["testmod:energy_per_tick", "time"]);
    r.shapeless(&ALLOYING, "tin_ingot", 2, &["#ingots/iron", "#gems/coal"], 100)
        .input_amounts(&[3, 1])
        .output_chance("base:diamond", 1, 0.05)
        .set(&ENERGY_PER_TICK, 32);
    r.recipe_kind(&INFUSING).layout(|ui, recipe| {
        ui.column(S::column().gap(2.0), |ui| {
            ui.row(S::row().gap(4.0).center(), |ui| {
                ui.recipe_input(0);
                ui.text("testmod.infusing.plus");
                ui.recipe_input(1);
                ui.recipe_arrow();
                ui.recipe_outputs();
            });
            ui.text_args("testmod.infusing.time", &[Arg::Int(recipe.time() as i64 / 20)]);
        });
    });
    r.shapeless(&INFUSING, "glow_crystal", 1, &["#gems/diamond", "#water"], 200)
        .input_amounts(&[1, 1000]);
}

/// Where the needle stands on Mars.
pub const NEEDLE: [i32; 2] = [1000, 1000];

/// Basalt boulders on Mars' scorched biome.
fn boulders(g: &mut Gen) {
    let Some(&cols) = g.columns() else { return };
    let BlockPos { x: ox, y: oy, z: oz } = g.origin();
    let (scorched, basalt) = (SCORCHED.id(), BASALT.id());
    for (i, c) in cols.iter().enumerate() {
        let y = c.height + 1 - oy;
        if c.biome != scorched || !(1..16).contains(&y) {
            continue;
        }
        let (x, y, z) = (i % 16, y as usize, i / 16);
        let h = noise::hash(g.seed() ^ 0xB0, (ox + x as i32) as i64, 0, (oz + z as i32) as i64);
        if h % 40 == 0 && g.get(x, y, z) == 0 && g.get(x, y - 1, z) != 0 {
            g.set(x, y, z, basalt);
        }
    }
}

fn island(g: &mut Gen) {
    let BlockPos { x: ox, y: oy, z: oz } = g.origin();
    let (stone, grass, dirt) = (STONE.id(), GRASS.id(), DIRT.id());
    for y in 0..16usize {
        for z in 0..16usize {
            for x in 0..16usize {
                let (wx, wy, wz) = ((ox + x as i32) as f32, oy + y as i32, (oz + z as i32) as f32);
                let r = (wx * wx + wz * wz).sqrt();
                let top = 63;
                let depth = (24.0 - r) * 0.8;
                if r < 24.0 && wy <= top && (top - wy) as f32 <= depth {
                    let s = if wy == top {
                        grass
                    } else if top - wy < 3 {
                        dirt
                    } else {
                        stone
                    };
                    g.set(x, y, z, s);
                }
            }
        }
    }
    let [cx, cy, cz] = ISLAND_CHEST;
    let (lx, ly, lz) = (cx - ox, cy - oy, cz - oz);
    if (0..16).contains(&lx) && (0..16).contains(&ly) && (0..16).contains(&lz) {
        g.set(lx as usize, ly as usize, lz as usize, CHEST.id());
    }
}

/// `/tagbench`: what a tag check costs a mod compared with reading a field.
fn tag_bench(cmd: &mut ServerCommand) {
    static PLANT_SOIL: Tag = Tag::named("plant_soil");
    let n = cmd.int("millions").unwrap_or(10).clamp(1, 1000) as u32 * 1_000_000;
    let states: Vec<u32> = (0..256).map(|i| i * 7 % 120).collect();
    let flags: Vec<u32> = (0..256).map(|i| i * 31).collect();
    let time = |f: &dyn Fn(u32) -> u32| {
        let t = std::time::Instant::now();
        let mut acc = 0u32;
        for i in 0..n {
            acc = acc.wrapping_add(f(states[(i & 255) as usize]));
        }
        std::hint::black_box(acc);
        t.elapsed().as_secs_f64() * 1e9 / n as f64
    };
    let field = time(&|s| flags[s as usize & 255]);
    let tag = time(&|s| PLANT_SOIL.on_block(s) as u32);
    let host_call = time(&|s| state_flags(s));
    let by_name = time(&|s| block_has_tag(s, "plant_soil") as u32);
    cmd.reply("command.testmod.tagbench", &[Arg::Float(field, 2), Arg::Float(tag, 2), Arg::Float(host_call, 2), Arg::Float(by_name, 2)]);
}
