# Recipes and loot reference

Recipe kinds, recipes, loot tables and their pools.

## LootBuilder

A loot table being registered (`Registry::loot`) or changed (`r.edit(ZOMBIE.loot())`); sent when dropped.

- <a id="LootBuilder-pool"></a>`.pool(p: Pool)`: One more pool, after the table's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L920)
- <a id="LootBuilder-replace_drop"></a>`.replace_drop(from: &str, to: &str)`: Every entry of item `from` gives `to` instead, in whatever pools the table has in the end. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L925)
- <a id="LootBuilder-remove_drop"></a>`.remove_drop(item: &str)`: Entries of `item` are left out, likewise. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L930)
- <a id="LootBuilder-clear"></a>`.clear()`: Empties the table, its swaps too; pools added after it stay. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L935)

## Pool

Rolls (1 by default) of one weighted choice each.

- <a id="Pool-new"></a>`.new() -> Pool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L865)
- <a id="Pool-rolls"></a>`.rolls(min: u32, max: u32) -> Pool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L868)
- <a id="Pool-when"></a>`.when(c: impl Into<Cond>) -> Pool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L872)
- <a id="Pool-entry"></a>`.entry(e: Entry) -> Pool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L876)
- <a id="Pool-item"></a>`.item(name: &str) -> Pool`: One of `name`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L881)
- <a id="Pool-items"></a>`.items(name: &str, min: u32, max: u32) -> Pool`: `min..=max` of `name`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L885)

## RecipeBuilder

- <a id="RecipeBuilder-named"></a>`.named(name: &str)`: Its name instead of `<mod>:<kind>/<output>`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1926)
- <a id="RecipeBuilder-input_amounts"></a>`.input_amounts(counts: &[u32])`: How many of each input it takes, in the order of the inputs (millibuckets for liquids). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1932)
- <a id="RecipeBuilder-output"></a>`.output(item: &str, count: u32)`: Another result (an item, or a liquid by its name and millibuckets). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1936)
- <a id="RecipeBuilder-output_chance"></a>`.output_chance(item: &str, count: u32, chance: f32)`: A result that comes out with chance `chance` (0..1) each time: a pool of it and nothing. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1940)
- <a id="RecipeBuilder-extra"></a>`.extra(pool: crate::Pool)`: Rolls of more results, as a loot table's (`r.loot(..).pool(..)`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1948)
- <a id="RecipeBuilder-keep"></a>`.keep(key: impl RecipeSlot)`: Works, but the recipe viewer doesn't list it. Ingredient `key` (its pattern character, or for a recipe without a pattern its place among the inputs) stays as it is: a mold, a catalyst. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1955)
- <a id="RecipeBuilder-wear"></a>`.wear(key: impl RecipeSlot, amount: u32)`: Ingredient `key` stays and wears by `amount` a run, breaking when it is worn out: a hammer. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1959)
- <a id="RecipeBuilder-leaves"></a>`.leaves(key: impl RecipeSlot, item: &str)`: Ingredient `key` is used up and `item` takes its place (any mod's): a lava bucket leaving its bucket. Without this one leaves its item's `remainder`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1964)
- <a id="RecipeBuilder-hidden"></a>`.hidden()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1972)

## RecipeKindBuilder

A recipe kind being registered; sent when dropped.

- <a id="RecipeKindBuilder-inputs"></a>`.inputs(cols: u32, rows: u32)`: The recipe viewer shows inputs as a grid this big, whatever a recipe uses (a crafting table's 3 x 3). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1998)
- <a id="RecipeKindBuilder-outputs"></a>`.outputs(cols: u32, rows: u32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2001)
- <a id="RecipeKindBuilder-info_line"></a>`.info_line(key: &str, values: &[&str])`: A line under each recipe: translation `key`, its `{0}`.. filled with `values`: `time` (seconds) or components the recipe carries (`mymod:energy_per_tick`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2006)
- <a id="RecipeKindBuilder-layout"></a>`.layout(f: impl Fn(&mut Ui, &RecipeView) + Send + Sync + 'static)`: Builds each recipe yourself instead of the usual inputs, arrow, outputs: `ui.recipe_inputs()`, `ui.recipe_outputs()` and `ui.recipe_input(i)` place its slots. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2013)

## RecipeView

One recipe of a kind, found by `recipe_find` (or shown by a kind's `layout`).

- <a id="RecipeView-time"></a>`.time() -> u32`: Its time in ticks. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2143)
- <a id="RecipeView-outputs"></a>`.outputs() -> Vec<(ItemStack, f32)>`: Its results with their chances; a liquid's is its item by the millibucket. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2147)
- <a id="RecipeView-roll"></a>`.roll(w: &crate::World) -> Vec<ItemStack>`: What else one run of it gives this time, its pools rolled. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2157)
- <a id="RecipeView-value"></a>`.value<T: Value, K>(comp: &Component<T, K>) -> Option<T>`: A value the recipe carries (`r.shaped(..).set(&COMP, v)`): its entry's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2164)
- <a id="RecipeView-consume"></a>`.consume(stacks: &mut [ItemStack]) -> bool`: Takes its inputs out of `stacks` (the ones `recipe_find` was given); false, and nothing taken, if they are not all there. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2169)
