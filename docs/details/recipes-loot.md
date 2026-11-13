# Recipes and loot



## Recipes

- `r.shaped(&KIND, out, count, &["iii", " s ", " s "], &[('i', "#ingots/iron"), ('s', "#stick")])`
- `r.shapeless(..)`
- what a run does to an ingredient other than using it up, by its pattern character (or its place among a shapeless one's inputs): `.keep('m')` (a mold stays), `.wear('h', 1)` (a hammer stays and wears a point a run, breaking when worn out), `.leaves('b', "bucket")` (any item takes its place). Without one an ingredient is used up and the last of a stack leaves its item's `remainder` (a lava bucket its bucket)

Recipe kinds are registered too (`r.recipe_kind`), a furnace is just a kind with stations.\
What else a recipe gives besides its result is loot pools: `.extra(Pool::new()..)` is rolled the way a loot table is, and `.output("slag", 1)` and `.output_chance("diamond", 1, 0.05)` are pools made for you. The recipe viewer shows each item with its chance, and `recipe.roll(&w)` gives what one run brings.\
A recipe is named after its kind and output in the namespace of the mod making it: base's crafting recipe for torches is `base:crafting/torch`, a second one of base's for torches `base:crafting/torch/2`, and so on in the order it makes them; `.named("torch_from_coal")` gives it one of your choice. By that name other mods change it like anything else: `r.remove(&Recipe::named("base:crafting/torch"))`, or `r.edit(&SMELTING_ALL).hidden()` with `static SMELTING_ALL: Recipe = Recipe::named("base:smelting/*")`.

## Loot

`r.loot(&RUBY_ORE).pool(Pool::new().when(Cond::harvested()).rolls(1, 2).item("ruby"))` (a block's or entity's table is what it drops, `LootTable::named(..)` is a table of its own)\
A block's table is all it drops, there is no tool check in front of it: put `Cond::harvested()` on the pools that need the right tool (broken by the world, like a torch losing its wall, always counts as harvested).\
Other mods' tables change without replacing them: `r.edit(ZOMBIE.loot()).pool(..)` with `ZOMBIE = EntityKind::named("base:zombie")` adds a pool, `.replace_drop("rotten_flesh", "mymod:brain")` and `.remove_drop(..)` change what any pool of it gives, `.clear()` empties it, and the pools are a list like any (`.remove_item(&comps::loot::POOLS, "#0")` takes out the first).
