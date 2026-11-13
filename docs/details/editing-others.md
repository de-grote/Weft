# Changing something of another mod

`edit`, `replace` and `remove` from [How registering works](../modding.md#how-registering-works), kind by kind:

- `r.edit(&base::TORCH).light(5)` goes on every state of a block (testmod edits its own torch this way); `.state_where("lit=true", |s| s.light(15))` on those states only, whatever properties the block has, and `.prop(..)` adds one. Stairs and slabs a template made are edited like any block. A block's edit says of its item only what it sets (`item_texture`, `no_item()`).
- every kind with a builder goes by its handle: entities (`r.edit(&PLAYER).attach(&MANA)`), menus, particles, sounds, keys, settings, weathers, poses, and so on. What belongs to something goes by its owner's: `IRON_ORE.ore()`, `WATER.liquid()`, `GRAVEL.loot()` or `ZOMBIE.loot()`, `OVERWORLD.terrain()`, `OVERWORLD.sky(&MOON)`, `WELL.structure()`
- what has no handle is named after what made it: recipes `<mod>:<kind>/<output>`, natural spawns `<mod>:<entity>`, decorations `<mod>:decoration`, the second of each `/2` and so on, liquid reactions `<mod>:<liquid>><touching>`, screen parts `<mod>:<screen>/<area>`. `.named(..)` on a recipe, spawn, decoration or reaction gives yours a name of your choice, and `Recipe::named("base:crafting/torch")`, `SpawnRule::named(..)` and the like reach theirs
- `r.set` and `r.tag` above are `edit_pattern` for blocks and items at once
- `r.replace` works for every kind with a builder, `r.remove` for every kind (a block takes its item along)

After a replace their handle, item and recipes point at yours.
