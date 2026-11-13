use std::hash::{Hash, Hasher};

use weft_sdk::*;

static HARVEST_LEVEL: Component<u32> = Component::named(abi::comp::HARVEST_LEVEL);

pub fn register(r: &mut Registry) {
    r.hud_element(&base_api::TARGET_INFO, hud_anchor::TOP_CENTER, 0, |ui| {
        let Some(target) = ui.view().target() else { return };
        let name = if target.state != 0 {
            block_info(target.state).map(|b| (format!("block:{}", b.name), b.owner.to_string(), b.item))
        } else {
            ui.view().client().entity(target.entity).map(|e| {
                let name = name_of(abi::entry_kind::ENTITY, e.kind);
                (format!("entity:{name}"), name.split_once(':').map_or("", |s| s.0).to_string(), None)
            })
        };
        let Some((name, owner, item)) = name else { return };
        ui.panel("panel", S::column().pad(4.0).gap(2.0), |ui| {
            ui.row(S::row().gap(4.0), |ui| {
                if let Some(item) = item {
                    ui.item(item, 1, S::default().size(16.0, 16.0));
                }
                ui.column(S::column().gap(2.0), |ui| {
                    ui.text_args("hud.base.target.name", &[Arg::Name(&name)]);
                    ui.text_styled(S::default().class("dim"), "hud.base.target.name", &[Arg::Name(&format!("mod:{owner}"))]);
                });
            });
            if target.state != 0 {
                let level = HARVEST_LEVEL.on_block(target.state).unwrap_or(0);
                for (tag, schema) in components_of(abi::entry_kind::BLOCK, target.state) {
                    if let Some(tool) = tag.strip_prefix("mineable/").filter(|_| schema.is_empty()) {
                        ui.text_args("hud.base.target.tool", &[Arg::Key(&format!("tool.{tool}")), Arg::Int(level as i64)]);
                    }
                }
            } else {
                ui.text_bind(move |c, out| {
                    if let Some(e) = c.client().entity(target.entity) {
                        out.set("hud.base.target.health", &[Arg::Float(e.health as f64, 0), Arg::Float(e.max_health as f64, 0)]);
                    }
                });
            }
            ui.screen_parts(base_api::TARGET_INFO.name(), "details");
        });
    })
    .rebuild_when(|c| {
        let mut hash = std::hash::DefaultHasher::new();
        c.target().hash(&mut hash);
        hash.finish()
    });
}
