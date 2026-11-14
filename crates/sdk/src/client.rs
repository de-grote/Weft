//! Client side for mods: frame and screen hooks that see the local game and may drive it
//! (simulated input, the local player, worlds, screens, screenshots), mod key bindings, and
//! message channels between a mod's client and server parts.
//!
//! ```ignore
//! static JETPACK: Key = Key::named("jetpack");
//! r.key(&JETPACK, "J").on_server(|w, player| { /* on the server, for that player */ });
//! // Any action on the server, the mouse buttons too, with how long it was held:
//! r.on_event(&KEY, |b| for k in b.iter() { /* k.player, k.key, k.down, k.held_ticks */ });
//! static SCAN: Key = Key::named("scan");
//! r.key(&SCAN, "K").on_client(|c| { /* on this client */ });
//! ```

use crate::abi::{self, ItemStack, Slice, Str};
use crate::abi::{BlockPos, Vec3};
use crate::{ClientView, Key, Menu, Registry, Value, WithComponents, World, callback, host};

pub use abi::{ClientState, client_flags};

/// The local client during a client callback. Only valid for the duration of that callback.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Client {
    raw: *mut abi::ClientCtx,
}

/// All a binding can read, a client can.
impl std::ops::Deref for Client {
    type Target = ClientView;
    fn deref(&self) -> &ClientView {
        // Both are the one pointer.
        unsafe { &*(self as *const Client as *const ClientView) }
    }
}

fn abi_str(x: &str) -> Str {
    Str::new(x)
}

impl Client {
    pub(crate) fn from_raw(raw: *mut abi::ClientCtx) -> Client {
        Client { raw }
    }

    pub(crate) fn raw(&self) -> *mut abi::ClientCtx {
        self.raw
    }

    /// The read-only view UI bindings get (own player, components, menu data).
    pub fn view(&self) -> ClientView {
        ClientView::from_raw(self.raw)
    }

    pub fn state(&self) -> ClientState {
        let mut st = ClientState::DEFAULT;
        unsafe { (host().client_state)(self.raw, &mut st) };
        st
    }

    /// Top screen name (`core:menu`, `core:pause`, ...), empty in the world.
    pub fn screen(&self) -> String {
        unsafe { self.state().screen.as_str().to_string() }
    }

    fn input(&self, op: u32, text: &str, x: f32, y: f32) -> bool {
        unsafe { (host().client_input)(self.raw, op, abi_str(text), x, y) != 0 }
    }

    /// Presses the key bound to `key` (yours from `Registry::key`, or one of `keys::*`) for
    /// `secs` (0 = a tap). False for a key nobody registered.
    pub fn input_action(&self, key: &Key, secs: f32) -> bool {
        self.input(abi::client_input::ACTION, key.name(), secs, 0.0)
    }

    /// A key by its settings name (`Enter`, `Tab`, `KeyE`, `F3`).
    pub fn input_key(&self, key: &str, secs: f32) -> bool {
        self.input(abi::client_input::KEY, key, secs, 0.0)
    }

    /// Mouse button 0 left, 1 right, 2 middle.
    pub fn input_click(&self, button: u32, secs: f32) {
        self.input(abi::client_input::CLICK, "", secs, button as f32);
    }

    /// Moves the mouse to fractions of the window.
    pub fn input_cursor(&self, x: f32, y: f32) {
        self.input(abi::client_input::CURSOR, "", x, y);
    }

    pub fn input_scroll(&self, steps: f32) {
        self.input(abi::client_input::SCROLL, "", steps, 0.0);
    }

    /// Text typed into the focused field.
    pub fn input_type(&self, text: &str) {
        self.input(abi::client_input::TYPE, text, 0.0, 0.0);
    }

    fn bone_pose(&self, entity: u64, bone: &str, turn: [f32; 3], by: [f32; 3], scale: [f32; 3]) {
        unsafe { (host().client_pose_bone)(self.raw, entity, abi_str(bone), &turn, &by, &scale) }
    }

    /// Turns a bone of `entity`'s model or of what it has on, for this frame, on top of its clips:
    /// degrees about x, y, z, the way Blockbench counts them. From `client_frame` or
    /// `client_entity_frame`, every frame it should last.
    pub fn bone_turn(&self, entity: u64, bone: &str, degrees: [f32; 3]) {
        let [x, y, z] = degrees.map(f32::to_radians);
        self.bone_pose(entity, bone, [-x, y, -z], [0.0; 3], [1.0; 3]);
    }

    /// Moves a bone this far in pixels, for this frame, the same way.
    pub fn bone_move(&self, entity: u64, bone: &str, pixels: [f32; 3]) {
        self.bone_pose(entity, bone, [0.0; 3], pixels, [1.0; 3]);
    }

    /// Scales a bone and what hangs on it, for this frame, the same way. All 0 hides it.
    pub fn bone_scale(&self, entity: u64, bone: &str, scale: [f32; 3]) {
        self.bone_pose(entity, bone, [0.0; 3], [0.0; 3], scale);
    }

    /// The action `entity` is playing now (`World::action_play`), until its clip is over.
    pub fn action_current(&self, entity: u64) -> Option<String> {
        let mut out = Str::new("");
        (unsafe { (host().client_action_current)(self.raw, entity, &mut out) } != 0).then(|| unsafe { out.as_str().to_string() })
    }

    /// How long a clip of action `name` is on `entity`'s model, in seconds.
    pub fn clip_length(&self, entity: u64, name: &str) -> Option<f32> {
        let secs = unsafe { (host().client_clip_length)(self.raw, entity, abi_str(name)) };
        (secs >= 0.0).then_some(secs)
    }

    /// The stack in `entity`'s hand, including its public components; empty for none.
    pub fn held(&self, entity: u64, hand: Hand) -> abi::ItemStack {
        let mut hands = [ItemStack::EMPTY; 2];
        unsafe { (host().client_displayed)(self.raw, entity, abi::display_source::HANDS, hands.as_mut_ptr(), hands.len()) };
        hands[hand as usize]
    }

    /// Public stacks from a display source, in slot order, including empty slots.
    /// Equipment, model visibility and attribute modifiers are independent.
    pub fn displayed(&self, entity: u64, source: DisplaySource) -> Vec<ItemStack> {
        let mut stacks = vec![ItemStack::EMPTY; 8];
        let n = unsafe { (host().client_displayed)(self.raw, entity, source as u32, stacks.as_mut_ptr(), stacks.len()) };
        if n > stacks.len() {
            stacks.resize(n, ItemStack::EMPTY);
            unsafe { (host().client_displayed)(self.raw, entity, source as u32, stacks.as_mut_ptr(), stacks.len()) };
        }
        stacks.truncate(n);
        stacks
    }

    /// Where a bone of `entity`'s model or of what it has on was in the world the last frame: for
    /// sparks off a sword's tip, smoke over a lantern.
    pub fn bone_position(&self, entity: u64, bone: &str) -> Option<Vec3> {
        let mut out = Vec3::ZERO;
        (unsafe { (host().client_bone_position)(self.raw, entity, abi_str(bone), &mut out) } != 0).then_some(out)
    }

    pub fn input_release_all(&self) {
        self.input(abi::client_input::RELEASE_ALL, "", 0.0, 0.0);
    }

    /// What the key (yours from `Registry::key`, or one of `keys::*`) does now: pressed this
    /// frame, held, for how many ticks. Keys belong to the world: while a screen has the input
    /// they are up.
    pub fn key(&self, key: &Key) -> abi::KeyState {
        let mut out = abi::KeyState::default();
        unsafe { (host().client_key_state)(self.raw, abi_str(key.name()), &mut out) };
        out
    }

    pub fn key_pressed(&self, key: &Key) -> bool {
        self.key(key).pressed != 0
    }

    pub fn key_held(&self, key: &Key) -> bool {
        self.key(key).held != 0
    }

    pub fn control(&self, c: &abi::PlayerControl) {
        unsafe { (host().client_control)(self.raw, c) }
    }

    /// Turns the local player, in degrees like the F3 screen.
    pub fn set_look(&self, yaw_deg: f32, pitch_deg: f32) {
        self.control(&abi::PlayerControl {
            mask: abi::control_mask::LOOK,
            yaw: yaw_deg.to_radians(),
            pitch: pitch_deg.to_radians(),
            ..abi::PlayerControl::DEFAULT
        });
    }

    /// Puts the local player's feet at `pos` (the server takes it as a move).
    pub fn teleport(&self, pos: impl Into<Vec3>) {
        let pos = pos.into();
        self.control(&abi::PlayerControl { mask: abi::control_mask::POS | abi::control_mask::VEL, pos, ..abi::PlayerControl::DEFAULT });
    }

    pub fn set_velocity(&self, vel: impl Into<Vec3>) {
        let vel = vel.into();
        self.control(&abi::PlayerControl { mask: abi::control_mask::VEL, vel, ..abi::PlayerControl::DEFAULT });
    }

    pub fn set_flying(&self, on: bool) {
        self.control(&abi::PlayerControl { mask: abi::control_mask::FLYING, flying: on as u32, ..abi::PlayerControl::DEFAULT });
    }

    pub fn set_fly_speed(&self, mul: f32) {
        self.control(&abi::PlayerControl { mask: abi::control_mask::FLY_SPEED, fly_speed: mul, ..abi::PlayerControl::DEFAULT });
    }

    /// Hotbar slot 0..9.
    pub fn select_slot(&self, slot: u32) {
        self.control(&abi::PlayerControl { mask: abi::control_mask::SELECTED, selected: slot, ..abi::PlayerControl::DEFAULT });
    }

    fn op(&self, op: u32, text: &str, num: f64) -> u64 {
        unsafe { (host().client_op)(self.raw, op, abi_str(text), num) }
    }

    /// A chat line or `/command`, as if the player typed and sent it.
    pub fn say(&self, line: &str) {
        self.op(abi::client_op::CHAT, line, 0.0);
    }

    /// Captures the next frame to `path` (a bare name goes into `screenshots/`). The pixels are
    /// available a frame later through `image(id)`.
    pub fn screenshot(&self, path: &str) -> u64 {
        self.op(abi::client_op::SCREENSHOT, path, 0.0)
    }

    pub fn quit(&self, code: i32) {
        self.op(abi::client_op::QUIT, "", code as f64);
    }

    pub fn world_open(&self, name: &str, seed: u64) {
        self.op(abi::client_op::OPEN_WORLD, name, seed as f64);
    }

    /// Opens a fresh world, deleting a save of the same name first.
    pub fn world_create(&self, name: &str, seed: u64) {
        self.op(abi::client_op::NEW_WORLD, name, seed as f64);
    }

    pub fn world_leave(&self) {
        self.op(abi::client_op::LEAVE_WORLD, "", 0.0);
    }

    pub fn world_host(&self) {
        self.op(abi::client_op::HOST, "", 0.0);
    }

    pub fn world_join(&self, ticket: &str) {
        self.op(abi::client_op::JOIN, ticket, 0.0);
    }

    /// A mod's screen or one of the engine's (`screens::*`).
    pub fn menu_open(&self, screen: &Menu) -> bool {
        self.op(abi::client_op::OPEN_SCREEN, screen.name(), 0.0) != 0
    }

    pub fn menu_close(&self) {
        self.op(abi::client_op::CLOSE_SCREEN, "", 0.0);
    }

    /// Sorts the open menu's storage: slot `slot`'s group, or with `None` what the menu is
    /// opened on (a chest), else the player's main inventory. Items of a kind come together,
    /// stacked, in the order they were registered.
    pub fn menu_sort(&self, slot: Option<u32>) -> bool {
        self.op(abi::client_op::MENU_SORT, "", slot.map_or(-1.0, |s| s as f64)) != 0
    }

    pub fn reload_assets(&self) {
        self.op(abi::client_op::RELOAD_ASSETS, "", 0.0);
    }

    /// Statistics over `secs`, logged as a `BENCH` line; numbers in `state().bench_*` once
    /// `state().benches` goes up.
    pub fn bench(&self, secs: f32, label: &str) {
        self.op(abi::client_op::BENCH, label, secs as f64);
    }

    pub fn respawn(&self) {
        self.op(abi::client_op::RESPAWN, "", 0.0);
    }

    fn text(&self, which: u32, index: u32) -> String {
        unsafe {
            (host().client_text)(self.raw, which, index)
                .as_str()
                .to_string()
        }
    }

    pub fn ticket(&self) -> String {
        self.text(abi::client_text::TICKET, 0)
    }

    pub fn run_dir(&self) -> String {
        self.text(abi::client_text::RUN_DIR, 0)
    }

    /// Chat line `index` back from the newest, empty past the end.
    pub fn chat_line(&self, index: u32) -> String {
        self.text(abi::client_text::CHAT, index)
    }

    pub fn bench_line(&self) -> String {
        self.text(abi::client_text::BENCH, 0)
    }

    /// Text of the top screen's focused input field.
    pub fn focused_text(&self) -> String {
        self.text(abi::client_text::INPUT, 0)
    }

    /// The block the client has at `pos`, `None` while that cube is not loaded.
    pub fn block(&self, pos: impl Into<BlockPos>) -> Option<u32> {
        self.body_block(0, pos)
    }

    /// The block the client has at `pos` of moving body `entity` (0: the world).
    pub fn body_block(&self, entity: u64, pos: impl Into<BlockPos>) -> Option<u32> {
        let pos = pos.into();
        let b = unsafe { (host().client_block_in)(self.raw, entity, &pos) };
        (b != u32::MAX).then_some(b)
    }

    /// Whether the cube holding `pos` is here and has been drawn.
    pub fn cube_shown(&self, pos: impl Into<BlockPos>) -> bool {
        let pos = pos.into();
        unsafe { (host().client_cube_shown)(self.raw, &pos) != 0 }
    }

    /// The light the client has at `pos` (sky level in the low byte, then block red, green,
    /// blue), `None` while that cube is not loaded.
    pub fn light(&self, pos: impl Into<BlockPos>) -> Option<u32> {
        let pos = pos.into();
        let l = unsafe { (host().client_light)(self.raw, &pos) };
        (l != u32::MAX).then_some(l)
    }

    pub fn menu_slot(&self, slot: u32) -> ItemStack {
        unsafe { (host().client_slot)(self.raw, abi::slot_container::MENU, slot) }
    }

    pub fn cursor_stack(&self) -> ItemStack {
        unsafe { (host().client_slot)(self.raw, abi::slot_container::CURSOR, 0) }
    }

    /// The open menu.
    pub fn menu(&self) -> Option<abi::OpenMenu> {
        let mut out = abi::OpenMenu::default();
        (unsafe { (host().client_menu)(self.raw, &mut out) } != 0).then_some(out)
    }

    /// Entities the client knows, the local player left out.
    pub fn entities(&self) -> Vec<u64> {
        let mut buf = vec![0u64; 256];
        let n = unsafe { (host().client_entities)(self.raw, buf.as_mut_ptr(), buf.len()) };
        if n > buf.len() {
            buf.resize(n, 0);
            unsafe { (host().client_entities)(self.raw, buf.as_mut_ptr(), buf.len()) };
        }
        buf.truncate(n);
        buf
    }

    pub fn entity(&self, id: u64) -> Option<abi::EntityState> {
        let mut st = abi::EntityState::default();
        (unsafe { (host().client_entity)(self.raw, id, &mut st) } != 0).then_some(st)
    }

    /// Where an element of the top screen is (fractions of the window):
    /// `slot:N`, `item:N` (creative list), `entity` (the entity figure), `scrollbar` (the first scroll bar's track), `class:name` (the first part of that theme class), `id:name` or `text:words`.
    pub fn ui_rect(&self, query: &str) -> Option<abi::Rect> {
        let mut r = abi::Rect::default();
        (unsafe { (host().client_ui_rect)(self.raw, abi_str(query), &mut r) } != 0).then_some(r)
    }

    /// Moves the mouse to the middle of a UI element and clicks it with `button`.
    pub fn ui_click(&self, query: &str, button: u32) -> bool {
        let Some(r) = self.ui_rect(query) else { return false };
        self.input_cursor(r.x + r.w / 2.0, r.y + r.h / 2.0);
        self.input_click(button, 0.0);
        true
    }

    /// The translation `key` with `args`, centered at fractions of the window, `size` times as
    /// big as menu text. Part of the GUI: F1 hides it with the rest.
    pub fn draw_screen_text(&self, x: f32, y: f32, key: &str, args: &[crate::Arg], color: u32, size: f32) {
        let raw = crate::ui::raw_args(args);
        unsafe { (host().client_screen_text)(self.raw, x, y, size, color, abi_str(key), Slice::new(&raw)) }
    }

    /// Sends `value` on `channel` to the server.
    pub fn send<T: Payload>(&self, channel: &Channel<T>, value: &T) {
        let data = value.encode();
        unsafe { (host().client_send_packet)(self.raw, abi_str(channel.name()), abi_str(&T::schema()), Slice::new(&data)) }
    }

    /// Runs a command as this player (`/tp ~ ~5 ~`): a client command here, any other on the
    /// server. The answer goes to `command_reply(id)`, not the chat.
    pub fn run_command(&self, line: &str) -> u32 {
        self.op(abi::client_op::RUN_COMMAND, line, 0.0) as u32
    }

    /// The answer of `run_command`'s `id` once it came, its lines in the client's language; read
    /// once.
    pub fn command_reply(&self, id: u32) -> Option<String> {
        let mut out = Str::EMPTY;
        let got = unsafe { (host().client_command_reply)(self.raw, id, &mut out) } != 0;
        got.then(|| unsafe { out.as_str() }.to_string())
    }
}

/// Where an entity displays stacks. A source does not imply a model or attribute modifiers.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplaySource {
    Hands = abi::display_source::HANDS,
    Equipment = abi::display_source::EQUIPMENT,
    Shown = abi::display_source::SHOWN,
    Ground = abi::display_source::GROUND,
}

/// Which hand.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hand {
    Main = abi::hand::MAIN,
    Off = abi::hand::OFF,
}

/// What a channel carries: a `value!` struct or number (checked by its layout like events),
/// `String`, `Vec<u8>` or `()`.
pub trait Payload: Sized + 'static {
    fn schema() -> String;
    fn encode(&self) -> Vec<u8>;
    fn decode(bytes: &[u8]) -> Option<Self>;
}

impl<T: Value> Payload for T {
    fn schema() -> String {
        T::schema()
    }
    fn encode(&self) -> Vec<u8> {
        unsafe { std::slice::from_raw_parts(self as *const T as *const u8, size_of::<T>()) }.to_vec()
    }
    fn decode(bytes: &[u8]) -> Option<T> {
        (bytes.len() == size_of::<T>()).then(|| unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const T) })
    }
}

impl Payload for String {
    fn schema() -> String {
        "text".into()
    }
    fn encode(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Option<String> {
        String::from_utf8(bytes.to_vec()).ok()
    }
}

impl Payload for Vec<u8> {
    fn schema() -> String {
        "bytes".into()
    }
    fn encode(&self) -> Vec<u8> {
        self.clone()
    }
    fn decode(bytes: &[u8]) -> Option<Vec<u8>> {
        Some(bytes.to_vec())
    }
}

impl Payload for () {
    fn schema() -> String {
        "unit".into()
    }
    fn encode(&self) -> Vec<u8> {
        Vec::new()
    }
    fn decode(_: &[u8]) -> Option<()> {
        Some(())
    }
}

/// A message channel between a mod's client and server halves, carrying `T`; both ends declare
/// the same type, which the engine checks like event values:
///
/// ```ignore
/// value! { pub struct Boost { pub power: f32 } }
/// static BOOST: Channel<Boost> = Channel::named("boost");
/// r.packet(&BOOST, |p, b| { /* p.player(), b.power */ });   // server
/// client.send(&BOOST, &Boost { power: 2.0 });               // client
/// ```
pub struct Channel<T: Payload> {
    name: &'static str,
    full: std::sync::OnceLock<String>,
    _p: std::marker::PhantomData<fn() -> T>,
}

impl<T: Payload> Channel<T> {
    pub const fn named(name: &'static str) -> Self {
        Channel { name, full: std::sync::OnceLock::new(), _p: std::marker::PhantomData }
    }
    /// For names made at runtime; lives as long as the game.
    pub fn generated(name: String) -> &'static Self {
        Box::leak(Box::new(Channel::named(Box::leak(name.into_boxed_str()))))
    }
    pub fn name(&self) -> &str {
        self.full.get_or_init(|| crate::full_name(self.name))
    }
}

/// RGBA pixels of a screenshot or loaded image: (width, height, pixels).
pub fn image_pixels(id: u64) -> Option<(u32, u32, &'static [u8])> {
    let (mut w, mut h) = (0, 0);
    let p = unsafe { (host().image_pixels)(id, &mut w, &mut h) };
    (!p.is_null()).then(|| (w, h, unsafe { std::slice::from_raw_parts(p, (w * h * 4) as usize) }))
}

pub fn image_load(path: &str) -> Option<u64> {
    let id = unsafe { (host().image_load)(abi_str(path)) };
    (id != 0).then_some(id)
}

pub fn image_save(path: &str, w: u32, h: u32, rgba: &[u8]) -> bool {
    unsafe { (host().image_save)(abi_str(path), Slice::new(rgba), w, h) != 0 }
}

pub fn image_unload(id: u64) {
    unsafe { (host().image_unload)(id) }
}

/// `--name value` from the command line.
pub fn launch_option(name: &str) -> Option<String> {
    let v = unsafe { (host().launch_option)(abi_str(name)).as_str() };
    (!v.is_empty()).then(|| v.to_string())
}

/// A message on a channel, on the server.
pub struct Packet {
    args: abi::PacketArgs,
}

impl Packet {
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn player(&self) -> u64 {
        self.args.player
    }
    /// Answers the sender on `channel`.
    pub fn reply<R: Payload>(&self, channel: &Channel<R>, value: &R) {
        self.world().send(self.args.player, channel, value);
    }
}

impl World {
    /// Sends `value` on `channel` to the clients of `who` (a player's id for one).
    pub fn send<T: Payload>(&self, who: impl Into<crate::Who>, channel: &Channel<T>, value: &T) {
        let data = value.encode();
        unsafe { (host().send_packet)(self.raw(), who.into(), abi_str(channel.name()), abi_str(&T::schema()), Slice::new(&data)) }
    }
}

/// A client callback for one entity (`ClientEntityArgs`).
fn entity_callback(f: impl Fn(&Client, &ClientEntity) + Send + Sync + 'static) -> abi::Callback {
    callback(move |a: &mut abi::ClientEntityArgs| {
        f(&Client::from_raw(a.client), unsafe { &*(a as *const abi::ClientEntityArgs as *const ClientEntity) })
    })
}

impl Registry {
    /// Every frame on the client, before input is handled.
    pub fn client_frame(&mut self, f: impl Fn(&Client) + Send + Sync + 'static) {
        let cb = callback(move |a: &mut abi::ClientFrameArgs| f(&Client::from_raw(a.client)));
        unsafe { (host().reg_client_frame)(self.raw, cb) }
    }

    /// Every frame on the client for each entity of `kind`, the local player too: its effects
    /// (wing flaps, sparks, purring), any number of mods on one kind. The engine's steps, idle
    /// noises and death smoke go on unless the kind says otherwise (`.effect(..)`, `.own_effects()`).
    pub fn client_entity_frame(&mut self, kind: &crate::EntityKind, f: impl Fn(&Client, &ClientEntity) + Send + Sync + 'static) {
        drop(
            self.edit_entity(kind.name())
                .push(&crate::comps::entity::FRAMES, entity_callback(f)),
        );
    }

    /// Server side of a channel: `f` gets every value a client sends on it, with who sent it.
    pub fn packet<T: Payload>(&mut self, channel: &Channel<T>, f: impl Fn(&Packet, T) + Send + Sync + 'static) {
        let name = channel.name();
        let cb = callback(move |a: &mut abi::PacketArgs| {
            if let Some(v) = T::decode(unsafe { a.data.as_slice() }) {
                f(unsafe { &*(a as *const abi::PacketArgs as *const Packet) }, v)
            }
        });
        unsafe { (host().reg_packet)(self.raw, abi_str(name), abi_str(&T::schema()), 0, cb) }
    }

    /// Client side of `channel`: called when the server sends on it.
    pub fn client_packet<T: Payload>(&mut self, channel: &Channel<T>, f: impl Fn(&Client, T) + Send + Sync + 'static) {
        let name = channel.name();
        let cb = callback(move |a: &mut abi::ClientPacketArgs| {
            if let Some(v) = T::decode(unsafe { a.data.as_slice() }) {
                f(&Client::from_raw(a.client), v)
            }
        });
        unsafe { (host().reg_packet)(self.raw, abi_str(name), abi_str(&T::schema()), 1, cb) }
    }

    /// Key binding with a default key (`J`, `KeyJ`, `F6`, `space`, `lshift`, `mouse3`), listed
    /// in Controls as `key.<ns>.<name>`; what it does with `.on_client(..)` or
    /// `.on_server(..)`, or read with `Client::key_pressed`/`key_held`. Keys only act while the
    /// world has the input (not while typing in the chat or with a screen open).
    pub fn key<'a>(&'a mut self, key: &'static Key, default: &str) -> KeyBuilder<'a> {
        self.key_entry(key)
            .set_text(&crate::comps::key::DEFAULT, default)
    }

    fn key_entry(&mut self, key: &'static Key) -> KeyBuilder<'_> {
        KeyBuilder { reg: self, e: crate::entry::Entry::of(key), key }
    }
}

pub struct KeyBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
    key: &'static Key,
}

crate::entry::entry_builder!(Key => KeyBuilder, key_entry;);

impl KeyBuilder<'_> {
    /// On the client, every time the key goes down.
    pub fn on_client(self, f: impl Fn(&Client) + Send + Sync + 'static) -> Self {
        let key = self.key;
        // By the edge of `held`, so a press is seen whichever frame hook made it.
        let was = std::sync::atomic::AtomicBool::new(false);
        self.reg.client_frame(move |c| {
            let now = c.key_held(key);
            if now && !was.swap(now, std::sync::atomic::Ordering::Relaxed) {
                f(c)
            } else if !now {
                was.store(false, std::sync::atomic::Ordering::Relaxed);
            }
        });
        self
    }

    /// On the server, for the player who pressed it (world, player): `crate::KEY` filtered to
    /// this key going down.
    pub fn on_server(self, f: impl Fn(&crate::World, u64) + Send + Sync + 'static) -> Self {
        let key = self.key;
        self.reg.on_event(&crate::KEY, move |b| {
            let w = b.world();
            for k in b.iter().filter(|k| k.down != 0 && k.key == key.id()) {
                f(&w, k.player);
            }
        });
        self
    }
    /// On the server, when the player lets it go (world, player, ticks it was held): a charge.
    pub fn on_release_server(self, f: impl Fn(&crate::World, u64, u32) + Send + Sync + 'static) -> Self {
        let key = self.key;
        self.reg.on_event(&crate::KEY, move |b| {
            let w = b.world();
            for k in b.iter().filter(|k| k.down == 0 && k.key == key.id()) {
                f(&w, k.player, k.held_ticks);
            }
        });
        self
    }
}

/// The keys as a mount's steering or a camera mode reads them, any action by its handle, and
/// where the player looks (radians; a detached camera's own look).
pub struct Keys {
    raw: *const abi::KeysCtx,
    pub yaw: f32,
    pub pitch: f32,
}

impl Keys {
    pub(crate) fn of(raw: *const abi::KeysCtx, yaw: f32, pitch: f32) -> Keys {
        Keys { raw, yaw, pitch }
    }

    /// Pressed, held and its charge: ticks held, and held before being let go this tick.
    pub fn state(&self, key: &Key) -> abi::KeyState {
        let mut out = abi::KeyState::default();
        unsafe { (host().keys_state)(self.raw, abi_str(key.name()), &mut out) };
        out
    }

    pub fn held(&self, key: &Key) -> bool {
        self.state(key).held != 0
    }

    /// -1..1 from `minus` held to `plus` held: `axis(&keys::FORWARD, &keys::BACK)`.
    pub fn axis(&self, plus: &Key, minus: &Key) -> f32 {
        self.held(plus) as i32 as f32 - self.held(minus) as i32 as f32
    }
}

/// The engine's key bindings.
pub mod keys {
    use crate::Key;
    pub static FORWARD: Key = Key::named("core:forward");
    pub static BACK: Key = Key::named("core:back");
    pub static LEFT: Key = Key::named("core:left");
    pub static RIGHT: Key = Key::named("core:right");
    pub static JUMP: Key = Key::named("core:jump");
    pub static SNEAK: Key = Key::named("core:sneak");
    pub static SPRINT: Key = Key::named("core:sprint");
    pub static CRAWL: Key = Key::named("core:crawl");
    pub static FLY: Key = Key::named("core:fly");
    pub static INVENTORY: Key = Key::named("core:inventory");
    pub static DROP: Key = Key::named("core:drop");
    pub static DROP_ALL: Key = Key::named("core:drop_all");
    pub static DISMOUNT: Key = Key::named("core:dismount");
    pub static FLY_FASTER: Key = Key::named("core:fly_faster");
    pub static FLY_SLOWER: Key = Key::named("core:fly_slower");
    pub static HOTBAR_NEXT: Key = Key::named("core:hotbar_next");
    pub static HOTBAR_PREV: Key = Key::named("core:hotbar_prev");
    pub static COMMAND: Key = Key::named("core:command");
    pub static CHAT: Key = Key::named("core:chat");
    pub static DEBUG: Key = Key::named("core:debug");
    pub static SCREENSHOT: Key = Key::named("core:screenshot");
    pub static PERSPECTIVE: Key = Key::named("core:perspective");
    pub static HIDE_HUD: Key = Key::named("core:hide_hud");
    pub static SWAP_HANDS: Key = Key::named("core:swap_hands");
    pub static RELOAD: Key = Key::named("core:reload");
    pub static RECIPES: Key = Key::named("core:recipes");
    pub static USES: Key = Key::named("core:uses");
    pub static ATTACK: Key = Key::named("core:attack");
    pub static USE: Key = Key::named("core:use");
    pub static PICK: Key = Key::named("core:pick");
}

/// The engine's screens: open them, add parts to them, or register one of these handles to
/// replace it.
pub mod screens {
    use crate::Menu;
    pub static TITLE: Menu = Menu::named("core:title");
    pub static PAUSE: Menu = Menu::named("core:pause");
    pub static SETTINGS: Menu = Menu::named("core:settings");
    pub static LANGUAGE: Menu = Menu::named("core:language");
    pub static CONTROLS: Menu = Menu::named("core:controls");
    pub static WORLDS: Menu = Menu::named("core:worlds");
    pub static CREATE_WORLD: Menu = Menu::named("core:create_world");
    pub static WORLD_RULES: Menu = Menu::named("core:world_rules");
    pub static CHAT: Menu = Menu::named("core:chat");
    pub static INVENTORY: Menu = Menu::named("core:inventory");
    pub static MOD_SETTINGS: Menu = Menu::named("core:mod_settings");
    pub static DEATH: Menu = Menu::named("core:death");
    pub static MODS: Menu = Menu::named("core:mods");
    pub static PROFILE: Menu = Menu::named("core:profile");
    pub static MULTIPLAYER: Menu = Menu::named("core:multiplayer");
    pub static RESOURCE_PACKS: Menu = Menu::named("core:resource_packs");
    pub static SHADER_PACKS: Menu = Menu::named("core:shader_packs");
    pub static SOUND: Menu = Menu::named("core:sound");
    pub static HUD_EDITOR: Menu = Menu::named("core:hud_editor");
    pub static DEBUG_OPTIONS: Menu = Menu::named("core:debug_options");
    /// Every menu's screen, for `Registry::screen_part`: its place `side` is beside any menu,
    /// `corner` in the top right corner inside its panel (small icon buttons).
    pub static MENUS: Menu = Menu::named("core:menus");
}

/// One entity in `Registry::client_entity_frame`.
#[repr(transparent)]
pub struct ClientEntity(abi::ClientEntityArgs);

impl ClientEntity {
    pub fn id(&self) -> u64 {
        self.0.entity
    }
    pub fn kind(&self) -> u32 {
        self.0.kind
    }
    pub fn pos(&self) -> Vec3 {
        self.0.pos
    }
    pub fn yaw(&self) -> f32 {
        self.0.yaw
    }
    /// Distance walked so far; steps go by how it grows.
    pub fn walked(&self) -> f32 {
        self.0.walked
    }
    /// Seconds into its death animation (1: over, the last call), `None` while alive.
    pub fn dying(&self) -> Option<f32> {
        (self.0.dying >= 0.0).then_some(self.0.dying)
    }
    /// Seconds since this client first saw it.
    pub fn age(&self) -> f32 {
        self.0.age
    }
    /// Seconds since the last call.
    pub fn dt(&self) -> f32 {
        self.0.dt
    }
}
