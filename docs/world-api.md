# World API

Positions come back as `BlockPos` (a block, `x`, `y`, `z` whole numbers) and `Vec3` (a point or a speed, in blocks); `p.offset(0, 1, 0)`, `p.above()`, `p.center()` and `v.block()` move between them. Anything that takes a position takes a plain array too: `w.set_block([x, y + 1, z], s)`.

Inside any callback you get a `World` (`w`). What it can do is grouped by what it works on:

| Page | What is in it |
|---|---|
| [Blocks](details/world-blocks.md) | reading and changing blocks, `fill` with shapes |
| [Entities](details/world-entities.md) | spawning, finding, walking, `entity_modify` |
| [Attributes](details/attributes.md) | speed, reach, fall damage and the rest, modifiers that come with things |
| [Raycasts and hitboxes](details/hitboxes.md) | what a line hits first, parts of a body |
| [Conditions](details/conditions.md) | the predicate language shared by music, loot, spawning, poses |
| [Poses and game modes](details/poses-and-modes.md) | |
| [Particles and sounds](details/particles-sounds.md) | |
| [Events, chains and questions](events.md) | hooking into what the game does |
| [Text](details/text.md), [commands](details/commands.md) | |

## Seeds

The world's seed (`seed`, and `seed_at(pos)`, `seed_of_cube(pos)` for a number that belongs to a place: what a chest holds when first opened, a villager's trades)

## Actions

What happens in the world comes as actions, a question before and an event after under one handle: `r.before_event(&actions::DIE, ..)` is a totem (denied, it lives on with one health), `r.on_event(&actions::INTERACT_ENTITY, ..)` hears every right click on an entity. A mod's own is `static WRENCH: Action<Wrenched> = Action::named("wrench")`, and `w.act(&WRENCH, &query, value)` asks, then tells everyone if the answer is yes

## Stacks and inventories

For stacks, `s.add_wear(1)` wears one down, and in a block entity's tick `output.room_for(result)` and `output.merged(result, n)` are what an output slot takes

Inventories (`inventory(h).get/set/insert`, `inventory(h).group("fuel")`), where `h` is an entity id, a block position or `Holder::held(player, slot)` for a backpack, or `STORAGE.of(h)` for a storage it carries (`r.storage(&S, slots)`, an ender chest's slots in every player); a player's by the groups its inventory was declared with (`r.player_slots().group("hotbar", Slots::row(9).hotbar())..`, the mod loaded last wins), `core:equipment` for all equipment slots

## Game rules

Game rules: settings of the world, `r.config(&HARD_MODE).bool(false).world()`, read with `HARD_MODE.get()`, changed with `/rule hard_mode 1` or in World Rules (when the world is made, and in game for whoever may `/rule`). What is the same for every player is a rule, what one player has apart is an attribute: `r.attribute(&THIRST_RATE, 1.0, 0.0, 10.0).rule()` makes the rule `mymod:thirst_rate` the base of it for players and every kind that doesn't set its own, and `/attribute mymod:thirst_rate 2` (a `Modifier::set`, at the end of the chain) sets one entity's. The engine's are `core:max_health`, `core:keep_inventory`, `core:mob_spawning`, `core:max_entity_cramming` (more than this many pushing on one hurt it, 24; 0 never), `core:lives` (deaths before going on as a spectator, 1 for hardcore), `core:rules_locked`, and a `core:damage.<cause>` for every damage cause, yours too (`core:damage.mymod.radiation`), to turn it off

## Gizmos

Gizmos for debugging (`w.draw_line/box/fill/shape/text(Some(&GROUP), ..)`, seen while a player shows the group, or `None` for always; `.ticks(n)` keeps the server's longer, a client's last a frame)

## Plain helpers

And a few plain helpers: `block_center(p)`, `block_floor(p)`, `block_pos_of(pos)`, `st.eye()`, `yaw_toward(from, to)`, `yaw_dir(yaw)`, `look_dir(yaw, pitch)`, `ratio(a, b)`. `EntityState`'s yaw and pitch are radians; what you pass to the API (`ModelPart::rotation`, `set_pose` facing, `Client::set_look`) is in degrees
