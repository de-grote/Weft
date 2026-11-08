use std::time::Duration;

use weft_sdk::abi::entry_kind as ek;
use weft_sdk::terrain::Column;
use weft_sdk::*;

use super::probe;
use super::{add, at};
use crate::runner::{R, T};

/// A player saved in a dimension that is gone (its mod removed) keeps everything, storages too,
/// and starts at the spawn; the world remembers the mods it was played with.
pub async fn missing_dimension(t: T) -> R {
    t.world(7).await?;
    t.op("put me 3 diamond 5").await?;
    t.op("stash base:chest stick 7").await?;
    t.screen("core:menu").await?;
    t.c().menu_close();
    t.no_screen().await?;
    let flat = Dimension::lookup("base:flatlands").ok_or("no flatlands")?;
    t.cmd("/dim base:flatlands").await?;
    t.until("in the flatlands", Duration::from_secs(20), |t| t.state().dim == flat)
        .await?;
    let name = format!("test-{}", t.name());
    t.c().world_leave();
    t.screen("core:title").await?;
    let dir = format!("{}/saves/{name}", t.c().run_dir());
    let world = std::fs::read_to_string(format!("{dir}/world.txt")).map_err(|e| e.to_string())?;
    t.check(world.contains("mod.base = ") && world.contains("mod.testkit = "), "world.txt lists the mods");
    std::fs::write(format!("{dir}/world.txt"), world + "mod.ghost = 1.0\n").map_err(|e| e.to_string())?;
    for f in std::fs::read_dir(format!("{dir}/players"))
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let src = std::fs::read_to_string(f.path()).map_err(|e| e.to_string())?;
        std::fs::write(f.path(), src.replace("dim = base:flatlands", "dim = ghost:gone")).map_err(|e| e.to_string())?;
    }
    // Opened from the list like a player would: the missing mod is named first.
    t.c().menu_open(&screens::WORLDS);
    t.screen("core:worlds").await?;
    t.frames(2).await?;
    // A click lasts 50 ms; buttons act on release.
    t.click_ui(&format!("text:{name} |"), 0);
    t.secs(0.2).await?;
    t.click_ui("id:play", 0);
    t.screen("core:missing_mods").await?;
    t.frames(2).await?;
    t.shot("missing_mods").await?;
    t.click_ui("id:continue", 0);
    t.until("joined again", Duration::from_secs(60), |t| t.flag(client::client_flags::JOINED))
        .await?;
    t.loaded().await?;
    t.check_eq(t.state().dim, Dimension::lookup("base:overworld").unwrap_or(u32::MAX), "the player is in the spawn dimension");
    t.check_eq(t.slot("me", 3).await?, ("base:diamond".into(), 5, 0), "and kept the inventory");
    t.check_eq(t.op("stashed 0").await?, "base:stick 7 0".to_string(), "and the stash");
    Ok(())
}

/// Riding: chains of mounts without rings, a mob jockey steering by its AI, the player steering
/// from its client with the kind's keys, getting off where the seat says.
pub async fn riding(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let spawn = async |t: &T, kind: &str, dz: i32| {
        t.op(&format!("spawn {kind} {} {} {}", f[0], f[1], f[2] + dz))
            .await
    };
    let (a, b) = (spawn(&t, "testmod:steed", -4).await?, spawn(&t, "testmod:steed", -7).await?);
    let y = |s: String| {
        s.split_whitespace()
            .nth(2)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::NAN)
    };
    t.check_eq(t.op(&format!("mount {a} {b}")).await?, "1".to_string(), "a steed on a steed");
    t.check_eq(t.op(&format!("mount {b} {a}")).await?, "0".to_string(), "no ring of riders");
    t.secs(0.3).await?;
    let (ya, yb) = (y(t.op(&format!("entity {a}")).await?), y(t.op(&format!("entity {b}")).await?));
    t.check((yb - ya - 1.0).abs() < 0.05, format!("the one on top sits in its seat: {yb:.2} over {ya:.2}"));

    // A zombie in the saddle walks the steed with its AI, towards the player.
    let z = spawn(&t, "base:zombie", -12).await?;
    let c = spawn(&t, "testmod:steed", -12).await?;
    t.op("time 18000").await?;
    t.check_eq(t.op(&format!("mount {c} {z}")).await?, "1".to_string(), "a zombie jockey");
    let at = |s: String| {
        let v: Vec<f64> = s
            .split_whitespace()
            .skip(1)
            .take(3)
            .filter_map(|v| v.parse().ok())
            .collect();
        (v.first().copied().unwrap_or(f64::NAN), v.get(2).copied().unwrap_or(f64::NAN))
    };
    let before = at(t.op(&format!("entity {c}")).await?);
    t.secs(2.0).await?;
    let after = at(t.op(&format!("entity {c}")).await?);
    let walked = (after.0 - before.0).hypot(after.1 - before.1);
    t.probe("blocks the zombie walked its steed", walked as f32, walked > 1.0);

    // The player in the saddle of the free one: its keys walk it where it looks.
    t.op(&format!("remove {z}")).await?;
    t.op(&format!("remove {c}")).await?;
    let d = spawn(&t, "testmod:steed", 3).await?;
    t.check_eq(t.op(&format!("mount {d}")).await?, "1".to_string(), "the player gets on");
    t.c().set_look(0.0, 0.0);
    t.secs(0.5).await?;
    let before = at(t.op(&format!("entity {d}")).await?);
    t.hold("core:forward", 1.0);
    t.secs(1.3).await?;
    let after = at(t.op(&format!("entity {d}")).await?);
    let rode = (after.0 - before.0).hypot(after.1 - before.1);
    t.probe("blocks the player rode it in a second", rode as f32, rode > 2.0);
    let seat = t.op("vehicle").await?;
    t.check(seat.starts_with(&d), format!("still on it: {seat}"));
    // From behind: sitting, legs forward.
    t.press("core:perspective");
    t.c().set_look(180.0, -35.0);
    t.secs(0.6).await?;
    t.shot("seated").await?;
    t.press("core:perspective");
    t.press("core:perspective");

    // Sent away, the steed takes its rider: in this dimension, then to another.
    t.op(&format!("send {d} here {} {} {}", f[0] + 40, f[1] + 3, f[2]))
        .await?;
    t.until("the rider came along", Duration::from_secs(5), |t| (t.state().pos.x - (f[0] + 40) as f64).abs() < 3.0)
        .await?;
    t.check(t.op("vehicle").await?.starts_with(&d), "still in the saddle after a teleport");
    let over = Dimension::lookup("base:overworld").ok_or("no overworld")?;
    t.op(&format!("send {d} base:overworld 0 200 0")).await?;
    t.until("the rider came to the other dimension", Duration::from_secs(10), |t| t.state().dim == over)
        .await?;
    t.secs(0.5).await?;
    t.check(t.op("vehicle").await?.starts_with(&d), "still in the saddle in the other dimension");
    t.press("core:dismount");
    t.secs(0.5).await?;
    t.check_eq(t.op("vehicle").await?, "0 0".to_string(), "the dismount key gets off");
    Ok(())
}

/// Camera modes: F5 through the three, third person kept out of a wall and sliding along it
/// smoothly, the free camera only in a game mode that flies and moving while the player stands,
/// a mod's own on its key, and a mode the server holds.
pub async fn camera(t: T) -> R {
    t.flat().await?;
    t.op("mode survival").await?;
    let f = t.feet();
    // From the eyes to the camera.
    let back = |t: &T| {
        let s = t.state();
        let (c, p) = (s.camera_pos, s.shown);
        ((c.x - p.x).powi(2) + (c.y - p.y - 1.62).powi(2) + (c.z - p.z).powi(2)).sqrt() as f32
    };
    t.c().set_look(0.0, 0.0);
    t.secs(0.3).await?;
    t.check_eq(t.state().camera, 0, "first person to begin with");
    t.press("core:perspective");
    t.secs(0.3).await?;
    t.check_eq(t.state().camera, 1, "F5: from behind");
    let d = back(&t);
    t.probe("blocks from the eyes behind", d, (d - 4.0).abs() < 0.2);

    // A wall 1.5 blocks behind the eyes (looking north, behind is south).
    t.op(&format!("fill {} {} {} {} {} {} base:stone", f[0] - 6, f[1], f[2] + 2, f[0] + 6, f[1] + 5, f[2] + 2))
        .await?;
    t.secs(0.3).await?;
    let d = back(&t);
    t.probe("blocks back with a wall at 1.5", d, d > 1.3 && d < 1.5);
    // Turning, it slides along the wall: no jumps from one block to the next.
    let mut last = d;
    let mut worst = 0.0f32;
    for i in 1..=30 {
        t.c().set_look(i as f32, 0.0);
        t.frames(2).await?;
        let d = back(&t);
        worst = worst.max((d - last).abs());
        last = d;
    }
    t.probe("biggest step along the wall, blocks", worst, worst < 0.08);
    t.op(&format!("fill {} {} {} {} {} {} core:air", f[0] - 6, f[1], f[2] + 2, f[0] + 6, f[1] + 5, f[2] + 2))
        .await?;
    for _ in 0..2 {
        t.press("core:perspective");
        t.frames(2).await?;
    }
    t.secs(0.2).await?;
    t.check_eq(t.state().camera, 0, "F5 comes round to first person");

    t.press("core:freecam");
    t.secs(0.4).await?;
    t.check_eq(t.state().camera, 0, "no free camera in survival");
    t.op("mode creative").await?;
    t.secs(0.2).await?;
    t.press("core:freecam");
    t.secs(0.4).await?;
    t.check_eq(t.state().camera, 3, "the free camera in creative");
    let pos = t.state().pos;
    t.c().set_look(0.0, 0.0);
    t.hold("core:forward", 1.0);
    t.secs(1.3).await?;
    let d = back(&t);
    t.probe("blocks the free camera flew", d, d > 5.0);
    let s = t.state().pos;
    let stood = ((s.x - pos.x).powi(2) + (s.z - pos.z).powi(2)).sqrt() as f32;
    t.probe("blocks the player moved meanwhile", stood, stood < 0.05);
    t.press("core:freecam");
    t.secs(0.3).await?;
    t.check_eq(t.state().camera, 0, "its key again comes back");

    let orbit = CameraMode::generated("testmod:orbit".to_string()).id();
    t.press("testmod:orbit");
    t.secs(0.8).await?;
    t.check_eq(t.state().camera, orbit, "a mod's camera on its key");
    let d = back(&t);
    t.probe("blocks the orbit is out", d, (d - 6.0).abs() < 0.3);
    t.press("testmod:orbit");
    t.secs(0.8).await?;

    t.op("camera core:behind 1").await?;
    t.secs(0.3).await?;
    t.check_eq(t.state().camera, 1, "the server puts it behind");
    t.press("core:perspective");
    t.secs(0.2).await?;
    t.check_eq(t.state().camera, 1, "and holds it there");
    t.op("camera release").await?;
    t.secs(0.3).await?;
    t.check_eq(t.state().camera, 0, "released, back to its own");
    Ok(())
}

/// Mods change a player's state from the server, and its client follows: a mace's smash by the
/// fall it reads, then the look, the hand, flight and fire set from outside.
/// `r.edit` of an item and a biome: the edit's values win, the rest stays (testkit's own, see
/// its registration; its ore, liquid, dimension and terrain edits of testmod's would fail the load
/// if they found nothing). A block's edit reaches every state, or the states of a key, and a
/// template's blocks; `r.set` and `r.tag` by tag and by state.
pub async fn edits(t: T) -> R {
    t.flat().await?;
    t.check_eq(
        t.op("edited").await?,
        "8 3 2 true true/true/false 4 true base:cobblestonex2,testkit:editedx1 true true true true [0.9, 2.5, 1.0] 3 4/15/1.5/false,4/3/1.5/true 40 true"
            .to_string(),
        "max stack, damage, warmth, tag, a weather's tags, an attribute's default, a recipe removed, a loot table's pools, a liquid's id, its block's and item's tag, a column of Mars, Mars' tag, an entity kind's size and eye, its value after the edits, a block's states after an edit of every state and one of a key (light, opacity, a value set by tag, a tag set on one state), a stair's states and their edited light",
    );
    // The edit of the menu adds two slots to the equipment (four and the other hand) and the
    // player's 36.
    t.op(&format!("emenu testkit:gear {}", t.c().player()))
        .await?;
    let gear = Menu::lookup("testkit:gear").unwrap_or(u32::MAX);
    t.until("the edited menu open", Duration::from_secs(5), |t| t.c().menu().is_some_and(|m| m.kind == gear))
        .await?;
    t.check_eq(t.c().menu().map(|m| m.slots), Some(43), "the slots of a menu an edit added a group to");
    t.c().menu_close();
    t.no_screen().await
}

pub async fn entity_modify(t: T) -> R {
    t.flat().await?;
    let (dmg, fell) = {
        let s = t.op("attack_damage testmod:mace 5").await?;
        let mut p = s
            .split_whitespace()
            .map(|v| v.parse::<f64>().unwrap_or(f64::NAN));
        (p.next().unwrap_or(f64::NAN), p.next().unwrap_or(f64::NAN))
    };
    t.check_eq(fell, 5.0, "the fall distance a mod set");
    t.check_eq(dmg, 22.0, "a mace after five blocks: 6 + 4 * 3 + 2 * 2");
    let (plain, _) = {
        let s = t.op("attack_damage base:stick 5").await?;
        (s.split_whitespace().next().unwrap_or("").to_string(), ())
    };
    t.check_eq(plain, "6".to_string(), "anything else doesn't care how far it fell");
    let ground = t.state().pos.y;
    t.check_eq(t.op("edit_me 1.5 4 1 60").await?, "4 60".to_string(), "slot and fire on the server");
    t.until("the client follows", Duration::from_secs(10), |t| {
        let s = t.state();
        (s.yaw - 1.5).abs() < 1e-3 && s.selected == 4 && t.flag(client::client_flags::FLYING) && s.pos.y > ground + 2.5
    })
    .await?;

    // The smash throws back what stands around the one hit.
    let f = t.feet();
    let f = [f[0], f[1] - 3, f[2]];
    t.op("put me 4 testmod:mace").await?;
    let hit = t
        .op(&format!("spawn base:pig {} {} {}", f[0], f[1], f[2] - 3))
        .await?;
    let near = t
        .op(&format!("spawn base:pig {} {} {}", f[0] + 1, f[1], f[2] - 3))
        .await?;
    t.secs(0.5).await?;
    let x = |s: String| {
        s.split_whitespace()
            .nth(1)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::NAN)
    };
    let before = x(t.op(&format!("entity {near}")).await?);
    t.op(&format!("hit_falling {hit} 5")).await?;
    t.secs(0.5).await?;
    let after = x(t.op(&format!("entity {near}")).await?);
    t.probe("the pig beside is thrown back", (after - before) as f32, after - before > 0.8);
    Ok(())
}

/// Menus open on a storage a player carries and on far away blocks; a menu for a block without
/// an entity is refused with a reason instead of opening something broken.
pub async fn virtual_menus(t: T) -> R {
    t.flat().await?;
    t.check_eq(t.op("stash base:chest diamond 3 stick 10").await?, "54".to_string(), "stash slots");
    t.screen("core:menu").await?;
    let diamond = Item::lookup("base:diamond").ok_or("no item base:diamond")?;
    t.until("chest contents on the client", Duration::from_secs(5), |t| t.c().menu_slot(0).item == diamond)
        .await?;
    t.check_eq(t.c().menu_slot(1).count, 10, "second stash slot");
    t.shot("virtual_chest").await?;
    t.c().menu_close();
    t.no_screen().await?;

    // On a held stack: what goes in stays in that stack, and the menu closes when it goes.
    t.op("put me 8 testmod:pouch").await?;
    t.check_eq(t.op("hput 8 2 base:stick 5").await?, "9".to_string(), "pouch slots");
    t.op("hmenu testmod:pouch 8").await?;
    t.screen("core:menu").await?;
    let stick = Item::lookup("base:stick").ok_or("no item base:stick")?;
    t.until("pouch contents on the client", Duration::from_secs(5), |t| t.c().menu_slot(2).item == stick)
        .await?;
    t.op("put me 8 base:dirt").await?;
    t.no_screen().await?;
    t.op("put me 8 testmod:pouch").await?;
    t.op("hmenu testmod:pouch 8").await?;
    t.frames(10).await?;
    t.check(t.c().menu_slot(2).is_empty(), "a new pouch shows the old one's items");
    t.c().menu_close();
    t.no_screen().await?;

    let f = t.feet();
    let plain = add(f, [2, -1, 0]);
    match t
        .op(&format!("menu base:furnace {} {} {}", plain[0], plain[1], plain[2]))
        .await
    {
        Ok(_) => t.fail("a furnace menu opened on grass"),
        Err(e) => {
            t.check(e.contains("no block entity"), format!("unexpected refusal: {e}"));
        }
    }
    t.frames(5).await?;
    t.check(t.state().screen.len == 0, "a screen opened for the refused menu");

    // Out of reach and one after another.
    let machines = [
        ("base:furnace", "base:furnace", [20, 0, 0]),
        ("base:chest", "base:chest", [-20, 0, 5]),
        ("testmod:generator", "testmod:generator", [0, 0, 24]),
    ];
    for (block, menu, d) in machines {
        let p = add(f, d);
        t.set(p, block).await?;
        t.op(&format!("menu {menu} {} {} {}", p[0], p[1], p[2]))
            .await?;
        let want = Menu::lookup(menu).unwrap_or(u32::MAX);
        t.until(&format!("{menu} open"), Duration::from_secs(5), |t| t.c().menu().is_some_and(|m| m.kind == want))
            .await?;
        t.screen("core:menu").await?;
    }
    t.c().menu_close();
    t.no_screen().await
}

/// Fresh world load time and idle frame time on this machine (thresholds are generous: they
/// catch large regressions, the numbers themselves go to the report).
pub async fn load_benchmark(t: T) -> R {
    let start = std::time::Instant::now();
    t.world(5).await?;
    t.idle().await?;
    let s = t.state();
    t.note(format!(
        "idle after {:.2} s with {} cubes (generated {}, {:.0} us each)",
        start.elapsed().as_secs_f32(),
        s.cubes,
        s.generated,
        s.gen_us
    ));
    let b = t.bench(4.0, "idle").await?;
    t.check(b[2] < 50.0, format!("p99 frame {:.1} ms", b[2]));
    t.check(b[4] < 25.0, format!("server tick {:.1} ms", b[4]));
    Ok(())
}

/// Keeping up with the fastest flight at the longest view: how far ahead the world has arrived
/// each second (the view is 1024 blocks), how many meshes wait, and the frames meanwhile.
pub async fn fly_load(t: T) -> R {
    use weft_sdk::settings::VIEW_DISTANCE;
    static LOD: Config<u32> = Config::named("core:lod_distance");
    VIEW_DISTANCE.set(64);
    LOD.set(1024);
    let start = std::time::Instant::now();
    t.world(5).await?;
    t.op("mode spectator").await?;
    t.idle().await?;
    let s = t.state();
    t.note(format!(
        "idle after {:.2} s with {} cubes (generated {}, {:.0} us each)",
        start.elapsed().as_secs_f32(),
        s.cubes,
        s.generated,
        s.gen_us
    ));
    let cubes_at_start = s.generated;
    // The way with the most mountains: their slopes ask the most of what comes first.
    let mountains = |x: i32| {
        (0..24)
            .filter(|i| name_of(ek::BIOME, OVERWORLD.column(5, x, -400 * i).biome) == "base:mountains")
            .count()
    };
    let way = (-40..40)
        .map(|i| i * 512)
        .max_by_key(|&x| mountains(x))
        .unwrap_or(0);
    t.note(format!("flying along x {way}: mountains at {} of 24 points", mountains(way)));
    t.cmd(&format!("/tp {way} 300 0")).await?;
    t.secs(0.5).await?;
    t.loaded().await?;
    t.idle().await?;
    t.c().set_look(0.0, 0.0);
    // A step of the wheel per frame, up to spectators' top speed.
    for _ in 0..12 {
        t.c().input_scroll(1.0);
        t.frames(2).await?;
    }
    let secs = 45.0;
    t.hold("core:forward", secs);
    t.hold("core:sprint", secs);
    let begin = std::time::Instant::now();
    let (mut worst, mut last_pos, mut samples, mut lows) = (i32::MAX, t.state().pos, Vec::new(), 0);
    let mut peak_pending = 0;
    let (mut looked, mut undrawn) = (0u32, 0u32);
    while begin.elapsed().as_secs_f32() < secs - 12.0 {
        t.secs(1.0).await?;
        let s = t.state();
        let speed = ((s.pos.x - last_pos.x).powi(2) + (s.pos.z - last_pos.z).powi(2)).sqrt();
        last_pos = s.pos;
        // Yaw 0 looks along -Z: the ground ahead, the first column not there yet.
        let reach = (1..=64)
            .map(|i| i * 16)
            .find(|d| {
                let (x, z) = (s.pos.x.floor() as i32, s.pos.z.floor() as i32 - d);
                t.c()
                    .block([x, OVERWORLD.column(5, x, z).height, z])
                    .is_none()
            })
            .unwrap_or(1040)
            - 16;
        if begin.elapsed().as_secs_f32() > 6.0 {
            worst = worst.min(reach);
            lows += (reach < 512) as u32;
        }
        peak_pending = peak_pending.max(s.pending_meshes);
        samples.push(format!("{:.0}b/s reach {reach} meshes {}", speed, s.pending_meshes));
        // Every few seconds: the ground ahead, well behind the front of what has arrived, has to
        // be drawn: a cube here but waiting for its mesh is a hole in the picture.
        if samples.len() % 5 == 4 {
            let (px, pz) = (s.pos.x.floor() as i32, s.pos.z.floor() as i32);
            for dz in (32..(reach / 2).max(33)).step_by(16) {
                for dx in (-448..=448).step_by(16) {
                    let (x, z) = (px + dx, pz - dz);
                    let top = OVERWORLD.column(5, x, z).height;
                    if t.c().block([x, top, z]).is_some() {
                        looked += 1;
                        undrawn += !t.c().cube_shown([x, top, z]) as u32;
                    }
                }
            }
        }
    }
    for line in samples.chunks(5) {
        t.note(line.join(" | "));
    }
    t.note(format!("ground ahead: {looked} columns looked at, {undrawn} not drawn"));
    t.shot("flying").await?;
    let b = t.bench(10.0, "flying").await?;
    let s = t.state();
    t.note(format!("flew to {:.0}: worst reach ahead {worst} blocks, seconds under 512: {lows}, peak meshes waiting {peak_pending}, generated {} cubes on the way, p99 {:.1} ms", s.pos.z, s.generated - cubes_at_start, b[2]));
    // Stopped, the rest of the columns comes.
    t.secs(2.5).await?;
    let stop = std::time::Instant::now();
    t.idle().await?;
    t.note(format!("after stopping: all in after {:.1} s", stop.elapsed().as_secs_f32()));
    t.shot("stopped").await?;
    VIEW_DISTANCE.set(12);
    LOD.set(256);
    Ok(())
}

/// Craters of growing radius through the mod API as it is (`w.set` per block), each on fresh
/// ground: the loop in the mod, the command round trip (the settling after it included), then
/// frames and server ticks while clients remesh.
pub async fn crater(t: T) -> R {
    t.world(5).await?;
    t.idle().await?;
    t.op("mode spectator").await?;
    for (i, r) in [16, 32, 48, 64].into_iter().enumerate() {
        let x = 4000 * (i as i32 + 1);
        let h = OVERWORLD.column(5, x, 0).height;
        t.cmd(&format!("/tp {x} {h} 0")).await?;
        t.secs(0.5).await?;
        t.loaded().await?;
        t.idle().await?;
        let start = std::time::Instant::now();
        let out = t.op(&format!("crater {r}")).await?;
        let round = start.elapsed().as_secs_f64() * 1000.0;
        let v: Vec<&str> = out.split_whitespace().collect();
        t.note(format!(
            "r {r}: {} set, {} already air, {} not loaded; loop {} ms, command round trip {round:.0} ms",
            v[0], v[1], v[2], v[3]
        ));
        t.bench(3.0, &format!("after crater {r}")).await?;
    }
    // The area edit: one call, the engine's own log line says where the time went
    // (`area fill r ...`); here the round trip, what is left, and the frames after.
    for (i, r) in [64, 128, 250, 500, 1000].into_iter().enumerate() {
        let x = 4000 * (i as i32 + 5);
        let h = OVERWORLD.column(5, x, 0).height;
        t.cmd(&format!("/tp {x} {h} 0")).await?;
        t.secs(0.5).await?;
        t.loaded().await?;
        t.idle().await?;
        let f = t.feet();
        let start = std::time::Instant::now();
        t.op(&format!("crater_fast {r}")).await?;
        // The fill runs at the end of the tick the command came in; the next answer comes after.
        t.op("look").await?;
        t.note(format!("fast r {r}: done {:.0} ms after the command", start.elapsed().as_secs_f64() * 1000.0));
        // Deeper than that is beyond what is loaded below the player.
        if r <= 64 {
            let below = add(f, [0, -(r - 2), 0]);
            t.check_eq(
                t.op(&format!("get {} {} {}", below[0], below[1], below[2]))
                    .await?,
                "core:air".to_string(),
                &format!("r {r}: hollow near the bottom"),
            );
            let out = add(f, [0, -(r + 2), 0]);
            t.check(
                t.op(&format!("get {} {} {}", out[0], out[1], out[2]))
                    .await?
                    != "core:air",
                format!("r {r}: solid below the crater"),
            );
            let light: u32 = t
                .op(&format!("light {} {} {}", below[0], below[1], below[2]))
                .await?
                .parse()
                .unwrap_or(0);
            t.check_eq(light & 255, 15, &format!("r {r}: full sky light near the bottom, the crater relit"));
        }
        // From beyond the rim, looking at the middle: the far part was never loaded when it
        // went off and gets the crater as it loads.
        let rim = f[2] + r + 40;
        let top = OVERWORLD.column(5, f[0], rim).height.max(f[1]);
        t.cmd(&format!("/tp {} {} {}", f[0], top + 40, rim)).await?;
        t.c().set_look(0.0, -25.0);
        t.secs(1.0).await?;
        t.loaded().await?;
        t.secs(1.0).await?;
        t.shot(&format!("crater_{r}")).await?;
        t.bench(3.0, &format!("after fast crater {r}")).await?;
    }
    Ok(())
}

/// Distant terrain shows what was built: a tower's columns reach the client once its cubes are
/// saved (left behind), and again after reopening; the untouched land around costs nothing.
pub async fn lod_columns(t: T) -> R {
    t.world(5).await?;
    t.op("mode creative").await?;
    let f = t.feet();
    t.check_eq(t.state().lod_columns, 0, "nothing built, nothing sent");
    t.op(&format!("fill {} {} {} {} {} {} base:stone", f[0] + 4, f[1], f[2] + 4, f[0] + 6, f[1] + 40, f[2] + 6))
        .await?;
    t.cmd(&format!("/tp {} 200 {}", f[0] + 3000, f[2])).await?;
    t.until("the tower's columns sent", Duration::from_secs(30), |t| t.state().lod_columns >= 9)
        .await?;
    let n = t.state().lod_columns;
    t.probe("columns sent", n as f32, n < 64);
    let name = format!("test-{}", t.name());
    t.c().world_leave();
    t.screen("core:title").await?;
    t.c().world_open(&name, 5);
    t.until("joined again", Duration::from_secs(60), |t| t.flag(client::client_flags::JOINED))
        .await?;
    t.until("the columns again after reopening", Duration::from_secs(5), |t| t.state().lod_columns >= 9)
        .await?;
    Ok(())
}

/// A crater bigger than what is loaded survives closing the world: the part never loaded gets
/// it when visited after reopening, and what was built in the crater since stays.
pub async fn crater_saved(t: T) -> R {
    t.world(8).await?;
    t.idle().await?;
    t.op("mode creative").await?;
    let f = t.feet();
    t.op("crater_fast 600").await?;
    t.op("look").await?;
    let built = add(f, [3, -20, 0]);
    t.set(built, "gold_block").await?;
    let name = format!("test-{}", t.name());
    t.c().world_leave();
    t.screen("core:title").await?;
    t.c().world_open(&name, 8);
    t.until("joined again", Duration::from_secs(60), |t| t.flag(client::client_flags::JOINED))
        .await?;
    t.loaded().await?;
    t.check_eq(t.block(built).await?, "base:gold_block".to_string(), "a block built in the crater stays after reopening");
    let far = [f[0] + 450, f[1] - 100, f[2]];
    t.cmd(&format!("/tp {} {} {}", far[0], far[1], far[2]))
        .await?;
    t.secs(1.0).await?;
    t.loaded().await?;
    for p in [far, add(far, [0, -40, 0]), add(far, [0, 40, 30])] {
        t.check_eq(t.block(p).await?, "core:air".to_string(), &format!("{p:?}, first loaded after reopening, is in the crater"));
    }
    Ok(())
}

use base_api::OVERWORLD;

/// Testmod edits Mars as from outside: a biome tried before its dust, with its own surface
/// and boulders, and a rock needle from a feature written in testmod.
pub async fn foreign_terrain(t: T) -> R {
    static MARS: Dimension = Dimension::named("testmod:mars");
    t.world(5).await?;
    t.op("mode spectator").await?;
    t.cmd("/dim testmod:mars").await?;
    t.until("on Mars", Duration::from_secs(20), |t| t.state().dim == MARS.id())
        .await?;
    let seed = 5;
    // testmod's `NEEDLE`.
    let needle = MARS.column(seed, 1000, 1000);
    let beside = MARS.column(seed, 1010, 1000);
    t.note(format!("needle {} beside {}", needle.height, beside.height));
    t.check(needle.height - beside.height > 60, "the needle rises from the land around it");
    t.cmd(&format!("/tp 1000 {} 1000", needle.height + 3))
        .await?;
    t.secs(0.5).await?;
    t.loaded().await?;
    let top = t.op(&format!("get 1000 {} 1000", needle.height)).await?;
    t.check(top != "core:air", format!("the needle is generated where the terrain says ({top})"));

    let grid: Vec<(i32, i32, Column)> = (-80..80)
        .flat_map(|i| (-80..80).map(move |j| (i * 64, j * 64)))
        .map(|(x, z)| (x, z, MARS.column(seed, x, z)))
        .collect();
    let count = |name: &str| {
        grid.iter()
            .filter(|g| name_of(ek::BIOME, g.2.biome) == name)
            .count()
    };
    t.note(format!("of {} columns: dust {}, scorched {}", grid.len(), count("testmod:dust"), count("testmod:scorched")));
    let scorched = grid
        .into_iter()
        .find(|&(x, z, c)| name_of(ek::BIOME, c.biome) == "testmod:scorched" && !MARS.surface_carved(seed, x, z));
    let (x, z, c) = scorched.ok_or("no scorched biome on Mars")?;
    t.note(format!("scorched at {x} {z}, height {}", c.height));
    t.cmd(&format!("/tp {x} {} {z}", c.height + 3)).await?;
    t.secs(0.5).await?;
    t.loaded().await?;
    let ground = t.op(&format!("get {x} {} {z}", c.height)).await?;
    t.check(ground == "testmod:regolith" || ground == "testmod:basalt", format!("the scorched biome's surface ({ground})"));
    let census = t.op("census 24 testmod:basalt,testmod:regolith").await?;
    t.note(census.clone());
    let n: Vec<u32> = census
        .split_whitespace()
        .filter_map(|e| e.rsplit_once('=')?.1.parse().ok())
        .collect();
    t.check(n.first().is_some_and(|&b| b > 0), "boulders from testmod's decoration");
    t.check(n.get(1).is_some_and(|&r| r > 100), "the biome's regolith");
    Ok(())
}

/// Plants grow on ground that is the top layer of its cube like on any other, though that ground
/// lies in the cube below theirs. Sugar cane only grows at sea level, which is such a layer.
pub async fn plants_on_cube_tops(t: T) -> R {
    t.world(5).await?;
    t.op("mode spectator").await?;
    let tops = |x: i32, z: i32| {
        (-20..=20)
            .flat_map(|dz| (-20..=20).map(move |dx| (dx, dz)))
            .filter(|&(dx, dz)| (dx + dz) % 3 == 0 && (OVERWORLD.column(5, x + dx, z + dz).height + 1) & 15 == 0)
            .count()
    };
    let plains = (-40..40)
        .flat_map(|i| (-40..40).map(move |j| (i * 48, j * 48)))
        .filter(|&(x, z)| {
            [(0, 0), (20, 0), (-20, 0), (0, 20), (0, -20)]
                .iter()
                .all(|&(dx, dz)| name_of(ek::BIOME, OVERWORLD.column(5, x + dx, z + dz).biome) == "base:plains")
        })
        .take(40)
        .max_by_key(|&(x, z)| tops(x, z));
    let (x, z) = plains.ok_or("no plains")?;
    t.cmd(&format!("/tp {x} {} {z}", OVERWORLD.column(5, x, z).height + 6))
        .await?;
    t.secs(0.5).await?;
    t.loaded().await?;
    let found = t.op("cube_top_plants 20").await?;
    t.note(format!("{x} {z}: {found}"));
    let n: Vec<f32> = found
        .split_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect();
    let [top, top_plants, other, other_plants] = n[..] else { return Err(format!("cube_top_plants: {found}")) };
    t.check(top >= 50.0, format!("too few columns to tell ({top})"));
    let (on_top, elsewhere) = (top_plants / top.max(1.0), other_plants / other.max(1.0));
    t.probe("plants on the top layer of a cube, against the others", on_top / elsewhere.max(0.001), on_top > elsewhere * 0.4);

    let shore = |x: i32, z: i32| {
        OVERWORLD.column(5, x, z).height == 63
            && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|&(dx, dz)| OVERWORLD.column(5, x + dx, z + dz).height < 63)
    };
    let near = |x: i32, z: i32| {
        (-16..=16)
            .flat_map(|dz| (-16..=16).map(move |dx| (dx, dz)))
            .filter(|&(dx, dz)| shore(x + dx, z + dz))
            .count()
    };
    let beach = (-60..60)
        .flat_map(|i| (-60..60).map(move |j| (i * 24, j * 24)))
        .filter(|&(x, z)| shore(x, z) && name_of(ek::BIOME, OVERWORLD.column(5, x, z).biome) == "base:beach")
        .take(30)
        .max_by_key(|&(x, z)| near(x, z));
    let (x, z) = beach.ok_or("no beach")?;
    t.cmd(&format!("/tp {x} 70 {z}")).await?;
    t.secs(0.5).await?;
    t.loaded().await?;
    let cane = t.op("census 28 base:sugar_cane").await?;
    t.note(format!("{x} {z}, {} shore columns: {cane}", near(x, z)));
    let canes: u32 = cane
        .rsplit_once('=')
        .and_then(|v| v.1.parse().ok())
        .unwrap_or(0);
    t.check(canes > 0, "sugar cane on the shore");
    Ok(())
}

/// `/tick`: a frozen world keeps its time, steps exactly, and a sprint runs a day at once.
pub async fn tick_control(t: T) -> R {
    t.flat().await?;
    let time = async || -> Result<u64, String> {
        let r = t.cmd("/time query").await?;
        r.rsplit("tick ")
            .next()
            .and_then(|v| v.trim_end_matches(')').parse().ok())
            .ok_or(format!("time: {r}"))
    };
    t.cmd("/tick freeze").await?;
    let a = time().await?;
    t.secs(0.5).await?;
    t.check_eq(time().await?, a, "time while frozen");
    t.cmd("/tick step 10").await?;
    t.secs(1.0).await?;
    t.check_eq(time().await?, a + 10, "time after ten steps");
    t.cmd("/tick unfreeze").await?;
    let b = time().await?;
    t.cmd("/tick sprint 24000").await?;
    t.until("sprint done", Duration::from_secs(60), |t| (0..20).any(|i| t.c().chat_line(i).contains("sprinted")))
        .await?;
    t.check(time().await? >= b + 24000, "a day went by in the sprint");
    Ok(())
}

/// Frame time with thousands of dropped items in view (item farms).
pub async fn item_farm(t: T) -> R {
    t.flat().await?;
    t.op("drops 3000").await?;
    t.c().set_look(0.0, -30.0);
    t.secs(2.0).await?;
    t.note(format!("{} entities on the client", t.c().entities().len()));
    t.shot("farm").await?;
    let b = t.bench(4.0, "item_farm").await?;
    t.note(format!(
        "fps {:.0}, frame {:.2} ms, p99 {:.2} ms (in tests the hidden window's presents are throttled: compare the log's cpu record ms)",
        b[0], b[1], b[2]
    ));
    Ok(())
}

/// A crowd of walking mobs in view: what posing their bones costs a frame.
pub async fn mob_crowd(t: T) -> R {
    t.flat().await?;
    t.cmd("/rule testkit:peace 1").await?;
    t.op("crowd base:zombie 4000").await?;
    t.op("crowd base:pig 4000").await?;
    t.c().set_look(0.0, -20.0);
    t.secs(4.0).await?;
    t.note(format!("{} entities on the client", t.c().entities().len()));
    t.shot("crowd").await?;
    t.bench(5.0, "mob_crowd").await?;
    Ok(())
}

/// Plants grow by world time: wheat ripens when the clock jumps ahead, and sugar cane in an area
/// nobody was in catches up when the player comes back.
pub async fn growth(t: T) -> R {
    t.flat().await?;
    t.op("time 1000").await?;
    let f = t.feet();
    let farm = add(f, [2, -1, 0]);
    t.set(farm, "farmland").await?;
    t.set(add(farm, [0, 1, 0]), "wheat").await?;
    // Sugar cane needs water beside the block it stands on.
    let cane = add(f, [-2, 0, 0]);
    t.set(cane, "sugar_cane").await?;
    t.secs(0.2).await?;
    t.check_eq(t.block(cane).await?, "core:air".to_string(), "sugar cane without water breaks off");
    t.set(add(cane, [0, -1, 1]), "water").await?;
    t.set(cane, "sugar_cane").await?;
    t.op("skip 199000").await?;
    t.secs(0.5).await?;
    t.check_eq(t.block(add(farm, [0, 1, 0])).await?, "base:wheat[age=7]".to_string(), "wheat after a jump of 2.5 hours");
    t.check_eq(t.block(add(cane, [0, 2, 0])).await?, "base:sugar_cane".to_string(), "sugar cane after the jump");
    // Far away the cane's area unloads; time passes; coming back, it has grown.
    let cane2 = add(f, [-4, 0, 0]);
    t.set(add(cane2, [0, -1, -1]), "water").await?;
    t.set(cane2, "sugar_cane").await?;
    t.cmd("/tp 3000 10 3000").await?;
    t.secs(3.0).await?;
    t.check_eq(t.block(cane2).await?, "core:air".to_string(), "far away, the cane's area is not here");
    t.op("skip 200000").await?;
    t.cmd(&format!("/tp {} {} {}", f[0], f[1], f[2])).await?;
    t.secs(1.0).await?;
    t.loaded().await?;
    t.secs(0.5).await?;
    t.check_eq(t.block(add(cane2, [0, 2, 0])).await?, "base:sugar_cane".to_string(), "sugar cane grown while unloaded");
    Ok(())
}

/// Time the world skips is made up, without random ticks: grass takes bare dirt and dies under a
/// block, a furnace smelts what its fuel allows and a generator runs an electric furnace through
/// a cable, all after a jump of the time and, for an area that was unloaded, when it comes back.
pub async fn catch_up(t: T) -> R {
    t.flat().await?;
    t.op("time 1000").await?;
    let f = t.feet();
    let dirt = add(f, [3, -1, 0]);
    t.set(dirt, "dirt").await?;
    let covered = add(f, [3, -1, 3]);
    t.set(covered, "grass").await?;
    t.set(add(covered, [0, 1, 0]), "stone").await?;
    let furnace = add(f, [-3, 0, 0]);
    t.set(furnace, "furnace").await?;
    t.op(&format!("insert {} raw_iron 10", at(furnace))).await?;
    t.op(&format!("insert {} coal 1", at(furnace))).await?;
    let g = add(f, [-3, 0, 3]);
    t.set(g, "testmod:generator").await?;
    t.set(add(g, [-1, 0, 0]), "testmod:cable").await?;
    let e = add(g, [-2, 0, 0]);
    t.set(e, "testmod:electric_furnace").await?;
    t.op(&format!("insert {} coal 4", at(g))).await?;
    t.op(&format!("insert {} iron_ore 3", at(e))).await?;
    t.secs(0.3).await?;
    t.check_eq(t.block(dirt).await?, "base:dirt".to_string(), "bare dirt is still dirt");
    t.op("skip 199000").await?;
    // Made up at once, in the tick after the jump.
    t.secs(1.0).await?;
    t.check_eq(t.block(dirt).await?, "base:grass".to_string(), "grass took the bare dirt");
    t.check_eq(t.block(covered).await?, "base:dirt".to_string(), "grass under stone died");
    // One coal burns 1600 ticks: eight of the ten ores, no more.
    t.check_eq(t.slot(&at(furnace), 2).await?, ("base:iron_ingot".into(), 8, 0), "furnace smelted what its coal allows");
    t.check_eq(t.slot(&at(furnace), 0).await?, ("base:raw_iron".into(), 2, 0), "and left the rest");
    t.check_eq(t.slot(&at(e), 1).await?, ("base:iron_ingot".into(), 3, 0), "the generator powered the electric furnace");
    // Unloaded the whole time: done when the area comes back.
    let far = add(f, [-6, 0, 0]);
    t.set(far, "furnace").await?;
    t.op(&format!("insert {} raw_iron 4", at(far))).await?;
    t.op(&format!("insert {} coal 1", at(far))).await?;
    t.secs(0.2).await?;
    t.cmd("/tp 3000 10 3000").await?;
    t.secs(3.0).await?;
    t.op("skip 200000").await?;
    t.cmd(&format!("/tp {} {} {}", f[0], f[1], f[2])).await?;
    t.secs(1.0).await?;
    t.loaded().await?;
    t.secs(0.5).await?;
    t.check_eq(t.slot(&at(far), 2).await?, ("base:iron_ingot".into(), 4, 0), "a furnace caught up while unloaded");
    Ok(())
}

/// What a player carries over leaving and opening a world again: placed blocks, the inventory,
/// a moving structure with a block added, permanent attribute modifiers; server-only stack data
/// is not saved, and expired modifiers are gone before.
pub async fn persistence(t: T) -> R {
    t.world(7).await?;
    let p = add(t.feet(), [2, 0, 2]);
    t.set(p, "gold_block").await?;
    t.op("put me 3 diamond 5").await?;
    // A moving structure with a block added, in a cube nothing else changed.
    let skiff = t
        .op(&format!("spawn testmod:skiff {} {} {}", p[0], p[1] + 6, p[2] + 8))
        .await?;
    let skiff = skiff.trim().trim_start_matches('#').to_string();
    t.op(&format!("sblock {skiff} 0 1 0 base:glass")).await?;
    t.op("put me 0 iron_sword 1").await?;
    // A machine's contents are kept, and its entity can't be made without the block.
    let chest = add(p, [-3, 0, 0]);
    t.set(chest, "chest").await?;
    t.op(&format!("insert {} {} {} coal 5", chest[0], chest[1], chest[2]))
        .await?;
    t.check_eq(
        t.op(&format!("spawn base:chest {} {} {}", p[0], p[1] + 2, p[2]))
            .await?,
        "0".to_string(),
        "no chest entity without a chest",
    );
    let said = t.cmd("/summon base:furnace").await?;
    t.check(said.contains("belongs to a block"), format!("/summon refuses a furnace: {said}"));
    t.op("secret me 0 1234").await?;
    t.check_eq(t.op("secret me 0").await?, "1234".into(), "server keeps the secret");
    t.frames(5).await?;
    t.check_eq(crate::server::SECRET.on_stack_own(&t.c().inventory_slot(0)), None, "client never sees the secret");
    // What a mod makes with a handle, data and all: the client has it, and damage is its own.
    static STICK: Item = Item::named("base:stick");
    let made = STICK.stack(3).with_wear(5);
    t.check_eq((made.item, made.count, made.wear(), made.without(&DAMAGE).data), (STICK.id(), 3, 5, 0), "a stack made from a handle");
    t.check_eq(value(&t.op("attr me core:movement_speed").await?), 1.0, "speed without modifiers");
    t.op("attr_mod me core:movement_speed testkit:haste 1 0.5 40")
        .await?;
    t.op("attr_mod me core:max_health testkit:vigor 0 10 0")
        .await?;
    let speed = t.op("attr me core:movement_speed").await?;
    t.check_eq(value(&speed), 1.5, "speed with +50% of base");
    t.check(speed.contains("testkit:haste"), format!("explained: {speed}"));
    t.check_eq(value(&t.op("attr me core:max_health").await?), 30.0, "max health +10");
    t.secs(2.5).await?;
    t.check_eq(value(&t.op("attr me core:movement_speed").await?), 1.0, "the 40 tick modifier ran out");
    // A timer due after the reload is saved with the player and fires there.
    let rings: u32 = t.op("rings").await?.parse().unwrap_or(0);
    t.op("after 120").await?;
    // A world's setting (a game rule) goes with the world.
    t.cmd("/rule testkit:count_marks 1").await?;

    let name = format!("test-{}", t.name());
    t.c().world_leave();
    t.screen("core:title").await?;
    t.c().world_open(&name, 7);
    t.until("joined again", Duration::from_secs(60), |t| t.flag(client::client_flags::JOINED))
        .await?;
    t.loaded().await?;
    t.check_eq(t.block(p).await?, "base:gold_block".into(), "placed block after reload");
    t.check_eq(t.slot("me", 3).await?, ("base:diamond".into(), 5, 0), "inventory after reload");
    t.check_eq(t.op(&format!("sblock {skiff} 0 1 0")).await?, "base:glass".to_string(), "the structure and its added block after reload");
    t.check_eq(t.slot("me", 0).await?.0, "base:iron_sword".into(), "the sword was saved");
    t.check_eq(
        t.op(&format!("inv {} {} {}", chest[0], chest[1], chest[2]))
            .await?,
        "0:base:coal:5".to_string(),
        "the chest kept its coal",
    );
    t.check_eq(t.op("secret me 0").await?, "none".into(), "the secret was not");
    t.check_eq(value(&t.op("attr me core:max_health").await?), 30.0, "max health modifier after reload");
    let mut now = rings;
    for _ in 0..40 {
        now = t.op("rings").await?.parse().unwrap_or(0);
        if now > rings {
            break;
        }
        t.secs(0.25).await?;
    }
    t.check_eq(now, rings + 1, "the timer saved with the player fired after the reload");
    let rule = t.cmd("/rule testkit:count_marks").await?;
    t.check(rule.contains("true"), format!("the world's setting after reload: {rule}"));
    t.cmd("/rule testkit:count_marks 0").await?;
    // One maximum everywhere: /heal fills to it and the client hears the same.
    t.cmd("/heal").await?;
    t.check_eq(t.op("health").await?, "30".to_string(), "healed to the modified maximum");
    t.until("the client has 30", Duration::from_secs(2), |t| t.state().health == 30.0)
        .await?;
    t.op("attr_rm me core:max_health testkit:vigor").await?;
    t.check_eq(value(&t.op("attr me core:max_health").await?), 20.0, "removed");
    t.sync_inventory().await
}

/// First number of an `attr` answer ("1.5 | ...").
fn value(s: &str) -> f64 {
    s.split('|')
        .next()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(f64::NAN)
}

/// Dimensions: each mod's own one is reached and drawn as itself; the engine's void is empty,
/// without gravity and refuses blocks; the debug dimension shows every state and never updates.
/// What a mod reads of the world's seed, and `/seed`.
pub async fn world_seed(t: T) -> R {
    t.world(5).await?;
    let seeds = |t: T, p: [i32; 3]| async move {
        let line = t.op(&format!("seed {}", at(p))).await?;
        let v: Vec<u64> = line
            .split_whitespace()
            .filter_map(|w| w.parse().ok())
            .collect();
        Ok::<[u64; 3], String>([v[0], v[1], v[2]])
    };
    let a = seeds(t.clone(), [1, 70, 2]).await?;
    t.check_eq(a[0], 5, "the seed the world was made with");
    t.check_eq(a[1], weft_sdk::noise::hash(5, 1, 70, 2), "a block's own number");
    t.check_eq(seeds(t.clone(), [1, 70, 2]).await?[1], a[1], "the same each time");
    let b = seeds(t.clone(), [2, 70, 2]).await?;
    t.check(b[1] != a[1], "another block has another");
    t.check_eq(b[2], a[2], "one cube, one number");
    t.check(seeds(t.clone(), [16, 70, 2]).await?[2] != a[2], "the next cube has another");
    t.check(seeds(t.clone(), [-1, 70, 2]).await?[2] != a[2], "also the other way, past zero");
    let said = t.cmd("/seed").await?;
    t.check(said.contains('5'), format!("/seed says it: {said}"));
    Ok(())
}

pub async fn dimensions(t: T) -> R {
    t.world(11).await?;
    type Look = Option<fn(&crate::runner::Shot) -> (f32, bool)>;
    let looks: [(&str, Look); 4] = [
        (
            "base:flatlands",
            Some(|s| {
                let v = s.share(0.0, 0.6, 1.0, 0.9, probe::grass);
                (v, v > 0.6)
            }),
        ),
        (
            "testmod:mars",
            Some(|s| {
                let v = s.share(0.0, 0.0, 1.0, 0.9, |c| c[0] as i32 > c[1] as i32 + 40 && c[0] as i32 > c[2] as i32 + 60);
                (v, v > 0.6)
            }),
        ),
        (
            "testmod:void",
            Some(|s| {
                let v = s.share(0.0, 0.0, 1.0, 0.25, probe::dark);
                (v, v > 0.9)
            }),
        ),
        // Generated terrain: reaching it is the check, the picture is for looking at.
        ("base:overworld", None),
    ];
    for (dim, look) in looks {
        let Some(id) = Dimension::lookup(dim) else {
            t.fail(format!("no dimension {dim}"));
            continue;
        };
        t.cmd(&format!("/dim {dim}")).await?;
        t.until(&format!("in {dim}"), Duration::from_secs(20), |t| t.state().dim == id)
            .await?;
        t.loaded().await?;
        t.idle().await?;
        t.settled().await?;
        t.c().set_look(30.0, -20.0);
        t.frames(3).await?;
        let shot = t.shot(dim.split(':').nth(1).unwrap_or(dim)).await?;
        if let Some(look) = look {
            let (v, ok) = look(&shot);
            t.probe(&format!("{dim} drawn as itself"), v, ok);
        }
        if dim == "testmod:void" {
            // testmod's island generator places a chest (`ISLAND_CHEST`) without its entity.
            t.check_eq(t.op("insert 4 64 0 coal 3").await, Ok("3".to_string()), "a chest placed by a generator has its block entity");
        }
    }

    let void = Dimension::lookup("core:void").ok_or("no core:void")?;
    t.cmd("/dim core:void").await?;
    t.until("in the void", Duration::from_secs(20), |t| t.state().dim == void)
        .await?;
    t.loaded().await?;
    let said = (0..4).any(|i| t.c().chat_line(i).contains("nothing here can be changed"));
    t.check(said, "chat says where the player is");
    let p = add(t.feet(), [0, -1, 0]);
    t.check(t.set(p, "stone").await.is_err(), "no block can be set in the void");
    t.check(t.block(p).await? != "base:stone", "the void stays empty");
    let y = t.state().pos.y;
    t.secs(1.0).await?;
    t.check((t.state().pos.y - y).abs() < 0.01, format!("no gravity in the void (y {y:.2} -> {:.2})", t.state().pos.y));

    t.cmd("/dim core:debug_blocks").await?;
    t.secs(0.5).await?;
    t.loaded().await?;
    let at = t.op("find 160 base:water").await?;
    let w: Vec<i32> = at
        .split_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect();
    let [x, y, z] = w[..] else { return Err(format!("no water among the debug blocks: {at}")) };
    t.secs(1.0).await?;
    t.check_eq(t.block([x + 1, y, z]).await?, "core:air".to_string(), "debug water does not flow");
    t.cmd(&format!("/setblock {} {y} {z} base:stone", x + 1))
        .await?;
    t.check_eq(t.block([x + 1, y, z]).await?, "core:air".to_string(), "debug blocks refuse /setblock");
    // Breaking is refused like placing, and nothing holds the grid up: the debug block beside
    // the water's.
    let (sx, sy, sz) = (x + 2, y, z);
    let was = t.block([sx, sy, sz]).await?;
    t.check(was != "core:air", format!("a debug block beside the water: {was}"));
    // Refused, not just undone: a refused break sends the block back to the player's client.
    let broke = t.op(&format!("break {sx} {sy} {sz}")).await?;
    t.check_eq(broke, "0".to_string(), "breaking a debug block is refused");
    t.check_eq(t.block([sx, sy, sz]).await?, was, "debug blocks refuse breaking");
    t.check_eq(t.block([sx, sy - 1, sz]).await?, "core:air".to_string(), "no floor under the debug blocks");
    t.cmd("/rule core:mob_spawning 1").await?;
    t.secs(4.5).await?;
    let mobs = t.op(&format!("near {sx} {sy} {sz} 64")).await?;
    t.cmd("/rule core:mob_spawning 0").await?;
    // Block entities (furnaces, chests) are there; mobs must not be.
    let spawned: Vec<&str> = mobs
        .split_whitespace()
        .filter(|e| {
            ["pig", "cow", "zombie", "spider"]
                .iter()
                .any(|m| e.ends_with(&format!("base:{m}")))
        })
        .collect();
    t.check(spawned.is_empty(), format!("nothing spawns among the debug blocks: {spawned:?}"));
    // Lava is drawn solid: blocks behind ice do not show through it.
    let at = t.op("find 200 base:lava").await?;
    let l: Vec<i32> = at
        .split_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect();
    if let [lx, ly, lz] = l[..] {
        t.cmd(&format!("/tp {} {} {}", lx as f64 + 0.5, ly + 2, lz as f64 + 4.5))
            .await?;
        t.c().set_look(0.0, -30.0);
        t.frames(20).await?;
        t.shot("lava").await?;
    }
    t.cmd("/tp -3 1 -3").await?;
    t.c().set_look(225.0, -25.0);
    t.frames(20).await?;
    let grid = t.shot("debug_blocks").await?;
    // Many different blocks in view: no color class covers most of the grid (it floats in the
    // sky, so only the band it fills counts).
    let biggest = [probe::grass, probe::dark, probe::light, probe::brown]
        .map(|f| grid.share(0.3, 0.22, 0.7, 0.36, f))
        .into_iter()
        .fold(0.0f32, f32::max);
    t.probe("debug blocks: largest share of one color class", biggest, biggest < 0.6);
    Ok(())
}

/// World generation from data, sampled with seed 5: birches in birch forests and oaks on the
/// plains, structures where the engine says they are, and on Mars the test mod's glow crystals
/// only in its crystal caves (an underground biome).
pub async fn worldgen(t: T) -> R {
    t.world(5).await?;
    t.op("mode spectator").await?;

    // Birch forests and plains of seed 5, found from the generator's columns: the middle of
    // a patch of the biome, above ground.
    let patch = |biome: &str, skip: usize| {
        (-40..40)
            .flat_map(|i| (-40..40).map(move |j| (i * 48, j * 48)))
            .filter(|&(x, z)| {
                [(0, 0), (24, 0), (-24, 0), (0, 24), (0, -24)]
                    .iter()
                    .all(|&(dx, dz)| name_of(ek::BIOME, OVERWORLD.column(5, x + dx, z + dz).biome) == biome)
            })
            .nth(skip)
            .map(|(x, z)| [x, OVERWORLD.column(5, x, z).height + 6, z])
    };
    let spots = [
        (patch("base:birch_forest", 0), true),
        (patch("base:birch_forest", 3), true),
        (patch("base:plains", 0), false),
        (patch("base:plains", 3), false),
    ];
    for (spot, forest) in spots {
        let [x, y, z] = spot.ok_or(format!("no {} patch", if forest { "birch forest" } else { "plains" }))?;
        t.cmd(&format!("/tp {x} {y} {z}")).await?;
        t.secs(0.5).await?;
        t.loaded().await?;
        let census = t
            .op("census 28 base:birch_log,base:oak_log,testmod:glow_log,testmod:glow_flower")
            .await?;
        let n: Vec<u32> = census
            .split_whitespace()
            .filter_map(|e| e.rsplit_once('=')?.1.parse().ok())
            .collect();
        t.note(format!("{x} {z}: {census}"));
        if forest {
            t.check(n[0] > 0, format!("birches in the birch forest at {x} {z}"));
        } else {
            t.check(n[1] > 0, format!("oaks on the plains at {x} {z}"));
        }
        t.check_eq(n[2] + n[3], 0, &format!("the test mod's glowing plants are not generated ({x} {z})"));
    }

    // The same template placed by code.
    let f = t.feet();
    t.check_eq(
        t.op(&format!("place testmod:obelisk {} {} {}", f[0] + 4, f[1] + 10, f[2]))
            .await?,
        "1".to_string(),
        "w.place puts a template",
    );
    t.check_eq(t.block([f[0] + 5, f[1] + 11, f[2] + 1]).await?, "testmod:glow_crystal".to_string(), "its glowing core");

    // Testmod puts base's forest on Mars; the engine scatters its trees there like at home.
    static MARS: Dimension = Dimension::named("testmod:mars");
    let seed = 5;
    let forest = |x: i32, z: i32| name_of(ek::BIOME, MARS.column(seed, x, z).biome) == "base:forest";
    let spot = (-60..60)
        .flat_map(|i| (-60..60).map(move |j| (i * 32, j * 32)))
        .find(|&(x, z)| {
            [(0, 0), (20, 0), (-20, 0), (0, 20), (0, -20)]
                .iter()
                .all(|&(dx, dz)| forest(x + dx, z + dz))
        });
    let (x, z) = spot.ok_or("no forest on Mars")?;
    let h = MARS.column(seed, x, z).height;
    t.cmd("/dim testmod:mars").await?;
    t.until("on Mars", Duration::from_secs(20), |t| t.state().dim == MARS.id())
        .await?;
    t.cmd(&format!("/tp {x} {} {z}", h + 3)).await?;
    t.secs(0.5).await?;
    t.loaded().await?;
    let census = t
        .op("census 20 base:oak_log,base:birch_log,base:grass")
        .await?;
    t.note(format!("Mars forest at {x} {z}: {census}"));
    let n: Vec<u32> = census
        .split_whitespace()
        .filter_map(|e| e.rsplit_once('=')?.1.parse().ok())
        .collect();
    t.check(n[0] + n[1] > 0, "trees grow in base's forest on Mars");
    t.check(n[2] > 0, "the forest brings its grass to Mars");
    t.check(t.op("structure testmod:obelisk 3000").await? != "none", "obelisks on Mars");
    let (mut caves, mut inside, mut outside) = (0, 0, 0);
    for (i, x) in [0, 200].into_iter().enumerate() {
        t.cmd(&format!("/tp {x} -40 {}", i * 150)).await?;
        t.secs(0.5).await?;
        t.loaded().await?;
        let survey = t.op("survey 12").await?;
        let (biomes, crystals) = survey
            .split_once(" | crystals ")
            .ok_or(format!("survey: {survey}"))?;
        caves += biomes
            .split_whitespace()
            .filter_map(|e| e.strip_prefix("testmod:crystal_caves="))
            .filter_map(|n| n.parse::<u32>().ok())
            .sum::<u32>();
        let n: Vec<u32> = crystals
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        inside += n.first().copied().unwrap_or(0);
        outside += n.get(1).copied().unwrap_or(0);
        t.note(format!("{x}: {survey}"));
    }
    t.check(caves > 0, "crystal caves were found under Mars");
    t.check(inside > 0, format!("glow crystals grow in crystal caves ({inside})"));
    t.check_eq(outside, 0, "glow crystals outside crystal caves");
    // Testmod's tin goes on as its basalt variant where a vein meets Mars' basalt.
    let ores = t
        .op("census 20 testmod:tin_ore,testmod:basalt_tin_ore")
        .await?;
    t.note(format!("Mars ores: {ores}"));
    let n: Vec<u32> = ores
        .split_whitespace()
        .filter_map(|e| e.rsplit_once('=')?.1.parse().ok())
        .collect();
    t.check(n[0] == 0 && n[1] > 0, "tin in basalt is basalt tin ore");
    Ok(())
}

/// A block placed while the whole view waits to be meshed again (as after a big crater or a
/// resource reload) shows at once: the cubes next to the player are meshed first, past the
/// far ones (it used to wait behind all of them).
pub async fn edit_while_loading(t: T) -> R {
    static VIEW: Config<i32> = Config::named("core:view_distance");
    let before = VIEW.get();
    VIEW.set(32);
    let out = edit_while_loading_at(&t).await;
    VIEW.set(before);
    out
}

async fn edit_while_loading_at(t: &T) -> R {
    // A stall of the machine can hit one try; behind the whole queue, every try is many seconds late.
    let first = edit_while_loading_once(t).await?;
    let ms = if first < 1000.0 { first } else { edit_while_loading_once(t).await? };
    t.check(ms < 1000.0, format!("shown after {ms:.0} ms"));
    Ok(())
}

async fn edit_while_loading_once(t: &T) -> Result<f64, String> {
    t.world(5).await?;
    t.idle().await?;
    t.op("mode spectator").await?;
    let low = t.state().pos.y;
    t.cmd("/tp ~ ~40 ~").await?;
    // Read too early, "here" is still the ground, and the block goes where the camera doesn't look.
    t.until("up in the air", Duration::from_secs(5), |t| t.state().pos.y > low + 30.0)
        .await?;
    t.c().set_look(0.0, 0.0);
    t.frames(5).await?;
    // Eye level, three blocks north.
    let f = t.feet();
    let p = add(f, [0, 1, -3]);
    let gold = |c: [u8; 3]| c[0] > 180 && c[1] > 130 && c[2] < 110;
    t.check(!gold(t.pixel(0.53, 0.53).await?), "sky in front before");
    t.c().reload_assets();
    let mut most = 0;
    t.until("everything to mesh again", Duration::from_secs(10), |t| {
        most = most.max(t.state().pending_meshes);
        most > 5000
    })
    .await
    .map_err(|e| format!("{e} (at most {most}, {} cubes)", t.state().cubes))?;
    let backlog = t.state().pending_meshes;
    let start = std::time::Instant::now();
    t.set(p, "gold_block").await?;
    let mut shown = false;
    while start.elapsed() < Duration::from_secs(5) && !shown {
        shown = gold(t.pixel(0.53, 0.53).await?);
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    t.note(format!("placed block shown after {ms:.0} ms with {backlog} meshes waiting"));
    t.check(shown, "the placed block shows");
    Ok(ms)
}

/// Terrain the GPU makes is the CPU's to the bit (integer math on both sides): every cube of a
/// fresh world is made again on the CPU and compared, in the overworld and on Mars, whose
/// presets use the rest of the expression steps.
pub async fn gpu_terrain(t: T) -> R {
    static CHECK: Config<i32> = Config::named("core:terrain_check");
    static MARS: Dimension = Dimension::named("testmod:mars");
    let before = CHECK.get();
    CHECK.set(1);
    let out = async {
        let start = t.state().gpu_cubes;
        t.world(5).await?;
        t.idle().await?;
        let overworld = t.state().gpu_cubes - start;
        t.cmd("/dim testmod:mars").await?;
        t.until("on Mars", Duration::from_secs(20), |t| t.state().dim == MARS.id())
            .await?;
        t.idle().await?;
        let s = t.state();
        let mars = s.gpu_cubes - start - overworld;
        t.note(format!("{overworld} cubes made on the GPU in the overworld, {mars} on Mars, {} differing", s.gpu_mismatches));
        t.check(overworld > 1000 && mars > 1000, format!("the GPU made the terrain ({overworld} and {mars} cubes)"));
        t.check_eq(s.gpu_mismatches, 0, "cubes the GPU made differently from the CPU");
        Ok(())
    }
    .await;
    CHECK.set(before);
    out
}
