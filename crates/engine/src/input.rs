//! Raw input plus named actions. Every action (engine or mod) is registered here with its default
//! bindings, so rebinding and conflict detection work on one table. An action has up to `SLOTS`
//! bindings, each a key held, switched on and off, or tapped twice (`Trigger`); `held` and
//! `pressed` answer for all of them, so no caller knows which.

use std::time::{Duration, Instant};

use glam::Vec2;
use rustc_hash::{FxHashMap, FxHashSet};
use winit::keyboard::KeyCode;

/// The hotbar slot actions, first to ninth.
pub const HOTBAR: [&str; 9] = [
    "core:hotbar1",
    "core:hotbar2",
    "core:hotbar3",
    "core:hotbar4",
    "core:hotbar5",
    "core:hotbar6",
    "core:hotbar7",
    "core:hotbar8",
    "core:hotbar9",
];

/// Where an action acts, a bit each (`abi::comp::key`, tags of its entry): two actions on one key
/// conflict only where their contexts meet, and a camera takes some for itself. An action with
/// none acts wherever the world has the input, all of these but a screen.
pub mod context {
    use weft_abi::comp::key as k;
    pub const MOVEMENT: u32 = 1;
    pub const HANDS: u32 = 2;
    pub const RIDING: u32 = 4;
    pub const DEBUG: u32 = 8;
    pub const SCREEN: u32 = 16;
    pub const TAGS: &[(&str, u32)] =
        &[(k::MOVEMENT, MOVEMENT), (k::HANDS, HANDS), (k::RIDING, RIDING), (k::DEBUG, DEBUG), (k::SCREEN, SCREEN)];
    /// What an action without a context is in.
    pub const WORLD: u32 = MOVEMENT | HANDS | RIDING | DEBUG;
}

/// The engine's actions, their default bindings (`Binding::parse_slots`) and contexts.
pub const ENGINE_KEYS: &[(&str, &str, u32)] = {
    use context::*;
    &[
        ("core:forward", "KeyW", MOVEMENT),
        ("core:back", "KeyS", MOVEMENT),
        ("core:left", "KeyA", MOVEMENT),
        ("core:right", "KeyD", MOVEMENT),
        ("core:jump", "Space", MOVEMENT),
        ("core:sneak", "ShiftLeft", MOVEMENT),
        // Meant as one key with sneaking.
        ("core:dismount", "ShiftLeft", RIDING),
        // Vanilla's double tap of forward is a second way to sprint.
        ("core:sprint", "ControlLeft, double KeyW", MOVEMENT),
        ("core:crawl", "KeyC", MOVEMENT),
        // And of jump to fly.
        ("core:fly", "KeyF, double Space", MOVEMENT),
        // A spectator's wheel, where others have the hotbar's.
        ("core:fly_faster", "wheel_up", MOVEMENT),
        ("core:fly_slower", "wheel_down", MOVEMENT),
        ("core:inventory", "KeyE", 0),
        ("core:drop", "KeyQ", HANDS),
        ("core:drop_all", "ctrl+KeyQ", HANDS),
        ("core:command", "Slash", 0),
        ("core:chat", "KeyT", 0),
        // Let go without another key pressed meanwhile: F3 and a key is that key's action.
        ("core:debug", "tap F3", DEBUG),
        ("core:screenshot", "F2", 0),
        ("core:perspective", "F5", 0),
        // Away from the keys a hand rests on.
        ("core:freecam", "KeyP", 0),
        ("core:fullscreen", "F11", 0),
        ("core:player_list", "Tab", 0),
        // In a screen with a search field (the creative inventory, the recipe viewer).
        ("core:focus_search", "Tab", SCREEN),
        ("core:hide_hud", "F1", 0),
        ("core:swap_hands", "KeyG", HANDS),
        ("core:reload", "F8, F3+KeyT", 0),
        ("core:cycle_game_mode", "F3+F4", DEBUG),
        ("core:debug_options", "F3+F6", DEBUG),
        ("core:show_cubes", "F3+KeyG", DEBUG),
        ("core:show_hitboxes", "F3+KeyB", DEBUG),
        ("core:debug_group1", "F3+Digit1", DEBUG),
        ("core:debug_group2", "F3+Digit2", DEBUG),
        ("core:debug_group3", "F3+Digit3", DEBUG),
        ("core:debug_group4", "F3+Digit4", DEBUG),
        ("core:debug_group5", "F3+Digit5", DEBUG),
        ("core:debug_group6", "F3+Digit6", DEBUG),
        ("core:debug_group7", "F3+Digit7", DEBUG),
        ("core:debug_group8", "F3+Digit8", DEBUG),
        ("core:debug_group9", "F3+Digit9", DEBUG),
        ("core:recipes", "KeyR", 0),
        ("core:uses", "KeyU", 0),
        ("core:attack", "mouse0", HANDS),
        ("core:use", "mouse1", HANDS),
        ("core:pick", "mouse2", HANDS),
        ("core:hotbar_next", "wheel_down", HANDS),
        ("core:hotbar_prev", "wheel_up", HANDS),
        ("core:hotbar1", "Digit1", 0),
        ("core:hotbar2", "Digit2", 0),
        ("core:hotbar3", "Digit3", 0),
        ("core:hotbar4", "Digit4", 0),
        ("core:hotbar5", "Digit5", 0),
        ("core:hotbar6", "Digit6", 0),
        ("core:hotbar7", "Digit7", 0),
        ("core:hotbar8", "Digit8", 0),
        ("core:hotbar9", "Digit9", 0),
    ]
};

/// Two presses closer than this are a double tap (vanilla's 7 ticks).
pub const DOUBLE_TAP: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Code(KeyCode),
    Mouse(u8),
    /// A notch of the mouse wheel, up or down: down and up again in the same frame.
    Wheel(bool),
}

/// Modifier bits of a `Chord`.
pub const CTRL: u8 = 1;
pub const SHIFT: u8 = 2;
pub const ALT: u8 = 4;

/// A key and the modifiers held with it (`ctrl+T`), and maybe another key held as one
/// (`F3+T`). A modifier key is also a key of its own (sneak on Shift).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub mods: u8,
    pub with: Option<Key>,
}

impl From<Key> for Chord {
    fn from(key: Key) -> Chord {
        Chord { key, mods: 0, with: None }
    }
}

/// The modifiers: its `Chord` bit, what it is called, and the two keys that are it.
pub const MODIFIERS: [(u8, &str, KeyCode, KeyCode); 3] = [
    (CTRL, "ctrl", KeyCode::ControlLeft, KeyCode::ControlRight),
    (SHIFT, "shift", KeyCode::ShiftLeft, KeyCode::ShiftRight),
    (ALT, "alt", KeyCode::AltLeft, KeyCode::AltRight),
];

/// The `Chord` bit of a modifier key, 0 for others.
pub fn modifier(k: Key) -> u8 {
    MODIFIERS
        .iter()
        .find(|m| k == Key::Code(m.2) || k == Key::Code(m.3))
        .map_or(0, |m| m.0)
}

/// How a binding's key sets its action off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Trigger {
    /// While the key is down.
    #[default]
    Hold,
    /// From one press to the next.
    Toggle,
    /// From a second press in quick succession (`DOUBLE_TAP`) for as long as the key stays down.
    Double,
    /// Let go with no other key pressed since it went down: a key also held as a modifier (F3).
    Tap,
}

/// One way to set an action off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Binding {
    pub chord: Chord,
    pub trigger: Trigger,
}

/// How many bindings an action has.
pub const SLOTS: usize = 2;

pub type Slots = [Option<Binding>; SLOTS];

impl Binding {
    /// `ctrl+KeyT`, `toggle KeyW`, `double Space`.
    pub fn id(self) -> String {
        let chord = self.chord.id();
        match self.trigger {
            Trigger::Hold => chord,
            Trigger::Toggle => format!("toggle {chord}"),
            Trigger::Double => format!("double {chord}"),
            Trigger::Tap => format!("tap {chord}"),
        }
    }

    pub fn parse(s: &str) -> Option<Binding> {
        let s = s.trim();
        let (trigger, chord) = match s.split_once(' ') {
            Some(("toggle", c)) => (Trigger::Toggle, c),
            Some(("double", c)) => (Trigger::Double, c),
            Some(("tap", c)) => (Trigger::Tap, c),
            _ => (Trigger::Hold, s),
        };
        Some(Binding { chord: Chord::parse(chord)?, trigger })
    }

    /// The bindings of one action, by slot, `none` for an empty one: `ControlLeft, double KeyW`.
    pub fn parse_slots(s: &str) -> Option<Slots> {
        let mut out = [None; SLOTS];
        for (slot, part) in s.split(',').map(str::trim).enumerate() {
            match part {
                "" | "none" => {}
                _ if slot >= SLOTS => return None,
                p => out[slot] = Some(Binding::parse(p)?),
            }
        }
        Some(out)
    }

    pub fn slots_id(slots: &Slots) -> String {
        let last = slots.iter().rposition(Option::is_some).map_or(0, |i| i + 1);
        slots[..last]
            .iter()
            .map(|b| b.map_or("none".into(), Binding::id))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub struct Bind {
    pub action: String,
    pub default: Slots,
    pub keys: Slots,
    /// `context` bits.
    pub contexts: u32,
    /// Per slot: switched on (a toggle, or a double tap while its key stays down), set off this
    /// frame, and a double tap's first press (a tap's press while it may still count).
    on: [bool; SLOTS],
    fired: [bool; SLOTS],
    tapped: [Option<Instant>; SLOTS],
}

impl Bind {
    fn new(action: &str, default: Slots, contexts: u32) -> Bind {
        Bind {
            action: action.to_string(),
            default,
            keys: default,
            contexts,
            on: [false; SLOTS],
            fired: [false; SLOTS],
            tapped: [None; SLOTS],
        }
    }
}

pub struct Input {
    pub captured: bool,
    /// Contexts a camera has for itself (`context` bits): `held` and `pressed` don't see their
    /// actions, and with movement the look is the camera's too.
    pub muted: u32,
    pub mouse_delta: Vec2,
    pub cursor: Vec2,
    pub scroll: f32,
    pub sensitivity: f32,
    pub invert_y: bool,
    pub typed: String,
    held: FxHashSet<Key>,
    pressed: FxHashSet<Key>,
    released: FxHashSet<Key>,
    repeated: FxHashSet<Key>,
    pub binds: Vec<Bind>,
    lookup: FxHashMap<String, usize>,
}

impl Default for Input {
    fn default() -> Self {
        Input {
            captured: false,
            muted: 0,
            mouse_delta: Vec2::ZERO,
            cursor: Vec2::ZERO,
            scroll: 0.0,
            sensitivity: 1.0,
            invert_y: false,
            typed: String::new(),
            held: FxHashSet::default(),
            pressed: FxHashSet::default(),
            released: FxHashSet::default(),
            repeated: FxHashSet::default(),
            binds: Vec::new(),
            lookup: FxHashMap::default(),
        }
    }
}

impl Input {
    pub fn register(&mut self, action: &str, default: Slots, contexts: u32) {
        if self.lookup.contains_key(action) {
            return;
        }
        self.lookup.insert(action.to_string(), self.binds.len());
        self.binds.push(Bind::new(action, default, contexts));
    }

    /// New keys for `action`; the same ones (settings applied again) keep what is held.
    pub fn rebind(&mut self, action: &str, keys: Slots) {
        if let Some(&i) = self.lookup.get(action) {
            let b = &self.binds[i];
            if b.keys != keys {
                self.binds[i] = Bind { keys, ..Bind::new(action, b.default, b.contexts) };
            }
        }
    }

    /// Actions with the same binding as another action where both act.
    pub fn conflicts(&self) -> Vec<(String, String)> {
        let reach = |b: &Bind| if b.contexts == 0 { context::WORLD } else { b.contexts };
        let mut out = Vec::new();
        for (i, a) in self.binds.iter().enumerate() {
            for b in &self.binds[i + 1..] {
                if reach(a) & reach(b) != 0 && a.keys.iter().flatten().any(|k| b.keys.contains(&Some(*k))) {
                    out.push((a.action.clone(), b.action.clone()));
                }
            }
        }
        out
    }

    /// The first binding of an action, for showing it and for pressing it on a player's behalf.
    pub fn binding_of(&self, action: &str) -> Option<Binding> {
        self.lookup
            .get(action)
            .and_then(|&i| self.binds[i].keys.iter().flatten().next().copied())
    }

    /// Whether a camera mode has `action` for itself now (`muted`).
    fn muted(&self, action: &str) -> bool {
        self.lookup
            .get(action)
            .is_some_and(|&i| self.binds[i].contexts & self.muted != 0)
    }

    pub fn held(&self, action: &str) -> bool {
        !self.muted(action) && self.held_raw(action)
    }

    pub fn pressed(&self, action: &str) -> bool {
        !self.muted(action) && self.pressed_raw(action)
    }

    /// `held` even while muted: the camera's own reading of the keys.
    pub fn held_raw(&self, action: &str) -> bool {
        self.lookup.get(action).is_some_and(|&i| {
            let b = &self.binds[i];
            (0..SLOTS).any(|s| match b.keys[s] {
                Some(Binding { chord, trigger: Trigger::Hold }) => self.active(chord, &self.held),
                Some(_) => b.on[s],
                None => false,
            })
        })
    }

    pub fn pressed_raw(&self, action: &str) -> bool {
        self.lookup.get(action).is_some_and(|&i| {
            let b = &self.binds[i];
            (0..SLOTS).any(|s| match b.keys[s] {
                Some(Binding { chord, trigger: Trigger::Hold }) => self.active(chord, &self.pressed),
                Some(_) => b.fired[s],
                None => false,
            })
        })
    }

    /// Switches the action off until its key is pressed again (a toggled sprint the server
    /// ended); held keys still count.
    pub fn stop(&mut self, action: &str) {
        if let Some(&i) = self.lookup.get(action) {
            (self.binds[i].on, self.binds[i].tapped) = ([false; SLOTS], [None; SLOTS]);
        }
    }

    /// Modifiers held now (`Chord` bits).
    pub fn mods(&self) -> u8 {
        self.held.iter().fold(0, |m, &k| m | modifier(k))
    }

    /// The chord is down: its key in `keys`, its modifiers held, and no binding of the same key
    /// with more of them, all held, takes it over (`ctrl+T` or `F3+T` rather than `T`).
    fn active(&self, c: Chord, keys: &FxHashSet<Key>) -> bool {
        let held = self.mods();
        let down = |c: Chord| held & c.mods == c.mods && c.with.is_none_or(|w| self.held.contains(&w));
        keys.contains(&c.key)
            && down(c)
            && !self
                .binds
                .iter()
                .flat_map(|b| b.keys.iter().flatten())
                .any(|b| {
                    let more = b.chord.mods & c.mods == c.mods && (c.with.is_none() || b.chord.with == c.with);
                    b.chord.key == c.key && b.chord != c && more && down(b.chord)
                })
    }

    /// Toggles and double taps see a key go down. They switch only while the game has the
    /// mouse, so typing in a screen leaves them be.
    fn switch(&mut self, k: Key) {
        if !self.captured {
            return;
        }
        let now = Instant::now();
        for i in 0..self.binds.len() {
            for s in 0..SLOTS {
                let Some(b) = self.binds[i].keys[s].filter(|b| b.chord.key == k && b.trigger != Trigger::Hold) else { continue };
                if !self.active(b.chord, &self.held) {
                    continue;
                }
                let bind = &mut self.binds[i];
                match b.trigger {
                    Trigger::Toggle => {
                        bind.on[s] = !bind.on[s];
                        bind.fired[s] = bind.on[s];
                    }
                    Trigger::Tap => bind.tapped[s] = Some(now),
                    _ => match bind.tapped[s].take() {
                        Some(t) if now.duration_since(t) < DOUBLE_TAP => (bind.on[s], bind.fired[s]) = (true, true),
                        _ => bind.tapped[s] = Some(now),
                    },
                }
            }
        }
    }

    /// A double tap lasts while its key is down; a tap is let go.
    fn let_go(&mut self, k: Key) {
        for b in &mut self.binds {
            for s in 0..SLOTS {
                match b.keys[s] {
                    Some(Binding { chord, trigger: Trigger::Double }) if chord.key == k => b.on[s] = false,
                    Some(Binding { chord, trigger: Trigger::Tap }) if chord.key == k => b.fired[s] = b.tapped[s].take().is_some(),
                    _ => {}
                }
            }
        }
    }

    pub fn held_key(&self, k: Key) -> bool {
        self.held.contains(&k)
    }

    pub fn pressed_key(&self, k: Key) -> bool {
        self.pressed.contains(&k)
    }

    pub fn pressed_list(&self) -> Vec<Key> {
        self.pressed.iter().copied().collect()
    }

    /// Keys let go of this frame.
    pub fn released_list(&self) -> Vec<Key> {
        self.released.iter().copied().collect()
    }

    pub fn held_button(&self, b: u8) -> bool {
        self.held.contains(&Key::Mouse(b))
    }

    pub fn pressed_button(&self, b: u8) -> bool {
        self.pressed.contains(&Key::Mouse(b))
    }

    /// Pressed this frame, or held long enough for the system's key repeat to fire.
    pub fn pressed_or_repeated(&self, k: Key) -> bool {
        self.pressed.contains(&k) || self.repeated.contains(&k)
    }

    pub fn key_repeat(&mut self, k: Key) {
        self.repeated.insert(k);
    }

    pub fn key_down(&mut self, k: Key) {
        if self.held.insert(k) {
            self.pressed.insert(k);
            // Another key down: what is held now is a modifier, not a tap.
            for b in &mut self.binds {
                for s in 0..SLOTS {
                    if b.keys[s].is_some_and(|b| b.trigger == Trigger::Tap && b.chord.key != k) {
                        b.tapped[s] = None;
                    }
                }
            }
            self.switch(k);
        }
    }

    /// Notches of the wheel: also the wheel's key, pressed and let go.
    pub fn wheel(&mut self, notches: f32) {
        self.scroll += notches;
        if notches != 0.0 {
            let k = Key::Wheel(notches > 0.0);
            self.key_down(k);
            self.key_up(k);
        }
    }

    pub fn key_up(&mut self, k: Key) {
        if self.held.remove(&k) {
            self.released.insert(k);
            self.let_go(k);
        }
    }

    pub fn release_all(&mut self) {
        for k in std::mem::take(&mut self.held) {
            self.let_go(k);
        }
    }

    /// Clears per-frame state after the frame consumed it.
    pub fn end_frame(&mut self) {
        for b in &mut self.binds {
            b.fired = [false; SLOTS];
        }
        self.pressed.clear();
        self.released.clear();
        self.repeated.clear();
        self.mouse_delta = Vec2::ZERO;
        self.scroll = 0.0;
        self.typed.clear();
    }
}

/// The keys as steering and camera modes read them (`abi::KeysCtx`): each action's, with the
/// client's tick counts (`Input::count_ticks`); `raw` also sees what a camera took.
pub struct Keys<'a> {
    pub input: &'a Input,
    pub ticks: &'a [(u32, u32)],
    pub raw: bool,
}

impl Keys<'_> {
    pub fn state(&self, action: &str) -> weft_abi::KeyState {
        let i = self.input;
        if !i.captured {
            return Default::default();
        }
        let (held, pressed) = if self.raw { (i.held_raw(action), i.pressed_raw(action)) } else { (i.held(action), i.pressed(action)) };
        let (held_ticks, released) = i
            .lookup
            .get(action)
            .and_then(|&n| self.ticks.get(n))
            .copied()
            .unwrap_or_default();
        weft_abi::KeyState { pressed: pressed as u32, held: held as u32, held_ticks, released }
    }

    /// -1..1 from `minus` to `plus` held.
    pub fn axis(&self, plus: &str, minus: &str) -> f32 {
        self.state(plus).held as f32 - self.state(minus).held as f32
    }

    pub fn ctx(&self) -> *const weft_abi::KeysCtx {
        self as *const Keys as *const weft_abi::KeysCtx
    }
}

/// `Host::keys_state`.
pub unsafe extern "C" fn keys_state(keys: *const weft_abi::KeysCtx, action: weft_abi::Str, out: *mut weft_abi::KeyState) {
    unsafe {
        if let Some(k) = (keys as *const Keys).as_ref() {
            *out = k.state(action.as_str());
        }
    }
}

impl Input {
    /// A game tick passed: how long each action has been held, and what was let go after how
    /// long (`Keys`).
    pub fn count_ticks(&self, ticks: &mut Vec<(u32, u32)>) {
        ticks.resize(self.binds.len(), (0, 0));
        for (b, t) in self.binds.iter().zip(ticks.iter_mut()) {
            *t = if self.captured && self.held(&b.action) { (t.0 + 1, 0) } else { (0, t.0) };
        }
    }
}

impl Key {
    /// What it is called in settings and to mods: `KeyR`, `Enter`, `mouse0`. `key_name` is what a
    /// player reads.
    pub fn id(self) -> String {
        match self {
            Key::Mouse(b) => format!("mouse{b}"),
            Key::Wheel(up) => if up { "wheel_up" } else { "wheel_down" }.into(),
            Key::Code(c) => format!("{c:?}"),
        }
    }

    pub fn parse(s: &str) -> Option<Key> {
        if let Some(b) = s.strip_prefix("mouse") {
            return b.parse().ok().map(Key::Mouse);
        }
        match s {
            "wheel_up" => return Some(Key::Wheel(true)),
            "wheel_down" => return Some(Key::Wheel(false)),
            _ => {}
        }
        use winit::keyboard::KeyCode::*;
        const ALL: &[winit::keyboard::KeyCode] = &[
            KeyA,
            KeyB,
            KeyC,
            KeyD,
            KeyE,
            KeyF,
            KeyG,
            KeyH,
            KeyI,
            KeyJ,
            KeyK,
            KeyL,
            KeyM,
            KeyN,
            KeyO,
            KeyP,
            KeyQ,
            KeyR,
            KeyS,
            KeyT,
            KeyU,
            KeyV,
            KeyW,
            KeyX,
            KeyY,
            KeyZ,
            Digit0,
            Digit1,
            Digit2,
            Digit3,
            Digit4,
            Digit5,
            Digit6,
            Digit7,
            Digit8,
            Digit9,
            F1,
            F2,
            F3,
            F4,
            F5,
            F6,
            F7,
            F8,
            F9,
            F10,
            F11,
            F12,
            Space,
            Enter,
            Tab,
            Backspace,
            Escape,
            ShiftLeft,
            ShiftRight,
            ControlLeft,
            ControlRight,
            AltLeft,
            AltRight,
            ArrowUp,
            ArrowDown,
            ArrowLeft,
            ArrowRight,
            Minus,
            Equal,
            BracketLeft,
            BracketRight,
            Backslash,
            Semicolon,
            Quote,
            Comma,
            Period,
            Slash,
            Backquote,
            CapsLock,
            Insert,
            Delete,
            Home,
            End,
            PageUp,
            PageDown,
            Numpad0,
            Numpad1,
            Numpad2,
            Numpad3,
            Numpad4,
            Numpad5,
            Numpad6,
            Numpad7,
            Numpad8,
            Numpad9,
            NumpadAdd,
            NumpadSubtract,
            NumpadMultiply,
            NumpadDivide,
            NumpadDecimal,
            NumpadEnter,
            NumpadEqual,
            NumLock,
            ScrollLock,
            PrintScreen,
            Pause,
            ContextMenu,
            IntlBackslash,
            F13,
            F14,
            F15,
            F16,
            F17,
            F18,
            F19,
            F20,
            F21,
            F22,
            F23,
            F24,
            SuperLeft,
            SuperRight,
        ];
        // Short names ("R", "5", "space", "lshift") as well as winit's ("KeyR", "Digit5", "ShiftLeft").
        let short = match s.to_ascii_lowercase().as_str() {
            l if l.len() == 1 && l.as_bytes()[0].is_ascii_alphabetic() => format!("Key{}", l.to_ascii_uppercase()),
            l if l.len() == 1 && l.as_bytes()[0].is_ascii_digit() => format!("Digit{l}"),
            "lshift" | "shift" => "ShiftLeft".into(),
            "rshift" => "ShiftRight".into(),
            "lctrl" | "ctrl" | "control" => "ControlLeft".into(),
            "rctrl" => "ControlRight".into(),
            "lalt" | "alt" => "AltLeft".into(),
            "ralt" => "AltRight".into(),
            "esc" => "Escape".into(),
            "up" => "ArrowUp".into(),
            "down" => "ArrowDown".into(),
            "left" => "ArrowLeft".into(),
            "right" => "ArrowRight".into(),
            _ => s.into(),
        };
        ALL.iter()
            .find(|c| format!("{c:?}").eq_ignore_ascii_case(&short))
            .map(|c| Key::Code(*c))
    }
}

impl Chord {
    /// `ctrl+shift+KeyT`, `F3+KeyT`: the modifiers, the key held with them, then the key.
    pub fn id(self) -> String {
        let mut out = String::new();
        for (bit, name, ..) in MODIFIERS {
            if self.mods & bit != 0 {
                out += name;
                out.push('+');
            }
        }
        if let Some(w) = self.with {
            out += &w.id();
            out.push('+');
        }
        out + &self.key.id()
    }

    /// A key with modifiers in front (`ctrl+T`, `shift+alt+F6`) and maybe one other key held
    /// (`F3+T`), or a key alone.
    pub fn parse(s: &str) -> Option<Chord> {
        let mut parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let key = Key::parse(parts.pop()?)?;
        let (mut mods, mut with) = (0, None);
        for p in parts {
            match MODIFIERS
                .iter()
                .find(|m| m.1.eq_ignore_ascii_case(p) || (m.1 == "ctrl" && p.eq_ignore_ascii_case("control")))
            {
                Some(m) => mods |= m.0,
                None if with.is_none() => with = Some(Key::parse(p)?),
                None => return None,
            }
        }
        Some(Chord { key, mods, with })
    }
}

pub fn key_name(k: Key) -> String {
    match k {
        Key::Mouse(0) => "LMB".into(),
        Key::Mouse(1) => "RMB".into(),
        Key::Mouse(2) => "MMB".into(),
        Key::Mouse(n) => format!("Mouse{}", n + 1),
        Key::Wheel(up) => if up { "WheelUp" } else { "WheelDown" }.into(),
        Key::Code(c) => {
            let s = format!("{c:?}");
            s.strip_prefix("Key")
                .or_else(|| s.strip_prefix("Digit"))
                .unwrap_or(&s)
                .to_string()
        }
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;

    fn input(action: &str, keys: &str) -> Input {
        let mut i = Input { captured: true, ..Input::default() };
        i.register(action, Binding::parse_slots(keys).unwrap(), 0);
        i
    }

    fn tap(i: &mut Input, k: KeyCode) {
        i.key_down(Key::Code(k));
        i.key_up(Key::Code(k));
        i.end_frame();
    }

    #[test]
    fn bindings_read_back() {
        for s in ["ControlLeft, double KeyW", "none, toggle ctrl+KeyT", "F3"] {
            assert_eq!(Binding::slots_id(&Binding::parse_slots(s).unwrap()), s);
        }
        assert!(Binding::parse_slots("KeyA, KeyB, KeyC").is_none(), "more than two");
        assert!(Binding::parse_slots("triple KeyA").is_none());
    }

    #[test]
    fn a_toggle_stays_on_until_pressed_again() {
        let mut i = input("core:sneak", "toggle ShiftLeft");
        i.key_down(Key::Code(KeyCode::ShiftLeft));
        assert!(i.pressed("core:sneak"));
        i.key_up(Key::Code(KeyCode::ShiftLeft));
        i.end_frame();
        assert!(i.held("core:sneak") && !i.pressed("core:sneak"));
        tap(&mut i, KeyCode::ShiftLeft);
        assert!(!i.held("core:sneak"));
        // Not while typing in a screen.
        i.captured = false;
        tap(&mut i, KeyCode::ShiftLeft);
        assert!(!i.held("core:sneak"));
    }

    #[test]
    fn a_double_tap_lasts_while_held_and_either_slot_counts() {
        let mut i = input("core:sprint", "ControlLeft, double KeyW");
        tap(&mut i, KeyCode::KeyW);
        assert!(!i.held("core:sprint"), "one tap");
        i.key_down(Key::Code(KeyCode::KeyW));
        assert!(i.pressed("core:sprint") && i.held("core:sprint"));
        i.key_up(Key::Code(KeyCode::KeyW));
        assert!(!i.held("core:sprint"));
        i.key_down(Key::Code(KeyCode::ControlLeft));
        assert!(i.held("core:sprint"));
        // Stopped, a toggle or a double tap waits for its key again; a held key still counts.
        let mut i = input("core:sprint", "toggle ControlLeft");
        tap(&mut i, KeyCode::ControlLeft);
        i.stop("core:sprint");
        assert!(!i.held("core:sprint"));
    }

    #[test]
    fn a_tap_is_a_key_alone_and_held_with_another_a_modifier() {
        let mut i = input("core:debug", "tap F3");
        i.register("core:show_cubes", Binding::parse_slots("F3+KeyG").unwrap(), 0);
        i.register("core:swap_hands", Binding::parse_slots("KeyG").unwrap(), 0);
        i.key_down(Key::Code(KeyCode::F3));
        assert!(!i.pressed("core:debug"), "not on the way down");
        i.key_up(Key::Code(KeyCode::F3));
        assert!(i.pressed("core:debug"));
        i.end_frame();
        i.key_down(Key::Code(KeyCode::F3));
        i.key_down(Key::Code(KeyCode::KeyG));
        assert!(i.pressed("core:show_cubes") && !i.pressed("core:swap_hands"), "F3+G over G");
        i.key_up(Key::Code(KeyCode::KeyG));
        i.key_up(Key::Code(KeyCode::F3));
        assert!(!i.pressed("core:debug"), "a modifier this time");
    }

    #[test]
    fn key_names() {
        assert_eq!(Key::parse("R"), Some(Key::Code(KeyCode::KeyR)));
        assert_eq!(Key::parse("KeyR"), Some(Key::Code(KeyCode::KeyR)));
        assert_eq!(Key::parse("5"), Some(Key::Code(KeyCode::Digit5)));
        assert_eq!(Key::parse("space"), Some(Key::Code(KeyCode::Space)));
        assert_eq!(Key::parse("LShift"), Some(Key::Code(KeyCode::ShiftLeft)));
        assert_eq!(Key::parse("F6"), Some(Key::Code(KeyCode::F6)));
        assert_eq!(Key::parse("mouse3"), Some(Key::Mouse(3)));
        assert_eq!(Key::parse("Rr"), None);
        assert_eq!(Chord::parse("ctrl+T"), Some(Chord { key: Key::Code(KeyCode::KeyT), mods: crate::input::CTRL, with: None }));
        assert_eq!(
            Chord::parse("Shift + Alt + F6"),
            Some(Chord { key: Key::Code(KeyCode::F6), mods: crate::input::SHIFT | crate::input::ALT, with: None })
        );
        assert_eq!(Chord::parse("LShift"), Some(Chord::from(Key::Code(KeyCode::ShiftLeft))));
        assert_eq!(Chord::parse("F3+T").map(|c| c.with), Some(Some(Key::Code(KeyCode::F3))));
        assert_eq!(Chord::parse("F3+G+T"), None);
        let c = Chord::parse("ctrl+shift+R").unwrap();
        assert_eq!(Chord::parse(&Chord::id(c)), Some(c));
    }

    #[test]
    fn what_a_key_is_called_reads_back() {
        for k in [
            Key::Mouse(0),
            Key::Mouse(3),
            Key::Wheel(true),
            Key::Code(KeyCode::KeyR),
            Key::Code(KeyCode::Digit5),
            Key::Code(KeyCode::Enter),
        ] {
            assert_eq!(Key::parse(&k.id()), Some(k));
        }
    }
}
