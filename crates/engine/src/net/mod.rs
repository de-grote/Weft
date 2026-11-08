//! Client/server messages. Both sides only see channel endpoints; a transport (in-process, iroh,
//! later Steam) bridges those channels to wherever the peer is. The in-process transport moves
//! Rust values directly, network transports serialize in their bridge threads.

use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use glam::{DVec3, IVec3};

use weft_abi::ItemStack;

use crate::server::lod_tops::LodColumn;
use crate::world::Cube;
use codec::Wire;

pub mod codec;
pub mod p2p;
pub mod screens;

/// Ticket others can use to join this host, once hosting is online.
pub fn current_ticket() -> Option<String> {
    p2p::HOSTING.lock().as_ref().and_then(|h| h.ticket.clone())
}

pub fn host(server: &crate::server::ServerHandle, settings: &crate::settings::Settings) {
    server
        .shared
        .hosting
        .store(true, std::sync::atomic::Ordering::Relaxed);
    p2p::host(server.connect.clone(), settings);
}

pub fn join(ticket: &str, settings: &crate::settings::Settings) -> Result<ClientConn, String> {
    p2p::join(ticket, settings)
}

/// A struct that travels in messages, written field by field.
macro_rules! wire_struct {
    ($(#[$m:meta])* pub struct $name:ident { $(pub $f:ident: $t:ty),* $(,)? }) => {
        $(#[$m])*
        pub struct $name {
            $(pub $f: $t),*
        }

        impl Wire for $name {
            fn put(&self, w: &mut codec::W) {
                $(self.$f.put(w);)*
            }
            fn get(r: &mut codec::R) -> Option<Self> {
                Some($name { $($f: <$t as Wire>::get(r)?),* })
            }
        }

        impl codec::Item for $name {}
    };
}

/// One variant's fields written or read in order.
macro_rules! wire_variant {
    (put $w:ident $x:ident $m:ident $v:ident) => {{
        let _ = $x;
    }};
    (put $w:ident $x:ident $m:ident $v:ident ($t:ty)) => {
        if let $m::$v(a) = $x {
            a.put($w)
        }
    };
    (put $w:ident $x:ident $m:ident $v:ident { $($f:ident),* }) => {
        if let $m::$v { $($f),* } = $x {
            $($f.put($w);)*
        }
    };
    (get $r:ident $m:ident $v:ident) => {
        $m::$v
    };
    (get $r:ident $m:ident $v:ident ($t:ty)) => {
        $m::$v(<$t as Wire>::get($r)?)
    };
    (get $r:ident $m:ident $v:ident { $($f:ident: $t:ty),* }) => {
        $m::$v { $($f: <$t as Wire>::get($r)?),* }
    };
}

/// Message kinds by name, for traffic counters (`net` log channel).
pub trait Message {
    const TO_SERVER: bool;
    const KINDS: &'static [&'static str];
    fn kind(&self) -> usize;
}

/// A message enum and everything that follows from its variants: the wire tag of each (its
/// position), names in traffic counters, the encoding. Variants are a unit, one value or named
/// fields.
macro_rules! messages {
    ($(#[$em:meta])* pub enum $msg:ident: $tag:ident, to_server = $to_server:expr; {
        $($(#[$vm:meta])* $v:ident $(($t:ty))? $({ $($f:ident: $ft:ty),* $(,)? })?),* $(,)?
    }) => {
        $(#[$em])*
        pub enum $msg {
            $($(#[$vm])* $v $(($t))? $({ $($f: $ft),* })?),*
        }

        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum $tag {
            $($v),*
        }

        impl $tag {
            pub const ALL: &'static [$tag] = &[$($tag::$v),*];
        }

        impl Message for $msg {
            const TO_SERVER: bool = $to_server;
            const KINDS: &'static [&'static str] = &[$(stringify!($v)),*];
            fn kind(&self) -> usize {
                match self {
                    $($msg::$v { .. } => $tag::$v as usize,)*
                }
            }
        }

        impl Wire for $msg {
            fn put(&self, w: &mut codec::W) {
                (self.kind() as u8).put(w);
                match self {
                    $(m @ $msg::$v { .. } => wire_variant!(put w m $msg $v $(($t))? $({ $($f),* })?),)*
                }
            }
            fn get(r: &mut codec::R) -> Option<Self> {
                Some(match *$tag::ALL.get(u8::get(r)? as usize)? {
                    $($tag::$v => wire_variant!(get r $msg $v $(($t))? $({ $($f: $ft),* })?),)*
                })
            }
        }
    };
}

/// How much of a player's own model is sent, all its files together.
pub const AVATAR_BYTES: usize = 512 * 1024;

wire_struct! {
    /// Text for the screen, however it ends up looking (`abi::ShowDesc`). Times below zero or a
    /// scale of 0 are for the client to fill in (`settled`). `from`: what the chat's visibility
    /// setting goes by.
    #[derive(Clone, Debug)]
    pub struct Shown {
        pub place: u32,
        pub msg: crate::ui::text::Msg,
        pub id: String,
        pub times: [f32; 3],
        pub scale: f32,
        pub at: Option<[f32; 2]>,
        pub color: Option<u32>,
        pub from: u8,
    }
}

impl Shown {
    pub const SYSTEM: u8 = 0;
    pub const PLAYER: u8 = 1;
    /// The answer to a command of whoever gets it.
    pub const REPLY: u8 = 2;

    pub fn new(place: u32, msg: crate::ui::text::Msg) -> Shown {
        Shown { place, msg, id: String::new(), times: [-1.0; 3], scale: 0.0, at: None, color: None, from: Shown::SYSTEM }
    }

    /// A chat line.
    pub fn chat(msg: crate::ui::text::Msg) -> Shown {
        Shown::new(weft_abi::show_place::CHAT, msg)
    }

    /// What its place is (`hud::places`).
    pub fn look(&self) -> &'static crate::hud::Place {
        let all = crate::hud::places();
        all.get(self.place as usize).unwrap_or(&all[0])
    }

    /// With what was left open as its place has it.
    pub fn settled(mut self) -> Shown {
        let look = self.look();
        if self.times[1] < 0.0 {
            self.times = look.times;
        }
        if self.scale <= 0.0 {
            self.scale = look.scale;
        }
        self.at = self.at.or(look.own_at);
        self
    }

    /// Asks for what its place and id show to go away.
    pub fn clears(&self) -> bool {
        self.msg.key.is_empty() && self.msg.args.is_empty()
    }

    /// How opaque it is `age` seconds in, 0 once over.
    pub fn alpha(&self, age: f32) -> f32 {
        let [fade_in, stay, fade_out] = self.times;
        if age < fade_in {
            age / fade_in
        } else if age < fade_in + stay {
            1.0
        } else if age < fade_in + stay + fade_out {
            (fade_in + stay + fade_out - age) / fade_out
        } else {
            0.0
        }
    }
}

wire_struct! {
    /// A player's own model: its files, which the server passes on unread.
    #[derive(Clone, Debug, Default)]
    pub struct AvatarData {
        pub geo: Vec<u8>,
        pub animations: Vec<u8>,
        pub bones: Vec<u8>,
        pub png: Vec<u8>,
        pub glow: Vec<u8>,
    }
}

impl AvatarData {
    pub fn size(&self) -> usize {
        self.geo.len() + self.animations.len() + self.bones.len() + self.png.len() + self.glow.len()
    }
}

wire_struct! {
    #[derive(Clone, Debug)]
    pub struct EntityView {
        pub id: u64,
        pub kind: u32,
        pub pos: DVec3,
        pub yaw: f32,
        pub pitch: f32,
        pub item: ItemStack,
    }
}

wire_struct! {
    /// A debug shape for a gizmo group (`client::gizmo`): `kind` a `gizmo_kind` from `a` to `b`, or
    /// `GizmoShape::TEXT`, `text` at `a` `size` times as big as menu text. Shown for `ticks`
    /// server ticks.
    #[derive(Clone, Debug, PartialEq)]
    pub struct GizmoShape {
        pub group: String,
        pub kind: u8,
        pub a: DVec3,
        pub b: DVec3,
        pub color: u32,
        pub ticks: u32,
        pub size: f32,
        pub text: crate::ui::text::Msg,
    }
}

impl GizmoShape {
    /// Not a `gizmo_kind`: a label.
    pub const TEXT: u8 = u8::MAX;

    pub fn shape(group: &str, kind: u8, a: DVec3, b: DVec3, color: u32, ticks: u32) -> GizmoShape {
        GizmoShape { group: group.into(), kind, a, b, color, ticks, size: 0.0, text: crate::ui::text::Msg::default() }
    }

    pub fn text(group: &str, at: DVec3, size: f32, color: u32, ticks: u32, text: crate::ui::text::Msg) -> GizmoShape {
        GizmoShape { group: group.into(), kind: GizmoShape::TEXT, a: at, b: at, color, ticks, size, text }
    }
}

/// Someone's mining as others see it: who, the cell, which of what is in it, how far along (0..1).
pub type Crack = (u64, IVec3, u32, f32);

/// An entity's move: id, position, yaw, pitch, crouching, and its frame: the solid body it
/// stands on (0: the world), which clients draw it relative to.
/// An entity, where it is, its look, pose, whether it sprints, and the body it stands on.
pub type Move = (u64, DVec3, f32, f32, crate::world::Pose, bool, u64);

/// The longest message a connection takes.
pub const MAX_MESSAGE: usize = 64 << 20;

messages! {
    #[derive(Clone)]
    pub enum ToClient: ClientTag, to_server = false; {
        /// `look` (yaw, pitch), `selected` and `flying` as it left them.
        Welcome { player: u64, dim: u32, pos: DVec3, seed: u64, look: [f32; 2], selected: u32, flying: bool },
        /// `edit`: sent again because blocks in it changed (shown whole, at once), not loading.
        /// `frame`: the body it is a cube of, in that body's coordinates; 0 for the world.
        Cube { dim: u32, frame: u64, pos: IVec3, cube: Arc<Cube>, edit: bool },
        Unload { dim: u32, pos: IVec3 },
        /// Cubes in view that differ from generation (saved, or changed while loaded): the
        /// client does not generate them and waits for them to be sent.
        Saved { dim: u32, cubes: Vec<IVec3> },
        /// Area edits of the dimension (`AreaFill::spec`): `all` replaces what the client has,
        /// otherwise they come after it. Clients make them on what they generate.
        Edits { dim: u32, specs: Vec<String>, all: bool },
        /// Columns of dimension `dim` that differ from the terrain, for distant terrain (`top`
        /// `u32::MAX` for one that doesn't any more); `all`: every one there is.
        LodColumns { dim: u32, all: bool, cols: Vec<LodColumn> },
        /// Of the world, or of body `frame`.
        /// `extras`: what else is in cells, after `changes` (which take it away).
        Blocks { dim: u32, frame: u64, changes: Vec<(IVec3, u32)>, extras: Vec<(IVec3, Vec<u32>)> },
        /// What moving bodies' lights give cells of the world besides what is there, for a client
        /// to work out light as the server would: (cell, its light), none taking it away.
        BodyGlow { dim: u32, cells: Vec<(IVec3, [u8; 3])> },
        /// The world's light on body `frame`, by its cubes.
        BodyLight { frame: u64, lights: Vec<(IVec3, u32)> },
        /// Power of pieces of wire: (cell, face, level), of the world or of body `frame`.
        Signals { dim: u32, frame: u64, changes: Vec<(IVec3, u8, u8)> },
        Time { dim: u32, time: u64 },
        /// `frame`: the solid body it was put on and the position relative to that body (clients
        /// draw bodies ahead of the server, so only the relative position is right for them).
        Teleport { dim: u32, pos: DVec3, frame: Option<(u64, DVec3)> },
        Spawn(Vec<EntityView>),
        /// Moves of one server tick (its number: clients time the moves by it, not by when they
        /// arrive).
        Moves { tick: u64, moves: Vec<Move> },
        /// Entities out of sight, and whether they were killed (they play the death animation).
        Despawn(Vec<(u64, bool)>),
        /// Items picked up: (item entity, who took it). They fly to the taker before going.
        Taken(Vec<(u64, u64)>),
        /// A use (right click) did something: the arm swings, and dips if the held item was used.
        Swing { used_item: bool, off_hand: bool },
        GameMode(crate::world::GameMode),
        /// Everyone online, sent when someone joins or leaves.
        Players(Vec<String>),
        /// Own inventory: (slot, stack) for changed slots.
        Inventory(Vec<(u32, ItemStack)>),
        /// Own synced components: (component id, bytes).
        Components(Vec<(u32, Vec<u8>)>),
        /// A hit knocks the player back, `away` being flat and of length 1 (`physics::knock`).
        Knock { away: [f32; 3] },
        /// Velocity change in blocks per tick (a mod's push, a sprinting hit).
        Velocity { add: [f32; 3] },
        /// A mod set the player's velocity, in blocks per tick.
        SetVelocity([f32; 3]),
        /// A mod turned the player: yaw and pitch in radians.
        Look([f32; 2]),
        /// A mod made the player fly or stop.
        Flying(bool),
        /// A mod put this hotbar slot in the player's hand.
        Selected(u32),
        /// Ticks a second the server runs at.
        TickRate(u32),
        /// The player's walking speed and jump multipliers.
        /// The player's attributes that changed, by id.
        Attributes(Vec<(u32, f32)>),
        /// Mining that block is not allowed (`action::BREAK_BLOCK`).
        BreakRefused { frame: u64, pos: IVec3 },
        /// Holding the use button started on an item that takes this many ticks, or it ended.
        /// Ticks it takes, and whether in the other hand.
        Using(Option<(u32, bool)>),
        /// The dimension's weather now (`registry::WeatherDef` by id).
        Weather(u32),


        /// Someone else's arm swings: a hit, a use, mining.
        /// Who, and whether with the other hand.
        EntitySwing { id: u64, off: bool },
        /// Who is mining what, besides this player.
        Cracks(Vec<Crack>),
        /// The player's `permission` level, for what its client offers.
        Permission(u32),
        /// A sprinting hit landed, which ends the sprint.
        StopSprint,
        /// Camera mode `mode` (`u32::MAX`: the player's own again), `locked` till told; the
        /// change over `transition` seconds (NaN: the mode's).
        Camera { mode: u32, locked: bool, transition: f32, easing: u32 },
        /// `block`: the state it was opened on (0: none), `name`: what it belongs to is called.
        OpenMenu { kind: u32, window: u32, slots: u32, owner: u64, block: u32, name: String },
        MenuSlots(Vec<(u32, ItemStack)>),
        Cursor(ItemStack),
        CloseMenu,
        Died,
        /// Formatted by the client in its own language.
        Show(Shown),
        /// The answer to `ToServer::Command` `id`, for whoever ran it rather than the chat.
        CommandReply { id: u32, lines: Vec<crate::ui::text::Msg> },
        Disconnect(String),
        Gizmos(Vec<GizmoShape>),
        /// Mod channel message (`Host::reg_packet`).
        Packet { channel: u32, data: Vec<u8> },
        /// Components of other entities: (entity, component id, bytes).
        EntityComponents(Vec<(u64, u32, Vec<u8>)>),
        Particles(Vec<weft_abi::ParticleBurst>),
        /// The server's values of `SYNCED` settings: (config id, value).
        Configs(Vec<(u32, f64)>),
        /// Sound events: (event, position, volume, pitch or 0).
        Sounds(Vec<(u32, DVec3, f32, f32)>),
        /// Keyframe animations started on entities: (entity, name, looping); empty name = none.
        PlayAction(Vec<(u64, String, bool)>),
        /// A player's own skin (PNG), for their entity.
        Skin { entity: u64, png: Vec<u8> },
        /// A player's own model, for their entity.
        Avatar { entity: u64, avatar: AvatarData },
        /// That player left: what they brought can go.
        PlayerGone { entity: u64 },
    }
}

messages! {
    #[derive(Clone)]
    pub enum ToServer: ServerTag, to_server = true; {
        /// `key`: the player's public key, their identity across sessions and addresses.
        Join { name: String, key: String },
        /// `frame`: the body it rides and its position relative to that body as the client shows it
        /// (the client shows bodies behind the server; only the relative position agrees).
        /// `jumped`: it jumped since the last one.
        Move { pos: DVec3, yaw: f32, pitch: f32, on_ground: bool, pose: crate::world::Pose, sprint: bool, flying: bool, jumped: bool, frame: Option<(u64, DVec3)> },
        /// Where the mount the player steers went this tick (its client moves it).
        MountMove { pos: DVec3, yaw: f32, vel: DVec3, on_ground: bool },
        /// Off what the player rides (`core:dismount`).
        Dismount,
        /// May it turn on camera mode `mode` (`camera_flags::ASK`)? Answered with `Camera`.
        Camera(u32),
        /// Started mining this block (`frame` and `part` like `Break`'s).
        StartBreak { pos: IVec3, part: u32, frame: u64 },
        /// Stopped mining without finishing.
        StopBreak,
        /// Let go of the use button before the item took effect.
        StopUsing,
        /// The main hand's item goes to the other hand and back.
        SwapHands,
        /// `frame`: the structure (entity) the block is part of, its position in the structure's
        /// coordinates; 0 for the world. `face`: of its selection that was hit.
        /// `part`: which of what is in the cell; `hit`: where on it, from its corner (`Use`'s).
        Break { pos: IVec3, part: u32, face: u8, hit: [f32; 3], frame: u64 },
        /// Right click on a block: the block reacts first (chests), otherwise the held item is used or
        /// placed against the clicked face. `frame` like `Break`'s; `look` in world space.
        /// `hit`: where on the block, from its corner.
        Use { pos: IVec3, part: u32, face: u8, look: [f32; 3], hit: [f32; 3], sneak: bool, frame: u64 },
        UseItem { target: Option<(IVec3, u8)> },
        Select(u32),
        Attack(u64),
        /// A swing that spends its charge, whatever it hit, sent after any `Attack` of that swing.
        Swing,
        Interact(u64),
        OpenInventory,
        Click { slot: u32, button: u8, shift: bool },
        /// Creative inventory: a full stack of this item onto the menu cursor.
        /// Click on a creative entry: 0 left (stack), 1 right (one).
        CreativeTake { item: u32, button: u8 },
        /// Middle click on a block whose item is not in the hotbar: bring it into the selected slot
        /// from the inventory, or in creative make one.
        Pick { item: u32 },
        /// The cursor stack spread over these slots by dragging (button 0 evenly, 1 one each).
        Drag { slots: Vec<u32>, button: u8 },
        CloseMenu,
        Drop { all: bool },
        Respawn,
        Chat(String),
        /// A command run from code (`client_op::RUN_COMMAND`), answered with `CommandReply`.
        Command { id: u32, line: String },
        Leave,
        /// Gizmo groups this player shows; the server only sends shapes of these.
        Gizmos(Vec<String>),
        Packet { channel: u32, data: Vec<u8> },
        /// The player's skin (PNG, 64 x 64 or 64 x 32).
        Skin(Vec<u8>),
        /// The player's own model.
        Avatar(AvatarData),
        /// The client's view distance in cubes: what the server announces edited cubes in.
        View(u32),
    }
}

pub struct Conn<S, R> {
    tx: Sender<S>,
    rx: Receiver<R>,
    closed: bool,
}

impl<S: Message, R> Conn<S, R> {
    pub fn new(tx: Sender<S>, rx: Receiver<R>) -> Self {
        Conn { tx, rx, closed: false }
    }

    pub fn send(&mut self, m: S) {
        crate::diag::net_message(S::TO_SERVER, m.kind(), 1, 0);
        if self.tx.send(m).is_err() {
            self.closed = true;
        }
    }

    pub fn recv(&mut self) -> Option<R> {
        match self.rx.try_recv() {
            Ok(m) => Some(m),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.closed = true;
                None
            }
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

pub type ClientConn = Conn<ToServer, ToClient>;
pub type ServerConn = Conn<ToClient, ToServer>;

pub fn local_pair() -> (ClientConn, ServerConn) {
    let (to_server_tx, to_server_rx) = unbounded();
    let (to_client_tx, to_client_rx) = unbounded();
    (Conn::new(to_server_tx, to_client_rx), Conn::new(to_client_tx, to_server_rx))
}
