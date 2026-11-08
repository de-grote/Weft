# Block entities guide

Blocks with data of their own, items, a window or all three: furnaces, chests, signs, water wheels, generators, batteries.\
This guide builds a furnace step by step, then adds energy so generators, machines and cables from different mods work together.

## Note
Examples to copy from:
- `mods/base/src/block_entities.rs`: the furnace (fuel, a recipe, a lit state) and the chest
- `mods/testmod/src/lib.rs`: a generator, an electric furnace and a battery on the energy contract, plus a `cable` block

## Quick links
[how it fits together](#how-it-fits-together) | [creating one](#creating-one) | [the tick](#the-tick) | [the screen](#the-screen) | [recipes](#recipes) | [energy](#energy) | [many of them](#many-of-them) | [holders](#holders)

## How it fits together
Three things you put together, each on its own: the **block** players see and place; its **block entity** (`.entity(|e| ..)`), an invisible entity of the same name sitting on the block, with the block's data (what is burning, how far along it is), items (`e.inventory(n)`), a model of its own and a tick; and a **menu** that using the block opens (`.opens(&MENU)`). A water wheel is a block entity with no menu, a sign one whose menu is only text, a lever a block with only `on_use`, a chest a block entity with items and a menu. The block entity comes and goes with the block, it is what gets ticked, once per tick for all of a kind, and data saved with `SAVE` stays in the world.

## Creating one
```rust
static FURNACE: Block = Block::named("furnace");
static FURNACE_MENU: Menu = Menu::named("furnace");
static SMELTING: Component<Smelting> = Component::named("base:smelting");

r.entity_component(&SMELTING, Smelting { burn: 0, burn_max: 0, cook: 0, cook_max: 0 }, component_flags::SAVE | component_flags::SYNC_MENU);
r.block(&FURNACE)
    .hardness(3.5)
    .mined_by("pickaxe", 0)
    .entity(|e| e.attach(&SMELTING).inventory(3))
    .opens(&FURNACE_MENU);
r.menu(&FURNACE_MENU)
    .group("input", Slots::single().holder())
    .group("fuel", Slots::single().accepts("fuel").holder())
    .group("output", Slots::single().output().holder())
    .player_inventory()
    .screen(|ui| { /* see below */ });
r.system(&SMELTING, tick_furnaces);
```
`.entity(|e| ..)` is the block entity's kind, an `EntityBuilder` like any other: `inventory(n)` its slots, which the menu's `.holder()` groups show. One that should keep its state when broken (a charged battery, a half-done job) declares that state with `r.instance_component(&C, default, flags | component_flags::CARRIED)`: the broken block's item takes the value along, and placing the item puts it back on the new block entity, the way a renamed chest keeps its name. `w.component(h, &C)` and `w.set_component(h, &C, v)` read and write such a value on any holder, the stack in a player's slot included. The block entity is named like the block, so `EntityKind::named("furnace")` finds it if your code needs to.

Slots come in named groups. The block entity's inventory holds all of its `holder()` groups, the engine works out how big it is and which slot is which, and the tick and the screen ask for a group by its name. A group starts from a shape and adds rules:

| | |
|---|---|
| `Slots::single()`, `row(n)`, `column(n)`, `grid(cols, rows)`, `chest(rows)` | how many slots and how the screen lays them out |
| `output()` | items only come out (results) |
| `input()` | items only go in, nothing comes back out by hand (fuel it burns) |
| `accepts("fuel")` | only items with that tag or component, any mod's |
| `liquid(capacity)` | a liquid slot of that many millibuckets |
| `limit(16)` | at most this many items per slot |
| `large()`, `size(px)` | a bigger slot, 18 GUI pixels by default |
| `background("flame_slot")` | a texture from `gui/` shown while the slot is empty, like the armor silhouettes |
| `tooltip("mymod.fuel_slot")` | a translation shown over the empty slot |
| `priority(n)` | shift-clicks from the inventory fill groups with a higher priority first |

Without `input()` or `output()` a slot takes and gives like a chest's. Shift-clicking an item in the player's inventory puts it into the first group that takes it, groups with an `accepts` rule before plain ones, so ore goes into the input and coal into the fuel slot without you saying anything.

Its state is a component, a `value!` struct of plain numbers like any other. Slots hold items and liquids, so anything else it keeps (heat, progress) goes into a component like this one:
```rust
value! {
    pub struct Smelting { pub burn: u32, pub burn_max: u32, pub cook: u32, pub cook_max: u32 }
}
```
The flags say who gets it. `SAVE` writes it with the world, `SYNC_MENU` sends it to players who have its menu open (progress bars need that), `SYNC_OWNER` sends it to the player who owns the entity (HUD values) and `SYNC_WATCHERS` to everyone who can see it. Since components are named, another mod that declares `base:smelting` with the same layout can read the furnace's state, see "Components and tags" in the [cheat sheet](modding.md#components-and-tags-as-inter-mod-communication).

One with nothing to show (a spring engine: state and energy, no items) has no `.opens`: using it opens nothing. A block that only needs state of its own gets just `.entity(|e| e.attach(..))`.

## The tick
A block entity works through its state component: `r.system(&BURNER, f)` hands `f` every loaded one with that component, their values side by side in one slice, changed in place (and saved and synced like any change). Measured on a thousand of them, that costs a few hundredths of what asking for each one's component costs, so it is the one way they tick:
```rust
fn tick_generators(b: &mut SystemBatch<Burner>) {
    let w = b.world();
    for (id, m, _ticks) in b.iter_mut() {
        let slot = w.inventory(id).group("fuel");
        if m.burn == 0 {
            let fuel = slot.get(0);
            // `fuel` is the common component, so any mod's fuel works
            if let Some(ticks) = FUEL.on_item(fuel.item).filter(|_| !fuel.is_empty()) {
                m.burn = ticks;
                slot.set(0, consume_one(fuel));
            }
        }
        if m.burn > 0 {
            m.burn -= 1;
        }
    }
}
```
`w.inventory(id).group("fuel")` is one group of its slots, numbered from 0 (`get`, `set`, `len`); the whole inventory also has `insert`, which returns what didn't fit. Give each kind a state component of its own (the generator's `Burner`, the furnace's `Smelting`), so its system reaches only its own. For stacks there are `consume_one` (a lava bucket leaves its bucket), `shrink`, `grow`, `stacks_with` and `max_stack`, and the rest of the world is in `w` too (`block`, `set_block`, `play_sound`, `emit`).

To show that it is working, give the block a property and flip it from the tick. The furnace has `.facing(..)` and `.prop("lit", ["false", "true"])`, and its tick does
```rust
w.prop_set(pos, "lit", if lit { "true" } else { "false" });
```
which keeps the way it faces.
`tick_furnaces` in `mods/base/src/block_entities.rs` is the whole thing in one place, with the recipe, the lit state and an event.

### Catching up
The third value is how many ticks the call stands for. It's 1 almost always. When its area was unloaded, or the world's time jumped (someone slept, `/time`), the engine doesn't load anything extra and doesn't tick it thousands of times: it hands over the missed ticks instead, right away, in at most twenty calls however long the gap was. After each of those the energy network moves that many ticks' worth, so a generator and its furnace keep up with each other. So one that should keep working while nobody watches does `ticks` ticks of work in one go. For a furnace that's simple arithmetic: as many items as the fuel, the input and the output slot allow, see base's `catch_up` in `block_entities.rs` and testmod's generator. One that doesn't care can ignore the number, it just loses the missed time.

### Keeping cubes loaded
Catching up is enough for one block entity. A network that has to run while nobody is there (a generator in one cube, its consumers in others, a quarry digging far below) keeps its cubes loaded: `w.keep_loaded(id, from, to, Loading::BlockEntities, 200)` keeps the cubes from block `from` to block `to` for 200 ticks under that key. Call it again before it runs out, with other blocks to move it; `w.release_loaded(id)` lets go. `Loading::BlockEntities` runs block entities, systems, scheduled ticks and redstone there while mobs stand still and nothing grows, as vanilla's lazy chunks; `Loading::All` runs everything as if a player stood there. Tickets survive a restart, all of them together keep at most `core:max_forced_cubes` cubes, and `question::FORCE_LOAD` lets a mod refuse one (someone else's claim).

## The screen
`screen(|ui| ..)` builds the window. `ui.group("input")` draws a group as its grid, `ui.group_slot("input", 1)` a single slot of it when a layout places them apart, `ui.equipment()` the equipment of what it is open on (a mob's, else the player's) and `ui.player_inventory(true)` the player's inventory and hotbar:
```rust
.screen(|ui| {
    ui.panel("panel", S::column().pad(7.0).gap(4.0).stretch(), |ui| {
        ui.title();
        ui.row(S::row().gap(8.0), |ui| {
            ui.group("input");
            ui.recipe_arrow_bind(|c| c.menu_component(&SMELTING).map_or(0.0, |s| ratio(s.cook, s.cook_max)));
            ui.group("output");
        });
        ui.player_inventory(true);
    });
})
```
`ui.title()` is the block's name, so the window says "Furnace" without a translation of its own; a chest a player renamed (`/rename Loot` on the item, the anvil's job) shows that name instead, and keeps it when it's broken and picked up again. A menu opened from code has `menu.<ns>.<name>` as its title.
The arrow reads the block entity's component through `menu_component`, which is why it needs `SYNC_MENU`. The screen belongs to the menu and not to a single block, so every block of one kind shows the same window. Parts, layout and themes are in the [UI guide](ui.md). `.default_screen()` instead of `.screen(..)` is the usual one: the title, the groups in order, the player's inventory. Using the block already opens the menu, and `world.open_menu(player, &MENU, pos)` does the same from code.

## Recipes
Anything where inputs turn into outputs over time is a recipe kind, and a block entity can use the base game's or one of its own:
```rust
static SMELTING_RECIPES: RecipeKind = RecipeKind::named("base:smelting");

r.menu(&FURNACE_MENU)
    .recipes(&SMELTING_RECIPES)          // its arrow opens them, the recipe viewer shows the blocks opening it
    ..;
// in the tick:
if let Some((output, time)) = recipe_process(&SMELTING_RECIPES, input.item) { .. }
```
`recipe_process` gives the output and the time in ticks for one input. Recipes are added the usual way, `r.shapeless(&SMELTING_RECIPES, "ruby", 1, &["ruby_ore"], 200)`, so everything that uses the kind smelts it, whichever mod it comes from. `recipes(..)` makes the menu's `ui.recipe_arrow_bind(..)` (or `ui.recipe_arrow()` without progress) open the kind in the recipe viewer, with "Recipes" over it, and shows the blocks opening it there as where those recipes are made.
An ordinary menu works the same way: `r.menu(&CRAFTING).recipes(&CRAFTING_RECIPES)`, and a block with `.opens(&CRAFTING)` opens it and becomes a station of those recipes (that's the whole crafting table). The grid's size comes from its group, `m.group("grid").cols()`.

A recipe can say more than one output and a time:
```rust
r.shapeless(&ALLOYING, "tin_ingot", 2, &["base:iron_ingot", "base:coal"], 100)
    .input_amounts(&[3, 1])                              // 3 iron and 1 coal
    .output_chance("base:diamond", 1, 0.05)       // and a diamond now and then
    .set(&ENERGY_PER_TICK, 32);                   // data its tick reads
```
A liquid is an item too: every liquid has one named like it, counted in millibuckets, so `"base:water"` or `"#water"` (its tags are the liquid's) goes into a recipe with `input_amounts(&[1000])` like any stack, and `hidden()` keeps a recipe out of the viewer. One with several inputs finds its recipe with `recipe_find(&KIND, &stacks)`, which gives the `time()`, the `outputs()` with their chances (stacks, liquids among them), the `value(&COMPONENT)` and `consume(&mut stacks)` to take the inputs out. Liquids live in liquid slots, GregTech's way: `.group("water", Slots::single().liquid(16_000))` holds sixteen buckets of one liquid, a player pours a bucket in or fills one by clicking with it, and the tick reads and sets it like any slot (`w.inventory(pos).group("water").get(0)` is the water item with its millibuckets as the count). Nothing else goes into a liquid slot, and a liquid goes nowhere else.

A kind of your own is registered with `r.recipe_kind(&KIND)`, and recipes that name a kind nobody registered fail at load. The same call says how the recipe viewer shows it:
```rust
r.recipe_kind(&ALLOYING)
    .inputs(2, 1).outputs(2, 1)                   // always these grids, like a crafting table's 3 x 3
    .info_line("recipe.mymod.alloying.info", &["mymod:energy_per_tick", "time"]);
```
`info_line` is a line under every recipe, a translation like `Uses {0} FE/t for {1} s` whose numbers come from the recipe's values and its time. For a layout of your own, `layout(|ui, recipe| ..)` builds each recipe with the usual UI parts plus `ui.recipe_inputs()`, `ui.recipe_outputs()` and `ui.recipe_input(i)` for its slots (`infusing` in testmod is one, with a fluid).

## Energy
Generators, cables and machines from different mods work together through one contract in the engine. An entity with the `core:energy` component takes part, and a block tagged `conductor` is a cable. The block entities next to a cable, or a chain of them, form a network, and once per tick the engine shares what the generators offer among the machines that want it.

```rust
r.block(&ELECTRIC_FURNACE)
    .entity(|e| e.attach(&WORK).attach_with(&ENERGY, Energy { stored: 0, capacity: 40_000, max_in: 400, max_out: 0 }).inventory(2))
    .opens(&FURNACE_MENU);
r.system(&WORK, tick_furnaces);
r.block(&CABLE).model_box([5.0, 5.0, 5.0], [11.0, 11.0, 11.0]).tag("conductor");
```
`Energy` is `stored` and `capacity`, plus `max_in` and `max_out`, the most it takes from or gives to the network per tick. A generator has `max_in: 0`, a machine `max_out: 0`, and a battery has both, without feeding itself. In their systems, generators add to `stored` and machines spend it:
```rust
let Some(mut e) = w.component(id, &ENERGY) else { continue };
e.stored = (e.stored + 40).min(e.capacity);      // a generator, per tick
w.set_component(id, &ENERGY, e);
// a machine: if e.stored >= COST { e.stored -= COST; work }
```
`w.component(id, &ENERGY)` reads it, and `w.energy_move` moves energy with the limits respected, positive in and negative out (`simulate = true` only reports how much would move). Networks are rebuilt when a cable or a block entity next to one is placed or broken, not every tick.

The same calls take any holder, not only a block entity's id: a block (`w.component(pos, &ENERGY)`) or a stack in someone's inventory, `Holder::held(player, slot)`. An item whose stacks carry `energy` (`.set(&ENERGY, Energy { .. })` on the item) is a battery, and a machine charging the tool in its slot is one `w.energy_move(Holder::held(id, slot), amount, false)`.

There are no units, conversion or losses along the way. Testmod uses 40 per tick and capacities in the tens of thousands, so if your mod should play along with others, stay in that range.

## Many of them
Systems already run once per tick and region with every one of a kind at once, so thousands cost little. Work that only changes the state and touches no inventory is the cheapest of all:
```rust
r.system(&HEAT, |batch| for (_, heat, ticks) in batch.iter_mut() { *heat = heat.saturating_sub(ticks); });   // HEAT: Component<u32>
```

Block entities have no built-in way to pass items to each other, so a pipe or a hopper is something you write: do it in a tick with `inventory(id).insert(..)`.

## Holders
What carries items is a holder: a block entity, any entity, a stack. `inventory(n)` is the same component on all of them. `r.block(&CRATE).entity(|e| e.inventory(27))` gives each placed crate 27 slots, `w.inventory(pos)` reaches them and `w.open_menu(player, &CRATE_MENU, pos)` shows them; a mob kind takes `.inventory(n)` the same way, and an item with `.inventory(n)` is a backpack: each stack keeps its own items, saved with it, and `w.open_menu(player, &BAG_MENU, Holder::held(player, slot))` opens it. While that menu is open its stack can't be moved, and the menu closes if the stack goes away.

More inventories than its own go on a holder as storages: `static ENDER: Storage = Storage::named("ender");`, `r.storage(&ENDER, 27)` and `r.edit(&PLAYER).attach_storage(&ENDER)` give every player 27 more slots, saved with them and kept on death. `w.inventory(ENDER.of(player))` reaches them, and an ender chest is a block whose use does `w.open_menu(player, &ENDER_MENU, ENDER.of(player))`, the menu's `.holder()` groups being the storage. Kinds of your own carry one with `.attach_storage(&S)`; any stack can carry one, `S.of(Holder::held(player, slot))`.

Equipment works the same for every entity that wears (`.equipment()` on its kind, the player always): a menu's `.equipment()` groups are the equipment of the entity it is open on, else the player's, and `ui.equipment()` draws them where the screen wants.
