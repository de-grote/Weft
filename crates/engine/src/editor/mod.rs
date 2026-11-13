//! The content editor: a mod's blocks and items made in a form, with their code beside it, both
//! editable and kept in step. The form is built from the SDK's own list of builder methods
//! (`code::METHODS`): each call is a card, each parameter a field chosen by its type, and a few
//! kinds of value get a picker from what the game has registered (pictures, sounds, tabs, tools,
//! tags, items). The work stays while the game runs, so the screen can be closed and opened again.

pub mod code;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use code::{Call, Param, Thing, What};

use crate::app::{Ctx, Request};
use crate::content::Content;
use crate::mods_screen::{badge, icon_button, separator};
use crate::screens::{BuildCx, Screen, close_button, label};
use crate::ui::text::{self, Arg};
use crate::ui::{Align, Kind, Scrollbar, Style, Ui};

const LIST_W: f32 = 130.0;
const FORM_W: f32 = 290.0;
/// What the form's rows span: its width less its padding and scroll bar. Everything in it ends
/// at the same right edge.
const CONTENT_W: f32 = FORM_W - 26.0;
/// A parameter's name column in a call card.
const NAME_W: f32 = 52.0;
/// Its value's.
const VALUE_W: f32 = CONTENT_W - NAME_W - 4.0;
/// Fields and value buttons alike.
const FIELD_H: f32 = 16.0;
/// Choices a picker shows at once.
const SHOWN: usize = 80;

/// A list to choose an argument (or a new call) from.
#[derive(Clone, Copy, PartialEq)]
enum List {
    Method,
    Picture(&'static str),
    Sound,
    Tab,
    Tool,
    Tier,
    Tag,
    Item,
}

#[derive(Clone, Copy, PartialEq)]
struct Picker {
    list: List,
    /// The call and argument it sets; `None` for a new call.
    target: Option<(usize, usize)>,
}

struct Session {
    things: Vec<Thing>,
    picked: usize,
    /// The code as the player typed it, while it is theirs; written from the things otherwise.
    code: Option<String>,
    code_error: Option<String>,
    /// Bumped when the form changes the code, so the code field shows the new text; and when the
    /// code changes the form, so its fields do.
    code_gen: u32,
    form_gen: u32,
    picker: Option<Picker>,
    find: String,
    code_scroll: Rc<Cell<[f32; 2]>>,
    /// The call being moved by its handle.
    dragging: Option<usize>,
}

thread_local! {
    static SESSION: Rc<RefCell<Session>> = Rc::new(RefCell::new(Session {
        things: vec![Thing::new(What::Block, "ruby_block")],
        picked: 0,
        code: None,
        code_error: None,
        code_gen: 0,
        form_gen: 0,
        picker: None,
        find: String::new(),
        code_scroll: Rc::default(),
        dragging: None,
    }));
}

type S = Rc<RefCell<Session>>;

impl Session {
    fn thing(&mut self) -> &mut Thing {
        let i = self.picked.min(self.things.len() - 1);
        &mut self.things[i]
    }

    /// The form changed something: the code is written again from it.
    fn changed(&mut self) {
        self.code = None;
        self.code_error = None;
        self.code_gen += 1;
    }
}

/// An action changing the session through the form.
fn edit(s: &S, f: impl Fn(&mut Session) + 'static) -> impl Fn(&mut Ctx) + 'static {
    let s = s.clone();
    move |c: &mut Ctx| {
        let mut st = s.borrow_mut();
        f(&mut st);
        st.changed();
        c.requests.push(Request::Rebuild);
    }
}

/// Sets argument `arg` of call `call` of the picked thing.
fn set_arg(st: &mut Session, call: usize, arg: usize, value: String) {
    if let Some(c) = st.thing().calls.get_mut(call) {
        if c.args.len() <= arg {
            c.args.resize(arg + 1, String::new());
        }
        c.args[arg] = value;
    }
}

/// What lowercase names and references may hold; anything else is dropped as it is typed.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| c.to_ascii_lowercase())
        .filter(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | ':' | '/' | '.'))
        .collect()
}

/// How a parameter is edited: by its type, with a picker where the game knows the choices.
fn list_for(kind: What, method: &str, p: &Param) -> Option<List> {
    if !p.picture.is_empty() {
        return Some(List::Picture(p.picture));
    }
    match (method, p.name, p.ty) {
        (_, _, "&CreativeTab") => Some(List::Tab),
        ("sound_group", _, _) => Some(List::Sound),
        ("mined_by", "class", _) => Some(List::Tool),
        ("mined_by", "level", _) => Some(List::Tier),
        ("tag" | "untag", _, _) => Some(List::Tag),
        ("drops", "item", _) if kind == What::Block => Some(List::Item),
        _ => None,
    }
}

#[derive(Clone)]
enum Icon {
    None,
    Item(u32),
    /// A texture by its id in `Content::textures`.
    Picture(u32),
}

/// A picture's icon, if the content has it.
fn picture(c: &Content, name: &str) -> Icon {
    c.textures
        .iter()
        .position(|n| n == name)
        .map_or(Icon::None, |i| Icon::Picture(i as u32))
}

struct Choice {
    label: String,
    icon: Icon,
    /// The argument as code (or the method's name for a new call).
    value: String,
    doc: String,
}

/// The first item having component `comp`.
fn item_with(c: &Content, comp: Option<u32>) -> Icon {
    let Some(comp) = comp else { return Icon::None };
    (0..c.items.len() as u32)
        .find(|&i| c.components.has(crate::components::ITEM, i, comp))
        .map_or(Icon::None, Icon::Item)
}

/// A block's own item, by the block's name.
fn block_item(c: &Content, block: &str) -> Icon {
    c.item_names
        .get(block)
        .map_or(Icon::None, |&i| Icon::Item(i))
}

fn tier_key(code: &str) -> String {
    format!("editor.tier_{}", code.rsplit("::").next().unwrap_or("").to_lowercase())
}

/// Everything `list` offers, from what is registered.
fn choices(c: &Content, kind: What, list: List, things: &[Thing], cx: &BuildCx) -> Vec<Choice> {
    let plain = |s: &str, icon: Icon, value: String| Choice { label: s.into(), icon, value, doc: String::new() };
    match list {
        List::Method => code::methods(kind)
            .map(|m| Choice { label: m.name.into(), icon: Icon::None, value: m.name.into(), doc: m.doc.into() })
            .collect(),
        List::Picture(folder) => c
            .textures
            .iter()
            .filter(|n| n.contains(&format!(":{folder}/")) && !n.contains("destroy_"))
            .map(|n| {
                let arg = n.replacen(&format!("{folder}/"), "", 1);
                plain(&arg, picture(c, n), code::quote(&arg))
            })
            .collect(),
        List::Sound => {
            let sound = c.components.id(weft_abi::comp::SOUND);
            let mut groups: Vec<String> = c
                .game
                .sound_names
                .keys()
                .filter_map(|k| k.strip_suffix(".break"))
                .filter(|k| k.contains(":block."))
                .map(String::from)
                .collect();
            groups.sort();
            groups
                .into_iter()
                .map(|g| {
                    // Shown as a block that sounds like this.
                    let icon = c
                        .blocks
                        .iter()
                        .enumerate()
                        .find(|(b, _)| sound.and_then(|s| c.components.text(crate::components::BLOCK, *b as u32, s)) == Some(g.as_str()))
                        .map_or(Icon::None, |(_, b)| block_item(c, &b.name));
                    plain(&g, icon, code::quote(&g))
                })
                .collect()
        }
        List::Tab => c
            .creative_tabs
            .iter()
            .map(|t| plain(&t.name, Icon::Item(t.icon), format!("&CreativeTab::named({})", code::quote(&t.name))))
            .collect(),
        List::Tool => c
            .tool_classes
            .iter()
            .filter_map(|&(mine, tools)| {
                let class = c.components.names[mine as usize].strip_prefix("mineable/")?;
                Some(plain(class, item_with(c, tools), code::quote(class)))
            })
            .collect(),
        List::Tier => code::TIERS
            .iter()
            .map(|n| {
                let value = format!("abi::tool_tier::{n}");
                plain(&text::plain_text(&cx.t(&tier_key(&value), &[])), Icon::None, value)
            })
            .collect(),
        List::Tag => {
            let mut tags: Vec<&String> = (0..c.components.count())
                .filter(|&i| c.components.is_tag(i))
                .map(|i| &c.components.names[i as usize])
                .collect();
            tags.sort();
            tags.into_iter()
                .map(|t| plain(t, Icon::None, code::quote(t)))
                .collect()
        }
        List::Item => {
            let mut v: Vec<Choice> = things
                .iter()
                .filter(|t| t.kind == What::Item)
                .map(|t| plain(&t.name, t.picture().map_or(Icon::None, |p| picture(c, &p)), code::quote(&t.name)))
                .collect();
            let mut names: Vec<(&String, &u32)> = c.item_names.iter().collect();
            names.sort();
            v.extend(
                names
                    .into_iter()
                    .map(|(n, &i)| plain(n, Icon::Item(i), code::quote(n))),
            );
            v
        }
    }
}

/// Whether an item of this name exists. A name without a namespace is the mod's own (as in the
/// SDK), so only the session can have it.
fn known_item(c: &Content, things: &[Thing], name: &str) -> bool {
    if name.contains(':') { c.item_names.contains_key(name) } else { things.iter().any(|t| t.name == name) }
}

fn icon(ui: &mut Ui, icon: &Icon, side: f32) {
    match icon {
        Icon::None => {
            ui.add(Kind::Container { class: None }, Style::default().size(side, side));
        }
        Icon::Item(i) => {
            ui.add(Kind::Custom { tag: crate::client::modui::TAG_ICON, data: *i as u64 }, Style::default().size(side, side));
        }
        Icon::Picture(p) => {
            ui.add(Kind::Custom { tag: TAG_PICTURE, data: *p as u64 }, Style::default().size(side, side));
        }
    }
}

fn no_wrap(ui: &mut Ui, n: crate::ui::NodeId) {
    if let Some(t) = ui.text_mut(n) {
        t.wrap = false;
    }
}

/// Text cut to `most` chars, the whole of it in a tooltip.
fn short_text(ui: &mut Ui, s: &str, most: usize, class: &str) {
    let cut = s.chars().count() > most;
    let shown: String = if cut { s.chars().take(most - 1).chain(Some('…')).collect() } else { s.into() };
    let t = ui.text(text::plain(&shown), class);
    no_wrap(ui, t);
    if cut {
        ui.nodes[t].tooltip = Some(text::plain(s));
    }
}

pub struct Editor {
    s: S,
}

impl Default for Editor {
    fn default() -> Self {
        Editor { s: SESSION.with(Rc::clone) }
    }
}

impl Screen for Editor {
    fn name(&self) -> &str {
        "core:editor"
    }

    /// A call moved by its handle: it changes places with the card the cursor passes the middle of.
    fn pointer(&mut self, cursor: glam::Vec2, ui: &mut Ui, ctx: &mut Ctx, _hud: &mut crate::hud::Hud) -> bool {
        let mut st = self.s.borrow_mut();
        let Some(from) = st.dragging else {
            let handle = ui
                .hit(cursor)
                .and_then(|h| ui.nodes[h].id.as_deref())
                .and_then(|i| i.strip_prefix("call_handle:"))
                .and_then(|i| i.parse::<usize>().ok());
            let Some(i) = handle.filter(|_| ctx.input.pressed_button(0)) else { return false };
            st.dragging = Some(i);
            ctx.requests.push(Request::Rebuild);
            return true;
        };
        ui.hovered = None;
        if !ctx.input.held_button(0) {
            st.dragging = None;
            ctx.requests.push(Request::Rebuild);
            return true;
        }
        let count = st.thing().calls.len();
        let to = (0..count)
            .filter(|&i| i != from)
            .filter_map(|i| ui.find(&format!("call:{i}")).map(|n| (i, ui.nodes[n].rect)))
            .filter(|(_, r)| r.y + r.h / 2.0 < cursor.y)
            .count();
        if to != from && to < count {
            let call = st.thing().calls.remove(from);
            st.thing().calls.insert(to, call);
            st.dragging = Some(to);
            st.form_gen += 1;
            st.changed();
            ctx.requests.push(Request::Rebuild);
        }
        true
    }

    fn draws_close(&self) -> bool {
        true
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let Some(c) = crate::content::try_content() else { return };
        let s = self.s.clone();
        {
            let mut st = s.borrow_mut();
            st.picked = st.picked.min(st.things.len().saturating_sub(1));
        }
        let tall = (cx.size.y - 70.0).clamp(160.0, 380.0);
        let wide = (cx.size.x - 30.0).clamp(600.0, 780.0);
        let bg = if cx.in_game { "overlay" } else { "background" };
        ui.root_style(Style::default().stretch());
        ui.panel(bg, Style::default().grow().center().pad(8.0), |ui| {
            ui.panel("panel", Style::default().w(wide).pad(10.0).gap(6.0), |ui| {
                ui.container(Style::row().gap(6.0).align(Align::Center).w(wide - 20.0), |ui| {
                    let t = label(ui, cx, "editor.title", &[], "title");
                    if let Some(t) = ui.text_mut(t) {
                        t.scale = 2.0;
                        t.wrap = false;
                    }
                    ui.spacer();
                    for (kind, icon_name, tip) in
                        [(What::Block, "editor_block", "editor.new_block"), (What::Item, "editor_item", "editor.new_item")]
                    {
                        let b = icon_button(
                            ui,
                            cx,
                            "button_install",
                            icon_name,
                            tip,
                            &[],
                            edit(&s, move |st| {
                                let base = format!("new_{}", kind.folder());
                                let n = (1..)
                                    .map(|i| format!("{base}_{i}"))
                                    .find(|n| !st.things.iter().any(|t| &t.name == n))
                                    .unwrap_or(base);
                                st.things.push(Thing::new(kind, &n));
                                st.picked = st.things.len() - 1;
                                st.picker = None;
                                st.form_gen += 1;
                            }),
                        );
                        ui.set_id(b, tip);
                    }
                    close_button(ui, 15.0);
                });
                let gap = Scrollbar::HANDLE[0] + Scrollbar::OUTSIDE_GAP * 2.0;
                let code_w = wide - 20.0 - LIST_W - FORM_W - 2.0 * gap;
                ui.container(Style::row().gap(gap).w(wide - 20.0), |ui| {
                    self.list(ui, cx, c, tall);
                    ui.panel(
                        "well",
                        Style::default()
                            .pad(6.0)
                            .gap(3.0)
                            .w(FORM_W)
                            .h(tall)
                            .scroll(),
                        |ui| {
                            let picker = s.borrow().picker;
                            match picker {
                                Some(p) => self.picker(ui, cx, c, p),
                                None => self.form(ui, cx, c),
                            }
                        },
                    );
                    self.code(ui, cx, code_w, tall);
                });
            });
        });
    }
}

impl Editor {
    /// What the session has made, each with its picture.
    fn list(&self, ui: &mut Ui, cx: &BuildCx, c: &Content, tall: f32) {
        let s = &self.s;
        let (things, picked) = {
            let st = s.borrow();
            (st.things.clone(), st.picked)
        };
        ui.panel(
            "well",
            Style::default()
                .pad(4.0)
                .gap(3.0)
                .w(LIST_W)
                .h(tall)
                .scroll_outside(),
            |ui| {
                for (i, th) in things.iter().enumerate() {
                    let s2 = s.clone();
                    let b = ui.button(
                        "tile",
                        Style::row()
                            .gap(4.0)
                            .pad(3.0)
                            .w(LIST_W - 8.0)
                            .align(Align::Center),
                        Rc::new(move |c: &mut Ctx| {
                            let mut st = s2.borrow_mut();
                            st.picked = i;
                            st.picker = None;
                            st.form_gen += 1;
                            c.requests.push(Request::Rebuild);
                        }),
                        |ui| {
                            preview(ui, c, i, th, 16.0);
                            ui.container(
                                Style::default()
                                    .gap(1.0)
                                    .w(LIST_W - 8.0 - 6.0 - 16.0 - 8.0 - 21.0),
                                |ui| {
                                    short_text(ui, &th.name, 13, "default");
                                    label(ui, cx, if th.kind == What::Block { "editor.block" } else { "editor.item" }, &[], "dim");
                                },
                            );
                            if things.len() > 1 {
                                icon_button(
                                    ui,
                                    cx,
                                    "button",
                                    "icon_trash",
                                    "editor.remove",
                                    &[],
                                    edit(s, move |st| {
                                        st.things.remove(i);
                                        st.picked = st.picked.min(st.things.len() - 1);
                                        st.form_gen += 1;
                                    }),
                                );
                            }
                        },
                    );
                    if let Kind::Button { selected, .. } = &mut ui.nodes[b].kind {
                        *selected = i == picked;
                    }
                    ui.set_id(b, &format!("thing:{}", th.name));
                }
            },
        );
    }

    /// The picked thing: its name, then a card per call, then a button for another.
    fn form(&self, ui: &mut Ui, cx: &BuildCx, c: &Content) {
        let s = &self.s;
        let (things, picked, generation) = {
            let st = s.borrow();
            (st.things.clone(), st.picked, st.form_gen)
        };
        let t = &things[picked];
        let id = |what: &str| format!("editor_{what}:{picked}:{generation}");
        ui.container(Style::row().gap(6.0).align(Align::Center), |ui| {
            preview(ui, c, picked, t, 40.0);
            ui.container(Style::default().gap(2.0), |ui| {
                let input = ui.add(Kind::input(&t.name, 40), Style::default().h(FIELD_H).w(CONTENT_W - 46.0));
                let s2 = s.clone();
                if let Kind::Input { on_change, .. } = &mut ui.nodes[input].kind {
                    *on_change = Some(Rc::new(move |c: &mut Ctx, v: &str| {
                        let mut st = s2.borrow_mut();
                        let v = clean(v);
                        st.thing().name = if v.is_empty() { "unnamed".into() } else { v };
                        st.changed();
                        c.requests.push(Request::Rebuild);
                    }));
                }
                ui.set_id(input, &id("name"));
                label(ui, cx, if t.kind == What::Block { "editor.block" } else { "editor.item" }, &[], "dim");
            });
        });
        let dragging = s.borrow().dragging;
        for (ci, call) in t.calls.iter().enumerate() {
            separator(ui);
            let m = code::method(t.kind, &call.method);
            let card = ui.container(Style::default().gap(3.0).w(CONTENT_W), |ui| {
                // The card's head: the method, a way to drop it and a handle to move it by.
                ui.container(Style::row().gap(4.0).align(Align::Center).w(CONTENT_W), |ui| {
                    let n = ui.text(text::plain(&call.method), if dragging == Some(ci) { "dim" } else { "default" });
                    no_wrap(ui, n);
                    let doc = m.map_or("", |m| m.doc);
                    ui.nodes[n].tooltip = Some(if doc.is_empty() { cx.t("editor.no_doc", &[]) } else { text::plain(doc) });
                    if m.is_none() {
                        badge(ui, cx, "badge_unverified", "editor.unknown", &[], Some("editor.unknown_tip"));
                    }
                    ui.spacer();
                    icon_button(
                        ui,
                        cx,
                        "button",
                        "icon_trash",
                        "editor.remove_call",
                        &[],
                        edit(s, move |st| {
                            st.thing().calls.remove(ci);
                            st.form_gen += 1;
                        }),
                    );
                    let handle = icon_button(ui, cx, "button", "icon_drag", "editor.drag", &[], |_| {});
                    ui.set_id(handle, &format!("call_handle:{ci}"));
                });
                let params: Vec<&Param> = m.map_or_else(Vec::new, |m| m.params.iter().collect());
                // A call the SDK doesn't list (or more arguments than it says) is edited as code.
                let n = params.len().max(call.args.len());
                for ai in 0..n {
                    let p = params.get(ai).copied();
                    let value = call.args.get(ai).cloned().unwrap_or_default();
                    ui.container(Style::row().gap(4.0).align(Align::Center), |ui| {
                        ui.container(Style::default().w(NAME_W), |ui| {
                            short_text(ui, p.map_or("", |p| p.name), 9, "dim");
                        });
                        self.value(ui, cx, c, &things, t.kind, call, ci, ai, p, &value, &id(&format!("{ci}_{ai}")));
                    });
                }
            });
            ui.set_id(card, &format!("call:{ci}"));
        }
        separator(ui);
        let b = crate::screens::button(ui, cx, "editor.add_call", &[], CONTENT_W, {
            let s = s.clone();
            move |c| {
                let mut st = s.borrow_mut();
                st.picker = Some(Picker { list: List::Method, target: None });
                st.find.clear();
                c.requests.push(Request::Rebuild);
            }
        });
        ui.set_id(b, "editor_add_call");
    }

    /// One argument's field.
    #[allow(clippy::too_many_arguments)]
    fn value(
        &self,
        ui: &mut Ui,
        cx: &BuildCx,
        c: &Content,
        things: &[Thing],
        kind: What,
        call: &Call,
        ci: usize,
        ai: usize,
        p: Option<&Param>,
        value: &str,
        id: &str,
    ) {
        let s = &self.s;
        let list = p.and_then(|p| list_for(kind, &call.method, p));
        if let Some(list) = list {
            // The value as a button: a click opens the list.
            let shown = choices(c, kind, list, things, cx)
                .into_iter()
                .find(|ch| ch.value == value);
            let (ic, words) = match (&shown, list) {
                (Some(ch), _) => (ch.icon.clone(), ch.label.clone()),
                (None, List::Picture(folder)) => {
                    let arg = code::unquote(value).unwrap_or_default();
                    (code::picture_name(&arg, folder).map_or(Icon::None, |p| picture(c, &p)), arg)
                }
                (None, _) => (Icon::None, code::unquote(value).unwrap_or_else(|| value.to_string())),
            };
            // An item not made yet: room beside it for the button that makes it.
            let missing = list == List::Item && {
                let name = code::unquote(value).unwrap_or_default();
                !name.is_empty() && !known_item(c, things, &name)
            };
            let w = if missing { VALUE_W - 21.0 } else { VALUE_W };
            let s2 = s.clone();
            let b = ui.button(
                "button",
                Style::row()
                    .gap(4.0)
                    .pad_xy(3.0, 1.0)
                    .w(w)
                    .h(FIELD_H)
                    .align(Align::Center),
                Rc::new(move |c: &mut Ctx| {
                    let mut st = s2.borrow_mut();
                    st.picker = Some(Picker { list, target: Some((ci, ai)) });
                    st.find.clear();
                    c.requests.push(Request::Rebuild);
                }),
                |ui| {
                    icon(ui, &ic, 14.0);
                    short_text(ui, if words.is_empty() { "-" } else { &words }, 26, "button");
                },
            );
            ui.nodes[b].tooltip = Some(cx.t("editor.choose", &[]));
            ui.set_id(b, id);
            if missing {
                let name = code::unquote(value).unwrap_or_default();
                {
                    let b = icon_button(
                        ui,
                        cx,
                        "button_update",
                        "editor_item",
                        "editor.make_it",
                        &[],
                        edit(s, move |st| {
                            st.things.push(Thing::new(What::Item, &name));
                            st.picked = st.things.len() - 1;
                            st.form_gen += 1;
                        }),
                    );
                    ui.set_id(b, "editor_make");
                }
            }
            return;
        }
        let ty = p.map_or("", |p| p.ty);
        match ty {
            "bool" => {
                let on = value.trim() == "true";
                let b = crate::screens::button(
                    ui,
                    cx,
                    if on { "settings.on" } else { "settings.off" },
                    &[],
                    50.0,
                    edit(s, move |st| {
                        set_arg(st, ci, ai, (!on).to_string());
                    }),
                );
                ui.set_id(b, id);
            }
            "[f32;3]" => {
                let v = code::three(value).unwrap_or([0.0; 3]);
                for k in 0..3 {
                    let s2 = s.clone();
                    let f = field(ui, &code::num(v[k]), (VALUE_W - 8.0) / 3.0, &format!("{id}_{k}"), move |c, text| {
                        let Ok(n) = text.trim().parse::<f32>() else { return };
                        let mut st = s2.borrow_mut();
                        let mut v = st
                            .thing()
                            .calls
                            .get(ci)
                            .and_then(|c| c.args.get(ai))
                            .and_then(|a| code::three(a))
                            .unwrap_or([0.0; 3]);
                        v[k] = n;
                        set_arg(&mut st, ci, ai, format!("[{}, {}, {}]", code::num(v[0]), code::num(v[1]), code::num(v[2])));
                        st.changed();
                        c.requests.push(Request::Rebuild);
                    });
                    ui.nodes[f].tooltip = Some(text::plain(["x", "y", "z"][k]));
                }
            }
            _ => {
                // Text is edited without its quotes; numbers and anything else as written.
                let quoted = ty == "&str";
                let shown = if quoted { code::unquote(value).unwrap_or_else(|| value.to_string()) } else { value.to_string() };
                let s2 = s.clone();
                let numeric = matches!(ty, "f32" | "f64");
                let f = field(ui, &shown, VALUE_W, id, move |c, text| {
                    let v = if quoted {
                        code::quote(text)
                    } else if numeric {
                        match text.trim().parse::<f32>() {
                            Ok(n) if !text.trim().ends_with('.') => code::num(n),
                            _ => text.to_string(),
                        }
                    } else {
                        text.to_string()
                    };
                    let mut st = s2.borrow_mut();
                    set_arg(&mut st, ci, ai, v);
                    st.changed();
                    c.requests.push(Request::Rebuild);
                });
                if !ty.is_empty() {
                    ui.nodes[f].tooltip = Some(text::plain(ty));
                }
            }
        }
    }

    /// A list to choose from: searchable, each choice with its picture.
    fn picker(&self, ui: &mut Ui, cx: &BuildCx, c: &Content, p: Picker) {
        let s = &self.s;
        let (things, picked, find) = {
            let st = s.borrow();
            (st.things.clone(), st.picked, st.find.clone())
        };
        let kind = things[picked].kind;
        ui.container(Style::row().gap(4.0).align(Align::Center), |ui| {
            let b = crate::screens::button(ui, cx, "editor.back", &[], 40.0, {
                let s = s.clone();
                move |c| {
                    s.borrow_mut().picker = None;
                    c.requests.push(Request::Rebuild);
                }
            });
            ui.set_id(b, "editor_back");
            ui.add(Kind::Image { texture: "core:gui/icon_search".into(), color: 0xFFFFFFFF }, Style::default().size(9.0, 9.0));
            let s2 = s.clone();
            let input = field(ui, &find, CONTENT_W - 57.0, "editor_find", move |c, v| {
                s2.borrow_mut().find = v.to_lowercase();
                c.requests.push(Request::Rebuild);
            });
            ui.nodes[input].tooltip = Some(cx.t("editor.find", &[]));
            // Typing goes into the search as soon as the list opens.
            if ui.focused.is_none() {
                ui.focused = Some(input);
            }
        });
        let all = choices(c, kind, p.list, &things, cx);
        let words: Vec<&str> = find.split_whitespace().collect();
        let fits = |ch: &Choice| {
            words
                .iter()
                .all(|w| ch.label.to_lowercase().contains(w) || ch.doc.to_lowercase().contains(w))
        };
        let mut found: Vec<Choice> = all.into_iter().filter(|ch| fits(ch)).collect();
        // Names starting with what was typed first, then names holding it, then descriptions.
        let rank = |ch: &Choice| match words.first() {
            Some(w) if ch.label.to_lowercase().starts_with(w) => 0,
            Some(w) if ch.label.to_lowercase().contains(w) => 1,
            Some(_) => 2,
            None => 0,
        };
        found.sort_by_key(|ch| rank(ch));
        let mut shown: Vec<Choice> = found.into_iter().take(SHOWN).collect();
        // What isn't listed may still be written: a new tag, another mod's sound or item.
        if !find.is_empty() && matches!(p.list, List::Tag | List::Sound | List::Item) && !shown.iter().any(|ch| ch.label == find) {
            let f = clean(&find);
            shown.insert(
                0,
                Choice {
                    label: f.clone(),
                    icon: Icon::None,
                    value: code::quote(&f),
                    doc: text::plain_text(&cx.t("editor.write_this", &[])),
                },
            );
        }
        let grid = matches!(p.list, List::Picture(_));
        let target = p.target;
        let pick = move |s: &S, value: String| {
            let s = s.clone();
            move |c: &mut Ctx| {
                let mut st = s.borrow_mut();
                match target {
                    Some((ci, ai)) => set_arg(&mut st, ci, ai, value.clone()),
                    None => {
                        let kind = st.thing().kind;
                        let args = code::method(kind, &value).map_or_else(Vec::new, |m| m.params.iter().map(code::default_arg).collect());
                        st.thing().calls.push(Call { method: value.clone(), args });
                    }
                }
                st.picker = None;
                st.form_gen += 1;
                st.changed();
                c.requests.push(Request::Rebuild);
            }
        };
        if grid {
            // Pictures in rows, their names in tooltips.
            let per_row = ((FORM_W - 26.0) / 20.0) as usize;
            for row in shown.chunks(per_row) {
                ui.container(Style::row().gap(2.0), |ui| {
                    for ch in row {
                        let b = ui.button("button", Style::default().size(18.0, 18.0).center(), Rc::new(pick(s, ch.value.clone())), |ui| {
                            icon(ui, &ch.icon, 16.0)
                        });
                        ui.nodes[b].tooltip = Some(text::plain(&ch.label));
                    }
                });
            }
            return;
        }
        for ch in &shown {
            let b = ui.button(
                "tile",
                Style::row()
                    .gap(4.0)
                    .pad(2.0)
                    .w(FORM_W - 26.0)
                    .align(Align::Center),
                Rc::new(pick(s, ch.value.clone())),
                |ui| {
                    icon(ui, &ch.icon, 14.0);
                    ui.container(Style::default().w(FORM_W - 26.0 - 22.0), |ui| {
                        short_text(ui, &ch.label, 40, "default");
                        if !ch.doc.is_empty() {
                            // About two lines of it; all of it in the tooltip.
                            let most = 90;
                            let cut: String = if ch.doc.chars().count() > most {
                                ch.doc.chars().take(most - 1).chain(Some('…')).collect()
                            } else {
                                ch.doc.clone()
                            };
                            let d = ui.text(text::plain(&cut), "dim");
                            ui.nodes[d].tooltip = Some(text::plain(&ch.doc));
                        }
                    });
                },
            );
            ui.set_id(b, &format!("choice:{}", ch.label));
        }
        if shown.is_empty() {
            label(ui, cx, "editor.nothing", &[], "dim");
        }
    }

    /// The code, edited in place; the copy button in its corner.
    fn code(&self, ui: &mut Ui, cx: &BuildCx, w: f32, tall: f32) {
        let s = &self.s;
        let (value, generation, error, scroll) = {
            let st = s.borrow();
            (st.code.clone().unwrap_or_else(|| code::write(&st.things)), st.code_gen, st.code_error.clone(), st.code_scroll.clone())
        };
        let color = |k: &str| cx.lang.color(&format!("color.code_{k}")).map(|c| c >> 8);
        let colors = code::Colors {
            keyword: color("keyword"),
            call: color("call"),
            string: color("string"),
            number: color("number"),
            comment: color("comment"),
        };
        let format: crate::ui::Format = Rc::new(move |src: &str| code::highlight(src, &colors));
        let error_h = if error.is_some() { 24.0 } else { 0.0 };
        ui.container(Style::default().gap(3.0).w(w).h(tall), |ui| {
            ui.container(Style::default().w(w).h(tall - error_h), |ui| {
                let area = ui.add(Kind::text_area(&value, 20_000, Some(format)), Style::default().w(w).h(tall - error_h));
                let s2 = s.clone();
                if let Kind::Input { on_change, area: Some(a), .. } = &mut ui.nodes[area].kind {
                    a.scroll = scroll;
                    // Lines stay clear of the copy button in the corner.
                    a.inset = 22.0;
                    *on_change = Some(Rc::new(move |c: &mut Ctx, v: &str| {
                        let mut st = s2.borrow_mut();
                        st.code = Some(v.to_string());
                        match code::read(v) {
                            Ok(things) => {
                                st.things = things;
                                st.picked = st.picked.min(st.things.len() - 1);
                                st.code_error = None;
                                st.form_gen += 1;
                            }
                            Err(e) => st.code_error = Some(e),
                        }
                        c.requests.push(Request::Rebuild);
                    }));
                }
                ui.set_id(area, &format!("editor_code:{generation}"));
                let all = value.clone();
                let b = icon_button(ui, cx, "button_install", "icon_copy", "editor.copy", &[], move |_| {
                    crate::clipboard::set(&all);
                });
                ui.nodes[b].style = ui.nodes[b].style.at_from([1.0, 0.0], -4.0, 4.0, [1.0, 0.0]);
                ui.set_id(b, "editor_copy");
            });
            if let Some(e) = &error {
                label(ui, cx, "editor.code_error", &[Arg::Text(e.clone())], "warning");
            }
        });
    }
}

/// A one-line field calling `on` with what it holds after each change.
fn field(ui: &mut Ui, value: &str, w: f32, id: &str, on: impl Fn(&mut Ctx, &str) + 'static) -> crate::ui::NodeId {
    let input = ui.add(Kind::input(value, 200), Style::default().h(FIELD_H).w(w));
    if let Kind::Input { on_change, .. } = &mut ui.nodes[input].kind {
        *on_change = Some(Rc::new(on));
    }
    ui.set_id(input, id);
    input
}

/// The thing as it would look: a block as its model, an item as its picture.
fn preview(ui: &mut Ui, c: &Content, index: usize, t: &Thing, side: f32) {
    if t.kind == What::Block {
        ui.add(Kind::Custom { tag: TAG_BLOCK, data: index as u64 }, Style::default().size(side, side));
    } else {
        let tex = t
            .picture()
            .and_then(|p| c.textures.iter().position(|n| *n == p))
            .unwrap_or(0);
        ui.add(Kind::Custom { tag: TAG_PICTURE, data: tex as u64 }, Style::default().size(side, side));
    }
}

/// Texture `data` of `Content::textures`, flat.
pub const TAG_PICTURE: u32 = 21;

/// Thing `data` of the session drawn as a slot shows a block: its boxes, its picture on them.
pub const TAG_BLOCK: u32 = 20;

pub fn draw_block(
    ren: &mut crate::ui::render::UiRenderer,
    c: &Content,
    textures: &crate::client::textures::GameTextures,
    r: crate::ui::Rect,
    data: u64,
) {
    let Some(t) = SESSION.with(|s| s.borrow().things.get(data as usize).cloned()) else { return };
    let tex = t
        .picture()
        .and_then(|p| c.textures.iter().position(|n| *n == p))
        .unwrap_or(0) as u32;
    let boxes: Vec<crate::content::ModelBox> = t
        .boxes()
        .iter()
        .map(|b| {
            let mut m = crate::content::ModelBox::whole([tex; 6]);
            m.min = [b[0] / 16.0, b[1] / 16.0, b[2] / 16.0];
            m.max = [b[3] / 16.0, b[4] / 16.0, b[5] / 16.0];
            m
        })
        .collect();
    ren.block_icon(c, textures, r, &boxes);
}
