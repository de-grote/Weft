# Entities in the world

What `w` can do with entities: spawning and finding them, making them walk, and changing what they are right now. Numbers like speed or reach are [attributes](attributes.md).

## Spawning, finding, hurting

- `spawn(&KIND, pos)` and `spawn_with(&KIND, pos, &Spawn::new().velocity(v))` for one that starts moving
- `spawn_item` for a dropped stack (`spawn_with(&ITEM, ..)` with `.stack(s)` throws it)
- `entity(id)`, `entities_near`, and `targets_near` for what can be hit
- `damage`, `heal`, `push` (a trampoline or a blast, players too)
- `game_mode`, `creative`, `outside_survival` for a mode that spares damage and hunger, `set_game_mode(player, &game_modes::CREATIVE)`
- `navigate` for pathfinding

## Walking like a player

`w.walk(id, Walk::toward(dir, 1.0).sprint().jump())` holds the keys a player would: `.jump()`, `.sneak()` (careful at edges, holds on to ladders, crouches, flies down), `.sprint()`, `.fly()` (with `attribute::CAN_FLY` 1 or more), and the same physics answers; with `.poses()` on its kind the player's poses do too: sprinting into water it swims lying down, under a low ceiling it crouches. Speed and jump are attributes (`attribute::MOVEMENT_SPEED`, `JUMP_STRENGTH`) for mobs and players alike: `w.attribute_add_modifier(player, &MOVEMENT_SPEED, ..)` is a speed effect, and `attribute::FOV` widens or narrows a player's view (a zoom, a speed rush).

## Changing what an entity is now

`w.entity_modify(id)` changes what an entity is now, only what you name, players' clients following: `.pos(..)`, `.velocity(..)`, `.yaw(..)`, `.pitch(..)`, `.health(..)`, `.fall_distance(0.0)` (a mace's smash, a parachute), `.flying(true)`, `.fire(ticks)`, `.selected(slot)`, `.persistent(true)`. Testmod's mace is a whole weapon on top of that and `attribute::ATTACK_DAMAGE`.
