# Liquids

`r.liquid(&LIQUID)` and then the calls below.

- `block(|b| ..)`: its block like any other, on top of what makes it a liquid: `b.translucent()` to see through it (water, otherwise drawn solid like lava), `tint_kind`, `light`, `light_opacity`, `drag` (how fast things move inside: water 1, lava 0.4), `texture` (`<name>_still` by default), and its tags (`tag("water")`), which its item gets too
- `flow_delay`, `drop_off`, `slope_find`, `flows_up()`
- `renewable(n)`: a new source where n sources meet (water 2)
- `heat` (roughly Kelvin, 1000 and more burns)
- what can swim in it isn't the liquid's say: an entity kind's `SWIMS_IN` (which blocks, base gives players `#water`) and the same on anything it wears, so a suit with `.set_text(&SWIMS_IN, "#lava")` swims in lava. It changes nothing else: a burning liquid keeps burning unless `actions::HURT` says no
- `fog(rgb, blocks)` and `overlay(texture, opacity)` or `no_overlay()`: what you see from inside. By default both come from its texture; set them when the inside should look different from the surface: `r.liquid(&TAR).fog(0x1A1408, 6.0).overlay("tar_inside", 0.4)`
- `bucket(..)`

Another mod's liquid: `r.edit(WATER.liquid()).fog(0x204020, 12.0)`, any of the above; what you don't set stays, and `.block(..)` there edits its block.

## Reactions

Reactions: `r.liquid_reaction("#hot", "#water").source_turns_into("obsidian")`, named `base:hot>water` for others' `edit` and `remove` (`.named(..)` for one of your choice).\
Every liquid is also an item of its name and tags, its count in millibuckets: what liquid slots (`Slots::liquid(capacity)`) hold and recipes count, see the [block entities guide](../block_entities.md).
