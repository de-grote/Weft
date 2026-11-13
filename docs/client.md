# Client side

Mods run on the client too. UI has its own [guide](ui.md); this page is the rest.

## Screens and the HUD

- UI is built in code: `r.menu(&ABOUT).screen(|ui| ui.panel(..)).pauses()`, the engine's screens are in `screens::*` and its pause, death and settings screens use the same parts (inputs, sliders, items, `S::mono()`, `.scroll()`...). Other mods' screens take additions in their places: `r.screen_part(&screens::MENUS, "corner", 0, |ui| ..)` is in the top right corner inside every menu's panel (small `ui.icon_button(..)`s, what they do in their tooltips: base's sort and put away), `side` beside it, a menu's own `corner` and `side` for that one only, the title, pause and settings screens have `buttons`. `ui.button_double(..)` does something more on a double click; `c.client().menu_sort(None)` sorts the open menu's storage. Base puts away what the player carries into blocks tagged `storage` around it: tag your chests. `Slots::single().trash()` is the player's trash: the last thing put in stays until the next takes its place or the player leaves, and the trash key (Delete) over any slot sends that stack there
- HUD elements are built with `r.hud_element(&MANA_BAR, hud_anchor::HOTBAR_LEFT, 0, |ui| ..)`
- a bar of icons is `ui.icons("mana", 10, |c| value)` (`gui/mana_full`, `_half`, `_empty`; a negative value hides it, so it shows when you say), and it moves however you tell it, each by its own value read every frame: `.shaking_by(|c| ..)` (0 still, 2 hard), `.shaking_below(4.0)`, `.flash_on_gain(0.5)` / `.flash_on_loss(1.0)` (what changed blinks, `mana_flash` lights the bar), `.preview(1.6, |c| ..)` (what it would be, fading in and out), `.overlay(|c| ..)` (a second row in `mana_overlay_*` icons on top), `.wave_while(2.0, |c| ..)`, `.max_by(|c| ..)` (how many halves it holds: past the count more rows go on above, as hearts past twenty health). Base's hearts and food are made of these: food shakes with no saturation, shows the saturation outlined in gold and what the food in hand would fill
- F3 panels are at `hud_anchor::DEBUG_LEFT`
- Looks come from textures named after the class (`gui/button.png`, `_hover` ...) and named colors in the language files (`color.button`), so a resource pack can reskin screens ([UI guide](ui.md#looks))
- Bind values instead of copying them: `ui.progress_bind("energy", style, |c| ..)`, `ui.text_bind(..)`

## Key bindings

- key bindings, one declaration each: `r.key(&DASH, "R").on_client(|c| ..)` on the client (a chord works too: `"ctrl+R"`, `"shift+alt+F6"`, and any other key held as a modifier, `"F3+KeyK"` the way the engine's debug keys are; the wheel is `wheel_up` and `wheel_down`; up to two bindings, each held, `toggle`, a `double` tap or a `tap`, let go with no other key pressed meanwhile, which is how F3 alone opens the overlay: `"ctrl+R, double KeyR"`; the player rebinds them the same way in Controls). Tagged with contexts (`abi::comp::key::MOVEMENT`, `HANDS`, `RIDING`, `DEBUG`, `SCREEN`) a key conflicts only with those where both act, and a camera that takes the moves or the hands mutes those; without any it acts wherever the world has the input. They only act while the world has the input, typing in the chat doesn't trigger them. `c.key_pressed(&DASH)` / `c.key_held(&DASH)` / `c.key(&DASH)` read them in a frame hook; the engine's keys are `keys::*`
- the server hears every action, yours and the engine's, the mouse buttons' `keys::USE` and `keys::ATTACK` too: `r.on_event(&KEY, ..)` gets each press and release (`k.player`, `k.key`, `k.down`, `k.held_ticks`), `w.key(player, &DASH)` what is held now and for how long, and `.on_server(|w, player| ..)` / `.on_release_server(|w, player, ticks| ..)` on a key are those for one key. A charge, a combo, a cheat code typed before a spell goes off are all built from that

## Messages between server and client

- messages both ways on a typed channel: `static BOOST: Channel<Boost> = Channel::named("boost")` with a `value!` struct (or `String`, `Vec<u8>`, `()`), `r.packet(&BOOST, |p, b| ..)` on the server (`p.reply(&OTHER, &v)` answers), `r.client_packet` on the client, `w.send(who, ..)` (a player's id or any `Who`) / `client.send`. Both ends must declare the same type, the engine checks it like event values
- `client.run_command("/tp ~ ~5 ~")` runs a command as the player and `client.command_reply(id)` gives its answer to your code instead of the chat (testkit drives its server half like this)

## Frames, cosmetics and assets

- frame hooks (`r.client_frame`)
- cosmetics of your mobs: `r.client_entity_frame(&MOB, |c, e| ..)` every frame, with its position, distance walked, age and death; `.own_effects()` on the kind turns off the engine's steps, idle noises and death smoke
- particles and screen effects from shader snippets in `assets/shaders`
- sounds with `r.sound(..).category(..)` (every category gets its own volume slider), music

## Settings

settings that show up in Mod Settings by themselves (`r.config(&SPARKS).bool(true)`); the player's own by default, `.server()` for the server's value (sent to its players on every change), `.world()` for one saved with the world. The engine's settings are the same kind of thing (`settings::FOV`, `settings::LANGUAGE`, key bindings `core:key.<action>`, all in `config/core.txt`): set one and the client applies it
