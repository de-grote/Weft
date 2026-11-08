# Redstone guide

Redstone here isn't vanilla's. The numbers are familiar (power 0 to 15, a redstone tick is 2 game ticks), but the rules are simpler and they don't care which way you build:

- A build works the same in any direction, turned or mirrored. There's no update order to fight and no quasi-connectivity, so 0-tick tricks and the like don't exist.
- Wire goes on floors, walls and ceilings. Gates and switches go on the face you click, like torches.
- A circuit that isn't changing costs nothing, however big it is.

Vanilla circuits that rely on those quirks behave differently. That's on purpose.

## Quick links
[The rules](#the-rules) | [What's in the game](#whats-in-the-game) | [Making your own](#making-your-own) | [Reading it from code](#reading-it-from-code) | [Limits](#limits)

## The rules

**Wire** lies on any face of a solid block, several faces in one cell if you like (it is an `on_sides` block, `west`..`south` properties saying where it lies). A piece connects to the next over each of its edges: into the corner of a wall in the same cell, on along the same plane, or around the edge of the block it lies on. Going up a step means going floor, wall, top. Power drops by one with each piece, so it reaches fifteen pieces.

Only wire of the same block connects, so another wire block is an insulated wire.

**Power reaches a block two ways:**

- *Strongly*, from a gate facing it, a lever on it, or a torch next to it (not the block the torch is on). Wire touching that block takes the power, and so do devices.
- *Weakly*, from wire lying on it. Only devices next to it take that. Otherwise all the wires on one wall would be one wire.

Solid blocks carry power by default, lamps and other blocks that only take power included. `.signal_blocker()` turns that off, the way glass, ice, leaves and the golden lamp have it. Wire can still lie on them. Slabs and other partial solid blocks carry power too.

**Wire reaches** the block it lies on and any device touching one of its edges or its back. It doesn't reach across the cell, so wire on a floor doesn't touch what is on the ceiling above it. Wire running past a lamp lights it. To get past without lighting it, take another face.

**Timing:** gates act after their delay and wire is instant. A piston takes 2 ticks: what it moves is there for circuits then, and is drawn as a block 2 ticks later, when the moving picture has arrived. All of a tick is read as the last tick left it, then applied, which is why order doesn't show. Every loop goes through a gate, so nothing can hang a tick.

**Pistons** move chests and anything else with an entity, which stays the same one, with what is in it. Those that want the same block in the same tick both give way, since letting one win would favor a direction, and the one still wanting it goes as soon as the other stops. Two sticky ones bringing back one block both come back without it. A sticky piston that loses power within 2 ticks of starting comes back without its block. A piston being pushed by another doesn't count as wanting anything: it waits, and goes from where it lands if it has power there.

**On what moves:** circuits go on on a piston's blocks and on a body, pulses on their way included, and a piston moving a block takes what stands on it: dust, gates, torches, levers. Only dust lying directly in the way breaks. Being on a piston's blocks, a piston waits till it lands. Bodies and the world power each other where they are next to each other: a redstone block going past lamps lights them, and a lamp going past one is lit. Wire of a body and wire of the world don't join or feed each other, though each lights what it touches on the other side. An observer sees a body come and go, and one on a body sees the world change as it goes.

**Unloaded blocks:** what can't see all of its inputs stays as it is until they load, instead of flipping. A saved world starts the same way. A pulse in flight inside a repeater isn't saved, and doesn't survive parts of the world joining into one region or splitting. Blocks on the way from a piston get there at once when that happens or when the world saves.

## What's in the game

| Block | What it does |
|---|---|
| `redstone` | The dust, one piece per click |
| `lever` | Toggles, strongly powers what it's on |
| `stone_button` | On for a second |
| `redstone_block` | Always on, no strong power |
| `redstone_torch` | Off while what it's on has power. Strongly powers every other block around it, so one on a wall powers both the block in front and the one above. 8 flips in 3 seconds burn it out for 8 |
| `repeater` | Full power out the front, 1 to 4 redstone ticks after the back gets any (click to change). A gate pointing into its side locks it |
| `comparator` | Back against the sides; the other mode subtracts (click to change). Reads how full a container behind it is |
| `redstone_lamp` | Lit with any power around it. Carries power like any solid block |
| `piston`, `sticky_piston` | Pushes up to 12 blocks, powered from any side but its front. Breaks what isn't solid, stops at block entities and `.immovable()` blocks. The blocks move as a body, so whoever stands on them rides along smoothly |
| `slime_block` | What a piston moves takes what touches it along, for flying machines. `.sticky()` on any block does the same |
| `floater` | What is fixed to its blocks comes along; what runs into a solid block on the way comes off and drops while it all goes on, so a torch or lever feeding it from the leading face stops it on arrival. Goes the way you look when placing it, any of the six, its green front first. With power from any other side it carries itself, the block in front of it and what is stuck to that through slime, half a block a second per level of power, circuits going on. Nothing else it touches goes along: the floor and what feeds it stay. Without power, with its floaters broken off, or at something solid in the way, it all lands on a whole block. It shows red when it can't go; a comparator reads 1 going, 15 stuck |
| `golden_lamp` | Each new pulse flips it. A comparator reads 15 while it is lit. Doesn't carry power |
| `anchor` | With power it can't be moved: in a row being pushed it stops the push, touching slime it stays behind. On a platform on its way, getting power with more of the platform pushing it stops the platform at the next whole block; with nothing pushing it, it rides on. Without power it moves like any block |
| `observer` | A 1 redstone tick pulse out of its back, 1 redstone tick after the block it looks at changes (wire's power too), and when a piston moves it |
| `dispenser` | One item out of the front for each new pulse |

## Making your own

Say what a block is and the engine runs it. No ticking and no neighbor callbacks to write.

```rust
// Always on.
r.block(&CRYSTAL).signal_constant();

// Follows the power around it: a `power` property (`off`, `on`) changes by itself.
r.block(&LIGHT)
    .signal_sink()
    .prop("power", ["off", "on"])
    .states(|p, s| if p.is("power", "on") { s.light(15) } else { s });

// The same with code: called when the power around it changes.
r.block(&ALARM).signal_sink().on_signal(|e| {
    if e.rising() {
        e.world().spawn_item(/* ... */);
    }
});

// A gate with your own logic, on the clicked face. Runs only when something around it changes,
// and what it returns comes out of the front after the delay (in redstone ticks, like a repeater's).
// `Gate::new()` reads behind it and both sides; `.inputs(&[Side::Left, Side::Right, Side::Top])`
// names other faces, `.outputs(&[Side::Front, Side::Top])` gives at more than the front, `.delay(4)`
// waits longer. Sides are the gate's own, so they turn with it.
r.block(&AND_GATE)
    .signal_gate(Gate::new(), |s| if s.side(Side::Rear) > 0 && (s.side(Side::Left) > 0 || s.side(Side::Right) > 0) { 15 } else { 0 })
    .model_box([0.0, 0.0, 0.0], [16.0, 2.0, 16.0])
    .not_solid();

// Wire that doesn't join base's. Textures: the dot on top, the line on the sides, in gray; power
// colors it.
r.block(&BLUE_WIRE).wire().textures("blue_dot", "blue_line", "blue_dot");

// Glass-like, and something a piston can't move.
r.block(&CASING).signal_blocker();
r.block(&BEDROCK_ISH).immovable();
```

The rest of the set, each with the properties it needs added for you:

- `.signal_switch()`: a lever or button. Goes on the clicked face; flip its `power` property from `on_use`.
- `.signal_torch()`, `.signal_repeater()`, `.signal_comparator()`: as in the game.
- `.signal_read(|w, p| level)`: what a comparator behind the block reads, instead of how full it is. It looks again when the block changes; `w.signal_read_changed(p)` for anything else it depends on.
- `.signal_floater(breaks)` after `.facing(..)`: a floater; `breaks` makes it break what is in its way. `power` is 0 idle, 1 going, 2 stuck.
- `w.body_from_blocks(center, &cells)` takes blocks out of the world into a moving body, with chests and what is in them, circuits going on. Move it with `w.entity_modify(id).velocity(..)` or `w.body_move_to(id, pos, ticks)`, and `w.body_to_blocks(id)` makes it blocks again, on the nearest whole block. Its blocks are reached the way the world's are, through `w.on_body(id)`: `w.on_body(ship)?.prop_set(pos, "lit", "true")` in the body's own coordinates, with parts, scheduled ticks and signals as in the world. `w.push_blocks(pos, dir)` moves blocks the way a piston does, without one: what would be pushed goes one block that way, slime and all, so a machine can open a drawbridge. `w.body_fall(id)` lets it drop instead, like sand: each block lands where it comes down, or drops if it can't stand there, so a felled tree or a collapsing roof is a lift and a fall.
- `.signal_observer()` and `.signal_piston(head, sticky)`, `.signal_piston_head()`: after `.facing(..)`.

**A model on any face:** those blocks have `attach` and `front` properties. Write the model once for a floor with the front to the north, and the block turns it onto its face, pictures included:

```rust
r.block(&GATE).signal_repeater().states(|p, s| {
    let top = if p.is("power", "on") { "gate_on" } else { "gate" };
    let mut faces = [("smooth_stone", UV_FROM_BOX); 6];
    faces[3].0 = top;
    s.model_box_faces([0.0, 0.0, 0.0], [16.0, 2.0, 16.0], faces)
});
```

A block with `facing` turns the same way, its front to it; `s.orientation(Orientation::up(p.facing()))` turns its top that way instead, like a piston.

`base` is written with exactly this: see `mods/base/src/redstone.rs`.

## Reading it from code

`w.signal(pos)` is the strongest power coming into a cell, 0 to 15.

## Limits

- Wire on moving bodies runs but isn't drawn.
- A gate changing is still a block change, so on the client it remeshes its cube. Only wire avoids that.
