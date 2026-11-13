# Attributes and modifiers

A number a body used to have as a constant is an attribute, so a modifier is all a long tool, a haste effect or a ring of flight takes. Client and server move a player by the same ones.

Reach and mining speed are attributes too (`attribute::BLOCK_REACH`, `ENTITY_REACH`, `MINING_SPEED`), and the client reads them: a modifier is all a long tool or a haste effect takes.

| Attribute | What it is |
|---|---|
| `MOVEMENT_SPEED`, `JUMP_STRENGTH` | speed and jump, mobs and players alike; `attribute_add_modifier` with one is a speed effect |
| `FOV` | widens or narrows a player's view (a zoom, a speed rush) |
| `BLOCK_REACH`, `ENTITY_REACH`, `MINING_SPEED` | how far a hand reaches and how fast it digs; the client reads them too |
| `SAFE_FALL` | blocks that can be fallen for free (3) |
| `FALL_DAMAGE_MULTIPLIER` | damage per block past that |
| `STEP_HEIGHT` | 0.6 |
| `GRAVITY` | a multiplier of the dimension's, below 0 floats up |
| `KNOCKBACK_RESISTANCE` | |
| `ATTACK_DAMAGE` | its own melee damage, what it holds adding to it |
| `CRIT_DAMAGE` | 1.5 |
| `ATTACK_KNOCKBACK`, `SWEEPING_DAMAGE_RATIO` | |
| `HURT_COOLDOWN` | ticks of grace after a hit (10) |
| `DAMAGE_RESISTANCE`, `FIRE_RESISTANCE` | shares taken off |
| `BURNING_TIME` | |
| `SUBMERGED_MINING_SPEED`, `AIRBORNE_MINING_SPEED` | 0.2 each |
| `SWIM_SPEED`, `FLYING_SPEED` | multipliers, 1; flying goes on top of a player's own wheel speed |
| `AIR_CONTROL` | how well it steers in the air |
| `SLIPPERINESS` | skates on any ground |
| `CLIMB_SPEED` | ladders |
| `CAN_FLY` | 1 or more flies, 1 in creative: a ring of flight is a modifier |
| `CAN_SPRINT` | |

## Making your own

Attributes are chains with a base: `r.attribute(&MANA, 100.0, 0.0, 1000.0)`. What things give them is a step with a number, a modifier: `Modifier::add`, `mul`, `set` (in `OVERRIDE` unless `.phase(..)` says; `FINAL` still comes after), `percent` (vanilla's percents, which add up: speed and slowness are +5%, not 1.2 x 0.85), and `Modifier::step(&A, &MY_STEP, 2.0)` runs a step of yours (`r.step(&MY_STEP).run(..)` without `.on(..)`) with 2.0 as `m.arg`. `w.attribute_add_modifier(id, "spell", &Modifier::mul(&attribute::MOVEMENT_SPEED, 0.5), 200)` puts one on an entity for ten seconds, saved with it.

## Modifiers that come with things

Modifiers also come with things, kept up by the engine: `r.item(&BOOTS).modifier(Modifier::add(&attribute::MOVEMENT_SPEED, 0.2).worn())` while worn (without `.worn()`: while in the main hand), `r.pose(&SIT).modifier(..)` while in that pose, and `r.edit(&PLAYER).modifier(Modifier::add(&attribute::CAN_SPRINT, -1.0).when(&HUNGRY))` while a condition holds.
