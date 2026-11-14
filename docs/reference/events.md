# Events and steps reference

Events, actions, steps, phases, modifiers and what a callback gets as its argument.

## Action

Something that happens, as one name: the question asked before it (`r.before_event(&A, ..)`) and the event after (`r.on_event(&A, ..)`). The engine's are in `actions`; a mod's own goes through `w.act(&WRENCH, &mut query, value)`.

- <a id="Action-named"></a>`.named(name: &'static str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L335)

## Ask

A question being asked (`Registry::before_event`); `answer` is what the steps before said.

- <a id="Ask-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L446)
- <a id="Ask-question"></a>`.question() -> &str` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L449)
- <a id="Ask-query"></a>`.query() -> &abi::Query` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L452)
- <a id="Ask-entity"></a>`.entity() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L455)
- <a id="Ask-pos"></a>`.pos() -> BlockPos` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L458)
- <a id="Ask-state"></a>`.state() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L461)
- <a id="Ask-text"></a>`.text() -> &str`: What a player is about to say (`actions::CHAT`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L465)
- <a id="Ask-item"></a>`.item() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L468)
- <a id="Ask-is_camera"></a>`.is_camera(mode: &crate::CameraMode) -> bool`: Whether `question::CAMERA` asks about `mode`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L472)

## AttributeBuilder

An attribute being registered (`Registry::attribute`).

- <a id="AttributeBuilder-rule"></a>`.rule()`: The base of it for kinds that don't set their own (the player) is a world rule named like it (`/rule`), with its default and range; `/attribute` sets one entity's apart. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L990)

## BlockEvent

- <a id="BlockEvent-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3511)
- <a id="BlockEvent-pos"></a>`.pos() -> BlockPos` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3514)
- <a id="BlockEvent-state"></a>`.state() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3517)
- <a id="BlockEvent-neighbor"></a>`.neighbor() -> BlockPos`: The neighbor that changed, for `on_neighbor`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3521)
- <a id="BlockEvent-body_pos"></a>`.body_pos() -> BlockPos`: Where the block is on the body, for `on_carried`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3525)
- <a id="BlockEvent-entity"></a>`.entity() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3528)

## Damage

A hurt described (`World::damage`): `Damage::new(4.0, &damage_causes::ATTACK).by(shooter) .through(arrow)`. Steps see all of it (`Query::other`, `direct`, `item`, `crit`).

- <a id="Damage-new"></a>`.new(amount: f32, cause: impl Into<Id<crate::DamageCause>>) -> Damage` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L93)
- <a id="Damage-by"></a>`.by(source: u64)`: Who did it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L97)
- <a id="Damage-through"></a>`.through(direct: u64)`: What hit, when not `by` itself (the arrow): knockback goes away from it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L102)
- <a id="Damage-with"></a>`.with(item: ItemStack)`: What it was done with. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L107)
- <a id="Damage-knock"></a>`.knock(way: impl Into<Vec3>)`: The way it pushes, instead of away from what hit. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L112)
- <a id="Damage-crit"></a>`.crit(mut self)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L116)
- <a id="Damage-part"></a>`.part(part: u32)`: The part of the hitbox it lands on (`hitbox_part::HEAD`, `hitbox_part_id("tail")`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L121)

## EffectTick

An effect's tick (`EntityEffectBuilder::on_tick`): every entity that has it, with its level.

- <a id="EffectTick-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L773)
- <a id="EffectTick-effect"></a>`.effect() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L776)
- <a id="EffectTick-iter"></a>`.iter() -> impl Iterator<Item = (u64, u32)> + '_`: (entity, level). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L780)

## EntityEvent

- <a id="EntityEvent-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L756)
- <a id="EntityEvent-entity"></a>`.entity() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L759)
- <a id="EntityEvent-other"></a>`.other() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L762)

## EntityTick

- <a id="EntityTick-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L743)
- <a id="EntityTick-ids"></a>`.ids() -> &[u64]` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L746)

## Event

A typed event, named like any other thing a mod owns: `Event::named("eaten")` is this mod's, `Event::named("base:eaten")` another's.

- <a id="Event-named"></a>`.named(name: &'static str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L252)
- <a id="Event-name"></a>`.name() -> &str` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L255)
- <a id="Event-id"></a>`.id() -> u32`: `u32::MAX` if the event is unknown or registries are not ready yet. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L259)

## EventBatch

Every value of one event queued during a tick, in the order they happened.

- <a id="EventBatch-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L414)
- <a id="EventBatch-iter"></a>`.iter() -> impl Iterator<Item = &T>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L417)
- <a id="EventBatch-len"></a>`.len() -> usize` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L420)
- <a id="EventBatch-is_empty"></a>`.is_empty() -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L423)

## Gate

Which faces of a `BlockBuilder::signal_gate` read power and give it (strongly, into the block there and on), and its delay.

- <a id="Gate-new"></a>`.new() -> Gate`: Reads behind it and both sides, gives out of the front, one redstone tick late. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3451)
- <a id="Gate-inputs"></a>`.inputs(sides: &[Side]) -> Gate`: The faces it reads; an output is never an input. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3455)
- <a id="Gate-outputs"></a>`.outputs(sides: &[Side]) -> Gate`: The faces it gives at. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3460)
- <a id="Gate-delay"></a>`.delay(ticks: u32) -> Gate`: In redstone ticks, two game ticks each. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3466)

## GateInputs

What comes in at a `BlockBuilder::signal_gate`'s faces.

- <a id="GateInputs-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3487)
- <a id="GateInputs-pos"></a>`.pos() -> BlockPos` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3490)
- <a id="GateInputs-side"></a>`.side(side: Side) -> u8`: Power coming in at `side`, 0 for a face it doesn't read. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3494)
- <a id="GateInputs-output"></a>`.output() -> u8`: What it gives now. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3498)

## ItemUse

- <a id="ItemUse-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3605)
- <a id="ItemUse-entity"></a>`.entity() -> u64`: Who uses it: a player, or a mob (`World::use_item`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3609)
- <a id="ItemUse-target_block"></a>`.target_block() -> Option<(BlockPos, u32)>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3612)
- <a id="ItemUse-held_ticks"></a>`.held_ticks() -> u32`: Ticks the use button was held, for `on_release` (a bow's draw); 0 for a use at once. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3616)
- <a id="ItemUse-stack"></a>`.stack() -> crate::ItemStack` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3619)
- <a id="ItemUse-slot"></a>`.slot() -> u32`: The slot of its inventory the stack is in. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3623)
- <a id="ItemUse-holder"></a>`.holder() -> crate::Holder`: The used stack as a holder: what a backpack opens its menu on. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3627)
- <a id="ItemUse-set_stack"></a>`.set_stack(s: crate::ItemStack)`: Replaces the held stack (use count 0 to consume it entirely). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3631)

## Modifier

A step with a number on an attribute, that comes with something (`.modifier(..)` on an item, a pose, an entity kind, an effect) or is put on an entity (`World::attribute_add_modifier`): `Modifier::add(&attribute::MOVEMENT_SPEED, 0.2)`, while worn (`.worn()`) or while conditions hold for its holder (`.when(..)`). Any code of your own is a step too: `Modifier::step(&A, &MY_STEP, 2.0)` runs `MY_STEP` (`Registry::step`) with 2.0 as `Modify::arg`.

- <a id="Modifier-step"></a>`.step(attribute: &crate::Attribute, step: &Step, amount: f64) -> Modifier` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L206)
- <a id="Modifier-add"></a>`.add(attribute: &crate::Attribute, amount: f64) -> Modifier` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L209)
- <a id="Modifier-mul"></a>`.mul(attribute: &crate::Attribute, factor: f64) -> Modifier` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L212)
- <a id="Modifier-set"></a>`.set(attribute: &crate::Attribute, value: f64) -> Modifier`: The value becomes `value`, at the end (`phase::OVERRIDE`) unless `.phase(..)` says. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L216)
- <a id="Modifier-percent"></a>`.percent(attribute: &crate::Attribute, share: f64) -> Modifier`: Adds `share` of the value its phase started from: vanilla's percent modifiers, which add up (speed and slowness together are +5%, not 1.2 x 0.85). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L221)
- <a id="Modifier-phase"></a>`.phase(phase: &Phase) -> Modifier`: Runs in `phase` rather than its step's own. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L225)
- <a id="Modifier-worn"></a>`.worn(mut self) -> Modifier`: An item's, while it is in an equipment slot rather than in the main hand. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L230)
- <a id="Modifier-when"></a>`.when(c: impl Into<Cond>) -> Modifier` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L234)

## Modify

One step's run (`Registry::modify`, `Registry::step`): read the context, change `value`. `base` is where the chain began, `start` where this phase began, `arg` the number a modifier gave the step (0 on a chain). A question's value is 1 for yes, 0 for no.

- <a id="Modify-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L491)
- <a id="Modify-query"></a>`.query() -> &abi::Query` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L494)
- <a id="Modify-chain"></a>`.chain() -> &str`: The chain it runs in: one step may be on several (a modifier on any attribute). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L498)

## Neighbors

The blocks around one (`BlockBuilder::derived`).

- <a id="Neighbors-pos"></a>`.pos() -> BlockPos` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1931)
- <a id="Neighbors-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1934)
- <a id="Neighbors-at"></a>`.at(side: crate::Dir) -> u32`: The block on that side, air where nothing is loaded. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1938)

## PhaseBuilder

A phase being registered (`Registry::phase`).

- <a id="PhaseBuilder-after"></a>`.after(phase: &Phase)`: Comes right after `phase` (after phases that said so before it). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L565)

## Place

- <a id="Place-pos"></a>`.pos() -> BlockPos` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3572)
- <a id="Place-face"></a>`.face() -> crate::Dir`: Face of the block that was clicked. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3576)
- <a id="Place-look_dir"></a>`.look_dir() -> crate::Vec3` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3579)
- <a id="Place-hit"></a>`.hit() -> crate::Vec3`: Just outside where it was clicked, 0..1 from this cell's corner: for a slab, `hit().y > 0.5` is the upper half, on a floor, under a ceiling, against a wall or on another slab alike. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3584)
- <a id="Place-placed"></a>`.placed(prop: &str) -> Option<&'static str>`: In `merge`: a property of how its item would go. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3588)

## SignalEvent

What `BlockBuilder::on_signal` is told.

- <a id="SignalEvent-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3395)
- <a id="SignalEvent-pos"></a>`.pos() -> BlockPos` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3398)
- <a id="SignalEvent-state"></a>`.state() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3401)
- <a id="SignalEvent-level"></a>`.level() -> u8`: Power around it now, 0..15. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3405)
- <a id="SignalEvent-level_before"></a>`.level_before() -> u8`: What it was. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3409)
- <a id="SignalEvent-rising"></a>`.rising() -> bool`: It just got power. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3413)

## StepBuilder

A step being registered (`Registry::step`, `modify`, `before_event`).

- <a id="StepBuilder-on"></a>`.on(chain: &impl Valued)`: Runs every time `chain` is computed; without, only where a modifier names it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L527)
- <a id="StepBuilder-phase"></a>`.phase(phase: &Phase)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L530)
- <a id="StepBuilder-after"></a>`.after(name: &str)`: Runs after step `name` when both are in one phase of a chain. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L534)
- <a id="StepBuilder-before"></a>`.before(name: &str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L537)
- <a id="StepBuilder-run"></a>`.run(f: impl Fn(&mut Modify) + Send + Sync + 'static)`: What it does to the value. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L541)
- <a id="StepBuilder-answer"></a>`.answer(f: impl Fn(&Ask) -> Verdict + Send + Sync + 'static)`: What it says to a question. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L545)

## SystemBatch

Every entity of a region that has a component, with the values side by side, and the ticks each call stands for: 1 normally, more while an entity catches up on time it missed (its region was unloaded, the world's time jumped). A system worth catching up does the work of `ticks` ticks at once (a furnace: so many items smelted), anything else may ignore it.

- <a id="SystemBatch-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L953)
- <a id="SystemBatch-ids"></a>`.ids() -> &[u64]` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L956)
- <a id="SystemBatch-values"></a>`.values() -> &mut [T]` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L959)
- <a id="SystemBatch-ticks"></a>`.ticks() -> &[u32]` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L962)
- <a id="SystemBatch-iter_mut"></a>`.iter_mut() -> impl Iterator<Item = (u64, &mut T, u32)>`: (entity, its value, ticks to do). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L966)

## Use

- <a id="Use-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3541)
- <a id="Use-pos"></a>`.pos() -> BlockPos` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3544)
- <a id="Use-state"></a>`.state() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3547)
- <a id="Use-face"></a>`.face() -> crate::Dir`: Face of the block that was clicked. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3551)
- <a id="Use-player"></a>`.player() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3554)
- <a id="Use-sneaking"></a>`.sneaking() -> bool`: The player sneaks: only with `.sneak_use(..)` saying so, or with empty hands. Returning false passes the click on to what it holds. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3559)
