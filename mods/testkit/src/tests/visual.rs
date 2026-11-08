//! Scenarios that are mostly about the picture, checked against reference screenshots, plus
//! multiplayer through a second game process.

use std::time::Duration;

use weft_sdk::*;

use super::{add, probe};
use crate::runner::{R, Shot, T};

/// Share of the box around where `p` is drawn (half size `r`) for which `f` holds.
fn around(t: &T, shot: &Shot, p: [f64; 3], r: f32, f: impl FnMut([u8; 3]) -> bool) -> f32 {
    let Some((x, y)) = t.on_screen(p) else { return 0.0 };
    shot.share(x - r, y - r, x + r, y + r, f)
}

/// Average color of the box around where `p` is drawn.
fn avg_around(t: &T, shot: &Shot, p: [f64; 3], r: f32) -> [f32; 3] {
    let Some((x, y)) = t.on_screen(p) else { return [0.0; 3] };
    shot.avg(x - r, y - r, x + r, y + r)
}

/// Share of pixels that changed in a box between two grabs of the same size.
fn changed(a: &[u8], b: &[u8], w: u32, h: u32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let (ax, ay, bx, by) =
        ((x0 * w as f32) as u32, (y0 * h as f32) as u32, ((x1 * w as f32) as u32).min(w), ((y1 * h as f32) as u32).min(h));
    let (mut n, mut all) = (0.0f32, 0.0f32);
    for y in ay..by {
        for x in ax..bx {
            let i = ((y * w + x) * 4) as usize;
            n += (0..3).any(|k| (a[i + k] as i32 - b[i + k] as i32).abs() > 40) as u8 as f32;
            all += 1.0;
        }
    }
    n / all.max(1.0)
}

/// Hovering a slot shows a tooltip box next to the cursor.
pub async fn tooltips(t: T) -> R {
    t.flat().await?;
    t.op("put me 0 furnace 1").await?;
    t.op("put me 1 coal 5").await?;
    t.op("put me 2 diamond_sword 1").await?;
    t.op("put me 3 bread 1").await?;
    t.op("put me 4 diamond_chestplate 1").await?;
    t.press("core:inventory");
    t.screen("core:menu").await?;
    t.c().input_cursor(0.02, 0.02);
    t.frames(4).await?;
    let plain = t.grab().await?;
    let (w, h) = (t.state().window[0], t.state().window[1]);
    for (slot, name) in [(36, "furnace"), (37, "coal"), (38, "sword"), (39, "bread"), (40, "chestplate")] {
        let Some(Rect { x, y, w: sw, h: sh }) = t.c().ui_rect(&format!("slot:{slot}")) else {
            t.fail(format!("no slot {slot}"));
            continue;
        };
        let (cx, cy) = (x + sw / 2.0, y + sh / 2.0);
        t.c().input_cursor(cx, cy);
        t.frames(4).await?;
        t.shot(name).await?;
        let now = t.grab().await?;
        // The tooltip opens to the right of the cursor. Its dark box is close to the panel's
        // color, so only part of the pixels change.
        let v = changed(&plain, &now, w, h, cx + 0.02, cy - 0.02, cx + 0.15, cy + 0.05);
        t.probe(&format!("{name} tooltip drawn"), v, v > 0.05);
        // Lines by what an item is (`tagged.<component>`), numbers from the item itself.
        let line = |c: probe::C| match name {
            "sword" => c[1] > 140 && c[0] < 60 && c[2] < 60,
            "chestplate" => c[2] > 200 && c[0] < 130 && c[1] < 130,
            _ => c[0] > 150 && c[1] > 70 && c[1] < 150 && c[2] < 90,
        };
        if matches!(name, "sword" | "bread" | "chestplate") {
            let share = t
                .shot(&format!("{name}_lines"))
                .await?
                .share(cx + 0.015, cy - 0.06, cx + 0.3, cy + 0.12, line);
            t.probe(&format!("{name}'s line from its tag"), share, share > 0.0005);
        }
        // Another mod's line under coal's own (`item.base.coal.tooltip.testmod`), in red.
        if name == "coal" {
            let red = t
                .shot("coal_lines")
                .await?
                .share(cx + 0.02, cy - 0.05, cx + 0.3, cy + 0.12, probe::red);
            t.probe("the test mod's red line under coal's tooltip", red, red > 0.002);
            // `{block:..}` in a tooltip draws the block, not an item icon.
            let block = t
                .shot("coal_block_icon")
                .await?
                .share(cx + 0.02, cy - 0.05, cx + 0.3, cy + 0.12, probe::cyan);
            t.probe("diamond block icon in the tooltip", block, block > 0.0008);
        }
    }
    t.c().menu_close();
    t.no_screen().await
}

/// Block light in the dark: a closed stone room is black until a glowstone and a red lamp light
/// it, the floor by the lamp taking its color.
pub async fn night_light(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    t.fill(add(f, [-5, -1, -9]), add(f, [5, 4, 2]), "stone")
        .await?;
    t.fill(add(f, [-4, 0, -8]), add(f, [4, 3, 1]), "air")
        .await?;
    t.fill(add(f, [-4, -1, -8]), add(f, [4, -1, 1]), "birch_planks")
        .await?;
    t.c().set_look(0.0, -35.0);
    t.secs(0.5).await?;
    let dark = t.shot("dark").await?;
    let v = dark.share(0.1, 0.3, 0.9, 0.8, probe::dark);
    t.probe("the closed room is dark", v, v > 0.9);
    let (glow, lamp) = (add(f, [3, 0, -6]), add(f, [-3, 0, -6]));
    t.set(glow, "glowstone").await?;
    t.set(lamp, "red_lamp").await?;
    t.secs(0.5).await?;
    let lit = t.shot("night").await?;
    let floor = |b: [i32; 3]| [b[0] as f64 + 0.5, b[1] as f64, b[2] as f64 + 0.5];
    let by_glow = avg_around(&t, &lit, floor(add(glow, [-1, 0, 1])), 0.02);
    let by_lamp = avg_around(&t, &lit, floor(add(lamp, [1, 0, 1])), 0.02);
    t.probe("floor by the glowstone is lit", crate::runner::lum(by_glow), crate::runner::lum(by_glow) > 50.0);
    let red = by_lamp[0] - by_lamp[1];
    t.probe("floor by the red lamp is reddish (red minus green)", red, red > 15.0);
    Ok(())
}

/// What stands behind water is seen through it, tinted: a pig in a pool shows violet, not
/// pink; lava covers what is under it.
pub async fn liquid_layers(t: T) -> R {
    t.flat().await?;
    t.op("time 6000").await?;
    let f = t.feet();
    // A pool dug into the ground, half of it under ice, and a lava pit beside it.
    t.fill(add(f, [-3, -3, -7]), add(f, [1, -1, -3]), "water")
        .await?;
    t.fill(add(f, [-3, -1, -7]), add(f, [-2, -1, -3]), "ice")
        .await?;
    t.fill(add(f, [3, -2, -6]), add(f, [4, -1, -4]), "lava")
        .await?;
    t.op(&format!("spawn pig {} {} {}", f[0], f[1] - 3, f[2] - 5))
        .await?;
    t.c().set_look(0.0, -40.0);
    t.frames(30).await?;
    let pool = t.shot("pool").await?;
    let water = around(&t, &pool, [f[0] as f64, f[1] as f64 - 0.1, f[2] as f64 - 4.5], 0.03, probe::blue);
    t.probe("the pool is drawn blue", water, water > 0.5);
    let lava = around(&t, &pool, [f[0] as f64 + 4.0, f[1] as f64 - 0.1, f[2] as f64 - 5.0], 0.02, probe::orange);
    t.probe("the lava pit is drawn orange", lava, lava > 0.5);
    // Over the pool, looking down at the pig under the water.
    t.c().set_look(0.0, -70.0);
    t.cmd(&format!("/tp {} {} {}", f[0] as f64 - 0.5, f[1], f[2] as f64 - 4.0))
        .await?;
    t.frames(20).await?;
    let over = t.shot("outline_water").await?;
    let violet = over.share(0.2, 0.2, 0.8, 0.9, probe::under_water);
    let pink = over.share(0.2, 0.2, 0.8, 0.9, |c| c[0] > 200 && c[1] > 120 && c[2] > 120 && c[0] as i32 > c[2] as i32 + 30);
    t.probe("the pig under water is tinted violet", violet, violet > 0.005);
    t.probe("and not drawn pink", pink, pink < 0.002);
    t.cmd(&format!("/tp {} {} {}", f[0] as f64 + 2.5, f[1], f[2] as f64 - 4.5))
        .await?;
    t.c().set_look(270.0, -55.0);
    t.frames(20).await?;
    let pit = t.shot("outline_lava").await?;
    let v = pit.share(0.3, 0.3, 0.7, 0.7, probe::orange);
    t.probe("looking into the lava pit", v, v > 0.6);
    Ok(())
}

/// Cracks cover a block's model (a ladder's board, not its cube), and instant breaks show none:
/// in creative the next block under a held button stays clean while the break waits.
pub async fn break_overlay(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let wall = add(f, [0, 0, -2]);
    t.fill(wall, add(wall, [0, 1, 0]), "cobblestone").await?;
    let ladder = add(wall, [0, 0, 1]);
    t.set(ladder, "ladder[west=false,north=true]").await?;
    t.look_at(ladder);
    t.frames(3).await?;
    let (w, h) = (t.state().window[0], t.state().window[1]);
    let before = t.grab().await?;
    // Mining slowly, so a late picture still finds the button down and the block there.
    t.op("attr_mod me core:mining_speed slow 0 -0.75 0").await?;
    t.secs(0.2).await?;
    t.hold("core:attack", 2.0);
    t.secs(1.0).await?;
    let cracked = t.shot("ladder").await?.px;
    let face = t
        .block_on_screen(ladder, [0.0, 0.0, -0.45])
        .unwrap_or((0.5, 0.5));
    let v = changed(&before, &cracked, w, h, face.0 - 0.04, face.1 - 0.06, face.0 + 0.04, face.1 + 0.06);
    t.probe("cracks on the ladder", v, v > 0.05);
    let above = t
        .block_on_screen(add(wall, [0, 1, 0]), [0.0, 0.0, 0.5])
        .unwrap_or((0.5, 0.2));
    let v = changed(&before, &cracked, w, h, above.0 - 0.04, above.1 - 0.04, above.0 + 0.04, above.1 + 0.04);
    t.probe("none on the wall above it", v, v < 0.02);
    t.secs(1.2).await?;
    t.op("attr_rm me core:mining_speed slow").await?;
    t.op("mode creative").await?;
    t.set(ladder, "air").await?;
    let next = add(wall, [0, 0, -1]);
    t.fill(wall, add(next, [0, 1, 0]), "dirt").await?;
    t.look_at(wall);
    t.frames(3).await?;
    t.hold("core:attack", 0.5);
    t.until("the first block broken", Duration::from_secs(2), |t| t.c().block(wall) == Some(0))
        .await?;
    let held = t.shot("creative").await?.px;
    // The same with a click, no button held when it is seen: the next block as it looks clean.
    // Broken the same way, for the light on it to be just as late: a block goes at once here, the
    // light comes from the server after. Past the longest a broken block's pieces fly.
    t.secs(1.2).await?;
    t.fill(wall, add(next, [0, 1, 0]), "dirt").await?;
    t.secs(0.5).await?;
    t.press("core:attack");
    t.until("the first block broken again", Duration::from_secs(2), |t| t.c().block(wall) == Some(0))
        .await?;
    let clean = t.grab().await?;
    let p = t
        .block_on_screen(next, [0.0, 0.0, 0.5])
        .unwrap_or((0.5, 0.45));
    let v = changed(&held, &clean, w, h, p.0 - 0.05, p.1 - 0.05, p.0 + 0.05, p.1 + 0.05);
    t.probe("no cracks on the next block of an instant break", v, v < 0.03);
    Ok(())
}

/// Hosts the test world and lets a second game process join it over the network transport.
pub async fn multiplayer(t: T) -> R {
    t.world(13).await?;
    t.c().world_host();
    t.until("hosting", Duration::from_secs(60), |t| t.flag(client::client_flags::HOSTING))
        .await?;
    let run = t.c().run_dir();
    let dir = format!("{run}/test-mp");
    let _ = std::fs::create_dir_all(&dir);
    let ticket = format!("{dir}/ticket.txt");
    let script = format!("{dir}/join.txt");
    std::fs::write(&ticket, t.c().ticket()).map_err(|e| e.to_string())?;
    std::fs::write(
        &script,
        format!("join {ticket}\nloaded\nchat hello from the second player\nwait 3\nsay_config testmod:wisp_sparks\nwait 1\nquit\n"),
    )
    .map_err(|e| e.to_string())?;
    static SPARKS: weft_sdk::Config<i32> = weft_sdk::Config::named("testmod:wisp_sparks");
    let sparks = SPARKS.get();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = std::process::Command::new(exe)
        .args(["--run-dir", &dir, "--script", &script, "--size", "640x360"])
        .spawn()
        .map_err(|e| format!("cannot start the second client: {e}"))?;
    let heard = t
        .until("the second player's chat", Duration::from_secs(90), |t| {
            (0..5).any(|i| t.c().chat_line(i).contains("hello from the second player"))
        })
        .await;
    let seen = t
        .c()
        .entities()
        .iter()
        .filter_map(|&id| t.c().entity(id))
        .any(|e| name_of(abi::entry_kind::ENTITY, e.kind) == "core:player");
    // What they hold reaches the others, to draw it in their hand.
    static HELD: weft_sdk::Component<u32> = weft_sdk::Component::named("core:held");
    if let Some(other) = t.c().entities().iter().copied().find(|&id| {
        t.c()
            .entity(id)
            .is_some_and(|e| name_of(abi::entry_kind::ENTITY, e.kind) == "core:player")
    }) {
        t.op(&format!("put {other} 0 stone 1")).await?;
        let stone = Item::lookup("base:stone").unwrap_or(u32::MAX);
        t.until("what the other player holds", Duration::from_secs(5), |t| t.c().view().component_of(other, &HELD) == Some(stone))
            .await?;
    }
    // A synced setting the host changes while the other player is in reaches them.
    SPARKS.set(7);
    let synced = t
        .until("the second player's config", Duration::from_secs(30), |t| {
            (0..5).any(|i| {
                t.c()
                    .chat_line(i)
                    .contains("config testmod:wisp_sparks = 7")
            })
        })
        .await;
    SPARKS.set(sparks);
    if heard.is_err() || synced.is_err() {
        let _ = child.kill();
    }
    let mut status = None;
    let _ = t
        .until("the second client to quit", Duration::from_secs(30), |_| {
            status = child.try_wait().ok().flatten();
            status.is_some()
        })
        .await;
    if status.is_none() {
        let _ = child.kill();
    }
    heard?;
    synced?;
    t.check(seen, "the host does not see the second player");
    t.check(status.is_some_and(|s| s.success()), format!("second client ended with {status:?}"));
    Ok(())
}

/// A mod key binding is listed in Controls and reaches the mod; a frame hook draws in the world
/// and on the screen.
pub async fn client_hooks(t: T) -> R {
    t.flat().await?;
    let before = crate::runner::PROBES.with(|p| p.get());
    t.c().input_key("F9", 0.0);
    t.until("probe key seen", Duration::from_secs(3), |_| crate::runner::PROBES.with(|p| p.get()) > before)
        .await?;
    t.c().menu_open(&screens::CONTROLS);
    t.screen("core:controls").await?;
    for _ in 0..40 {
        if t.c().ui_rect("text:test probe").is_some() {
            break;
        }
        t.c().input_cursor(0.5, 0.5);
        t.c().input_scroll(-3.0);
        t.frames(2).await?;
    }
    t.check(t.c().ui_rect("text:test probe").is_some(), "Controls does not list the mod's key");
    t.c().menu_close();
    t.no_screen().await?;
    let f = t.feet();
    let b = [f[0] as f64 + 2.0, f[1] as f64, f[2] as f64 - 4.0];
    t.c().set_look(0.0, -10.0);
    static DRAWN: weft_sdk::GizmoGroup = weft_sdk::GizmoGroup::named("drawn");
    let green = |shot: &crate::runner::Shot| shot.share(0.0, 0.0, 1.0, 0.9, |c| c[1] > 200 && c[0] < 80 && c[2] < 80);
    for _ in 0..3 {
        t.c()
            .draw_fill(Some(&DRAWN), [b[0] - 3.0, b[1], b[2]], [b[0] - 2.0, b[1] + 1.0, b[2] + 1.0], 0x20FF20FF);
        t.c()
            .draw_box(None, b, [b[0] + 1.0, b[1] + 2.0, b[2] + 1.0], 0xFF4040FF);
        t.c()
            .draw_text(None, [b[0] + 0.5, b[1] + 2.5, b[2] + 0.5], "testkit.drawn", &[], 0xFFFF80FF, 2.0);
        t.c()
            .draw_screen_text(0.5, 0.3, "testkit.screen_text", &[], 0x80FFFFFF, 1.0);
        t.frames(1).await?;
    }
    t.c()
        .draw_fill(Some(&DRAWN), [b[0] - 3.0, b[1], b[2]], [b[0] - 2.0, b[1] + 1.0, b[2] + 1.0], 0x20FF20FF);
    t.c()
        .draw_box(None, b, [b[0] + 1.0, b[1] + 2.0, b[2] + 1.0], 0xFF4040FF);
    t.c()
        .draw_text(None, [b[0] + 0.5, b[1] + 2.5, b[2] + 0.5], "testkit.drawn", &[], 0xFFFF80FF, 2.0);
    t.c()
        .draw_screen_text(0.5, 0.3, "testkit.screen_text", &[], 0x80FFFFFF, 1.0);
    let shot = t.shot("drawn").await?;
    let hidden = green(&shot);
    t.probe("a shape of a group that is off is not drawn", hidden, hidden < 0.0005);
    let red = shot.share(0.0, 0.0, 1.0, 0.9, probe::red);
    t.probe("the box drawn in the world", red, red > 0.001);
    let cyan = shot.share(0.25, 0.25, 0.75, 0.4, |c| c[2] > 200 && c[1] > 200 && c[0] < 190);
    t.probe("the screen text", cyan, cyan > 0.003);
    let yellow = shot.share(0.0, 0.0, 1.0, 0.5, |c| c[0] > 200 && c[1] > 200 && c[2] < 170);
    t.probe("the text drawn in the world", yellow, yellow > 0.001);
    t.cmd("/gizmo testkit:drawn").await?;
    for _ in 0..3 {
        t.c()
            .draw_fill(Some(&DRAWN), [b[0] - 3.0, b[1], b[2]], [b[0] - 2.0, b[1] + 1.0, b[2] + 1.0], 0x20FF20FF);
        t.frames(1).await?;
    }
    t.c()
        .draw_fill(Some(&DRAWN), [b[0] - 3.0, b[1], b[2]], [b[0] - 2.0, b[1] + 1.0, b[2] + 1.0], 0x20FF20FF);
    let shot = t.shot("drawn_group").await?;
    t.cmd("/gizmo testkit:drawn").await?;
    let shown = green(&shot);
    t.probe("the same shape once its group is on", shown, shown > 0.001);
    // Screen text is text like any other: icons of blocks and items show in it.
    for _ in 0..3 {
        t.c()
            .draw_screen_text(0.5, 0.7, "testkit.rich_label", &[], 0xFFFFFFFF, 2.0);
        t.frames(1).await?;
    }
    t.c()
        .draw_screen_text(0.5, 0.7, "testkit.rich_label", &[], 0xFFFFFFFF, 2.0);
    let rich = t.shot("rich_label").await?;
    let diamond = rich.share(0.35, 0.65, 0.65, 0.8, probe::cyan);
    t.probe("a block icon in screen text", diamond, diamond > 0.002);
    Ok(())
}
