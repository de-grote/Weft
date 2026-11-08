//! Redstone blocks. The engine runs them from what they say they are (`signal_*`); this is their
//! look and what they do with a player. Gates and switches go on any face.

use weft_sdk::*;

pub static REDSTONE: Block = Block::named("redstone");
pub static REDSTONE_ORE: Block = Block::named("redstone_ore");
pub static REDSTONE_BLOCK: Block = Block::named("redstone_block");
pub static REDSTONE_TORCH: Block = Block::named("redstone_torch");
pub static REPEATER: Block = Block::named("repeater");
pub static COMPARATOR: Block = Block::named("comparator");
pub static LEVER: Block = Block::named("lever");
pub static BUTTON: Block = Block::named("stone_button");
pub static LAMP: Block = Block::named("redstone_lamp");
pub static PISTON: Block = Block::named("piston");
pub static STICKY_PISTON: Block = Block::named("sticky_piston");
pub static PISTON_HEAD: Block = Block::named("piston_head");
pub static STICKY_PISTON_HEAD: Block = Block::named("sticky_piston_head");
pub static OBSERVER: Block = Block::named("observer");
pub static DISPENSER: Block = Block::named("dispenser");
pub static DISPENSER_MENU: Menu = Menu::named("dispenser");
pub static SLIME_BLOCK: Block = Block::named("slime_block");
pub static ANCHOR: Block = Block::named("anchor");
pub static FLOATER: Block = Block::named("floater");
pub static GOLDEN_LAMP: Block = Block::named("golden_lamp");

const BUTTON_TICKS: u32 = 20;

fn all(t: &str) -> [(&str, [f32; 4]); 6] {
    [(t, UV_FROM_BOX); 6]
}

/// The post's lower end cut away, on a gate where only its top shows.
fn short_post(t: &str) -> [(&str, [f32; 4]); 6] {
    let side = [7.0, 6.0, 9.0, 11.0];
    [(t, side), (t, side), (t, [7.0, 13.0, 9.0, 15.0]), (t, [7.0, 6.0, 9.0, 8.0]), (t, side), (t, side)]
}

/// A gate's slab: its picture on top, power going toward the top of that picture.
fn slab(top: &str) -> [(&str, [f32; 4]); 6] {
    let mut f = all("smooth_stone");
    f[3].0 = top;
    f
}

fn toggle(u: &mut Use) -> bool {
    u.world().prop_cycle(u.pos(), "power");
    true
}

pub fn register(r: &mut Registry) {
    // One item, on whichever face is clicked. Each piece breaks and drops by itself.
    r.block(&REDSTONE)
        .wire()
        .textures("redstone_dust_dot", "redstone_dust_line", "redstone_dust_dot")
        .item_texture("redstone")
        .sound_group("stone")
        .tag("dusts/redstone")
        .tab(&crate::tabs::REDSTONE);

    r.block(&REDSTONE_ORE)
        .sound_group("stone")
        .hardness(3.0)
        .mined_by("pickaxe", tool_tier::IRON)
        .drops("redstone", 4)
        .tag("ores")
        .tag("ores/redstone")
        .tab(&crate::tabs::NATURAL);

    r.block(&REDSTONE_BLOCK)
        .signal_constant()
        .sound_group("stone")
        .hardness(5.0)
        .mined_by("pickaxe", 0)
        .tag("ore_blocks/redstone")
        .tab(&crate::tabs::REDSTONE);

    r.block(&LAMP)
        .signal_sink()
        .prop("power", ["off", "on"])
        .states(|p, s| if p.is("power", "on") { s.texture("redstone_lamp_on").light(15) } else { s.texture("redstone_lamp") })
        .sound_group("glass")
        .hardness(0.3)
        .tab(&crate::tabs::REDSTONE);

    r.block(&REDSTONE_TORCH)
        .signal_torch()
        .states(|p, s| {
            let on = p.is("power", "on");
            let attach = Dir::from_name(p.get("attach")).unwrap_or(Dir::Down);
            let s = crate::blocks::torch_state(attach.opposite(), s, if on { "redstone_torch" } else { "redstone_torch_off" });
            if on { s.light(7) } else { s }
        })
        .not_solid()
        .hardness(0.0)
        .sound_group("wood")
        .item_texture("redstone_torch")
        .tab(&crate::tabs::REDSTONE);

    r.block(&REPEATER)
        .signal_repeater()
        .states(|p, s| {
            let on = p.is("power", "on");
            let (top, torch) = if on { ("repeater_on", "redstone_torch") } else { ("repeater", "redstone_torch_off") };
            // The second post moves back with the delay.
            let z = 4.0 + p.int("delay") as f32 * 2.0;
            s.model_box_faces([0.0, 0.0, 0.0], [16.0, 2.0, 16.0], slab(top))
                .model_box_faces([7.0, 2.0, 2.0], [9.0, 7.0, 4.0], short_post(torch))
                .model_box_faces([7.0, 2.0, z], [9.0, 7.0, z + 2.0], short_post(torch))
        })
        .on_use(|u| {
            u.world().prop_cycle(u.pos(), "delay");
            true
        })
        .not_solid()
        .hardness(0.0)
        .sound_group("stone")
        .item_texture("repeater")
        .tab(&crate::tabs::REDSTONE);

    r.block(&COMPARATOR)
        .signal_comparator()
        .states(|p, s| {
            let on = p.int("power") > 0;
            let subtract = p.is("mode", "subtract");
            let (top, torch) = if on { ("comparator_on", "redstone_torch") } else { ("comparator", "redstone_torch_off") };
            s.model_box_faces([0.0, 0.0, 0.0], [16.0, 2.0, 16.0], slab(top))
                .model_box_faces([4.0, 2.0, 11.0], [6.0, 7.0, 13.0], short_post(torch))
                .model_box_faces([10.0, 2.0, 11.0], [12.0, 7.0, 13.0], short_post(torch))
                .model_box_faces(
                    [7.0, 2.0, 2.0],
                    [9.0, 5.0, 4.0],
                    short_post(if subtract { "redstone_torch" } else { "redstone_torch_off" }),
                )
        })
        .on_use(|u| {
            u.world().prop_cycle(u.pos(), "mode");
            true
        })
        .not_solid()
        .hardness(0.0)
        .sound_group("stone")
        .item_texture("comparator")
        .tab(&crate::tabs::REDSTONE);

    r.block(&LEVER)
        .signal_switch()
        .states(|p, s| {
            // Vanilla's: the handle over one way or the other, about the middle of the base.
            let lean = if p.is("power", "on") { -45.0 } else { 45.0 };
            let (top, side, end) = ([5.0, 4.0, 11.0, 12.0], [5.0, 0.0, 11.0, 3.0], [4.0, 0.0, 12.0, 3.0]);
            let base = [
                ("cobblestone", end),
                ("cobblestone", end),
                ("cobblestone", top),
                ("cobblestone", top),
                ("cobblestone", side),
                ("cobblestone", side),
            ];
            s.model_box_faces([5.0, 0.0, 4.0], [11.0, 3.0, 12.0], base)
                .model_box_faces([7.0, 1.0, 7.0], [9.0, 11.0, 9.0], crate::blocks::stick_faces("lever"))
                .tilted([8.0, 1.0, 8.0], Dir::East, lean)
                .hitbox([5.0, 0.0, 4.0], [11.0, 6.0, 12.0])
        })
        .on_use(|u| toggle(u))
        .not_solid()
        .hardness(0.5)
        .sound_group("wood")
        .item_texture("lever")
        .tab(&crate::tabs::REDSTONE);

    r.block(&BUTTON)
        .signal_switch()
        .states(|p, s| {
            let height = if p.is("power", "on") { 1.0 } else { 2.0 };
            s.model_box_faces([5.0, 0.0, 6.0], [11.0, height, 10.0], all("button"))
        })
        .on_use(|u| {
            let (w, p) = (u.world(), u.pos());
            // Pressed already: it comes up when its time is over.
            if w.prop(p, "power") != Some("on") {
                w.prop_set(p, "power", "on");
                w.schedule_tick(p, BUTTON_TICKS);
            }
            true
        })
        .on_scheduled(|e| {
            e.world().prop_set(e.pos(), "power", "off");
        })
        .not_solid()
        .hardness(0.5)
        .sound_group("stone")
        .texture("stone")
        .tab(&crate::tabs::REDSTONE);

    for (piston, head, sticky, name) in
        [(&PISTON, &PISTON_HEAD, false, "base:piston_head"), (&STICKY_PISTON, &STICKY_PISTON_HEAD, true, "base:sticky_piston_head")]
    {
        let top = if sticky { "piston_top_sticky" } else { "piston_top" };
        r.block(piston)
            .facing(Facings::ALL, Toward::Player)
            .default_facing(Dir::Up)
            .signal_piston(name, sticky)
            .states(move |p, s| {
                let mut f = all("piston_side");
                f[2].0 = "piston_bottom";
                let s = s.orientation(Orientation::up(p.facing()));
                if p.is("power", "on") {
                    f[3].0 = "piston_inner";
                    s.model_box_faces([0.0, 0.0, 0.0], [16.0, 12.0, 16.0], f)
                } else {
                    f[3].0 = top;
                    s.model_box_faces([0.0, 0.0, 0.0], [16.0, 16.0, 16.0], f)
                        .light_opacity(15)
                }
            })
            .sound_group("stone")
            .hardness(1.5)
            .mined_by("pickaxe", tool_tier::HAND)
            .tab(&crate::tabs::REDSTONE);
        r.block(head)
            .facing(Facings::ALL, Toward::Player)
            .signal_piston_head()
            // The short one is what moves while it is inside the piston.
            .prop("short", ["false", "true"])
            .states(move |p, s| {
                // Vanilla's: a plate, and a rod reaching back into the piston.
                let strip = ("piston_side", [0.0, 0.0, 16.0, 4.0]);
                let plate = [strip, strip, ("piston_top", UV_FROM_BOX), (top, UV_FROM_BOX), strip, strip];
                let none = ("piston_side", UV_NO_FACE);
                let rod = [strip, strip, none, none, strip, strip];
                let from = if p.is("short", "true") { 0.0 } else { -4.0 };
                s.orientation(Orientation::up(p.facing()))
                    .model_box_faces([0.0, 12.0, 0.0], [16.0, 16.0, 16.0], plate)
                    .model_box_faces([6.0, from, 6.0], [10.0, 12.0, 10.0], rod)
                    .turned([1, 1, 0, 0, 1, 1])
                    .hitbox([0.0, 12.0, 0.0], [16.0, 16.0, 16.0])
                    .hitbox([6.0, 0.0, 6.0], [10.0, 12.0, 10.0])
            })
            .sound_group("stone")
            .hardness(1.5)
            .mined_by("pickaxe", tool_tier::HAND)
            .drops_nothing()
            .no_item();
    }

    r.block(&SLIME_BLOCK)
        .sticky()
        .translucent()
        .light_opacity(1)
        .friction(0.8)
        .hardness(0.0)
        .sound_group("cloth")
        .tab(&crate::tabs::REDSTONE);

    // In a row being pushed it stops the push, also of a platform on its way; touching slime it
    // just stays behind.
    r.block(&ANCHOR)
        .signal_sink()
        .prop("power", ["off", "on"])
        .states(|p, s| if p.is("power", "on") { s.texture("anchor_on").immovable() } else { s.texture("anchor") })
        .sound_group("stone")
        .hardness(5.0)
        .mined_by("pickaxe", 0)
        .tab(&crate::tabs::REDSTONE);

    r.block(&FLOATER)
        // It goes where the player looks, and what it was placed against is what it takes along.
        .facing(Facings::ALL, Toward::Away)
        .signal_floater(false)
        .states(|p, s| {
            // Arrows on the sides, toward the sticky front, in the color of how it is doing.
            let side = match p.int("power") {
                1 => "floater_side_going",
                2 => "floater_side_stuck",
                _ => "floater_side",
            };
            let back = match p.int("power") {
                1 => "floater_back_going",
                2 => "floater_back_going",
                _ => "floater_back",
            };
            let whole = [0.0, 0.0, 16.0, 16.0];
            let faces = [(side, whole), (side, whole), (side, whole), (side, whole), ("floater_front", whole), (back, whole)];
            s.model_box_faces([0.0, 0.0, 0.0], [16.0, 16.0, 16.0], faces)
                .turned([1, 3, 2, 0, 0, 0])
                .light_opacity(15)
        })
        .signal_read(|w, p| match w.prop(p, "power") {
            Some("1") => 1,
            Some("2") => 15,
            _ => 0,
        })
        .sound_group("stone")
        .hardness(3.5)
        .mined_by("pickaxe", tool_tier::HAND)
        .tab(&crate::tabs::REDSTONE);

    // Vanilla's copper bulb: each new pulse flips it, and a comparator reads which way it is.
    r.block(&GOLDEN_LAMP)
        .signal_sink()
        .signal_blocker()
        .prop("power", ["off", "on"])
        .prop("lit", ["false", "true"])
        .states(|p, s| {
            let power = p.is("lit", "true");
            let (side, top) = if power { ("golden_lamp_lit", "golden_lamp_top_lit") } else { ("golden_lamp", "golden_lamp_top") };
            let whole = [0.0, 0.0, 16.0, 16.0];
            let faces = [(side, whole), (side, whole), (side, whole), (side, whole), (top, whole), (top, whole)];
            s.orientation(Orientation::front(Dir::Up))
                .model_box_faces([0.0, 0.0, 0.0], [16.0, 16.0, 16.0], faces)
                .turned([1, 3, 2, 0, 0, 0])
                .light_opacity(15)
                .light(if power { 15 } else { 0 })
        })
        .on_signal(|e| {
            if e.rising() {
                let (w, p) = (e.world(), e.pos());
                let lit = w.prop(p, "lit") == Some("true");
                w.prop_set(p, "lit", if lit { "false" } else { "true" });
            }
        })
        .signal_read(|w, p| if w.prop(p, "lit") == Some("true") { 15 } else { 0 })
        .sound_group("stone")
        .hardness(3.0)
        .mined_by("pickaxe", tool_tier::HAND)
        .tab(&crate::tabs::REDSTONE);

    r.block(&OBSERVER)
        // It looks where the player looks, the way it was placed.
        .facing(Facings::ALL, Toward::Away)
        .signal_observer()
        .states(|p, s| {
            let back = if p.is("power", "on") { "observer_back_on" } else { "observer_back" };
            let front = if p.is("power", "on") { "observer_front_on" } else { "observer_front" };
            let whole = [0.0, 0.0, 16.0, 16.0];
            let faces = [
                ("observer_side", whole),
                ("observer_side", whole),
                ("observer_top", whole),
                ("observer_top", [0.0, 16.0, 16.0, 0.0]),
                (front, whole),
                (back, whole),
            ];
            s.model_box_faces([0.0, 0.0, 0.0], [16.0, 16.0, 16.0], faces)
                .light_opacity(15)
        })
        .sound_group("stone")
        .hardness(3.0)
        .mined_by("pickaxe", tool_tier::HAND)
        .tab(&crate::tabs::REDSTONE);

    r.block(&DISPENSER)
        .sound_group("stone")
        .facing(Facings::ALL, Toward::Player)
        .signal_sink()
        .states(|_, s| {
            // Vanilla's: a furnace's sides, or all its top with the front up or down.
            s.front_faces("furnace_top", "furnace_top", "dispenser_front")
        })
        // With each new pulse, out of the front.
        .on_signal(|e| {
            if !e.rising() {
                return;
            }
            let (w, p) = (e.world(), e.pos());
            let dir = w
                .prop(p, "facing")
                .and_then(Dir::from_name)
                .unwrap_or(Dir::Up)
                .offset();
            let items = w.inventory(p);
            for i in 0..items.len() {
                let stack = items.get(i);
                if !stack.is_empty() {
                    items.set(i, ItemStack { count: stack.count - 1, ..stack });
                    let center = block_center(p);
                    let dir = Vec3::new(dir.x as f64, dir.y as f64, dir.z as f64);
                    let at = center + dir * 0.8;
                    // Shot out about as fast as vanilla's, a little to either side.
                    let r = |_| (w.random_below(1000) as f64 / 1000.0 - 0.5) * 0.6;
                    let v = dir * 5.0 + Vec3::new(r(0), r(1), r(2));
                    let how = Spawn::new()
                        .stack(ItemStack { count: 1, ..stack })
                        .velocity(v);
                    w.spawn_with(&ITEM, at, &how);
                    break;
                }
            }
        })
        .hardness(3.5)
        .mined_by("pickaxe", tool_tier::HAND)
        .tab(&crate::tabs::REDSTONE)
        .entity(|e| e.inventory(9))
        .opens(&DISPENSER_MENU);
    r.menu(&DISPENSER_MENU)
        .group("dispenser", Slots::chest(1).holder())
        .default_screen();

    const C: &RecipeKind = &base_api::CRAFTING;
    r.shaped(C, "redstone_block", 1, &["rrr", "rrr", "rrr"], &[('r', "redstone")]);
    r.shaped(C, "redstone", 9, &["b"], &[('b', "redstone_block")]);
    r.shaped(C, "redstone_torch", 1, &["r", "s"], &[('r', "redstone"), ('s', "#stick")]);
    r.shaped(C, "repeater", 1, &["trt", "sss"], &[('t', "redstone_torch"), ('r', "redstone"), ('s', "stone")]);
    r.shaped(C, "comparator", 1, &[" t ", "trt", "sss"], &[('t', "redstone_torch"), ('r', "redstone"), ('s', "stone")]);
    r.shaped(C, "lever", 1, &["s", "c"], &[('s', "#stick"), ('c', "#cobblestone")]);
    r.shaped(C, "stone_button", 1, &["s"], &[('s', "stone")]);
    r.shaped(C, "redstone_lamp", 1, &[" r ", "rgr", " r "], &[('r', "redstone"), ('g', "glowstone")]);
    r.shaped(C, "piston", 1, &["ppp", "cic", "crc"], &[('p', "#planks"), ('c', "#cobblestone"), ('i', "iron_ingot"), ('r', "redstone")]);
    r.shaped(C, "slime_block", 1, &["ccc", "cwc", "ccc"], &[('c', "clay"), ('w', "#crops/wheat")]);
    r.shaped(C, "sticky_piston", 1, &["s", "p"], &[('s', "slime_block"), ('p', "piston")]);
    r.shaped(C, "floater", 1, &["ipi", "iri", "ipi"], &[('i', "iron_ingot"), ('r', "redstone"), ('p', "piston")]);
    r.shaped(C, "golden_lamp", 1, &[" c ", "crc", " c "], &[('c', "gold_ingot"), ('r', "redstone")]);
    r.shaped(C, "anchor", 1, &["iii", "iri", "iii"], &[('i', "iron_ingot"), ('r', "redstone")]);
    r.shaped(C, "observer", 1, &["ccc", "rri", "ccc"], &[('c', "#cobblestone"), ('r', "redstone"), ('i', "iron_ingot")]);
    r.shaped(C, "dispenser", 1, &["ccc", "csc", "crc"], &[('c', "#cobblestone"), ('s', "#stick"), ('r', "redstone")]);
}
