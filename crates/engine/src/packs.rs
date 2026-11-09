//! Player-chosen client packs. Resource packs (`resourcepacks/<id>/`) are laid out like mod assets
//! per namespace (`<ns>/textures/...`, `<ns>/lang/...`, `.ctm`, GUI textures) and stack in the order the
//! player picks. A shader pack (`shaderpacks/<id>/`) replaces engine shaders file by file
//! (`world.wgsl`, `entity.wgsl`, `model.wgsl`, `particle.wgsl`, `sky.wgsl`, `post.wgsl`, `ui.wgsl`) and has
//! to keep their entry points (structs and bindings come from the prelude); a file that fails to
//! compile falls back to the engine's own, an asset under `core:shaders/`.
//! Snippets mods ship (`shaders/<kind>/`, see `shaders.rs`) are assets, overridden by resource
//! packs like textures. Both may have a `pack.cfg` with
//! `name = ...` and `description = ...`.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::RwLock;

use crate::assets::Assets;

use weft_render::{Shader, ShaderError};

use crate::app::{Ctx, Request};
use crate::screens::{BuildCx, Screen, button, frame, label};
use crate::ui::text::Arg;
use crate::ui::{Justify, Kind, Style, Ui};

pub struct PackInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

pub fn list(dir: &Path) -> Vec<PackInfo> {
    let mut out: Vec<PackInfo> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            let cfg = std::fs::read_to_string(e.path().join("pack.cfg")).unwrap_or_default();
            let get = |key: &str| crate::kv::get(&cfg, key).map(str::to_string);
            PackInfo { name: get("name").unwrap_or_else(|| id.clone()), description: get("description").unwrap_or_default(), id }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn resource_dir(run_dir: &Path) -> PathBuf {
    run_dir.join("resourcepacks")
}

pub fn shader_dir(run_dir: &Path) -> PathBuf {
    run_dir.join("shaderpacks")
}

/// Folders of the enabled resource packs that exist, highest priority first.
pub fn enabled(run_dir: &Path, ids: &[String]) -> Vec<PathBuf> {
    ids.iter()
        .map(|id| resource_dir(run_dir).join(id))
        .filter(|p| p.is_dir())
        .collect()
}

static SHADER_PACK: RwLock<Option<PathBuf>> = RwLock::new(None);

pub fn set_shader_pack(dir: Option<PathBuf>) {
    *SHADER_PACK.write().unwrap() = dir;
}

/// The chosen pack's `name` mentions `what` (features the engine only pays for when used).
pub fn pack_uses(name: &str, what: &str) -> bool {
    pack_source(name).is_some_and(|s| s.contains(what))
}

fn pack_source(name: &str) -> Option<String> {
    let path = SHADER_PACK.read().unwrap().as_ref().map(|d| d.join(name))?;
    std::fs::read_to_string(path)
        .ok()
        .map(|s| without_bom(&s).to_string())
}

/// Windows editors may save a file with a byte order mark, which WGSL has no place for.
fn without_bom(src: &str) -> &str {
    src.strip_prefix('\u{feff}').unwrap_or(src)
}

/// An engine shader as an asset (`core:shaders/<name>`) like everything else: development runs
/// read the source tree, so edits show after a reload.
fn engine_source(assets: &Assets, name: &str) -> String {
    let asset = format!("core:{}", name.trim_end_matches(".wgsl"));
    assets
        .read(&asset, "shaders", "wgsl")
        .map(|b| without_bom(&String::from_utf8_lossy(&b)).to_string())
        .unwrap_or_else(|| {
            log::error!("engine shader {name} is missing from the assets");
            String::new()
        })
}

/// An engine shader no pack replaces, after `prelude`.
pub fn engine_shader(assets: &Assets, name: &str, prelude: &str) -> Result<Shader, ShaderError> {
    Shader::from_wgsl(name, &format!("{prelude}{}", engine_source(assets, name)))
}

/// What every engine shader starts with: its prelude (structs, bindings, constants) and
/// `common.wgsl` (the lightmap, fog, the structs snippets get), so a shader only writes functions.
fn with_common(assets: &Assets, name: &str, src: &str) -> String {
    format!("{}{src}", crate::gpu_layout::prelude(name, &engine_source(assets, "common.wgsl")))
}

/// `name` from the shader pack if it has a valid one, else the engine's.
pub fn shader(assets: &Assets, name: &str) -> Result<Shader, ShaderError> {
    if let Some(src) = pack_source(name) {
        match Shader::from_wgsl(name, &with_common(assets, name, &src)) {
            Ok(s) => return Ok(s),
            Err(e) => log::error!("shader pack {name}: {}", e.0),
        }
    }
    Shader::from_wgsl(name, &with_common(assets, name, &engine_source(assets, name)))
}

/// `shader` for engine shaders that take mod snippets (`shaders::compose`).
pub fn compose(
    assets: &Assets,
    name: &str,
    parts: &[(&crate::shaders::Hook, &[(String, String)])],
) -> Result<(Shader, Vec<crate::shaders::Slots>), ShaderError> {
    if let Some(src) = pack_source(name) {
        match crate::shaders::compose(name, &with_common(assets, name, &src), parts) {
            Ok(made) => return Ok(made),
            Err(e) => log::error!("shader pack {name}: {}", e.0),
        }
    }
    crate::shaders::compose(name, &with_common(assets, name, &engine_source(assets, name)), parts)
}

pub(crate) fn open_folder(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    open_path(dir);
}

/// Opens a file or folder the way the system does (Explorer, Finder, xdg-open).
pub(crate) fn open_path(path: &Path) {
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(opener).arg(path).spawn() {
        log::warn!("opening {}: {e}", path.display());
    }
}

fn pack_text(ui: &mut Ui, cx: &BuildCx, info: &PackInfo, name_class: &str) {
    ui.text(cx.rich(&info.name), name_class);
    if !info.description.is_empty() {
        ui.text(cx.rich(&info.description), "dim");
    }
}

/// A pack's row with the handle it is dragged by; `dragged` leaves it dim under the cursor's copy.
fn pack_row(ui: &mut Ui, cx: &BuildCx, info: &PackInfo, dragged: bool) {
    let row = ui.panel("button", Style::row().pad(4.0).gap(4.0).stretch(), |ui| {
        ui.container(Style::default().gap(1.0).w(198.0), |ui| pack_text(ui, cx, info, if dragged { "dim" } else { "default" }));
        ui.container(Style::default().grow(), |_| {});
        let icon = cx.look.texture("icon_drag", "");
        let handle = ui.button("button", Style::default().w(20.0).h(20.0), Rc::new(|_: &mut Ctx| {}), |ui| {
            if let Some(texture) = icon {
                ui.add(Kind::Image { texture, color: 0xFFFFFFFF }, Style::default().w(9.0).h(9.0));
            }
        });
        ui.set_id(handle, &format!("pack_handle:{}", info.id));
    });
    ui.set_id(row, &format!("pack:{}", info.id));
}

/// Where the cursor's copy of a dragged pack goes: its handle under the cursor.
fn ghost_at(cursor: glam::Vec2) -> [f32; 4] {
    [cursor.x + 16.0, cursor.y, 1.0, 0.5]
}

/// Available and selected resource packs side by side; the top selected pack wins. Packs move by
/// their handles, and the lists show where one would land while it is dragged. Put into effect
/// when the screen closes (`app::Running::apply_settings`).
#[derive(Default)]
pub struct ResourcePacks {
    dragging: Option<String>,
    cursor: glam::Vec2,
}

impl Screen for ResourcePacks {
    fn name(&self) -> &str {
        "core:resource_packs"
    }

    fn pauses(&self) -> bool {
        true
    }

    fn pointer(&mut self, cursor: glam::Vec2, ui: &mut Ui, ctx: &mut Ctx, _hud: &mut crate::hud::Hud) -> bool {
        let Some(id) = self.dragging.clone() else {
            let handle = ui
                .hit(cursor)
                .and_then(|h| ui.nodes[h].id.as_deref())
                .and_then(|i| i.strip_prefix("pack_handle:"));
            let Some(id) = handle.filter(|_| ctx.input.pressed_button(0)) else { return false };
            self.dragging = Some(id.to_string());
            self.cursor = cursor;
            ctx.requests.push(Request::Rebuild);
            return true;
        };
        ui.hovered = None;
        if !ctx.input.held_button(0) {
            self.dragging = None;
            ctx.requests.push(Request::Rebuild);
            return true;
        }
        if cursor != self.cursor {
            self.cursor = cursor;
            if let Some(ghost) = ui.find("pack_ghost") {
                ui.nodes[ghost].style.at = Some(ghost_at(cursor));
                ui.dirty = true;
            }
        }
        let (Some(available), Some(chosen)) = (ui.find("packs_available"), ui.find("packs_selected")) else { return true };
        let (left, right) = (ui.nodes[available].rect, ui.nodes[chosen].rect);
        let before = crate::settings::resource_packs();
        let mut packs: Vec<String> = before.iter().filter(|p| **p != id).cloned().collect();
        if cursor.x >= (left.x + left.w + right.x) / 2.0 {
            let above = packs
                .iter()
                .filter_map(|p| ui.find(&format!("pack:{p}")))
                .filter(|&n| ui.nodes[n].rect.y + ui.nodes[n].rect.h / 2.0 < cursor.y)
                .count();
            packs.insert(above, id);
        }
        if packs != before {
            crate::settings::set_resource_packs(&packs);
            ctx.requests.push(Request::Rebuild);
        }
        true
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let dir = resource_dir(&cx.settings.run_dir);
        let all = list(&dir);
        let selected = crate::settings::resource_packs();
        let missing =
            |id: &str| PackInfo { id: id.to_string(), name: id.to_string(), description: cx.lang.get("packs.missing").to_string() };
        let dragging = self.dragging.as_deref();
        frame(ui, cx, "settings.resource_packs", |ui| {
            ui.container(Style::row().gap(8.0).grow().h(0.0).stretch(), |ui| {
                ui.container(Style::default().gap(4.0).w(260.0).stretch(), |ui| {
                    label(ui, cx, "packs.available", &[], "default");
                    let panel = ui.panel(
                        "panel",
                        Style::default()
                            .pad(6.0)
                            .gap(4.0)
                            .stretch()
                            .grow()
                            .h(0.0)
                            .scroll(),
                        |ui| {
                            let available: Vec<&PackInfo> = all.iter().filter(|p| !selected.contains(&p.id)).collect();
                            if available.is_empty() {
                                label(ui, cx, if all.is_empty() { "packs.none" } else { "packs.all_selected" }, &[], "dim");
                            }
                            for p in available {
                                pack_row(ui, cx, p, dragging == Some(p.id.as_str()));
                            }
                        },
                    );
                    ui.set_id(panel, "packs_available");
                });
                ui.container(Style::default().gap(4.0).w(260.0).stretch(), |ui| {
                    label(ui, cx, "packs.selected", &[], "default");
                    let panel = ui.panel(
                        "panel",
                        Style::default()
                            .pad(6.0)
                            .gap(4.0)
                            .stretch()
                            .grow()
                            .h(0.0)
                            .scroll(),
                        |ui| {
                            for id in &selected {
                                let gone = missing(id);
                                pack_row(ui, cx, all.iter().find(|p| &p.id == id).unwrap_or(&gone), dragging == Some(id.as_str()));
                            }
                        },
                    );
                    ui.set_id(panel, "packs_selected");
                });
            });
            ui.container(Style::row().gap(6.0).justify(Justify::Center), |ui| {
                button(ui, cx, "packs.open_folder", &[], 140.0, move |_| open_folder(&dir));
                button(ui, cx, "menu.done", &[], 140.0, |c| c.requests.push(Request::Close));
            });
        });
        if let Some(id) = dragging {
            let gone = missing(id);
            let info = all.iter().find(|p| p.id == id).unwrap_or(&gone);
            let mut style = Style::default().w(234.0).pad(4.0).gap(1.0);
            style.at = Some(ghost_at(self.cursor));
            let ghost = ui.panel("button", style, |ui| pack_text(ui, cx, info, "default"));
            ui.set_id(ghost, "pack_ghost");
        }
    }
}

/// One shader pack or the built-in shaders.
pub struct ShaderPacks;

impl Screen for ShaderPacks {
    fn name(&self) -> &str {
        "core:shader_packs"
    }

    fn pauses(&self) -> bool {
        true
    }

    fn build(&mut self, ui: &mut Ui, cx: &BuildCx) {
        let dir = shader_dir(&cx.settings.run_dir);
        let all = list(&dir);
        let current = crate::settings::shader_pack();
        frame(ui, cx, "settings.shader_packs", |ui| {
            ui.panel(
                "panel",
                Style::default()
                    .pad(8.0)
                    .gap(4.0)
                    .w(320.0)
                    .h(0.0)
                    .grow()
                    .min_h(60.0)
                    .scroll()
                    .stretch(),
                |ui| {
                    let choices = std::iter::once((None, cx.t("packs.shaders_off", &[]), String::new())).chain(
                        all.iter()
                            .map(|p| (Some(p.id.clone()), cx.rich(&p.name), p.description.clone())),
                    );
                    for (id, name, description) in choices {
                        let selected = id == current;
                        let b = ui.button(
                            "button",
                            Style::default().min_h(20.0).pad_xy(6.0, 4.0).stretch(),
                            Rc::new(move |c: &mut Ctx| {
                                crate::settings::set_text("shader_pack", id.as_deref().unwrap_or(""));
                                c.requests.push(Request::Rebuild);
                            }),
                            |ui| {
                                ui.text(name, "button");
                                if !description.is_empty() {
                                    ui.text(cx.rich(&description), "button");
                                }
                            },
                        );
                        if let Kind::Button { selected: sel, .. } = &mut ui.nodes[b].kind {
                            *sel = selected;
                        }
                    }
                },
            );
            label(
                ui,
                cx,
                "packs.shaders_note",
                &[Arg::Text("world.wgsl, entity.wgsl, model.wgsl, particle.wgsl, weather.wgsl, sky.wgsl, post.wgsl".into())],
                "dim",
            );
            ui.container(Style::row().gap(6.0).justify(Justify::Center), |ui| {
                button(ui, cx, "packs.open_folder", &[], 140.0, move |_| open_folder(&dir));
                button(ui, cx, "menu.done", &[], 140.0, |c| c.requests.push(Request::Close));
            });
        });
    }
}
