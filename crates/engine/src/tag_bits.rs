//! What the engine reads of some tags all the time, as bits made once when content is frozen:
//! `TAGS` says which tag sets each. Mods only see the tags (`abi::comp`).

/// A module per kind: a bit per tag of `abi::comp::<kind>` of the same name, in this order, and
/// `TAGS` pairing them; what is in `[..]` after the list goes in the module as it is.
macro_rules! tag_bits {
    ($($m:ident of $kind:ident { $($t:ident),* $(,)? } $([ $($extra:item)* ])?)*) => {$(
        pub mod $m {
            #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
            enum Bit { $($t),* }
            $(pub const $t: u32 = 1 << Bit::$t as u32;)*
            pub const TAGS: &[(&str, u32)] = &[$((weft_abi::comp::$kind::$t, $t)),*];
            $($($extra)*)?
        }
    )*}
}

tag_bits! {
    screen of menu { PAUSES, KEEP_ON_ESCAPE, CLOSES_ON_RESPAWN, KEYS_FIRST, SHOWS_CHAT }
    camera of camera { BODY, HAND, HUD, EFFECTS, CLIP, SOLID, CYCLE, DETACHED, AIM, NO_INTERACT }
    damage of damage_cause { BYPASSES_ARMOR, PIERCES_INVULNERABLE, IGNORES_COOLDOWN, BURNING, NO_KNOCKBACK, SILENT }
    game_mode of game_mode {
        INSTANT_BREAK, NO_BREAK, NO_PLACE, NO_COSTS, INVULNERABLE, ALWAYS_FLY, NOCLIP, NO_INTERACT, HIDDEN, NO_HUD,
        CREATIVE_INVENTORY,
    }
    particle of particle { COLLIDE, FULLBRIGHT, TRANSLUCENT, SHRINK }
    config of config { RESTART_REQUIRED }
    entity of entity { NO_GRAVITY, INVULNERABLE, BURNS_IN_SUN, SWIMS, SOLID, KINEMATIC, CRUSHES, RIDERS_FALL, WEARS } [
        /// What every kind does unless a tag in `OFF` takes it away.
        pub const FALL_DAMAGE: u32 = 1 << 20;
        pub const PUSHABLE: u32 = 1 << 21;
        pub const BLOCKED: u32 = weft_abi::entity_state::BLOCKED;
        pub const OFF: &[(&str, u32)] = &[(weft_abi::comp::entity::NO_FALL_DAMAGE, FALL_DAMAGE), (weft_abi::comp::entity::UNPUSHABLE, PUSHABLE)];
    ]
}

/// Weather is read the other way round: what a weather doesn't say, rain does.
pub mod weather {
    pub const FALLS: u32 = 1;
    pub const LIGHTNING: u32 = 2;
    pub const SHELTERS: u32 = 4;
}

/// Blocks' bits are the ABI's (`block_flags`): what a cell holds is read by mods too.
pub mod block {
    use weft_abi::{block_flags as f, comp::block as b};
    pub const TAGS: &[(&str, u32)] = &[
        (b::REPLACEABLE, f::REPLACEABLE),
        (b::CLIMBABLE, f::CLIMBABLE),
        (b::NOT_TARGETABLE, f::NOT_TARGETABLE),
        (b::CULL_SAME, f::CULL_SAME),
        (b::IMMOVABLE, f::IMMOVABLE),
        (b::SIGNAL_BLOCKER, f::SIGNAL_BLOCKER),
        (b::STICKY, f::STICKY),
        (b::NOT_STOOD_ON, f::NOT_STOOD_ON),
    ];
}
