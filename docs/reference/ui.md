# UI reference

Screens, HUD parts and the styles they use.

## ClientView

Read-only client state for bindings.

- <a id="ClientView-client"></a>`.client() -> crate::Client`: The client's actions (`menu_open`, `world_open`, `quit`..), for a button's callback: a button may drive the client, a binding that only shows a value may not. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L24)
- <a id="ClientView-player"></a>`.player() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L27)
- <a id="ClientView-target"></a>`.target() -> Option<abi::Target>`: The block or entity under the crosshair, as the client uses it for interactions. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L31)
- <a id="ClientView-attribute"></a>`.attribute(a: &crate::Attribute) -> f64`: Own player's attribute, as the server says. Mods' attributes come too. Unlike `World::attribute` it takes no entity: the client only has its own player's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L37)
- <a id="ClientView-effect_level"></a>`.effect_level(effect: &crate::EntityEffect) -> u32`: The level of `effect` on the local player, 0 without it (poisoned hearts). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L41)
- <a id="ClientView-swing_strength"></a>`.swing_strength() -> f32`: How charged the next swing is, 0..1: for a bar of your own. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L45)
- <a id="ClientView-use_ticks_left"></a>`.use_ticks_left() -> Option<f32>`: Ticks left of the item being used. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L49)
- <a id="ClientView-day_time"></a>`.day_time() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L53)
- <a id="ClientView-weather"></a>`.weather() -> u32`: The weather's id now (`weathers::RAIN.id()`), as the server says. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L57)
- <a id="ClientView-permission"></a>`.permission() -> u32`: The player's permission level (`abi::permission`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L61)
- <a id="ClientView-component"></a>`.component<T: Value>(c: &Component<T>) -> Option<T>`: Own player's synced component. Unlike `World::component` it takes no entity: another one's is `component_of`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L66)
- <a id="ClientView-player_state"></a>`.player_state() -> Option<abi::EntityState>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L70)
- <a id="ClientView-inventory_slot"></a>`.inventory_slot(slot: u32) -> ItemStack`: Own inventory slot by index (the engine lays the inventory out from its groups; with the base game the hotbar comes first). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L76)
- <a id="ClientView-inventory_group"></a>`.inventory_group(name: &str) -> Vec<ItemStack>`: The stacks of a group of the own inventory (`hotbar`, `core:equipment`...). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L80)
- <a id="ClientView-creative"></a>`.creative() -> bool`: The local player is in creative mode. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L90)
- <a id="ClientView-outside_survival"></a>`.outside_survival() -> bool`: The local player's mode spares it damage and hunger: no bars to show. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L94)
- <a id="ClientView-game_mode"></a>`.game_mode() -> u32`: `abi::game_mode`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L98)
- <a id="ClientView-component_of"></a>`.component_of<T: Value>(entity: u64, c: &Component<T>) -> Option<T>`: Another entity's component the server shares with this client (`SYNC_WATCHERS`, or `SYNC_MENU` while its menu is open). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L104)
- <a id="ClientView-menu_owner"></a>`.menu_owner() -> Option<u64>`: The entity the open menu is on (a block entity, a mob), if any. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L112)
- <a id="ClientView-menu_component"></a>`.menu_component<T: Value>(c: &Component<T>) -> Option<T>`: A component of what the open menu is on, synced with `SYNC_MENU`, for screens to bind to. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L117)

## HudElementBuilder

A HUD element being registered (`Registry::hud_element`).

- <a id="HudElementBuilder-rebuild_when"></a>`.rebuild_when(f: impl Fn(&crate::ClientView) -> u64 + Send + Sync + 'static)`: Rebuilds the layout when this revision changes. Keep changing text and numbers in bindings; use this for a different set of widgets, such as a new inspected block. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2044)
- <a id="HudElementBuilder-anchor"></a>`.anchor(anchor: &str)`: Where it sits (`hud_anchor::*`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2053)
- <a id="HudElementBuilder-order"></a>`.order(order: i32)`: 0 sits nearest the anchor's edge. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2057)
- <a id="HudElementBuilder-ui"></a>`.ui(f: impl Fn(&mut Ui) + Send + Sync + 'static)`: What it shows. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2061)

## Icons

A row of icons in the making (`Ui::icons`); it goes in as this goes.

- <a id="Icons-from_right"></a>`.from_right()`: Filling from the right (hunger). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L715)
- <a id="Icons-dir"></a>`.dir(dir: u32)`: Filling in direction `dir` (`abi::ui_dir`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L719)
- <a id="Icons-shaking"></a>`.shaking()`: Jumping about at 4 or less (hearts near the end). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L726)
- <a id="Icons-shaking_below"></a>`.shaking_below(value: f32)`: Jumping about at `value` or less. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L730)
- <a id="Icons-shaking_by"></a>`.shaking_by(f: impl Fn(&ClientView) -> f32 + Send + Sync + 'static)`: Jumping about as much as `f` says (0 still, 1 a pixel, 2 two...): food with no saturation left, harder when nearly empty. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L738)
- <a id="Icons-flash_on_gain"></a>`.flash_on_gain(secs: f32)`: What it gains blinks for `secs`, and `<class>_flash` lights it where there is one (hearts healing). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L743)
- <a id="Icons-flash_on_loss"></a>`.flash_on_loss(secs: f32)`: What it loses blinks for `secs` (hearts hurt). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L747)
- <a id="Icons-preview"></a>`.preview(secs: f32, f: impl Fn(&ClientView) -> f32 + Send + Sync + 'static)`: What the row would be at `f`'s value, fading in and out every `secs` (what the held food would fill, as AppleSkin shows it). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L752)
- <a id="Icons-overlay"></a>`.overlay(f: impl Fn(&ClientView) -> f32 + Send + Sync + 'static)`: A second row at `f`'s value in `<class>_overlay` icons over this one (saturation). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L756)
- <a id="Icons-wave_while"></a>`.wave_while(pixels: f32, f: impl Fn(&ClientView) -> f32 + Send + Sync + 'static)`: While `f` is above 0 a wave runs along the row, `pixels` high (hearts regenerating). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L760)
- <a id="Icons-max_by"></a>`.max_by(f: impl Fn(&ClientView) -> f32 + Send + Sync + 'static)`: The halves the row holds, `f`'s value (none: twice the count): past the count more rows go on above, closer the more there are (hearts past twenty health). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L765)
- <a id="Icons-effect"></a>`.effect(kind: u32, amount: f32, f: Option<Box<dyn Fn(&ClientView) -> f32 + Send + Sync>>)`: Any `abi::icon_effect` by `f`'s value (none: 1). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L769)
- <a id="Icons-look_while"></a>`.look_while(class: &'static str, f: impl Fn(&ClientView) -> f32 + Send + Sync + 'static)`: Drawn in `class`'s icons instead while `f` is above 0 (food turned green by hunger). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L776)

## MenuCtx

- <a id="MenuCtx-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1317)
- <a id="MenuCtx-taken_in"></a>`.taken_in(name: &str) -> Option<u32>`: In `on_take`, which slot of group `name` was taken from; `None` for another group's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1321)
- <a id="MenuCtx-player"></a>`.player() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1327)
- <a id="MenuCtx-times"></a>`.times() -> u32`: In `on_take` of a crafting grid's result (`Slots::crafting`), how many runs of its recipe that was (a shift-click crafts all that fit at once); 1 otherwise. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1332)
- <a id="MenuCtx-owner"></a>`.owner() -> Option<Holder>`: What the menu was opened on (a block entity, a mob, a pouch in a slot); `None` for the player's own and a crafting table's. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1337)
- <a id="MenuCtx-group"></a>`.group(name: &str) -> SlotGroup<'_>`: Slot group `name` of this menu; asking for one it doesn't have is a mistake that gets logged, and the group is empty. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1342)
- <a id="MenuCtx-find_group"></a>`.find_group(name: &str) -> Option<SlotGroup<'_>>`: `group`, when not having it is fine. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L1349)

## S

How a part is laid out and shown: the same properties the engine's own screens use. Sizes are GUI pixels; `None` = fit content.

- <a id="S-row"></a>`.row() -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L212)
- <a id="S-column"></a>`.column() -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L215)
- <a id="S-tooltip"></a>`.tooltip(key: &str) -> S`: A tooltip over the part (a translation key, with all of rich text). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L219)
- <a id="S-tooltip_args"></a>`.tooltip_args(key: &str, args: &[Arg]) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L222)
- <a id="S-class"></a>`.class(class: &'static str) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L234)
- <a id="S-h"></a>`.h(h: f32) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L238)
- <a id="S-w_pct"></a>`.w_pct(p: f32) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L242)
- <a id="S-h_pct"></a>`.h_pct(p: f32) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L246)
- <a id="S-min_w"></a>`.min_w(w: f32) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L250)
- <a id="S-min_h"></a>`.min_h(h: f32) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L254)
- <a id="S-max_w"></a>`.max_w(w: f32) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L258)
- <a id="S-self_align"></a>`.self_align(a: u32) -> S`: Its own place across its parent (`abi::ui_align`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L263)
- <a id="S-scroll"></a>`.scroll(mut self) -> S`: Children scroll (mouse wheel) and are clipped to the part, with a bar when longer. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L268)
- <a id="S-no_wrap"></a>`.no_wrap(mut self) -> S`: Text on one line. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L273)
- <a id="S-mono"></a>`.mono(mut self) -> S`: Text with every glyph in a digit-wide cell, so changing numbers don't move what follows (`{mono}..{/mono}` does it for a part of a text). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L279)
- <a id="S-scale"></a>`.scale(s: f32) -> S`: Text size (2 for a screen's title). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L284)
- <a id="S-disabled"></a>`.disabled(yes: bool) -> S`: A button that can't be pressed now. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L289)
- <a id="S-selected"></a>`.selected(yes: bool) -> S`: A button shown as the chosen one (the open tab). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L296)
- <a id="S-gap"></a>`.gap(g: f32) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L302)
- <a id="S-pad"></a>`.pad(p: f32) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L306)
- <a id="S-size"></a>`.size(w: f32, h: f32) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L310)
- <a id="S-w"></a>`.w(w: f32) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L315)
- <a id="S-grow"></a>`.grow(mut self) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L319)
- <a id="S-center"></a>`.center(mut self) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L323)
- <a id="S-stretch"></a>`.stretch(mut self) -> S`: Children fill the cross axis (rows as wide as the column), so `spacer()`s can center. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L329)
- <a id="S-end"></a>`.end(mut self) -> S` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L333)

## TextOut

- <a id="TextOut-set"></a>`.set(key: &str, args: &[Arg])` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L695)
- <a id="TextOut-text"></a>`.text(text: &str)`: Ready text, shown as it is (not a translation). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L701)

## Ui

- <a id="Ui-view"></a>`.view() -> ClientView` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L413)
- <a id="Ui-column"></a>`.column(s: S, f: impl FnOnce(&mut Ui))` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L436)
- <a id="Ui-row"></a>`.row(s: S, f: impl FnOnce(&mut Ui))` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L440)
- <a id="Ui-panel"></a>`.panel(class: &str, s: S, f: impl FnOnce(&mut Ui))`: A background of class `class` (`gui/<class>.png`, e.g. "panel"). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L445)
- <a id="Ui-text"></a>`.text(key: &str)`: Translated text. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L452)
- <a id="Ui-text_args"></a>`.text_args(key: &str, args: &[Arg])` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L456)
- <a id="Ui-text_styled"></a>`.text_styled(s: S, key: &str, args: &[Arg])`: Text with a class, size, one line, `mono`...: `ui.text_styled(S::column().class("title").scale(2.0), "mymod.title", &[])`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L461)
- <a id="Ui-text_bind"></a>`.text_bind(f: impl Fn(&ClientView, &mut TextOut) + Send + Sync + 'static)`: Text recomputed every frame; redrawn only when it changes. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L470)
- <a id="Ui-text_bind_styled"></a>`.text_bind_styled(s: S, f: impl Fn(&ClientView, &mut TextOut) + Send + Sync + 'static)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L474)
- <a id="Ui-screen_parts"></a>`.screen_parts(screen: &str, area: &str)`: What mods add to place `area` of a screen or HUD element (`Registry::screen_part`). A screen of your own that replaces an engine one (`core:title`) keeps other mods' buttons by building its places. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L486)
- <a id="Ui-recipe_inputs"></a>`.recipe_inputs()`: In a recipe kind's `display`: the recipe's inputs as the kind's grid. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L494)
- <a id="Ui-recipe_outputs"></a>`.recipe_outputs()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L497)
- <a id="Ui-recipe_input"></a>`.recipe_input(i: u32)`: Only input `i`, for layouts that place them apart. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L501)
- <a id="Ui-recipe_output"></a>`.recipe_output(i: u32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L504)
- <a id="Ui-group"></a>`.group(name: &str)`: Every slot of the menu's group `name` (`MenuBuilder::group`), laid out as its grid. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L516)
- <a id="Ui-group_slot"></a>`.group_slot(name: &str, i: u32)`: Only slot `i` of group `name`, for layouts that place a group's slots apart. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L521)
- <a id="Ui-player_inventory"></a>`.player_inventory(title: bool)`: The player's inventory and hotbar (`MenuBuilder::player_inventory`), spaced like vanilla, under the title "Inventory" when `title`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L534)
- <a id="Ui-title"></a>`.title()`: The open menu's title: what a player named the furnace or chest, else the name of the block it was opened on, else the translation `menu.<ns>.<name>`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L548)
- <a id="Ui-equipment"></a>`.equipment()`: The player's equipment slots of a menu with `.equipment()`, in a column. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L553)
- <a id="Ui-recipe_arrow"></a>`.recipe_arrow()`: The arrow of a menu that makes recipes (`.recipes(..)` on the menu): clicking it opens them in the recipe viewer. Empty, for a crafting grid. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L559)
- <a id="Ui-recipe_arrow_bind"></a>`.recipe_arrow_bind(f: impl Fn(&ClientView) -> f32 + Send + Sync + 'static)`: `recipe_arrow` showing progress (a furnace's cooking). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L564)
- <a id="Ui-progress_bind"></a>`.progress_bind(class: &str, s: S, f: impl Fn(&ClientView) -> f32 + Send + Sync + 'static)`: Progress from any value 0..1; a negative one hides it, like `icons`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L571)
- <a id="Ui-progress_bind_dir"></a>`.progress_bind_dir(class: &str, s: S, dir: u32, f: impl Fn(&ClientView) -> f32 + Send + Sync + 'static)`: Like `progress_bind`, filling in direction `dir` (`abi::ui_dir`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L576)
- <a id="Ui-icons"></a>`.icons(class: &str, count: u32, f: impl Fn(&ClientView) -> f32 + Send + Sync + 'static) -> Icons<'_>`: `count` icons showing a value in half-icon steps (hearts, hunger); a negative value hides them. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L586)
- <a id="Ui-image"></a>`.image(texture: &str, w: f32, h: f32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L594)
- <a id="Ui-player_model"></a>`.player_model(w: f32, h: f32)`: The local player's model with what it wears. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L601)
- <a id="Ui-spacer"></a>`.spacer()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L605)
- <a id="Ui-button"></a>`.button(key: &str, s: S, f: impl Fn(&ClientView) + Send + Sync + 'static)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L609)
- <a id="Ui-button_double"></a>`.button_double( &mut self, key: &str, s: S, f: impl Fn(&ClientView) + Send + Sync + 'static, double: impl Fn(&ClientView) + Send + Sync + 'static)`: A button that does something more when clicked twice in a row (a world in a list: once selects, twice plays). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L615)
- <a id="Ui-icon_button"></a>`.icon_button(texture: &str, w: f32, h: f32, s: S, f: impl Fn(&ClientView) + Send + Sync + 'static)`: A button showing a picture (a texture like `image`'s, `"mymod:gui/sort"`, `w` by `h` GUI pixels) and nothing else: what it does goes in its tooltip (`S::tooltip`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L633)
- <a id="Ui-button_args"></a>`.button_args(key: &str, args: &[Arg], s: S, f: impl Fn(&ClientView) + Send + Sync + 'static)`: A button whose label has arguments (`settings.gui_scale` with the scale). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L643)
- <a id="Ui-input"></a>`.input(value: &str, max: u32, s: S, on_change: impl Fn(&ClientView, &str) + Send + Sync + 'static)`: A one-line text field starting with `value` (at most `max` characters, 0 for any); `on_change` gets the text after every edit. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L654)
- <a id="Ui-slider"></a>`.slider( &mut self, s: S, value: impl Fn(&ClientView) -> f32 + Send + Sync + 'static, on_change: impl Fn(&ClientView, f32) + Send + Sync + 'static)`: A slider at `value` (0..1, read every frame); `on_change` gets where the player drags it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L663)
- <a id="Ui-item"></a>`.item(item: u32, count: u32, s: S)`: An item's icon, with `count` under it when above 1. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/ui.rs#L676)

## WordSetBuilder

A word set being registered or added to (`Registry::word_set`).

- <a id="WordSetBuilder-words"></a>`.words(words: &[&str])` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2079)
- <a id="WordSetBuilder-remove_word"></a>`.remove_word(word: &str)`: Takes a word out (another mod's, in an edit). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2085)
