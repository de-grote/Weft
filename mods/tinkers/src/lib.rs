use std::{collections::HashMap, sync::OnceLock};

use base_api::CRAFTING;
use weft_sdk::*;

declare_mod!(deps: ["base"], init: init);

static TINKER_TAB: CreativeTab = CreativeTab::named("tinkers");
static TINKER_MATS_TAB: CreativeTab = CreativeTab::named("tinkers_materials");

static IRON_PICKAXE: Item = Item::named("base:iron_pickaxe");
static PICKAXE: Item = Item::named("pickaxe");
static AXE: Item = Item::named("axe");
static SHOVEL: Item = Item::named("shovel");

static WORKBENCH: Block = Block::named("workbench");
static WORKBENCH_RECIPE: RecipeKind = RecipeKind::named("workbench");
static WORKBENCH_MENU: Menu = Menu::named("workbench");

static PART_BUILDER: Block = Block::named("part_builder");
static PART_BUILDER_RECIPE: RecipeKind = RecipeKind::named("part_builder");
static PART_BUILDER_MENU: Menu = Menu::named("part_builder");

// static ROD: Tag = Tag::named("rod");
// static BINDING: Tag = Tag::named("binding");
// static PICKAXE_HEAD: Tag = Tag::named("pickaxe_head");

static GENERATED_ITEMS: OnceLock<HashMap<String, &'static Item>> = OnceLock::new();
const COMPONENTS: &[&str] = &["rod", "binding", "pickaxe_head", "axe_head", "shovel_head"];

#[derive(Debug, Clone, Copy)]
struct MaterialInfo {
    name: &'static str,
    color: u32,
    durability: u32,
    level: u32,
    speed: f32,
}
static MATERIALS: &[MaterialInfo] = &[
    MaterialInfo { name: "wood", color: 0x8B4513, durability: 20, level: tool_tier::WOOD, speed: 2.0 },
    MaterialInfo { name: "stone", color: 0x708090, durability: 45, level: tool_tier::STONE, speed: 4.0 },
    MaterialInfo { name: "iron", color: 0xD7D7D7, durability: 85, level: tool_tier::IRON, speed: 6.0 },
    MaterialInfo { name: "gold", color: 0xFFD700, durability: 10, level: tool_tier::WOOD, speed: 12.0 },
    MaterialInfo { name: "diamond", color: 0x00CED1, durability: 520, level: tool_tier::DIAMOND, speed: 8.0 },
];

fn init(r: &mut Registry) {
    r.creative_tab(&TINKER_TAB, &IRON_PICKAXE);
    r.creative_tab(&TINKER_MATS_TAB, &Item::generated("iron_pickaxe_head".into()));

    r.recipe_kind(&PART_BUILDER_RECIPE)
        .inputs(1, 1)
        .outputs(1, 1);

    r.recipe_kind(&WORKBENCH_RECIPE).inputs(1, 3).outputs(1, 1);
    r.shaped(&WORKBENCH_RECIPE, PICKAXE.name(), 1, &["h", "b", "r"], &[('h', "#pickaxe_head"), ('b', "#binding"), ('r', "#rod")]);
    r.shaped(&WORKBENCH_RECIPE, AXE.name(), 1, &["h", "b", "r"], &[('h', "#axe_head"), ('b', "#binding"), ('r', "#rod")]);
    r.shaped(&WORKBENCH_RECIPE, SHOVEL.name(), 1, &["h", "b", "r"], &[('h', "#shovel_head"), ('b', "#binding"), ('r', "#rod")]);

    r.block(&WORKBENCH)
        .tab(&TINKER_TAB)
        .hardness(3.0)
        .mined_by("axe", abi::tool_tier::HAND)
        .opens(&WORKBENCH_MENU)
        .textures("workbench_top", "workbench_side", "base:crafting_table_bottom");
    r.shaped(&CRAFTING, WORKBENCH.name(), 1, &["iii", "lsl", "lll"], &[('i', "#ingots/iron"), ('l', "#planks"), ('s', "#stone")]);

    r.block(&PART_BUILDER)
        .tab(&TINKER_TAB)
        .hardness(3.0)
        .mined_by("axe", abi::tool_tier::HAND)
        .opens(&PART_BUILDER_MENU)
        .textures("part_builder_top", "part_builder_side", "base:crafting_table_bottom");
    r.shaped(&CRAFTING, PART_BUILDER.name(), 1, &["ppp", "lil", "lll"], &[('p', "#paper"), ('l', "#planks"), ('i', "#ingots/iron")]);

    let mut generated_items = HashMap::new();
    for material in MATERIALS.iter() {
        for &component in COMPONENTS {
            let name = format!("{}_{}", &material.name, component);
            let item = Item::generated(name.clone());
            generated_items.insert(name.clone(), item);
            r.shapeless(&PART_BUILDER_RECIPE, &name, 1, &[], 0);

            let mut ib = r
                .item(item)
                .tab(&TINKER_MATS_TAB)
                .texture(component)
                .color(material.color)
                .tag(&component)
                .tag(&format!("tinkers/{}", material.name));
            if component.ends_with("head") {
                ib = ib.tag("tool_head");
            }
            if component == "rod" {
                ib = ib.tag("stick");
            }
        }
    }
    let Ok(_) = GENERATED_ITEMS.set(generated_items) else {
        panic!("failed to save generated items");
    };

    r.item(&PICKAXE)
        .tool("pickaxe", 0)
        .tab(&TINKER_TAB)
        .texture("pickaxe")
        .max_stack(1);

    r.item(&AXE)
        .tool("axe", 0)
        .tab(&TINKER_TAB)
        .texture("axe")
        .max_stack(1);

    r.item(&SHOVEL)
        .tool("shovel", 0)
        .tab(&TINKER_TAB)
        .texture("shovel")
        .max_stack(1);

    r.menu(&WORKBENCH_MENU)
        .group("head", Slots::single().accepts("tool_head"))
        .group("binding", Slots::single().accepts("binding"))
        .group("rod", Slots::single().accepts("rod"))
        .group("result", Slots::single().output().large())
        .player_inventory()
        .recipes(&WORKBENCH_RECIPE)
        .on_change(craft_result_workbench)
        .on_take(consume_grid_workbench)
        .screen(|ui| {
            ui.panel("panel", S::column().pad(7.0).gap(4.0).stretch(), |ui| {
                ui.title();
                ui.row(S::row().gap(8.0), |ui| {
                    ui.spacer();
                    ui.column(S::column(), |ui| {
                        ui.group("head");
                        ui.group("binding");
                        ui.group("rod");
                    });
                    ui.recipe_arrow();
                    ui.group("result");
                    ui.spacer();
                });
                ui.player_inventory(true);
            });
        });

    r.menu(&PART_BUILDER_MENU)
        .group("input", Slots::single())
        .group("output", Slots::grid(3, 2).output())
        .player_inventory()
        .recipes(&PART_BUILDER_RECIPE)
        .on_change(craft_result_part_builder)
        .on_take(consume_grid_part_builder)
        .screen(|ui| {
            ui.panel("panel", S::column().pad(7.0).gap(4.0), |ui| {
                ui.title();
                ui.row(S::row().gap(6.0), |ui| {
                    ui.group("input");
                    ui.recipe_arrow();
                    ui.group("output");
                });
                ui.player_inventory(true);
            });
        });
}

/// The `result` slot shows what the `grid` makes.
fn craft_result_workbench(m: &MenuCtx) {
    let head = m.group("head").get(0);
    let binding = m.group("binding").get(0);
    let rod = m.group("rod").get(0);
    if head.is_empty() || binding.is_empty() || rod.is_empty() {
        m.group("result").set(0, ItemStack::EMPTY);
        return;
    }
    let head_name = item_name(head.item);
    let binding_name = item_name(binding.item);
    let rod_name = item_name(rod.item);
    let (Some(head_mat), Some(binding_mat), Some(rod_mat)) = (material_of(head_name), material_of(binding_name), material_of(rod_name))
    else {
        error!("could not get info of items");
        m.group("result").set(0, ItemStack::EMPTY);
        return;
    };

    let stack = match tool_of(head_name) {
        "pickaxe" => with_tool_components(ItemStack::new(PICKAXE.id(), 1), head_mat, binding_mat, rod_mat).with_layers(&[
            ("rod", Some(rod_mat.color)),
            ("pickaxe_head", Some(head_mat.color)),
            ("pickaxe_binding", Some(binding_mat.color)),
        ]),
        "axe" => with_tool_components(ItemStack::new(AXE.id(), 1), head_mat, binding_mat, rod_mat).with_layers(&[
            ("rod", Some(rod_mat.color)),
            ("axe_head", Some(head_mat.color)),
            ("axe_binding", Some(binding_mat.color)),
        ]),
        "shovel" => with_tool_components(ItemStack::new(SHOVEL.id(), 1), head_mat, binding_mat, rod_mat).with_layers(&[
            ("small_rod", Some(rod_mat.color)),
            ("shovel_head", Some(head_mat.color)),
            ("shovel_binding", Some(binding_mat.color)),
        ]),
        _ => {
            error!("unknown tool");
            m.group("result").set(0, ItemStack::EMPTY);
            return;
        }
    };

    m.group("result").set(0, stack);
}

/// Finds the material by checking the first part of the name
fn material_of(name: &str) -> Option<MaterialInfo> {
    let (_, id) = name.split_once(":")?;
    let (mat, _) = id.split_once("_")?;
    MATERIALS.iter().find(|m| m.name == mat).copied()
}

// gets the name of the tool
fn tool_of(name: &str) -> &str {
    let Some((_, a)) = name.split_once("_") else { return "" };
    let Some((tool, _)) = a.split_once("_") else { return "" };
    return tool;
}

fn with_tool_components(stack: ItemStack, head_mat: MaterialInfo, binding_mat: MaterialInfo, rod_mat: MaterialInfo) -> ItemStack {
    stack
        .with(&DURABILITY, head_mat.durability + binding_mat.durability + rod_mat.durability)
        .with(&MINING_SPEED, head_mat.speed)
        .with(&TOOL_LEVEL, head_mat.level)
}

/// Takes one of each ingredient; the last of a stack leaves its `remainder` (a bucket).
fn consume_grid_workbench(m: &MenuCtx) {
    for grid in [m.group("head"), m.group("binding"), m.group("rod")] {
        let s = grid.get(0);
        if !s.is_empty() {
            grid.set(0, consume_one(s));
        }
    }
}

fn craft_result_part_builder(m: &MenuCtx) {
    let input = m.group("input").get(0);
    let output = m.group("output");
    let tags = [("planks", "wood"), ("stone", "stone"), ("ingots/gold", "gold"), ("ingots/iron", "iron"), ("gems/diamond", "diamond")];
    for (tag, material) in tags {
        if item_has_tag(input.item, tag) {
            for i in 0..COMPONENTS.len() {
                if let Some(item) = GENERATED_ITEMS
                    .get()
                    .unwrap()
                    .get(&format!("{}_{}", material, COMPONENTS[i]))
                {
                    output.set(i as u32, ItemStack::new(item.id(), 1));
                }
            }
            return;
        }
    }
    // clear
    for i in 0..COMPONENTS.len() {
        output.set(i as u32, ItemStack::EMPTY);
    }
}

fn consume_grid_part_builder(m: &MenuCtx) {
    let grid = m.group("input");
    let s = grid.get(0);
    if !s.is_empty() {
        grid.set(0, consume_one(s));
    }
}
