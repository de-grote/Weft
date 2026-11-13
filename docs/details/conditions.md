# Conditions

Conditions are one predicate language for music, text, loot, spawning, poses and camera modes. A condition is a name with maybe an argument, put together with `&`, `|`, `!` and parentheses: `night & light(..7) & !block(#mycelium)`.

## Built in

| Condition | True when |
|---|---|
| `day`, `night` | the time of day |
| `underground`, `surface` | |
| `light(min..max)` | the light level is in range |
| `chance(0.05)` | a roll succeeds |
| `block(#grass)`, `state(age=7)` | the block asked about is such: the one broken, the one under a spawn |
| `tool(tools/shears)`, `tool_level(2)` | what the actor holds |
| `killed_by_player`, `on_fire`, `harvested` | about the actor (`harvested` is for loot) |
| `in_liquid`, `flying`, `sprinting` | about the actor, handy for poses |
| `attribute(core:can_fly, 1..)` | an attribute of who it is about is in range (`Cond::attribute(&attribute::CAN_FLY, 1.0, f64::MAX)`) |
| `wears(head)` | something is worn over that bone, or in the equipment slot of that name: a helmet keeps the sun off a zombie |
| `creative` and the rest | every game mode by its name |

The engine's go without their namespace.

## In code

In code it is a `Cond`: `Cond::light(0, 7) & !Cond::from(&conditions::DAY)`, `Cond::new("night | underground")`, and `.when(..)` on a pool, an entry, a spawn rule, a pose, music or a camera mode takes one (again for more, all of which hold); `{if:night & surface}` in text.

## Your own

Your own: `r.condition(&BLOOD_MOON, |c| ..)` answers on whichever side asks (`c.world()` on the server, `c.client()` on the client, with `c.pos()`, `c.entity()` and `c.arg()`, what `mymod:blood_moon(3)` has between its parentheses). `w.holds(&cond, pos, entity)` asks one from code, and an AI trigger takes one with `.only_while(..)`
