//! Redstone: what each part does, and the rule that matters most, that a build behaves the same
//! whichever way it is turned.

use std::time::Duration;

use weft_sdk::*;

use super::{add, probe};
use crate::runner::{R, T};

const EAST: u8 = 1;
const DOWN: u8 = 2;
const UP: u8 = 3;
const NORTH: u8 = 4;
const SOUTH: u8 = 5;
const NAMES: [&str; 6] = ["west", "east", "down", "up", "north", "south"];
const VECS: [[i32; 3]; 6] = [[-1, 0, 0], [1, 0, 0], [0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1]];

fn wire(faces: &[u8]) -> String {
    let sides: Vec<String> = faces
        .iter()
        .map(|&f| format!("{}=true", NAMES[f as usize]))
        .collect();
    format!("base:redstone[{}]", sides.join(","))
}

/// The sides a state of wire lies on, a bit per face.
fn wire_sides(state: u32) -> u8 {
    (0..6u8)
        .filter(|&f| state_prop(state, NAMES[f as usize]) == Some("true"))
        .fold(0, |m, f| m | 1 << f)
}

fn on_floor(name: &str, front: u8, rest: &str) -> String {
    format!("base:{name}[attach=down,front={}{rest}]", NAMES[front as usize])
}

fn lever(on: bool) -> String {
    on_floor("lever", NORTH, if on { ",power=on" } else { ",power=off" })
}

async fn lit(t: &T, p: [i32; 3]) -> Result<bool, String> {
    Ok(t.block(p).await?.contains("power=on"))
}

/// Lets a circuit settle: some ticks, more than any delay used here.
async fn settle(t: &T) -> R {
    t.secs(0.6).await
}

async fn run_of_wire(t: &T, f: [i32; 3], from: i32, to: i32, z: i32) -> R {
    for x in from..=to {
        t.set(add(f, [x, 0, z]), &wire(&[DOWN])).await?;
    }
    Ok(())
}

/// Power gets fifteen blocks along a wire and stops, and only while something gives it.
pub async fn redstone_wire(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let z = -3;
    t.set(add(f, [3, 0, z]), &lever(true)).await?;
    run_of_wire(&t, f, 4, 19, z).await?;
    let (near, far) = (add(f, [18, 0, z - 1]), add(f, [19, 0, z - 1]));
    t.set(near, "base:redstone_lamp").await?;
    t.set(far, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(lit(&t, near).await?, "the fifteenth block of wire has power");
    t.check(!lit(&t, far).await?, "the sixteenth hasn't");
    t.set(add(f, [3, 0, z]), &lever(false)).await?;
    settle(&t).await?;
    t.check(!lit(&t, near).await?, "off with the lever");
    Ok(())
}

/// Wire goes from the floor up a wall, across the ceiling, and around an outer edge.
pub async fn redstone_on_walls(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let z = -4;
    t.fill(add(f, [5, 0, z]), add(f, [5, 3, z]), "base:stone")
        .await?;
    t.set(add(f, [4, 3, z]), "base:stone").await?;
    t.set(add(f, [2, 0, z]), &lever(true)).await?;
    t.set(add(f, [3, 0, z]), &wire(&[DOWN])).await?;
    t.set(add(f, [4, 0, z]), &wire(&[DOWN, EAST])).await?;
    let middle = add(f, [4, 1, z]);
    t.set(middle, &wire(&[EAST])).await?;
    t.set(add(f, [4, 2, z]), &wire(&[EAST, UP])).await?;
    let lamp = add(f, [3, 2, z]);
    t.set(lamp, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(lit(&t, lamp).await?, "across floor, wall and ceiling");
    t.set(middle, "air").await?;
    settle(&t).await?;
    t.check(!lit(&t, lamp).await?, "cut in the middle");

    // Around the top edge of a block, with no wire on any step.
    let z = -8;
    t.set(add(f, [5, 0, z]), "base:stone").await?;
    t.set(add(f, [2, 0, z]), &lever(true)).await?;
    t.set(add(f, [3, 0, z]), &wire(&[DOWN])).await?;
    t.set(add(f, [4, 0, z]), &wire(&[DOWN, EAST])).await?;
    t.set(add(f, [5, 1, z]), &wire(&[DOWN])).await?;
    let lamp = add(f, [6, 1, z]);
    t.set(lamp, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(lit(&t, lamp).await?, "around an outer edge");
    Ok(())
}

/// Blocks carry a lever's power on to wire, a lamp too, except blockers; wire on a block powers a
/// device next to it but not other wire.
pub async fn redstone_conduction(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let blocks = [
        (-3, "base:stone", true),
        (-6, "base:glass", false),
        (-9, "base:oak_planks", true),
        (-15, "base:redstone_lamp", true),
        (-18, "base:golden_lamp", false),
    ];
    for (z, block, carries) in blocks {
        t.set(add(f, [4, 0, z]), block).await?;
        t.set(add(f, [3, 0, z]), &format!("base:lever[attach=east,front=north,power=on]"))
            .await?;
        t.set(add(f, [5, 0, z]), &wire(&[DOWN])).await?;
        let lamp = add(f, [6, 0, z]);
        t.set(lamp, "base:redstone_lamp").await?;
        settle(&t).await?;
        t.check_eq(lit(&t, lamp).await?, carries, &format!("through {block}"));
    }
    // Wire on top of a raised block, and under it, which no edge joins to the top.
    let z = -12;
    t.set(add(f, [3, 1, z]), "base:stone").await?;
    t.set(add(f, [4, 1, z]), "base:stone").await?;
    t.set(add(f, [3, 2, z]), &lever(true)).await?;
    t.set(add(f, [4, 2, z]), &wire(&[DOWN])).await?;
    let device = add(f, [4, 1, z + 1]);
    t.set(device, "base:redstone_lamp").await?;
    t.set(add(f, [4, 0, z]), &wire(&[UP])).await?;
    let other = add(f, [5, 0, z]);
    t.set(other, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(lit(&t, device).await?, "a block with wire on it powers a device next to it");
    t.check(!lit(&t, other).await?, "but not other wire");
    Ok(())
}

/// A torch is off while its block has power, and goes on a wall as well as on a floor.
pub async fn redstone_torch(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let z = -3;
    t.set(add(f, [4, 0, z]), "base:stone").await?;
    t.set(add(f, [4, 1, z]), "base:redstone_torch[attach=down]")
        .await?;
    let lamp = add(f, [5, 1, z]);
    t.set(lamp, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(lit(&t, lamp).await?, "a torch gives power");
    t.set(add(f, [3, 0, z]), "base:lever[attach=east,front=north,power=on]")
        .await?;
    settle(&t).await?;
    t.check(!lit(&t, lamp).await?, "a torch on a powered block doesn't");
    // Against a wall: the lamp is beyond it, on the side away from the wall.
    let z = -7;
    t.set(add(f, [4, 1, z]), "base:stone").await?;
    t.set(add(f, [5, 1, z]), "base:redstone_torch[attach=west]")
        .await?;
    let lamp = add(f, [6, 1, z]);
    t.set(lamp, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(lit(&t, lamp).await?, "on a wall");
    // A tower: a torch powers the block above it, which turns off a torch on top of that.
    let z = -11;
    t.set(add(f, [4, 0, z]), "base:stone").await?;
    t.set(add(f, [4, 1, z]), "base:redstone_torch[attach=down]")
        .await?;
    t.set(add(f, [4, 2, z]), "base:stone").await?;
    t.set(add(f, [4, 3, z]), "base:redstone_torch[attach=down]")
        .await?;
    let lamp = add(f, [5, 3, z]);
    t.set(lamp, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(!lit(&t, lamp).await?, "a torch powers the block above it");
    // On a wall it powers the block in front and the one above, through to what is past them.
    let z = -15;
    t.set(add(f, [4, 1, z]), "base:stone").await?;
    t.set(add(f, [5, 1, z]), "base:redstone_torch[attach=west]")
        .await?;
    t.set(add(f, [6, 1, z]), "base:stone").await?;
    t.set(add(f, [5, 2, z]), "base:stone").await?;
    let (ahead, over) = (add(f, [7, 1, z]), add(f, [5, 3, z]));
    t.set(ahead, "base:redstone_lamp").await?;
    t.set(over, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(lit(&t, ahead).await?, "through the block in front");
    t.check(lit(&t, over).await?, "and the one above");
    Ok(())
}

/// A repeater takes as long as its delay, one way only; it can be locked from the side.
pub async fn redstone_repeater(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let z = -3;
    let (switch, lamp) = (add(f, [3, 0, z]), add(f, [6, 0, z]));
    t.set(switch, &lever(false)).await?;
    t.set(add(f, [4, 0, z]), &wire(&[DOWN])).await?;
    t.set(add(f, [5, 0, z]), &on_floor("repeater", EAST, ",delay=4"))
        .await?;
    t.set(lamp, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(!lit(&t, lamp).await?, "nothing yet");
    t.set(switch, &lever(true)).await?;
    let start = std::time::Instant::now();
    t.until("the lamp on", Duration::from_secs(3), |t| {
        t.c()
            .block(lamp)
            .is_some_and(|s| state_prop(s, "power") == Some("on"))
    })
    .await?;
    let took = start.elapsed().as_secs_f32();
    t.probe("seconds through a delay of four", took, (0.25..=1.2).contains(&took));

    // Not backward: a lever at its front does nothing behind it.
    let z = -6;
    let behind = add(f, [4, 0, z]);
    t.set(behind, "base:redstone_lamp").await?;
    t.set(add(f, [5, 0, z]), &on_floor("repeater", EAST, ""))
        .await?;
    t.set(add(f, [6, 0, z]), &lever(true)).await?;
    settle(&t).await?;
    t.check(!lit(&t, behind).await?, "one way");

    // Locked from the side by a repeater pointing into it.
    let z = -10;
    let (input, lamp) = (add(f, [3, 0, z]), add(f, [5, 0, z]));
    t.set(input, &lever(false)).await?;
    t.set(add(f, [4, 0, z]), &on_floor("repeater", EAST, ""))
        .await?;
    t.set(lamp, "base:redstone_lamp").await?;
    t.set(add(f, [4, 0, z - 1]), &on_floor("repeater", SOUTH, ""))
        .await?;
    t.set(add(f, [4, 0, z - 2]), &lever(true)).await?;
    settle(&t).await?;
    t.set(input, &lever(true)).await?;
    settle(&t).await?;
    t.check(!lit(&t, lamp).await?, "locked");
    Ok(())
}

/// A comparator compares, subtracts, and reads how full a chest is.
pub async fn redstone_comparator(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    for (z, mode, want) in [(-3, "compare", true), (-7, "subtract", false)] {
        t.set(add(f, [3, 0, z]), &lever(true)).await?;
        t.set(add(f, [4, 0, z]), &on_floor("comparator", EAST, &format!(",mode={mode}")))
            .await?;
        let lamp = add(f, [5, 0, z]);
        t.set(lamp, "base:redstone_lamp").await?;
        t.set(add(f, [4, 0, z - 1]), &lever(true)).await?;
        settle(&t).await?;
        t.check_eq(t.block(lamp).await?.contains("power=on"), want, &format!("{mode}, side as strong as the rear"));
    }
    let z = -11;
    let chest = add(f, [3, 0, z]);
    t.set(chest, "base:chest").await?;
    t.set(add(f, [4, 0, z]), &on_floor("comparator", EAST, ""))
        .await?;
    let lamp = add(f, [5, 0, z]);
    t.set(lamp, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(!lit(&t, lamp).await?, "an empty chest gives nothing");
    t.op(&format!("put {} {} {} 0 cobblestone 64", chest[0], chest[1], chest[2]))
        .await?;
    settle(&t).await?;
    t.check(lit(&t, lamp).await?, "a chest with something in it does");
    Ok(())
}

/// A mod's gate reads the faces it names and only those: power behind it does nothing.
pub async fn redstone_gate(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let z = -4;
    let gate = add(f, [4, 0, z]);
    let lamp = add(f, [5, 0, z]);
    t.set(gate, &format!("testmod:and_gate[attach=down,front={}]", NAMES[EAST as usize]))
        .await?;
    t.set(lamp, "base:redstone_lamp").await?;
    t.set(add(f, [3, 0, z]), &lever(true)).await?;
    t.set(add(f, [4, 0, z - 1]), &lever(true)).await?;
    settle(&t).await?;
    t.check(!lit(&t, lamp).await?, "the rear and one side are not both sides");
    t.set(add(f, [4, 0, z + 1]), &lever(true)).await?;
    settle(&t).await?;
    t.check(lit(&t, lamp).await?, "both sides");
    Ok(())
}

/// A piston pushes, a sticky one pulls back, and two wanting one block both give way.
pub async fn redstone_piston(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let z = -4;
    let (switch, piston, block) = (add(f, [2, 0, z]), add(f, [3, 0, z]), add(f, [4, 0, z]));
    t.set(switch, &lever(false)).await?;
    t.set(piston, "base:piston[facing=east]").await?;
    t.set(block, "base:cobblestone").await?;
    settle(&t).await?;
    t.set(switch, &lever(true)).await?;
    settle(&t).await?;
    t.check(t.block(add(f, [5, 0, z])).await?.contains("cobblestone"), "pushed");
    t.check(t.block(block).await?.contains("piston_head"), "the head out");
    t.check(t.block(piston).await?.contains("power=on"), "the piston out");
    t.set(switch, &lever(false)).await?;
    settle(&t).await?;
    t.check(t.block(add(f, [5, 0, z])).await?.contains("cobblestone"), "a plain one leaves it");
    t.check(t.block(block).await?.contains("air"), "the head in");

    let z = -8;
    let (switch, block, moved) = (add(f, [2, 0, z]), add(f, [4, 0, z]), add(f, [5, 0, z]));
    t.set(switch, &lever(false)).await?;
    t.set(add(f, [3, 0, z]), "base:sticky_piston[facing=east]")
        .await?;
    t.set(block, "base:cobblestone").await?;
    settle(&t).await?;
    t.set(switch, &lever(true)).await?;
    settle(&t).await?;
    t.check(t.block(moved).await?.contains("cobblestone"), "pushed");
    t.set(switch, &lever(false)).await?;
    settle(&t).await?;
    t.check(t.block(block).await?.contains("cobblestone"), "a sticky one brings it back");

    // Face to face over one block, powered together by one wire along them.
    let z = -12;
    let switch = add(f, [3, 0, z - 1]);
    t.set(switch, &lever(false)).await?;
    run_of_wire(&t, f, 4, 6, z - 1).await?;
    t.set(add(f, [4, 0, z]), "base:piston[facing=east]").await?;
    t.set(add(f, [5, 0, z]), "base:cobblestone").await?;
    t.set(add(f, [6, 0, z]), "base:piston[facing=west]").await?;
    settle(&t).await?;
    t.set(switch, &lever(true)).await?;
    settle(&t).await?;
    t.check(t.block(add(f, [5, 0, z])).await?.contains("cobblestone"), "it stays where it is");
    t.check(t.block(add(f, [4, 0, z])).await?.contains("power=off"), "neither moves");
    t.check(t.block(add(f, [6, 0, z])).await?.contains("power=off"), "neither moves");

    // What can't be moved stops it, and what isn't solid is broken.
    let z = -16;
    t.set(add(f, [4, 0, z]), "base:piston[facing=east]").await?;
    t.set(add(f, [5, 0, z]), "base:obsidian").await?;
    t.set(add(f, [4, 0, z - 1]), "base:redstone_block").await?;
    settle(&t).await?;
    t.check(t.block(add(f, [4, 0, z])).await?.contains("power=off"), "obsidian stays");
    Ok(())
}

/// Pushed blocks move as a body: whoever stands on one rides along without a jump. Slime takes
/// what touches it along. Breaking the head breaks the piston, for good.
pub async fn redstone_piston_moves(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    // A block under the feet, a piston behind it, the ground around gone.
    let (switch, piston) = (add(f, [-2, -1, 0]), add(f, [-1, -1, 0]));
    t.set(add(f, [0, -1, 0]), "base:cobblestone").await?;
    t.fill(add(f, [1, -1, -3]), add(f, [6, -1, 3]), "air")
        .await?;
    t.fill(add(f, [-3, -1, 1]), add(f, [0, -1, 3]), "air")
        .await?;
    t.fill(add(f, [-3, -1, -3]), add(f, [0, -1, -1]), "air")
        .await?;
    t.set(piston, "base:piston[facing=east]").await?;
    t.set(switch, &lever(false)).await?;
    t.secs(0.5).await?;
    // What the player sees: where they are shown, frame by frame.
    let steps = |t: &T, frames: u32| {
        let (t, mut last, mut largest) = (t.clone(), t.state().shown.x, 0.0f64);
        async move {
            // What a step has beyond the speed it goes at: a long frame makes a long step too.
            let mut frame = std::time::Instant::now();
            for _ in 0..frames {
                t.frames(1).await?;
                let dt = std::mem::replace(&mut frame, std::time::Instant::now())
                    .elapsed()
                    .as_secs_f64();
                let x = t.state().shown.x;
                largest = largest.max((x - last).abs() - 10.0 * dt);
                last = x;
            }
            Ok::<_, String>(largest)
        }
    };
    let start = t.state().pos.x;
    t.set(switch, &lever(true)).await?;
    let largest = steps(&t, 150).await?;
    let went = t.state().pos.x - start;
    t.probe("carried along", went as f32, (0.9..1.1).contains(&went));
    t.probe("largest step of a frame when carried", largest as f32, largest < 0.15);
    t.check(t.block(add(f, [1, -1, 0])).await?.contains("cobblestone"), "the block got there");
    t.fill(add(f, [-3, -2, -3]), add(f, [6, 2, 3]), "air")
        .await?;
    t.fill(add(f, [-3, -2, -3]), add(f, [8, -2, 3]), "base:stone")
        .await?;

    // Shoved by a block coming at it from the side.
    t.cmd(&format!("/tp {}.5 {} {}.5", f[0] + 2, f[1] - 1, f[2]))
        .await?;
    t.secs(0.5).await?;
    let (switch, piston) = (add(f, [-1, -1, 0]), add(f, [0, -1, 0]));
    t.set(piston, "base:piston[facing=east]").await?;
    t.set(add(f, [1, -1, 0]), "base:cobblestone").await?;
    t.set(switch, &lever(false)).await?;
    t.secs(0.3).await?;
    let start = t.state().pos.x;
    t.set(switch, &lever(true)).await?;
    let largest = steps(&t, 150).await?;
    let went = t.state().pos.x - start;
    t.probe("shoved out of the way", went as f32, went > 0.2);
    t.probe("largest step of a frame when shoved", largest as f32, largest < 0.15);
    t.cmd(&format!("/tp {} {} {}", f[0], f[1] + 12, f[2]))
        .await?;
    t.c().set_flying(true);
    t.secs(0.3).await?;

    // Slime brings the block touching it, not what it can't move.
    let z = 8;
    t.set(add(f, [0, 3, z]), "base:sticky_piston[facing=east]")
        .await?;
    t.set(add(f, [1, 3, z]), "base:slime_block").await?;
    t.set(add(f, [1, 3, z + 1]), "base:cobblestone").await?;
    t.set(add(f, [1, 3, z - 1]), "base:obsidian").await?;
    t.set(add(f, [0, 4, z]), "base:redstone_block").await?;
    settle(&t).await?;
    t.check(t.block(add(f, [2, 3, z])).await?.contains("slime"), "slime moved");
    t.check(
        t.block(add(f, [2, 3, z + 1]))
            .await?
            .contains("cobblestone"),
        "what touches it came along",
    );
    t.check(t.block(add(f, [1, 3, z - 1])).await?.contains("obsidian"), "obsidian stayed");

    // Without what holds it up.
    let z = 12;
    let base = add(f, [0, 0, z]);
    t.set(base, "base:piston[facing=east]").await?;
    t.set(add(f, [0, 1, z]), "base:redstone_block").await?;
    settle(&t).await?;
    let head = add(f, [1, 0, z]);
    t.check(t.block(head).await?.contains("piston_head"), "out");
    t.op(&format!("break {} {} {}", head[0], head[1], head[2]))
        .await?;
    settle(&t).await?;
    t.check(t.block(base).await?.contains("air"), "breaking the head breaks the piston");
    t.check(t.block(head).await?.contains("air"), "and it doesn't come back");
    Ok(())
}

/// A door somebody built: pistons pushing blocks and other pistons, with some of those powered
/// themselves. A piston being pushed waits for where it ends up, or nothing here ever moves.
pub async fn redstone_door(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let at = |x: i32, y: i32, z: i32| add(f, [x, y, z - 8]);
    t.fill(at(2, 2, 1), at(7, 2, 1), "base:stone").await?;
    t.set(at(4, 3, 1), "base:stone").await?;
    for z in 0..2 {
        for y in 0..2 {
            t.set(at(2, y, z), "base:sticky_piston[facing=east]")
                .await?;
            t.set(at(7, y, z), "base:sticky_piston[facing=west]")
                .await?;
            let (a, b) = if z == 0 {
                ("base:stone", "base:stone")
            } else {
                ("base:sticky_piston[facing=north]", "base:sticky_piston[facing=north]")
            };
            t.set(at(3, y, z), a).await?;
            t.set(at(6, y, z), b).await?;
            t.set(at(1, y, z), &wire(&[EAST])).await?;
            t.set(at(8, y, z), &wire(&[0])).await?;
        }
    }
    t.set(at(1, 2, 1), &wire(&[EAST])).await?;
    t.set(at(8, 2, 1), &wire(&[0])).await?;
    for x in [2, 3, 5, 6, 7] {
        t.set(at(x, 3, 1), &wire(&[DOWN])).await?;
    }
    settle(&t).await?;
    t.set(at(4, 3, 2), "base:lever[attach=north,front=down,power=on]")
        .await?;
    t.secs(1.5).await?;
    for (y, z) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        t.check(lit(&t, at(2, y, z)).await?, &format!("the west piston at height {y}, row {z} is out"));
        t.check(lit(&t, at(7, y, z)).await?, &format!("the east piston at height {y}, row {z} is out"));
    }
    t.check(t.block(at(4, 1, 0)).await?.contains("stone"), "the block got pushed in");
    t.check(
        t.block(at(4, 1, 1))
            .await?
            .contains("sticky_piston[facing=north"),
        "and the piston behind it",
    );
    t.check(t.block(at(5, 1, 1)).await?.contains("power=on"), "which is out on the side with power over it");
    Ok(())
}

/// A piston is done in 2 ticks for circuits, and what it moved shows up as a block later, where the
/// moving one is drawn. A gate put back keeps nothing of the one that stood there.
pub async fn redstone_timing(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let z = -5;
    // The same wire starts a piston bringing a source to one lamp and a gate of 4 ticks to another.
    let (piston, block, near, far) = (add(f, [3, 0, z]), add(f, [4, 0, z]), add(f, [5, 0, z + 1]), add(f, [6, 0, z - 2]));
    t.set(piston, "base:piston[facing=east]").await?;
    t.set(block, "base:redstone_block").await?;
    t.set(near, "base:redstone_lamp").await?;
    t.set(add(f, [5, 0, z - 2]), &on_floor("repeater", EAST, ",delay=2"))
        .await?;
    t.set(far, "base:redstone_lamp").await?;
    t.fill(add(f, [2, 0, z - 2]), add(f, [2, 0, z]), &wire(&[DOWN]))
        .await?;
    t.fill(add(f, [3, 0, z - 2]), add(f, [4, 0, z - 2]), &wire(&[DOWN]))
        .await?;
    settle(&t).await?;
    t.set(add(f, [1, 0, z - 1]), &lever(true)).await?;
    let mut first = false;
    for _ in 0..100 {
        first = lit(&t, near).await?;
        if first || lit(&t, far).await? {
            break;
        }
    }
    // Read in that order, the gate might have just got there too; never the other way around.
    t.check(first && !lit(&t, far).await?, "the moved block lit its lamp before a gate of 4 ticks got there");

    // There for circuits, not yet on screen, where it stays once it has come. Under the open sky,
    // for the light to send the whole cube along.
    let z = -14;
    let (piston, block, to) = (add(f, [3, 0, z]), add(f, [4, 0, z]), add(f, [5, 0, z]));
    let (taken, gone) = (add(f, [3, 0, z - 3]), add(f, [5, 0, z - 3]));
    for at in [piston, taken] {
        t.set(at, "base:piston[facing=east]").await?;
    }
    t.set(block, "base:cobblestone").await?;
    t.set(add(f, [4, 0, z - 3]), "base:cobblestone").await?;
    settle(&t).await?;
    t.fill(add(f, [2, 0, z - 3]), add(f, [2, 0, z]), &wire(&[DOWN]))
        .await?;
    settle(&t).await?;
    t.set(add(f, [1, 0, z - 1]), &lever(true)).await?;
    let mut ahead = false;
    let mut broke = false;
    for _ in 0..60 {
        let there = t.block(to).await?.contains("cobblestone");
        ahead |= there && t.c().block(to) == Some(0);
        if there && !broke {
            // Broken before clients heard of it, they never see it come back.
            t.set(gone, "air").await?;
            broke = true;
        }
        if there && t.c().block(to).is_some_and(|s| s != 0) {
            break;
        }
    }
    t.check(ahead, "the block is there on the server before the client gets it");
    t.check(t.c().block(to).is_some_and(|s| s != 0), "and then on the client");
    settle(&t).await?;
    t.check(t.c().block(gone) == Some(0), "broken as it landed stays broken on the client");
    t.check(t.c().block(add(f, [4, 0, z])).is_some_and(|s| s != 0), "and the client has the head");

    // Put back while the old one's pulse is on its way: the new one takes its whole delay.
    let z = -10;
    let gate = add(f, [3, 0, z]);
    let lamp = add(f, [4, 0, z]);
    let one = on_floor("repeater", EAST, ",delay=4");
    t.set(gate, &one).await?;
    t.set(lamp, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.set(add(f, [2, 0, z]), "base:redstone_block").await?;
    t.secs(0.1).await?;
    t.set(gate, "air").await?;
    t.set(gate, &one).await?;
    let start = std::time::Instant::now();
    while !t.block(gate).await?.contains("power=on") && start.elapsed() < Duration::from_secs(2) {
        t.frames(1).await?;
    }
    let took = start.elapsed().as_secs_f32();
    t.probe("seconds a gate of 8 ticks put back takes", took, took > 0.3);

    // An observer goes on 2 ticks after what it watches changes, wire's power too, for 2 ticks.
    let z = -18;
    let (dust, observer) = (add(f, [3, 0, z]), add(f, [3, 0, z - 1]));
    t.set(dust, &wire(&[DOWN])).await?;
    t.set(observer, "base:observer[facing=south]").await?;
    t.set(add(f, [3, 0, z - 2]), "base:redstone_lamp").await?;
    settle(&t).await?;
    t.op(&format!("trace {} {} {} {} {} {} 40", dust[0], dust[1], dust[2], observer[0], observer[1], observer[2]))
        .await?;
    t.set(add(f, [3, 0, z + 1]), &lever(true)).await?;
    t.secs(0.5).await?;
    let seen = t.op("traced").await?;
    let when = |at: [i32; 3], what: &dyn Fn(&str) -> bool| {
        let cell = format!("{},{},{}", at[0], at[1], at[2]);
        seen.lines()
            .map(|l| l.split(' ').collect::<Vec<_>>())
            .find(|w| w.len() == 4 && w[1] == cell && what(&format!("{} {}", w[2], w[3])))
            .and_then(|w| w[0].parse::<i64>().ok())
    };
    let lit_wire = when(dust, &|s| s.ends_with(" 15"));
    let on = when(observer, &|s| s.contains("power=on"));
    let off = on.and_then(|on| {
        seen.lines()
            .filter_map(|l| l.split_once(' '))
            .filter(|(tick, rest)| rest.contains("observer") && rest.contains("power=off") && tick.parse::<i64>().is_ok_and(|t| t > on))
            .find_map(|(tick, _)| tick.parse::<i64>().ok())
    });
    let ticks = |a: Option<i64>, b: Option<i64>| a.zip(b).map(|(a, b)| b - a);
    t.check_eq(ticks(lit_wire, on), Some(2), "ticks from the wire to the observer");
    t.check_eq(ticks(on, off), Some(2), "ticks the observer is on");
    Ok(())
}

/// With power it stops a push and stays behind what moves; without, it is any block.
pub async fn redstone_anchor(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    // In the row.
    let z = -4;
    let (piston, anchor) = (add(f, [3, 0, z]), add(f, [4, 0, z]));
    t.set(anchor, "base:anchor").await?;
    t.set(piston, "base:piston[facing=east]").await?;
    let wire_to = add(f, [4, 1, z]);
    t.set(wire_to, "base:lever[attach=down,front=north,power=on]")
        .await?;
    settle(&t).await?;
    t.check(t.block(anchor).await?.contains("power=on"), "lit by the lever on it");
    t.set(add(f, [2, 0, z]), "base:redstone_block").await?;
    settle(&t).await?;
    t.check(!lit(&t, piston).await?, "it stops the piston");
    t.set(wire_to, "base:lever[attach=down,front=north,power=off]")
        .await?;
    settle(&t).await?;
    t.check(lit(&t, piston).await?, "and lets it go without power");
    t.check(t.block(add(f, [5, 0, z])).await?.contains("base:anchor"), "moved like any block");

    // Touching slime, in the air for it not to take the ground along.
    let z = -8;
    t.set(add(f, [3, 3, z]), "base:sticky_piston[facing=east]")
        .await?;
    t.set(add(f, [4, 3, z]), "base:slime_block").await?;
    let beside = add(f, [4, 3, z - 1]);
    t.set(beside, "base:anchor").await?;
    t.set(add(f, [4, 4, z - 1]), "base:lever[attach=down,front=north,power=on]")
        .await?;
    settle(&t).await?;
    t.set(add(f, [2, 3, z]), "base:redstone_block").await?;
    settle(&t).await?;
    t.check(t.block(add(f, [5, 3, z])).await?.contains("slime"), "the slime moved");
    t.check(t.block(beside).await?.contains("anchor"), "the anchor touching it stayed behind");

    // On a floater's way. Power for the anchors from the world, along the way from x = 6: what is
    // pushed stops there, what isn't flies on to the wall.
    let (y, z) = (5, -14);
    let at = |x: i32, dy: i32, z: i32| add(f, [x, y + dy, z]);
    for z in [z, z - 4] {
        t.fill(at(13, -1, z), at(13, 1, z), "base:obsidian").await?;
        t.fill(at(13, -1, z + 1), at(13, 1, z + 1), "base:obsidian")
            .await?;
    }
    let pushed = z;
    t.fill(at(6, 1, pushed), at(9, 1, pushed), "base:redstone_block")
        .await?;
    t.set(at(2, 0, pushed), "base:anchor").await?;
    let beside = z - 4;
    t.fill(at(6, 1, beside + 1), at(9, 1, beside + 1), "base:redstone_block")
        .await?;
    t.set(at(1, 0, beside + 1), "base:anchor").await?;
    for z in [pushed, beside] {
        t.set(at(1, 0, z), "base:slime_block").await?;
        t.set(at(1, -1, z), "base:slime_block").await?;
        t.set(at(0, 0, z), "base:floater[facing=east]").await?;
    }
    settle(&t).await?;
    for z in [pushed, beside] {
        t.set(at(0, -1, z), "base:redstone_block").await?;
    }
    t.secs(3.0).await?;
    let stopped_at = {
        let mut v = Vec::new();
        for x in 2..13 {
            if t.block(at(x, 0, pushed)).await?.contains("anchor") {
                v.push(x);
            }
        }
        v
    };
    t.check(
        stopped_at.len() == 1 && (6..=7).contains(&stopped_at[0]),
        format!("the one it pushes stops it where it got power: {stopped_at:?}"),
    );
    let stopped = stopped_at.first().copied().unwrap_or(0);
    t.check(
        t.block(at(stopped - 2, 0, pushed))
            .await?
            .contains("power=2"),
        format!("its floater is stuck: {}", t.block(at(stopped - 2, 0, pushed)).await?),
    );
    t.check(
        t.block(at(12, 0, beside + 1)).await?.contains("anchor"),
        format!("the one beside rides on: {}", t.block(at(12, 0, beside + 1)).await?),
    );
    // And goes on without power.
    t.fill(at(6, 1, pushed), at(9, 1, pushed), "air").await?;
    t.secs(3.0).await?;
    t.check(t.block(at(12, 0, pushed)).await?.contains("anchor"), format!("it goes on to the wall: {}", t.block(at(12, 0, pushed)).await?));
    Ok(())
}

/// A chest a piston moves is the same chest where it ends up, with what is in it.
pub async fn redstone_moves_chest(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let z = -5;
    let (piston, chest, to) = (add(f, [3, 0, z]), add(f, [4, 0, z]), add(f, [5, 0, z]));
    t.set(chest, "base:chest").await?;
    t.set(piston, "base:piston[facing=east]").await?;
    t.frames(5).await?;
    let list = t
        .op(&format!("near {} {} {} 2", chest[0], chest[1], chest[2]))
        .await?;
    let id = list
        .split_whitespace()
        .find(|e| e.contains("chest"))
        .and_then(|e| e.split(':').next())
        .ok_or(format!("no chest among {list}"))?
        .to_string();
    t.op(&format!("put {id} 0 base:cobblestone 9")).await?;
    let powered = add(f, [2, 0, z]);
    t.set(powered, &lever(true)).await?;
    settle(&t).await?;
    t.check(t.block(to).await?.contains("chest"), "it moved");
    let list = t
        .op(&format!("near {} {} {} 2", to[0], to[1], to[2]))
        .await?;
    t.check(
        list.split_whitespace()
            .any(|e| e.starts_with(&format!("{id}:"))),
        format!("it is the same one: {id} among {list}"),
    );
    let (name, count, _) = t.slot(&id, 0).await?;
    t.check(name.contains("cobblestone") && count == 9, format!("with what it held: {name} {count}"));
    t.check_eq(
        t.op(&format!("slot {} {} {} 0", to[0], to[1], to[2]))
            .await?
            .split_whitespace()
            .nth(1)
            .map(str::to_string),
        Some("9".to_string()),
        "and it is at its new place",
    );
    // Nothing fell out, nor is anything left behind.
    let list = t
        .op(&format!("near {} {} {} 4", chest[0], chest[1], chest[2]))
        .await?;
    t.check(!list.contains("core:item"), format!("nothing dropped: {list}"));
    // And back, with a sticky piston.
    let z = -10;
    let (piston, chest, switch) = (add(f, [3, 0, z]), add(f, [4, 0, z]), add(f, [2, 0, z]));
    t.set(chest, "base:chest").await?;
    t.set(piston, "base:sticky_piston[facing=east]").await?;
    t.frames(5).await?;
    t.op(&format!("put {} {} {} 0 base:cobblestone 3", chest[0], chest[1], chest[2]))
        .await?;
    t.set(switch, &lever(true)).await?;
    settle(&t).await?;
    t.set(switch, &lever(false)).await?;
    settle(&t).await?;
    t.check(t.block(chest).await?.contains("chest"), "back where it was");
    t.check_eq(
        t.op(&format!("slot {} {} {} 0", chest[0], chest[1], chest[2]))
            .await?
            .split_whitespace()
            .nth(1)
            .map(str::to_string),
        Some("3".to_string()),
        "with what it held",
    );
    Ok(())
}

/// What moves is lit by the world where it is, not as under open sky; and light on a body lights
/// the world around it, and only while it is there.
pub async fn redstone_body_light(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    // A dark box around the player.
    t.c().set_flying(true);
    t.fill(add(f, [-4, -1, -8]), add(f, [8, 6, 4]), "base:stone")
        .await?;
    t.fill(add(f, [-3, 0, -7]), add(f, [7, 5, 3]), "air")
        .await?;
    let z = -5;
    let piston = add(f, [1, 2, z]);
    t.set(piston, "base:piston[facing=east]").await?;
    t.set(add(f, [2, 2, z]), "base:snow").await?;
    t.look_at(add(f, [3, 2, z]));
    t.secs(1.0).await?;
    let Some((x, y)) = t.on_screen([f[0] as f64 + 3.0, f[1] as f64 + 2.5, (f[2] + z) as f64 + 0.5]) else {
        return Err("what moves isn't in view".into());
    };
    let shade = |c: probe::C| c[0] as i32 + c[1] as i32 + c[2] as i32;
    t.set(add(f, [0, 2, z]), "base:redstone_block").await?;
    let mut brightest = 0;
    for _ in 0..40 {
        t.frames(1).await?;
        brightest = brightest.max(shade(t.pixel(x + 0.012, y + 0.01).await?));
    }
    settle(&t).await?;
    let landed = shade(t.pixel(x + 0.012, y + 0.01).await?);
    t.check(brightest < landed + 40, format!("no flash on its way in the dark: {brightest}, landed {landed}"));

    // A lamp on a body.
    t.fill(add(f, [-4, -1, -8]), add(f, [8, 6, 4]), "air")
        .await?;
    let (y, z) = (6, -8);
    let id: u64 = t
        .op(&format!("spawn testmod:skiff {} {} {}", f[0] - 8, f[1] + y, f[2] + z))
        .await?
        .trim()
        .parse()
        .map_err(|_| "no skiff".to_string())?;
    t.op(&format!("structureset {id} 0 -1 0 base:glowstone"))
        .await?;
    let probe_at = add(f, [0, y + 1, z + 2]);
    let block_light = |s: String| {
        s.trim().parse::<u32>().map_or(0, |v| {
            [v >> 8, v >> 16, v >> 24]
                .into_iter()
                .map(|b| b & 255)
                .max()
                .unwrap_or(0)
        })
    };
    let (mut most, mut bench_frames) = (0, 0u32);
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        t.frames(1).await?;
        bench_frames += 1;
        most = most.max(block_light(
            t.op(&format!("light {} {} {}", probe_at[0], probe_at[1], probe_at[2]))
                .await?,
        ));
    }
    t.note(format!("{bench_frames} frames in 5 s with a light on a body"));
    t.check(most >= 6, format!("the world around a body is lit by it: {most}"));
    t.op(&format!("remove {id}")).await?;
    settle(&t).await?;
    let after = block_light(
        t.op(&format!("light {} {} {}", probe_at[0], probe_at[1], probe_at[2]))
            .await?,
    );
    t.check(after == 0, format!("and no longer once it is gone: {after}"));
    Ok(())
}

/// A floater takes what it is stuck to along: at a wall it lands, with the circuit on it working all the
/// way, what stands on it carried without slipping.
pub async fn redstone_floater(t: T) -> R {
    t.flat().await?;
    t.cmd("/rule testkit:no_hurt 1").await?;
    let f = t.feet();
    let (y, z) = (4, -7);
    let at = |x: i32, dy: i32| add(f, [x, y + dy, z]);
    t.fill(at(10, -1), at(10, 1), "base:obsidian").await?;
    t.set(at(2, 0), "base:slime_block").await?;
    t.set(at(1, 0), "base:floater[facing=east]").await?;
    t.set(at(3, 0), "base:slime_block").await?;
    t.set(at(2, -1), "base:slime_block").await?;
    t.set(at(2, 1), "base:stone").await?;
    // What it carries: a gate fed from its own block, into a lamp; dust; a chest.
    t.set(at(3, 1), "base:redstone_lamp").await?;
    t.set(add(f, [3, y, z + 1]), "base:chest").await?;
    t.frames(5).await?;
    t.op(&format!("put {} {} {} 0 base:cobblestone 4", f[0] + 3, f[1] + y, f[2] + z + 1))
        .await?;
    settle(&t).await?;
    t.cmd(&format!("/tp {} {} {}", f[0] as f64 + 2.5, f[1] + y + 2, f[2] as f64 + z as f64 + 0.5))
        .await?;
    t.secs(0.5).await?;
    let start = t.state().pos.x;
    // Power to the floater from what comes along.
    // Under the floater, stuck to the slime next to it: it goes along.
    t.set(at(1, -1), "base:redstone_block").await?;
    let lamp_local = [1, 1, 0];
    let (mut own, mut seen_lit) = (None, false);
    for _ in 0..60 {
        t.frames(1).await?;
        let list = t
            .op(&format!("near {} {} {} 12", f[0] + 4, f[1] + y, f[2] + z))
            .await?;
        if let Some(id) = list
            .split_whitespace()
            .find(|e| e.ends_with("core:platform"))
            .and_then(|e| e.split(':').next())
        {
            own = Some(id.to_string());
            let sb = t
                .op(&format!("sblock {id} {} {} {}", lamp_local[0] + 1, lamp_local[1], lamp_local[2]))
                .await
                .unwrap_or_default();
            seen_lit |= sb.contains("lamp");
        }
    }
    t.check(own.is_some(), "it is a platform on its way");
    t.check(seen_lit, "the lamp is on it");
    t.secs(4.0).await?;
    t.check(t.block(at(9, 0)).await?.contains("slime"), format!("at the wall it landed: {}", t.block(at(9, 0)).await?));
    t.check(t.block(at(7, 0)).await?.contains("floater"), "the floater with it");
    t.check(t.block(at(8, 1)).await?.contains("stone"), "and the block on the slime");
    t.check(!t.block(at(1, 0)).await?.contains("floater"), "nothing left behind");
    let now = t.state().pos.x;
    let slip = (now - start) - (now - start).round();
    t.probe("how far what stood on it slipped", slip.abs() as f32, slip.abs() < 0.02);
    t.probe("how far what stood on it went", (now - start) as f32, (now - start) > 4.0);
    let (name, count, _) = t
        .slot(&format!("{} {} {}", f[0] + 9, f[1] + y, f[2] + z + 1), 0)
        .await
        .unwrap_or_default();
    t.check(name.contains("cobblestone") && count == 4, format!("the chest came with what it held: {name} {count}"));

    // Fed in full: 7.5 blocks a second.
    let z = 16;
    t.set(add(f, [0, y + 12, z]), "base:floater[facing=east]")
        .await?;
    t.set(add(f, [1, y + 12, z]), "base:slime_block").await?;
    t.set(add(f, [1, y + 13, z]), "base:slime_block").await?;
    t.set(add(f, [0, y + 13, z]), "base:redstone_block").await?;
    t.secs(0.4).await?;
    let list = t
        .op(&format!("near {} {} {} 12", f[0] + 2, f[1] + y + 12, f[2] + z))
        .await?;
    let id = list
        .split_whitespace()
        .find(|e| e.ends_with("core:platform"))
        .and_then(|e| e.split(':').next())
        .unwrap_or("0")
        .to_string();
    let along = |t: T, id: String| async move {
        let line = t.op(&format!("entity {id}")).await?;
        Ok::<f64, String>(
            line.split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0),
        )
    };
    let (a, began) = (along(t.clone(), id.clone()).await?, std::time::Instant::now());
    t.secs(0.5).await?;
    let (b, took) = (along(t.clone(), id.clone()).await?, began.elapsed().as_secs_f64());
    let speed = (b - a) / took;
    t.probe("blocks a second with full power", speed as f32, (speed - 7.5).abs() < 1.5);
    // Its floater broken on the way, it lands as it would without power.
    let mut floater = None;
    for x in -1..=1 {
        if t.op(&format!("sblock {id} {x} 0 0"))
            .await?
            .contains("floater")
        {
            floater = Some(x);
        }
    }
    t.check(floater.is_some(), "the floater is on it");
    t.op(&format!("sblock {id} {} 0 0 core:air", floater.unwrap_or(0)))
        .await?;
    t.secs(1.0).await?;
    t.check(t.op(&format!("entity {id}")).await.is_err(), "it landed");
    let mut landed = Vec::new();
    for x in 1..30 {
        if t.block(add(f, [x, y + 12, z])).await?.contains("slime") {
            landed.push(x);
        }
    }
    t.check(landed.len() == 1, format!("with its blocks: {landed:?}"));

    // Powered from the world only: out of reach of that it stops, and on a whole block.
    t.cmd(&format!("/tp {} {} {}", f[0], f[1] + 14, f[2]))
        .await?;
    let z = 8;
    let at = |x: i32, dy: i32| add(f, [x, y + dy, z]);
    t.set(at(0, 0), "base:floater[facing=east]").await?;
    t.set(at(1, 0), "base:slime_block").await?;
    // The floor it touches isn't in front of it, so it doesn't come, nor does anything else.
    t.set(at(0, -1), "base:stone").await?;
    // Dust beside its way, which a floater doesn't take along as it would a block touching it.
    t.fill(add(f, [-2, y - 1, z + 1]), add(f, [2, y - 1, z + 1]), "base:stone")
        .await?;
    t.fill(add(f, [-1, y, z + 1]), add(f, [2, y, z + 1]), &wire(&[DOWN]))
        .await?;
    t.set(add(f, [-2, y, z + 1]), "base:redstone_block").await?;
    t.secs(3.0).await?;
    t.check(t.block(at(0, 0)).await?.contains("air"), "it left");
    t.check(t.block(at(0, -1)).await?.contains("stone"), "without the block under it");
    let found: Vec<i32> = {
        let mut v = Vec::new();
        for x in 2..9 {
            if t.block(at(x, 0)).await?.contains("floater") {
                v.push(x);
            }
        }
        v
    };
    t.check(found.len() == 1 && found[0] >= 2 && found[0] <= 5, format!("it stopped just past what fed it: {found:?}"));

    // Something in the way from the first: it doesn't go, until that is gone.
    let z = 12;
    let at = |x: i32, dy: i32| add(f, [x, y + dy, z]);
    t.set(at(0, 0), "base:floater[facing=east]").await?;
    t.set(at(1, 0), "base:obsidian").await?;
    t.set(at(-1, 0), "base:redstone_block").await?;
    settle(&t).await?;
    t.check(t.block(at(0, 0)).await?.contains("floater"), "it stays, with obsidian in front of it");
    t.check(t.block(at(0, 0)).await?.contains("power=2"), "and shows it is stuck");
    // Which a comparator behind it reads.
    let (gate, lamp) = (add(f, [0, y, z + 1]), add(f, [0, y, z + 2]));
    t.fill(add(f, [0, y - 1, z + 1]), add(f, [0, y - 1, z + 2]), "base:stone")
        .await?;
    t.set(gate, &on_floor("comparator", SOUTH, "")).await?;
    t.set(lamp, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(lit(&t, lamp).await?, "a comparator reads that it is stuck");
    // How it looks, stuck and not, on the floor and on a wall.
    t.set(at(-1, 2), "base:floater[facing=up]").await?;
    t.set(at(-3, 2), "base:floater[facing=south]").await?;
    t.cmd(&format!("/tp {} {} {}", f[0] as f64 - 0.5, f[1] + y + 4, f[2] as f64 + z as f64 + 4.5))
        .await?;
    t.c().set_flying(true);
    t.op("put me 1 base:floater 1").await?;
    t.op("put me 2 base:lever 1").await?;
    t.op("put me 3 base:redstone_torch 1").await?;
    t.c().select_slot(1);
    t.look_at(at(-1, 1));
    t.secs(0.5).await?;
    t.shot("floaters").await?;
    t.set(at(1, 0), "air").await?;
    t.secs(0.8).await?;
    t.check(t.block(at(0, 0)).await?.contains("air"), "and goes when that has gone");
    Ok(())
}

/// A mob on a flying structure walks its deck, the structure going on under it.
pub async fn body_walking(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let id: u64 = t
        .op(&format!("spawn testmod:skiff {} {} {}", f[0] - 6, f[1] + 3, f[2] - 6))
        .await?
        .trim()
        .parse()
        .map_err(|_| "no skiff".to_string())?;
    // A wider deck, along the way it goes.
    for x in -4..=4 {
        for z in -2..=2 {
            t.op(&format!("structureset {id} {x} 0 {z} base:oak_planks"))
                .await?;
        }
    }
    let pig = t
        .op(&format!("spawn base:pig {} {} {}", f[0] - 6, f[1] + 5, f[2] - 6))
        .await?;
    t.op(&format!("onto {id} 1.0 {pig}")).await?;
    t.secs(0.5).await?;
    let pos = |t: T, id: String| async move {
        let line = t.op(&format!("entity {id}")).await?;
        Ok::<Vec<f64>, String>(
            line.split_whitespace()
                .skip(1)
                .take(3)
                .filter_map(|s| s.parse().ok())
                .collect(),
        )
    };
    let (b, p) = (pos(t.clone(), id.to_string()).await?, pos(t.clone(), pig.clone()).await?);
    let start_z = p[2] - b[2];
    // Somewhere on the deck.
    t.op(&format!("navigate {pig} {} {} {}", p[0].floor(), p[1].floor(), p[2].floor() as i32 + 2))
        .await?;
    t.secs(3.0).await?;
    let (b, p) = (pos(t.clone(), id.to_string()).await?, pos(t.clone(), pig.clone()).await?);
    t.check(p[1] > b[1] + 0.5, format!("it is still on the deck: {p:?} on {b:?}"));
    t.probe("how far along the deck it walked", (p[2] - b[2] - start_z) as f32, (p[2] - b[2] - start_z).abs() > 0.8);
    Ok(())
}

/// What the review of all this found, each a way a body and the world got out of step.
pub async fn redstone_body_edges(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let y = 4;
    // A lamp taken away from what lights it lands as it is by then.
    let z = -5;
    let (piston, lamp, to) = (add(f, [1, y, z]), add(f, [2, y, z]), add(f, [3, y, z]));
    t.set(piston, "base:piston[facing=east]").await?;
    t.set(lamp, "base:redstone_lamp").await?;
    t.set(add(f, [2, y - 1, z]), "base:redstone_block").await?;
    settle(&t).await?;
    t.check(lit(&t, lamp).await?, "lit where it is");
    t.set(add(f, [0, y, z]), "base:redstone_block").await?;
    settle(&t).await?;
    t.check(t.block(to).await?.contains("redstone_lamp"), "moved");
    t.check(!lit(&t, to).await?, "dark away from its power, not as it left");

    // Sand landing on a platform is part of that.
    let z = -9;
    let deck = add(f, [2, y, z]);
    t.set(deck, "base:stone").await?;
    let id = t
        .op(&format!("lift {} {} {} 0", deck[0], deck[1], deck[2]))
        .await?;
    t.set(add(deck, [0, 3, 0]), "base:sand").await?;
    t.secs(1.5).await?;
    t.check_eq(t.op(&format!("sblock {id} 0 1 0")).await?, "base:sand".to_string(), "sand came to lie on the platform");
    t.check(!t.block(add(deck, [0, 1, 0])).await?.contains("sand"), "not on the world's block there");

    // Wire of a platform and wire of the world don't feed each other, and nothing is left behind
    // when a platform goes.
    let z = -13;
    let deck = add(f, [3, y, z]);
    t.set(deck, "base:stone").await?;
    t.set(add(deck, [0, 1, 0]), &wire(&[DOWN])).await?;
    let id = t
        .op(&format!("lift {} {} {} 0", deck[0], deck[1], deck[2]))
        .await?;
    t.set(add(deck, [-1, 0, 0]), "base:stone").await?;
    let beside = add(deck, [-1, 1, 0]);
    t.set(add(deck, [-2, 0, 0]), "base:stone").await?;
    t.set(add(deck, [-2, 1, 0]), &wire(&[DOWN])).await?;
    t.set(beside, &wire(&[DOWN])).await?;
    let (feed, lamp) = (add(deck, [-3, 1, 0]), add(deck, [0, 1, -1]));
    t.set(lamp, "base:redstone_lamp").await?;
    t.set(feed, &lever(true)).await?;
    settle(&t).await?;
    let id_num: u64 = id.parse().map_err(|_| "no platform".to_string())?;
    t.check(!lit(&t, lamp).await?, "the platform's wire isn't fed by the wire next to it");
    let sb = t.c().body_block(id_num, [0, 1, 0]);
    t.check(sb.is_some(), "the client has what is on it");

    // A gate on a platform changes its look for clients as well.
    let z = -19;
    let deck = add(f, [3, y, z]);
    t.set(deck, "base:stone").await?;
    t.set(add(deck, [0, 1, 0]), "base:redstone_block").await?;
    t.set(add(deck, [1, 0, 0]), "base:stone").await?;
    t.set(add(deck, [1, 1, 0]), &on_floor("repeater", EAST, ""))
        .await?;
    settle(&t).await?;
    t.set(add(deck, [0, 1, 0]), "air").await?;
    settle(&t).await?;
    let id = t
        .op(&format!("lift {} {} {} 1", deck[0], deck[1], deck[2]))
        .await?;
    let id_num: u64 = id.parse().map_err(|_| "no platform".to_string())?;
    t.secs(0.3).await?;
    let before = t.c().body_block(id_num, [1, 1, 0]);
    t.op(&format!("structureset {id} 0 1 0 base:redstone_block"))
        .await?;
    settle(&t).await?;
    let after = t.c().body_block(id_num, [1, 1, 0]);
    t.check(before.is_some() && before != after, format!("its look changes for clients: {before:?} then {after:?}"));
    t.check(
        t.op(&format!("sblock {id} 1 1 0"))
            .await?
            .contains("power=on"),
        "and it is on",
    );
    Ok(())
}

/// What is fixed to a body's leading face comes along, whichever way that is. What it runs into
/// that is solid knocks it off, and the body goes on: a lever feeding the floater is a way to stop
/// on arrival. What isn't solid it breaks, like the body would.
pub async fn redstone_floater_riders(t: T) -> R {
    t.flat().await?;
    t.cmd("/rule testkit:no_hurt 1").await?;
    let f = t.feet();
    let y = 6;
    let items = |t: T, p: [i32; 3], r: i32| async move {
        let list = t
            .op(&format!("near {} {} {} {r}", p[0], p[1], p[2]))
            .await?;
        Ok::<usize, String>(
            list.split_whitespace()
                .filter(|e| e.ends_with("core:item"))
                .count(),
        )
    };
    let ways: [(&str, [i32; 3], [i32; 3], &str); 4] = [
        ("up", [0, 1, 0], [0, 1, 0], "base:lever[attach=down,front=north,power=off]"),
        ("down", [0, -1, 0], [0, -1, 0], "base:lever[attach=up,front=north,power=off]"),
        ("east", [1, 0, 0], [1, 0, 0], "base:lever[attach=west,front=east,power=off]"),
        ("east", [1, 0, 0], [0, 1, 0], "base:lever[attach=down,front=north,power=off]"),
    ];
    for (i, (facing, way, on, lever)) in ways.into_iter().enumerate() {
        let m = add(f, [0, y, -6 - 5 * i as i32]);
        let front = add(m, way);
        let stone = add(front, if way[0] != 0 { [0, 0, 1] } else { [1, 0, 0] });
        let at = add(stone, on);
        t.set(m, &format!("base:floater[facing={facing}]")).await?;
        t.set(front, "base:slime_block").await?;
        t.set(stone, "base:stone").await?;
        t.set(at, lever).await?;
        t.frames(4).await?;
        // Power from a block that stays behind: a block's way and it stops.
        t.set(add(m, [-way[0], -way[1], -way[2]]), "base:redstone_block")
            .await?;
        t.secs(1.5).await?;
        let mut seen = Vec::new();
        for d in 0..4 {
            if t.block(add(at, [way[0] * d, way[1] * d, way[2] * d]))
                .await?
                .contains("lever")
            {
                seen.push(d);
            }
        }
        t.check(seen == [1], format!("{facing}, lever at {on:?}: it came along: {seen:?}"));
        t.check_eq(items(t.clone(), at, 6).await?, 0, "nothing dropped");
    }

    // The lever on the leading face is what feeds the floater, through the stone it is on.
    let z = -32;
    let at = |x: i32, dy: i32, dz: i32| add(f, [x, y + dy, z + dz]);
    t.set(at(0, 0, 0), "base:floater[facing=east]").await?;
    t.set(at(1, 0, 0), "base:slime_block").await?;
    t.set(at(1, 1, 0), "base:slime_block").await?;
    t.set(at(0, 1, 0), "base:slime_block").await?;
    t.set(at(0, 1, -1), "base:slime_block").await?;
    t.set(at(0, 0, -1), "base:stone").await?;
    // Something to break on the way, and what it stops at.
    t.set(at(3, -1, -1), "base:stone").await?;
    t.set(at(3, 0, -1), "base:dandelion").await?;
    t.set(at(6, 0, -1), "base:obsidian").await?;
    t.frames(4).await?;
    t.set(at(1, 0, -1), "base:lever[attach=west,front=east,power=on]")
        .await?;
    t.secs(3.0).await?;
    t.check(!t.block(at(3, 0, -1)).await?.contains("dandelion"), "the flower in its way broke");
    t.check(t.block(at(6, 0, -1)).await?.contains("obsidian"), "what stops it stays");
    let mut stone = Vec::new();
    let mut levers = 0;
    for x in 0..12 {
        stone.extend(
            t.block(at(x, 0, -1))
                .await?
                .contains("base:stone")
                .then_some(x),
        );
        levers += t.block(at(x, 0, -1)).await?.contains("lever") as i32;
    }
    t.check(stone.len() == 1 && (3..=5).contains(&stone[0]), format!("it stopped short of the wall: the stone at {stone:?}"));
    t.check_eq(levers, 0, "the lever came off");
    t.check(items(t.clone(), at(5, 0, -1), 12).await? >= 2, "the lever and the flower dropped");

    // A wall only the lever meets, on a body with its own power: on it goes without the lever.
    let z = -40;
    let at = |dx: i32, dy: i32| add(f, [dx, y + dy, z]);
    t.set(at(0, 0), "base:floater[facing=up]").await?;
    t.set(at(0, 1), "base:slime_block").await?;
    t.set(at(1, 1), "base:slime_block").await?;
    t.set(at(2, 1), "base:lever[attach=west,front=east,power=off]")
        .await?;
    t.set(at(2, 4), "base:obsidian").await?;
    t.set(at(0, 8), "base:obsidian").await?;
    t.frames(4).await?;
    t.set(at(1, 0), "base:redstone_block").await?;
    t.secs(2.5).await?;
    t.check(t.block(at(0, 7)).await?.contains("slime"), format!("the body went on to the top: {}", t.block(at(0, 7)).await?));
    t.check(t.block(at(2, 4)).await?.contains("obsidian"), "the wall stays");
    let mut levers = 0;
    for dy in 0..10 {
        levers += t.block(at(2, dy)).await?.contains("lever") as i32;
    }
    t.check_eq(levers, 0, "the lever didn't get through it");
    t.check(items(t.clone(), at(2, 3), 14).await? >= 1, "it dropped");
    Ok(())
}

/// Floaters stuck to the same thing go as one, the same whichever way it is all turned; ones pulling
/// opposite ways stay where they are.
pub async fn redstone_floaters_together(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let y = 4;
    // Two floaters with one block of slime in front of both, going ways across each other, fed in
    // one tick by a piece of dust touching both. A quarter turn each time, around a spot of its own.
    let turn = |q: i32, p: [i32; 2]| match q {
        0 => [p[0], p[1]],
        1 => [-p[1], p[0]],
        2 => [-p[0], -p[1]],
        _ => [p[1], -p[0]],
    };
    let names = |q: i32, p: [i32; 2]| {
        let v = turn(q, p);
        match v {
            [1, 0] => "east",
            [-1, 0] => "west",
            [0, 1] => "south",
            _ => "north",
        }
    };
    let mut ends = Vec::new();
    for q in 0..4 {
        // Near enough for all around it to be loaded: a wire by what isn't holds as it is.
        let o = [[-9, -9], [9, -9], [9, 9], [-9, 9]][q as usize];
        let at = |p: [i32; 2], dy: i32| {
            let v = turn(q, p);
            add(f, [o[0] + v[0], y + dy, o[1] + v[1]])
        };
        t.set(at([1, 0], 0), "base:slime_block").await?;
        t.set(at([0, 0], 0), &format!("base:floater[facing={}]", names(q, [1, 0])))
            .await?;
        t.set(at([1, -1], 0), &format!("base:floater[facing={}]", names(q, [0, 1])))
            .await?;
        t.set(at([0, -1], -1), "base:stone").await?;
        t.set(at([-1, -1], -1), "base:stone").await?;
        t.set(at([0, -1], 0), &wire(&[DOWN])).await?;
        settle(&t).await?;
        t.set(at([-1, -1], 0), &lever(true)).await?;
        t.secs(2.5).await?;
        let mut found = Vec::new();
        for dx in -2..7 {
            for dz in -2..7 {
                if t.block(at([dx, dz], 0)).await?.contains("floater") {
                    found.push([dx, dz]);
                }
            }
        }
        ends.push(found);
    }
    t.note(format!("where they ended up: {ends:?}"));
    t.check(ends[0].len() == 2, format!("both are there: {:?}", ends[0]));
    t.check(ends[0] != vec![[0, 0], [1, -1]] && ends[0] != vec![[1, -1], [0, 0]], "they went");
    for q in 1..4 {
        t.check_eq(ends[q].clone(), ends[0].clone(), &format!("the same turned {q} times"));
    }

    // Pulling opposite ways as hard, nothing goes: the dust feeding them runs out from the middle.
    let (y, z) = (y + 8, 0);
    t.set(add(f, [1, y, z]), "base:slime_block").await?;
    t.set(add(f, [0, y, z]), "base:floater[facing=east]")
        .await?;
    t.set(add(f, [2, y, z]), "base:floater[facing=west]")
        .await?;
    t.fill(add(f, [0, y - 1, z + 1]), add(f, [2, y - 1, z + 2]), "base:stone")
        .await?;
    t.fill(add(f, [0, y, z + 1]), add(f, [2, y, z + 1]), &wire(&[DOWN]))
        .await?;
    settle(&t).await?;
    t.set(add(f, [1, y, z + 2]), &lever(true)).await?;
    let mut lifted = false;
    for _ in 0..12 {
        t.frames(3).await?;
        lifted |= t
            .op(&format!("near {} {} {} 6", f[0] + 1, f[1] + y, f[2] + z))
            .await?
            .contains("core:platform");
    }
    t.check(!lifted, "opposite floaters never set off");
    t.check(t.block(add(f, [0, y, z])).await?.contains("floater") && t.block(add(f, [2, y, z])).await?.contains("floater"), "and stay");
    Ok(())
}

/// What stands on what a piston moves goes with it, and its circuit goes on: power goes on through
/// a gate, and a lamp fed from the world doesn't go off for a moment.
pub async fn redstone_rides(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let y = 3;
    let at = |x: i32, z: i32| add(f, [x, y, z]);
    let z = -5;
    t.set(at(1, z), "base:piston[facing=east]").await?;
    // All of it in the air, for nothing else to go along; slime for the block on it to come too.
    t.fill(at(2, z), at(4, z), "base:slime_block").await?;
    t.set(add(f, [2, y + 1, z]), "base:redstone_block").await?;
    t.set(add(f, [3, y + 1, z]), &wire(&[DOWN])).await?;
    t.set(add(f, [4, y + 1, z]), &on_floor("repeater", EAST, ",delay=1"))
        .await?;
    settle(&t).await?;
    t.check(lit(&t, add(f, [4, y + 1, z])).await?, "power goes through the gate to begin with");
    t.set(at(0, z), "base:redstone_block").await?;
    settle(&t).await?;
    t.check(t.block(at(5, z)).await?.contains("slime"), "the row moved");
    t.check(t.block(add(f, [4, y + 1, z])).await?.contains("redstone"), "wire on it came along, not broken");
    t.check(t.block(add(f, [5, y + 1, z])).await?.contains("repeater"), "the gate on it too");
    t.check(
        t.block(add(f, [3, y + 1, z]))
            .await?
            .contains("redstone_block"),
        "the block on it too",
    );
    let near = t
        .op(&format!("near {} {} {} 8", f[0] + 4, f[1] + y, f[2] + z))
        .await?;
    t.check(!near.contains("core:item"), format!("nothing dropped: {near}"));
    t.check(lit(&t, add(f, [5, y + 1, z])).await?, "power still goes through it");

    // Dust that merely lies in the way of what moves breaks.
    let z = -8;
    t.set(at(1, z), "base:piston[facing=east]").await?;
    t.set(at(2, z), "base:stone").await?;
    t.set(at(3, z - 1), "base:stone").await?;
    t.set(at(3, z), &wire(&[NORTH])).await?;
    t.set(at(0, z), "base:redstone_block").await?;
    settle(&t).await?;
    t.check(t.block(at(3, z)).await?.contains("stone"), "the block got there");
    t.check(!t.block(at(4, z)).await?.contains("redstone"), "dust on something else did not ride");

    // On a piston's blocks, a lamp fed from the world doesn't flicker, for power reaches it in the
    // air as well.
    let z = -14;
    t.set(at(1, z), "base:piston[facing=east]").await?;
    t.set(at(2, z), "base:redstone_lamp").await?;
    t.set(add(f, [2, y - 1, z]), "base:redstone_block").await?;
    t.set(add(f, [3, y - 1, z]), "base:redstone_block").await?;
    settle(&t).await?;
    t.check(lit(&t, at(2, z)).await?, "the lamp is lit, by the block under it");
    t.set(at(0, z), "base:redstone_block").await?;
    let mut was_off = false;
    let mut bodies = 0;
    for _ in 0..30 {
        t.frames(1).await?;
        let there = t
            .op(&format!("near {} {} {} 6", f[0] + 2, f[1] + y, f[2] + z))
            .await?;
        if let Some(body) = there
            .split_whitespace()
            .find(|e| e.ends_with("core:moving_blocks"))
            .and_then(|e| e.split(':').next())
        {
            let state = t
                .op(&format!("sblock {body} 1 0 0"))
                .await
                .unwrap_or_default();
            bodies += state.contains("lamp") as u32;
            was_off |= state.contains("power=off");
        }
    }
    t.check(bodies > 0, "it was seen on its way");
    t.check(!was_off, "the lamp stayed lit on the way");
    t.check(lit(&t, at(3, z)).await?, "and where it got to");
    Ok(())
}

/// What is on a body and what is in the world power each other where they are next to each other,
/// and an observer sees a body go past.
pub async fn redstone_body_and_world(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    // Along the skiff's way: a redstone block hanging under its deck goes past these at its level.
    let (y, z) = (3, -8);
    let id: u64 = t
        .op(&format!("spawn testmod:skiff {} {} {}", f[0] - 8, f[1] + y + 1, f[2] + z))
        .await?
        .trim()
        .parse()
        .map_err(|_| "no skiff".to_string())?;
    t.op(&format!("structureset {id} 0 -1 0 base:redstone_block"))
        .await?;
    t.op(&format!("structureset {id} 1 -1 0 testkit:drill"))
        .await?;
    // Its own lamp, going past a block of the world.
    t.op(&format!("structureset {id} -1 -1 0 base:redstone_lamp"))
        .await?;
    let lamp = add(f, [-4, y, z + 1]);
    t.set(lamp, "base:redstone_lamp").await?;
    let dust = add(f, [-3, y, z + 1]);
    t.set(add(f, [-3, y - 1, z + 1]), "base:stone").await?;
    t.set(dust, &wire(&[DOWN])).await?;
    let far = add(f, [-3, y, z + 2]);
    t.set(add(f, [-3, y - 1, z + 2]), "base:stone").await?;
    t.set(far, &wire(&[DOWN])).await?;
    let fed = add(f, [-3, y, z + 3]);
    t.set(fed, "base:redstone_lamp").await?;
    // An observer watching a cell of the way says so in a lamp behind it.
    let (observer, its_lamp) = (add(f, [0, y, z + 1]), add(f, [0, y, z + 2]));
    t.set(observer, "base:observer[facing=north]").await?;
    t.set(its_lamp, "base:redstone_lamp").await?;
    // A block of the world for its lamp, off to the side of the way.
    t.set(add(f, [-1, y, z - 1]), "base:redstone_block").await?;
    let passes_before: u32 = t.op("passes").await?.parse().unwrap_or(0);
    let (mut lamp_lit, mut fed_lit, mut own_lit, mut seen) = (false, false, false, false);
    for _ in 0..240 {
        t.frames(1).await?;
        lamp_lit |= lit(&t, lamp).await?;
        fed_lit |= lit(&t, fed).await?;
        seen |= lit(&t, its_lamp).await?;
        own_lit |= t
            .op(&format!("sblock {id} -1 -1 0"))
            .await?
            .contains("power=on");
    }
    t.check(lamp_lit, "a redstone block going past a lamp lights it");
    t.check(fed_lit, "and dust, through to what the dust feeds");
    t.check(seen, "an observer sees it go past");
    t.check(own_lit, "its own lamp is lit going past a block of the world");
    let passes: u32 = t.op("passes").await?.parse().unwrap_or(0);
    t.check(passes > passes_before + 3, format!("on_carried as it goes: {passes_before} then {passes}"));
    Ok(())
}

/// Two wanting the same block both give way, for as long as that lasts and no longer.
pub async fn redstone_contest(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let z = -6;
    let (block, west, south) = (add(f, [4, 0, z]), add(f, [3, 0, z]), add(f, [4, 0, z + 1]));
    t.set(block, "base:cobblestone").await?;
    t.set(west, "base:piston[facing=east]").await?;
    t.set(south, "base:piston[facing=north]").await?;
    // One wire behind both, for them to get power in the same tick.
    let last = add(f, [4, 0, z + 2]);
    for p in [[2, 0, z], [2, 0, z + 1], [2, 0, z + 2], [3, 0, z + 2]] {
        t.set(add(f, p), &wire(&[DOWN])).await?;
    }
    t.set(last, &wire(&[DOWN])).await?;
    settle(&t).await?;
    t.set(add(f, [1, 0, z + 1]), &lever(true)).await?;
    settle(&t).await?;
    t.check(t.block(block).await?.contains("cobblestone"), "neither gets the block");
    t.set(last, "air").await?;
    settle(&t).await?;
    t.check(t.block(block).await?.contains("piston_head"), "the other one does once one of them stops wanting it");

    // Two sticky ones at one block both come back without it.
    let z = -20;
    let (block, a, b) = (add(f, [5, 0, z]), add(f, [3, 0, z]), add(f, [5, 0, z + 2]));
    t.set(block, "base:cobblestone").await?;
    t.set(a, "base:sticky_piston[facing=east]").await?;
    t.set(b, "base:sticky_piston[facing=north]").await?;
    for p in [[2, 0, z], [2, 0, z + 1], [2, 0, z + 2], [3, 0, z + 2], [4, 0, z + 2]] {
        t.set(add(f, p), &wire(&[DOWN])).await?;
    }
    let switch = add(f, [1, 0, z + 1]);
    t.set(switch, &lever(true)).await?;
    settle(&t).await?;
    t.check(lit(&t, a).await? && lit(&t, b).await?, "both out at it");
    t.set(switch, &lever(false)).await?;
    settle(&t).await?;
    t.check(!lit(&t, a).await? && !lit(&t, b).await?, "both back");
    t.check(t.block(block).await?.contains("cobblestone"), "and it stays");

    // Asleep behind what it can't move, until that is swapped for something it can.
    let z = -12;
    let (piston, there) = (add(f, [3, 0, z]), add(f, [4, 0, z]));
    t.set(there, "base:obsidian").await?;
    t.set(piston, "base:piston[facing=east]").await?;
    t.set(add(f, [2, 0, z]), "base:redstone_block").await?;
    settle(&t).await?;
    t.check(!lit(&t, piston).await?, "obsidian stops it");
    t.set(there, "base:cobblestone").await?;
    settle(&t).await?;
    t.check(lit(&t, piston).await?, "cobblestone doesn't");

    // What a gate reads through turning into something that carries nothing.
    let z = -16;
    let (between, lamp) = (add(f, [4, 0, z]), add(f, [6, 0, z]));
    t.set(add(f, [2, 0, z]), "base:redstone_block").await?;
    t.set(add(f, [3, 0, z]), &on_floor("repeater", EAST, ""))
        .await?;
    t.set(between, "base:stone").await?;
    t.set(add(f, [5, 0, z]), &on_floor("comparator", EAST, ""))
        .await?;
    t.set(lamp, "base:redstone_lamp").await?;
    settle(&t).await?;
    t.check(lit(&t, lamp).await?, "through a block");
    t.set(between, "base:glass").await?;
    settle(&t).await?;
    t.check(!lit(&t, lamp).await?, "not through glass");
    t.set(between, "base:stone").await?;
    settle(&t).await?;
    t.check(lit(&t, lamp).await?, "and through a block again");
    Ok(())
}

/// Wherever one stands on a flying machine, it carries them: when it stops against a wall they are
/// where they were on the block under them, not shoved along it.
pub async fn redstone_flying_rider(t: T) -> R {
    t.flat().await?;
    // Getting off one machine is a fall.
    t.cmd("/rule testkit:no_hurt 1").await?;
    let f = t.feet();
    let y = 5;
    // Across the front half's top, near its edges too, and on the back half's.
    for (i, (name, x, on)) in [
        ("the middle of the front", 2.5, 2),
        ("the front's far edge", 2.65, 2),
        ("the front's near edge", 2.35, 2),
        ("over the front's edge", 2.85, 2),
        ("the back", 1.5, 1),
    ]
    .into_iter()
    .enumerate()
    {
        // Within the cube the player's physics has around it.
        let z = [-6, -11, -16, 6, 11][i];
        let at = |x: i32, y: i32| add(f, [x, y, z]);
        t.set(at(1, y), "base:piston[facing=east]").await?;
        t.set(at(1, y + 1), "base:slime_block").await?;
        t.set(at(2, y), "base:slime_block").await?;
        t.set(at(2, y + 1), "base:sticky_piston[facing=west]")
            .await?;
        t.set(at(2, y - 1), "base:observer[facing=down]").await?;
        t.set(at(1, y + 2), "base:observer[facing=up]").await?;
        // What stops it: an obsidian wall some steps ahead, under where the rider's head goes.
        t.fill(at(11, y - 1), at(11, y + 1), "base:obsidian")
            .await?;
        settle(&t).await?;
        let top = if on == 1 { y + 3 } else { y + 2 };
        t.cmd(&format!("/tp {} {} {}", f[0] as f64 + x, f[1] + top, f[2] as f64 + z as f64 + 0.5))
            .await?;
        t.secs(0.5).await?;
        let start = t.state().pos.x;
        // A pulse into the piston, as its observer would give it: someone may stand on that one.
        t.set(at(0, y), "base:redstone_block").await?;
        t.set(at(0, y), "air").await?;
        // What is shown only goes forward.
        let (mut was, mut back) = (t.state().shown.x, 0.0f64);
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_secs(4) {
            t.frames(1).await?;
            let now = t.state().shown.x;
            back = back.max(was - now);
            was = now;
        }
        t.probe(&format!("how far back {name} went at most"), back as f32, back < 0.01);
        // Whichever block of its row the machine ended on, the rider is as far along it as before.
        let now = t.state().pos.x;
        let slip = (now - start) - (now - start).round();
        t.probe(&format!("how far {name} slipped"), slip.abs() as f32, slip.abs() < 0.02);
        t.probe(&format!("blocks {name} went"), (now - start) as f32, now - start > 2.0);
        t.cmd(&format!("/tp {} {} {}", f[0], f[1] + 14, f[2]))
            .await?;
        t.secs(0.3).await?;
    }
    // Not a player: what stands on a block a piston takes rides from the first moment.
    let z = 14;
    t.set(add(f, [3, 1, z]), "base:cobblestone").await?;
    t.set(add(f, [2, 1, z]), "base:piston[facing=east]").await?;
    let idol = t
        .op(&format!("spawn testmod:idol {}.5 {} {}.5", f[0] + 3, f[1] + 2, f[2] + z))
        .await?;
    let id: u64 = idol.trim().parse().map_err(|_| format!("spawned {idol}"))?;
    t.secs(0.5).await?;
    let start = t.c().entity(id).map_or(0.0, |e| e.pos.x);
    t.set(add(f, [1, 1, z]), "base:redstone_block").await?;
    t.secs(1.0).await?;
    let went = t.c().entity(id).map_or(0.0, |e| e.pos.x) - start;
    t.probe("how far what stood on the block went", went as f32, (went - 1.0).abs() < 0.02);
    Ok(())
}

/// A flying machine: two pistons and two observers on slime, going by itself with a player on it.
/// Each half lands, which sets off its observer, which moves the other half.
pub async fn redstone_flying_machine(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let y = 5;
    let at = |x: i32, y: i32| add(f, [x, y, -6]);
    // The back half: slime on a plain piston, an observer on the slime pointing into it.
    let back = at(1, y + 1);
    t.set(at(1, y), "base:piston[facing=east]").await?;
    t.set(back, "base:slime_block").await?;
    // The front half: slime in front of that piston, a sticky one on it pointing back.
    t.set(at(2, y), "base:slime_block").await?;
    t.set(at(2, y + 1), "base:sticky_piston[facing=west]")
        .await?;
    t.set(at(2, y - 1), "base:observer[facing=down]").await?;
    t.set(at(1, y + 2), "base:observer[facing=up]").await?;
    settle(&t).await?;
    let start = t.block(back).await?;
    t.check(start.contains("slime"), "built");
    // A player rides on top, from the first moment.
    t.cmd(&format!("/tp {} {} {}", at(2, y + 2)[0] as f64 + 0.5, at(2, y + 2)[1], at(2, y + 2)[2] as f64 + 0.5))
        .await?;
    t.secs(0.5).await?;
    let from = t.state().shown;
    // A block next to the observer sets it off.
    t.set(at(1, y + 3), "base:stone").await?;
    t.set(at(1, y + 3), "air").await?;
    let (mut last, mut largest) = (from, 0.0f64);
    for _ in 0..1000 {
        t.frames(1).await?;
        let now = t.state().shown;
        largest = largest
            .max((now.x - last.x).abs())
            .max((now.y - last.y).abs());
        last = now;
    }
    // At most half a block a tick, and a frame isn't longer than that even under load.
    t.probe("largest step of a frame on it", largest as f32, largest < 0.55);
    // Wherever it has got to: look along the row for the back half's slime.
    let mut reached = None;
    for x in 2..60 {
        if t.block(at(x, y + 1)).await?.contains("slime") {
            reached = Some(x);
            break;
        }
    }
    t.probe("blocks a flying machine went in 1000 frames", reached.unwrap_or(0) as f32, reached.is_some_and(|x| x > 4));
    let now = t.state().shown;
    t.probe("blocks the player rode", (now.x - from.x) as f32, now.x - from.x > 5.0);
    Ok(())
}

/// By hand: dust goes on the face clicked, next to any there, and breaks one piece at a time; gates
/// go on the face clicked and turn with the look; and a piece without support comes off.
pub async fn redstone_by_hand(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let z = -3;
    let (wall, cell) = (add(f, [0, 0, z]), add(f, [0, 0, z + 1]));
    t.set(wall, "base:stone").await?;
    t.op("put me 0 redstone 8").await?;
    t.c().select_slot(0);
    t.frames(2).await?;
    t.look_at(wall);
    t.frames(2).await?;
    t.press("core:use");
    t.until("dust on the wall", Duration::from_secs(2), |t| t.c().block(cell).is_some_and(|s| wire_sides(s) == 16))
        .await?;
    t.look_at(add(cell, [0, -1, 0]));
    t.secs(0.3).await?;
    t.press("core:use");
    t.until("dust on the floor as well", Duration::from_secs(2), |t| t.c().block(cell).is_some_and(|s| wire_sides(s) == 20))
        .await?;
    t.look_at(wall);
    t.secs(0.3).await?;
    t.press("core:attack");
    t.until("the wall's piece gone", Duration::from_secs(2), |t| t.c().block(cell).is_some_and(|s| wire_sides(s) == 4))
        .await?;
    // The one left goes with its floor.
    t.set(add(cell, [0, -1, 0]), "air").await?;
    settle(&t).await?;
    t.check(t.block(cell).await?.contains("air"), "no floor, no dust");
    t.set(add(cell, [0, -1, 0]), "base:stone").await?;

    t.op("put me 1 repeater 1").await?;
    t.c().select_slot(1);
    t.frames(2).await?;
    t.look_at(wall);
    t.secs(0.3).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    let gate = t.block(cell).await?;
    t.check(gate.contains("attach=north"), format!("a repeater on the wall: {gate}"));
    t.check(!gate.contains("front=north") && !gate.contains("front=south"), format!("facing along the wall: {gate}"));
    // A lever on a wall stands upright, whichever way it was looked at.
    t.set(cell, "air").await?;
    t.op("put me 1 lever 1").await?;
    t.frames(2).await?;
    t.press("core:use");
    t.secs(0.3).await?;
    let switch = t.block(cell).await?;
    t.check(switch.contains("attach=north") && switch.contains("front=down"), format!("a lever on the wall: {switch}"));
    Ok(())
}

/// Wire lit and unlit, on a floor and a wall, seen on the screen.
pub async fn redstone_looks(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let z = -5;
    t.fill(add(f, [-6, 0, z - 1]), add(f, [6, 4, z - 1]), "base:snow")
        .await?;
    t.fill(add(f, [-6, -1, z - 1]), add(f, [6, -1, z + 2]), "base:snow")
        .await?;
    t.set(add(f, [0, 0, z]), &lever(false)).await?;
    for x in 1..=3 {
        t.set(add(f, [x, 0, z]), &wire(&[DOWN])).await?;
    }
    t.set(add(f, [4, 0, z]), &wire(&[DOWN, NORTH])).await?;
    for y in 1..=2 {
        t.set(add(f, [4, y, z]), &wire(&[NORTH])).await?;
    }
    t.look_at(add(f, [2, 1, z]));
    t.secs(0.5).await?;
    // Bright red, not the dull red of wire without power.
    let red = |c: probe::C| c[0] > 170 && c[1] < 110 && c[2] < 110;
    let off = t.shot("off").await?.share(0.1, 0.1, 0.9, 0.9, red);
    t.set(add(f, [0, 0, z]), &lever(true)).await?;
    t.secs(0.5).await?;
    let on = t.shot("on").await?.share(0.1, 0.1, 0.9, 0.9, red);
    t.note(format!("bright red in the picture: off {off}, on {on}"));
    t.check(on > off + 0.002, format!("power shows on the wire: {off} then {on}"));

    // Wire that went with power on isn't shown with it when it comes back without.
    t.set(add(f, [2, 0, z]), "air").await?;
    t.set(add(f, [0, 0, z]), &lever(false)).await?;
    t.secs(0.3).await?;
    t.set(add(f, [2, 0, z]), &wire(&[DOWN])).await?;
    t.secs(0.5).await?;
    let back = t.shot("back").await?.share(0.1, 0.1, 0.9, 0.9, red);
    t.check(back == 0.0, format!("nothing bright with the lever off: {back}"));

    // Everything on a floor, a wall and a ceiling, to look at: behind the player.
    let z = 8;
    t.fill(add(f, [-8, 0, z]), add(f, [8, 5, z]), "base:stone")
        .await?;
    t.fill(add(f, [-8, 5, z - 2]), add(f, [8, 5, z - 1]), "base:stone")
        .await?;
    t.fill(add(f, [-8, -1, z - 2]), add(f, [8, -1, z - 1]), "base:stone")
        .await?;
    for (i, name) in ["repeater", "comparator", "lever", "stone_button", "redstone_torch"]
        .iter()
        .enumerate()
    {
        let x = i as i32 * 3 - 7;
        let facing = if *name == "redstone_torch" { String::new() } else { ",front=east".to_string() };
        t.set(add(f, [x, 0, z - 1]), &format!("base:{name}[attach=down{facing}]"))
            .await?;
        let upright = if matches!(*name, "lever" | "stone_button") { ",front=down".to_string() } else { facing.clone() };
        t.set(add(f, [x, 2, z - 1]), &format!("base:{name}[attach=south{upright}]"))
            .await?;
        t.set(add(f, [x, 4, z - 1]), &format!("base:{name}[attach=up{facing}]"))
            .await?;
    }
    t.set(add(f, [8, 0, z - 1]), "base:piston[facing=up]")
        .await?;
    t.set(add(f, [8, 2, z - 1]), "base:observer[facing=north]")
        .await?;
    t.set(add(f, [8, 4, z - 1]), "base:dispenser[facing=north]")
        .await?;
    t.set(add(f, [-8, 0, z - 1]), "base:torch[facing=up]")
        .await?;
    t.set(add(f, [-8, 2, z - 1]), "base:torch[facing=north]")
        .await?;
    t.set(add(f, [-3, 0, z - 2]), "base:piston[facing=north]")
        .await?;
    t.set(add(f, [-3, 1, z - 2]), "base:sticky_piston[facing=north]")
        .await?;
    t.set(add(f, [-3, 2, z - 2]), "base:slime_block").await?;
    t.set(add(f, [-5, 0, z - 2]), "base:piston[facing=west]")
        .await?;
    t.set(add(f, [-5, 1, z - 2]), "base:redstone_block").await?;
    t.c().set_look(180.0, 8.0);
    t.secs(0.5).await?;
    t.shot("gallery").await?;

    // Pistons, an observer and a dispenser up close, and their items.
    let z = -3;
    t.fill(add(f, [-6, 0, z - 1]), add(f, [6, 3, z + 1]), "air")
        .await?;
    t.set(add(f, [-3, 0, z]), "base:piston[facing=up]").await?;
    t.set(add(f, [-2, 0, z]), "base:sticky_piston[facing=up]")
        .await?;
    t.set(add(f, [-1, 0, z]), "base:observer[facing=north]")
        .await?;
    t.set(add(f, [0, 0, z]), "base:dispenser[facing=up]")
        .await?;
    t.set(add(f, [1, 0, z]), "base:slime_block").await?;
    t.set(add(f, [2, 1, z]), "base:redstone_block").await?;
    t.set(add(f, [2, 0, z]), "base:piston[facing=south]")
        .await?;
    t.set(add(f, [3, 1, z]), "base:redstone_block").await?;
    t.set(add(f, [3, 0, z]), "base:sticky_piston[facing=south]")
        .await?;
    for (i, item) in ["piston", "sticky_piston", "observer", "dispenser", "slime_block", "redstone", "lever", "repeater"]
        .iter()
        .enumerate()
    {
        t.op(&format!("put me {i} {item} 1")).await?;
    }
    t.c().set_look(0.0, -32.0);
    t.c().select_slot(8);
    t.secs(1.0).await?;
    t.shot("close").await?;

    Ok(())
}

// The rule that matters most. A random circuit is built in all 48 ways of turning and mirroring it,
// in one tick, and every part must say the same in all of them on every frame.

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Solid(&'static str),
    Wire(u8),
    /// Name, what holds it, where it faces, a fixed property.
    Device(&'static str, Option<u8>, Option<u8>, Option<(&'static str, &'static str)>),
    /// `facing`.
    Faced(&'static str, u8),
    Plain(&'static str),
}

struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 20) as u32 % n
    }
}

const REACH: i32 = 4;

/// Parts of a random circuit, around (0, 0, 0).
fn circuit(seed: u64) -> Vec<([i32; 3], Part)> {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut cells = std::collections::BTreeMap::new();
    let range = -REACH..=REACH;
    for x in range.clone() {
        for y in range.clone() {
            for z in range.clone() {
                match r.next(100) {
                    0..=19 => {
                        cells.insert([x, y, z], Part::Solid("base:stone"));
                    }
                    20..=22 => {
                        cells.insert([x, y, z], Part::Solid("base:glass"));
                    }
                    _ => {}
                }
            }
        }
    }
    let solid = |c: &std::collections::BTreeMap<[i32; 3], Part>, p: [i32; 3]| matches!(c.get(&p), Some(Part::Solid(_)));
    let at = |p: [i32; 3], d: u8| [p[0] + VECS[d as usize][0], p[1] + VECS[d as usize][1], p[2] + VECS[d as usize][2]];
    for x in range.clone() {
        for y in range.clone() {
            for z in range.clone() {
                let p = [x, y, z];
                if cells.contains_key(&p) || r.next(100) > 45 {
                    continue;
                }
                let holds: Vec<u8> = (0..6).filter(|&d| solid(&cells, at(p, d))).collect();
                let attach = (!holds.is_empty()).then(|| holds[r.next(holds.len() as u32) as usize]);
                let turn = r.next(4) as usize;
                let front = |attach: u8| {
                    let along: Vec<u8> = (0..6).filter(|d| d / 2 != attach / 2).collect();
                    along[turn]
                };
                let part = match (r.next(100), attach) {
                    (0..=44, Some(_)) => {
                        let mask = holds.iter().fold(0, |m, d| m | 1 << d);
                        Part::Wire(mask)
                    }
                    (45..=52, Some(a)) => Part::Device("base:redstone_torch", Some(a), None, None),
                    (53..=60, Some(a)) => {
                        let fr = front(a);
                        Part::Device("base:repeater", Some(a), Some(fr), Some(("delay", "2")))
                    }
                    (61..=66, Some(a)) => {
                        let fr = front(a);
                        Part::Device("base:comparator", Some(a), Some(fr), Some(("mode", if fr % 2 == 0 { "subtract" } else { "compare" })))
                    }
                    (67..=71, Some(a)) => {
                        let fr = front(a);
                        Part::Device("base:lever", Some(a), Some(fr), Some(("power", "on")))
                    }
                    (72..=80, _) => Part::Plain("base:redstone_lamp"),
                    (81..=84, _) => Part::Plain("base:redstone_block"),
                    (85..=90, _) => Part::Faced("base:observer", r.next(6) as u8),
                    (91..=95, _) => Part::Faced("base:piston", r.next(6) as u8),
                    (96..=99, _) => Part::Faced("base:sticky_piston", r.next(6) as u8),
                    _ => continue,
                };
                cells.insert(p, part);
            }
        }
    }
    // A clock in a corner, so that there is always something going on: a torch on a block, wire
    // going clear of that block around to a repeater into it.
    for x in -3..=1 {
        for z in -4..=-2 {
            cells.insert([x, -4, z], Part::Solid("base:stone"));
        }
    }
    for p in [[1, -3, -4], [1, -3, -3], [1, -3, -2], [0, -3, -2], [-1, -3, -2], [-2, -3, -2], [-3, -3, -2], [-3, -3, -3], [-3, -3, -4]] {
        cells.insert(p, Part::Wire(1 << DOWN));
    }
    for p in [[0, -3, -3], [-1, -3, -3], [-2, -3, -3]] {
        cells.remove(&p);
    }
    cells.insert([-1, -3, -4], Part::Solid("base:stone"));
    cells.insert([0, -3, -4], Part::Device("base:redstone_torch", Some(0), None, None));
    cells.insert([-2, -3, -4], Part::Device("base:repeater", Some(DOWN), Some(EAST), Some(("delay", "4"))));
    cells.insert([0, -2, -4], Part::Plain("base:redstone_lamp"));
    // The clock took the place of some solid blocks: nothing is left without what holds it, as
    // that would be found out unevenly (a build this big is over the limit of updates a tick).
    let solids = cells.clone();
    cells.retain(|p, part| match part {
        Part::Wire(mask) => {
            *mask = (0..6u8)
                .filter(|d| *mask & (1 << d) != 0 && solid(&solids, at(*p, *d)))
                .fold(0, |m, d| m | 1 << d);
            *mask != 0
        }
        Part::Device(_, Some(a), ..) => solid(&solids, at(*p, *a)),
        _ => true,
    });
    cells.into_iter().collect()
}

/// One of the 48 ways of turning and mirroring: which axis each comes from, and its sign.
fn turning(i: u32) -> ([usize; 3], [i32; 3]) {
    const AXES: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
    let s = |b: u32| if (i >> b) & 1 == 1 { -1 } else { 1 };
    (AXES[(i / 8) as usize], [s(0), s(1), s(2)])
}

fn turned(way: ([usize; 3], [i32; 3]), p: [i32; 3]) -> [i32; 3] {
    std::array::from_fn(|i| p[way.0[i]] * way.1[i])
}

fn turned_dir(way: ([usize; 3], [i32; 3]), d: u8) -> u8 {
    let v = turned(way, VECS[d as usize]);
    VECS.iter().position(|o| *o == v).unwrap_or(0) as u8
}

fn spot(i: u32) -> [i32; 3] {
    const STEP: i32 = REACH * 2 + 4;
    [(i % 7) as i32 * STEP - 3 * STEP, 8, (i / 7) as i32 * STEP - 3 * STEP]
}

fn state_of(part: Part, way: ([usize; 3], [i32; 3])) -> Option<u32> {
    let first = |name: &str| Block::lookup(name);
    Some(match part {
        Part::Solid(n) | Part::Plain(n) => first(n)?,
        Part::Wire(mask) => (0..6u8)
            .filter(|d| mask & (1 << d) != 0)
            .try_fold(first("base:redstone")?, |s, d| state_with(s, NAMES[turned_dir(way, d) as usize], "true"))?,
        Part::Device(n, attach, front, prop) => {
            let mut s = first(n)?;
            if let Some(a) = attach {
                s = state_with(s, "attach", NAMES[turned_dir(way, a) as usize])?;
            }
            if let Some(fr) = front {
                s = state_with(s, "front", NAMES[turned_dir(way, fr) as usize])?;
            }
            if let Some((k, v)) = prop {
                s = state_with(s, k, v)?;
            }
            s
        }
        Part::Faced(n, d) => state_with(first(n)?, "facing", NAMES[turned_dir(way, d) as usize])?,
    })
}

/// The server's side of `redstone_turned`, for all of it to start in the same tick.
pub fn build_turned(w: &World, seed: u64, from: [i32; 3]) -> Result<String, String> {
    let parts = circuit(seed);
    let air = 0;
    for i in 0..48 {
        let o = spot(i);
        let at = |p: [i32; 3]| [from[0] + o[0] + p[0], from[1] + o[1] + p[1], from[2] + o[2] + p[2]];
        for x in -REACH..=REACH {
            for y in -REACH..=REACH {
                for z in -REACH..=REACH {
                    if !w.set_block(at([x, y, z]), air) {
                        return Err(format!("{:?} is not loaded", at([x, y, z])));
                    }
                }
            }
        }
    }
    // Solid first: what isn't holds on to it.
    for solid in [true, false] {
        for i in 0..48 {
            let way = turning(i);
            let o = spot(i);
            for &(p, part) in &parts {
                if matches!(part, Part::Solid(_)) != solid {
                    continue;
                }
                let q = turned(way, p);
                let state = state_of(part, way).ok_or("a state of the circuit doesn't exist")?;
                w.set_block([from[0] + o[0] + q[0], from[1] + o[1] + q[1], from[2] + o[2] + q[2]], state);
            }
        }
    }
    Ok(String::new())
}

/// Wire and devices on a moving structure that came from a save run, and don't bring the game down.
pub async fn redstone_on_a_structure(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let id = t
        .op(&format!("spawn testmod:lift {} {} {}", f[0] + 4, f[1] + 2, f[2] - 4))
        .await?;
    t.frames(5).await?;
    // Added by what mods can do.
    let structure_set = |x: i32, block: &str| format!("structureset {id} {x} 1 0 {block}");
    t.op(&structure_set(0, "base:redstone_block")).await?;
    t.op(&structure_set(1, "base:redstone[down=true]")).await?;
    t.frames(5).await?;
    let name = format!("test-{}", t.name());
    t.c().world_leave();
    t.screen("core:title").await?;
    t.c().world_open(&name, 1);
    t.until("joined again", Duration::from_secs(60), |t| t.flag(client::client_flags::JOINED))
        .await?;
    t.loaded().await?;
    let list = t
        .op(&format!("near {} {} {} 40", f[0] + 4, f[1] + 2, f[2] - 4))
        .await?;
    let id = list
        .split_whitespace()
        .find(|e| e.ends_with("testmod:lift"))
        .and_then(|e| e.split(':').next())
        .ok_or(format!("no lift among {list}"))?
        .to_string();
    // Wire next to what was saved, which is what used to be fatal.
    t.op(&format!("structureset {id} 2 1 0 base:redstone[down=true]"))
        .await?;
    settle(&t).await?;
    t.check(t.state().frame > 0, "still here");

    // Wire on it is drawn, in its power's color, where the structure is.
    t.op(&format!("structureset {id} 0 0 -1 base:stone"))
        .await?;
    t.op(&format!("structureset {id} 0 1 -1 base:redstone[down=true]"))
        .await?;
    let uid: u64 = id.parse().map_err(|_| "not an id".to_string())?;
    // From above.
    t.c().set_flying(true);
    t.cmd(&format!("/tp {} {} {}", f[0] + 4, f[1] + 7, f[2] - 8))
        .await?;
    t.secs(0.5).await?;
    let mut red = false;
    for _ in 0..40 {
        t.frames(1).await?;
        let Some(o) = t.c().entity(uid).map(|e| e.pos) else { continue };
        let at = [o.x + 0.0, o.y + 1.03, o.z - 1.0];
        let st = t.state();
        let (dx, dy, dz) = (at[0] - st.shown.x, at[1] - st.shown.y - 1.62, at[2] - st.shown.z);
        t.c()
            .set_look((-dx).atan2(-dz).to_degrees() as f32, dy.atan2((dx * dx + dz * dz).sqrt()).to_degrees() as f32);
        t.frames(2).await?;
        let Some(o) = t.c().entity(uid).map(|e| e.pos) else { continue };
        if let Some((x, y)) = t.on_screen([o.x + 0.0, o.y + 1.03, o.z - 1.0]) {
            // Beside the crosshair.
            let px = t.pixel(x + 0.012, y + 0.01).await?;
            red |= px[0] as i32 > px[1] as i32 * 2 && px[0] > 60;
        }
    }
    t.check(red, "wire on it shows red");

    // A piston on it: what it moves goes on showing as a body that goes with the structure too,
    // and ends up where it was going on the structure.
    t.op(&format!("structureset {id} 0 2 0 base:cobblestone"))
        .await?;
    t.op(&format!("structureset {id} -1 2 0 base:piston[facing=east]"))
        .await?;
    let (mut seen, mut carried) = (false, false);
    let at = |t: T, id: String| async move {
        let line = t.op(&format!("entity {id}")).await?;
        let v: Vec<f64> = line
            .split_whitespace()
            .skip(1)
            .take(3)
            .filter_map(|s| s.parse().ok())
            .collect();
        Ok::<_, String>(v)
    };
    let lift = at(t.clone(), id.clone()).await?;
    t.op(&format!("structureset {id} -1 3 0 base:redstone_block"))
        .await?;
    for _ in 0..8 {
        let list = t
            .op(&format!("near {} {} {} 8", lift[0].floor(), lift[1].floor(), lift[2].floor()))
            .await?;
        if let Some(m) = list
            .split_whitespace()
            .find(|e| e.ends_with("core:moving_blocks"))
            .and_then(|e| e.split(':').next())
        {
            seen = true;
            let (l, b) = (at(t.clone(), id.clone()).await?, at(t.clone(), m.to_string()).await?);
            carried |= b.len() == 3 && (b[1] - l[1] - 2.0).abs() < 0.3;
        }
    }
    t.check(seen, "what the piston moves is a body of the region, not lost inside the structure");
    t.check(carried, "at the height of the structure it is on");
    settle(&t).await?;
    t.check_eq(t.op(&format!("sblock {id} 1 2 0")).await?, "base:cobblestone".to_string(), "and it got there on the structure");
    Ok(())
}

/// A circuit is as it was after leaving the world, wire power on the screen too, and a clock goes on.
pub async fn redstone_saved(t: T) -> R {
    t.world(1).await?;
    t.op("mode creative").await?;
    let f = add(t.feet(), [0, 0, 0]);
    let z = -4;
    t.fill(add(f, [-1, -1, z - 8]), add(f, [9, -1, z + 3]), "base:snow")
        .await?;
    t.fill(add(f, [-1, 0, z - 8]), add(f, [9, 5, z + 3]), "air")
        .await?;
    t.set(add(f, [0, 0, z]), &lever(true)).await?;
    run_of_wire(&t, f, 1, 4, z).await?;
    let lamp = add(f, [5, 0, z]);
    t.set(lamp, "base:redstone_lamp").await?;
    let torch = add(f, [4, 0, z - 6]);
    t.op(&format!("rsclock {} {} {} 0", f[0] + 3, f[1], f[2] + z - 6))
        .await?;
    t.look_at(add(f, [2, 0, z]));
    settle(&t).await?;
    let red = |c: probe::C| c[0] > 170 && c[1] < 110 && c[2] < 110;
    let before = t.shot("before").await?.share(0.05, 0.05, 0.95, 0.95, red);
    t.check(lit(&t, lamp).await?, "lit before");

    t.c().world_leave();
    t.screen("core:title").await?;
    t.c().world_open(&format!("test-{}", t.name()), 1);
    t.loaded().await?;
    t.look_at(add(f, [2, 0, z]));
    settle(&t).await?;
    t.check(lit(&t, lamp).await?, "lit after");
    let after = t.shot("after").await?.share(0.05, 0.05, 0.95, 0.95, red);
    t.note(format!("bright wire on the screen: {before} then {after}"));
    t.check(after > 0.001, format!("wire with power after coming back: {after}"));
    let on = |t: &T| {
        t.c()
            .block(torch)
            .is_some_and(|s| state_prop(s, "power") == Some("on"))
    };
    let mut flips = 0;
    let mut was = on(&t);
    for _ in 0..100 {
        t.secs(0.02).await?;
        let now = on(&t);
        flips += (now != was) as u32;
        was = now;
    }
    t.check(flips > 0, "the clock still goes");
    Ok(())
}

/// A torch fed back through a repeater goes on and off for as long as it is left alone, slowly
/// enough not to burn out, and burns out when it is too quick.
pub async fn redstone_clock(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let torch = add(f, [4, 0, -4]);
    t.op(&format!("rsclock {} {} {} 0", f[0] + 3, f[1], f[2] - 4))
        .await?;
    let on = |t: &T, p: [i32; 3]| {
        t.c()
            .block(p)
            .is_some_and(|s| state_prop(s, "power") == Some("on"))
    };
    let mut flips = 0;
    let mut was = on(&t, torch);
    for _ in 0..200 {
        t.secs(0.02).await?;
        let now = on(&t, torch);
        flips += (now != was) as u32;
        was = now;
    }
    t.probe("flips of a clock in four seconds", flips as f32, (4..=12).contains(&flips));

    // A torch on a block and a repeater of the shortest delay: burns out.
    let z = -10;
    let quick = add(f, [4, 0, z]);
    t.set(add(f, [3, 0, z]), "base:stone").await?;
    t.set(quick, "base:redstone_torch[attach=west]").await?;
    t.set(add(f, [2, 0, z]), &on_floor("repeater", EAST, ",delay=1"))
        .await?;
    for (x, zz) in [(5, z), (5, z + 1), (5, z + 2), (4, z + 2), (3, z + 2), (2, z + 2), (1, z + 2), (1, z + 1), (1, z)] {
        t.set(add(f, [x, 0, zz]), &wire(&[DOWN])).await?;
    }
    t.secs(2.5).await?;
    let mut flips = 0;
    let mut was = on(&t, quick);
    for _ in 0..60 {
        t.secs(0.02).await?;
        let now = on(&t, quick);
        flips += (now != was) as u32;
        was = now;
    }
    t.check(flips <= 1, format!("a burnt out torch keeps still: {flips} flips"));
    Ok(())
}

/// A build behaves the same however it is turned or mirrored, every moment.
pub async fn redstone_turned(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let mut flips = 0u32;
    for seed in 1..=6u64 {
        t.op(&format!("rsbuild {} {} {} {seed}", f[0], f[1], f[2]))
            .await?;
        let parts: Vec<[i32; 3]> = circuit(seed)
            .iter()
            .filter(|(_, p)| !matches!(p, Part::Solid(_) | Part::Wire(_)))
            .map(|(p, _)| *p)
            .collect();
        let cells: Vec<Vec<[i32; 3]>> = (0..48)
            .map(|i| {
                let o = spot(i);
                parts
                    .iter()
                    .map(|p| {
                        let q = turned(turning(i), *p);
                        [f[0] + o[0] + q[0], f[1] + o[1] + q[1], f[2] + o[2] + q[2]]
                    })
                    .collect()
            })
            .collect();
        let mut last: Vec<Option<&'static str>> = Vec::new();
        for frame in 0..180 {
            t.frames(1).await?;
            let read = |i: usize| -> Vec<Option<&'static str>> {
                cells[i]
                    .iter()
                    .map(|c| t.c().block(*c).and_then(|s| state_prop(s, "power")))
                    .collect()
            };
            let first = read(0);
            for i in 1..48 {
                let other = read(i);
                if other != first {
                    let n = first
                        .iter()
                        .zip(&other)
                        .position(|(a, b)| a != b)
                        .unwrap_or(0);
                    t.fail(format!(
                        "seed {seed}, frame {frame}, way {i}: the part at {:?} is {:?}, unturned {:?}",
                        parts[n], other[n], first[n]
                    ));
                    return Ok(());
                }
            }
            flips += first.iter().zip(&last).filter(|(a, b)| a != b).count() as u32;
            last = first;
        }
    }
    t.note(format!("{flips} changes of parts seen, the same in all 48"));
    t.check(flips > 20, format!("circuits doing something: {flips} changes"));
    Ok(())
}

/// Numbers: what circuits cost on the server. A big idle base should be nothing.
pub async fn redstone_bench(t: T) -> R {
    t.flat().await?;
    let f = t.feet();
    let empty = t.bench(1.5, "redstone: nothing").await?;

    // A plate of wire, 256 x 256, idle.
    t.op(&format!("rsfill {} {} {} 128", f[0], f[1], f[2]))
        .await?;
    t.secs(1.0).await?;
    let idle = t.bench(1.5, "redstone: 65k pieces of wire, idle").await?;

    // The same plate fed by a clock at one corner.
    t.op(&format!("rsclock {} {} {} 0", f[0] - 128, f[1], f[2] - 128))
        .await?;
    t.secs(0.5).await?;
    let fed = t.bench(1.5, "redstone: that wire on a clock").await?;
    t.note(format!("server ms a tick: nothing {:.3}, idle wire {:.3}, wire on a clock {:.3}", empty[4], idle[4], fed[4]));
    t.op(&format!("rsclear {} {} {} 128", f[0], f[1], f[2]))
        .await?;

    // A thousand clocks of a torch and repeater each, going all the time.
    let built = t
        .op(&format!("rsclocks {} {} {} 1000", f[0], f[1], f[2]))
        .await?;
    t.secs(0.5).await?;
    let clocks = t.bench(2.0, "redstone: 1000 clocks").await?;
    // That they go, and how much: the first clock's torch over three seconds.
    let side = 32;
    let torch = [f[0] + (side / 2) * 6 - side * 3 + 1, f[1], f[2] + (side / 2) * 6 - side * 3];
    let mut flips = 0;
    let mut was = t.c().block(torch);
    for _ in 0..120 {
        t.secs(0.025).await?;
        let now = t.c().block(torch);
        flips += (now != was) as u32;
        was = now;
    }
    t.note(format!("server ms a tick with {built} clocks: {:.3}; a clock flipped {flips} times in 3 seconds", clocks[4]));

    // Light on a body going by itself: each block it crosses relights and sends cubes.
    let skiff = |dy: i32| format!("spawn testmod:skiff {} {} {}", f[0] - 8, f[1] + 20 + dy, f[2] - 40);
    let plain = t.op(&skiff(0)).await?;
    t.secs(0.3).await?;
    let without = t.bench(2.0, "a structure going by").await?;
    t.op(&format!("remove {plain}")).await?;
    let lit = t.op(&skiff(0)).await?;
    t.op(&format!("structureset {lit} 0 1 0 base:glowstone"))
        .await?;
    t.secs(0.3).await?;
    let with = t.bench(2.0, "a structure with a light going by").await?;
    t.op(&format!("remove {lit}")).await?;
    t.note(format!(
        "a light on a body 6 blocks a second: frame {:.2} to {:.2} ms, p99 {:.2} to {:.2}, server {:.3} to {:.3}",
        without[1], with[1], without[2], with[2], without[4], with[4]
    ));
    t.check(flips > 0, "the clocks are going");
    Ok(())
}
