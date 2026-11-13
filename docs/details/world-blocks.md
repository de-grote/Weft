# Blocks in the world

Reading and changing blocks from a `World`: `block`, `set_block`, `schedule_tick`, `prop`, `prop_int`, `prop_set` and `prop_cycle` (a lever, a repeater's delay) do what the names say. For anything big there is `fill`.

## Filling shapes

`w.fill(shape, block)` fills a whole shape in one call (a crater with air). Shapes are values: `Shape::ball`, `cuboid`, `cylinder`, `ellipsoid`, `noisy`, `blast` (a ragged crater like vanilla's explosions leave), combined with `.union`, `.intersect` and `.subtract` (a geode is a ball minus a smaller one); `.keep("#unbreakable #liquids")` and `.only("#stone")` choose what changes. The same shapes pick entities (`w.entities_in(&shape, EntityFilter::any().targets().touching())`, a conveyor's top, an arrow's tip), outline themselves with `w.draw_shape` and place particles (`.area(&shape)` on a burst). The engine does a fill at the end of the tick on every core, a cube at a time, lights each cube once, loads what it opens up in players' views right away and gives the rest the change when it loads. Players see it appear whole. Hundreds of blocks of radius take well under a second, where `set_block` per block costs about 1.6 microseconds each. Testmod's dynamite uses it
