//! Hooking into what happens without replacing anything:
//!
//! ```ignore
//! // Notifications after the fact, batched once per tick:
//! pub static EATEN: Event<Eaten> = Event::named("eaten");
//! r.on_event(&EATEN, |b| for e in b.iter() { b.world().damage(..) });
//! w.emit(&EATEN, Eaten { .. });
//! // May it happen? Each answer is no, keep or yes, the last that isn't keep wins:
//! r.before_event(&actions::BREAK_BLOCK, |q| if q.pos()[1] < 0 { Verdict::Deny } else { Verdict::Abstain });
//! // Values computed step by step in phases, explainable; attributes and questions are chains too:
//! r.modify(&chain::DAMAGE, &phase::MULTIPLY, |m| m.value *= 0.5);
//! r.modify(&attribute::MOVEMENT_SPEED, &phase::ADD, |m| m.value += 0.1);
//! // Loot tables any mod can change:
//! r.loot(&OAK_LEAVES).pool(Pool::new().when(Cond::chance(0.05)).item("sapling"));
//! r.edit(base::ZOMBIE.loot()).pool(Pool::new().item("mymod:brain"));
//! ```

use crate::abi::BlockPos;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::abi::{self, ItemStack, Slice, Str};
use crate::component::Value;
use crate::{
    Attribute, Chain, LootSource, LootTable, Phase, Question, Registry, Step, WithComponents, World, callback, comps, host, qualify,
};

pub use abi::{
    BlockChange, ChatMessage, Death, DimensionChange, EffectChange, Hurt, Interaction, ItemUsed, Pickup, PlayerEvent, Removed, Spawned,
    permission, removal, spawn_cause,
};

/// The engine's phases, in the order they run (`Registry::phase` puts a mod's after one).
pub mod phase {
    use crate::Phase;
    /// What the thing is: a weapon's own damage, a set base.
    pub static BASE: Phase = Phase::named(crate::abi::phase::BASE);
    pub static ADD: Phase = Phase::named(crate::abi::phase::ADD);
    /// Shares of the value the phase started from, added up (vanilla's percent modifiers).
    pub static PERCENT: Phase = Phase::named(crate::abi::phase::PERCENT);
    pub static MULTIPLY: Phase = Phase::named(crate::abi::phase::MULTIPLY);
    /// Where questions are answered unless said otherwise.
    pub static RULES: Phase = Phase::named(crate::abi::phase::RULES);
    pub static OVERRIDE: Phase = Phase::named(crate::abi::phase::OVERRIDE);
    /// For what has to see the finished value.
    pub static FINAL: Phase = Phase::named(crate::abi::phase::FINAL);
}

/// The engine's steps that take a modifier's number: `ADD` adds it, `MUL` multiplies by it,
/// `SET` makes the value it, `PERCENT` adds that share of what the phase started from.
pub mod step {
    use crate::Step;
    pub static ADD: Step = Step::named(crate::abi::step::ADD);
    pub static MUL: Step = Step::named(crate::abi::step::MUL);
    pub static SET: Step = Step::named(crate::abi::step::SET);
    pub static PERCENT: Step = Step::named(crate::abi::step::PERCENT);
}

/// What a step can change: a chain, an attribute, a question (an action's included).
pub trait Valued {
    fn chain_name(&self) -> &str;
}

impl Valued for Chain {
    fn chain_name(&self) -> &str {
        self.name()
    }
}

impl Valued for Attribute {
    fn chain_name(&self) -> &str {
        self.name()
    }
}

impl Valued for Question {
    fn chain_name(&self) -> &str {
        self.name()
    }
}

impl<T: Value> Valued for Action<T> {
    fn chain_name(&self) -> &str {
        self.ask.name()
    }
}

/// The engine's questions with no event after them (`Registry::before_event`, `World::ask`); the
/// others are `actions`.
pub mod question {
    use crate::Question;
    /// `entity` attacking `other` with `item`.
    pub static ATTACK: Question = Question::named(crate::abi::question::ATTACK);
    /// Player `entity` turning on a camera mode with conditions (`.when(..)`), which one
    /// `Ask::is_camera` tells. `Allow` lets it where they wouldn't.
    pub static CAMERA: Question = Question::named(crate::abi::action::CAMERA);
    /// Cubes kept loaded (`World::keep_loaded`) from block `pos`, `amount` the mode
    /// (`force_load::*`), `other` the ticket's key: deny one in someone else's claim.
    pub static FORCE_LOAD: Question = Question::named(crate::abi::action::FORCE_LOAD);
    /// Whether a melee hit (`entity` on `other` with `item`, swing `charge`) is a critical one;
    /// vanilla's test is the step `core:vanilla_crit`.
    pub static CRIT: Question = Question::named(crate::abi::question::CRIT);
    /// Whether the hit also sweeps those beside the target (`core:vanilla_sweep`).
    pub static SWEEP: Question = Question::named(crate::abi::question::SWEEP);
    /// Whether `entity` is still recovering from its last hurt, so this one only lands what
    /// goes beyond it and doesn't knock (`core:hurt_cooldown`). Deny it for a weapon that
    /// ignores the pause.
    pub static RECOVERING: Question = Question::named(crate::abi::question::RECOVERING);
}

/// The engine's value chains (`Registry::modify`, `World::chain_compute`).
pub mod chain {
    use crate::Chain;
    /// Damage an entity takes (`entity` hurt by `other` through `direct`, `cause`, `crit`):
    /// `core:resistance`, `core:not_lethal`.
    pub static DAMAGE: Chain = Chain::named(crate::abi::chain::DAMAGE);
    /// What a melee hit does from the attacker's `attribute::ATTACK_DAMAGE` against the target:
    /// `core:charge` (0.2 + 0.8 x charge squared), `core:crit` (times `CRIT_DAMAGE`).
    pub static MELEE: Chain = Chain::named(crate::abi::chain::MELEE);
    /// How hard a hurt knocks (0.4).
    pub static KNOCKBACK: Chain = Chain::named(crate::abi::chain::KNOCKBACK);
    /// What a sweep does to those beside the target, from the hit's damage.
    pub static SWEEP_DAMAGE: Chain = Chain::named(crate::abi::chain::SWEEP_DAMAGE);
    pub static HEAL: Chain = Chain::named(crate::abi::chain::HEAL);
    /// What a projectile does to what it hits, from its damage: times its speed, rounded up.
    pub static PROJECTILE_DAMAGE: Chain = Chain::named(crate::abi::chain::PROJECTILE_DAMAGE);
    /// What a fall does, from how far (`amount`): past `SAFE_FALL`, times
    /// `FALL_DAMAGE_MULTIPLIER`. Hay or a trampoline is a step reading what is under the faller.
    pub static FALL_DAMAGE: Chain = Chain::named(crate::abi::chain::FALL_DAMAGE);
}

/// Attributes the engine reads.
pub mod attribute {
    use crate::Attribute;
    pub static MAX_HEALTH: Attribute = Attribute::named(crate::abi::attribute::MAX_HEALTH);
    /// Multiplier on walking speed (1 = the kind's speed).
    pub static MOVEMENT_SPEED: Attribute = Attribute::named(crate::abi::attribute::MOVEMENT_SPEED);
    /// Multiplier on how far mobs notice it (1; 0 in creative).
    pub static VISIBILITY: Attribute = Attribute::named(crate::abi::attribute::VISIBILITY);
    /// Multiplier on jump speed, players and mobs (1).
    pub static JUMP_STRENGTH: Attribute = Attribute::named(crate::abi::attribute::JUMP_STRENGTH);
    /// Multiplier on how fast a swing recharges (1).
    pub static ATTACK_SPEED: Attribute = Attribute::named(crate::abi::attribute::ATTACK_SPEED);
    /// Below 1: can't sprint (1).
    pub static CAN_SPRINT: Attribute = Attribute::named(crate::abi::attribute::CAN_SPRINT);
    /// How far a player reaches blocks (4.5; 5 in creative).
    pub static BLOCK_REACH: Attribute = Attribute::named(crate::abi::attribute::BLOCK_REACH);
    /// How far a player reaches entities (3; 5 in creative).
    pub static ENTITY_REACH: Attribute = Attribute::named(crate::abi::attribute::ENTITY_REACH);
    /// Multiplier on how fast a player mines (1).
    pub static MINING_SPEED: Attribute = Attribute::named(crate::abi::attribute::MINING_SPEED);
    /// Multiplier on a player's field of view (1): a zoom, a speed rush.
    pub static FOV: Attribute = Attribute::named(crate::abi::attribute::FOV);
    /// 1 or more: may fly, jumping twice (0; 1 in creative).
    pub static CAN_FLY: Attribute = Attribute::named(crate::abi::attribute::CAN_FLY);
    /// 1: no damage, hunger or air but from what pierces it (creative, a shield effect).
    pub static INVULNERABLE: Attribute = Attribute::named(crate::abi::attribute::INVULNERABLE);
    /// Blocks it falls without harm (3).
    pub static SAFE_FALL: Attribute = Attribute::named(crate::abi::attribute::SAFE_FALL);
    /// Damage per block fallen past `SAFE_FALL` (1).
    pub static FALL_DAMAGE_MULTIPLIER: Attribute = Attribute::named(crate::abi::attribute::FALL_DAMAGE_MULTIPLIER);
    /// Blocks it walks up without jumping (0.6).
    pub static STEP_HEIGHT: Attribute = Attribute::named(crate::abi::attribute::STEP_HEIGHT);
    /// Multiplier on its dimension's gravity (1; below 0 floats up).
    pub static GRAVITY: Attribute = Attribute::named(crate::abi::attribute::GRAVITY);
    /// Share of knockback shrugged off (0..1).
    pub static KNOCKBACK_RESISTANCE: Attribute = Attribute::named(crate::abi::attribute::KNOCKBACK_RESISTANCE);
    /// Its own melee damage as vanilla's (1; a kind's `.attack_damage(..)`), what it holds adding
    /// to it.
    pub static ATTACK_DAMAGE: Attribute = Attribute::named(crate::abi::attribute::ATTACK_DAMAGE);
    /// Share of damage taken off after armor (0..1).
    pub static DAMAGE_RESISTANCE: Attribute = Attribute::named(crate::abi::attribute::DAMAGE_RESISTANCE);
    /// Share of burning damage taken off (0..1).
    pub static FIRE_RESISTANCE: Attribute = Attribute::named(crate::abi::attribute::FIRE_RESISTANCE);
    /// Multiplier on how long fire keeps it burning (1).
    pub static BURNING_TIME: Attribute = Attribute::named(crate::abi::attribute::BURNING_TIME);
    /// Multiplier on mining speed with its head in a liquid (0.2).
    pub static SUBMERGED_MINING_SPEED: Attribute = Attribute::named(crate::abi::attribute::SUBMERGED_MINING_SPEED);
    /// Multiplier on mining speed off the ground (0.2).
    pub static AIRBORNE_MINING_SPEED: Attribute = Attribute::named(crate::abi::attribute::AIRBORNE_MINING_SPEED);
    /// How much its player sees as if everything were lit (0..1).
    pub static NIGHT_VISION: Attribute = Attribute::named(crate::abi::attribute::NIGHT_VISION);
    /// Multiplier on how fast it moves itself in liquids (1).
    pub static SWIM_SPEED: Attribute = Attribute::named(crate::abi::attribute::SWIM_SPEED);
    /// Multiplier on how fast it flies (1).
    pub static FLYING_SPEED: Attribute = Attribute::named(crate::abi::attribute::FLYING_SPEED);
    /// What a critical hit multiplies by (1.5).
    pub static CRIT_DAMAGE: Attribute = Attribute::named(crate::abi::attribute::CRIT_DAMAGE);
    /// Share of a hit's damage a sweep adds to its 1 (0).
    pub static SWEEPING_DAMAGE_RATIO: Attribute = Attribute::named(crate::abi::attribute::SWEEPING_DAMAGE_RATIO);
    /// Extra knockback of its melee hits (0).
    pub static ATTACK_KNOCKBACK: Attribute = Attribute::named(crate::abi::attribute::ATTACK_KNOCKBACK);
    /// Ticks after a hurt in which a weaker one doesn't land (10).
    pub static HURT_COOLDOWN: Attribute = Attribute::named(crate::abi::attribute::HURT_COOLDOWN);
}

/// A step with a number on an attribute, that comes with something (`.modifier(..)` on an item,
/// a pose, an entity kind, an effect) or is put on an entity (`World::attribute_add_modifier`):
/// `Modifier::add(&attribute::MOVEMENT_SPEED, 0.2)`, while worn (`.worn()`) or while conditions
/// hold for its holder (`.when(..)`). Any code of your own is a step too: `Modifier::step(&A,
/// &MY_STEP, 2.0)` runs `MY_STEP` (`Registry::step`) with 2.0 as `Modify::arg`.
#[derive(Clone)]
pub struct Modifier(pub(crate) abi::Modifier);

impl Modifier {
    pub fn step(attribute: &crate::Attribute, step: &Step, amount: f64) -> Modifier {
        Modifier(abi::Modifier { attribute: attribute.name().to_string(), step: step.name().to_string(), amount, ..Default::default() })
    }
    pub fn add(attribute: &crate::Attribute, amount: f64) -> Modifier {
        Modifier::step(attribute, &step::ADD, amount)
    }
    pub fn mul(attribute: &crate::Attribute, factor: f64) -> Modifier {
        Modifier::step(attribute, &step::MUL, factor)
    }
    /// The value becomes `value`, at the end (`phase::OVERRIDE`) unless `.phase(..)` says.
    pub fn set(attribute: &crate::Attribute, value: f64) -> Modifier {
        Modifier::step(attribute, &step::SET, value)
    }
    /// Adds `share` of the value its phase started from: vanilla's percent modifiers, which add
    /// up (speed and slowness together are +5%, not 1.2 x 0.85).
    pub fn percent(attribute: &crate::Attribute, share: f64) -> Modifier {
        Modifier::step(attribute, &step::PERCENT, share)
    }
    /// Runs in `phase` rather than its step's own.
    pub fn phase(mut self, phase: &Phase) -> Modifier {
        self.0.phase = phase.name().to_string();
        self
    }
    /// An item's, while it is in an equipment slot rather than in the main hand.
    pub fn worn(mut self) -> Modifier {
        self.0.worn = true;
        self
    }
    pub fn when(mut self, c: impl Into<Cond>) -> Modifier {
        self.0.when.push(c.into().0);
        self
    }
}

const UNRESOLVED: u32 = u32::MAX - 1;

/// A typed event, named like any other thing a mod owns: `Event::named("eaten")` is this mod's,
/// `Event::named("base:eaten")` another's.
pub struct Event<T: Value> {
    name: &'static str,
    full: std::sync::OnceLock<String>,
    id: AtomicU32,
    _p: PhantomData<fn() -> T>,
}

impl<T: Value> Event<T> {
    pub const fn named(name: &'static str) -> Self {
        Event { name, full: std::sync::OnceLock::new(), id: AtomicU32::new(UNRESOLVED), _p: PhantomData }
    }
    pub fn name(&self) -> &str {
        self.full.get_or_init(|| crate::full_name(self.name))
    }
    /// `u32::MAX` if the event is unknown or registries are not ready yet.
    pub fn id(&self) -> u32 {
        let id = self.id.load(Ordering::Relaxed);
        if id != UNRESOLVED {
            return id;
        }
        if unsafe { (host().component_table)() }.is_null() {
            return u32::MAX;
        }
        let id = unsafe { (host().find_event)(Str::new(self.name()), Str::new(&T::schema())) };
        self.id.store(id, Ordering::Relaxed);
        id
    }
}

macro_rules! schema_values {
    ($($t:ty),*) => {$(
        unsafe impl Value for $t {}
    )*};
}
schema_values!(
    ItemStack,
    abi::ChatMessage,
    abi::BlockChange,
    abi::Death,
    abi::Hurt,
    abi::PlayerEvent,
    abi::Pickup,
    abi::ItemUsed,
    abi::Interaction,
    abi::Mounting,
    abi::DimensionChange,
    abi::WeatherTick,
    abi::Spawned,
    abi::Removed,
    abi::ProjectileHit,
    abi::KeyChange,
    abi::EffectChange,
    abi::FeaturePlaced
);

/// The engine's events with no question before them; the others are `actions`.
/// A block's own tick, by a timer on it (`schedule_tick`).
pub static BLOCK_TICK: Event<[i32; 3]> = Event::named(abi::event::BLOCK_TICK);
pub static PLAYER_JOINED: Event<abi::PlayerEvent> = Event::named(abi::event::PLAYER_JOINED);
/// A dead player came back at the spawn point, components reset but the `KEEP_ON_RESPAWN` ones;
/// moving them on from here (a bed) happens before anyone saw them there.
pub static PLAYER_RESPAWNED: Event<abi::PlayerEvent> = Event::named(abi::event::PLAYER_RESPAWNED);
pub static ENTITY_HEALED: Event<abi::Hurt> = Event::named(abi::event::ENTITY_HEALED);
/// Every tick in each dimension with weather: which it is.
pub static WEATHER_TICK: Event<abi::WeatherTick> = Event::named(abi::event::WEATHER_TICK);
/// A player pressed an action or let go of it (`held_ticks` after how long): any key binding, the
/// mouse buttons' `keys::USE` and `keys::ATTACK` too. Keys of your own are `Registry::key`.
pub static KEY: Event<abi::KeyChange> = Event::named(abi::event::KEY);
/// An entity left the world, and why (`removal`): died, despawned, unloaded, picked up..
pub static ENTITY_REMOVED: Event<abi::Removed> = Event::named(abi::event::ENTITY_REMOVED);
/// A feature on a grid stands in the world now, its start's cube generated: `feature` is its id
/// (`VILLAGE.id()`).
pub static FEATURE_PLACED: Event<abi::FeaturePlaced> = Event::named(abi::event::FEATURE_PLACED);
/// An effect ran out or was taken off.
pub static EFFECT_ENDED: Event<abi::EffectChange> = Event::named(abi::event::EFFECT_ENDED);

/// Something that happens, as one name: the question asked before it (`r.before_event(&A, ..)`)
/// and the event after (`r.on_event(&A, ..)`). The engine's are in `actions`; a mod's
/// own goes through `w.act(&WRENCH, &mut query, value)`.
///
/// ```ignore
/// static WRENCH: Action<Wrenched> = Action::named("wrench");
/// r.before_event(&actions::DIE, |a| if has_totem(a) { Verdict::Deny } else { Verdict::Abstain });
/// r.on_event(&actions::BREAK_BLOCK, |b| for c in b.iter() { /* .. */ });
/// ```
pub struct Action<T: Value> {
    ask: Question,
    done: Event<T>,
}

impl<T: Value> Action<T> {
    pub const fn named(name: &'static str) -> Self {
        Action { ask: Question::named(name), done: Event::named(name) }
    }
}

impl<T: Value> AsRef<Question> for Action<T> {
    fn as_ref(&self) -> &Question {
        &self.ask
    }
}

impl<T: Value> AsRef<Event<T>> for Action<T> {
    fn as_ref(&self) -> &Event<T> {
        &self.done
    }
}

impl AsRef<Question> for Question {
    fn as_ref(&self) -> &Question {
        self
    }
}

impl<T: Value> AsRef<Event<T>> for Event<T> {
    fn as_ref(&self) -> &Event<T> {
        self
    }
}

impl World {
    /// Asks `action`'s question with `q`, and when the answer is yes sends its event with `value`.
    /// True if it may happen.
    pub fn act<T: Value>(&self, action: &Action<T>, q: &abi::Query, value: T) -> bool {
        let ok = self.ask(action, q);
        if ok {
            self.emit(action, value);
        }
        ok
    }
}

/// The engine's actions (`abi::action`): what may be refused and what is told after, for each.
pub mod actions {
    use super::Action;
    use crate::abi::{self, action as a};
    pub static BREAK_BLOCK: Action<abi::BlockChange> = Action::named(a::BREAK_BLOCK);
    pub static PLACE_BLOCK: Action<abi::BlockChange> = Action::named(a::PLACE_BLOCK);
    pub static USE_BLOCK: Action<abi::BlockChange> = Action::named(a::USE_BLOCK);
    /// Sent when using the item did something (`stack` as it was before): eating, emptying a
    /// bucket. Another mod reacts to food here, by the item's components.
    pub static USE_ITEM: Action<abi::ItemUsed> = Action::named(a::USE_ITEM);
    pub static INTERACT_ENTITY: Action<abi::Interaction> = Action::named(a::INTERACT_ENTITY);
    /// Asked before the damage chain, sent after armor and every modifier.
    pub static HURT: Action<abi::Hurt> = Action::named(a::HURT);
    pub static DIE: Action<abi::Death> = Action::named(a::DIE);
    pub static PICKUP_ITEM: Action<abi::Pickup> = Action::named(a::PICKUP_ITEM);
    pub static ENTER_DIMENSION: Action<abi::DimensionChange> = Action::named(a::ENTER_DIMENSION);
    pub static CHAT: Action<abi::ChatMessage> = Action::named(a::CHAT);
    /// Asked with `entity` the rider, `other` the mount, `amount` the seat.
    pub static MOUNT: Action<abi::Mounting> = Action::named(a::MOUNT);
    pub static DISMOUNT: Action<abi::Mounting> = Action::named(a::DISMOUNT);
    /// Asked with `entity` just made and not yet seen, `cause` (`spawn_cause`), `pos`: a no and
    /// it never was. Sent when it came. Not for dropped stacks and moving blocks.
    pub static SPAWN: Action<abi::Spawned> = Action::named(a::SPAWN);
    /// Asked with `entity` about to get effect `cause` at level `amount` for `state` ticks (an
    /// immunity, a milk of your own); sent when it got it.
    pub static EFFECT: Action<abi::EffectChange> = Action::named(a::EFFECT);
    /// Asked with `entity` the projectile, `other` what it hits (0: the block at `pos`), `amount`
    /// its speed: no, and it flies on past them. Sent when it hit.
    pub static PROJECTILE_HIT: Action<abi::ProjectileHit> = Action::named(a::PROJECTILE_HIT);
}

/// Every value of one event queued during a tick, in the order they happened.
pub struct EventBatch<T> {
    world: World,
    values: Vec<T>,
}

impl<T: Copy> EventBatch<T> {
    pub fn world(&self) -> World {
        self.world
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.values.iter()
    }
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// A question's answer at one step: `Deny` (no), `Abstain` (keep what the steps before said),
/// `Allow` (yes). The last that isn't `Abstain` wins; nobody saying either way is yes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Abstain,
    Allow,
    Deny,
}

/// A question being asked (`Registry::before_event`); `answer` is what the steps before said.
pub struct Ask {
    world: World,
    question: &'static str,
    query: abi::Query,
    pub answer: bool,
}

impl Ask {
    pub fn world(&self) -> World {
        self.world
    }
    pub fn question(&self) -> &str {
        self.question
    }
    pub fn query(&self) -> &abi::Query {
        &self.query
    }
    pub fn entity(&self) -> u64 {
        self.query.entity
    }
    pub fn pos(&self) -> BlockPos {
        self.query.pos
    }
    pub fn state(&self) -> u32 {
        self.query.state
    }
    /// What a player is about to say (`actions::CHAT`).
    pub fn text(&self) -> &str {
        unsafe { self.query.text.as_str() }
    }
    pub fn item(&self) -> u32 {
        self.query.item
    }
    /// Whether `question::CAMERA` asks about `mode`.
    pub fn is_camera(&self, mode: &crate::CameraMode) -> bool {
        self.query.amount == mode.id() as f64
    }
}

/// One step's run (`Registry::modify`, `Registry::step`): read the context, change `value`.
/// `base` is where the chain began, `start` where this phase began, `arg` the number a modifier
/// gave the step (0 on a chain). A question's value is 1 for yes, 0 for no.
pub struct Modify {
    world: World,
    query: abi::Query,
    chain: &'static str,
    pub base: f64,
    pub start: f64,
    pub value: f64,
    pub arg: f64,
}

impl Modify {
    pub fn world(&self) -> World {
        self.world
    }
    pub fn query(&self) -> &abi::Query {
        &self.query
    }
    /// The chain it runs in: one step may be on several (a modifier on any attribute).
    pub fn chain(&self) -> &str {
        self.chain
    }
}

fn step_cb(f: impl Fn(&mut Modify) + Send + Sync + 'static) -> abi::Callback {
    callback(move |a: &mut abi::StepArgs| {
        let mut m = Modify {
            world: World::from_raw(a.world),
            query: unsafe { *a.ctx },
            chain: unsafe { std::mem::transmute::<&str, &'static str>(a.chain.as_str()) },
            base: a.base,
            start: a.start,
            value: a.value,
            arg: a.arg,
        };
        f(&mut m);
        a.value = m.value;
    })
}

/// A step being registered (`Registry::step`, `modify`, `before_event`).
pub struct StepBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl StepBuilder<'_> {
    /// Runs every time `chain` is computed; without, only where a modifier names it.
    pub fn on(self, chain: &impl Valued) -> Self {
        self.set_text(&comps::step::CHAIN, chain.chain_name())
    }
    pub fn phase(self, phase: &Phase) -> Self {
        self.set_text(&comps::step::PHASE, phase.name())
    }
    /// Runs after step `name` when both are in one phase of a chain.
    pub fn after(self, name: &str) -> Self {
        self.push(&comps::step::AFTER, crate::full_name(name))
    }
    pub fn before(self, name: &str) -> Self {
        self.push(&comps::step::BEFORE, crate::full_name(name))
    }
    /// What it does to the value.
    pub fn run(self, f: impl Fn(&mut Modify) + Send + Sync + 'static) -> Self {
        self.set(&comps::step::CALL, step_cb(f))
    }
    /// What it says to a question.
    pub fn answer(self, f: impl Fn(&Ask) -> Verdict + Send + Sync + 'static) -> Self {
        self.run(move |m| {
            let ask = Ask { world: m.world, question: m.chain, query: m.query, answer: m.value != 0.0 };
            match f(&ask) {
                Verdict::Abstain => {}
                Verdict::Allow => m.value = 1.0,
                Verdict::Deny => m.value = 0.0,
            }
        })
    }
}

/// A phase being registered (`Registry::phase`).
pub struct PhaseBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl PhaseBuilder<'_> {
    /// Comes right after `phase` (after phases that said so before it).
    pub fn after(self, phase: &Phase) -> Self {
        self.set_text(&comps::phase::AFTER, phase.name())
    }
}

crate::entry::entry_builder!(
    Step => StepBuilder, step_entry;
    Phase => PhaseBuilder, phase_entry;
);

/// Names of steps registered without a handle (`modify`, `before_event`).
static UNNAMED: AtomicU32 = AtomicU32::new(0);

/// What a loot roll knows.
#[derive(Clone, Copy, Default)]
pub struct LootCtx {
    pub tool: ItemStack,
    pub killer: u64,
    pub by_player: bool,
    pub on_fire: bool,
    /// The block state mined with `tool`, for `Cond::harvested` (0 = none).
    pub mined: u32,
}

/// A condition (`abi::condition`): one of the engine's below, a registered one
/// (`Cond::from(&conditions::NIGHT)`, `Cond::with(&MY_COND, "lava")`) or written out
/// (`Cond::new("night & light(..7)")`), put together with `&`, `|` and `!`. Loot tables, spawning,
/// poses, music, camera modes and text (`{if:..}`) all take them, and `w.holds` asks one.
#[derive(Clone, Debug)]
pub struct Cond(pub(crate) String);

impl Cond {
    pub fn new(text: &str) -> Cond {
        Cond(text.to_string())
    }
    /// A registered condition with its argument, `name(arg)`.
    pub fn with(c: &crate::Condition, arg: &str) -> Cond {
        Cond(format!("{}({arg})", c.name()))
    }
    pub fn text(&self) -> &str {
        &self.0
    }
    /// Holds that share of the times it is asked.
    pub fn chance(p: f32) -> Cond {
        Cond(format!("chance({p})"))
    }
    /// The light there, inclusive (0..15): the brighter of sky light times daylight and block light.
    pub fn light(min: u32, max: u32) -> Cond {
        Cond(format!("light({min}..{max})"))
    }
    /// The block asked about is one `what` takes (a block matcher): one broken, one under a spawn.
    pub fn block(what: &str) -> Cond {
        Cond(format!("block({})", crate::qualify_matcher(crate::mod_id(), what)))
    }
    /// The cell on side `d` of the one asked about has a block `what` takes, or (empty) is solid.
    /// Not known while unloaded, so a rule waits. In a block's rule the side is of its own frame.
    pub fn side(d: crate::Dir, what: &str) -> Cond {
        match what.trim() {
            "" => Cond(format!("side({})", d.name())),
            m => Cond(format!("side({}, {})", d.name(), crate::qualify_matcher(crate::mod_id(), m))),
        }
    }
    /// A block horizontally next to the cell on side `d` is one `what` takes.
    pub fn beside(d: crate::Dir, what: &str) -> Cond {
        Cond(format!("beside({}, {})", d.name(), crate::qualify_matcher(crate::mod_id(), what)))
    }
    /// Side `d` can be fallen into: air, a liquid, what placing replaces.
    pub fn open(d: crate::Dir) -> Cond {
        Cond(format!("open({})", d.name()))
    }
    /// Something opaque is on top.
    pub fn covered() -> Cond {
        Cond::new("covered")
    }
    /// That block has these properties, written as in `w.prop`: `state("age=7")` for a ripe crop.
    pub fn state(props: &str) -> Cond {
        Cond(format!("state({props})"))
    }
    /// What it was broken or killed with carries the tag (`tools/shears`).
    pub fn tool(tag: &str) -> Cond {
        Cond(format!("tool({tag})"))
    }
    pub fn tool_level(level: u32) -> Cond {
        Cond(format!("tool_level({level})"))
    }
    pub fn killed_by_player() -> Cond {
        Cond::new("killed_by_player")
    }
    pub fn on_fire() -> Cond {
        Cond::new("on_fire")
    }
    /// The block was harvested: broken by the world, or mined with a tool its `mined_by` level
    /// accepts. `drops(..)` tables have it; write it on pools of your own tables that should.
    pub fn harvested() -> Cond {
        Cond::new("harvested")
    }
    /// The body asked about is in a liquid it swims in, flies, sprints (poses).
    pub fn in_liquid() -> Cond {
        Cond::new("in_liquid")
    }
    pub fn flying() -> Cond {
        Cond::new("flying")
    }
    pub fn sprinting() -> Cond {
        Cond::new("sprinting")
    }
    /// An attribute of the body asked about is within `min..=max`: `attribute(&attribute::CAN_FLY,
    /// 1.0, f64::MAX)` where it may fly.
    pub fn attribute(a: &crate::Attribute, min: f64, max: f64) -> Cond {
        let end = |v: f64| if v.is_finite() && v.abs() < 1e30 { v.to_string() } else { String::new() };
        Cond(format!("attribute({}, {}..{})", a.name(), end(min), end(max)))
    }
}

impl From<&crate::Condition> for Cond {
    fn from(c: &crate::Condition) -> Cond {
        Cond(c.name().to_string())
    }
}

impl std::ops::BitAnd for Cond {
    type Output = Cond;
    fn bitand(self, o: Cond) -> Cond {
        Cond(format!("({}) & ({})", self.0, o.0))
    }
}

impl std::ops::BitOr for Cond {
    type Output = Cond;
    fn bitor(self, o: Cond) -> Cond {
        Cond(format!("({}) | ({})", self.0, o.0))
    }
}

impl std::ops::Not for Cond {
    type Output = Cond;
    fn not(self) -> Cond {
        Cond(format!("!({})", self.0))
    }
}

/// What happens to a block by itself (`.rule(..)` on a block or a state): when
/// (`Rule::random(mean_secs)` now and then, `Rule::neighbor()` as soon as something next to it
/// changes, `Rule::after(secs)` that much later), if (`.when(..)`, again for more, about its
/// own cell and state), then (`.becomes("age=0")` or `.becomes("base:dirt")`, `.grows()`,
/// `.breaks()`, `.falls()`). Time goes on in unloaded areas and is caught up when they load.
/// `grows`, `smothered_into`, `needs`, `falling` are rules.
#[derive(Clone)]
pub struct Rule(pub(crate) abi::BlockRule);

impl Rule {
    fn of(when: u32, secs: f32) -> Rule {
        Rule(abi::BlockRule { when, secs, conditions: Vec::new(), then: abi::rule_then::GROW, into: String::new() })
    }
    pub fn random(mean_secs: f32) -> Rule {
        Rule::of(abi::rule_when::RANDOM, mean_secs)
    }
    pub fn neighbor() -> Rule {
        Rule::of(abi::rule_when::NEIGHBOR, 0.0)
    }
    pub fn after(secs: f32) -> Rule {
        Rule::of(abi::rule_when::NEIGHBOR, secs)
    }
    pub fn when(mut self, c: impl Into<Cond>) -> Rule {
        self.0.conditions.push(c.into().0);
        self
    }
    /// Properties set (`age=0`) or another block (`base:dirt`, `base:farmland[moisture=0]`).
    pub fn becomes(mut self, into: &str) -> Rule {
        let props = into.contains('=') && !into.contains('[');
        let into = if props { into.to_string() } else { crate::full_name(into) };
        (self.0.then, self.0.into) = (abi::rule_then::BECOME, into);
        self
    }
    /// Its `on_grow`, else its `age` one higher.
    pub fn grows(mut self) -> Rule {
        self.0.then = abi::rule_then::GROW;
        self
    }
    /// A piece comes off: it becomes `into` (`north=false`), dropping what the piece drops.
    pub fn sheds(mut self, into: &str) -> Rule {
        (self.0.then, self.0.into) = (abi::rule_then::SHED, into.to_string());
        self
    }
    /// Breaks off, dropping what it drops.
    pub fn breaks(mut self) -> Rule {
        self.0.then = abi::rule_then::BREAK;
        self
    }
    pub fn falls(mut self) -> Rule {
        self.0.then = abi::rule_then::FALL;
        self
    }
}

/// What a block, an entity kind or a dimension does to an entity meeting it (`.hazard(..)`):
/// `Hazard::touch(1.0, &CACTUS)` a cactus, `Hazard::on(1.0, &HOT_FLOOR)` magma,
/// `Hazard::inside(1.0, &damage_causes::FIRE).fire(160)` fire, `Hazard::own(..)` a kind's to
/// itself or a dimension's to whoever is in it, `Hazard::crushed(..)` a body's to what it
/// squeezes; `.every(ticks)` between hits, `.when(..)` conditions about the entity there.
#[derive(Clone)]
pub struct Hazard(pub(crate) abi::Hazard);

impl Hazard {
    fn of(at: u32, damage: f32, cause: &crate::DamageCause) -> Hazard {
        Hazard(abi::Hazard { at, damage, cause: cause.name().to_string(), fire: 0, every: 0, when: Vec::new(), level: 0 })
    }
    /// Its box in the block's shape.
    pub fn inside(damage: f32, cause: &crate::DamageCause) -> Hazard {
        Hazard::of(abi::hazard_at::INSIDE, damage, cause)
    }
    /// Standing on it.
    pub fn on(damage: f32, cause: &crate::DamageCause) -> Hazard {
        Hazard::of(abi::hazard_at::ON, damage, cause)
    }
    /// Its eyes in it.
    pub fn head(damage: f32, cause: &crate::DamageCause) -> Hazard {
        Hazard::of(abi::hazard_at::HEAD, damage, cause)
    }
    /// Within a sixteenth of its shape.
    pub fn touch(damage: f32, cause: &crate::DamageCause) -> Hazard {
        Hazard::of(abi::hazard_at::TOUCH, damage, cause)
    }
    /// Wherever it is, asked at its eyes: an entity kind's to itself, a dimension's to everyone
    /// in it (`.when(..)` says where: `height(..0)`).
    pub fn own(damage: f32, cause: &crate::DamageCause) -> Hazard {
        Hazard::of(abi::hazard_at::SELF, damage, cause)
    }
    /// A body of this kind squeezes it into something solid; the body crushes instead of pushing.
    pub fn crushed(damage: f32, cause: &crate::DamageCause) -> Hazard {
        Hazard::of(abi::hazard_at::CRUSHED, damage, cause)
    }
    /// Sets it burning for that many ticks too.
    pub fn fire(mut self, ticks: u32) -> Hazard {
        self.0.fire = ticks;
        self
    }
    /// No damage of its own: only `fire`.
    pub fn harmless(mut self) -> Hazard {
        self.0.cause.clear();
        self
    }
    pub fn every(mut self, ticks: u32) -> Hazard {
        self.0.every = ticks;
        self
    }
    pub fn when(mut self, c: impl Into<Cond>) -> Hazard {
        self.0.when.push(c.into().0);
        self
    }
}

/// One choice of a pool.
#[derive(Clone)]
pub struct Entry {
    item: String,
    weight: u32,
    min: u32,
    max: u32,
    when: Vec<Cond>,
    smelt: String,
}

impl Entry {
    /// An item name (this mod's namespace by default) or `#tag`.
    pub fn item(name: &str) -> Entry {
        Entry { item: name.to_string(), weight: 1, min: 1, max: 1, when: Vec::new(), smelt: String::new() }
    }
    /// A choice that gives nothing (weight against the others).
    pub fn nothing() -> Entry {
        Entry::item("")
    }
    pub fn count(mut self, min: u32, max: u32) -> Entry {
        (self.min, self.max) = (min, max.max(min));
        self
    }
    pub fn weight(mut self, w: u32) -> Entry {
        self.weight = w;
        self
    }
    pub fn when(mut self, c: impl Into<Cond>) -> Entry {
        self.when.push(c.into());
        self
    }
    /// A burning victim drops it cooked by this recipe type (`base:smelting`).
    pub fn smelted(mut self, recipe_kind: &crate::RecipeKind) -> Entry {
        self.smelt = recipe_kind.name().to_string();
        self
    }
}

/// Rolls (1 by default) of one weighted choice each.
#[derive(Clone, Default)]
pub struct Pool {
    rolls: (u32, u32),
    when: Vec<Cond>,
    entries: Vec<Entry>,
}

impl Pool {
    pub fn new() -> Pool {
        Pool { rolls: (1, 1), when: Vec::new(), entries: Vec::new() }
    }
    pub fn rolls(mut self, min: u32, max: u32) -> Pool {
        self.rolls = (min, max.max(min));
        self
    }
    pub fn when(mut self, c: impl Into<Cond>) -> Pool {
        self.when.push(c.into());
        self
    }
    pub fn entry(mut self, e: Entry) -> Pool {
        self.entries.push(e);
        self
    }
    /// One of `name`.
    pub fn item(self, name: &str) -> Pool {
        self.entry(Entry::item(name))
    }
    /// `min..=max` of `name`.
    pub fn items(self, name: &str, min: u32, max: u32) -> Pool {
        self.entry(Entry::item(name).count(min, max))
    }
    pub(crate) fn raw(&self, ns: &str) -> abi::LootPool {
        let conds = |c: &[Cond]| c.iter().map(|c| c.0.clone()).collect();
        let entries = self
            .entries
            .iter()
            .map(|e| abi::LootEntry {
                item: loot_item(ns, &e.item),
                weight: e.weight,
                min: e.min,
                max: e.max,
                when: conds(&e.when),
                smelt: e.smelt.clone(),
            })
            .collect();
        abi::LootPool { rolls_min: self.rolls.0, rolls_max: self.rolls.1, when: conds(&self.when), entries }
    }
}

/// An item name in this mod's namespace by default; `#tag` and nothing stay as they are.
fn loot_item(ns: &str, item: &str) -> String {
    if item.is_empty() || item.starts_with('#') { item.to_string() } else { qualify(ns, item) }
}

/// A loot table being registered (`Registry::loot`) or changed (`r.edit(ZOMBIE.loot())`); sent
/// when dropped.
pub struct LootBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl LootBuilder<'_> {
    /// One more pool, after the table's.
    pub fn pool(self, p: Pool) -> Self {
        let pool = p.raw(self.reg.ns);
        self.push(&comps::loot::POOLS, pool)
    }
    /// Every entry of item `from` gives `to` instead, in whatever pools the table has in the end.
    pub fn replace_drop(self, from: &str, to: &str) -> Self {
        let swap = abi::LootSwap { name: loot_item(self.reg.ns, from), to: loot_item(self.reg.ns, to) };
        self.push(&comps::loot::SWAPS, swap)
    }
    /// Entries of `item` are left out, likewise.
    pub fn remove_drop(self, item: &str) -> Self {
        let swap = abi::LootSwap { name: loot_item(self.reg.ns, item), to: String::new() };
        self.push(&comps::loot::SWAPS, swap)
    }
    /// Empties the table, its swaps too; pools added after it stay.
    pub fn clear(self) -> Self {
        self.set(&comps::loot::POOLS, Vec::new())
            .unset(&comps::loot::SWAPS)
    }
}

/// Every entity of a region that has a component, with the values side by side, and the ticks
/// each call stands for: 1 normally, more while an entity catches up on time it missed (its
/// region was unloaded, the world's time jumped). A system worth catching up does the work of
/// `ticks` ticks at once (a furnace: so many items smelted), anything else may ignore it.
pub struct SystemBatch<'a, T> {
    world: World,
    ids: &'a [u64],
    values: &'a mut [T],
    ticks: &'a [u32],
}

impl<T> SystemBatch<'_, T> {
    pub fn world(&self) -> World {
        self.world
    }
    pub fn ids(&self) -> &[u64] {
        self.ids
    }
    pub fn values(&mut self) -> &mut [T] {
        self.values
    }
    pub fn ticks(&self) -> &[u32] {
        self.ticks
    }
    /// (entity, its value, ticks to do).
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (u64, &mut T, u32)> {
        self.ids
            .iter()
            .copied()
            .zip(self.values.iter_mut())
            .zip(self.ticks.iter().copied())
            .map(|((id, v), t)| (id, v, t))
    }
}

/// An attribute being registered (`Registry::attribute`).
pub struct AttributeBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(
    Attribute => AttributeBuilder, attribute_entry;
    LootTable => LootBuilder, loot_entry;
);

impl AttributeBuilder<'_> {
    /// The base of it for kinds that don't set their own (the player) is a world rule named like
    /// it (`/rule`), with its default and range; `/attribute` sets one entity's apart.
    pub fn rule(self) -> Self {
        use crate::WithComponents;
        self.tag(abi::comp::attribute::RULE)
    }
}

impl Registry {
    /// A system over a per-entity component (`Registry::entity_component`): once per tick and
    /// region with all values in one slice, no host call per entity.
    pub fn system<T: Value>(&mut self, comp: &crate::Component<T>, f: impl Fn(&mut SystemBatch<T>) + Send + Sync + 'static) {
        let cb = callback(move |a: &mut abi::SystemArgs| {
            if a.stride as usize != size_of::<T>() || a.count == 0 {
                return;
            }
            let n = a.count as usize;
            let mut b = SystemBatch {
                world: World::from_raw(a.world),
                ids: unsafe { std::slice::from_raw_parts(a.ids, n) },
                values: unsafe { std::slice::from_raw_parts_mut(a.values as *mut T, n) },
                ticks: unsafe { std::slice::from_raw_parts(a.ticks, n) },
            };
            f(&mut b);
        });
        unsafe { (host().reg_system)(self.raw, Str::new(comp.name()), Str::new(&T::schema()), cb) }
    }

    /// Handles every value of `event` once per tick (and region), in one call.
    pub fn on_event<T: Value>(&mut self, event: &impl AsRef<Event<T>>, f: impl Fn(&EventBatch<T>) + Send + Sync + 'static) {
        let cb = callback(move |a: &mut abi::EventArgs| {
            let stride = a.stride as usize;
            if stride != size_of::<T>() {
                return;
            }
            let values = (0..a.count as usize)
                .map(|i| unsafe { std::ptr::read_unaligned(a.data.add(i * stride) as *const T) })
                .collect();
            f(&EventBatch { world: World::from_raw(a.world), values });
        });
        unsafe { (host().reg_event_handler)(self.raw, Str::new(event.as_ref().name()), Str::new(&T::schema()), cb) }
    }

    /// Answers `question` (`question::*`, `actions::*` or a mod's own) before it happens, in
    /// `phase::RULES` unless `.phase(..)` says otherwise.
    pub fn before_event(&mut self, question: &impl Valued, f: impl Fn(&Ask) -> Verdict + Send + Sync + 'static) -> StepBuilder<'_> {
        self.unnamed_step()
            .on(question)
            .phase(&phase::RULES)
            .answer(f)
    }

    /// A step on `chain` (`chain::*`, an attribute, a question, a mod's own) in `phase`. One
    /// others should be able to order around or replace is `Registry::step`.
    pub fn modify(&mut self, chain: &impl Valued, phase: &Phase, f: impl Fn(&mut Modify) + Send + Sync + 'static) -> StepBuilder<'_> {
        self.unnamed_step().on(chain).phase(phase).run(f)
    }

    /// A step of this mod's by name: `.on(&chain)` runs it in every computation of the chain,
    /// without it only modifiers naming it run it (`Modifier::step`). `r.edit(&Step::named(
    /// "core:crit"))` reorders or recodes anyone's, `r.remove` takes it out.
    pub fn step(&mut self, step: &'static Step) -> StepBuilder<'_> {
        self.step_entry(step)
    }

    fn step_entry(&mut self, step: &'static Step) -> StepBuilder<'_> {
        StepBuilder { reg: self, e: crate::entry::Entry::of(step) }
    }

    fn unnamed_step(&mut self) -> StepBuilder<'_> {
        let n = UNNAMED.fetch_add(1, Ordering::Relaxed);
        self.step_entry(Step::generated(format!("step/{n}")))
    }

    /// A phase of this mod's, right after `after`.
    pub fn phase(&mut self, phase: &'static Phase, after: &Phase) -> PhaseBuilder<'_> {
        self.phase_entry(phase).after(after)
    }

    fn phase_entry(&mut self, phase: &'static Phase) -> PhaseBuilder<'_> {
        PhaseBuilder { reg: self, e: crate::entry::Entry::of(phase) }
    }

    /// An attribute of living entities (`attribute::*` are the engine's).
    pub fn attribute(&mut self, attribute: &'static Attribute, default: f64, min: f64, max: f64) -> AttributeBuilder<'_> {
        self.attribute_entry(attribute)
            .set(&crate::comps::attribute::DEFAULT, default)
            .set(&crate::comps::attribute::RANGE, [min, max])
    }

    fn attribute_entry(&mut self, attribute: &'static Attribute) -> AttributeBuilder<'_> {
        AttributeBuilder { reg: self, e: crate::entry::Entry::of(attribute) }
    }

    /// A loot table: a block's or entity kind's is what it drops, a `LootTable` is one of its own.
    pub fn loot<'a>(&'a mut self, table: &dyn LootSource) -> LootBuilder<'a> {
        LootBuilder { reg: self, e: crate::entry::Entry::new(abi::entry_kind::LOOT, table.table_name().to_string()) }
    }

    fn loot_entry(&mut self, table: &'static LootTable) -> LootBuilder<'_> {
        LootBuilder { reg: self, e: crate::entry::Entry::of(table) }
    }

    /// Declares a component stacks carry a value of (`stack.with(&COMP, v)`), with its flags
    /// (`abi::stack_flags`: server-only, not saved, or read by conditional looks), 0 for none.
    pub fn stack_component<T: crate::component::Data>(
        &mut self,
        c: &crate::Component<T>,
        flags: u32,
    ) -> crate::game::DeclarationBuilder<'_> {
        self.declare::<T>(c.name()).stack(flags)
    }
}

impl World {
    /// In `ticks` ticks `event` fires with `value` (a calm-down, a fuse, a hunger pulse). The
    /// timer is saved with `holder`: an entity (and goes with it), a block (`BLOCK_TICK` is its
    /// own tick, `schedule_tick`), `Holder::NONE` for the dimension. Time away counts: a timer due
    /// while its holder wasn't loaded fires when it is.
    pub fn timer<T: Value>(&self, holder: impl Into<abi::Holder>, ticks: u32, event: &Event<T>, value: T) {
        let id = event.id();
        if id == u32::MAX {
            return;
        }
        let b = unsafe { std::slice::from_raw_parts(&value as *const T as *const u8, size_of::<T>()) };
        unsafe { (host().timer)(self.raw(), &holder.into(), ticks, id, Slice::new(b)) }
    }

    /// The block's own tick (`block.on_scheduled`, a liquid's flow) in `ticks` ticks.
    pub fn schedule_tick(&self, pos: impl Into<abi::BlockPos>, ticks: u32) {
        let pos = pos.into();
        self.timer(abi::Holder::block(pos), ticks, &BLOCK_TICK, [pos.x, pos.y, pos.z]);
    }

    pub fn emit<T: Value>(&self, event: &impl AsRef<Event<T>>, value: T) {
        let id = event.as_ref().id();
        if id == u32::MAX {
            return;
        }
        let b = unsafe { std::slice::from_raw_parts(&value as *const T as *const u8, size_of::<T>()) };
        unsafe { (host().emit)(self.raw(), id, Slice::new(b)) }
    }

    /// Question `question` about `q`; true if the answer is yes.
    pub fn ask(&self, question: &impl Valued, q: &abi::Query) -> bool {
        unsafe { (host().ask)(self.raw(), Str::new(question.chain_name()), q) != 0 }
    }

    pub fn chain_compute(&self, chain: &impl Valued, base: f64, q: &abi::Query) -> f64 {
        unsafe { (host().chain_compute)(self.raw(), Str::new(chain.chain_name()), base, q, 0) }
    }

    /// Like `chain_compute`, with the steps that led to the value
    /// ("core:damage: 4, core:multiply mymod:step/0: 4 -> 2 = 2").
    pub fn chain_explain(&self, chain: &impl Valued, base: f64, q: &abi::Query) -> (f64, String) {
        let v = unsafe { (host().chain_compute)(self.raw(), Str::new(chain.chain_name()), base, q, 1) };
        (v, unsafe { (host().explanation)().as_str().to_string() })
    }

    pub fn attribute(&self, entity: u64, attribute: &Attribute) -> f64 {
        unsafe { (host().attribute)(self.raw(), entity, Str::new(attribute.name()), 0) }
    }

    pub fn attribute_explain(&self, entity: u64, attribute: &Attribute) -> (f64, String) {
        let v = unsafe { (host().attribute)(self.raw(), entity, Str::new(attribute.name()), 1) };
        (v, unsafe { (host().explanation)().as_str().to_string() })
    }

    /// Puts modifier `m` on an entity as `id` (replacing one of that id on the same attribute)
    /// for `ticks` (0 = until removed). Saved with it; `worn` and `when` don't apply here.
    pub fn attribute_add_modifier(&self, entity: u64, id: &str, m: &Modifier, ticks: u64) {
        let m = &m.0;
        unsafe {
            (host().attribute_add_modifier)(
                self.raw(),
                entity,
                Str::new(&m.attribute),
                Str::new(id),
                Str::new(&m.step),
                Str::new(&m.phase),
                m.amount,
                ticks,
            )
        }
    }

    pub fn attribute_remove_modifier(&self, entity: u64, attr: &Attribute, id: &str) {
        unsafe { (host().attribute_remove_modifier)(self.raw(), entity, Str::new(attr.name()), Str::new(id)) }
    }

    /// Whether `c` holds at `pos` for `entity` (0 for nobody): `w.holds(&Cond::new("night"), pos, 0)`.
    pub fn holds(&self, c: &Cond, pos: crate::Vec3, entity: u64) -> bool {
        unsafe { (host().holds)(self.raw(), Str::new(&c.0), pos, entity) != 0 }
    }

    pub fn roll_loot(&self, table: &dyn LootSource, ctx: &LootCtx) -> Vec<ItemStack> {
        let table = table.table_name();
        let c = abi::LootCtx {
            tool: ctx.tool,
            killer: ctx.killer,
            by_player: ctx.by_player as u32,
            on_fire: ctx.on_fire as u32,
            mined: ctx.mined,
        };
        let mut buf = vec![ItemStack::EMPTY; 16];
        let n = unsafe { (host().roll_loot)(self.raw(), Str::new(table), &c, buf.as_mut_ptr(), buf.len()) };
        if n > buf.len() {
            buf.resize(n, ItemStack::EMPTY);
            unsafe { (host().roll_loot)(self.raw(), Str::new(table), &c, buf.as_mut_ptr(), buf.len()) };
        }
        buf.truncate(n);
        buf
    }

    /// Breaks a block as if mined with `tool` by `player` (0 = nobody), with its loot when
    /// `drops`. False if nothing broke or the question said no.
    pub fn break_block(&self, pos: impl Into<BlockPos>, tool: ItemStack, player: u64, drops: bool) -> bool {
        let pos = pos.into();
        unsafe { (host().break_block)(self.raw(), &pos, tool, player, drops as u32) != 0 }
    }
}

/// A query for `World::ask` and value chains.
pub fn query() -> abi::Query {
    abi::Query::DEFAULT
}
