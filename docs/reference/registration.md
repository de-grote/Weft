# Registration reference

The registry, components, tags, settings, slots and menus.

## Block

- <a id="Block-ore"></a>`.ore() -> &'static Feature`: Its ore (a feature named like it), for `Registry::edit`: `r.edit(base::IRON_ORE.ore()).exposed(0.5)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L170)
- <a id="Block-liquid"></a>`.liquid() -> &'static Liquid`: Its liquid, for `Registry::edit`: `r.edit(base::WATER.liquid()).fog(0x204020, -8.0, 12.0)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L174)
- <a id="Block-loot"></a>`.loot() -> &'static LootTable`: What it drops, for `Registry::edit`: `r.edit(base::GRAVEL.loot()).pool(..)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L178)

## BlockList

Blocks with weights a component names: `set_text(&TREE_KINDS, "birch_log:1 oak_log:4")` (weight 1 when left out), resolved when the game loads.

- <a id="BlockList-entries"></a>`.entries() -> &'static [[u32`: (state, weight) pairs. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L181)
- <a id="BlockList-pick"></a>`.pick(roll: u64) -> Option<u32>`: One state by its weight, `roll` being any random number. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L185)

## Component

Typed handle to a component, found by name the first time it is used. Component and tag names are a vocabulary shared by all mods, not things a mod owns: `fuel` means the same to everyone and is used as written, a `modid:` prefix only keeps a term to one mod. `K` is the kind whose registrations take it (`crate::kind`), any kind by default.

- <a id="Component-named"></a>`.named(name: &'static str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L450)
- <a id="Component-name"></a>`.name() -> &'static str` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L453)
- <a id="Component-declaration"></a>`.declaration() -> &'static crate::Declaration`: Its declaration, for `Registry::edit`: `r.edit(HUNGER.declaration())`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L457)
- <a id="Component-id"></a>`.id() -> u32`: `u32::MAX` if nothing uses it (or before the registries are frozen). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L462)
- <a id="Component-on"></a>`.on(kind: u32, id: u32) -> Option<T>`: Its value on entry `id` of `kind` (`abi::entry_kind`): a weather's, a game mode's, any kind's but word sets and component declarations. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L468)
- <a id="Component-on_block"></a>`.on_block(state: u32) -> Option<T>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L472)
- <a id="Component-on_item"></a>`.on_item(item: u32) -> Option<T>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L476)
- <a id="Component-on_record"></a>`.on_record(record: u32) -> Option<T>`: Its value on a record (`Registry::record`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L481)
- <a id="Component-on_entity"></a>`.on_entity(kind: u32) -> Option<T>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L485)
- <a id="Component-on_biome"></a>`.on_biome(biome: u32) -> Option<T>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L489)
- <a id="Component-on_dimension"></a>`.on_dimension(dim: u32) -> Option<T>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L493)
- <a id="Component-on_stack"></a>`.on_stack(stack: &ItemStack) -> Option<T>`: The stack's own value (damage taken, charge ...), else its item's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L497)
- <a id="Component-on_stack_item"></a>`.on_stack_item(stack: &ItemStack) -> Option<T>`: What a stack's item has, nothing for an empty stack: fuel in a fuel slot. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L502)
- <a id="Component-on_stack_own"></a>`.on_stack_own(stack: &ItemStack) -> Option<T>`: Only what the stack itself carries. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L506)
- <a id="Component-on_id"></a>`.on_id(id: u32) -> Option<T>`: Its value on entry `id` of its kind: `FALLS.on_id(w.weather())`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L582)

## Config

- <a id="Config-named"></a>`.named(name: &'static str)`: This mod's setting (`"sparks"`) or another's (`"othermod:sparks"`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L54)
- <a id="Config-id"></a>`.id() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L58)
- <a id="Config-get"></a>`.get() -> T` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L62)
- <a id="Config-set"></a>`.set(v: T)`: Changes and saves it (clamped to its range); whoever reads it sees the change. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L67)

## ConfigBuilder

- <a id="ConfigBuilder-bool"></a>`.bool(default: bool)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L131)
- <a id="ConfigBuilder-int"></a>`.int(default: i64, min: i64, max: i64)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L135)
- <a id="ConfigBuilder-float"></a>`.float(default: f64, min: f64, max: f64)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L141)
- <a id="ConfigBuilder-step"></a>`.step(step: f64)`: Values the slider snaps to, and how far the arrow keys move it (`int` has 1). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L147)
- <a id="ConfigBuilder-choice"></a>`.choice(default: u32, choices: &[&str])`: One of `choices`, read as its index. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L151)
- <a id="ConfigBuilder-text"></a>`.text(default: &str)`: Text, read with a `TextConfig`; the player's own. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L157)
- <a id="ConfigBuilder-server"></a>`.server()`: The server's value counts for everyone on it, and its changes reach them. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L161)
- <a id="ConfigBuilder-world"></a>`.world()`: Saved with the world and changed there with `/rule` (vanilla's game rules); its players get it like the server's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L166)
- <a id="ConfigBuilder-restart_required"></a>`.restart_required()`: Only read at start: the screen says a restart is needed. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L170)
- <a id="ConfigBuilder-on_release"></a>`.on_release()`: Its slider takes the value when let go, not on its way there (a value that changes what the slider is drawn by, as the GUI scale). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L175)

## Entry

One choice of a pool.

- <a id="Entry-item"></a>`.item(name: &str) -> Entry`: An item name (this mod's namespace by default) or `#tag`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L830)
- <a id="Entry-nothing"></a>`.nothing() -> Entry`: A choice that gives nothing (weight against the others). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L834)
- <a id="Entry-count"></a>`.count(min: u32, max: u32) -> Entry` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L837)
- <a id="Entry-weight"></a>`.weight(w: u32) -> Entry` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L841)
- <a id="Entry-when"></a>`.when(c: impl Into<Cond>) -> Entry` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L845)
- <a id="Entry-smelted"></a>`.smelted(recipe_kind: &crate::RecipeKind) -> Entry`: A burning victim drops it cooked by this recipe type (`base:smelting`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L850)

## Handle

A thing a mod owns (block, item, entity kind, menu, key, screen...), named in its handle:

- <a id="Handle-named"></a>`.named(name: &'static str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L134)
- <a id="Handle-generated"></a>`.generated(name: String) -> &'static Self`: A handle for a name made at runtime, for families of content built in a loop (`r.item(Item::generated(format!("{metal}_ingot")))`). Lives as long as the game, like a `static`; meant for registration, not for every tick. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L141)
- <a id="Handle-name"></a>`.name() -> &str`: Full name, `ns:name`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L153)
- <a id="Handle-path"></a>`.path() -> &str`: The name without its namespace. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L162)
- <a id="Handle-id"></a>`.id() -> u32`: For blocks, the first state. `u32::MAX` if unknown. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L220)
- <a id="Handle-lookup"></a>`.lookup(name: &str) -> Option<u32>`: The id of what a player typed (`/give` style input), `None` if there is no such thing. Code names things with handles; this is for text that only exists at runtime. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L252)
- <a id="Handle-with"></a>`.with(props: &[(&str, &str)]) -> u32`: The block's state with properties `props` set, the others at their defaults: `FURNACE.with(&[("lit", "true")])`. `u32::MAX` if the block or a value is unknown. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L259)
- <a id="Handle-is"></a>`.is(state: u32) -> bool`: True if `state` belongs to this block. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L267)

## Id

A thing by its handle, or by the id that handle gives: what takes this takes either.

- <a id="Id-raw"></a>`.raw() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L304)

## Inventory

- <a id="Inventory-len"></a>`.len() -> u32`: Slots, 0 when the holder has no inventory. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L610)
- <a id="Inventory-is_empty"></a>`.is_empty() -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L613)
- <a id="Inventory-get"></a>`.get(slot: u32) -> ItemStack` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L616)
- <a id="Inventory-set"></a>`.set(slot: u32, stack: ItemStack)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L619)
- <a id="Inventory-insert"></a>`.insert(stack: ItemStack) -> ItemStack`: Returns what did not fit. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L623)
- <a id="Inventory-group"></a>`.group(name: &str) -> SlotGroup<'static>`: A slot group of this inventory (a block's: the `holder` groups of the menu it opens); a name it doesn't have is logged as a mistake and gives no slots. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L628)
- <a id="Inventory-find_group"></a>`.find_group(name: &str) -> Option<SlotGroup<'static>>`: The group if this inventory has it: a player's equipment, which mobs don't have. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L636)

## Item

- <a id="Item-stack"></a>`.stack(count: u32) -> abi::ItemStack`: `count` of this item, plain; `.with(&COMP, v)` and `.with_wear(n)` give it data. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L212)

## Layers

How an item or a stack looks (`abi::comp::LAYERS`): pictures one over the other, the first at the bottom, each tinted or not. The same for an item (`ItemBuilder::layer`) and a stack (`with_layers`).

- <a id="Layers-new"></a>`.new() -> Layers` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L723)
- <a id="Layers-layer"></a>`.layer(texture: &str) -> Layers`: One more picture on top (under `item/`, `"other:x"` another mod's). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L727)
- <a id="Layers-tint"></a>`.tint(color: u32) -> Layers`: Tints the layer just added `0xRRGGBB` (multiplied: white parts take the color). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L732)
- <a id="Layers-tint_kind"></a>`.tint_kind(kind: &crate::TintKind) -> Layers`: Tints the layer just added by a tint kind's default color (`tints::GRASS`): an item looking like the block it places away from any biome. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L740)
- <a id="Layers-is_empty"></a>`.is_empty() -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L746)

## Matcher

Which blocks, as a component names them: a block matcher (`set_text(&SUPPORT, "#plant_soil !base:sand")`, syntax in `docs/modding.md`), compiled once when the game loads and read back as its id.

- <a id="Matcher-has"></a>`.has(state: u32) -> bool`: Whether it takes block state `state`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L137)

## MenuBuilder

- <a id="MenuBuilder-group"></a>`.group(name: &str, slots: Slots)`: A group of slots, by default the menu's own (`Slots::menu`). Groups come in this order. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1539)
- <a id="MenuBuilder-player_inventory"></a>`.player_inventory()`: The player's inventory and hotbar as two groups (`INVENTORY_GROUP`, `HOTBAR_GROUP`), shown with `ui.player_inventory(..)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1548)
- <a id="MenuBuilder-equipment"></a>`.equipment()`: The equipment slots, here among the groups, each named like its slot: of the entity the menu is open on when it wears (`EntityBuilder::equipment`, a mob's), else the player's. Only the slots: `ui.equipment()` draws them where the screen wants. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1556)
- <a id="MenuBuilder-recipes"></a>`.recipes(kind: &RecipeKind)`: Recipes of this kind are made here: clicking an `arrow` bar of the screen shows them. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1561)
- <a id="MenuBuilder-on_change"></a>`.on_change(f: impl Fn(&MenuCtx) + Send + Sync + 'static)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1564)
- <a id="MenuBuilder-on_take"></a>`.on_take(f: impl Fn(&MenuCtx) + Send + Sync + 'static)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1567)
- <a id="MenuBuilder-on_tick"></a>`.on_tick(f: impl Fn(&MenuCtx) + Send + Sync + 'static)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1570)
- <a id="MenuBuilder-screen"></a>`.screen(f: impl Fn(&mut Ui) + Send + Sync + 'static)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1573)
- <a id="MenuBuilder-pauses"></a>`.pauses()`: Single player stops while it is open (the pause and settings screens). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1577)
- <a id="MenuBuilder-keep_on_escape"></a>`.keep_on_escape()`: Escape doesn't close it (the death screen). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1581)
- <a id="MenuBuilder-closes_on_respawn"></a>`.closes_on_respawn()`: It closes when the player respawns. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1585)
- <a id="MenuBuilder-shows_chat"></a>`.shows_chat()`: Like the chat screen: the HUD leaves the messages to it and Enter sends the text of its input of class `chat`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1590)
- <a id="MenuBuilder-on_key"></a>`.on_key(f: impl Fn(&crate::ClientView, &str) + Send + Sync + 'static)`: Keys pressed while it is on top, by their settings name (`Enter`, `KeyE`, `mouse3`), before Escape and the other global keys. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1595)
- <a id="MenuBuilder-default_screen"></a>`.default_screen(mut self)`: The usual screen: its title, its own groups in order, the player's inventory. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1600)

## PlayerInventoryBuilder

- <a id="PlayerInventoryBuilder-group"></a>`.group(name: &str, slots: Slots)`: A group of the player's inventory: its size from `slots` (`Slots::row(9).hotbar()`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L708)

## Registry

- <a id="Registry-camera"></a>`.camera(mode: &'static CameraMode) -> CameraBuilder<'_>`: A camera mode; `r.edit(&cameras::FRONT)` changes one already there (the engine's, another mod's): flags added and taken away, its shot, key and change replaced. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/camera.rs#L200)
- <a id="Registry-client_frame"></a>`.client_frame(f: impl Fn(&Client) + Send + Sync + 'static)`: Every frame on the client, before input is handled. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L582)
- <a id="Registry-client_entity_frame"></a>`.client_entity_frame(kind: &crate::EntityKind, f: impl Fn(&Client, &ClientEntity) + Send + Sync + 'static)`: Every frame on the client for each entity of `kind`, the local player too: its effects (wing flaps, sparks, purring), any number of mods on one kind. The engine's steps, idle noises and death smoke go on unless the kind says otherwise (`.effect(..)`, `.own_effects()`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L590)
- <a id="Registry-packet"></a>`.packet<T: Payload>(channel: &Channel<T>, f: impl Fn(&Packet, T) + Send + Sync + 'static)`: Server side of a channel: `f` gets every value a client sends on it, with who sent it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L598)
- <a id="Registry-client_packet"></a>`.client_packet<T: Payload>(channel: &Channel<T>, f: impl Fn(&Client, T) + Send + Sync + 'static)`: Client side of `channel`: called when the server sends on it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L609)
- <a id="Registry-key"></a>`.key<'a>(&'a mut self, key: &'static Key, default: &str) -> KeyBuilder<'a>`: Key binding with a default key (`J`, `KeyJ`, `F6`, `space`, `lshift`, `mouse3`), listed in Controls as `key.<ns>.<name>`; what it does with `.on_client(..)` or `.on_server(..)`, or read with `Client::key_pressed`/`key_held`. Keys only act while the world has the input (not while typing in the chat or with a screen open). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L623)
- <a id="Registry-config"></a>`.config<'a, T: ConfigValue>(&'a mut self, config: &'static Config<T>) -> ConfigBuilder<'a>`: A setting, see `ConfigBuilder` (a bool `false` unless told otherwise). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L182)
- <a id="Registry-text_config"></a>`.text_config<'a>(&'a mut self, config: &'static TextConfig, default: &str) -> ConfigBuilder<'a>`: A text setting, see `ConfigBuilder::text`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L187)
- <a id="Registry-entity_effect"></a>`.entity_effect<'a>(&'a mut self, effect: &'static EntityEffect) -> EntityEffectBuilder<'a>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L411)
- <a id="Registry-precipitation"></a>`.precipitation<'a>(&'a mut self, p: &'static Precipitation) -> PrecipitationBuilder<'a>`: Something to fall from the sky (`.texture`, `.speed`, `.density`); a biome lets it fall with `.precipitation(..)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L417)
- <a id="Registry-weather"></a>`.weather<'a>(&'a mut self, weather: &'static Weather) -> WeatherBuilder<'a>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L553)
- <a id="Registry-pose"></a>`.pose<'a>(&'a mut self, pose: &'static Pose) -> PoseBuilder<'a>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L627)
- <a id="Registry-game_mode"></a>`.game_mode<'a>(&'a mut self, mode: &'static GameMode) -> GameModeBuilder<'a>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L726)
- <a id="Registry-damage_cause"></a>`.damage_cause<'a>(&'a mut self, cause: &'static DamageCause) -> DamageCauseBuilder<'a>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L811)
- <a id="Registry-namespace"></a>`.namespace() -> &'static str` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1105)
- <a id="Registry-has_mod"></a>`.has_mod(id: &str) -> bool`: Whether mod `id` is loaded, to register what goes with it only then (all mods are known before any registers, whatever the order). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1111)
- <a id="Registry-edit"></a>`.edit<K: kind::Builds>(handle: &'static Handle<K>) -> K::Builder<'_>`: Changes only what the builder sets of something registered already, another mod's or this one's; the rest stays. Blocks (`r.edit(&base::CACTUS).light(5)`: on every state, `state_where` on some), items, biomes, dimensions, menus, ores (`IRON_ORE.ore()`), liquids (`WATER.liquid()`), terrains (`OVERWORLD.terrain()`), loot tables (`ZOMBIE.loot()`), entity kinds (`r.edit(&PLAYER).attach(&MANA)`) and the rest by their handles. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1131)
- <a id="Registry-edit_pattern"></a>`.edit_pattern<K: kind::Builds + 'static>(pattern: &str) -> K::Builder<'_>`: `edit` of everything of a kind `pattern` names, any mod's: `#tag` (a block's states by their own tags; a component too, `#fuel` anything that burns), a name with `*` in it, `name[prop=value]` (those states of a block): `r.edit_pattern::<kind::Block>("#planks/*").set(&FUEL, 300)`. Patterns apply after edits by name. For blocks and items a pattern matching nothing is a load error (a `#tag` one when no such tag exists), for other kinds it is fine. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1140)
- <a id="Registry-replace"></a>`.replace<K: kind::Builds>(handle: &'static Handle<K>) -> K::Builder<'_>`: Registers `handle` from scratch over what other mods registered as it, nothing of theirs staying: `r.replace(&base::TORCH).texture("blue_torch")`. Of several mods replacing one thing, the last in load order wins. A block's item is replaced too (`.no_item()` leaves theirs), its loot table only with `.drops*`, its block entity only with `.entity`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1148)
- <a id="Registry-remove"></a>`.remove<K: kind::Kind>(handle: &Handle<K>)`: As if nobody had registered `handle` (a block takes its item along, not its loot table or block entity). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1154)
- <a id="Registry-block"></a>`.block<'a>(&'a mut self, block: &'static Block) -> BlockBuilder<'a>`: Registers a block and, unless `.no_item()` is called, an item that places it (and carries the block's components). Texture defaults to `block/<name>` for all faces. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1167)
- <a id="Registry-item"></a>`.item<'a>(&'a mut self, item: &'static Item) -> ItemBuilder<'a>`: Texture defaults to `item/<name>`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1192)
- <a id="Registry-biome"></a>`.biome<'a>(&'a mut self, biome: &'static Biome) -> BiomeBuilder<'a>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1209)
- <a id="Registry-creative_tab"></a>`.creative_tab(tab: &'static CreativeTab, icon: &Item) -> CreativeTabBuilder<'_>`: Creative inventory tab named `tab.<ns>.<name>` in the language files, shown with `icon`. Blocks and items join with `.tab(&TAB)`; untabbed items land in a tab named after the mod. Tabs come in mod load order, then registration order. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1216)
- <a id="Registry-template"></a>`.template(template: &'static Template) -> TemplateBuilder<'_>`: A fixed shape of blocks, drawn in layers of rows (`layer` + `key`) or filled with boxes (`fill`), checked when the game loads. A file `structures/<name>.json` of the mod is one by itself; this adds to it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1228)
- <a id="Registry-record"></a>`.record(record: &'static Record) -> RecordBuilder<'_>`: A mod's own data, for itself or an API other mods use: tags and components say what it is, and other mods `edit`, `replace` and `remove` it like anything registered. Read once the game is loaded: `entries_with(entry_kind::RECORD, "structgen:structure")` lists them, `CHANCE.on_record(id)` reads one. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1244)
- <a id="Registry-palette"></a>`.palette(palette: &'static Palette) -> PaletteBuilder<'_>`: The blocks templates' slots take where a feature uses it (`FeatureBuilder::palette`): `r.palette(&SPRUCE).slot("wall", "base:spruce_planks")`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1250)
- <a id="Registry-dimension"></a>`.dimension<'a>(&'a mut self, dimension: &'static Dimension) -> DimensionBuilder<'a>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1254)
- <a id="Registry-set"></a>`.set<T: Value>(what: &str, comp: &Component<T>, v: T)`: Sets a component on blocks and items of any mod, by reference: `r.set("base:coal", &FUEL, 1600)`, `r.set("#planks/*", &FUEL, 300)`, `r.set("base:furnace[lit=true]", &HOT, ..)`. An edit by pattern of both kinds (`edit_pattern`), so after every edit by name; among patterns the last mod in load order wins, and naming nothing of either is a load error. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1262)
- <a id="Registry-tag"></a>`.tag(tag: &str, names: &[&str])`: Adds `tag` to blocks and items of any mod, by reference: `r.tag("ingots/tin", &["othermod:tin_ingot"])`, `r.tag("plant_soil", &["#dirt"])`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1268)
- <a id="Registry-set_model"></a>`.set_model(what: impl ModelTarget, model: impl ModelSource)`: What an item or an entity of any mod looks like. From a file, `models/<file>.geo.json` with its `textures/<file>.png`, shown everywhere the item shows, or from code: `r.set_model("base:apple", "foodmod:apple")`, `r.set_model(&PLAYER, humanoid(false))`. A whole tag at once takes `{name}`; items without a file stay as they were: `r.set_model("#food", "foodmod:food/{name}")`. An entity kind's is an edit of it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1299)
- <a id="Registry-set_animations"></a>`.set_animations(what: impl ModelTarget, file: &str)`: The clips of an item or an entity of any mod: `r.set_animations(&PLAYER, "mymod:player")` plays `animations/player.animation.json` of `mymod` on every player. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1305)
- <a id="Registry-bone_model"></a>`.bone_model(kind: &crate::EntityKind, model: &str, bone: &str)`: Every entity of `kind` has `model` on its bone `bone`, drawn in that model's own pixels, with the same origin as the kind's. What plays on the entity plays on it too, by the names of clips and bones: `r.bone_model(&PLAYER, "tailmod:tail", "body")`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1322)
- <a id="Registry-bone_motion"></a>`.bone_motion(model: &str, bone: &str, motion: BoneMotion)`: `bone` of `model` moves by itself. A bone called `hanging` or `hanging_<anything>` does without this. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L1332)
- <a id="Registry-liquid"></a>`.liquid(liquid: &'static Block) -> LiquidBuilder<'_>`: A liquid the engine flows; see `LiquidBuilder`. Defaults are vanilla water's numbers, an average of its texture for the fog's color. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3822)
- <a id="Registry-liquid_reaction"></a>`.liquid_reaction(liquid: &str, touching: &str) -> ReactionBuilder<'_>`: When liquid `liquid` touches `touching` (block matchers), see `ReactionBuilder`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/content.rs#L3832)
- <a id="Registry-sky_body"></a>`.sky_body<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a>`: A picture on an orbit, a sun or a moon, independent of everything else in the sky: [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L292)
- <a id="Registry-sky_stars"></a>`.sky_stars<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a>`: Points scattered over the sky and turning together, seen at night unless `opacity` says otherwise. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L298)
- <a id="Registry-sky_dome"></a>`.sky_dome<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a>`: The whole sky at once, a skybox picture or a shader, standing still unless given an orbit. It starts out still, and over the day's colors; `opacity(0.0, 1.0)` keeps it to the night. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L304)
- <a id="Registry-sky_clouds"></a>`.sky_clouds<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a>`: A layer of boxes in the air, 12 blocks each, wherever `texture`'s pixels are filled: `r.sky_clouds(&OVERWORLD, &CLOUDS).texture("clouds").height(300.0)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L310)
- <a id="Registry-particle"></a>`.particle<'a>(&'a mut self, particle: &'static Particle) -> ParticleBuilder<'a>`: A particle kind, see `ParticleBuilder`. Editing or replacing `core:poof` (`Particle::named("core:poof")`) changes how the engine's own smoke looks, `core:block` its block dust. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L317)
- <a id="Registry-condition"></a>`.condition(condition: &'static crate::Condition, f: impl Fn(&ConditionCtx) -> bool + Send + Sync + 'static)`: A condition music, text (`{if:<ns>:<name>}`), loot, spawning, poses and cameras (`.when`, `Cond::from(&C)`) can ask, with an argument if written so (`mymod:near(lava)`, `c.arg()`). `f` is answered on whichever side asks: on the server (`c.world()`) for loot and spawning, on the client (`c.client()`) for music and text, every frame such a text is on screen. Every game mode is a condition too, by its name. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L326)
- <a id="Registry-sound"></a>`.sound<'a>(&'a mut self, sound: &'static crate::Sound) -> SoundBuilder<'a>`: A sound event, see `SoundBuilder`; blocks and entities name groups of them with `.sound_group()`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L570)
- <a id="Registry-sound_category"></a>`.sound_category(category: &'static SoundCategory, world: bool) -> SoundCategoryBuilder<'_>`: A sound category with its own volume setting (`<ns>:volume_<name>`, in the mod's settings); `world`: heard through caves and water like block sounds. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L576)
- <a id="Registry-music"></a>`.music<'a>(&'a mut self, music: &'static Music) -> MusicBuilder<'a>`: Background music, see `MusicBuilder`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L586)
- <a id="Registry-system"></a>`.system<T: Value>(comp: &crate::Component<T>, f: impl Fn(&mut SystemBatch<T>) + Send + Sync + 'static)`: A system over a per-entity component (`Registry::entity_component`): once per tick and region with all values in one slice, no host call per entity. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L999)
- <a id="Registry-on_event"></a>`.on_event<T: Value>(event: &impl AsRef<Event<T>>, f: impl Fn(&EventBatch<T>) + Send + Sync + 'static)`: Handles every value of `event` once per tick (and region), in one call. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L1017)
- <a id="Registry-before_event"></a>`.before_event(question: &impl Valued, f: impl Fn(&Ask) -> Verdict + Send + Sync + 'static) -> StepBuilder<'_>`: Answers `question` (`question::*`, `actions::*` or a mod's own) before it happens, in `phase::RULES` unless `.phase(..)` says otherwise. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L1033)
- <a id="Registry-modify"></a>`.modify(chain: &impl Valued, phase: &Phase, f: impl Fn(&mut Modify) + Send + Sync + 'static) -> StepBuilder<'_>`: A step on `chain` (`chain::*`, an attribute, a question, a mod's own) in `phase`. One others should be able to order around or replace is `Registry::step`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L1042)
- <a id="Registry-step"></a>`.step(step: &'static Step) -> StepBuilder<'_>`: A step of this mod's by name: `.on(&chain)` runs it in every computation of the chain, without it only modifiers naming it run it (`Modifier::step`). `r.edit(&Step::named( "core:crit"))` reorders or recodes anyone's, `r.remove` takes it out. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L1049)
- <a id="Registry-phase"></a>`.phase(phase: &'static Phase, after: &Phase) -> PhaseBuilder<'_>`: A phase of this mod's, right after `after`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L1063)
- <a id="Registry-attribute"></a>`.attribute(attribute: &'static Attribute, default: f64, min: f64, max: f64) -> AttributeBuilder<'_>`: An attribute of living entities (`attribute::*` are the engine's). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L1072)
- <a id="Registry-loot"></a>`.loot<'a>(&'a mut self, table: &dyn LootSource) -> LootBuilder<'a>`: A loot table: a block's or entity kind's is what it drops, a `LootTable` is one of its own. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L1083)
- <a id="Registry-stack_component"></a>`.stack_component<T: crate::component::Data>( &mut self, c: &crate::Component<T>, flags: u32) -> crate::game::DeclarationBuilder<'_>`: Declares a component stacks carry a value of (`stack.with(&COMP, v)`), with its flags (`abi::stack_flags`: server-only, not saved, or read by conditional looks), 0 for none. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/extend.rs#L1093)
- <a id="Registry-feature"></a>`.feature(feature: &'static Feature) -> FeatureBuilder<'_>`: A feature with nothing set: say how it spreads (`per_cube`, `spacing`, `generate`), its shape (`vein`, `template`, `plan`) and what it writes. The presets below are the usual mixes. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L113)
- <a id="Registry-ore"></a>`.ore(block: &Block) -> FeatureBuilder<'_>`: An ore, named like its block: 4 veins of 8 blocks per cube unless said, in every dimension but bare ones (`.dimension(..)` limits it), only where it `replaces`: `r.ore(&TIN_ORE).y(0, 64).per_cube(6).vein_size(8).replaces("#stone")`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L120)
- <a id="Registry-scatter"></a>`.scatter(feature: &'static Feature) -> FeatureBuilder<'_>`: Something scattered over the ground (trees): one start per `spacing` x `spacing` square (8 unless said) kept with the chance a biome component gives (`density`), drawn by a `plan`. Starts under the sea or where caves took the ground are left out, as are those that would stand in a structure. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L138)
- <a id="Registry-structure"></a>`.structure(template: &Template) -> FeatureBuilder<'_>`: A template placed now and then on the ground, named like it: a `chance` of 0.5 per `spacing` of 32 unless said, in every dimension but bare ones. It keeps trees out, and gives way to structures of a higher priority (`priority`, 1 here). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L147)
- <a id="Registry-decoration"></a>`.decoration(f: impl Fn(&mut Gen) + Send + Sync + 'static) -> FeatureBuilder<'_>`: Runs `f` on every generated cube after everything else, column by column (grass, flowers), in every dimension but bare ones unless `.dimension(..)` limits it. `g.columns()` has the surface of every column. Pure, like a generator. Named `<mod>:decoration`, `/2`.. unless `named`, for other mods' `edit` and `remove`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/feature.rs#L159)
- <a id="Registry-entity"></a>`.entity<'a>(&'a mut self, kind: &'static EntityKind) -> EntityBuilder<'a>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1624)
- <a id="Registry-instance_component"></a>`.instance_component<T: Value>(comp: &Component<T>, default: T, flags: u32) -> DeclarationBuilder<'_>`: Declares `comp` for anything that holds a value of its own: entities (as `entity_component` does) and stacks (as `stack_component`). With `component_flags::CARRIED` the value goes from a broken block onto its item and back when placed, as a renamed chest's name does. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1638)
- <a id="Registry-entity_component"></a>`.entity_component<T: Value>(comp: &Component<T>, default: T, flags: u32) -> DeclarationBuilder<'_>`: Declares per-entity storage for `comp`: the value an entity starts with and the `component_flags` (saved, synced to whom). Kinds carry it with `.attach(&COMP)` (another mod's kind with `r.edit(&KIND).attach(&COMP)`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1648)
- <a id="Registry-world_tick"></a>`.world_tick(f: impl Fn(&World) + Send + Sync + 'static)`: Logic that is nobody's own (players' hunger, a mod's weather): once per tick in every region of every dimension, where `w.players()` are the players in that region. Another mod's entities are reached through events (`r.on_event(&actions::HURT, ..)`) and questions. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1665)
- <a id="Registry-menu"></a>`.menu<'a>(&'a mut self, menu: &'static Menu) -> MenuBuilder<'a>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1670)
- <a id="Registry-player_slots"></a>`.player_slots() -> PlayerInventoryBuilder<'_>`: The player inventory as named groups, one of them `.hotbar()`: `r.player_slots().group("hotbar", Slots::row(9).hotbar()).group("main", Slots::grid(9, 3))`. The engine lays them out, hotbar first, the equipment slots (`equipment_slot`) after everything; `w.inventory(player).group(name)` reads a group, `core:equipment` all of the equipment. The mod loaded last wins. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1679)
- <a id="Registry-equipment_slot"></a>`.equipment_slot(slot: &'static EquipmentSlot, covers: &[&str], background: &str) -> EquipmentSlotBuilder<'_>`: An equipment slot of players (head, chest...): what is worn there covers the bones named in `covers` (`worn::CHEST`, `&["leftWing", "rightWing"]`); a name with a role (`head`, `leftArm`, `leg0`) covers every bone of that role. `background` (from `gui/`) shows while it is empty. Items go there with `.wearable(&SLOT, ..)`; menus show the slots with `.equipment()`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1688)
- <a id="Registry-player_menu"></a>`.player_menu(menu: &Menu)`: The menu the inventory key opens. The mod loaded last wins. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1701)
- <a id="Registry-storage"></a>`.storage(storage: &Storage, slots: u32) -> DeclarationBuilder<'_>`: An inventory of `slots` a holder can keep besides its own (`Storage`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1722)
- <a id="Registry-command"></a>`.command( &mut self, name: &str, usage: &str, f: impl Fn(&mut crate::ServerCommand) + Send + Sync + 'static) -> crate::CommandOptions`: Chat command `/name` run on the server; `usage` declares its arguments (`<name:type>`, `[name:type]`, literal words, `|` alternatives), which the engine parses, checks and completes. An engine command of that name (`/give`, `/tp` ...) is replaced by it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1733)
- <a id="Registry-client_command"></a>`.client_command(name: &str, usage: &str, f: impl Fn(&mut crate::ClientCommand) + Send + Sync + 'static)`: Like `command`, but runs on the client that types it (`cmd.client()`), never on the server: for what only the player's own game has (views, local settings, debug output). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1746)
- <a id="Registry-word_set"></a>`.word_set<'a>(&'a mut self, set: &'static WordSet) -> WordSetBuilder<'a>`: A word set: command parameters `word(#set)` complete from its words, which other mods add to with `r.edit(&SET).words(..)` (the engine's sets too, `word_sets::GIZMOS`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1753)
- <a id="Registry-gizmo_group"></a>`.gizmo_group(group: &GizmoGroup)`: A gizmo group of this mod, offered by `/gizmo`'s completion. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1758)
- <a id="Registry-command_branch"></a>`.command_branch(command: &str, usage: &str, f: impl Fn(&mut crate::ServerCommand) + Send + Sync + 'static)`: A new branch of an existing server command, handled here: `r.command_branch("time", "dawn", |cmd| ..)` makes `/time dawn` this mod's. Earlier alternatives win when both fit. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1764)
- <a id="Registry-client_command_branch"></a>`.client_command_branch(command: &str, usage: &str, f: impl Fn(&mut crate::ClientCommand) + Send + Sync + 'static)`: The same for a client command (`/gizmo`, `/log`, a mod's `client_command`): `r. client_command_branch("gizmo", "clear", |cmd| ..)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1771)
- <a id="Registry-screen_part"></a>`.screen_part<K: crate::kind::Kind>( &mut self, screen: &crate::Handle<K>, area: &str, order: i32, f: impl Fn(&mut Ui) + Send + Sync + 'static) -> ScreenPartBuilder<'_>`: Adds to place `area` of a screen or HUD element without replacing it: a main menu button is `r.screen_part(&screens::TITLE, "buttons", 0, |ui| ..)`. Lower `order` comes first. The engine's places: `TITLE` `buttons` and `side`, `PAUSE` `buttons`, `SETTINGS` `buttons`, and beside menus: any menu's own `side` (`r.screen_part(&base::CHEST, "side", ..)`, another mod's menu too) and `MENUS` `side` beside every one (a sort button). Named `<mod>:<screen>/<area>` (`base:pause/buttons`, the next `/2`..), other mods edit it by that. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1782)
- <a id="Registry-hud_element"></a>`.hud_element( &mut self, element: &'static HudElement, anchor: &str, order: i32, f: impl Fn(&mut Ui) + Send + Sync + 'static) -> HudElementBuilder<'_>`: HUD element at `anchor` (`hud_anchor::*`); `order` 0 sits nearest the anchor's edge. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1803)
- <a id="Registry-tint_kind"></a>`.tint_kind(kind: &'static TintKind, default: u32) -> TintKindBuilder<'_>`: A tint kind: blocks with `.tint(&KIND)` take its color from the biome they stand in (`BiomeBuilder::tint`, `r.edit(&BIOME).tint(..)` for another mod's), `default` (0xRRGGBB) elsewhere and on items. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1823)
- <a id="Registry-entity_category"></a>`.entity_category(category: &'static EntityCategory, cap: u32, despawn: f32) -> CategoryBuilder<'_>`: An entity category: at most `cap` of it spawn naturally near a player; with `despawn` > 0 they vanish when no player is within that many blocks. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1834)
- <a id="Registry-recipe_kind"></a>`.recipe_kind<'a>(&'a mut self, kind: &'static RecipeKind) -> RecipeKindBuilder<'a>`: Registers a recipe kind this mod owns, and how the recipe viewer shows it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1845)
- <a id="Registry-shaped"></a>`.shaped<'a>( &'a mut self, kind: &RecipeKind, output: &str, count: u32, pattern: &[&str], keys: &[(char, &str)]) -> RecipeBuilder<'a>`: Shaped recipe: `pattern` rows use the characters in `keys`; ingredients are item names or `#tag`. More results and data with the builder it returns, named as `RecipeBuilder` says. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1851)
- <a id="Registry-shapeless"></a>`.shapeless<'a>(&'a mut self, kind: &RecipeKind, output: &str, count: u32, inputs: &[&str], time: u32) -> RecipeBuilder<'a>`: Shapeless recipe or a processing recipe (inputs used up over `time` ticks). Inputs are items or `#tags`; a liquid is its item, named like it, counted in millibuckets. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1871)
- <a id="Registry-natural_spawn"></a>`.natural_spawn(entity: &EntityKind) -> SpawnBuilder<'_>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2226)
- <a id="Registry-shapes_of"></a>`.shapes_of<'a>(&'a mut self, name: &str, sources: &str, suffix: &str) -> BlockBuilder<'a>`: Blocks made of others: `sources` are names and tags (`#has_shapes`, `#planks/*`, `base:stone`), and each full cube among them, of any mod, gets a block `<its name><suffix>` from this one. It shows the source's textures on what `.states(..)` leaves to it, and is mined and sounds like it. The lang key `shape.<name>` names them, `{0}` being the source. Each is tagged `<name>/<source>` and its source `<name>_of/<source>`, for one recipe to cover all: `r.shaped(C, "#wall/{m}", 6, &["###", "###"], &[('#', "#wall_of/{m}")])`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/shapes.rs#L53)
- <a id="Registry-stairs"></a>`.stairs<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a>`: Stairs of `sources` (see `shapes_of`): `<name>_stairs`, turned the way the player looks, joining into corners with those around them. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/shapes.rs#L60)
- <a id="Registry-slabs"></a>`.slabs<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a>`: Slabs of `sources` (see `shapes_of`): `<name>_slab`, in the half of the block clicked. The other half takes one of any block, the same one making it a whole block of it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/shapes.rs#L91)
- <a id="Registry-vertical_slabs"></a>`.vertical_slabs<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a>`: Vertical slabs of `sources` (see `shapes_of`): `<name>_vertical_slab`, against what was clicked. On a floor or a ceiling, on the side of the block nearest to where it was clicked. The other side takes one of any block, the same one making it a whole block of it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/shapes.rs#L119)
- <a id="Registry-beams"></a>`.beams<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a>`: Beams of `sources` (see `shapes_of`), a quarter of a block (1 x 1/2 x 1/2): `<name>_beam`, across where the player looks, in the half of the cell clicked, high or low. Several share a cell, as eighths do. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/shapes.rs#L149)
- <a id="Registry-eighths"></a>`.eighths<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a>`: Eighths of `sources` (see `shapes_of`), half a block each way: `<name>_eighth`, in the corner of the cell clicked. Eight fill a cell, each broken alone. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/shapes.rs#L174)
- <a id="Registry-text_place"></a>`.text_place<'a>(&'a mut self, place: &'static TextPlace) -> TextPlaceBuilder<'a>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L37)
- <a id="Registry-chat_format"></a>`.chat_format(f: impl Fn(&mut ChatFormat) + Send + Sync + 'static)`: Writes players' chat lines. There is one line, so one writer: the last mod loaded wins. To keep a message out of the chat intercept `actions::CHAT`, to read them listen to it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L174)
- <a id="Registry-terrain"></a>`.terrain(dimension: &Dimension) -> TerrainBuilder<'_>`: The terrain of your dimension; another mod's is `r.edit(OVERWORLD.terrain())`, applied after the owner's description in load order. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/terrain.rs#L514)

## SlotGroup

Slots of one group, numbered from 0: of an inventory (`w.inventory(h).group(..)`) or of an open menu (`m.group(..)`, whoever's inventory they show), valid while the menu callback runs.

- <a id="SlotGroup-len"></a>`.len() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L666)
- <a id="SlotGroup-is_empty"></a>`.is_empty() -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L669)
- <a id="SlotGroup-cols"></a>`.cols() -> u32`: Its width as laid out (a 3 x 3 grid has 3). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L673)
- <a id="SlotGroup-rows"></a>`.rows() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L676)
- <a id="SlotGroup-get"></a>`.get(i: u32) -> ItemStack` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L679)
- <a id="SlotGroup-set"></a>`.set(i: u32, stack: ItemStack)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L688)

## Slots

A group of slots: a grid, row or column of one inventory sharing its rules. Shown with `ui.group(name)`, read and written with `MenuCtx::group(name)`.

- <a id="Slots-grid"></a>`.grid(cols: u32, rows: u32) -> Slots` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1387)
- <a id="Slots-row"></a>`.row(n: u32) -> Slots` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1402)
- <a id="Slots-column"></a>`.column(n: u32) -> Slots` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1405)
- <a id="Slots-single"></a>`.single() -> Slots` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1408)
- <a id="Slots-chest"></a>`.chest(rows: u32) -> Slots`: Rows of nine, like a chest. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1412)
- <a id="Slots-menu"></a>`.menu(mut self)`: The menu's own slots, given back to the player when it closes (crafting grids). The default for `MenuBuilder::group`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1417)
- <a id="Slots-holder"></a>`.holder(mut self)`: The inventory of what the menu opened on: a block entity's (`.entity(|e| e.inventory(n))`), an entity's, a storage's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1423)
- <a id="Slots-hotbar"></a>`.hotbar(mut self)`: The player's hotbar, in `Registry::player_slots`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1428)
- <a id="Slots-at"></a>`.at(start: u32)`: From this index of the owner's inventory instead of right after the group before. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1433)
- <a id="Slots-trash"></a>`.trash(mut self)`: The player's trash (`slot_flags::TRASH`), one slot: it keeps the last thing put in until the next takes its place or the player leaves, and the trash key (Delete) over any slot sends that stack there. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1440)
- <a id="Slots-input"></a>`.input(mut self)`: Items only go in (a machine uses them up). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1445)
- <a id="Slots-liquid"></a>`.liquid(capacity: u32)`: Holds `capacity` millibuckets of one liquid: its item (`Item::named` the liquid's name), poured in and taken out with buckets by hand, read and set like any slot. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1451)
- <a id="Slots-crafting"></a>`.crafting(mut self)`: A crafting grid of the menu's first `.recipes(..)` kind: the engine shows what it makes in the menu's `output()` group and uses it up when that is taken, as the recipe says (`RecipeBuilder::keep`, `wear`, `leaves`); a shift-click crafts all that fits at once. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1459)
- <a id="Slots-output"></a>`.output(mut self)`: Items only come out (results). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1464)
- <a id="Slots-accepts"></a>`.accepts(items: &str)`: Only these items, a reference like everywhere else: `#fuel` (anything with the component), `#fuel #mymod:fire_starter` (either), `#fuel !base:lava_bucket` (but that one), names. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1470)
- <a id="Slots-limit"></a>`.limit(n: u32)`: At most this many items per slot. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1475)
- <a id="Slots-size"></a>`.size(px: f32)`: Side of a slot in GUI pixels (18 by default). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1480)
- <a id="Slots-large"></a>`.large()`: The big slot vanilla uses for results. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1485)
- <a id="Slots-background"></a>`.background(texture: &str)`: A texture from `gui/` shown while a slot is empty (the armor silhouettes). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1489)
- <a id="Slots-tooltip"></a>`.tooltip(key: &str)`: A translation shown over an empty slot. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1494)
- <a id="Slots-priority"></a>`.priority(p: i32)`: Shift-clicks from the player's inventory fill groups with a higher priority first. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1499)

## Storage

An inventory a holder keeps besides its own, under a name: each player's ender chest, a second pocket of a backpack. Declared with `r.storage(&S, slots)`; entity kinds carry it with `.attach_storage(&S)` (`r.edit(&PLAYER).attach_storage(&S)` for another mod's kind), stacks always can. `w.inventory(S.of(holder))` reaches it and `w.open_menu(player, &MENU, S.of(holder))` shows it, its `.holder()` groups being the storage's slots. Saved with its holder, and kept when a player respawns.

- <a id="Storage-named"></a>`.named(name: &'static str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L554)
- <a id="Storage-name"></a>`.name() -> &'static str` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L557)
- <a id="Storage-declaration"></a>`.declaration() -> &'static crate::Declaration`: Its declaration, for `Registry::edit`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L561)
- <a id="Storage-id"></a>`.id() -> u32`: `u32::MAX` before the registries are frozen or if nothing declared it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L566)
- <a id="Storage-slots"></a>`.slots() -> u32`: Its slots, 0 before the registries are frozen. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L570)
- <a id="Storage-of"></a>`.of(h: impl Into<crate::Holder>) -> crate::Holder`: This storage of `h` (an entity, a block, a held stack), for inventory calls and menus. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L574)

## Tag

A component without a value: membership in a group (`plant_soil`, `logs`, `ingots/iron`). `K` is the kind it is about (`crate::kind`), any kind by default, like `Component`'s.

- <a id="Tag-on_id"></a>`.on_id(id: u32) -> bool`: Entry `id` of its kind has it: `DRY.on_id(w.weather())`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L598)
- <a id="Tag-named"></a>`.named(name: &'static str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L604)
- <a id="Tag-name"></a>`.name() -> &'static str` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L607)
- <a id="Tag-id"></a>`.id() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L611)
- <a id="Tag-on"></a>`.on(kind: u32, id: u32) -> bool`: Entry `id` of `kind` (`abi::entry_kind`) has it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L616)
- <a id="Tag-on_block"></a>`.on_block(state: u32) -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L620)
- <a id="Tag-on_item"></a>`.on_item(item: u32) -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L624)
- <a id="Tag-on_record"></a>`.on_record(record: u32) -> bool`: A record (`Registry::record`) has it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L629)
- <a id="Tag-on_entity"></a>`.on_entity(kind: u32) -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L633)
- <a id="Tag-on_biome"></a>`.on_biome(biome: u32) -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L637)
- <a id="Tag-on_dimension"></a>`.on_dimension(dim: u32) -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L641)

## Text

A stable text view (`str` schema). Values serialize as UTF-8, never as pointers.

- <a id="Text-new"></a>`.new(s: &'static str) -> Text`: A text value from a static string. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L78)
- <a id="Text-as_str"></a>`.as_str() -> &'static str` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/component.rs#L81)

## TextConfig

A text setting (`ConfigBuilder::text`), the player's own.

- <a id="TextConfig-named"></a>`.named(name: &'static str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L78)
- <a id="TextConfig-get"></a>`.get() -> String` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L81)
- <a id="TextConfig-set"></a>`.set(v: &str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/config.rs#L84)
