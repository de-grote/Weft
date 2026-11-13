//! The mods screen: what is installed (every loaded mod, its settings, turning it off, what came
//! from the catalog) and the catalog to browse (`mod_index`), in two tabs. A mod is a tile with
//! what it is and what can be done with it at a click; the one picked shows the rest beside.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use crate::app::{Ctx, Request};
use crate::mod_index::{self, IndexMod, Status};
use crate::screens::{BuildCx, ConfigScreen, Screen, Shown, close_button, label, mod_name};
use crate::ui::text::{self, Arg, Span};
use crate::ui::{Align, Kind, NodeId, Scrollbar, Style, Ui};

const LIST_W: f32 = 300.0;
const TILE_H: f32 = 38.0;
const ICON: f32 = 30.0;
const BIG_ICON: f32 = 40.0;

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    Installed,
    Browse,
}

#[derive(Default)]
pub struct ModsScreen {
    tab: Rc<Cell<Tab>>,
    /// The mod picked in each tab, by id.
    picked: Rc<RefCell<[String; 2]>>,
    unverified: Rc<Cell<bool>>,
    query: Rc<RefCell<String>>,
    seen: u64,
}

/// Where a mod stands, for its badges and what can be done.
#[derive(Clone, PartialEq)]
enum State {
    /// Loaded, from the game or a mod folder: nothing to download.
    Local,
    /// A zip from the catalog: the version it is, the newer one that fits if there is one.
    Installed { have: String, newer: Option<String> },
    /// In the catalog and fits this game.
    Get { version: String },
    /// In the catalog, no build for this game.
    Incompatible,
}

/// One tile's worth of a mod.
struct Row {
    id: String,
    name: Vec<Span>,
    author: String,
    icon: Option<String>,
    /// From the index: whether its build was read by a maintainer.
    verified: Option<bool>,
    state: State,
    /// A zip in the player's `mods`.
    removable: bool,
    /// Loaded or turned off by the player: its light.
    toggle: bool,
    entry: Option<IndexMod>,
}

fn version_of(m: &IndexMod, sha: &str) -> String {
    m.builds
        .iter()
        .find(|b| b.sha256 == sha)
        .map_or_else(|| "?".into(), |b| b.version.clone())
}

impl ModsScreen {
    fn rows(&self, cx: &BuildCx, tab: Tab, dir: &std::path::Path, cache: &std::path::Path) -> Vec<Row> {
        let index = mod_index::index().unwrap_or_default();
        let have = mod_index::installed(dir);
        let entry = |id: &str| index.iter().find(|m| m.id == id).cloned();
        let state_of = |m: &IndexMod| match (have.iter().find(|h| h.0 == m.id), m.fitting()) {
            (Some((_, sha)), fit) => {
                State::Installed { have: version_of(m, sha), newer: fit.filter(|b| &b.sha256 != sha).map(|b| b.version.clone()) }
            }
            (None, Some(b)) => State::Get { version: b.version.clone() },
            (None, None) => State::Incompatible,
        };
        let verified = |m: &IndexMod| {
            let sha = have.iter().find(|h| h.0 == m.id).map(|h| h.1.clone());
            sha.and_then(|s| m.builds.iter().find(|b| b.sha256 == s))
                .or(m.fitting())
                .map(|b| b.verified)
        };
        let removable = |id: &str| have.iter().any(|h| h.0 == id);
        match tab {
            Tab::Installed => {
                let c = crate::content::content();
                let mut loaded: Vec<&crate::content::ModMeta> = c.mods.iter().collect();
                loaded.sort_by_key(|m| m.order);
                loaded.extend(crate::modloader::turned_off());
                let mut rows: Vec<Row> = loaded
                    .iter()
                    .map(|m| {
                        let e = entry(&m.id);
                        let key = format!("mod.{}.authors", m.id);
                        let author = if cx.lang.has(&key) {
                            cx.lang.get(&key).to_string()
                        } else {
                            e.as_ref().map(|e| e.author.clone()).unwrap_or_default()
                        };
                        let icon = format!("{}:gui/icon", m.id);
                        Row {
                            id: m.id.clone(),
                            name: mod_name(cx.lang, &m.id),
                            author,
                            icon: crate::assets::texture_size(&icon).map(|_| icon),
                            verified: e.as_ref().and_then(verified),
                            state: e
                                .as_ref()
                                .filter(|_| removable(&m.id))
                                .map_or(State::Local, state_of),
                            removable: removable(&m.id),
                            toggle: true,
                            entry: e,
                        }
                    })
                    .collect();
                // Installed from the catalog since the start: they load on the next.
                for (id, _) in &have {
                    if rows.iter().any(|r| &r.id == id) {
                        continue;
                    }
                    let e = entry(id);
                    rows.push(Row {
                        id: id.clone(),
                        name: text::plain(e.as_ref().map_or(id.as_str(), |e| e.name.as_str())),
                        author: e.as_ref().map(|e| e.author.clone()).unwrap_or_default(),
                        icon: e.as_ref().and_then(|e| mod_index::icon(cache, e)),
                        verified: e.as_ref().and_then(verified),
                        state: e.as_ref().map_or(State::Local, state_of),
                        removable: true,
                        toggle: true,
                        entry: e,
                    });
                }
                rows
            }
            Tab::Browse => {
                let query = self.query.borrow().to_lowercase();
                index
                    .iter()
                    .filter(|m| self.unverified.get() || m.builds.iter().any(|b| b.verified))
                    .filter(|m| {
                        query.is_empty()
                            || [&m.name, &m.summary, &m.author, &m.id]
                                .iter()
                                .any(|t| t.to_lowercase().contains(&query))
                    })
                    .map(|m| Row {
                        id: m.id.clone(),
                        name: text::plain(&m.name),
                        author: m.author.clone(),
                        icon: mod_index::icon(cache, m),
                        verified: verified(m),
                        state: state_of(m),
                        removable: removable(&m.id),
                        toggle: false,
                        entry: Some(m.clone()),
                    })
                    .collect()
            }
        }
    }
}

/// A mod's picture, or its first letter on a color of its own.
fn mod_picture(ui: &mut Ui, icon: &Option<String>, name: &[Span], id: &str, side: f32) {
    if let Some(t) = icon {
        ui.add(Kind::Image { texture: t.clone(), color: 0xFFFFFFFF }, Style::default().size(side, side));
        return;
    }
    const COLORS: [u32; 8] = [0x8C6A3CFF, 0x5A7A5AFF, 0x6A5A8CFF, 0x3C6A8CFF, 0x8C4A4AFF, 0x7A7A3CFF, 0x4A7A7AFF, 0x7A4A7AFF];
    let mut style = Style::default().size(side, side).center();
    style.fill = Some(COLORS[crate::registry::name_hash(id) as usize % COLORS.len()]);
    ui.container(style, |ui| {
        let letter: String = text::plain_text(name)
            .chars()
            .find(|c| c.is_alphanumeric())
            .unwrap_or('?')
            .to_uppercase()
            .collect();
        let t = ui.text(text::plain(&letter), "badge");
        if let Some(t) = ui.text_mut(t) {
            t.scale = (side / 14.0).floor().max(1.0);
            t.wrap = false;
        }
    });
}

/// A badge: words on a colored plate, with a tooltip if it says more.
pub(crate) fn badge(ui: &mut Ui, cx: &BuildCx, class: &str, key: &str, args: &[Arg], tip: Option<&str>) -> NodeId {
    let b = ui.panel(class, Style::row().pad_xy(3.0, 1.0), |ui| {
        let t = label(ui, cx, key, args, "badge");
        if let Some(t) = ui.text_mut(t) {
            t.wrap = false;
        }
    });
    if let Some(tip) = tip {
        ui.nodes[b].tooltip = Some(cx.t(tip, &[]));
    }
    b
}

/// An icon button with its words in a tooltip.
pub(crate) fn icon_button(
    ui: &mut Ui,
    cx: &BuildCx,
    class: &str,
    icon: &str,
    tip: &str,
    args: &[Arg],
    action: impl Fn(&mut Ctx) + 'static,
) -> NodeId {
    let b = ui.button(class, Style::default().size(17.0, 17.0).center(), Rc::new(action), |ui| {
        ui.add(Kind::Image { texture: format!("core:gui/{icon}"), color: 0xFFFFFFFF }, Style::default().size(9.0, 9.0));
    });
    ui.nodes[b].tooltip = Some(cx.t(tip, args));
    b
}

fn set_enabled(ui: &mut Ui, b: NodeId, on: bool) {
    if let Kind::Button { enabled, .. } = &mut ui.nodes[b].kind {
        *enabled = on;
    }
}

pub(crate) fn separator(ui: &mut Ui) {
    ui.add(Kind::Container { class: Some("separator".into()) }, Style::default().h(1.0).stretch());
}

/// What can be done to a mod from its tile: get it, update it, take it away.
fn actions(ui: &mut Ui, cx: &BuildCx, r: &Row, dir: &PathBuf, busy: bool) {
    match &r.state {
        State::Get { .. } => {
            let (id, to) = (r.id.clone(), dir.clone());
            let b = icon_button(ui, cx, "button_install", "icon_download", "mods.install", &[], move |c| {
                request_install(c, &id, &to);
            });
            set_enabled(ui, b, !busy);
        }
        State::Installed { newer: Some(v), .. } => {
            let (id, to) = (r.id.clone(), dir.clone());
            let b = icon_button(ui, cx, "button_update", "icon_update", "mods.update_to", &[Arg::Text(v.clone())], move |c| {
                request_install(c, &id, &to);
            });
            set_enabled(ui, b, !busy);
        }
        _ => {}
    }
    if r.removable {
        let (id, to) = (r.id.clone(), dir.clone());
        let b = icon_button(ui, cx, "button", "icon_trash", "mods.remove", &[], move |c| {
            mod_index::remove(&id, &to);
            c.requests.push(Request::Rebuild);
        });
        set_enabled(ui, b, !busy);
    }
}

fn request_install(c: &mut Ctx, id: &str, dir: &std::path::Path) {
    let plan = mod_index::install_plan(id, dir);
    c.requests
        .push(Request::Open(Box::new(InstallMods { plan, dir: dir.to_path_buf() })));
}

pub struct DisabledMods;

impl Screen for DisabledMods {
    fn name(&self) -> &str {
        "core:disabled_mods"
    }

    fn draws_close(&self) -> bool {
        true
    }

    fn overlays(&self) -> bool {
        true
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        ui.root_style(Style::default().stretch());
        ui.panel("overlay", Style::default().grow().center().pad(12.0), |ui| {
            ui.panel(
                "panel",
                Style::default()
                    .w(320.0)
                    .pad(12.0)
                    .gap(10.0)
                    .align(Align::Center),
                |ui| {
                    crate::screens::title(ui, cx, "mods.disabled_title");
                    label(ui, cx, "mods.disabled_hint", &[], "default");
                    let style = Style { max_h: (cx.size.y - 160.0).max(40.0), ..Style::default().w(296.0).pad(6.0).gap(4.0).scroll() };
                    ui.panel("well", style, |ui| {
                        for id in crate::modloader::newly_disabled() {
                            ui.text(mod_name(cx.lang, id), "default");
                        }
                    });
                    ui.container(Style::row().gap(8.0), |ui| {
                        crate::screens::button(ui, cx, "mods.title", &[], 120.0, |c| {
                            c.requests.push(Request::Close);
                            c.requests.push(Request::Screen("core:mods".into()));
                        });
                        crate::screens::button(ui, cx, "menu.done", &[], 120.0, |c| c.requests.push(Request::Close));
                    });
                },
            );
        });
    }
}

struct InstallMods {
    plan: Result<Vec<IndexMod>, String>,
    dir: PathBuf,
}

impl Screen for InstallMods {
    fn name(&self) -> &str {
        "core:install_mods"
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        crate::screens::frame(ui, cx, "mods.install_confirm", |ui| {
            match &self.plan {
                Ok(plan) => {
                    label(ui, cx, "mods.install_count", &[Arg::Int(plan.len() as i64)], "default");
                    let style = Style { max_h: (cx.size.y - 150.0).max(60.0), ..Style::default().w(300.0).pad(6.0).gap(4.0).scroll() };
                    ui.panel("well", style, |ui| {
                        for m in plan {
                            ui.container(Style::row().gap(4.0).align(Align::Center), |ui| {
                                ui.text(text::plain(&m.name), "default");
                                let b = m.fitting().unwrap();
                                ui.text(text::plain(&b.version), "dim");
                                badge(
                                    ui,
                                    cx,
                                    if b.verified { "badge_verified" } else { "badge_unverified" },
                                    if b.verified { "mods.verified" } else { "mods.unverified" },
                                    &[],
                                    None,
                                );
                            });
                        }
                    });
                    let (plan, dir) = (plan.clone(), self.dir.clone());
                    let b = crate::screens::button(ui, cx, "mods.install", &[], 120.0, move |c| {
                        mod_index::install(plan.clone(), dir.clone());
                        c.requests.push(Request::Close);
                        c.requests.push(Request::Rebuild);
                    });
                    ui.set_id(b, "mods_install_confirm");
                }
                Err(e) => {
                    label(ui, cx, "mods.failed", &[Arg::Text(e.clone())], "warning");
                }
            }
            crate::screens::button(ui, cx, "menu.cancel", &[], 120.0, |c| c.requests.push(Request::Close));
        });
    }
}

/// The badges that say where a mod stands.
fn badges(ui: &mut Ui, cx: &BuildCx, r: &Row) {
    if crate::modloader::load_error(&r.id).is_some() {
        badge(ui, cx, "badge_incompatible", "mods.incompatible", &[], None);
        return;
    }
    match r.verified {
        Some(true) => {
            badge(ui, cx, "badge_verified", "mods.verified", &[], Some("mods.safe"));
        }
        Some(false) => {
            badge(ui, cx, "badge_unverified", "mods.unverified", &[], Some("mods.unsafe"));
        }
        None => {}
    }
    match &r.state {
        State::Installed { have, newer: Some(v) } => {
            badge(ui, cx, "badge_update", "mods.update_badge", &[Arg::Text(have.clone()), Arg::Text(v.clone())], None);
        }
        State::Installed { .. } => {
            badge(ui, cx, "badge_installed", "mods.installed", &[], None);
        }
        State::Incompatible => {
            badge(ui, cx, "badge_incompatible", "mods.incompatible", &[], Some("mods.incompatible_tip"));
        }
        _ => {}
    }
}

/// A name cut to fit a tile, the whole of it in the tooltip.
fn short(name: &[Span], most: usize) -> (Vec<Span>, bool) {
    let all = text::plain_text(name);
    if all.chars().count() <= most {
        return (name.to_vec(), false);
    }
    let cut: String = all.chars().take(most - 1).collect();
    (text::plain(&format!("{}…", cut.trim_end())), true)
}

impl Screen for ModsScreen {
    fn name(&self) -> &str {
        "core:mods"
    }

    fn draws_close(&self) -> bool {
        true
    }

    /// Built again when the background work moves on.
    fn pointer(&mut self, _cursor: glam::Vec2, _ui: &mut Ui, ctx: &mut Ctx, _hud: &mut crate::hud::Hud) -> bool {
        if mod_index::generation() != self.seen {
            ctx.requests.push(Request::Rebuild);
        }
        false
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        self.seen = mod_index::generation();
        let dir = cx.settings.run_dir.join("mods");
        let cache = cx.settings.run_dir.join("cache");
        if mod_index::index().is_none() && matches!(mod_index::status(), Status::Idle) {
            mod_index::refresh(&cache);
        }
        let status = mod_index::status();
        let busy = matches!(status, Status::Busy(_) | Status::Loading);
        let tab = self.tab.get();
        let rows = self.rows(cx, tab, &dir, &cache);
        let installed_count = self.rows(cx, Tab::Installed, &dir, &cache).len();
        let browse_count = if tab == Tab::Browse { rows.len() } else { self.rows(cx, Tab::Browse, &dir, &cache).len() };
        let slot = tab as usize;
        let picked_id = self.picked.borrow()[slot].clone();
        let sel = rows
            .iter()
            .position(|r| r.id == picked_id)
            .or_else(|| {
                rows.iter()
                    .position(|r| crate::modloader::load_error(&r.id).is_some())
            })
            .unwrap_or(0);
        let tall = (cx.size.y - 110.0).clamp(120.0, 330.0);
        let wide = (cx.size.x - 40.0).clamp(480.0, 680.0);
        let bg = if cx.in_game { "overlay" } else { "background" };
        ui.root_style(Style::default().stretch());
        ui.panel(bg, Style::default().grow().center().pad(10.0), |ui| {
            ui.panel("panel", Style::default().w(wide).pad(10.0).gap(6.0), |ui| {
                // Title and tabs, the search and the tools.
                ui.container(
                    Style::row()
                        .gap(6.0)
                        .align(Align::End)
                        .self_align(Align::Stretch),
                    |ui| {
                        ui.container(Style::default().gap(4.0), |ui| {
                            let t = label(ui, cx, "mods.title", &[], "title");
                            if let Some(t) = ui.text_mut(t) {
                                t.scale = 2.0;
                                t.wrap = false;
                            }
                            ui.container(Style::row().gap(2.0), |ui| {
                                for (t, key, n) in [
                                    (Tab::Installed, "mods.tab_installed", installed_count),
                                    (Tab::Browse, "mods.tab_browse", browse_count),
                                ] {
                                    let me = self.tab.clone();
                                    let b = ui.button(
                                        "tab",
                                        Style::row().gap(4.0).pad_xy(6.0, 3.0),
                                        Rc::new(move |c: &mut Ctx| {
                                            me.set(t);
                                            c.requests.push(Request::Rebuild);
                                        }),
                                        |ui| {
                                            label(ui, cx, key, &[], if tab == t { "tab_selected" } else { "tab" });
                                            ui.text(text::plain(&n.to_string()), "count");
                                        },
                                    );
                                    if let Kind::Button { selected, .. } = &mut ui.nodes[b].kind {
                                        *selected = tab == t;
                                    }
                                    ui.set_id(b, if t == Tab::Installed { "tab_installed" } else { "tab_browse" });
                                }
                            });
                        });
                        ui.spacer();
                        if tab == Tab::Browse {
                            ui.container(Style::row().gap(3.0).align(Align::Center), |ui| {
                                ui.add(
                                    Kind::Image { texture: "core:gui/icon_search".into(), color: 0xFFFFFFFF },
                                    Style::default().size(9.0, 9.0),
                                );
                                let query = self.query.clone();
                                let input = ui.add(crate::ui::Kind::input(&self.query.borrow(), 40), Style::default().w(130.0).h(14.0));
                                if let Kind::Input { on_change, .. } = &mut ui.nodes[input].kind {
                                    *on_change = Some(Rc::new(move |c: &mut Ctx, v: &str| {
                                        *query.borrow_mut() = v.to_string();
                                        c.requests.push(Request::Rebuild);
                                    }));
                                }
                                ui.set_id(input, "mods_search");
                                ui.nodes[input].tooltip = Some(cx.t("mods.search", &[]));
                            });
                            let all = self.unverified.clone();
                            let (class, tip) = if self.unverified.get() {
                                ("button", "mods.showing_all")
                            } else {
                                ("button_install", "mods.showing_verified")
                            };
                            let hidden = mod_index::index()
                                .unwrap_or_default()
                                .iter()
                                .filter(|m| !m.builds.iter().any(|b| b.verified))
                                .count();
                            let filter = icon_button(ui, cx, class, "icon_verified", tip, &[Arg::Int(hidden as i64)], move |c| {
                                all.set(!all.get());
                                c.requests.push(Request::Rebuild);
                            });
                            if let Kind::Button { selected, .. } = &mut ui.nodes[filter].kind {
                                *selected = !self.unverified.get();
                            }
                            ui.set_id(filter, "mods_verified_filter");
                            let to = cache.clone();
                            let b = icon_button(ui, cx, "button", "icon_refresh", "mods.refresh", &[], move |c| {
                                mod_index::refresh(&to);
                                c.requests.push(Request::Rebuild);
                            });
                            set_enabled(ui, b, !busy);
                        }
                        let folder = dir.clone();
                        icon_button(ui, cx, "button", "icon_folder", "mods.folder", &[], move |_| crate::packs::open_folder(&folder));
                        close_button(ui, 15.0);
                    },
                );
                // The list's scroll bar is in the gap beside it, with room on both sides.
                let gap = Scrollbar::HANDLE[0] + Scrollbar::OUTSIDE_GAP * 2.0;
                ui.container(Style::row().gap(gap), |ui| {
                    // The tiles.
                    let list = ui.panel(
                        "well",
                        Style::default()
                            .pad(4.0)
                            .gap(3.0)
                            .w(LIST_W)
                            .h(tall)
                            .scroll_outside()
                            .stretch(),
                        |ui| {
                            if rows.is_empty() {
                                let key = match (&status, tab) {
                                    (Status::Loading, _) => "mods.loading",
                                    (Status::Failed(_), _) => "mods.offline",
                                    (_, Tab::Browse) if !self.query.borrow().is_empty() => "mods.no_results",
                                    _ => "mods.empty",
                                };
                                label(ui, cx, key, &[], "dim");
                            }
                            for (i, r) in rows.iter().enumerate() {
                                let (picked, id) = (self.picked.clone(), r.id.clone());
                                let b = ui.button(
                                    "tile",
                                    Style::row()
                                        .gap(6.0)
                                        .pad(4.0)
                                        .min_h(TILE_H)
                                        .w(LIST_W - 8.0)
                                        .align(Align::Center),
                                    Rc::new(move |c: &mut Ctx| {
                                        picked.borrow_mut()[slot] = id.clone();
                                        c.requests.push(Request::Rebuild);
                                    }),
                                    |ui| {
                                        mod_picture(ui, &r.icon, &r.name, &r.id, ICON);
                                        ui.container(Style::default().gap(2.0).grow(), |ui| {
                                            let (name, cut) = short(&r.name, 30);
                                            let t = ui.text(name, "default");
                                            if let Some(t) = ui.text_mut(t) {
                                                t.wrap = false;
                                            }
                                            if cut {
                                                ui.nodes[t].tooltip = Some(r.name.clone());
                                            }
                                            ui.container(Style::row().gap(3.0).align(Align::Center), |ui| {
                                                if !r.author.is_empty() {
                                                    let (a, _) = short(&text::plain(&r.author), 18);
                                                    let t = ui.text(a, "dim");
                                                    if let Some(t) = ui.text_mut(t) {
                                                        t.wrap = false;
                                                    }
                                                }
                                                badges(ui, cx, r);
                                            });
                                        });
                                        actions(ui, cx, r, &dir, busy);
                                        if r.toggle {
                                            // Takes effect on the next start: nothing here changes what is running.
                                            let id = r.id.clone();
                                            let on = crate::modloader::wanted_on(&r.id);
                                            let led = ui.button(
                                                "button",
                                                Style::default().size(17.0, 17.0).center(),
                                                Rc::new(move |c: &mut Ctx| {
                                                    crate::modloader::toggle(&id);
                                                    c.requests.push(Request::Rebuild);
                                                }),
                                                |ui| {
                                                    let texture = format!("core:gui/led_{}", if on { "on" } else { "off" });
                                                    ui.add(Kind::Image { texture, color: 0xFFFFFFFF }, Style::default().size(7.0, 7.0));
                                                },
                                            );
                                            ui.nodes[led].tooltip = Some(cx.t(if on { "mods.turn_off" } else { "mods.turn_on" }, &[]));
                                            set_enabled(ui, led, crate::modloader::load_error(&r.id).is_none());
                                            ui.set_id(led, &format!("mod_led:{}", r.id));
                                        }
                                    },
                                );
                                if let Kind::Button { selected, .. } = &mut ui.nodes[b].kind {
                                    *selected = i == sel;
                                }
                                ui.set_id(b, &format!("mod:{}", r.id));
                            }
                        },
                    );
                    ui.set_id(list, "mod_list");
                    // The one picked.
                    let detail_w = wide - LIST_W - 20.0 - gap;
                    ui.panel(
                        "well",
                        Style::default()
                            .pad(8.0)
                            .gap(4.0)
                            .w(detail_w)
                            .h(tall)
                            .scroll()
                            .stretch(),
                        |ui| {
                            let Some(r) = rows.get(sel) else { return };
                            detail(ui, cx, r, &dir, busy, detail_w - 16.0);
                        },
                    );
                });
                // What is going on, and the restart what changed waits for.
                ui.container(
                    Style::row()
                        .gap(6.0)
                        .align(Align::Center)
                        .self_align(Align::Stretch),
                    |ui| {
                        match &status {
                            Status::Busy(id) => {
                                label(ui, cx, "mods.downloading", &[Arg::Text(id.clone())], "note");
                            }
                            Status::Failed(e) => {
                                label(ui, cx, "mods.failed", &[Arg::Text(e.clone())], "warning");
                            }
                            _ => {}
                        }
                        if crate::modloader::restart_pending() || mod_index::changed() {
                            label(ui, cx, "mods.restart_note", &[], "note");
                            ui.spacer();
                            // A phone's game can't start itself again: it closes, to be opened again.
                            let key = if cfg!(target_os = "android") { "mods.close" } else { "mods.restart" };
                            let b = crate::screens::button(ui, cx, key, &[], 100.0, |c| c.requests.push(Request::Restart));
                            ui.set_id(b, "mods_restart");
                        }
                    },
                );
            });
        });
    }
}

/// Everything about the mod picked: who made it and where it lives, its versions, what it needs,
/// whether it was checked, what it is, and what to do with it.
fn detail(ui: &mut Ui, cx: &BuildCx, r: &Row, dir: &PathBuf, busy: bool, width: f32) {
    let e = r.entry.as_ref();
    let key = |k: &str| format!("mod.{}.{k}", r.id);
    ui.container(Style::row().gap(8.0), |ui| {
        mod_picture(ui, &r.icon, &r.name, &r.id, BIG_ICON);
        ui.container(Style::default().gap(2.0).w(width - BIG_ICON - 8.0), |ui| {
            let t = ui.text(r.name.clone(), "default");
            if let Some(t) = ui.text_mut(t) {
                t.scale = 2.0;
            }
            if !r.author.is_empty() {
                label(ui, cx, "mods.by", &[Arg::Text(r.author.clone())], "dim");
            }
            let url = e
                .map(|e| e.source.clone())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    cx.lang
                        .has(&key("url"))
                        .then(|| cx.lang.get(&key("url")).to_string())
                });
            if let Some(url) = url {
                let open = url.clone();
                let b = ui.button(
                    "",
                    Style::row().gap(3.0).align(Align::Center),
                    Rc::new(move |_: &mut Ctx| crate::packs::open_path(std::path::Path::new(&open))),
                    |ui| {
                        ui.add(Kind::Image { texture: "core:gui/icon_link".into(), color: 0xFFFFFFFF }, Style::default().size(9.0, 9.0));
                        label(ui, cx, "mods.source_code", &[], "link");
                    },
                );
                ui.nodes[b].tooltip = Some(text::plain(&url));
            }
        });
    });
    separator(ui);
    let fact = |ui: &mut Ui, k: &str, v: &mut dyn FnMut(&mut Ui)| {
        ui.container(Style::row().gap(6.0).align(Align::Center), |ui| {
            let t = label(ui, cx, k, &[], "dim");
            ui.nodes[t].style = Style::default().w(64.0);
            v(ui);
        });
    };
    let loaded = crate::content::content()
        .mods
        .iter()
        .chain(crate::modloader::turned_off())
        .find(|m| m.id == r.id);
    if let Some(reason) = crate::modloader::load_error(&r.id) {
        label(ui, cx, "mods.failed", &[Arg::Text(reason.to_string())], "warning");
    }
    match &r.state {
        State::Local => {
            let v = loaded.map(|m| m.version.clone()).unwrap_or_default();
            fact(ui, "mods.fact_version", &mut |ui| {
                ui.text(text::plain(&v), "default");
            });
        }
        State::Installed { have, newer } => {
            fact(ui, "mods.fact_installed", &mut |ui| {
                ui.text(text::plain(have), "default");
            });
            if let Some(v) = newer {
                fact(ui, "mods.fact_newest", &mut |ui| {
                    ui.text(text::plain(v), "default");
                });
            }
        }
        State::Get { version } => fact(ui, "mods.fact_newest", &mut |ui| {
            ui.text(text::plain(version), "default");
        }),
        State::Incompatible => fact(ui, "mods.fact_newest", &mut |ui| {
            badge(ui, cx, "badge_incompatible", "mods.incompatible", &[], Some("mods.incompatible_tip"));
        }),
    }
    let deps: Vec<String> = e
        .and_then(|e| e.fitting().or(e.builds.first()))
        .map(|b| b.deps.clone())
        .or_else(|| loaded.map(|m| m.deps.clone()))
        .unwrap_or_default();
    fact(ui, "mods.fact_needs", &mut |ui| {
        if deps.is_empty() {
            label(ui, cx, "mods.nothing_else", &[], "dim");
        }
        for d in &deps {
            let name = text::plain_text(&mod_name(cx.lang, d));
            badge(ui, cx, "badge_dep", "mods.plain", &[Arg::Text(name)], None);
        }
    });
    if let Some(v) = r.verified {
        ui.container(Style::row().gap(4.0).align(Align::Center), |ui| {
            if v {
                badge(ui, cx, "badge_verified", "mods.verified", &[], None);
                label(ui, cx, "mods.safe", &[], "dim");
            } else {
                badge(ui, cx, "badge_unverified", "mods.unverified", &[], None);
                label(ui, cx, "mods.unsafe", &[], "dim");
            }
        });
    }
    separator(ui);
    label(ui, cx, "mods.about", &[], "heading");
    let about = e.map(|e| e.summary.clone()).filter(|s| !s.is_empty());
    let d = match about {
        Some(s) if !cx.lang.has(&key("description")) => ui.text(text::plain(&s), "default"),
        _ if cx.lang.has(&key("description")) => ui.text(cx.t(&key("description"), &[]), "default"),
        _ => label(ui, cx, "mods.no_description", &[], "dim"),
    };
    ui.set_id(d, "mod_description");
    ui.spacer();
    // What to do with it.
    ui.container(Style::row().gap(4.0).align(Align::Center), |ui| {
        let main = match &r.state {
            State::Get { version } => Some(("button_install", "icon_download", "mods.install_version", version.clone())),
            State::Installed { newer: Some(v), .. } => Some(("button_update", "icon_update", "mods.update_to", v.clone())),
            _ => None,
        };
        if let Some((class, icon, k, v)) = main {
            let (id, to) = (r.id.clone(), dir.clone());
            let b = ui.button(
                class,
                Style::row()
                    .gap(4.0)
                    .pad_xy(6.0, 4.0)
                    .min_h(17.0)
                    .align(Align::Center),
                Rc::new(move |c: &mut Ctx| {
                    request_install(c, &id, &to);
                }),
                |ui| {
                    ui.add(Kind::Image { texture: format!("core:gui/{icon}"), color: 0xFFFFFFFF }, Style::default().size(9.0, 9.0));
                    label(ui, cx, k, &[Arg::Text(v.clone())], "button");
                },
            );
            set_enabled(ui, b, !busy);
        }
        let has_settings = crate::content::content()
            .game
            .configs
            .iter()
            .any(|d| crate::screens::on_config_screen(d, &Shown::Mod(r.id.clone())));
        if has_settings {
            let id = r.id.clone();
            crate::screens::button(ui, cx, "mods.settings", &[], 80.0, move |c| {
                c.requests
                    .push(Request::Open(Box::new(ConfigScreen { shown: Shown::Mod(id.clone()) })))
            });
        }
        if r.removable {
            let (id, to) = (r.id.clone(), dir.clone());
            let b = icon_button(ui, cx, "button", "icon_trash", "mods.remove", &[], move |c| {
                mod_index::remove(&id, &to);
                c.requests.push(Request::Rebuild);
            });
            set_enabled(ui, b, !busy);
        }
    });
}
