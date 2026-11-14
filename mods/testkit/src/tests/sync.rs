//! Machine progress reaches the client through menu-synced components.

use std::time::Duration;

use super::add;
use crate::runner::{R, T};

use base_api::SMELTING_STATE as SMELTING;
use weft_sdk::*;

/// Public stack data and slot order agree across display sources. Visual dependencies share
/// one conditional result, while unrelated public changes still reach the client.
pub async fn displayed_stacks(t: T) -> R {
    use crate::server::{MARK, SECRET};
    static DISPLAY_PHASE: Component<u32> = Component::named("testmod:display_phase");
    static DISPLAY_PARTS: Storage = Storage::named("testmod:display_parts");

    async fn look_calls(t: &T, sample: &str) -> Result<u32, String> {
        let reply = t.cmd(&format!("/display_calls {sample}")).await?;
        reply
            .split_whitespace()
            .last()
            .ok_or_else(|| format!("missing display count: {reply}"))?
            .parse()
            .map_err(|e| format!("display count: {reply}: {e}"))
    }

    t.flat().await?;
    let f = t.feet();
    let carrier: u64 = t
        .op(&format!("spawn testmod:display_carrier {} {} {}", f[0] - 2, f[1], f[2] - 3))
        .await?
        .parse()
        .map_err(|e| format!("carrier: {e}"))?;
    let p = add(f, [2, 0, -3]);
    t.set(p, "testkit:pedestal").await?;
    let block = format!("{} {} {}", p[0], p[1], p[2]);
    let slots = [
        ("me".to_string(), 0),
        ("me".to_string(), 36),
        (carrier.to_string(), 0),
        (carrier.to_string(), 2),
        (carrier.to_string(), 6),
        (block.clone(), 0),
    ];
    let mut pedestal = 0;
    for (owner, slot) in &slots {
        t.op(&format!("put {owner} {slot} testmod:display_token 3"))
            .await?;
        pedestal = t
            .op(&format!("display_data {owner} {slot} 0 0 41"))
            .await?
            .parse()
            .map_err(|e| format!("display owner: {e}"))?;
    }
    t.until("all display sources", Duration::from_secs(5), |t| {
        t.c()
            .displayed(pedestal, DisplaySource::Shown)
            .first()
            .is_some_and(|s| DISPLAY_PHASE.on_stack(s) == Some(0))
            && t.c()
                .displayed(carrier, DisplaySource::Equipment)
                .first()
                .is_some_and(|s| DISPLAY_PHASE.on_stack(s) == Some(0))
            && t.c().held(t.state().player, Hand::Main).count == 3
    })
    .await?;
    let dropped: u64 = t
        .op(&format!("drop_stack {carrier} 0 {} {} {}", f[0], f[1] + 2, f[2] - 6))
        .await?
        .parse()
        .map_err(|e| format!("drop: {e}"))?;
    t.until("dropped stack data", Duration::from_secs(5), |t| {
        t.c()
            .displayed(dropped, DisplaySource::Ground)
            .first()
            .is_some_and(|s| DISPLAY_PHASE.on_stack(s) == Some(0))
    })
    .await?;
    let reference = t.c().held(carrier, Hand::Main);
    let player = t.state().player;
    for (id, source) in [
        (player, DisplaySource::Hands),
        (player, DisplaySource::Equipment),
        (carrier, DisplaySource::Hands),
        (carrier, DisplaySource::Equipment),
        (carrier, DisplaySource::Shown),
        (pedestal, DisplaySource::Shown),
        (dropped, DisplaySource::Ground),
    ] {
        let stacks = t.c().displayed(id, source);
        t.check(stacks.iter().any(|s| *s == reference), format!("{id} {source:?}: the same public stack"));
        for stack in stacks.iter().filter(|s| !s.is_empty()) {
            t.check_eq(SECRET.on_stack_own(stack), None, "private stack data is absent");
            let parts = stack.stacks(&DISPLAY_PARTS);
            t.check_eq(parts.len(), 1, "one nested stack");
            t.check(parts.iter().all(|s| SECRET.on_stack_own(s).is_none()), "nested private data is absent");
        }
    }
    let shown = t.c().displayed(carrier, DisplaySource::Shown);
    t.check(shown.len() == 2 && shown[0].is_empty() && shown[1] == reference, "shown slots preserve order and empties");
    t.check_eq(t.c().displayed(carrier, DisplaySource::Equipment).len(), 4, "equipment slot count");
    t.frames(5).await?;
    let calls = look_calls(&t, "initial").await?;
    t.op(&format!("display_data {carrier} 0 0 10 42")).await?;
    t.until("nonvisual public data", Duration::from_secs(5), |t| MARK.on_stack(&t.c().held(carrier, Hand::Main)) == Some(10))
        .await?;
    t.frames(5).await?;
    t.check_eq(look_calls(&t, "nonvisual").await?, calls, "nonvisual data reuses the conditional look");
    let before = t.c().held(carrier, Hand::Main);
    t.op(&format!("display_data {carrier} 0 0 10 43")).await?;
    t.frames(5).await?;
    t.check_eq(t.c().held(carrier, Hand::Main), before, "private changes preserve the public stack");
    for (owner, slot) in &slots {
        t.op(&format!("display_data {owner} {slot} 1 0 44")).await?;
    }
    t.until("visual data reaches the holder", Duration::from_secs(5), |t| {
        DISPLAY_PHASE.on_stack(&t.c().held(carrier, Hand::Main)) == Some(1)
    })
    .await?;
    t.frames(5).await?;
    t.check_eq(look_calls(&t, "visual").await?, calls + 1, "all uses share the new conditional look");
    t.c().set_look(0.0, -10.0);
    t.shot("stack_models").await?;

    // Text views and owned strings use the same portable stack representation.
    t.op("put me 0 stick 3").await?;
    t.op("display_data me 0 2 0 45").await?;
    t.until("a per-stack model", Duration::from_secs(5), |t| {
        MODEL
            .on_stack_own(&t.c().held(player, Hand::Main))
            .is_some()
    })
    .await?;
    let stack = t.c().held(player, Hand::Main);
    static MODEL_TEXT: Component<String> = Component::named("model");
    t.check_eq(MODEL.on_stack_own(&stack).map(|s| s.as_str()), Some("testmod:katana"), "portable model text view");
    t.check_eq(MODEL_TEXT.on_stack_own(&stack).as_deref(), Some("testmod:katana"), "portable owned model text");
    t.frames(5).await?;
    t.shot("stack_icon").await?;
    for _ in 0..2 {
        t.op(&format!("drop_stack me 0 {} {} {}", f[0] + 1, f[1] + 2, f[2] - 6))
            .await?;
    }
    t.until("merged dropped stack count", Duration::from_secs(5), |t| {
        t.c().entities().iter().any(|&id| {
            t.c()
                .displayed(id, DisplaySource::Ground)
                .iter()
                .any(|s| s.item == stack.item && s.data == stack.data && s.count == 6)
        })
    })
    .await?;
    for slot in [0, 2, 6] {
        t.op(&format!("put {carrier} {slot} stick 0")).await?;
    }
    t.until("cleared display slots", Duration::from_secs(5), |t| {
        [DisplaySource::Hands, DisplaySource::Equipment, DisplaySource::Shown]
            .iter()
            .all(|&source| {
                t.c()
                    .displayed(carrier, source)
                    .iter()
                    .all(ItemStack::is_empty)
            })
    })
    .await
}

pub async fn menu_sync(t: T) -> R {
    t.flat().await?;
    let p = add(t.feet(), [2, 0, -2]);
    t.set(p, "furnace").await?;
    t.op(&format!("insert {} {} {} raw_gold 2", p[0], p[1], p[2]))
        .await?;
    t.op(&format!("insert {} {} {} coal 1", p[0], p[1], p[2]))
        .await?;
    t.op(&format!("menu base:furnace {} {} {}", p[0], p[1], p[2]))
        .await?;
    t.screen("core:menu").await?;
    t.until("furnace progress on the client", Duration::from_secs(5), |t| {
        t.c()
            .view()
            .menu_component(&SMELTING)
            .is_some_and(|s| s.cook > 0)
    })
    .await?;
    let first = t.c().view().menu_component(&SMELTING).map_or(0, |s| s.cook);
    t.secs(1.0).await?;
    let later = t.c().view().menu_component(&SMELTING).map_or(0, |s| s.cook);
    t.check(later > first + 10, format!("cook progress {first} -> {later} in a second (20 ticks)"));
    t.check(
        t.c()
            .view()
            .menu_component(&SMELTING)
            .is_some_and(|s| s.burn_max == 1600),
        "coal burn time synced",
    );
    t.shot("progress").await?;
    t.c().menu_close();
    t.no_screen().await
}
