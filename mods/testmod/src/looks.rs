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
//! - the player waves when using the rune: keyframes for the player's model, seen from anywhere.

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

pub fn register(r: &mut Registry) {
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
        .buried(1);
    // Glow crystals grow only in Mars' crystal caves (an underground biome).
    r.ore(&GLOW_CRYSTAL)
        .dimension(&crate::MARS)
        .veins(3)
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
        .tint(&GLOW)
        .hardness(0.6)
        .mined_by("shovel", tool_tier::HAND);
    // A glowing birch (a log's `tree` component grows it from a sapling or worldgen) and
    // flower; not generated in the world, only from the creative tabs.
    static TREE_LEAVES: Component<BlockRef> = Component::named("tree_leaves");
    r.block(&GLOW_LOG)
        .textures("glow_log_top", "glow_log", "glow_log_top")
        .hardness(2.0)
        .mined_by("axe", tool_tier::HAND)
        .tag("logs")
        .set_text(&TREE_LEAVES, "glow_leaves");
    r.block(&GLOW_LEAVES)
        .cutout()
        .light_opacity(1)
        .tint(&GLOW)
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
        .tint(&GLOW);
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
        w.action_play(u.player(), "wave", false);
        if let Some(st) = w.entity(u.player()) {
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
        w.walk(id, yaw_dir(st.yaw), 1.0, false);
        if (time + phase) % 20 == 0 {
            w.spawn_particles(&SPARK, [st.pos.x, st.pos.y + 0.2, st.pos.z])
                .count(WISP_SPARKS.get().max(0) as u32)
                .spread(0.15)
                .speed(0.3);
        }
    }
}
