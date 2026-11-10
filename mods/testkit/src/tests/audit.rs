//! Bugs found by going through the game, each as it was reproduced then.

use std::time::Duration;

use weft_sdk::*;

use super::add;
use crate::runner::{R, T};

async fn reopen(t: &T, seed: u64) -> R {
    let name = format!("test-{}", t.name());
    t.c().world_leave();
    t.screen("core:title").await?;
    t.c().world_open(&name, seed);
    t.until("joined again", Duration::from_secs(60), |t| t.flag(client::client_flags::JOINED))
        .await?;
    t.loaded().await?;
    Ok(())
}

fn at(p: [i32; 3]) -> String {
    format!("{} {} {}", p[0], p[1], p[2])
}

/// What left a cube doesn't come back from its copy on disk: neither a removed mob nor a taken item.
pub async fn saved_entities_stay_gone(t: T) -> R {
    t.flat().await?;
    let p = add(t.feet(), [3, 0, 3]);
    let pig = t.op(&format!("spawn base:pig {}", at(p))).await?;
    t.secs(0.2).await?;
    reopen(&t, 1).await?;
    t.check(
        t.op(&format!("near {} 16", at(p)))
            .await?
            .contains("base:pig"),
        "the pig was saved",
    );
    t.op(&format!("remove {}", pig.trim())).await?;
    t.secs(0.2).await?;
    reopen(&t, 1).await?;
    let near = t.op(&format!("near {} 16", at(p))).await?;
    t.check(!near.contains("base:pig"), format!("the removed pig came back: {near}"));
    Ok(())
}

/// Dead is dead: nothing heals it or lets it pick things up, and what was on the cursor falls too.
pub async fn death_is_a_state(t: T) -> R {
    t.flat().await?;
    t.op("put me 0 diamond 5").await?;
    t.op("put me 1 gold_ingot 3").await?;
    t.press("core:inventory");
    t.screen("core:menu").await?;
    t.frames(3).await?;
    t.click_ui("slot:36", 0);
    t.until("on the cursor", Duration::from_secs(2), |t| t.c().inventory_slot(0).is_empty())
        .await?;
    t.cmd("/clear @s").await?;
    t.screen("core:death").await?;
    t.secs(5.0).await?;
    t.check_eq(t.op("health").await?, "0".to_string(), "still dead after a hunger pulse");
    t.check_eq(t.op("inv me").await?, String::new(), "picked up nothing, carried nothing");
    let f = t.feet();
    let near = t.op(&format!("near {} 8", at(f))).await?;
    t.check(near.matches("core:item").count() >= 2, format!("the cursor fell with the rest: {near}"));
    // And leaving doesn't undo it.
    reopen(&t, 1).await?;
    t.screen("core:death").await?;
    t.check_eq(t.op("health").await?, "0".to_string(), "dead after coming back");
    t.c().respawn();
    t.no_screen().await?;
    t.until("alive", Duration::from_secs(5), |t| t.state().health > 0.0)
        .await
}

/// Reach and mining speed are attributes, and client and server read the same ones.
pub async fn reach_and_mining_attributes(t: T) -> R {
    t.flat().await?;
    t.op("mode creative").await?;
    let far = add(t.feet(), [8, 0, 0]);
    t.set(far, "stone").await?;
    t.until("the block on the client", Duration::from_secs(2), |t| t.c().block(far).is_some_and(|s| s != 0))
        .await?;
    t.look_at(far);
    t.frames(3).await?;
    t.press("core:attack");
    t.secs(0.3).await?;
    t.check_eq(t.block(far).await?, "base:stone".to_string(), "out of reach at first");
    t.op("attr_mod me core:block_reach reach core:add 10 0")
        .await?;
    t.secs(0.2).await?;
    t.press("core:attack");
    t.until("reached", Duration::from_secs(2), |t| t.c().block(far).is_some_and(|s| s == 0))
        .await?;
    t.secs(0.2).await?;
    t.check_eq(t.block(far).await?, "core:air".to_string(), "and the server agrees");
    t.op("attr_rm me core:block_reach reach").await?;

    t.op("mode survival").await?;
    let near = add(t.feet(), [2, 0, 0]);
    t.set(near, "dirt").await?;
    t.until("the block on the client", Duration::from_secs(2), |t| t.c().block(near).is_some_and(|s| s != 0))
        .await?;
    t.look_at(near);
    t.frames(3).await?;
    t.op("attr_mod me core:mining_speed haste core:add 49 0")
        .await?;
    t.secs(0.2).await?;
    // Dirt takes 0.75 s by hand, and the server must not call this too soon.
    t.hold("core:attack", 0.3);
    t.until("mined at once", Duration::from_millis(280), |t| t.c().block(near).is_some_and(|s| s == 0))
        .await?;
    t.secs(0.2).await?;
    t.check_eq(t.block(near).await?, "core:air".to_string(), "not refused as too soon");
    Ok(())
}
