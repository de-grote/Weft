//! Things that only change how the game looks, all as plain mod code and assets:
//! - a glow crystal whose veins glow by a `_e` mask (LabPBR-style emission, no light source),
//!   and a glowing birch, leaves, moss and flower with masks of their own,
//! - sparks: a particle kind with its own shader, sent from the server for everyone nearby,
//! - wisps: floating entities drawn by an entity shader, their core glowing by a mask,
//! - a rune whose tooltip uses text effects (engine and own) and conditions (own and engine),
//!   and which shows the area it would affect as a see-through box while held,
//! - a screen effect toggled with a key, and machine screens drawn by a GUI shader (named in a `.gui`),
//! - an idol from a Blockbench model file (bones, a mesh pair of wings) that idles by keyframes
//!   and bows when used, and an orb whose model is built in code (a sphere mesh and a ring of
//!   stones on a child bone) but animated from a file,
//! - the player waves when using the rune: keyframes for the player's model, seen from anywhere,
//! - items drawn from layers: a ruby that is base's diamond tinted red (one picture, many
//!   colors), and a wand put together from a stick and a tinted diamond on top of it.

use weft_sdk::*;

pub static SPARK: Particle = Particle::named("spark");
pub static DROP: Particle = Particle::named("drop");
pub static RIPPLE: Particle = Particle::named("ripple");
static WISP: EntityKind = EntityKind::named("wisp");
static IDOL: EntityKind = EntityKind::named("idol");
static ORB: EntityKind = EntityKind::named("orb");
static MOSAIC: Block = Block::named("mosaic");
static GLOW_MOSS: Block = Block::named("glow_moss");
static GLOW_LOG: Block = Block::named("glow_log");
static GLOW_LEAVES: Block = Block::named("glow_leaves");
static GLOW_FLOWER: Block = Block::named("glow_flower");
/// A thin layer that can lie over a plant: a cell holds them both.
static SNOW_LAYER: Block = Block::named("snow_layer");
static GLOW_CRYSTAL: Block = Block::named("glow_crystal");
static GLOW: TintKind = TintKind::named("glow");
static PLAINS: Biome = Biome::named("base:plains");
static SPIRITS: EntityCategory = EntityCategory::named("spirits");
static HURT: Condition = Condition::named("hurt");
static DREAM_KEY: Key = Key::named("dream");
static DREAM_EFFECT: Shader = Shader::named("dream");
static DREAM_COLD: Shader = Shader::named("dream_cold");
static RUNE: Item = Item::named("rune");
/// Drawn from `models/item/totem.json`: a stone base and a crystal, no icon of its own.
static TOTEM: Item = Item::named("totem");
/// Idols currently bowing.
static BOWED: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());
/// Settings players change in Settings > Mod Settings.
/// The generator's low hum (in `lib.rs`).
pub static GENERATOR_HUM: Config<bool> = Config::named("generator_hum");
static WISP_SPARKS: Config<i32> = Config::named("wisp_sparks");
static DREAM_STRENGTH: Config<f32> = Config::named("dream_strength");
static DREAM_STYLE: Config<u32> = Config::named("dream_style");

static DISPLAY_PHASE: Component<u32> = Component::named("testmod:display_phase");
static DISPLAY_PARTS: Storage = Storage::named("testmod:display_parts");
static DISPLAY_TOKEN: Item = Item::named("display_token");
static DISPLAY_CARRIER: EntityKind = EntityKind::named("display_carrier");
static DISPLAY_LOOK_CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn register(r: &mut Registry) {
    environment_controls(r);
    r.stack_component(&DISPLAY_PHASE, abi::stack_flags::VISUAL);
    r.storage(&DISPLAY_PARTS, 1).stack(abi::stack_flags::VISUAL);
    r.item(&DISPLAY_TOKEN)
        .wearable(&base_api::HEAD, 0.5, "base:armor/iron_1")
        .condition(|s, look| {
            DISPLAY_LOOK_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            match DISPLAY_PHASE.on_stack(s).unwrap_or(0) {
                0 => {
                    look.model("testmod:katana")
                        .worn_model("testmod:fox_tail", "body");
                }
                1 => {
                    look.model("testmod:katana_broken")
                        .worn_texture("base:armor/diamond_1");
                }
                _ => {}
            }
        });
    r.entity(&DISPLAY_CARRIER)
        .size(0.6, 1.8)
        .kinematic()
        .inventory(2)
        .equipment()
        .shown_slots(&[1, 0])
        .model(humanoid(false))
        .texture("base:zombie")
        .no_egg();
    r.client_command("display_calls", "<sample:word>", |cmd| {
        let sample = cmd.text("sample").unwrap_or_default().to_string();
        let calls = DISPLAY_LOOK_CALLS.load(std::sync::atomic::Ordering::Relaxed);
        cmd.reply("command.testmod.display_calls", &[Arg::Text(&sample), Arg::Int(calls as i64)]);
    });
    r.config(&GENERATOR_HUM).bool(true);
    // The server decides how much its wisps sparkle; joining players get its value.
    r.config(&WISP_SPARKS).int(2, 0, 10).server();
    r.config(&DREAM_STRENGTH).float(1.0, 0.1, 1.0).step(0.05);
    r.config(&DREAM_STYLE).choice(0, &["warm", "cold"]);
    r.particle(&SPARK)
        .textures(&["core:glint_3", "core:glint_2", "core:glint_1", "core:glint_0"])
        .life(0.6, 1.2)
        .size(0.12, 0.2)
        .gravity(-0.6)
        .drag(0.9)
        .color(0xFFFFFFFF, 0xFFFFFF60)
        .fullbright()
        .translucent()
        .shader(&Shader::named("twinkle"));
    // A drop that rings where it falls into water and is gone where it hits the ground.
    r.particle(&DROP)
        .textures(&["core:generic_0"])
        .life(3.0, 3.0)
        .size(0.25, 0.25)
        .gravity(12.0)
        .drag(1.0)
        .on_enter_liquid(When::tag("water"), Then::turn_into_still(&RIPPLE))
        .on_land(When::any(), Then::die());
    r.particle(&RIPPLE)
        .textures(&["core:generic_0"])
        .life(3.0, 3.0)
        .size(0.4, 0.4)
        .drag(0.0)
        .color(0xFF2020FF, 0xFF2020FF)
        .fullbright();

    // A glowing obelisk here and there in the dry parts of Mars.
    static OBELISK: Template = Template::named("obelisk");
    let mut obelisk = r.template(&OBELISK).layer(&["sss", "sss", "sss"]);
    for _ in 0..4 {
        obelisk = obelisk.layer(&["   ", " g ", "   "]);
    }
    drop(obelisk.key('s', "base:sandstone").key('g', "glow_crystal"));
    r.structure(&OBELISK)
        .dimension(&crate::MARS)
        .spacing(128)
        .chance(0.5)
        .biome("#dry")
        .buried(1)
        .turns();
    // An outpost here and there on Mars: a hub with a chest and a pig, halls joined to its sides
    // and to each other by their joints, in bricks where the template says stone bricks.
    outpost(r);
    // Glow crystals grow only in Mars' crystal caves (an underground biome).
    r.ore(&GLOW_CRYSTAL)
        .dimension(&crate::MARS)
        .per_cube(3)
        .vein_size(6)
        .replaces("#stone_ore_replaceables")
        .biome("#cave");
    // A 64x64 texture among 16x16 ones: every texture keeps its own size.
    r.block(&MOSAIC)
        .hardness(1.5)
        .mined_by("pickaxe", tool_tier::HAND);
    // A tint kind of the mod's own: violet by default (and on the item), teal in plains.
    r.tint_kind(&GLOW, 0xc080ff);
    r.edit(&PLAINS).tint(&GLOW, 0x40e0c0);
    r.block(&GLOW_MOSS)
        .tint_kind(&GLOW)
        .hardness(0.6)
        .mined_by("shovel", tool_tier::HAND);
    // A glowing birch (a log's `tree` component grows it from a sapling or worldgen) and
    // flower; not generated in the world, only from the creative tabs.
    static TREE_LEAVES: Component<BlockRef> = Component::named("tree_leaves");
    r.block(&GLOW_LOG)
        .texture("glow_log")
        .ends("glow_log_top")
        .hardness(2.0)
        .mined_by("axe", tool_tier::HAND)
        .tag("logs")
        .set_text(&TREE_LEAVES, "glow_leaves");
    r.block(&GLOW_LEAVES)
        .cutout()
        .light_opacity(1)
        .tint_kind(&GLOW)
        .hardness(0.2)
        .tag("leaves")
        .mined_by("shears", tool_tier::HAND);
    r.block(&SNOW_LAYER)
        .texture("base:snow")
        .model_box([0.0, 0.0, 0.0], [16.0, 2.0, 16.0])
        .not_solid()
        .needs_below("")
        .shares_cell_with("#plantable")
        .hardness(0.1);
    r.block(&GLOW_FLOWER)
        .plant()
        .needs_below("#plant_soil")
        .tint_kind(&GLOW);
    r.block(&GLOW_CRYSTAL)
        .hardness(1.5)
        .mined_by("pickaxe", tool_tier::HAND)
        // Sparks now and then, shown by the client near the player.
        .ambient_particles(&["spark", "core:smoke"]);

    // Its own category (no spawning rule: wisps come only from commands and creative): they
    // drift off again when nobody is within 64 blocks.
    r.entity_category(&SPIRITS, 4, 64.0);
    r.entity(&WISP)
        .category(&SPIRITS)
        .size(0.4, 0.4)
        .max_health(4.0)
        .walk_speed(1.5)
        .tag(abi::comp::entity::NO_GRAVITY)
        .unpushable()
        .model(
            Model::from(vec![ModelPart::new("wisp", [0.0, 3.0, 0.0]).cuboid([-3.0, -3.0, -3.0], [6.0, 6.0, 6.0], [0.0, 0.0])])
                .uv_size(32, 16),
        )
        .shader(&Shader::named("ghost"))
        .on_tick(wisps);

    r.entity(&IDOL)
        .size(0.6, 1.1)
        .max_health(20.0)
        .animations("idol")
        .on_interact(|e| {
            let mut bowed = BOWED.lock().unwrap();
            let id = e.entity();
            if let Some(i) = bowed.iter().position(|&b| b == id) {
                bowed.swap_remove(i);
                e.world().action_play(id, "", false);
            } else {
                // Bows to whoever used it.
                let w = e.world();
                if let (Some(me), Some(them)) = (w.entity(id), w.entity(e.other())) {
                    w.entity_modify(id).yaw(yaw_toward(me.pos, them.pos));
                }
                bowed.push(id);
                w.action_play(id, "bow", false);
            }
            true
        });
    let ring = ModelPart::new("ring", [0.0, 8.0, 0.0]).child_of(0);
    let ring = [[6.0, -1.0, -1.0], [-8.0, -1.0, -1.0], [-1.0, -1.0, 6.0], [-1.0, -1.0, -8.0]]
        .into_iter()
        .fold(ring, |p, at| p.cuboid(at, [2.0, 2.0, 2.0], [16.0, 0.0]));
    r.entity(&ORB)
        .environment(
            Environment::new("orb_aura")
                .radius(4.0, 8.0)
                .fade(0.2, 0.2)
                .acoustics(4000.0, 0.25, 0.8),
        )
        .size(0.6, 0.9)
        .max_health(4.0)
        .tag(abi::comp::entity::NO_GRAVITY)
        .model(
            Model::from(vec![
                ModelPart::new("core", [0.0, 8.0, 0.0]).mesh(sphere_mesh([0.0, 8.0, 0.0], 4.0, 8, 12, [0.0, 0.0, 16.0, 16.0])),
                ring,
            ])
            .uv_size(32, 16),
        );

    r.set_animations(&PLAYER, "testmod:player");
    r.item(&TOTEM).max_stack(1);
    // An item's look is layers, a stack's the same (`stack.with_layers(Layers::new()...)`
    // replaces it per stack). `tint` colors the layer just before it, multiplying: white
    // parts take the color. A plain item is one layer.
    static RUBY: Item = Item::named("ruby");
    static WAND: Item = Item::named("wand");
    r.item(&RUBY).texture("base:diamond").tint(0xE0204A);
    r.item(&WAND)
        .layer("base:stick")
        .layer("base:diamond")
        .tint(0x40E0FF)
        .max_stack(1)
        .handheld();
    // A block's faces are layers the same way, picked by `texture`, `top`, `sides` and the
    // like: wool dyed by a tint baked in as the game loads, and stone with grass over it that
    // the biome colors, as a grass block's sides.
    static BLACK_WOOL: Block = Block::named("black_wool");
    static MOSSY_STONE: Block = Block::named("mossy_stone");
    r.block(&BLACK_WOOL)
        .texture("base:wool_white")
        .tint(0x303038)
        .hardness(0.8);
    // Tinted glass keeps the glass's connected edges, the tiles tinted too, and joins plain
    // glass: `glass.ctm` connects by tile, whatever the tint.
    static RED_GLASS: Block = Block::named("red_glass");
    r.block(&RED_GLASS)
        .texture("base:glass")
        .tint(0xE04040)
        .cutout()
        .cull_same()
        .hardness(0.3);
    r.block(&MOSSY_STONE)
        .texture("base:stone")
        .layer("base:grass_side_overlay")
        .tint_kind(&tints::GRASS)
        .hardness(1.5)
        .mined_by("pickaxe", tool_tier::HAND);
    // Goes before what the other hand holds, and leaves it its turn with nothing to shoot.
    static LONG_BOW: Item = Item::named("long_bow");
    r.item(&LONG_BOW)
        .use_priority(10)
        .on_use(|u| u.stack().count > 1);
    // Both hands or nothing, swinging with its whole body.
    static GREATSWORD: Item = Item::named("greatsword");
    r.item(&GREATSWORD)
        .max_stack(1)
        .two_handed()
        .no_creative_break()
        .attack_damage(16f32)
        .attack_speed(0.7)
        .condition(|s, v| {
            if s.wear() > 0 {
                v.model("katana_broken");
            }
        })
        .on_use(|_| true);
    // A metre of katana in the hand, on the ground and in a slot; much less of one once damaged.
    r.set_model(&GREATSWORD, "katana");
    // Its apple has a model of its own, by one line and a file.
    r.set_model(&crate::APPLE, "food/apple");
    // Its chain hangs and sways by itself: its bones are called `hanging_..`.
    static LANTERN: Item = Item::named("lantern");
    r.item(&LANTERN).max_stack(1);
    r.set_model(&LANTERN, "lantern");
    // Only who wears it has a tail, moved by what plays on them and swinging by itself.
    static FOX_TAIL: Item = Item::named("fox_tail");
    r.item(&FOX_TAIL)
        .max_stack(1)
        .wearable_model(&EquipmentSlot::named("base:legs"), "fox_tail", "body");
    r.bone_motion("fox_tail", "tail2", BoneMotion::SPRINGY);
    r.bone_motion("fox_tail", "tail3", BoneMotion::SPRINGY);
    // Both hands, or one if the other is busy.
    static MAP_SHEET: Item = Item::named("map_sheet");
    r.item(&MAP_SHEET)
        .max_stack(1)
        .one_handed_action("hold_one")
        .on_use(|_| true);
    // A model of bones in `models/item/`, its slide going back in the same clip as the arm.
    static BLASTER: Item = Item::named("blaster");
    r.item(&BLASTER).max_stack(1).handheld();
    r.item(&RUNE).max_stack(1).on_use(|u| {
        let w = u.world();
        w.action_play(u.entity(), "wave", false);
        if let Some(st) = w.entity(u.entity()) {
            w.spawn_particles(&SPARK, [st.pos.x, st.pos.y + 1.2, st.pos.z])
                .count(40)
                .spread(0.2)
                .speed(4.0);
        }
        true
    });
    r.condition(&HURT, |c| {
        c.client()
            .and_then(|v| v.player_state())
            .is_some_and(|m| m.health < m.max_health)
    });

    r.key(&DREAM_KEY, "N").on_client(|c| {
        let on = DREAM.fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
        let effect = if DREAM_STYLE.get() == 1 { &DREAM_COLD } else { &DREAM_EFFECT };
        c.post_effect(&DREAM_EFFECT, 0.0);
        c.post_effect(&DREAM_COLD, 0.0);
        if !on {
            c.post_effect(effect, DREAM_STRENGTH.get());
        }
    });
    r.client_frame(|c| {
        // Holding the rune shows what it would reach: a see-through sheet over the ground around.
        let st = c.state();
        if c.inventory_slot(st.selected).item == RUNE.id() {
            let p = st.pos.block();
            let (x, y, z) = (p.x as f64, p.y as f64, p.z as f64);
            c.draw_fill(None, [x - 2.0, y, z - 2.0], [x + 3.0, y + 0.06, z + 3.0], 0x60D0FF50);
        }
    });
}

static DREAM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Wisps drift in a new direction every two seconds, bob up and down and shed sparks.
fn wisps(t: &EntityTick) {
    let w = t.world();
    let time = w.game_time();
    for &id in t.ids() {
        let Some(mut st) = w.entity(id) else { continue };
        let phase = (id % 40) as u64;
        if (time + phase) % 40 == 0 {
            let a = w.random_below(360) as f32 * std::f32::consts::PI / 180.0;
            st.yaw = a;
        }
        st.vel.y = ((time as f64 + phase as f64) * 0.1).sin() * 0.8;
        w.entity_modify(id).yaw(st.yaw).velocity(st.vel);
        w.walk(id, weft_sdk::Walk::toward(yaw_dir(st.yaw), 1.0));
        if (time + phase) % 20 == 0 {
            w.spawn_particles(&SPARK, [st.pos.x, st.pos.y + 0.2, st.pos.z])
                .count(WISP_SPARKS.get().max(0) as u32)
                .spread(0.15)
                .speed(0.3);
        }
    }
}

/// What an outpost may be built of, as records other mods add to and change: a weight each.
static STONE_STYLE: Record = Record::named("stone_style");
static STYLE: Tag = Tag::named("testmod:style");
static STYLE_WEIGHT: Component<f32> = Component::named("testmod:weight");

pub static OUTPOST: Feature = Feature::named("outpost");
pub static OUTPOST_HUB: Template = Template::named("outpost_hub");
/// From `assets/structures/outpost_hall.json`.
pub static OUTPOST_HALL: Template = Template::named("outpost_hall");
static OUTPOST_LOOT: LootTable = LootTable::named("outpost");
static RED: Palette = Palette::named("red");
static PIG: EntityKind = EntityKind::named("base:pig");

fn outpost(r: &mut Registry) {
    r.record(&STONE_STYLE)
        .tag(STYLE.name())
        .set(&STYLE_WEIGHT, 3.0);
    // 5 x 4 x 5, open at the middle of each side, where its joints are.
    r.template(&OUTPOST_HUB)
        .layer(&["wwwww", "wwwww", "wwwww", "wwwww", "wwwww"])
        .layer(&["ww.ww", "w...w", ".....", "w...w", "ww.ww"])
        .layer(&["wwwww", "w...w", "w...w", "w...w", "wwwww"])
        .layer(&["sssss", "sssss", "sssss", "sssss", "sssss"])
        .key('w', "@wall")
        .key('s', "@wall:slab")
        .key('.', "air")
        .fill("base:chest", [2, 1, 2], [2, 1, 2])
        .slot("wall", "base:stone_bricks")
        .loot([2, 1, 2], &OUTPOST_LOOT)
        .entity(&PIG, [1.5, 1.0, 1.5])
        .joint([2, 1, 0], 4, "side", "door", "halls")
        .joint([4, 1, 2], 1, "side", "door", "halls")
        .joint([2, 1, 4], 5, "side", "door", "halls")
        .joint([0, 1, 2], 0, "side", "door", "halls");
    // The hall is the file `structures/outpost_hall.json`: 3 x 3 x 4 along z, its door at the
    // north end, going on at the south end.
    r.palette(&RED).slot("wall", "base:bricks");
    r.loot(&OUTPOST_LOOT)
        .pool(Pool::new().rolls(3, 3).entry(Entry::item("base:diamond")));
    r.feature(&OUTPOST)
        .dimension(&crate::MARS)
        .spacing(320)
        .chance(1.0)
        .priority(2)
        .reach(40)
        .reach_y(12, 4)
        .palette(&RED)
        .plan(|p| {
            let at = p.pos();
            let halls = Pools::new().pool("halls", &[(&OUTPOST_HALL, 1)]);
            p.grow(&OUTPOST_HUB, at, Transform::NONE, &halls, 2);
        });
}

fn environment_controls(r: &mut Registry) {
    let suns = [base_api::OVERWORLD.sky(&base_api::SUN), base_api::FLATLANDS.sky(&base_api::SUN)];
    r.command("eclipse", "<state:word(on,off)> [entity:int]", move |cmd| {
        let (w, player) = (cmd.world(), cmd.player());
        let entity = cmd.int("entity").map_or(player, |id| id as u64);
        if cmd.text("state") == Some("off") {
            w.clear_environment(entity, "eclipse");
        } else {
            let mut layer = Environment::new("eclipse")
                .priority(100)
                .fade(1.0, 1.0)
                .sunlight(0.0)
                .sky_color(0x000000)
                .fog(0x000000, 12.0, 48.0)
                .cloud_fog(12.0, 48.0);
            for sun in suns {
                layer = layer.sky_object(sun, 0.0);
            }
            if entity != player {
                layer = layer.radius(12.0, 24.0);
            }
            w.environment(entity, &layer);
        }
        cmd.reply("command.testmod.eclipse", &[]);
    });
}
