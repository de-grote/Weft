//! Particles, conditions, screen effects: things that only change what players see.
//!
//! ```ignore
//! static SPARK: Particle = Particle::named("spark");
//! r.particle(&SPARK).textures(&["glint_0", "glint_1"]).life(0.3, 0.6).fullbright();
//! w.spawn_particles(&SPARK, pos).count(12).speed(2.0);          // server: everyone nearby sees them
//! // Rain that rings on water and is gone anywhere else:
//! r.particle(&DROP).on_enter_liquid(When::tag("water"), Then::turn_into_still(&RIPPLE)).on_land(When::any(), Then::die());
//! r.condition(&CHARGED, |c| c.client().is_some_and(|v| v.component(&CHARGE).is_some_and(|c| c > 0)));
//! c.post_effect(&Shader::named("mymod:underwater"), 1.0);                  // client: until turned off
//! ```
//! Shaders for text (`{fx:..}`), GUI panels (`shader` in a class's `.gui`), particles, entities and screen
//! effects are assets: `shaders/fx|gui|particle|entity|block|sky|post/<name>.wgsl` in the mod's folder.

use crate::abi::Vec3;

use crate::abi;
use crate::{
    Client, ClientView, Dimension, Music, Particle, Registry, SkyObject, SoundCategory, WithComponents, World, callback, comps, host,
};

/// A particle kind being registered (`Registry::particle`): half a second to a second, a tenth to
/// a fifth of a block, white, without gravity, unless told otherwise.
pub struct ParticleBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(
    Particle => ParticleBuilder, particle;
    SkyObject => SkyBuilder, sky_entry;
);

/// Where a particle's rule holds: the block it hit, or the one it is in.
pub struct When {
    kind: u32,
    tag: String,
}

impl When {
    pub fn any() -> When {
        When { kind: abi::particle_when::ANY, tag: String::new() }
    }
    pub fn liquid() -> When {
        When { kind: abi::particle_when::LIQUID, tag: String::new() }
    }
    /// A block with `tag` (`water`, `lava`, a mod's own). At most 5 different tags over all
    /// particle kinds of the game: the GPU keeps them with each block.
    pub fn tag(tag: &str) -> When {
        When { kind: abi::particle_when::TAG, tag: tag.trim_start_matches('#').to_string() }
    }
}

/// What a particle's rule does.
pub struct Then {
    kind: u32,
    amount: f32,
    into: String,
}

impl Then {
    fn of(kind: u32) -> Then {
        Then { kind, amount: 0.0, into: String::new() }
    }
    pub fn die() -> Then {
        Then::of(abi::particle_then::DIE)
    }
    /// Stays where it is for the rest of its life.
    pub fn stop() -> Then {
        Then::of(abi::particle_then::STOP)
    }
    /// Keeps going along what it hit.
    pub fn slide() -> Then {
        Then::of(abi::particle_then::SLIDE)
    }
    /// Comes back off what it hit with `share` of its speed.
    pub fn bounce(share: f32) -> Then {
        Then { amount: share, ..Then::of(abi::particle_then::BOUNCE) }
    }
    /// Becomes a particle of `kind` where it is, its speed kept and its life starting anew;
    /// `kind` must be drawn the same way (`translucent` or not).
    pub fn turn_into(kind: &Particle) -> Then {
        Then { into: kind.name().to_string(), ..Then::of(abi::particle_then::BECOME) }
    }
    /// `turn_into`, standing still: a ripple where a drop fell.
    pub fn turn_into_still(kind: &Particle) -> Then {
        Then { into: kind.name().to_string(), ..Then::of(abi::particle_then::BECOME_STILL) }
    }
}

impl ParticleBuilder<'_> {
    fn rule(self, event: u32, when: When, then: Then) -> Self {
        let rule = abi::ParticleRule { event, when: when.kind, tag: when.tag, then: then.kind, amount: then.amount, into: then.into };
        self.push(&comps::particle::RULES, rule)
    }
    /// What landing on a block does where `when` holds; rules are tried in order, and without
    /// one that holds it lies there. Any landing or hitting rule makes it stop on blocks.
    pub fn on_land(self, when: When, then: Then) -> Self {
        self.rule(abi::particle_event::LAND, when, then)
    }
    /// Running into a block's side; without a rule it slides along.
    pub fn on_wall(self, when: When, then: Then) -> Self {
        self.rule(abi::particle_event::WALL, when, then)
    }
    /// Running into a block's underside; without a rule it stops.
    pub fn on_ceiling(self, when: When, then: Then) -> Self {
        self.rule(abi::particle_event::CEILING, when, then)
    }
    /// Going into a liquid (`when` asks about that liquid).
    pub fn on_enter_liquid(self, when: When, then: Then) -> Self {
        self.rule(abi::particle_event::ENTER_LIQUID, when, then)
    }
    /// Coming out of one: a bubble pops.
    pub fn on_leave_liquid(self, when: When, then: Then) -> Self {
        self.rule(abi::particle_event::LEAVE_LIQUID, when, then)
    }
    /// The end of its life: by default the end of it, `Then::turn_into` goes on as another kind
    /// (a spark dying into smoke).
    pub fn on_expire(self, when: When, then: Then) -> Self {
        self.rule(abi::particle_event::EXPIRE, when, then)
    }
    /// Frames over its life; relative names are under `particle/`. None: block dust (pieces of
    /// the burst's block).
    pub fn textures(self, names: &[&str]) -> Self {
        let textures = names.iter().map(|t| crate::asset("particle", t)).collect();
        self.set(&comps::particle::TEXTURES, textures)
    }
    /// Seconds, picked between the two.
    pub fn life(self, min: f32, max: f32) -> Self {
        self.set(&comps::particle::LIFE, [min, max])
    }
    /// Edge length in blocks, picked between the two.
    pub fn size(self, min: f32, max: f32) -> Self {
        self.set(&comps::particle::SIZE, [min, max])
    }
    /// Blocks per second squared, pulling down (negative rises).
    pub fn gravity(self, g: f32) -> Self {
        self.set(&comps::particle::GRAVITY, g)
    }
    /// Fraction of speed kept per tick (vanilla 0.98).
    pub fn drag(self, d: f32) -> Self {
        self.set(&comps::particle::DRAG, d)
    }
    /// 0xRRGGBBAA at birth and at the end of its life.
    pub fn color(self, start: u32, end: u32) -> Self {
        self.set(&comps::particle::COLOR, [start, end])
    }
    /// Stops on blocks: lies where it lands, slides along walls.
    pub fn colliding(self) -> Self {
        self.tag(abi::comp::particle::COLLIDE)
    }
    pub fn fullbright(self) -> Self {
        self.tag(abi::comp::particle::FULLBRIGHT)
    }
    pub fn translucent(self) -> Self {
        self.tag(abi::comp::particle::TRANSLUCENT)
    }
    pub fn shrinking(self) -> Self {
        self.tag(abi::comp::particle::SHRINK)
    }
    /// Colors it with `shaders/particle/<name>.wgsl` (`fn shade(p: ParticleFrag) -> vec4<f32>`).
    pub fn shader(self, shader: &crate::Shader) -> Self {
        let name = shader.name().to_string();
        self.set_text(&comps::particle::SHADER, &name)
    }
}

/// A thing in a dimension's sky (`Registry::sky_body`, `sky_stars`, `sky_dome`, `sky_clouds`),
/// an entry named `<dimension>/<object>`: `r.edit(OVERWORLD.sky(&MOON))` changes another mod's.
pub struct SkyBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl SkyBuilder<'_> {
    /// Picture, relative names are under `textures/sky/`. A dome tells its layout by its shape:
    /// twice as wide as high is a panorama, three by two is the six faces of a box.
    pub fn texture(self, name: &str) -> Self {
        let texture = crate::asset("sky", name);
        self.set_text(&comps::sky::TEXTURE, &texture)
    }
    /// Colors it with `shaders/sky/<name>.wgsl` instead of the plain picture.
    pub fn shader(self, shader: &crate::Shader) -> Self {
        let name = shader.name().to_string();
        self.set_text(&comps::sky::SHADER, &name)
    }
    /// Ticks per turn around the sky, negative the other way. Without it, it follows the
    /// dimension's day like the sun does.
    pub fn orbit(self, ticks: f32) -> Self {
        self.set(&comps::sky::ORBIT, ticks)
    }
    /// Stays where its phase, tilt and heading put it.
    pub fn orbit_still(self) -> Self {
        self.orbit(0.0)
    }
    /// Back to following the dimension's day, for changing another mod's object.
    pub fn orbit_day(self) -> Self {
        self.orbit(f32::INFINITY)
    }
    /// Where on its orbit it starts, in turns: 0 rising in the east, 0.25 overhead, 0.5 setting.
    /// A moon opposite the sun is 0.5.
    pub fn phase(self, turns: f32) -> Self {
        self.set(&comps::sky::PHASE, turns)
    }
    /// Degrees its orbit leans away from overhead, to the south.
    pub fn tilt(self, degrees: f32) -> Self {
        self.set(&comps::sky::TILT, degrees)
    }
    /// Degrees its orbit turns around the vertical: 90 rises in the north instead of the east.
    pub fn heading(self, degrees: f32) -> Self {
        self.set(&comps::sky::HEADING, degrees)
    }
    /// Degrees across the sky (vanilla's sun 33, moon 23); of each star, for stars.
    pub fn size(self, degrees: f32) -> Self {
        self.set(&comps::sky::SIZE, degrees)
    }
    /// Ticks per turn around its own middle.
    pub fn spin(self, ticks: f32) -> Self {
        self.set(&comps::sky::SPIN, ticks)
    }
    /// How much of it shows by full day and by deep night, 0..1 (stars: 0 and 0.22).
    pub fn opacity(self, day: f32, night: f32) -> Self {
        self.set(&comps::sky::OPACITY, [day, night])
    }
    /// 0xRRGGBB multiplied in.
    pub fn color(self, rgb: u32) -> Self {
        self.set(&comps::sky::COLOR, rgb)
    }
    /// The picture is a grid of frames, shown one after another: a moon's phases.
    pub fn frames(self, columns: u32, rows: u32) -> Self {
        self.set(&comps::sky::FRAMES, [columns, rows])
    }
    /// Ticks each frame lasts; by default a day of the dimension.
    pub fn frame_ticks(self, ticks: u32) -> Self {
        self.set(&comps::sky::FRAME_TICKS, ticks)
    }
    /// Lights the horizon while rising and setting, a sunset in this color (0xRRGGBB).
    pub fn glow(self, rgb: u32) -> Self {
        self.set(&comps::sky::GLOW, rgb)
    }
    /// Adds its light to what is behind it. Without it, it covers what is behind it, so a moon
    /// with an opaque picture hides any other that passes behind it.
    pub fn blend_add(self) -> Self {
        self.tag(abi::comp::sky::ADDITIVE)
    }
    /// Covers what is behind it, for an object someone made `blend_add`.
    pub fn blend_cover(self) -> Self {
        self.untag(abi::comp::sky::ADDITIVE)
    }
    /// Drawn low to high, one over the other; registration order among equals. Bodies start at
    /// 0, stars at -100, domes at -200.
    pub fn order(self, order: i32) -> Self {
        self.set(&comps::sky::ORDER, order)
    }
    /// The height of clouds' underside (only clouds use it, like `drift`).
    pub fn height(self, y: f32) -> Self {
        self.set(&comps::sky::HEIGHT, y)
    }
    /// Blocks per tick clouds drift along x, negative the other way.
    pub fn drift(self, blocks_per_tick: f32) -> Self {
        self.set(&comps::sky::DRIFT, blocks_per_tick)
    }
    /// How many stars (only stars use it, like `seed`).
    pub fn count(self, n: u32) -> Self {
        self.set(&comps::sky::COUNT, n)
    }
    /// What scatters the stars: another seed, another sky.
    pub fn seed(self, seed: u32) -> Self {
        self.set(&comps::sky::SEED, seed)
    }
}

impl Registry {
    fn sky_object<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject, kind: u32) -> SkyBuilder<'a> {
        let e = crate::entry::Entry::new(abi::entry_kind::SKY, format!("{}/{}", dimension.name(), object.name()));
        SkyBuilder { reg: self, e }.set(&comps::sky::KIND, kind)
    }

    fn sky_entry(&mut self, object: &'static SkyObject) -> SkyBuilder<'_> {
        SkyBuilder { reg: self, e: crate::entry::Entry::of(object) }
    }

    /// A picture on an orbit, a sun or a moon, independent of everything else in the sky:
    ///
    /// ```ignore
    /// static PLANET: SkyObject = SkyObject::named("planet");
    /// r.sky_body(&OVERWORLD, &PLANET).texture("planet").size(6.0).orbit(96000.0).phase(0.1).tilt(20.0);
    /// ```
    ///
    /// Another mod's object, by its dimension: `r.edit(OVERWORLD.sky(&MOON)).size(40.0)` changes
    /// only what it sets, `r.remove(OVERWORLD.sky(&STARS))` takes it out of that sky.
    pub fn sky_body<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a> {
        self.sky_object(dimension, object, abi::sky_kind::BODY)
    }

    /// Points scattered over the sky and turning together, seen at night unless `opacity` says
    /// otherwise.
    pub fn sky_stars<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a> {
        self.sky_object(dimension, object, abi::sky_kind::STARS)
    }

    /// The whole sky at once, a skybox picture or a shader, standing still unless given an orbit.
    /// It starts out still, and over the day's colors; `opacity(0.0, 1.0)` keeps it to the night.
    pub fn sky_dome<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a> {
        self.sky_object(dimension, object, abi::sky_kind::DOME)
    }

    /// A layer of boxes in the air, 12 blocks each, wherever `texture`'s pixels are filled:
    /// `r.sky_clouds(&OVERWORLD, &CLOUDS).texture("clouds").height(300.0)`.
    pub fn sky_clouds<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a> {
        self.sky_object(dimension, object, abi::sky_kind::CLOUDS)
    }

    /// A particle kind, see `ParticleBuilder`. Editing or replacing `core:poof`
    /// (`Particle::named("core:poof")`) changes how the engine's own smoke looks, `core:block` its
    /// block dust.
    pub fn particle<'a>(&'a mut self, particle: &'static Particle) -> ParticleBuilder<'a> {
        ParticleBuilder { reg: self, e: crate::entry::Entry::of(particle) }
    }

    /// A condition music, text (`{if:<ns>:<name>}`), loot, spawning, poses and cameras (`.when`,
    /// `Cond::from(&C)`) can ask, with an argument if written so (`mymod:near(lava)`, `c.arg()`).
    /// `f` is answered on whichever side asks: on the server (`c.world()`) for loot and spawning,
    /// on the client (`c.client()`) for music and text, every frame such a text is on screen.
    /// Every game mode is a condition too, by its name.
    pub fn condition(&mut self, condition: &'static crate::Condition, f: impl Fn(&ConditionCtx) -> bool + Send + Sync + 'static) {
        let cb = callback(move |a: &mut abi::ConditionArgs| a.result = f(&ConditionCtx(*a)) as u32);
        let mut e = crate::entry::Entry::of(condition);
        e.ops
            .push(crate::content::Pending::value(abi::comp::CONDITION_CALL, "callback", abi::data::bytes_of(&cb)));
        e.send(self);
    }
}

/// What a condition is asked about.
pub struct ConditionCtx(abi::ConditionArgs);

impl ConditionCtx {
    /// The server's world, when the server asks (loot, spawning).
    pub fn world(&self) -> Option<World> {
        (!self.0.world.is_null()).then(|| World::from_raw(self.0.world))
    }
    /// The client, when it asks (music, text): its own player.
    pub fn client(&self) -> Option<ClientView> {
        (!self.0.client.is_null()).then(|| ClientView::from_raw(self.0.client))
    }
    /// The spot, the block, the player.
    pub fn pos(&self) -> Vec3 {
        self.0.pos
    }
    /// Who it is about (the killer or breaker for loot), 0 for nobody.
    pub fn entity(&self) -> u64 {
        self.0.entity
    }
    /// What is between its parentheses where it is used, empty for none.
    pub fn arg(&self) -> &str {
        unsafe { self.0.arg.as_str() }
    }
}

/// Particles sent when this is dropped: `w.spawn_particles(&SPARK, pos).count(10).speed(1.5)`.
pub struct Burst {
    target: crate::out::Out,
    burst: abi::ParticleBurst,
    area: Option<crate::Shape>,
}

impl Burst {
    pub(crate) fn new(target: crate::out::Out, kind: u32, pos: Vec3) -> Burst {
        Burst { target, burst: abi::ParticleBurst { kind, pos, ..abi::ParticleBurst::DEFAULT }, area: None }
    }
    /// For whom, of the players on the server (those near it unless said).
    pub fn to(mut self, who: impl Into<crate::Who>) -> Self {
        self.target.to(who.into());
        self
    }
    /// The particles start anywhere inside `shape` (a ring, a shell, a magic circle) instead
    /// of a box around the position; `outward` sends them away from the position.
    pub fn area(mut self, shape: &crate::Shape) -> Self {
        self.area = Some(shape.clone());
        self
    }
    pub fn count(mut self, n: u32) -> Self {
        self.burst.count = n;
        self
    }
    /// Each particle starts up to this far from the position on every axis.
    pub fn spread(mut self, s: f32) -> Self {
        self.burst.spread = Vec3::new(s as f64, s as f64, s as f64);
        self
    }
    /// `spread` of its own on each axis: a flat sheet, a column.
    pub fn spread_box(mut self, s: impl Into<Vec3>) -> Self {
        self.burst.spread = s.into();
        self
    }
    /// Starting velocity, blocks per second.
    pub fn velocity(mut self, v: impl Into<Vec3>) -> Self {
        self.burst.velocity = v.into();
        self
    }
    /// Random extra velocity up to this, in any direction.
    pub fn speed(mut self, s: f32) -> Self {
        self.burst.speed = s;
        self
    }
    /// 0xRRGGBB the kind's color is multiplied by: one white kind in any color.
    pub fn color(mut self, rgb: u32) -> Self {
        self.burst.color = rgb;
        self
    }
    /// Each flies away from the middle of the burst, more or less, like the pieces of a broken block.
    pub fn outward(mut self) -> Self {
        self.burst.outward = 1;
        self
    }
    /// Block state the textureless kinds (block dust) take their texture from.
    pub fn block(mut self, state: u32) -> Self {
        self.burst.block = state;
        self
    }
}

impl Drop for Burst {
    fn drop(&mut self) {
        if std::thread::panicking() || self.burst.kind == u32::MAX {
            return;
        }
        if !self.target.here() {
            return;
        }
        let send = |b: &abi::ParticleBurst| match self.target {
            crate::out::Out::World(w, who) => unsafe { (host().particle_burst)(w.raw(), who, b) },
            crate::out::Out::Client(c, _) => unsafe { (host().client_particle_burst)(c.raw(), b) },
        };
        let Some(area) = &self.area else { return send(&self.burst) };
        // One particle per point inside, found by trying points in the box around the shape.
        let (lo, hi) = area.bounds();
        let mut seed = self.burst.pos.x.to_bits() ^ self.burst.pos.y.to_bits().rotate_left(21) ^ self.burst.pos.z.to_bits().rotate_left(42);
        let mut next = || {
            seed = abi::fnv(&seed.to_le_bytes());
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let c = self.burst.pos;
        let v = self.burst.velocity;
        let speed = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt().max(1.0);
        let mut placed = 0;
        for _ in 0..self.burst.count.saturating_mul(20).min(20_000) {
            if placed >= self.burst.count {
                break;
            }
            let p = Vec3::new(lo.x + (hi.x - lo.x) * next(), lo.y + (hi.y - lo.y) * next(), lo.z + (hi.z - lo.z) * next());
            if !area.contains(p) {
                continue;
            }
            let mut one = abi::ParticleBurst { pos: p, count: 1, spread: Vec3::ZERO, outward: 0, ..self.burst };
            if self.burst.outward != 0 {
                let d = p - c;
                let len = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt().max(1e-6);
                one.velocity = Vec3::new(d.x / len * speed, d.y / len * speed, d.z / len * speed);
            }
            send(&one);
            placed += 1;
        }
    }
}

impl Client {
    /// Turns a screen effect (`shaders/post/<name>.wgsl`) on at `strength` (0 turns it off).
    pub fn post_effect(&self, effect: &crate::Shader, strength: f32) -> bool {
        let name = format!("{}/{}", crate::full_name("post"), effect.name());
        if strength <= 0.0 {
            self.clear_environment(&name);
            true
        } else {
            self.environment(&crate::Environment::new(&name).post(effect, strength))
        }
    }
}

/// A sound event being registered (`Registry::sound`): at volume 1 and as recorded, heard 16
/// blocks away, a block sound, unless told otherwise.
pub struct SoundBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(
    Sound => SoundBuilder, sound;
    Music => MusicBuilder, music;
    SoundCategory => SoundCategoryBuilder, sound_category_entry;
);

impl SoundBuilder<'_> {
    /// Variants picked at random: `sounds/<name>.ogg` or `.wav`, relative to this mod.
    pub fn files(self, names: &[&str]) -> Self {
        let files = names.iter().map(|n| crate::asset("", n)).collect();
        self.set(&crate::comps::sound::FILES, files)
    }
    /// Category for its volume setting: the engine's (`core:creatures` ...) or one registered
    /// with `Registry::sound_category` (any mod's).
    pub fn category(self, category: &SoundCategory) -> Self {
        self.set_text(&crate::comps::sound::CATEGORY, category.name())
    }
    pub fn volume(self, v: f32) -> Self {
        self.set(&crate::comps::sound::VOLUME, v)
    }
    /// Playback speed picked between the two.
    pub fn pitch(self, min: f32, max: f32) -> Self {
        self.set(&crate::comps::sound::PITCH, [min, max])
    }
    /// Blocks away it is still heard.
    pub fn range(self, blocks: f32) -> Self {
        self.set(&crate::comps::sound::RANGE, blocks)
    }
    /// Full volume within `full_within` blocks, then fading to nothing at `range` along `curve`
    /// (by default within 1 block, evenly).
    pub fn falloff(self, full_within: f32, curve: Falloff) -> Self {
        let n = match curve {
            Falloff::Even => 0,
            Falloff::Late(n) => n.clamp(1, 8) as i32,
            Falloff::Early(n) => -(n.clamp(1, 8) as i32),
        };
        self.set(&crate::comps::sound::FULL_WITHIN, full_within)
            .set(&crate::comps::sound::FALLOFF, n)
    }
}

/// How a sound fades with distance (`SoundBuilder::falloff`).
#[derive(Clone, Copy, Debug)]
pub enum Falloff {
    Even,
    /// Stays loud longer, then drops near the edge; the number is how sharply (1..8).
    Late(u8),
    /// Drops soon after `full_within`, then trails off.
    Early(u8),
}

/// Background music being registered (`Registry::music`): after a quiet while, a track of the
/// first music whose conditions hold.
pub struct MusicBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl MusicBuilder<'_> {
    /// Files streamed (`sounds/<name>.ogg|.wav`), relative to this mod.
    pub fn tracks(self, names: &[&str]) -> Self {
        let tracks = names.iter().map(|n| crate::asset("", n)).collect();
        self.set(&crate::comps::music::TRACKS, tracks)
    }
    pub fn dimension(self, dimension: &crate::Dimension) -> Self {
        self.set_text(&crate::comps::music::DIMENSION, dimension.name())
    }
    /// Plays only while this holds (`&conditions::NIGHT`, a mod's own, any `Cond`); call again
    /// for more, all of which have to hold.
    pub fn when(self, c: impl Into<crate::Cond>) -> Self {
        self.push(&crate::comps::music::WHEN, c.into().0)
    }
}

/// A sound category being registered (`Registry::sound_category`).
pub struct SoundCategoryBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl Registry {
    /// A sound event, see `SoundBuilder`; blocks and entities name groups of them with `.sound_group()`.
    pub fn sound<'a>(&'a mut self, sound: &'static crate::Sound) -> SoundBuilder<'a> {
        SoundBuilder { reg: self, e: crate::entry::Entry::of(sound) }
    }

    /// A sound category with its own volume setting (`<ns>:volume_<name>`, in the mod's
    /// settings); `world`: heard through caves and water like block sounds.
    pub fn sound_category(&mut self, category: &'static SoundCategory, world: bool) -> SoundCategoryBuilder<'_> {
        let b = self.sound_category_entry(category);
        if world { b.tag(abi::comp::SOUND_CATEGORY_WORLD) } else { b }
    }

    fn sound_category_entry(&mut self, category: &'static SoundCategory) -> SoundCategoryBuilder<'_> {
        SoundCategoryBuilder { reg: self, e: crate::entry::Entry::of(category) }
    }

    /// Background music, see `MusicBuilder`.
    pub fn music<'a>(&'a mut self, music: &'static Music) -> MusicBuilder<'a> {
        MusicBuilder { reg: self, e: crate::entry::Entry::of(music) }
    }
}

/// A sound about to play (`Output::play_sound`).
pub struct PlaySound {
    to: crate::out::Out,
    sound: u32,
    pos: Option<Vec3>,
    volume: f32,
    pitch: f32,
}

impl PlaySound {
    pub(crate) fn new(to: crate::out::Out, sound: u32, pos: Option<Vec3>, volume: f32) -> PlaySound {
        PlaySound { to, sound, pos, volume, pitch: 0.0 }
    }
    /// Who hears it, of the players on the server (those in its range unless said).
    pub fn to(mut self, who: impl Into<crate::Who>) -> Self {
        self.to.to(who.into());
        self
    }
    /// Playback speed (1 = as recorded, 2 an octave up) instead of one picked from the sound's
    /// range: a note block's `2f32.powf((key - 12) as f32 / 12.0)`.
    pub fn pitch(mut self, pitch: f32) -> Self {
        self.pitch = pitch.max(0.01);
        self
    }
}

impl Drop for PlaySound {
    fn drop(&mut self) {
        let p = self
            .pos
            .as_ref()
            .map_or(std::ptr::null(), |p| p as *const Vec3);
        if !self.to.here() {
            return;
        }
        unsafe {
            match self.to {
                crate::out::Out::World(w, who) => (host().play_sound)(w.raw(), who, self.sound, p, self.volume, self.pitch),
                crate::out::Out::Client(c, _) => (host().client_play_sound)(c.raw(), self.sound, p, self.volume, self.pitch),
            }
        }
    }
}

/// The engine's sound categories, each with its volume setting.
pub mod sound_categories {
    use crate::SoundCategory;
    pub static MUSIC: SoundCategory = SoundCategory::named("core:music");
    pub static BLOCKS: SoundCategory = SoundCategory::named("core:blocks");
    pub static CREATURES: SoundCategory = SoundCategory::named("core:creatures");
    pub static PLAYERS: SoundCategory = SoundCategory::named("core:players");
    pub static AMBIENT: SoundCategory = SoundCategory::named("core:ambient");
    pub static UI: SoundCategory = SoundCategory::named("core:ui");
}
