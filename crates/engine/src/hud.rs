//! In-game overlay: crosshair, hotbar, debug lines, chat, and HUD parts registered by mods.
//! Rebuilt only when what it shows changes; mod parts update through their bindings.

use weft_abi::ItemStack;

use crate::app::Ctx;
use crate::client::Client;
use crate::client::modui::{self, TAG_ENTITY, TAG_SLOT};
use crate::client::textures::GameTextures;
use crate::content::content;
use crate::ui::lang::Lang;
use crate::ui::look::Look;
use crate::ui::render::UiRenderer;
use crate::ui::text::{self, Arg};
use crate::ui::{Align, Justify, Kind, Rect, Style, Ui};

pub const TAG_HOTBAR: u32 = 1;

/// One element of the HUD: everything drawn over the world is one, the engine's and mods', and
/// goes through the same placing, so a player can arrange all of it (`hud_editor`).
#[derive(Clone, Copy)]
enum Part {
    Hotbar,
    HeldName,
    Crosshair,
    PlayerList,
    Chat,
    /// Text in `PLACES[i]`.
    Place(usize),
    /// `DEBUG_GROUPS[i]`.
    DebugGroup(usize),
    /// `mod_debug_groups()[i]`.
    DebugMod(usize),
    DebugHelp,
    Mod(weft_abi::ModId, weft_abi::Callback),
}
pub const TAG_CROSSHAIR: u32 = 2;

/// A player's say on one element (`config/hud.txt`): its place in an anchor's flow or a free spot,
/// and whether it shows.
#[derive(Clone, Default)]
pub struct Placement {
    flow: Option<(String, i32)>,
    /// `[ax, ay, x, y]`: this point of the element (fractions of its size) lies at the same point
    /// of the window, moved by (x, y) GUI pixels.
    at: Option<[f32; 4]>,
    hidden: bool,
}

impl Placement {
    /// `base:food = top_right 2`, `core:hotbar = at 0.5 1 0 -4`, either or neither followed by
    /// `hidden`.
    fn parse(value: &str) -> Placement {
        let mut words: Vec<&str> = value.split_whitespace().collect();
        let hidden = words.last() == Some(&"hidden");
        if hidden {
            words.pop();
        }
        let mut p = Placement { hidden, ..Default::default() };
        match words.as_slice() {
            ["at", rest @ ..] => {
                let n: Vec<f32> = rest.iter().filter_map(|v| v.parse().ok()).collect();
                if let [ax, ay, x, y] = n[..] {
                    p.at = Some([ax, ay, x, y]);
                }
            }
            [anchor, rest @ ..] => p.flow = Some((anchor.to_string(), rest.first().and_then(|o| o.parse().ok()).unwrap_or(0))),
            [] => {}
        }
        p
    }

    fn value(&self) -> String {
        let place = match (&self.at, &self.flow) {
            (Some([ax, ay, x, y]), _) => format!("at {ax} {ay} {x} {y}"),
            (None, Some((anchor, order))) => format!("{anchor} {order}"),
            (None, None) => String::new(),
        };
        if self.hidden { format!("{place} hidden").trim().to_string() } else { place }
    }
}

/// What the HUD editor wants shown, while it is open.
#[derive(Clone, Default, PartialEq)]
pub struct Edit {
    pub selected: Option<String>,
    pub labels: bool,
    /// The F3 panels are there to place, F3 or not.
    pub debug: bool,
}

/// When a line of the F3 overlay shows.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum LineState {
    #[default]
    WithF3,
    Always,
    Never,
}

impl LineState {
    pub fn next(self) -> LineState {
        match self {
            LineState::WithF3 => LineState::Always,
            LineState::Always => LineState::Never,
            LineState::Never => LineState::WithF3,
        }
    }

    /// Its name on the debug options screen.
    pub fn key(self) -> &'static str {
        match self {
            LineState::WithF3 => "hud.options.with_f3",
            LineState::Always => "hud.options.always",
            LineState::Never => "hud.options.never",
        }
    }
}

const DEBUG_LINES: &str = "core:debug_lines";

/// The lines not at their default (`core:debug_lines`: `fps=always,cube=never`).
pub fn debug_lines() -> rustc_hash::FxHashMap<String, LineState> {
    let text = crate::config::by_name(DEBUG_LINES).map_or(String::new(), crate::config::text);
    text.split(',')
        .filter_map(|e| {
            let (id, state) = e.split_once('=')?;
            Some((
                id.to_string(),
                match state {
                    "always" => LineState::Always,
                    "never" => LineState::Never,
                    _ => return None,
                },
            ))
        })
        .collect()
}

pub fn set_debug_line(id: &str, state: LineState) {
    let mut lines = debug_lines();
    lines.remove(id);
    if state != LineState::WithF3 {
        lines.insert(id.to_string(), state);
    }
    let mut parts: Vec<String> = lines
        .iter()
        .map(|(id, s)| format!("{id}={}", if *s == LineState::Always { "always" } else { "never" }))
        .collect();
    parts.sort();
    if let Some(cfg) = crate::config::by_name(DEBUG_LINES) {
        crate::config::set_text(cfg, &parts.join(","));
    }
}

pub fn reset_debug_lines() {
    if let Some(cfg) = crate::config::by_name(DEBUG_LINES) {
        crate::config::set_text(cfg, "");
    }
}

/// A line's id: its translation key without the common part.
pub fn line_id(key: &str) -> &str {
    if key.is_empty() { "mods" } else { key.strip_prefix("hud.debug.").unwrap_or(key) }
}

/// What the editor calls element `name`: `hud.element.<ns>.<name>`, else the name itself.
pub fn element_title(lang: &Lang, name: &str) -> Vec<text::Span> {
    let key = text::lang_key("hud.element", name);
    if let Some(m) = mod_debug_groups().iter().find(|m| m.3 == name) {
        return text::rich(lang.get(m.0), lang, None);
    }
    if lang.has(&key) { text::rich(lang.get(&key), lang, None) } else { text::plain(name) }
}

/// One element of this build: where it goes and what draws it.
struct Placed {
    name: String,
    anchor: String,
    order: i32,
    part: Part,
    at: Option<[f32; 4]>,
    hidden: bool,
}

#[derive(Default)]
pub struct Hud {
    pub ui: Ui,
    key: Option<(bool, usize, [u32; 3], bool, bool, u32, u8, (i32, i32), (u32, u32, u8), u64)>,
    /// Player overrides from `config/hud.txt` by element name.
    layout: rustc_hash::FxHashMap<String, Placement>,
    file: std::path::PathBuf,
    edit: Option<Edit>,
    /// The elements of the current tree.
    names: Vec<String>,
}

impl Hud {
    /// Reads the player's arrangement. An automated run keeps its own beside it, so that what a
    /// test sees never depends on it.
    pub fn load_layout(&mut self, run_dir: &std::path::Path) {
        let dir = if crate::config::automated() { run_dir.join("test-config") } else { crate::paths::config(run_dir) };
        self.file = dir.join("hud.txt");
        self.layout.clear();
        let src = std::fs::read_to_string(&self.file).unwrap_or_default();
        for (_, name, v) in crate::kv::pairs(&src) {
            self.layout.insert(name.to_string(), Placement::parse(v));
        }
        self.key = None;
    }

    /// Writes the arrangement back.
    pub fn save(&self) {
        let mut lines: Vec<String> = self
            .layout
            .iter()
            .map(|(name, p)| (name, p.value()))
            .filter(|(_, v)| !v.is_empty())
            .map(|(name, v)| format!("{name} = {v}\n"))
            .collect();
        lines.sort();
        let _ = self.file.parent().map(std::fs::create_dir_all);
        if let Err(e) = std::fs::write(&self.file, lines.concat()) {
            log::error!("saving {}: {e}", self.file.display());
        }
    }

    pub fn invalidate(&mut self) {
        self.key = None;
    }

    pub fn editing(&self) -> bool {
        self.edit.is_some()
    }

    pub fn set_edit(&mut self, edit: Option<Edit>) {
        if self.edit != edit {
            self.edit = edit;
            self.key = None;
        }
    }

    pub fn element_rect(&self, name: &str) -> Option<Rect> {
        self.ui
            .find(&format!("hud:{name}"))
            .map(|n| self.ui.nodes[n].rect)
    }

    /// The smallest element under a point, so one lying inside another can be reached.
    pub fn element_at(&self, p: glam::Vec2) -> Option<String> {
        let area = |r: &Rect| r.w * r.h;
        self.names
            .iter()
            .filter_map(|n| {
                self.element_rect(n)
                    .filter(|r| r.contains(p))
                    .map(|r| (n, r))
            })
            .min_by(|a, b| area(&a.1).total_cmp(&area(&b.1)))
            .map(|f| f.0.clone())
    }

    pub fn place(&mut self, name: &str, at: [f32; 4]) {
        self.layout.entry(name.to_string()).or_default().at = Some(at);
        self.key = None;
    }

    pub fn toggle_hidden(&mut self, name: &str) {
        let p = self.layout.entry(name.to_string()).or_default();
        p.hidden = !p.hidden;
        self.key = None;
        self.save();
    }

    /// Back to where its mod or the engine put it.
    pub fn reset(&mut self, name: &str) {
        self.layout.remove(name);
        self.key = None;
        self.save();
    }

    pub fn reset_all(&mut self) {
        self.layout.clear();
        self.key = None;
        self.save();
    }

    /// Returns true if the tree was rebuilt.
    /// `chat_open`: the chat screen shows the messages itself.
    /// `groups`: bit per `DEBUG_GROUPS` entry shown with the debug overlay.
    pub fn update(&mut self, client: &Client, debug: bool, groups: u32, chat_open: bool, lang: &Lang, input: &crate::input::Input) -> bool {
        let now = std::time::Instant::now();
        let chat_visible = if chat_open {
            0
        } else {
            client
                .chat
                .iter()
                .filter(|(s, t)| s.alpha(now.duration_since(*t).as_secs_f32()) > 0.0)
                .count()
        };
        // The held item's name shows for two seconds after a change, fading for the last half.
        let (held, since) = client.held_name;
        let fade = if held.is_empty() { 0 } else { ((2.0 - since.elapsed().as_secs_f32()) / 0.5 * 16.0).clamp(0.0, 16.0) as u8 };
        // Text on the screen: what is up now, with how far it faded, in 16 steps like the held
        // name's. Rounded up: step 0 has to mean gone, or the last rebuild leaves it there faint.
        let up = |place: Option<usize>| -> Vec<(&crate::net::Shown, f32)> {
            client
                .shown
                .iter()
                .filter(|(s, _)| place.map_or(s.at.is_some(), |p| s.place as usize == p && s.at.is_none()))
                .map(|(s, t)| (s, s.alpha(now.duration_since(*t).as_secs_f32())))
                .filter(|(_, alpha)| *alpha > 0.0)
                .collect()
        };
        let shown_steps = client
            .chat
            .iter()
            .chain(&client.shown)
            .fold(client.shown_changes as u64, |h, (s, t)| {
                h.wrapping_mul(17)
                    .wrapping_add((s.alpha(now.duration_since(*t).as_secs_f32()) * 16.0).ceil() as u64)
            });
        // Free elements are placed from the window's size; narrow screens stack the debug panels.
        let size = (self.ui.size.x as i32, self.ui.size.y as i32);
        // Tab, and only over the game: in the chat it completes.
        let list_shown = input.captured && input.held("core:player_list");
        let chat = crate::settings::chat_look();
        let key = (
            debug,
            chat_visible,
            [chat.width as u32, chat.closed_height as u32, chat.dim as u32],
            list_shown,
            client.dead,
            groups,
            client.mode.0 as u8,
            size,
            (held.item, held.data, fade),
            shown_steps,
        );
        if self.key == Some(key) {
            return false;
        }
        self.key = Some(key);
        let c = content();
        use weft_abi::hud_anchor as a;
        let mods_debug = mod_debug_groups();
        let edit = self.edit.clone();
        let editing = edit.is_some();
        // Lines by their state: F3 panels show with F3 and their group on, or in the editor.
        let lines = debug_lines();
        let debug_on = edit.as_ref().is_some_and(|e| e.debug);
        let line_shown = |id: &str, group: Option<usize>| match lines.get(id).copied().unwrap_or_default() {
            LineState::Always => true,
            LineState::Never => false,
            LineState::WithF3 => debug_on || (debug && group.is_none_or(|g| groups & (1 << g) != 0)),
        };
        let shown = |part: Part| match part {
            Part::DebugGroup(i) => DEBUG_GROUPS[i]
                .2
                .iter()
                .any(|p| p.1.iter().any(|l| line_shown(line_id(l.0), Some(i)))),
            Part::DebugMod(i) => line_shown(mods_debug[i].3, Some(DEBUG_GROUPS.len() + i)),
            Part::DebugHelp => line_shown("help", None),
            _ => true,
        };
        // Everything on the overlay, the engine's elements placed like the mods'.
        let mut all: Vec<(String, String, i32, Part)> = vec![
            ("core:hotbar".into(), a::HOTBAR.into(), 0, Part::Hotbar),
            ("core:held_name".into(), a::ABOVE_HOTBAR.into(), 0, Part::HeldName),
            ("core:crosshair".into(), CENTER.into(), 0, Part::Crosshair),
            ("core:player_list".into(), CENTER.into(), 2, Part::PlayerList),
            ("core:chat".into(), CHAT.into(), 0, Part::Chat),
            ("core:debug.help".into(), a::DEBUG_LEFT.into(), i32::MAX, Part::DebugHelp),
        ];
        for (i, p) in places()
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.element.is_empty())
        {
            all.push((p.element.clone(), p.anchor.clone(), p.order, Part::Place(i)));
        }
        for (i, g) in DEBUG_GROUPS.iter().enumerate() {
            let side = if g.1 { a::DEBUG_RIGHT } else { a::DEBUG_LEFT };
            all.push((
                format!("core:debug.{}", g.0.rsplit('.').next().unwrap_or("")),
                side.into(),
                i32::MIN + i as i32,
                Part::DebugGroup(i),
            ));
        }
        for (i, m) in mods_debug.iter().enumerate() {
            all.push((
                m.3.to_string(),
                if m.1 { a::DEBUG_RIGHT } else { a::DEBUG_LEFT }.into(),
                i32::MIN / 2 + i as i32,
                Part::DebugMod(i),
            ));
        }
        all.extend(
            c.game
                .huds
                .iter()
                .filter(|h| h.anchor != a::DEBUG_LEFT && h.anchor != a::DEBUG_RIGHT)
                .map(|h| (h.name.clone(), h.anchor.clone(), h.order, Part::Mod(h.owner, h.cb))),
        );
        let mut placed: Vec<Placed> = all
            .into_iter()
            .filter(|e| shown(e.3))
            .map(|(name, anchor, order, part)| {
                let p = self.layout.get(&name).cloned().unwrap_or_default();
                let at = match part {
                    Part::Place(i) if p.flow.is_none() => p.at.or(places()[i].at),
                    _ => p.at,
                };
                let (anchor, order) = p.flow.unwrap_or((anchor, order));
                Placed { name, anchor, order, part, at, hidden: p.hidden }
            })
            .filter(|e| editing || !e.hidden)
            .collect();
        placed.sort_by_key(|p| p.order);
        self.names = placed.iter().map(|p| p.name.clone()).collect();
        let at = |anchor: &str, upwards: bool| -> Vec<&Placed> {
            let mut v: Vec<_> = placed
                .iter()
                .filter(|p| p.at.is_none() && p.anchor == anchor)
                .collect();
            if upwards {
                v.reverse();
            }
            v
        };
        let draw = |ui: &mut Ui, part: Part, align: Align| match part {
            Part::Mod(owner, cb) => modui::build(ui, lang, Some(input), owner, cb, Some(client)),
            Part::HeldName if fade > 0 => {
                let id = ui.text(stack_name(lang, Some(input), held), "label");
                if let Kind::Text(t) = &mut ui.nodes[id].kind {
                    t.wrap = false;
                    t.color = Some(0xFFFFFF00 | (fade as u32 * 255 / 16));
                }
            }
            Part::HeldName if editing => drop(ui.text(text::rich(lang.get("hud.edit.sample_name"), lang, None), "label")),
            Part::HeldName => {}
            Part::Hotbar if client.mode.has(crate::tag_bits::game_mode::NO_HUD) => {}
            Part::Hotbar => {
                ui.container(Style::row().gap(0.0), |ui| {
                    for i in 0..content().game.player_layout.hotbar as u64 {
                        ui.add(Kind::Custom { tag: TAG_HOTBAR, data: i }, Style::default().size(20.0, 20.0));
                    }
                });
            }
            Part::Crosshair => drop(ui.add(Kind::Custom { tag: TAG_CROSSHAIR, data: 0 }, Style::default().size(9.0, 9.0))),
            Part::PlayerList if list_shown || editing => {
                let mut style = Style::default().pad(4.0).gap(1.0);
                style.at = Some([0.0, 12.0 - ui.size.y / 2.0, 0.5, 0.0]);
                ui.panel("panel", style, |ui| {
                    for name in client.players.iter() {
                        ui.text(text::rich(&text::escape(name), lang, None), "label");
                    }
                });
            }
            Part::PlayerList => {}
            Part::Chat if chat_visible > 0 => crate::game_screens::chat_lines(ui, client, lang, input, false),
            Part::Chat if editing => drop(ui.text(text::rich(lang.get("hud.edit.sample_chat"), lang, None), "chat")),
            Part::Chat => {}
            Part::Place(i) => {
                let lines = up(Some(i));
                for (s, alpha) in &lines {
                    shown_text(ui, s, *alpha, lang, Some(input));
                }
                if lines.is_empty() && editing {
                    let place = &places()[i];
                    let id = ui.text(text::rich(lang.get(&place.sample), lang, None), &place.class);
                    if let Some(t) = ui.text_mut(id) {
                        t.scale = place.scale;
                        t.wrap = place.wrap;
                    }
                }
            }
            Part::DebugGroup(i) => debug_group(ui, i, align, &|id| line_shown(id, Some(i))),
            Part::DebugMod(i) => debug_mod_panel(ui, mods_debug[i].0, mods_debug[i].2, align, &|ui, (o, cb)| {
                modui::build(ui, lang, Some(input), o, cb, Some(client))
            }),
            Part::DebugHelp => debug_help(ui, groups, align, &mods_debug),
        };
        let selected = edit.as_ref().and_then(|e| e.selected.clone());
        let labels = edit.as_ref().is_some_and(|e| e.labels);
        // An element's own node, which carries its place: empty ones leave no gap in the flow,
        // and in the editor have a size to take hold of.
        let emit = |ui: &mut Ui, e: &Placed, style: Style, align: Align| {
            let class = editing.then(|| {
                if selected.as_deref() == Some(e.name.as_str()) {
                    "hud_frame_selected"
                } else if e.hidden {
                    "hud_frame_hidden"
                } else {
                    "hud_frame"
                }
                .to_string()
            });
            let id = ui.open(Kind::Container { class }, style.align(align));
            draw(ui, e.part, align);
            if editing && ui.nodes[id].children.is_empty() {
                ui.nodes[id].style.min = [40.0, 9.0];
            }
            if labels {
                let t = ui.text(element_title(lang, &e.name), "debug");
                ui.nodes[t].style.at = Some([0.0, -1.0, 0.0, 1.0]);
                if let Some(t) = ui.text_mut(t) {
                    t.wrap = false;
                }
            }
            ui.close();
            ui.set_id(id, &format!("hud:{}", e.name));
            if !editing {
                ui.unlink_if_empty(id);
            }
        };
        let window = self.ui.size;
        let ui = &mut self.ui;
        ui.clear();
        ui.root_style(Style::default().stretch());
        // Both halves start at zero height and split the rest evenly, so the crosshair sits at the
        // exact screen center whatever they contain.
        ui.container(Style::row().h(0.0).grow().pad(4.0).align(Align::Start), |ui| {
            // Half the screen each, so wide text on one side never pushes the other off screen
            // (the two may overlap on narrow screens, like vanilla's).
            ui.container(Style::default().w(0.0).grow().gap(1.0).align(Align::Start), |ui| {
                for e in at(a::DEBUG_LEFT, false)
                    .into_iter()
                    .chain(at(a::TOP_LEFT, false))
                {
                    emit(ui, e, Style::default(), Align::Start);
                }
            });
            ui.container(Style::default().w(0.0).grow().gap(1.0).align(Align::End), |ui| {
                for e in at(a::DEBUG_RIGHT, false)
                    .into_iter()
                    .chain(at(a::TOP_RIGHT, false))
                {
                    emit(ui, e, Style::default(), Align::End);
                }
            });
        });
        for e in at(CENTER, false) {
            emit(ui, e, Style::default().self_align(Align::Center), Align::Start);
        }
        // No height of its own, so what is under the crosshair doesn't move it off the center.
        ui.container(
            Style::default()
                .h(0.0)
                .gap(1.0)
                .self_align(Align::Center)
                .align(Align::Center),
            |ui| {
                ui.container(Style::default().h(3.0), |_| {});
                for e in at(a::UNDER_CROSSHAIR, false) {
                    emit(ui, e, Style::default(), Align::Center);
                }
            },
        );
        ui.container(
            Style::default()
                .h(0.0)
                .grow()
                .justify(Justify::End)
                .align(Align::Stretch)
                .pad(2.0)
                .gap(1.0),
            |ui| {
                for e in at(CHAT, false) {
                    emit(ui, e, Style::default().self_align(Align::Start), Align::Start);
                }
                ui.container(Style::row().align(Align::End), |ui| {
                    ui.container(Style::default().w(0.0).gap(1.0).align(Align::Start), |ui| {
                        for e in at(a::BOTTOM_LEFT, true) {
                            emit(ui, e, Style::default(), Align::Start);
                        }
                    });
                    ui.container(Style::default().grow(), |_| {});
                    ui.container(Style::default().gap(1.0).align(Align::Center), |ui| {
                        for e in at(a::ABOVE_HOTBAR, true) {
                            emit(ui, e, Style::default(), Align::Center);
                        }
                        ui.container(Style::row().w(182.0).gap(2.0).align(Align::End), |ui| {
                            ui.container(Style::default().gap(1.0).align(Align::Start), |ui| {
                                for e in at(a::HOTBAR_LEFT, true) {
                                    emit(ui, e, Style::default(), Align::Start);
                                }
                            });
                            ui.container(Style::default().grow(), |_| {});
                            ui.container(Style::default().gap(1.0).align(Align::End), |ui| {
                                for e in at(a::HOTBAR_RIGHT, true) {
                                    emit(ui, e, Style::default(), Align::End);
                                }
                            });
                        });
                        for e in at(a::HOTBAR, true) {
                            emit(ui, e, Style::default(), Align::Center);
                        }
                    });
                    ui.container(Style::default().grow(), |_| {});
                    ui.container(Style::default().w(0.0).gap(1.0).align(Align::End), |ui| {
                        for e in at(a::BOTTOM_RIGHT, true) {
                            emit(ui, e, Style::default(), Align::End);
                        }
                    });
                });
            },
        );
        for e in placed.iter().filter(|p| p.at.is_some()) {
            let [ax, ay, x, y] = e.at.unwrap_or_default();
            emit(ui, e, Style::default().at(ax * window.x + x, ay * window.y + y, [ax, ay]), Align::Start);
        }
        for (s, alpha) in up(None) {
            let [ax, ay] = s.at.unwrap_or_default();
            ui.container(Style::default().at(ax * window.x, ay * window.y, [ax, ay]), |ui| {
                shown_text(ui, s, alpha, lang, Some(input));
            });
        }
        true
    }
}

/// The engine's own anchors, which mods have no use for.
const CENTER: &str = "core:center";
const CHAT: &str = "core:chat";

/// A place for text on the screen (`abi::show_place` and mods' own): what text there looks like
/// unless it says otherwise, and the HUD element it is.
#[derive(Clone, Debug)]
pub struct Place {
    pub name: String,
    pub scale: f32,
    /// Fading in, staying, fading out.
    pub times: [f32; 3],
    /// More text without an id adds to what is there instead of replacing it.
    pub stacks: bool,
    pub class: String,
    pub wrap: bool,
    /// Where text stands if no one says, instead of joining an element.
    pub own_at: Option<[f32; 2]>,
    /// The element of the HUD it joins and where that goes by default: `anchor` and `order`, or
    /// a free spot `at` (`Placement::at`). Empty for what is no element (free text, the chat).
    pub element: String,
    pub anchor: String,
    pub order: i32,
    pub at: Option<[f32; 4]>,
    /// What the HUD editor shows of it.
    pub sample: String,
}

impl Place {
    fn plain(name: &str) -> Place {
        Place {
            name: name.into(),
            scale: 1.0,
            times: [0.0, 2.5, 0.5],
            stacks: true,
            class: "label".into(),
            wrap: false,
            own_at: None,
            element: String::new(),
            anchor: String::new(),
            order: 0,
            at: None,
            sample: String::new(),
        }
    }

    /// A place from its entry: plain text where it says nothing.
    pub fn bake(e: &crate::entries::Entry) -> Place {
        use weft_abi::comp::text_place as t;
        let anchor = e.text(t::ANCHOR).unwrap_or_default().to_string();
        let class = e.text(t::CLASS).unwrap_or("label").to_string();
        Place {
            element: if e.has(t::NO_ELEMENT) { String::new() } else { e.name.clone() },
            at: (anchor.is_empty() && !e.has(t::OWN_AT)).then(|| e.or(t::AT, [0.5, 0.5, 0.0, 0.0])),
            own_at: e.get(t::OWN_AT),
            anchor,
            order: e.or(t::ORDER, 0),
            scale: e.get::<f32>(t::SCALE).filter(|s| *s > 0.0).unwrap_or(1.0),
            times: e.or(t::TIMES, [0.0f32, 2.5, 0.5]).map(|v| v.max(0.0)),
            stacks: e.has(t::STACKS),
            wrap: e.has(t::WRAP),
            class,
            sample: e.text(t::SAMPLE).unwrap_or_default().to_string(),
            ..Place::plain(&e.name)
        }
    }
}

/// The engine's places, by `abi::show_place`: vanilla's numbers, with the crosshair between a
/// title and its subtitle.
pub fn engine_places() -> Vec<crate::entries::RawEntry> {
    use weft_abi::comp::text_place as t;
    let n = weft_abi::show_place::NAMES;
    let e = |name: &str| crate::entries::RawEntry::engine(weft_abi::entry_kind::TEXT_PLACE, name);
    let title = |name: &str, scale: f32, dy: f32, sample: &str| {
        e(name)
            .set(t::SCALE, scale)
            .set(t::TIMES, [0.5f32, 3.5, 1.0])
            .set(t::AT, [0.5f32, 0.5, 0.0, dy])
            .set(t::SAMPLE, sample.to_string())
    };
    vec![
        e(n[0])
            .set(t::OWN_AT, [0.5f32, 0.5])
            .tag(t::STACKS)
            .tag(t::NO_ELEMENT),
        e(n[1])
            .set(t::TIMES, [0.0f32, 10.0, 0.0])
            .set(t::CLASS, "chat".to_string())
            .tag(t::WRAP)
            .tag(t::STACKS)
            .tag(t::NO_ELEMENT),
        e(n[2])
            .set(t::ANCHOR, weft_abi::hud_anchor::ABOVE_HOTBAR.to_string())
            .set(t::ORDER, 1i32)
            .set(t::SAMPLE, "hud.edit.sample_hint".to_string()),
        title(n[3], 4.0, -24.0, "hud.edit.sample_title"),
        title(n[4], 2.0, 18.0, "hud.edit.sample_subtitle"),
    ]
}

/// Every place, the engine's first.
pub fn places() -> &'static [Place] {
    &crate::content::content().game.text_places
}

/// One text on the screen, whichever way it got there.
pub fn shown_text(ui: &mut Ui, s: &crate::net::Shown, alpha: f32, lang: &Lang, input: Option<&crate::input::Input>) -> crate::ui::NodeId {
    let look = s.look();
    let id = ui.text(s.msg.spans(lang, input), &look.class);
    // Its class's color spelled out, for the alpha to go with it and not jump at the end of a fade.
    let color = s.color.unwrap_or_else(|| {
        let class = lang
            .color(&format!("color.{}", look.class))
            .or_else(|| lang.color("color.default"));
        class.map_or(0xFFFFFF, |c| c >> 8)
    });
    if let Some(t) = ui.text_mut(id) {
        t.wrap = look.wrap && s.at.is_none();
        t.scale = s.scale;
        t.color = Some(color << 8 | (alpha * 255.0) as u32);
    }
    id
}

type DebugLine = (&'static str, fn(&Ctx) -> Vec<Arg>);

type Panels = &'static [(&'static str, &'static [DebugLine])];

/// Debug overlay groups in F3+number order: title key (for the help), right side, and its
/// panels (title, lines), side by side.
pub const DEBUG_GROUPS: &[(&str, bool, Panels)] = &[
    ("hud.debug.group.performance", true, &[("hud.debug.group.cpu", CPU), ("hud.debug.group.gpu", GPU)]),
    ("hud.debug.group.position", false, &[("hud.debug.group.position", POSITION)]),
    ("hud.debug.group.world", false, &[("hud.debug.group.world", WORLD)]),
    ("hud.debug.group.mods", true, &[("hud.debug.group.mods", MODS)]),
    ("hud.debug.group.system", true, &[("hud.debug.group.system", SYSTEM)]),
];

fn cpu(c: &Ctx) -> Vec<Arg> {
    c.stats
        .cpu
        .iter()
        .map(|v| Arg::Float(*v as f64, 2))
        .collect()
}

fn gpu(c: &Ctx) -> Vec<Arg> {
    let g = &c.stats.gpu;
    let at = |i: usize| Arg::Float(g.get(i).copied().unwrap_or(0.0) as f64, 2);
    vec![Arg::Float(g.iter().sum::<f32>() as f64, 2), at(1), at(2), at(3)]
}

// Short lines: the two panels stand side by side, also at GUI scale 3 in Russian.
const CPU: &[DebugLine] = &[
    ("hud.debug.fps", |c| vec![Arg::Int(c.stats.fps.round() as i64), Arg::Float(c.stats.frame_ms as f64, 2)]),
    ("hud.debug.cpu", cpu),
    ("hud.debug.cpu_submit", cpu),
    ("hud.debug.server", |c| vec![Arg::Float(c.stats.server_ms as f64, 2), Arg::Int(c.stats.regions as i64)]),
    ("hud.debug.phases", |c| {
        let p = c.stats.phases;
        vec![Arg::Float(p[0] as f64, 2), Arg::Float(p[2] as f64, 2), Arg::Float((p[3] + p[4]) as f64, 2)]
    }),
];

const GPU: &[DebugLine] = &[("hud.debug.gpu", gpu), ("hud.debug.gpu_world", gpu), ("hud.debug.gpu_ui", gpu)];

const POSITION: &[DebugLine] = &[
    ("hud.debug.pos", |c| {
        let p = c
            .client
            .as_ref()
            .map_or(glam::DVec3::ZERO, |cl| cl.player.pos);
        vec![Arg::Float(p.x, 3), Arg::Float(p.y, 3), Arg::Float(p.z, 3)]
    }),
    ("hud.debug.block", |c| {
        let p = c
            .client
            .as_ref()
            .map_or(glam::IVec3::ZERO, |cl| cl.player.pos.floor().as_ivec3());
        vec![Arg::Int(p.x as i64), Arg::Int(p.y as i64), Arg::Int(p.z as i64)]
    }),
    ("hud.debug.cube", |c| {
        let p = c
            .client
            .as_ref()
            .map_or(glam::IVec3::ZERO, |cl| cl.player.pos.floor().as_ivec3() >> 4i32);
        vec![Arg::Int(p.x as i64), Arg::Int(p.y as i64), Arg::Int(p.z as i64)]
    }),
    ("hud.debug.facing", |c| {
        let (yaw, pitch) = c
            .client
            .as_ref()
            .map_or((0.0, 0.0), |cl| (cl.player.yaw, cl.player.pitch));
        // Yaw 0 looks along -Z (north); turning right goes to east (+X).
        let deg = (-yaw.to_degrees()).rem_euclid(360.0);
        let dir = ["north", "east", "south", "west"][(((deg + 45.0) / 90.0) as usize) % 4];
        vec![Arg::Key(format!("hud.debug.dir.{dir}")), Arg::Float(deg as f64, 1), Arg::Float(pitch.to_degrees() as f64, 1)]
    }),
    ("hud.debug.biome", |c| {
        let Some(cl) = c.client.as_ref() else { return vec![Arg::Text("-".into())] };
        let p = cl.player.pos.floor().as_ivec3();
        let id = cl
            .world
            .cube(p >> 4i32)
            .map_or(0, |cube| cube.biome((p.x & 15) as usize, (p.z & 15) as usize));
        vec![Arg::Text(
            content()
                .biomes
                .get(id as usize)
                .map_or("-".into(), |b| b.name.clone()),
        )]
    }),
    ("hud.debug.dimension", |c| {
        let d = c.client.as_ref().map_or(0, |cl| cl.world.dim);
        vec![Arg::Text(content().dims[d as usize].name.clone())]
    }),
    ("hud.debug.light", |c| {
        let l = c.client.as_ref().map_or(0, |cl| {
            cl.world
                .light((cl.player.pos + glam::DVec3::Y * 0.5).floor().as_ivec3())
        });
        let ch = |s: u32| Arg::Int(((l >> s) & 255) as i64);
        vec![ch(0), ch(8), ch(16), ch(24)]
    }),
    ("hud.debug.looking", |c| {
        let name = c
            .client
            .as_ref()
            .and_then(|cl| cl.target)
            .map(|h| content().state_name(h.state))
            .unwrap_or_else(|| "-".into());
        vec![Arg::Text(name)]
    }),
];

const WORLD: &[DebugLine] = &[
    ("hud.debug.world", |c| {
        let Some(cl) = c.client.as_ref() else { return vec![] };
        vec![Arg::Int(cl.world.len() as i64), Arg::Int(cl.pending_meshes() as i64)]
    }),
    ("hud.debug.draws", |c| {
        let Some(cl) = c.client.as_ref() else { return vec![] };
        vec![Arg::Int(cl.render.drawn as i64), Arg::Int((cl.render.faces_drawn / 1000) as i64)]
    }),
    ("hud.debug.lod", |c| {
        let Some(cl) = c.client.as_ref() else { return vec![] };
        let (shown, made, quads, pending) = cl.lod_stats();
        vec![Arg::Int(shown as i64), Arg::Int(made as i64), Arg::Int((quads / 1000) as i64), Arg::Int(pending as i64)]
    }),
    ("hud.debug.entities", |c| {
        vec![
            Arg::Int(c.client.as_ref().map_or(0, |cl| cl.entities.len()) as i64),
            Arg::Int(c.client.as_ref().map_or(0, |cl| cl.particles.len()) as i64),
        ]
    }),
    ("hud.debug.load", |c| vec![Arg::Int(c.stats.load.0 as i64), Arg::Int(c.stats.load.1 as i64)]),
];

const MODS: &[DebugLine] = &[("", |_| vec![Arg::Text(mod_profile())])];

const SYSTEM: &[DebugLine] = &[
    ("hud.debug.gpu_name", |c| vec![Arg::Text(c.stats.gpu_name.clone())]),
    ("hud.debug.cpu_name", |c| vec![Arg::Text(c.stats.cpu_name.clone()), Arg::Int(c.stats.threads as i64)]),
    ("hud.debug.memory", |c| vec![Arg::Text(crate::diag::size(crate::diag::memory())), Arg::Text(crate::diag::size(c.stats.memory_total))]),
    ("hud.debug.window", |c| {
        vec![Arg::Int(c.stats.window.0 as i64), Arg::Int(c.stats.window.1 as i64), Arg::Float(c.stats.gui_scale as f64, 1)]
    }),
];

/// Milliseconds per server tick and calls per tick for each mod, averaged over the last second.
fn mod_profile() -> String {
    use std::sync::Mutex;
    static LAST: Mutex<Option<(std::time::Instant, String, crate::diag::ModWindow)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    let (at, text, window) =
        last.get_or_insert_with(|| (std::time::Instant::now() - std::time::Duration::from_secs(2), String::new(), Default::default()));
    if at.elapsed().as_secs_f32() < 1.0 {
        return text.clone();
    }
    let (mods, secs) = window.take();
    let ticks = (secs * crate::world::TPS as f64).max(1.0);
    let lines: Vec<String> = mods
        .iter()
        .map(|(id, ns, calls)| format!("{id}: {:.2} ms, {:.0} calls", *ns as f64 / 1e6 / ticks, *calls as f64 / ticks))
        .collect();
    *text = lines.join(
        "
",
    );
    *at = std::time::Instant::now();
    text.clone()
}

/// Engine-drawn nodes: crosshair, hotbar slots, menu slots.
#[allow(clippy::too_many_arguments)]
pub fn draw_custom(
    ren: &mut UiRenderer,
    look: &Look,
    textures: &GameTextures,
    client: Option<&Client>,
    tag: u32,
    data: u64,
    r: Rect,
    hovered: bool,
    icon: Option<u32>,
    scale: f32,
    text_unit: f32,
) {
    match tag {
        TAG_CROSSHAIR => loaded(ren, look, "crosshair", r, scale),
        TAG_HOTBAR => {
            let Some(client) = client else { return };
            let selected = client.selected as u64 == data;
            slot_background(ren, look, "hotbar", r, selected, scale);
            let stack = client
                .inventory
                .get(data as usize)
                .copied()
                .unwrap_or_default();
            stack_icon(ren, textures, r, stack, scale, text_unit);
        }
        TAG_ENTITY => {
            // Inset frame; the model itself is drawn in a later pass.
            loaded(ren, look, "well", r, scale);
        }
        modui::TAG_ICON => stack_icon(ren, textures, r, ItemStack::new(data as u32, 1), scale, text_unit),
        modui::TAG_CREATIVE => {
            slot_background(ren, look, "slot", r, false, scale);
            stack_icon(ren, textures, r, ItemStack::new(data as u32, 1), scale, text_unit);
            if hovered {
                loaded(
                    ren,
                    look,
                    "slot_highlight",
                    Rect { x: r.x + scale, y: r.y + scale, w: r.w - 2.0 * scale, h: r.h - 2.0 * scale },
                    scale,
                );
            }
        }
        TAG_SLOT => {
            let Some(client) = client else { return };
            slot_background(ren, look, "slot", r, false, scale);
            let stack = client
                .menu
                .as_ref()
                .and_then(|m| m.slots.get(data as usize))
                .copied()
                .unwrap_or_default();
            if let (Some(t), true) = (icon, stack.is_empty()) {
                let s = 16.0 * scale;
                ren.image(Rect { x: r.x + (r.w - s) / 2.0, y: r.y + (r.h - s) / 2.0, w: s, h: s }, t, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
            }
            stack_icon(ren, textures, r, stack, scale, text_unit);
            let dragged = client
                .drag
                .as_ref()
                .is_some_and(|d| d.1.contains(&(data as u32)));
            if hovered || dragged {
                loaded(
                    ren,
                    look,
                    "slot_highlight",
                    Rect { x: r.x + scale, y: r.y + scale, w: r.w - 2.0 * scale, h: r.h - 2.0 * scale },
                    scale,
                );
            }
        }
        _ => {}
    }
}

/// A stack in `r` (`UiRenderer::stack_icon`).
pub fn stack_icon(ren: &mut UiRenderer, textures: &GameTextures, r: Rect, stack: ItemStack, scale: f32, text_unit: f32) {
    ren.stack_icon(content(), textures, r, stack, scale, text_unit);
}

/// A slot of class `class` (`slot` in menus, `hotbar` on the HUD), with `hotbar_selected` around
/// it when selected.
pub fn slot_background(ren: &mut UiRenderer, look: &Look, class: &str, r: Rect, selected: bool, scale: f32) {
    loaded(ren, look, class, r, scale);
    if selected {
        loaded(ren, look, "hotbar_selected", Rect { x: r.x - scale, y: r.y - scale, w: r.w + 2.0 * scale, h: r.h + 2.0 * scale }, scale);
    }
}

/// A class's texture already loaded for engine-drawn nodes, else the missing-texture checkerboard.
fn loaded(ren: &mut UiRenderer, look: &Look, class: &str, r: Rect, scale: f32) {
    if !look
        .texture(class, "")
        .is_some_and(|t| ren.draw_loaded(&t, r, scale, 0xFFFFFFFF))
    {
        ren.checker(r, scale);
    }
}

/// What a stack is called: its own name, else its item's.
pub fn stack_name(lang: &Lang, input: Option<&crate::input::Input>, stack: weft_abi::ItemStack) -> Vec<text::Span> {
    match crate::stack::name(&stack) {
        Some(n) => text::rich(&text::escape(&n), lang, input),
        None => text::Name::Item(stack.item).spans(lang, input),
    }
}

/// The name, then the `<name key>.tooltip` translation in gray (details usually behind
/// `{on_key:core:sneak}`) and the mods' own lines under it, then durability for damaged tools.
pub fn item_tooltip(lang: &Lang, input: &crate::input::Input, stack: weft_abi::ItemStack) -> Vec<text::Span> {
    let gray = lang.color("color.tooltip_dim").map(|c| c >> 8);
    let name = text::Name::Item(stack.item);
    let mut out = stack_name(lang, Some(input), stack);
    let line = |template: &str, args: &[text::Arg], own: Option<u32>, out: &mut Vec<text::Span>| {
        let mut cx = text::Ctx::new(lang, Some(input));
        cx.subject = Some(text::Subject { stack, own });
        let break_at = out.len();
        out.push(text::Span::text("\n", None));
        let start = out.len();
        text::format(template, args, &cx, out);
        for s in &mut out[start..] {
            s.color = s.color.or(gray);
        }
        // A line hidden as a whole leaves no empty line behind.
        let first = out.get(start).map_or(Vec::new(), |s| s.when.clone());
        let common: Vec<text::Cond> = first
            .into_iter()
            .filter(|c| out[start..].iter().all(|s| s.when.contains(c)))
            .collect();
        out[break_at].when = common;
    };
    // With additions by mods, one group per mod in load order: `<key>.<mod id>`.
    let mut mods: Vec<_> = content().mods.iter().collect();
    mods.sort_by_key(|m| m.order);
    let group = |key: &str, own: Option<u32>, out: &mut Vec<text::Span>| {
        if lang.has(key) {
            line(lang.get(key), &[], own, out);
        }
        for m in &mods {
            let extra = format!("{key}.{}", m.id);
            if lang.has(&extra) {
                line(lang.get(&extra), &[], own, out);
            }
        }
    };
    if let Some(key) = name.key(lang).map(|k| format!("{k}.tooltip")) {
        group(&key, None, &mut out);
    }
    // What a tag or component says of every item with it, its numbers from this one.
    let c = content();
    for comp in c.components.of(crate::components::ITEM, stack.item) {
        let key = text::lang_key("tagged", &c.components.names[comp as usize]);
        group(&key, Some(comp), &mut out);
    }
    let max = c.durability(&stack);
    if max > 0 {
        let damage = crate::stack::damage(&stack);
        line(lang.get("tooltip.durability"), &[text::Arg::Int((max - damage.min(max)) as i64), text::Arg::Int(max as i64)], None, &mut out);
    }
    out
}

type ModPart = (weft_abi::ModId, weft_abi::Callback);

/// Mods' F3 panels in group order (after the engine's): title key, right side, part, element name.
pub fn mod_debug_groups() -> Vec<(&'static str, bool, ModPart, &'static str)> {
    use weft_abi::hud_anchor as a;
    let mut v: Vec<_> = content()
        .game
        .huds
        .iter()
        .filter(|h| h.anchor == a::DEBUG_LEFT || h.anchor == a::DEBUG_RIGHT)
        .collect();
    v.sort_by_key(|h| h.order);
    v.into_iter()
        .take(9 - DEBUG_GROUPS.len())
        .map(|h| (h.title.as_str(), h.anchor == a::DEBUG_RIGHT, (h.owner, h.cb), h.name.as_str()))
        .collect()
}

fn panel_title(ui: &mut Ui, key: String) {
    let t = ui.text_bound("debug_title", Box::new(move |_: &Ctx| (key.clone(), vec![])));
    if let Some(t) = ui.text_mut(t) {
        t.wrap = false;
    }
}

/// A debug overlay group's panels with the lines `shown` says, each panel dark with a title.
fn debug_group(ui: &mut Ui, i: usize, align: Align, shown: &dyn Fn(&str) -> bool) {
    let panels = DEBUG_GROUPS[i].2;
    // Side by side where they fit (GUI scale 3 in Russian does not), else stacked.
    let style = if ui.size.x >= 640.0 { Style::row().gap(2.0).align(Align::Start) } else { Style::default().gap(2.0).align(align) };
    ui.container(style, |ui| {
        for (title, lines) in panels.iter() {
            if !lines.iter().any(|l| shown(line_id(l.0))) {
                continue;
            }
            ui.container(Style::default().pad(2.0).gap(0.0).align(align), |ui| {
                ui.nodes.last_mut().unwrap().kind = Kind::Container { class: Some("debug_panel".into()) };
                panel_title(ui, title.to_string());
                for (key, f) in lines.iter().filter(|l| shown(line_id(l.0))) {
                    let key = key.to_string();
                    let n = ui.text_bound("debug", Box::new(move |c: &Ctx| (key.clone(), f(c))));
                    if let Some(t) = ui.text_mut(n) {
                        t.wrap = false;
                        t.mono = true;
                    }
                }
            });
        }
    });
}

/// A mod's F3 panel.
fn debug_mod_panel(ui: &mut Ui, title: &str, part: ModPart, align: Align, draw: &dyn Fn(&mut Ui, ModPart)) {
    ui.container(Style::default().pad(2.0).gap(0.0).align(align), |ui| {
        ui.nodes.last_mut().unwrap().kind = Kind::Container { class: Some("debug_panel".into()) };
        panel_title(ui, title.to_string());
        let first = ui.nodes.len();
        draw(ui, part);
        // The engine's panel style for every line: as wide as it needs, digits in cells.
        for n in &mut ui.nodes[first..] {
            if let Kind::Text(t) = &mut n.kind {
                t.wrap = false;
                t.mono = true;
            }
        }
    });
}

/// The help panel: the F3 combinations.
fn debug_help(ui: &mut Ui, groups: u32, align: Align, mods: &[(&'static str, bool, ModPart, &'static str)]) {
    ui.container(Style::default().pad(2.0).gap(0.0).align(align), |ui| {
        ui.nodes.last_mut().unwrap().kind = Kind::Container { class: Some("debug_panel".into()) };
        panel_title(ui, "hud.debug.group.help".into());
        let titles = DEBUG_GROUPS
            .iter()
            .map(|g| g.0)
            .chain(mods.iter().map(|m| m.0));
        for (i, title) in titles.enumerate() {
            let on = groups & (1 << i) != 0;
            let state = if on { "hud.debug.shown" } else { "hud.debug.hidden" };
            let n = ui.text_bound(
                "debug",
                Box::new(move |_: &Ctx| {
                    ("hud.debug.toggle".into(), vec![Arg::Int(i as i64 + 1), Arg::Key(title.to_string()), Arg::Key(state.into())])
                }),
            );
            if let Some(t) = ui.text_mut(n) {
                t.wrap = false;
            }
        }
        for key in ["hud.debug.options", "hud.debug.gamemode", "hud.debug.cubes", "hud.debug.hitboxes"] {
            let n = ui.text_bound("debug", Box::new(move |_: &Ctx| (key.into(), vec![])));
            if let Some(t) = ui.text_mut(n) {
                t.wrap = false;
            }
        }
    });
}
