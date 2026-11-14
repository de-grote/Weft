# Client reference

The client side: input, camera, screen text, packets.

## CameraBuilder

`Registry::camera`: registers when dropped.

- <a id="CameraBuilder-shot"></a>`.shot(shot: Shot)`: How it moves, with the tags that takes. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/camera.rs#L207)
- <a id="CameraBuilder-in_cycle"></a>`.in_cycle()`: One F5 goes through. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/camera.rs#L215)
- <a id="CameraBuilder-when"></a>`.when(c: impl Into<crate::Cond>)`: Only where this holds for the player, asked on the server, and the server lets it (`actions::CAMERA`): `.when(Cond::attribute(&attribute::CAN_FLY, 1.0, f64::MAX))`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/camera.rs#L220)
- <a id="CameraBuilder-key"></a>`.key(key: &'static Key, default: &str)`: A key that turns it on and back off, `default` as `Registry::key` takes it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/camera.rs#L224)
- <a id="CameraBuilder-transition"></a>`.transition(secs: f32, easing: u32)`: The change into it takes `secs`, eased by `easing` (`easing::*`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/camera.rs#L229)

## Channel

A message channel between a mod's client and server halves, carrying `T`; both ends declare the same type, which the engine checks like event values:

- <a id="Channel-named"></a>`.named(name: &'static str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L509)
- <a id="Channel-generated"></a>`.generated(name: String) -> &'static Self`: For names made at runtime; lives as long as the game. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L513)
- <a id="Channel-name"></a>`.name() -> &str` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L516)

## ChatFormat

A player's message on its way to the chat (`Registry::chat_format`).

- <a id="ChatFormat-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L151)
- <a id="ChatFormat-player"></a>`.player() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L155)
- <a id="ChatFormat-text"></a>`.text() -> &str`: What they typed. Pass it as `Arg::Text`, or a player gets to format the line. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L160)
- <a id="ChatFormat-set"></a>`.set(key: &str, args: &[Arg])`: The line everyone gets. Not called, it stays the engine's `<name> text`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L165)

## Client

The local client during a client callback. Only valid for the duration of that callback.

- <a id="Client-view"></a>`.view() -> ClientView`: The read-only view UI bindings get (own player, components, menu data). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L50)
- <a id="Client-state"></a>`.state() -> ClientState` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L54)
- <a id="Client-screen"></a>`.screen() -> String`: Top screen name (`core:menu`, `core:pause`, ...), empty in the world. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L61)
- <a id="Client-input_action"></a>`.input_action(key: &Key, secs: f32) -> bool`: Presses the key bound to `key` (yours from `Registry::key`, or one of `keys::*`) for `secs` (0 = a tap). False for a key nobody registered. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L71)
- <a id="Client-input_key"></a>`.input_key(key: &str, secs: f32) -> bool`: A key by its settings name (`Enter`, `Tab`, `KeyE`, `F3`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L76)
- <a id="Client-input_click"></a>`.input_click(button: u32, secs: f32)`: Mouse button 0 left, 1 right, 2 middle. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L81)
- <a id="Client-input_cursor"></a>`.input_cursor(x: f32, y: f32)`: Moves the mouse to fractions of the window. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L86)
- <a id="Client-input_scroll"></a>`.input_scroll(steps: f32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L90)
- <a id="Client-input_type"></a>`.input_type(text: &str)`: Text typed into the focused field. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L95)
- <a id="Client-bone_turn"></a>`.bone_turn(entity: u64, bone: &str, degrees: [f32`: Turns a bone of `entity`'s model or of what it has on, for this frame, on top of its clips: degrees about x, y, z, the way Blockbench counts them. From `client_frame` or `client_entity_frame`, every frame it should last. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L106)
- <a id="Client-bone_move"></a>`.bone_move(entity: u64, bone: &str, pixels: [f32`: Moves a bone this far in pixels, for this frame, the same way. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L112)
- <a id="Client-bone_scale"></a>`.bone_scale(entity: u64, bone: &str, scale: [f32`: Scales a bone and what hangs on it, for this frame, the same way. All 0 hides it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L117)
- <a id="Client-action_current"></a>`.action_current(entity: u64) -> Option<String>`: The action `entity` is playing now (`World::action_play`), until its clip is over. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L122)
- <a id="Client-clip_length"></a>`.clip_length(entity: u64, name: &str) -> Option<f32>`: How long a clip of action `name` is on `entity`'s model, in seconds. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L128)
- <a id="Client-held"></a>`.held(entity: u64, hand: Hand) -> abi::ItemStack`: The stack in `entity`'s hand, including its public components; empty for none. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L134)
- <a id="Client-displayed"></a>`.displayed(entity: u64, source: DisplaySource) -> Vec<ItemStack>`: Public stacks from a display source, in slot order, including empty slots. Equipment, model visibility and attribute modifiers are independent. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L142)
- <a id="Client-bone_position"></a>`.bone_position(entity: u64, bone: &str) -> Option<Vec3>`: Where a bone of `entity`'s model or of what it has on was in the world the last frame: for sparks off a sword's tip, smoke over a lantern. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L155)
- <a id="Client-input_release_all"></a>`.input_release_all()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L160)
- <a id="Client-key"></a>`.key(key: &Key) -> abi::KeyState`: What the key (yours from `Registry::key`, or one of `keys::*`) does now: pressed this frame, held, for how many ticks. Keys belong to the world: while a screen has the input they are up. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L167)
- <a id="Client-key_pressed"></a>`.key_pressed(key: &Key) -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L173)
- <a id="Client-key_held"></a>`.key_held(key: &Key) -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L177)
- <a id="Client-control"></a>`.control(c: &abi::PlayerControl)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L181)
- <a id="Client-set_look"></a>`.set_look(yaw_deg: f32, pitch_deg: f32)`: Turns the local player, in degrees like the F3 screen. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L186)
- <a id="Client-teleport"></a>`.teleport(pos: impl Into<Vec3>)`: Puts the local player's feet at `pos` (the server takes it as a move). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L196)
- <a id="Client-set_velocity"></a>`.set_velocity(vel: impl Into<Vec3>)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L201)
- <a id="Client-set_flying"></a>`.set_flying(on: bool)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L206)
- <a id="Client-set_fly_speed"></a>`.set_fly_speed(mul: f32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L210)
- <a id="Client-select_slot"></a>`.select_slot(slot: u32)`: Hotbar slot 0..9. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L215)
- <a id="Client-say"></a>`.say(line: &str)`: A chat line or `/command`, as if the player typed and sent it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L224)
- <a id="Client-screenshot"></a>`.screenshot(path: &str) -> u64`: Captures the next frame to `path` (a bare name goes into `screenshots/`). The pixels are available a frame later through `image(id)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L230)
- <a id="Client-quit"></a>`.quit(code: i32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L234)
- <a id="Client-world_open"></a>`.world_open(name: &str, seed: u64)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L238)
- <a id="Client-world_create"></a>`.world_create(name: &str, seed: u64)`: Opens a fresh world, deleting a save of the same name first. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L243)
- <a id="Client-world_leave"></a>`.world_leave()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L247)
- <a id="Client-world_host"></a>`.world_host()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L251)
- <a id="Client-world_join"></a>`.world_join(ticket: &str)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L255)
- <a id="Client-menu_open"></a>`.menu_open(screen: &Menu) -> bool`: A mod's screen or one of the engine's (`screens::*`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L260)
- <a id="Client-menu_close"></a>`.menu_close()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L264)
- <a id="Client-menu_sort"></a>`.menu_sort(slot: Option<u32>) -> bool`: Sorts the open menu's storage: slot `slot`'s group, or with `None` what the menu is opened on (a chest), else the player's main inventory. Items of a kind come together, stacked, in the order they were registered. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L271)
- <a id="Client-reload_assets"></a>`.reload_assets()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L275)
- <a id="Client-bench"></a>`.bench(secs: f32, label: &str)`: Statistics over `secs`, logged as a `BENCH` line; numbers in `state().bench_*` once `state().benches` goes up. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L281)
- <a id="Client-respawn"></a>`.respawn()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L285)
- <a id="Client-ticket"></a>`.ticket() -> String` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L297)
- <a id="Client-run_dir"></a>`.run_dir() -> String` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L301)
- <a id="Client-chat_line"></a>`.chat_line(index: u32) -> String`: Chat line `index` back from the newest, empty past the end. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L306)
- <a id="Client-bench_line"></a>`.bench_line() -> String` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L310)
- <a id="Client-focused_text"></a>`.focused_text() -> String`: Text of the top screen's focused input field. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L315)
- <a id="Client-block"></a>`.block(pos: impl Into<BlockPos>) -> Option<u32>`: The block the client has at `pos`, `None` while that cube is not loaded. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L320)
- <a id="Client-body_block"></a>`.body_block(entity: u64, pos: impl Into<BlockPos>) -> Option<u32>`: The block the client has at `pos` of moving body `entity` (0: the world). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L325)
- <a id="Client-cube_shown"></a>`.cube_shown(pos: impl Into<BlockPos>) -> bool`: Whether the cube holding `pos` is here and has been drawn. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L332)
- <a id="Client-light"></a>`.light(pos: impl Into<BlockPos>) -> Option<u32>`: The light the client has at `pos` (sky level in the low byte, then block red, green, blue), `None` while that cube is not loaded. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L339)
- <a id="Client-menu_slot"></a>`.menu_slot(slot: u32) -> ItemStack` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L345)
- <a id="Client-cursor_stack"></a>`.cursor_stack() -> ItemStack` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L349)
- <a id="Client-menu"></a>`.menu() -> Option<abi::OpenMenu>`: The open menu. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L354)
- <a id="Client-entities"></a>`.entities() -> Vec<u64>`: Entities the client knows, the local player left out. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L360)
- <a id="Client-entity"></a>`.entity(id: u64) -> Option<abi::EntityState>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L371)
- <a id="Client-ui_rect"></a>`.ui_rect(query: &str) -> Option<abi::Rect>`: Where an element of the top screen is (fractions of the window): `slot:N`, `item:N` (creative list), `entity` (the entity figure), `scrollbar` (the first scroll bar's track), `class:name` (the first part of that theme class), `id:name` or `text:words`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L378)
- <a id="Client-ui_click"></a>`.ui_click(query: &str, button: u32) -> bool`: Moves the mouse to the middle of a UI element and clicks it with `button`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L384)
- <a id="Client-draw_screen_text"></a>`.draw_screen_text(x: f32, y: f32, key: &str, args: &[crate::Arg], color: u32, size: f32)`: The translation `key` with `args`, centered at fractions of the window, `size` times as big as menu text. Part of the GUI: F1 hides it with the rest. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L393)
- <a id="Client-send"></a>`.send<T: Payload>(channel: &Channel<T>, value: &T)`: Sends `value` on `channel` to the server. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L399)
- <a id="Client-run_command"></a>`.run_command(line: &str) -> u32`: Runs a command as this player (`/tp ~ ~5 ~`): a client command here, any other on the server. The answer goes to `command_reply(id)`, not the chat. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L406)
- <a id="Client-command_reply"></a>`.command_reply(id: u32) -> Option<String>`: The answer of `run_command`'s `id` once it came, its lines in the client's language; read once. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L412)

## ClientEntity

One entity in `Registry::client_entity_frame`.

- <a id="ClientEntity-id"></a>`.id() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L782)
- <a id="ClientEntity-kind"></a>`.kind() -> u32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L785)
- <a id="ClientEntity-pos"></a>`.pos() -> Vec3` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L788)
- <a id="ClientEntity-yaw"></a>`.yaw() -> f32` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L791)
- <a id="ClientEntity-walked"></a>`.walked() -> f32`: Distance walked so far; steps go by how it grows. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L795)
- <a id="ClientEntity-dying"></a>`.dying() -> Option<f32>`: Seconds into its death animation (1: over, the last call), `None` while alive. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L799)
- <a id="ClientEntity-age"></a>`.age() -> f32`: Seconds since this client first saw it. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L803)
- <a id="ClientEntity-dt"></a>`.dt() -> f32`: Seconds since the last call. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L807)

## Drawing

Gizmo shapes, drawn when this is dropped (`Output::draw_line` ..).

- <a id="Drawing-to"></a>`.to(who: impl Into<Who>)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/out.rs#L157)
- <a id="Drawing-ticks"></a>`.ticks(n: u32)`: Ticks the server's stay; a client draws for the frame. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/out.rs#L163)

## KeyBuilder

- <a id="KeyBuilder-on_client"></a>`.on_client(f: impl Fn(&Client) + Send + Sync + 'static)`: On the client, every time the key goes down. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L643)
- <a id="KeyBuilder-on_server"></a>`.on_server(f: impl Fn(&crate::World, u64) + Send + Sync + 'static)`: On the server, for the player who pressed it (world, player): `crate::KEY` filtered to this key going down. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L660)
- <a id="KeyBuilder-on_release_server"></a>`.on_release_server(f: impl Fn(&crate::World, u64, u32) + Send + Sync + 'static)`: On the server, when the player lets it go (world, player, ticks it was held): a charge. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L671)

## Keys

The keys as a mount's steering or a camera mode reads them, any action by its handle, and where the player looks (radians; a detached camera's own look).

- <a id="Keys-state"></a>`.state(key: &Key) -> abi::KeyState`: Pressed, held and its charge: ticks held, and held before being let go this tick. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L697)
- <a id="Keys-held"></a>`.held(key: &Key) -> bool` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L703)
- <a id="Keys-axis"></a>`.axis(plus: &Key, minus: &Key) -> f32`: -1..1 from `minus` held to `plus` held: `axis(&keys::FORWARD, &keys::BACK)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L708)

## Packet

A message on a channel, on the server.

- <a id="Packet-world"></a>`.world() -> World` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L553)
- <a id="Packet-player"></a>`.player() -> u64` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L556)
- <a id="Packet-reply"></a>`.reply<R: Payload>(channel: &Channel<R>, value: &R)`: Answers the sender on `channel`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/client.rs#L560)

## Shot

How a camera moves: called each frame on the client with the keys, the player's eyes and last frame's camera (`CameraArgs`), it sets `out`. It brings the tags it needs along (`comp::camera`: a free camera is `DETACHED`), `CameraBuilder::shot` adds them.

- <a id="Shot-new"></a>`.new(tags: &'static [&'static str], f: impl Fn(&mut CameraArgs) + Send + Sync + 'static) -> Shot`: Yours: `|a| a.out.pos = ..`, with these tags (`comp::camera`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/camera.rs#L45)

## Show

Text on its way to the screen: it goes when this is dropped, at the end of the statement.

- <a id="Show-to"></a>`.to(who: impl Into<Who>)`: Who sees it, of the players on the server (everyone in the dimension unless said); a client shows it only if this takes it in. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L92)
- <a id="Show-id"></a>`.id(id: &'a str)`: New text with the same place and id replaces this one instead of adding another: a timer counting in one chat line, several free texts side by side. Empty key and no args takes it away. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L100)
- <a id="Show-at"></a>`.at(x: f32, y: f32)`: This point of the text at the same point of the window, in fractions of either: (0, 0) the top left corner of both, (0.5, 0.5) centered, (1, 1) the bottom right corner of both. It stands by itself then, whatever its place. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L108)
- <a id="Show-scale"></a>`.scale(scale: f32)`: How big, 1 being menu text. A title is 4, a subtitle 2. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L114)
- <a id="Show-times"></a>`.times(fade_in: f32, stay: f32, fade_out: f32)`: Seconds of fading in, staying and fading out. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L120)
- <a id="Show-color"></a>`.color(rgb: u32)`: `0xRRGGBB`, for what doesn't color itself. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L126)

## TextPlaceBuilder

A place for text a mod adds (`Registry::text_place`): an element of the HUD the player can move and hide, and how text there looks unless `show` says otherwise.

- <a id="TextPlaceBuilder-hud"></a>`.hud(anchor: &str, order: i32)`: Where the HUD puts it by default: an anchor (`hud_anchor::TOP_RIGHT`) and its order there. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L44)
- <a id="TextPlaceBuilder-at"></a>`.at(x: f32, y: f32, dx: f32, dy: f32)`: A spot of its own by default instead: this point of it at the same point of the window, moved by `dx`, `dy` GUI pixels. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L50)
- <a id="TextPlaceBuilder-scale"></a>`.scale(scale: f32)`: How big, 1 being menu text. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L55)
- <a id="TextPlaceBuilder-times"></a>`.times(fade_in: f32, stay: f32, fade_out: f32)`: Seconds of fading in, staying and fading out, by default. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L59)
- <a id="TextPlaceBuilder-stacks"></a>`.stacks()`: Several texts at once, one under another; by default a new one replaces what is there. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L63)
- <a id="TextPlaceBuilder-class"></a>`.class(class: &str)`: Theme class of its text (`label` by default). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L67)
- <a id="TextPlaceBuilder-sample"></a>`.sample(key: &str)`: The translation the HUD editor shows while the place is empty. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/show.rs#L71)
