//! What the server and a client put out alike: text, sounds, particles and gizmo shapes, the
//! `Output` methods of `World` and `Client`. The server's go to whoever they reach by themselves
//! (`Who::Around`: a sound's range, the players near particles, everyone in the dimension for
//! text), `.to(who)` for others; a client's to that client alone, and only when `.to(..)` takes it
//! in, so code both sides run says it once.
//!
//! ```ignore
//! w.hint(..).to(player);
//! w.play_sound(&BOOM, pos, 4.0);
//! out.spawn_particles(&SPARK, pos).count(10).to(Who::Near(dim, pos, 8.0));
//! w.draw_text(Some(&PIPES), pos, "mymod.flow", &args, 0x60FF60FF, 1.0).ticks(11);
//! ```

use crate::abi::{self, Str, Vec3, Who, gizmo_kind as gk, show_place as place};
use crate::effects::{Burst, PlaySound};
use crate::{Arg, Client, GizmoGroup, Id, Particle, Show, Sound, TextPlace, World, host};

/// Where output goes (`Output::out`).
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum Out {
    World(World, Who),
    Client(Client, Who),
}

impl Out {
    pub(crate) fn to(&mut self, who: Who) {
        let (Out::World(_, w) | Out::Client(_, w)) = self;
        *w = who;
    }

    /// On a client, whether its `Who` takes this client in; the engine picks for the server.
    pub(crate) fn here(&self) -> bool {
        let Out::Client(c, who) = *self else { return true };
        let s = c.state();
        let near = |at: Vec3, r: f64| {
            let (x, y, z) = (s.pos.x - at.x, s.pos.y - at.y, s.pos.z - at.z);
            x * x + y * y + z * z < r * r
        };
        match who {
            Who::Around | Who::All => true,
            Who::Player(p) => p == s.player,
            Who::Dimension(d) => d == s.dim,
            Who::Near(d, at, r) => d == s.dim && near(at, r),
        }
    }
}

/// See the module.
pub trait Output {
    #[doc(hidden)]
    fn out(&self) -> Out;

    /// Text in a place on the screen: one of `text_places` or a mod's own.
    fn show_at<'a>(&self, place: impl Into<Id<TextPlace>>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(self.out(), place.into().raw(), key, args)
    }
    /// Text by itself, in the middle of the screen unless `at` says otherwise.
    fn show<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(self.out(), place::FREE, key, args)
    }
    /// A line in the chat, for the player to scroll back to.
    fn chat<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(self.out(), place::CHAT, key, args)
    }
    /// A short line above the hotbar. A player can move or hide `core:hint` in the HUD editor.
    fn hint<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(self.out(), place::HINT, key, args)
    }
    /// Big text over the crosshair. A player can move or hide `core:title` in the HUD editor.
    fn title<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(self.out(), place::TITLE, key, args)
    }
    /// Smaller text under the crosshair. A player can move or hide `core:subtitle` likewise.
    fn subtitle<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(self.out(), place::SUBTITLE, key, args)
    }

    /// Sound event `sound` at `pos` (`None`: not positioned, a UI sound), sent at the end of the
    /// statement; `.pitch(p)` sets the playback speed.
    fn play_sound(&self, sound: &Sound, pos: impl Into<Option<Vec3>>, volume: f32) -> PlaySound {
        PlaySound::new(self.out(), sound.id(), pos.into(), volume)
    }

    /// Particles at `pos`, one by default.
    fn spawn_particles(&self, kind: &Particle, pos: impl Into<Vec3>) -> Burst {
        Burst::new(self.out(), kind.id(), pos.into())
    }

    /// Debug shapes in color `0xRRGGBBAA`: always, or with a `group` while the player shows it
    /// (`/gizmo <group>`). The server's last a tick (`.ticks(n)`), a client's this frame.
    fn draw_line<'a>(&self, group: Option<&'a GizmoGroup>, a: impl Into<Vec3>, b: impl Into<Vec3>, color: u32) -> Drawing<'a> {
        Drawing::new(self.out(), group, color, Drawn::Lines(vec![(gk::LINE, a.into(), b.into())]))
    }
    fn draw_box<'a>(&self, group: Option<&'a GizmoGroup>, min: impl Into<Vec3>, max: impl Into<Vec3>, color: u32) -> Drawing<'a> {
        Drawing::new(self.out(), group, color, Drawn::Lines(vec![(gk::BOX, min.into(), max.into())]))
    }
    /// See-through by `color`'s alpha (range previews, blueprints).
    fn draw_fill<'a>(&self, group: Option<&'a GizmoGroup>, min: impl Into<Vec3>, max: impl Into<Vec3>, color: u32) -> Drawing<'a> {
        Drawing::new(self.out(), group, color, Drawn::Lines(vec![(gk::FILL, min.into(), max.into())]))
    }
    /// The outline of a shape: where a fill or a search will reach, before it does.
    fn draw_shape<'a>(&self, group: Option<&'a GizmoGroup>, shape: &crate::Shape, color: u32) -> Drawing<'a> {
        let lines = shape
            .outline()
            .into_iter()
            .map(|(a, b)| (gk::LINE, a, b))
            .collect();
        Drawing::new(self.out(), group, color, Drawn::Lines(lines))
    }
    /// The translation `key` with `args` at `pos`, in each viewer's language, `size` times as big
    /// as menu text.
    fn draw_text<'a>(
        &self,
        group: Option<&'a GizmoGroup>,
        pos: impl Into<Vec3>,
        key: &'a str,
        args: &'a [Arg<'a>],
        color: u32,
        size: f32,
    ) -> Drawing<'a> {
        Drawing::new(self.out(), group, color, Drawn::Text(pos.into(), key, crate::ui::raw_args(args), size))
    }
}

impl Output for World {
    fn out(&self) -> Out {
        Out::World(*self, Who::Around)
    }
}

impl Output for Client {
    fn out(&self) -> Out {
        Out::Client(*self, Who::Around)
    }
}

enum Drawn<'a> {
    Lines(Vec<(u32, Vec3, Vec3)>),
    Text(Vec3, &'a str, Vec<abi::UiArg>, f32),
}

/// Gizmo shapes, drawn when this is dropped (`Output::draw_line` ..).
pub struct Drawing<'a> {
    out: Out,
    group: Option<&'a GizmoGroup>,
    color: u32,
    ticks: u32,
    what: Drawn<'a>,
}

impl<'a> Drawing<'a> {
    fn new(out: Out, group: Option<&'a GizmoGroup>, color: u32, what: Drawn<'a>) -> Drawing<'a> {
        Drawing { out, group, color, ticks: 1, what }
    }

    pub fn to(mut self, who: impl Into<Who>) -> Self {
        self.out.to(who.into());
        self
    }

    /// Ticks the server's stay; a client draws for the frame.
    pub fn ticks(mut self, n: u32) -> Self {
        self.ticks = n;
        self
    }
}

impl Drop for Drawing<'_> {
    fn drop(&mut self) {
        if !self.out.here() {
            return;
        }
        let group = Str::new(self.group.map_or("", |g| g.name()));
        let (color, ticks) = (self.color, self.ticks);
        unsafe {
            match (&self.what, self.out) {
                (Drawn::Lines(lines), Out::World(w, who)) => lines
                    .iter()
                    .for_each(|(k, a, b)| (host().gizmo)(w.raw(), who, group, *k, a, b, color, ticks)),
                (Drawn::Lines(lines), Out::Client(c, _)) => lines
                    .iter()
                    .for_each(|(k, a, b)| (host().client_draw)(c.raw(), group, *k, a, b, color)),
                (Drawn::Text(pos, key, args, size), Out::World(w, who)) => {
                    (host().gizmo_text)(w.raw(), who, group, pos, *size, color, ticks, Str::new(key), abi::Slice::new(args))
                }
                (Drawn::Text(pos, key, args, size), Out::Client(c, _)) => {
                    (host().client_draw_text)(c.raw(), group, pos, *size, color, Str::new(key), abi::Slice::new(args))
                }
            }
        }
    }
}
