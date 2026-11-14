//! Particles, text effects and conditions, shaders from mods, glow masks, models.

use std::time::Duration;

use weft_sdk::*;

use super::{add, probe};
use crate::runner::{R, T};

static SPARK: Particle = Particle::named("testmod:spark");

/// Huge counts stay cheap: moved by a compute shader, drawn in one instanced draw.
pub async fn particle_crowd(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let pos = [f[0] as f64 + 0.5, f[1] as f64 + 1.5, f[2] as f64 - 2.5];
    for _ in 0..10 {
        drop(
            t.c()
                .spawn_particles(&SPARK, pos)
                .count(10_000)
                .spread(4.0)
                .speed(1.0),
        );
    }
    t.frames(2).await?;
    let n = t.state().particles;
    let b = t.bench(0.6, "100k particles").await?;
    t.note(format!("{n} particles: {:.0} fps, frame {:.2} ms, gpu {:.2} ms", b[0], b[1], b[3]));
    t.check(n >= 99_000 && b[1] < 33.0, format!("{n} particles in {:.1} ms frames", b[1]));
    Ok(())
}

static DROP: Particle = Particle::named("testmod:drop");

/// A kind's rules run on the GPU: testmod's drops turn into red rings falling into water and are
/// gone falling on the ground.
pub async fn particle_rules(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    t.fill(add(f, [-3, -1, -7]), add(f, [3, -1, -3]), "water")
        .await?;
    t.c().set_look(0.0, -30.0);
    t.settled().await?;
    let rain = |at: [i32; 3]| {
        let pos = [at[0] as f64 + 0.5, at[1] as f64 + 3.0, at[2] as f64 + 0.5];
        drop(
            t.c()
                .spawn_particles(&DROP, pos)
                .count(300)
                .spread_box([2.5, 0.5, 1.5]),
        );
    };
    let middle = |s: &crate::runner::Shot| s.share(0.2, 0.3, 0.8, 0.9, probe::red);
    rain(add(f, [0, 0, -5]));
    t.secs(1.0).await?;
    let rings = middle(&t.shot("rings").await?);
    t.probe("drops falling into water turn into rings", rings, rings > 0.01);
    t.secs(3.0).await?;
    t.fill(add(f, [-3, -1, -7]), add(f, [3, -1, -3]), "stone")
        .await?;
    t.settled().await?;
    rain(add(f, [0, 0, -5]));
    t.secs(1.0).await?;
    let ground = t.shot("ground").await?;
    let red = middle(&ground);
    t.probe("drops on the ground leave nothing", red, red < 0.001);
    Ok(())
}

/// A stack's own `layers` show over one another, tinted, in the hotbar: the sprite of a tool
/// made of parts.
pub async fn layered_icon(t: T) -> R {
    t.flat().await?;
    t.c().select_slot(4);
    // The first hotbar slot, under the hearts.
    let hotbar = |s: &crate::runner::Shot| s.share(0.385, 0.95, 0.415, 0.995, probe::red);
    let before = hotbar(&t.shot("plain").await?);
    t.op("layered 0").await?;
    t.frames(6).await?;
    let after = hotbar(&t.shot("layered").await?);
    t.probe("red of the tinted layer in the hotbar", after - before, after - before > 0.04);
    Ok(())
}

/// A 16x16 texture of one color.
fn write_png(path: &str, rgb: [u8; 3]) -> Result<(), String> {
    let dir = std::path::Path::new(path).parent().ok_or("no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 16, 16);
    enc.set_color(png::ColorType::Rgba);
    let px: Vec<u8> = (0..256)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect();
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&px))
        .map_err(|e| e.to_string())
}

/// Reloading assets picks up a resource pack's texture and later changes to that file.
pub async fn texture_reload(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    t.fill(add(f, [-3, 0, -3]), add(f, [3, 3, -3]), "dirt")
        .await?;
    t.c().set_look(0.0, 0.0);
    t.settled().await?;
    let plain = t.grab().await?;
    let pack = format!("{}/resourcepacks/testkit_reload", t.c().run_dir());
    let file = format!("{pack}/base/textures/block/dirt.png");
    let reload = async |rgb: Option<[u8; 3]>| -> Result<Vec<u8>, String> {
        match rgb {
            Some(c) => write_png(&file, c)?,
            None => drop(std::fs::remove_dir_all(&pack)),
        }
        t.c().reload_assets();
        t.frames(5).await?;
        t.settled().await?;
        t.grab().await
    };
    weft_sdk::settings::RESOURCE_PACKS.set("testkit_reload");
    let red = reload(Some([200, 40, 40])).await?;
    let blue = reload(Some([40, 40, 200])).await?;
    weft_sdk::settings::RESOURCE_PACKS.set("");
    let back = reload(None).await?;
    t.check(T::differ(&plain, &red) > 0.2, "the pack's dirt shows after a reload");
    t.check(T::differ(&red, &blue) > 0.2, "a changed file shows after the next reload");
    t.check(T::differ(&plain, &back) < 0.01, "without the pack the dirt is back");
    Ok(())
}

/// A resource pack's translations change only the keys it has; the rest of the same mod's file
/// stays.
pub async fn pack_lang(t: T) -> R {
    let pack = format!("{}/resourcepacks/testkit_lang", t.c().run_dir());
    let dir = format!("{pack}/core/lang");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(format!("{dir}/en_us.lang"), "menu.done = Pack Done\n").map_err(|e| e.to_string())?;
    weft_sdk::settings::RESOURCE_PACKS.set("testkit_lang");
    t.c().reload_assets();
    t.frames(2).await?;
    t.c().menu_open(&screens::SETTINGS);
    t.screen("core:settings").await?;
    t.frames(2).await?;
    t.laid_out(&["text:Pack Done", "text:View distance", "text:Language"]);
    t.c().menu_close();
    weft_sdk::settings::RESOURCE_PACKS.set("");
    drop(std::fs::remove_dir_all(&pack));
    t.c().reload_assets();
    t.frames(2).await?;
    t.c().menu_open(&screens::SETTINGS);
    t.screen("core:settings").await?;
    t.frames(2).await?;
    t.check(t.c().ui_rect("text:Pack Done").is_none(), "without the pack the key is the mod's again");
    t.laid_out(&["text:Done"]);
    t.c().menu_close();
    Ok(())
}

/// The test mod's looks at night: a glow crystal's veins glow without lighting the floor next to
/// it, a wisp floats nearby, and the daydream screen effect changes the picture.
pub async fn shaders_and_glow(t: T) -> R {
    t.world(9).await?;
    t.idle().await?;
    t.op("time 18000").await?;
    let f = t.feet();
    // A level stage of its own, so terrain changes do not move the picture.
    t.fill(add(f, [-4, -1, -7]), add(f, [4, -1, 3]), "stone")
        .await?;
    t.fill(add(f, [-4, 0, -7]), add(f, [4, 5, 3]), "air")
        .await?;
    let crystal = add(f, [0, 1, -4]);
    t.fill(add(f, [-1, 0, -4]), add(f, [1, 2, -4]), "testmod:glow_crystal")
        .await?;
    // A block shader (`shaders/block/stripes.wgsl`): magenta bands, lit or not.
    let striped = add(f, [-3, 1, -4]);
    t.set(striped, "testmod:reinforced_stone").await?;
    // Off to the side and back a little: it drifts, so it stays small in the picture.
    t.op(&format!("spawn testmod:wisp {} {} {}", f[0] + 3, f[1] + 1, f[2] - 5))
        .await?;
    t.check(t.count_near("wisp", 8.0).await? == 1, "a wisp floats nearby");
    t.op("put me 0 testmod:rune 1").await?;
    t.c().select_slot(0);
    t.c().set_look(0.0, -5.0);
    t.settled().await?;
    t.secs(1.0).await?;
    let night = t.shot("night_glow").await?;
    let veins = match t.block_on_screen(crystal, [0.0, 0.0, 0.51]) {
        Some((x, y)) => night.share(x - 0.12, y - 0.15, x + 0.12, y + 0.15, |c| probe::cyan(c) && probe::lum(c) > 100.0),
        None => 0.0,
    };
    t.probe("the crystal's veins glow", veins, veins > 0.03);
    let floor = match t.on_screen([f[0] as f64 + 0.5, f[1] as f64, f[2] as f64 - 2.5]) {
        Some((x, y)) => crate::runner::lum(night.avg(x - 0.02, y - 0.02, x + 0.02, y + 0.02)),
        None => 255.0,
    };
    t.probe("the floor in front of it stays dark (glow is not light)", floor, floor < 60.0);
    let bands = match t.block_on_screen(striped, [0.0, 0.0, 0.51]) {
        Some((x, y)) => night.share(x - 0.03, y - 0.05, x + 0.03, y + 0.05, |c| c[0] > 100 && c[2] > 100 && c[1] < 60),
        None => 0.0,
    };
    t.probe("the block shader's magenta bands", bands, bands > 0.2 && bands < 0.9);
    let before = t.grab().await?;
    t.check(t.c().post_effect(&Shader::named("testmod:dream"), 1.0), "the daydream effect exists");
    t.check(
        !t.c()
            .post_effect(&Shader::named("testmod:nonexistent"), 1.0),
        "an unknown effect is refused",
    );
    t.frames(3).await?;
    t.shot("dream").await?;
    let after = t.grab().await?;
    let d = T::differ(&before, &after) as f32;
    t.probe("the daydream effect changes the picture", d, d > 0.03);
    t.c().post_effect(&Shader::named("testmod:dream"), 0.0);
    Ok(())
}

/// Models: the orb (built in code, a sphere mesh and a child bone) turns by its file's idle
/// animation; the idol (a Blockbench file with bones and mesh wings) bows when used, a server
/// triggered animation that holds its last frame.
pub async fn models(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    t.op(&format!("spawn testmod:orb {} {} {}", f[0], f[1] + 1, f[2] - 3))
        .await?;
    let idol = t
        .op(&format!("spawn testmod:idol {} {} {}", f[0], f[1], f[2] + 3))
        .await?;
    t.check(t.count_near("orb", 8.0).await? == 1 && t.count_near("idol", 8.0).await? == 1, "an orb and an idol stand nearby");
    t.c().set_look(0.0, 0.0);
    t.settled().await?;
    t.secs(0.5).await?;
    let a = t.grab().await?;
    t.secs(0.5).await?;
    let b = t.grab().await?;
    t.check(T::differ(&a, &b) > 0.001, "the orb's ring turns by its keyframes");
    t.look_at(add(f, [0, 0, 3]));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(1.5).await?;
    let a = t.grab().await?;
    t.secs(0.3).await?;
    let b = t.grab().await?;
    // Below the horizon: clouds drift up there.
    let ground = |f: &[u8]| f[f.len() / 4 / 3 * 4..].to_vec();
    t.check(T::differ(&ground(&a), &ground(&b)) == 0.0, "the bow holds its last frame");
    t.shot("idol_bow").await?;
    // A mod has it play one action as another for a while.
    t.op(&format!("action {idol}")).await?;
    t.secs(0.2).await?;
    let plain = t.grab().await?;
    t.op(&format!("swap {idol} idle bow")).await?;
    t.secs(1.0).await?;
    let swapped = t.grab().await?;
    t.check(T::differ(&ground(&plain), &ground(&swapped)) > 0.0005, "idle plays as the bow");
    t.op(&format!("swap {idol} idle")).await?;
    // The test mod gives the player a wave, seen from the first person like from anywhere.
    t.c().set_look(90.0, 0.0);
    t.frames(3).await?;
    let a = t.grab().await?;
    t.op("action me wave").await?;
    t.secs(0.5).await?;
    let b = t.grab().await?;
    t.check(T::differ(&a, &b) > 0.002, "the first person hand waves");
    Ok(())
}

/// Tooltip parts behind `{on_key}` and `{if}` appear while their condition holds: a hurt player
/// sees the rune's red line, sneaking swaps "more" for its long text.
pub async fn rich_text(t: T) -> R {
    t.flat().await?;
    t.op("put me 0 testmod:rune 1").await?;
    // A fall hurts the player, which the test mod's `{if:testmod:hurt}` shows.
    let s = t.state();
    t.c().teleport([s.pos.x, s.pos.y + 6.0, s.pos.z]);
    t.until("hurt by the fall", Duration::from_secs(5), |t| t.state().health < 20.0)
        .await?;
    t.press("core:inventory");
    t.screen("core:menu").await?;
    let Some(Rect { x, y, w, h }) = t.c().ui_rect("slot:36") else { return Err("no hotbar slot in the inventory".into()) };
    t.c().input_cursor(x + w / 2.0, y + h / 2.0);
    t.frames(4).await?;
    let rune = t.shot("rune").await?;
    let wound = |c: [u8; 3]| c[0] > 200 && (100..170).contains(&c[1]) && (100..170).contains(&c[2]);
    // The tooltip opens beside the cursor.
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let v = rune.share(cx, cy - 0.15, cx + 0.3, cy + 0.15, wound);
    t.probe("the {if:testmod:hurt} line (light red)", v, v > 0.003);
    t.hold("core:sneak", 2.0);
    t.frames(4).await?;
    let sneak = t.shot("rune_sneak").await?;
    let d = T::differ(&rune.px, &sneak.px) as f32;
    t.probe("sneaking changes the tooltip ({on_key})", d, d > 0.002);
    t.c().input_release_all();
    t.c().menu_close();
    t.no_screen().await
}

/// The sky: the sun at noon and the moon at midnight stand overhead, stars show only at night, a
/// sunset warms the horizon on its side, and from inside water everything is its fog.
pub async fn sky(t: T) -> R {
    t.world(5).await?;
    t.op("mode creative").await?;
    t.c().set_look(0.0, 89.0);
    let bright = |c: probe::C| c.iter().all(|&v| v > 215);
    t.op("time 6000").await?;
    t.secs(0.6).await?;
    let noon = t.shot("noon").await?;
    t.probe("the sun overhead", noon.share(0.45, 0.40, 0.55, 0.57, bright), noon.share(0.45, 0.40, 0.55, 0.57, bright) > 0.5);
    // A star is a speck: bright with sky a little way off on every side, which a cloud isn't.
    let specks = |shot: &crate::runner::Shot| {
        let (sx, sy) = (1.0 / 1600.0, 1.0 / 900.0);
        let mut n = 0;
        for y in (10..225).map(|y| y as f32 * sy * 1.0) {
            for x in (10..1590).map(|x| x as f32 * sx) {
                let lit = |x: f32, y: f32| shot.at(x, y).iter().all(|&v| v > 215);
                if lit(x, y) && !lit(x - 6.0 * sx, y) && !lit(x + 6.0 * sx, y) && !lit(x, y - 6.0 * sy) && !lit(x, y + 6.0 * sy) {
                    n += 1;
                }
            }
        }
        n
    };
    t.check_eq(specks(&noon), 0, "no stars by day");
    t.op("time 18000").await?;
    t.secs(0.6).await?;
    let night = t.shot("midnight").await?;
    let moon = night.share(0.45, 0.42, 0.55, 0.56, |c| c.iter().all(|&v| v > 150));
    t.probe("the moon overhead", moon, moon > 0.4);
    let stars = night.share(0.0, 0.0, 1.0, 0.3, |c| c.iter().all(|&v| v > 60));
    t.probe("stars at night", stars, stars > 0.0005);
    // Looking at the sunset and away from it.
    t.op("time 12400").await?;
    t.c().set_look(90.0, 8.0);
    t.secs(0.6).await?;
    let west = t.shot("sunset").await?;
    let warm = |c: probe::C| c[0] > 170 && c[0] as i32 > c[2] as i32 + 50;
    t.c().set_look(-90.0, 8.0);
    t.secs(0.6).await?;
    let east = t.shot("sunset_back").await?;
    let (w, e) = (west.share(0.0, 0.3, 1.0, 0.5, warm), east.share(0.0, 0.3, 1.0, 0.5, warm));
    t.note(format!("warm sky: toward the sunset {w:.3}, away {e:.3}"));
    t.check(w > e + 0.02, format!("a sunset on its side: {w:.3}, away {e:.3}"));
    Ok(())
}

/// Clouds reach far past any grid around the camera, cover what is under them, stay behind what is
/// nearer, and the setting lifts them.
pub async fn clouds(t: T) -> R {
    static LIFT: Config<i32> = Config::named("core:cloud_height");
    t.world(5).await?;
    t.op("mode spectator").await?;
    t.op("time 6000").await?;
    t.cmd("/weather clear").await?;
    LIFT.set(0);
    let white = |c: probe::C| c.iter().all(|&v| v > 205);
    let s = t.state();
    // Near the horizon from well below them: what is there is thousands of blocks away.
    t.c().teleport([s.pos.x, 150.0, s.pos.z]);
    t.c().set_look(0.0, 2.0);
    t.secs(1.0).await?;
    let (x0, y0, x1, y1) = (0.0, 0.47, 1.0, 0.53);
    let far = t.shot("far").await?.share(x0, y0, x1, y1, white);
    let world_fog = t.c().environment_values().fog_end;
    settings::CLOUD_DISTANCE.set(64);
    t.frames(3).await?;
    let limited = t
        .shot("distance_limited")
        .await?
        .share(x0, y0, x1, y1, white);
    t.probe("cloud distance removes distant clouds", limited, limited < far - 0.02);
    t.check_eq(t.c().environment_values().fog_end, world_fog, "cloud distance leaves world fog independent");
    t.c().environment(
        &Environment::new("test_cloud_range")
            .priority(100)
            .cloud_fog(3072.0, 6144.0),
    );
    t.frames(3).await?;
    let overridden = t
        .shot("distance_override")
        .await?
        .share(x0, y0, x1, y1, white);
    t.probe("environment source overrides the cloud distance setting", overridden, overridden > limited + 0.02);
    t.c().clear_environment("test_cloud_range");
    t.frames(3).await?;
    t.check_eq(t.c().environment_values().cloud_fog_end, 64.0, "removing the source restores the slider value");
    settings::CLOUD_DISTANCE.set(6144);
    // Lifted out of reach at that angle.
    LIFT.set(512);
    t.secs(0.3).await?;
    let lifted = t.shot("far_lifted").await?.share(x0, y0, x1, y1, white);
    t.note(format!("white near the horizon: {far:.3}, lifted {lifted:.3}"));
    t.check(far > lifted + 0.02, format!("clouds far away: {far:.3}, lifted out of reach {lifted:.3}"));
    // From above they cover the ground; lifted over the camera they don't.
    LIFT.set(0);
    t.c().teleport([s.pos.x, 330.0, s.pos.z]);
    t.c().set_look(0.0, -60.0);
    t.secs(1.0).await?;
    let above = t.shot("above").await?.share(0.0, 0.3, 1.0, 1.0, white);
    settings::CLOUD_DISTANCE.set(64);
    t.frames(3).await?;
    let culled = t
        .shot("layer_culled")
        .await?
        .share(0.0, 0.3, 1.0, 1.0, white);
    t.probe("a layer beyond the cloud fog sphere is culled", culled, culled < above - 0.02);
    settings::CLOUD_DISTANCE.set(6144);
    LIFT.set(512);
    t.secs(0.3).await?;
    let lifted = t
        .shot("above_lifted")
        .await?
        .share(0.0, 0.3, 1.0, 1.0, white);
    t.note(format!("white from above: {above:.3}, lifted {lifted:.3}"));
    t.check(above > lifted + 0.02, format!("over the ground: {above:.3}, lifted away {lifted:.3}"));
    // A wall nearer than they are hides them.
    LIFT.set(0);
    let at = [s.pos.x.floor() as i32, 150, s.pos.z.floor() as i32];
    t.fill(add(at, [-30, 0, -12]), add(at, [30, 70, -12]), "stone")
        .await?;
    t.c().teleport([s.pos.x, 150.0, s.pos.z]);
    t.c().set_look(0.0, 35.0);
    t.secs(1.0).await?;
    let wall = t.shot("wall").await?.share(0.2, 0.3, 0.8, 0.7, white);
    t.check(wall < 0.005, format!("clouds through a wall: {wall:.3}"));
    // Weather supplies cloud appearance to the same environment as external sources.
    t.c().teleport([s.pos.x + 80.0, 150.0, s.pos.z]);
    t.c().set_look(0.0, 75.0);
    t.secs(0.4).await?;
    let sky_lum = |shot: &crate::runner::Shot| crate::runner::lum(shot.avg(0.1, 0.05, 0.9, 0.8));
    let clear = sky_lum(&t.shot("clear_clouds").await?);
    t.cmd("/weather rain").await?;
    t.until("rain cloud tint", Duration::from_secs(8), |t| t.c().environment_values().cloud_gray > 0.9)
        .await?;
    let rain = sky_lum(&t.shot("rain_clouds").await?);
    t.cmd("/weather thunder").await?;
    t.until("thunder cloud tint", Duration::from_secs(8), |t| t.c().environment_values().cloud_dark > 0.9)
        .await?;
    let thunder = sky_lum(&t.shot("thunder_clouds").await?);
    t.probe("rain grays clouds", clear - rain, clear > rain + 15.0);
    t.probe("thunder darkens clouds", rain - thunder, rain > thunder + 15.0);
    t.c().environment(
        &Environment::new("test_clouds")
            .priority(100)
            .cloud_color(0xE03020)
            .fade(0.3, 0.3),
    );
    t.secs(0.4).await?;
    let red = t.shot("override_clouds").await?;
    let red_clouds = red.share(0.05, 0.05, 0.95, 0.8, probe::red);
    t.probe("cloud tint can replace thunder independently", red_clouds, red_clouds > 0.05);
    let values = t.c().environment_values();
    t.check(
        values.cloud_gray == 0.0 && values.cloud_dark == 0.0 && values.fog_gray > 0.1,
        "cloud override leaves other weather contributions intact",
    );
    t.c().clear_environment("test_clouds");
    t.secs(0.4).await?;
    let restored = sky_lum(&t.shot("restored_clouds").await?);
    t.probe("removing the source restores weather tint", (restored - thunder).abs(), (restored - thunder).abs() < 10.0);
    // Both cloud sliders use the normal video settings page.
    t.c().menu_open(&Menu::named("core:video"));
    t.screen("core:video").await?;
    t.frames(3).await?;
    t.laid_out(&["text:Clouds elevation", "text:Cloud render distance"]);
    t.shot("settings").await?;
    t.c().menu_close();
    t.no_screen().await?;
    Ok(())
}

/// Light adds up and bends toward full: by day a lamp's color still shows on what is around it,
/// over the sun's.
pub async fn light_by_day(t: T) -> R {
    t.flat().await?;
    t.op("time 6000").await?;
    t.cmd("/weather clear").await?;
    let f = t.feet();
    t.op("mode spectator").await?;
    t.fill(add(f, [-9, 0, -8]), add(f, [9, 0, -2]), "base:snow")
        .await?;
    t.c()
        .teleport([f[0] as f64 + 0.5, f[1] as f64 + 7.0, f[2] as f64 - 2.0]);
    t.c().set_look(0.0, -60.0);
    t.secs(0.4).await?;
    let spot = t
        .on_screen([f[0] as f64 + 0.5, f[1] as f64 + 1.0, f[2] as f64 - 5.5])
        .ok_or("not in view")?;
    let at = |s: &crate::runner::Shot| s.avg(spot.0 - 0.01, spot.1 - 0.01, spot.0 + 0.01, spot.1 + 0.01);
    let plain = at(&t.shot("plain").await?);
    t.set(add(f, [0, 1, -5]), "base:blue_lamp").await?;
    t.frames(4).await?;
    // Beside the lamp, on the snow.
    let spot2 = t
        .on_screen([f[0] as f64 + 1.5, f[1] as f64 + 1.0, f[2] as f64 - 4.5])
        .ok_or("not in view")?;
    let beside = |s: &crate::runner::Shot| s.avg(spot2.0 - 0.01, spot2.1 - 0.01, spot2.0 + 0.01, spot2.1 + 0.01);
    t.secs(0.4).await?;
    let lit = beside(&t.shot("lamp").await?);
    t.note(format!("snow {plain:?}, beside a blue lamp {lit:?}"));
    t.probe("how much bluer the snow is than red, by a blue lamp at noon", lit[2] - lit[0], lit[2] - lit[0] > 12.0);
    Ok(())
}

/// Any method on any block, here plants and box models connecting up and down, and a tile that
/// is a model file with its picture an unwrap, on a plain cube.
pub async fn ctm_any_model(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    t.fill(add(f, [-9, 0, -7]), add(f, [9, 6, -7]), "snow")
        .await?;
    t.fill(add(f, [-9, 0, 7]), add(f, [9, 6, 7]), "snow")
        .await?;
    for (x, block) in [(-4, "testkit:varied_plant"), (1, "testkit:varied_box")] {
        for y in 0..3 {
            t.set(add(f, [x, y, -5]), block).await?;
        }
        t.set(add(f, [x + 2, 0, -5]), block).await?;
    }
    // A pack's model of a tinted block, its own textures on it: only what the block tints is.
    let grass = add(f, [-1, 0, -5]);
    t.set(grass, "base:grass").await?;
    let row: Vec<i32> = (-5..=5).collect();
    for &x in &row {
        t.set(add(f, [x, 0, 4]), "testkit:varied_cube").await?;
    }
    let pack = format!("{}/resourcepacks/testkit_ctm", t.c().run_dir());
    let write = |file: &str, text: &str| -> R {
        let path = format!("{pack}/testkit/{file}");
        std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())
    };
    let whole = |f: &str| format!(r#"{{"elements":[{{"from":[0,0,0],"to":[16,16,16],"faces":{{{f}}}}}]}}"#);
    let base_model = format!("{pack}/base/models/block/grass.json");
    std::fs::create_dir_all(std::path::Path::new(&base_model).parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&base_model, whole(r#""north":{},"south":{},"east":{},"west":{},"up":{},"down":{}"#)).map_err(|e| e.to_string())?;
    write("textures/block/varied.ctm", "method = vertical\ntiles = colors/red colors/green colors/blue colors/yellow\n")?;
    write("textures/block/varied_cube.ctm", "method = random\ntiles = shapes/thin <default>\nhides_neighbors = false\n")?;
    let face = r##"{"texture":"#unwrap","uv":[0.25,0.25,1.25,1.25]}"##;
    write(
        "models/block/shapes/thin.json",
        &format!(
            r##"{{"textures":{{"unwrap":"block/unwrap"}},"elements":[{{"from":[6,0,6],"to":[10,16,10],"faces":{{"north":{face},"south":{face},"east":{face},"west":{face},"up":{face},"down":{face}}}}}]}}"##
        ),
    )?;
    weft_sdk::settings::RESOURCE_PACKS.set("testkit_ctm");
    t.c().reload_assets();
    t.frames(5).await?;
    let out = async {
        t.look_at(add(f, [0, 1, -5]));
        t.settled().await?;
        let s = t.shot("connected").await?;
        let at = |s: &crate::runner::Shot, p: [f64; 3], test: fn(probe::C) -> bool| {
            t.on_screen(p)
                .map_or(0.0, |(x, y)| s.share(x - 0.004, y - 0.004, x + 0.004, y + 0.004, test))
        };
        let red: fn(probe::C) -> bool = |c| c[0] > 120 && c[1] < 70 && c[2] < 70;
        let green: fn(probe::C) -> bool = |c| c[1] > 120 && c[0] < 70 && c[2] < 70;
        let blue: fn(probe::C) -> bool = |c| c[2] > 120 && c[0] < 70 && c[1] < 90;
        let yellow: fn(probe::C) -> bool = |c| c[0] > 120 && c[1] > 110 && c[2] < 70;
        for (x, depth, what) in [(-4, 0.5, "plant"), (1, 0.13, "box model")] {
            let o = |x: i32, y: i32| [(f[0] + x) as f64 + 0.5, (f[1] + y) as f64 + 0.5, (f[2] - 5) as f64 + 1.0 - depth];
            t.check(at(&s, o(x, 0), red) > 0.5, format!("{what}: red at the bottom"));
            t.check(at(&s, o(x, 1), green) > 0.5, format!("{what}: green in the middle"));
            t.check(at(&s, o(x, 2), blue) > 0.5, format!("{what}: blue on top"));
            t.check(at(&s, o(x + 2, 0), yellow) > 0.5, format!("{what}: yellow alone"));
        }

        let side = |h: f64| [grass[0] as f64 + 0.5, grass[1] as f64 + h, grass[2] as f64 + 1.0];
        let brown: fn(probe::C) -> bool = |c| c[0] > c[1] && c[1] > c[2];
        let greenish: fn(probe::C) -> bool = |c| c[1] > c[0] && c[1] > c[2];
        t.check(at(&s, side(0.3), brown) > 0.5, "a model's dirt side isn't tinted");
        t.check(at(&s, side(0.97), greenish) > 0.3, "and it has its tinted overlay");

        t.look_at(add(f, [0, 0, 4]));
        t.settled().await?;
        let s = t.shot("models").await?;
        let (mut thin, mut plain, mut stray, mut ground, mut dark) = (0, 0, 0, 0, 0);
        let orange: fn(probe::C) -> bool = |c| c[0] > 120 && c[1] > 50 && c[1] < 130 && c[2] < 60;
        let grass: fn(probe::C) -> bool = |c| c[1] > c[0] && c[1] > c[2] && c[1] > 40;
        for &x in &row {
            let middle = [(f[0] + x) as f64 + 0.5, f[1] as f64 + 0.5, (f[2] + 4) as f64 + 0.375];
            if at(&s, middle, blue) > 0.5 {
                thin += 1;
                // All of a face is the island, right up to its corners (further out a cube may
                // stand in front of it).
                for (a, b) in [(0.41, 0.08), (0.59, 0.08), (0.41, 0.92), (0.59, 0.92)]
                    .into_iter()
                    .filter(|_| x.abs() <= 3)
                {
                    if at(&s, [(f[0] + x) as f64 + a, f[1] as f64 + b, (f[2] + 4) as f64 + 0.375], blue) < 0.5 {
                        stray += 1;
                    }
                }
                // A side is lit, though the block's own cell and maybe the next one have no light.
                if x != 0 {
                    let side = if x < 0 { 0.625 } else { 0.375 };
                    if t.on_screen([(f[0] + x) as f64 + side, f[1] as f64 + 0.5, (f[2] + 4) as f64 + 0.5])
                        .is_some_and(|(sx, sy)| crate::runner::lum(s.avg(sx - 0.002, sy - 0.004, sx + 0.002, sy + 0.004)) < 25.0)
                    {
                        dark += 1;
                    }
                }
                // Beside the column, the floor its cube no longer hides.
                let side = if x <= 0 { 0.12 } else { 0.88 };
                if at(&s, [(f[0] + x) as f64 + side, f[1] as f64 + 0.01, (f[2] + 4) as f64 + 0.5], grass) > 0.5 {
                    ground += 1;
                }
            } else if at(&s, middle, orange) > 0.5 {
                plain += 1;
            } else if at(&s, middle, red) > 0.1 {
                stray += 1;
            }
        }
        t.note(format!("thin {thin}, plain {plain}, off the island {stray}, floor seen by {ground}, dark sides {dark}"));
        t.check(dark == 0, format!("a model's sides are lit: {dark} are dark"));
        t.check(thin > 0 && plain > 0, format!("the rule picks the model and the plain cube: {thin} and {plain}"));
        t.check(stray == 0, format!("the model's texels are the island on its unwrap: {stray} show around it"));
        t.check(ground > 0, format!("hides_neighbors = false: the floor shows beside a model: {ground} of {thin}"));
        Ok(())
    }
    .await;
    weft_sdk::settings::RESOURCE_PACKS.set("");
    let _ = std::fs::remove_dir_all(&pack);
    t.c().reload_assets();
    t.frames(2).await?;
    out
}

/// Sources share independent parameters, stable priority and lifetime rules across local code,
/// synchronized entity data and content presets. Camera and body sample different positions.
pub async fn environment_sources(t: T) -> R {
    use weft_sdk::environment::{blend, observer};
    t.flat().await?;
    t.op("mode spectator").await?;
    t.cmd("/weather clear").await?;
    t.c().set_look(0.0, -25.0);
    t.frames(3).await?;
    let initial = t.c().environment_values();
    let mut base = Environment::new("test_base")
        .priority(10)
        .fog(0x203040, 2.0, 40.0)
        .acoustics(800.0, 0.2, 0.8);
    base.values.sunlight = 0.5;
    t.check(t.c().environment(&base), "local source accepted");
    let mut addition = Environment::new("test_add").priority(20).blend(blend::ADD);
    addition.values.fog_end = 10.0;
    t.c().environment(&addition);
    let mut mul = Environment::new("test_mul")
        .priority(30)
        .blend(blend::MULTIPLY);
    mul.values.sunlight = 0.5;
    t.c().environment(&mul);
    t.frames(3).await?;
    let values = t.c().environment_values();
    t.check_eq(
        (values.fog_start, values.fog_end, values.sunlight, values.lowpass),
        (2.0, 50.0, 0.25, 800.0),
        "ordered operations leave unrelated parameters alone",
    );
    // Updating a named source replaces it, even when insertion order differs from priority.
    addition.values.fog_end = 20.0;
    t.c().environment(&addition);
    t.frames(3).await?;
    t.check_eq(t.c().environment_values().fog_end, 60.0, "one contribution per named source");
    for name in ["test_base", "test_add", "test_mul"] {
        t.c().clear_environment(name);
    }
    t.frames(3).await?;
    t.check_eq(t.c().environment_values().lowpass, initial.lowpass, "clearing restores the underlying environment");

    let fade = Environment::new("test_fade")
        .priority(50)
        .fade(1.0, 1.0)
        .sunlight(0.0);
    t.c().environment(&fade);
    t.secs(0.25).await?;
    let partial = t.c().environment_values().sunlight;
    t.check(partial > 0.1 && partial < 0.95, format!("fade begins smoothly: {partial}"));
    // A frame hook may refresh the source every frame without restarting its transition.
    for _ in 0..80 {
        t.c().environment(&fade);
        t.frames(1).await?;
    }
    t.secs(1.1).await?;
    t.check_eq(t.c().environment_values().sunlight, 0.0, "a refreshed source reaches its target");
    t.c().clear_environment("test_fade");
    t.secs(0.25).await?;
    let partial = t.c().environment_values().sunlight;
    t.check(partial > 0.05 && partial < 0.9, format!("removed source fades out: {partial}"));
    t.secs(1.0).await?;

    let f = t.feet();
    let eye = t.state().camera_pos;
    let center = [eye.x, eye.y, eye.z];
    let cell = [eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32];
    t.set(cell, "stone").await?;
    let camera = Environment::new("test_point")
        .priority(50)
        .around(center, 1.0, 2.0)
        .when("block(base:stone)")
        .acoustics(600.0, 0.0, 1.0);
    t.c().environment(&camera);
    t.frames(3).await?;
    t.check_eq(t.c().environment_values().lowpass, 600.0, "condition reads the sampled block");
    t.set(cell, "air").await?;
    t.frames(3).await?;
    t.check_eq(t.c().environment_values().lowpass, initial.lowpass, "condition failure removes its contribution");
    let camera = camera.when("");
    t.c().environment(&camera);
    t.op("camera testmod:orbit").await?;
    t.secs(0.8).await?;
    t.check_eq(t.c().environment_values().lowpass, initial.lowpass, "detached camera leaves the zone");
    let body = camera.clone().observer(observer::BODY);
    t.c().environment(&body);
    t.frames(3).await?;
    t.check_eq(t.c().environment_values().lowpass, 600.0, "body source stays at the player's eyes");
    t.c().clear_environment("test_point");
    t.op("camera core:first_person").await?;
    t.set(cell, "air").await?;
    t.secs(0.8).await?;

    let orb = t
        .op(&format!("spawn testmod:orb {} {} {}", f[0], f[1], f[2] - 2))
        .await?;
    t.until("entity-kind zone reaches observer", Duration::from_secs(5), |t| (t.c().environment_values().lowpass - 4000.0).abs() < 1.0)
        .await?;
    t.op(&format!("remove {orb}")).await?;
    t.until("despawn removes entity-kind source", Duration::from_secs(5), |t| t.c().environment_values().lowpass == initial.lowpass)
        .await?;

    let plain = t.shot("sunlight").await?;
    let screen = Environment::new("test_screen")
        .priority(5)
        .overlay("base:block/stone", 0.4);
    t.c().environment(&screen);
    t.frames(3).await?;
    let textured = t.grab().await?;
    t.check(T::differ(&plain.px, &textured) > 0.03, "overlay contributes through a source, resolving a tiled block texture");
    t.c().environment(
        &Environment::new("test_shader")
            .priority(6)
            .post(&Shader::named("testmod:dream"), 1.0),
    );
    t.frames(3).await?;
    let filtered = t.grab().await?;
    t.check(T::differ(&textured, &filtered) > 0.01, "post shader contributes independently of the overlay");
    t.c().environment(
        &Environment::new("test_clear_screen")
            .priority(100)
            .replace_post()
            .replace_overlays(),
    );
    t.frames(3).await?;
    let restored = t.grab().await?;
    t.check(T::differ(&plain.px, &restored) < 0.01, "higher empty replacement suppresses all preceding screen effects");
    t.c().clear_environment("test_clear_screen");
    t.c().clear_environment("test_screen");
    t.c().clear_environment("test_shader");
    t.frames(3).await?;
    t.cmd("/eclipse on").await?;
    t.until("server source reaches client", Duration::from_secs(5), |t| t.c().environment_values().sunlight == 0.0)
        .await?;
    let dark = t.shot("eclipse").await?;
    t.check_eq(t.c().environment_values().fog_end, 48.0, "server source controls fog independently");
    t.check_eq(t.c().environment_values().cloud_fog_end, 48.0, "server source controls cloud fog independently");
    t.c().environment(
        &Environment::new("test_cloud_range")
            .priority(200)
            .cloud_fog(initial.cloud_fog_start, initial.cloud_fog_end),
    );
    t.frames(3).await?;
    let cloud_lighting = t.shot("eclipse_cloud_light").await?;
    let daylight = crate::runner::lum(plain.avg(0.2, 0.6, 0.8, 0.85));
    let eclipse = crate::runner::lum(dark.avg(0.2, 0.6, 0.8, 0.85));
    let white_cloud = |c: probe::C| c[0] > 150 && c[0].abs_diff(c[1]) < 15 && c[0].abs_diff(c[2]) < 15;
    let bright_clouds = plain.share(0.05, 0.02, 0.95, 0.2, white_cloud);
    let eclipsed_clouds = cloud_lighting.share(0.05, 0.02, 0.95, 0.2, white_cloud);
    t.probe("daylight illuminates clouds", bright_clouds, bright_clouds > 0.01);
    t.probe("clouds lose sunlight together with the world", eclipsed_clouds, eclipsed_clouds < 0.005);
    t.c().clear_environment("test_cloud_range");
    t.frames(3).await?;
    t.probe("solar illumination can be extinguished at noon", daylight - eclipse, daylight - eclipse > 30.0);
    t.set(add(f, [0, 0, -3]), "base:torch").await?;
    t.secs(0.5).await?;
    let lamp = t.shot("eclipse_lamp").await?;
    t.check(T::differ(&dark.px, &lamp.px) > 0.01, "block lighting remains visible without sunlight");
    t.set(add(f, [0, 0, -3]), "air").await?;
    t.secs(0.5).await?;
    let mut unlit = Environment::new("test_unlit").priority(200).sunlight(0.0);
    unlit.values.ambient = 0.0;
    t.c().environment(&unlit);
    t.frames(3).await?;
    t.check_eq(t.c().environment_values().ambient, 0.0, "ambient minimum is externally replaceable");
    let black = t.shot("no_ambient").await?;
    let dark_floor = crate::runner::lum(black.avg(0.4, 0.65, 0.6, 0.8));
    t.probe("no hidden minimum remains in the renderer", dark_floor, dark_floor < 0.5);
    t.check_eq(t.c().environment_values().lowpass, initial.lowpass, "lighting override leaves acoustics alone");
    t.c().clear_environment("test_unlit");
    t.cmd("/eclipse off").await?;
    t.until("server removal restores underlying light", Duration::from_secs(5), |t| t.c().environment_values().sunlight == 1.0)
        .await?;
    t.check_eq(t.c().environment_values().fog_end, initial.fog_end, "server removal restores fog");
    t.check_eq(t.c().environment_values().cloud_fog_end, initial.cloud_fog_end, "server removal restores cloud distance");
    Ok(())
}
