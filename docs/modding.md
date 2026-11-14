# Weft modding quick start

## Contents
1. [Introduction](#introduction)
2. [Preparation](#preparation)
3. [Creating a mod in two minutes](#creating-a-mod-in-two-minutes)
4. [Names and paths](#names-and-paths)
5. [How registering works](#how-registering-works)
6. [Components and tags as inter-mod communication](#components-and-tags-as-inter-mod-communication)
7. [Where to go next](#where-to-go-next)
8. [Debugging](#debugging)
9. [Things that make your and other modders' life easier](#things-that-make-your-and-other-modders-life-easier)

## Introduction
This is a guide describing how the base game itself is written.\
The game is a mod called `base`, which has no private hooks or priorities from the engine, so if base can do something,
your mod can too.\
When in doubt, check `mods/base` and `mods/testmod` and copy from there.\
testmod exists to prove you don't have to touch the engine.

## Preparation
1. Before starting working on Weft, you'll need workspace. The best recommendation I can give is using JetBrains RustRover, which is free and has everything needed.\
2. Clone the repo using built-in Clone Repository feature, where you'll put https://github.com/Weft-Rust/Weft as a link, which will create folder on your pc with the code.\
3. Then after finishing you need to configure Run Configuration. Find Run Configuration tab in top-right corner, and set the command to `run`, and call it whatever, after that the game will be launched from your local copy of the repo.

## Creating a mod in two minutes

Fastest way to begin is to copy `examples/examplemod`.\
It is a ready mod with one item, a setting and an event.\
Every kind of registration is written out there as a commented example with a pointer to where the base game uses it.\
Rename three things listed at the top of its `lib.rs` and you can begin creating your mod.\
The bare minimum looks like this:

```toml
# Cargo.toml
[package]
name = "mymod"            # your mod id: the library, the mod's folder and its lang keys go by it
version = "0.1.0"         # the version players see in the Mods screen
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
# you want to replace this path with the one to the SDK after copying this folder somewhere else
weft_sdk = { path = "../../crates/sdk" }

# only while the mod folder lies inside another cargo project (like this repository), see examples/examplemod/Cargo.toml
[workspace]
```

```rust
// lib.rs
use weft_sdk::*;

declare_mod!(deps: ["base"], init: init);

pub static RUBY: Item = Item::named("ruby");
pub static RUBY_ORE: Block = Block::named("ruby_ore");
static INGREDIENTS: CreativeTab = CreativeTab::named("base:ingredients");

fn init(r: &mut Registry) {
    r.block(&RUBY_ORE).hardness(3.0).mined_by("pickaxe", tool_tier::IRON).drops("ruby", 1);
    r.item(&RUBY).tag("gems/ruby").tab(&INGREDIENTS);
    r.ore(&RUBY_ORE).y(-64, 16).per_cube(2).vein_size(5).replaces("#stone_ore_replaceables");
}
```

Build with `cargo build --release`, then make a folder in the mods folder and copy the library from `target/release/` into it. That's `run/mods/` when you run the game from the repo; a released game keeps it in your user folders (see the README), and the Mods screen has a button that opens it.

```
run/mods/mymod/
    mymod.dll          (libmymod.so / libmymod.dylib elsewhere)
    assets/
        textures/block/ruby_ore.png
        textures/item/ruby.png
        lang/en_us.lang
```

`weft pack-mod run/mods/mymod` makes a `mymod.zip` of the same thing, which is what you can send to anyone.\
The folder has to hold the library that's what gets packed.

The library is built for one system, so that zip only runs on yours, but you write your mod once and just build it three times. Put `mymod.dll`, `libmymod.so` and `libmymod.dylib` in the folder and `pack-mod` takes them all, then the same zip works everywhere: the game picks the file of its system.\
Phones too: build for `aarch64-linux-android` with the Android NDK and put that `libmymod.so` in an `android` folder inside yours, since it has the same name as the Linux one. On the phone the zip goes into `Android/data/org.weft.game/files/mods`.\
`examples/examplemod/.github/workflows/release.yml` builds all three on GitHub and publishes the zip when you push a tag, the top of the file says what to set up.

To get your mod into the launcher, its source has to be public and its `Cargo.toml` has to take the SDK from the game's repository at a release tag, `weft_sdk = { git = "https://github.com/Weft-Rust/Weft", tag = "v26.11.3" }`. Commit, push, and run `weft submit-mod path/to/mymod`: it builds the mod once to be sure, then sends that commit to the [mod index](https://github.com/Weft-Rust/Weft-Mods-Index) as a pull request (with the GitHub CLI logged in, by itself; otherwise in the browser, filled in). The index builds the mod there for all three systems, and builds it again for each new Weft release. It shows as not verified until a maintainer reads the source. The id is yours from the first submission.

The mod's id and version come from `Cargo.toml`, and its name from the lang file (`mod.mymod.name`), so `declare_mod!` only lists the mods yours needs.\
To load after a mod without needing it, `declare_mod!(deps: ["base"], after: ["othermod"], init: init)`: if it is there, yours comes later and wins where both change one thing, else nothing happens (`before: [..]` the other way round).\
A folder or zip named differently from the crate stops the game at load, since the library inside wouldn't match.

A word on versioning: the mod interface is checked by a hash of its source. If your mod was built
using another engine version, it just refuses to load and tells you to rebuild.

## Names and paths
### Names
Everything a mod adds has a name `namespace:path`: blocks, items, entities, menus, biomes, dimensions,
settings, sounds, particles, events, keys, screens, HUD elements, rules, recipe kinds and so on.\
In your own mod you write just `ruby` and it becomes `mymod:ruby`, the same when you register it and when you read it.\
Other mods' things go by their full name: `base:furnace`. The engine's own are `core:..`.

### References
Wherever something is named as text (recipes, loot, what features replace and stand on, `.biome(..)`, dimension lists, `r.set`, support rules, spawn rules, `.ctm` files, `{block:..}` and `{item:..}`, commands), it is written the same way, whatever kind it is:
- `base:stone` is that thing, `base:furnace[lit=true]` the furnace's states with that value
- `#logs` is anything with the tag, `#ingots/*` anything with a tag starting with `ingots/`; components count too, so `#fuel` is anything that burns
- where one thing goes (a recipe's result, a loot line, a tab's icon, `{item:..}`) it takes a name, as a tag stands for several. A recipe made per material is the one exception: the result of `"#planks/{m}"` is the first item with that tag

Where a rule asks which blocks or items (a slot's `accepts`, support rules, `spreads`, `joins`, `shares_cell_with`, an ore's `replaces`, scatter and spawn `ground`, liquid reactions, `SWIMS_IN`, a fill's `keep` and `only`), it takes several of these apart by spaces, and `!` in front of one leaves out what it names: `"#plant_soil !base:sand"` is any plant soil but sand, `"!#liquids"` anything that isn't a liquid. The engine puts two tags on blocks itself for this: `#liquids` on every liquid's block and `#unbreakable` on blocks of negative hardness. A plain `#tag` no block has yet is fine there and takes nothing, since another mod may bring it; a name nothing has still stops the game. Biomes, dimensions and a dimension's weathers are named the same way: `.biome("#forest !base:birch_forest")`.

A name nothing has, or a tag pattern no tag matches, stops the game at load with your mod's name and what was wrong.\
Where one item has to come out of a tag (a recipe output, a loot entry), it's the preferred one: the first registered in mod load order.\
Recipes have one more form, a variable: `#ores/{m}` in and `#ingots/{m}` out makes one recipe per material that has both.

### Handles
The name lives in a handle, and you register by the handle:

```rust
static RUBY: Item = Item::named("ruby");
static DASH: Key = Key::named("dash");
static STONE: Block = Block::named("base:stone");   // someone else's, only read

r.item(&RUBY).max_stack(16);
r.key(&DASH, "R").on_server(|w, player| dash(w, player));
if STONE.is(w.block(pos)) { .. }
```

Every kind of thing works like this, so there is nothing else to remember.\
Once the game is loaded a handle gives you the numeric id (`RUBY.id()`), and hot code works only with ids.\
Families made in a loop take `Item::generated(format!("{metal}_ingot"))`, a handle made from a name at runtime.

### Textures and other assets
Every call that takes a file knows its folder: blocks look in `block/`, items in `item/`, mobs and worn armor in `entity/`, particles in `particle/`, slot backgrounds and `{img:..}` in `gui/`, sounds in `sounds/`.\
So `.texture("ruby")` for a block means `mymod:block/ruby`, for an item `mymod:item/ruby`, and `"base:ruby"` is the same folder of the base game.\
Subfolders are fine (`.wearable(.., "armor/ruby_1")` is `entity/armor/ruby_1`), but a name can't leave its folder: `"block/ruby"` for an item, `..` or an absolute path stop the game at load. An item that wants to look like a block gets a model, not the block's texture path.\
Resource packs replace any of them by putting a file at the same place.\
Textures can also be [animated, glow and have overlays](textures.md), or [be connected](ctm.md)).

### Translations
Format: `type.modname.name = value`, e.g. `item.examplemod.example_item = Example Item`\
Any item can have a tooltip using `type.modname.name.tooltip = Some tooltip`, more in the [localization guide](localization.md).\
If the player's language doesn't have a key (or a whole file), English is shown, so `en_us.lang` is the one you always need.\
The Mods screen reads your mod's page from there too: `mod.mymod.name`, `mod.mymod.description` (as long as you like, with pictures), `mod.mymod.authors` and `mod.mymod.url`.

### The mod's icon
`assets/textures/gui/icon.png`, a square PNG (16x16 is enough, bigger is fine, under 256 KB). The Mods screen shows it on your mod's tile and page, and the catalog takes it from the same place at the commit you submitted, so players see it before they download. Without one your mod gets its first letter on a color.

Text can carry colors and effects:
- `{#ff8080}This text is red{/}`
- `{fx:core:wave}wavy{/fx}` (new fx can be added from your mod too)
- `{on_key:!core:sneak}This text will appear when the bound key is NOT pressed{/on_key}`
- `{on_key:core:sneak}This text will appear only when the bound key is pressed{/on_key}`
- `{if:creative}This text will appear if the player is in creative mode{/if}` (any condition works, a mod's own as `{if:mymod:adventure}`, and put together: `{if:night & !creative}`)
- `{img:mymod:logo}` (from `gui/`), `{block:base:furnace}` and `{align:center}..{/align}` for pictures, blocks and centered lines
- `{name:item:base:coal}` for the name of anything players see named (items, blocks, liquids, tabs, mods, recipe kinds, settings, keys), the same name everywhere

All of it works in any text the game shows, chat and command replies included, so there is no second kind of text to learn.

## How registering works
Everything you register is an entry: a kind (block, item, ore, recipe, sky object, weather...), a name, and the components it is made of.\
Builder methods are shortcuts for components: `.hardness(3.0)` sets `hardness`, `.vein_size(6)` on an ore sets `comps::feature::SIZE`. `comps::<kind>` lists what the engine reads off each kind, and every builder has `.set(&C, v)`, `.unset(&C)`, `.tag("t")` and `.untag("t")`, so nothing is out of reach when a shortcut is missing.

Because it's all components, anything registered, yours, another mod's or the engine's, changes the same way:
- `r.block(&X)`, `r.ore(..)` and the rest register it once; the same name twice is a load error
- `r.edit(&X)` changes only what you set, the rest stays
- `r.replace(&X)` registers it again from scratch, nothing of the old one stays
- `r.remove(&X)` takes it out, as if nobody had registered it
- `r.edit_pattern::<kind::Block>("#logs")` is one edit of everything a pattern names: `#tag`, a name with `*`, or some states of a block, `base:furnace[lit=true]`

Lists (a dimension's weathers, a loot table's pools, a terrain's rules) take `push`, `insert_before`, `insert_after`, `replace_item` and `remove_item`, pointing at an element by its name or `#index`.\
The compiler checks that a component fits the kind: `comps::ore::VEINS` goes on an ore and won't build on a block, while your own `Component::named(..)` goes on anything.

```rust
static GLOWSTONE: Block = Block::named("base:glowstone");

r.edit(&GLOWSTONE).light(10);                                   // on every state, the rest stays
r.edit(&base_api::OVERWORLD).push(&comps::dimension::WEATHERS, ACID_RAIN.name().into());
r.remove(&Recipe::named("base:crafting/torch"));
r.edit_pattern::<kind::Block>("#logs").hardness(3.0);
```

Edits wait for what they edit, so load order only matters between two mods changing the same thing: the later one wins (`after`/`before` in `declare_mod!` decide it without depending on the other), and edits by pattern (`r.set` too) come after edits by name.
Every kind with all its components and an example of changing someone else's is in the [registration guide](registration.md).

## Components and tags as inter-mod communication

This is the most important part that you should remember to make life easier for you and other modders working with your mod.\
Properties are named values on content. Things like fuel, food, durability, etc etc, are components or tags.\
This is done so any mod reads and writes them without depending on whoever added a certain block.

```rust
// Declared where it's used, or taken from the mod that reads it (`base_api::FUEL`, see below)
static FUEL: Component<u32> = Component::named("fuel");
static HOT: Tag = Tag::named("hot");

r.item(&COAL_DUST).set(&FUEL, 800).tag("hot");      // on your own content
r.set("base:stick", &FUEL, 100);                    // on someone else's
r.set("#planks/*", &FUEL, 300);                     // on everything with a tag
r.tag("hot", &["base:lava", "base:magma"]);
r.set("base:stone", &HARDNESS, 3.0);                // the engine's own block properties too

// Reading
if let Some(ticks) = FUEL.on_item(stack.item) { /* burn it */ }
if HOT.on_block(state) { /* harm player */ }
if comps::weather::LIGHTNING.on_id(w.weather()) { /* typed by kind, `.on(entry_kind::X, id)` for any */ }
```

Component and tag names are not things a mod owns, they are a vocabulary.\
Blocks and items belong to the mod that adds them, but `fuel`, `food`, `sand`, `conductor` or `ores/iron` belong to nobody
and mean the same in every mod, so they are written as they are, without a namespace.\
A term that only makes sense for your mod can carry its prefix, `mymod:heat`, so nobody else picks the same word by accident.

Block properties the engine reads are components as well: `HARDNESS`, `FRICTION`, `DRAG`, `LIGHT` and `HARVEST_LEVEL`. `.hardness(1.5)`, `.light(14)` on a builder just set them, so another mod changes them with `r.set` or `r.edit` and reads them with `HARDNESS.on_block(state)` like any other. `TINT` and `TINT_FACES` are read the same way, but come from the faces' layers (`tint_kind`), nothing sets them by hand.

Your own data structures:

```rust
value! {
    #[derive(Default)]
    struct Burner { burn: u32, burn_max: u32 }
}
static BURNER: Component<Burner> = Component::named("mymod:burner");
```

A value that names blocks is a reference, not text: `Component<BlockRef>` for one block (or state) and `Component<BlockList>` for blocks with weights, set with `.set_text(&C, "oak_leaves")` or `.set_text(&C, "birch_log:1 oak_log:4")`. The engine turns the names into ids when the game loads, so a typo is a load error instead of a tree that never grows; reading gives `BlockRef(state)` or a `BlockList` with `pick(random)`. Base's trees work this way (`tree_leaves` on logs, `tree_kinds` and `flowers` on biomes). `Component<Matcher>` holds [which blocks](#references), compiled once at load, and `m.has(state)` asks it.

Per-entity components like a furnace's state, hunger or anything else that changes in time go through
`r.entity_component(&COMP, default, flags)`, declared once (`r.edit(COMP.declaration())` changes another mod's), and then carried by the kinds that need it: `.attach(&COMP)` on an entity builder (a block entity's too), `r.edit(&PLAYER).attach(&COMP)` for someone else's kind, `.attach_with(&COMP, value)` to start from another value.\
`.set(&C, v)` is different: one value of the kind itself (like a mob's attack damage), while `.attach` gives every entity its own value that changes.\
Flags from `component_flags` are `SAVE`, `SYNC_OWNER`, `SYNC_WATCHERS`, `SYNC_MENU`, `KEEP_ON_RESPAWN` (kept when a player respawns; everything else starts over from its default).\
You can change them through `w.component_mut(id, &COMP)` and the engine will figure out what to send.\
A `SAVE` component is saved as its bytes, so don't keep ids in one: ids of dimensions, items, blocks and the like are numbers that shift when a mod comes or goes, and yours would then point at something else. For a dimension save `dimension_key(id)` and read it back with `dimension_from_key(key)`.\
What the engine itself shows of an entity travels the same way, so a client can read it with `view.component_of(id, &C)`: `core:health` (`[f32; 2]`, health and, for players, max health) `core:burning` (`u8`) and `core:held` (`u32`, a player's item in hand, 0 for none). They are the engine's copies: writing them changes nothing.

### Sharing types with other mods
Events and components carry values, and two mods have to agree on their layout. Copying a struct into every mod works, but the mistake shows up only when the game loads.
So a mod can publish a small crate with just its types and handles, the way the base game does with `base_api` (`mods/base/api`): the `value!` structs of its events and components, and `Event`, `Component`, `Chain`, `RecipeKind`, `EquipmentSlot`, `Dimension` and tab handles with full names (`base:smelted`, since the statics compile into whoever uses them).
```toml
[dependencies]
weft_sdk = { path = "../../crates/sdk" }
base_api = { path = "../../mods/base/api" }
```
```rust
r.on_event(&base_api::SMELTED, |b| for s in b.iter() { /* s.furnace, s.input, s.output */ });
```
The mod itself uses the same crate, so there's one declaration of everything, and other mods depend on types only, never on the mod's code. The engine still compares the layouts at load. Without depending on it, `r.has_mod("othermod")` tells whether it is loaded (all mods are known before any registers), to add what goes with it only then. A batch has the world too: `b.world()` in `r.on_event(&actions::USE_BLOCK, |b| ..)`, and each `BlockChange` the player, position, state and item.

## Where to go next

Directional shields use `comps::BLOCKS_ATTACKS` and can keep projectiles in their
stack data; see [entities](entities.md#coming-going-and-getting-hurt).

For HUD information about the crosshair's target, see `ClientView::target` and the target panel
extension in the [UI guide](ui.md#hud). A projectile's relative launch speed uses
`Spawn::inherit_velocity`, described in the [entities guide](entities.md#projectiles).

The rest is split by what you are making. Each page starts with the common case and goes on to the rare ones; the [reference](reference/index.md) lists every method of every builder with a link to its source.

**Content you register**

- [Blocks](blocks.md), [items](items.md), [entities](entities.md), [liquids](details/liquids.md), [status effects and potions](details/effects.md), [recipes and loot](details/recipes-loot.md)
- [World content](details/world-content.md): biomes, dimensions, weather, ores, structures, spawns
- [Changing something of another mod](details/editing-others.md)
- [Block entities](block_entities.md): blocks with data, inventories, menus, energy

**Making things happen**

- [Events, chains and questions](events.md): hooks, phases, how a hit is worked out
- [Conditions](details/conditions.md), [attributes](details/attributes.md), [poses and game modes](details/poses-and-modes.md)
- [Commands](details/commands.md), [text on the screen](details/text.md)

**The world from code**

- [World API](world-api.md): positions, blocks, entities, raycasts, hitboxes, particles, sounds
- [Reading generation](worldgen.md#reading-generation): `p.read(pos)`, spatial searches and postprocessing with `abi::stage::POST_PROCESS`

**Client side**

- [Client](client.md): keys, packets, settings, frame hooks
- [Environment](environment.md): fog, sky lighting, overlays, post effects and acoustics from content or live code
- [UI](ui.md), [camera modes](details/camera.md), [models and animations](models.md), [shaders](shaders.md)

**Assets and systems**

- [Registration](registration.md): every kind, its components and lists
- [Textures](textures.md), [CTM](ctm.md), [Sky](sky.md), [Resource packs](resourcepacks.md), [Localization](localization.md)
- [Redstone](redstone.md), [Worldgen](worldgen.md)

## Debugging

`--log mod:mymod=debug` (or `/log` in chat) turns on your `debug!` / `trace!` lines.\
`info!(target: "pipes", ..)` gives you `mod:mymod::pipes` to filter on.

F3 shows your mod's time per tick. `/gizmo` draws debug shapes, your own groups included (`r.gizmo_group(&PIPES)`).

A panic in your callback disables that callback and names your mod in the log instead of taking the game down.\
Native crashes write a report into `crash-reports/` of the game folder (`run/` from the repo) with the mod that was running.

F3+T reloads textures, lang, shaders and UI without restarting.

## Things that make your and other modders' life easier
Don't hardcode other mods' blocks, use a tag or a component.\
Tag `#sand` allows other mods to work with your mod, whereas `base:sand` does not.

Don't do per-object work across the mod boundary in hot paths if a batch exists (systems, events, entity ticks get all entities of a kind at once).

If you had to work around the engine to do something reasonable, that is an engine flaw.\
Tell me about this instead of building a hack, so I can adjust the engine for those cases.

You can contact me on Discord: `@vortex_so`
