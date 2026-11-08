//! Goal-driven mob behavior: goals in priority order, and each tick every entity runs the first
//! goal that wants to act. Movement goes through the engine's navigation (`World::navigate`),
//! so mobs walk around walls instead of into them.
//!
//! ```ignore
//! r.entity(&ZOMBIE).goals(Goals::new().aggressive(35.0).then(ai::melee()).then(ai::chase_target(35.0, 1.0)).then(ai::wander(0.6, 10.0)));
//! ```
//!
//! Every mob leaves everyone alone until something sets it off. A trigger (`Trigger::sight`,
//! `hurt`, `gaze`, `when`) says when, a reaction what then: any function of the mob and who set
//! it off, or `react::anger` (`melee`, `chase_target` and `target` go after them), `react::fear`
//! (`panic`, `flee` run), `react::alert` (the same for its kind around). `aggressive` and
//! `neutral` are the usual sets of them.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::{EntityKind, EntityState, EntityTick, World};

/// What a mob feels about whom: per mob, (who, until which tick) for anger and for fear. Kept
/// by this mod while it runs, not saved with the world.
#[derive(Default)]
struct Moods {
    anger: Vec<(u64, u64)>,
    fear: Vec<(u64, u64)>,
}

static MOODS: Mutex<Option<HashMap<u64, Moods>>> = Mutex::new(None);

/// `f` on mob `id`'s moods, what has run out already dropped.
fn moods<T>(w: &World, id: u64, f: impl FnOnce(&mut Moods) -> T) -> T {
    let now = w.game_time();
    let mut all = MOODS.lock().unwrap();
    let all = all.get_or_insert_with(HashMap::new);
    // Now and then everything run out, so mobs gone leave nothing behind.
    if now % 200 == 0 && all.len() > 64 {
        all.retain(|_, m| m.anger.iter().chain(&m.fear).any(|e| e.1 > now));
    }
    let m = all.entry(id).or_default();
    m.anger.retain(|e| e.1 > now);
    m.fear.retain(|e| e.1 > now);
    f(m)
}

/// Adds `who` until `until`, or keeps them longer; with `max` there already, the one that runs
/// out first makes room.
fn feel(list: &mut Vec<(u64, u64)>, who: u64, until: u64, max: usize) {
    if let Some(e) = list.iter_mut().find(|e| e.0 == who) {
        e.1 = e.1.max(until);
        return;
    }
    if list.len() >= max.max(1) {
        let first = (0..list.len()).min_by_key(|&i| list[i].1).unwrap_or(0);
        list.remove(first);
    }
    list.push((who, until));
}

/// What a mob does when a trigger goes off: (world, the mob, who set it off; 0 for nothing).
pub type Reaction = Arc<dyn Fn(&World, u64, u64) + Send + Sync>;

/// Is a mob open to it now? (world, the mob)
type Only = Arc<dyn Fn(&World, &EntityState) -> bool + Send + Sync>;
/// Does it set the mob off? (world, the mob, them)
type Test = Arc<dyn Fn(&World, &EntityState, &EntityState) -> bool + Send + Sync>;

#[derive(Clone)]
enum Sense {
    Sight(f64),
    Gaze(f64),
    Hurt,
    When(f64, Test),
}

/// Which entities count. (world, its id, its state)
type Pick = Arc<dyn Fn(&World, u64, &EntityState) -> bool + Send + Sync>;

/// Who a trigger notices.
#[derive(Clone)]
enum Whom {
    Players,
    Entities(Pick),
    /// Anyone and anything, the world included (a fall, a cactus): hurt only.
    Anything,
}

/// When a mob reacts: what sets it off, at whom, and while what holds.
#[derive(Clone)]
pub struct Trigger {
    sense: Sense,
    whom: Whom,
    only: Option<Only>,
}

impl Trigger {
    /// Someone within `range` blocks (8 up and down), as near as their `visibility` makes them;
    /// it goes on as long as they are.
    pub fn sight(range: f64) -> Trigger {
        Trigger { sense: Sense::Sight(range), whom: Whom::Players, only: None }
    }

    /// Someone within `range` looking at it, with nothing in between (an enderman).
    pub fn gaze(range: f64) -> Trigger {
        Trigger { sense: Sense::Gaze(range), whom: Whom::Players, only: None }
    }

    /// Someone hurting it.
    pub fn hurt() -> Trigger {
        Trigger { sense: Sense::Hurt, whom: Whom::Players, only: None }
    }

    /// Someone within `range` for whom `test(world, the mob, them)` holds.
    pub fn when(range: f64, test: impl Fn(&World, &EntityState, &EntityState) -> bool + Send + Sync + 'static) -> Trigger {
        Trigger { sense: Sense::When(range, Arc::new(test)), whom: Whom::Players, only: None }
    }

    /// Only while `f(world, the mob)` holds: a spider out of the daylight.
    pub fn only_if(mut self, f: impl Fn(&World, &EntityState) -> bool + Send + Sync + 'static) -> Trigger {
        self.only = Some(Arc::new(f));
        self
    }

    /// Only while `c` holds where the mob is (`!Cond::new("day")`).
    pub fn only_while(self, c: crate::Cond) -> Trigger {
        self.only_if(move |w, st| w.holds(&c, st.pos, 0))
    }

    /// Notices mobs of `kind` instead of players (a wolf and sheep).
    pub fn of(self, kind: &'static EntityKind) -> Trigger {
        self.of_any(move |_, _, st| st.kind == kind.id())
    }

    /// Notices entities of kinds with `tag` instead of players (`#undead`).
    pub fn of_tag(self, tag: &'static crate::Tag) -> Trigger {
        self.of_any(move |_, _, st| tag.on_entity(st.kind))
    }

    /// Notices whatever `pick(world, id, state)` accepts instead of players, them included or not.
    pub fn of_any(mut self, pick: impl Fn(&World, u64, &EntityState) -> bool + Send + Sync + 'static) -> Trigger {
        self.whom = Whom::Entities(Arc::new(pick));
        self
    }

    /// For `hurt`: whatever hurts it, other mobs and the world too.
    pub fn by_anything(mut self) -> Trigger {
        self.whom = Whom::Anything;
        self
    }

    fn open(&self, w: &World, st: &EntityState) -> bool {
        self.only.as_ref().is_none_or(|f| f(w, st))
    }

    fn noticed(&self, w: &World, who: u64) -> bool {
        match &self.whom {
            Whom::Players => w.players_iter().any(|p| p == who),
            Whom::Entities(pick) => w.entity(who).is_some_and(|s| pick(w, who, &s)),
            Whom::Anything => true,
        }
    }

    /// Who sets it off now, the farthest first (with room for one, the nearest stays); hurt
    /// goes by the events instead.
    fn seen(&self, w: &World, id: u64, st: &EntityState) -> Vec<u64> {
        let (range, test): (f64, &dyn Fn(&EntityState) -> bool) = match &self.sense {
            Sense::Hurt => return Vec::new(),
            Sense::Sight(r) => (*r, &|_| true),
            Sense::Gaze(r) => (*r, &|them| looks_at(w, them, st)),
            Sense::When(r, f) => (*r, &|them| f(w, st, them)),
        };
        let area = around(st, range);
        let mut found: Vec<(f64, u64)> = match &self.whom {
            Whom::Entities(pick) => w
                .entities_near(st.pos, range + 8.0, None)
                .into_iter()
                .filter(|&e| e != id)
                .filter_map(|e| w.entity(e).map(|s| (e, s)))
                .filter(|(e, s)| s.health > 0.0 && area.contains(s.pos) && pick(w, *e, s) && test(s))
                .map(|(e, s)| ((s.pos - st.pos).length_squared(), e))
                .collect(),
            _ => w
                .players_iter()
                .filter_map(|p| w.entity(p).map(|s| (p, s)))
                .filter(|(p, s)| {
                    let seen = w.attribute(*p, &crate::attribute::VISIBILITY);
                    s.health > 0.0 && seen > 0.0 && area.contains(st.pos + (s.pos - st.pos) * (1.0 / seen)) && test(s)
                })
                .map(|(p, s)| ((s.pos - st.pos).length_squared(), p))
                .collect(),
        };
        found.sort_by(|a, b| b.0.total_cmp(&a.0));
        found.into_iter().map(|f| f.1).collect()
    }
}

/// Whether `who` looks at `st` (vanilla's enderman test, a narrower cone further off) with no
/// block in between.
fn looks_at(w: &World, who: &EntityState, st: &EntityState) -> bool {
    let (from, to) = (who.eye(), st.eye());
    let d = to - from;
    let len = d.length_squared().sqrt().max(1e-6);
    let look = crate::look_dir(who.yaw, who.pitch);
    let dot = (look.x * d.x + look.y * d.y + look.z * d.z) / len;
    dot > 1.0 - 0.025 / len && w.raycast(from, d * (1.0 / len), len, false).is_none()
}

/// The usual reactions.
pub mod react {
    use super::*;

    /// Goes after them for `secs` (again from now each time it goes off), and after at most
    /// `max` at once: 1 for most mobs, more for one that charges through a crowd (`targets`).
    pub fn anger(secs: f64, max: usize) -> impl Fn(&World, u64, u64) + Send + Sync + Clone + 'static {
        move |w: &World, mob: u64, who: u64| {
            let until = w.game_time() + (secs * 20.0) as u64;
            moods(w, mob, |m| feel(&mut m.anger, who, until, max));
        }
    }

    /// Runs from them for `secs` (`panic` anywhere, `flee` away from all it fears), of at most
    /// `max` at once.
    pub fn fear(secs: f64, max: usize) -> impl Fn(&World, u64, u64) + Send + Sync + Clone + 'static {
        move |w: &World, mob: u64, who: u64| {
            let until = w.game_time() + (secs * 20.0) as u64;
            moods(w, mob, |m| feel(&mut m.fear, who, until, max));
        }
    }

    /// `then` for this mob and every one of its kind within `radius`: a pack.
    pub fn alert(
        radius: f64,
        then: impl Fn(&World, u64, u64) + Send + Sync + 'static,
    ) -> impl Fn(&World, u64, u64) + Send + Sync + 'static {
        move |w: &World, mob: u64, who: u64| {
            let Some(st) = w.entity(mob) else { return };
            let near = w.entities_near(st.pos, radius, None);
            for id in near
                .into_iter()
                .filter(|&id| w.entity(id).is_some_and(|s| s.kind == st.kind))
            {
                then(w, id, who);
            }
        }
    }
}

/// One behavior: acts this tick and returns true, or returns false to let a lower one act.
pub trait Goal: Send + Sync + 'static {
    fn run(&self, w: &World, id: u64, st: &EntityState) -> bool;
    /// A goal above it acted this tick, so this one didn't get to run.
    fn passed_over(&self, _w: &World, _id: u64) {}
}

impl<F: Fn(&World, u64, &EntityState) -> bool + Send + Sync + 'static> Goal for F {
    fn run(&self, w: &World, id: u64, st: &EntityState) -> bool {
        self(w, id, st)
    }
}

/// How long a mob goes after someone once nothing sets it off any more.
const SIGHT_ANGER: f64 = 2.0;
/// And after who hurt it (vanilla's 20 seconds).
const HURT_ANGER: f64 = 20.0;

#[derive(Default)]
pub struct Goals {
    goals: Vec<Box<dyn Goal>>,
    pub(crate) reactions: Vec<(Trigger, Reaction)>,
}

impl Goals {
    pub fn new() -> Goals {
        Goals::default()
    }
    /// Adds a goal below the ones added before.
    pub fn then(mut self, g: impl Goal) -> Goals {
        self.goals.push(Box::new(g));
        self
    }
    /// When `trigger` goes off, `reaction`: `react::anger(30.0, 1)`, or anything of your own
    /// (world, the mob, who set it off).
    pub fn react(mut self, trigger: Trigger, reaction: impl Fn(&World, u64, u64) + Send + Sync + 'static) -> Goals {
        self.reactions.push((trigger, Arc::new(reaction)));
        self
    }
    /// Goes for a player within `range` and for who hurt it: a zombie.
    pub fn aggressive(self, range: f64) -> Goals {
        self.react(Trigger::sight(range), react::anger(SIGHT_ANGER, 1))
            .neutral()
    }
    /// Goes for who hurt it, for 20 seconds; leaves everyone else alone.
    pub fn neutral(self) -> Goals {
        self.react(Trigger::hurt(), react::anger(HURT_ANGER, 1))
    }
    /// Runs about for `secs` when hurt, by anything: an animal.
    pub fn timid(self, secs: f64) -> Goals {
        self.react(Trigger::hurt().by_anything(), react::fear(secs, 8))
    }
    /// Runs the triggers, then the goals, for every entity of the tick.
    pub fn tick(&self, t: &EntityTick) {
        let w = t.world();
        for &id in t.ids() {
            let Some(st) = w.entity(id) else { continue };
            for (trigger, reaction) in self.reactions.iter() {
                if trigger.open(&w, &st) {
                    for who in trigger.seen(&w, id, &st) {
                        reaction(&w, id, who);
                    }
                }
            }
            let mut goals = self.goals.iter();
            for g in goals.by_ref() {
                if g.run(&w, id, &st) {
                    break;
                }
            }
            for g in goals {
                g.passed_over(&w, id);
            }
        }
    }

    /// One of these was hurt: the hurt triggers go off.
    pub(crate) fn hurt(reactions: &[(Trigger, Reaction)], w: &World, h: &crate::Hurt) {
        let Some(st) = w.entity(h.entity) else { return };
        for (trigger, reaction) in reactions {
            if matches!(trigger.sense, Sense::Hurt) && trigger.open(w, &st) && trigger.noticed(w, h.source) {
                reaction(w, h.entity, h.source);
            }
        }
    }
}

struct Playing<G> {
    from: &'static str,
    to: &'static str,
    goal: G,
}

impl<G: Goal> Goal for Playing<G> {
    fn run(&self, w: &World, id: u64, st: &EntityState) -> bool {
        let on = self.goal.run(w, id, st);
        if on {
            w.action_override(id, self.from, self.to);
        } else {
            w.action_override_clear(id, self.from);
        }
        on
    }

    fn passed_over(&self, w: &World, id: u64) {
        w.action_override_clear(id, self.from);
        self.goal.passed_over(w, id);
    }
}

/// `goal`, and while it is what the mob does, it plays action `to` wherever it would play `from`.
pub fn with_action_override(from: &'static str, to: &'static str, goal: impl Goal) -> impl Goal {
    Playing { from, to, goal }
}

/// Now and then walks somewhere within `radius` blocks at `speed`; idle in between.
pub fn wander(speed: f32, radius: f64) -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| {
        if w.navigating(id) {
            return true;
        }
        if w.random_below(100) != 0 {
            return false;
        }
        let a = w.random_below(628) as f64 / 100.0;
        let r = radius * (0.3 + w.random_below(70) as f64 / 100.0);
        w.navigate(id, [st.pos.x + a.cos() * r, st.pos.y, st.pos.z + a.sin() * r], speed);
        true
    }
}

/// The nearest living player within `range` and 8 blocks up or down, seen as if its
/// `visibility` attribute times nearer (0 in creative): its id and state.
pub fn nearest_player(w: &World, st: &EntityState, range: f64) -> Option<(u64, EntityState)> {
    nearest_player_by(w, st, &around(st, range), |_, _| true)
}

/// The usual zone a mob looks for players in: `range` around, 8 up and down.
fn around(st: &EntityState, range: f64) -> crate::Shape {
    crate::Shape::cylinder([st.pos.x, st.pos.y - 8.0, st.pos.z], range, 16.0, crate::Dir::Up)
}

/// The nearest living player in `area` that `wanted` accepts, by horizontal distance. A player
/// counts as if its `visibility` attribute times as close to the mob (0: never): a flyer can
/// look far down with a tall area, a mob of your own can have any condition.
pub fn nearest_player_by(
    w: &World,
    st: &EntityState,
    area: &crate::Shape,
    wanted: impl Fn(u64, &EntityState) -> bool,
) -> Option<(u64, EntityState)> {
    let d2 = |s: &EntityState| (s.pos.x - st.pos.x).powi(2) + (s.pos.z - st.pos.z).powi(2);
    w.players_iter()
        .filter_map(|p| w.entity(p).map(|s| (p, s)))
        .filter(|(p, s)| {
            let seen = w.attribute(*p, &crate::attribute::VISIBILITY);
            s.health > 0.0 && seen > 0.0 && area.contains(st.pos + (s.pos - st.pos) * (1.0 / seen)) && wanted(*p, s)
        })
        .min_by(|a, b| d2(&a.1).total_cmp(&d2(&b.1)))
}

/// Everyone this mob goes after (`react::anger`) who is within `range` and alive, as near as
/// their `visibility` makes them (0 in creative: not at all), the nearest first.
pub fn targets(w: &World, id: u64, st: &EntityState, range: f64) -> Vec<(u64, EntityState)> {
    let angry = moods(w, id, |m| m.anger.iter().map(|e| e.0).collect::<Vec<_>>());
    let area = around(st, range);
    let mut out: Vec<(u64, EntityState)> = angry
        .into_iter()
        .filter_map(|who| w.entity(who).filter(|s| s.health > 0.0).map(|s| (who, s)))
        .filter(|(who, s)| {
            let seen = if w.players_iter().any(|p| p == *who) { w.attribute(*who, &crate::attribute::VISIBILITY) } else { 1.0 };
            seen > 0.0 && area.contains(st.pos + (s.pos - st.pos) * (1.0 / seen))
        })
        .collect();
    out.sort_by(|a, b| {
        (a.1.pos - st.pos)
            .length_squared()
            .total_cmp(&(b.1.pos - st.pos).length_squared())
    });
    out
}

/// The nearest of `targets`: who `melee` and `chase_target` go for.
pub fn target(w: &World, id: u64, st: &EntityState, range: f64) -> Option<(u64, EntityState)> {
    targets(w, id, st, range).into_iter().next()
}

/// What it is afraid of now (`react::fear`; 0 is nothing in particular), if anything.
fn afraid(w: &World, id: u64) -> Option<Vec<u64>> {
    let of = moods(w, id, |m| m.fear.iter().map(|e| e.0).collect::<Vec<_>>());
    (!of.is_empty()).then_some(of)
}

/// Runs about while afraid (`react::fear`), a new way every second, at `speed` times its walk:
/// vanilla's animals when hurt.
pub fn panic(speed: f32) -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| {
        if afraid(w, id).is_none() {
            return false;
        }
        if w.game_time() % 20 == id % 20 || !w.navigating(id) {
            let a = w.random_below(628) as f64 / 100.0;
            w.navigate(id, [st.pos.x + a.cos() * 8.0, st.pos.y, st.pos.z + a.sin() * 8.0], speed);
        }
        true
    }
}

/// Runs away from all it is afraid of (`react::fear`), from their middle, at `speed`; afraid of
/// nothing in particular, about like `panic`.
pub fn flee(speed: f32) -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| {
        let Some(of) = afraid(w, id) else { return false };
        let them: Vec<crate::Vec3> = of
            .iter()
            .filter(|&&e| e != 0)
            .filter_map(|&e| w.entity(e).map(|s| s.pos))
            .collect();
        if them.is_empty() {
            return panic(speed).run(w, id, st);
        }
        let middle = them.iter().fold(crate::Vec3::ZERO, |a, &p| a + p) * (1.0 / them.len() as f64);
        let away = st.pos - middle;
        let len = (away.x * away.x + away.z * away.z).sqrt().max(1e-3);
        if w.game_time() % 10 == id % 10 || !w.navigating(id) {
            w.navigate(id, [st.pos.x + away.x / len * 8.0, st.pos.y, st.pos.z + away.z / len * 8.0], speed);
        }
        true
    }
}

/// Looks at the nearest player within `range` while standing around (vanilla's
/// LookAtPlayerGoal). The body stays where it is until the head has turned too far. Never takes
/// over: what comes after it runs. Like everything aimed at players it goes by `visibility`, so
/// mobs don't look at someone in creative.
pub fn watch_player(range: f64) -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| {
        if !w.navigating(id) {
            if let Some((_, ps)) = nearest_player(w, st, range) {
                let (dx, dz) = (ps.pos.x - st.pos.x, ps.pos.z - st.pos.z);
                let dy = ps.eye().y - st.eye().y;
                w.entity_modify(id)
                    .yaw(crate::yaw_toward(st.pos, ps.pos))
                    .pitch(dy.atan2((dx * dx + dz * dz).sqrt()) as f32);
            }
        }
        false
    }
}

/// Hits its `target` when it is in reach (`World::in_reach`: the kind's `.reach(..)`, an
/// attribute modifiers change) with `World::attack`, which keeps the pace. Never takes over:
/// chasing goes on.
pub fn melee() -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| {
        let reach = w.attribute(id, &crate::attribute::ENTITY_REACH);
        if let Some((p, _)) = target(w, id, st, reach + st.size.width as f64 + 2.0) {
            if w.in_reach(id, p) {
                w.attack(id, p);
            }
        }
        false
    }
}

/// `melee` with a reach of its own instead of the attribute, from its eyes to the target's box.
pub fn melee_within(reach: f64) -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| {
        if let Some((p, ps)) = target(w, id, st, reach + 2.0) {
            let eye = st.eye();
            let half = ps.size.width as f64 / 2.0;
            let near = crate::Vec3::new(
                eye.x.clamp(ps.pos.x - half, ps.pos.x + half),
                eye.y.clamp(ps.pos.y, ps.pos.y + ps.size.height as f64),
                eye.z.clamp(ps.pos.z - half, ps.pos.z + half),
            );
            let d2 = (near - eye).length_squared();
            if d2 <= reach * reach {
                w.attack(id, p);
            }
        }
        false
    }
}

/// Walks to its `target` within `range` at `speed`.
pub fn chase_target(range: f64, speed: f32) -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| match target(w, id, st, range) {
        Some((_, p)) => {
            w.navigate(id, p.pos, speed);
            true
        }
        None => false,
    }
}
