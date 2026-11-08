use std::time::Duration;

use weft_sdk::*;

use super::{add, at};
use crate::runner::{R, T};

fn item(name: &str) -> u32 {
    Item::lookup(name).unwrap_or(u32::MAX)
}

use base_api::HUNGER;

/// Sprint and sneak held, toggled (Shift + right click on the binding in Controls) and sprint from
/// a double tap of forward, its second binding.
pub async fn sprint_and_sneak_modes(t: T) -> R {
    static SPRINT: TextConfig = TextConfig::named("core:key.core:sprint");
    static SNEAK: TextConfig = TextConfig::named("core:key.core:sneak");
    t.flat().await?;
    // Switching a binding between held and toggled.
    let toggle = async |action: &str| -> R {
        t.c().menu_open(&screens::CONTROLS);
        t.screen("core:controls").await?;
        t.frames(3).await?;
        t.c().input_key("ShiftLeft", 0.2);
        t.frames(2).await?;
        t.click_ui(&format!("id:bind:{action}:0"), 1);
        // Shift let go again, or the walks after start sneaking.
        t.secs(0.3).await?;
        t.c().menu_close();
        t.no_screen().await?;
        // Toggles and double taps switch only once the game has the mouse again.
        t.frames(3).await?;
        Ok(())
    };
    toggle("core:sneak").await?;
    t.check_eq(SNEAK.get(), "toggle ShiftLeft".to_string(), "Shift + right click makes sneak a toggle");
    t.c().menu_open(&screens::CONTROLS);
    t.screen("core:controls").await?;
    t.frames(3).await?;
    t.shot("controls").await?;
    t.c().menu_close();
    t.no_screen().await?;
    t.c().set_look(0.0, 0.0);

    // Blocks walked in a second of holding forward.
    let walk = async || -> Result<f64, String> {
        let a = t.state().pos;
        t.hold("core:forward", 1.0);
        t.secs(1.2).await?;
        Ok(((t.state().pos.x - a.x).powi(2) + (t.state().pos.z - a.z).powi(2)).sqrt())
    };
    // Toggle sneak: one press slows the walk until the next.
    t.press("core:sneak");
    t.frames(4).await?;
    let d = walk().await?;
    t.press("core:sneak");
    t.frames(4).await?;
    let plain = walk().await?;
    t.note(format!("walking: {plain:.2}"));
    t.probe("toggled sneak, against walking", (d / plain) as f32, d < plain * 0.5);
    toggle("core:sneak").await?;
    let fast = |d: f64| d > plain * 1.15;

    // A double tap sprints as long as forward stays down.
    t.c().input_key("KeyW", 0.05);
    t.secs(0.1).await?;
    let d = walk().await?;
    t.probe("double tap, against walking", (d / plain) as f32, fast(d));
    let d = walk().await?;
    t.probe("after letting go of forward, against walking", (d / plain) as f32, !fast(d));

    // Toggle sprint: the key switches it on and off, as it goes down.
    toggle("core:sprint").await?;
    t.check_eq(SPRINT.get(), "toggle ControlLeft, double KeyW".to_string(), "the other binding stays");
    t.press("core:sprint");
    t.frames(4).await?;
    let d = walk().await?;
    t.probe("toggled sprint, against walking", (d / plain) as f32, fast(d));
    t.hold("core:sprint", 1.0);
    t.frames(4).await?;
    let d = walk().await?;
    t.probe("toggled off with the key still down, against walking", (d / plain) as f32, !fast(d));
    toggle("core:sprint").await?;
    Ok(())
}

/// Breaking blocks with and without the right tool: drops, pick up, tool wear.
pub async fn mining_and_wear(t: T) -> R {
    t.flat().await?;
    t.op("put me 0 stone_pickaxe 1").await?;
    t.c().select_slot(0);
    let f = t.feet();
    let stone = add(f, [2, 0, 0]);
    t.set(stone, "stone").await?;
    t.look_at(stone);
    t.frames(2).await?;
    // A stone pickaxe takes 0.56 s; letting go right after keeps the block behind intact.
    t.hold("core:attack", 0.8);
    t.secs(1.2).await?;
    t.check_eq(t.block(stone).await?, "core:air".into(), "stone mined");
    t.check_eq(t.slot("me", 0).await?.2, 1, "pickaxe wear");
    t.c()
        .teleport([stone[0] as f64 + 0.5, stone[1] as f64, stone[2] as f64 + 0.5]);
    let cobble = item("base:cobblestone");
    let picked = t
        .until("cobblestone picked up", Duration::from_secs(5), |t| (0..36).any(|i| t.c().inventory_slot(i).item == cobble))
        .await;
    t.check(picked.is_ok(), "no cobblestone after mining stone with a pickaxe");
    // Dirt by hand drops dirt.
    t.c().select_slot(4);
    let dirt = add(f, [0, 0, 2]);
    t.set(dirt, "dirt").await?;
    t.look_at(dirt);
    t.frames(2).await?;
    t.hold("core:attack", 0.9);
    t.secs(1.4).await?;
    t.check_eq(t.block(dirt).await?, "core:air".into(), "dirt dug by hand");
    t.c()
        .teleport([dirt[0] as f64 + 0.5, dirt[1] as f64, dirt[2] as f64 + 0.5]);
    let d = item("base:dirt");
    let got = t
        .until("dirt picked up", Duration::from_secs(5), |t| (0..36).any(|i| t.c().inventory_slot(i).item == d))
        .await;
    t.check(got.is_ok(), "digging dirt by hand gave nothing");
    // A break an interceptor refuses puts the block back on the client too.
    let gold = add(f, [-2, 0, 0]);
    t.set(gold, "gold_block").await?;
    t.op("put me 1 diamond_pickaxe 1").await?;
    t.c().select_slot(1);
    t.cmd("/rule testkit:protect_gold 1").await?;
    t.look_at(gold);
    t.frames(2).await?;
    t.hold("core:attack", 1.0);
    t.secs(1.5).await?;
    t.cmd("/rule testkit:protect_gold 0").await?;
    t.check_eq(t.block(gold).await?, "base:gold_block".into(), "protected gold on the server");
    t.sync_blocks(gold, gold).await?;
    Ok(())
}

/// Shift-click puts armor into its slots; hits wear it.
pub async fn armor(t: T) -> R {
    t.flat().await?;
    for (slot, name) in [(0, "iron_helmet"), (1, "diamond_chestplate"), (2, "gold_leggings"), (3, "leather_boots")] {
        t.op(&format!("put me {slot} {name} 1")).await?;
    }
    t.press("core:inventory");
    t.screen("core:menu").await?;
    for i in 36..40 {
        t.c().input_key("ShiftLeft", 0.3);
        t.frames(1).await?;
        t.click_ui(&format!("slot:{i}"), 0);
        t.frames(4).await?;
    }
    t.until("armor worn", Duration::from_secs(5), |t| (36..40).all(|s| !t.c().inventory_slot(s).is_empty()))
        .await?;
    // The player's inventory by group name: four equipment slots, all filled; the hotbar empty.
    t.check_eq(t.op("group me core:equipment").await?, "4 4".to_string(), "equipment group (slots, filled)");
    t.check_eq(t.op("group me base:head").await?, "1 1".to_string(), "the head slot");
    t.check_eq(t.op("group me hotbar").await?, "9 0".to_string(), "hotbar group");
    t.shot("worn").await?;
    t.c().menu_close();
    t.no_screen().await?;
    let f = t.feet();
    t.op(&format!("spawn zombie {}", at(add(f, [1, 0, 1]))))
        .await?;
    t.until("hit by the zombie", Duration::from_secs(15), |t| t.state().health < 20.0)
        .await?;
    t.secs(0.5).await?;
    let mut worn = 0;
    for s in 36..40 {
        worn += t.slot("me", s).await?.2;
    }
    t.check(worn > 0, "armor took no damage from a zombie hit");
    let hp = t.state().health;
    t.check(hp > 17.0, format!("armor did not reduce the hit: health {hp}"));
    // A zombie wears the same slots: a helmet goes on its head.
    let z = t
        .op(&format!("spawn zombie {}", at(add(f, [3, 0, -2]))))
        .await?;
    t.check_eq(t.op(&format!("group {z} core:equipment")).await?, "4 0".to_string(), "a zombie's equipment");
    t.op(&format!("put {z} 0 diamond_helmet 1")).await?;
    t.check_eq(t.op(&format!("group {z} base:head")).await?, "1 1".to_string(), "the zombie's head slot");
    t.c().set_look((-3f32).atan2(2.0).to_degrees(), -10.0);
    t.secs(0.5).await?;
    t.shot("zombie_helmet").await?;
    // A menu's equipment groups are those of the entity it is open on.
    t.op(&format!("emenu testkit:gear {z}")).await?;
    t.until("the zombie's helmet in the menu", Duration::from_secs(5), |t| {
        t.c().menu_slot(0).item == Item::named("base:diamond_helmet").id()
    })
    .await?;
    t.check(t.c().menu_slot(1).is_empty(), "nothing on its chest");
    t.shot("zombie_gear").await?;
    Ok(())
}

/// A slot holding at most 16: shift-clicking 32 ores into the test mod's electric furnace moves
/// 16 and leaves the rest.
pub async fn slot_limit(t: T) -> R {
    t.flat().await?;
    let p = add(t.feet(), [0, 0, -2]);
    t.set(p, "testmod:electric_furnace").await?;
    t.op("put me 0 iron_ore 32").await?;
    t.op(&format!("menu testmod:electric_furnace {} {} {}", p[0], p[1], p[2]))
        .await?;
    t.screen("core:menu").await?;
    // Menu slots: 0 input, 1 output, 2-28 main inventory, 29-37 hotbar.
    t.c().input_key("ShiftLeft", 0.3);
    t.frames(1).await?;
    t.click_ui("slot:29", 0);
    t.until("ores in the furnace", Duration::from_secs(5), |t| t.c().menu_slot(0).count > 0)
        .await?;
    t.frames(4).await?;
    t.check_eq(t.c().menu_slot(0).count, 16, "the input slot holds 16");
    t.check_eq(t.c().menu_slot(29).count, 16, "the rest stays in the hotbar");
    t.c().menu_close();
    t.no_screen().await
}

/// A slot taking only some items: the test mod's generator burns fuel, so shift-clicked dirt
/// stays with the player and coal goes in.
pub async fn slot_accepts(t: T) -> R {
    t.flat().await?;
    let p = add(t.feet(), [0, 0, -2]);
    t.set(p, "testmod:generator").await?;
    t.op("put me 0 dirt 16").await?;
    t.op("put me 1 coal 8").await?;
    t.op(&format!("menu testmod:generator {} {} {}", p[0], p[1], p[2]))
        .await?;
    t.screen("core:menu").await?;
    // Menu slots: 0 the generator's, 1-27 main inventory, 28-36 hotbar.
    for slot in [28, 29] {
        t.c().input_key("ShiftLeft", 0.3);
        t.frames(1).await?;
        t.click_ui(&format!("slot:{slot}"), 0);
        t.frames(4).await?;
    }
    let at = format!("{} {} {}", p[0], p[1], p[2]);
    // It starts burning one at once.
    t.until("coal in the generator", Duration::from_secs(5), |t| t.c().menu_slot(0).count >= 7)
        .await?;
    t.check_eq(t.slot(&at, 0).await?.0, "base:coal".to_string(), "the generator's slot");
    t.check_eq(t.slot("me", 9).await?, ("base:dirt".into(), 16, 0), "dirt moved to the main inventory");
    // Its fuel slot only takes: a click or a shift-click gets nothing back out.
    let before = t.c().menu_slot(0).count;
    t.click_ui("slot:0", 0);
    t.frames(4).await?;
    t.check(t.c().cursor_stack().is_empty(), "nothing comes out of an input slot");
    t.c().input_key("ShiftLeft", 0.3);
    t.frames(1).await?;
    t.click_ui("slot:0", 0);
    t.frames(4).await?;
    t.check(t.c().menu_slot(0).count + 1 >= before, "a shift-click doesn't take it out either");
    t.c().menu_close();
    t.no_screen().await
}

/// A liquid slot takes a bucket's water and gives it back, and holds no item.
pub async fn liquid_slot(t: T) -> R {
    t.flat().await?;
    let p = add(t.feet(), [0, 0, -2]);
    t.set(p, "testmod:tank").await?;
    t.op("put me 0 water_bucket 1").await?;
    t.op("put me 1 dirt 1").await?;
    t.op(&format!("menu testmod:tank {} {} {}", p[0], p[1], p[2]))
        .await?;
    t.screen("core:menu").await?;
    let at = format!("{} {} {}", p[0], p[1], p[2]);
    // A click held a moment, as a hand does: with something on the cursor a press starts a drag.
    let click = |slot: u32| -> Result<(), String> {
        let Rect { x, y, w, h } = t
            .c()
            .ui_rect(&format!("slot:{slot}"))
            .ok_or(format!("no slot {slot}"))?;
        t.c().input_cursor(x + w / 2.0, y + h / 2.0);
        t.c().input_click(0, 0.1);
        Ok(())
    };
    // Menu slots: 0 the tank's, 1-27 main inventory, 28-36 hotbar.
    click(28)?;
    t.secs(0.4).await?;
    click(0)?;
    t.secs(0.4).await?;
    t.check_eq(t.slot(&at, 0).await?, ("base:water".into(), 1000, 0), "a bucket poured in");
    t.check_eq(item_name(t.c().cursor_stack().item), "base:bucket", "the empty bucket on the cursor");
    t.c().input_cursor(0.2, 0.2);
    t.frames(3).await?;
    t.shot("poured").await?;
    click(0)?;
    t.secs(0.4).await?;
    t.check(t.slot(&at, 0).await?.1 == 0, "scooped out again");
    t.check_eq(item_name(t.c().cursor_stack().item), "base:water_bucket", "a full bucket back");
    click(28)?;
    t.secs(0.4).await?;
    click(29)?;
    t.secs(0.4).await?;
    click(0)?;
    t.secs(0.4).await?;
    t.check(t.slot(&at, 0).await?.1 == 0, "dirt stays out");
    t.c().menu_close();
    t.no_screen().await
}

/// Pouring and scooping water.
pub async fn bucket(t: T) -> R {
    t.flat().await?;
    t.op("put me 0 water_bucket 1").await?;
    t.c().select_slot(0);
    let f = t.feet();
    // A one block hole: the source cannot spread, so it is still there to scoop.
    let water = add(f, [1, -1, 0]);
    t.set(water, "air").await?;
    let ground = add(water, [0, -1, 0]);
    t.look_at(ground);
    t.frames(2).await?;
    t.press("core:use");
    t.until("water poured", Duration::from_secs(5), |t| t.c().inventory_slot(0).item == item("base:bucket"))
        .await?;
    t.check_eq(t.block(water).await?, "base:water[level=0]".into(), "water source");
    t.secs(1.5).await?;
    t.shot("poured").await?;
    t.look_at(water);
    t.frames(2).await?;
    t.press("core:use");
    t.until("water scooped", Duration::from_secs(5), |t| t.c().inventory_slot(0).item == item("base:water_bucket"))
        .await?;
    Ok(())
}

/// An ingot in the inventory and a placed torch each make an advancement, the client gets them
/// (a toast, the list on L), and `/advancements reset` takes them back.
pub async fn advancements(t: T) -> R {
    fn made(t: &T) -> u64 {
        t.c()
            .view()
            .component(&base_api::ADVANCEMENTS)
            .map_or(0, |a| a.done)
    }
    t.flat().await?;
    t.check_eq(made(&t), 0, "a new player's advancements");
    t.op("put me 0 iron_ingot 1").await?;
    t.until("advancement for the ingot", Duration::from_secs(5), |t| made(t).count_ones() == 1)
        .await?;
    t.secs(0.15).await?;
    t.shot("toast_sliding").await?;
    t.secs(0.6).await?;
    t.shot("toast").await?;
    t.op("put me 1 torch 1").await?;
    t.c().select_slot(1);
    t.look_at(add(t.feet(), [1, -1, 0]));
    t.frames(2).await?;
    t.press("core:use");
    t.until("advancement for the torch", Duration::from_secs(5), |t| made(t).count_ones() == 2)
        .await?;
    t.press("base:advancements");
    t.screen("base:advancements").await?;
    t.frames(3).await?;
    t.laid_out(&["text:Advancements", "text:Getting Started", "text:Exploration", "text:2 of 19 made"]);
    t.shot("list").await?;
    // The first tile of a group, right of its 90 pixel wide name, tells its details on hover.
    if let Some(Rect { x, y, w, h }) = t.c().ui_rect("text:Making Progress") {
        let st = t.state();
        t.c()
            .input_cursor(x + w + 13.0 * st.gui_scale / st.window[0] as f32, y + h / 2.0);
        t.frames(5).await?;
        t.shot("tooltip").await?;
    }
    t.c().menu_close();
    t.no_screen().await?;
    // The pause menu's button, as wide as the engine's own, opens the list too.
    t.c().menu_open(&screens::PAUSE);
    t.screen("core:pause").await?;
    t.frames(3).await?;
    t.shot("pause").await?;
    t.check(t.click_ui("text:Advancements", 0), "the pause menu's advancements button");
    t.screen("base:advancements").await?;
    // It opened on top of the pause menu, which is back once it closes.
    t.c().menu_close();
    t.screen("core:pause").await?;
    t.c().menu_close();
    t.no_screen().await?;
    // Dying takes none of them.
    t.cmd("/clear @s").await?;
    t.screen("core:death").await?;
    t.c().respawn();
    t.no_screen().await?;
    t.secs(1.0).await?;
    t.check_eq(made(&t).count_ones(), 2, "advancements after dying");
    t.cmd("/advancements reset").await?;
    t.until("advancements reset", Duration::from_secs(5), |t| made(t) == 0)
        .await?;
    Ok(())
}

fn bed_spawn(t: &T) -> Option<[i32; 3]> {
    t.c()
        .view()
        .component(&base_api::BED_SPAWN)
        .filter(|s| s.set != 0)
        .map(|s| s.bed)
}

/// Clears room around the player and places a bed two blocks north the way a player does: gives
/// its foot.
async fn place_bed(t: &T) -> Result<[i32; 3], String> {
    let f = t.feet();
    t.fill(add(f, [-3, -1, -6]), add(f, [3, -1, 2]), "stone")
        .await?;
    t.fill(add(f, [-3, 0, -6]), add(f, [3, 4, 2]), "air")
        .await?;
    t.op("put me 0 bed 1").await?;
    t.c().select_slot(0);
    let foot = add(f, [0, 0, -2]);
    t.look_at(add(foot, [0, -1, 0]));
    t.frames(2).await?;
    t.press("core:use");
    for _ in 0..30 {
        if t.block(add(foot, [0, 0, -1]))
            .await?
            .starts_with("base:bed")
        {
            break;
        }
        t.secs(0.1).await?;
    }
    // The server has it first: a click before the client does hits the floor.
    t.until("the bed on the client", Duration::from_secs(2), |t| t.c().block(foot).is_some_and(|s| s != 0))
        .await?;
    Ok(foot)
}

/// Dies and comes back, up to the point the world is seen again.
async fn die_and_respawn(t: &T) -> R {
    t.cmd("/clear @s").await?;
    t.screen("core:death").await?;
    t.c().respawn();
    t.no_screen().await
}

fn off_bed(t: &T, foot: [i32; 3]) -> f64 {
    let p = t.state().pos;
    ((p.x - (foot[0] as f64 + 0.5)).powi(2) + (p.z - (foot[2] as f64 + 0.5)).powi(2)).sqrt()
}

/// A bed placed by a player takes two blocks, the head away from them. Used by day it sets the
/// respawn point; at night the player lies down unless it can't be done (no room, a monster of
/// any kind near) and, everybody asleep, the night is skipped; being hurt gets them up; dead, the
/// player comes back at the bed. Breaking one half takes the other and drops one bed, and an
/// explosion taking one half takes the other too.
/// The second hand: what goes first on a right click, and what needs both hands.
/// What is on an entity is where a mod can ask, moves when a mod says, and what dangles hangs.
pub async fn model_bones(t: T) -> R {
    t.flat().await?;
    let me = t.c().player();
    let at = |bone: &str| t.c().bone_position(me, bone);
    t.op("put me 0 testmod:lantern").await?;
    t.secs(2.5).await?;
    t.check(at("no_such_bone").is_none(), "no bone of that name");
    let (hook, lamp) = (at("hook").ok_or("the lantern's hook")?, at("hanging_lamp").ok_or("the lantern's lamp")?);
    t.check(hook.y - lamp.y > 0.5, format!("it hangs from the hand: {hook:?} {lamp:?}"));
    t.check((hook.x - lamp.x).abs() + (hook.z - lamp.z).abs() < 0.3, format!("straight down when still: {hook:?} {lamp:?}"));
    // Past its hold easing in.
    t.op("put me 0 testmod:greatsword").await?;
    t.secs(0.5).await?;
    let tip = at("tip").ok_or("the sword's tip")?;
    for _ in 0..6 {
        t.c().bone_turn(me, "blade", [90.0, 0.0, 0.0]);
        t.frames(1).await?;
    }
    let bent = at("tip").ok_or("the sword's tip")?;
    let moved = (tip - bent).length_squared();
    t.check(moved > 0.1, format!("a mod turned the blade: {tip:?} to {bent:?}"));
    t.frames(4).await?;
    let back = at("tip").ok_or("the sword's tip")?;
    let again = (tip - back).length_squared();
    t.check(again < 0.01, format!("for as long as it said so: {tip:?} and {back:?}"));
    // Put away, it goes down as the model it is, not as its sprite.
    t.op("put me 0 testmod:apple").await?;
    t.secs(1.0).await?;
    t.check(at("fruit").is_some(), "an apple in hand is its model");
    t.c().input_key("Digit2", 0.0);
    t.frames(3).await?;
    t.check(at("fruit").is_some(), "the apple going down is still its model");
    t.secs(1.0).await?;
    t.check(at("fruit").is_none(), "and then it is gone");
    Ok(())
}

pub async fn second_hand(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let used = async |t: &T| -> Result<u32, String> {
        let all = t.op("events").await?;
        Ok(all
            .split_whitespace()
            .find_map(|e| e.strip_prefix("item_used="))
            .and_then(|n| n.parse().ok())
            .unwrap_or(0))
    };
    const OFF: u32 = 40;
    let id = |name: &str| Item::lookup(name).unwrap_or(u32::MAX);
    let ground = add(f, [2, -1, 0]);
    let above = add(ground, [0, 1, 0]);
    t.look_at(ground);
    t.frames(4).await?;
    t.op(&format!("put me {OFF} torch 8")).await?;
    // The bow goes before the torches, in the main hand or the other.
    t.op("put me 0 testmod:long_bow 2").await?;
    let before = used(&t).await?;
    t.press("core:use");
    t.secs(0.4).await?;
    t.check_eq(used(&t).await?, before + 1, "the bow shoots");
    t.check_eq(t.block(above).await?, "core:air".to_string(), "and no torch goes down");
    // With nothing to shoot it does nothing, so the torch gets its turn.
    t.op("put me 0 testmod:long_bow 1").await?;
    t.press("core:use");
    t.secs(0.4).await?;
    t.check_eq(t.block(above).await?, "base:torch[facing=up]".to_string(), "a bow that can't shoot leaves it to the other hand");
    t.set(above, "air").await?;
    t.op(&format!("put me {OFF} testmod:long_bow 2")).await?;
    t.op("put me 0 torch 8").await?;
    let before = used(&t).await?;
    t.secs(0.2).await?;
    t.press("core:use");
    t.secs(0.4).await?;
    t.check_eq(used(&t).await?, before + 1, "the priority holds from the other hand too");
    t.check_eq(t.block(above).await?, "core:air".to_string(), "no torch from the main hand");
    // With equal priorities the main hand goes first.
    t.op(&format!("put me {OFF} stone 8")).await?;
    t.secs(0.2).await?;
    t.press("core:use");
    t.secs(0.4).await?;
    t.check_eq(t.block(above).await?, "base:torch[facing=up]".to_string(), "the main hand goes first");
    t.set(above, "air").await?;
    // Two hands or nothing.
    t.op("put me 0 testmod:greatsword 1").await?;
    t.op(&format!("put me {OFF} torch 8")).await?;
    let before = used(&t).await?;
    t.secs(0.2).await?;
    t.press("core:use");
    t.secs(0.4).await?;
    t.check_eq(used(&t).await?, before, "a two-handed sword does nothing with the other hand busy");
    t.check_eq(t.block(above).await?, "base:torch[facing=up]".to_string(), "and the other hand still works");
    t.set(above, "air").await?;
    t.press("core:swap_hands");
    t.secs(0.3).await?;
    t.check_eq(t.c().inventory_slot(0).item, id("testmod:greatsword"), "it can't go to the other hand");
    t.op(&format!("put me {OFF} stick 0")).await?;
    let before = used(&t).await?;
    t.secs(0.2).await?;
    t.press("core:use");
    t.secs(0.4).await?;
    t.check_eq(used(&t).await?, before + 1, "both hands free, it works");
    // What may be held in one hand can go to the other, and the key swaps them.
    t.op("put me 0 testmod:map_sheet 1").await?;
    t.op(&format!("put me {OFF} torch 8")).await?;
    let before = used(&t).await?;
    t.secs(0.2).await?;
    t.press("core:use");
    t.secs(0.4).await?;
    t.check_eq(used(&t).await?, before + 1, "a map works with the other hand busy");
    t.press("core:swap_hands");
    t.until("the hands swapped", Duration::from_secs(2), |t| t.c().inventory_slot(0).item == id("base:torch"))
        .await?;
    t.check_eq(t.c().inventory_slot(OFF).item, id("testmod:map_sheet"), "it went to the other hand");
    t.shot("map_in_other_hand").await?;
    Ok(())
}

pub async fn beds(t: T) -> R {
    let spawn = bed_spawn;
    fn beds_held(t: &T) -> u32 {
        (0..36)
            .map(|i| t.c().inventory_slot(i))
            .filter(|s| s.item == item("base:bed"))
            .map(|s| s.count)
            .sum()
    }
    t.world(3).await?;
    t.op("mode survival").await?;
    t.op("time 6000").await?;
    let f = t.feet();
    let foot = place_bed(&t).await?;
    let head = add(foot, [0, 0, -1]);
    t.check(t.block(foot).await?.contains("part=foot"), format!("the foot where it was placed: {}", t.block(foot).await?));
    t.check(t.block(head).await?.contains("part=head"), format!("the head beyond it: {}", t.block(head).await?));

    // By day only the respawn point.
    t.look_at(foot);
    t.frames(2).await?;
    t.press("core:use");
    t.until("respawn point set", Duration::from_secs(5), |t| spawn(t) == Some(foot))
        .await?;
    t.secs(0.5).await?;
    t.check(t.c().screen() != "base:sleeping", "no sleeping by day");
    t.shot("placed").await?;

    t.op("time 18000").await?;
    // No room to stand on it.
    t.set(add(foot, [0, 2, 0]), "stone").await?;
    t.press("core:use");
    t.secs(0.4).await?;
    t.check(t.c().screen() != "base:sleeping", "no sleeping under a low ceiling");
    t.set(add(foot, [0, 2, 0]), "air").await?;

    // Any monster keeps the player up, by its tag.
    let creeper = t
        .op(&format!("spawn base:creeper {} {} {}", foot[0] + 5, foot[1], foot[2]))
        .await?;
    t.press("core:use");
    t.secs(0.4).await?;
    t.check(t.c().screen() != "base:sleeping", "no sleeping with a creeper near");
    t.op(&format!("remove {}", creeper.trim())).await?;

    // Being hurt gets them up.
    t.press("core:use");
    t.screen("base:sleeping").await?;
    t.op("hurt_me 1").await?;
    t.until("woken by the hit", Duration::from_secs(3), |t| t.c().screen() != "base:sleeping")
        .await?;

    // At night the player lies down, and the night is skipped.
    t.look_at(foot);
    t.frames(2).await?;
    t.press("core:use");
    t.screen("base:sleeping").await?;
    // Lying down on the bed, as the others see it too: the server says so.
    t.secs(0.3).await?;
    t.check_eq(t.op("pose").await?.trim().to_string(), abi::pose::SLEEP.to_string(), "lying down");
    let p = t.state().pos;
    t.check((p.z - foot[2] as f64).abs() < 0.1, format!("over the middle of the bed: {p:?}, foot {foot:?}"));
    t.secs(2.5).await?;
    t.shot("sleeping").await?;
    t.until("morning", Duration::from_secs(15), |t| t.c().screen() != "base:sleeping")
        .await?;
    let time = t.cmd("/time query").await?;
    let tick: u64 = time
        .split(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
        .unwrap_or(u64::MAX);
    t.check(tick % 24000 < 2000, format!("woke up in the morning: {time}"));

    // Dead, back at the bed (on its foot), the respawn point still set.
    // The world's spawn is two blocks from the bed: only the bed puts the player on it.
    die_and_respawn(&t).await?;
    t.secs(1.0).await?;
    let off = off_bed(&t, foot);
    t.check(off < 1.5, format!("respawned {off:.1} blocks from the bed"));
    t.check(spawn(&t) == Some(foot), "the respawn point survives death");

    // Breaking the head takes the foot and drops one bed.
    t.op(&format!("break {} {} {}", head[0], head[1], head[2]))
        .await?;
    t.frames(2).await?;
    t.check_eq(t.block(foot).await?, "core:air".into(), "the foot goes with the head");
    t.until("the bed picked up", Duration::from_secs(5), |t| beds_held(t) >= 1)
        .await?;
    t.secs(0.5).await?;
    t.check_eq(beds_held(&t), 1, "beds dropped");

    // Placed each way: the head away from the player, the bed facing them. Seen from above, the
    // pillow is at the far end of each.
    t.op("mode creative").await?;
    t.c().set_flying(true);
    t.fill(add(f, [-4, -1, -4]), add(f, [4, -1, 4]), "stone")
        .await?;
    t.fill(add(f, [-4, 0, -4]), add(f, [4, 4, 4]), "air")
        .await?;
    t.c()
        .teleport([f[0] as f64 + 0.5, f[1] as f64, f[2] as f64 + 0.5]);
    t.secs(0.3).await?;
    for (facing, d) in [("south", [0, 0, -1]), ("west", [1, 0, 0]), ("north", [0, 0, 1]), ("east", [-1, 0, 0])] {
        t.op("put me 0 bed 1").await?;
        let foot = [f[0] + d[0] * 2, f[1], f[2] + d[2] * 2];
        let head = add(foot, d);
        t.look_at(add(foot, [0, -1, 0]));
        t.frames(2).await?;
        t.press("core:use");
        t.until(&format!("the {facing} bed"), Duration::from_secs(3), |t| t.c().block(head).is_some_and(|s| s != 0))
            .await?;
        let (a, b) = (t.block(foot).await?, t.block(head).await?);
        t.check(a.contains("part=foot") && a.contains(&format!("facing={facing}")), format!("the foot: {a}"));
        t.check(b.contains("part=head") && b.contains(&format!("facing={facing}")), format!("the head: {b}"));
    }
    t.c()
        .teleport([f[0] as f64 + 0.5, f[1] as f64 + 7.0, f[2] as f64 + 0.5]);
    // Landing ended the flight.
    t.c().set_flying(true);
    t.c().set_look(0.0, -89.0);
    t.secs(1.0).await?;
    t.shot("facings").await?;

    // An explosion takes one half without breaking it.
    let foot = add(f, [0, 0, -2]);
    let head = add(foot, [0, 0, -1]);
    t.c()
        .teleport([head[0] as f64 + 1.5, head[1] as f64, head[2] as f64 + 0.5]);
    t.secs(0.2).await?;
    t.op("crater_fast 1").await?;
    t.until("the head gone", Duration::from_secs(2), |t| t.c().block(head) == Some(0))
        .await?;
    t.secs(0.3).await?;
    t.check_eq(t.block(foot).await?, "core:air".into(), "the foot goes with the head in an explosion");
    Ok(())
}

/// The respawn point holds wherever the bed is: far from the spawn, so nothing around it is
/// loaded when the player comes back, and in another dimension. A bed gone meanwhile puts them back
/// at the spawn, without a respawn point.
pub async fn beds_far(t: T) -> R {
    t.world(3).await?;
    t.op("mode creative").await?;
    t.op("time 6000").await?;
    let home = t.state().pos;
    t.cmd("/tp ~1500 ~30 ~").await?;
    t.until("far away", Duration::from_secs(10), |t| (t.state().pos.x - home.x).abs() > 1000.0)
        .await?;
    // Only now: the news of the mode, on its way above, turns flying off.
    t.secs(0.2).await?;
    t.c().set_flying(true);
    t.loaded().await?;
    let foot = place_bed(&t).await?;
    t.look_at(foot);
    t.frames(2).await?;
    t.press("core:use");
    t.until("respawn point set", Duration::from_secs(5), |t| bed_spawn(t) == Some(foot))
        .await?;
    t.op("mode survival").await?;

    die_and_respawn(&t).await?;
    t.until("back on the bed", Duration::from_secs(20), |t| off_bed(t, foot) < 1.5)
        .await?;
    t.secs(1.0).await?;
    t.check(off_bed(&t, foot) < 1.5, format!("still {:.1} blocks from the bed", off_bed(&t, foot)));
    t.check(bed_spawn(&t) == Some(foot), "the respawn point stays");

    t.op(&format!("break {} {} {}", foot[0], foot[1], foot[2]))
        .await?;
    t.frames(2).await?;
    die_and_respawn(&t).await?;
    t.until("the respawn point gone", Duration::from_secs(20), |t| bed_spawn(t).is_none())
        .await?;
    t.until("back at the spawn", Duration::from_secs(20), |t| (t.state().pos.x - home.x).abs() < 100.0)
        .await?;

    t.cmd("/dim base:flatlands").await?;
    let dim = Dimension::lookup("base:flatlands").unwrap_or(u32::MAX);
    t.until("flatlands", Duration::from_secs(20), |t| t.state().dim == dim)
        .await?;
    t.loaded().await?;
    t.op("time 6000").await?;
    let foot = place_bed(&t).await?;
    t.look_at(foot);
    t.frames(2).await?;
    t.press("core:use");
    t.until("respawn point set there", Duration::from_secs(5), |t| bed_spawn(t) == Some(foot))
        .await?;
    die_and_respawn(&t).await?;
    t.until("back on the bed there", Duration::from_secs(20), |t| t.state().dim == dim && off_bed(t, foot) < 1.5)
        .await?;
    t.secs(1.0).await?;
    t.check_eq(t.state().dim, dim, "stays in the bed's dimension");
    t.check(bed_spawn(&t) == Some(foot), "the respawn point stays there too");
    Ok(())
}

/// A creeper going off under water breaks nothing and the water stays. A crater in a lake is
/// filled again by the water around it.
pub async fn explosions_and_water(t: T) -> R {
    t.flat().await?;
    t.cmd("/rule testkit:no_hurt 1").await?;
    let f = t.feet();
    t.fill(add(f, [-6, -1, -6]), add(f, [6, -1, 6]), "stone")
        .await?;
    t.fill(add(f, [-6, 0, -6]), add(f, [6, 2, 6]), "water")
        .await?;
    t.secs(0.3).await?;
    let spot = add(f, [2, 0, 0]);
    let creeper = t.op(&format!("spawn base:creeper {}", at(spot))).await?;
    let id: u64 = creeper
        .trim()
        .parse()
        .map_err(|_| format!("spawn creeper: {creeper}"))?;
    t.until("the creeper going off", Duration::from_secs(6), |t| t.c().entity(id).is_none())
        .await?;
    t.secs(0.3).await?;
    t.check(t.block(spot).await?.contains("water"), format!("the water stays: {}", t.block(spot).await?));
    t.check_eq(t.block(add(spot, [0, -1, 0])).await?, "base:stone".into(), "the floor under it stays");
    // The same place, a hole in the water that the water must fill.
    t.op("mode creative").await?;
    t.c().set_flying(true);
    t.c()
        .teleport([f[0] as f64 + 0.5, f[1] as f64 + 1.0, f[2] as f64 + 0.5]);
    t.secs(0.2).await?;
    t.op("crater_fast 3").await?;
    let hole = add(f, [0, 1, 2]);
    t.until("the hole", Duration::from_secs(2), |t| t.c().block(hole) == Some(0))
        .await?;
    t.until("water flowing back in", Duration::from_secs(6), |t| t.c().block(hole).is_some_and(|s| s != 0))
        .await?;
    t.cmd("/rule testkit:no_hurt 0").await?;
    Ok(())
}

/// 12 blocks down costs 9 health like vanilla.
pub async fn fall_damage(t: T) -> R {
    t.flat().await?;
    t.until("full health", Duration::from_secs(5), |t| t.state().health >= 20.0)
        .await?;
    let s = t.state();
    t.c().teleport([s.pos.x, s.pos.y + 12.0, s.pos.z]);
    t.until("falling", Duration::from_secs(5), |t| !t.flag(client::client_flags::ON_GROUND))
        .await?;
    t.until("landed", Duration::from_secs(10), |t| t.flag(client::client_flags::ON_GROUND))
        .await?;
    // The lowest it gets: full and saturated it heals fast, a point in half a second.
    let mut hp = f32::MAX;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(500) {
        hp = hp.min(t.state().health);
        t.frames(1).await?;
    }
    t.check((10.0..=13.0).contains(&hp), format!("health after a 12 block fall: {hp}, vanilla 11"));
    Ok(())
}

/// The server and the client measure the player with the same box, before and after `/morph`.
pub async fn player_box(t: T) -> R {
    t.flat().await?;
    for step in ["start", "morphed", "back"] {
        match step {
            "morphed" => t.cmd("/morph base:pig").await.map(|_| ())?,
            "back" => t.cmd("/morph").await.map(|_| ())?,
            _ => {}
        }
        t.secs(0.3).await?;
        let server = t.op("size me").await?;
        let c = t.c().entity(0).ok_or("no local player")?.size;
        let client = format!("{:.3} {:.3} {:.3}", c.width, c.height, c.eye);
        t.check(server == client, format!("{step}: server box {server}, client box {client}"));
        if step == "start" {
            t.check(server.starts_with("0.600 1.800"), format!("player box is {server}, not 0.6 x 1.8"));
        }
    }
    Ok(())
}

/// Sneaking at an edge stops at it instead of falling off.
pub async fn sneak_edge(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let base = add(f, [5, 0, 5]);
    t.fill(base, add(base, [0, 2, 0]), "stone").await?;
    t.c()
        .teleport([base[0] as f64 + 0.5, base[1] as f64 + 3.0, base[2] as f64 + 0.5]);
    t.until("on the pillar", Duration::from_secs(5), |t| t.flag(client::client_flags::ON_GROUND))
        .await?;
    t.c().set_look(30.0, 0.0);
    t.hold("core:sneak", 4.0);
    t.frames(2).await?;
    t.hold("core:forward", 2.0);
    t.secs(1.0).await?;
    t.hold("core:left", 1.0);
    t.secs(1.5).await?;
    let y = t.state().pos.y;
    t.check(y > base[1] as f64 + 2.9, format!("fell off while sneaking: y {y:.2}"));
    Ok(())
}

/// The player moves by its attributes like a mob: +100% movement speed walks about twice as far,
/// a stronger jump goes higher, and a push from the server lifts it.
pub async fn movement_attributes(t: T) -> R {
    t.flat().await?;
    t.c().set_look(0.0, 0.0);
    let walk = |t: T| async move {
        let start = t.state().pos;
        t.hold("core:forward", 1.0);
        t.secs(1.3).await?;
        let end = t.state().pos;
        Ok::<f64, String>(((end.x - start.x).powi(2) + (end.z - start.z).powi(2)).sqrt())
    };
    let slow = walk(t.clone()).await?;
    t.op("attr_mod me core:movement_speed testkit:fast 1 1 0")
        .await?;
    t.secs(0.2).await?;
    let fast = walk(t.clone()).await?;
    t.note(format!("walked {slow:.2} then {fast:.2} blocks in a second"));
    t.check(fast > slow * 1.6, format!("faster with the attribute: {slow:.2} -> {fast:.2}"));
    t.op("attr_rm me core:movement_speed testkit:fast").await?;
    let jump = |t: T| async move {
        let y0 = t.state().pos.y;
        let mut top = y0;
        t.hold("core:jump", 0.1);
        for _ in 0..30 {
            t.secs(0.03).await?;
            top = top.max(t.state().pos.y);
        }
        t.secs(1.0).await?;
        Ok::<f64, String>(top - y0)
    };
    let low = jump(t.clone()).await?;
    t.op("attr_mod me core:jump_strength testkit:spring 1 0.5 0")
        .await?;
    t.secs(0.2).await?;
    let high = jump(t.clone()).await?;
    t.note(format!("jumped {low:.2} then {high:.2}"));
    t.check(high > low * 1.4, format!("higher with the attribute: {low:.2} -> {high:.2}"));
    t.op("attr_rm me core:jump_strength testkit:spring").await?;
    let y = t.state().pos.y;
    t.op("push 0 12 0").await?;
    t.secs(0.25).await?;
    t.check(t.state().pos.y > y + 0.3, format!("the push lifted the player: {y:.2} -> {:.2}", t.state().pos.y));
    Ok(())
}

/// Mobs move by the player's rules: a pig pushed across ice slides further than on grass, walks
/// slower through oil, and climbs a ladder it walks into.
/// A ladder (`on_sides`, walls only) goes against the wall clicked, a second wall of its cell
/// adds a side, and a side whose wall goes falls off, then the rest.
pub async fn ladder_sides(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let c = add(f, [0, 1, -3]);
    let (north, west) = (add(c, [0, 0, -1]), add(c, [-1, 0, 0]));
    for w in [north, west] {
        t.set(w, "cobblestone").await?;
    }
    t.op("mode creative").await?;
    t.op("put me 0 ladder 8").await?;
    let at = |p: [i32; 3]| format!("/tp {} {} {}", p[0] as f64 + 0.5, p[1] - 1, p[2] as f64 + 0.5);
    let side = |d: &str| format!("prop {} {} {} {d}", c[0], c[1], c[2]);
    t.cmd(&at(add(c, [0, 0, 2]))).await?;
    t.secs(0.3).await?;
    t.look_at(north);
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.4).await?;
    t.check_eq(t.op(&side("north")).await?, "true".to_string(), "on the wall clicked");
    t.check_eq(t.op(&side("west")).await?, "false".to_string(), "and only there");
    t.cmd(&at(add(c, [2, 0, 0]))).await?;
    t.secs(0.3).await?;
    t.look_at(west);
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.4).await?;
    t.check_eq(t.op(&side("west")).await?, "true".to_string(), "a second wall of the cell adds a side");
    t.check_eq(t.op(&side("north")).await?, "true".to_string(), "keeping the first");
    t.cmd(&at(add(c, [1, 0, 1]))).await?;
    t.secs(0.3).await?;
    t.look_at(c);
    t.frames(3).await?;
    t.shot("two_sides").await?;
    t.set(north, "air").await?;
    t.secs(0.3).await?;
    t.check_eq(t.op(&side("north")).await?, "false".to_string(), "a side without its wall falls off");
    t.check_eq(t.op(&side("west")).await?, "true".to_string(), "the other stays");
    t.set(west, "air").await?;
    t.secs(0.3).await?;
    t.check_eq(t.op(&format!("get {} {} {}", c[0], c[1], c[2])).await?, "core:air".to_string(), "with no side left it goes");
    Ok(())
}

pub async fn mob_physics(t: T) -> R {
    t.flat().await?;
    t.cmd("/rule core:mob_spawning 0").await?;
    let f = t.feet();
    let floor = |p: [i32; 3]| add(p, [0, -1, 0]);
    // How far a pig slides after a push east, over `ground`.
    let run = |t: T, at: [i32; 3], ground: &'static str| async move {
        t.fill(floor(at), floor(add(at, [24, 0, 2])), ground)
            .await?;
        t.fill(at, add(at, [24, 2, 2]), "air").await?;
        let id = t
            .op(&format!("spawn base:pig {} {} {}", at[0], at[1], at[2] + 1))
            .await?;
        t.secs(0.5).await?;
        let x0: f64 = t
            .op(&format!("entity {id}"))
            .await?
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        t.op(&format!("push 12 0 0 {id}")).await?;
        t.secs(1.0).await?;
        let x1: f64 = t
            .op(&format!("entity {id}"))
            .await?
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        t.op(&format!("remove {id}")).await?;
        Ok::<f64, String>(x1 - x0)
    };
    let grass = run(t.clone(), add(f, [-12, 0, 6]), "grass").await?;
    let ice = run(t.clone(), add(f, [-12, 0, 12]), "ice").await?;
    t.note(format!("a pig went {grass:.2} blocks on grass, {ice:.2} on ice"));
    t.check(ice > grass * 1.3, format!("it slides on ice: {grass:.2} -> {ice:.2}"));
    // Walked 30 ticks over grass, then through oil (a liquid with drag).
    let walk = |t: T, at: [i32; 3], liquid: bool| async move {
        t.fill(floor(at), floor(add(at, [24, 0, 2])), "grass")
            .await?;
        t.fill(at, add(at, [24, 2, 2]), "air").await?;
        if liquid {
            t.fill(at, add(at, [24, 0, 2]), "testmod:oil").await?;
        }
        let id = t
            .op(&format!("spawn base:pig {} {} {}", at[0], at[1], at[2] + 1))
            .await?;
        t.secs(0.5).await?;
        let x0: f64 = t
            .op(&format!("entity {id}"))
            .await?
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        t.op(&format!("drive {id} 1 0 30")).await?;
        t.secs(1.5).await?;
        let x1: f64 = t
            .op(&format!("entity {id}"))
            .await?
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        t.op(&format!("remove {id}")).await?;
        Ok::<f64, String>(x1 - x0)
    };
    let dry = walk(t.clone(), add(f, [-12, 0, 18]), false).await?;
    let oil = walk(t.clone(), add(f, [-12, 0, 24]), true).await?;
    t.note(format!("walked {dry:.2} blocks on grass, {oil:.2} through oil"));
    t.check(oil < dry * 0.75, format!("slower through oil: {dry:.2} -> {oil:.2}"));
    // A wall with a ladder: walked into it, the pig goes up.
    let wall = add(f, [6, 0, -6]);
    t.fill(add(wall, [0, 0, -1]), add(wall, [0, 5, 1]), "stone")
        .await?;
    t.fill(add(wall, [-1, 0, -1]), add(wall, [-1, 5, 1]), "base:ladder[north=false,south=false,west=false,east=true]")
        .await?;
    let id = t
        .op(&format!("spawn base:pig {} {} {}", wall[0] - 2, wall[1], wall[2]))
        .await?;
    t.secs(0.5).await?;
    let y0: f64 = t
        .op(&format!("entity {id}"))
        .await?
        .split_whitespace()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    t.op(&format!("drive {id} 1 0 60")).await?;
    t.secs(3.0).await?;
    let y1: f64 = t
        .op(&format!("entity {id}"))
        .await?
        .split_whitespace()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    t.note(format!("the pig on the ladder: y {y0:.2} -> {y1:.2}"));
    t.check(y1 > y0 + 1.5, format!("it climbs the ladder: {y0:.2} -> {y1:.2}"));
    Ok(())
}

pub async fn drop_and_pickup(t: T) -> R {
    t.flat().await?;
    t.op("put me 0 cobblestone 10").await?;
    t.c().select_slot(0);
    t.c().set_look(0.0, 0.0);
    t.frames(2).await?;
    t.press("core:drop");
    t.until("one dropped", Duration::from_secs(5), |t| t.c().inventory_slot(0).count == 9)
        .await?;
    t.check(t.count_near("core:item", 8.0).await? == 1, "no item entity after dropping");
    t.secs(2.2).await?;
    let item = t.c().entities().first().copied();
    t.hold("core:forward", 1.0);
    // The client lets the taken item fly to the player for three ticks before it goes.
    let mut flying = false;
    t.until("picked up again", Duration::from_secs(6), |t| {
        let got = t.c().inventory_slot(0).count == 10;
        flying |= got && item.is_some_and(|id| t.c().entity(id).is_some());
        got
    })
    .await?;
    t.check(flying, "the taken item was still on its way when it reached the inventory");
    t.secs(0.3).await?;
    t.check(item.is_none_or(|id| t.c().entity(id).is_none()), "the taken item is gone");
    Ok(())
}

/// Creative tools never wear, and invulnerable entities (the test mod's crate) take no damage
/// from hits.
pub async fn creative_and_invulnerable(t: T) -> R {
    t.flat().await?;
    t.op("mode creative").await?;
    t.op("put me 0 stone_pickaxe 1").await?;
    t.c().select_slot(0);
    let f = t.feet();
    let stone = add(f, [2, 0, 0]);
    t.set(stone, "stone").await?;
    t.look_at(stone);
    t.frames(2).await?;
    t.hold("core:attack", 0.3);
    t.secs(0.6).await?;
    t.check_eq(t.block(stone).await?, "core:air".into(), "stone broken in creative");
    t.check_eq(t.slot("me", 0).await?.2, 0, "no pickaxe wear in creative");
    t.op("mode survival").await?;
    let id = t
        .op(&format!("spawn testmod:crate {} {} {}", stone[0], stone[1], stone[2]))
        .await?;
    t.look_at(stone);
    t.frames(2).await?;
    t.hold("core:attack", 1.5);
    t.secs(2.0).await?;
    let health = t.op(&format!("entity {}", id.trim())).await?;
    t.check(health.ends_with(" 10"), format!("the crate kept its health ({health})"));
    // The same hits do hurt an ordinary entity there.
    t.op(&format!("remove {}", id.trim())).await?;
    let idol = t
        .op(&format!("spawn testmod:idol {} {} {}", stone[0], stone[1], stone[2]))
        .await?;
    let before = t.op(&format!("entity {}", idol.trim())).await?;
    t.look_at(stone);
    t.frames(2).await?;
    t.hold("core:attack", 1.5);
    t.secs(2.0).await?;
    let after = t.op(&format!("entity {}", idol.trim())).await?;
    let hp = |s: &str| {
        s.rsplit(' ')
            .next()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0)
    };
    t.check(hp(&after) < hp(&before), format!("an idol is hurt by the same hits ({before} -> {after})"));
    Ok(())
}

/// Every creative click breaks a block, also at over 1000 frames a second (instant breaks once
/// needed a frame longer than 1 ms), and a held button keeps breaking.
/// Looks at a point from the player's eyes.
fn aim(t: &T, p: [f64; 3]) {
    let s = t.state();
    let eye = [s.pos.x, s.pos.y + t.c().entity(0).map_or(1.62, |e| e.size.eye as f64), s.pos.z];
    let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
    let yaw = (-d[0]).atan2(-d[2]).to_degrees() as f32;
    let pitch = d[1].atan2((d[0] * d[0] + d[2] * d[2]).sqrt()).to_degrees() as f32;
    t.c().set_look(yaw, pitch);
}

/// Stairs, slabs and vertical slabs: made for all with a tag, also another mod's block; placed by
/// where they are clicked, two slabs are one block and give back two; stairs join into corners
/// the way vanilla's do; and they are as solid as they look.
pub async fn block_shapes(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let centre = |p: [i32; 3], dx: f64, dy: f64, dz: f64| [p[0] as f64 + dx, p[1] as f64 + dy, p[2] as f64 + dz];

    // Another mod's block, tagged in that mod.
    t.set(add(f, [0, 3, 5]), "testmod:reinforced_stone_stairs")
        .await?;
    t.check(
        t.block(add(f, [0, 3, 5]))
            .await?
            .contains("reinforced_stone_stairs"),
        "a block of another mod has its stairs",
    );
    t.set(add(f, [0, 3, 5]), "air").await?;

    // How they look, for whoever is looking.
    let row = add(f, [-6, 0, 6]);
    let shown = [
        "base:stone_stairs[facing=south]",
        "base:stone_stairs[facing=south]",
        "base:oak_planks_stairs[facing=east]",
        "base:oak_planks_slab",
        "base:oak_planks_slab[type=top]",
        "base:bricks_vertical_slab[facing=south]",
        "base:bricks_vertical_slab[facing=east]",
        "base:sandstone_slab[type=double]",
        "base:spruce_planks_stairs[facing=south,half=top]",
        "base:mossy_cobblestone_stairs[facing=north]",
    ];
    for (i, b) in shown.into_iter().enumerate() {
        t.set(add(row, [i as i32, 0, 0]), b).await?;
    }
    t.set(add(row, [1, 0, 1]), "base:stone_stairs[facing=west]")
        .await?;
    aim(&t, centre(row, 5.0, 0.5, 0.5));
    t.secs(0.3).await?;
    t.shot("shapes").await?;
    t.fill(row, add(row, [9, 0, 1]), "air").await?;

    let wall = add(f, [0, 0, -3]);
    t.fill(add(wall, [-1, 0, 0]), add(wall, [1, 2, 0]), "stone")
        .await?;
    t.op("put me 0 base:oak_planks_slab 16").await?;
    t.c().select_slot(0);
    t.frames(4).await?;

    // High on a wall the top half, low on it the bottom one.
    let cell = add(wall, [0, 1, 1]);
    aim(&t, centre(wall, 0.5, 1.8, 1.0));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    t.check(t.block(cell).await?.contains("type=top"), format!("high on a wall: {}", t.block(cell).await?));
    t.set(cell, "air").await?;
    aim(&t, centre(wall, 0.5, 1.2, 1.0));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    t.check(t.block(cell).await?.contains("type=bottom"), format!("low on it: {}", t.block(cell).await?));
    let before = t.slot("me", 0).await?.1;

    // On its top, one makes it whole, one less in the hand.
    aim(&t, centre(cell, 0.5, 0.5, 0.5));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    t.check(t.block(cell).await?.contains("type=double"), format!("the second one joins it: {}", t.block(cell).await?));
    t.check_eq(t.slot("me", 0).await?.1, before - 1, "and is used up");

    // Breaking it gives back both.
    t.op(&format!("break {}", at(cell))).await?;
    t.c().teleport(centre(cell, 0.5, -1.0, 0.5));
    t.secs(1.5).await?;
    t.check_eq(t.slot("me", 0).await?.1, before + 1, "both come back");
    t.c().teleport(centre(f, 0.0, 0.0, 0.0));
    t.secs(0.2).await?;

    // On a floor, from above, wherever one is: flying over it, next to it, at one's feet.
    t.op("mode creative").await?;
    t.c().set_flying(true);
    let floor = add(f, [8, -1, -8]);
    t.fill(add(floor, [-3, 0, -3]), add(floor, [3, 0, 3]), "stone")
        .await?;
    for (name, over, height) in [("flying over", [0.5, 0.5], 3.0), ("off to a side", [1.5, 0.5], 1.0), ("next to it", [0.5, 1.4], 0.0)] {
        t.op("put me 0 base:sandstone_slab 16").await?;
        t.c().select_slot(0);
        t.c()
            .teleport(centre(floor, over[0], 1.0 + height, over[1]));
        t.secs(0.4).await?;
        aim(&t, centre(floor, 0.5, 1.0, 0.45));
        t.frames(4).await?;
        t.press("core:use");
        t.secs(0.3).await?;
        let there = add(floor, [0, 1, 0]);
        t.check(t.block(there).await?.contains("type=bottom"), format!("{name}: {}", t.block(there).await?));
        t.set(there, "air").await?;
    }
    t.c().set_flying(false);
    // Standing on a slab beside it, over its edge: in the air over the block, not in it.
    t.set(add(floor, [0, 1, 1]), "base:sandstone_slab").await?;
    t.c().teleport(centre(floor, 0.5, 1.6, 1.2));
    t.secs(0.8).await?;
    aim(&t, centre(floor, 0.5, 1.0, 0.45));
    t.frames(4).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    let there = add(floor, [0, 1, 0]);
    t.check(t.block(there).await?.contains("type=bottom"), format!("on a slab beside it: {}", t.block(there).await?));
    t.set(there, "air").await?;
    // But not in what would fill the place where one stands.
    t.set(add(floor, [0, 1, 1]), "air").await?;
    t.c().teleport(centre(floor, 0.5, 1.0, 1.2));
    t.secs(0.5).await?;
    aim(&t, centre(floor, 0.5, 1.0, 0.45));
    t.frames(4).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    t.check(t.block(there).await?.contains("air"), format!("into one's own feet: {}", t.block(there).await?));
    t.op("mode survival").await?;
    t.c().teleport(centre(f, 0.0, 0.0, 0.0));
    t.secs(0.3).await?;
    t.op("put me 0 base:oak_planks_slab 16").await?;
    t.c().select_slot(0);

    // Of two different blocks too, which is all of them it gives back.
    let mix = add(f, [-8, 0, -8]);
    t.set(add(mix, [0, -1, 0]), "stone").await?;
    t.set(mix, "base:oak_planks_slab").await?;
    t.op("put me 2 base:sandstone_slab 8").await?;
    t.c().select_slot(2);
    t.c().teleport(centre(mix, 0.5, 0.0, 2.4));
    t.secs(0.4).await?;
    aim(&t, centre(mix, 0.5, 0.5, 0.5));
    t.frames(4).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    let both = t.op(&format!("parts {}", at(mix))).await?;
    t.check(both.contains("oak_planks_slab") && both.contains("sandstone_slab[type=top]"), format!("two kinds in one cell: {both}"));
    t.check_eq(t.slot("me", 2).await?.1, 7, "with one from the hand");
    let shot = t.shot("mixed").await?;
    // Sandstone over wood, of nothing else: no block's shader or texture comes along with a mix.
    let pink = shot.share(0.38, 0.3, 0.62, 0.72, |c| c[0] > 150 && c[2] > 150 && c[1] < 130);
    t.probe("what isn't sandstone or wood on it", pink, pink < 0.01);
    // They stop a walker as one, each of them being there.
    t.c().teleport(centre(mix, 0.5, 2.0, 0.5));
    t.secs(0.8).await?;
    t.check((t.state().pos.y - (mix[1] as f64 + 1.0)).abs() < 0.01, format!("stands on the top one: {:?}", t.state().pos));
    t.c().teleport(centre(mix, 0.5, 0.0, 2.4));
    t.secs(0.3).await?;
    // Each part by what it asks of the tool, as it would on its own: sandstone wants a pickaxe.
    t.op(&format!("break {} base:iron_pickaxe", at(mix)))
        .await?;
    t.c().teleport(centre(mix, 0.5, 0.0, 0.5));
    t.secs(1.5).await?;
    t.check_eq(t.slot("me", 2).await?.1, 8, "it gives back the one");
    let inv = t.op("inv me").await?;
    t.check(inv.contains("oak_planks_slab"), format!("and the other: {inv}"));
    t.c().teleport(centre(f, 0.0, 0.0, 0.0));
    t.secs(0.2).await?;
    t.c().select_slot(0);

    // A vertical one goes against what is clicked, and a second makes it whole too.
    t.op("put me 1 base:oak_planks_vertical_slab 16").await?;
    t.c().select_slot(1);
    t.frames(4).await?;
    aim(&t, centre(wall, 0.5, 1.5, 1.0));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    t.check(t.block(cell).await?.contains("facing=south"), format!("against the wall it is on: {}", t.block(cell).await?));
    aim(&t, centre(cell, 0.5, 0.5, 0.5));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    t.check(t.block(cell).await?.contains("double=true"), format!("and a second one joins it: {}", t.block(cell).await?));
    t.set(cell, "air").await?;

    // Corners, by vanilla's rules: what is behind a stair crossways makes it an outer corner, what
    // is in front of it an inner one.
    let a = add(f, [6, 0, 6]);
    t.set(a, "base:stone_stairs[facing=east]").await?;
    t.set(add(a, [1, 0, 0]), "base:stone_stairs[facing=south]")
        .await?;
    t.frames(4).await?;
    t.check(t.block(a).await?.contains("shape=outer_right"), format!("an outer corner: {}", t.block(a).await?));
    t.set(add(a, [1, 0, 0]), "air").await?;
    t.frames(4).await?;
    t.check(t.block(a).await?.contains("shape=straight"), format!("and straight again: {}", t.block(a).await?));
    t.set(add(a, [-1, 0, 0]), "base:stone_stairs[facing=north]")
        .await?;
    t.frames(4).await?;
    t.check(t.block(a).await?.contains("shape=inner_left"), format!("an inner corner: {}", t.block(a).await?));
    let top = add(a, [0, 0, 3]);
    t.set(top, "base:stone_stairs[facing=east,half=top]")
        .await?;
    t.set(add(top, [1, 0, 0]), "base:stone_stairs[facing=south]")
        .await?;
    t.frames(4).await?;
    t.check(t.block(top).await?.contains("shape=straight"), "only with those of its own half");

    // They carry you up, and stop you where they stand.
    t.set(add(f, [-12, 0, 2]), "base:oak_planks_stairs[facing=north]")
        .await?;
    t.fill(add(f, [-12, 0, -4]), add(f, [-12, 0, 1]), "stone")
        .await?;
    t.c().teleport(centre(f, -11.5, 0.0, 4.5));
    t.c().set_look(0.0, 0.0);
    t.secs(0.3).await?;
    t.hold("core:forward", 1.4);
    t.secs(1.5).await?;
    let up = t.state().pos;
    t.check(up.y >= f[1] as f64 + 0.99, format!("up the stairs, one block: {up:?}"));
    let v = add(f, [-16, 0, 2]);
    t.set(v, "base:oak_planks_vertical_slab[facing=south]")
        .await?;
    t.set(add(v, [0, 1, 0]), "base:oak_planks_vertical_slab[facing=south]")
        .await?;
    t.c().teleport(centre(v, 0.5, 0.0, 2.5));
    t.c().set_look(0.0, 0.0);
    t.secs(0.3).await?;
    t.hold("core:forward", 1.2);
    t.secs(1.3).await?;
    let stopped = t.state().pos.z;
    t.check((stopped - (v[2] as f64 + 0.8)).abs() < 0.05, format!("up to the half it fills: z {stopped:.2}, it is at {}", v[2]));

    // One recipe for all of them.
    t.check_eq(
        t.op("craft 3 1 base:oak_planks base:oak_planks base:oak_planks")
            .await?,
        "base:oak_planks_slab 6".to_string(),
        "a slab's recipe",
    );
    t.check_eq(
        t.op("craft 2 2 base:bricks air base:bricks base:bricks")
            .await?,
        "base:bricks_stairs 4".to_string(),
        "a stair's",
    );
    t.check_eq(t.op("craft 1 1 base:bricks_slab").await?, "base:bricks_vertical_slab 1".to_string(), "from a slab, the vertical one");
    t.check_eq(t.op("craft 1 1 base:bricks_vertical_slab").await?, "base:bricks_slab 1".to_string(), "and back");
    Ok(())
}

/// Any blocks in one cell: placed next to each other where their shapes leave room, or when they
/// say they may; each broken by itself, drawn, solid and saved; a slab holds water, which flows
/// out but not through it; and a body takes all of a cell along.
pub async fn cell_parts(t: T) -> R {
    t.flat().await?;
    t.op("mode creative").await?;
    let f = t.feet();
    let centre = |p: [i32; 3], dx: f64, dy: f64, dz: f64| [p[0] as f64 + dx, p[1] as f64 + dy, p[2] as f64 + dz];
    let parts = |p: [i32; 3]| format!("parts {}", at(p));

    // Aimed at, each by itself.
    let cell = add(f, [0, 0, -3]);
    t.set(cell, "base:oak_planks_slab").await?;
    t.op(&format!("addpart {} base:sandstone_slab[type=top]", at(cell)))
        .await?;
    t.c().teleport(centre(cell, 0.5, 0.0, 2.5));
    t.secs(0.3).await?;
    aim(&t, centre(cell, 0.5, 0.8, 0.5));
    t.secs(0.2).await?;
    t.shot("two_slabs").await?;
    t.press("core:attack");
    t.secs(0.3).await?;
    let left = t.op(&parts(cell)).await?;
    t.check(left.contains("oak_planks_slab") && !left.contains("sandstone"), format!("the one aimed at goes: {left}"));
    // The other one, when what the cell stands for goes.
    t.op(&format!("addpart {} base:sandstone_slab[type=top]", at(cell)))
        .await?;
    t.frames(3).await?;
    aim(&t, centre(cell, 0.5, 0.2, 1.0));
    t.secs(0.2).await?;
    t.press("core:attack");
    t.secs(0.3).await?;
    t.check(t.block(cell).await?.contains("sandstone_slab"), format!("what is left stands for the cell: {}", t.block(cell).await?));
    t.set(cell, "air").await?;

    // Water in a slab, from a bucket: it stays a source and flows out sideways, not through.
    let pool = add(f, [6, 0, -6]);
    t.fill(add(pool, [-3, -2, -3]), add(pool, [3, -1, 3]), "stone")
        .await?;
    t.set(add(pool, [0, -1, 0]), "air").await?;
    t.set(pool, "base:stone_slab").await?;
    t.op("put me 0 base:water_bucket 1").await?;
    t.c().select_slot(0);
    t.c().teleport(centre(pool, 0.5, 0.0, 3.5));
    t.secs(0.3).await?;
    aim(&t, centre(pool, 0.5, 0.5, 0.5));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(2.0).await?;
    let wet = t.op(&parts(pool)).await?;
    t.check(wet.contains("stone_slab") && wet.contains("water"), format!("the slab holds the water: {wet}"));
    t.check(t.block(add(pool, [1, 0, 0])).await?.contains("water"), "it flows out sideways");
    t.check(!t.block(add(pool, [0, -1, 0])).await?.contains("water"), "and not through the slab under it");
    t.shot("slab_in_water").await?;
    t.c().teleport(centre(pool, 0.5, 0.2, 0.5));
    t.secs(0.2).await?;
    let in_it = t
        .op(&format!("in_liquid {} {} {}", pool[0] as f64 + 0.5, pool[1] as f64 + 0.7, pool[2] as f64 + 0.5))
        .await?;
    t.check_eq(in_it, "1".to_string(), "over the slab is in water");
    t.c().teleport(centre(pool, 0.5, 0.0, 4.5));
    t.secs(0.2).await?;
    // Broken, the water stays where it was, by itself.
    t.op(&format!("break {}", at(pool))).await?;
    t.frames(3).await?;
    t.check(t.block(pool).await?.contains("water"), format!("the water stays: {}", t.block(pool).await?));
    // A slab put in a source keeps it, and two of them leave no room.
    t.fill(add(pool, [-1, 0, -1]), add(pool, [1, 0, 1]), "air")
        .await?;
    t.set(pool, "base:water").await?;
    t.frames(2).await?;
    t.op("put me 1 base:stone_slab 8").await?;
    t.c().select_slot(1);
    t.fill(add(pool, [-1, 0, 0]), add(pool, [-1, 1, 0]), "stone")
        .await?;
    t.c().teleport(centre(pool, 0.5, 0.0, 3.5));
    t.secs(0.3).await?;
    aim(&t, centre(add(pool, [-1, 0, 0]), 1.0, 0.8, 0.5));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    let kept = t.op(&parts(pool)).await?;
    t.check(kept.contains("type=top") && kept.contains("water"), format!("placed in a source, it holds it: {kept}"));
    aim(&t, centre(add(pool, [-1, 0, 0]), 1.0, 0.2, 0.5));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    let full = t.op(&parts(pool)).await?;
    t.check(full.contains("type=double") && !full.contains("water"), format!("whole, it has no room for it: {full}"));
    aim(&t, centre(pool, 0.5, 0.8, 1.0));
    t.frames(3).await?;
    t.press("core:attack");
    t.secs(0.3).await?;
    let half = t.op(&parts(pool)).await?;
    t.check(half.contains("type=bottom"), format!("a hit takes the half aimed at: {half}"));

    // Stairs hold it too, from a bucket.
    let steps = add(pool, [3, 0, 0]);
    t.set(steps, "base:stone_stairs[facing=north]").await?;
    t.op("put me 0 base:water_bucket 1").await?;
    t.c().select_slot(0);
    t.c().teleport(centre(steps, 0.5, 0.0, 3.5));
    t.secs(0.3).await?;
    aim(&t, centre(steps, 0.5, 0.3, 1.0));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    let wet = t.op(&parts(steps)).await?;
    t.check(wet.contains("stone_stairs") && wet.contains("water"), format!("stairs hold water: {wet}"));
    // An empty bucket takes it back out, and only that: from above, over what flowed out of it.
    t.op("put me 0 base:bucket 1").await?;
    aim(&t, centre(steps, 0.5, 1.0, 0.25));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    let dry = t.op(&parts(steps)).await?;
    t.check(dry.contains("stone_stairs") && !dry.contains("water"), format!("a bucket takes it back: {dry}"));
    t.fill(add(pool, [2, 0, -1]), add(pool, [8, 0, 3]), "air")
        .await?;

    // A plant and a layer over it: their shapes overlap, and they said so.
    let bed = add(f, [-6, 0, -6]);
    t.set(add(bed, [0, -1, 0]), "base:grass").await?;
    t.set(bed, "base:rose").await?;
    t.op("put me 2 testmod:snow_layer 8").await?;
    t.c().select_slot(2);
    t.c().teleport(centre(bed, 0.5, 0.0, 2.5));
    t.secs(0.3).await?;
    aim(&t, centre(bed, 0.5, 0.4, 0.5));
    t.frames(3).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    let both = t.op(&parts(bed)).await?;
    t.check(both.contains("rose") && both.contains("snow_layer"), format!("a layer goes over a plant: {both}"));
    t.shot("snow_on_rose").await?;
    // Without its support each goes by itself; all of it here.
    t.set(add(bed, [0, -1, 0]), "air").await?;
    t.frames(4).await?;
    let gone = t.op(&parts(bed)).await?;
    t.check(!gone.contains("rose"), format!("the plant goes without soil: {gone}"));

    // Mods' callbacks of a part see it, not what the cell stands for: a stair in with a torch
    // still finds its corner, and the torch stays.
    let a = add(f, [6, 0, 6]);
    t.set(add(a, [0, -1, 0]), "stone").await?;
    t.set(a, "base:torch[facing=up]").await?;
    t.op(&format!("addpart {} base:stone_stairs[facing=east]", at(a)))
        .await?;
    t.set(add(a, [1, 0, 0]), "base:stone_stairs[facing=south]")
        .await?;
    t.frames(4).await?;
    let stair = t.op(&parts(a)).await?;
    t.check(stair.contains("torch") && stair.contains("shape=outer_right"), format!("a part keeps up with its neighbors: {stair}"));
    t.fill(a, add(a, [1, 0, 0]), "air").await?;
    // Any part shines, not only what the cell stands for.
    let lamp = add(f, [12, 0, 12]);
    t.set(lamp, "base:stone_slab[type=top]").await?;
    t.op(&format!("addpart {} base:torch[facing=up]", at(lamp)))
        .await?;
    t.frames(3).await?;
    let lit: u32 = t
        .op(&format!("light {}", at(lamp)))
        .await?
        .parse()
        .unwrap_or(0);
    t.check(lit >> 8 != 0, format!("a torch in with a slab lights the cell: {lit:x}"));
    t.set(lamp, "air").await?;

    // A piston moves all of a cell.
    let row = add(f, [-6, 0, 6]);
    t.fill(add(row, [0, -1, 0]), add(row, [4, -1, 0]), "stone")
        .await?;
    t.set(row, "base:piston[facing=east]").await?;
    t.set(add(row, [1, 0, 0]), "base:oak_planks_slab").await?;
    t.op(&format!("addpart {} base:sandstone_slab[type=top]", at(add(row, [1, 0, 0]))))
        .await?;
    t.set(add(row, [0, 0, 1]), "base:redstone_block").await?;
    t.secs(1.0).await?;
    let moved = t.op(&parts(add(row, [2, 0, 0]))).await?;
    t.check(moved.contains("oak_planks_slab") && moved.contains("sandstone_slab"), format!("both go together: {moved}"));
    t.frames(5).await?;
    t.c().teleport(centre(row, 2.5, 0.0, 3.5));
    t.secs(0.3).await?;
    aim(&t, centre(row, 2.5, 0.5, 0.5));
    t.secs(0.2).await?;
    t.shot("pushed_cell").await?;

    // It is all in the save.
    let name = format!("test-{}", t.name());
    t.c().world_leave();
    t.screen("core:title").await?;
    t.c().menu_open(&screens::WORLDS);
    t.screen("core:worlds").await?;
    t.frames(2).await?;
    t.click_ui(&format!("text:{name} |"), 0);
    t.secs(0.2).await?;
    t.click_ui("id:play", 0);
    t.until("joined again", std::time::Duration::from_secs(60), |t| t.flag(client::client_flags::JOINED))
        .await?;
    t.loaded().await?;
    let back = t.op(&parts(add(row, [2, 0, 0]))).await?;
    t.check(back.contains("oak_planks_slab") && back.contains("sandstone_slab"), format!("both come back from the save: {back}"));
    Ok(())
}

/// A block's shape is `hitbox`: what the crosshair sees and, if solid, what stops a walker.
pub async fn hitboxes(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let centre = |p: [i32; 3], dx: f64, dy: f64, dz: f64| [p[0] as f64 + dx, p[1] as f64 + dy, p[2] as f64 + dz];
    // A flower's is narrower than its picture: past it the crosshair finds what is behind.
    let row = add(f, [0, 0, -4]);
    t.set(add(row, [0, -1, 0]), "base:grass").await?;
    t.set(row, "base:rose").await?;
    t.set(add(row, [0, 0, -1]), "stone").await?;
    t.c().teleport(centre(row, 0.5, 0.0, 3.5));
    t.secs(0.3).await?;
    aim(&t, centre(row, 0.5, 0.3, 0.5));
    t.secs(0.2).await?;
    let seen = t.op("look").await?;
    t.check(seen.contains("rose"), format!("its middle is the flower: {seen}"));
    t.shot("flower_outline").await?;
    aim(&t, centre(row, 0.15, 0.3, 0.5));
    t.secs(0.2).await?;
    let seen = t.op("look").await?;
    t.check(seen.contains("stone"), format!("beside it, what is behind: {seen}"));
    // A cactus stands narrower than its cell, a walker gets that much closer.
    let wall = add(f, [6, 0, 0]);
    t.set(add(wall, [0, -1, 0]), "base:sand").await?;
    t.set(wall, "base:cactus").await?;
    t.c().teleport(centre(wall, 0.5, 0.0, 3.5));
    t.c().set_look(0.0, 0.0);
    t.secs(0.3).await?;
    t.hold("core:forward", 1.0);
    t.secs(1.1).await?;
    let z = t.state().pos.z - wall[2] as f64;
    t.check((z - (0.9375 + 0.3)).abs() < 0.05, format!("up to its box: {z:.3}"));
    Ok(())
}

/// Food: eating raises hunger, the base game's way for another mod's food too, and that mod's
/// component on it plus the engine's `item_used` event heals.
pub async fn food(t: T) -> R {
    t.flat().await?;
    let s = t.state();
    t.c().teleport([s.pos.x, s.pos.y + 8.0, s.pos.z]);
    t.until("falling", Duration::from_secs(5), |t| !t.flag(client::client_flags::ON_GROUND))
        .await?;
    t.until("landed", Duration::from_secs(10), |t| t.flag(client::client_flags::ON_GROUND))
        .await?;
    t.until("hurt", Duration::from_secs(3), |t| t.state().health < 20.0)
        .await?;
    let before = t.state().health;
    t.cmd("/feed 10").await?;
    t.op("put me 0 testmod:apple 3").await?;
    t.c().select_slot(0);
    t.until("hunger synced", Duration::from_secs(5), |t| {
        t.c()
            .view()
            .component(&HUNGER)
            .is_some_and(|h| h.food == 10.0)
    })
    .await?;
    t.secs(0.3).await?;
    t.hold("core:use", 2.0);
    t.until("ate", Duration::from_secs(5), |t| t.c().inventory_slot(0).count == 2)
        .await?;
    t.until("food went up", Duration::from_secs(5), |t| {
        t.c()
            .view()
            .component(&HUNGER)
            .is_some_and(|h| h.food == 14.0)
    })
    .await?;
    t.until("healed", Duration::from_secs(3), |t| t.state().health >= before + 2.0)
        .await?;
    let after = t.state().health;
    t.check(after <= before + 3.0, format!("health {before} -> {after}: two from testmod's food_heal (plus maybe a regen tick)"));
    Ok(())
}

/// Lava hurts the player, who keeps burning after getting out.
pub async fn burning(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let pool = add(f, [6, -1, 0]);
    t.fill(pool, add(pool, [1, 0, 1]), "lava").await?;
    t.c()
        .teleport([pool[0] as f64 + 0.5, pool[1] as f64 + 0.1, pool[2] as f64 + 0.5]);
    t.until("hurt by lava", Duration::from_secs(5), |t| t.state().health <= 16.0)
        .await?;
    t.c()
        .teleport([f[0] as f64 + 0.5, f[1] as f64, f[2] as f64 + 0.5]);
    t.secs(0.5).await?;
    let out = t.state().health;
    t.secs(2.5).await?;
    let later = t.state().health;
    t.check(later < out, format!("still burning after leaving lava: health {out} -> {later}"));
    Ok(())
}

static ON_FIRE: Component<u8> = Component::named("core:burning");

/// Fields the engine keeps on entities reach clients like any synced component: a pig in lava
/// shows on fire and losing health, and stays for its death animation once the server drops it.
pub async fn entity_fields(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    t.fill(add(f, [3, 0, -1]), add(f, [5, 1, 1]), "stone")
        .await?;
    t.fill(add(f, [4, 0, 0]), add(f, [4, 1, 0]), "air").await?;
    t.fill(add(f, [4, -2, 0]), add(f, [4, -1, 0]), "lava")
        .await?;
    let id = t
        .op(&format!("spawn pig {}", at(add(f, [4, -1, 0]))))
        .await?;
    let id: u64 = id.trim().parse().map_err(|_| format!("spawn pig: {id}"))?;
    t.until("the pig burns on the client", Duration::from_secs(3), |t| t.c().view().component_of(id, &ON_FIRE) == Some(1))
        .await?;
    t.until("the pig loses health on the client", Duration::from_secs(3), |t| t.c().entity(id).is_some_and(|e| e.health < e.max_health))
        .await?;
    for _ in 0..400 {
        if t.op(&format!("entity {id}")).await.is_err() {
            t.check(t.c().entity(id).is_some(), "the killed pig stays on the client for its death animation");
            return Ok(());
        }
    }
    Err("the pig did not die in lava".into())
}

/// Respawning: every player has variable-sized components (hunger's timer), which once crashed
/// the server on any death. What belongs to the body starts over (hunger, effects); mods' timers
/// go on, or hunger's pulse, armed once on joining, would stop for good.
pub async fn respawn(t: T) -> R {
    t.flat().await?;
    t.op("mode survival").await?;
    for with_effect in [false, true] {
        t.cmd("/feed 5").await?;
        t.until("food down", Duration::from_secs(5), |t| {
            t.c()
                .view()
                .component(&HUNGER)
                .is_some_and(|h| h.food <= 5.0)
        })
        .await?;
        if with_effect {
            t.op("attr_mod me core:movement_speed testkit:fast 1 1 0")
                .await?;
        }
        t.cmd("/clear @s").await?;
        t.screen("core:death").await?;
        t.c().respawn();
        t.no_screen().await?;
        t.until("hunger starts over", Duration::from_secs(5), |t| {
            t.c()
                .view()
                .component(&HUNGER)
                .is_some_and(|h| h.food == 20.0)
        })
        .await?;
        let speed = t.op("attr me core:movement_speed").await?;
        t.check(!speed.contains("testkit:fast"), format!("no effect after respawning: {speed}"));
        t.check_eq(t.op("health").await?, "20".to_string(), "full health again");
    }
    // Starving takes health at hunger's next pulse (every 4 seconds).
    t.cmd("/feed 0").await?;
    t.secs(4.5).await?;
    let health: f32 = t.op("health").await?.parse().unwrap_or(20.0);
    t.check(health < 20.0, format!("hunger's pulse still runs: health {health}"));
    Ok(())
}

/// Placing a block over a plant replaces it and drops what it would when broken, except in
/// creative.
pub async fn replaced_plants_drop(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let flower = add(f, [2, 0, 0]);
    t.op("put me 0 stone 2").await?;
    t.c().select_slot(0);
    for creative in [false, true] {
        if creative {
            t.op("mode creative").await?;
        }
        t.set(flower, "dandelion").await?;
        t.until("the flower on the client", Duration::from_secs(2), |t| t.c().block(flower).is_some_and(|s| s != 0))
            .await?;
        t.look_at(flower);
        t.frames(2).await?;
        t.press("core:use");
        t.until("stone over the flower", Duration::from_secs(2), |t| {
            t.c()
                .block(flower)
                .is_some_and(|s| Block::named("base:stone").is(s))
        })
        .await?;
        t.secs(0.2).await?;
        let near = t
            .op(&format!("near {} {} {} 2", flower[0], flower[1], flower[2]))
            .await?;
        if creative {
            t.check(!near.contains("core:item"), format!("nothing drops in creative: {near}"));
        } else {
            let d = t.op("inv me").await?;
            t.check(near.contains("core:item") || d.contains("dandelion"), format!("the flower dropped: near '{near}', inventory '{d}'"));
        }
        t.set(flower, "air").await?;
        t.secs(0.5).await?;
        t.cmd("/clear @e[type=core:item]").await?;
    }
    Ok(())
}

/// Sprinting under water swims in a low box at about 3.9 blocks a second, where wading does 2;
/// lava is not swum in, unless something worn says so (`swims_in`, testkit's on a chestplate).
pub async fn swimming(t: T) -> R {
    t.flat().await?;
    t.op("mode creative").await?;
    let f = t.feet();
    let (a, b) = (add(f, [-8, 0, -40]), add(f, [8, 12, 8]));
    let pool = |name: &'static str| {
        let t = t.clone();
        async move { t.fill(a, b, name).await }
    };
    let sprint_speed = |t: T, pitch: f32| async move {
        let s = t.state().pos;
        t.c().teleport([s.x, f[1] as f64 + 8.0, f[2] as f64 + 0.5]);
        t.secs(0.3).await?;
        t.c().set_look(0.0, pitch);
        t.hold("core:sprint", 2.0);
        t.hold("core:forward", 2.0);
        t.secs(1.0).await?;
        let from = t.state().pos;
        t.secs(0.8).await?;
        let to = t.state().pos;
        t.secs(1.0).await?;
        Ok::<(f64, f64), String>((((to.x - from.x).powi(2) + (to.z - from.z).powi(2)).sqrt() / 0.8, to.y - from.y))
    };
    pool("water").await?;
    t.secs(0.5).await?;
    let (swim, _) = sprint_speed(t.clone(), 0.0).await?;
    t.note(format!("sprinting under water: {swim:.2} blocks/s"));
    t.check((3.3..4.6).contains(&swim), format!("swimming speed {swim:.2}, vanilla's code gives 3.9"));
    let (_, down) = sprint_speed(t.clone(), -60.0).await?;
    t.note(format!("looking down, moved {down:.2} in y"));
    t.check(down < -0.3, format!("swimming goes where it looks: y {down:.2}"));
    pool("lava").await?;
    t.secs(0.5).await?;
    let (wade, _) = sprint_speed(t.clone(), 0.0).await?;
    t.note(format!("sprinting in lava: {wade:.2} blocks/s"));
    t.check(wade < 1.8, format!("no swimming in lava: {wade:.2} blocks/s"));
    t.op("put me 37 base:diamond_chestplate").await?;
    t.secs(0.2).await?;
    let (suit, _) = sprint_speed(t.clone(), 0.0).await?;
    t.note(format!("with the suit: {suit:.2} blocks/s"));
    t.check(suit > wade * 1.3, format!("the suit swims in lava: {wade:.2} -> {suit:.2}"));
    t.op("put me 37 base:diamond_chestplate 0").await?;
    pool("air").await?;
    Ok(())
}

/// Modern combat: a hit right after the last does a fraction, a charged one all of it, and a
/// stronger hit during the target's recovery still adds the difference.
pub async fn attack_charge(t: T) -> R {
    t.flat().await?;
    t.op("put me 0 iron_sword 1").await?;
    t.c().select_slot(0);
    let f = t.feet();
    let spot = add(f, [0, 0, -2]);
    // Walled in with the player, or it would run off from the first hit.
    for (from, to) in [([-1, 0, -3], [-1, 2, 1]), ([1, 0, -3], [1, 2, 1]), ([0, 0, -3], [0, 2, -3]), ([0, 0, 1], [0, 2, 1])] {
        t.fill(add(f, from), add(f, to), "stone").await?;
    }
    let id = t
        .op(&format!("spawn base:cow {} {} {}", spot[0], spot[1], spot[2]))
        .await?;
    let health = |t: T, id: String| async move {
        let s = t.op(&format!("entity {id}")).await?;
        Ok::<f32, String>(
            s.split_whitespace()
                .last()
                .and_then(|v| v.parse().ok())
                .unwrap_or(-1.0),
        )
    };
    t.look_at(spot);
    // The sword just came to hand: nearly no charge, and the hand comes up over the next second.
    t.secs(0.2).await?;
    t.c().select_slot(1);
    t.c().select_slot(0);
    t.frames(2).await?;
    let before = health(t.clone(), id.clone()).await?;
    t.press("core:attack");
    t.secs(0.15).await?;
    let weak = before - health(t.clone(), id.clone()).await?;
    t.note(format!("uncharged: {weak:.2}"));
    t.check((0.1..2.5).contains(&weak), format!("a hit with no charge does little: {weak:.2} of 6"));
    t.secs(1.5).await?;
    let before = health(t.clone(), id.clone()).await?;
    t.press("core:attack");
    t.secs(0.15).await?;
    let full = before - health(t.clone(), id.clone()).await?;
    t.note(format!("charged: {full:.2}"));
    t.check((5.5..6.5).contains(&full), format!("a charged iron sword does 6: {full:.2}"));
    Ok(())
}

/// One level for everything: below what a command asks for it refuses and isn't offered, and a
/// player can't give out more than it has.
pub async fn permissions(t: T) -> R {
    t.flat().await?;
    t.check(t.cmd("/time query").await?.contains("time is"), "the host may do it all");
    t.op("put me 0 stone 1").await?;
    t.c().select_slot(0);
    t.secs(0.1).await?;
    t.cmd("/op @s 0").await?;
    let no = t.cmd("/time set 1000").await?;
    t.check(no.contains("permission level 2"), format!("/time at level 0: {no}"));
    t.cmd("/rename shiny").await?;
    t.until("a command for everyone still works", Duration::from_secs(2), |t| t.c().inventory_slot(0).data != 0)
        .await?;
    let up = t.cmd("/op @s 4").await?;
    t.check(up.contains("permission level 3"), format!("/op from level 0: {up}"));
    Ok(())
}

/// Every item and mob of the base game has its name in every language it comes with.
pub async fn names_translated(t: T) -> R {
    // From a process of several, the run folder is deeper down.
    let run = t.c().run_dir();
    let Some(game) = std::path::Path::new(&run)
        .ancestors()
        .find(|d| d.join("mods/base/assets/lang").is_dir())
    else {
        return Err("no base mod sources".into());
    };
    let root = game.join("mods/base/assets/lang").display().to_string();
    for lang in ["en_us", "ru_ru"] {
        let text = std::fs::read_to_string(format!("{root}/{lang}.lang")).map_err(|e| e.to_string())?;
        let has = |key: String| {
            text.lines()
                .any(|l| l.split(" = ").next() == Some(key.as_str()))
        };
        let mut missing = Vec::new();
        for id in 1.. {
            let name = item_name(id);
            if name.is_empty() {
                break;
            }
            if let Some(n) = name
                .strip_prefix("base:")
                // Named after what hatches from them, or what they are made of.
                .filter(|n| {
                    !["_spawn_egg", "_stairs", "_slab", "_beam", "_eighth"]
                        .iter()
                        .any(|end| n.ends_with(end))
                })
            {
                if !has(format!("item.base.{n}")) && !has(format!("block.base.{n}")) {
                    missing.push(name.to_string());
                }
            }
        }
        for id in 0.. {
            let name = name_of(weft_sdk::abi::entry_kind::ENTITY, id);
            if name.is_empty() {
                break;
            }
            if let Some(n) = name.strip_prefix("base:") {
                // Machines' entities are called what their block is.
                if !has(format!("entity.base.{n}")) && !has(format!("block.base.{n}")) {
                    missing.push(name.to_string());
                }
            }
        }
        t.check(missing.is_empty(), format!("no name in {lang}: {}", missing.join(" ")));
    }
    Ok(())
}

/// With every other mod turned off the game still starts, whatever they brought (no mod but this
/// one has settings of its own to lean on).
pub async fn base_alone(t: T) -> R {
    use std::io::Write;
    let port = 30000 + (std::process::id() % 20000) as u16 + 1;
    let dir = format!("{}/test-base-alone", t.c().run_dir());
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(format!("{dir}/config")).map_err(|e| e.to_string())?;
    std::fs::write(format!("{dir}/config/disabled_mods.txt"), "testkit\ntestmod").map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut server = std::process::Command::new(exe)
        .args(["--server", "--world", "w", "--seed", "1", "--view", "4", "--port", &port.to_string(), "--run-dir", &dir])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let up = t
        .until("the game up with only base", Duration::from_secs(30), |_| std::net::TcpStream::connect(("127.0.0.1", port)).is_ok())
        .await;
    if up.is_err() {
        let _ = server.kill();
        return up;
    }
    writeln!(server.stdin.take().ok_or("no console")?, "stop").map_err(|e| e.to_string())?;
    t.until("it stops", Duration::from_secs(20), |_| server.try_wait().ok().flatten().is_some())
        .await?;
    t.check(
        server
            .try_wait()
            .ok()
            .flatten()
            .is_some_and(|s| s.success()),
        "stopped cleanly",
    );
    Ok(())
}

/// A game with no window, reached by address: nobody owns it by connecting first, `op` from its
/// console makes someone more, its whitelist keeps out who isn't on it, `stop` saves and leaves.
pub async fn dedicated_server(t: T) -> R {
    use std::io::Write;
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
        }
    }
    let port = 30000 + (std::process::id() % 20000) as u16;
    let dir = format!("{}/test-server", t.c().run_dir());
    let _ = std::fs::remove_dir_all(&dir);
    static SPARKS: Config<u32> = Config::named("testmod:wisp_sparks");
    let own_sparks = SPARKS.get();
    std::fs::create_dir_all(format!("{dir}/config")).map_err(|e| e.to_string())?;
    std::fs::write(format!("{dir}/config/testmod.txt"), format!("wisp_sparks = {}\n", own_sparks + 3)).map_err(|e| e.to_string())?;
    let start = || -> Result<Child, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        std::process::Command::new(exe)
            .args(["--server", "--world", "w", "--seed", "1", "--view", "4", "--port", &port.to_string(), "--run-dir", &dir])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(Child)
            .map_err(|e| e.to_string())
    };
    let listening = |t: &T| {
        let _ = t;
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
    };
    let mut server = start()?;
    t.until("the server listening", Duration::from_secs(30), listening)
        .await?;
    t.c().world_join(&format!("127.0.0.1:{port}"));
    t.until("joined by address", Duration::from_secs(30), |t| t.flag(client::client_flags::JOINED))
        .await?;
    t.loaded().await?;
    t.check_eq(SPARKS.get(), own_sparks + 3, "the server's setting counts while on it");
    let refused = t.cmd("/time query").await?;
    t.check(!refused.contains("time is"), format!("the first to connect owns nothing: {refused}"));
    static NAME: TextConfig = TextConfig::named("core:name");
    let mut console = server.0.stdin.take().ok_or("no console")?;
    writeln!(console, "op {}", NAME.get()).map_err(|e| e.to_string())?;
    let _ = console.flush();
    t.secs(0.5).await?;
    let allowed = t.cmd("/time query").await?;
    t.check(allowed.contains("time is"), format!("op from the console: {allowed}"));
    t.c().world_leave();
    t.screen("core:title").await?;
    t.check_eq(SPARKS.get(), own_sparks, "the player's own setting after leaving");
    writeln!(console, "stop").map_err(|e| e.to_string())?;
    let _ = console.flush();
    t.until("the server stopped", Duration::from_secs(20), |_| server.0.try_wait().ok().flatten().is_some())
        .await?;
    t.check(std::fs::read_dir(format!("{dir}/saves/w/players")).is_ok_and(|mut d| d.next().is_some()), "the player was saved");

    std::fs::create_dir_all(format!("{dir}/config")).map_err(|e| e.to_string())?;
    std::fs::write(format!("{dir}/config/whitelist.txt"), "somebody else\n").map_err(|e| e.to_string())?;
    std::fs::write(format!("{dir}/config/disabled_mods.txt"), "").map_err(|e| e.to_string())?;
    let _server = start()?;
    t.until("the server listening again", Duration::from_secs(30), listening)
        .await?;
    t.c().world_join(&format!("127.0.0.1:{port}"));
    t.screen("core:message").await?;
    t.check(!t.flag(client::client_flags::IN_WORLD), "not on the whitelist, not in");
    t.c().menu_close();
    Ok(())
}
