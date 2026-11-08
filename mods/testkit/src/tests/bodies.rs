//! Stage 8: moving collisions with the test mod's lift, skiff and crate.

use std::time::Duration;

use weft_sdk::*;

use super::add;
use crate::runner::{R, T};

async fn spawn(t: &T, kind: &str, p: [i32; 3]) -> Result<u64, String> {
    let id = t
        .op(&format!("spawn {kind} {} {} {}", p[0], p[1], p[2]))
        .await?;
    id.trim().parse().map_err(|_| format!("spawn {kind}: {id}"))
}

/// Server position of an entity (`me`: the player as the server last heard).
async fn pos_of(t: &T, id: &str) -> Result<[f64; 3], String> {
    let line = t.op(&format!("entity {id}")).await?;
    let v: Vec<f64> = line
        .split_whitespace()
        .skip(1)
        .take(3)
        .filter_map(|s| s.parse().ok())
        .collect();
    v.try_into().map_err(|_| format!("entity #{id}: {line}"))
}

/// A chest on a flying structure is a chest: it keeps what is in it, opens, goes with the structure,
/// is saved with it, and spills where it is when it breaks.
pub async fn body_chest(t: T) -> R {
    t.world(1).await?;
    t.op("mode creative").await?;
    let f = t.feet();
    let id = spawn(&t, "testmod:skiff", add(f, [0, 12, 0])).await?;
    t.op(&format!("structureset {id} 1 1 0 base:chest")).await?;
    t.frames(5).await?;
    let near = |t: T, id: u64| async move {
        let at = pos_of(&t, &id.to_string()).await?;
        t.op(&format!("near {} {} {} 8", at[0].floor(), at[1].floor(), at[2].floor()))
            .await
    };
    let list = near(t.clone(), id).await?;
    let chest = list
        .split_whitespace()
        .find(|e| e.contains("chest"))
        .and_then(|e| e.split(':').next())
        .ok_or(format!("no chest entity among {list}"))?
        .to_string();
    t.op(&format!("put {chest} 0 base:cobblestone 7")).await?;
    // It is where its block is, however the structure goes.
    let (a, b) = (pos_of(&t, &id.to_string()).await?, pos_of(&t, &chest).await?);
    // Asked a tick apart, the structure having gone on by 0.3 of a block.
    t.check(
        (b[0] - a[0] - 1.0).abs() < 0.7 && (b[1] - a[1] - 1.5).abs() < 0.1,
        format!("the chest is at its block: structure {a:?}, chest {b:?}"),
    );
    t.secs(1.0).await?;
    let (a, b2) = (pos_of(&t, &id.to_string()).await?, pos_of(&t, &chest).await?);
    t.check((b2[0] - b[0]).abs() > 2.0, "it goes with the structure");
    t.check((b2[0] - a[0] - 1.0).abs() < 0.7, format!("and stays at its block: structure {a:?}, chest {b2:?}"));

    // Open from on board.
    t.op(&format!("onto {id} 1.0")).await?;
    t.secs(0.4).await?;
    t.c().set_look(-90.0, -40.0);
    t.secs(0.2).await?;
    t.check_eq(t.state().target_body, id, "the chest on the structure is looked at");
    t.press("core:use");
    t.secs(0.4).await?;
    let open = t.c().screen();
    t.check(!open.is_empty(), "it opens from on board");
    // Still open after it flew on.
    t.secs(0.4).await?;
    t.check_eq(t.c().screen(), open, "and stays open as it flies");
    t.c().menu_close();
    t.no_screen().await?;

    // Saved with it.
    let name = format!("test-{}", t.name());
    t.c().world_leave();
    t.screen("core:title").await?;
    t.c().world_open(&name, 1);
    t.until("joined again", Duration::from_secs(60), |t| t.flag(client::client_flags::JOINED))
        .await?;
    t.loaded().await?;
    let list = t
        .op(&format!("near {} {} {} 96", f[0], f[1] + 12, f[2]))
        .await?;
    let id: u64 = list
        .split_whitespace()
        .find(|e| e.ends_with("testmod:skiff"))
        .and_then(|e| e.split(':').next())
        .and_then(|e| e.parse().ok())
        .ok_or(format!("no skiff among {list}"))?;
    let list = near(t.clone(), id).await?;
    let chest = list
        .split_whitespace()
        .find(|e| e.contains("chest"))
        .and_then(|e| e.split(':').next())
        .ok_or(format!("no chest after loading among {list}"))?
        .to_string();
    let (name, count, _) = t.slot(&chest, 0).await?;
    t.check(name.contains("cobblestone") && count == 7, format!("what it held came back: {name} {count}"));

    // Spills in the world, at the chest.
    let at = pos_of(&t, &chest).await?;
    t.op(&format!("structureset {id} 1 1 0 air")).await?;
    t.frames(5).await?;
    let list = t
        .op(&format!("near {} {} {} 6", at[0].floor(), at[1].floor(), at[2].floor()))
        .await?;
    t.check(list.contains("core:item"), format!("what it held fell out right there: {list}"));
    t.check(t.op(&format!("entity {chest}")).await.is_err(), "the chest is gone");
    Ok(())
}

/// Sneaking, nobody steps off the edge of what they stand on, whichever way that is going.
/// Each walk has to get to the edge, or it says nothing.
pub async fn body_sneak_edge(t: T) -> R {
    t.flat().await?;
    t.cmd("/rule testkit:no_hurt 1").await?;
    let f = t.feet();
    let y = 8;
    let skiff = t
        .op(&format!("spawn testmod:skiff {} {} {}", f[0], f[1] + y, f[2] + 9))
        .await?;
    // With more on it, lower, the way it goes: past the deck's edge isn't past the box around it.
    let more = t
        .op(&format!("spawn testmod:skiff {} {} {}", f[0], f[1] + y + 8, f[2] + 9))
        .await?;
    for x in [-2, 2] {
        t.op(&format!("structureset {more} {x} -1 0 base:oak_planks"))
            .await?;
    }
    let still_at = add(f, [-8, y, 0]);
    t.fill(add(still_at, [-1, 0, -1]), add(still_at, [1, 0, 1]), "base:stone")
        .await?;
    let still = t
        .op(&format!("lift {} {} {} 1", still_at[0], still_at[1], still_at[2]))
        .await?;
    let cross_at = add(f, [8, y, 0]);
    for p in [[0, 0], [1, 0], [-1, 0], [0, 1], [0, -1]] {
        t.set(add(cross_at, [p[0], 0, p[1]]), "base:stone").await?;
    }
    let cross = t
        .op(&format!("lift {} {} {} 1", cross_at[0], cross_at[1], cross_at[2]))
        .await?;
    t.frames(5).await?;
    let along = [("core:left", 0.0f32), ("core:right", 0.0)];
    let corners = [("core:forward", 45.0f32), ("core:forward", 225.0)];
    for (name, id, walks) in [
        ("a structure going sideways", &skiff, &along),
        ("one with more on it", &more, &along),
        ("one standing still", &still, &along),
        ("the corners of a cross", &cross, &corners),
    ] {
        let Ok(body) = id.parse::<u64>() else { return Err(format!("no {name}")) };
        for &(key, yaw) in walks {
            // Off what it was on first.
            t.cmd(&format!("/tp {} {} {}", f[0], f[1] + 20, f[2]))
                .await?;
            t.secs(0.2).await?;
            t.op(&format!("onto {id} 1.0")).await?;
            t.c().set_look(yaw, 0.0);
            t.secs(0.5).await?;
            t.hold("core:sneak", 2.4);
            t.frames(3).await?;
            t.hold(key, 2.2);
            let began = std::time::Instant::now();
            let (mut far, mut fell, mut log) = (0.0f64, false, Vec::new());
            while began.elapsed() < Duration::from_secs_f32(2.2) {
                t.frames(1).await?;
                let st = t.state();
                let Some(on) = t.c().entity(body).map(|e| e.pos) else { continue };
                let rel = [st.pos.x - on.x, st.pos.y - on.y, st.pos.z - on.z];
                far = far.max(rel[0].abs()).max(rel[2].abs());
                log.push(format!("{:.3} {:.3} {:.3} ground {}", rel[0], rel[1], rel[2], t.flag(client::client_flags::ON_GROUND)));
                if rel[1] < 0.7 {
                    fell = true;
                    break;
                }
            }
            let last = log[log.len().saturating_sub(6)..].join(" | ");
            t.check(!fell, format!("not off {name}, {key} at {yaw}: {last}"));
            t.check(fell || far > 0.7, format!("walking got to the edge of {name}, {key} at {yaw}: only {far:.2}"));
            t.secs(2.5 - began.elapsed().as_secs_f32().min(2.5)).await?;
        }
    }
    Ok(())
}

/// Blocks go on a structure wherever it has none yet, not only in the parts of it that happen to
/// be stored already.
pub async fn body_place(t: T) -> R {
    t.flat().await?;
    t.op("mode creative").await?;
    t.op("put me 0 oak_planks 64").await?;
    t.c().select_slot(0);
    let f = t.feet();
    // A plate lifted around a spot over it: all it has is a layer under its middle.
    let center = add(f, [0, 4, -5]);
    t.fill(add(center, [-1, -1, -1]), add(center, [1, -1, 1]), "base:slime_block")
        .await?;
    let id = t
        .op(&format!("lift {} {} {} 1", center[0], center[1], center[2]))
        .await?;
    t.cmd(&format!("/tp {} {} {}", center[0] as f64 + 0.5, center[1] + 2, center[2] as f64 + 3.5))
        .await?;
    t.c().set_flying(true);
    t.secs(0.5).await?;
    t.look_at(add(center, [0, -1, 0]));
    t.secs(0.3).await?;
    t.check(t.state().target_body.to_string() == id, "looking at the structure");
    t.press("core:use");
    t.secs(0.5).await?;
    t.check_eq(t.op(&format!("sblock {id} 0 0 0")).await?, "base:oak_planks".to_string(), "a block goes on top of the plate");
    // Beyond anything it has: past its edge, from the side.
    t.look_at(add(center, [1, -1, 1]));
    t.secs(0.3).await?;
    t.press("core:use");
    t.secs(0.5).await?;
    t.check_eq(t.op(&format!("sblock {id} 1 0 1")).await?, "base:oak_planks".to_string(), "and another");
    Ok(())
}

/// Standing on a lift carries the player up and back down with it, without fall damage.
pub async fn lift(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let id = spawn(&t, "testmod:lift", add(f, [3, 0, 0])).await?;
    // Its box comes from its blocks (2 x 2 planks from block (0, 0, 0) on), no size of its own.
    t.check_eq(t.op(&format!("size {id}")).await?, "3.000 1.000 0.850".to_string(), "the lift's box from its blocks");
    let p = pos_of(&t, &format!("{id}")).await?;
    t.op(&format!("onto {id} 1.0")).await?;
    t.secs(2.0).await?;
    let (me, lift) = (t.state().pos, pos_of(&t, &format!("{id}")).await?);
    t.check(lift[1] > p[1] + 1.2, "the lift went up");
    t.check((me.y - (lift[1] + 1.0)).abs() < 0.15, format!("the player rides on top (feet {:.2}, deck {:.2})", me.y, lift[1] + 1.0));
    // Up to the top, then all the way down.
    t.secs(5.0).await?;
    let (me, lift) = (t.state().pos, pos_of(&t, &format!("{id}")).await?);
    t.check(lift[1] < p[1] + 1.5, "the lift came down");
    t.check((me.y - (lift[1] + 1.0)).abs() < 0.15, format!("the player rode it down (feet {:.2}, deck {:.2})", me.y, lift[1] + 1.0));
    t.check_eq(t.op("health").await?, "20".to_string(), "health after the ride");
    Ok(())
}

/// Motion in the frame of what one stands on: the camera glides with a lift (no 0.05 block
/// jump a tick), and an idol riding the skiff stands still on its deck, not walking in place.
pub async fn ride_frames(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let id = spawn(&t, "testmod:lift", add(f, [3, 0, 0])).await?;
    t.op(&format!("onto {id} 1.0")).await?;
    t.secs(0.5).await?;
    let mut steps = Vec::new();
    let mut last = t.state().shown.y;
    for _ in 0..60 {
        t.frames(1).await?;
        let y = t.state().shown.y;
        steps.push(y - last);
        last = y;
    }
    let still = steps.iter().filter(|d| d.abs() < 1e-5).count();
    let big = steps.iter().fold(0.0f64, |a, d| a.max(d.abs()));
    t.note(format!("{still} still frames of 60 while climbing, largest step {big:.4}"));
    t.check(still < 6, "the camera glides with the lift");
    t.check(big < 0.04, "no tick-sized jumps");

    t.cmd(&format!("/tp {} {} {}", f[0], f[1], f[2] + 10))
        .await?;
    let skiff = spawn(&t, "testmod:skiff", add(f, [0, 3, -6])).await?;
    let idol = spawn(&t, "testmod:idol", add(f, [0, 0, -6])).await?;
    t.op(&format!("onto {skiff} 1.0 {idol}")).await?;
    t.secs(1.0).await?;
    let (s, i) = (pos_of(&t, &format!("{skiff}")).await?, pos_of(&t, &format!("{idol}")).await?);
    t.check((i[0] - s[0]).abs() < 1.6 && (i[1] - (s[1] + 1.0)).abs() < 0.1, format!("the idol rides the skiff (idol {i:?}, skiff {s:?})"));
    let swing = t.c().entity(idol).map_or(1.0, |e| e.moved);
    t.check(swing < 0.15, format!("the riding idol does not walk ({swing:.2})"));
    Ok(())
}

/// A skiff and its rider glide on the client: drawn every frame at the speed the server moves
/// them (6 blocks a second), with no frames standing still and none jumping a tick ahead.
pub async fn skiff_smooth(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let skiff = spawn(&t, "testmod:skiff", add(f, [-8, 3, -6])).await?;
    let idol = spawn(&t, "testmod:idol", add(f, [-8, 0, -6])).await?;
    t.op(&format!("onto {skiff} 1.0 {idol}")).await?;
    // Into a leg, away from its turn (it turns every 3 seconds of its age).
    t.secs(0.5).await?;
    let x = |t: &T, id| t.c().entity(id).map_or(0.0, |e| e.pos.x);
    // For 3.5 seconds (a turn is in it): it reverses once (overshooting and coming back would
    // reverse three times, jitter many times), never jumps, and its rider stays on its spot.
    // A jump is what a step has beyond its 6 blocks a second: a long frame of the machine's makes
    // a long step of a steady glide too, seen a frame later.
    let (mut turns, mut dir, mut jump, mut slip, mut longest, mut before) = (0, 0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut last, mut rel) = (x(&t, skiff), x(&t, idol) - x(&t, skiff));
    let start = std::time::Instant::now();
    let mut frame = start;
    while start.elapsed().as_secs_f64() < 3.5 {
        t.frames(1).await?;
        let dt = frame.elapsed().as_secs_f64();
        frame = std::time::Instant::now();
        longest = longest.max(dt);
        let (now, now_rel) = (x(&t, skiff), x(&t, idol) - x(&t, skiff));
        let d = now - last;
        if d.abs() > 1e-6 {
            if dir != 0.0 && d.signum() != dir {
                turns += 1;
            }
            dir = d.signum();
        }
        (jump, slip) = (jump.max(d.abs() - 6.0 * (dt + before)), slip.max((now_rel - rel).abs()));
        (last, rel, before) = (now, now_rel, dt);
    }
    t.note(format!(
        "{turns} reversals in 3.5 s, jumps up to {jump:.3}, longest frame {:.0} ms, rider moves on the deck up to {slip:.4} a frame",
        longest * 1000.0
    ));
    t.check_eq(turns, 1, "the skiff turns once, without overshooting or jitter");
    t.check(jump < 0.15, "the skiff glides (no jumps)");
    t.check(slip < 0.02, "its rider stays on its spot of the deck");
    // Riding it too: the deck keeps its place in front of the camera.
    t.op(&format!("onto {skiff} 1.0")).await?;
    t.secs(0.5).await?;
    // For 3.5 seconds, so a turn is in it.
    let mut steps = Vec::new();
    let mut last = x(&t, skiff) - t.state().shown.x;
    let start = std::time::Instant::now();
    while start.elapsed().as_secs_f64() < 3.5 {
        t.frames(1).await?;
        let now = x(&t, skiff) - t.state().shown.x;
        steps.push((now - last).abs());
        last = now;
    }
    steps.sort_by(|a, b| a.total_cmp(b));
    let big = steps.last().copied().unwrap_or(0.0);
    t.note(format!("skiff seen from its rider: steps up to {big:.4}"));
    t.check(big < 0.01, "the skiff stays put in front of a rider, turns included");
    Ok(())
}

/// A neutral mob: a spider by day minds its own business until hit, then fights back for a while;
/// in the dark it comes by itself. And a condition by distance, like a warden's.
pub async fn neutral_mobs(t: T) -> R {
    t.flat().await?;
    t.op("time 6000").await?;
    t.cmd("/weather clear").await?;
    let f = t.feet();
    let health = |t: T| async move { Ok::<f32, String>(t.op("health").await?.parse().unwrap_or(0.0)) };
    let spider = spawn(&t, "spider", add(f, [3, 0, 0])).await?;
    t.secs(3.0).await?;
    t.check_eq(health(t.clone()).await?, 20.0, "by day a spider leaves you alone");
    t.op(&format!("hurt {spider} 1")).await?;
    t.until("the spider fights back", std::time::Duration::from_secs(6), |t| t.c().entity(0).is_some_and(|e| e.health < 20.0))
        .await?;
    // Away for longer than it holds a grudge, and back.
    t.cmd(&format!("/tp {} {} {}", f[0], f[1], f[2] + 60))
        .await?;
    t.cmd("/tick sprint 450").await?;
    t.secs(1.0).await?;
    let at = pos_of(&t, &format!("{spider}")).await?;
    t.cmd(&format!("/tp {} {} {}", at[0] + 2.0, at[1], at[2]))
        .await?;
    t.op("heal").await?;
    t.secs(3.0).await?;
    t.check_eq(health(t.clone()).await?, 20.0, "it calms down");
    t.op(&format!("remove {spider}")).await?;

    // In the dark it needs no reason (this dimension's time stands still, so under a roof).
    t.fill(add(f, [-16, 3, -16]), add(f, [16, 3, 16]), "stone")
        .await?;
    let dark = spawn(&t, "spider", add(f, [3, 0, 0])).await?;
    t.cmd(&format!("/tp {} {} {}", f[0], f[1], f[2])).await?;
    t.until("a spider in the dark comes by itself", std::time::Duration::from_secs(8), |t| {
        t.c().entity(0).is_some_and(|e| e.health < 20.0)
    })
    .await?;
    t.op(&format!("remove {dark}")).await?;
    t.op("heal").await?;

    let sentry = spawn(&t, "testkit:sentry", add(f, [10, 0, 0])).await?;
    t.secs(2.5).await?;
    t.check_eq(health(t.clone()).await?, 20.0, "out of its distance it does nothing");
    let at = pos_of(&t, &format!("{sentry}")).await?;
    t.check((at[0] - (f[0] as f64 + 10.5)).abs() < 1.0, format!("nor does it come: {at:?}"));
    t.cmd(&format!("/tp {} {} {}", at[0] + 3.0, at[1], at[2]))
        .await?;
    t.until("within it, it attacks", std::time::Duration::from_secs(5), |t| t.c().entity(0).is_some_and(|e| e.health < 20.0))
        .await?;
    Ok(())
}

/// A zombie's goals: it finds its way around a wall to the player and hits it.
pub async fn zombie_chases(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    t.fill(add(f, [4, 0, -6]), add(f, [4, 2, 6]), "cobblestone")
        .await?;
    t.fill(add(f, [4, 0, 5]), add(f, [4, 1, 5]), "air").await?;
    let id = spawn(&t, "zombie", add(f, [8, 0, 0])).await?;
    t.secs(10.0).await?;
    let z = pos_of(&t, &format!("{id}")).await?;
    let health: f32 = t.op("health").await?.parse().unwrap_or(20.0);
    t.check(z[0] < f[0] as f64 + 4.0, format!("the zombie came round the wall (x {:.1})", z[0]));
    t.check(health < 20.0, format!("and hit the player (health {health})"));
    // Mobs hit through the same question as players: a mod that forbids attacks stops them.
    t.op("heal").await?;
    t.cmd("/rule testkit:peace 1").await?;
    t.secs(2.5).await?;
    let calm: f32 = t.op("health").await?.parse().unwrap_or(0.0);
    t.cmd("/rule testkit:peace 0").await?;
    t.check(calm >= 20.0, format!("a zombie hit while attacks were forbidden (health {calm})"));
    Ok(())
}

/// The skiff is a structure of blocks: a player riding it places a block on its deck and
/// breaks it again while it flies; the block flies with it, collides, and mods read and write
/// its blocks.
pub async fn skiff_blocks(t: T) -> R {
    t.flat().await?;
    t.op("mode creative").await?;
    t.op("put me 0 stone 64").await?;
    t.c().select_slot(0);
    let f = t.feet();
    let id = spawn(&t, "testmod:skiff", add(f, [0, 3, 0])).await?;
    let sb = |p: [i32; 3]| format!("sblock {id} {} {} {}", p[0], p[1], p[2]);
    t.check_eq(t.op(&sb([1, 0, 1])).await?, "base:oak_planks".to_string(), "it is built of planks");
    t.op(&format!("onto {id} 1.0")).await?;
    t.secs(0.4).await?;
    // Down at the deck ahead: the block goes on top of it.
    t.c().set_look(0.0, -60.0);
    t.secs(0.2).await?;
    t.press("core:use");
    t.secs(0.5).await?;
    t.check_eq(t.op(&sb([0, 1, -1])).await?, "base:stone".to_string(), "a stone placed on the flying deck");
    // The server's pick (what mods' `look_at` gets) sees the structure's blocks too.
    let look = t.op("look").await?;
    t.check(look.starts_with(&format!("{id} ")), format!("look_at hits the flying structure: {look}"));
    // Looking at a spot of the deck 0.02 from the edge between two planks (along the way it
    // flies) while riding, the same block stays targeted.
    let st = t.state();
    let o = t.c().entity(id).map_or([0.0; 3], |e| e.pos.to_array());
    let spot = [o[0] - 0.5 + 0.98, o[1] + 1.0, o[2] - 0.5 + 0.5 - 1.0];
    let eye = [st.shown.x, st.shown.y + t.c().entity(0).map_or(1.62, |e| e.size.eye as f64), st.shown.z];
    let (dx, dy, dz) = (spot[0] - eye[0], spot[1] - eye[1], spot[2] - eye[2]);
    t.c()
        .set_look((-dx).atan2(-dz).to_degrees() as f32, dy.atan2((dx * dx + dz * dz).sqrt()).to_degrees() as f32);
    t.frames(2).await?;
    let mut targets = std::collections::BTreeSet::new();
    for _ in 0..60 {
        t.frames(1).await?;
        let s = t.state();
        targets.insert((s.target, s.target_body));
    }
    t.check(targets.len() == 1, format!("one block looked at while riding ({targets:?})"));
    let (a, sa) = (pos_of(&t, &format!("{id}")).await?, t.op(&sb([0, 1, -1])).await?);
    t.secs(1.0).await?;
    let b = pos_of(&t, &format!("{id}")).await?;
    t.check((b[0] - a[0]).abs() > 2.0 && sa == t.op(&sb([0, 1, -1])).await?, "it flies on with the stone on it");
    // A mod's write, then the player breaks the stone.
    t.check_eq(t.op(&format!("{} base:cobblestone", sb([1, 1, 1]))).await?, "1".to_string(), "a mod sets a block of it");
    t.check_eq(t.op(&sb([1, 1, 1])).await?, "base:cobblestone".to_string(), "and reads it back");
    t.c().set_look(0.0, -60.0);
    t.secs(0.2).await?;
    t.hold("core:attack", 0.1);
    t.secs(0.5).await?;
    t.check_eq(t.op(&sb([0, 1, -1])).await?, "core:air".to_string(), "the stone broken off the flying deck");
    Ok(())
}
