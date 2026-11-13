//! Fingers on a touch screen, turned into what a mouse and keys give. Over a screen a finger is
//! the mouse where it touches, its left button down while it stays. In the world the touch
//! buttons (a list the player edits, `buttons`) press their action's keys; a finger on the
//! joystick (`STICK`, or near it) steers from its middle, as Bedrock's (pushed past its edge
//! forward it sprints); one on the hotbar picks that slot; one anywhere else turns the view, aimed
//! at the crosshair: a tap attacks what is there if it is a creature and uses it otherwise
//! (placing, opening; an item used by holding only opens what opens, or swings in the air), holding still
//! breaks the block there, or uses the item in hand where there is none or the item is one used
//! by holding (a bow, food).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use glam::Vec2;
use weft_abi::hud_anchor as a;
use winit::event::TouchPhase;
use winit::keyboard::KeyCode;

use crate::input::{Input, Key};
use crate::ui::Rect;

/// A touch button's action that is no bind: Escape, what pauses and closes.
pub const ESCAPE: &str = "escape";

/// One touch button: the action it presses, its side in GUI pixels, and whether a tap switches
/// it on and the next off instead of holding it.
#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub action: String,
    pub size: f32,
    pub toggle: bool,
}

/// GUI pixels: a new button's side, the joystick frame's, and how far from its middle the knob
/// goes, as a part of the frame.
pub const BUTTON_SIZE: f32 = 32.0;
pub const STICK_SIZE: f32 = 64.0;
pub const KNOB_REACH: f32 = 0.28;
/// Sizes the HUD editor lets a touch element take.
pub const SIZES: (f32, f32) = (12.0, 240.0);

/// The joystick's HUD element.
pub const STICK: &str = "core:touch.move";
const PREFIX: &str = "core:touch.";

/// The buttons a phone starts with.
const DEFAULTS: [(&str, bool); 5] =
    [(ESCAPE, false), ("core:chat", false), ("core:inventory", false), ("core:sneak", true), ("core:jump", false)];

/// The buttons, the joystick's side, where they are kept, and a count of changes for the HUD.
struct Layout {
    buttons: Vec<Button>,
    stick: f32,
    file: PathBuf,
    generation: u64,
}

static LAYOUT: parking_lot::RwLock<Option<Layout>> = parking_lot::RwLock::new(None);

/// Reads `config/touch.txt` (`stick = 64`, then `button = <action> <size> [toggle]` in order).
/// An automated run keeps its own, so what a test sees never depends on it.
pub fn load(run_dir: &std::path::Path) {
    let dir = if crate::config::automated() { run_dir.join("test-config") } else { crate::paths::config(run_dir) };
    let file = dir.join("touch.txt");
    let src = std::fs::read_to_string(&file).unwrap_or_default();
    let mut stick = STICK_SIZE;
    let mut buttons = Vec::new();
    for (_, name, value) in crate::kv::pairs(&src) {
        let words: Vec<&str> = value.split_whitespace().collect();
        match (name, words.as_slice()) {
            ("stick", [size]) => stick = size.parse().unwrap_or(STICK_SIZE),
            ("button", [action, size, rest @ ..]) => buttons.push(Button {
                action: action.to_string(),
                size: size.parse().unwrap_or(BUTTON_SIZE),
                toggle: rest.first() == Some(&"toggle"),
            }),
            _ => {}
        }
    }
    if src.trim().is_empty() {
        buttons = defaults();
    }
    let generation = LAYOUT.read().as_ref().map_or(0, |l| l.generation + 1);
    *LAYOUT.write() = Some(Layout { buttons, stick, file, generation });
}

pub fn defaults() -> Vec<Button> {
    DEFAULTS
        .iter()
        .map(|&(action, toggle)| Button { action: action.into(), size: BUTTON_SIZE, toggle })
        .collect()
}

pub fn buttons() -> Vec<Button> {
    LAYOUT
        .read()
        .as_ref()
        .map_or_else(defaults, |l| l.buttons.clone())
}

pub fn stick_size() -> f32 {
    LAYOUT.read().as_ref().map_or(STICK_SIZE, |l| l.stick)
}

/// Changes so far, for what is built from the layout to know it is out of date.
pub fn generation() -> u64 {
    LAYOUT.read().as_ref().map_or(0, |l| l.generation)
}

/// New buttons; written out at once.
pub fn set_buttons(buttons: Vec<Button>) {
    edit(|l| l.buttons = buttons);
    save();
}

/// Each touch element with its side: the joystick and the buttons.
pub fn sizes() -> Vec<(String, f32)> {
    std::iter::once((STICK.to_string(), stick_size()))
        .chain(buttons().into_iter().map(|b| (element(&b.action), b.size)))
        .collect()
}

/// A touch element's new side (the HUD editor dragging its corner); `save` once let go.
pub fn set_size(element: &str, size: f32) {
    let size = size.clamp(SIZES.0, SIZES.1).round();
    edit(|l| {
        if element == STICK {
            l.stick = size;
        } else if let Some(b) = action_of(element).and_then(|a| l.buttons.iter_mut().find(|b| b.action == a)) {
            b.size = size;
        }
    });
}

fn edit(f: impl FnOnce(&mut Layout)) {
    if let Some(l) = LAYOUT.write().as_mut() {
        f(l);
        l.generation += 1;
    }
}

pub fn save() {
    let guard = LAYOUT.read();
    let Some(l) = guard.as_ref() else { return };
    let mut text = format!("stick = {}\n", l.stick);
    for b in &l.buttons {
        text += &format!("button = {} {}{}\n", b.action, b.size, if b.toggle { " toggle" } else { "" });
    }
    let _ = l.file.parent().map(std::fs::create_dir_all);
    if let Err(e) = std::fs::write(&l.file, text) {
        log::error!("saving {}: {e}", l.file.display());
    }
}

/// A button's HUD element.
pub fn element(action: &str) -> String {
    format!("{PREFIX}{action}")
}

/// The action of a button's HUD element.
pub fn action_of(element: &str) -> Option<&str> {
    element.strip_prefix(PREFIX).filter(|_| element != STICK)
}

/// Whether a HUD element is one of the touch controls (resizable in the HUD editor).
pub fn is_touch(element: &str) -> bool {
    element.starts_with(PREFIX)
}

/// Where a button goes until the player moves it: movement and the inventory by the right thumb,
/// the rest along the top right.
pub fn place(action: &str, index: usize) -> (&'static str, i32) {
    match action {
        "core:jump" | "core:sneak" | "core:inventory" => (a::BOTTOM_RIGHT, -(index as i32)),
        _ => (a::TOP_RIGHT, index as i32 - 64),
    }
}

/// The keys that set `action` off, to press on the player's behalf: its first bind's key with
/// the modifiers and the key held with it (`F3+KeyG`).
pub fn keys_of(input: &Input, action: &str) -> Vec<Key> {
    if action == ESCAPE {
        return vec![Key::Code(KeyCode::Escape)];
    }
    let Some(b) = input.binding_of(action) else { return Vec::new() };
    let mods = crate::input::MODIFIERS
        .iter()
        .filter(|m| b.chord.mods & m.0 != 0)
        .map(|m| Key::Code(m.2));
    b.chord
        .with
        .into_iter()
        .chain(mods)
        .chain([b.chord.key])
        .collect()
}

/// What an action is called, for players to read: its bind's name (`key.<action>`).
pub fn action_name(action: &str) -> crate::ui::text::Arg {
    if action == ESCAPE {
        crate::ui::text::Arg::Text(crate::input::key_name(Key::Code(KeyCode::Escape)))
    } else {
        crate::ui::text::Arg::Name(crate::ui::text::Name::Key(action.to_string()))
    }
}

/// What a button shows: an icon (`gui/<name>`) for a key whose name is long, else the name.
pub enum Label {
    Icon(&'static str),
    Text(String),
}

pub fn label(input: &Input, action: &str) -> Label {
    let keys = keys_of(input, action);
    if let [k] = keys[..]
        && let Some(icon) = icon(k)
    {
        return Label::Icon(icon);
    }
    Label::Text(
        keys.iter()
            .map(|&k| crate::input::key_name(k))
            .collect::<Vec<_>>()
            .join("+"),
    )
}

fn icon(k: Key) -> Option<&'static str> {
    use KeyCode as C;
    Some(match k {
        Key::Code(C::Escape) => "key_escape",
        Key::Code(C::ShiftLeft | C::ShiftRight) => "key_shift",
        Key::Code(C::ControlLeft | C::ControlRight) => "key_ctrl",
        Key::Code(C::AltLeft | C::AltRight) => "key_alt",
        Key::Code(C::Space) => "key_space",
        Key::Code(C::Tab) => "key_tab",
        Key::Code(C::Enter | C::NumpadEnter) => "key_enter",
        Key::Code(C::Backspace) => "key_backspace",
        Key::Code(C::ArrowUp) => "key_up",
        Key::Code(C::ArrowDown) => "key_down",
        Key::Code(C::ArrowLeft) => "key_left",
        Key::Code(C::ArrowRight) => "key_right",
        Key::Mouse(0) => "key_mouse_left",
        Key::Mouse(1) => "key_mouse_right",
        Key::Mouse(2) => "key_mouse_middle",
        Key::Wheel(true) => "key_wheel_up",
        Key::Wheel(false) => "key_wheel_down",
        _ => return None,
    })
}

/// What the crosshair is on, for what a finger on the view does.
#[derive(Clone, Copy, Default)]
pub struct Aim {
    pub entity: bool,
    pub block: bool,
    /// The block does something of its own when used (a chest, a door).
    pub usable: bool,
    /// The item in hand is used by holding (food, a bow, a shield).
    pub holding_item: bool,
}

/// A finger still longer than this holds; let go sooner it was a tap.
const HOLD: Duration = Duration::from_millis(300);
/// GUI pixels a finger may wander and still be still.
const STILL: f32 = 6.0;
/// From the middle, as parts of the joystick's side: where steering starts, and where it
/// sprints (past the frame).
const DEAD: f32 = 0.1;
const SPRINT: f32 = 0.5;
/// The view turns this much per physical pixel a finger moves, against the mouse's one.
const LOOK: f32 = 0.6;

enum Role {
    /// The mouse, over a screen.
    Mouse,
    Button {
        keys: Vec<Key>,
        toggle: bool,
    },
    /// From the joystick's middle and its side, with the keys it holds.
    Stick {
        middle: Vec2,
        side: f32,
        held: Vec<Key>,
    },
    /// Turning the view; once held still, the keys it holds.
    Look {
        moved: bool,
        held: Option<Vec<Key>>,
    },
}

struct Finger {
    id: u64,
    /// GUI pixels.
    start: Vec2,
    last: Vec2,
    since: Instant,
    role: Role,
}

#[derive(Default)]
pub struct Touches {
    fingers: Vec<Finger>,
}

/// The movement keys a finger `off` the joystick's middle holds, pressing and letting go of
/// what changed since `held`.
fn steer(input: &mut Input, off: Vec2, side: f32, held: &[Key]) -> Vec<Key> {
    let off = off / side;
    let mut want: Vec<Key> = [
        (off.y < -DEAD, "core:forward"),
        (off.y > DEAD, "core:back"),
        (off.x < -DEAD, "core:left"),
        (off.x > DEAD, "core:right"),
        (-off.y > SPRINT, "core:sprint"),
    ]
    .into_iter()
    .filter(|w| w.0)
    .flat_map(|w| keys_of(input, w.1))
    .collect();
    want.dedup();
    for &k in held.iter().filter(|k| !want.contains(k)) {
        input.key_up(k);
    }
    for &k in want.iter().filter(|k| !held.contains(k)) {
        input.key_down(k);
    }
    want
}

fn tap(input: &mut Input, keys: &[Key]) {
    // Down and up in one frame: pressed for it, held for none.
    keys.iter().for_each(|&k| input.key_down(k));
    keys.iter().rev().for_each(|&k| input.key_up(k));
}

impl Touches {
    /// Where the knob is from the joystick's middle, GUI pixels.
    pub fn knob(&self) -> Vec2 {
        self.fingers
            .iter()
            .find_map(|f| match &f.role {
                Role::Stick { middle, side, .. } => Some((f.last - *middle).clamp_length_max(side * KNOB_REACH)),
                _ => None,
            })
            .unwrap_or(Vec2::ZERO)
    }

    /// A finger at physical `at`. `world`: the game has the input, no screen over it. `element`
    /// finds the HUD element at a point in GUI pixels and its place, `stick` is the joystick's.
    #[allow(clippy::too_many_arguments)]
    pub fn finger(
        &mut self,
        input: &mut Input,
        id: u64,
        phase: TouchPhase,
        at: Vec2,
        scale: f32,
        world: bool,
        aim: Aim,
        element: &dyn Fn(Vec2) -> Option<(String, Rect)>,
        stick: Option<Rect>,
    ) {
        let gui = at / scale;
        match phase {
            TouchPhase::Started => {
                let hit = if world { element(gui) } else { None };
                let button = hit
                    .as_ref()
                    .and_then(|(e, _)| action_of(e))
                    .and_then(|a| buttons().into_iter().find(|b| b.action == a));
                let role = if !world {
                    input.cursor = at;
                    input.key_down(Key::Mouse(0));
                    Role::Mouse
                } else if let Some(b) = button {
                    let keys = keys_of(input, &b.action);
                    if b.toggle && !keys.is_empty() && keys.iter().all(|&k| input.held_key(k)) {
                        keys.iter().rev().for_each(|&k| input.key_up(k));
                    } else {
                        keys.iter().for_each(|&k| input.key_down(k));
                    }
                    Role::Button { keys, toggle: b.toggle }
                } else if let Some((_, r)) = hit.filter(|(e, _)| e == "core:hotbar") {
                    let slots = crate::content::content().game.player_layout.hotbar as usize;
                    let slot = (((gui.x - r.x) / r.w * slots as f32) as usize).min(slots.saturating_sub(1));
                    let keys = crate::input::HOTBAR
                        .get(slot)
                        .map_or(Vec::new(), |a| keys_of(input, a));
                    keys.iter().for_each(|&k| input.key_down(k));
                    Role::Button { keys, toggle: false }
                } else if let Some(r) = stick.filter(|r| {
                    // Half its size around it too: a thumb finds it without looking.
                    let (mx, my) = (r.w / 2.0, r.h / 2.0);
                    gui.x >= r.x - mx && gui.x <= r.x + r.w + mx && gui.y >= r.y - my && gui.y <= r.y + r.h + my
                }) {
                    let middle = Vec2::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
                    Role::Stick { middle, side: r.w.max(1.0), held: steer(input, gui - middle, r.w.max(1.0), &[]) }
                } else {
                    Role::Look { moved: false, held: None }
                };
                self.fingers
                    .push(Finger { id, start: gui, last: gui, since: Instant::now(), role });
            }
            TouchPhase::Moved => {
                let Some(f) = self.fingers.iter_mut().find(|f| f.id == id) else { return };
                match &mut f.role {
                    Role::Mouse => input.cursor = at,
                    Role::Button { .. } => {}
                    Role::Stick { middle, side, held } => *held = steer(input, gui - *middle, *side, held),
                    Role::Look { moved, .. } => {
                        input.mouse_delta += (gui - f.last) * scale * LOOK;
                        *moved |= gui.distance(f.start) > STILL;
                    }
                }
                f.last = gui;
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(i) = self.fingers.iter().position(|f| f.id == id) else { return };
                let f = self.fingers.remove(i);
                match f.role {
                    Role::Mouse => input.key_up(Key::Mouse(0)),
                    Role::Button { keys, toggle: false } => keys.iter().rev().for_each(|&k| input.key_up(k)),
                    Role::Button { .. } => {}
                    Role::Stick { held, .. } => held.into_iter().for_each(|k| input.key_up(k)),
                    Role::Look { held: Some(keys), .. } => keys.iter().rev().for_each(|&k| input.key_up(k)),
                    Role::Look { moved: false, .. } if f.since.elapsed() < HOLD => {
                        // An item used by holding isn't started by a tap (a bow would twitch): with
                        // one a tap opens what opens, swings in the air, and on another block does
                        // nothing (an attack would break it in creative).
                        let action = if aim.entity {
                            Some("core:attack")
                        } else if !aim.holding_item || aim.usable {
                            Some("core:use")
                        } else if !aim.block {
                            Some("core:attack")
                        } else {
                            None
                        };
                        if let Some(action) = action {
                            tap(input, &keys_of(input, action));
                        }
                    }
                    Role::Look { .. } => {}
                }
            }
        }
    }

    /// Each frame: a finger held still on the view long enough starts breaking the block under
    /// the crosshair, or using the item in hand.
    pub fn tick(&mut self, input: &mut Input, aim: Aim) {
        for f in &mut self.fingers {
            if let Role::Look { moved: false, held: held @ None } = &mut f.role
                && f.since.elapsed() >= HOLD
            {
                let action = if aim.block && !aim.holding_item { "core:attack" } else { "core:use" };
                let keys = keys_of(input, action);
                keys.iter().for_each(|&k| input.key_down(k));
                *held = Some(keys);
            }
        }
    }
}

/// The touch buttons as a list to edit: added for any action from the controls, removed, held or
/// switched. Where they go and their sizes are the HUD editor's.
#[derive(Default)]
pub struct TouchButtons {
    /// Picking the action of a button to add.
    choosing: bool,
}

impl TouchButtons {
    fn choose(c: &mut crate::app::Ctx, on: bool) {
        if let Some(s) = c.screen::<TouchButtons>() {
            s.choosing = on;
        }
        c.requests.push(crate::app::Request::Rebuild);
    }

    fn change(c: &mut crate::app::Ctx, f: impl FnOnce(&mut Vec<Button>)) {
        let mut all = buttons();
        f(&mut all);
        set_buttons(all);
        c.requests.push(crate::app::Request::Rebuild);
    }
}

impl crate::screens::Screen for TouchButtons {
    fn name(&self) -> &str {
        "core:touch_buttons"
    }

    fn pauses(&self) -> bool {
        true
    }

    fn build(&mut self, ui: &mut crate::ui::Ui, cx: &crate::screens::BuildCx) {
        use crate::screens::{button, frame};
        use crate::ui::{Align, Style, text};
        let choosing = self.choosing;
        let row = |ui: &mut crate::ui::Ui, action: &str| {
            ui.container(Style::row().gap(6.0).self_align(Align::Stretch), |ui| {
                ui.panel("touch_button", Style::default().size(20.0, 20.0).center(), |ui| match label(cx.input, action) {
                    Label::Icon(icon) => drop(ui.add(
                        crate::ui::Kind::Image { texture: format!("core:gui/{icon}"), color: 0xFFFFFFFF },
                        Style::default().size(12.0, 12.0),
                    )),
                    Label::Text(t) => {
                        let n = ui.text(text::plain(&t), "label");
                        if let Some(t) = ui.text_mut(n) {
                            t.wrap = false;
                        }
                    }
                });
                let name = match action_name(action) {
                    text::Arg::Name(n) => n.spans(cx.lang, Some(cx.input)),
                    text::Arg::Text(t) => text::plain(&t),
                    _ => Vec::new(),
                };
                let n = ui.text(name, "default");
                ui.nodes[n].style.grow = 1.0;
            });
        };
        frame(ui, cx, if choosing { "touch.choose" } else { "touch.title" }, |ui| {
            ui.panel(
                "panel",
                Style::default()
                    .pad(8.0)
                    .gap(3.0)
                    .stretch()
                    .w(420.0)
                    .h(0.0)
                    .grow()
                    .min_h(60.0)
                    .scroll(),
                |ui| {
                    if choosing {
                        let actions = std::iter::once(ESCAPE.to_string()).chain(cx.input.binds.iter().map(|b| b.action.clone()));
                        for action in actions {
                            let a = action.clone();
                            ui.container(Style::row().gap(6.0).self_align(Align::Stretch), |ui| {
                                ui.container(Style::default().grow(), |ui| row(ui, &action));
                                button(ui, cx, "touch.add", &[], 80.0, move |c| {
                                    let action = a.clone();
                                    Self::change(c, |all| all.push(Button { action, size: BUTTON_SIZE, toggle: false }));
                                    Self::choose(c, false);
                                });
                            });
                        }
                    } else {
                        for (i, b) in buttons().into_iter().enumerate() {
                            ui.container(Style::row().gap(6.0).self_align(Align::Stretch), |ui| {
                                ui.container(Style::default().grow(), |ui| row(ui, &b.action));
                                let mode = if b.toggle { "touch.switch" } else { "touch.hold" };
                                button(ui, cx, mode, &[], 80.0, move |c| Self::change(c, |all| all[i].toggle = !all[i].toggle));
                                button(ui, cx, "touch.remove", &[], 80.0, move |c| Self::change(c, |all| drop(all.remove(i))));
                            });
                        }
                    }
                },
            );
            ui.container(Style::row().gap(6.0), |ui| {
                if choosing {
                    button(ui, cx, "menu.back", &[], 120.0, |c| Self::choose(c, false));
                } else {
                    button(ui, cx, "touch.add_button", &[], 120.0, |c| Self::choose(c, true));
                    button(ui, cx, "controls.reset", &[], 120.0, |c| Self::change(c, |all| *all = defaults()));
                    button(ui, cx, "menu.done", &[], 120.0, |c| c.requests.push(crate::app::Request::Close));
                }
            });
        });
    }
}
