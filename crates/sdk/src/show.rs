//! Text on players' screens. `show` is the one way to put it there; `chat`, `hint`, `title` and
//! `subtitle` are `show` with a place's defaults, and every setting below works on all of them.
//!
//! ```ignore
//! w.hint(player, "bed.base.occupied", &[]);
//! w.title(Who::All, "mymod.round", &[Arg::Int(3)]).times(0.2, 2.0, 0.5);
//! w.show(player, "mymod.timer", &[Arg::Int(left)]).id("mymod:timer").at(0.98, 0.02).scale(2.0);
//! ```

use std::marker::PhantomData;

use crate::abi::{self, Slice, Str, Who, show_place as place};
use crate::ui::raw_args;
use crate::{Arg, Client, Id, Registry, TextPlace, WithComponents, World, callback, host};

/// The engine's places for text; `show_at` takes them or a mod's own (`Registry::text_place`).
pub mod text_places {
    use crate::TextPlace;
    pub static FREE: TextPlace = TextPlace::named("core:free");
    pub static CHAT: TextPlace = TextPlace::named("core:chat");
    pub static HINT: TextPlace = TextPlace::named("core:hint");
    pub static TITLE: TextPlace = TextPlace::named("core:title");
    pub static SUBTITLE: TextPlace = TextPlace::named("core:subtitle");
}

/// A place for text a mod adds (`Registry::text_place`): an element of the HUD the player can
/// move and hide, and how text there looks unless `show` says otherwise.
pub struct TextPlaceBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(TextPlace => TextPlaceBuilder, text_place;);

impl Registry {
    pub fn text_place<'a>(&'a mut self, place: &'static TextPlace) -> TextPlaceBuilder<'a> {
        TextPlaceBuilder { reg: self, e: crate::entry::Entry::of(place) }
    }
}

impl TextPlaceBuilder<'_> {
    /// Where the HUD puts it by default: an anchor (`hud_anchor::TOP_RIGHT`) and its order there.
    pub fn hud(self, anchor: &str, order: i32) -> Self {
        self.set_text(&crate::comps::text_place::ANCHOR, anchor)
            .set(&crate::comps::text_place::ORDER, order)
    }
    /// A spot of its own by default instead: this point of it at the same point of the window,
    /// moved by `dx`, `dy` GUI pixels.
    pub fn at(self, x: f32, y: f32, dx: f32, dy: f32) -> Self {
        self.unset(&crate::comps::text_place::ANCHOR)
            .set(&crate::comps::text_place::AT, [x, y, dx, dy])
    }
    /// How big, 1 being menu text.
    pub fn scale(self, scale: f32) -> Self {
        self.set(&crate::comps::text_place::SCALE, scale)
    }
    /// Seconds of fading in, staying and fading out, by default.
    pub fn times(self, fade_in: f32, stay: f32, fade_out: f32) -> Self {
        self.set(&crate::comps::text_place::TIMES, [fade_in, stay, fade_out])
    }
    /// Several texts at once, one under another; by default a new one replaces what is there.
    pub fn stacks(self) -> Self {
        self.tag(abi::comp::text_place::STACKS)
    }
    /// Theme class of its text (`label` by default).
    pub fn class(self, class: &str) -> Self {
        self.set_text(&crate::comps::text_place::CLASS, class)
    }
    /// The translation the HUD editor shows while the place is empty.
    pub fn sample(self, key: &str) -> Self {
        self.set_text(&crate::comps::text_place::SAMPLE, key)
    }
}

#[derive(Clone, Copy)]
enum Target {
    World(World, Who),
    Client(Client),
}

/// Text on its way to the screen: it goes when this is dropped, at the end of the statement.
pub struct Show<'a> {
    target: Target,
    desc: abi::ShowDesc,
    args: Vec<abi::UiArg>,
    text: PhantomData<&'a str>,
}

impl<'a> Show<'a> {
    fn new(target: Target, place: u32, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        let desc = abi::ShowDesc { place, key: Str::new(key), ..abi::ShowDesc::DEFAULT };
        Show { target, desc, args: raw_args(args), text: PhantomData }
    }

    /// New text with the same place and id replaces this one instead of adding another: a timer
    /// counting in one chat line, several free texts side by side. Empty key and no args takes it
    /// away.
    pub fn id(mut self, id: &'a str) -> Self {
        self.desc.id = Str::new(id);
        self
    }

    /// This point of the text at the same point of the window, in fractions of either: (0, 0) the
    /// top left corner of both, (0.5, 0.5) centered, (1, 1) the bottom right corner of both. It
    /// stands by itself then, whatever its place.
    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.desc.at = [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)];
        self
    }

    /// How big, 1 being menu text. A title is 4, a subtitle 2.
    pub fn scale(mut self, scale: f32) -> Self {
        self.desc.scale = scale;
        self
    }

    /// Seconds of fading in, staying and fading out.
    pub fn times(mut self, fade_in: f32, stay: f32, fade_out: f32) -> Self {
        self.desc.times = [fade_in.max(0.0), stay.max(0.0), fade_out.max(0.0)];
        self
    }

    /// `0xRRGGBB`, for what doesn't color itself.
    pub fn color(mut self, rgb: u32) -> Self {
        self.desc.color = (rgb & 0xFFFFFF) as i64;
        self
    }
}

impl Drop for Show<'_> {
    fn drop(&mut self) {
        self.desc.args = Slice::new(&self.args);
        unsafe {
            match self.target {
                Target::World(w, who) => (host().show)(w.raw(), who, &self.desc),
                Target::Client(c) => (host().client_show)(c.raw(), &self.desc),
            }
        }
    }
}

impl World {
    /// Text in a place on the screen: one of `text_places` or a mod's own.
    pub fn show_at<'a>(&self, who: impl Into<Who>, place: impl Into<Id<TextPlace>>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::World(*self, who.into()), place.into().raw(), key, args)
    }

    /// Text by itself, in the middle of the screen unless `at` says otherwise.
    pub fn show<'a>(&self, who: impl Into<Who>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::World(*self, who.into()), place::FREE, key, args)
    }

    /// A line in the chat, for the player to scroll back to.
    pub fn chat<'a>(&self, who: impl Into<Who>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::World(*self, who.into()), place::CHAT, key, args)
    }

    /// A short line above the hotbar. A player can move or hide `core:hint` in the HUD editor.
    pub fn hint<'a>(&self, who: impl Into<Who>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::World(*self, who.into()), place::HINT, key, args)
    }

    /// Big text over the crosshair. A player can move or hide `core:title` in the HUD editor.
    pub fn title<'a>(&self, who: impl Into<Who>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::World(*self, who.into()), place::TITLE, key, args)
    }

    /// Smaller text under the crosshair. A player can move or hide `core:subtitle` likewise.
    pub fn subtitle<'a>(&self, who: impl Into<Who>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::World(*self, who.into()), place::SUBTITLE, key, args)
    }
}

impl Client {
    /// `World::show_at`, for this client alone.
    pub fn show_at<'a>(&self, place: impl Into<Id<TextPlace>>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::Client(*self), place.into().raw(), key, args)
    }

    /// `World::show`, for this client alone.
    pub fn show<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::Client(*self), place::FREE, key, args)
    }

    /// `World::chat`, for this client alone; `say` sends a line as if typed.
    pub fn chat<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::Client(*self), place::CHAT, key, args)
    }

    /// `World::hint`, for this client alone.
    pub fn hint<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::Client(*self), place::HINT, key, args)
    }

    /// `World::title`, for this client alone.
    pub fn title<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::Client(*self), place::TITLE, key, args)
    }

    /// `World::subtitle`, for this client alone.
    pub fn subtitle<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::Client(*self), place::SUBTITLE, key, args)
    }
}

/// A player's message on its way to the chat (`Registry::chat_format`).
pub struct ChatFormat(*mut abi::ChatFormatArgs);

impl ChatFormat {
    pub fn world(&self) -> World {
        unsafe { World::from_raw((*self.0).world) }
    }

    pub fn player(&self) -> u64 {
        unsafe { (*self.0).player }
    }

    /// What they typed. Pass it as `Arg::Text`, or a player gets to format the line.
    pub fn text(&self) -> &str {
        unsafe { (*self.0).text.as_str() }
    }

    /// The line everyone gets. Not called, it stays the engine's `<name> text`.
    pub fn set(&self, key: &str, args: &[Arg]) {
        let raw = raw_args(args);
        unsafe { (host().chat_format_set)(self.0, Str::new(key), Slice::new(&raw)) }
    }
}

impl Registry {
    /// Writes players' chat lines. There is one line, so one writer: the last mod loaded wins. To
    /// keep a message out of the chat intercept `actions::CHAT`, to read them listen to it.
    pub fn chat_format(&mut self, f: impl Fn(&mut ChatFormat) + Send + Sync + 'static) {
        let cb = callback(move |args: &mut abi::ChatFormatArgs| f(&mut ChatFormat(args)));
        unsafe { (host().reg_chat_format)(self.raw, cb) }
    }
}
