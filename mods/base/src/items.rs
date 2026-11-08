use weft_sdk::*;

pub static STICK: Item = Item::named("stick");
pub static COAL: Item = Item::named("coal");
pub static IRON_INGOT: Item = Item::named("iron_ingot");
pub static GOLD_INGOT: Item = Item::named("gold_ingot");
pub static DIAMOND: Item = Item::named("diamond");
pub static WHEAT_SEEDS: Item = Item::named("wheat_seeds");
pub static WHEAT: Item = Item::named("wheat");
pub static BUCKET: Item = Item::named("bucket");
pub static FLINT: Item = Item::named("flint");
pub static CLAY_BALL: Item = Item::named("clay_ball");

pub use base_api::{ARMOR, Armor, FLAMMABLE, FOOD, FUEL, Food};
/// Tags of armor pieces, head to feet; `<tag>/<material>` picks the recipe (see recipes.rs).
pub const ARMOR_SLOTS: [&str; 4] = ["armor_head", "armor_chest", "armor_legs", "armor_feet"];

struct Material {
    name: &'static str,
    level: u32,
    speed: f32,
    durability: u32,
    damage: f32,
}

const MATERIALS: [Material; 5] = [
    Material { name: "wood", level: tool_tier::WOOD, speed: 2.0, durability: 59, damage: 0.0 },
    Material { name: "stone", level: tool_tier::STONE, speed: 4.0, durability: 131, damage: 1.0 },
    Material { name: "iron", level: tool_tier::IRON, speed: 6.0, durability: 250, damage: 2.0 },
    Material { name: "gold", level: tool_tier::WOOD, speed: 12.0, durability: 32, damage: 0.0 },
    Material { name: "diamond", level: tool_tier::DIAMOND, speed: 8.0, durability: 1561, damage: 3.0 },
];

pub fn register(r: &mut Registry) {
    r.item(&STICK)
        .tag("stick")
        .handheld()
        .set(&FUEL, 100)
        .tab(&crate::tabs::INGREDIENTS);
    r.item(&COAL)
        .tag("gems/coal")
        .set(&FUEL, 1600)
        .tab(&crate::tabs::INGREDIENTS);
    r.item(&IRON_INGOT)
        .tag("ingots/iron")
        .tag("materials/iron")
        .tab(&crate::tabs::INGREDIENTS);
    r.item(&GOLD_INGOT)
        .tag("ingots/gold")
        .tag("materials/gold")
        .tab(&crate::tabs::INGREDIENTS);
    // Metal ores drop raw pieces (like modern versions) that smelt into ingots.
    r.item(Item::generated("raw_iron".into()))
        .tag("raw_ores/iron")
        .tab(&crate::tabs::INGREDIENTS);
    r.item(Item::generated("raw_gold".into()))
        .tag("raw_ores/gold")
        .tab(&crate::tabs::INGREDIENTS);
    r.item(&DIAMOND)
        .tag("gems/diamond")
        .tag("materials/diamond")
        .tab(&crate::tabs::INGREDIENTS);
    r.item(&WHEAT_SEEDS)
        .places(&crate::blocks::WHEAT)
        .tab(&crate::tabs::NATURAL);
    r.item(&WHEAT)
        .tag("crops/wheat")
        .tab(&crate::tabs::INGREDIENTS);
    // What gravel and clay drop.
    r.item(&FLINT).tab(&crate::tabs::INGREDIENTS);
    r.item(&CLAY_BALL).tab(&crate::tabs::INGREDIENTS);
    // Filled buckets come with their liquids (blocks.rs); the engine fills and pours them.
    r.item(&BUCKET).max_stack(16).tab(&crate::tabs::TOOLS);
    r.set("lava_bucket", &FUEL, 20000);
    r.tag("buckets/lava", &["lava_bucket"]);
    r.tag("bread", &["bread"]);

    for (name, hunger, saturation, tag) in [
        ("apple", 4, 2.4, ""),
        ("bread", 5, 6.0, ""),
        ("porkchop", 3, 1.8, "raw_meat/pork"),
        ("cooked_porkchop", 8, 12.8, "cooked_meat/pork"),
        ("beef", 3, 1.8, "raw_meat/beef"),
        ("cooked_beef", 8, 12.8, "cooked_meat/beef"),
        ("rotten_flesh", 4, 0.8, ""),
        ("chicken", 2, 1.2, "raw_meat/chicken"),
        ("cooked_chicken", 6, 7.2, "cooked_meat/chicken"),
        ("mutton", 2, 1.2, "raw_meat/mutton"),
        ("cooked_mutton", 6, 9.6, "cooked_meat/mutton"),
        ("spider_eye", 2, 3.2, ""),
    ] {
        let i = r
            .item(Item::generated(name.into()))
            .set(&FOOD, Food { hunger, saturation })
            .use_ticks(32)
            .on_use(base_api::eat)
            .tab(&crate::tabs::FOOD);
        let i = if tag.starts_with("cooked_meat/") { i.tag("cooked_meat") } else { i };
        if !tag.is_empty() {
            i.tag(tag);
        }
    }
    r.item(Item::generated("leather".into()))
        .tag("materials/leather")
        .tab(&crate::tabs::INGREDIENTS);
    for name in ["bone", "string", "sugar", "gunpowder", "feather", "arrow", "egg"] {
        r.item(Item::generated(name.into()))
            .tag(name)
            .tab(&crate::tabs::INGREDIENTS);
    }

    for m in &MATERIALS {
        // `.tool(kind)` tags it `tools/<kind>`; `<kind>s/<material>` picks its recipe.
        let tool = |r: &mut Registry, kind: &str, damage: f32, speed: f32| {
            let b = r
                .item(Item::generated(format!("{}_{kind}", m.name)))
                .tool(kind, m.level)
                .mining_speed(m.speed)
                .durability(m.durability)
                .attack_damage(damage)
                .attack_speed(speed)
                .tag(&format!("{kind}s/{}", m.name))
                // Vanilla: weapons wear by one per hit, tools (made for blocks) by two.
                .set(&WEAR_PER_HIT, if kind == "sword" { 1 } else { 2 })
                .tab(if kind == "sword" { &crate::tabs::COMBAT } else { &crate::tabs::TOOLS });
            if kind == "sword" {
                b.sweeps().no_creative_break();
            }
        };
        tool(r, "pickaxe", 2.0 + m.damage, 1.2);
        tool(r, "shovel", 2.5 + m.damage, 1.0);
        tool(r, "sword", 4.0 + m.damage, 1.6);
        // Modern's: slow and strong axes, hoes barely hit but swing fast with the material.
        let (axe_damage, axe_speed, hoe_speed) = match m.name {
            "wood" => (7.0, 0.8, 1.0),
            "stone" => (9.0, 0.8, 2.0),
            "iron" => (9.0, 0.9, 3.0),
            "diamond" => (9.0, 1.0, 4.0),
            _ => (7.0, 1.0, 1.0),
        };
        tool(r, "axe", axe_damage, axe_speed);
        tool(r, "hoe", 1.0, hoe_speed);
    }

    // Points per piece, head to feet, as in modern.
    for (mat, points, toughness, durability) in [
        ("leather", [1u32, 3, 2, 1], 0.0f32, 5u32),
        ("iron", [2, 6, 5, 2], 0.0, 15),
        ("gold", [2, 5, 3, 1], 0.0, 7),
        ("diamond", [3, 8, 6, 3], 2.0, 33),
    ] {
        for (slot, piece, dur) in [(0, "helmet", 11), (1, "chestplate", 16), (2, "leggings", 15), (3, "boots", 13)] {
            let points = points[slot];
            let tag = ARMOR_SLOTS[slot];
            // Leggings are the inner layer, drawn closer to the body.
            let (inflate, layer) =
                if mat == "leather" { if slot == 2 { (0.2, 2) } else { (0.4, 1) } } else { if slot == 2 { (0.5, 2) } else { (0.75, 1) } };
            r.item(Item::generated(format!("{mat}_{piece}")))
                .durability(durability * dur)
                .set(&ARMOR, Armor { points, toughness })
                .tag(tag)
                .tag(&format!("{tag}/{mat}"))
                .wearable(crate::player::EQUIPMENT[slot as usize], inflate, &format!("armor/{mat}_{layer}"))
                .tab(&crate::tabs::COMBAT);
        }
    }
}
