# Registration guide

Everything a mod registers, kind by kind: what it is for, how to make one, what it is made of, and how to change someone else's.\
The short version of the model is in the cheat sheet, [How registering works](modding.md#how-registering-works). This is the long one, for when you need the name of a component or want to know what an edit of some kind can reach.

## Quick links
[the model](#the-model) | [blocks](#block) | [block states](#block-states) | [items](#item) | [entities](#entity) | [biomes](#biome) | [dimensions](#dimension) | [terrain](#terrain) | [menus and screens](#menu-and-screen) | [features](#feature) | [liquids](#liquid) | [reactions](#reaction) | [templates](#template) | [palettes](#palette) | [spawn rules](#spawn-rule) | [recipes](#recipe) | [recipe kinds](#recipe-kind) | [loot tables](#loot-table) | [particles](#particle) | [sky objects](#sky-object) | [sounds](#sound) | [music](#music) | [weather](#weather) | [precipitation](#precipitation) | [poses](#pose) | [game modes](#game-mode) | [damage causes](#damage-cause) | [entity effects](#entity-effect) | [text places](#text-place) | [conditions](#condition) | [cameras](#camera) | [keys](#key) | [settings](#config) | [attributes](#attribute) | [equipment slots](#equipment-slot) | [creative tabs](#creative-tab) | [categories](#sound-and-entity-category) | [tint kinds](#tint-kind) | [HUD elements](#hud-element) | [screen parts](#screen-part) | [word sets](#word-set) | [component declarations](#component-and-storage-declaration) | [bone motion](#bone-motion) | [records](#record)

## The model
Every registration is an entry: a kind, a name, and operations on its components. A builder method is a shortcut, `.per_cube(6)` on an ore is `.set(&comps::feature::COUNT, 6)` (and the spread it implies), so anything a shortcut doesn't cover is still reachable with the raw calls every builder has:

| call | what it does |
|---|---|
| `.set(&C, v)` | a value; `.set_text(&C, "name")` for one written as text (block and item names, `BlockRef`, `BlockList`) |
| `.unset(&C)` | takes a value away, in an edit |
| `.tag("t")`, `.untag("t")` | a tag on, or off |
| `.push(&L, x)` | one more element at the end of a list |
| `.insert_before(&L, "anchor", x)`, `.insert_after(..)` | an element next to another |
| `.replace_item(&L, "anchor", x)`, `.remove_item(&L, "anchor")` | swaps or takes out one element |

The anchor is an element's `name` field (a text element's text), or `#index`; elements without a `name` field (biome tints, game mode modifiers, template blocks, daylight points) only go by `#index`.

`comps::<kind>::X` handles are typed by their kind, so `comps::ore::VEINS` builds on an ore and not on a block. Your own `Component::named(..)` and the shared ones (`HARDNESS`, `LIGHT`, `MAX_STACK`...) go on anything; most kinds keep such extra values in their component table, readable with `C.on(entry_kind::WEATHER, id)` for any of them, or `C.on_block(..)`, `on_item`, `on_entity`, `on_biome`, `on_dimension`. Recipes, loot, ores, scatters, decorations, structures, spawn rules, terrains, sky objects, screen parts, word sets, component declarations, tint kinds and creative tabs have none: what they register isn't what the game numbers, or nothing reads them by id.

| what | how | notes |
|---|---|---|
| new | `r.block(&X)`, `r.ore(..)`... | once per name, a second registration is a load error |
| edit | `r.edit(&X)` | only what you set changes; waits for a later owner in load order |
| replace | `r.replace(&X)` | from scratch, nothing of theirs stays; of several, the last loaded wins |
| remove | `r.remove(&X)` | as if nobody had registered it; works on every kind |
| edit by pattern | `r.edit_pattern::<kind::Block>("#logs")` | `#tag`, a name with one `*`, `name[prop=v]`; after the edits by name. Relative names are your mod's |

`r.set(what, &C, v)` and `r.tag(tag, &[what])` are pattern edits of blocks and items at once. For blocks and items a pattern matching nothing is a load error (a `#tag` one when no such tag exists); for other kinds it is fine.

Defaults below are what the engine uses where nothing is set, so an edit only needs what it changes.

## Block
A block and its states; its item comes along unless `.no_item()`. Entry point `r.block(&BLOCK)`, edit `r.edit(&BLOCK)`. Shapes of other blocks: `r.stairs(..)`, `r.slabs(..)`, `r.vertical_slabs(..)`, `r.shapes_of(name, sources, suffix)`. A layer against the sides of its cell, several in one block: `.on_sides(Facings::ALL)` (properties `north`..`down`).

Block-level components (`comps::block`):

| component | schema | default | what |
|---|---|---|---|
| `PROPS` | `[BlockProp]` | one state | properties; every combination is a state (`.prop`, `.facing`, `.joins`...) |
| `ON_USE`, `ON_NEIGHBOR`, `ON_SCHEDULED`, `ON_CARRIED`, `ON_GROW` | callback | none | `.on_use`, `.on_neighbor`, `.on_scheduled`, `.on_carried`, `.on_grow` |
| `PLACEMENT`, `MERGE` | callback | none | the state placed (`.placement`, `.facing`), its own item placed into its cell (`.merge`) |
| `ON_SIGNAL`, `SIGNAL_GATE`, `SIGNAL_READ` | callback | none | redstone: `.on_signal`, `.signal_gate`, `.signal_read` |
| `OPENS` | `str` | none | the menu using it opens (`.opens`) |
| `SHARES_CELL` | `match` | none | which blocks may share its cell (`.shares_cell_with`) |
| `MODEL_FILE` | `str` | `models/block/<name>.json` | another model file (`.model_file`) |
| `MODEL_TEXTURES` | `[ModelTexture]` | none | what goes where the file says `#name` (`.model_texture`) |
| `RETEXTURE` | `[Retexture]` | none | texture renamed on its states and item (`.retexture`) |
| `SHAPES_OF` | `ShapesOf` | none | makes it a template (`r.shapes_of`) |

Shared components it carries per state: `HARDNESS` (`f32`, 1; `.hardness`, `.unbreakable`), `LIGHT` (`[u8;3]`; `.light`, `.light_rgb`), `FRICTION` (0.6), `DRAG` (1), `TINT` and `TINT_FACES` (from the faces' layers, `.tint_kind`), `HARVEST_LEVEL` (`.mined_by`), `LOOT_ROLLS`, `SPREADS_TO` and the other spread ones (`.spreads`, `.spread_reach`), `AMBIENT_PARTICLE`, `SHADER`, `SOUND` (`.sound_group`).

Tags it carries: `mineable/<class>` (`.mined_by`), `tab/<tab>` (`.tab`), `plantable` (`.plant`), and anything you tag.

```rust
static FURNACE: Block = Block::named("base:furnace");
r.edit(&FURNACE).hardness(5.0).push(&comps::block::SHARES_CELL, "#plantable".to_string());
r.edit_pattern::<kind::Block>("#logs").set(&FUEL, 300);
r.remove(&Block::named("base:mossy_cobblestone"));
```

Item, loot and block entity are separate entries the builder sends with the block: `.no_item()`, `.item_texture(..)`, `.drops(..)`, `.entity(|e| ..)`. A remove takes the item along, not the loot table or the block entity.

## Block states
Not an entry of its own: the parts of a block. The engine builds the states from `PROPS`, then puts on each the block's operations and those of every part whose key (`prop=value,..`) fits it, in order. `.states(|p, s| ..)` makes a part per combination of the builder's own properties, `.state_where("lit=true", |s| ..)` one part by key, any mod's properties.

Per-state components (`comps::block`):

| component | schema | default | what |
|---|---|---|---|
| `LIGHT_FILTER` | `[u8;3]` | none | absorption of passing light, colored glass (`.light_filter`) |
| `LIGHT_OPACITY` | `u8` | 15 opaque, else 0 | light it takes away (`.light_opacity`) |
| `LAYER` | `u32` (`RenderLayer`) | cutout for boxes, else opaque | `.cutout`, `.translucent` |
| `MODEL` | `u32` (`BlockModel`) | cube | `.plant`, `.air`, boxes |
| `TEXTURES` | `[str;6]` | `<ns>:block/<name>` | by face, each `LAYERS` text: `.texture`, `.top`, `.sides`.., `.layer`, `.tint`, `.tint_kind` |
| `TURNS` | `[u8;6]` | none | quarter turns of a cube's faces (`.face_turns`) |
| `BOXES` | `[ModelBox]` | none | `.model_box`, `.model_box_faces` |
| `TURN` | `u8` (`abi::Turn`) | none | how all of its model is turned (`.orientation`, by itself with `.facing`, `.axis`, `attach` and `front`) |
| `HITBOX`, `OUTLINE` | `[[f32;6]]` | the boxes | `.hitbox`, `.outline` |
| `SIGNAL` | `SignalDesc` | none | set by the `signal_*` presets |
| `LIQUID_WALLS` | `u8` | sides the hitbox covers | `.liquid_walls` |
| `PIECES` | `[Piece]` | none: a hit breaks all of it | `.piece` |
| `RULES` | `[BlockRule]` | none | what happens to it by itself: `.rule`, and `.grows`, `.smothered_into`, `.needs_below`, `.needs`, `.needs_near`, `.falling` |

Tags per state (`abi::comp::block`): `NOT_SOLID`, `REPLACEABLE`, `CLIMBABLE`, `NOT_TARGETABLE`, `CULL_SAME`, `IMMOVABLE`, `SIGNAL_BLOCKER`, `STICKY`, `NOT_STOOD_ON`. Presets set them (`.not_solid()`, `.replaceable()`...), `.tag(..)` sets any.

```rust
static LAMP: Block = Block::named("base:redstone_lamp");
r.edit(&LAMP).state_where("power=on", |s| s.light_rgb(15, 10, 4));
r.edit_pattern::<kind::Block>("base:furnace[lit=true]").light(15);
```

## Item
Something that goes in an inventory. `r.item(&ITEM)`; a block's item is named like the block.

| component (`comps::item`) | schema | default | what |
|---|---|---|---|
| `TEXTURE` | `str` | `<ns>:item/<name>`, a block's model for a block item | `.texture` |
| `BLOCK` | `str` | none | the block it places (`.places`) |
| `ON_USE` | callback | none | `.on_use` |
| `CONDITION` | callback | none | how a stack looks by its data (`.condition`) |

Shared ones: `MAX_STACK` (64), `DURABILITY`, `TOOL_LEVEL` + `tools/<class>` (`.tool`), `MINING_SPEED` (1), `ATTACK_DAMAGE` (1), `ATTACK_SPEED`, `USE_TICKS`, `USE_PRIORITY`, `INVENTORY_SLOTS` (`.inventory(n)`, a backpack), `LIGHT` (`[u8;3]`; `.light`, `.light_rgb`), `WORN`/`WORN_TEXTURE` (`.wearable`), `WORN_MODEL` (`.wearable_model`), `ANIMATIONS`, `REMAINDER`, `LIQUID`, plus what mods read (`FOOD`, `FUEL`). Tags: `handheld`, `tab/<tab>`, anything yours.

Light: `LIGHT` on an item lights what is around whoever holds or wears it, or where it lies dropped. An item that places a block uses the block's light unless it sets its own.

```rust
static COAL: Item = Item::named("base:coal");
r.edit(&COAL).max_stack(16).set(&FUEL, 2000);
static APPLE: Item = Item::named("base:apple");
r.replace(&APPLE).texture("golden_apple");
```

## Entity
A kind of entity: mobs, players, projectiles, moving platforms, block entities. `r.entity(&KIND)`, the engine's `PLAYER` and the rest editable like any.

| component (`comps::entity`) | schema | default | what |
|---|---|---|---|
| `SIZE` | `BodySize` | 0.6 by 1.8 | `.size` |
| `EYE` | `f32` | 0.85 of the height | `.eye_height` |
| `SPEED` | `f32` | 3 | blocks per second walking (`.walk_speed`) |
| `CARRY_HEIGHT` | `f32` | 0 | riders this high above a solid body move with it (`.carry_height`) |
| `RIDE_CAMERA` | `f32` | by its size | third person distance riding it (`.ride_camera`) |
| `TEXTURE`, `TEXTURE_SIZE` | `str`, `[u32;2]` | `<ns>:entity/<name>`, 64 by 32 | skin (`.texture`, `.model`) |
| `PARTS` | `[ModelPart]` | none | model in code (`.model`); a model file wins |
| `BONE_MODELS` | `[BoneModel]` | none | models on bones (`r.bone_model`) |
| `CATEGORY` | `str` | `core:misc` | `.category`, `.hostile`, `.passive` |
| `TEMPLATE` | `str` | none | a moving body of a template (`.body`) |
| `BLOCK` | `str` | none | the block it is the block entity of |
| `SEATS` | `[Seat]` | none | `.seat` |
| `ATTACH` | `[Attach]` | none | components each one carries, starting as the kind's own value (`.attach`, `.attach_with`, `.attach_storage`) |
| `LIGHT_AT` | `[f32;3]` | its middle, a block entity's block | where its light comes from, from its feet, +Z ahead, turned with it (`.light_at`) |
| `SHOWN_SLOTS` | `[u32]` | none | inventory slots shared as public stacks (`.shown_slots`, `Client::displayed`) |
| `ON_TICK`, `ON_INTERACT`, `STEER`, `DISMOUNT` | callback | none | `.on_tick`, `.on_interact`, `.steering`, seat exits |
| `FRAMES` | `[callback]` | none | client frame hooks (`r.client_entity_frame`) |
| `SLOT_GROUPS`, `INVENTORY_MENU` | `[SlotGroupDef]`, `str` | 9 + 27 | the player's inventory (`r.player_slots`, `r.player_menu`) |

Shared ones: `MODEL`, `ANIMATIONS`, `SOUND`, `SHADER`, `SWIMS_IN`, `INVENTORY_SLOTS`, `LIGHT` (`[u8;3]`; `.light`, `.light_rgb`), `HAZARDS` (`.hazard(..)`, `.burns_in_sun()`, `.crushing()`), attributes (`.max_health`, `.reach`). Tags (`abi::comp::entity`): `NO_GRAVITY`, `SWIMS`, `SOLID`, `KINEMATIC`, `RIDERS_FALL`, `WEARS` (`.equipment()`), and `NO_FALL_DAMAGE`, `UNPUSHABLE` turning off what every kind does; `immune/<cause>` against a damage cause.

Light: `LIGHT` lights what is around each entity of the kind, from its middle (`LIGHT_AT` moves it: a lantern on the head). One entity's own changes while it lives: attach `LIGHT` (it starts as the kind's) and `w.set_component(e, &LIGHT, [r, g, b])`. Everything an entity gives (its own light, both hands, worn items, `SHOWN_SLOTS`, a dropped stack, its effects, fire as `core:burning`'s) is combined per channel by taking the brightest value, not the sum. Light that moves with entities is drawn by each client: it spreads as block light does (walls stop it, colored glass tints it) and follows the entity block by block, but the world doesn't have it, so it keeps no mob from spawning and grows no plant. The 64 sources nearest the camera are drawn.

```rust
static ZOMBIE: EntityKind = EntityKind::named("base:zombie");
r.edit(&ZOMBIE).walk_speed(4.0).unset(&comps::entity::HAZARDS);
r.edit(&PLAYER).attach(&MANA).max_health(30.0);
```

The spawn egg (`<name>_spawn_egg`) is an item sent with a new kind; loot is `ZOMBIE.loot()`.

## Biome
A biome's climate, surface and colors; terrain rules placing it start from these. `r.biome(&BIOME)`.

| component (`comps::biome`) | schema | default | what |
|---|---|---|---|
| `RANGES` | `[RuleRange]` | everywhere | `.temperature(..)`, `.humidity(..)`; a field given again replaces its range |
| `HEIGHT` | `[i32;2]` | any | surface heights it fits |
| `TOP`, `FILLER`, `DEPTH` | `str`, `str`, `i32` | stone, 3 | `.surface(top, filler, depth)` |
| `UNDER`, `UNDER_LAYERS` | `str`, `i32` | none, 0 | `.under` |
| `DEEP_TOP`, `DEEP_BELOW` | `str`, `i32` | none | `.deep_top` |
| `BARE_ABOVE` | `i32` | none | `.bare_above` |
| `ICE` | `str` | none | `.frozen` (also tags `frozen`) |
| `TINTS` | `[BiomeTint]` | each kind's default | `.tint(&kind, color)`, a kind given again wins |

Shared: `PRECIPITATION` (`.precipitation`, `.snows`), scatter densities, anything yours. Tags: `animals`, `dry`, `spawn_biome`, yours.

```rust
static PLAINS: Biome = Biome::named("base:plains");
r.edit(&PLAINS).tint(&tints::GRASS, 0x80c040).tag("animals");
```

## Dimension
A separate world. `r.dimension(&DIM)`.

| component (`comps::dimension`) | schema | default | what |
|---|---|---|---|
| `GENERATOR` | callback | none | `.generator` |
| `SPAWN` | callback | open ground near 0, 0 | `.spawn_point` |
| `COLUMN` | callback | none | `.column` |
| `CUBES` | `[i32;2]` | unbounded | `.height_cubes` |
| `SKY_ABOVE` | `i32` | unknown | `.sky_above` |
| `GRAVITY` | `f32` | 32 | `.gravity` |
| `DAY_LENGTH`, `FIXED_TIME` | `u32` | 24000, 6000 | `.day_length`, `.fixed_time` |
| `DAYLIGHT` | `[DaylightPoint]` | vanilla's curve | `.daylight` |
| `SKY_COLOR`, `FOG_COLOR` | `u32` | | `.sky_color(sky, fog)` |
| `AMBIENT` | `u32` | 0 | `.ambient_light` |
| `WEATHERS` | `[str]` | always clear | `.weathers(..)` sets the whole list |

Tags (`abi::comp::dimension`): `NO_SKY_LIGHT`, `PLAYER_SPAWN` (`.player_spawn`), `NO_BLOCK_UPDATES`, `BARE`.

```rust
r.edit(&base_api::OVERWORLD).gravity(28.0).push(&comps::dimension::WEATHERS, ACID_RAIN.name().into());
```

## Terrain
A dimension's terrain from parts, named like the dimension. `r.terrain(&DIM)`, another mod's `r.edit(DIM.terrain())`. Details in the [worldgen guide](worldgen.md#terrain).

| component (`comps::terrain`) | schema | default | what |
|---|---|---|---|
| `STONE` | `str` | | `.stone` |
| `BASE_HEIGHT` | `f64` | 64 | `.base_height` |
| `SEA`, `CAVES` | `TerrainSea`, `TerrainCaves` | none | `.sea`, `.caves` |
| `FIELDS` | `[TerrainField]` | temperature, humidity, continent | `.field`; one named again replaces the earlier |
| `LANDFORMS` | `[Landform]` | none | `.landform`, `.landform_remove` |
| `MASKS` | `[LandformMask]` | none | `.landform_mask` |
| `RULES` | `[BiomeRule]` | none | `.rule`, `.rule_before`, `.rule_after`, `.rule_replace`, `.rule_remove` |
| `RULE_EDITS` | `[BiomeRule]` | none | `.rule_edit`: what it sets replaces that of the rule of its name |
| `CAVE_BIOMES` | `[CaveBiome]` | none | `.cave_biome`, `.cave_biome_remove` |

```rust
r.edit(OVERWORLD.terrain()).rule_before("base:desert", terrain::BiomeRule::new("scorched")).landform_remove("base:mountains");
```

## Menu and screen
A set of slots the server runs plus the screen showing it; a screen without slots is a menu that stays on the client. `r.menu(&MENU)`. Engine screens (`screens::*`) are replaced by registering their handle with `r.menu`, not edited.

| component (`comps::menu`) | schema | what |
|---|---|---|
| `GROUPS` | `[SlotGroupDef]` | `.group(name, Slots::..)`, `.player_inventory()`, `.equipment()` |
| `RECIPES` | `[str]` | recipe kinds its arrows open (`.recipes`) |
| `ON_CHANGE`, `ON_TAKE`, `ON_TICK` | callback | server side (`.on_change`, `.on_take`, `.on_tick`) |
| `SCREEN` | callback | client: builds the screen (`.screen`, `.default_screen`) |
| `ON_KEY` | callback | keys while it is on top (`.on_key`) |

Tags (`abi::comp::menu`): `menu.pauses`, `menu.keep_on_escape`, `menu.closes_on_respawn`, `menu.keys_first`, `menu.shows_chat` (`.pauses()`, `.keep_on_escape()`..., `.tag(..)` for the rest).

```rust
static CHEST: Menu = Menu::named("base:chest");
r.edit(&CHEST).group("trash", Slots::single()).tag("menu.pauses");
```

## Feature
Anything generation puts into the world after the terrain. `r.feature(&F)`, or the presets `r.ore(&BLOCK)` (named like the block, `BLOCK.ore()`), `r.structure(&TEMPLATE)` (named like the template, `TEMPLATE.structure()`), `r.scatter(&F)` and `r.decoration(f)` (`<mod>:decoration`, `/2`.. unless `.named`). Placement (`comps::common`, `Placed`): `DIMENSIONS` (`[str]`, `.dimension`, pushes), `BIOME` (`str` tag, `.biome`), `Y` (`[i32;2]`, `.y`). Details in the [worldgen guide](worldgen.md#features).

| component (`comps::feature`) | schema | default | what |
|---|---|---|---|
| `STAGE` | `u32` | by spread | `abi::stage`, `.stage` |
| `AFTER`, `BEFORE` | `[str]` | | `.after`, `.before` |
| `SPREAD` | `u32` | per cube | `abi::spread`: `.per_cube`, `.spacing`, `.generate` |
| `COUNT`, `HEIGHT` | `u32`, `HeightCurve` | 4, even | per cube (`.per_cube`, `.height(Height::..)`) |
| `SPACING`, `CHANCE`, `DENSITY` | `i32`, `f32`, `str` | 32, 1, none | on a grid (`.spacing`, `.chance`, `.density`) |
| `GROUND`, `MAX_SLOPE`, `BURIED` | `match`, `i32`, `i32` | any, any, 0 | `.ground`, `.max_slope`, `.buried` |
| `OVER_WATER`, `OVER_CAVES` | tags | | `.over_water`, `.over_caves` |
| `PRIORITY`, `SEPARATION` | `i32` | 0, 0 | `.priority`, `.separation` |
| `GENERATE` | callback | | on every cube (`.generate`) |
| `SHAPE` | `u32` | from what is set | `abi::shape` |
| `VEIN`, `SIZE`, `STEPS` | `VeinShape`, `u32`, `[ShapeStep]` | a walk, 8 | `.vein(Vein::..)`, `.vein_size` |
| `TEMPLATE`, `TRANSFORMS`, `PALETTES` | `str`, `u32`, `[str]` | , as drawn, none | `.template`, `.turns`, `.turns_and_mirrors`, `.palette` |
| `PLAN`, `REACH` | callback, `[i32;3]` | , 0 32 16 | `.plan`, `.reach`, `.reach_y` |
| `BLOCK`, `REPLACES`, `REPLACES_WITH`, `EXPOSED` | `str`, `match`, `[Variant]`, `f32` | its name, any, , 0 | `.block`, `.replaces`, `.replaces_with`, `.exposed` |
| `FOUNDATION`, `CLEAR_ABOVE` | `str`, tag | none | `.foundation`, `.foundation_of_ground`, `.clear_above` |

```rust
static IRON_ORE: Block = Block::named("base:iron_ore");
r.edit(IRON_ORE.ore()).per_cube(12).exposed(0.3);
r.edit(WELL.structure()).spacing(96).turns_and_mirrors();
r.remove(&Feature::named("base:decoration"));
```

## Liquid
How a liquid flows and looks from inside, named like its 16-state block. `r.liquid(&BLOCK)`, another mod's `r.edit(BLOCK.liquid())`; `.block(|b| ..)` makes or edits the block.

| component (`comps::liquid`) | schema | default | what |
|---|---|---|---|
| `DELAY` | `u32` | 5 | ticks per step (`.flow_delay`) |
| `DROP_OFF` | `u32` | 1 | `.drop_off` |
| `SLOPE_FIND` | `u32` | 4 | `.slope_find` |
| `RENEWABLE` | `u32` | never | `.renewable(n)` |
| `DENSITY` | `i32` | 1000 | negative rises (`.flows_up`) |
| `HEAT` | `u32` | 300 | from 1000 it burns (`.heat`) |
| `FOG`, `OVERLAY` | `LiquidFog`, `LiquidOverlay` | from its texture | `.fog`, `.overlay`, `.no_overlay` |

```rust
r.edit(WATER.liquid()).fog(0x204020, 12.0).block(|b| b.light(2));
```

## Reaction
What happens where two liquids meet, named `<mod>:<liquid>><touching>` (`#` and namespaces dropped) unless `.named`. `r.liquid_reaction(liquid, touching)`.

| component (`comps::reaction`) | schema | what |
|---|---|---|
| `LIQUID`, `TOUCHING` | `match` | which blocks, the two sides |
| `SOURCE`, `FLOWING` | `str` | what the liquid turns into (`.source_turns_into`, `.flowing_turns_into`) |
| `FALLS_INTO` | `str` | what the touching block becomes under it (`.below_turns_into`) |

```rust
static HOT_WATER: Reaction = Reaction::named("base:hot>water");
r.edit(&HOT_WATER).source_turns_into("base:stone");
```

## Template
A fixed shape of blocks for features, plans, moving bodies and `w.place_template`. `r.template(&T)` with `.layer`, `.key`, `.fill`, or a mod's file `structures/<path>.json` (or `.wst`), which is template `<mod>:<path>` by itself; an entry of that name adds to it. Its blocks are checked when the game loads.

| component (`comps::template`) | schema | what |
|---|---|---|
| `BLOCKS` | `[TemplateBlock]` | from its lowest corner; a block, a state, `core:air`, or a slot `@wall`, `@wall:stairs[..]`, `@ground` |
| `SLOTS` | `[PaletteSlot]` | what each slot takes unless a palette says (`.slot`) |
| `ENTITIES` | `[TemplateEntity]` | `.entity(&KIND, pos)`, or saved |
| `LOOT` | `[TemplateLoot]` | a block entity filled when placed (`.loot(pos, &TABLE)`) |
| `DATA` | `[TemplateData]` | a block entity as it was saved |
| `JOINTS` | `[TemplateJoint]` | where `Plan::grow` joins pieces (`.joint`) |

```rust
r.edit(&WELL).fill("base:lantern", [1, 3, 1], [1, 3, 1]);
```

## Palette
The blocks templates' slots take where a feature uses it. `r.palette(&P).slot("wall", "base:bricks")`; `SLOTS` (`[PaletteSlot]`, `comps::palette`). A slot `@wall:stairs` takes the stairs `shapes_of` made of the palette's block.

## Spawn rule
Where a mob shows up by itself, named `<mod>:<entity>`, `/2`.. unless `.named`. `r.natural_spawn(&KIND)`, placement as for ores.

| component (`comps::spawn`) | schema | default | what |
|---|---|---|---|
| `ENTITY` | `str` | | the mob |
| `PLACE` | `u32` | ground | `.place` |
| `GROUP` | `[u32;2]` | 1, 1 | `.pack_size` |
| `WEIGHT` | `u32` | 1 | `.weight` |
| `CHANCE` | `f32` | 1 | `.chance` |
| `WHEN` | `[str]` | none | conditions, all of which hold for the spot and the block it rests on (`.when`, `.light` is `light(a..b)`, `.ground` and `.place(SpawnPlace::in_liquid(..))` are `block(..)`) |

```rust
static SPIDER: SpawnRule = SpawnRule::named("base:spider");
r.edit(&SPIDER).weight(8).light(0, 4);
```

## Recipe
Named `<mod>:<kind>/<output>`, `/2`.. unless `.named`. `r.shaped(..)`, `r.shapeless(..)`.

| component (`comps::recipe`) | schema | what |
|---|---|---|
| `KIND`, `OUTPUT`, `COUNT` | `str`, `str`, `u32` | kind, result, how many (1) |
| `PATTERN`, `KEYS` | `[str]`, `[RecipeKey]` | shaped |
| `INPUTS`, `AMOUNTS` | `[str]`, `[u32]` | shapeless or processing (`.input_amounts`) |
| `TIME` | `u32` | ticks, 0 |
| `OUTPUTS` | `[LootPool]` | more results, rolled as loot (`.extra`, `.output`, `.output_chance`) |

Tag `recipe.hidden` (`.hidden()`). Any other component is data it carries (energy per tick).

```rust
r.remove(&Recipe::named("base:crafting/torch"));
static SMELTING_ALL: Recipe = Recipe::named("base:smelting/*");
r.edit(&SMELTING_ALL).set(&comps::recipe::TIME, 100);
```

## Recipe kind
A kind of recipe and how the recipe viewer shows it. `r.recipe_kind(&KIND)`.

| component (`comps::recipe_kind`) | schema | what |
|---|---|---|
| `INPUTS`, `OUTPUTS` | `[u32;2]` | grid sizes (`.inputs`, `.outputs`) |
| `INFO`, `INFO_VALUES` | `str`, `[str]` | a line under each recipe (`.info_line`) |
| `LAYOUT` | callback | each recipe's own layout (`.layout`) |

```rust
r.edit(&base_api::SMELTING).info_line("recipe.mymod.smelting.info", &["time"]);
```

## Loot table
What a block or entity drops, named like it, or a table of its own. `r.loot(&SOURCE)`, another mod's `r.edit(BLOCK.loot())`.

| component (`comps::loot`) | schema | what |
|---|---|---|
| `POOLS` | `[LootPool]` | `.pool(Pool::new()..)` pushes, `.clear()` empties |
| `SWAPS` | `[LootSwap]` | `.replace_drop(from, to)`, `.remove_drop(item)` over every pool |

```rust
r.edit(ZOMBIE.loot()).replace_drop("base:rotten_flesh", "mymod:brain").remove_item(&comps::loot::POOLS, "#1");
```

## Particle
A kind of particle; particles live on the GPU of each client. `r.particle(&P)`.

| component (`comps::particle`) | schema | default | what |
|---|---|---|---|
| `TEXTURES` | `[str]` | a piece of the block | `.textures` |
| `LIFE` | `[f32;2]` | 0.5..1 | `.life` |
| `SIZE` | `[f32;2]` | 0.1..0.2 | `.size` |
| `GRAVITY` | `f32` | 0 | `.gravity` |
| `DRAG` | `f32` | 0.98 | `.drag` |
| `COLOR` | `[u32;2]` | white | `.color(start, end)` |
| `SHADER` | `str` | none | `.shader` |
| `RULES` | `[ParticleRule]` | none | `.on_land`, `.on_wall`, `.on_ceiling`, `.on_enter_liquid`, `.on_leave_liquid`, `.on_expire` push |

Tags: `particle.collide`, `particle.fullbright`, `particle.translucent`, `particle.shrink` (`.colliding()`, `.fullbright()`...).

```rust
static POOF: Particle = Particle::named("core:poof");
r.edit(&POOF).color(0x8080FFFF, 0x2020FF00);
```

## Sky object
A sun, moon, stars, clouds or skybox of one dimension, named `<dimension>/<object>`. `r.sky_body`, `r.sky_stars`, `r.sky_dome`, `r.sky_clouds`, each `(&DIM, &OBJECT)`; another mod's `DIM.sky(&OBJECT)`. Details in the [sky guide](sky.md).

`comps::sky`: `KIND` (`u32`, `sky_kind`, body), `TEXTURE`, `SHADER` (`str`), `PHASE`, `ORBIT`, `TILT`, `HEADING`, `SIZE`, `SPIN` (`f32`), `OPACITY` (`[f32;2]`), `COLOR`, `GLOW` (`u32`), `FRAMES` (`[u32;2]`), `FRAME_TICKS`, `COUNT`, `SEED` (`u32`), `HEIGHT`, `DRIFT` (`f32`), `ORDER` (`i32`); tag `sky.additive` (`.blend_add()`). Each has the `SkyBuilder` method of its name.

```rust
r.edit(OVERWORLD.sky(&MOON)).size(40.0);
r.remove(OVERWORLD.sky(&STARS));
```

## Sound
A sound event. `r.sound(&S)`.

| component (`comps::sound`) | schema | default | what |
|---|---|---|---|
| `FILES` | `[str]` | | `.files`, one at random |
| `VOLUME` | `f32` | 1 | `.volume` |
| `PITCH` | `[f32;2]` | 1 | `.pitch` |
| `RANGE` | `f32` | 16 | `.range` |
| `FULL_WITHIN`, `FALLOFF` | `f32`, `i32` | 1, even | `.falloff(full_within, Falloff::..)` |
| `CATEGORY` | `str` | `core:blocks` | `.category` |

```rust
static UI_ADVANCEMENT: Sound = Sound::named("base:ui.advancement");
r.edit(&UI_ADVANCEMENT).push(&comps::sound::FILES, "mymod:crack".to_string());
```

## Music
Background music, the first whose conditions hold. `r.music(&M)`. `comps::music`: `TRACKS` (`[str]`, `.tracks`), `DIMENSION` (`str`, `.dimension`), `WHEN` (`[str]` conditions, `.when` pushes).

```rust
static CALM: Music = Music::named("base:calm");
r.edit(&CALM).push(&comps::music::TRACKS, "mymod:music/calm".to_string());
```

## Weather
`r.weather(&W)`, the engine's in `weathers::*`; a dimension lists which come.

| component (`comps::weather`) | schema | what |
|---|---|---|
| `FALLS`, `SKY_GRAY`, `SKY_COVER`, `FOG_GRAY`, `CLOUD_GRAY`, `CLOUD_DARK`, `DARKNESS` | `f32` 0..1 | its look, rain's unless set (`.gray`, `.darkness`) |
| `SOUND` | `str` | `.sound` |
| `DURATION` | `[u32;2]` | `.duration` |
| `WEIGHT` | `u32` | `.weight` |

Tags: `weather.dry` (`.no_falling()`), `weather.lightning` (`.lightning()`), `weather.unsheltered` (`.no_shelter()`).

```rust
r.edit(&weathers::RAIN).darkness(0.4).set(&comps::weather::CLOUD_DARK, 0.6);
```

## Precipitation
What falls in a biome. `r.precipitation(&P)`. `comps::precipitation`: `TEXTURE` (`str`, `.texture`), `SPEED`, `DENSITY` (`f32` shares of rain's, `.speed`, `.density`).

```rust
r.edit(&precipitations::SNOW).speed(0.2);
```

## Pose
A way to hold a body. `r.pose(&P)`, the engine's in `poses::*`. `comps::pose`: `HEIGHT`, `EYE`, `SPEED` (`f32`, shares of standing, 1), `TILT` (`u32`, `pose_tilt`), `ACTION` (`str`), `PRIORITY` (`i32`), `KEY` (`str`), `FALLBACK` (`[str]`, `.fallback` sets the list), `WHEN` (`[str]` conditions, `.when(Cond::in_liquid())`); tag `pose.no_bob` (`.no_bob()`).

```rust
r.edit(&poses::CROUCH).speed(0.4);
```

## Game mode
A set of abilities. `r.game_mode(&M)`, the engine's in `game_modes::*`. `comps::game_mode::MODIFIERS` (`[Modifier]`, `.modifier` pushes; flight is `attribute::CAN_FLY`, no damage, hunger or air `attribute::INVULNERABLE`, `.invulnerable()`); tags `game_mode.instant_break`, `no_break`, `no_place`, `no_costs`, `always_fly`, `noclip`, `no_interact`, `hidden`, `no_hud`, `creative_inventory` (presets `.breaks`, `.flight`, `.no_placing`...).

```rust
r.edit(&game_modes::SURVIVAL).modifier(Modifier::add(&attribute::MAX_HEALTH, 4.0));
```

## Damage cause
`r.damage_cause(&C)`, the engine's in `damage_causes::*`. `comps::damage_cause`: `DEATH_MESSAGE`, `DEATH_MESSAGE_BY` (`str` keys); tags `damage.bypasses_armor`, `pierces_invulnerable`, `ignores_cooldown`, `burning`, `no_knockback`, `silent` (`.bypasses_armor()`...).

```rust
r.edit(&damage_causes::FALL).death_message("death.mymod.fell");
```

## Entity effect
What is on a mob for a while (`w.entity_effect(id, &E, level, ticks)`), several at once: a shader drawn over it, what it does to it and what it gives it, the last two times the level. With an icon it is a status effect its player sees (`docs/modding.md`, Status effects). `r.entity_effect(&E)`. `comps::entity_effect::SHADER` (`str`, its own name by default; `.shader`, `.unseen()` for none), `HAZARDS` (`.hazard(Hazard::own(..))`), `MODIFIERS` (`.modifier(..)`), `LIGHT` (`[u8;3]`, given off while on; `.light`, `.light_rgb`). The engine's `core:burning` draws nothing and is the light of every burning entity (`r.edit(&entity_effects::BURNING).light_rgb(..)` for another fire).

```rust
r.edit(&entity_effects::HURT).shader(&Shader::named("mymod:hurt_blue"));
r.entity_effect(&POISON).unseen().hazard(Hazard::own(1.0, &MAGIC).every(25));
r.entity_effect(&SWIFTNESS).modifier(Modifier::percent(&attribute::MOVEMENT_SPEED, 0.2));
```

## Text place
Defaults for text on the screen, and its HUD element. `r.text_place(&P)`, the engine's in `text_places::*`. `comps::text_place`: `ANCHOR` (`str`), `ORDER` (`i32`) (`.hud`), `AT` (`[f32;4]`, `.at`), `SCALE` (`f32`), `TIMES` (`[f32;3]`), `CLASS`, `SAMPLE` (`str`); tag `text_place.stacks` (`.stacks()`).

```rust
r.edit(&text_places::HINT).scale(1.5);
```

## Condition
One predicate for music, text, loot, spawning, poses and cameras (`abi::condition` has the language). `r.condition(&C, |c| ..)`, the engine's in `conditions::*`. Its callback is `abi::comp::CONDITION_CALL`; there is no builder, so it is registered or removed, not edited.

```rust
r.remove(&conditions::NIGHT);
```

## Camera
A camera mode. `r.camera(&MODE)`, the engine's in `cameras::*`. `comps::camera`: `KEY` (`str`, `.key`), `CALL` (callback, `.shot`), `TRANSITION` (`f32`), `EASING` (`u32`) (`.transition`), `WHEN` (`[str]` conditions the server checks for the player, `.when(Cond::attribute(&attribute::CAN_FLY, 1.0, f64::MAX))`); tags `camera.body`, `hand`, `hud`, `effects`, `clip`, `solid`, `cycle`, `detached`, `aim`, `no_interact` (a shot's own, `.in_cycle()`, `.tag(..)` and `.untag(..)`).

```rust
r.edit(&cameras::FRONT).untag(abi::comp::camera::CYCLE);
```

## Key
A key binding. `r.key(&KEY, "R")` with `.on_client`/`.on_server`, the engine's in `keys::*`. `comps::key::DEFAULT` (`str`, the binding); tags `abi::comp::key::MOVEMENT`, `HANDS`, `RIDING`, `DEBUG`, `SCREEN` say where it acts (none: anywhere in the world).

```rust
r.edit(&keys::CRAWL).set_text(&comps::key::DEFAULT, "KeyC");
```

## Config
A setting. `r.config(&C)` with `.bool`, `.int`, `.float`, `.choice`, `.text`, `.server()`, `.world()`; `r.text_config(&C, default)`. A `Config` isn't a handle, so another mod's setting is edited through `Handle::<kind::Config>::generated(name)`. `comps::config`: `KIND`, `SCOPE` (`u32`), `DEFAULT`, `STEP` (`f64`), `RANGE` (`[f64;2]`), `CHOICES` (`[str]`), `TEXT` (`str`); tag `config.restart_required`.

```rust
r.edit(Handle::<kind::Config>::generated("core:fov".to_string())).set(&comps::config::RANGE, [30.0, 130.0]);
```

## Attribute
A value of living entities, worked out as a chain from its base, its modifiers among the steps. `r.attribute(&A, default, min, max)`, the engine's in `attribute::*`. `comps::attribute`: `DEFAULT` (`f64`), `RANGE` (`[f64;2]`).

```rust
r.edit(&attribute::MAX_HEALTH).set(&comps::attribute::RANGE, [1.0, 2048.0]);
```

## Step
Code that changes a value (`docs/modding.md`, Hooking into the game). `r.step(&S)`, `r.modify(&chain, &phase, ..)` and `r.before_event(..)` for one without a name of yours; the engine's are named `core:..`. `comps::step`: `CHAIN` (`str`, run in every computation of it; none: only where a modifier names it), `PHASE` (`str`), `AFTER`, `BEFORE` (`[str]`, steps of its phase), `CALL` (`callback`, `.run`, `.answer`).

```rust
r.step(&SHARP).on(&attribute::ATTACK_DAMAGE).phase(&phase::ADD).after("othermod:blunt").run(|m| m.value += 1.0);
r.edit(&Step::named("core:charge")).run(|m| m.value *= 0.2 + 0.8 * m.query().charge as f64);
r.remove(&Step::named("core:vanilla_sweep"));
```

## Phase
A place in the order steps run in, right after another. `r.phase(&P, &after)`, the engine's in `phase::*`. `comps::phase`: `AFTER` (`str`).

```rust
r.phase(&CRITS, &phase::MULTIPLY);
```

## Equipment slot
A slot things are worn in. `r.equipment_slot(&S, covers, background)`. `comps::equipment_slot`: `COVERS` (`[str]` bones), `BACKGROUND` (`str`).

```rust
r.edit(&base_api::CHEST).push(&comps::equipment_slot::COVERS, "leftWing".to_string());
```

## Creative tab
`r.creative_tab(&TAB, &ICON)`; blocks and items join with `.tab(&TAB)` (tag `tab/<tab>`). `comps::creative_tab::ICON` (`str`).

```rust
r.edit(&base_api::tabs::FOOD).set_text(&comps::creative_tab::ICON, "base:bread");
```

## Sound and entity category
A sound category has its own volume slider: `r.sound_category(&C, world)`, tag `abi::comp::SOUND_CATEGORY_WORLD` (heard like block sounds). An entity category caps spawning: `r.entity_category(&C, cap, despawn)` with `.cap_radius`; `comps::entity_category`: `CAP` (`u32`, 0), `DESPAWN` (`f32`, never), `CAP_RADIUS` (`f32`, 64).

```rust
r.edit(&entity_categories::HOSTILE).set(&comps::entity_category::CAP, 12);
```

## Tint kind
A color blocks take from their biome. `r.tint_kind(&K, default)`, the engine's in `tints::*`. `comps::tint_kind::DEFAULT` (`u32`, white). Biomes give it with `.tint(&K, color)`.

```rust
r.edit(&tints::WATER).set(&comps::tint_kind::DEFAULT, 0x3050c0);
```

## HUD element
A part of the in-game overlay. `r.hud_element(&E, anchor, order, |ui| ..)`. `comps::hud_element`: `ANCHOR` (`str`, `.anchor`), `ORDER` (`i32`, `.order`), `CALL` (callback, `.ui`).

```rust
static FOOD: HudElement = HudElement::named("base:food");
r.edit(&FOOD).anchor(hud_anchor::TOP_RIGHT).order(2);
```

## Screen part
Something added to a place of a screen, named `<mod>:<screen>/<area>`, `/2`... `r.screen_part(&screens::TITLE, "buttons", 0, |ui| ..)`. Menus: a menu's own `corner` (top right inside its panel) and `side` (beside it), `r.screen_part(&base::CHEST_MENU, "side", ..)` for any mod's menu, and `screens::MENUS` `corner` and `side` for every menu (base's sort and put away icons are in its `corner`; sorting is `c.client().menu_sort(None)`). `comps::screen_part`: `SCREEN`, `AREA` (`str`), `ORDER` (`i32`), `CALL` (callback).

```rust
static PAUSE_BUTTONS: ScreenPart = ScreenPart::named("base:pause/buttons");
r.edit(&PAUSE_BUTTONS).set(&comps::screen_part::ORDER, 10);
```

## Word set
Words command parameters `word(#set)` complete from. `r.word_set(&S)` with `.words(..)`, `.remove_word(..)`; the engine's in `word_sets::*`. `comps::words::WORDS` (`[str]`).

```rust
r.edit(&word_sets::GIZMOS).words(&["mymod:pipes"]);
```

## Component and storage declaration
What a component is for things that hold their own value: `r.entity_component(&C, default, flags)` (entities), `r.stack_component(&C, flags)` (stacks), `r.instance_component(&C, default, flags)` (both), `r.storage(&S, slots)` (an extra inventory). Named like the component, `C.declaration()`. `comps::component`: `SCHEMA` (`str`), `DEFAULT` (`[u8]`), `SLOTS` (`u32`); tags per flag, `component.save`, `sync_owner`, `sync_watchers`, `sync_menu`, `keep_on_respawn`, `carried`, `server_only`, `no_save`, and `component.stack`.

```rust
r.edit(base_api::HUNGER.declaration()).untag("component.keep_on_respawn");
```

## Bone motion
A bone of a model file moving by itself, named `<model>/<bone>`. `r.bone_motion(model, bone, BoneMotion::..)`. `comps::bone_motion`: `GRAVITY`, `SPRING`, `DRAG` (`f32`, 0). There is no builder, so one bone takes one registration; `r.remove(Handle::<kind::BoneMotion>::generated(..))` takes another mod's away.

```rust
r.bone_motion("tailmod:tail", "tail2", BoneMotion::SPRINGY);
```

## Record
A mod's own data that other mods add to and change, the way they do the engine's: an API's structures, a machine's processing table, a quest. `r.record(&R)`, named like its handle; it has nothing of its own, only the tags and components set on it, and those say what it is. Another mod's `r.edit(&R)`, `r.replace` and `r.remove` work as on anything else, in load order, and an edit of a record nobody registered stops the load.

They are read once the game is loaded, never in `init`, since only then are everyone's edits in: `entries_with(entry_kind::RECORD, "structgen:structure")` lists the records with that tag or component, `CHANCE.on_record(id)` and `STRUCTURE.on_record(id)` read one, `Record::lookup(name)` finds one by name.

```rust
pub static STRUCTURE: Tag = Tag::named("structgen:structure");
pub static CHANCE: Component<f32> = Component::named("structgen:chance");
static HUT: Record = Record::named("hut");
r.record(&HUT).tag(STRUCTURE.name()).set(&CHANCE, 0.3);
// Another mod:
static THEIR_HUT: Record = Record::named("othermod:hut");
r.edit(&THEIR_HUT).set(&CHANCE, 0.05);
```
An API crate wraps this in builders of its own (`structgen_api::place(r, ..)` making the record and `.chance(..)` one `.set`), the way the SDK's builders are shortcuts for components.
