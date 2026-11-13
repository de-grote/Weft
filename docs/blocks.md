# Blocks

Everything a block can be told, in the order you usually need it. `r.block(&BLOCK)` starts it and each section below is a group of calls on that builder. For the complete list of methods see the [reference](reference/blocks.md).

## The basics

`r.block(&BLOCK)` and then:

- `hardness`
- `mined_by("pickaxe", tier)`: tag `mineable/pickaxe` and the `harvest_level` component (`tool_tier::HAND` for none); loot pools with `Cond::harvested()` pass only for a fitting tool of that tier
- `drops(item, n)` or `drops_nothing()`: shorthand for its loot table (`drops` puts `Cond::harvested()` on the pool). Without either the block drops its own item the same way
- `light(15)` or `light_rgb(r, g, b)`
- `cutout()`, `translucent()`

## Faces and textures

faces: `texture(t)` on all of them, then `top`, `bottom`, `ends` (top and bottom), `sides` (the four around), `north`/`south`/`west`/`east`, `front`/`back` (north and south: where `.facing(..)` turns it) for some, the last said wins: `.texture("furnace_side").ends("furnace_top").front("furnace_front")`. Each picks the faces it set, and `layer(t)` puts another picture over them, `tint(0xRRGGBB)` colors their top layer for good (a tinted copy made on load, its frames and connected tiles too: one white wool for every color) and `tint_kind(&tints::GRASS)` by the biome. A grass block is dirt with green over it: `.texture("dirt").top("grass_top").tint_kind(&tints::GRASS).sides("dirt").layer("grass_side_overlay").tint_kind(&tints::GRASS)`. Each layer is drawn by itself over the one under it, with its own animation, connected textures and tint; stairs and slabs made of it get the same faces, and its item shows them in the hand and in slots, the biome's colors where the player is. Items are layers the same way, see [Items](items.md)

## Models, turning and hitboxes

- a model is written once facing north, its top up, and turned with the block: boxes, a model file, hitboxes, a cube's faces, liquid walls and support alike. `.facing(..)` turns it to its `facing`, `.axis()` (a log: an `axis` property set along the face clicked) along its axis, redstone's `attach` and `front` onto that face; `.orientation(Orientation::up(p.facing()))` in `.states` for anything else (`Orientation::new(attach, front)`, `front`, `up`, `leaning`, `axis`). `state_turned(state, o)` gives any block's state turned with the world around it (a structure placed turned, a wrench)
- `model_box(min, max)` (faces show the state's textures) or a `models/block/<name>.json` from Blockbench ([models guide](models.md#blocks-and-items)), `hitbox(min, max)` for its shape in the same 1/16 units, one box a call: what stops a walker, what the crosshair outlines and cracks, only the outline on a block that isn't solid. Without any: the model's boxes, else a plant's cross, else the whole block. `turned(..)`, `tilted(..)` and `unshaded()` change the last box: the last draws it without the shading by side, as a plant's cross is (panels of a tall plant). The order of the other calls doesn't matter (`cutout()` and `light_opacity(2)`, say)

## Rules: what a block does by itself

what happens to a block by itself is rules: when, if, then. `.rule(Rule::random(300.0).when(Cond::new("light(..4) & !beside(#water)")).becomes("moisture=0"))` dries farmland now and then; `Rule::neighbor()` acts as soon as something next to it changes and `Rule::after(secs)` that much later; `.when(..)` takes [conditions](details/conditions.md) about its own cell: `side(down, #soil)` (a side of its own frame, turning with it; `side(down)` is solid), `beside(down, #water)`, `open(down)`, `covered`, `light(9..)`, its `state(..)`; then `.becomes("age=0")` (its own properties) or `.becomes("dirt")` (a block, a state of it with `[..]`), `.grows()`, `.breaks()` (with its drops), `.falls()`. Rules hold for every block of a cell, a crop in with snow too. `needs_below("#soil")`, `needs(Dir::West, "")` for another side, `needs_near("#water")` (sugar cane), `grows`, `smothered_into` and `falling` are rules: a neighbor rule that breaks a block also keeps it from being placed where it would

## Hazards, sounds and particles

- what a block does to whoever meets it: `.hazard(Hazard::touch(1.0, &PRICKED))` is a cactus; `Hazard::inside` (fire, a berry bush), `on` (magma), `head` (where the eyes are), with `.fire(ticks)`, `.every(ticks)` and `.when(..)` about the entity there (`!Cond::flying()`). Every opaque full block suffocates, and a `hot` one without hazards of its own burns like lava
- what a block sounds and shows: its `sound_group` gives `<group>.break`, `.place`, `.step`, `.hit`, and the engine flies pieces of it on breaking, hitting, landing and running; `.effect("step", "mymod:squelch", "")` changes one event (`break`, `place`, `step`, `hit`, `land`, `sprint`; `""` keeps what it would be, `"none"` is nothing). An entity kind takes the same for `hurt`, `death`, `ambient`, `step`
- `ambient_particle("mymod:spark")` for particles the client shows around a block now and then (torch smoke), nothing on the server

## Plants and things that spread

- `plant()`: a cross like grass and flowers, nothing stops a walker, broken at once; `tall_plant("lower", "upper")` the same two blocks tall (its two pictures, a `tint_kind` after it on both)
- `grows(seconds, min_light)` + `on_grow(..)` for plants (`Rule::random(seconds).when(Cond::light(min_light, 15)).grows()`)
- `spreads("#grass_spreads_to", seconds, min_light)` for what creeps onto neighbors (grass onto dirt), with `spread_reach(min, max)` when it reaches otherwise than grass does (moss every way and up walls), `smothered_into("dirt", seconds)` for what turns when covered (`Rule::random(seconds).when(Cond::covered()).becomes("dirt")`). There are no random ticks: rules, spreading and scheduled ticks all run on world time, so they catch up on time nobody was there for

## Callbacks

- `on_use`, `on_neighbor`, `on_scheduled`, `on_carried` (on a moving body, it came to another cell of the world: a drill, a plow). A sneaking player uses a block as vanilla's only with empty hands; `.sneak_use(sneak_use::ALWAYS)` sends every sneaking click to `on_use`, which reads `u.sneaking()` and passes with `false` (a sneak-click that does something else), `NEVER` none
- `on_place(|w, change| ..)`, `on_break(..)`: `actions::PLACE_BLOCK` and `actions::BREAK_BLOCK` for this block only, at the end of the tick

## States

- `prop("lit", ["false", "true"])`, `prop("age", 0..=7)` and `facing(Facings::HORIZONTAL, Toward::Player)` (or `Toward::Face`, out of what was clicked, or `Toward::Away`, where the player looks) for states, like blockstates in modern Minecraft: every combination of values is a state
- `states(|p, s| ..)` to make each state look different (`p.facing()`, `p.is("lit", "true")`), `placement(|place, p| p.set(..))` to pick values when placed
- `joins(Facings::HORIZONTAL, "#fences")` for what connects to its neighbors (fences, panes, pipes, cables): a `true`/`false` property per side the engine keeps right as blocks around change, so `.states` only draws `p.is("north", "true")`; joining by a shared tag connects other mods' blocks too, the block's own name only its own kind. `derived("shape", |n| ..)` is the same for any property worked out from `n.at(Dir::North)` and the rest (stairs' corners are one)

In code a state is read and changed by name, `w.prop(pos, "lit")` and `w.prop_set(pos, "lit", "true")`, the other properties stay as they are.
Anywhere a block is named as text, `base:furnace[lit=true]` picks a state: in structures, `r.set(..)`, `{block:..}` and commands (`/setblock ~ ~ ~ base:furnace[facing=east]`).

## Blocks bigger than a cell

`cells(&[[0, 1, 0]])` for a block bigger than one cell (a door; a bed is `[0, 0, -1]`, a 3x3x3 machine every cell around), as its model is written, facing north: it is one block in all of them. Whatever is done to any cell is done to it, reading any gives it, it is placed only with room in all of them and goes whole. Its model and boxes are written from its own cell and reach the others; the cells touch by faces. Any size, a big one costs what its cells do

## Stairs, slabs and other shapes of a block

A block comes in stairs, slabs and vertical slabs by one tag, `.tag("has_shapes")`: base gives them to every block with it, whichever mod it is from, with its textures, its hardness and sounds, and recipes. Beams (a quarter of a block, 1 x 1/2 x 1/2) and eighths come the same way, several of them sharing a cell, and base's woodcutter cuts a block into all of these with nothing lost (`base_api::WOODCUTTING`, one input, its first output shown: add your own cuts to it). To make such blocks yourself, `r.stairs("#my_tag")`, `r.slabs(..)`, `r.vertical_slabs(..)`, `r.beams(..)` and `r.eighths(..)` take names and tags, and `.tab(..)` and the like go on what they return. `r.shapes_of("wall", "#my_tag", "_wall")` is the same for a shape of your own: `.prop(..)`, `.states(..)` and the rest as on any block, each made block being `<source>_wall`, and `shape.wall = {0} Wall` in your lang file naming them all. They are tagged `wall/<source>` and the source `wall_of/<source>`, so one recipe does for all: `r.shaped(C, "#wall/{m}", 6, &["###", "###"], &[('#', "#wall_of/{m}")])`. A block with its own name for one, `mymod:ruby_stairs` when there is a `ruby`, isn't made twice. Sources are written like any reference, so `"#logs !#log_not_hollow"` leaves out what a mod tagged. What differs per source goes in the values themselves: any text of a made block says `{source}` for the source's name (`base:birch_log`), `{source.name}` and `{source.mod}` for its parts (`birch_log`, `base`: what a file name can have, `"mymod:block/{source.name}_inner"`) and `{source.side}` (`top`, `bottom`, `north`..) for its texture there, and `a | b` takes the first texture that exists: `r.shapes_of("hollow_log", "#logs", "_hollow").model_file("hollow_log").model_texture("inside", "{source.side}_inner | {source.top}")`.

## Blocks on the sides of a cell

A block one pixel thick against the sides of its cell (vines, ladders, glow lichen) is `.on_sides(Facings::ALL)`: a `north`..`down` property per allowed side, it goes against the face clicked, its own item put into the same cell adds a side (up to all six), and a side whose wall goes falls off, and breaking takes one side at a time. The engine runs all of that; redstone wire is the same kind of block. `Facings::HORIZONTAL` keeps it to walls, as base's ladder; any `Facings::only(&[..])` works. Its textures are the block's, `.states(..)` still adds to it.

`merge(|place, p| ..)` is what its own item does placed in its cell: `p` is what is there, `place.placed("type")` how the item would go, and setting a property takes it in (two slabs of one block make it whole). `loot_rolls(2)` on a state rolls its loot twice. `place.hit()` is the point just outside what was clicked, from this cell's corner: `hit().y > 0.5` is the upper half, on a floor, under a ceiling, against a wall or on a slab alike.

## Several blocks in one cell

A cell can hold any number of blocks. A block goes in with what is there when their hitboxes don't overlap, two slabs of different blocks say, with nothing to declare; `.shares_cell_with("#plantable")` lets it in though they do (snow over a plant). A liquid fills its whole cell, so `.shares_cell_with("#water")` is waterlogging: the block keeps a source of water it is placed in and a bucket fills it, while lava stays out; slabs and stairs have it out of the box, and a block whose shapes fill the cell has no room left. The liquid doesn't pass the sides its block's hitbox covers whole (a stair's back holds it, a hollow log lets it out its ends), or the ones `.liquid_walls(&[Dir::North, Dir::South])` names (written facing north, turned with the block; `&[]` for none). From code: `w.parts(pos)` is all that is in a cell, `w.part_add(pos, state)` adds one whatever its shape, `w.part_remove(pos, state)` and `w.part_replace(pos, old, new)` act on one. `w.block` gives the one the cell stands for, and `w.set_block` replaces all of it, unless it is the same block, a property changing. `w.prop` and `w.prop_set` find the part that has the property. Inside a block's own callback `w.block(pos)` and `w.set_block(pos, ..)` mean that block, so it works the same in a cell with others. Only one block in a cell can have a circuit, and that is the one the cell stands for; each part shines, grows by its rules, has its own block entity, gets neighbor updates, scheduled ticks, its own support and its own loot, and the player breaks and uses the one aimed at. A state can be made of pieces broken off one at a time: `.piece(min, max, left, alone)` in its `.states(..)` says that a hit inside the box leaves the state with the properties `left` sets and drops what the state with `alone` would. A double slab's halves and every side of an `on_sides` block are pieces out of the box; candles or snow layers are a piece each with the whole box (`"candles=2"`, `"candles=1"`).

## Sharing one model file

Several blocks, any mod's, can share one model file: `.model_file("othermod:hollow_log")`, with `.model_texture("inside", "stripped_oak_log")` putting this block's own picture where the file says `#inside`. Both go on a state too, for a state on a model of its own: `.states(|p, s| if p.is("filled", "true") { s.model_file("drying_rack_filled") } else { s })`, the filled state's textures going over the block's. `.hitbox(..)` is what stops a walker and is outlined; `.outline(..)` outlines something else (a hollow log outlined whole). Both take several boxes, one a call.

## Reading what a builder set

Everything a builder set is readable from code, any mod's block or item: `block_info(state)` (its block, mod, `solid()`, `opaque()`, light, hardness, friction, model, textures, liquid, item), `item_info(item)` (mod, stack size, durability, attack, what it places, where it is worn), `components_of(entry_kind::BLOCK, state)` for every component and tag by name, values then through their handles, and `entries_with(entry_kind::BLOCK, "logs")` for every state with a tag. That's a WAILA, or hollow logs made for every mod's logs. `texture_exists("othermod:block/log_oak_inner")` says whether a mod has a picture (its own files, not resource packs), so you make one only where there is one; `asset_exists(name, "models", "json")` for any other file.
