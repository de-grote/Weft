//! Built-in screens; a mod replaces one by registering a screen with its name (`named`). The
//! pause, death and settings screens are built from the node descriptors mods send
//! (`ui::desc`), so a replacement can do all they do.

use std::rc::Rc;

use crate::app::Running;
use crate::app::{Ctx, Request};
use crate::input::{Binding, Input, Key, Trigger};
use crate::settings::Settings;
use crate::ui::lang::Lang;
use crate::ui::text::{self, Arg, Span};
use crate::ui::{Justify, Kind, NodeId, Style, Ui, desc};
use weft_abi as abi;

pub struct BuildCx<'a> {
    pub lang: &'a Lang,
    pub input: &'a Input,
    pub settings: &'a Settings,
    pub languages: &'a [(String, String)],
    pub in_game: bool,
    /// Joined someone else's world (their synced settings apply).
    pub remote: bool,
    pub mods: usize,
    pub client: Option<&'a crate::client::Client>,
    /// The window in GUI pixels.
    pub size: glam::Vec2,
    pub look: &'a crate::ui::look::Look,
    pub assets: &'a crate::assets::Assets,
    /// The renderer drawing now (`core:renderer` may have chosen another for the next start).
    pub renderer: &'a str,
}

impl BuildCx<'_> {
    pub fn t(&self, key: &str, args: &[Arg]) -> Vec<Span> {
        let mut out = Vec::new();
        let cx = text::Ctx::new(self.lang, Some(self.input));
        text::format(self.lang.get(key), args, &cx, &mut out);
        out
    }

    /// A template that is not a translation key (a pack's description), formatted the same way.
    pub fn rich(&self, template: &str) -> Vec<Span> {
        text::rich(template, self.lang, Some(self.input))
    }
}

pub trait Screen: std::any::Any {
    fn name(&self) -> &str;
    fn build(&mut self, ui: &mut Ui, cx: &BuildCx);
    /// Single player stops the server tick while this is open.
    fn pauses(&self) -> bool {
        false
    }
    /// Keys go to this screen; `true` means it handled them.
    fn key(&mut self, _key: Key, _ctx: &mut Ctx) -> bool {
        false
    }
    /// Keys let go of, while it gets keys first.
    fn key_up(&mut self, _key: Key, _ctx: &mut Ctx) -> bool {
        false
    }
    /// Before the mouse goes to the nodes, each frame on top, cursor in GUI pixels: `true` takes
    /// it this frame (dragging what the nodes alone cannot).
    fn pointer(&mut self, _cursor: glam::Vec2, _ui: &mut Ui, _ctx: &mut Ctx, _hud: &mut crate::hud::Hud) -> bool {
        false
    }
    /// Shows the open server menu: its slots take clicks, closing the screen closes the menu.
    fn shows_menu(&self) -> bool {
        false
    }
    /// The chat line: Enter sends its `chat` input, the HUD leaves the messages to it.
    fn shows_chat(&self) -> bool {
        false
    }
    /// Gets the keys before Escape and the other global keys.
    fn keys_first(&self) -> bool {
        false
    }
    fn escape_closes(&self) -> bool {
        true
    }
    fn closes_on_respawn(&self) -> bool {
        false
    }
    /// Lists the installed languages (read when it opens).
    fn needs_languages(&self) -> bool {
        false
    }
}

/// A screen by name (`core:` may be left out): a mod's of that name, else the engine's.
/// Engine screens open each other through this too, so a mod's replacement is used everywhere.
pub fn named(name: &str) -> Option<Box<dyn Screen>> {
    let name = if name.contains(':') { name.to_string() } else { format!("core:{name}") };
    // A menu's screen opens on the client alone too, with nothing to sync.
    if let Some(&m) = crate::content::try_content().and_then(|c| {
        c.game
            .menu_names
            .get(&name)
            .filter(|&&m| c.game.menus[m as usize].screen.is_some())
    }) {
        return Some(Box::new(ModScreen(m)));
    }
    Some(match name.as_str() {
        "core:title" => Box::new(Title),
        "core:pause" => Box::new(Pause),
        "core:settings" => Box::new(SettingsScreen),
        "core:language" => Box::new(LanguageScreen),
        "core:controls" => Box::new(Controls::default()),
        "core:mod_settings" => Box::new(ModSettings),
        "core:worlds" => Box::new(crate::saves::WorldSelect::default()),
        "core:create_world" => Box::new(crate::saves::CreateWorld::new()),
        "core:chat" => Box::new(crate::game_screens::Chat::new("")),
        "core:death" => Box::new(crate::game_screens::Death),
        "core:multiplayer" => Box::new(crate::net::screens::Multiplayer::default()),
        "core:resource_packs" => Box::new(crate::packs::ResourcePacks::default()),
        "core:shader_packs" => Box::new(crate::packs::ShaderPacks),
        "core:sound" => Box::new(ConfigScreen { shown: Shown::Sound }),
        "core:world_rules" => Box::new(ConfigScreen { shown: Shown::World }),
        "core:chat_settings" => Box::new(ChatSettings),
        "core:hud_editor" => Box::new(crate::hud_editor::HudEditor::default()),
        "core:debug_options" => Box::new(crate::hud_editor::DebugOptions),
        "core:tags" => Box::new(crate::game_screens::TagList::default()),
        "core:mods" => Box::new(ModList::default()),
        "core:profile" => Box::new(Profile::default()),
        _ => return None,
    })
}

/// An engine screen; they always exist.
pub fn engine(name: &str) -> Box<dyn Screen> {
    named(name).unwrap_or_else(|| panic!("no screen {name}"))
}

/// Opens a screen by name (a mod's replacement if any).
fn open(c: &mut Ctx, name: &str) {
    c.requests.push(Request::Screen(name.to_string()));
}

/// What mods add to place `area` of `screen` (`Registry::screen_part`).
pub fn parts(ui: &mut Ui, cx: &BuildCx, screen: &str, area: &str) {
    ui.container(Style::default().gap(4.0).stretch(), |ui| {
        crate::client::modui::parts(ui, cx.lang, Some(cx.input), cx.client, screen, area);
    });
}

/// A mod's name as players see it: `mod.<id>.name` from the language files, else its id.
pub fn mod_name(lang: &Lang, id: &str) -> Vec<text::Span> {
    text::Name::Mod(id.to_string()).spans(lang, None)
}

// ---------------------------------------------------------------- mods

/// Every loaded mod with its icon, version, authors and description (translations
/// `mod.<id>.description`, `.authors`, `.url`; icon `textures/gui/icon.png`), and its settings.
#[derive(Default)]
pub struct ModList {
    selected: Rc<std::cell::Cell<usize>>,
}

impl Screen for ModList {
    fn name(&self) -> &str {
        "core:mods"
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let c = crate::content::content();
        let mut mods: Vec<&crate::content::ModMeta> = c.mods.iter().collect();
        mods.sort_by_key(|m| m.order);
        mods.extend(crate::modloader::turned_off());
        let sel = self.selected.get().min(mods.len().saturating_sub(1));
        let tall = (cx.size.y - 120.0).clamp(80.0, 300.0);
        let icon = |id: &str| {
            let name = format!("{id}:gui/icon");
            crate::assets::texture_size(&name).map(|_| name)
        };
        frame(ui, cx, "mods.title", |ui| {
            ui.container(Style::row().gap(6.0), |ui| {
                let list = ui.panel(
                    "well",
                    Style::default()
                        .pad(4.0)
                        .gap(2.0)
                        .w(184.0)
                        .h(tall)
                        .scroll()
                        .stretch(),
                    |ui| {
                        for (i, m) in mods.iter().enumerate() {
                            let s = self.selected.clone();
                            let b = ui.button(
                                "button",
                                Style::row().min_h(22.0).pad_xy(4.0, 2.0).gap(2.0).w(172.0),
                                Rc::new(move |c: &mut Ctx| {
                                    s.set(i);
                                    c.requests.push(Request::Rebuild);
                                }),
                                |ui| {
                                    ui.container(Style::row().gap(4.0).grow().center(), |ui| {
                                        match icon(&m.id) {
                                            Some(t) => drop(
                                                ui.add(Kind::Image { texture: t, color: 0xFFFFFFFF }, Style::default().size(16.0, 16.0)),
                                            ),
                                            None => drop(ui.add(Kind::Container { class: None }, Style::default().size(16.0, 16.0))),
                                        }
                                        ui.text(mod_name(cx.lang, &m.id), "button");
                                    });
                                    // Takes effect on the next start: nothing here changes what is running.
                                    let id = m.id.clone();
                                    let on = crate::modloader::wanted_on(&m.id);
                                    let led = ui.button(
                                        "",
                                        Style::default().size(14.0, 18.0),
                                        Rc::new(move |c: &mut Ctx| {
                                            crate::modloader::toggle(&id);
                                            c.requests.push(Request::Rebuild);
                                        }),
                                        |ui| {
                                            ui.add(
                                                Kind::Image {
                                                    texture: format!("core:gui/led_{}", if on { "on" } else { "off" }),
                                                    color: 0xFFFFFFFF,
                                                },
                                                Style::default().size(6.0, 6.0),
                                            );
                                        },
                                    );
                                    ui.set_id(led, &format!("mod_led:{}", m.id));
                                },
                            );
                            if let Kind::Button { selected, .. } = &mut ui.nodes[b].kind {
                                *selected = i == sel;
                            }
                            ui.set_id(b, &format!("mod:{}", m.id));
                        }
                    },
                );
                ui.set_id(list, "mod_list");
                let Some(m) = mods.get(sel) else { return };
                let key = |k: &str| format!("mod.{}.{k}", m.id);
                ui.panel(
                    "panel",
                    Style::default()
                        .pad(8.0)
                        .gap(4.0)
                        .w(300.0)
                        .h(tall)
                        .scroll()
                        .stretch(),
                    |ui| {
                        ui.container(Style::row().gap(6.0), |ui| {
                            if let Some(t) = icon(&m.id) {
                                ui.add(Kind::Image { texture: t, color: 0xFFFFFFFF }, Style::default().size(32.0, 32.0));
                            }
                            ui.container(Style::default().gap(1.0), |ui| {
                                ui.text(mod_name(cx.lang, &m.id), "default");
                                label(ui, cx, "mods.version", &[Arg::Text(m.version.clone()), Arg::Text(m.id.clone())], "dim");
                            });
                        });
                        for (k, line) in [("authors", "mods.authors"), ("url", "mods.url")] {
                            if cx.lang.has(&key(k)) {
                                label(ui, cx, line, &[Arg::Key(key(k))], "dim");
                            }
                        }
                        if !m.deps.is_empty() {
                            let deps: Vec<String> = m
                                .deps
                                .iter()
                                .map(|d| text::Name::Mod(d.clone()).text(cx.lang))
                                .collect();
                            label(ui, cx, "mods.deps", &[Arg::Text(deps.join(", "))], "dim");
                        }
                        ui.add(Kind::Container { class: None }, Style::default().h(4.0));
                        if cx.lang.has(&key("description")) {
                            let d = ui.text(cx.t(&key("description"), &[]), "default");
                            ui.set_id(d, "mod_description");
                        } else {
                            label(ui, cx, "mods.no_description", &[], "dim");
                        }
                    },
                );
            });
            ui.container(Style::row().gap(6.0), |ui| {
                let id = mods.get(sel).map(|m| m.id.clone()).unwrap_or_default();
                let has_settings = c
                    .game
                    .configs
                    .iter()
                    .any(|d| on_config_screen(d, &Shown::Mod(id.clone())));
                let b = button(ui, cx, "mods.settings", &[], 120.0, move |c| {
                    c.requests
                        .push(Request::Open(Box::new(ConfigScreen { shown: Shown::Mod(id.clone()) })))
                });
                if let Kind::Button { enabled, .. } = &mut ui.nodes[b].kind {
                    *enabled = has_settings;
                }
                let dir = cx.settings.run_dir.join("mods");
                button(ui, cx, "mods.folder", &[], 120.0, move |_| crate::packs::open_folder(&dir));
                button(ui, cx, "menu.done", &[], 120.0, |c| c.requests.push(Request::Close));
            });
            if crate::modloader::restart_pending() {
                ui.container(Style::row().gap(6.0).align(crate::ui::Align::Center), |ui| {
                    label(ui, cx, "mods.restart_note", &[], "dim");
                    let b = button(ui, cx, "mods.restart", &[], 120.0, |c| c.requests.push(Request::Restart));
                    ui.set_id(b, "mods_restart");
                });
            }
        });
    }
}

// ---------------------------------------------------------------- profile

/// Who the player is everywhere: the name, the key servers know them by, and the skin.
#[derive(Default)]
pub struct Profile;

impl Screen for Profile {
    fn name(&self) -> &str {
        "core:profile"
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let key = crate::net::p2p::player_key(&cx.settings.run_dir);
        frame(ui, cx, "profile.title", |ui| {
            ui.panel("panel", Style::default().pad(8.0).gap(4.0).w(320.0), |ui| {
                label(ui, cx, "profile.name", &[], "default");
                let input = ui.add(
                    Kind::Input {
                        value: cx.settings.name().clone(),
                        caret: cx.settings.name().chars().count(),
                        max: 16,
                        on_change: Some(Rc::new(|c: &mut Ctx, v: &str| c.settings.set_text("name", v))),
                    },
                    Style::default().h(16.0).w(300.0),
                );
                ui.set_id(input, "profile_name");
                ui.add(Kind::Container { class: None }, Style::default().h(4.0));
                label(ui, cx, "profile.key", &[], "default");
                ui.text(text::plain(if key.is_empty() { "-" } else { &key }), "dim");
                let copy = key.clone();
                button(ui, cx, "profile.copy", &[], 120.0, move |_| {
                    crate::clipboard::set(&copy);
                });
                ui.add(Kind::Container { class: None }, Style::default().h(4.0));
                label(ui, cx, "profile.skin", &[], "default");
                label(ui, cx, "profile.skin_hint", &[], "dim");
                label(ui, cx, "profile.avatar_hint", &[], "dim");
                let dir = cx.settings.run_dir.join("skins");
                button(ui, cx, "profile.skins_folder", &[], 120.0, move |_| crate::packs::open_folder(&dir));
            });
            button(ui, cx, "menu.done", &[], 200.0, |c| c.requests.push(Request::Close));
        });
    }
}

/// A menu's screen opened on the client (`Client::menu_open`), by its menu.
pub struct ModScreen(u32);

impl ModScreen {
    fn def(&self) -> &'static crate::registry::MenuDef {
        &crate::content::content().game.menus[self.0 as usize]
    }
    fn has(&self, flag: u32) -> bool {
        self.def().flags & flag != 0
    }
}

impl Screen for ModScreen {
    fn name(&self) -> &str {
        &self.def().name
    }

    fn pauses(&self) -> bool {
        self.has(crate::tag_bits::screen::PAUSES)
    }

    fn escape_closes(&self) -> bool {
        !self.has(crate::tag_bits::screen::KEEP_ON_ESCAPE)
    }

    fn closes_on_respawn(&self) -> bool {
        self.has(crate::tag_bits::screen::CLOSES_ON_RESPAWN)
    }

    fn shows_chat(&self) -> bool {
        self.has(crate::tag_bits::screen::SHOWS_CHAT)
    }

    fn keys_first(&self) -> bool {
        self.has(crate::tag_bits::screen::KEYS_FIRST) || self.def().on_key.is_some()
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> bool {
        if let Some((by, cb)) = self.def().on_key {
            ctx.requests.push(Request::ModKey(by, cb, key.id()));
        }
        false
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let bg = if cx.in_game { "overlay" } else { "background" };
        ui.root_style(Style::default().stretch());
        ui.panel(bg, Style::default().grow().center(), |ui| {
            if let Some((by, cb)) = self.def().screen {
                crate::client::modui::build(ui, cx.lang, Some(cx.input), by, cb, cx.client);
            }
        });
    }
}

// ---------------------------------------------------------------- helpers

pub fn label(ui: &mut Ui, cx: &BuildCx, key: &str, args: &[Arg], class: &str) -> NodeId {
    ui.text(cx.t(key, args), class)
}

/// A button whose text `text` says each frame (a choice going round).
pub fn bound_button(
    ui: &mut Ui,
    width: f32,
    text: impl Fn() -> (String, Vec<Arg>) + 'static,
    action: impl Fn(&mut Ctx) + 'static,
) -> NodeId {
    ui.button("button", Style::default().min_w(width).min_h(20.0).pad_xy(6.0, 4.0), Rc::new(action), |ui| {
        ui.text_bound("button", Box::new(move |_: &Ctx| text()));
    })
}

pub fn button(ui: &mut Ui, cx: &BuildCx, key: &str, args: &[Arg], width: f32, action: impl Fn(&mut Ctx) + 'static) -> NodeId {
    let spans = cx.t(key, args);
    ui.button("button", Style::default().min_w(width).min_h(20.0).pad_xy(6.0, 4.0), Rc::new(action), |ui| {
        let t = ui.text(spans, "button");
        if let Some(t) = ui.text_mut(t) {
            t.wrap = false;
        }
    })
}

/// A menu button with an icon (class `icon`, 9x9 like a glyph and its shadow) at its left edge;
/// the label stays in the middle, so labels line up whatever the icons.
#[allow(clippy::too_many_arguments)]
pub fn icon_button(
    ui: &mut Ui,
    cx: &BuildCx,
    class: &str,
    icon: &str,
    key: &str,
    args: &[Arg],
    width: f32,
    action: impl Fn(&mut Ctx) + 'static,
) -> NodeId {
    let spans = cx.t(key, args);
    let icon = cx.look.texture(icon, "");
    ui.button(class, Style::default().min_w(width).min_h(20.0).pad_xy(6.0, 4.0), Rc::new(action), |ui| {
        if let Some(texture) = icon {
            ui.add(Kind::Image { texture, color: 0xFFFFFFFF }, Style::default().w(9.0).h(9.0).at(-1.0, 6.0, [0.0, 0.5]));
        }
        let t = ui.text(spans, "button");
        if let Some(t) = ui.text_mut(t) {
            t.wrap = false;
        }
    })
}

/// The face (8x8, hat layer over it) of the player's own skin, else of the player kind's: a GUI
/// texture name `UiRenderer::texture` makes from the file.
pub fn face_texture(cx: &BuildCx) -> String {
    format!("{}{}", crate::ui::render::FACE, cx.settings.run_dir.join("skins").join("skin.png").display())
}

/// Full-screen dimmed column with a title, used by most menus.
pub fn frame(ui: &mut Ui, cx: &BuildCx, title: &str, f: impl FnOnce(&mut Ui)) {
    let bg = if cx.in_game { "overlay" } else { "background" };
    ui.root_style(Style::default().stretch());
    ui.panel(bg, Style::default().grow().center().pad(12.0).gap(10.0), |ui| {
        let t = label(ui, cx, title, &[], "title");
        if let Some(t) = ui.text_mut(t) {
            t.scale = 2.0;
            t.wrap = false;
        }
        f(ui);
    });
}

// ---------------------------------------------------------------- title

pub struct Title;

impl Screen for Title {
    fn name(&self) -> &str {
        "core:title"
    }

    /// The menu down a bar on the left under the logo, the rest of the window for whatever packs
    /// and mods show there (`menu.news`, the `side` place).
    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        ui.root_style(Style::default().stretch());
        ui.panel("background", Style::row().grow().stretch(), |ui| {
            ui.panel("sidebar", Style::default().w(180.0).pad(12.0).gap(4.0).stretch(), |ui| {
                // The `logo` texture across the bar: whole multiples of its pixels while it fits.
                if let Some(logo) = cx.look.texture("logo", "") {
                    let size = cx
                        .assets
                        .read(&logo, "textures", "png")
                        .and_then(|b| crate::assets::decode_png(&b))
                        .map_or([1.0, 1.0], |i| [i.width as f32, i.height as f32]);
                    let k = if size[0] <= 156.0 { (156.0 / size[0]).floor() } else { 156.0 / size[0] };
                    ui.add(Kind::Image { texture: logo, color: 0xFFFFFFFF }, Style::default().w(size[0] * k).h(size[1] * k));
                }
                ui.spacer();
                icon_button(ui, cx, "button", "menu_singleplayer", "menu.singleplayer", &[], 156.0, |c| open(c, "core:worlds"));
                // The profile is a square beside Multiplayer: the face of the player's skin.
                ui.container(Style::row().gap(4.0).stretch(), |ui| {
                    let mp =
                        icon_button(ui, cx, "button", "menu_multiplayer", "menu.multiplayer", &[], 132.0, |c| open(c, "core:multiplayer"));
                    ui.nodes[mp].style.grow = 1.0;
                    let face = face_texture(cx);
                    let p = ui.button("button", Style::default().w(20.0).h(20.0), Rc::new(|c: &mut Ctx| open(c, "core:profile")), |ui| {
                        ui.add(Kind::Image { texture: face, color: 0xFFFFFFFF }, Style::default().w(8.0).h(8.0).at(6.0, 6.0, [0.0, 0.0]));
                    });
                    ui.nodes[p].tooltip = Some(cx.t("menu.profile", &[]));
                    ui.set_id(p, "profile");
                });
                icon_button(ui, cx, "button", "menu_mods", "menu.mods", &[Arg::Int(cx.mods as i64)], 156.0, |c| open(c, "core:mods"));
                icon_button(ui, cx, "button", "menu_settings", "menu.settings", &[], 156.0, |c| open(c, "core:settings"));
                parts(ui, cx, "core:title", "buttons");
                ui.add(Kind::Container { class: None }, Style::default().h(6.0));
                let quit = button(ui, cx, "menu.quit", &[], 156.0, |c| c.requests.push(Request::Quit));
                if let Kind::Button { class, .. } = &mut ui.nodes[quit].kind {
                    *class = "button_quit".into();
                }
                ui.add(Kind::Container { class: None }, Style::default().h(6.0));
                label(ui, cx, "menu.version", &[Arg::Text(env!("CARGO_PKG_VERSION").into()), Arg::Int(cx.mods as i64)], "dim");
            });
            ui.container(
                Style::default()
                    .grow()
                    .pad(16.0)
                    .gap(8.0)
                    .justify(Justify::End)
                    .align(crate::ui::Align::End),
                |ui| {
                    if cx.lang.has("menu.news") && !cx.lang.get("menu.news").is_empty() {
                        ui.panel("news", Style::default().pad(8.0).w(150.0), |ui| {
                            ui.text(cx.t("menu.news", &[]), "default");
                        });
                    }
                    parts(ui, cx, "core:title", "side");
                },
            );
        });
    }
}

// ---------------------------------------------------------------- pause

pub struct Pause;

impl Screen for Pause {
    fn name(&self) -> &str {
        "core:pause"
    }

    fn pauses(&self) -> bool {
        true
    }

    /// Built from node descriptors like a mod's screen, so a replacement can match it.
    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        desc::build(ui, cx, |d| {
            desc::frame(d, cx.in_game, "menu.paused", |d| {
                d.button("menu.resume", &[], 200.0, false, |r| r.handle(vec![Request::Close]));
                d.button("menu.multiplayer", &[], 200.0, false, |r| r.handle(vec![Request::Screen("core:multiplayer".into())]));
                d.button("menu.settings", &[], 200.0, false, |r| r.handle(vec![Request::Screen("core:settings".into())]));
                d.parts("core:pause", "buttons");
                d.button("menu.save_quit", &[], 200.0, false, |r| r.handle(vec![Request::LeaveWorld]));
            });
        });
    }
}

// ---------------------------------------------------------------- settings

pub struct SettingsScreen;

pub fn on_off(v: bool) -> Arg {
    Arg::Key(if v { "settings.on".into() } else { "settings.off".into() })
}

impl Screen for SettingsScreen {
    fn name(&self) -> &str {
        "core:settings"
    }

    fn pauses(&self) -> bool {
        true
    }

    /// Built from node descriptors like a mod's screen, so a replacement can match it.
    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let s = cx.settings;
        let screen = |name: &'static str| move |r: &mut Running| r.handle(vec![Request::Screen(name.into())]);
        desc::build(ui, cx, |d| {
            desc::frame(d, cx.in_game, "settings.title", |d| {
                // Fills the space between the title and Done, so a short window scrolls the options
                // instead of cutting off everything below the fold.
                let panel = abi::UiNode {
                    class: abi::Str::new("panel"),
                    pad: [8.0; 4],
                    gap: 6.0,
                    align: abi::ui_align::STRETCH,
                    width: 320.0,
                    height: 0.0,
                    grow: 1.0,
                    min: [0.0, 40.0],
                    flags: abi::ui_flags::SCROLL,
                    ..desc::node(abi::ui_kind::PANEL)
                };
                d.open(&panel, |d| {
                    d.slider("settings.view_distance", "core:view_distance", 1.0);
                    d.slider("settings.lod_distance", "core:lod_distance", 1.0);
                    d.slider("settings.cloud_height", "core:cloud_height", 1.0);
                    d.slider("settings.fov", "core:fov", 1.0);
                    d.slider("settings.sensitivity", "core:sensitivity", 100.0);
                    d.slider("settings.brightness", "core:brightness", 100.0);
                    let scale =
                        if s.gui_scale() == 0 { Arg::Key("settings.gui_scale.auto".into()) } else { Arg::Int(s.gui_scale() as i64) };
                    d.button("settings.gui_scale", &[scale], 300.0, false, |r| {
                        // Auto, then every scale the window can actually show.
                        let max = crate::settings::max_gui_scale(r.stats.window);
                        let next = if r.settings.gui_scale() >= max { 0 } else { r.settings.gui_scale() + 1 };
                        r.settings.set("gui_scale", next as f64);
                    });
                    d.button("settings.vsync", &[on_off(s.vsync())], 300.0, false, |r| {
                        r.settings.set("vsync", !r.settings.vsync() as u8 as f64);
                        r.handle(vec![Request::Rebuild]);
                    });
                    let chosen = s.renderer();
                    let key = if chosen == cx.renderer { "settings.renderer" } else { "settings.renderer.restart" };
                    d.button(key, &[Arg::Key(format!("settings.renderer.{chosen}"))], 300.0, false, |r| {
                        let id = crate::config::by_name("core:renderer").unwrap_or(0);
                        let count = crate::content::content().game.configs[id as usize]
                            .choices
                            .len()
                            .max(1);
                        crate::config::set(id, (crate::config::get(id) + 1.0) % count as f64);
                        r.handle(vec![Request::Rebuild]);
                    });

                    let row = abi::UiNode {
                        row: 1,
                        gap: 6.0,
                        justify: abi::ui_justify::CENTER,
                        align: abi::ui_align::CENTER,
                        self_align: abi::ui_align::STRETCH,
                        ..desc::node(abi::ui_kind::CONTAINER)
                    };
                    for [(a, sa), (b, sb)] in [
                        [("settings.language", "core:language"), ("settings.controls", "core:controls")],
                        [("settings.sound", "core:sound"), ("settings.mods", "core:mod_settings")],
                        [("settings.resource_packs", "core:resource_packs"), ("settings.shader_packs", "core:shader_packs")],
                    ] {
                        d.open(&row, |d| {
                            d.button(a, &[], 147.0, true, screen(sa));
                            d.button(b, &[], 147.0, true, screen(sb));
                        });
                    }
                    if cx.in_game {
                        d.open(&row, |d| {
                            d.button("settings.chat", &[], 147.0, true, screen("core:chat_settings"));
                            d.button("settings.hud", &[], 147.0, true, screen("core:hud_editor"));
                        });
                        // Who may change them with `/rule` may here.
                        if cx
                            .client
                            .is_some_and(|cl| cl.permission >= weft_abi::permission::ADMIN)
                        {
                            d.button("world.rules", &[], 300.0, false, screen("core:world_rules"));
                        }
                    } else {
                        d.button("settings.chat", &[], 300.0, false, screen("core:chat_settings"));
                    }
                    // Below the screens a short window must show without scrolling.
                    d.slider("settings.fov_effects", "core:fov_effects", 100.0);
                    d.button("settings.first_person_model", &[on_off(s.first_person_model())], 300.0, false, |r| {
                        r.settings
                            .set("first_person_model", !r.settings.first_person_model() as u8 as f64);
                        r.handle(vec![Request::Rebuild]);
                    });
                    d.button("settings.show_avatars", &[on_off(s.show_avatars())], 300.0, false, |r| {
                        r.settings
                            .set("show_avatars", !r.settings.show_avatars() as u8 as f64);
                        r.handle(vec![Request::Rebuild]);
                    });
                    d.parts("core:settings", "buttons");
                });
                d.button("menu.done", &[], 200.0, false, |r| r.handle(vec![Request::Close]));
            });
        });
    }
}

/// How big the chat is and how dark behind it.
pub struct ChatSettings;

impl Screen for ChatSettings {
    fn name(&self) -> &str {
        "core:chat_settings"
    }

    fn pauses(&self) -> bool {
        true
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        desc::build(ui, cx, |d| {
            desc::frame(d, cx.in_game, "settings.chat", |d| {
                let panel = abi::UiNode {
                    class: abi::Str::new("panel"),
                    pad: [8.0; 4],
                    gap: 6.0,
                    align: abi::ui_align::STRETCH,
                    width: 320.0,
                    ..desc::node(abi::ui_kind::PANEL)
                };
                d.open(&panel, |d| {
                    d.slider("settings.chat_width", "core:chat_width", 1.0);
                    d.slider("settings.chat_height", "core:chat_height", 1.0);
                    d.slider("settings.chat_height_closed", "core:chat_height_closed", 1.0);
                    d.slider("settings.chat_opacity", "core:chat_opacity", 1.0);
                    let id = crate::config::by_name("core:chat_visibility").unwrap_or(0);
                    let now = ["all", "system", "hidden"][crate::config::get(id) as usize % 3];
                    let shown = [Arg::Key(format!("settings.chat_visibility.{now}"))];
                    d.button("settings.chat_visibility", &shown, 304.0, false, move |r| {
                        crate::config::set(id, (crate::config::get(id) + 1.0) % 3.0);
                        r.handle(vec![Request::Rebuild]);
                    });
                });
                d.button("menu.done", &[], 200.0, false, |r| r.handle(vec![Request::Close]));
            });
        });
    }
}

// ---------------------------------------------------------------- settings mods declare

/// Mods that declared settings (`entry_kind::CONFIG`).
pub struct ModSettings;

impl Screen for ModSettings {
    fn name(&self) -> &str {
        "core:mod_settings"
    }

    fn pauses(&self) -> bool {
        true
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let c = crate::content::content();
        let mut namespaces: Vec<&str> = c
            .game
            .configs
            .iter()
            .filter_map(|d| {
                d.name
                    .split_once(':')
                    .map(|x| x.0)
                    .filter(|ns| on_config_screen(d, &Shown::Mod(ns.to_string())))
            })
            .filter(|ns| *ns != "core")
            .collect();
        namespaces.dedup();
        frame(ui, cx, "settings.mods", |ui| {
            if namespaces.is_empty() {
                label(ui, cx, "settings.mods.none", &[], "dim");
            }
            for ns in namespaces {
                let name = Arg::Name(text::Name::Mod(ns.to_string()));
                let ns = ns.to_string();
                button(ui, cx, "settings.mods.entry", &[name], 220.0, move |c| {
                    c.requests
                        .push(Request::Open(Box::new(ConfigScreen { shown: Shown::Mod(ns.clone()) })))
                });
            }
            button(ui, cx, "menu.done", &[], 200.0, |c| c.requests.push(Request::Close));
        });
    }
}

/// Which settings a `ConfigScreen` shows: every mod's volumes, a mod's own, the world's rules.
pub enum Shown {
    Sound,
    Mod(String),
    World,
}

/// Whether a setting shows on that screen. Text settings have their own screens (keys, packs,
/// profile), and a mod's rules are with the world's.
fn on_config_screen(d: &crate::config::ConfigDef, shown: &Shown) -> bool {
    use weft_abi::{config_kind as ck, config_scope as scope};
    let Some((owner, short)) = d.name.split_once(':') else { return false };
    let volume = short.starts_with("volume_");
    match shown {
        Shown::Sound => volume,
        Shown::Mod(ns) => owner == ns && !volume && d.kind != ck::TEXT && d.scope != scope::WORLD,
        Shown::World => d.scope == scope::WORLD && d.kind != ck::TEXT,
    }
}

/// Settings built from their declarations: toggles, sliders, choices.
pub struct ConfigScreen {
    pub shown: Shown,
}

impl Screen for ConfigScreen {
    fn name(&self) -> &str {
        match self.shown {
            Shown::Sound => "core:sound",
            Shown::Mod(_) => "core:config",
            Shown::World => "core:world_rules",
        }
    }

    fn pauses(&self) -> bool {
        true
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        use weft_abi::config_kind as ck;
        let c = crate::content::content();
        let shown = &self.shown;
        let title = match shown {
            Shown::Sound => "settings.sound",
            Shown::Mod(_) => "settings.mods.title",
            Shown::World => "world.rules",
        };
        // Rules a world was made with locked stay as they are while it is played.
        let rules_locked = matches!(shown, Shown::World) && cx.in_game && crate::config::get(c.game.rules_locked) != 0.0;
        frame(ui, cx, title, |ui| {
            if let Shown::Mod(ns) = shown {
                label(ui, cx, "settings.mods.of", &[Arg::Name(text::Name::Mod(ns.to_string()))], "default");
            }
            let list = ui.panel(
                "panel",
                Style::default()
                    .pad(8.0)
                    .gap(6.0)
                    .stretch()
                    .w(320.0)
                    .h(0.0)
                    .grow()
                    .min_h(40.0)
                    .scroll(),
                |ui| {
                    let mut listed: Vec<_> = c
                        .game
                        .configs
                        .iter()
                        .enumerate()
                        .filter(|(_, d)| on_config_screen(d, shown))
                        .collect();
                    // The master volume first, the damage rules together at the end.
                    listed.sort_by_key(|(_, d)| (d.name != "core:volume_master", d.name.starts_with("core:damage.")));
                    for (id, d) in listed {
                        let id = id as u32;
                        let key = text::lang_key("config", &d.name);
                        let locked = (d.synced() && cx.remote) || rules_locked;
                        let show = |d: &crate::config::ConfigDef, key: &str, v: f64| -> Arg {
                            match d.kind {
                                ck::BOOL => on_off(v != 0.0),
                                ck::CHOICE => {
                                    let choice = d.choices.get(v as usize).cloned().unwrap_or_default();
                                    Arg::Key(format!("{key}.{choice}"))
                                }
                                _ => Arg::Text(d.show(v)),
                            }
                        };
                        let row = match d.kind {
                            ck::BOOL | ck::CHOICE => {
                                let (key2, d2) = (key.clone(), d.clone());
                                let (kind, count) = (d.kind, d.choices.len().max(1) as f64);
                                let b = ui.button(
                                    "button",
                                    Style::default().min_h(20.0).pad_xy(6.0, 4.0).stretch(),
                                    Rc::new(move |_: &mut Ctx| {
                                        let v = crate::config::get(id);
                                        let next = if kind == ck::BOOL { 1.0 - v } else { (v + 1.0) % count };
                                        crate::config::set(id, next);
                                    }),
                                    |ui| {
                                        let (k, d3) = (key2.clone(), d2.clone());
                                        ui.text_bound(
                                            "button",
                                            Box::new(move |_: &Ctx| {
                                                (
                                                    "settings.config_value".into(),
                                                    vec![Arg::Key(k.clone()), show(&d3, &k, crate::config::get(id))],
                                                )
                                            }),
                                        );
                                    },
                                );
                                if let Kind::Button { enabled, .. } = &mut ui.nodes[b].kind {
                                    *enabled = !locked;
                                }
                                b
                            }
                            _ => ui.container(Style::default().gap(2.0).stretch(), |ui| {
                                let (k, d2) = (key.clone(), d.clone());
                                ui.text_bound(
                                    "default",
                                    Box::new(move |_: &Ctx| {
                                        ("settings.config_value".into(), vec![Arg::Key(k.clone()), show(&d2, &k, crate::config::get(id))])
                                    }),
                                );
                                if !locked {
                                    let (min, range) = (d.min, (d.max - d.min).max(1e-9));
                                    let unit = if d.kind == weft_abi::config_kind::INT { 1.0 } else { range / 100.0 };
                                    ui.add(
                                        Kind::Slider {
                                            value: 0.0,
                                            bind: Box::new(move |_: &Ctx| ((crate::config::get(id) - min) / range) as f32),
                                            on_change: Rc::new(move |_: &mut Ctx, f: f32| crate::config::set(id, min + f as f64 * range)),
                                            steps: d.steps(unit),
                                        },
                                        Style::default().h(20.0).stretch(),
                                    );
                                }
                            }),
                        };
                        let tip = format!("{key}.tooltip");
                        let mut spans = if cx.lang.has(&tip) { cx.t(&tip, &[]) } else { Vec::new() };
                        for (on, note) in [
                            (locked, "settings.config_synced"),
                            (d.flags & crate::tag_bits::config::RESTART_REQUIRED != 0, "settings.config_restart"),
                        ] {
                            if on {
                                if !spans.is_empty() {
                                    spans.push(Span::text("\n", None));
                                }
                                spans.extend(cx.t(note, &[]));
                            }
                        }
                        if !spans.is_empty() {
                            ui.nodes[row].tooltip = Some(spans);
                        }
                    }
                },
            );
            ui.set_id(list, "config");
            button(ui, cx, "menu.done", &[], 200.0, |c| c.requests.push(Request::Close));
        });
    }
}

// ---------------------------------------------------------------- language

pub struct LanguageScreen;

impl Screen for LanguageScreen {
    fn needs_languages(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "core:language"
    }

    fn pauses(&self) -> bool {
        true
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let current = cx
            .languages
            .iter()
            .find(|l| l.0 == cx.lang.code)
            .map_or(cx.lang.code.clone(), |l| l.1.clone());
        frame(ui, cx, "settings.language", |ui| {
            label(ui, cx, "settings.language.current", &[Arg::Text(current)], "default");
            ui.panel("panel", Style::default().pad(8.0).gap(4.0).stretch().w(240.0), |ui| {
                for (code, name) in cx.languages {
                    let code = code.clone();
                    let selected = code == cx.lang.code;
                    let b = ui.button(
                        "button",
                        Style::default().min_h(20.0).pad_xy(6.0, 4.0).stretch(),
                        Rc::new(move |c: &mut Ctx| {
                            c.settings.set_text("language", &code);
                        }),
                        |ui| {
                            ui.text(text::plain(name), "button");
                        },
                    );
                    if let Kind::Button { selected: sel, .. } = &mut ui.nodes[b].kind {
                        *sel = selected;
                    }
                }
            });
            button(ui, cx, "menu.done", &[], 200.0, |c| c.requests.push(Request::Close));
        });
    }
}

// ---------------------------------------------------------------- controls

#[derive(Default)]
pub struct Controls {
    /// The binding waiting for its key: the action and which of its slots.
    waiting: Option<(String, usize)>,
    /// A modifier pressed while waiting: with a key it makes a chord (`ctrl+T`), let go alone
    /// it is the key.
    modifier: Option<Key>,
    /// The key pressed while waiting, kept for a moment: pressed again in time it binds a double
    /// tap.
    first: Option<(crate::input::Chord, std::time::Instant)>,
}

impl Controls {
    /// The action's bindings with `slot` changed by `f`.
    fn change(ctx: &mut Ctx, action: &str, slot: usize, f: impl FnOnce(Option<Binding>) -> Option<Binding>) {
        let Some(b) = ctx.input.binds.iter().find(|b| b.action == action) else { return };
        let mut keys = b.keys;
        keys[slot] = f(keys[slot]);
        ctx.requests.push(Request::Rebind(action.to_string(), keys));
        ctx.requests.push(Request::Rebuild);
    }

    /// Binds what was pressed to the slot waiting, a toggle staying a toggle.
    fn bind(&mut self, ctx: &mut Ctx, chord: crate::input::Chord, double: bool) {
        let Some((action, slot)) = self.waiting.take() else { return };
        (self.modifier, self.first) = (None, None);
        Self::change(ctx, &action, slot, |old| {
            let trigger = match (double, old.map(|b| b.trigger)) {
                (true, _) => Trigger::Double,
                (false, Some(t @ (Trigger::Toggle | Trigger::Tap))) => t,
                _ => Trigger::Hold,
            };
            Some(Binding { chord, trigger })
        });
    }
}

impl Screen for Controls {
    fn name(&self) -> &str {
        "core:controls"
    }

    fn pauses(&self) -> bool {
        true
    }

    /// While waiting for the key to bind, every key is that key, Escape included (it cancels).
    fn keys_first(&self) -> bool {
        self.waiting.is_some()
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> bool {
        if self.waiting.is_none() {
            return false;
        }
        if crate::input::modifier(key) != 0 {
            self.modifier = Some(key);
            return true;
        }
        // Only a key the settings file can name: anything else would be back to the default
        // after a restart.
        if key == Key::Code(winit::keyboard::KeyCode::Escape) || Key::parse(&key.id()).is_none() {
            (self.waiting, self.modifier, self.first) = (None, None, None);
            ctx.requests.push(Request::Rebuild);
            return true;
        }
        let chord = crate::input::Chord { key, mods: ctx.input.mods(), with: None };
        match self.first {
            Some((first, _)) if first == chord => self.bind(ctx, chord, true),
            // Pressed while the first is still down: that one is held with it (F3+T).
            Some((first, _)) if first.with.is_none() && ctx.input.held_key(first.key) => {
                self.bind(ctx, crate::input::Chord { with: Some(first.key), ..chord }, false)
            }
            Some((first, _)) => self.bind(ctx, first, false),
            None => self.first = Some((chord, std::time::Instant::now())),
        }
        true
    }

    fn key_up(&mut self, key: Key, ctx: &mut Ctx) -> bool {
        if self.modifier != Some(key) || self.first.is_some() || self.waiting.is_none() {
            return false;
        }
        self.bind(ctx, key.into(), false);
        true
    }

    /// A key pressed once binds when the time for a second press is over; the right button
    /// clears a binding, with Shift it switches between held and toggled.
    fn pointer(&mut self, cursor: glam::Vec2, ui: &mut Ui, ctx: &mut Ctx, _hud: &mut crate::hud::Hud) -> bool {
        if let Some((chord, at)) = self
            .first
            .filter(|f| f.1.elapsed() >= crate::input::DOUBLE_TAP)
        {
            let _ = at;
            self.bind(ctx, chord, false);
            return true;
        }
        if self.waiting.is_some() || !ctx.input.pressed_button(1) {
            return false;
        }
        let cell = ui
            .hit(cursor)
            .and_then(|h| ui.nodes[h].id.as_deref())
            .and_then(|id| id.strip_prefix("bind:"))
            .and_then(|id| id.rsplit_once(':'))
            .and_then(|(action, slot)| Some((action.to_string(), slot.parse::<usize>().ok()?)));
        let Some((action, slot)) = cell else { return false };
        if ctx.input.mods() & crate::input::SHIFT != 0 {
            Self::change(ctx, &action, slot, |old| {
                old.map(|b| match b.trigger {
                    Trigger::Hold => Binding { trigger: Trigger::Toggle, ..b },
                    Trigger::Toggle => Binding { trigger: Trigger::Hold, ..b },
                    Trigger::Double | Trigger::Tap => b,
                })
            });
        } else {
            Self::change(ctx, &action, slot, |_| None);
        }
        true
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let conflicts = cx.input.conflicts();
        let waiting = self.waiting.clone();
        frame(ui, cx, "controls.title", |ui| {
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
                    let s = cx.settings;
                    let on = s.invert_y();
                    let spans = cx.t("settings.invert_y", &[on_off(on)]);
                    ui.button(
                        "button",
                        Style::default().min_h(20.0).pad_xy(6.0, 4.0).stretch(),
                        Rc::new(move |c: &mut Ctx| {
                            c.settings.set("invert_y", !on as u8 as f64);
                            c.requests.push(Request::Rebuild);
                        }),
                        |ui| {
                            ui.text(spans, "button");
                        },
                    );
                    for b in &cx.input.binds {
                        let name = text::Name::Key(b.action.clone());
                        let clash: Vec<String> = conflicts
                            .iter()
                            .filter_map(|(x, y)| {
                                if *x == b.action {
                                    Some(y.clone())
                                } else if *y == b.action {
                                    Some(x.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        ui.container(Style::row().gap(6.0).self_align(crate::ui::Align::Stretch), |ui| {
                            let n = ui.text(name.spans(cx.lang, Some(cx.input)), "default");
                            ui.nodes[n].style.grow = 1.0;
                            if !clash.is_empty() {
                                let names: Vec<String> = clash
                                    .iter()
                                    .map(|c| text::Name::Key(c.clone()).text(cx.lang))
                                    .collect();
                                ui.text(cx.t("controls.conflict", &[Arg::Text(names.join(", "))]), "default");
                            }
                            for slot in 0..crate::input::SLOTS {
                                bind_cell(ui, cx, &b.action, slot, b.keys[slot], waiting.as_ref());
                            }
                        });
                    }
                },
            );
            ui.set_id(list, "controls");
            ui.container(Style::row().gap(6.0), |ui| {
                button(ui, cx, "controls.reset", &[], 120.0, |c| c.requests.push(Request::ResetBinds));
                button(ui, cx, "menu.done", &[], 120.0, |c| c.requests.push(Request::Close));
            });
        });
    }
}

/// One binding of an action: what it is (a toggle in its own frame), clicked to bind anew.
fn bind_cell(ui: &mut Ui, cx: &BuildCx, action: &str, slot: usize, binding: Option<Binding>, waiting: Option<&(String, usize)>) {
    let toggled = binding.is_some_and(|b| b.trigger == Trigger::Toggle);
    let shown = binding.map_or(String::new(), |b| text::display_binding(cx.lang, b));
    let a = action.to_string();
    let cell = ui.button(
        if toggled { "bind_toggle" } else { "button" },
        Style::default().w(90.0).h(20.0).pad_xy(4.0, 0.0),
        Rc::new(move |c: &mut Ctx| {
            if let Some(s) = c.screen::<Controls>() {
                (s.waiting, s.first, s.modifier) = (Some((a.clone(), slot)), None, None);
            }
            c.requests.push(Request::Rebuild);
        }),
        |ui| {
            ui.text(text::plain(&shown), "button");
        },
    );
    if let Kind::Button { selected, .. } = &mut ui.nodes[cell].kind {
        *selected = waiting.is_some_and(|w| w.0 == action && w.1 == slot);
    }
    let mode = if toggled { "controls.mode_toggle" } else { "controls.mode_hold" };
    let mut tip = cx.t(mode, &[]);
    tip.push(Span::text(
        "
", None,
    ));
    tip.extend(cx.t("controls.cell_help", &[]));
    ui.nodes[cell].tooltip = Some(tip);
    ui.set_id(cell, &format!("bind:{action}:{slot}"));
}

/// Shows a message with a back button (disconnects, errors).
pub struct Message {
    pub title: String,
    pub args: Vec<Arg>,
}

impl Screen for Message {
    fn name(&self) -> &str {
        "core:message"
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let (title, args) = (self.title.clone(), self.args.clone());
        ui.root_style(Style::default().stretch());
        ui.panel(if cx.in_game { "overlay" } else { "background" }, Style::default().grow().center().pad(12.0).gap(10.0), |ui| {
            label(ui, cx, &title, &args, "title");
            let in_game = cx.in_game;
            button(ui, cx, "menu.back", &[], 200.0, move |c| {
                c.requests.push(Request::Close);
                if !in_game {
                    open(c, "core:title");
                }
            });
        });
    }
}
