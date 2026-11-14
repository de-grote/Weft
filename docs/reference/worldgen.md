# World generation reference

Dimensions, biomes, terrain, features, structures and the sky.

## BiomeBuilder

- <a id="BiomeBuilder-temperature"></a>`.temperature(range: impl std::ops::RangeBounds<f64>)`: Where the climate fits it, like `0.35..` or `..-0.45` (from inclusive, to exclusive); the terrain's fields are about -1.4..1.4. Without one it fits everywhere. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2943)
- <a id="BiomeBuilder-humidity"></a>`.humidity(range: impl std::ops::RangeBounds<f64>)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2946)
- <a id="BiomeBuilder-surface"></a>`.surface(top: &str, filler: &str, depth: i32)`: Top block, then `depth` blocks of filler, then the terrain's stone. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2950)
- <a id="BiomeBuilder-under"></a>`.under(block: &str, layers: i32)`: The lowest `layers` of the filler become `block` (sandstone under sand). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2957)
- <a id="BiomeBuilder-deep_top"></a>`.deep_top(block: &str, below: i32)`: Top block for columns lower than `below` (gravel on deep sea floors). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2963)
- <a id="BiomeBuilder-bare_above"></a>`.bare_above(y: i32)`: Plain stone above this height (bare peaks). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2969)
- <a id="BiomeBuilder-frozen"></a>`.frozen(block: &str)`: A frozen biome: the water surface freezes into `block`, and it has the tag `frozen` (decorations put no flowers there). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2974)
- <a id="BiomeBuilder-snows"></a>`.snows()`: Rain falls as snow here: `precipitation(&precipitations::SNOW)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2980)
- <a id="BiomeBuilder-precipitation"></a>`.precipitation(p: &'static Precipitation)`: What falls here when a weather brings rain: `precipitations::SNOW`, or a mod's own. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2984)
- <a id="BiomeBuilder-tint"></a>`.tint(kind: &'static TintKind, color: u32)`: The color (`0xRRGGBB`) blocks tinted with `kind` take here (`tints::GRASS`, or a mod's own kind). Kinds not given keep their default. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2989)

## BiomeRule

Where a biome stands in a terrain. Its climate and surface come from the biome (`r.biome(&X).temperature(..).surface(..)`); a rule adds where else it must hold (height, continent, any field) and sets only what differs here. Rules are tried in order; the first match wins and the last one is used where none does.

- <a id="BiomeRule-new"></a>`.new(biome: &str) -> BiomeRule`: A rule for `biome`, named like it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L372)
- <a id="BiomeRule-named"></a>`.named(name: &str)`: A name of its own, for a second rule of the same biome (other mods find rules by name). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L387)
- <a id="BiomeRule-biome"></a>`.biome(biome: &str)`: In an edit: the rule picks this biome instead. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L392)
- <a id="BiomeRule-field"></a>`.field(name: &str, range: impl RangeBounds<f64>)`: Where the field (`temperature`, `humidity`, `continent` or one the terrain declares) is in `range`, like `0.35..` or `..-0.45` (from inclusive, to exclusive). Given again for the same field, the new range replaces the old. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L399)
- <a id="BiomeRule-temperature"></a>`.temperature(range: impl RangeBounds<f64>)`: Climate, about -1.4..1.4. Usually the biome's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L412)
- <a id="BiomeRule-humidity"></a>`.humidity(range: impl RangeBounds<f64>)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L415)
- <a id="BiomeRule-continent"></a>`.continent(range: impl RangeBounds<f64>)`: Continentalness: below 0 is sea, coasts are about 0..0.1. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L419)
- <a id="BiomeRule-height"></a>`.height(range: impl RangeBounds<i32>)`: Surface heights, like `..=SEA + 1`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L423)
- <a id="BiomeRule-surface"></a>`.surface(top: &str, filler: &str, depth: i32)`: Top block, then `depth` blocks of filler, then the terrain's stone. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L429)
- <a id="BiomeRule-under"></a>`.under(block: &str, layers: i32)`: The lowest `layers` of the filler become `block` (sandstone under sand). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L435)
- <a id="BiomeRule-deep_top"></a>`.deep_top(block: &str, below: i32)`: Top block for columns lower than `below` (gravel on deep sea floors). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L441)
- <a id="BiomeRule-bare_above"></a>`.bare_above(y: i32)`: Plain stone above this height (bare peaks). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L447)
- <a id="BiomeRule-frozen"></a>`.frozen(block: &str)`: The water surface freezes into `block`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L452)

## Caves

Two-noise tunnels plus large caverns. Between `lava_from` and `lava_to` some layers of height hold flat lava lakes.

- <a id="Caves-new"></a>`.new(lava: &str, lava_from: i32, lava_to: i32) -> Caves` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L506)

## Dimension

- <a id="Dimension-sky"></a>`.sky(object: &SkyObject) -> &'static SkyObject`: Its object `object` of the sky, for `Registry::edit` and `remove`: `r.remove(OVERWORLD.sky(&base::STARS))`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L186)
- <a id="Dimension-terrain"></a>`.terrain() -> &'static Terrain`: Its terrain, for `Registry::edit`: `r.edit(OVERWORLD.terrain()).rule_before(..)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L190)
- <a id="Dimension-generation"></a>`.generation(seed: u64, stage: u32) -> crate::Generation<'_>`: Inspect the deterministic result before `stage` (use `abi::stage::POST_PROCESS` for terrain, structures and vegetation), without loading the live world. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L624)
- <a id="Dimension-column"></a>`.column(seed: u64, x: i32, z: i32) -> Column`: Height and biome of column (`x`, `z`) for `seed`, generated or not: from the terrain or the dimension's `column`. A dimension with neither gives height `i32::MIN`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L629)
- <a id="Dimension-surface_carved"></a>`.surface_carved(seed: u64, x: i32, z: i32) -> bool`: Whether caves took the surface block of column (`x`, `z`), so nothing should stand there. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L634)

## DimensionBuilder

- <a id="DimensionBuilder-hazard"></a>`.hazard(h: crate::Hazard)`: What it does to whoever is in it (`Hazard::own(..).when(..)`), besides its void 64 blocks under its floor. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3002)
- <a id="DimensionBuilder-generator"></a>`.generator(f: impl Fn(&mut Gen) + Send + Sync + 'static)`: Must be a pure function of its input: it runs in parallel and must give the same cube for the same seed and position on every machine. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3007)
- <a id="DimensionBuilder-spawn_point"></a>`.spawn_point(f: impl Fn(u64) -> BlockPos + Send + Sync + 'static)`: The block a player starts on top of in this dimension, for a seed. Without one the engine finds open ground near 0, 0 on the dimension's columns (biomes tagged `spawn_biome` first), out of caves and trees. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3013)
- <a id="DimensionBuilder-player_spawn"></a>`.player_spawn()`: Players start here and respawn here. When several mods claim it, the one loaded last wins; without any, the first dimension registered is used. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3018)
- <a id="DimensionBuilder-no_block_updates"></a>`.no_block_updates()`: Blocks never update here (no neighbor updates, block ticks or growth). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3022)
- <a id="DimensionBuilder-bare"></a>`.bare()`: Only what names it: ores, scatters and decorations meant for every dimension stay out (a superflat test world). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3027)
- <a id="DimensionBuilder-column"></a>`.column(f: impl Fn(u64, i32, i32) -> (i32, u32) + Send + Sync + 'static)`: Surface height and biome id of column (x, z) for (seed, x, z), for a dimension without a terrain. The generator reads them from `g.columns()` instead of working them out again, and structures, scatters (trees) and the spawn search stand on them. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3033)
- <a id="DimensionBuilder-height_cubes"></a>`.height_cubes(min_cube: i32, max_cube: i32)`: Inclusive limits in cubes (16 blocks). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3038)
- <a id="DimensionBuilder-sky_above"></a>`.sky_above(cube_y: i32)`: Everything at or above this cube y is empty, so sky light there needs no generation. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3042)
- <a id="DimensionBuilder-weathers"></a>`.weathers(list: &[&Weather])`: The weathers that come here now and then after clear weather, each by its weight and duration (`&[&weathers::RAIN, &weathers::THUNDER]` is vanilla's). Without, always clear. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3047)
- <a id="DimensionBuilder-no_sky_light"></a>`.no_sky_light()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3050)
- <a id="DimensionBuilder-gravity"></a>`.gravity(g: f32)`: Blocks per second squared. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3054)
- <a id="DimensionBuilder-day_length"></a>`.day_length(ticks: u32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3057)
- <a id="DimensionBuilder-daylight"></a>`.daylight(points: &[(f32, f32)])`: Daylight through the day as points (turn of the sky, level 0..1): the turn 0 is sunrise, 0.25 noon, 0.5 sunset, 0.75 midnight, with straight lines between. Endless dusk is `&[(0.0, 0.4)]`, a long polar day `&[(0.0, 1.0), (0.6, 1.0), (0.7, 0.0), (0.95, 0.0)]`. Spawning, growth, burning in the sun and the sky all follow it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3064)
- <a id="DimensionBuilder-fixed_time"></a>`.fixed_time(time: u32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3071)
- <a id="DimensionBuilder-sky_color"></a>`.sky_color(sky: u32, fog: u32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3075)
- <a id="DimensionBuilder-ambient_light"></a>`.ambient_light(level: u32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3079)

## Expr

A number for every column of a terrain, worked out from where the column is: the height a landform adds, the strength of a mask, a field biome rules ask for. Built from numbers, the position and noise with `+`, `-`, `*` and the methods here; the engine runs it on the CPU and the GPU with the same integer math, so every machine gets the same world.

- <a id="Expr-value"></a>`.value(v: f64) -> Expr`: A plain number. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L70)
- <a id="Expr-blocks"></a>`.blocks(b: f64) -> Expr`: A length in blocks. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L75)
- <a id="Expr-int"></a>`.int(n: i32) -> Expr`: An integer as it is, for `idiv` and `modulo` by a count. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L80)
- <a id="Expr-x"></a>`.x() -> Expr`: The column's position, a length. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L85)
- <a id="Expr-z"></a>`.z() -> Expr` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L89)
- <a id="Expr-field"></a>`.field(name: &str) -> Expr`: A field of the terrain: `temperature`, `humidity`, `continent`, or one declared with `TerrainBuilder::field` before what uses it. Worked out once per column however many times it is used, so share costly parts through a field. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L96)
- <a id="Expr-temperature"></a>`.temperature() -> Expr`: About -1.4..1.4, cold to hot. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L101)
- <a id="Expr-humidity"></a>`.humidity() -> Expr`: About -1.4..1.4, dry to wet. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L106)
- <a id="Expr-continent"></a>`.continent() -> Expr`: Continentalness: below 0 is sea, coasts are about 0..0.1. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L111)
- <a id="Expr-fbm"></a>`.fbm(salt: u64, scale: f64, octaves: u32) -> Expr`: Noise about -1..1 in patches about `scale` blocks across, `octaves` layers of it each twice as fine and half as strong. Noises with the same `salt` and size line up. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L117)
- <a id="Expr-noise"></a>`.noise(scale: f64) -> Expr`: `fbm` of 3 octaves with a salt of its own size. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L122)
- <a id="Expr-ridge"></a>`.ridge(salt: u64, scale: f64) -> Expr`: 1 along lines where the noise crosses 0, falling to 0 a quarter of the way to its peaks: winding ridges or cuts `scale` blocks apart. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L128)
- <a id="Expr-sites"></a>`.sites(salt: u64, spacing: f64, chance: f64, radius: f64) -> Expr`: Points scattered one per `spacing` x `spacing` square at most, each there with `chance` and `radius` blocks big times 0.5 to 1.5: the square of the distance to the nearest, in units of its radius (below 1 inside it), 2 or more where none is near. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L138)
- <a id="Expr-at"></a>`.at(x: Expr, z: Expr) -> Expr`: This expression at another position: `noise.at(Expr::x() + warp, Expr::z())` bends it. Fields stay what they are at the column. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L144)
- <a id="Expr-abs"></a>`.abs() -> Expr` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L156)
- <a id="Expr-min"></a>`.min(other: Expr) -> Expr` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L160)
- <a id="Expr-max"></a>`.max(other: Expr) -> Expr` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L164)
- <a id="Expr-clamp"></a>`.clamp(lo: Expr, hi: Expr) -> Expr` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L168)
- <a id="Expr-idiv"></a>`.idiv(other: Expr) -> Expr`: Integer division, toward zero: a length by `Expr::int(2)` is half as long. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L173)
- <a id="Expr-modulo"></a>`.modulo(other: Expr) -> Expr`: What is left over below `other` (a length or a count), never negative: repeats along the world. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L179)
- <a id="Expr-ramp"></a>`.ramp(from: f64, to: f64) -> Expr`: 0 at `from`, 1 at `to` and straight in between, a strength for a mask; `from > to` turns it around. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L185)
- <a id="Expr-ramp_blocks"></a>`.ramp_blocks(from: f64, to: f64) -> Expr`: `ramp` of a length, `from` and `to` in blocks. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L190)
- <a id="Expr-not"></a>`.not() -> Expr`: 1 minus it, for masks. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L195)
- <a id="Expr-and"></a>`.and(other: Expr) -> Expr`: Both masks: their product. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L200)

## FeatureBuilder

A feature being registered or changed (`r.edit(IRON_ORE.ore())`: what it sets replaces the feature's, lists add to it).

- <a id="FeatureBuilder-named"></a>`.named(name: &str)`: Its name instead of the one it was given (`<mod>:decoration`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L190)
- <a id="FeatureBuilder-stage"></a>`.stage(stage: u32)`: When its turn comes (`abi::stage`; by its spread unless said: underground per cube, structures on a grid, decoration on every cube). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L200)
- <a id="FeatureBuilder-after"></a>`.after(other: &Feature)`: After `other` in the same stage. Stage order cannot be reversed. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L204)
- <a id="FeatureBuilder-before"></a>`.before(other: &Feature)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L207)
- <a id="FeatureBuilder-per_cube"></a>`.per_cube(n: u32)`: `n` starts tried in every cube, over its heights (`y` or `height`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L214)
- <a id="FeatureBuilder-height"></a>`.height(h: Height)`: How its starts spread over heights: `Height::triangle(-24, 56)` puts most in the middle, like modern vanilla's iron. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L220)
- <a id="FeatureBuilder-spacing"></a>`.spacing(blocks: i32)`: One start per `blocks` x `blocks` square, on the ground. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L225)
- <a id="FeatureBuilder-chance"></a>`.chance(c: f32)`: The chance of a start per square (1 unless said). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L230)
- <a id="FeatureBuilder-density"></a>`.density(component: &crate::Component<f32>)`: The biome component (0..1) that is the chance of a start per square; biomes without it get none. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L235)
- <a id="FeatureBuilder-ground"></a>`.ground(what: &str)`: Only where `what` takes the surface block (a block matcher, `.ground("#plant_soil")`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L239)
- <a id="FeatureBuilder-over_water"></a>`.over_water()`: It may stand where the ground is under the sea. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L243)
- <a id="FeatureBuilder-over_caves"></a>`.over_caves()`: It may stand where caves took the ground. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L247)
- <a id="FeatureBuilder-max_slope"></a>`.max_slope(blocks: i32)`: Only where the ground under it varies by at most `blocks`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L251)
- <a id="FeatureBuilder-buried"></a>`.buried(blocks: i32)`: How far into the ground its bottom layer goes. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L255)
- <a id="FeatureBuilder-priority"></a>`.priority(p: i32)`: One with a higher priority in this or an earlier stage keeps it `separation` blocks away; only what really stands counts. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L260)
- <a id="FeatureBuilder-separation"></a>`.separation(blocks: i32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L263)
- <a id="FeatureBuilder-generate"></a>`.generate(f: impl Fn(&mut Gen) + Send + Sync + 'static)`: `f` on every generated cube, deciding everything itself (a decoration). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L267)
- <a id="FeatureBuilder-vein"></a>`.vein(v: Vein)`: What a vein looks like (a random walk of 8 blocks unless said). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L275)
- <a id="FeatureBuilder-vein_size"></a>`.vein_size(n: u32)`: Blocks per vein (8 unless said). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L287)
- <a id="FeatureBuilder-template"></a>`.template(t: &Template)`: Template `t` from its lowest corner. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L291)
- <a id="FeatureBuilder-turns"></a>`.turns()`: Each one turned a random quarter, its blocks with it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L296)
- <a id="FeatureBuilder-turns_and_mirrors"></a>`.turns_and_mirrors()`: Each one turned a random quarter and mirrored or not. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L300)
- <a id="FeatureBuilder-transforms"></a>`.transforms(bits: u32)`: The ways its template may be placed: a bit per `Transform` (`1 << t.0`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L304)
- <a id="FeatureBuilder-palette"></a>`.palette(p: &Palette)`: One more palette for its slots; the seed picks one per start. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L308)
- <a id="FeatureBuilder-plan"></a>`.plan(f: impl Fn(&mut Plan) + Send + Sync + 'static)`: Lays out each start on the server and clients alike. `p.read` sees the completed earlier stages and this plan's own writes; `p.random()` is deterministic. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L313)
- <a id="FeatureBuilder-reach"></a>`.reach(blocks: i32)`: How far a plan may put things sideways from its start (0 unless said); past that they are left out. Up and down it may go 32 and 16 unless `reach_y` says. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L319)
- <a id="FeatureBuilder-reach_y"></a>`.reach_y(up: i32, down: i32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L323)
- <a id="FeatureBuilder-block"></a>`.block(b: &Block)`: What a vein or steps put (an ore named like its block puts that). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L337)
- <a id="FeatureBuilder-replaces"></a>`.replaces(what: &str)`: What it may replace (a block matcher); anything unless said. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L341)
- <a id="FeatureBuilder-replaces_with"></a>`.replaces_with(what: &str, block: &Block)`: Where a vein meets what `what` takes (a block matcher), `block` goes there instead: one vein through stone and granite, iron ore in one and granite iron ore in the other. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L346)
- <a id="FeatureBuilder-exposed"></a>`.exposed(share: f32)`: Share of its blocks left out where they would touch air, as vanilla's ores in caves (0.5 for most of them). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L352)
- <a id="FeatureBuilder-foundation"></a>`.foundation(block: &Block)`: Its template stands on `block` filled down to the ground. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L356)
- <a id="FeatureBuilder-foundation_of_ground"></a>`.foundation_of_ground()`: Its template stands on the ground's own top block filled down (grass on plains, sand on a beach). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L361)
- <a id="FeatureBuilder-clear_above"></a>`.clear_above()`: The ground above its template is cut away. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L365)

## Gen

Generation context for one cube.

- <a id="Gen-view"></a>`.view() -> crate::Generation<'_>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3244)
- <a id="Gen-read"></a>`.read(pos: impl Into<BlockPos>) -> u32`: Read a block in world coordinates, including outside this cube and earlier local writes. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3248)
- <a id="Gen-seed"></a>`.seed() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3251)
- <a id="Gen-cube"></a>`.cube() -> BlockPos`: Cube coordinates (block coords = cube * 16). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3255)
- <a id="Gen-origin"></a>`.origin() -> BlockPos` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3258)
- <a id="Gen-blocks"></a>`.blocks() -> &mut [u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3262)
- <a id="Gen-biomes"></a>`.biomes() -> &mut [u16` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3268)
- <a id="Gen-set"></a>`.set(x: usize, y: usize, z: usize, state: u32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3275)
- <a id="Gen-get"></a>`.get(x: usize, y: usize, z: usize) -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3286)
- <a id="Gen-dimension"></a>`.dimension() -> u32`: The dimension being generated. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L642)
- <a id="Gen-columns"></a>`.columns() -> Option<&[Column`: The surface of the cube's 256 columns, index `z * 16 + x`: from the terrain or the dimension's `column`. None for a dimension with neither. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L648)
- <a id="Gen-column"></a>`.column(x: i32, z: i32) -> Column`: Any column of this dimension, in world coordinates. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L653)
- <a id="Gen-surface_carved"></a>`.surface_carved(x: i32, z: i32) -> bool`: Whether caves took the surface block of column (`x`, `z`), in world coordinates. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L658)
- <a id="Gen-surface_block"></a>`.surface_block(x: i32, z: i32) -> u32`: The block at the terrain's reference surface, in this callback's generation view. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L663)

## Height

Heights a feature's starts spread over (`FeatureBuilder::height`).

- <a id="Height-uniform"></a>`.uniform(min: i32, max: i32) -> Height` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L63)
- <a id="Height-triangle"></a>`.triangle(min: i32, max: i32) -> Height`: Most in the middle, fewer towards the ends, none past them. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L67)
- <a id="Height-trapezoid"></a>`.trapezoid(min: i32, max: i32, plateau: i32) -> Height`: At its most over `plateau` blocks in the middle, then falling off. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L71)

## Landform

Adds to the height of every column, scaled by its mask.

- <a id="Landform-new"></a>`.new(height: Expr) -> Landform`: A landform adding `height` (a length) at every column. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L242)
- <a id="Landform-named"></a>`.named(name: &str) -> Landform`: Other mods find the landform by this name. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L247)
- <a id="Landform-mask"></a>`.mask(mask: Expr) -> Landform`: Where it applies, 0..1: nothing where it is 0 or less (and its height isn't worked out there), all of it from 1. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L254)

## PaletteBuilder

Blocks for templates' slots.

- <a id="PaletteBuilder-slot"></a>`.slot(slot: &str, block: &str)`: Slot `slot` takes `block` (a state too: `base:oak_log[axis=x]`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3229)

## Piece

A template a plan puts (`Plan::template`).

- <a id="Piece-foundation"></a>`.foundation(block: u32)`: It stands on `block` filled down to the ground. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L645)
- <a id="Piece-foundation_of_ground"></a>`.foundation_of_ground(mut self)`: It stands on the ground's own top block filled down. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L650)
- <a id="Piece-clear_above"></a>`.clear_above(mut self)`: The ground above it is cut away. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L655)
- <a id="Piece-palette"></a>`.palette(p: &'p Palette)`: Its slots filled from `p` (else from one of the feature's palettes, else its defaults). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L660)
- <a id="Piece-put"></a>`.put(mut self) -> bool`: Puts it in now: whether it went in (it exists and stays within the reach). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L665)

## Plan

One start of a feature being laid out (`FeatureBuilder::plan`), in world coordinates.

- <a id="Plan-view"></a>`.view() -> crate::Generation<'_>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L418)
- <a id="Plan-read"></a>`.read(pos: impl Into<BlockPos>) -> u32`: A block in the earlier stages and this plan's own writes, in world coordinates. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L422)
- <a id="Plan-pos"></a>`.pos() -> BlockPos`: The start: the block above the ground for one on a grid. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L426)
- <a id="Plan-biome"></a>`.biome() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L429)
- <a id="Plan-dimension"></a>`.dimension() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L432)
- <a id="Plan-seed"></a>`.seed() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L435)
- <a id="Plan-random"></a>`.random() -> u64`: Random bits of this start, the same every time it is laid out. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L439)
- <a id="Plan-random_n"></a>`.random_n(n: u64) -> u64`: More random bits, the same every time for the same `n`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L443)
- <a id="Plan-put"></a>`.put(pos: impl Into<BlockPos>, state: u32) -> bool`: `state` at `pos` where there is air. Whether it went in (not past the reach). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L455)
- <a id="Plan-set"></a>`.set(pos: impl Into<BlockPos>, state: u32) -> bool`: `state` at `pos` whatever is there (a trunk through another tree's leaves). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L460)
- <a id="Plan-fill"></a>`.fill(from: impl Into<BlockPos>, to: impl Into<BlockPos>, state: u32) -> bool`: A box of `state` between two opposite corners. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L465)
- <a id="Plan-replace"></a>`.replace(from: impl Into<BlockPos>, to: impl Into<BlockPos>, what: &str, state: u32) -> bool`: What `what` takes in the box (a block matcher, `#logs`) becomes `state`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L469)
- <a id="Plan-entity"></a>`.entity(kind: &crate::EntityKind, pos: impl Into<BlockPos>) -> bool`: An entity of `kind` on block `pos` once the start's cube joins the world. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L473)
- <a id="Plan-loot"></a>`.loot(pos: impl Into<BlockPos>, table: &LootTable) -> bool`: The block entity at `pos` (a chest) filled from `table` once it joins the world. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L478)
- <a id="Plan-template"></a>`.template<'p>(&'p mut self, t: &'p Template, corner: impl Into<BlockPos>, transform: Transform) -> Piece<'p>`: Template `t` with its lowest corner at `corner` once placed with `transform`; say more on the piece, it goes in when dropped or `put`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L484)
- <a id="Plan-ground"></a>`.ground(x: i32, z: i32) -> Column`: The ground of column (`x`, `z`): its height and biome. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L489)
- <a id="Plan-surface_carved"></a>`.surface_carved(x: i32, z: i32) -> bool`: Whether caves took the surface block of column (`x`, `z`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L493)
- <a id="Plan-size"></a>`.size(t: &Template, transform: Transform) -> BlockPos`: Template `t`'s size once placed with `transform`; zero for none. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L497)
- <a id="Plan-joints"></a>`.joints(t: &Template, transform: Transform) -> Vec<Joint>`: Template `t`'s joints once placed with `transform`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L501)
- <a id="Plan-fit"></a>`.fit(t: &Template, x: i32, z: i32, transform: Transform, max_slope: i32) -> Option<BlockPos>`: Where template `t` placed with `transform` would stand with its north-west corner at column (`x`, `z`): its lowest corner on the ground (its middle column's), if the ground under it varies by at most `max_slope` and caves took none of it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L507)
- <a id="Plan-grow"></a>`.grow(start: &Template, corner: impl Into<BlockPos>, transform: Transform, pools: &Pools, depth: u32) -> usize`: Joins pieces from `pools` onto template `start` (placed at `corner` with `transform`) by their joints, up to `depth` steps out: each open joint takes a piece from its pool whose joint is called its `target`, faced back at it, that overlaps nothing placed so far and stays within the reach. How many pieces went in. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L515)

## Pools

Where `Plan::grow` takes pieces from: pools by name, each a weighted list of templates.

- <a id="Pools-new"></a>`.new() -> Pools<'a>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L394)
- <a id="Pools-pool"></a>`.pool(name: &'a str, pieces: &[(&'a Template, u32)])`: Pool `name`: these templates, by weight, placed as their joints say. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L398)
- <a id="Pools-pool_on_ground"></a>`.pool_on_ground(name: &'a str, pieces: &[(&'a Template, u32)])`: Pool `name`, each piece put down on the ground where it lands (streets over hills). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L403)

## PrecipitationBuilder

What falls from the sky, rain's numbers unless set.

- <a id="PrecipitationBuilder-texture"></a>`.texture(texture: &str)`: `textures/<texture>.png`, a strip of drops like vanilla's rain. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L506)
- <a id="PrecipitationBuilder-speed"></a>`.speed(share: f32)`: As a share of rain's (snow drifts at 0.1). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L511)
- <a id="PrecipitationBuilder-density"></a>`.density(share: f32)`: The share of the picture shown (rain 0.1). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L515)

## SkyBuilder

A thing in a dimension's sky (`Registry::sky_body`, `sky_stars`, `sky_dome`, `sky_clouds`), an entry named `<dimension>/<object>`: `r.edit(OVERWORLD.sky(&MOON))` changes another mod's.

- <a id="SkyBuilder-texture"></a>`.texture(name: &str)`: Picture, relative names are under `textures/sky/`. A dome tells its layout by its shape: twice as wide as high is a panorama, three by two is the six faces of a box. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L178)
- <a id="SkyBuilder-shader"></a>`.shader(shader: &crate::Shader)`: Colors it with `shaders/sky/<name>.wgsl` instead of the plain picture. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L183)
- <a id="SkyBuilder-orbit"></a>`.orbit(ticks: f32)`: Ticks per turn around the sky, negative the other way. Without it, it follows the dimension's day like the sun does. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L189)
- <a id="SkyBuilder-orbit_still"></a>`.orbit_still()`: Stays where its phase, tilt and heading put it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L193)
- <a id="SkyBuilder-orbit_day"></a>`.orbit_day()`: Back to following the dimension's day, for changing another mod's object. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L197)
- <a id="SkyBuilder-phase"></a>`.phase(turns: f32)`: Where on its orbit it starts, in turns: 0 rising in the east, 0.25 overhead, 0.5 setting. A moon opposite the sun is 0.5. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L202)
- <a id="SkyBuilder-tilt"></a>`.tilt(degrees: f32)`: Degrees its orbit leans away from overhead, to the south. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L206)
- <a id="SkyBuilder-heading"></a>`.heading(degrees: f32)`: Degrees its orbit turns around the vertical: 90 rises in the north instead of the east. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L210)
- <a id="SkyBuilder-size"></a>`.size(degrees: f32)`: Degrees across the sky (vanilla's sun 33, moon 23); of each star, for stars. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L214)
- <a id="SkyBuilder-spin"></a>`.spin(ticks: f32)`: Ticks per turn around its own middle. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L218)
- <a id="SkyBuilder-opacity"></a>`.opacity(day: f32, night: f32)`: How much of it shows by full day and by deep night, 0..1 (stars: 0 and 0.22). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L222)
- <a id="SkyBuilder-color"></a>`.color(rgb: u32)`: 0xRRGGBB multiplied in. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L226)
- <a id="SkyBuilder-frames"></a>`.frames(columns: u32, rows: u32)`: The picture is a grid of frames, shown one after another: a moon's phases. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L230)
- <a id="SkyBuilder-frame_ticks"></a>`.frame_ticks(ticks: u32)`: Ticks each frame lasts; by default a day of the dimension. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L234)
- <a id="SkyBuilder-glow"></a>`.glow(rgb: u32)`: Lights the horizon while rising and setting, a sunset in this color (0xRRGGBB). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L238)
- <a id="SkyBuilder-blend_add"></a>`.blend_add()`: Adds its light to what is behind it. Without it, it covers what is behind it, so a moon with an opaque picture hides any other that passes behind it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L243)
- <a id="SkyBuilder-blend_cover"></a>`.blend_cover()`: Covers what is behind it, for an object someone made `blend_add`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L247)
- <a id="SkyBuilder-order"></a>`.order(order: i32)`: Drawn low to high, one over the other; registration order among equals. Bodies start at 0, stars at -100, domes at -200. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L252)
- <a id="SkyBuilder-height"></a>`.height(y: f32)`: The height of clouds' underside (only clouds use it, like `drift`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L256)
- <a id="SkyBuilder-drift"></a>`.drift(blocks_per_tick: f32)`: Blocks per tick clouds drift along x, negative the other way. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L260)
- <a id="SkyBuilder-count"></a>`.count(n: u32)`: How many stars (only stars use it, like `seed`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L264)
- <a id="SkyBuilder-seed"></a>`.seed(seed: u32)`: What scatters the stars: another seed, another sky. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L268)

## Template

- <a id="Template-structure"></a>`.structure() -> &'static Feature`: The structure placing it (a feature named like it), for `Registry::edit`: `r.edit(WELL.structure()).spacing(96)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L198)

## TemplateBuilder

Layers from the bottom up, each a list of rows along z, each character one block along x: a key says which block it means, a space leaves what is there, `air` clears. An edit adds its blocks to the template's.

- <a id="TemplateBuilder-layer"></a>`.layer(rows: &[&str])` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3128)
- <a id="TemplateBuilder-key"></a>`.key(c: char, block: &str)`: `block` may name a state (`furnace[lit=true]`) or `air` to clear. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3134)
- <a id="TemplateBuilder-fill"></a>`.fill(block: &str, from: impl Into<BlockPos>, to: impl Into<BlockPos>)`: A box of `block` from `from` to `to`, inclusive (a moving body's deck). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3139)
- <a id="TemplateBuilder-slot"></a>`.slot(slot: &str, block: &str)`: What slot `slot` (a key `@slot` or `@slot:stairs[facing=north]`) takes unless a palette says otherwise. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3147)
- <a id="TemplateBuilder-entity"></a>`.entity(kind: &crate::EntityKind, pos: impl Into<crate::Vec3>)`: An entity of `kind` standing at `pos` (from the lowest corner) whenever it is placed. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3152)
- <a id="TemplateBuilder-loot"></a>`.loot(pos: impl Into<BlockPos>, table: &crate::LootTable)`: The block entity at `pos` (a chest) filled from loot table `table` whenever it is placed. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3160)
- <a id="TemplateBuilder-joint"></a>`.joint(pos: impl Into<BlockPos>, face: usize, name: &str, target: &str, pool: &str)`: A joint at `pos` facing `face` (`FACE_DIRS` index), where `Plan::grow` joins a piece from pool `pool` by its joint called `target`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3166)

## TerrainBuilder

- <a id="TerrainBuilder-stone"></a>`.stone(block: &str)`: The block the ground is made of. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L542)
- <a id="TerrainBuilder-base_height"></a>`.base_height(y: f64)`: The ground height where no landform adds anything (64 unless said). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L547)
- <a id="TerrainBuilder-sea"></a>`.sea(level: i32, water: &str)`: Everything up to `level` that isn't ground is `water`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L551)
- <a id="TerrainBuilder-caves"></a>`.caves(caves: Caves)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L555)
- <a id="TerrainBuilder-field"></a>`.field(name: &str, value: Expr)`: A named value over the terrain's columns, for biome rules (`BiomeRule::field`) and other expressions (`Expr::field`), worked out once per column. The name is used as written: give yours your mod's prefix (`mymod:aridity`). Declaring `temperature`, `humidity`, `continent` or another mod's field again replaces it for everything that uses it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L563)
- <a id="TerrainBuilder-landform"></a>`.landform(f: impl Into<Landform>)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L566)
- <a id="TerrainBuilder-landform_remove"></a>`.landform_remove(name: &str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L570)
- <a id="TerrainBuilder-landform_mask"></a>`.landform_mask(name: &str, mask: Expr)`: Limits the named landform further: its mask is multiplied by `mask`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L575)
- <a id="TerrainBuilder-rule"></a>`.rule(rule: BiomeRule)`: A rule after all others. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L580)
- <a id="TerrainBuilder-rule_before"></a>`.rule_before(rule: &str, new: BiomeRule)`: A rule tried right before the rule named `rule` (a biome's rule is named like the biome). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L585)
- <a id="TerrainBuilder-rule_after"></a>`.rule_after(rule: &str, new: BiomeRule)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L589)
- <a id="TerrainBuilder-rule_replace"></a>`.rule_replace(rule: &str, new: BiomeRule)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L593)
- <a id="TerrainBuilder-rule_remove"></a>`.rule_remove(rule: &str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L597)
- <a id="TerrainBuilder-rule_edit"></a>`.rule_edit(rule: &str, f: impl FnOnce(BiomeRule) -> BiomeRule)`: Changes what `f` sets of the rule named `rule`, the rest stays: `.rule_edit("base:desert", |r| r.height(..=90))`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L603)
- <a id="TerrainBuilder-cave_biome"></a>`.cave_biome(biome: &str, depth: i32, size: f64, threshold: f64)`: A biome in 3D: whole cubes at least `depth` blocks under the surface where a 3D noise of patches about `size` blocks across is above `threshold` (-1..1). Biomes are kept per column of each cube, so underground ones change every 16 blocks of height at most. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L611)
- <a id="TerrainBuilder-cave_biome_remove"></a>`.cave_biome_remove(biome: &str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L615)

## Transform

One of the 8 ways a template is placed: quarter turns clockwise seen from above, after a mirror across x. Blocks turn and mirror with it, a door's hinge and a stair's corner too.

- <a id="Transform-turned"></a>`.turned(quarters: u32) -> Transform`: `quarters` clockwise. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L27)
- <a id="Transform-mirrored"></a>`.mirrored() -> Transform`: The same, mirrored across x first. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L31)
- <a id="Transform-quarters"></a>`.quarters() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L34)
- <a id="Transform-is_mirrored"></a>`.is_mirrored() -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L37)
- <a id="Transform-all"></a>`.all() -> impl Iterator<Item = Transform>`: All 8. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L41)
- <a id="Transform-face"></a>`.face(face: usize) -> usize`: Which face its own face `face` (`FACE_DIRS` index) becomes. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L45)

## Vein

What a vein looks like (`FeatureBuilder::vein`). Every vein stays within 16 blocks of where it starts.

- <a id="Vein-walk"></a>`.walk(blocks: u32) -> Vein`: A random walk of `blocks` steps, the default. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L89)
- <a id="Vein-blob"></a>`.blob(blocks: u32) -> Vein`: A lump of about `blocks`, like modern vanilla's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L93)
- <a id="Vein-sphere"></a>`.sphere(radius: f32) -> Vein` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L96)
- <a id="Vein-disk"></a>`.disk(radius: f32, thickness: f32) -> Vein`: Flat and round (clay, gravel and sand under water). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L100)
- <a id="Vein-shape"></a>`.shape(shape: &crate::Shape) -> Vein`: Any shape about its start, as a fill takes (`Shape::ball(..).subtract(..)`: a geode's shell). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L104)

## WeatherBuilder

A weather being registered: rain's rules unless told otherwise.

- <a id="WeatherBuilder-no_falling"></a>`.no_falling()`: Nothing falls (by default rain, snow, each biome's own does). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L560)
- <a id="WeatherBuilder-lightning"></a>`.lightning()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L563)
- <a id="WeatherBuilder-no_shelter"></a>`.no_shelter()`: Things burn in the sun under it, a sandstorm (under rain they don't). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L567)
- <a id="WeatherBuilder-darkness"></a>`.darkness(share: f32)`: Share of daylight it takes away (rain 0.3125). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L571)
- <a id="WeatherBuilder-gray"></a>`.gray(sky: f32, fog: f32, clouds: f32)`: How far the sky (rain 0.75), the fog (0.5) and the clouds (0.95) go gray. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L575)
- <a id="WeatherBuilder-sound"></a>`.sound(sound: &str)`: A sound event played now and then around players under the open sky while it falls. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L581)
- <a id="WeatherBuilder-duration"></a>`.duration(min: u32, max: u32)`: Ticks it lasts, at least and at most. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L586)
- <a id="WeatherBuilder-weight"></a>`.weight(w: u32)`: How likely it comes after clear weather next to the others (rain 3, thunder 1); 0 only by command or code. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L591)
