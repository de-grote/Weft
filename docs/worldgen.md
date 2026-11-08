# Worldgen guide

This covers everything that shapes the world: ores, structures, spawns, biomes, and whole dimensions with their own terrain.\
Base builds its overworld with the functions described here, and other mods can change it with the same ones.

## Note
Examples to copy from:
- `mods/testmod/src/lib.rs`: Mars, a planet made only from terrain presets, `void`, an island written by hand, and a biome, a rock needle and boulders added to Mars as from outside
- `mods/testmod/src/looks.rs`: an obelisk structure standing in the dry parts of Mars
- `mods/base/src/worldgen.rs`: the overworld, with biomes, caves, trees and flowers

## Quick links
[how generation works](#how-generation-works) | [ores](#ores) | [structures](#structures) | [spawns](#spawns) | [biomes](#biomes) | [dimensions](#dimensions) | [terrain](#terrain) | [trees](#trees-and-other-scattered-things) | [decorations](#decorations) | [changing another mod's terrain](#changing-another-mods-terrain) | [writing a generator](#writing-a-generator)

## How generation works
The world is made of 16x16x16 cubes, and each one is generated on its own, whenever somebody needs it. That's why a generator has to be a **pure function**: the same seed and position must always give the same blocks, on every machine and in any order. Don't use random numbers, clock, something remembered between cubes for worldgen.

It matters more than it sounds: the server only generates what it simulates around players (`core:simulation_distance`, 8 cubes by default), and everything further away the client generates itself from the seed with your generator. If the two ever disagree, players see one thing and walk into another. The server only sends what differs from generation: blocks players changed and area edits like a crater.

A cube is made in steps: the dimension's terrain (or its generator) first, then scattered things like trees, then decorations (plants), then ores and structures, and spawns happen later in the finished world. None of these steps cares about the others, which is also what lets a mod add any of them to a dimension that belongs to someone else. Ores, scatters and decorations go into every dimension by default: a tree follows its biome, an ore follows the stone it replaces.

Cube coordinates are block coordinates divided by 16, so cube `y = 4` holds the blocks from 64 to 79.

## Ores
`r.ore(&BLOCK)` adds veins of a block to every dimension whose ground it may replace, yours and other mods' alike: testmod's tin is in base's stone and in Mars' basalt from one line.

```rust
static TIN_ORE: Block = Block::named("tin_ore");
r.ore(&TIN_ORE).y(-64, 90).veins(5).vein_size(8).replaces("#stone_ore_replaceables");
```
- `y(min, max)`: the heights veins can start at, as many at each; `height(Height::triangle(-24, 56))` puts most in the middle like modern vanilla's iron, `Height::trapezoid(min, max, plateau)` keeps a flat top
- `shape(Vein::blob(9))`: what a vein looks like, a random walk of `vein_size` blocks unless you say otherwise. `Vein::sphere(r)` and `Vein::disk(r, thickness)` (clay and gravel under water, with `replaces("#dirt")`) are the others; none goes further than 16 blocks from its start
- `veins(n)`: how many veins are started per cube
- `vein_size(n)`: blocks in a vein
- `dimension(&D)`: only there (call it again for more). Another mod's dimension is a handle with its full name, `Dimension::named("base:overworld")`
- `replaces(..)`: which blocks the ore may replace ([written like this](modding.md#references))
- `replaces_with("#granite", &GRANITE_IRON_ORE)`: where the vein meets these, that block goes instead, so a vein doesn't stop at a stone border
- `exposed(0.5)`: leaves out that share of what would touch a cave, as vanilla's gold and diamonds
- `biome("#cave")`: only in these biomes, underground ones included; names and tags like anywhere else, `!` leaving some out

An ore is named like its block, so `r.edit(base::IRON_ORE.ore()).veins(10)` changes base's iron: what you set replaces its own (`replaces` too: a block in `#stone_ore_replaceables` is in every ore's way), and `r.remove(base::IRON_ORE.ore())` takes it out of the world.

## Structures
A structure is a template, a fixed shape of blocks, that the engine places on the surface now and then. You draw a template in layers from the bottom up: each layer is a list of rows along z, and each character in a row is one block along x. A key says which block a character means, a space leaves whatever is already there, and `air` clears it. `fill(block, from, to)` adds a box instead. Its blocks are checked when the game loads, so a typo is an error then, and the same template serves a structure, a moving body (`.body(&TEMPLATE)` on an entity) and code (`w.place_template(&TEMPLATE, pos)`).

```rust
static OBELISK: Template = Template::named("obelisk");
r.template(&OBELISK)
    .layer(&["sss", "sss", "sss"])
    .layer(&["   ", " g ", "   "])
    .key('s', "base:sandstone").key('g', "glow_crystal");
r.structure(&OBELISK).dimension(&MARS).spacing(128).chance(0.5).biome("#dry").buried(1);
```
That's a sandstone obelisk with a glowing crystal inside, in every dry biome of Mars.

Where and how often it appears is set by four calls. `spacing(n)` gives one chance per `n` x `n` blocks and `chance(c)` says how likely that chance is to be taken, while `biome(tag)` limits it to biomes with that tag and `buried(n)` puts the bottom layer that many blocks into the ground. A key can also name a block state, like `furnace[facing=east,lit=true]`.

A structure is named like its template: `r.edit(OBELISK.structure()).spacing(256)` makes another mod's rarer, and `r.edit(&OBELISK).fill(..)` adds blocks to the template itself.

Structures stand on the surface of the dimension's [terrain](#terrain). A dimension generated by hand gets its surface from `.column(..)` instead, and without it gets no structures. To find one later, `world.structure_near("mymod:obelisk", pos, radius)` gives the closest, even in cubes that don't exist yet.

## Spawns
`r.natural_spawn(entity)` decides where a mob shows up by itself:
- `biome(tag)` and `ground(blocks)`: the mob needs a biome with that tag, and one of [those blocks](modding.md#references) under it
- `place(SpawnPlace::in_liquid("#water"))` or `place(SpawnPlace::ceiling())`: a spot in a liquid or hanging under a block instead of on the ground. The mob's own height decides how much room it needs
- `dimension(&D)`: only in that dimension
- `light(min, max)` and `y(min, max)`: the light level (0 to 15) and heights it needs
- `when(cond)`: only where a [condition](modding.md) holds too, asked about the spot and the block under it: `when(Cond::new("night & !block(#mycelium)"))`
- `pack_size(min, max)`: how many appear together
- `weight(n)` and `chance(c)`: how likely it is picked among the rules that fit, and how likely it is to happen per try

A rule is named `<mod>:<entity>`, then `/2`, `/3` for more of the same mob, or what `.named(..)` says, so with `static SPIDERS: SpawnRule = SpawnRule::named("base:spider/2")`, `r.edit(&SPIDERS).weight(8)` or `r.remove(&SPIDERS)` changes another mod's.

A category's cap (`r.entity_category(&C, cap, despawn)`) counts its mobs within 64 blocks of a player, or `.cap_radius(r)`.

```rust
r.natural_spawn(&GHOST).light(0, 4).weight(1);
r.natural_spawn(&SHEEP).biome("#animals").ground("#animal_spawn").light(7, 15).pack_size(2, 3).chance(0.05);
```

## Biomes
A biome says once what it is: the climate it likes, its ground, its colors, plus whatever data and tags you attach.

```rust
static DESERT: Biome = Biome::named("desert");
r.biome(&DESERT).temperature(0.35..).humidity(..0.0).surface("sand", "sand", 3).under("sandstone", 1).tint(&tints::GRASS, 0xbfb755).tag("dry");
```
- `temperature(..)`, `humidity(..)`: where the climate fits it, as ranges like `0.35..` or `..-0.45` (the fields run about -1.4 to 1.4). Without them it fits anywhere.
- `surface(top, filler, depth)`: a top block, `depth` blocks of filler, then the terrain's stone
- `under(block, layers)`: the lowest layers of filler become another block (sandstone under sand)
- `deep_top(block, below)`: a different top block where the ground is lower than `below` (gravel on a sea floor)
- `bare_above(y)`: plain stone above this height
- `frozen(block)`: the water surface turns into this block, and the biome gets the tag `frozen` (base grows no flowers there)
- `tint(&kind, color)`: what blocks tinted with that kind take here, `tints::GRASS`, `FOLIAGE`, `WATER` or a mod's own kind. Kinds you don't give keep their default, a kind given again takes the new color. To color another mod's biome with your kind, `r.edit(&PLAINS).tint(&kind, color)`.
- `snows()`: rain falls as snow here

Tags (`tag("animals")`) are how everything else asks what a biome is, since spawns, ores and structures look at tags and never at names. `set(&COMPONENT, value)` attaches data, like how many trees base grows there.

Registering a biome doesn't put it anywhere. The rules of a dimension's terrain decide where it appears, and that can be underground too: base's `deep_caves` is a biome of whole cubes deep below the surface. Your biome can go into your own dimension or into someone else's, see [changing another mod's terrain](#changing-another-mods-terrain).

## Dimensions
`r.dimension(&MARS)` makes a separate world with its own terrain, sky and gravity, and players get there with `/dim mymod:mars`. It needs something to fill it, usually a [terrain](#terrain) and sometimes a generator written by hand.

```rust
static MARS: Dimension = Dimension::named("mars");
r.dimension(&MARS)
    .spawn_point(|seed| [8, MARS.column(seed, 8, 8).height + 1, 8].into())
    .height_cubes(-4, 12)
    .sky_above(11)
    .gravity(12.0)
    .fixed_time(5000)
    .sky_color(0xd8a080, 0xe0b090);
```

All the settings:
- `spawn_point(f)`: where a new player starts, from the seed. `MARS.column(seed, x, z)` gives the height and biome anywhere, even where nothing is generated yet. Without it the engine looks for open ground near 0, 0, in biomes tagged `spawn_biome` first, out of caves and trees
- `player_spawn()`: new players start here and respawn here (without any dimension doing it, the first registered is used)
- `generator(f)`: fills one cube by hand, see [writing a generator](#writing-a-generator)
- `column(f)`: with a generator of your own, the surface height and biome of a column. It's the one place they come from: your generator reads them from `g.columns()`, and structures, trees and the spawn search stand on them
- `bare()`: only ores, scatters and decorations that name this dimension come in (base's flat test world)
- `height_cubes(min_cube, max_cube)`: the limits of the world, in cubes and inclusive
- `sky_above(cube_y)`: cubes at this height and above are empty, so their sky light is free
- `no_sky_light()`: no sky light at all, for caves and the like
- `gravity(g)`: blocks per second squared, 32 by default
- `day_length(ticks)` or `fixed_time(time)`: either a day of that length, or a time of day that never changes
- `daylight(&[(turn, level), ..])`: how bright the day gets when, if not vanilla's curve. The turn 0 is sunrise, 0.25 noon, 0.5 sunset, 0.75 midnight, and levels go in straight lines between the points, so `&[(0.0, 0.4)]` is an endless dusk. Mobs, crops and burning in the sun follow it as well as the sky
- `sky_color(sky, fog)`: the two colors
- `ambient_light(level)`: the lowest light any block gets, 0 to 15
- `no_block_updates()`: nothing grows, falls or flows here

## Terrain
Most dimensions need real terrain, and writing noise by hand is the slow way. `r.terrain(&dimension)` builds yours from parts: a base height, features that add to it, biome rules that choose a biome and its surface, then water and caves. The engine generates it, so there's no generator to write.

```rust
r.terrain(&MARS)
    .stone("basalt")
    .base_height(60.0)
    .feature(preset::hills(8.0, 220.0))
    .feature(preset::craters(32.0, 16.0, 150.0))
    .feature(preset::dunes(3.0, 22.0).mask(Expr::noise(500.0).ramp(0.15, 0.45)))
    .rule(BiomeRule::new("dust"));
```
`stone` is the block the ground is made of and `base_height` the ground height when nothing is added. Blocks and biomes are given by name, like everywhere else: yours without a prefix, other mods' as `base:sand`.

### Features
A feature adds height to every column. These come ready:

| preset | what it makes |
|---|---|
| `continents(depth)` | land and oceans, `depth` blocks above or below the base |
| `hills(height, size)` | rolling hills |
| `mountains(height, width)` | ranges along ridge lines `width` blocks apart |
| `plateaus(height, size, step)` | flat mesas in terraces |
| `canyons(depth, width)` | winding cuts, filled with water below sea level |
| `dunes(height, wavelength)` | parallel sand waves |
| `craters(radius, depth, spacing)` | bowls with a raised rim |

Each preset is `Feature::new` of a formula from `shape`, and yours is the same: `Feature::new(expr)` with an `Expr` built from numbers, the position and noise. The engine runs it on the GPU and on the CPU with the same integer math, so your shape costs what a preset costs and every machine gets the same world.

- `Expr::blocks(12.0)` is a length, `Expr::value(0.5)` a plain number; a length times a number is a length
- `Expr::x()`, `Expr::z()`: where the column is
- `Expr::fbm(salt, scale, octaves)` and `Expr::noise(scale)`: noise about -1 to 1 in patches about `scale` blocks across; `Expr::ridge(salt, scale)` is 1 along winding lines
- `Expr::sites(salt, spacing, chance, radius)`: points scattered one per `spacing` square, how far the nearest is in units of its radius (below 1 inside it), what craters are made of
- `+`, `-`, `*`, `abs`, `min`, `max`, `clamp`, `idiv`, `modulo` (repeats along the world: terraces, waves)
- `ramp(from, to)`: 0 at `from`, 1 at `to`, straight between; `from == to` is a step
- `e.at(x, z)`: the same formula somewhere else, `noise.at(Expr::x() + warp, Expr::z())` bends it

Testmod's needle, a column of rock 90 blocks high at one spot of Mars, is 1 within 3 blocks of it times 90:

```rust
let far = (Expr::x() - Expr::blocks(1000.0)).abs().max((Expr::z() - Expr::blocks(1000.0)).abs());
Feature::new(far.ramp_blocks(3.0, 3.0).not() * Expr::blocks(90.0)).named("needle")
```
The presets' own formulas in `crates/sdk/src/terrain.rs` are the easiest start: copy one and change it.

### Masks
`.mask(expr)` limits a feature to some places with a strength from 0 to 1 that fades in between; where it is 0 the height isn't even worked out. A mask is an expression like any other, usually a `ramp` of noise or of the climate (swap `from` and `to` to turn it around). Masks combine with `.and(..)` and flip with `.not()`.

```rust
preset::hills(16.0, 110.0).mask(Expr::temperature().ramp(-0.35, -0.6).not())
```

### Fields
A field is a named value over the whole terrain. `temperature`, `humidity` and `continent` come with every terrain (all about -1.4 to 1.4, continentalness below 0 is sea); `.field(name, expr)` declares your own. Masks and other fields use one with `Expr::field(name)`, and biome rules ask for it. It's worked out once per column however many things use it, so one mask shared by a feature and a biome goes here. Base's mountains:

```rust
.field("base:mountains", Expr::fbm(0x44, 700.0, 3).ramp(-0.1, 0.3).and(Expr::continent().ramp(0.0, 0.2)))
.feature(preset::mountains(150.0, 260.0).mask(Expr::field("base:mountains")).named("mountains"))
.rule(BiomeRule::new("mountains").field("base:mountains", 0.45..))
```
The name is used as written, so give yours your mod's prefix. A field can only use fields declared before it. Declaring one that exists, `temperature` included, replaces it for everything that uses it, which is how a mod gives the whole overworld another climate.

### Biome rules
A rule puts a biome into a terrain. It starts from the biome's climate and surface, so `BiomeRule::new("desert")` is a whole rule. Rules are tried in order and the **first one that fits wins**, so specific ones go first and a plain catch-all goes last. What a rule adds is where else it must hold:
- `height(..)`: the surface height, like `..=SEA + 1`
- `field(name, range)`: where a field is in a range like `0.45..` (from inclusive, to exclusive); `temperature(..)`, `humidity(..)` and `continent(..)` are the same for those three

Any of the biome's own settings (`temperature`, `surface`, `frozen`...) can also be given on a rule, and then that rule uses its own.

A rule is named like its biome, which is how other mods point at it (`base:desert`). When one biome has two rules, like base's snowy plains on the coast and inland, the second gets its own name with `.named("snowy_coast")`.

### Water, caves and underground biomes
`.sea(level, "water")` fills everything below a level with water. `.caves(Caves::new("lava", from_y, to_y))` carves tunnels and big caverns, and between those two heights some levels hold flat lava lakes. `.cave_biome("deep_caves", depth, size, threshold)` gives whole cubes at least `depth` blocks down a biome of their own, in patches about `size` across.

## Trees and other scattered things
Anything bigger than a cube that stands on the ground here and there, like a tree, is a scatter. The engine splits the ground into cells, picks one spot per cell from the seed, keeps it with the chance a biome component gives, and asks your shape for the blocks. It skips spots under the sea and where caves took the ground, puts the parts that reach into neighboring cubes there, and keeps players from spawning inside. That works in every dimension where the biome stands, so testmod's Mars grows trees in its patches of `base:forest` without any code of its own.

```rust
static TREE_SITES: Scatter = Scatter::named("trees");
r.scatter(&TREE_SITES).cell(5).reach(3).density(&TREES).shape(|s| {
    let p = s.pos();                                // the block above the ground
    for dy in 0..5 { s.put_force(p.offset(0, dy, 0), OAK_LOG.id()); }
    s.put(p.offset(0, 5, 0), OAK_LEAVES.id());       // put: only into air
});
```
- `cell(n)`: one spot per `n` x `n` blocks at most
- `reach(n)`: how far the shape goes sideways from its spot (up and down it may go 32 and 16)
- `density(&COMPONENT)`: a biome component, 0 to 1, the chance of a spot per cell; biomes without it get none
- `shape(f)`: the blocks, in world coordinates. `s.biome()` and `s.random()` (the same bits every time) pick the kind. It runs again for each cube the thing reaches, so it has to be pure
- `dimension(&D)`: only there

`r.edit(&TREE_SITES)` changes another mod's scatter by its handle.

Base's real trees are in `mods/base/src/worldgen.rs`; how many grow where and which kinds are the biome components `trees` and `tree_kinds` (a `BlockList` of logs, `"birch_log:1 oak_log:4"`), and a log's leaves its `tree_leaves` (a `BlockRef`; logs tagged `cone_tree` grow spruce-like), so another mod can give a biome trees without touching code.

## Decorations
Grass, flowers and anything else that is decided column by column go in `r.decoration(f)`, which runs for every generated cube after the terrain and the scatters, in every dimension (`.dimension(&D)` to limit it). It is named `<mod>:decoration`, `/2`.. or `.named(..)`, so another mod can limit it or `r.remove(&Decoration::named("base:decoration"))`. `g.columns()` has the surface `height` and `biome` of each of the cube's columns, indexed `z * 16 + x`. Base does grass, flowers and cacti like this:

```rust
let Some(&cols) = g.columns() else { return };
for z in 0..16usize { for x in 0..16usize {
    let col = &cols[z * 16 + x];
    let y = col.height + 1 - oy;           // the block above the ground, inside this cube
    if (0..16).contains(&y) && g.get(x, y as usize, z) == 0 {
        g.set(x, y as usize, z, TALL_GRASS.id());
    }
}}
```
`g.column(x, z)` gives any other column in world coordinates, and `g.surface_carved(x, z)` says whether a cave opened the ground there. `g.get` only sees this cube, so when `y` is 0 the ground is out of its reach: `g.surface_block(x, z)` (world coordinates) says what the terrain left at the top of a column, 0 if nothing. How much grows where is data on the biome (the components `grass` and `flowers`), which another mod can attach to any biome with an edit, `r.edit(&FOREST).set(&COMPONENT, value)`.

## Changing another mod's terrain
`r.edit(OVERWORLD.terrain())` with `OVERWORLD = Dimension::named("base:overworld")` takes the same calls as your own terrain. Whatever you call on it is applied after the owner's description, and edits from several mods go in load order, so the result is the same on every machine. Testmod puts a hot, dry biome (`r.biome(&SCORCHED).temperature(0.4..).humidity(..-0.1)...`) into base's deserts like this:

```rust
r.edit(OVERWORLD.terrain())
    .rule_before("base:desert", BiomeRule::new("scorched"))
    .feature(Feature::new(needle).named("needle"));
r.decoration(boulders).dimension(&OVERWORLD);
```
Because rules are tried in order, a new biome goes in next to the rule it takes space from: `rule_before` takes the place where both would fit, `rule_after` only what the other leaves. A biome's climate and surface are its own, wherever it goes: `r.edit(&DESERT).temperature(0.5..)` changes them in every dimension (each is a component, `comps::biome`, and a range of a field given again replaces the old one). The rest of the edits:

| call | what it does |
|---|---|
| `rule_before(rule, new)`, `rule_after(rule, new)` | adds a rule next to another |
| `rule_replace(rule, new)`, `rule_remove(rule)` | swaps a rule for yours, or takes it out |
| `rule_edit(rule, \|r\| r.height(..=90))` | changes only what you set of a rule |
| `feature(f)`, `feature_remove(name)` | adds a feature or takes a named one out |
| `feature_mask(name, mask)` | limits a named feature further, its mask is multiplied by yours |
| `field(name, expr)` | adds a field, or replaces the one of that name |
| `cave_biome(..)`, `cave_biome_remove(biome)` | the same for underground biomes |
| `sea`, `caves`, `stone`, `base_height` | replace the owner's setting, the last mod in load order wins |

A rule is named like its biome, `#0` names the first one whatever it is. The features, rules, fields and underground biomes are lists (`comps::terrain`), so `push`, `insert_before` and the rest work on them like on any list, and `r.replace(OVERWORLD.terrain())` describes a terrain from scratch. An edit whose target isn't there (a typo, or a rule another mod already removed) is a load error naming it, like any edit of something missing. A dimension generated by hand, like testmod's `void`, has no terrain to edit; there a mod can still add ores, scatters, decorations, structures and spawns, and tag its biomes.

## Writing a generator
When terrain from parts doesn't fit, a dimension can fill its cubes in code with `.generator(f)`. A generator receives a `Gen`, the cube being made. `g.origin()` is the block position of its corner (`g.cube()` is the cube's own), `g.seed()` the world seed, `g.set(x, y, z, state)` and `g.get(x, y, z)` work with blocks (each coordinate 0 to 15, states come from `BLOCK.id()`), and `g.biomes()` is the biome id of each of the 256 columns, indexed `z * 16 + x`.

```rust
fn island(g: &mut Gen) {
    let BlockPos { x: ox, y: oy, z: oz } = g.origin();
    for y in 0..16usize { for z in 0..16usize { for x in 0..16usize {
        let (wx, wy, wz) = ((ox + x as i32) as f32, oy + y as i32, (oz + z as i32) as f32);
        if (wx * wx + wz * wz).sqrt() < 24.0 && wy <= 63 {
            g.set(x, y, z, STONE.id());
        }
    }}}
}
```
That's `void` from testmod, a round island in an empty dimension.

With `.column(f)` the generator doesn't work the surface out again: `g.columns()` hands it the heights and biomes `f` gave, the same ones structures and trees stand on.

Two rules to keep, in generators and decorations alike. A cube only writes its own blocks, so anything bigger than a cube (a tree) is better a [scatter](#trees-and-other-scattered-things), where the engine does the asking around. And keep floats out of terrain: `sin`, `cos`, `exp` and even plain float math can come out slightly differently on another platform or a GPU, and neighboring cubes would stop matching. Use integers; `noise::hash` and the integer noise in `noise::fixed` (`perlin2`, `perlin3`, `fbm`) were written for this.
