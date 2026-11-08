//! Saved locations: `/sethome`, `/home`, `/delhome` and `/homes` for up to `MAX_HOMES` named homes,
//! and `/back` for the spot of the last death

use weft_sdk::*;

const MAX_HOMES: usize = 16;
const MAX_NAME_BYTES: usize = 32;
const DEFAULT_NAME: &str = "default";

value! {
    struct Waypoint { pos: [f64; 3], dim_key: u64, name: [u8; MAX_NAME_BYTES], set: u64 }
}
value! {
    struct Waypoints { homes: [Waypoint; MAX_HOMES], death: Waypoint }
}
static HOMES: Component<Waypoints> = Component::named("base:homes");

pub fn register(r: &mut Registry) {
    let empty = Waypoint { pos: [0.0; 3], dim_key: 0, name: [0; MAX_NAME_BYTES], set: 0 };
    r.entity_component(
        &HOMES,
        Waypoints { homes: [empty; MAX_HOMES], death: empty },
        component_flags::SAVE | component_flags::KEEP_ON_RESPAWN,
    );
    r.edit(&PLAYER).attach(&HOMES);

    r.on_event(&actions::DIE, |b| {
        let w = b.world();
        for d in b.iter() {
            // Only players have the component, so mobs are skipped.
            if let Some(h) = w.component_mut(d.entity, &HOMES) {
                h.death = Waypoint { pos: d.pos.into(), dim_key: dimension_key(w.dimension()), name: [0; MAX_NAME_BYTES], set: 1 };
            }
        }
    });

    r.command("sethome", "[name:word]", |cmd: &mut ServerCommand| {
        let (w, p) = (cmd.world(), cmd.player());
        let key = name_arg(cmd);
        let name = unpack(&key);
        let Some(st) = w.entity(p) else { return };
        let Some(h) = w.component_mut(p, &HOMES) else { return };
        let slot = h
            .homes
            .iter()
            .position(|s| s.set != 0 && s.name == key)
            .or_else(|| h.homes.iter().position(|s| s.set == 0));
        let Some(i) = slot else {
            return cmd.reply("command.base.sethome.full", &[Arg::Int(MAX_HOMES as i64)]);
        };
        h.homes[i] = Waypoint { pos: st.pos.into(), dim_key: dimension_key(w.dimension()), name: key, set: 1 };
        cmd.reply("command.base.sethome.done", &[Arg::Text(name)]);
    })
    .permission(permission::EVERYONE);

    r.command("home", "[name:word]", |cmd: &mut ServerCommand| {
        let (w, p) = (cmd.world(), cmd.player());
        let key = name_arg(cmd);
        let name = unpack(&key);
        let Some(homes) = w.component_mut(p, &HOMES) else { return };
        let Some(h) = homes.homes.iter_mut().find(|s| s.set != 0 && s.name == key) else {
            return cmd.reply("command.base.home.unset", &[Arg::Text(name)]);
        };
        if !teleport_to(&w, p, h) {
            return cmd.reply("command.base.home.missing_dimension", &[Arg::Text(name)]);
        }
        cmd.reply("command.base.home.done", &[Arg::Text(name)]);
    })
    .permission(permission::EVERYONE);

    r.command("delhome", "<name:word>", |cmd: &mut ServerCommand| {
        let (w, p) = (cmd.world(), cmd.player());
        let key = name_arg(cmd);
        let name = unpack(&key);
        let Some(homes) = w.component_mut(p, &HOMES) else { return };
        match homes.homes.iter_mut().find(|s| s.set != 0 && s.name == key) {
            Some(h) => {
                h.set = 0;
                cmd.reply("command.base.delhome.done", &[Arg::Text(name)]);
            }
            None => cmd.reply("command.base.home.unset", &[Arg::Text(name)]),
        }
    })
    .permission(permission::EVERYONE);

    r.command("homes", "", |cmd: &mut ServerCommand| {
        let w = cmd.world();
        let Some(h) = w.component(cmd.player(), &HOMES) else { return };
        let names: Vec<&str> = h
            .homes
            .iter()
            .filter(|s| s.set != 0)
            .map(|s| unpack(&s.name))
            .collect();
        if names.is_empty() {
            cmd.reply("command.base.homes.none", &[]);
        } else {
            cmd.reply("command.base.homes.list", &[Arg::Text(&names.join(", "))]);
        }
    })
    .permission(permission::EVERYONE);

    r.command("back", "", |cmd: &mut ServerCommand| {
        let (w, p) = (cmd.world(), cmd.player());
        let Some(homes) = w.component_mut(p, &HOMES) else { return };
        if homes.death.set == 0 {
            return cmd.reply("command.base.back.unset", &[]);
        }
        if !teleport_to(&w, p, &mut homes.death) {
            return cmd.reply("command.base.back.missing_dimension", &[]);
        }
        // `set` stays 1 so the spot can be returned to again.
        cmd.reply("command.base.back.done", &[]);
    })
    .permission(permission::EVERYONE);
}

/// Teleports to a saved waypoint. If its dimension is gone (its mod was removed) the home is dropped
/// and this is false.
fn teleport_to(w: &World, entity: u64, home: &mut Waypoint) -> bool {
    let Some(dim) = dimension_from_key(home.dim_key) else {
        home.set = 0;
        return false;
    };
    let target = (w.dimension() != dim).then(|| Dimension::generated(name_of(abi::entry_kind::DIMENSION, dim).to_string()));
    w.teleport(entity, target, home.pos);
    true
}

/// Lowercased so names are case-insensitive; truncation respects char boundaries.
fn name_arg(cmd: &ServerCommand) -> [u8; MAX_NAME_BYTES] {
    let mut out = [0u8; MAX_NAME_BYTES];
    let mut len = 0;
    for c in cmd
        .text("name")
        .unwrap_or(DEFAULT_NAME)
        .to_lowercase()
        .chars()
    {
        let n = c.len_utf8();
        if len + n > MAX_NAME_BYTES {
            break;
        }
        c.encode_utf8(&mut out[len..]);
        len += n;
    }
    out
}

fn unpack(bytes: &[u8; MAX_NAME_BYTES]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(MAX_NAME_BYTES);
    std::str::from_utf8(&bytes[..end]).unwrap_or("")
}
