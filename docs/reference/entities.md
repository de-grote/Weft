# Entities reference

Entity kinds, their AI, riding, status effects, poses, game modes and damage causes.

## CategoryBuilder

An entity category being registered (`Registry::entity_category`).

- <a id="CategoryBuilder-cap_radius"></a>`.cap_radius(blocks: f32)`: How close to a player its mobs count for the cap (64 by default). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2182)

## DamageCauseBuilder

`r.damage_cause(&RADIATION).bypasses_armor().death_message("death.mymod.radiation")`.

- <a id="DamageCauseBuilder-bypasses_armor"></a>`.bypasses_armor()`: Armor doesn't soften it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L818)
- <a id="DamageCauseBuilder-bypasses_blocking"></a>`.bypasses_blocking()`: Held items cannot block it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L822)
- <a id="DamageCauseBuilder-pierces_invulnerable"></a>`.pierces_invulnerable()`: Hurts invulnerable entities and players outside survival too. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L826)
- <a id="DamageCauseBuilder-ignores_cooldown"></a>`.ignores_cooldown()`: Lands in full even right after another hit. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L830)
- <a id="DamageCauseBuilder-burning"></a>`.burning()`: What dies of it drops cooked food. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L834)
- <a id="DamageCauseBuilder-no_knockback"></a>`.no_knockback()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L837)
- <a id="DamageCauseBuilder-silent"></a>`.silent()`: No hurt sound. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L841)
- <a id="DamageCauseBuilder-not_lethal"></a>`.not_lethal()`: Never kills: it leaves half a heart (poison). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L845)
- <a id="DamageCauseBuilder-death_message"></a>`.death_message(key: &str)`: Translation key said when a player dies of it, `{0}` their name. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L849)
- <a id="DamageCauseBuilder-death_message_by"></a>`.death_message_by(key: &str)`: The key when something did it, `{1}` that thing's name. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L853)

## EntityBuilder

- <a id="EntityBuilder-modifier"></a>`.modifier(m: crate::Modifier)`: An attribute modifier its entities have, `.when(..)` while conditions hold (`Modifier`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L986)
- <a id="EntityBuilder-attach"></a>`.attach<T: Value>(component: &Component<T>)`: Every entity of this kind carries `component` (declared with `r.entity_component`, by any mod), starting at the kind's own value of it (`.set`), else its declared one; another mod's kind with `r.edit(&PLAYER).attach(..)`. Not `.set`: that is one value of the kind itself, `.attach` gives each entity its own, which changes. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L994)
- <a id="EntityBuilder-attach_with"></a>`.attach_with<T: Value>(component: &Component<T>, value: T)`: `attach` with another starting value. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L998)
- <a id="EntityBuilder-attach_storage"></a>`.attach_storage(storage: &Storage)`: Every entity of the kind carries `storage` (declared with `r.storage`), empty at first. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1002)
- <a id="EntityBuilder-solid"></a>`.solid()`: A solid body: others stand on it and ride along, bump into it and get pushed by it (platforms, boats, lifts). Not pushed around itself. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1010)
- <a id="EntityBuilder-invulnerable"></a>`.invulnerable()`: Takes no damage (no hurt flash, health stays); `/clear` and the void still remove it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1014)
- <a id="EntityBuilder-unpushable"></a>`.unpushable()`: Neither pushes nor is pushed by other entities (ghosts, projectiles). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1018)
- <a id="EntityBuilder-kinematic"></a>`.kinematic()`: Moved exactly by the velocity mod logic gives it (`entity_modify`), through blocks, no gravity: lifts, airships, contraptions. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1023)
- <a id="EntityBuilder-hazard"></a>`.hazard(h: crate::Hazard)`: What it does to entities: itself (`Hazard::own`) or what it squeezes (`Hazard::crushed`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1027)
- <a id="EntityBuilder-crushing"></a>`.crushing()`: A solid body that hurts what it squeezes into blocks instead of only pushing it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1031)
- <a id="EntityBuilder-burns_in_sun"></a>`.burns_in_sun()`: Daylight from an open sky sets it burning for 8 seconds, as the undead in vanilla, unless it wears something on its head. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1036)
- <a id="EntityBuilder-carry_height"></a>`.carry_height(blocks: f32)`: Riders up to `blocks` above the body (jumping, flying around inside it) keep moving with it and their own motion stays relative to it, so a jump on a fast ship lands on the ship. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1046)
- <a id="EntityBuilder-light_at"></a>`.light_at(x: f32, y: f32, z: f32)`: Where the light it gives comes from: blocks from its feet, +Z ahead, turned with it (a lantern on its head). Its middle when not said, a block entity's block. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1051)
- <a id="EntityBuilder-shown_slots"></a>`.shown_slots(slots: &[u32])`: Shares these inventory slots as public stacks, preserving order and empty slots. Clients read them with `Client::displayed` (`DisplaySource::Shown`); their light is independent of whether a model draws them. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1057)
- <a id="EntityBuilder-size"></a>`.size(width: f32, height: f32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1060)
- <a id="EntityBuilder-eye_height"></a>`.eye_height(blocks: f32)`: Eye height in blocks, `EYE_SHARE` of its height when not said. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1064)
- <a id="EntityBuilder-no_egg"></a>`.no_egg(mut self)`: No spawn egg. Every entity has one otherwise, `<name>_spawn_egg`, called after it and looking generic until `item/<name>_spawn_egg.png` exists: for what isn't a creature (a projectile, a moving platform). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1070)
- <a id="EntityBuilder-own_effects"></a>`.own_effects()`: The client makes no steps, idle noises or death smoke for it: `r.client_entity_frame` does its effects instead. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1076)
- <a id="EntityBuilder-effect"></a>`.effect(event: &str, sound: &str, particle: &str)`: What `event` (`hurt`, `death`, `ambient`, `step`) sounds and shows instead of its `.sound(..)` and the engine's (smoke when it dies): a sound event and a particle kind, `""` for what it would be anyway, `"none"` for nothing. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1084)
- <a id="EntityBuilder-equipment"></a>`.equipment()`: It has the player's equipment slots (`Registry::equipment_slot`) after its own: what it wears shows on its model and counts like a player's armor; `group(EQUIPMENT_GROUP)` or a slot's name in `w.inventory(id)` reaches them. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1090)
- <a id="EntityBuilder-poses"></a>`.poses()`: It takes the poses the player does, by the keys its walk holds (`Walk`) and the poses' conditions: crouches, swims lying down, crawls where nothing higher fits. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1095)
- <a id="EntityBuilder-picks_up_items"></a>`.picks_up_items()`: It picks up the dropped items it touches into its inventory, as the player does. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1099)
- <a id="EntityBuilder-attack_damage"></a>`.attack_damage(damage: f32)`: Its own melee damage (`World::attack`), the base of its `attribute::ATTACK_DAMAGE`; what it holds adds to it, as vanilla's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1104)
- <a id="EntityBuilder-attack_speed"></a>`.attack_speed(per_second: f32)`: Its melee hits per second (1), like an item's; `attribute::ATTACK_SPEED` multiplies it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1108)
- <a id="EntityBuilder-reach"></a>`.reach(blocks: f64)`: How far it hits (the `ENTITY_REACH` attribute, from its eyes to the target's box); a modifier changes it like any attribute. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1113)
- <a id="EntityBuilder-max_health"></a>`.max_health(h: f32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1116)
- <a id="EntityBuilder-attribute"></a>`.attribute(a: &crate::Attribute, v: f64)`: The kind's own base of an attribute: `.attribute(&attribute::GRAVITY, 0.625)` falls like vanilla's arrow. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1121)
- <a id="EntityBuilder-projectile"></a>`.projectile(p: Projectile)`: It flies as a projectile (`Projectile`): the engine moves it, a ray along its speed each tick decides what it hits (`actions::PROJECTILE_HIT`), however fast. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1129)
- <a id="EntityBuilder-hitboxes"></a>`.hitboxes(h: Hitboxes)`: What hits land on, part by part (`Hitboxes::humanoid()` ...); without, its whole box. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1133)
- <a id="EntityBuilder-walk_speed"></a>`.walk_speed(s: f32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1136)
- <a id="EntityBuilder-category"></a>`.category(category: &EntityCategory)`: A category registered with `Registry::entity_category` (any mod's), for spawn caps and despawning. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1141)
- <a id="EntityBuilder-hostile"></a>`.hostile()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1144)
- <a id="EntityBuilder-passive"></a>`.passive()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1147)
- <a id="EntityBuilder-texture"></a>`.texture(t: &str)`: Skin texture under `entity/`; without this, the kind's name (`entity/zombie`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1151)
- <a id="EntityBuilder-model"></a>`.model(model: impl Into<Model>)`: A model built in code (`humanoid(..)`, `quadruped(..)`, `spider()`, or parts of your own). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1156)
- <a id="EntityBuilder-sound"></a>`.sound(event: &str)`: Sound event `event` (`hurt`, `death`, `ambient`, `step`): one of the files `sounds/entity/<kind>/<event>*.ogg|wav` at random. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1164)
- <a id="EntityBuilder-sound_variants"></a>`.sound_variants(event: &str, files: &[&str])`: Sound event `event` from these files of `sounds/entity/<kind>/`, one at random. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1169)
- <a id="EntityBuilder-sound_group"></a>`.sound_group(group: &str)`: Sounds from `<ns>:entity.<group>` (`.hurt`, `.death`, `.ambient`, `.step` events); a name with a namespace is used as it is. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1187)
- <a id="EntityBuilder-animations"></a>`.animations(name: &str)`: Keyframe animations from `animations/<name>.animation.json`: `idle` and `walk` play by themselves, others with `World::action_play`. A file named like the kind is used even without this. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1195)
- <a id="EntityBuilder-body"></a>`.body(template: &crate::Template)`: A moving body built of `template` (`Registry::template`; block (0, 0, 0) is centered on the entity's position, its box is what the blocks fill). Blocks are placed and broken on it like in the world, and it carries what stands on it; usually `.solid()`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1202)
- <a id="EntityBuilder-shader"></a>`.shader(shader: &crate::Shader)`: Shades the model with `shaders/entity/<name>.wgsl`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1206)
- <a id="EntityBuilder-on_tick"></a>`.on_tick(f: impl Fn(&EntityTick) + Send + Sync + 'static)`: Called once per tick with every entity of this kind. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1210)
- <a id="EntityBuilder-goals"></a>`.goals(goals: crate::ai::Goals)`: Behavior from goals and what sets it off (`ai::Goals`), run every tick; replaces `on_tick`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1214)
- <a id="EntityBuilder-on_hurt"></a>`.on_hurt(f: impl Fn(&World, &crate::Hurt) + Send + Sync + 'static)`: One of these was hurt: `actions::HURT` for this kind, after the damage, with the other events at the end of the tick. Changing the damage is `r.modify(&chain::DAMAGE, ..)`, preventing it `r.before_event(&actions::HURT, ..)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1222)
- <a id="EntityBuilder-on_died"></a>`.on_died(f: impl Fn(&World, &crate::Death) + Send + Sync + 'static)`: One of these died: `actions::DIE` for this kind. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1236)
- <a id="EntityBuilder-on_interact"></a>`.on_interact(f: impl Fn(&mut EntityEvent) -> bool + Send + Sync + 'static)`: A player right clicked one, and `actions::INTERACT_ENTITY` let it; true if it did something, which swings the player's arm. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1248)
- <a id="EntityBuilder-seat"></a>`.seat(seat: crate::riding::Seat)`: A place to ride it, in the order seats are numbered (`crate::riding::Seat`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1252)
- <a id="EntityBuilder-steering"></a>`.steering(s: crate::riding::Steering)`: How the keys of a player in a seat that steers move it (`steer::*`, or your own). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1257)
- <a id="EntityBuilder-rider_rules"></a>`.rider_rules(rules: crate::riding::RiderRules)`: What its riders may do (`RiderRules`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1262)
- <a id="EntityBuilder-riders_fall"></a>`.riders_fall()`: Its falls hurt its riders too, as a horse's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1267)
- <a id="EntityBuilder-ride_camera"></a>`.ride_camera(blocks: f32)`: Blocks the third-person camera stays further back while riding it, 0 for none; without this, by its size. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1272)
- <a id="EntityBuilder-rideable"></a>`.rideable()`: A right click gets on it, in its first free seat (`on_interact` doing `w.mount`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1276)

## EntityEffectBuilder

An entity effect being registered: drawn by `shaders/entity_effect/<its name>.wgsl` unless `shader` says another.

- <a id="EntityEffectBuilder-shader"></a>`.shader(shader: &crate::Shader)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L423)
- <a id="EntityEffectBuilder-unseen"></a>`.unseen()`: Nothing drawn: an effect only of hazards and modifiers. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L428)
- <a id="EntityEffectBuilder-hazard"></a>`.hazard(h: crate::Hazard)`: What it does to its entity while on (`Hazard::own(1.0, &MAGIC).every(25)`: poison), its damage times the level (negative heals); once when given if it is `instant`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L433)
- <a id="EntityEffectBuilder-hazard_at"></a>`.hazard_at(level: u32, mut h: crate::Hazard)`: `hazard` for `level` only, as it is: at that level the others don't count. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L437)
- <a id="EntityEffectBuilder-modifier"></a>`.modifier(m: crate::Modifier)`: A modifier its entity has while it is on (`Modifier::percent(&attribute::MOVEMENT_SPEED, 0.2)`), run once for every level (`set` gives the same at any level). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L443)
- <a id="EntityEffectBuilder-on_tick"></a>`.on_tick(f: impl Fn(&crate::EffectTick) + Send + Sync + 'static)`: Every tick, once with every entity that has it and its level there: levitation of your own, a mana drain, saturation. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L448)
- <a id="EntityEffectBuilder-modifier_at"></a>`.modifier_at(level: u32, mut m: crate::Modifier)`: `modifier` for `level` only, as it is: at that level the others of its attribute don't count (mining fatigue's steep steps). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L453)
- <a id="EntityEffectBuilder-icon"></a>`.icon(texture: &str)`: A status effect: its player sees it, by this icon (`textures/<icon>.png`, 18 pixels square like vanilla's), on the HUD and beside the inventory, named `effect.<namespace>.<name>` in lang. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L460)
- <a id="EntityEffectBuilder-color"></a>`.color(rgb: u32)`: Swirls of this color (`0xRRGGBB`) around who has it, as vanilla's potion effects. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L465)
- <a id="EntityEffectBuilder-beneficial"></a>`.beneficial()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L468)
- <a id="EntityEffectBuilder-harmful"></a>`.harmful()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L471)
- <a id="EntityEffectBuilder-instant"></a>`.instant()`: It acts once when given, its hazards times the level (healing), and doesn't stay. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L475)
- <a id="EntityEffectBuilder-screen"></a>`.screen(shader: &crate::Shader)`: What its player sees goes through this screen shader while it has it (`shaders/post/<name>.wgsl`, `PostPixel`): night vision, blindness. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L480)
- <a id="EntityEffectBuilder-hides"></a>`.hides()`: Its entity isn't drawn for anyone else while it has it (invisibility). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L491)

## EntityKind

- <a id="EntityKind-loot"></a>`.loot() -> &'static LootTable`: What it drops, for `Registry::edit`: `r.edit(base::ZOMBIE.loot()).pool(..)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L205)

## EntityModify

What `World::entity_modify` changes, sent when dropped.

- <a id="EntityModify-pos"></a>`.pos(pos: impl Into<Vec3>)`: A player is teleported there. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L548)
- <a id="EntityModify-velocity"></a>`.velocity(v: impl Into<Vec3>)`: Blocks per second. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L553)
- <a id="EntityModify-yaw"></a>`.yaw(yaw: f32)`: Radians, like `EntityState::yaw`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L558)
- <a id="EntityModify-pitch"></a>`.pitch(pitch: f32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L561)
- <a id="EntityModify-health"></a>`.health(health: f32)`: Up to its max health; 0 or less kills it (`damage_cause::VOID`). Only a respawn brings back the dead. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L566)
- <a id="EntityModify-fall_distance"></a>`.fall_distance(blocks: f32)`: What the next landing hurts by: 0 after a mace's smash, a parachute opening. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L570)
- <a id="EntityModify-flying"></a>`.flying(on: bool)`: Players: flying or not, whatever the game mode allows (landing still stops it). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L574)
- <a id="EntityModify-fire"></a>`.fire(ticks: u32)`: Ticks it burns for, 0 puts it out. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L578)
- <a id="EntityModify-selected"></a>`.selected(slot: u32)`: Players: the hotbar slot in hand. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L582)
- <a id="EntityModify-persistent"></a>`.persistent(on: bool)`: Never despawns however far from players (a boss, a pet, a prisoner), or does again as its category says. Naming it or it picking something up keeps it too. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L587)

## Exit

Where a rider gets off.

- <a id="Exit-nearest"></a>`.nearest(reach: f64) -> Exit` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L68)
- <a id="Exit-offset"></a>`.offset(at: impl Into<Vec3>) -> Exit` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L71)
- <a id="Exit-stay"></a>`.stay() -> Exit` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L74)
- <a id="Exit-call"></a>`.call(f: impl Fn(&World, u64, u64, Vec3) -> Vec3 + Send + Sync + 'static) -> Exit` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L77)

## GameModeBuilder

A game mode being registered: survival's rules unless told otherwise.

- <a id="GameModeBuilder-breaks"></a>`.breaks(how: Breaking)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L732)
- <a id="GameModeBuilder-flight"></a>`.flight(how: Flight)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L740)
- <a id="GameModeBuilder-no_placing"></a>`.no_placing()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L748)
- <a id="GameModeBuilder-no_costs"></a>`.no_costs()`: Nothing gets used up: placed blocks, poured buckets, wear. Picking a block gives a stack. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L752)
- <a id="GameModeBuilder-invulnerable"></a>`.invulnerable()`: No damage, hunger or air (`attribute::INVULNERABLE` 1). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L756)
- <a id="GameModeBuilder-noclip"></a>`.noclip()`: Through blocks. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L760)
- <a id="GameModeBuilder-hidden"></a>`.hidden()`: Unnoticed by mobs, spawning and other players, and not a body to hit. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L764)
- <a id="GameModeBuilder-no_interacting"></a>`.no_interacting()`: It can't use, hit, pick up, drop or hold anything. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L768)
- <a id="GameModeBuilder-no_hud"></a>`.no_hud()`: The hotbar and bars don't show. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L772)
- <a id="GameModeBuilder-creative_inventory"></a>`.creative_inventory()`: The inventory key opens the creative inventory. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L776)
- <a id="GameModeBuilder-modifier"></a>`.modifier(m: crate::Modifier)`: An attribute modifier its players have while in it (longer reach, `visibility` 0). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L780)

## Goals

- <a id="Goals-new"></a>`.new() -> Goals` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L279)
- <a id="Goals-then"></a>`.then(g: impl Goal) -> Goals`: Adds a goal below the ones added before. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L283)
- <a id="Goals-react"></a>`.react(trigger: Trigger, reaction: impl Fn(&World, u64, u64) + Send + Sync + 'static) -> Goals`: When `trigger` goes off, `reaction`: `react::anger(30.0, 1)`, or anything of your own (world, the mob, who set it off). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L289)
- <a id="Goals-aggressive"></a>`.aggressive(range: f64) -> Goals`: Goes for a player within `range` and for who hurt it: a zombie. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L294)
- <a id="Goals-neutral"></a>`.neutral() -> Goals`: Goes for who hurt it, for 20 seconds; leaves everyone else alone. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L299)
- <a id="Goals-timid"></a>`.timid(secs: f64) -> Goals`: Runs about for `secs` when hurt, by anything: an animal. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L303)
- <a id="Goals-tick"></a>`.tick(t: &EntityTick)`: Runs the triggers, then the goals, for every entity of the tick. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L307)

## Hitboxes

What hits land on, part by part (`EntityBuilder::hitboxes`): boxes in pixels (1/16 block, its feet at 0, -Z its front, -X its right) at the kind's size; one made bigger is hit bigger. Rays, arrows and blows tell the part they hit (`RayHit::part`, `Damage::part`, the `part` a chain step reads in its query). They go with the body, a head with the look, arms and legs swing as it walks: run and an arrow may pass between them. Not the model's boxes: how a mob looks never changes where it can be hit.

- <a id="Hitboxes-new"></a>`.new() -> Hitboxes` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2328)
- <a id="Hitboxes-part"></a>`.part(name: &str, min: [f32`: A part going with the body. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2338)
- <a id="Hitboxes-head"></a>`.head(name: &str, min: [f32`: A head: it turns with the look about `pivot`, its neck. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2342)
- <a id="Hitboxes-limb"></a>`.limb(name: &str, min: [f32`: A leg or an arm: it swings about `pivot` as it walks, `swing` radians at a full stride, `phase` radians after the beat (`PI` the other half of the step). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2347)
- <a id="Hitboxes-humanoid"></a>`.humanoid() -> Hitboxes`: Two legs, two arms, a body and a head, 32 pixels tall: the player's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2351)
- <a id="Hitboxes-quadruped"></a>`.quadruped() -> Hitboxes`: A body on four legs with a head ahead, 25 pixels tall: a cow's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2362)
- <a id="Hitboxes-reaching"></a>`.reaching(mut self)`: Its arms held out ahead, not swinging, as a zombie's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2373)
- <a id="Hitboxes-slim"></a>`.slim(px: f32)`: Arms and legs `px` pixels thick, as a skeleton's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2388)

## Looks

What `ItemBuilder::condition` changes of a stack's look; what it doesn't set stays.

- <a id="Looks-worn_model"></a>`.worn_model(model: &str, bone: &str) -> &mut Self`: A model attached to a bone while this stack is in equipment. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2903)
- <a id="Looks-worn_texture"></a>`.worn_texture(texture: &str) -> &mut Self`: A texture on the wearer's armor geometry, under `entity/`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2908)
- <a id="Looks-texture"></a>`.texture(t: &str) -> &mut Self`: A texture from `item/` instead of its own. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2913)
- <a id="Looks-model"></a>`.model(m: &str) -> &mut Self`: A model file instead of its own (`set_model`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L2918)

## Model

A model built in code: its parts and the size of the skin its UVs are laid out on (the picture itself may be bigger, a pack's high resolution skin keeps lining up).

- <a id="Model-uv_size"></a>`.uv_size(width: u32, height: u32)`: The UV size, if the layout isn't on a 64 x 32 skin. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L890)

## ModelPart

A model part (a bone): its name, pivot, boxes, free-form triangles, and the part it hangs from. The name says how it moves by itself, like in Blockbench's Bedrock models: `head` looks around, `rightArm`, `leftArm`, `rightLeg`, `leftLeg` swing while walking (arms resting held out, like a zombie's, only sway), `body` leans when sneaking, `leg0`..`leg3` are a quadruped's and `leg0`..`leg7` a spider's sweeping legs; other names keep still. Keyframe animations address bones by the same names.

- <a id="ModelPart-new"></a>`.new(name: &str, pivot: [f32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L818)
- <a id="ModelPart-rotation"></a>`.rotation(x: f32, y: f32, z: f32) -> ModelPart`: Resting rotation in degrees about x, y, z (applied in that order). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L822)
- <a id="ModelPart-cuboid"></a>`.cuboid(origin: [f32`: Box relative to the pivot in model pixels, `uv` = top-left texel of its unwrap. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L827)
- <a id="ModelPart-inflated"></a>`.inflated(by: f32) -> ModelPart`: The last box grown by this much on every side, its texture staying the same (wool, a skin's outer layer). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L834)
- <a id="ModelPart-mirrored"></a>`.mirrored(mut self) -> ModelPart`: The last box's unwrap mirrored left to right. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L841)
- <a id="ModelPart-child_of"></a>`.child_of(index: usize) -> ModelPart`: Hangs from the part at `index` in the same model: moves and turns with it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L848)
- <a id="ModelPart-mesh"></a>`.mesh(triangles: Vec<abi::MeshVertex>) -> ModelPart`: Free-form triangles (three corners each), in model pixels like the pivot. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L853)

## PoseBuilder

A pose being registered: standing's box and speed unless told otherwise.

- <a id="PoseBuilder-modifier"></a>`.modifier(m: crate::Modifier)`: An attribute modifier its holder gets while in it (`Modifier`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L634)
- <a id="PoseBuilder-height"></a>`.height(share: f32)`: Box height as a share of standing (crouching is 1.5 / 1.8). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L638)
- <a id="PoseBuilder-eye"></a>`.eye(share: f32)`: Eye height as a share of standing. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L642)
- <a id="PoseBuilder-speed"></a>`.speed(share: f32)`: Walking speed as a share of normal, 0 to stay put. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L646)
- <a id="PoseBuilder-tilt"></a>`.tilt(tilt: u32)`: How the model lies (`abi::pose_tilt`): along the look like swimming, face down, on its back. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L650)
- <a id="PoseBuilder-action"></a>`.action(name: &str)`: The model's clip for it, by action name. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L654)
- <a id="PoseBuilder-no_bob"></a>`.no_bob()`: The camera doesn't bob walking in it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L658)
- <a id="PoseBuilder-priority"></a>`.priority(p: i32)`: Of the poses a player may take, the highest is taken (the engine's: stand 0, crouch 10, crawl 20, swim 30). Without `key` or `when` a pose is only ever held by a mod. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L663)
- <a id="PoseBuilder-key"></a>`.key(key: &crate::Key)`: Taken while the player holds this action. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L667)
- <a id="PoseBuilder-when"></a>`.when(c: impl Into<crate::Cond>)`: And only where this holds (`Cond::in_liquid`, `!Cond::flying()`); again for more. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L672)
- <a id="PoseBuilder-fallback"></a>`.fallback(poses: &[&Pose])`: Where it has no room, the first of these that has and whose `when` holds; the last even without room. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L677)

## Projectile

How a projectile flies (`EntityBuilder::projectile`): vanilla's arrow unless said otherwise, 2 damage per block a tick of speed, 0.99 of its speed kept a tick in the air and 0.6 in a liquid, gone where it hits a block.

- <a id="Projectile-new"></a>`.new() -> Projectile` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2299)
- <a id="Projectile-damage"></a>`.damage(per_speed: f32)`: Damage per block a tick of its speed, rounded up (`chain::PROJECTILE_DAMAGE`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2303)
- <a id="Projectile-drag"></a>`.drag(air: f32, liquid: f32)`: Shares of its speed kept each tick in the air and in a liquid. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2308)
- <a id="Projectile-sticks"></a>`.sticks(ticks: u32)`: It stays stuck where it hits a block, for `ticks` (vanilla's arrow a minute). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2313)

## RiderRules

What riders of a kind may do, each a test of the world, the mount and the rider, so it can hang on anything (only on the ground, only by day, only its owner). A rule not given lets it.

- <a id="RiderRules-new"></a>`.new() -> RiderRules` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L182)
- <a id="RiderRules-hits"></a>`.hits(f: impl Fn(&World, u64, u64) -> bool + Send + Sync + 'static)`: Whether a rider may hit (`|w, mount, rider| ..`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L186)
- <a id="RiderRules-builds"></a>`.builds(f: impl Fn(&World, u64, u64) -> bool + Send + Sync + 'static)`: Whether a rider may place and break blocks. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L191)
- <a id="RiderRules-hurt_reaches_riders"></a>`.hurt_reaches_riders(share: f32)`: Share of what hurts the mount that hurts each rider too (a blast under a boat), 0 for none. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L197)

## Seat

A place on a mount: where a rider's feet go, from the mount's position in its own frame (+Z ahead, +X right, turned with it), whether who sits there steers, where they get off.

- <a id="Seat-at"></a>`.at(pos: impl Into<Vec3>) -> Seat` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L29)
- <a id="Seat-steers"></a>`.steers(mut self) -> Seat`: Who sits here steers: a player with its keys (`steering`), a mob with its AI. With nobody steering a mount moves by its own AI. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L35)
- <a id="Seat-exit"></a>`.exit(exit: Exit) -> Seat`: Where a rider of this seat gets off (`Exit::nearest(2.0)` unless told otherwise). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L40)

## Spawn

How a spawned entity starts (`World::spawn_with`); what isn't set is the kind's default.

- <a id="Spawn-new"></a>`.new() -> Spawn` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L30)
- <a id="Spawn-owner"></a>`.owner(id: u64)`: Who it belongs to: a projectile's shooter, which it doesn't hit as it leaves them and whose hits its hits are. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L47)
- <a id="Spawn-cause"></a>`.cause(cause: u32)`: Why it comes (`spawn_cause`, `MOD` unless said), what `actions::SPAWN` is told. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L52)
- <a id="Spawn-stack"></a>`.stack(s: ItemStack)`: What an item entity is. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L57)
- <a id="Spawn-velocity"></a>`.velocity(v: impl Into<Vec3>)`: Blocks per second. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L62)
- <a id="Spawn-inherit_velocity"></a>`.inherit_velocity(mut self)`: Adds its owner's current velocity once at spawn, including a mount or moving platform. Combine with `owner(id)` and `velocity(v)` to launch relative to the shooter. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L69)
- <a id="Spawn-yaw"></a>`.yaw(radians: f32)`: Radians, like `EntityState::yaw`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L74)
- <a id="Spawn-pickup_delay"></a>`.pickup_delay(ticks: u32)`: Ticks before an item can be picked up (10 by default, 40 for one a player threw). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L80)

## SpawnBuilder

A natural spawning rule (see `abi::comp::spawn`), named `<mod>:<entity>`, `/2`.. unless `named`: `r.natural_spawn(&PIG).biome("#animals").ground("#animal_spawn").light(9, 15).pack_size(2, 3).chance(0.05)`.

- <a id="SpawnBuilder-named"></a>`.named(name: &str)`: Its name instead of `<mod>:<entity>`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2240)
- <a id="SpawnBuilder-ground"></a>`.ground(what: &str)`: Only on blocks `what` takes (a block matcher; under the spot, what `place` says it rests on): `when(Cond::block(what))`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2247)
- <a id="SpawnBuilder-place"></a>`.place(place: SpawnPlace)`: What kind of spot it takes: on the ground (the default), in a liquid, under a ceiling. The mob's height decides how much room it needs. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2252)
- <a id="SpawnBuilder-when"></a>`.when(c: impl Into<crate::Cond>)`: Only where this holds too (asked about the spot and the block it rests on). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2260)
- <a id="SpawnBuilder-light"></a>`.light(min: u32, max: u32)`: Light at the spot, inclusive (0..15): `when(Cond::light(min, max))`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2264)
- <a id="SpawnBuilder-pack_size"></a>`.pack_size(min: u32, max: u32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2267)
- <a id="SpawnBuilder-weight"></a>`.weight(w: u32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2270)
- <a id="SpawnBuilder-chance"></a>`.chance(c: f32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2273)

## SpawnPlace

Where a natural spawn stands (`SpawnBuilder::place`).

- <a id="SpawnPlace-ground"></a>`.ground() -> SpawnPlace`: On a solid block, in air. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2205)
- <a id="SpawnPlace-in_liquid"></a>`.in_liquid(tag: &'static str) -> SpawnPlace`: Inside a liquid with this tag (`"#water"`): fish, squid. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2209)
- <a id="SpawnPlace-ceiling"></a>`.ceiling() -> SpawnPlace`: Hanging under a solid block: bats. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2213)

## Steering

How a player's keys move a mount: called on that player's client each tick with its keys and the mount as it is, giving what the mount does. `steer::*` are the usual ones; `and` puts two together (a walk with a jump).

- <a id="Steering-new"></a>`.new(f: impl Fn(&Keys, &EntityState) -> Steer + Send + Sync + 'static) -> Steering`: Yours from scratch. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L89)
- <a id="Steering-and"></a>`.and(other: Steering) -> Steering`: Both: the first's movement and facing unless it gives none, the higher jump, flying if either flies. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/riding.rs#L94)

## Trigger

When a mob reacts: what sets it off, at whom, and while what holds.

- <a id="Trigger-sight"></a>`.sight(range: f64) -> Trigger`: Someone within `range` blocks (8 up and down), as near as their `visibility` makes them; it goes on as long as they are. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L103)
- <a id="Trigger-gaze"></a>`.gaze(range: f64) -> Trigger`: Someone within `range` looking at it, with nothing in between (an enderman). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L108)
- <a id="Trigger-hurt"></a>`.hurt() -> Trigger`: Someone hurting it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L113)
- <a id="Trigger-when"></a>`.when(range: f64, test: impl Fn(&World, &EntityState, &EntityState) -> bool + Send + Sync + 'static) -> Trigger`: Someone within `range` for whom `test(world, the mob, them)` holds. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L118)
- <a id="Trigger-only_if"></a>`.only_if(f: impl Fn(&World, &EntityState) -> bool + Send + Sync + 'static) -> Trigger`: Only while `f(world, the mob)` holds: a spider out of the daylight. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L123)
- <a id="Trigger-only_while"></a>`.only_while(c: crate::Cond) -> Trigger`: Only while `c` holds where the mob is (`!Cond::new("day")`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L129)
- <a id="Trigger-of"></a>`.of(kind: &'static EntityKind) -> Trigger`: Notices mobs of `kind` instead of players (a wolf and sheep). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L134)
- <a id="Trigger-of_tag"></a>`.of_tag(tag: &'static crate::Tag) -> Trigger`: Notices entities of kinds with `tag` instead of players (`#undead`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L139)
- <a id="Trigger-of_any"></a>`.of_any(pick: impl Fn(&World, u64, &EntityState) -> bool + Send + Sync + 'static) -> Trigger`: Notices whatever `pick(world, id, state)` accepts instead of players, them included or not. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L144)
- <a id="Trigger-by_anything"></a>`.by_anything(mut self) -> Trigger`: For `hurt`: whatever hurts it, other mobs and the world too. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ai.rs#L150)

## Walk

One tick of a mob's walking for `World::walk`: a direction and the keys a player would hold.

- <a id="Walk-toward"></a>`.toward(dir: [f32`: Toward `dir` (x, z, length up to 1) at `speed` times its kind's walking speed. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2450)
- <a id="Walk-jump"></a>`.jump()`: Jumps; swims and flies up. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2454)
- <a id="Walk-sneak"></a>`.sneak()`: Never steps off an edge, holds on to ladders, crouches; flies down. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2458)
- <a id="Walk-sprint"></a>`.sprint()`: Faster, a longer jump, swims lying down in a liquid it swims in. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2462)
- <a id="Walk-fly"></a>`.fly()`: Takes off and flies, if its `attribute::CAN_FLY` is 1 or more (`.attribute(..)` on its kind, a modifier); it comes down when it lands without `jump`, as a player does. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2467)
