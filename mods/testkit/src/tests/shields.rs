use crate::runner::{R, T};
use std::time::Duration;
use weft_sdk::*;

pub async fn shields(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    t.c().set_look(0.0, 0.0);
    t.op("put me 0 base:shield 1").await?;
    t.c().select_slot(0);
    t.sync_inventory().await?;
    t.secs(0.3).await?;
    t.shot("main_idle").await?;
    t.hold("core:use", 10.0);
    t.secs(0.3).await?;
    t.shot("main_block").await?;
    t.c().input_release_all();
    t.press("core:perspective");
    t.secs(0.2).await?;
    t.press("core:perspective");
    t.secs(0.3).await?;
    t.shot("third_idle").await?;
    t.hold("core:use", 10.0);
    t.secs(0.3).await?;
    t.shot("third_block").await?;
    t.c().input_release_all();
    t.press("core:perspective");
    t.secs(0.3).await?;
    let front = t
        .op(&format!("spawn core:player {} {} {}", f[0], f[1], f[2] - 6))
        .await?;
    let back = t
        .op(&format!("spawn core:player {} {} {}", f[0], f[1], f[2] + 6))
        .await?;
    t.check_eq(
        t.op(&format!("hit_from {front} 1 base:stick {} main", abi::damage_cause::ATTACK))
            .await?,
        "0".into(),
        "activation blocks in the same server call",
    );
    t.hold("core:use", 30.0);
    t.secs(0.15).await?;
    let health = t.state().health;
    for damage in [1, 3, 4, 7] {
        t.check_eq(
            t.op(&format!("hit_from {front} {damage} base:stick"))
                .await?,
            "0".into(),
            "front hit blocked",
        );
    }
    t.secs(0.15).await?;
    t.check_eq(t.state().health, health, "blocked hits preserve health");
    t.check_eq(t.c().inventory_slot(0).wear(), 13, "only hits above threshold wear the shield");
    t.check_eq(t.op(&format!("hit_from {back} 2 base:stick")).await?, "1".into(), "rear hit lands");
    t.secs(0.6).await?;
    let pos = t.state().pos;
    t.op(&format!("shoot base:arrow {} {} {} 0 0 35", pos.x, pos.y + 1.2, pos.z - 1.5))
        .await?;
    t.until("projectile retained in shield stack", Duration::from_secs(3), |t| {
        !comps::EMBEDDED_PROJECTILES
            .on_stack(&t.c().inventory_slot(0))
            .unwrap_or_default()
            .is_empty()
    })
    .await?;
    t.shot("blocking_arrow").await?;
    t.op(&format!("remove {front}")).await?;
    t.op(&format!("remove {back}")).await?;
    t.press("core:perspective");
    t.secs(0.2).await?;
    t.press("core:perspective");
    t.secs(0.3).await?;
    t.shot("embedded_front").await?;
    t.press("core:perspective");
    t.secs(0.2).await?;
    t.c().input_release_all();
    t.secs(0.3).await?;
    t.shot("lowered_arrow").await?;
    t.check_eq(
        comps::EMBEDDED_PROJECTILES
            .on_stack(&t.c().inventory_slot(0))
            .unwrap_or_default()
            .len(),
        1,
        "lowering retains projectile",
    );
    let front = t
        .op(&format!("spawn core:player {} {} {}", f[0], f[1], f[2] - 6))
        .await?;
    let back = t
        .op(&format!("spawn core:player {} {} {}", f[0], f[1], f[2] + 6))
        .await?;
    t.hold("core:use", 30.0);
    t.secs(0.15).await?;
    t.check_eq(t.op(&format!("hit_from {front} 4 base:iron_axe")).await?, "0".into(), "axe hit is blocked");
    t.secs(0.2).await?;
    t.hold("core:use", 30.0);
    t.secs(0.2).await?;
    t.check_eq(t.op(&format!("hit_from {front} 2 base:stick")).await?, "1".into(), "axe disables further blocks");
    t.c().input_release_all();
    t.secs(5.0).await?;
    t.hold("core:use", 30.0);
    t.secs(0.2).await?;
    t.check_eq(t.op(&format!("hit_from {front} 2 base:stick")).await?, "0".into(), "blocking recovers after cooldown");
    t.c().input_release_all();
    t.press("core:swap_hands");
    t.secs(0.3).await?;
    t.shot("offhand_idle").await?;
    t.press("core:perspective");
    t.secs(0.2).await?;
    t.press("core:perspective");
    t.secs(0.3).await?;
    t.shot("offhand_third_idle").await?;
    t.hold("core:use", 10.0);
    t.secs(0.3).await?;
    t.shot("offhand_third_block").await?;
    t.c().input_release_all();
    t.press("core:perspective");
    t.secs(0.3).await?;
    t.hold("core:use", 30.0);
    t.secs(0.2).await?;
    t.check_eq(
        t.op(&format!("hit_from {front} 5 base:stick {} off", abi::damage_cause::EXPLOSION))
            .await?,
        "0".into(),
        "offhand blocks frontal explosion damage",
    );
    t.check_eq(
        t.op(&format!("hit_from {back} 2 base:stick {} off 0 0 1", abi::damage_cause::EXPLOSION))
            .await?,
        "0".into(),
        "blast direction takes precedence over the igniter behind",
    );
    t.shot("offhand").await?;
    t.c().input_release_all();
    t.press("core:swap_hands");
    t.secs(0.3).await?;
    let saved = t.c().inventory_slot(0);
    t.c().world_leave();
    t.screen("core:title").await?;
    t.c().world_open(&format!("test-{}", t.name()), 1);
    t.loaded().await?;
    t.sync_inventory().await?;
    let restored = t.c().inventory_slot(0);
    t.check_eq(restored.wear(), saved.wear(), "shield wear survives reopening");
    t.check_eq(
        comps::EMBEDDED_PROJECTILES.on_stack(&restored),
        comps::EMBEDDED_PROJECTILES.on_stack(&saved),
        "embedded projectiles survive reopening",
    );
    t.op("wear me 0 335").await?;
    let f = t.feet();
    let source = t
        .op(&format!("spawn core:player {} {} {}", f[0], f[1], f[2] - 3))
        .await?;
    t.c().set_look(0.0, 0.0);
    t.hold("core:use", 10.0);
    t.secs(0.2).await?;
    t.check_eq(t.op(&format!("hit_from {source} 4 base:stick")).await?, "0".into(), "last hit is blocked as the shield breaks");
    t.until("shield broke", Duration::from_secs(3), |t| t.c().inventory_slot(0).is_empty())
        .await?;
    t.c().input_release_all();
    Ok(())
}
