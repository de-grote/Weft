//! Batched events and placement by facing; the other hooks are checked in the scene.

use std::collections::HashMap;
use std::time::Duration;

use super::{add, at};
use crate::runner::{R, T};

fn counts(s: &str) -> HashMap<String, u32> {
    s.split_whitespace()
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.parse().unwrap_or(0)))
        .collect()
}

pub async fn events_batched(t: T) -> R {
    t.flat().await?;
    t.op("events reset").await?;
    let f = t.feet();
    for i in 0..3 {
        let p = add(f, [3, 0, i]);
        t.set(p, "stone").await?;
        t.op(&format!("break {} stone_pickaxe", at(p))).await?;
    }
    t.op("put me 0 dirt 4").await?;
    t.c().select_slot(0);
    let ground = add(f, [0, -1, -2]);
    t.look_at(ground);
    t.frames(3).await?;
    t.press("core:use");
    t.until("dirt placed", Duration::from_secs(5), |t| t.c().inventory_slot(0).count == 3)
        .await?;
    t.cmd("/feed 10").await?;
    t.op("put me 1 bread 2").await?;
    t.c().select_slot(1);
    t.secs(0.3).await?;
    t.hold("core:use", 2.0);
    t.until("ate", Duration::from_secs(5), |t| t.c().inventory_slot(1).count == 1)
        .await?;
    // The broken stone dropped cobblestone next to the player.
    t.c()
        .teleport([f[0] as f64 + 3.5, f[1] as f64, f[2] as f64 + 1.5]);
    t.secs(1.0).await?;
    let n = counts(&t.op("events").await?);
    t.check_eq(n["block_broken"], 3, "block_broken events");
    t.check_eq(n["block_placed"], 1, "block_placed events");
    t.check_eq(n["item_used"], 1, "item_used events (eating; placing a block is not one)");
    t.check(n["item_picked_up"] >= 1, format!("item_picked_up events: {}", n["item_picked_up"]));
    Ok(())
}

/// Status effects: modifiers times the level, a level given its own replacing those of that
/// attribute only, gone with the effect; a potion drunk gives its effect and leaves its bottle.
pub async fn status_effects(t: T) -> R {
    t.flat().await?;
    let attr = |t: &T, a: &str| {
        let t = t.clone();
        let a = a.to_string();
        async move {
            let v = t.op(&format!("attr me {a}")).await?;
            Ok::<f64, String>(
                v.split(' ')
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(f64::NAN),
            )
        }
    };
    t.cmd("/effect give @s base:speed 30 2").await?;
    t.cmd("/effect give @s base:mining_fatigue 30 1").await?;
    t.secs(0.3).await?;
    t.check((attr(&t, "core:movement_speed").await? - 1.4).abs() < 1e-6, "speed II: 20% a level");
    t.check((attr(&t, "core:mining_speed").await? - 0.3).abs() < 1e-6, "mining fatigue I: its own level's");
    t.check((attr(&t, "core:attack_speed").await? - 0.9).abs() < 1e-6, "and its attack speed times the level beside it");
    t.cmd("/effect clear @s").await?;
    t.secs(0.3).await?;
    t.check((attr(&t, "core:movement_speed").await? - 1.0).abs() < 1e-6, "cleared, back to normal");
    t.op("put me 0 potion_strength 1").await?;
    t.c().select_slot(0);
    t.hold("core:use", 2.5);
    t.until("drank", Duration::from_secs(5), |t| t.c().inventory_slot(0).item == weft_sdk::Item::named("base:glass_bottle").id())
        .await?;
    t.secs(0.3).await?;
    t.check((attr(&t, "core:attack_damage").await? - 3.0).abs() < 1e-6, "the potion's strength");
    Ok(())
}

/// The questions before things happen and timers: a denied `HURT` keeps a player alive, a
/// denied `USE_ITEM` keeps the bread, a timer fires its event once, when due.
pub async fn vetoes_and_timers(t: T) -> R {
    t.flat().await?;
    t.cmd("/rule testkit:no_hurt 1").await?;
    t.cmd("/clear @s").await?;
    t.secs(0.3).await?;
    t.check(!t.flag(weft_sdk::client::client_flags::DEAD), "actions::HURT denied, the player lives");
    t.cmd("/rule testkit:no_hurt 0").await?;
    t.cmd("/feed 10").await?;
    t.op("put me 1 bread 2").await?;
    t.c().select_slot(1);
    t.cmd("/rule testkit:no_use 1").await?;
    t.secs(0.3).await?;
    t.press("core:use");
    t.secs(0.5).await?;
    t.check_eq(t.c().inventory_slot(1).count, 2, "actions::USE_ITEM denied, the bread stays");
    t.cmd("/rule testkit:no_use 0").await?;
    t.hold("core:use", 2.0);
    t.until("ate", Duration::from_secs(5), |t| t.c().inventory_slot(1).count == 1)
        .await?;
    let before: u32 = t.op("rings").await?.parse().unwrap_or(0);
    t.op("after 30").await?;
    t.op("after 20").await?;
    t.secs(0.5).await?;
    t.check_eq(t.op("rings").await?, before.to_string(), "not before it is due");
    t.secs(1.5).await?;
    t.check_eq(t.op("rings").await?, (before + 2).to_string(), "both timers fired");
    Ok(())
}

/// Block properties are components: testkit's `r.set` gives base's mossy cobblestone blue light,
/// and the engine's lighting uses it.
pub async fn block_properties(t: T) -> R {
    t.flat().await?;
    let p = add(t.feet(), [3, 0, 0]);
    t.set(p, "mossy_cobblestone").await?;
    t.frames(5).await?;
    let l: u32 = t
        .op(&format!("light {}", at(add(p, [0, 1, 0]))))
        .await?
        .parse()
        .map_err(|e| format!("light: {e}"))?;
    t.check_eq([(l >> 8) & 255, (l >> 16) & 255, l >> 24], [0, 0, 8], "block light above the overridden block (r, g, b)");
    Ok(())
}
