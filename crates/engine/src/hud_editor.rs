//! Screens that arrange the overlay: the HUD editor, where the elements (`hud::Hud`) are dragged
//! around the live picture, and the debug options (F3+F6), which say when each F3 line shows.

use glam::Vec2;

use crate::app::{Ctx, Request};
use crate::hud::{self, Edit, Hud};
use crate::screens::{BuildCx, Screen, button, frame, label};
use crate::ui::text::Arg;
use crate::ui::{Justify, Kind, Style, Ui};

/// Grid step in GUI pixels, and how near the middle of the window an element snaps to it.
const GRID: f32 = 4.0;
const SNAP: f32 = 3.0;

enum Pending {
    ToggleHidden,
    Reset,
    ResetAll,
}

#[derive(Default)]
pub struct HudEditor {
    selected: Option<String>,
    grid: bool,
    labels: bool,
    debug: bool,
    /// The element being dragged and where the cursor holds it, from its corner.
    drag: Option<(String, Vec2)>,
    /// Where the tool box stands (its top middle) once moved, and the cursor's hold on it.
    tools: Option<Vec2>,
    tools_drag: Option<Vec2>,
    pending: Option<Pending>,
}

impl HudEditor {
    /// Where element `name` goes for its corner at `corner`: tied to the nearest of the window's
    /// nine points, so it keeps its place when the window changes size.
    fn place(&self, hud: &mut Hud, name: &str, corner: Vec2, size: Vec2) {
        let (w, h) = (size.x, size.y);
        let window = hud.ui.size;
        let third = |c: f32, all: f32| {
            if c < all / 3.0 {
                0.0
            } else if c < all * 2.0 / 3.0 {
                0.5
            } else {
                1.0
            }
        };
        let (ax, ay) = (third(corner.x + w / 2.0, window.x), third(corner.y + h / 2.0, window.y));
        let mut x = corner.x + ax * w - ax * window.x;
        let mut y = corner.y + ay * h - ay * window.y;
        if self.grid {
            (x, y) = ((x / GRID).round() * GRID, (y / GRID).round() * GRID);
        }
        // The middle of the window is where things usually line up.
        if ax == 0.5 && x.abs() <= SNAP {
            x = 0.0;
        }
        if ay == 0.5 && y.abs() <= SNAP {
            y = 0.0;
        }
        hud.place(name, [ax, ay, x, y]);
    }
}

impl Screen for HudEditor {
    fn name(&self) -> &str {
        "core:hud_editor"
    }

    fn pauses(&self) -> bool {
        true
    }

    fn pointer(&mut self, cursor: Vec2, ui: &mut Ui, ctx: &mut Ctx, hud: &mut Hud) -> bool {
        hud.set_edit(Some(Edit { selected: self.selected.clone(), labels: self.labels, debug: self.debug }));
        match (self.pending.take(), self.selected.clone()) {
            (Some(Pending::ToggleHidden), Some(name)) => hud.toggle_hidden(&name),
            (Some(Pending::Reset), Some(name)) => hud.reset(&name),
            (Some(Pending::ResetAll), _) => hud.reset_all(),
            _ => {}
        }
        let (left, right) = (ctx.input.pressed_button(0), ctx.input.pressed_button(1));
        let held = ctx.input.held_button(0);
        if let Some((name, grab)) = self.drag.clone() {
            if held {
                if let Some(r) = hud.element_rect(&name) {
                    self.place(hud, &name, cursor - grab, Vec2::new(r.w, r.h));
                }
            } else {
                self.drag = None;
                hud.save();
            }
            return true;
        }
        if let Some(grab) = self.tools_drag {
            if held {
                let at = cursor - grab;
                self.tools = Some(at);
                if let Some(tools) = ui.find("hud_tools") {
                    ui.nodes[tools].style.at = Some([at.x, at.y, 0.5, 0.0]);
                    ui.dirty = true;
                }
            } else {
                self.tools_drag = None;
            }
            return true;
        }
        if ui
            .find("hud_tools")
            .is_some_and(|t| ui.nodes[t].rect.contains(cursor))
        {
            // Its empty parts move it; its buttons work as usual.
            if left && ui.hit(cursor).is_none() {
                let at = self.tools.unwrap_or(Vec2::new(ui.size.x / 2.0, 8.0));
                self.tools_drag = Some(cursor - at);
                return true;
            }
            return false;
        }
        if left || right {
            let found = hud.element_at(cursor);
            if found != self.selected {
                self.selected = found.clone();
                ctx.requests.push(Request::Rebuild);
            }
            if let Some(name) = found {
                if right {
                    hud.toggle_hidden(&name);
                } else if let Some(r) = hud.element_rect(&name) {
                    self.drag = Some((name, cursor - Vec2::new(r.x, r.y)));
                }
            }
            return true;
        }
        false
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        ui.root_style(Style::default().stretch());
        let at = self.tools.unwrap_or(Vec2::new(cx.size.x / 2.0, 8.0));
        let mut style = Style::default().pad(6.0).gap(4.0).w(330.0).stretch();
        style.at = Some([at.x, at.y, 0.5, 0.0]);
        let (grid, labels, debug) = (self.grid, self.labels, self.debug);
        let selected = self.selected.clone();
        let tools = ui.panel("panel", style, |ui| {
            let help = ui.text(cx.t("hud.edit.help", &[]), "dim");
            ui.nodes[help].style.self_align = Some(crate::ui::Align::Center);
            let row = || Style::row().gap(4.0).justify(Justify::Center);
            ui.container(row(), |ui| {
                let toggle = |ui: &mut Ui, key: &str, on: bool, set: fn(&mut HudEditor)| {
                    let b = button(ui, cx, key, &[], 104.0, move |c| {
                        if let Some(s) = c.screen::<HudEditor>() {
                            set(s);
                        }
                        c.requests.push(Request::Rebuild);
                    });
                    if let Kind::Button { selected, .. } = &mut ui.nodes[b].kind {
                        *selected = on;
                    }
                };
                toggle(ui, "hud.edit.grid", grid, |s| s.grid = !s.grid);
                toggle(ui, "hud.edit.labels", labels, |s| s.labels = !s.labels);
                toggle(ui, "hud.edit.debug", debug, |s| s.debug = !s.debug);
            });
            if let Some(name) = &selected {
                let t = ui.text(hud::element_title(cx.lang, name), "default");
                ui.nodes[t].style.self_align = Some(crate::ui::Align::Center);
                ui.container(row(), |ui| {
                    let act = |ui: &mut Ui, key: &str, what: fn() -> Pending| {
                        button(ui, cx, key, &[], 158.0, move |c| {
                            if let Some(s) = c.screen::<HudEditor>() {
                                s.pending = Some(what());
                            }
                        });
                    };
                    act(ui, "hud.edit.hide", || Pending::ToggleHidden);
                    act(ui, "hud.edit.reset", || Pending::Reset);
                });
            }
            ui.container(row(), |ui| {
                button(ui, cx, "hud.edit.lines", &[], 104.0, |c| {
                    c.requests
                        .push(Request::Screen("core:debug_options".into()))
                });
                button(ui, cx, "hud.edit.reset_all", &[], 104.0, |c| {
                    if let Some(s) = c.screen::<HudEditor>() {
                        s.pending = Some(Pending::ResetAll);
                        s.selected = None;
                    }
                    c.requests.push(Request::Rebuild);
                });
                button(ui, cx, "menu.done", &[], 104.0, |c| c.requests.push(Request::Close));
            });
        });
        ui.set_id(tools, "hud_tools");
    }
}

/// When each line of the F3 overlay shows: with F3, always, or never. A line set to always stays
/// on the HUD without F3 (a frame counter, say), where the editor can put it.
pub struct DebugOptions;

impl Screen for DebugOptions {
    fn name(&self) -> &str {
        "core:debug_options"
    }

    fn pauses(&self) -> bool {
        true
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let lines = hud::debug_lines();
        let groups = crate::settings::debug_groups();
        frame(ui, cx, "hud.options.title", |ui| {
            let list = ui.panel(
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
                    let row = |ui: &mut Ui, id: String, name: Vec<crate::ui::text::Span>| {
                        let state = lines.get(&id).copied().unwrap_or_default();
                        ui.container(Style::row().gap(6.0).self_align(crate::ui::Align::Stretch), |ui| {
                            let n = ui.text(name, "default");
                            ui.nodes[n].style.grow = 1.0;
                            let shown = id.clone();
                            let b = button(ui, cx, state.key(), &[], 90.0, move |c| {
                                hud::set_debug_line(&id, state.next());
                                c.requests.push(Request::Rebuild);
                            });
                            ui.set_id(b, &format!("line:{shown}"));
                        });
                    };
                    let mods = hud::mod_debug_groups();
                    let group = |ui: &mut Ui, i: usize, title: &str| {
                        let on = groups & (1 << i) != 0;
                        let key = if on { "hud.debug.shown" } else { "hud.debug.hidden" };
                        ui.add(Kind::Container { class: None }, Style::default().h(3.0));
                        ui.container(Style::row().gap(6.0).self_align(crate::ui::Align::Stretch), |ui| {
                            let n = ui.text(cx.t(title, &[]), "default");
                            ui.nodes[n].style.grow = 1.0;
                            let b = button(ui, cx, "hud.options.group", &[Arg::Int(i as i64 + 1), Arg::Key(key.into())], 90.0, move |c| {
                                let groups = crate::settings::debug_groups();
                                crate::settings::set("debug_groups", (groups ^ (1 << i)) as f64);
                                c.requests.push(Request::Rebuild);
                            });
                            ui.set_id(b, &format!("group:{i}"));
                        });
                    };
                    for (i, (title, _, panels)) in hud::DEBUG_GROUPS.iter().enumerate() {
                        group(ui, i, title);
                        for (_, panel) in panels.iter() {
                            for (key, _) in panel.iter() {
                                let id = hud::line_id(key).to_string();
                                let name = cx.t(&format!("hud.options.line.{id}"), &[]);
                                row(ui, id, name);
                            }
                        }
                    }
                    for (k, m) in mods.iter().enumerate() {
                        group(ui, hud::DEBUG_GROUPS.len() + k, m.0);
                        row(ui, m.3.to_string(), cx.t(m.0, &[]));
                    }
                    label(ui, cx, "hud.debug.group.help", &[], "default");
                    row(ui, "help".to_string(), cx.t("hud.options.line.help", &[]));
                },
            );
            ui.set_id(list, "debug_options");
            ui.container(Style::row().gap(6.0), |ui| {
                button(ui, cx, "hud.options.reset", &[], 120.0, |c| {
                    hud::reset_debug_lines();
                    c.requests.push(Request::Rebuild);
                });
                button(ui, cx, "menu.done", &[], 120.0, |c| c.requests.push(Request::Close));
            });
        });
    }
}
