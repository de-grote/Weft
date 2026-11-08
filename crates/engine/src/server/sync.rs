//! What each player hears every tick: cubes, entities, their own state, an open menu.

use super::*;

impl Server {
    pub(super) fn sync_players(&mut self, c: &'static Content) {
        let mining_now = self.mining_now(c);
        let settled: Vec<Vec<IVec3>> = self.dims.iter_mut().map(|d| d.take_settled()).collect();
        let moved = self.collect_moves(c);

        let mut unsent = 0u64;
        for pi in 0..self.players.len() {
            if !self.players[pi].joined {
                continue;
            }
            let view = self.view(&self.players[pi]);
            let full = self.full_view(&self.players[pi]);
            let Server { players, dims, tick, storage, tick_rate, .. } = self;
            let p = &mut players[pi];
            let dim = &mut dims[p.dim as usize];
            let d = p.dim as usize;
            send_far(c, p, dim, storage.as_ref(), full, *tick);
            send_block_changes(p, dim);
            send_mining(p, &mining_now[d], *tick);
            send_clock(p, dim, *tick, *tick_rate);
            let viewer = dim
                .entity(p.entity)
                .map_or((DVec3::ZERO, GameMode::SURVIVAL), |e| (e.pos, e.mode));
            send_entities(c, p, dim, &moved[d], *tick, view.radius, viewer);
            send_own_state(c, p, dim);
            if !p.gizmos.is_empty() {
                let shapes = gizmos_for(p, dim, *tick, viewer.0);
                if !shapes.is_empty() {
                    p.conn.send(ToClient::Gizmos(shapes));
                }
            }
            unsent += send_cubes(p, dim, &c.dims[d], view, full, &settled[d]);
        }
        self.shared.unsent.store(unsent, Ordering::Relaxed);
        let pending = self.backlog.len() + self.dims.iter().map(|d| d.generating.len()).sum::<usize>();
        self.shared.pending.store(pending as u64, Ordering::Relaxed);
        self.clear_tick_changes();
        self.unload_unused(c);
    }

    /// Mining goes on by what each one holds and where it is this tick. Returns who is breaking
    /// what, in tenths as crack pictures go, so most ticks nothing is sent.
    fn mining_now(&mut self, c: &'static Content) -> Vec<Vec<crate::net::Crack>> {
        // This also runs between ticks, where nothing goes on.
        let ticked = std::mem::replace(&mut self.mining_tick, self.tick) != self.tick;
        let mut mining_now: Vec<Vec<crate::net::Crack>> = vec![Vec::new(); self.dims.len()];
        for pi in 0..self.players.len() {
            let Some(m) = self.players[pi].mining.filter(|_| self.players[pi].joined) else { continue };
            let Some((r, id)) = self.region(pi) else { continue };
            let rate = if ticked { r.mining_rate_of(c, id, m.frame, m.pos, m.part) } else { 0.0 };
            let progress = (m.progress + rate).min(1.0);
            let p = &mut self.players[pi];
            p.mining = Some(Mining { progress, ..m });
            if m.frame == 0 {
                mining_now[p.dim as usize].push((p.entity, m.pos, m.part, (progress.min(0.99) * 10.0).floor() / 10.0));
            }
        }
        mining_now
    }

    /// Entities that moved since last tick, per dimension (all regions).
    fn collect_moves(&mut self, c: &'static Content) -> Vec<Vec<crate::net::Move>> {
        let mut moved: Vec<Vec<crate::net::Move>> = Vec::with_capacity(self.dims.len());
        for d in &mut self.dims {
            let mut m = Vec::new();
            for r in &mut d.regions {
                let changed: Vec<u64> = r.entities.dirty_inv.iter().copied().collect();
                for id in changed {
                    let now = worn_of(c, &r.entities, id);
                    if let Some(e) = r.entities.get_mut(id) {
                        e.worn = now;
                    }
                }
                r.mirror_fields(c);
                r.entities.find_changes(&c.game.synced_comps);
                for e in r
                    .entities
                    .list
                    .iter_mut()
                    .filter(|e| !e.removed && e.anchor.is_none())
                {
                    let now = (e.pos, e.yaw, e.pitch, e.pose, e.sprinting);
                    // Solid bodies every tick: clients show them between server positions, and a
                    // body that stops must say so (two equal positions).
                    let solid = c.game.entities[e.kind as usize].is(crate::tag_bits::entity::SOLID);
                    if solid
                        || e.sent.is_none_or(|s| {
                            (s.0 - now.0).length_squared() > 1e-6
                                || (s.1 - now.1).abs() > 0.01
                                || (s.2 - now.2).abs() > 0.01
                                || s.3 != now.3
                                || s.4 != now.4
                        })
                    {
                        if e.sent.is_some() {
                            m.push((e.id, e.pos, e.yaw, e.pitch, e.pose, e.sprinting, e.frame.unwrap_or(0)));
                        }
                        e.sent = Some(now);
                    }
                }
            }
            moved.push(m);
        }
        moved
    }

    /// This tick's lists were heard by everyone.
    fn clear_tick_changes(&mut self) {
        for d in &mut self.dims {
            d.day_moved = false;
            d.weather_moved = false;
            for r in &mut d.regions {
                for st in r.bodies.values_mut() {
                    st.resend.clear();
                    st.changes.clear();
                    st.extra_changes.clear();
                    st.signals.level_changes.clear();
                    st.edited.clear();
                    st.world_light_changed = false;
                }
                r.space.resend.clear();
                r.space.body_light_changes.clear();
                r.space.changes.clear();
                r.space.extra_changes.clear();
                r.space.signals.level_changes.clear();
                r.entities.killed.clear();
                r.entities.taken.clear();
                r.entities.told.clear();
                r.entities.dirty_comp.clear();
                r.gizmos.clear();
                // Block entity inventory changes must reach the save.
                let anchored: Vec<IVec3> = r
                    .entities
                    .list
                    .iter()
                    .filter(|e| e.anchor.is_some() && r.entities.dirty_inv.contains(&e.id))
                    .map(|e| cube_of(e.pos.floor().as_ivec3()))
                    .collect();
                r.space.modified.extend(anchored);
                r.entities.dirty_inv.clear();
            }
        }
    }
}

fn send_block_changes(p: &mut Player, dim: &DimSet) {
    for reg in &dim.regions {
        // For what the client works out itself; all of it when it gets here, then what changes.
        let glow: Vec<(IVec3, [u8; 3])> =
            if !p.glow_sent { reg.space.body_light.iter().map(|(c, v)| (*c, *v)).collect() } else { reg.space.body_light_changes.clone() };
        if !glow.is_empty() {
            p.conn.send(ToClient::BodyGlow { dim: p.dim, cells: glow });
        }
        for (pos, cube) in &reg.space.resend {
            if p.sent.contains(pos) {
                p.conn
                    .send(ToClient::Cube { dim: p.dim, frame: 0, pos: *pos, cube: cube.clone(), edit: true });
            }
        }
        if !reg.space.changes.is_empty() || !reg.space.extra_changes.is_empty() {
            let changes: Vec<(IVec3, u32)> = reg
                .space
                .changes
                .iter()
                .filter(|(b, _)| p.sent.contains(&(*b >> 4)))
                .copied()
                .collect();
            let extras: Vec<(IVec3, Vec<u32>)> = reg
                .space
                .extra_changes
                .iter()
                .filter(|(b, _)| p.sent.contains(&(*b >> 4)))
                .cloned()
                .collect();
            if !changes.is_empty() || !extras.is_empty() {
                p.conn
                    .send(ToClient::Blocks { dim: p.dim, frame: 0, changes, extras });
            }
        }
        if !reg.space.signals.level_changes.is_empty() {
            let changes: Vec<(IVec3, u8, u8)> = reg
                .space
                .signals
                .level_changes
                .iter()
                .filter(|(b, ..)| p.sent.contains(&(*b >> 4)))
                .copied()
                .collect();
            if !changes.is_empty() {
                p.conn
                    .send(ToClient::Signals { dim: p.dim, frame: 0, changes });
            }
        }
    }
    p.glow_sent = true;
}

/// Everyone else's mining in this dimension: cracks for those near, and their arms.
fn send_mining(p: &mut Player, cracks: &[crate::net::Crack], tick: u64) {
    let mine: Vec<crate::net::Crack> = cracks
        .iter()
        .copied()
        .filter(|m| m.0 != p.entity && p.known.contains(&m.0))
        .collect();
    if mine != p.sent_cracks {
        p.conn.send(ToClient::Cracks(mine.clone()));
        p.sent_cracks = mine;
    }
    if tick % 5 == 0 {
        for m in cracks
            .iter()
            .filter(|m| m.0 != p.entity && p.known.contains(&m.0))
        {
            p.conn.send(ToClient::EntitySwing { id: m.0, off: false });
        }
    }
}

fn send_clock(p: &mut Player, dim: &DimSet, tick: u64, rate: u32) {
    if p.first_sync || p.sent_rate != rate {
        p.sent_rate = rate;
        p.conn.send(ToClient::TickRate(rate));
    }
    if dim.weather_moved || p.first_sync {
        p.conn.send(ToClient::Weather(dim.weather as u32));
    }
    if tick % TPS as u64 == 0 || dim.day_moved {
        p.conn
            .send(ToClient::Time { dim: p.dim, time: dim.day_time() });
    }
}

/// Entities in range: spawn new ones, despawn gone ones, stream movement.
fn send_entities(
    c: &'static Content,
    p: &mut Player,
    dim: &DimSet,
    moved: &[crate::net::Move],
    tick: u64,
    radius: i32,
    (me, me_mode): (DVec3, GameMode),
) {
    let range2 = ((radius * 16) as f64).powi(2);
    let mut spawn = Vec::new();
    let mut visible = FxHashSet::default();
    for reg in &dim.regions {
        // Spectators are seen only by other spectators.
        // Hidden ones are seen by the hidden alone.
        let hidden = |m: GameMode| m.has(crate::tag_bits::game_mode::HIDDEN);
        let shown = |e: &&Entity| !hidden(e.mode) || hidden(me_mode);
        // A dead player is gone for the others (its death animation plays) until it respawns.
        for e in reg
            .entities
            .list
            .iter()
            .filter(|e| e.in_world() && e.id != p.entity && (!e.player || e.alive()))
            .filter(shown)
        {
            if (e.pos - me).length_squared() > range2 {
                continue;
            }
            visible.insert(e.id);
            if !p.known.contains(&e.id) {
                spawn.push(EntityView {
                    id: e.id,
                    kind: e.kind,
                    pos: e.pos,
                    yaw: e.yaw,
                    pitch: e.pitch,
                    item: crate::stack::for_client(e.item),
                });
            }
        }
    }
    let taken: Vec<(u64, u64)> = dim
        .regions
        .iter()
        .flat_map(|r| r.entities.taken.iter())
        .filter(|t| p.known.contains(&t.0))
        .copied()
        .collect();
    if !taken.is_empty() {
        p.conn.send(ToClient::Taken(taken));
    }
    // A mob killed this tick is gone already: the flag starts its death animation.
    let killed = |id: &u64| dim.regions.iter().any(|r| r.entities.killed.contains(id));
    let gone: Vec<(u64, bool)> = p
        .known
        .iter()
        .copied()
        .filter(|id| !visible.contains(id))
        .map(|id| (id, killed(&id)))
        .collect();
    if !gone.is_empty() {
        p.conn.send(ToClient::Despawn(gone));
    }
    // Components every viewer gets: all of newly seen entities, changes of known ones.
    let mut shared: Vec<(u64, u32, Vec<u8>)> = Vec::new();
    for reg in &dim.regions {
        for &(id, comp) in &reg.entities.dirty_comp {
            if id != p.entity
                && p.known.contains(&id)
                && visible.contains(&id)
                && c.game.components[comp as usize].flags & cf::SYNC_WATCHERS != 0
            {
                shared.extend(
                    reg.entities
                        .comps
                        .get(&(id, comp))
                        .map(|b| (id, comp, b.to_vec())),
                );
            }
        }
        for v in &spawn {
            for &comp in &c.game.watched_comps {
                shared.extend(
                    reg.entities
                        .comps
                        .get(&(v.id, comp))
                        .map(|b| (v.id, comp, b.to_vec())),
                );
            }
        }
    }
    // Bodies: all their cubes with the entity, then what changes.
    let mut bodies = Vec::new();
    for reg in &dim.regions {
        for (&id, st) in &reg.bodies {
            if spawn.iter().any(|v| v.id == id) {
                bodies.push(ToClient::BodyLight { frame: id, lights: st.world_light.iter().map(|(p, l)| (*p, *l)).collect() });
                for (pos, cube) in &st.cubes {
                    bodies.push(ToClient::Cube { dim: p.dim, frame: id, pos: *pos, cube: cube.clone(), edit: false });
                    let changes = st.signals.levels_in(c, *pos, cube);
                    if !changes.is_empty() {
                        bodies.push(ToClient::Signals { dim: p.dim, frame: id, changes });
                    }
                }
            } else if p.known.contains(&id) && visible.contains(&id) {
                bodies.extend(st.resend.iter().map(|(pos, cube)| ToClient::Cube {
                    dim: p.dim,
                    frame: id,
                    pos: *pos,
                    cube: cube.clone(),
                    edit: true,
                }));
                if st.world_light_changed {
                    bodies.push(ToClient::BodyLight { frame: id, lights: st.world_light.iter().map(|(p, l)| (*p, *l)).collect() });
                }
                if !st.changes.is_empty() || !st.extra_changes.is_empty() {
                    bodies.push(ToClient::Blocks { dim: p.dim, frame: id, changes: st.changes.clone(), extras: st.extra_changes.clone() });
                }
                if !st.signals.level_changes.is_empty() {
                    bodies.push(ToClient::Signals { dim: p.dim, frame: id, changes: st.signals.level_changes.clone() });
                }
            }
        }
    }
    if !spawn.is_empty() {
        p.conn.send(ToClient::Spawn(spawn));
    }
    for m in bodies {
        p.conn.send(m);
    }
    if !shared.is_empty() {
        p.conn.send(ToClient::EntityComponents(shared));
    }
    let moves: Vec<crate::net::Move> = moved
        .iter()
        .filter(|m| p.known.contains(&m.0) && visible.contains(&m.0))
        .copied()
        .collect();
    if !moves.is_empty() {
        p.conn.send(ToClient::Moves { tick, moves });
    }
    p.known = visible;
}

/// What only the player itself hears: pushes, movement numbers, inventory, components, its menu.
fn send_own_state(c: &'static Content, p: &mut Player, dim: &mut DimSet) {
    let Some(reg) = dim.entity_region(p.entity) else { return };
    for &(_, told) in reg.entities.told.iter().filter(|h| h.0 == p.entity) {
        p.conn.send(match told {
            entity::Told::Knock(away) => ToClient::Knock { away },
            entity::Told::Add(add) => ToClient::Velocity { add },
            entity::Told::Velocity(v) => ToClient::SetVelocity(v),
            entity::Told::Look(look) => ToClient::Look(look),
            entity::Told::Flying(on) => ToClient::Flying(on),
            entity::Told::Selected(slot) => ToClient::Selected(slot),
        });
    }
    // The client moves its player and times its mining: it needs them all.
    if p.sent_attributes.is_empty() {
        p.sent_attributes = vec![f32::NAN; c.game.attributes.len()];
    }
    let attributes: Vec<(u32, f32)> = (0..c.game.attributes.len() as u32)
        .map(|a| (a, reg.attribute(c, p.entity, a, false) as f32))
        .filter(|&(a, v)| p.sent_attributes[a as usize] != v)
        .collect();
    if !attributes.is_empty() {
        for &(a, v) in &attributes {
            p.sent_attributes[a as usize] = v;
        }
        p.conn.send(ToClient::Attributes(attributes));
    }
    // Own inventory and synced components.
    if p.first_sync || reg.entities.dirty_inv.contains(&p.entity) {
        if let Some(inv) = reg.entities.inventories.get(&p.entity) {
            p.conn.send(ToClient::Inventory(
                inv.iter()
                    .enumerate()
                    .map(|(i, s)| (i as u32, crate::stack::for_client(*s)))
                    .collect(),
            ));
        }
    }
    let comps: Vec<(u32, Vec<u8>)> = reg
        .entities
        .comps
        .iter()
        .filter(|((id, comp), _)| {
            *id == p.entity
                && c.game.components[*comp as usize].flags & cf::SYNC_OWNER != 0
                && (p.first_sync || reg.entities.dirty_comp.contains(&(*id, *comp)))
        })
        .map(|((_, comp), b)| (*comp, b.to_vec()))
        .collect();
    if !comps.is_empty() {
        p.conn.send(ToClient::Components(comps));
    }
    p.first_sync = false;
    if let Some(m) = &mut p.menu {
        // The machine's menu-synced components, all at first, then what changed.
        let owner = m.owner();
        let first = !std::mem::replace(&mut m.comps_sent, true);
        let comps: Vec<(u64, u32, Vec<u8>)> = c
            .game
            .menu_comps
            .iter()
            .filter(|&&comp| owner != 0 && (first || reg.entities.dirty_comp.contains(&(owner, comp))))
            .filter_map(|&comp| {
                reg.entities
                    .comps
                    .get(&(owner, comp))
                    .map(|b| (owner, comp, b.to_vec()))
            })
            .collect();
        if !comps.is_empty() {
            p.conn.send(ToClient::EntityComponents(comps));
        }
        let (slots, cursor) = menu::changes(m, reg, p.entity);
        if !slots.is_empty() {
            p.conn.send(ToClient::MenuSlots(slots));
        }
        if let Some(cur) = cursor {
            p.conn.send(ToClient::Cursor(cur));
        }
    }
}

/// Cubes that came into view, nearest first, and unloading those far behind. Returns how many are
/// still waiting for their turn.
fn send_cubes(p: &mut Player, dim: &DimSet, def: &crate::content::DimensionDef, view: View, full: View, settled: &[IVec3]) -> u64 {
    let (center, r, h, down) = (view.center, view.radius, view.up, view.down);
    let in_range = |pos: IVec3| view.contains_in(def, pos);
    let moved = p.sync_range != Some(view);
    if moved {
        // Settled cubes that came into range; the ones that left are unloaded below.
        let was = p.sync_range;
        p.unsent.retain(|&q| in_range(q));
        for dy in -down..=h {
            for dz in -r..=r {
                for dx in -r..=r {
                    let pos = center + IVec3::new(dx, dy, dz);
                    if in_range(pos) && !was.is_some_and(|w| w.contains(pos)) && !p.sent.contains(&pos) && dim.settled(pos) {
                        p.unsent.insert(pos);
                    }
                }
            }
        }
        p.sync_range = Some(view);
    }
    p.unsent.extend(
        settled
            .iter()
            .copied()
            .filter(|&q| in_range(q) && !p.sent.contains(&q)),
    );
    // Send a cube once its neighbors exist and its sky light is final.
    let mut to_send: Vec<(i32, IVec3)> = p
        .unsent
        .iter()
        .copied()
        .filter(|&pos| {
            crate::world::DIRS.iter().all(|d| {
                let n = pos + *d;
                dim.has(n) || !in_range(n) || !p.vis.as_ref().is_some_and(|v| v.seen.contains(&n))
            })
        })
        .map(|pos| {
            let d = pos - center;
            (d.x * d.x + d.z * d.z + d.y * d.y, pos)
        })
        .collect();
    to_send.sort_by_key(|t| t.0);
    let unsent = to_send.len().saturating_sub(SEND_PER_TICK) as u64;
    for &(_, pos) in to_send.iter().take(SEND_PER_TICK) {
        if let Some(cube) = dim.snapshot(pos) {
            p.conn
                .send(ToClient::Cube { frame: 0, dim: p.dim, pos, cube, edit: false });
            p.send_levels(dim, pos);
            p.sent.insert(pos);
            p.unsent.remove(&pos);
        }
    }
    if !moved {
        return unsent;
    }
    // A cube more around it, against flicker at the edge.
    let keep = full.grown(2);
    let (_, hi) = full.loaded_y(def);
    let gone = |pos: IVec3| !(keep.column(pos) && pos.y >= keep.center.y - keep.down && pos.y <= hi + 2);
    p.announced.retain(|&q| !gone(q));
    let far: Vec<IVec3> = p.sent.iter().copied().filter(|&q| gone(q)).collect();
    for pos in far {
        p.sent.remove(&pos);
        p.conn.send(ToClient::Unload { dim: p.dim, pos });
    }
    unsent
}

/// Gizmo shapes for one player: what mods drew in the groups it shows, and every second the
/// engine's server-side groups.
pub(super) fn gizmos_for(p: &Player, dim: &DimSet, tick: u64, me: DVec3) -> Vec<crate::net::GizmoShape> {
    use crate::net::GizmoShape;
    let mut out: Vec<GizmoShape> = Vec::new();
    for r in &dim.regions {
        out.extend(
            r.gizmos
                .iter()
                .filter(|g| p.gizmos.contains(&g.group))
                .cloned(),
        );
    }
    if tick % TPS as u64 != 0 {
        return out;
    }
    let on = |g: &str| p.gizmos.iter().any(|x| x == g);
    let shape = |group: &str, kind: u8, a: DVec3, b: DVec3, color: u32| GizmoShape::shape(group, kind, a, b, color, TPS + 2);
    if on("regions") {
        const COLORS: [u32; 6] = [0xFF4040FF, 0x40FF40FF, 0x4080FFFF, 0xFFFF40FF, 0xFF40FFFF, 0x40FFFFFF];
        for (i, r) in dim.regions.iter().enumerate() {
            let (Some(lo), Some(hi)) = (r.space.cubes.keys().copied().reduce(IVec3::min), r.space.cubes.keys().copied().reduce(IVec3::max))
            else {
                continue;
            };
            let (a, b) = ((lo * 16).as_dvec3() + 0.05, ((hi + 1) * 16).as_dvec3() - 0.05);
            out.push(shape("regions", 1, a, b, COLORS[i % COLORS.len()]));
        }
    }
    if let Some(v) = p.vis.as_ref().filter(|_| on("visibility")) {
        let here: IVec3 = me.floor().as_ivec3() >> 4;
        for &c in &v.seen {
            let d = c - here;
            if d.x.abs() > 8 || d.z.abs() > 8 || d.y.abs() > 4 {
                continue;
            }
            let color = if dim.has(c) { 0x40FF60FF } else { 0xFFA030FF };
            let m = (c * 16).as_dvec3() + 8.0;
            for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
                out.push(shape("visibility", 0, m - axis, m + axis, color));
            }
        }
    }
    out
}

impl Player {
    /// What has power in a cube this player just got.
    pub(super) fn send_levels(&mut self, dim: &DimSet, pos: IVec3) {
        if let Some(r) = dim
            .regions
            .iter()
            .find(|r| r.space.cubes.contains_key(&pos))
        {
            let changes = r
                .space
                .signals
                .levels_in(crate::content::content(), pos, r.space.cubes[&pos].as_ref());
            if !changes.is_empty() {
                self.conn
                    .send(ToClient::Signals { dim: self.dim, frame: 0, changes });
            }
        }
    }
}
