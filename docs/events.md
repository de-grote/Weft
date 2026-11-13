# Events, chains and questions

Two mechanisms carry everything a mod hooks into: events, and values worked out step by step.

```rust
// Events: batched, delivered once per tick, nobody can cancel them.
static LEVEL_UP: Event<LevelUp> = Event::named("level_up");
r.on_event(&LEVEL_UP, |b| for e in b.iter() { /* do something */ });
w.emit(&LEVEL_UP, value);

// Values worked out step by step: damage, an attribute, a question, your own. A step is any code.
r.modify(&chain::DAMAGE, &phase::MULTIPLY, |m| m.value *= 0.5);
r.modify(&attribute::MOVEMENT_SPEED, &phase::ADD, |m| m.value += 0.1);

// A question is a value too, yes (1) or no (0). Each step says no, keeps what was said or says
// yes, and the last that doesn't keep wins.
r.before_event(&actions::BREAK_BLOCK, |q| if is_protected(q.pos()) { Verdict::Deny } else { Verdict::Abstain });
```

## Phases and ordering

Steps go by phase, never by who loaded first: `phase::BASE` (what the thing is), `ADD`, `PERCENT`, `MULTIPLY`, `RULES` (where `before_event` answers), `OVERRIDE`, `FINAL`. One mod setting every sword to 7 in `BASE` and another adding half for some armor in `MULTIPLY` come out right whichever loaded first. Inside a phase they go in load order unless one says otherwise, by name: `r.step(&SHARP).on(&attribute::ATTACK_DAMAGE).phase(&phase::ADD).after("othermod:blunt").run(|m| ..)`. A phase of your own goes after one of these, `r.phase(&CRITS, &phase::MULTIPLY)`. If two mods still disagree, the pack decides in `config/step_order.txt`, a chain a line: `core:attack_damage: othermod:blunt, mymod:sharp`. `w.chain_explain(..)`, `w.attribute_explain(..)` and `--log modifiers=debug` list every step and what it did.

A no past someone else's yes works the same way: armor that stops hits says no in `RULES`, the sword that always hits says yes in `OVERRIDE`. A deny nobody gets past (a claim) is a step in `FINAL`.

What the engine works out is steps of its own, `core:` ones, and you can reorder, recode or take out any of them: `r.edit(&Step::named("core:charge")).run(|m| m.value *= bell(m.query().charge))` makes a weapon hit hardest at half a swing, `r.remove(&Step::named("core:vanilla_crit"))` ends crits.

## How a hit is worked out

A melee hit: `question::ATTACK`, then the attacker's `attribute::ATTACK_DAMAGE` against the target (its own damage as vanilla's, 1 or its kind's `attack_damage`, plus what it holds over a bare hand by `core:weapon`, a 6 sword's 5, and strength's +3), then `chain::MELEE` (`core:charge` for how charged the swing was, `core:crit` times `attribute::CRIT_DAMAGE` when `question::CRIT` says so: `core:vanilla_crit`). Then the damage: `actions::HURT` (`core:immune`), `question::RECOVERING` (`core:hurt_cooldown`: `attribute::HURT_COOLDOWN`, 10 ticks; deny it for a weapon that ignores the pause), `chain::DAMAGE` (`core:resistance`, `core:not_lethal`), knockback from `chain::KNOCKBACK` (0.4, away from what hit), extra knockback from `attribute::ATTACK_KNOCKBACK` (a full swing while sprinting adds 1), and a sweep (`question::SWEEP`, `chain::SWEEP_DAMAGE`). Steps read the hit from the query: `charge`, `crit`, `direct` (an arrow), `item`.

## Attributes

Attributes are chains with a base: `r.attribute(&MANA, 100.0, 0.0, 1000.0)`. What things give them is a step with a number, a modifier: `Modifier::add`, `mul`, `set` (in `OVERRIDE` unless `.phase(..)` says; `FINAL` still comes after), `percent` (vanilla's percents, which add up: speed and slowness are +5%, not 1.2 x 0.85), and `Modifier::step(&A, &MY_STEP, 2.0)` runs a step of yours (`r.step(&MY_STEP).run(..)` without `.on(..)`) with 2.0 as `m.arg`. `w.attribute_add_modifier(id, "spell", &Modifier::mul(&attribute::MOVEMENT_SPEED, 0.5), 200)` puts one on an entity for ten seconds, saved with it.

See [attributes](details/attributes.md) for the engine's own.

## Timers and ticks

Two helpers go with them. `w.timer(holder, ticks, &EVENT, value)` fires an event later, kept with whatever holds it: an entity (a calm-down after being hurt) and gone with it, a block (a fuse), or `Holder::NONE` for the dimension (an invasion at dawn). Time away counts, so a timer due while its holder wasn't loaded fires as soon as it is. A block's own tick is the same timer with the engine's `BLOCK_TICK` event, which is all `w.schedule_tick(pos, ticks)` does. `r.world_tick(|w| ..)` runs once per tick in every region for logic that is nobody's own, `w.players()` being the players there.

Keeping someone quiet is a question too: `r.before_event(&actions::CHAT, |q| if q.text().contains("spam") { Verdict::Deny } else { Verdict::Abstain })`. Reading what was said is `r.on_event(&actions::CHAT, ..)` (`e.text()`, `e.player`).

## Built-in events

Actions, each a question and an event under one name (`r.before_event(&actions::X, ..)`, `r.on_event(&actions::X, ..)`): `BREAK_BLOCK`, `PLACE_BLOCK`, `USE_BLOCK`, `USE_ITEM`, `INTERACT_ENTITY`, `HURT`, `DIE`, `PICKUP_ITEM`, `ENTER_DIMENSION`, `CHAT`, `MOUNT`, `DISMOUNT`, `SPAWN`, `EFFECT`, `PROJECTILE_HIT`. Events alone: `PLAYER_JOINED`, `PLAYER_RESPAWNED`, `ENTITY_HEALED`, `WEATHER_TICK`, `ENTITY_REMOVED`, `EFFECT_ENDED`, `KEY`. Questions alone: `question::ATTACK`, `CRIT`, `SWEEP`, `RECOVERING`, `CAMERA`, `FORCE_LOAD`. Chains: `chain::DAMAGE`, `MELEE`, `KNOCKBACK`, `SWEEP_DAMAGE`, `PROJECTILE_DAMAGE`, `FALL_DAMAGE` (hay, a trampoline: a step reading what is under the faller), `HEAL`, and every attribute.\
base adds `base:smelted`.

## Systems and energy

Systems run over every entity with a component all at once, and that is how block entities tick:
`r.system(&BURNER, |batch| for (id, m, ticks) in batch.iter_mut() { .. })`. `ticks` is 1 unless it is catching up on time it missed (unloaded, the time jumped): then it should do that many ticks of work at once, see [block entities](block_entities.md#catching-up).

Energy is a built-in engine contract, you can give a block entity the `core:energy` component, tag
cables with `conductor`, and every mod's generators and machines will work together.\
Networks balance once per tick.
