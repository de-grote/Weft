use crate::abi::BlockPos;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::abi::{self, BlockModel, RenderLayer, Slice, Str};
use crate::component::{self as c, Component, Data, Value};
use crate::comps::block as bc;
use crate::{World, callback, host, qualify};

/// What a handle can name. Kinds with engine ids (`HasId`) can also be resolved to one.
pub mod kind {
    pub trait Kind {
        /// `abi::entry_kind`, or `u32::MAX` for things known only by name.
        const KIND: u32;
    }
    /// Kinds the engine numbers (blocks, items...): their handles give ids.
    pub trait HasId: Kind {}
    /// A component of kind marker `Self` goes on registrations of kind `K`.
    pub trait On<K> {}
    /// Components of any kind: mods' own vocabulary.
    pub struct Any;
    impl<K> On<K> for Any {}
    /// Kinds registered with a builder: `Registry::edit` and `replace` start it in that mode
    /// (`abi::entry_mode`).
    pub trait Builds: Kind + Sized {
        type Builder<'a>
        where
            Self: 'a;
        fn builder<'a>(r: &'a mut crate::Registry, h: &'static super::Handle<Self>, mode: u32) -> Self::Builder<'a>;
    }
    macro_rules! kinds {
        ($($n:ident = $v:expr),*) => {
            $(pub struct $n;
            impl Kind for $n {
                const KIND: u32 = $v;
            }
            impl HasId for $n {}
            impl On<$n> for $n {})*
        };
    }
    kinds!(
        Block = crate::abi::entry_kind::BLOCK,
        Item = crate::abi::entry_kind::ITEM,
        Entity = crate::abi::entry_kind::ENTITY,
        Biome = crate::abi::entry_kind::BIOME,
        Dimension = crate::abi::entry_kind::DIMENSION,
        Menu = crate::abi::entry_kind::MENU,
        Particle = crate::abi::entry_kind::PARTICLE,
        Config = crate::abi::entry_kind::CONFIG,
        Sound = crate::abi::entry_kind::SOUND,
        DamageCause = crate::abi::entry_kind::DAMAGE_CAUSE,
        EntityEffect = crate::abi::entry_kind::ENTITY_EFFECT,
        TextPlace = crate::abi::entry_kind::TEXT_PLACE,
        GameMode = crate::abi::entry_kind::GAME_MODE,
        Pose = crate::abi::entry_kind::POSE,
        Weather = crate::abi::entry_kind::WEATHER,
        Precipitation = crate::abi::entry_kind::PRECIPITATION,
        Condition = crate::abi::entry_kind::CONDITION,
        Music = crate::abi::entry_kind::MUSIC,
        CameraMode = crate::abi::entry_kind::CAMERA,
        EquipmentSlot = crate::abi::entry_kind::EQUIPMENT_SLOT,
        CreativeTab = crate::abi::entry_kind::CREATIVE_TAB,
        SoundCategory = crate::abi::entry_kind::SOUND_CATEGORY,
        EntityCategory = crate::abi::entry_kind::ENTITY_CATEGORY,
        TintKind = crate::abi::entry_kind::TINT_KIND,
        RecipeKind = crate::abi::entry_kind::RECIPE_KIND,
        Attribute = crate::abi::entry_kind::ATTRIBUTE,
        Key = crate::abi::entry_kind::KEY,
        Liquid = crate::abi::entry_kind::LIQUID,
        Feature = crate::abi::entry_kind::FEATURE,
        Record = crate::abi::entry_kind::RECORD
    );
    /// Kinds without ids: entries of their own number, or things known only by name.
    macro_rules! named_kinds {
        ($($n:ident $(= $v:expr)?),*) => {
            $(pub struct $n;
            impl Kind for $n {
                const KIND: u32 = named_kinds!(@kind $($v)?);
            }
            impl On<$n> for $n {})*
        };
        (@kind $v:expr) => { $v };
        (@kind) => { u32::MAX };
    }
    named_kinds!(
        SkyObject = crate::abi::entry_kind::SKY,
        HudElement = crate::abi::entry_kind::HUD_ELEMENT,
        ScreenPart = crate::abi::entry_kind::SCREEN_PART,
        WordSet = crate::abi::entry_kind::WORDS,
        Declaration = crate::abi::entry_kind::COMPONENT,
        Recipe = crate::abi::entry_kind::RECIPE,
        LootTable = crate::abi::entry_kind::LOOT,
        Reaction = crate::abi::entry_kind::REACTION,
        Template = crate::abi::entry_kind::TEMPLATE,
        Palette = crate::abi::entry_kind::PALETTE,
        SpawnRule = crate::abi::entry_kind::SPAWN,
        Terrain = crate::abi::entry_kind::TERRAIN,
        BoneMotion = crate::abi::entry_kind::BONE_MOTION,
        Step = crate::abi::entry_kind::STEP,
        Phase = crate::abi::entry_kind::PHASE,
        GizmoGroup,
        Shader,
        Question,
        Chain
    );
    /// The components saying where things go in the world (`comps::common`).
    pub struct Placement;
    impl On<Feature> for Placement {}
    impl On<SpawnRule> for Placement {}
}

const UNRESOLVED: u32 = u32::MAX - 1;

/// A thing a mod owns (block, item, entity kind, menu, key, screen...), named in its handle:
///
/// ```ignore
/// static STONE: Block = Block::named("stone");        // this mod's, "<ns>:stone"
/// static COAL: Item = Item::named("base:coal");       // another mod's
/// r.block(&STONE).hardness(1.5);                      // registered by its handle
/// if STONE.is(w.get(pos)) { .. }
/// ```
///
/// A name without a namespace is the declaring mod's, the same when registering and reading.
/// Ids are known once the engine froze the registries.
pub struct Handle<K: kind::Kind> {
    id: AtomicU32,
    count: AtomicU32,
    name: &'static str,
    full: std::sync::OnceLock<String>,
    _k: PhantomData<fn() -> K>,
}

impl<K: kind::Kind> Handle<K> {
    pub const fn named(name: &'static str) -> Self {
        Handle { id: AtomicU32::new(UNRESOLVED), count: AtomicU32::new(0), name, full: std::sync::OnceLock::new(), _k: PhantomData }
    }

    /// A handle for a name made at runtime, for families of content built in a loop
    /// (`r.item(Item::generated(format!("{metal}_ingot")))`). Lives as long as the game, like
    /// a `static`; meant for registration, not for every tick.
    pub fn generated(name: String) -> &'static Self {
        Box::leak(Box::new(Self::named(Box::leak(name.into_boxed_str()))))
    }

    /// `generated`, the name as written, even without a namespace (a component's).
    pub(crate) fn as_written(name: &str) -> &'static Self {
        let h = Self::generated(name.to_string());
        let _ = h.full.set(name.to_string());
        h
    }

    /// Full name, `ns:name`.
    pub fn name(&self) -> &str {
        self.full.get_or_init(|| crate::full_name(self.name))
    }

    pub(crate) fn id_cell(&self) -> *const AtomicU32 {
        &self.id
    }

    /// The name without its namespace.
    pub fn path(&self) -> &str {
        let n = self.name();
        n.split_once(':').map_or(n, |x| x.1)
    }
}

impl Block {
    /// Its ore (a feature named like it), for `Registry::edit`: `r.edit(base::IRON_ORE.ore()).exposed(0.5)`.
    pub fn ore(&self) -> &'static Feature {
        Feature::generated(self.name().to_string())
    }
    /// Its liquid, for `Registry::edit`: `r.edit(base::WATER.liquid()).fog(0x204020, -8.0, 12.0)`.
    pub fn liquid(&self) -> &'static Liquid {
        Liquid::generated(self.name().to_string())
    }
    /// What it drops, for `Registry::edit`: `r.edit(base::GRAVEL.loot()).pool(..)`.
    pub fn loot(&self) -> &'static LootTable {
        LootTable::generated(self.name().to_string())
    }
}

impl Dimension {
    /// Its object `object` of the sky, for `Registry::edit` and `remove`:
    /// `r.remove(OVERWORLD.sky(&base::STARS))`.
    pub fn sky(&self, object: &SkyObject) -> &'static SkyObject {
        SkyObject::generated(format!("{}/{}", self.name(), object.name()))
    }
    /// Its terrain, for `Registry::edit`: `r.edit(OVERWORLD.terrain()).rule_before(..)`.
    pub fn terrain(&self) -> &'static Terrain {
        Terrain::generated(self.name().to_string())
    }
}

impl Template {
    /// The structure placing it (a feature named like it), for `Registry::edit`:
    /// `r.edit(WELL.structure()).spacing(96)`.
    pub fn structure(&self) -> &'static Feature {
        Feature::generated(self.name().to_string())
    }
}

impl EntityKind {
    /// What it drops, for `Registry::edit`: `r.edit(base::ZOMBIE.loot()).pool(..)`.
    pub fn loot(&self) -> &'static LootTable {
        LootTable::generated(self.name().to_string())
    }
}

impl Item {
    /// `count` of this item, plain; `.with(&COMP, v)` and `.with_wear(n)` give it data.
    pub fn stack(&self, count: u32) -> abi::ItemStack {
        abi::ItemStack::new(self.id(), count)
    }
}

impl<K: kind::HasId> Handle<K> {
    /// For blocks, the first state. `u32::MAX` if unknown.
    #[inline]
    pub fn id(&self) -> u32 {
        let id = self.id.load(Ordering::Relaxed);
        if id < UNRESOLVED { id } else { self.resolve(id) }
    }

    #[cold]
    fn resolve(&self, id: u32) -> u32 {
        let frozen = unsafe { !(host().component_table)().is_null() };
        if id == u32::MAX || !frozen {
            return u32::MAX;
        }
        let mut count = 0;
        let id = unsafe { (host().find)(K::KIND, Str::new(self.name()), &mut count) };
        if id != u32::MAX {
            self.count.store(count, Ordering::Relaxed);
            self.id.store(id, Ordering::Relaxed);
        } else {
            // Frozen and still unknown: a typo or a missing mod. Say so once.
            crate::error!(
                "no {} named {}",
                abi::entry_kind::NAMES
                    .get(K::KIND as usize)
                    .unwrap_or(&"entry"),
                self.name()
            );
            self.id.store(u32::MAX, Ordering::Relaxed);
        }
        id
    }

    /// The id of what a player typed (`/give` style input), `None` if there is no such thing.
    /// Code names things with handles; this is for text that only exists at runtime.
    pub fn lookup(name: &str) -> Option<u32> {
        let id = unsafe { (host().find)(K::KIND, Str::new(name), std::ptr::null_mut()) };
        (id != u32::MAX).then_some(id)
    }

    /// The block's state with properties `props` set, the others at their defaults:
    /// `FURNACE.with(&[("lit", "true")])`. `u32::MAX` if the block or a value is unknown.
    pub fn with(&self, props: &[(&str, &str)]) -> u32 {
        props
            .iter()
            .fold(self.id(), |s, (k, v)| if s == u32::MAX { s } else { unsafe { (host().state_with)(s, Str::new(k), Str::new(v)) } })
    }

    /// True if `state` belongs to this block.
    #[inline]
    pub fn is(&self, state: u32) -> bool {
        let n = match self.count.load(Ordering::Relaxed) {
            0 => self.count(),
            n => n,
        };
        state.wrapping_sub(self.id()) < n
    }

    /// How many states it has (1 for what isn't a block), 0 before the registries are frozen.
    #[cold]
    fn count(&self) -> u32 {
        if unsafe { (host().component_table)().is_null() } {
            return 0;
        }
        let mut n = 0;
        unsafe { (host().find)(K::KIND, Str::new(self.name()), &mut n) };
        self.count.store(n.max(1), Ordering::Relaxed);
        n.max(1)
    }
}

pub type Block = Handle<kind::Block>;
pub type Item = Handle<kind::Item>;
pub type Biome = Handle<kind::Biome>;
pub type Dimension = Handle<kind::Dimension>;
pub type Particle = Handle<kind::Particle>;
pub type SkyObject = Handle<kind::SkyObject>;
pub type Sound = Handle<kind::Sound>;
pub type EntityKind = Handle<kind::Entity>;

/// A thing by its handle, or by the id that handle gives: what takes this takes either.
pub struct Id<H> {
    raw: u32,
    _p: std::marker::PhantomData<fn() -> H>,
}

impl<H> Id<H> {
    pub fn raw(&self) -> u32 {
        self.raw
    }
}

impl<H> From<u32> for Id<H> {
    fn from(raw: u32) -> Self {
        Id { raw, _p: std::marker::PhantomData }
    }
}

impl<K: kind::HasId> From<&Handle<K>> for Id<Handle<K>> {
    fn from(h: &Handle<K>) -> Self {
        h.id().into()
    }
}
pub type Menu = Handle<kind::Menu>;
pub type Key = Handle<kind::Key>;
pub type CameraMode = Handle<kind::CameraMode>;

pub type HudElement = Handle<kind::HudElement>;
/// Something a mod adds to a place of a screen (`Registry::screen_part`), named
/// `<mod>:<screen>/<area>`.
pub type ScreenPart = Handle<kind::ScreenPart>;
/// Words command parameters `word(#set)` complete from (`Registry::word_set`); the engine's are
/// in `word_sets`.
pub type WordSet = Handle<kind::WordSet>;
/// A component's declaration (`Registry::entity_component`, `stack_component`, `storage`),
/// named like the component: `HUNGER.declaration()`.
pub type Declaration = Handle<kind::Declaration>;
/// A recipe (`Registry::shaped`, `shapeless`), for `Registry::edit` and `remove`.
pub type Recipe = Handle<kind::Recipe>;

/// The engine's word sets.
pub mod word_sets {
    use super::WordSet;
    /// `/gizmo`'s groups (`Registry::gizmo_group`).
    pub static GIZMOS: WordSet = WordSet::named("core:gizmos");
}
pub type GizmoGroup = Handle<kind::GizmoGroup>;
/// A shader file of a mod, by its name: `shaders/<block|entity|particle|sky|post>/<name>.wgsl`,
/// the folder by what uses it. Nothing to register, the file is it.
pub type Shader = Handle<kind::Shader>;
/// The liquid of a block, named like it (`r.liquid(&WATER)` makes one), for `Registry::edit`;
/// its id is what `liquid_of` gives.
pub type Liquid = Handle<kind::Liquid>;
/// What happens where two liquids meet (`Registry::liquid_reaction`).
pub type Reaction = Handle<kind::Reaction>;
pub type TintKind = Handle<kind::TintKind>;
/// Anything generation puts into the world (`Registry::feature` and its presets `ore`,
/// `scatter`, `structure`, `decoration`); its id is `FeaturePlaced::feature`.
pub type Feature = Handle<kind::Feature>;
/// A fixed shape of blocks (`Registry::template`), for features, moving bodies and
/// `World::place_template`.
pub type Template = Handle<kind::Template>;
/// Blocks for templates' slots (`Registry::palette`).
pub type Palette = Handle<kind::Palette>;
/// A mod's own data (`Registry::record`): what it is says its tags and components, the engine
/// reads none of it.
pub type Record = Handle<kind::Record>;
/// A natural spawning rule (`Registry::natural_spawn`), named `<mod>:<entity>`, `/2`.. unless
/// `named`.
pub type SpawnRule = Handle<kind::SpawnRule>;
/// A dimension's terrain, named like it (`Registry::terrain`, `OVERWORLD.terrain()`).
pub type Terrain = Handle<kind::Terrain>;

/// The engine's tint kinds; biomes color them with `.tint(&GRASS, 0x..)`.
pub mod tints {
    use super::TintKind;
    pub static GRASS: TintKind = TintKind::named("core:grass");
    pub static FOLIAGE: TintKind = TintKind::named("core:foliage");
    pub static WATER: TintKind = TintKind::named("core:water");
}
pub type EntityCategory = Handle<kind::EntityCategory>;
/// What hurt an entity (`World::damage`): its armor and invulnerability rules and death message
/// come with it (`Registry::damage_cause`). The engine's are in `damage_causes`.
pub type DamageCause = Handle<kind::DamageCause>;

/// A shader effect drawn on entities (`World::entity_effect`): `shaders/entity_effect/<name>.wgsl`
/// of the mod with `fn effect(p: EntityPixel, c: vec4<f32>, strength: f32) -> vec4<f32>`.
pub type EntityEffect = Handle<kind::EntityEffect>;
/// A place on the screen for text (`World::show_at`); the engine's are in `text_places`.
pub type TextPlace = Handle<kind::TextPlace>;

/// The engine's entity effects.
pub mod entity_effects {
    use super::EntityEffect;
    /// White.
    pub static FLASH: EntityEffect = EntityEffect::named("core:flash");
    /// Red, what damage shows by itself.
    pub static HURT: EntityEffect = EntityEffect::named("core:hurt");
    /// Lit up whatever the light around it.
    pub static GLOW: EntityEffect = EntityEffect::named("core:glow");
    /// Not drawn: the light every burning entity gives (`r.edit(&BURNING).light_rgb(..)`).
    pub static BURNING: EntityEffect = EntityEffect::named("core:burning");
}

/// An entity effect being registered: drawn by `shaders/entity_effect/<its name>.wgsl` unless
/// `shader` says another.
pub struct EntityEffectBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(EntityEffect => EntityEffectBuilder, entity_effect;);

impl crate::Registry {
    pub fn entity_effect<'a>(&'a mut self, effect: &'static EntityEffect) -> EntityEffectBuilder<'a> {
        EntityEffectBuilder { reg: self, e: crate::entry::Entry::of(effect) }
    }

    /// Something to fall from the sky (`.texture`, `.speed`, `.density`); a biome lets it fall
    /// with `.precipitation(..)`.
    pub fn precipitation<'a>(&'a mut self, p: &'static Precipitation) -> PrecipitationBuilder<'a> {
        PrecipitationBuilder { reg: self, e: crate::entry::Entry::of(p) }
    }
}

impl EntityEffectBuilder<'_> {
    pub fn shader(self, shader: &crate::Shader) -> Self {
        let name = shader.name().to_string();
        self.set_text(&crate::comps::entity_effect::SHADER, &name)
    }
    /// Nothing drawn: an effect only of hazards and modifiers.
    pub fn unseen(self) -> Self {
        self.set_text(&crate::comps::entity_effect::SHADER, "")
    }
    /// What it does to its entity while on (`Hazard::own(1.0, &MAGIC).every(25)`: poison), its
    /// damage times the level (negative heals); once when given if it is `instant`.
    pub fn hazard(self, h: crate::Hazard) -> Self {
        self.push(&crate::comps::entity_effect::HAZARDS, h.0)
    }
    /// `hazard` for `level` only, as it is: at that level the others don't count.
    pub fn hazard_at(self, level: u32, mut h: crate::Hazard) -> Self {
        h.0.level = level.max(1);
        self.push(&crate::comps::entity_effect::HAZARDS, h.0)
    }
    /// A modifier its entity has while it is on (`Modifier::percent(&attribute::MOVEMENT_SPEED,
    /// 0.2)`), run once for every level (`set` gives the same at any level).
    pub fn modifier(self, m: crate::Modifier) -> Self {
        self.push(&crate::comps::entity_effect::MODIFIERS, m.0)
    }
    /// Every tick, once with every entity that has it and its level there: levitation of your
    /// own, a mana drain, saturation.
    pub fn on_tick(self, f: impl Fn(&crate::EffectTick) + Send + Sync + 'static) -> Self {
        self.set(&crate::comps::entity_effect::ON_TICK, crate::game::effect_tick_cb(f))
    }
    /// `modifier` for `level` only, as it is: at that level the others of its attribute don't
    /// count (mining fatigue's steep steps).
    pub fn modifier_at(self, level: u32, mut m: crate::Modifier) -> Self {
        m.0.level = level.max(1);
        self.push(&crate::comps::entity_effect::MODIFIERS, m.0)
    }
    /// A status effect: its player sees it, by this icon (`textures/<icon>.png`, 18 pixels square
    /// like vanilla's), on the HUD and beside the inventory, named `effect.<namespace>.<name>`
    /// in lang.
    pub fn icon(self, texture: &str) -> Self {
        let tex = crate::asset("", texture);
        self.set_text(&crate::comps::entity_effect::ICON, &tex)
    }
    /// Swirls of this color (`0xRRGGBB`) around who has it, as vanilla's potion effects.
    pub fn color(self, rgb: u32) -> Self {
        self.set(&crate::comps::entity_effect::COLOR, rgb)
    }
    pub fn beneficial(self) -> Self {
        self.tag(abi::comp::ENTITY_EFFECT_BENEFICIAL)
    }
    pub fn harmful(self) -> Self {
        self.tag(abi::comp::ENTITY_EFFECT_HARMFUL)
    }
    /// It acts once when given, its hazards times the level (healing), and doesn't stay.
    pub fn instant(self) -> Self {
        self.tag(abi::comp::ENTITY_EFFECT_INSTANT)
    }
    /// What its player sees goes through this screen shader while it has it
    /// (`shaders/post/<name>.wgsl`, `PostPixel`): night vision, blindness.
    pub fn screen(self, shader: &crate::Shader) -> Self {
        self.environment(
            crate::Environment::new("core:screen")
                .blend(crate::environment::blend::MAX)
                .observer(crate::environment::observer::BODY)
                .priority(-400)
                .fade(0.0, 1.0)
                .post(shader, 1.0),
        )
    }
    /// Its entity isn't drawn for anyone else while it has it (invisibility).
    pub fn hides(self) -> Self {
        self.tag(abi::comp::ENTITY_EFFECT_HIDES)
    }
}

/// What falls from the sky, rain's numbers unless set.
pub struct PrecipitationBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(Precipitation => PrecipitationBuilder, precipitation;);

impl PrecipitationBuilder<'_> {
    /// `textures/<texture>.png`, a strip of drops like vanilla's rain.
    pub fn texture(self, texture: &str) -> Self {
        let tex = crate::asset("", texture);
        self.set_text(&crate::comps::precipitation::TEXTURE, &tex)
    }
    /// As a share of rain's (snow drifts at 0.1).
    pub fn speed(self, share: f32) -> Self {
        self.set(&crate::comps::precipitation::SPEED, share)
    }
    /// The share of the picture shown (rain 0.1).
    pub fn density(self, share: f32) -> Self {
        self.set(&crate::comps::precipitation::DENSITY, share)
    }
}

/// A weather (`Registry::weather`); the engine's are in `weathers`.
pub type Weather = Handle<kind::Weather>;
/// What falls from the sky in a biome (`Registry::precipitation`, `BiomeBuilder::precipitation`).
pub type Precipitation = Handle<kind::Precipitation>;

pub mod weathers {
    use super::Weather;
    pub static CLEAR: Weather = Weather::named("core:clear");
    pub static RAIN: Weather = Weather::named("core:rain");
    pub static THUNDER: Weather = Weather::named("core:thunder");
}

pub mod precipitations {
    use super::Precipitation;
    pub static RAIN: Precipitation = Precipitation::named("core:rain");
    pub static SNOW: Precipitation = Precipitation::named("core:snow");
}

/// A weather being registered: rain's rules unless told otherwise.
///
/// ```ignore
/// static ACID_RAIN: Weather = Weather::named("acid_rain");
/// r.weather(&ACID_RAIN).darkness(0.5).duration(2400, 6000).weight(1);
/// ```
/// `/weather mymod:acid_rain` sets it; the engine brings it now and then after clear weather.
pub struct WeatherBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(Weather => WeatherBuilder, weather;);

impl crate::Registry {
    pub fn weather<'a>(&'a mut self, weather: &'static Weather) -> WeatherBuilder<'a> {
        WeatherBuilder { reg: self, e: crate::entry::Entry::of(weather) }
    }
}

impl WeatherBuilder<'_> {
    /// Nothing falls (by default rain, snow, each biome's own does).
    pub fn no_falling(self) -> Self {
        self.tag(abi::comp::weather::DRY)
    }
    pub fn lightning(self) -> Self {
        self.tag(abi::comp::weather::LIGHTNING)
    }
    /// Things burn in the sun under it, a sandstorm (under rain they don't).
    pub fn no_shelter(self) -> Self {
        self.tag(abi::comp::weather::UNSHELTERED)
    }
    /// Share of daylight it takes away (rain 0.3125).
    pub fn darkness(self, share: f32) -> Self {
        self.set(&crate::comps::weather::DARKNESS, share)
    }
    /// How far the sky (rain 0.75), the fog (0.5) and the clouds (0.95) go gray.
    pub fn gray(self, sky: f32, fog: f32, clouds: f32) -> Self {
        self.set(&crate::comps::weather::SKY_GRAY, sky)
            .set(&crate::comps::weather::FOG_GRAY, fog)
            .set(&crate::comps::weather::CLOUD_GRAY, clouds)
    }
    /// A sound event played now and then around players under the open sky while it falls.
    pub fn sound(self, sound: &str) -> Self {
        let name = crate::full_name(sound);
        self.set_text(&crate::comps::weather::SOUND, &name)
    }
    /// Ticks it lasts, at least and at most.
    pub fn duration(self, min: u32, max: u32) -> Self {
        self.set(&crate::comps::weather::DURATION, [min, max])
    }
    /// How likely it comes after clear weather next to the others (rain 3, thunder 1); 0 only
    /// by command or code.
    pub fn weight(self, w: u32) -> Self {
        self.set(&crate::comps::weather::WEIGHT, w)
    }
}

/// A way to hold a body (`Registry::pose`): its box and eyes, its speed, how the model lies and
/// what it plays. `EntityState::pose` holds one; the engine's are in `poses`.
pub type Pose = Handle<kind::Pose>;

/// The engine's poses; it picks among the first four by itself as players move.
pub mod poses {
    use super::Pose;
    pub static STAND: Pose = Pose::named("core:stand");
    pub static CROUCH: Pose = Pose::named("core:crouch");
    pub static SWIM: Pose = Pose::named("core:swim");
    pub static CRAWL: Pose = Pose::named("core:crawl");
    pub static SLEEP: Pose = Pose::named("core:sleep");
}

/// A pose being registered: standing's box and speed unless told otherwise.
///
/// ```ignore
/// static SIT: Pose = Pose::named("sit");
/// r.pose(&SIT).height(0.7).eye(0.62).speed(0.0).action("sit");
/// w.set_pose(pet, Some(&SIT), None);
/// // Taken by the player: a slide while sprinting and holding crawl, above the engine's crawl.
/// r.pose(&SLIDE).height(0.4).eye(0.35).speed(1.2).key(&keys::CRAWL).when(Cond::sprinting()).priority(25);
/// ```
pub struct PoseBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(Pose => PoseBuilder, pose;);

impl crate::Registry {
    pub fn pose<'a>(&'a mut self, pose: &'static Pose) -> PoseBuilder<'a> {
        PoseBuilder { reg: self, e: crate::entry::Entry::of(pose) }
    }
}

impl PoseBuilder<'_> {
    /// An attribute modifier its holder gets while in it (`Modifier`).
    pub fn modifier(self, m: crate::Modifier) -> Self {
        self.push(&crate::component::MODIFIERS, m.0)
    }
    /// Box height as a share of standing (crouching is 1.5 / 1.8).
    pub fn height(self, share: f32) -> Self {
        self.set(&crate::comps::pose::HEIGHT, share)
    }
    /// Eye height as a share of standing.
    pub fn eye(self, share: f32) -> Self {
        self.set(&crate::comps::pose::EYE, share)
    }
    /// Walking speed as a share of normal, 0 to stay put.
    pub fn speed(self, share: f32) -> Self {
        self.set(&crate::comps::pose::SPEED, share)
    }
    /// How the model lies (`abi::pose_tilt`): along the look like swimming, face down, on its back.
    pub fn tilt(self, tilt: u32) -> Self {
        self.set(&crate::comps::pose::TILT, tilt)
    }
    /// The model's clip for it, by action name.
    pub fn action(self, name: &str) -> Self {
        self.set_text(&crate::comps::pose::ACTION, name)
    }
    /// The camera doesn't bob walking in it.
    pub fn no_bob(self) -> Self {
        self.tag(abi::comp::pose::NO_BOB)
    }
    /// Of the poses a player may take, the highest is taken (the engine's: stand 0, crouch 10,
    /// crawl 20, swim 30). Without `key` or `when` a pose is only ever held by a mod.
    pub fn priority(self, p: i32) -> Self {
        self.set(&crate::comps::pose::PRIORITY, p)
    }
    /// Taken while the player holds this action.
    pub fn key(self, key: &crate::Key) -> Self {
        let name = key.name().to_string();
        self.set_text(&crate::comps::pose::KEY, &name)
    }
    /// And only where this holds (`Cond::in_liquid`, `!Cond::flying()`); again for more.
    pub fn when(self, c: impl Into<crate::Cond>) -> Self {
        self.push(&crate::comps::pose::WHEN, c.into().0)
    }
    /// Where it has no room, the first of these that has and whose `when` holds; the last
    /// even without room.
    pub fn fallback(self, poses: &[&Pose]) -> Self {
        self.set(&crate::comps::pose::FALLBACK, poses.iter().map(|p| p.name().to_string()).collect())
    }
}

/// A game mode: what its players may do and what they are spared (`Registry::game_mode`); the
/// engine's are in `game_modes`.
pub type GameMode = Handle<kind::GameMode>;

/// The engine's game modes.
pub mod game_modes {
    use super::GameMode;
    pub static SURVIVAL: GameMode = GameMode::named("core:survival");
    pub static CREATIVE: GameMode = GameMode::named("core:creative");
    pub static SPECTATOR: GameMode = GameMode::named("core:spectator");
}

/// A game mode being registered: survival's rules unless told otherwise.
///
/// ```ignore
/// static ADVENTURE: GameMode = GameMode::named("adventure");
/// r.game_mode(&ADVENTURE).breaks(Breaking::None).no_placing();
/// ```
/// `/gamemode mymod:adventure` sets it, `{if:mymod:adventure}` in text asks for it.
pub struct GameModeBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(GameMode => GameModeBuilder, game_mode;);

/// How a game mode breaks blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Breaking {
    Normal,
    Instant,
    None,
}

/// How a game mode flies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flight {
    None,
    /// Jumping twice starts and stops it.
    Allowed,
    Always,
}

impl crate::Registry {
    pub fn game_mode<'a>(&'a mut self, mode: &'static GameMode) -> GameModeBuilder<'a> {
        GameModeBuilder { reg: self, e: crate::entry::Entry::of(mode) }
    }
}

impl GameModeBuilder<'_> {
    pub fn breaks(self, how: Breaking) -> Self {
        use abi::comp::game_mode as g;
        match how {
            Breaking::Instant => self.tag(g::INSTANT_BREAK),
            Breaking::None => self.tag(g::NO_BREAK),
            Breaking::Normal => self,
        }
    }
    pub fn flight(self, how: Flight) -> Self {
        use abi::comp::game_mode as g;
        match how {
            Flight::Allowed => self.modifier(crate::Modifier::add(&crate::attribute::CAN_FLY, 1.0)),
            Flight::Always => self.tag(g::ALWAYS_FLY),
            Flight::None => self,
        }
    }
    pub fn no_placing(self) -> Self {
        self.tag(abi::comp::game_mode::NO_PLACE)
    }
    /// Nothing gets used up: placed blocks, poured buckets, wear. Picking a block gives a stack.
    pub fn no_costs(self) -> Self {
        self.tag(abi::comp::game_mode::NO_COSTS)
    }
    /// No damage, hunger or air (`attribute::INVULNERABLE` 1).
    pub fn invulnerable(self) -> Self {
        self.modifier(crate::Modifier::set(&crate::attribute::INVULNERABLE, 1.0))
    }
    /// Through blocks.
    pub fn noclip(self) -> Self {
        self.tag(abi::comp::game_mode::NOCLIP)
    }
    /// Unnoticed by mobs, spawning and other players, and not a body to hit.
    pub fn hidden(self) -> Self {
        self.tag(abi::comp::game_mode::HIDDEN)
    }
    /// It can't use, hit, pick up, drop or hold anything.
    pub fn no_interacting(self) -> Self {
        self.tag(abi::comp::game_mode::NO_INTERACT)
    }
    /// The hotbar and bars don't show.
    pub fn no_hud(self) -> Self {
        self.tag(abi::comp::game_mode::NO_HUD)
    }
    /// The inventory key opens the creative inventory.
    pub fn creative_inventory(self) -> Self {
        self.tag(abi::comp::game_mode::CREATIVE_INVENTORY)
    }
    /// An attribute modifier its players have while in it (longer reach, `visibility` 0).
    pub fn modifier(self, m: crate::Modifier) -> Self {
        self.push(&crate::comps::game_mode::MODIFIERS, m.0)
    }
}

/// The engine's damage causes.
pub mod damage_causes {
    use super::DamageCause;
    pub static GENERIC: DamageCause = DamageCause::named("core:generic");
    pub static FALL: DamageCause = DamageCause::named("core:fall");
    pub static ATTACK: DamageCause = DamageCause::named("core:attack");
    pub static FIRE: DamageCause = DamageCause::named("core:fire");
    pub static STARVE: DamageCause = DamageCause::named("core:starve");
    pub static DROWN: DamageCause = DamageCause::named("core:drown");
    pub static VOID: DamageCause = DamageCause::named("core:void");
    pub static LAVA: DamageCause = DamageCause::named("core:lava");
    pub static CRUSH: DamageCause = DamageCause::named("core:crush");
    pub static SUFFOCATE: DamageCause = DamageCause::named("core:suffocate");
    pub static EXPLOSION: DamageCause = DamageCause::named("core:explosion");
    pub static HEAL: DamageCause = DamageCause::named("core:heal");
}

/// `r.damage_cause(&RADIATION).bypasses_armor().death_message("death.mymod.radiation")`.
pub struct DamageCauseBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(DamageCause => DamageCauseBuilder, damage_cause;);

impl crate::Registry {
    pub fn damage_cause<'a>(&'a mut self, cause: &'static DamageCause) -> DamageCauseBuilder<'a> {
        DamageCauseBuilder { reg: self, e: crate::entry::Entry::of(cause) }
    }
}

impl DamageCauseBuilder<'_> {
    /// Armor doesn't soften it.
    pub fn bypasses_armor(self) -> Self {
        self.tag(abi::comp::damage_cause::BYPASSES_ARMOR)
    }
    /// Held items cannot block it.
    pub fn bypasses_blocking(self) -> Self {
        self.tag(abi::comp::damage_cause::BYPASSES_BLOCKING)
    }
    /// Hurts invulnerable entities and players outside survival too.
    pub fn pierces_invulnerable(self) -> Self {
        self.tag(abi::comp::damage_cause::PIERCES_INVULNERABLE)
    }
    /// Lands in full even right after another hit.
    pub fn ignores_cooldown(self) -> Self {
        self.tag(abi::comp::damage_cause::IGNORES_COOLDOWN)
    }
    /// What dies of it drops cooked food.
    pub fn burning(self) -> Self {
        self.tag(abi::comp::damage_cause::BURNING)
    }
    pub fn no_knockback(self) -> Self {
        self.tag(abi::comp::damage_cause::NO_KNOCKBACK)
    }
    /// No hurt sound.
    pub fn silent(self) -> Self {
        self.tag(abi::comp::damage_cause::SILENT)
    }
    /// Never kills: it leaves half a heart (poison).
    pub fn not_lethal(self) -> Self {
        self.tag(abi::comp::damage_cause::NOT_LETHAL)
    }
    /// Translation key said when a player dies of it, `{0}` their name.
    pub fn death_message(self, key: &str) -> Self {
        self.set_text(&crate::comps::damage_cause::DEATH_MESSAGE, key)
    }
    /// The key when something did it, `{1}` that thing's name.
    pub fn death_message_by(self, key: &str) -> Self {
        self.set_text(&crate::comps::damage_cause::DEATH_MESSAGE_BY, key)
    }
}
pub type SoundCategory = Handle<kind::SoundCategory>;
pub type CreativeTab = Handle<kind::CreativeTab>;
pub type Question = Handle<kind::Question>;
pub type Chain = Handle<kind::Chain>;
pub type Step = Handle<kind::Step>;
pub type Phase = Handle<kind::Phase>;
pub type Attribute = Handle<kind::Attribute>;

/// What puts a block or an item in `tab`.
fn tab_tag(tab: &CreativeTab) -> String {
    format!("tab/{}", tab.name())
}

/// A creative tab being registered (`Registry::creative_tab`).
pub struct CreativeTabBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(CreativeTab => CreativeTabBuilder, creative_tab_entry;);

/// The creative tab every entity's spawn egg goes to.
pub static SPAWN_EGGS: CreativeTab = CreativeTab::named("core:spawn_eggs");
pub type RecipeKind = Handle<kind::RecipeKind>;
pub type LootTable = Handle<kind::LootTable>;
/// A predicate music, text, loot and spawning share (`Registry::condition`); the engine's are in
/// `conditions`.
pub type Condition = Handle<kind::Condition>;

pub mod conditions {
    use super::Condition;
    pub static DAY: Condition = Condition::named("core:day");
    pub static NIGHT: Condition = Condition::named("core:night");
    /// No sky over it.
    pub static UNDERGROUND: Condition = Condition::named("core:underground");
    pub static SURFACE: Condition = Condition::named("core:surface");
}
pub type Music = Handle<kind::Music>;
pub type EquipmentSlot = Handle<kind::EquipmentSlot>;
/// The second hand, in the player's equipment: register it to have one, with any background.
pub static OFF_HAND: EquipmentSlot = EquipmentSlot::named(abi::OFF_HAND_SLOT);

/// Things that name a loot table: a block or entity kind (what it drops) or a table of its own.
pub trait LootSource {
    fn table_name(&self) -> &str;
}
impl LootSource for Block {
    fn table_name(&self) -> &str {
        self.name()
    }
}
impl LootSource for EntityKind {
    fn table_name(&self) -> &str {
        self.name()
    }
}
impl LootSource for LootTable {
    fn table_name(&self) -> &str {
        self.name()
    }
}

/// Every builder (each an entry of its kind) tags, sets values and edits lists the same way.
pub trait WithComponents: Sized {
    /// The kind it registers (`kind`): which components it takes.
    type Kind;
    #[doc(hidden)]
    fn pending(&mut self) -> &mut Vec<Pending>;

    fn tag(mut self, tag: &str) -> Self {
        self.pending().push(Pending::tag(tag));
        self
    }
    /// Perception from a dimension, weather, biome, sampled block/liquid, entity kind,
    /// active entity effect or equipped item. Other entry kinds do not emit sources.
    fn environment(self, layer: crate::Environment) -> Self {
        crate::environment::attach(self, layer)
    }
    /// Takes a tag away (another mod's, in an edit).
    fn untag(mut self, tag: &str) -> Self {
        self.pending()
            .push(Pending::tag(tag).as_op(abi::value_op::UNSET, ""));
        self
    }
    /// A component value (on every state of a block, and its item).
    fn set<T: Data, K: kind::On<Self::Kind>>(mut self, comp: &Component<T, K>, v: T) -> Self {
        self.pending().push(comp.raw_value(&v));
        self
    }
    /// Takes a component away (another mod's, in an edit).
    fn unset<T: Data, K: kind::On<Self::Kind>>(mut self, comp: &Component<T, K>) -> Self {
        self.pending()
            .push(Pending::tag(comp.name()).as_op(abi::value_op::UNSET, ""));
        self
    }
    /// A value written as text: plain text, or names the engine resolves (`BlockRef`,
    /// `BlockList`).
    fn set_text<T: c::TextValue, K: kind::On<Self::Kind>>(mut self, comp: &Component<T, K>, text: &str) -> Self {
        self.pending()
            .push(Pending::text_as(comp.name(), c::schema_of::<T>(), text));
        self
    }
    /// One more element at the end of a list.
    fn push<T: Data, K: kind::On<Self::Kind>>(mut self, list: &Component<Vec<T>, K>, item: T) -> Self {
        self.pending()
            .push(element(list, &item, abi::value_op::PUSH, ""));
        self
    }
    /// An element before the one `anchor` names: its `name` field (a text element's text), or
    /// `#index`; elements without a `name` field only by `#index`.
    fn insert_before<T: Data, K: kind::On<Self::Kind>>(mut self, list: &Component<Vec<T>, K>, anchor: &str, item: T) -> Self {
        self.pending()
            .push(element(list, &item, abi::value_op::INSERT_BEFORE, anchor));
        self
    }
    fn insert_after<T: Data, K: kind::On<Self::Kind>>(mut self, list: &Component<Vec<T>, K>, anchor: &str, item: T) -> Self {
        self.pending()
            .push(element(list, &item, abi::value_op::INSERT_AFTER, anchor));
        self
    }
    /// The element `anchor` names becomes `item`.
    fn replace_item<T: Data, K: kind::On<Self::Kind>>(mut self, list: &Component<Vec<T>, K>, anchor: &str, item: T) -> Self {
        self.pending()
            .push(element(list, &item, abi::value_op::REPLACE_ITEM, anchor));
        self
    }
    fn remove_item<T: Data + Default, K: kind::On<Self::Kind>>(mut self, list: &Component<Vec<T>, K>, anchor: &str) -> Self {
        self.pending()
            .push(element(list, &T::default(), abi::value_op::REMOVE_ITEM, anchor));
        self
    }
}

/// A list operation's value: one element, with the element's schema.
fn element<T: Data, K>(list: &Component<Vec<T>, K>, item: &T, op: u32, anchor: &str) -> Pending {
    Pending::value(list.name(), c::schema_of::<T>(), abi::data::bytes_of(item)).as_op(op, anchor)
}

/// What carries items: entity kinds (block entities among them) and item stacks.
pub trait Holds: WithComponents {
    /// Each one carries an inventory of `slots` (`w.inventory(holder)`): an entity's, a block
    /// entity's (`w.inventory(pos)`, a menu's `Slots::holder()` groups), a stack's (a backpack,
    /// `w.inventory(Holder::held(player, slot))`, saved in the stack; stacks holding something
    /// only stack with stacks holding the same).
    fn inventory(self, slots: u32) -> Self {
        self.set(&crate::component::INVENTORY_SLOTS, slots)
    }
}
impl Holds for ItemBuilder<'_> {}
impl Holds for crate::EntityBuilder<'_> {}

/// Light that moves with an entity: an item's (held, worn, shown or dropped), an entity kind's or an
/// entity effect's, drawn by clients alone. Blocks set `light` per state; an item that places a
/// block gives that block's light. One entity's own changes while it lives: its kind attaches
/// `LIGHT` (starting as the kind's) and `w.set_component(e, &LIGHT, [r, g, b])` sets it.
pub trait Glows: WithComponents {
    /// White light. 15 is full strength; more is allowed and reaches further.
    fn light(self, level: u8) -> Self {
        self.light_rgb(level, level, level)
    }
    /// Colored light, per channel.
    fn light_rgb(self, r: u8, g: u8, b: u8) -> Self {
        self.set(&c::LIGHT, [r, g, b])
    }
}
impl Glows for ItemBuilder<'_> {}
impl Glows for crate::EntityBuilder<'_> {}
impl Glows for EntityEffectBuilder<'_> {}

/// On some states only, on top of the block's.
impl WithComponents for State<'_> {
    type Kind = kind::Block;
    fn pending(&mut self) -> &mut Vec<Pending> {
        self.ops
    }
}

/// A component operation waiting to be sent with its entry. Text is kept as a `String` until then.
#[derive(Clone)]
pub struct Pending {
    pub(crate) name: String,
    schema: &'static str,
    pub(crate) bytes: Vec<u8>,
    text: Option<String>,
    /// `abi::value_op`, and the list element it is about.
    pub(crate) op: u32,
    pub(crate) anchor: String,
}

impl Pending {
    pub(crate) fn value(name: &str, schema: &'static str, bytes: Vec<u8>) -> Pending {
        Pending { name: name.to_string(), schema, bytes, text: None, op: abi::value_op::SET, anchor: String::new() }
    }
    /// The same value as another operation (`abi::value_op`) on `anchor`.
    pub(crate) fn as_op(self, op: u32, anchor: &str) -> Pending {
        Pending { op, anchor: anchor.to_string(), ..self }
    }
    pub(crate) fn text(name: &str, text: &str) -> Pending {
        Pending::text_as(name, "str", text)
    }
    pub(crate) fn text_as(name: &str, schema: &'static str, text: &str) -> Pending {
        // Block names without a namespace are this mod's, like everywhere else.
        let text = match schema {
            "block" | "blocks" | "item" => text
                .split_whitespace()
                .map(|e| match e.rsplit_once(':') {
                    // `oak_log:4`: the weight stays, the name gets the namespace.
                    Some((n, w)) if w.parse::<u32>().is_ok() => format!("{}:{w}", crate::full_name(n)),
                    _ => crate::full_name(e),
                })
                .collect::<Vec<_>>()
                .join(" "),
            "match" => crate::qualify_matcher(crate::mod_id(), text),
            _ => text.to_string(),
        };
        Pending { name: name.to_string(), schema, bytes: Vec::new(), text: Some(text), op: abi::value_op::SET, anchor: String::new() }
    }
    pub(crate) fn tag(name: &str) -> Pending {
        Pending::value(name, "", Vec::new())
    }
    fn is_tag(&self) -> bool {
        self.schema.is_empty() && self.text.is_none() && self.bytes.is_empty()
    }
}

/// `CompValue`s pointing into `list`; text's packed bytes live in `scratch`.
pub(crate) fn raw_values(list: &[Pending], scratch: &mut Vec<Vec<u8>>) -> Vec<abi::CompValue> {
    *scratch = list
        .iter()
        .filter_map(|p| p.text.as_ref().map(abi::data::bytes_of))
        .collect();
    let mut texts = scratch.iter();
    list.iter()
        .map(|p| {
            let bytes = if p.text.is_some() { texts.next().unwrap() } else { &p.bytes };
            abi::CompValue { name: Str::new(&p.name), schema: Str::new(p.schema), value: Slice::new(bytes) }
        })
        .collect()
}

pub struct Registry {
    pub(crate) raw: *mut abi::Registrar,
    pub(crate) ns: &'static str,
    /// Names given to what has none of its own (recipes, screen parts), each once.
    pub(crate) derived: Vec<String>,
}

impl Registry {
    pub fn namespace(&self) -> &'static str {
        self.ns
    }

    /// Whether mod `id` is loaded, to register what goes with it only then (all mods are known
    /// before any registers, whatever the order).
    pub fn has_mod(&self, id: &str) -> bool {
        unsafe { (crate::host().mod_loaded)(abi::Str::new(id)) != 0 }
    }

    /// `<this mod>:<path>`, numbered `/2`, `/3`.. when this mod gave it already.
    pub(crate) fn derive_name(&mut self, path: &str) -> String {
        let base = format!("{}:{path}", self.ns);
        let name = (1..)
            .map(|n| if n == 1 { base.clone() } else { format!("{base}/{n}") })
            .find(|n| !self.derived.contains(n))
            .unwrap_or_default();
        self.derived.push(name.clone());
        name
    }

    /// Changes only what the builder sets of something registered already, another mod's or
    /// this one's; the rest stays. Blocks (`r.edit(&base::CACTUS).light(5)`: on every state,
    /// `state_where` on some), items, biomes, dimensions, menus, ores (`IRON_ORE.ore()`), liquids
    /// (`WATER.liquid()`), terrains (`OVERWORLD.terrain()`), loot tables (`ZOMBIE.loot()`), entity
    /// kinds (`r.edit(&PLAYER).attach(&MANA)`) and the rest by their handles.
    pub fn edit<K: kind::Builds>(&mut self, handle: &'static Handle<K>) -> K::Builder<'_> {
        K::builder(self, handle, abi::entry_mode::EDIT)
    }

    /// `edit` of everything of a kind `pattern` names, any mod's: `#tag` (a block's states by
    /// their own tags; a component too, `#fuel` anything that burns), a name with `*` in it, `name[prop=value]` (those states of a block):
    /// `r.edit_pattern::<kind::Block>("#planks/*").set(&FUEL, 300)`. Patterns apply after edits by
    /// name. For blocks and items a pattern matching nothing is a load error (a `#tag` one when
    /// no such tag exists), for other kinds it is fine.
    pub fn edit_pattern<K: kind::Builds + 'static>(&mut self, pattern: &str) -> K::Builder<'_> {
        self.edit(Handle::<K>::generated(pattern.to_string()))
    }

    /// Registers `handle` from scratch over what other mods registered as it, nothing of theirs
    /// staying: `r.replace(&base::TORCH).texture("blue_torch")`. Of several mods replacing one
    /// thing, the last in load order wins. A block's item is replaced too (`.no_item()` leaves
    /// theirs), its loot table only with `.drops*`, its block entity only with `.entity`.
    pub fn replace<K: kind::Builds>(&mut self, handle: &'static Handle<K>) -> K::Builder<'_> {
        K::builder(self, handle, abi::entry_mode::REPLACE)
    }

    /// As if nobody had registered `handle` (a block takes its item along, not its loot table or
    /// block entity).
    pub fn remove<K: kind::Kind>(&mut self, handle: &Handle<K>) {
        let mut e = crate::entry::Entry::of(handle);
        e.mode = abi::entry_mode::REMOVE;
        e.send(self);
        if K::KIND == abi::entry_kind::BLOCK {
            let mut item = crate::entry::Entry::new(abi::entry_kind::ITEM, handle.name().to_string());
            item.mode = abi::entry_mode::REMOVE;
            item.send(self);
        }
    }

    /// Registers a block and, unless `.no_item()` is called, an item that places it (and carries
    /// the block's components). Texture defaults to `block/<name>` for all faces.
    pub fn block<'a>(&'a mut self, block: &'static Block) -> BlockBuilder<'a> {
        BlockBuilder {
            reg: self,
            e: crate::entry::Entry::of(block),
            handle: block,
            turns: None,
            axis: false,
            props: Vec::new(),
            looks: None,
            keyed: Vec::new(),
            drops: None,
            item: true,
            item_texture: String::new(),
            picked: faces::ALL,
            facing: None,
            sides: None,
            signal: None,
            placement: None,
            merge: None,
            entity: None,
            neighbor_fns: Vec::new(),
        }
    }

    /// Texture defaults to `item/<name>`.
    pub fn item<'a>(&'a mut self, item: &'static Item) -> ItemBuilder<'a> {
        ItemBuilder { reg: self, e: crate::entry::Entry::of(item) }
    }

    /// The item placing `block`, named like it, registered as `mode` says (`entry_mode`): the
    /// engine gives it the block's components, and without `texture` it shows the block's model.
    fn block_item(&mut self, block: &str, texture: String, mode: u32) -> ItemBuilder<'_> {
        let mut e = crate::entry::Entry::new(abi::entry_kind::ITEM, block.to_string());
        e.mode = mode;
        if mode == abi::entry_mode::REMOVE {
            return ItemBuilder { reg: self, e };
        }
        ItemBuilder { reg: self, e }
            .set_text(&crate::comps::item::BLOCK, block)
            .set_text(&crate::comps::item::LAYERS, &texture)
    }

    pub fn biome<'a>(&'a mut self, biome: &'static Biome) -> BiomeBuilder<'a> {
        BiomeBuilder { reg: self, e: crate::entry::Entry::of(biome) }
    }

    /// Creative inventory tab named `tab.<ns>.<name>` in the language files, shown with `icon`.
    /// Blocks and items join with `.tab(&TAB)`; untabbed items land in a tab named after the mod.
    /// Tabs come in mod load order, then registration order.
    pub fn creative_tab(&mut self, tab: &'static CreativeTab, icon: &Item) -> CreativeTabBuilder<'_> {
        self.creative_tab_entry(tab)
            .set_text(&crate::comps::creative_tab::ICON, icon.name())
    }

    fn creative_tab_entry(&mut self, tab: &'static CreativeTab) -> CreativeTabBuilder<'_> {
        CreativeTabBuilder { reg: self, e: crate::entry::Entry::of(tab) }
    }

    /// A fixed shape of blocks, drawn in layers of rows (`layer` + `key`) or filled with boxes
    /// (`fill`), checked when the game loads. A file `structures/<name>.json` of the mod is one
    /// by itself; this adds to it.
    pub fn template(&mut self, template: &'static Template) -> TemplateBuilder<'_> {
        TemplateBuilder { reg: self, e: crate::entry::Entry::of(template), layers: Vec::new(), keys: Vec::new(), fills: Vec::new() }
    }

    /// A mod's own data, for itself or an API other mods use: tags and components say what it is,
    /// and other mods `edit`, `replace` and `remove` it like anything registered. Read once the
    /// game is loaded: `entries_with(entry_kind::RECORD, "structgen:structure")` lists them,
    /// `CHANCE.on_record(id)` reads one.
    ///
    /// ```ignore
    /// static HUT: Record = Record::named("hut");
    /// r.record(&HUT).tag("structgen:structure").set(&CHANCE, 0.3).set(&SPACING, 256);
    /// // Another mod's:
    /// static THEIR_HUT: Record = Record::named("othermod:hut");
    /// r.edit(&THEIR_HUT).set(&CHANCE, 0.05);
    /// ```
    pub fn record(&mut self, record: &'static Record) -> RecordBuilder<'_> {
        RecordBuilder { reg: self, e: crate::entry::Entry::of(record) }
    }

    /// The blocks templates' slots take where a feature uses it (`FeatureBuilder::palette`):
    /// `r.palette(&SPRUCE).slot("wall", "base:spruce_planks")`.
    pub fn palette(&mut self, palette: &'static Palette) -> PaletteBuilder<'_> {
        PaletteBuilder { reg: self, e: crate::entry::Entry::of(palette) }
    }

    pub fn dimension<'a>(&'a mut self, dimension: &'static Dimension) -> DimensionBuilder<'a> {
        DimensionBuilder { reg: self, e: crate::entry::Entry::of(dimension) }
    }

    /// Sets a component on blocks and items of any mod, by reference: `r.set("base:coal", &FUEL,
    /// 1600)`, `r.set("#planks/*", &FUEL, 300)`, `r.set("base:furnace[lit=true]", &HOT, ..)`. An
    /// edit by pattern of both kinds (`edit_pattern`), so after every edit by name; among patterns
    /// the last mod in load order wins, and naming nothing of either is a load error.
    pub fn set<T: Value>(&mut self, what: &str, comp: &Component<T>, v: T) {
        self.edit_matching(what, comp.raw_value(&v));
    }

    /// Adds `tag` to blocks and items of any mod, by reference: `r.tag("ingots/tin",
    /// &["othermod:tin_ingot"])`, `r.tag("plant_soil", &["#dirt"])`.
    pub fn tag(&mut self, tag: &str, names: &[&str]) {
        for n in names {
            self.edit_matching(n, Pending::tag(tag));
        }
    }

    /// One operation on the blocks and items `what` names (`r.set`).
    fn edit_matching(&mut self, what: &str, op: Pending) {
        let name = qualify(self.ns, what);
        // A plain name too is a pattern (all states of the block, the item), so that one kind
        // having nothing of that name is fine.
        let pattern = if name.starts_with('#') || name.contains('*') || name.contains('[') { name } else { format!("{name}[]") };
        let states = pattern
            .split_once('[')
            .is_some_and(|(_, k)| !k.trim_end_matches(']').trim().is_empty());
        for kind in [abi::entry_kind::BLOCK, abi::entry_kind::ITEM] {
            if kind == abi::entry_kind::ITEM && states {
                continue;
            }
            let mut e = crate::entry::Entry::new(kind, pattern.clone());
            e.mode = abi::entry_mode::EDIT;
            e.ops.push(op.clone());
            e.send(self);
        }
    }

    /// What an item or an entity of any mod looks like. From a file, `models/<file>.geo.json` with
    /// its `textures/<file>.png`, shown everywhere the item shows, or from code:
    /// `r.set_model("base:apple", "foodmod:apple")`, `r.set_model(&PLAYER, humanoid(false))`.
    /// A whole tag at once takes `{name}`; items without a file stay as they were:
    /// `r.set_model("#food", "foodmod:food/{name}")`. An entity kind's is an edit of it.
    pub fn set_model(&mut self, what: impl ModelTarget, model: impl ModelSource) {
        model.set_on(self, &what);
    }

    /// The clips of an item or an entity of any mod: `r.set_animations(&PLAYER, "mymod:player")`
    /// plays `animations/player.animation.json` of `mymod` on every player.
    pub fn set_animations(&mut self, what: impl ModelTarget, file: &str) {
        let file = crate::asset("", file);
        self.model_text(&what, &c::ANIMATIONS, &file);
    }

    /// A text component of what `set_model` reaches: an edit of an entity kind, or blocks and items.
    fn model_text(&mut self, what: &dyn ModelTarget, comp: &Component<c::Text>, text: &str) {
        if what.is_entity() {
            drop(self.edit_entity(what.model_name()).set_text(comp, text));
        } else {
            self.edit_matching(what.model_name(), Pending::text(comp.name(), text));
        }
    }

    /// Every entity of `kind` has `model` on its bone `bone`, drawn in that model's own pixels, with
    /// the same origin as the kind's. What plays on the entity plays on it too, by the names of
    /// clips and bones: `r.bone_model(&PLAYER, "tailmod:tail", "body")`.
    pub fn bone_model(&mut self, kind: &crate::EntityKind, model: &str, bone: &str) {
        let model = crate::asset("", model);
        let place = abi::BoneModel { model, bone: bone.to_string() };
        drop(
            self.edit_entity(kind.name())
                .push(&crate::comps::entity::BONE_MODELS, place),
        );
    }

    /// `bone` of `model` moves by itself. A bone called `hanging` or `hanging_<anything>` does without this.
    pub fn bone_motion(&mut self, model: &str, bone: &str, motion: BoneMotion) {
        use crate::comps::bone_motion as m;
        let name = format!("{}/{bone}", crate::asset("", model));
        let mut e = crate::entry::Entry::new(abi::entry_kind::BONE_MOTION, name);
        e.ops = vec![m::GRAVITY.raw_value(&motion.gravity), m::SPRING.raw_value(&motion.spring), m::DRAG.raw_value(&motion.drag)];
        e.send(self);
    }
}

/// How a bone moves by itself, on top of its clips. Numbers don't depend on the bone's size.
#[derive(Clone, Copy, Debug)]
pub struct BoneMotion {
    /// Pull toward hanging straight down. The bigger, the faster it swings.
    pub gravity: f32,
    /// Pull back to where the clips have it.
    pub spring: f32,
    /// How fast the swing dies down.
    pub drag: f32,
}

impl BoneMotion {
    /// On a chain: a lantern. What a bone called `hanging` does.
    pub const HANGING: BoneMotion = BoneMotion { gravity: 40.0, spring: 0.0, drag: 2.5 };
    /// Keeps its shape, droops a little and lags behind: a tail.
    pub const SPRINGY: BoneMotion = BoneMotion { gravity: 6.0, spring: 30.0, drag: 5.0 };
}

/// What `Registry::set_model` takes: a file's name, or a model made in code.
pub trait ModelSource {
    fn set_on(self, r: &mut Registry, what: &dyn ModelTarget);
}

impl ModelSource for &str {
    fn set_on(self, r: &mut Registry, what: &dyn ModelTarget) {
        let file = crate::asset("", self);
        r.model_text(what, &c::MODEL, &file);
    }
}

/// What has a model: a name or `#tag` of blocks and items as text, or the handle of an item, a
/// block or an entity kind.
pub trait ModelTarget {
    #[doc(hidden)]
    fn model_name(&self) -> &str;
    #[doc(hidden)]
    fn is_entity(&self) -> bool {
        false
    }
}

impl ModelTarget for &str {
    fn model_name(&self) -> &str {
        self
    }
}

impl ModelTarget for &Item {
    fn model_name(&self) -> &str {
        self.name()
    }
}

impl ModelTarget for &Block {
    fn model_name(&self) -> &str {
        self.name()
    }
}

impl ModelTarget for &crate::EntityKind {
    fn model_name(&self) -> &str {
        self.name()
    }
    fn is_entity(&self) -> bool {
        true
    }
}

/// The actions the engine plays by itself, by the names clips have in files. Any other name is a
/// mod's own.
pub mod action {
    pub const IDLE: &str = "idle";
    pub const WALK: &str = "walk";
    pub const SNEAK: &str = "sneak";
    pub const SWIM: &str = "swim";
    pub const CRAWL: &str = "crawl";
    pub const DEATH: &str = "death";
    /// Sitting on a mount, instead of walking.
    pub const RIDE: &str = "ride";
    pub const HOLD: &str = "hold";
    pub const HOLD_TOOL: &str = "hold_tool";
    pub const HOLD_EMPTY: &str = "hold_empty";
    pub const SWING: &str = "swing";
    pub const SWING_HELD: &str = "swing_held";
    pub const USE: &str = "use";
    pub const EQUIP: &str = "equip";
    /// An item's model lying on the ground, and in a slot.
    pub const GROUND: &str = "ground";
    pub const GUI: &str = "gui";
}

/// The last box a builder added: `turned` and `tilted` change it.
fn last_box(ops: &mut [Pending], f: impl FnOnce(&mut abi::ModelBox)) {
    let Some(p) = ops
        .iter_mut()
        .rfind(|p| p.name == abi::comp::block::BOXES && p.op == abi::value_op::PUSH)
    else {
        return;
    };
    if let Some(mut b) = abi::data::from_bytes::<abi::ModelBox>(&p.bytes) {
        f(&mut b);
        p.bytes = abi::data::bytes_of(&b);
    }
}

/// Per-state settings, used directly and through `BlockBuilder::states`.
pub struct State<'a> {
    ops: &'a mut Vec<Pending>,
    /// The faces `layer`, `tint` and `tint_kind` go on (`faces`).
    picked: u8,
    /// Its block's faces, which a state's own start from.
    block_faces: &'a [String; 6],
}

impl<'a> State<'a> {
    fn new(ops: &'a mut Vec<Pending>, block_faces: &'a [String; 6]) -> State<'a> {
        State { ops, picked: faces::ALL, block_faces }
    }
    fn picked(&mut self) -> &mut u8 {
        &mut self.picked
    }
    fn own_faces(&self) -> [String; 6] {
        self.block_faces.clone()
    }
}

/// A block's faces as bits, -X, +X, -Y, +Y, -Z, +Z (`abi::FACE_DIRS`).
mod faces {
    pub const WEST: u8 = 1;
    pub const EAST: u8 = 2;
    pub const BOTTOM: u8 = 4;
    pub const TOP: u8 = 8;
    pub const NORTH: u8 = 16;
    pub const SOUTH: u8 = 32;
    pub const SIDES: u8 = WEST | EAST | NORTH | SOUTH;
    pub const ALL: u8 = 63;
}

/// `face`'s (`LAYERS` text) top layer tinted, by a color or a kind or both.
fn top_layer(face: &mut String, tint: Option<u32>, kind: Option<&str>) {
    let mut layers: Vec<abi::Layer> = abi::layers(face).collect();
    if let Some(top) = layers.last_mut() {
        top.tint = tint.or(top.tint);
        top.kind = kind.or(top.kind);
    }
    *face = abi::layers_text(layers);
}

macro_rules! state_setters {
    () => {
        /// Seconds to mine by hand (`hardness`). Every setter here writes a component; the shared
        /// ones (hardness, light, friction, drag, support, tint) other mods also set with `r.set`.
        pub fn hardness(self, h: f32) -> Self {
            self.set(&c::HARDNESS, h)
        }
        pub fn unbreakable(self) -> Self {
            self.hardness(-1.0)
        }
        /// White light. 15 is full strength; more is allowed and reaches further.
        pub fn light(self, level: u8) -> Self {
            self.light_rgb(level, level, level)
        }
        pub fn light_rgb(self, r: u8, g: u8, b: u8) -> Self {
            self.set(&c::LIGHT, [r, g, b])
        }
        /// Per-channel absorption of passing light (stained glass).
        pub fn light_filter(self, r: u8, g: u8, b: u8) -> Self {
            self.set(&bc::LIGHT_FILTER, [r, g, b])
        }
        /// Its shape from model file `name` (`models/block/<path>.json` of that mod) instead of
        /// the one named like the block: several blocks, any mod's, on one model (#88), or a state
        /// on a model of its own (a drying rack with something on it).
        pub fn model_file(self, name: &str) -> Self {
            self.set_text(&bc::MODEL_FILE, &crate::full_name(name))
        }
        /// The texture put where the model file says `#name`: one model, each block its own
        /// pictures (a hollow log's inside, stripped). A state's go over its block's. Relative
        /// names are under `block/`.
        pub fn model_texture(self, name: &str, texture: &str) -> Self {
            let t = abi::ModelTexture { name: name.trim_start_matches('#').to_string(), texture: crate::asset("block", texture) };
            self.push(&bc::MODEL_TEXTURES, t)
        }
        /// Every face shows this picture (relative names are under `block/`; without any:
        /// `block/<name>`), and every face is picked: what `layer`, `tint` and `tint_kind` go on.
        /// The other presets pick some faces the same way, the last one said wins:
        /// `.texture("dirt").top("grass_top").tint_kind(&tints::GRASS).sides("dirt").layer("grass_side_overlay").tint_kind(&tints::GRASS)`.
        pub fn texture(self, t: &str) -> Self {
            self.pick(faces::ALL, t)
        }
        pub fn top(self, t: &str) -> Self {
            self.pick(faces::TOP, t)
        }
        pub fn bottom(self, t: &str) -> Self {
            self.pick(faces::BOTTOM, t)
        }
        /// Top and bottom: a log's rings, a furnace's top.
        pub fn ends(self, t: &str) -> Self {
            self.pick(faces::TOP | faces::BOTTOM, t)
        }
        /// The four around.
        pub fn sides(self, t: &str) -> Self {
            self.pick(faces::SIDES, t)
        }
        /// Toward the north, where `.facing(..)` turns its front: a furnace's.
        pub fn front(self, t: &str) -> Self {
            self.pick(faces::NORTH, t)
        }
        pub fn back(self, t: &str) -> Self {
            self.pick(faces::SOUTH, t)
        }
        pub fn north(self, t: &str) -> Self {
            self.pick(faces::NORTH, t)
        }
        pub fn south(self, t: &str) -> Self {
            self.pick(faces::SOUTH, t)
        }
        pub fn west(self, t: &str) -> Self {
            self.pick(faces::WEST, t)
        }
        pub fn east(self, t: &str) -> Self {
            self.pick(faces::EAST, t)
        }
        /// One more picture on top of the picked faces' (relative names under `block/`): the
        /// green over a grass block's sides, an ore's stones over stone. Each layer is drawn by
        /// itself over the one under it, with its own animation and connected textures. Like
        /// `tint` and `tint_kind` it goes on the faces with pictures of their own of the model
        /// boxes made so far too, toward the picked sides.
        pub fn layer(self, t: &str) -> Self {
            let t = crate::asset("block", t);
            self.change_layers(|face| {
                if !face.is_empty() {
                    face.push(' ');
                }
                face.push_str(&t);
            })
        }
        /// The picked faces' top layer multiplied by `0xRRGGBB`: one picture, many colors
        /// (wool). A tinted copy of the picture is made as the game loads, every frame of it and
        /// its connected tiles too, so it costs nothing to draw.
        pub fn tint(self, color: u32) -> Self {
            self.change_layers(|face| top_layer(face, Some(color & 0xFF_FFFF), None))
        }
        /// The picked faces' top layer colored by the biome the block stands in (`tints::GRASS`,
        /// `FOLIAGE`, `WATER` or a mod's own `TintKind`), and an item of it by the kind's
        /// default. Each layer its own, any kind and in any order.
        pub fn tint_kind(self, kind: &TintKind) -> Self {
            self.change_layers(|face| top_layer(face, None, Some(kind.name())))
        }
        fn pick(mut self, which: u8, t: &str) -> Self {
            let t = crate::asset("block", t);
            *self.picked() = which;
            self.change_faces(|face| *face = t.clone())
        }
        /// `change_faces`, and the faces toward the picked sides of the model boxes made so far
        /// that have pictures of their own (a tall plant's panels).
        fn change_layers(mut self, mut f: impl FnMut(&mut String)) -> Self {
            let which = *self.picked();
            for p in self
                .pending()
                .iter_mut()
                .filter(|p| p.name == bc::BOXES.name() && p.op == abi::value_op::PUSH)
            {
                let Some(mut b) = abi::data::from_bytes::<abi::ModelBox>(&p.bytes) else { continue };
                for (i, face) in b.textures.iter_mut().enumerate() {
                    if which & 1 << i != 0 && !face.is_empty() {
                        f(face);
                    }
                }
                p.bytes = abi::data::bytes_of(&b);
            }
            self.change_faces(f)
        }
        /// Its faces as they are so far, the picked ones changed by `f`.
        fn change_faces(mut self, mut f: impl FnMut(&mut String)) -> Self {
            let which = *self.picked();
            let last = self
                .pending()
                .iter()
                .rposition(|p| p.name == bc::TEXTURES.name() && p.op == abi::value_op::SET);
            let mut faces: [String; 6] = last
                .and_then(|i| abi::data::from_bytes(&self.pending()[i].bytes))
                .unwrap_or_else(|| self.own_faces());
            for (i, face) in faces.iter_mut().enumerate() {
                if which & 1 << i != 0 {
                    f(face);
                }
            }
            match last {
                Some(i) => self.pending()[i].bytes = abi::data::bytes_of(&faces),
                None => {
                    let v = bc::TEXTURES.raw_value(&faces);
                    self.pending().push(v);
                }
            }
            self
        }
        /// Quarter turns of each face's picture, -X, +X, -Y, +Y, -Z, +Z: a log lying on its side
        /// turns its bark along it. For a cube; boxes have `turned`.
        pub fn face_turns(self, turns: [u8; 6]) -> Self {
            self.set(&bc::TURNS, turns.map(|t| t % 4))
        }
        /// Not opaque, rendered alpha-tested, lets light through (glass, leaves).
        pub fn cutout(self) -> Self {
            self.set(&bc::LAYER, RenderLayer::Cutout as u32)
        }
        /// Hide faces between neighbors of the same block (glass panes of a window).
        pub fn cull_same(self) -> Self {
            self.tag(abi::comp::block::CULL_SAME)
        }
        /// Light it takes away, 0..15 (leaves 1, ice 2), when what it looks like isn't enough to
        /// go by: otherwise 15 for what is opaque and 0 for the rest.
        pub fn light_opacity(self, o: u8) -> Self {
            self.set(&bc::LIGHT_OPACITY, o)
        }
        pub fn translucent(self) -> Self {
            self.set(&bc::LAYER, RenderLayer::Translucent as u32)
                .cull_same()
        }
        /// Needs a block `what` takes underneath (a block matcher: `#plant_soil`): placed only
        /// there, and breaks off when it goes.
        pub fn needs_below(self, what: &str) -> Self {
            self.needs(crate::Dir::Down, what)
        }
        /// Needs the neighbor that way to be one `what` takes (a block matcher), or to be solid
        /// if `what` is empty: a rule breaking it off without (`Rule::neighbor`), which also keeps
        /// it from being placed so. That way is of its own frame, turning with it.
        pub fn needs(self, side: crate::Dir, what: &str) -> Self {
            self.rule(
                crate::Rule::neighbor()
                    .when(!crate::Cond::side(side, what))
                    .breaks(),
            )
        }
        /// A rule of what happens to it by itself (`Rule`), of this state or every state.
        pub fn rule(self, r: crate::Rule) -> Self {
            self.push(&bc::RULES, r.0)
        }
        /// What `event` (`break`, `place`, `step`, `hit`, `land`, `sprint`) sounds and shows
        /// instead of its sound group's and the engine's pieces of it: a sound event and a
        /// particle kind, `""` for what it would be anyway, `"none"` for nothing.
        pub fn effect(self, event: &str, sound: &str, particle: &str) -> Self {
            self.push(&c::EFFECTS, crate::component::effect(event, sound, particle))
        }
        /// What it does to entities meeting it (`Hazard`), of this state or every state.
        pub fn hazard(self, h: crate::Hazard) -> Self {
            self.push(&bc::HAZARDS, h.0)
        }
        /// Draw its item as the sprite of this block face instead of the model.
        pub fn item_sprite(self, face: crate::Dir) -> Self {
            self.set(&bc::ITEM_FACE, face.face() as u8)
        }
        /// A plant: tagged `plantable`, crossed flat boxes, no collision. Placing a block into it is
        /// refused, as for vanilla's flowers and sugar cane; grass that gives way is `.replaceable()`.
        pub fn plant(self) -> Self {
            self.not_solid()
                .set(&bc::MODEL, BlockModel::Cross as u32)
                .cutout()
                .hardness(0.0)
                .tag("plantable")
        }
        /// `plant` two blocks tall (vanilla's tall grass, a sunflower): a cross showing `lower`
        /// in its cell and `upper` in the one above (relative names under `block/`), taking room
        /// in both. Every face is picked after, so `.tint_kind(..)` goes on both.
        pub fn tall_plant(self, lower: &str, upper: &str) -> Self {
            let mut b = self
                .plant()
                .cells(&[[0, 1, 0]])
                .hitbox([2.0, 0.0, 2.0], [14.0, 30.0, 14.0])
                .texture(lower)
                .top(upper);
            *b.picked() = faces::ALL;
            b
        }
        pub fn not_solid(self) -> Self {
            self.tag(abi::comp::block::NOT_SOLID)
        }
        pub fn replaceable(self) -> Self {
            self.tag(abi::comp::block::REPLACEABLE)
        }
        pub fn climbable(self) -> Self {
            self.tag(abi::comp::block::CLIMBABLE)
        }
        pub fn friction(self, f: f32) -> Self {
            self.set(&c::FRICTION, f)
        }
        pub fn drag(self, d: f32) -> Self {
            self.set(&c::DRAG, d)
        }
        /// How many times its loot is rolled when it breaks: 2 for two slabs in one block.
        pub fn loot_rolls(self, n: u32) -> Self {
            self.set(&c::LOOT_ROLLS, n)
        }
        /// Custom model made of boxes in 1/16 units, each face showing the state's texture of
        /// that face (`texture`, `textures`, `faces`). It collides and is selected box by box, a
        /// stair being one slab and a quarter, unless `hitbox` is set. A
        /// `models/block/<name>.json` (Blockbench's Java block model) does the same from a file.
        pub fn model_box(self, min: [f32; 3], max: [f32; 3]) -> Self {
            self.add_box(min, max, Default::default(), [abi::UV_FROM_BOX; 6])
        }
        /// `model_box` with its own textures and texel rectangles (u0, v0, u1, v1, 0..16) per face
        /// in -X, +X, -Y, +Y, -Z, +Z order, as a model file's element has; `UV_FROM_BOX` takes it
        /// from where the box is, `UV_NO_FACE` leaves a face out. An empty texture is the block's
        /// own on that side, with its layers and tint.
        pub fn model_box_faces(self, min: [f32; 3], max: [f32; 3], faces: [(&str, [f32; 4]); 6]) -> Self {
            let textures = std::array::from_fn(|i| if faces[i].0.is_empty() { String::new() } else { crate::asset("block", faces[i].0) });
            self.add_box(min, max, textures, faces.map(|f| f.1))
        }
        /// The model box just added drawn without the shading by side, like a cross plant (a
        /// model file's `shade: false`): a tall plant's panels.
        pub fn unshaded(mut self) -> Self {
            let last = self
                .pending()
                .iter_mut()
                .rev()
                .find(|p| p.name == abi::comp::block::BOXES && p.op == abi::value_op::PUSH);
            if let Some(p) = last
                && let Some(mut b) = abi::data::from_bytes::<abi::ModelBox>(&p.bytes)
            {
                b.flat = true;
                p.bytes = abi::data::bytes_of(&b);
            }
            self
        }
        fn add_box(self, min: [f32; 3], max: [f32; 3], textures: [String; 6], uv: [[f32; 4]; 6]) -> Self {
            let b = abi::ModelBox { min, max, textures, uv, turn: [0; 6], tilt: abi::BoxTilt::NONE, flat: false };
            self.push(&bc::BOXES, b)
                .set(&bc::MODEL, BlockModel::Boxes as u32)
        }
        /// How its model is turned: everything here (boxes, a model file, hitboxes, faces, liquid
        /// walls, support) is written facing north, its top up, and turned so. `.facing(..)`,
        /// `.axis()` and redstone's `attach` and `front` set it by themselves.
        pub fn orientation(self, o: crate::Orientation) -> Self {
            self.set(&bc::TURN, o.turn().index())
        }
        /// The last box's pictures get quarter turns of their own, per face.
        pub fn turned(mut self, turns: [u8; 6]) -> Self {
            last_box(self.pending(), |last| {
                for (f, t) in turns.iter().enumerate() {
                    last.turn[f] = (last.turn[f] + t) % 4;
                }
            });
            self
        }
        /// The last box is tilted: around a point (texels) about an axis (its own -X, +X ... as a
        /// `Dir`) by degrees, right-handed, like a model file's element.
        pub fn tilted(mut self, origin: [f32; 3], axis: crate::Dir, degrees: f32) -> Self {
            let o = axis.offset();
            let axis = [o.x as f32, o.y as f32, o.z as f32];
            last_box(self.pending(), |last| last.tilt = abi::BoxTilt { origin, axis, degrees });
            self
        }
        /// Doesn't carry redstone power, the way glass doesn't. Wire can still lie on it.
        pub fn signal_blocker(self) -> Self {
            self.tag(abi::comp::block::SIGNAL_BLOCKER)
        }
        /// Pistons and floaters can't move it. A state with this on a platform stops the platform, if
        /// more of the platform is pushing it.
        pub fn immovable(self) -> Self {
            self.tag(abi::comp::block::IMMOVABLE)
        }
        /// A piston moving it takes what touches it along, like slime.
        pub fn sticky(self) -> Self {
            self.tag(abi::comp::block::STICKY)
        }
        /// Its shape, in 1/16 units like the model's (a slab: `[0.0; 3]` to `[16.0, 8.0, 16.0]`), one
        /// box a call: what stops a walker, what the crosshair outlines and cracks. On a block that
        /// isn't solid (`not_solid`, `plant`), only the outline. Without any, the model's boxes,
        /// else a plant's cross, else the whole block.
        pub fn hitbox(self, min: [f32; 3], max: [f32; 3]) -> Self {
            self.push(&bc::HITBOX, [min[0], min[1], min[2], max[0], max[1], max[2]])
        }
        /// What the crosshair outlines and cracks, one box a call, when it shouldn't be the
        /// hitbox's: a hollow log stopped by its walls and outlined whole.
        pub fn outline(self, min: [f32; 3], max: [f32; 3]) -> Self {
            self.push(&bc::OUTLINE, [min[0], min[1], min[2], max[0], max[1], max[2]])
        }
        /// A piece a player breaks off alone (in 1/16, like the boxes): a hit inside it leaves
        /// this state with the properties `left` sets and drops what the state with those `alone`
        /// sets would. A double slab's lower half: `.piece([0.0; 3], [16.0, 8.0, 16.0],
        /// "type=top", "type=bottom")`. Without pieces a hit breaks all of it.
        pub fn piece(self, min: [f32; 3], max: [f32; 3], left: &str, alone: &str) -> Self {
            self.push(&bc::PIECES, abi::Piece { min, max, left: left.into(), alone: alone.into() })
        }
        /// More cells than its own, as its model is written (facing north, its top up): a door
        /// `&[[0, 1, 0]]`, a 3x3x3 machine every cell around. It is one block in all of them: what
        /// is done to any is done to it, and it is placed and gone whole. Its model and boxes span
        /// them from its own cell. The cells touch each other by faces.
        pub fn cells(self, cells: &[[i32; 3]]) -> Self {
            self.set(&bc::CELLS, cells.to_vec())
        }
        /// The sides a liquid in its cell can't pass (`.shares_cell_with("#water")`): without
        /// this, the ones its hitbox covers whole (a stair's back holds water, a hollow log's open
        /// ends don't). `&[]` lets it out everywhere.
        pub fn liquid_walls(self, sides: &[crate::Dir]) -> Self {
            self.set(&bc::LIQUID_WALLS, sides.iter().fold(0u8, |m, d| m | 1 << d.face()))
        }
        /// Air-like: invisible, no collision, not targetable.
        pub fn air(self) -> Self {
            self.not_solid()
                .replaceable()
                .tag(abi::comp::block::NOT_TARGETABLE)
                .set(&bc::MODEL, BlockModel::None as u32)
                .set(&bc::LAYER, RenderLayer::Opaque as u32)
        }
    };
}

impl<'a> State<'a> {
    state_setters!();
    /// How it is turned so far (`orientation`).
    fn turn(&self) -> crate::Orientation {
        self.ops
            .iter()
            .rfind(|p| p.name == abi::comp::block::TURN)
            .and_then(|p| p.bytes.first())
            .and_then(|&i| abi::Turn::from_index(i))
            .map_or(crate::Orientation::NONE, crate::Orientation::from)
    }
}

type BlockLook = Box<dyn for<'r> FnOnce(BlockBuilder<'r>) -> BlockBuilder<'r>>;
type EntityLook = Box<dyn for<'r> FnOnce(crate::EntityBuilder<'r>) -> crate::EntityBuilder<'r>>;
type StateLook = Box<dyn for<'s> FnOnce(State<'s>) -> State<'s>>;

/// A block being registered or changed (`r.edit(&X)`: what it sets goes on every state on top of
/// what is there, `state_where` on some; `.prop(..)` adds a property).
impl BlockBuilder<'_> {
    fn picked(&mut self) -> &mut u8 {
        &mut self.picked
    }
    fn own_faces(&self) -> [String; 6] {
        let own = crate::asset("block", &self.e.name);
        std::array::from_fn(|_| own.clone())
    }
    /// Its faces as set so far, which its states' own start from.
    fn faces_now(&self) -> [String; 6] {
        self.e
            .ops
            .iter()
            .rev()
            .find(|p| p.name == bc::TEXTURES.name() && p.op == abi::value_op::SET)
            .and_then(|p| abi::data::from_bytes(&p.bytes))
            .unwrap_or_else(|| self.own_faces())
    }
}

pub struct BlockBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
    handle: &'static Block,
    /// How each state is turned by its properties (`facing`, `axis`, `attach` and `front`).
    turns: Option<fn(&Props) -> crate::Orientation>,
    /// `.axis()`: placed along the face clicked.
    axis: bool,
    /// Its own properties (an edit's: those it adds), for `.states(..)` and placing.
    props: Vec<(String, Vec<String>)>,
    looks: Option<Box<dyn for<'s> Fn(&Props, State<'s>) -> State<'s>>>,
    /// `state_where`: keys and what goes on those states, after `.states(..)`.
    keyed: Vec<(String, StateLook)>,
    /// A loot table to register: (item, count), an empty item = nothing.
    drops: Option<(String, u32)>,
    item: bool,
    item_texture: String,
    /// The faces `layer`, `tint` and `tint_kind` go on (`faces`).
    picked: u8,
    facing: Option<(crate::Facings, crate::Toward)>,
    /// `on_sides`: the sides it may lie on.
    pub(crate) sides: Option<crate::Facings>,
    signal: Option<SignalBuild>,
    placement: Option<std::sync::Arc<dyn Fn(&Place, &mut Props) + Send + Sync>>,
    merge: Option<Box<dyn Fn(&Place, &mut Props) + Send + Sync>>,
    /// Its block entity's kind, `entity`.
    entity: Option<EntityLook>,
    /// What its `on_neighbor` does, `joins` and `derived` included, run in order.
    neighbor_fns: Vec<NeighborFn>,
}

type NeighborFn = Box<dyn Fn(&mut BlockEvent) + Send + Sync>;

fn block_event_cb(f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> abi::Callback {
    callback(move |a: &mut abi::BlockEventArgs| f(BlockEvent::wrap(a)))
}

/// The blocks around one (`BlockBuilder::derived`).
pub struct Neighbors {
    world: World,
    pos: BlockPos,
}

impl Neighbors {
    pub fn pos(&self) -> BlockPos {
        self.pos
    }
    pub fn world(&self) -> World {
        self.world
    }
    /// The block on that side, air where nothing is loaded.
    pub fn at(&self, side: crate::Dir) -> u32 {
        let o = side.offset();
        self.world
            .block([self.pos.x + o.x, self.pos.y + o.y, self.pos.z + o.z])
    }
}

/// Values a property can take: names (`["false", "true"]`) or numbers (`0..=7`).
pub trait PropValues {
    fn values(self) -> Vec<String>;
}
impl<const N: usize> PropValues for [&str; N] {
    fn values(self) -> Vec<String> {
        self.iter().map(|v| v.to_string()).collect()
    }
}
impl PropValues for &[&str] {
    fn values(self) -> Vec<String> {
        self.iter().map(|v| v.to_string()).collect()
    }
}
impl PropValues for std::ops::RangeInclusive<u32> {
    fn values(self) -> Vec<String> {
        self.map(|v| v.to_string()).collect()
    }
}

/// The property values of one state (in `.states(..)`) or of the state being placed (in
/// `.placement(..)`, where `set` changes them).
#[derive(Clone)]
pub struct Props {
    values: Vec<(String, String)>,
}

impl Props {
    /// The value of `name` (empty if the block has no such property).
    pub fn get(&self, name: &str) -> &str {
        self.values
            .iter()
            .find(|v| v.0 == name)
            .map_or("", |v| v.1.as_str())
    }
    pub fn is(&self, name: &str, value: &str) -> bool {
        self.get(name) == value
    }
    /// A number property (`age`); 0 if it is not a number.
    pub fn int(&self, name: &str) -> u32 {
        self.get(name).parse().unwrap_or(0)
    }
    /// The `facing` property.
    pub fn facing(&self) -> crate::Dir {
        crate::Dir::from_name(self.get("facing")).unwrap_or(crate::Dir::Up)
    }
    pub fn set(&mut self, name: &str, value: &str) {
        if let Some(v) = self.values.iter_mut().find(|v| v.0 == name) {
            v.1 = value.to_string();
        }
    }

    /// `prop=value,..`: the key of the states with these values.
    fn key(&self) -> String {
        self.values
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// `state` with these values, by the engine's layout of its block, whatever properties
    /// other mods gave it.
    fn state_from(&self, state: u32) -> u32 {
        self.values
            .iter()
            .fold(state, |s, (n, v)| crate::state_with(s, n, v).unwrap_or(s))
    }
}

/// Every combination of property values, the last property changing fastest.
fn state_combinations(props: &[(String, Vec<String>)]) -> Vec<Props> {
    let mut out = vec![Props { values: Vec::new() }];
    for (name, values) in props {
        out = out
            .into_iter()
            .flat_map(|p| {
                values.iter().map(move |v| {
                    let mut q = p.clone();
                    q.values.push((name.clone(), v.clone()));
                    q
                })
            })
            .collect();
    }
    out
}

impl<'a> BlockBuilder<'a> {
    state_setters!();

    /// A named property with its values, the first being the default: `.prop("lit", ["false",
    /// "true"])`, `.prop("age", 0..=7)`. The block has a state for every combination of its
    /// properties; read and change them with `World::prop` / `prop_set`. An edit adds it.
    pub fn prop(mut self, name: &str, values: impl PropValues) -> Self {
        self.props.push((name.to_string(), values.values()));
        self
    }

    /// Customizes each state by its property values, on top of the block's own settings:
    /// `.states(|p, s| if p.is("lit", "true") { s.light(13) } else { s })`. Its own properties
    /// only (an edit's: those it adds).
    pub fn states(mut self, f: impl for<'s> Fn(&Props, State<'s>) -> State<'s> + 'static) -> Self {
        self.looks = Some(Box::new(f));
        self
    }

    /// Changes the states `key` picks (`prop=value,..`: those with every value it names), any
    /// mod's properties: `r.edit(&base::FURNACE).state_where("lit=true", |s| s.light(15))`. On
    /// top of `.states(..)`.
    pub fn state_where(mut self, key: &str, f: impl for<'s> FnOnce(State<'s>) -> State<'s> + 'static) -> Self {
        self.keyed.push((key.to_string(), Box::new(f)));
        self
    }

    /// Tools of `class` (`tools/<class>` items) mine it at their speed (tag `mineable/<class>`),
    /// and it drops nothing unless mined by one of at least `level` (`tool_tier`; `HAND`: drops
    /// anyway). The level is the `harvest_level` component, checked by `Cond::harvested`.
    pub fn mined_by(self, class: &str, level: u32) -> Self {
        let b = self.tag(&format!("mineable/{class}"));
        if level != abi::tool_tier::HAND { b.set(&c::HARVEST_LEVEL, level) } else { b }
    }

    /// Shades its textures with a shader handle (`Shader::named("stripes")`, the file
    /// `shaders/block/stripes.wgsl`): the shader goes
    /// with the textures, so blocks sharing one share it.
    pub fn shader(self, shader: &crate::Shader) -> Self {
        self.set_text(&c::SHADER, shader.name())
    }

    /// Creative inventory tab of the block's item, see `Registry::creative_tab`.
    pub fn tab(self, tab: &CreativeTab) -> Self {
        self.tag(&tab_tag(tab))
    }
    /// Its item is in no creative tab (`.untag(abi::comp::NO_TAB)` brings it back).
    pub fn no_tab(self) -> Self {
        self.tag(abi::comp::NO_TAB)
    }

    /// Sounds from the group `<ns>:block.<group>` (`.break`, `.place`, `.step`, `.hit` events);
    /// a name with a namespace is used as it is (`base:block.stone` from another mod).
    pub fn sound_group(self, group: &str) -> Self {
        let prefix = if group.contains(':') { group.to_string() } else { format!("{}:block.{group}", self.reg.ns) };
        self.set_text(&c::SOUND, &prefix)
    }

    /// Its loot table: `count` of `item` when harvested (`Cond::harvested`). Without it the
    /// block drops its own item the same way; `r.loot(&BLOCK)` writes any other table.
    pub fn drops(mut self, item: &str, count: u32) -> Self {
        self.drops = Some((item.to_string(), count));
        self
    }

    /// An empty loot table.
    pub fn drops_nothing(mut self) -> Self {
        self.drops = Some((String::new(), 0));
        self
    }

    /// The block's item shows this flat texture from `item/` instead of the block model.
    pub fn item_texture(mut self, t: &str) -> Self {
        self.item_texture = crate::asset("item", t);
        self
    }

    pub fn no_item(mut self) -> Self {
        self.item = false;
        self
    }

    /// In an edit (`Registry::edit`): block texture `from` (another mod's, `base:torch`) becomes
    /// `to` on its states, and the item texture of that name too.
    pub fn retexture(self, from: &str, to: &str) -> Self {
        let pair = |folder: &str| abi::Retexture { from: crate::asset(folder, from), to: crate::asset(folder, to) };
        self.push(&bc::RETEXTURE, pair("block"))
            .push(&bc::RETEXTURE, pair("item"))
    }

    /// Grows over time, a step every `mean_seconds` on average (spread like random ticks) while
    /// the light is at least `min_light`: by default to the next state until the last. Timed by
    /// world time, so unloaded plants catch up when their area loads again. A rule
    /// (`Rule::random(..).grows()`).
    pub fn grows(self, mean_seconds: f32, min_light: u32) -> Self {
        let r = crate::Rule::random(mean_seconds).grows();
        self.rule(if min_light > 0 { r.when(crate::Cond::light(min_light, 15)) } else { r })
    }

    /// The block it stands on must have one `what` takes beside it (a block matcher, sugar
    /// cane: `#water`), unless that is this same block.
    pub fn needs_near(self, what: &str) -> Self {
        let own = crate::Cond::side(crate::Dir::Down, &self.e.name);
        self.rule(
            crate::Rule::neighbor()
                .when(!own & !crate::Cond::beside(crate::Dir::Down, what))
                .breaks(),
        )
    }

    /// What a growth step does instead of moving to the next state (growing a tree, a block on
    /// top); it keeps growing as long as the block stays.
    pub fn on_grow(self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.set(&bc::ON_GROW, block_event_cb(f))
    }

    /// Spreads onto blocks `to` takes near it (a block matcher, grass onto `#grass_spreads_to`):
    /// a target within 1 block sideways, 3 down and 1 up with `min_light` above it and nothing
    /// opaque on top turns into this block in `mean_seconds` on average (sooner with more of this
    /// around), also while nobody is near (caught up on loading).
    pub fn spreads(self, to: &str, mean_seconds: f32, min_light: u32) -> Self {
        self.set_text(&c::SPREADS_TO, to)
            .set(&c::SPREAD_TIME, mean_seconds)
            .spread_when(crate::Cond::new(&format!("light({min_light}.., up) & !covered")))
    }

    /// What has to hold for a target of `spreads` instead (moss up a wall:
    /// `Cond::new("light(9.., any)")`, covered or not).
    pub fn spread_when(self, when: impl Into<crate::Cond>) -> Self {
        self.set(&c::SPREAD_WHEN, vec![when.into().0])
    }

    /// Where `spreads` reaches, from this block to the target: min and max offsets, at most 4
    /// each way (grass: `[-1, -3, -1]` to `[1, 1, 1]`, the default).
    pub fn spread_reach(self, min: impl Into<BlockPos>, max: impl Into<BlockPos>) -> Self {
        let (min, max) = (min.into(), max.into());
        self.set(&c::SPREAD_REACH, [min.x, min.y, min.z, max.x, max.y, max.z])
    }

    /// Turns into `block` in `mean_seconds` on average once something opaque covers it (grass
    /// into dirt).
    pub fn smothered_into(self, block: &str, mean_seconds: f32) -> Self {
        self.rule(
            crate::Rule::random(mean_seconds)
                .when(crate::Cond::covered())
                .becomes(block),
        )
    }

    /// Particles of `kind` clients show around it now and then, near the player (vanilla's
    /// torch smoke, a crystal's sparks). A kind nothing registers is a load error.
    pub fn ambient_particle(self, kind: &str) -> Self {
        self.ambient_particles(&[kind])
    }

    /// Like `ambient_particle` with several kinds at once (a torch's flame and smoke).
    pub fn ambient_particles(self, kinds: &[&str]) -> Self {
        let kinds = kinds
            .iter()
            .map(|k| crate::full_name(k))
            .collect::<Vec<_>>()
            .join(",");
        self.set_text(&c::AMBIENT_PARTICLE, &kinds)
    }

    pub fn on_use(self, f: impl Fn(&mut Use) -> bool + Send + Sync + 'static) -> Self {
        let cb = callback(move |a: &mut abi::UseArgs| {
            let handled = f(Use::wrap(a));
            a.handled = handled as u32;
        });
        self.set(&bc::ON_USE, cb)
    }

    /// One of these was placed: `actions::PLACE_BLOCK` for this block, at the end of the tick.
    pub fn on_place(self, f: impl Fn(&World, &crate::BlockChange) + Send + Sync + 'static) -> Self {
        self.on_own(&crate::actions::PLACE_BLOCK, f)
    }

    /// One of these was broken, by a player (`player`) or by losing its support (0).
    pub fn on_break(self, f: impl Fn(&World, &crate::BlockChange) + Send + Sync + 'static) -> Self {
        self.on_own(&crate::actions::BREAK_BLOCK, f)
    }

    /// `action`'s changes of this block.
    fn on_own(
        self,
        action: &'static crate::Action<crate::BlockChange>,
        f: impl Fn(&World, &crate::BlockChange) + Send + Sync + 'static,
    ) -> Self {
        let block = self.handle;
        self.reg.on_event(action, move |b| {
            let w = b.world();
            for p in b.iter().filter(|p| block.is(p.state)) {
                f(&w, p);
            }
        });
        self
    }

    pub fn on_neighbor(mut self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.neighbor_fns.push(Box::new(f));
        self
    }

    /// A `true`/`false` property for each of `sides` (`north`, `east` .. `up`, `down`) kept saying
    /// whether the block there is one `to` takes (a block matcher: `#fences`, another mod's pipes
    /// by a shared tag, its own name). Fences, walls, panes, pipes and cables: `.states` shapes it
    /// by `p.is("north", "true")`.
    pub fn joins(mut self, sides: crate::Facings, to: &str) -> Self {
        for d in sides.dirs() {
            let there = crate::Cond::new(&format!("side({}, {to})", d.name()));
            self = self
                .prop(d.name(), ["false", "true"])
                .rule(
                    crate::Rule::neighbor()
                        .when(there.clone())
                        .becomes(&format!("{}=true", d.name())),
                )
                .rule(
                    crate::Rule::neighbor()
                        .when(!there)
                        .becomes(&format!("{}=false", d.name())),
                );
        }
        self
    }

    /// Property `prop` worked out from the blocks around, kept up to date by the engine as they
    /// change (a stair's corner shape). `f` gives the value; the property must have it.
    pub fn derived(self, prop: &'static str, f: impl Fn(&Neighbors) -> &'static str + Send + Sync + 'static) -> Self {
        self.derive_each(move |n, _| {
            let want = f(n);
            if n.world().prop(n.pos(), prop) != Some(want) {
                n.world().prop_set(n.pos(), prop, want);
            }
        })
    }

    fn derive_each(mut self, f: impl Fn(&Neighbors, &BlockEvent) + Send + Sync + 'static) -> Self {
        self.neighbor_fns.push(Box::new(move |e: &mut BlockEvent| {
            let n = Neighbors { world: e.world(), pos: e.pos() };
            f(&n, e)
        }));
        self
    }

    /// On a moving body, it came to another cell of the world: a drill, a plow. `pos` is that cell,
    /// in the world's coordinates; `body_pos` where this is on the body, `entity` the body.
    pub fn on_carried(self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.set(&bc::ON_CARRIED, block_event_cb(f))
    }
    pub fn on_scheduled(self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.set(&bc::ON_SCHEDULED, block_event_cb(f))
    }

    /// Falls like sand when nothing holds it up: a block next to it changing checks two ticks
    /// later whether below is free, and if so it drops as a moving block (`World::fall_block`).
    pub fn falling(self) -> Self {
        self.rule(
            crate::Rule::after(0.1)
                .when(crate::Cond::open(crate::Dir::Down))
                .falls(),
        )
    }

    /// Each placed block gets a block entity, an invisible entity of a kind named like the block
    /// that comes and goes with it: data of its own (`e.attach(..)`, ticked by a system), a
    /// model, items (`e.inventory(27)`). Doesn't make it usable: that is `opens` or `on_use`.
    /// `.entity(|e| e.attach(&SPEED))`. Its items drop when it breaks.
    pub fn entity(mut self, f: impl for<'r> FnOnce(crate::EntityBuilder<'r>) -> crate::EntityBuilder<'r> + 'static) -> Self {
        self.entity = Some(Box::new(f));
        self
    }

    /// Using it opens `menu`, on its block entity if it has one (a furnace), else on nothing (a
    /// crafting table). A menu with `.recipes(..)` makes the block a station of those recipes in
    /// the recipe viewer.
    pub fn opens(self, menu: &crate::Menu) -> Self {
        self.set_text(&bc::OPENS, menu.name())
    }

    /// Whether a sneaking player uses it (`abi::sneak_use`): without, as vanilla, only with empty
    /// hands, what is held going first otherwise (a block placed against a chest). `ALWAYS`
    /// calls `on_use` with `sneaking` set, and it passes the click on with `handled = 0`.
    pub fn sneak_use(self, rule: u32) -> Self {
        self.set(&bc::SNEAK_USE, rule)
    }

    /// A `facing` property with the directions in `allowed` (`p.facing()` in `.states`), set by
    /// `toward` when placed: three allowed directions are three values. Its model, written facing
    /// north, turns to it (`Orientation::front`) unless a state says otherwise.
    pub fn facing(mut self, allowed: crate::Facings, toward: crate::Toward) -> Self {
        self.facing = Some((allowed, toward));
        self.turns = Some(|p| crate::Orientation::front(p.facing()));
        self.prop(
            "facing",
            allowed
                .dirs()
                .map(|d| d.name())
                .collect::<Vec<_>>()
                .as_slice(),
        )
    }

    /// An `axis` property (`y`, `x`, `z`), set along the face clicked, its model written standing
    /// up and laid that way (a log): `p.get("axis")` in `.states`.
    pub fn axis(mut self) -> Self {
        self.axis = true;
        self.turns = Some(|p| crate::Orientation::axis(p.get("axis")));
        self.prop("axis", ["y", "x", "z"])
    }

    /// Which way it faces as an item and by default (after `facing`).
    pub fn default_facing(mut self, d: crate::Dir) -> Self {
        if let Some((_, values)) = self.props.iter_mut().find(|p| p.0 == "facing") {
            if let Some(i) = values.iter().position(|v| v == d.name()) {
                values.swap(0, i);
            }
        }
        self
    }

    /// Wire that lies on any face of a block, connecting to wire of the same block only; textures
    /// are the dot on top and the line on the sides. It lies on sides of its cell like `on_sides`
    /// (`west`..`south` properties).
    pub fn wire(mut self) -> Self {
        self.signal = Some(SignalBuild { kind: abi::signal_kind::WIRE, ..SignalBuild::default() });
        self.not_solid()
            .set(&bc::MODEL, BlockModel::Wire as u32)
            .cutout()
            .hardness(0.0)
            .lying_on(crate::Facings::ALL, false)
    }

    fn signal_props(mut self, kind: u32, levels: u32, attach: bool, front: bool) -> Self {
        self.signal = Some(SignalBuild { kind, attaches: attach, ..self.signal.unwrap_or_default() });
        if attach && front {
            self.turns = Some(|p| {
                let dir = |n: &str| crate::Dir::from_name(p.get(n));
                crate::Orientation::new(dir("attach").unwrap_or(crate::Dir::Down), dir("front").unwrap_or(crate::Dir::North))
            });
        }
        if attach {
            self = self.prop("attach", ["down", "up", "north", "south", "west", "east"]);
        }
        if front {
            self = self.prop("front", ["north", "south", "west", "east", "up", "down"]);
        }
        match levels {
            0 => self,
            2 => self.prop("power", ["off", "on"]),
            3 => self.prop("power", 0..=2),
            _ => self.prop("power", 0..=15),
        }
    }

    /// Always gives full power to what is next to it.
    pub fn signal_constant(self) -> Self {
        self.signal_props(abi::signal_kind::SWITCH, 0, false, false)
    }
    /// A lever or button: goes on the clicked face; its `power` property (`off`, `on`) gives full
    /// power around it and strongly into the block it is on.
    pub fn signal_switch(self) -> Self {
        self.signal_props(abi::signal_kind::SWITCH, 2, true, true)
    }
    /// Goes on the clicked face and gives full power unless the block it is on has any.
    pub fn signal_torch(self) -> Self {
        self.signal_props(abi::signal_kind::TORCH, 2, true, false)
    }
    /// Goes on the clicked face; full power out of the front, a `delay` property (1..4 redstone
    /// ticks, two game ticks each) after what is behind it has any.
    pub fn signal_repeater(self) -> Self {
        self.signal_props(abi::signal_kind::REPEATER, 2, true, true)
            .prop("delay", 1..=4)
    }
    /// Goes on the clicked face; out of the front what is behind it, unless a side has more. Its
    /// `mode` property (`compare`, `subtract`) takes the sides from it instead.
    pub fn signal_comparator(self) -> Self {
        self.signal_props(abi::signal_kind::COMPARATOR, 16, true, true)
            .prop("mode", ["compare", "subtract"])
    }
    /// A pulse out of its back when the block at its `facing` changes (after `facing`).
    pub fn signal_observer(self) -> Self {
        self.signal_props(abi::signal_kind::OBSERVER, 2, false, false)
    }
    /// Its `power` property, if it has one, and `on_signal` follow the power around it.
    pub fn signal_sink(self) -> Self {
        self.signal_props(abi::signal_kind::SINK, 0, false, false)
    }
    /// What a comparator behind it reads, 0..15, instead of how full it is. It looks again when
    /// the block changes; for anything else it depends on, call `World::signal_read_changed`.
    pub fn signal_read(self, f: impl Fn(&World, BlockPos) -> u8 + Send + Sync + 'static) -> Self {
        let cb = callback(move |a: &mut abi::SignalReadArgs| a.output = f(&World::from_raw(a.world), a.pos) as u32);
        self.set(&bc::SIGNAL_READ, cb)
    }
    /// Goes along its `facing` (after `facing`), any of the six ways. Its front is what sticks: with
    /// power from any other side it carries itself, the block in front of it and what is stuck to
    /// that the way slime does, half a block a second per level of power. Circuits on it go on.
    /// Without power, or against what it can't go through, it all lands on a whole block.
    /// `power`: 0 idle, 1 going, 2 stuck. `breaks`: it breaks what is in its way instead.
    pub fn signal_floater(mut self, breaks: bool) -> Self {
        let flags = if breaks { abi::signal_flags::BREAKS } else { 0 };
        self.signal = Some(SignalBuild { flags, ..SignalBuild::default() });
        self.signal_props(abi::signal_kind::FLOATER, 3, false, false)
    }
    /// Pushes blocks at its `facing` with power from any other side (after `facing`). Its `power`
    /// property: `off` in, `moving` while its head is on the way, `on` out, the head being the
    /// state's own cell in front (`.cells`). `head`: what the head looks like on the way, a block
    /// with a `facing` property that is never in the world. `sticky` brings a block back.
    pub fn signal_piston(mut self, head: &'static str, sticky: bool) -> Self {
        let flags = if sticky { abi::signal_flags::STICKY } else { 0 };
        self.signal = Some(SignalBuild { head, flags, ..SignalBuild::default() });
        self.signal_props(abi::signal_kind::PISTON, 0, false, false)
            .prop("power", ["off", "moving", "on"])
    }
    /// A piston's head on its way (after `facing`), drawn by the body moving it.
    pub fn signal_piston_head(self) -> Self {
        self.tag(abi::comp::block::NOT_STOOD_ON)
            .signal_props(abi::signal_kind::PISTON_HEAD, 0, false, false)
    }
    /// A gate with logic of your own, on the clicked face: what comes out of its outputs a
    /// `gate`'s delay after what goes in at its inputs calls for it. Runs only when something
    /// around it changes.
    pub fn signal_gate(mut self, gate: Gate, f: impl Fn(&GateInputs) -> u8 + Send + Sync + 'static) -> Self {
        // In redstone ticks, like a repeater's: two game ticks each.
        self.signal = Some(SignalBuild {
            delay: Some(gate.delay * 2),
            inputs: gate.inputs,
            outputs: gate.outputs,
            ..self.signal.unwrap_or_default()
        });
        self.set(&bc::SIGNAL_GATE, callback(move |a: &mut abi::GateArgs| a.output = f(GateInputs::wrap(a)) as u32))
            .signal_props(abi::signal_kind::GATE, 16, true, true)
    }
    /// Called when the power around a `signal_sink` changes.
    pub fn on_signal(self, f: impl Fn(&SignalEvent) + Send + Sync + 'static) -> Self {
        self.set(&bc::ON_SIGNAL, callback(move |a: &mut abi::SignalArgs| f(SignalEvent::wrap(a))))
    }

    /// Its own item placed in its cell: `.merge(|p, props| props.set("type", "double"))` takes the
    /// item in, with `props` those of what is there and `p.placed("type")` how the item would go;
    /// leaving them as they are places it as usual.
    pub fn merge(mut self, f: impl Fn(&Place, &mut Props) + Send + Sync + 'static) -> Self {
        self.merge = Some(Box::new(f));
        self
    }

    /// What may be in one cell with it though their shapes overlap (a block matcher): snow on a
    /// plant, `#water` for a source of water to stay in it and a bucket to fill it (a liquid fills
    /// its whole cell). Blocks whose shapes don't overlap share a cell without asking.
    pub fn shares_cell_with(self, what: &str) -> Self {
        self.set_text(&bc::SHARES_CELL, what)
    }

    /// Not a block but a template: see `Registry::shapes_of`.
    pub(crate) fn shapes_of(self, sources: &str, suffix: &str) -> Self {
        self.set(&bc::SHAPES_OF, abi::ShapesOf { sources: sources.to_string(), suffix: suffix.to_string() })
    }

    /// Chooses property values when a player places it (after `facing`):
    /// `.placement(|p, props| props.set("axis", if p.face().horizontal() { "x" } else { "y" }))`.
    pub fn placement(mut self, f: impl Fn(&Place, &mut Props) + Send + Sync + 'static) -> Self {
        self.placement = Some(std::sync::Arc::new(f));
        self
    }
}

impl BlockBuilder<'_> {
    /// Its properties, the parts of its states, its callbacks, and its item, loot and block entity.
    fn finish(&mut self) {
        if std::thread::panicking() {
            return;
        }
        for (name, values) in &self.props {
            let p = abi::BlockProp { name: name.clone(), values: values.clone() };
            self.e
                .ops
                .push(element(&bc::PROPS, &p, abi::value_op::PUSH, ""));
        }
        // A part per combination of its own properties' values, from `.states(..)` and its signal.
        let block_faces = self.faces_now();
        let looks = self.looks.take();
        let (signal, turns) = (self.signal, self.turns);
        if looks.is_some() || signal.is_some() || turns.is_some() {
            for p in state_combinations(&self.props) {
                let mut ops = Vec::new();
                let mut s = State::new(&mut ops, &block_faces);
                if let Some(o) = turns
                    .map(|f| f(&p))
                    .filter(|o| *o != crate::Orientation::NONE)
                {
                    s = s.orientation(o);
                }
                if let Some(f) = &looks {
                    s = f(&p, s);
                }
                if let Some(b) = &signal {
                    b.apply(&p, s);
                }
                if !ops.is_empty() {
                    self.e.parts.push((p.key(), ops));
                }
            }
        }
        for (key, f) in std::mem::take(&mut self.keyed) {
            let mut ops = Vec::new();
            f(State::new(&mut ops, &block_faces));
            self.e.parts.push((key, ops));
        }
        let attaches = signal.is_some_and(|b| b.attaches);
        let switch = signal.is_some_and(|b| b.kind == abi::signal_kind::SWITCH);
        if self.facing.is_some() || self.placement.is_some() || attaches || self.axis || self.sides.is_some() {
            let (facing, axis, sides, user) = (self.facing, self.axis, self.sides, self.placement.take());
            let defaults = Props {
                values: self
                    .props
                    .iter()
                    .map(|(n, v)| (n.clone(), v[0].clone()))
                    .collect(),
            };
            let cb = callback(move |a: &mut abi::PlaceArgs| {
                let mut p = defaults.clone();
                if let Some((allowed, toward)) = facing {
                    p.set("facing", toward.choose(allowed, a.face, a.look, a.hit).name());
                }
                // Against what was clicked, else the allowed side nearest the look; the engine adds
                // it to what is there.
                let back = crate::Dir::from_face(a.face).opposite();
                if let Some(allowed) = sides {
                    let on = if allowed.allows(back) { back } else { crate::Toward::Away.choose(allowed, a.face, a.look, a.hit) };
                    for d in allowed.dirs() {
                        p.set(d.name(), if d == on { "true" } else { "false" });
                    }
                }
                if axis {
                    p.set("axis", ["x", "y", "z"][a.face as usize / 2 % 3]);
                }
                if attaches {
                    let attach = back;
                    p.set("attach", attach.name());
                    // A switch on a wall is always upright, as in vanilla; a gate goes with the look.
                    let front =
                        if switch && attach.horizontal() { crate::Dir::Down } else { crate::facing::dir_along_face(attach, a.look) };
                    p.set("front", front.name());
                }
                if let Some(f) = &user {
                    f(Place::wrap(a), &mut p);
                }
                a.state = p.state_from(a.state);
            });
            self.e.ops.push(bc::PLACEMENT.raw_value(&cb));
        }
        // What lies on sides takes in its own item as one more side.
        let sides = self
            .sides
            .map(|allowed| allowed.dirs().map(|d| d.name()).collect::<Vec<_>>());
        if let Some(sides) = sides.filter(|_| self.merge.is_none()) {
            self.merge = Some(Box::new(move |p, props| {
                for d in sides.iter().filter(|d| p.placed(d) == Some("true")) {
                    props.set(d, "true");
                }
            }));
        }
        if let Some(f) = self.merge.take() {
            let names: Vec<String> = self.props.iter().map(|p| p.0.clone()).collect();
            let cb = callback(move |a: &mut abi::PlaceArgs| {
                let values = names
                    .iter()
                    .map(|n| {
                        (
                            n.clone(),
                            crate::world::state_prop(a.state, n)
                                .unwrap_or_default()
                                .to_string(),
                        )
                    })
                    .collect();
                let mut p = Props { values };
                f(Place::wrap(a), &mut p);
                a.state = p.state_from(a.state);
            });
            self.e.ops.push(bc::MERGE.raw_value(&cb));
        }
        if !self.neighbor_fns.is_empty() {
            let fns = std::mem::take(&mut self.neighbor_fns);
            let cb = block_event_cb(move |e| {
                for f in &fns {
                    f(e);
                }
            });
            self.e.ops.push(bc::ON_NEIGHBOR.raw_value(&cb));
        }
        let (name, mode) = (self.e.name.clone(), self.e.mode);
        let edit = mode == abi::entry_mode::EDIT;
        if let Some(f) = self.entity.take() {
            // Its block entity is edited or replaced with it.
            self.reg
                .block_entity(Block::generated(name.clone()), mode, f);
        }
        let texture = std::mem::take(&mut self.item_texture);
        match (edit, self.item) {
            // An edit leaves the block's item as it is, but for what it says of it.
            (true, true) if !texture.is_empty() => drop(
                self.reg
                    .edit(Item::generated(name.clone()))
                    .set_text(&crate::comps::item::LAYERS, &texture),
            ),
            (true, true) => {}
            (true, false) => self.reg.remove(Item::generated(name.clone())),
            (false, true) => drop(self.reg.block_item(&name, texture, mode)),
            (false, false) => {}
        }
        if let Some((item, count)) = self.drops.take() {
            // An edit's drops are the block's table from now on.
            let block = Block::generated(name);
            let table = if mode == abi::entry_mode::NEW { self.reg.loot(block) } else { self.reg.replace(block.loot()) };
            let table = table.set(&crate::comps::loot::POOLS, Vec::new());
            if !item.is_empty() {
                drop(
                    table.pool(
                        crate::extend::Pool::new()
                            .when(crate::extend::Cond::harvested())
                            .items(&item, count, count),
                    ),
                );
            }
        }
    }
}

pub struct ItemBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(
    Block => BlockBuilder, block, finish;
    Item => ItemBuilder, item;
    Biome => BiomeBuilder, biome;
    Dimension => DimensionBuilder, dimension;
);

impl ItemBuilder<'_> {
    /// An attribute modifier its holder gets while it is in the main hand, or `.worn()` in an equipment slot (`Modifier`).
    pub fn modifier(self, m: crate::Modifier) -> Self {
        self.push(&crate::component::MODIFIERS, m.0)
    }
    /// How a stack looks, wherever it is displayed. Reads public components marked
    /// `stack_flags::VISUAL` (wear is marked by default); count is normalized to one.
    /// The result is cached until those components change.
    pub fn condition(self, f: impl Fn(&abi::ItemStack, &mut Looks) + Send + Sync + 'static) -> Self {
        let cb = callback(move |a: &mut abi::ItemModelArgs| {
            let mut v = Looks::default();
            f(&a.stack, &mut v);
            LOOKS.with(|l| {
                *l.borrow_mut() = v;
                let l = l.borrow();
                (a.model, a.texture, a.worn_model, a.worn_texture) =
                    (Str::new(&l.model), Str::new(&l.texture), Str::new(&l.worn_model), Str::new(&l.worn_texture));
            });
        });
        self.set(&crate::comps::item::CONDITION, cb)
    }
    /// Its only picture (under `item/`; without one: the item's name), the layers given so
    /// far dropped: `.texture("ingot").tint(0xC0C0FF)`.
    pub fn texture(self, t: &str) -> Self {
        self.with_layers(crate::Layers::new().layer(t))
    }
    /// One more picture on top, the first at the bottom: a tool made of parts,
    /// `.layer("handle").layer("head").tint(0xFF8800)`. The first replaces its own picture.
    /// Stacks can still show their own (`with_layers`).
    pub fn layer(self, t: &str) -> Self {
        let layers = self.set_layers().unwrap_or_default().layer(t);
        self.with_layers(layers)
    }
    /// Tints the layer just added `0xRRGGBB` (its own picture if none was), multiplied: white
    /// parts take the color. One picture for many items of a color.
    pub fn tint(self, color: u32) -> Self {
        let layers = self
            .set_layers()
            .unwrap_or_else(|| self.own_layers())
            .tint(color);
        self.with_layers(layers)
    }
    /// The layers given so far, if any.
    fn set_layers(&self) -> Option<crate::Layers> {
        self.e
            .ops
            .iter()
            .rev()
            .find(|p| p.name == abi::comp::LAYERS)
            .and_then(|p| p.text.as_deref())
            .map(crate::Layers::parse)
    }
    /// Its picture when none is given: `<ns>:item/<name>`.
    fn own_layers(&self) -> crate::Layers {
        let (ns, path) = self
            .e
            .name
            .split_once(':')
            .unwrap_or((self.reg.ns, self.e.name.as_str()));
        crate::Layers::parse(&format!("{ns}:item/{path}"))
    }
    fn with_layers(self, layers: crate::Layers) -> Self {
        let text = layers.text();
        self.set_text(&crate::comps::item::LAYERS, &text)
    }
    /// Stack size (`max_stack`).
    pub fn max_stack(self, n: u32) -> Self {
        self.set(&c::MAX_STACK, n)
    }
    /// Its clips, from `animations/<name>.animation.json`; a file named after the item is used
    /// even without this. They play on whoever holds it.
    pub fn animations(self, name: &str) -> Self {
        let name = crate::asset("", name);
        self.set_text(&c::ANIMATIONS, &name)
    }
    /// Needs both hands: while the other holds something it does nothing, and it can't go in
    /// the other hand. Its `hold` action shows the two.
    pub fn two_handed(self) -> Self {
        self.tag(abi::comp::TWO_HANDED)
    }
    /// A `two_handed` item may be held in one hand after all, playing this action. It goes in
    /// the other hand then too.
    pub fn one_handed_action(mut self, action: &str) -> Self {
        self.e
            .ops
            .push(Pending::text(abi::comp::ONE_HANDED, action));
        self.tag(abi::comp::TWO_HANDED)
    }
    /// On a right click the hand whose item has the higher priority goes first, whichever hand
    /// that is (default 0, then the main hand first). The other hand gets its turn when this
    /// item does nothing: a bow with no arrows.
    pub fn use_priority(self, priority: i32) -> Self {
        self.set(&c::USE_PRIORITY, priority)
    }
    /// Uses before it breaks (`durability`); damageable items do not stack.
    pub fn durability(self, d: u32) -> Self {
        self.set(&c::DURABILITY, d).set(&c::MAX_STACK, 1)
    }
    /// A tool of `class` (tag `tools/<class>`): it mines blocks `mined_by` that class, at its
    /// `mining_speed`, harvesting those needing up to `level` (`tool_level`). Held like one.
    pub fn tool(self, class: &str, level: u32) -> Self {
        let tag = format!("tools/{class}");
        self.tag(&tag).set(&c::TOOL_LEVEL, level).handheld()
    }
    /// Held like a tool in third person, by its end and pointing ahead (tag `handheld`, vanilla's
    /// `item/handheld` model), instead of flat in the hand. Tools have it already.
    pub fn handheld(self) -> Self {
        self.tag("handheld")
    }
    /// How fast it mines its class's blocks (vanilla wood 2 ... gold 12).
    pub fn mining_speed(self, speed: f32) -> Self {
        self.set(&c::MINING_SPEED, speed)
    }
    /// A full swing on the ground also hits what stands next to the target, like a sword.
    pub fn sweeps(self) -> Self {
        self.tag(abi::comp::SWEEPS)
    }
    /// For fighting: in creative it doesn't break blocks, as a sword in modern. Client and server
    /// both know, so nothing flickers. A rule that depends on more is `actions::BREAK_BLOCK`.
    pub fn no_creative_break(self) -> Self {
        self.tag(abi::comp::NO_CREATIVE_BREAK)
    }
    /// Takes effect after holding the use button this many ticks (food 32), slowing the player down.
    pub fn use_ticks(self, ticks: u32) -> Self {
        self.set(&c::USE_TICKS, ticks)
    }
    /// Held as long as the use button is, slowing the player down, with nothing at the end:
    /// `on_release` says what letting go does after how long (a bow, a charged spear).
    pub fn use_until_released(self) -> Self {
        self.use_ticks(abi::USE_HOLD)
    }
    /// The use button let go while using it (`use_until_released`, or before `use_ticks` ran
    /// out): `u.held_ticks()` is how long it was held. True if it did something.
    pub fn on_release(self, f: impl Fn(&mut ItemUse) -> bool + Send + Sync + 'static) -> Self {
        let cb = callback(move |a: &mut abi::ItemUseArgs| {
            let handled = f(ItemUse::wrap(a));
            a.handled = handled as u32;
        });
        self.set(&crate::comps::item::ON_RELEASE, cb)
    }
    /// What is left of it when it is used up (a potion's bottle, fuel's bucket).
    pub fn remainder(self, item: &crate::Item) -> Self {
        self.set_text(&c::REMAINDER, item.name())
    }
    /// Used, it gives `effect` at `level` for `secs` (`f32::INFINITY` for good; a potion: with `.use_ticks(32)` and
    /// `.remainder(&GLASS_BOTTLE)` that is all of it); several for several effects. It turns into
    /// its remainder, or one fewer; with an `on_use` of its own (eating) that does the rest and
    /// these come when it goes through.
    pub fn gives_effect(self, effect: &crate::EntityEffect, level: u32, secs: f32) -> Self {
        let gift = abi::EffectGift { effect: effect.name().to_string(), level: level.max(1), ticks: (secs * 20.0) as u32 };
        self.push(&c::GIVES_EFFECTS, gift)
    }
    /// Full swings per second: hitting sooner does less (a sword 1.6, an empty hand 4).
    pub fn attack_speed(self, per_second: f32) -> Self {
        self.set(&c::ATTACK_SPEED, per_second)
    }
    pub fn attack_damage(self, damage: f32) -> Self {
        self.set(&c::ATTACK_DAMAGE, damage)
    }
    pub fn places(self, block: &Block) -> Self {
        self.set_text(&crate::comps::item::BLOCK, block.name())
    }
    /// Worn in equipment `slot`, where it shows as `model` on the wearer's bone `bone`, moved by
    /// what plays on the wearer, like `Registry::bone_model`: a tail only those who wear it have.
    pub fn wearable_model(self, slot: &EquipmentSlot, model: &str, bone: &str) -> Self {
        let model = format!("{}@{bone}", crate::asset("", model));
        self.tag(&format!("worn/{}", slot.name()))
            .set(&c::WORN, abi::Worn { inflate: 0.0 })
            .set_text(&c::WORN_MODEL, &model)
    }

    /// Creative inventory tab, see `Registry::creative_tab`.
    pub fn tab(self, tab: &CreativeTab) -> Self {
        self.tag(&tab_tag(tab))
    }
    /// In no creative tab nor its search, still obtainable (`.untag(abi::comp::NO_TAB)` brings it
    /// back).
    pub fn no_tab(self) -> Self {
        self.tag(abi::comp::NO_TAB)
    }
    /// Worn in equipment `slot` (any mod's), where it shows on the wearer's model over the parts
    /// the slot covers: `texture` (under `entity/`, like `armor/iron_1`) in the wearer's skin
    /// layout, `inflate` pixels out from the body.
    pub fn wearable(self, slot: &EquipmentSlot, inflate: f32, texture: &str) -> Self {
        let tex = crate::asset("entity", texture);
        self.tag(&format!("worn/{}", slot.name()))
            .set(&c::WORN, abi::Worn { inflate })
            .set_text(&c::WORN_TEXTURE, &tex)
    }

    /// Used on a block, puts `entity` on that side of it. One is used up, outside creative. Entities
    /// have an egg of their own without this; it is for something else that hatches one.
    pub fn spawns(mut self, entity: &'static crate::EntityKind) -> Self {
        self.e
            .ops
            .push(Pending::text(abi::comp::SPAWN_EGG, entity.name()));
        self.on_use(move |u| {
            let Some((p, face)) = u.target_block() else { return false };
            let (w, side) = (u.world(), crate::Dir::from_face(face).offset());
            let at = [(p.x + side.x) as f64 + 0.5, (p.y + side.y) as f64, (p.z + side.z) as f64 + 0.5];
            w.spawn_with(entity, at, &crate::Spawn::new().cause(abi::spawn_cause::EGG));
            if !w.creative(u.entity()) {
                u.set_stack(crate::StackData::shrink(u.stack(), 1));
            }
            true
        })
    }

    pub fn on_use(self, f: impl Fn(&mut ItemUse) -> bool + Send + Sync + 'static) -> Self {
        let cb = callback(move |a: &mut abi::ItemUseArgs| {
            let handled = f(ItemUse::wrap(a));
            a.handled = handled as u32;
        });
        self.set(&crate::comps::item::ON_USE, cb)
    }
}

/// What `ItemBuilder::condition` changes of a stack's look; what it doesn't set stays.
#[derive(Default)]
pub struct Looks {
    model: String,
    texture: String,
    worn_model: String,
    worn_texture: String,
}

impl Looks {
    /// A model attached to a bone while this stack is in equipment.
    pub fn worn_model(&mut self, model: &str, bone: &str) -> &mut Self {
        self.worn_model = format!("{}@{bone}", crate::asset("", model));
        self
    }
    /// A texture on the wearer's armor geometry, under `entity/`.
    pub fn worn_texture(&mut self, texture: &str) -> &mut Self {
        self.worn_texture = crate::asset("entity", texture);
        self
    }
    /// A texture from `item/` instead of its own.
    pub fn texture(&mut self, t: &str) -> &mut Self {
        self.texture = crate::asset("item", t);
        self
    }
    /// A model file instead of its own (`set_model`).
    pub fn model(&mut self, m: &str) -> &mut Self {
        self.model = crate::asset("", m);
        self
    }
}

thread_local! {
    static LOOKS: std::cell::RefCell<Looks> = std::cell::RefCell::new(Looks::default());
}

pub struct BiomeBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

/// A biome's climate and surface (`comps::biome`). Terrain rules placing it start from these
/// (`BiomeRule::new("desert")` needs nothing else) and may change them there.
impl BiomeBuilder<'_> {
    /// Where field `name` of the terrain fits it; given again, the new range replaces the old.
    fn range(self, name: &str, range: impl std::ops::RangeBounds<f64>) -> Self {
        let (min, max) = crate::terrain::range_f64(range);
        self.push(&crate::comps::biome::RANGES, abi::RuleRange { name: name.to_string(), min, max })
    }
    /// Where the climate fits it, like `0.35..` or `..-0.45` (from inclusive, to exclusive);
    /// the terrain's fields are about -1.4..1.4. Without one it fits everywhere.
    pub fn temperature(self, range: impl std::ops::RangeBounds<f64>) -> Self {
        self.range("temperature", range)
    }
    pub fn humidity(self, range: impl std::ops::RangeBounds<f64>) -> Self {
        self.range("humidity", range)
    }
    /// Top block, then `depth` blocks of filler, then the terrain's stone.
    pub fn surface(self, top: &str, filler: &str, depth: i32) -> Self {
        let (top, filler) = (qualify(self.reg.ns, top), qualify(self.reg.ns, filler));
        self.set_text(&crate::comps::biome::TOP, &top)
            .set_text(&crate::comps::biome::FILLER, &filler)
            .set(&crate::comps::biome::DEPTH, depth)
    }
    /// The lowest `layers` of the filler become `block` (sandstone under sand).
    pub fn under(self, block: &str, layers: i32) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::biome::UNDER, &block)
            .set(&crate::comps::biome::UNDER_LAYERS, layers)
    }
    /// Top block for columns lower than `below` (gravel on deep sea floors).
    pub fn deep_top(self, block: &str, below: i32) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::biome::DEEP_TOP, &block)
            .set(&crate::comps::biome::DEEP_BELOW, below)
    }
    /// Plain stone above this height (bare peaks).
    pub fn bare_above(self, y: i32) -> Self {
        self.set(&crate::comps::biome::BARE_ABOVE, y)
    }
    /// A frozen biome: the water surface freezes into `block`, and it has the tag `frozen`
    /// (decorations put no flowers there).
    pub fn frozen(self, block: &str) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::biome::ICE, &block)
            .tag("frozen")
    }
    /// Rain falls as snow here: `precipitation(&precipitations::SNOW)`.
    pub fn snows(self) -> Self {
        self.precipitation(&precipitations::SNOW)
    }
    /// What falls here when a weather brings rain: `precipitations::SNOW`, or a mod's own.
    pub fn precipitation(self, p: &'static Precipitation) -> Self {
        self.set(&crate::component::PRECIPITATION, crate::component::Text::new(p.name()))
    }
    /// The color (`0xRRGGBB`) blocks tinted with `kind` take here (`tints::GRASS`, or a mod's
    /// own kind). Kinds not given keep their default.
    pub fn tint(self, kind: &'static TintKind, color: u32) -> Self {
        self.push(&crate::comps::biome::TINTS, abi::BiomeTint { kind: kind.name().to_string(), color })
    }
}

pub struct DimensionBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl DimensionBuilder<'_> {
    /// What it does to whoever is in it (`Hazard::own(..).when(..)`), besides its void 64 blocks
    /// under its floor.
    pub fn hazard(self, h: crate::Hazard) -> Self {
        self.push(&crate::comps::dimension::HAZARDS, h.0)
    }
    /// Must be a pure function of its input: it runs in parallel and must give the same cube for the
    /// same seed and position on every machine.
    pub fn generator(self, f: impl Fn(&mut Gen) + Send + Sync + 'static) -> Self {
        self.set(&crate::comps::dimension::GENERATOR, callback(move |a: &mut abi::GenArgs| f(Gen::wrap(a))))
    }
    /// The block a player starts on top of in this dimension, for a seed. Without one the engine
    /// finds open ground near 0, 0 on the dimension's columns (biomes tagged `spawn_biome`
    /// first), out of caves and trees.
    pub fn spawn_point(self, f: impl Fn(u64) -> BlockPos + Send + Sync + 'static) -> Self {
        self.set(&crate::comps::dimension::SPAWN, callback(move |a: &mut abi::SpawnArgs| a.pos = f(a.seed)))
    }
    /// Players start here and respawn here. When several mods claim it, the one loaded last
    /// wins; without any, the first dimension registered is used.
    pub fn player_spawn(self) -> Self {
        self.tag(abi::comp::dimension::PLAYER_SPAWN)
    }
    /// Blocks never update here (no neighbor updates, block ticks or growth).
    pub fn no_block_updates(self) -> Self {
        self.tag(abi::comp::dimension::NO_BLOCK_UPDATES)
    }
    /// Only what names it: ores, scatters and decorations meant for every dimension stay out
    /// (a superflat test world).
    pub fn bare(self) -> Self {
        self.tag(abi::comp::dimension::BARE)
    }
    /// Surface height and biome id of column (x, z) for (seed, x, z), for a dimension without a
    /// terrain. The generator reads them from `g.columns()` instead of working them out again,
    /// and structures, scatters (trees) and the spawn search stand on them.
    pub fn column(self, f: impl Fn(u64, i32, i32) -> (i32, u32) + Send + Sync + 'static) -> Self {
        let cb = callback(move |a: &mut abi::ColumnArgs| (a.height, a.biome) = f(a.seed, a.x, a.z));
        self.set(&crate::comps::dimension::COLUMN, cb)
    }
    /// Inclusive limits in cubes (16 blocks).
    pub fn height_cubes(self, min_cube: i32, max_cube: i32) -> Self {
        self.set(&crate::comps::dimension::CUBES, [min_cube, max_cube])
    }
    /// Everything at or above this cube y is empty, so sky light there needs no generation.
    pub fn sky_above(self, cube_y: i32) -> Self {
        self.set(&crate::comps::dimension::SKY_ABOVE, cube_y)
    }
    /// The weathers that come here now and then after clear weather, each by its weight and
    /// duration (`&[&weathers::RAIN, &weathers::THUNDER]` is vanilla's). Without, always clear.
    pub fn weathers(self, list: &[&Weather]) -> Self {
        self.set(&crate::comps::dimension::WEATHERS, list.iter().map(|w| w.name().to_string()).collect())
    }
    pub fn no_sky_light(self) -> Self {
        self.tag(abi::comp::dimension::NO_SKY_LIGHT)
    }
    /// Blocks per second squared.
    pub fn gravity(self, g: f32) -> Self {
        self.set(&crate::comps::dimension::GRAVITY, g)
    }
    pub fn day_length(self, ticks: u32) -> Self {
        self.set(&crate::comps::dimension::DAY_LENGTH, ticks)
    }
    /// Daylight through the day as points (turn of the sky, level 0..1): the turn 0 is sunrise,
    /// 0.25 noon, 0.5 sunset, 0.75 midnight, with straight lines between. Endless dusk is
    /// `&[(0.0, 0.4)]`, a long polar day `&[(0.0, 1.0), (0.6, 1.0), (0.7, 0.0), (0.95, 0.0)]`.
    /// Spawning, growth, burning in the sun and the sky all follow it.
    pub fn daylight(self, points: &[(f32, f32)]) -> Self {
        let points = points
            .iter()
            .map(|&(turn, level)| abi::DaylightPoint { turn, level })
            .collect();
        self.set(&crate::comps::dimension::DAYLIGHT, points)
    }
    pub fn fixed_time(self, time: u32) -> Self {
        self.set(&crate::comps::dimension::DAY_LENGTH, 0)
            .set(&crate::comps::dimension::FIXED_TIME, time)
    }
    pub fn sky_color(self, sky: u32, fog: u32) -> Self {
        self.set(&crate::comps::dimension::SKY_COLOR, sky)
            .set(&crate::comps::dimension::FOG_COLOR, fog)
    }
    pub fn ambient_light(self, level: u32) -> Self {
        self.set(&crate::comps::dimension::AMBIENT, level)
    }
}

/// The same limits on everything placed in the world (`comps::common`): ores, structures,
/// scatters, decorations (dimensions only) and natural spawns.
pub trait Placed: WithComponents
where
    kind::Placement: kind::On<Self::Kind>,
{
    /// Only in this dimension (call again for more). Without, in every one but bare ones.
    fn dimension(self, dimension: &Dimension) -> Self {
        self.push(&crate::comps::common::DIMENSIONS, dimension.name().to_string())
    }
    /// Only in these biomes, underground ones too: any reference to them (`#cave`,
    /// `base:plains`, `#forest !base:birch_forest`).
    fn biome(self, biomes: &str) -> Self {
        self.set_text(&crate::comps::common::BIOME, biomes)
    }
    /// Heights, inclusive: where an ore's veins start, a scatter's site, a structure's ground,
    /// a spawn spot.
    fn y(self, min: i32, max: i32) -> Self {
        self.set(&crate::comps::common::Y, [min, max])
    }
}

impl<B: WithComponents> Placed for B where kind::Placement: kind::On<B::Kind> {}

/// Layers from the bottom up, each a list of rows along z, each character one block along x:
/// a key says which block it means, a space leaves what is there, `air` clears. An edit adds its
/// blocks to the template's.
pub struct TemplateBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
    layers: Vec<Vec<String>>,
    keys: Vec<(char, String)>,
    fills: Vec<(String, BlockPos, BlockPos)>,
}

crate::entry::entry_builder!(
    Template => TemplateBuilder, template, finish;
    Palette => PaletteBuilder, palette;
    Record => RecordBuilder, record;
    Liquid => LiquidBuilder, liquid_entry, finish;
    Reaction => ReactionBuilder, reaction_entry;
);

impl TemplateBuilder<'_> {
    pub fn layer(mut self, rows: &[&str]) -> Self {
        self.layers
            .push(rows.iter().map(|r| r.to_string()).collect());
        self
    }
    /// `block` may name a state (`furnace[lit=true]`) or `air` to clear.
    pub fn key(mut self, c: char, block: &str) -> Self {
        self.keys.push((c, qualify_block(self.reg.ns, block)));
        self
    }
    /// A box of `block` from `from` to `to`, inclusive (a moving body's deck).
    pub fn fill(mut self, block: &str, from: impl Into<BlockPos>, to: impl Into<BlockPos>) -> Self {
        let from = from.into();
        self.fills
            .push((qualify_block(self.reg.ns, block), from, to.into()));
        self
    }
    /// What slot `slot` (a key `@slot` or `@slot:stairs[facing=north]`) takes unless a palette
    /// says otherwise.
    pub fn slot(self, slot: &str, block: &str) -> Self {
        let block = qualify_block(self.reg.ns, block);
        self.push(&crate::comps::template::SLOTS, abi::PaletteSlot { name: slot.to_string(), block })
    }
    /// An entity of `kind` standing at `pos` (from the lowest corner) whenever it is placed.
    pub fn entity(self, kind: &crate::EntityKind, pos: impl Into<crate::Vec3>) -> Self {
        let p = pos.into();
        self.push(
            &crate::comps::template::ENTITIES,
            abi::TemplateEntity { pos: [p.x as f32, p.y as f32, p.z as f32], kind: kind.name().to_string(), data: Vec::new() },
        )
    }
    /// The block entity at `pos` (a chest) filled from loot table `table` whenever it is placed.
    pub fn loot(self, pos: impl Into<BlockPos>, table: &crate::LootTable) -> Self {
        let p = pos.into();
        self.push(&crate::comps::template::LOOT, abi::TemplateLoot { pos: [p.x, p.y, p.z], table: table.name().to_string() })
    }
    /// A joint at `pos` facing `face` (`FACE_DIRS` index), where `Plan::grow` joins a piece from
    /// pool `pool` by its joint called `target`.
    pub fn joint(self, pos: impl Into<BlockPos>, face: usize, name: &str, target: &str, pool: &str) -> Self {
        let p = pos.into();
        let j = abi::TemplateJoint { pos: [p.x, p.y, p.z], face: face as u32, name: name.into(), target: target.into(), pool: pool.into() };
        self.push(&crate::comps::template::JOINTS, j)
    }

    /// The layers and boxes as blocks: the whole list of a new template, more for an edit.
    fn finish(&mut self) {
        let mut blocks: Vec<abi::TemplateBlock> = Vec::new();
        for (y, layer) in self.layers.iter().enumerate() {
            for (z, row) in layer.iter().enumerate() {
                for (x, ch) in row.chars().enumerate() {
                    if let Some((_, b)) = self.keys.iter().find(|k| k.0 == ch) {
                        blocks.push(abi::TemplateBlock { pos: [x as i32, y as i32, z as i32], block: b.clone() });
                    }
                }
            }
        }
        for (b, f, t) in &self.fills {
            for y in f.y.min(t.y)..=f.y.max(t.y) {
                for z in f.z.min(t.z)..=f.z.max(t.z) {
                    for x in f.x.min(t.x)..=f.x.max(t.x) {
                        blocks.push(abi::TemplateBlock { pos: [x, y, z], block: b.clone() });
                    }
                }
            }
        }
        let list = &crate::comps::template::BLOCKS;
        if self.e.mode == abi::entry_mode::EDIT {
            let ops = blocks
                .iter()
                .map(|b| element(list, b, abi::value_op::PUSH, ""));
            self.e.ops.extend(ops);
        } else {
            self.e.ops.push(list.raw_value(&blocks));
        }
    }
}

pub(crate) fn qualify_block(ns: &str, block: &str) -> String {
    if block == "air" {
        "core:air".to_string()
    } else if block.starts_with('@') {
        block.to_string()
    } else {
        qualify(ns, block)
    }
}

/// A record: nothing of its own, only the tags and components set on it.
pub struct RecordBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

/// Blocks for templates' slots.
pub struct PaletteBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl PaletteBuilder<'_> {
    /// Slot `slot` takes `block` (a state too: `base:oak_log[axis=x]`).
    pub fn slot(self, slot: &str, block: &str) -> Self {
        let block = qualify_block(self.reg.ns, block);
        self.push(&crate::comps::palette::SLOTS, abi::PaletteSlot { name: slot.to_string(), block })
    }
}

/// Generation context for one cube.
pub struct Gen {
    pub(crate) args: abi::GenArgs,
}

impl Gen {
    pub(crate) fn wrap(a: &mut abi::GenArgs) -> &mut Gen {
        unsafe { &mut *(a as *mut abi::GenArgs as *mut Gen) }
    }
    pub fn view(&self) -> crate::Generation<'_> {
        crate::Generation::new(self.args.source)
    }
    /// Read a block in world coordinates, including outside this cube and earlier local writes.
    pub fn read(&self, pos: impl Into<BlockPos>) -> u32 {
        self.view().read(pos)
    }
    pub fn seed(&self) -> u64 {
        self.args.seed
    }
    /// Cube coordinates (block coords = cube * 16).
    pub fn cube(&self) -> BlockPos {
        self.args.cube
    }
    pub fn origin(&self) -> BlockPos {
        let c = self.args.cube;
        [c.x * 16, c.y * 16, c.z * 16].into()
    }
    pub fn blocks(&mut self) -> &mut [u32; 4096] {
        if !self.args.written.is_null() {
            unsafe { std::slice::from_raw_parts_mut(self.args.written, 64).fill(u64::MAX) };
        }
        unsafe { &mut *(self.args.blocks as *mut [u32; 4096]) }
    }
    pub fn biomes(&mut self) -> &mut [u16; 256] {
        if !self.args.biomes_written.is_null() {
            unsafe { std::slice::from_raw_parts_mut(self.args.biomes_written, 4).fill(u64::MAX) };
        }
        unsafe { &mut *(self.args.biomes as *mut [u16; 256]) }
    }
    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, state: u32) {
        let i = (y * 16 + z) * 16 + x;
        assert!(i < 4096 && x < 16 && y < 16 && z < 16);
        unsafe {
            *self.args.blocks.add(i) = state;
            if !self.args.written.is_null() {
                *self.args.written.add(i / 64) |= 1 << (i % 64);
            }
        }
    }
    #[inline]
    pub fn get(&mut self, x: usize, y: usize, z: usize) -> u32 {
        let o = self.origin();
        self.read([o.x + x as i32, o.y + y as i32, o.z + z as i32])
    }
}

/// How `BlockBuilder` makes a state's `SignalSpec` from its properties.
#[derive(Clone, Copy, Default)]
struct SignalBuild {
    kind: u32,
    attaches: bool,
    delay: Option<u32>,
    flags: u32,
    /// A gate's faces (`Side` bits).
    inputs: u32,
    outputs: u32,
    head: &'static str,
}

impl SignalBuild {
    fn apply<'s>(&self, p: &Props, mut s: State<'s>) -> State<'s> {
        use abi::signal_kind as k;
        let dir = |name: &str| crate::Dir::from_name(p.get(name));
        // Faces as the model is written: what `facing` and `attach` name, turned back.
        let turn = s.turn();
        let own = |d: crate::Dir| 1u32 << turn.own(d).face();
        let mut sig = abi::SignalDesc { kind: self.kind, flags: self.flags, head: self.head.to_string(), ..abi::SignalDesc::DEFAULT };
        let (attach, facing) = (dir("attach"), dir("facing"));
        match self.kind {
            k::SWITCH => (sig.gives, sig.strong) = (0x3F, attach.map_or(0, own)),
            k::TORCH => {
                // Off with power from what holds it, strong into every other block around it: on a
                // wall both out and up, the same whichever way a build is turned.
                sig.inputs = attach.map_or(side_bits(&[Side::Bottom]), own);
                (sig.gives, sig.strong) = (0x3F & !sig.inputs, 0x3F & !sig.inputs);
            }
            k::REPEATER => {
                sig.inputs = side_bits(&[Side::Rear]);
                sig.locks = side_bits(&[Side::Left, Side::Right]);
                (sig.gives, sig.strong) = (side_bits(&[Side::Front]), side_bits(&[Side::Front]));
            }
            k::COMPARATOR => {
                sig.inputs = side_bits(&[Side::Rear, Side::Left, Side::Right]);
                (sig.gives, sig.strong) = (side_bits(&[Side::Front]), side_bits(&[Side::Front]));
            }
            k::GATE => (sig.inputs, sig.gives, sig.strong) = (self.inputs, self.outputs, self.outputs),
            k::OBSERVER => {
                let f = facing.unwrap_or(crate::Dir::North);
                (sig.watches, sig.gives, sig.strong) = (own(f), own(f.opposite()), own(f.opposite()));
            }
            k::SINK => sig.inputs = 0x3F,
            // Their front is stuck to what they take along, or is where the head goes.
            k::PISTON | k::FLOATER | k::PISTON_HEAD => {
                let f = turn.own(facing.unwrap_or(crate::Dir::Up)).face();
                sig.front = f;
                sig.inputs = if self.kind == k::PISTON_HEAD { 0 } else { 0x3F & !(1 << f) };
            }
            _ => {}
        }
        if let Some(a) = attach.filter(|_| self.attaches) {
            s = s.needs(turn.own(a), "");
        }
        if let Some(t) = self.delay {
            sig.delay = t;
        }
        // A `delay` is in redstone ticks, two game ticks each.
        if p.int("delay") > 0 {
            sig.delay = p.int("delay") * 2;
        }
        if p.is("mode", "subtract") {
            sig.flags |= abi::signal_flags::SUBTRACT;
        }
        s = s.set(&bc::SIGNAL, sig);
        if self.kind == abi::signal_kind::WIRE {
            let mask = (0..6)
                .filter(|&f| p.is(abi::FACE_NAMES[f], "true"))
                .fold(0, |m, f| m | 1 << f);
            // A thin plate on each face it lies on.
            const PLATES: [([f32; 3], [f32; 3]); 6] = [
                ([0.0, 0.0, 0.0], [1.0, 16.0, 16.0]),
                ([15.0, 0.0, 0.0], [16.0, 16.0, 16.0]),
                ([0.0, 0.0, 0.0], [16.0, 1.0, 16.0]),
                ([0.0, 15.0, 0.0], [16.0, 16.0, 16.0]),
                ([0.0, 0.0, 0.0], [16.0, 16.0, 1.0]),
                ([0.0, 0.0, 15.0], [16.0, 16.0, 16.0]),
            ];
            let flat = |(a, b): ([f32; 3], [f32; 3])| [a[0], a[1], a[2], b[0], b[1], b[2]];
            let mut plates: Vec<[f32; 6]> = (0..6)
                .filter(|f| mask & (1 << f) != 0)
                .map(|f| flat(PLATES[f as usize]))
                .collect();
            if mask == 0 {
                plates = vec![flat(PLATES[2])];
            }
            s = s.set(&bc::HITBOX, plates);
        }
        s
    }
}

/// What `BlockBuilder::on_signal` is told.
pub struct SignalEvent {
    args: abi::SignalArgs,
}

impl SignalEvent {
    fn wrap(a: &mut abi::SignalArgs) -> &mut SignalEvent {
        unsafe { &mut *(a as *mut abi::SignalArgs as *mut SignalEvent) }
    }
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn pos(&self) -> BlockPos {
        self.args.pos
    }
    pub fn state(&self) -> u32 {
        self.args.state
    }
    /// Power around it now, 0..15.
    pub fn level(&self) -> u8 {
        self.args.level as u8
    }
    /// What it was.
    pub fn level_before(&self) -> u8 {
        self.args.before as u8
    }
    /// It just got power.
    pub fn rising(&self) -> bool {
        self.args.before == 0 && self.args.level > 0
    }
}

/// A device's faces, from where it looks: its own, as its model is written (front north, bottom
/// down), so they turn with it (`Gate::inputs`, `GateInputs::side`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left = 0,
    Right = 1,
    /// Against what it is on.
    Bottom = 2,
    Top = 3,
    Front = 4,
    Rear = 5,
}

fn side_bits(sides: &[Side]) -> u32 {
    sides.iter().fold(0, |m, s| m | 1 << *s as u32)
}

/// Which faces of a `BlockBuilder::signal_gate` read power and give it (strongly, into the block
/// there and on), and its delay.
///
/// ```ignore
/// // a repeater-like AND of the sides, two redstone ticks late
/// .signal_gate(Gate::new().inputs(&[Side::Left, Side::Right]).delay(2), |g| if g.side(Side::Left) > 0 && g.side(Side::Right) > 0 { 15 } else { 0 })
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    inputs: u32,
    outputs: u32,
    delay: u32,
}

impl Gate {
    /// Reads behind it and both sides, gives out of the front, one redstone tick late.
    pub fn new() -> Gate {
        Gate { inputs: side_bits(&[Side::Rear, Side::Left, Side::Right]), outputs: side_bits(&[Side::Front]), delay: 1 }
    }
    /// The faces it reads; an output is never an input.
    pub fn inputs(mut self, sides: &[Side]) -> Gate {
        self.inputs = side_bits(sides) & !self.outputs;
        self
    }
    /// The faces it gives at.
    pub fn outputs(mut self, sides: &[Side]) -> Gate {
        self.outputs = side_bits(sides);
        self.inputs &= !self.outputs;
        self
    }
    /// In redstone ticks, two game ticks each.
    pub fn delay(mut self, ticks: u32) -> Gate {
        self.delay = ticks;
        self
    }
}

impl Default for Gate {
    fn default() -> Gate {
        Gate::new()
    }
}

/// What comes in at a `BlockBuilder::signal_gate`'s faces.
pub struct GateInputs {
    args: abi::GateArgs,
}

impl GateInputs {
    fn wrap(a: &mut abi::GateArgs) -> &mut GateInputs {
        unsafe { &mut *(a as *mut abi::GateArgs as *mut GateInputs) }
    }
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn pos(&self) -> BlockPos {
        self.args.pos
    }
    /// Power coming in at `side`, 0 for a face it doesn't read.
    pub fn side(&self, side: Side) -> u8 {
        self.args.inputs[side as usize] as u8
    }
    /// What it gives now.
    pub fn output(&self) -> u8 {
        self.args.output as u8
    }
}

pub struct BlockEvent {
    args: abi::BlockEventArgs,
}

impl BlockEvent {
    fn wrap(a: &mut abi::BlockEventArgs) -> &mut BlockEvent {
        unsafe { &mut *(a as *mut abi::BlockEventArgs as *mut BlockEvent) }
    }
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn pos(&self) -> BlockPos {
        self.args.pos
    }
    pub fn state(&self) -> u32 {
        self.args.state
    }
    /// The neighbor that changed, for `on_neighbor`.
    pub fn neighbor(&self) -> BlockPos {
        self.args.other
    }
    /// Where the block is on the body, for `on_carried`.
    pub fn body_pos(&self) -> BlockPos {
        self.args.other
    }
    pub fn entity(&self) -> u64 {
        self.args.entity
    }
}

pub struct Use {
    args: abi::UseArgs,
}

impl Use {
    fn wrap(a: &mut abi::UseArgs) -> &mut Use {
        unsafe { &mut *(a as *mut abi::UseArgs as *mut Use) }
    }
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn pos(&self) -> BlockPos {
        self.args.pos
    }
    pub fn state(&self) -> u32 {
        self.args.state
    }
    /// Face of the block that was clicked.
    pub fn face(&self) -> crate::Dir {
        crate::Dir::from_face(self.args.face)
    }
    pub fn player(&self) -> u64 {
        self.args.player
    }
    /// The player sneaks: only with `.sneak_use(..)` saying so, or with empty hands. Returning
    /// false passes the click on to what it holds.
    pub fn sneaking(&self) -> bool {
        self.args.sneaking != 0
    }
}

pub struct Place {
    args: abi::PlaceArgs,
}

impl Place {
    fn wrap(a: &mut abi::PlaceArgs) -> &Place {
        unsafe { &*(a as *mut abi::PlaceArgs as *const Place) }
    }
    pub fn pos(&self) -> BlockPos {
        self.args.pos
    }
    /// Face of the block that was clicked.
    pub fn face(&self) -> crate::Dir {
        crate::Dir::from_face(self.args.face)
    }
    pub fn look_dir(&self) -> crate::Vec3 {
        vec3(self.args.look)
    }
    /// Just outside where it was clicked, 0..1 from this cell's corner: for a slab, `hit().y > 0.5`
    /// is the upper half, on a floor, under a ceiling, against a wall or on another slab alike.
    pub fn hit(&self) -> crate::Vec3 {
        vec3(self.args.hit)
    }
    /// In `merge`: a property of how its item would go.
    pub fn placed(&self, prop: &str) -> Option<&'static str> {
        crate::world::state_prop(self.args.placed, prop)
    }
}

fn vec3(v: [f32; 3]) -> crate::Vec3 {
    crate::Vec3 { x: v[0] as f64, y: v[1] as f64, z: v[2] as f64 }
}

pub struct ItemUse {
    args: abi::ItemUseArgs,
}

impl ItemUse {
    fn wrap(a: &mut abi::ItemUseArgs) -> &mut ItemUse {
        unsafe { &mut *(a as *mut abi::ItemUseArgs as *mut ItemUse) }
    }
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    /// Who uses it: a player, or a mob (`World::use_item`).
    pub fn entity(&self) -> u64 {
        self.args.entity
    }
    pub fn target_block(&self) -> Option<(BlockPos, u32)> {
        (self.args.has_target != 0).then_some((self.args.pos, self.args.face))
    }
    /// Ticks the use button was held, for `on_release` (a bow's draw); 0 for a use at once.
    pub fn held_ticks(&self) -> u32 {
        self.args.held_ticks
    }
    pub fn stack(&self) -> crate::ItemStack {
        self.args.stack
    }
    /// The slot of its inventory the stack is in.
    pub fn slot(&self) -> u32 {
        self.args.slot
    }
    /// The used stack as a holder: what a backpack opens its menu on.
    pub fn holder(&self) -> crate::Holder {
        crate::Holder::held(self.args.entity, self.args.slot)
    }
    /// Replaces the held stack (use count 0 to consume it entirely).
    pub fn set_stack(&mut self, s: crate::ItemStack) {
        self.args.stack = s;
    }
}

/// A liquid: its block (16 states) and flow rules, optionally a filled bucket item. Its own
/// components are its flow (`comps::liquid`); its block's (tags, particles) go in `.block(..)`,
/// and the block's tags are its item's too.
///
/// ```ignore
/// r.liquid(&OIL).block(|b| b.light_opacity(4).tag("oil")).flow_delay(15).drop_off(2).bucket(&OIL_BUCKET, &BUCKET, &TOOLS);
/// r.liquid_reaction("#hot", "#water").source_turns_into("obsidian").flowing_turns_into("cobblestone").below_turns_into("stone");
/// ```
pub struct LiquidBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
    block: &'static Block,
    look: Option<BlockLook>,
    bucket: Option<(&'static Item, String, String)>,
}

impl LiquidBuilder<'_> {
    /// Its block, on top of what makes it a liquid (texture `<name>_still`, drawn solid like
    /// lava): `.block(|b| b.tint(&tints::WATER).translucent().drag(0.4).light(15).tag("water"))`.
    /// In an edit, what it changes of the block (and the tags of its item).
    pub fn block(mut self, f: impl for<'r> FnOnce(BlockBuilder<'r>) -> BlockBuilder<'r> + 'static) -> Self {
        self.look = Some(Box::new(f));
        self
    }
    /// Ticks between flow steps (water 5, lava 30).
    pub fn flow_delay(self, ticks: u32) -> Self {
        self.set(&crate::comps::liquid::DELAY, ticks)
    }
    /// Level lost per block spread (water 1: 7 blocks, lava 2: 3 blocks).
    pub fn drop_off(self, n: u32) -> Self {
        self.set(&crate::comps::liquid::DROP_OFF, n)
    }
    /// How far it looks for a way down (water 4, lava 2).
    pub fn slope_find(self, n: u32) -> Self {
        self.set(&crate::comps::liquid::SLOPE_FIND, n)
    }
    /// New sources form where at least `sources` sources are beside a spot over solid ground
    /// (vanilla water 2). 0: always, one bucket fills everything up to its level; more than the
    /// sides there are: never, as without this call.
    pub fn renewable(self, sources: u32) -> Self {
        self.set(&crate::comps::liquid::RENEWABLE, sources)
    }
    /// Rises instead of falling.
    pub fn flows_up(self) -> Self {
        self.set(&crate::comps::liquid::DENSITY, -1000)
    }
    /// Roughly Kelvin; 1000 and more burns what touches it.
    pub fn heat(self, t: u32) -> Self {
        self.set(&crate::comps::liquid::HEAT, t)
    }
    /// A camera environment preset: color (0xRRGGBB), start and end in blocks.
    /// Defaults to the average texture color, start -8 and end 96. Acoustics are independent.
    pub fn fog(self, rgb: u32, start: f32, end: f32) -> Self {
        let mut layer = crate::Environment::new("core:liquid_fog")
            .priority(-699)
            .fog(rgb, start, end);
        if rgb == u32::MAX {
            layer.fog_from_block = true;
        }
        self.environment(layer)
    }
    /// The picture over the screen from inside, relative names are under `block/`, and how much
    /// of it shows. By default its own texture at 0.1; for an inside that looks unlike the surface, set
    /// this and `fog` apart from its texture.
    pub fn overlay(self, texture: &str, opacity: f32) -> Self {
        let texture = crate::asset("block", texture);
        let mut layer = crate::Environment::new("core:liquid_overlay")
            .priority(-698)
            .replace_overlays();
        let block_face = if texture.is_empty() { 0 } else { -1 };
        layer.overlays.push(abi::environment::EnvironmentOverlay {
            texture,
            color: [1.0, 1.0, 1.0, opacity],
            tiles: 1.0,
            block_face,
            lit: true,
            ..Default::default()
        });
        self.environment(layer)
    }
    /// Nothing over the screen from inside.
    pub fn no_overlay(self) -> Self {
        self.environment(
            crate::Environment::new("core:liquid_overlay")
                .priority(-698)
                .replace_overlays(),
        )
    }
    /// Also registers `item`: `empty` (a container item, any mod's) filled with this liquid, in
    /// creative tab `tab` (any mod's). The engine fills and pours it; no code needed.
    pub fn bucket(mut self, item: &'static Item, empty: &Item, tab: &CreativeTab) -> Self {
        self.bucket = Some((item, empty.name().to_string(), tab_tag(tab)));
        self
    }

    /// Its block and item: registered with a new liquid, edited where `.block(..)` says so.
    fn finish(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let name = self.e.name.clone();
        let new = self.e.mode == abi::entry_mode::NEW;
        if new || self.look.is_some() {
            let look = self.look.take().unwrap_or_else(|| Box::new(|b| b));
            // The engine makes its 16 states liquid (flags, look, level) from its entry.
            let tex = format!("{}_still", self.block.path());
            let texture = new.then(|| crate::asset("block", &tex));
            let b = if new {
                look(
                    self.reg
                        .block(self.block)
                        .texture(&tex)
                        .prop("level", 0..=15)
                        .unbreakable()
                        .light_opacity(2)
                        .no_item()
                        .drops_nothing(),
                )
            } else {
                look(self.reg.edit(self.block))
            };
            // The same tags on its item: `#water` in a recipe is the liquid too.
            let tags: Vec<Pending> =
                b.e.ops
                    .iter()
                    .filter(|p| p.is_tag() && !p.name.starts_with("block."))
                    .cloned()
                    .collect();
            drop(b);
            let item = Item::generated(name.clone());
            let mut it = match texture {
                Some(texture) => self
                    .reg
                    .item(item)
                    .set_text(&c::FLUID, &name)
                    .max_stack(u32::MAX)
                    .set_text(&crate::comps::item::LAYERS, &texture),
                None => self.reg.edit(item),
            };
            it.e.ops.extend(tags);
        }
        if let Some((item, empty, tab)) = self.bucket.take() {
            self.reg
                .item(item)
                .max_stack(1)
                .set_text(&c::LIQUID, &name)
                .set_text(&c::REMAINDER, &empty)
                .tag(&tab);
        }
    }
}

/// What happens where two liquids meet, named `<mod>:<liquid>><touching>` (`base:hot>water`)
/// unless `named`.
pub struct ReactionBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl ReactionBuilder<'_> {
    /// Its name instead of `<mod>:<liquid>><touching>`.
    pub fn named(mut self, name: &str) -> Self {
        let derived = std::mem::replace(&mut self.e.name, qualify(self.reg.ns, name));
        self.reg.derived.retain(|n| *n != derived);
        self
    }
    /// What a source of the liquid becomes.
    pub fn source_turns_into(self, block: &str) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::reaction::SOURCE, &block)
    }
    /// What flowing liquid becomes.
    pub fn flowing_turns_into(self, block: &str) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::reaction::FLOWING, &block)
    }
    /// What the touched liquid becomes when this one flows down into it.
    pub fn below_turns_into(self, block: &str) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::reaction::FALLS_INTO, &block)
    }
}

impl Registry {
    /// A liquid the engine flows; see `LiquidBuilder`. Defaults are vanilla water's numbers, an
    /// average of its texture for the fog's color.
    pub fn liquid(&mut self, liquid: &'static Block) -> LiquidBuilder<'_> {
        LiquidBuilder { e: crate::entry::Entry::of(liquid.liquid()), reg: self, block: liquid, look: None, bucket: None }
    }

    fn liquid_entry(&mut self, liquid: &'static Liquid) -> LiquidBuilder<'_> {
        let block = Block::generated(liquid.name().to_string());
        LiquidBuilder { e: crate::entry::Entry::of(liquid), reg: self, block, look: None, bucket: None }
    }

    /// When liquid `liquid` touches `touching` (block matchers), see `ReactionBuilder`.
    pub fn liquid_reaction(&mut self, liquid: &str, touching: &str) -> ReactionBuilder<'_> {
        let path = |n: &str| {
            let n = n.trim_start_matches('#');
            n.split_once(':').map_or(n, |x| x.1).to_string()
        };
        let name = self.derive_name(&format!("{}>{}", path(liquid), path(touching)));
        ReactionBuilder { reg: self, e: crate::entry::Entry::new(abi::entry_kind::REACTION, name) }
            .set_text(&crate::comps::reaction::LIQUID, &liquid)
            .set_text(&crate::comps::reaction::TOUCHING, &touching)
    }

    fn reaction_entry(&mut self, reaction: &'static Reaction) -> ReactionBuilder<'_> {
        ReactionBuilder { reg: self, e: crate::entry::Entry::of(reaction) }
    }
}

/// Liquid id of a block state (`None` for other blocks).
pub fn liquid_of(state: u32) -> Option<u32> {
    let l = unsafe { (host().liquid_of)(state) };
    (l != u32::MAX).then_some(l)
}

#[cfg(test)]
mod order_tests {
    use super::*;

    #[test]
    fn a_state_knows_its_turn() {
        let f = crate::Orientation::new(crate::Dir::Down, crate::Dir::South);
        let (mut ops, faces) = (Vec::new(), Default::default());
        let s = State::new(&mut ops, &faces)
            .orientation(f)
            .model_box([0.0; 3], [16.0, 16.0, 1.0]);
        assert_eq!(s.turn(), f);
        assert_eq!(s.turn().own(crate::Dir::South), crate::Dir::North);
    }
}
