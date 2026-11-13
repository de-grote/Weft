# Entities

`r.entity(&ENTITY)` and then the calls below. What a mob thinks is in `weft_sdk::ai`, how it looks in the [models guide](models.md), and every method in the [reference](reference/entities.md).

## Basics, looks and sounds

- every entity gets a spawn egg by itself, `<name>_spawn_egg`, called after it and in the creative tab of eggs. Draw `item/<name>_spawn_egg.png` or it looks generic; `no_egg()` for what isn't a creature
- `size(w, h)`, with `eye_height(blocks)` when its eyes aren't at 85% of that, `max_health`, `walk_speed`
- `hostile()` or `passive()` or `category(..)`
- `texture` (by default `entity/<name>`)
- `model(humanoid(..))`, or `r.set_model(&KIND, "thing")` for a Blockbench geo.json (any item's or entity's, also someone else's, `"#tag"` for many items). A kind with neither is drawn by `models/<name>.geo.json` if there is one, which a resource pack can replace (base's mobs are files). `weft export-models <dir>` writes every model made in code, any mod's, as a geo.json to go on with in Blockbench. Files in your `assets/minecraft/` go into the pack made from the player's Minecraft, so a model for vanilla's texture layout goes there (base's sheep)
- `r.edit(&KIND)` changes any of this on someone else's kind, the engine's too (`r.edit(&PLAYER).attach(&MANA)`), and `r.replace(&KIND)` starts it over
- `r.bone_model(..)`, `.wearable_model(..)` on an item: a model on a bone. `r.bone_motion`, `c.bone_turn`, `c.bone_position`: bones moving by themselves or by code (`docs/models.md`)
- `light(..)`, `light_rgb(..)` and `light_at(x, y, z)` for where on it the light comes from (a lantern on its head). One mob's own light changes with `.attach(&LIGHT)` then `w.set_component(mob, &LIGHT, [r, g, b])`; effects give light too (`r.entity_effect(&E).light(..)`), and `shown_slots(&[0])` makes a kind's slots seen by everyone, glowing if what's there glows (a pedestal holding a torch)
- `sound("hurt")`, `sound("death")`, `sound("ambient")`: files from `sounds/entity/<name>/`
- `animations(..)`

## Callbacks

- `on_tick`: every entity of the kind at once, once per tick
- `on_interact`: a player right clicked one (after `actions::INTERACT_ENTITY`); true if it did something, which swings the player's arm

## AI and hands

- `goals(..)` for AI (see `weft_sdk::ai`); `ai::melee()` with `.attack_damage(3.0)` and `.reach(1.3)` makes it hit like a player does, with full swings paced by `.attack_speed(..)` (the reach is the `ENTITY_REACH` attribute, so a modifier lengthens a mob's arms the same as a player's; `w.in_reach(id, target)` is the test, `w.swing_strength(id)` how charged it is), so `question::ATTACK` and `attribute::ATTACK_DAMAGE` (a strength effect, a peaceful zone) work on mobs too
- a mob can do whatever a player does with its hands. `equipment()` gives it the player's armor slots and other hand, and what it holds (its first slot, or another of its own with `w.entity_modify(id).selected(slot)`) shows in its hand and hits, wearing out, like a player's sword. `w.use_item(id, off_hand)` uses it (a witch drinking, a skeleton drawing its bow, eating taking its time), `w.use_item_on(id, off_hand, pos, face)` on a block, and `w.start_mining(id, pos)` breaks a block at the speed its tool mines it, cracks showing. `picks_up_items()` makes it take the dropped stacks it touches. When it dies, what it carries falls with its loot, and a named one's death goes in the chat
- what sets a mob off is on its goals: every mob leaves everyone alone until a trigger goes off (`ai::Trigger::sight(range)`, `hurt()`, `gaze(range)` for an enderman, `when(range, |w, mob, them| ..)`, each with `.only_if(|w, mob| ..)`, `.of(&KIND)`, `.of_tag(&TAG)` or `.of_any(|w, id, st| ..)` for other mobs, `hurt().by_anything()`), and then a reaction does whatever it should: `ai::react::anger(secs, max)` (`ai::melee`, `ai::chase_target` and `ai::target` go after the nearest, `ai::targets` gives all of them, up to `max` at once: a charge through five players), `react::fear(secs, max)` (`ai::panic`, `ai::flee` away from all it fears), `react::alert(radius, ..)` for the pack around, or any `|w, mob, who| ..` of yours (points toward a grudge, a call for help). `Goals::new().aggressive(16.0)` is sight and hurt to anger, `.neutral()` hurt alone, `.timid(3.0)` an animal's panic; a spider is `.react(Trigger::sight(35.0).only_if(not_in_daylight), react::anger(2.0, 1)).neutral()`. Anger and fear last while the mod runs, they aren't saved with the world

## Projectiles

`projectile(Projectile::new().damage(2.0).sticks(1200))`: it flies as a projectile and the engine moves it. Each tick a ray along its speed decides what it hits first (blocks by what stops a body: through grass), so nothing is flown through however fast, and it sticks right at the face it met: on a moving body it rides along, once its block is gone it falls. It points where it flies, the whole model tipping as it falls. In a mob it stays in its box, held by each client on the body as drawn, turning with the body and not the head, until the mob dies; and off something it did no harm to it bounces back, as vanilla's. `actions::PROJECTILE_HIT` asks (no, and it flies on past them: arrows through your own team) and tells (`owner`, `target`, the block, the point). An entity hit takes `chain::PROJECTILE_DAMAGE` (its damage times its speed, rounded up: vanilla's arrow), going away from where it flew. Shoot one with `w.spawn_with(&ARROW, eye, &Spawn::new().owner(me).velocity(v))`: it won't hit its owner until it has flown clear, and its hits are the owner's. With `.attribute(&attribute::GRAVITY, 0.625)` it falls like vanilla's arrow; that also sets any other attribute's base for the kind

For a shot relative to its shooter, add `.inherit_velocity()` to `Spawn` alongside
`.owner(shooter).velocity(launch_speed)`. This adds the shooter's velocity once at spawn,
including its mount or moving platform; `velocity` alone remains an absolute speed.

## Coming, going and getting hurt

An item with `comps::BLOCKS_ATTACKS` protects its user while held in use. Pair
`.use_until_released()` with a `Blocking` value: `delay_ticks`, the frontal `arc` in
degrees, and `wear_threshold`, `wear_base` and `wear_factor` for durability loss.
Blocked attacks cause no health loss or knockback. The
`damage.bypasses_blocking` tag on a damage cause skips this protection independently
of armor; `DamageCauseBuilder::bypasses_blocking()` sets it. A weapon's
`comps::DISABLE_BLOCKING` value disables blocking for that many seconds. The base
shield activates immediately, has a 180-degree arc, and axes disable it for five seconds.

`comps::CATCH_PROJECTILES` takes a `ProjectileCatch`: a model bone, bounds in its
pixels and an incoming direction. Sticking projectiles blocked by that item become
`comps::EMBEDDED_PROJECTILES` stack data. They stay with the item through hand changes,
drops and saves, and follow its model's bone. Other clients receive held stack data
through the normal entity fields. Up to 32 projectile models are retained per stack.

- comings and goings: `actions::SPAWN` asks before anyone sees a new one (`cause`: `spawn_cause::NATURAL`, `EGG`, `COMMAND`, `MOD`, set with `Spawn::new().cause(..)`, and `DROP` for a dropped stack) and tells after: a zombie in random armor is that event. `ENTITY_REMOVED` tells when one leaves the world and why (`removal::DIED`, `DESPAWNED`, `UNLOADED`, `PICKED_UP`, `HIT` ...): a boss bar gone with its boss. Moving blocks and block entities are blocks, so their comings and goings are the blocks' events, not these. What despawns far from players is its category's; `w.entity_modify(id).persistent(true)` keeps one, and so does naming it or it picking something up, as vanilla's
- `on_hurt(|w, hurt| ..)`, `on_died(|w, death| ..)`: `actions::HURT` and `actions::DIE` for this kind. The damage itself is the `chain::DAMAGE` chain and `actions::HURT` can stop it, for any kind, yours or not. Every hit has a cause (`damage_causes::FALL`, `LAVA`, `PROJECTILE` ...); your own is `r.damage_cause(&RADIATION).bypasses_armor().death_message("death.mymod.radiation")`, and a kind tagged `immune/fall` or `immune/mymod:radiation` takes none of it. From code a hurt is described: `w.damage(id, Damage::new(4.0, &RADIATION).by(who).through(arrow).knock(dir))`, every step seeing who, through what (`direct`), with what and whether it crits. Healing is the same call with a negative amount (`w.heal` is that), going through `chain::HEAL` and the `ENTITY_HEALED` event instead

## Riding

riding: `.seat(Seat::at([0.0, 1.0, 0.0]).steers())` is a place to sit, as many as you like, each with its own point in the mount's frame (+Z ahead) and `.exit(Exit::offset(..))`, `Exit::nearest(2.0)` (the default), `Exit::stay()` or `Exit::call(|w, mount, rider, nearest| ..)` for where its rider gets off. Riders ride riders, as deep as you like, never in a ring. Who sits in a seat that `steers` steers it: a mob with its own AI (a zombie jockey), a player with its keys through `.steering(..)`, which runs on that player's client and gives what the mount does; `steer::walk_turn`, `walk_look`, `strafe`, `fly`, `jump` and `charged_jump` are ready, `.and(..)` puts two together, `Steering::new(|keys, mount| ..)` is your own: `keys.axis(&keys::FORWARD, &keys::BACK)`, `keys.held(..)` and `keys.state(..)` read any action, its `held_ticks` and `released` being a charge (how long it is held, and was before being let go this tick), and `keys.yaw` is where the rider looks. Nobody steering, its own AI moves it. `.rider_rules(RiderRules::new().hits(|w, mount, rider| ..).builds(..).hurt_reaches_riders(0.5))` says what riders may do, by any condition; `.riders_fall()` passes its falls on; `.ride_camera(blocks)` how far back the third-person camera goes; `.rideable()` gets on with a right click. From code `w.mount(rider, mount)`, `w.mount_seat(..)`, `w.dismount(rider)`, `w.vehicle(id)`, `w.riders(mount)`, and `actions::MOUNT` / `DISMOUNT` to refuse or follow them. Players get off with `core:dismount`, Shift unless rebound. A teleport, to another dimension too, takes a mount's riders along and a rider off its mount. Riders sit by their model's `ride` clip (legs forward on any two-legged one) instead of walking. Testmod's steed shows it all

## Moving platforms

Moving platforms: `solid()`, `kinematic()`, `carry_height(..)`, and
`body(&DECK)` with a template (`r.template(&DECK).fill("base:oak_planks", from, to)`) makes a flying structure players can build on; its box is what the blocks fill, no `size` needed. `EntityState.size` is an entity's box and eye height as they are now (crouching and `w.set_size` included), so `st.pos.y + st.size.eye` is where its eyes are (or just `st.eye()`).
