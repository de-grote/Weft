//! Getting cubes: asking for what players' views need, taking in what was made, unloading the rest.

use super::*;

impl Server {
    /// Inserts generated cubes and keeps the workers fed until `deadline`, between ticks.
    pub(super) fn load_until(&mut self, deadline: Instant) {
        let c = content();
        let mut refilled = Instant::now();
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let busy = |s: &Self| !s.backlog.is_empty() || s.dims.iter().any(|d| !d.generating.is_empty());
            if !busy(self) {
                // What just arrived may have opened up more to load: look before sleeping.
                self.request_generation(c);
                refilled = Instant::now();
                if !busy(self) {
                    break;
                }
            }
            let before = self.backlog.len();
            self.receive_generated(c, deadline);
            // Keep the queue full without redoing the visibility walk on every pass.
            if now - refilled > Duration::from_millis(5) {
                self.request_generation(c);
                refilled = Instant::now();
            }
            if self.backlog.len() == before {
                // Nothing could be inserted: wait for the next cube from the workers.
                match self
                    .gen_rx
                    .recv_timeout((deadline - Instant::now().min(deadline)).min(Duration::from_millis(2)))
                {
                    Ok(g) => self.backlog.push_back(g),
                    Err(_) => continue,
                }
            }
        }
    }

    /// `load` channel: where the pipeline stands; at trace level also why waiting cubes wait.
    pub(super) fn report_load(&mut self) {
        let sh = &self.shared;
        let (made, put) = (sh.generated.load(Ordering::Relaxed), sh.inserted.load(Ordering::Relaxed));
        let (made_before, put_before) = std::mem::replace(&mut self.load_seen, (made, put));
        let generating: usize = self.dims.iter().map(|d| d.generating.len()).sum();
        log::debug!(
            target: "load",
            "generating {generating}, waiting to insert {}, wanted {}, unsent {} | last second: {} generated, {} inserted",
            self.backlog.len(),
            self.wanted.len(),
            sh.unsent.load(Ordering::Relaxed),
            made - made_before,
            put - put_before,
        );
        if !crate::log::on(crate::log::Channel::Load, log::Level::Trace) || self.backlog.is_empty() {
            return;
        }
        // Same rule as `receive_generated`: a cube waits for the one above it.
        let queued: FxHashSet<(u32, IVec3)> = self.backlog.iter().map(|g| (g.dim, g.pos)).collect();
        let (mut above_queued, mut above_generating, mut above_unrequested, mut ready) = (0, 0, 0, 0);
        let mut stuck = Vec::new();
        for g in &self.backlog {
            let above = (g.dim, g.pos + IVec3::Y);
            if queued.contains(&above) {
                above_queued += 1;
            } else if self.dims[g.dim as usize].generating.contains(&above.1) {
                above_generating += 1;
            } else if self.wanted.contains(&above) {
                above_unrequested += 1;
                if stuck.len() < 5 {
                    stuck.push(format!("{} {}", g.dim, g.pos));
                }
            } else {
                ready += 1;
            }
        }
        log::trace!(
            target: "load",
            "waiting: {above_queued} for a queued cube above, {above_generating} for one generating, {above_unrequested} for one not requested yet {stuck:?}, {ready} ready (out of time)"
        );
    }

    pub(super) fn receive_generated(&mut self, c: &'static Content, deadline: Instant) {
        self.backlog.extend(self.gen_rx.try_iter());
        // Top-down so sky light is right the first time: a cube waits while the one above it is
        // still generating or waiting itself. Sorted to keep runs reproducible.
        let mut list: Vec<Generated> = self.backlog.drain(..).collect();
        list.sort_by_key(|g| (g.dim, -g.pos.y, g.pos.x, g.pos.z));
        let mut pending: FxHashSet<(u32, IVec3)> = list.iter().map(|g| (g.dim, g.pos)).collect();
        for g in list {
            if g.cube.is_none() {
                self.dims[g.dim as usize].generating.remove(&g.pos);
                pending.remove(&(g.dim, g.pos));
                continue;
            }
            let above = (g.dim, g.pos + IVec3::Y);
            let late = Instant::now() >= deadline;
            let above_coming =
                pending.contains(&above) || self.dims[g.dim as usize].generating.contains(&above.1) || self.wanted.contains(&above);
            if late || above_coming {
                self.backlog.push_back(g);
                continue;
            }
            pending.remove(&(g.dim, g.pos));
            self.wanted.remove(&(g.dim, g.pos));
            let t = Instant::now();
            self.dims[g.dim as usize].insert(c, g.pos, g.cube.unwrap(), &g.entities, g.conn, true);
            self.shared
                .insert_ns
                .fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
            self.shared.inserted.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Cubes players need, nearest columns first and each column top-down; loading and
    /// generation run on the rayon pool.
    pub(super) fn request_generation(&mut self, c: &'static Content) {
        // Enough jobs to keep every worker busy for a whole tick; the backlog counts too so
        // generation never runs far ahead of lighting.
        let budget = crate::background().current_num_threads() * 64;
        let queued: FxHashSet<(u32, IVec3)> = self.backlog.iter().map(|g| (g.dim, g.pos)).collect();
        // Cubes the backlog waits on (the ones above, see `receive_generated`) go first, whatever
        // the budget: a backlog that fills the budget waiting for them would stall loading for good.
        let mut blockers: Vec<(u32, IVec3)> = self
            .backlog
            .iter()
            .map(|g| (g.dim, g.pos + IVec3::Y))
            .filter(|&(d, p)| self.wanted.contains(&(d, p)) && !self.dims[d as usize].generating.contains(&p) && !queued.contains(&(d, p)))
            .collect();
        blockers.sort_unstable_by_key(|&(d, p)| (d, p.x, p.y, p.z));
        blockers.dedup();
        for (d, p) in blockers {
            self.spawn_generation(d, p);
        }
        let in_flight: usize = self.dims.iter().map(|d| d.generating.len()).sum::<usize>() + self.backlog.len();
        if in_flight >= budget {
            return;
        }
        *self.shared.views.write() = self
            .players
            .iter()
            .filter(|p| p.joined)
            .map(|p| (p.dim, self.view(p)))
            .collect();
        let mut wanted: Vec<(i32, u32, IVec3)> = Vec::new();
        self.wanted.clear();
        let edited: Vec<Vec<IVec3>> = self.dims.iter_mut().map(|d| d.take_edited()).collect();
        for pi in 0..self.players.len() {
            let p = &self.players[pi];
            if !p.joined {
                continue;
            }
            let view = self.view(p);
            let (center, r, h) = (view.center, view.radius, view.up);
            let pdim = p.dim;
            let edited = &edited[pdim as usize];
            let key = (pdim, view, self.dims[pdim as usize].settled_version());
            if p.gen_done == Some(key) && edited.is_empty() {
                continue;
            }
            let def = &c.dims[pdim as usize];
            let (_, hi) = view.loaded_y(def);
            let in_view = |pos: IVec3| view.contains_in(def, pos);
            let Server { players, dims, wanted: wanted_set, .. } = &mut *self;
            let dim = &mut dims[pdim as usize];
            let p = &mut players[pi];
            // The visibility fill follows the player and goes on from cubes that arrived.
            let vis = p.vis.get_or_insert_with(visibility::Visibility::new);
            let (arrived, left) = vis.update(dim, c, view, &in_view, NEAR, edited);
            let vis = p.vis.as_ref().unwrap();
            let mut missing: Vec<IVec3> = vis.waiting.keys().copied().collect();
            // Above the view the column is loaded up to open sky, so light coming down is right.
            let sky_column = |pos: IVec3| view.column(pos) && pos.y > center.y + h && pos.y <= hi;
            for y in center.y + h + 1..=hi {
                for dz in -r..=r {
                    for dx in -r..=r {
                        let pos = IVec3::new(center.x + dx, y, center.z + dz);
                        if sky_column(pos) && !dim.has(pos) {
                            missing.push(pos);
                        }
                    }
                }
            }
            // Columns capped by ground nobody will load are dark from above: their light is final.
            // Candidates: cubes just reached, and the ones under cubes that left the view.
            let tops: Vec<IVec3> = arrived
                .iter()
                .copied()
                .chain(left.iter().map(|&q| q - IVec3::Y))
                .filter(|&q| {
                    dim.has(q)
                        && !dim.settled(q)
                        && !dim.has(q + IVec3::Y)
                        && !vis.seen.contains(&(q + IVec3::Y))
                        && !sky_column(q + IVec3::Y)
                })
                .collect();
            for t in tops {
                dim.settle_down(t);
            }
            // Everything above a wanted cube up to the top of the range comes first, so light
            // always arrives from above before a cube is lit; otherwise it would be lit dark and
            // the whole column relit when the sky shows up. Solid cubes cost almost nothing.
            if def.sky_light {
                let mut seen: FxHashSet<IVec3> = missing.iter().copied().collect();
                let mut above = Vec::new();
                for &pos in &missing {
                    let mut q = pos + IVec3::Y;
                    while q.y <= hi && !dim.has(q) && seen.insert(q) {
                        above.push(q);
                        q += IVec3::Y;
                    }
                }
                missing.extend(above);
            }
            for &pos in &missing {
                if !dim.generating.contains(&pos) && !queued.contains(&(pdim, pos)) {
                    wanted.push((view.order(pos), pdim, pos));
                    wanted_set.insert((pdim, pos));
                }
            }
            self.players[pi].gen_done = missing.is_empty().then_some(key);
        }
        // What tickets keep, after what players see.
        for d in 0..self.dims.len() as u32 {
            let dim = &self.dims[d as usize];
            for pos in self.forced_missing(d, |p| dim.has(p)) {
                if !dim.generating.contains(&pos) && !queued.contains(&(d, pos)) {
                    wanted.push((i32::MAX, d, pos));
                    self.wanted.insert((d, pos));
                }
            }
        }
        if wanted.is_empty() {
            return;
        }
        wanted.sort_by_key(|w| w.0);
        wanted.dedup_by_key(|w| (w.1, w.2));
        for &(_, dim, pos) in wanted.iter().take(budget - in_flight) {
            self.spawn_generation(dim, pos);
        }
    }

    /// Loads or generates a cube on the worker pool; it arrives through `gen_rx`.
    pub(super) fn spawn_generation(&mut self, dim: u32, pos: IVec3) {
        self.dims[dim as usize].generating.insert(pos);
        let tx = self.gen_tx.clone();
        let seed = self.cfg.seed;
        let storage = self.storage.clone();
        let shared = self.shared.clone();
        let edits = self.dims[dim as usize].edits.clone();
        crate::background().spawn(move || {
            // Columns go up to open sky, so nothing above the view's floor counts as left behind.
            let wanted = shared.views.read().iter().any(|&(d, v)| {
                let v = v.grown(2);
                d == dim && v.column(pos) && pos.y >= v.center.y - v.down
            }) || shared
                .forced
                .read()
                .iter()
                .any(|&(d, lo, hi)| d == dim && pos.cmpge(lo).all() && pos.cmple(hi).all());
            if !wanted {
                let _ = tx.send(Generated { dim, pos, cube: None, entities: Vec::new(), conn: 0 });
                return;
            }
            let t = Instant::now();
            let loaded = storage.and_then(|s| s.lock().load(dim, pos));
            let (mut cube, entities) = loaded.unwrap_or_else(|| (dim::generate(content(), dim, seed, pos), Vec::new()));
            dim::prepare(content(), dim, &edits, pos, &mut cube);
            let conn = dim::see_through(content(), &cube);
            shared
                .gen_ns
                .fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
            shared.generated.fetch_add(1, Ordering::Relaxed);
            let _ = tx.send(Generated { dim, pos, cube: Some(cube), entities, conn });
        });
    }

    /// Every half second, so a fast flight drops a few thousand cubes at a time, not tens.
    pub(super) fn unload_unused(&mut self, c: &'static Content) {
        if self.tick % 10 != 0 {
            return;
        }
        self.expire_forced();
        for di in 0..self.dims.len() {
            let def = &c.dims[di];
            let views: Vec<(View, i32, i32)> = self
                .players
                .iter()
                .filter(|p| p.joined && p.dim as usize == di)
                .map(|p| {
                    let view = self.view(p);
                    let (lo, hi) = view.loaded_y(def);
                    (view.grown(3), lo - 3, hi + 3)
                })
                .collect();
            let near = |pos: IVec3| {
                views
                    .iter()
                    .any(|&(v, lo, hi)| v.column(pos) && pos.y >= lo && pos.y <= hi)
            };
            let idle = self.idle_cubes(di as u32, near);
            let tickets: Vec<super::forced::Ticket> = self.forced[di].values().copied().collect();
            let storage = self.storage.clone();
            let d = &mut self.dims[di];
            for r in &mut d.regions {
                r.idle = idle.clone();
            }
            let before: usize = d.regions.iter().map(|r| r.space.cubes.len()).sum();
            for r in &mut d.regions {
                let removed = r.retain_cubes(c, |pos| {
                    views
                        .iter()
                        .any(|&(v, lo, hi)| v.column(pos) && pos.y >= lo && pos.y <= hi)
                        || tickets.iter().any(|t| t.contains(pos))
                });
                let around: Vec<_> = removed
                    .iter()
                    .map(|x| r.around(x.0, |p| removed.iter().find(|o| o.0 == p).map(|o| o.1.clone())))
                    .collect();
                for ((pos, cube, inside), around) in removed.into_iter().zip(around) {
                    if let Some(s) = &storage {
                        // Written in the background; loads queue behind it there.
                        let (s, mut bytes, dim) = (s.clone(), serialize(c, r, &inside), r.id);
                        persist::append_ticks(&mut bytes, &r.ticks_in(pos));
                        crate::background().spawn(move || s.lock().save(c, dim, pos, &cube, &around, &bytes));
                    }
                    for id in inside {
                        r.entities.remove(id);
                    }
                }
                r.entities.compact();
            }
            d.dirty |= d.regions.iter().map(|r| r.space.cubes.len()).sum::<usize>() != before;
        }
    }

    /// An area edit, with the cubes it opens up to players' views loaded first on every core:
    /// the visibility fill would reach them one wave of loading at a time otherwise (seconds of
    /// floating caves and holes into the sky). Cubes it fills whole are made, not generated.
    pub(super) fn area_fill(&mut self, c: &'static Content, dim: u32, fill: area::AreaFill) {
        let start = Instant::now();
        let views: Vec<View> = self
            .players
            .iter()
            .filter(|p| p.joined && p.dim == dim)
            .map(|p| self.view(p))
            .collect();
        let def = &c.dims[dim as usize];
        let in_view = |cp: IVec3| views.iter().any(|v| v.contains_in(def, cp));
        // Blocks it keeps may be anywhere in terrain: such cubes are generated.
        let keeps = fill.keeps_any();
        let d = &self.dims[dim as usize];
        let (lo, hi) = fill.shape.bounds();
        let (lo, hi) = (((lo - 17.0) / 16.0).floor().as_ivec3(), ((hi + 17.0) / 16.0).floor().as_ivec3());
        let (mut whole, mut load) = (Vec::new(), Vec::new());
        for z in lo.z..=hi.z {
            for y in lo.y..=hi.y {
                for x in lo.x..=hi.x {
                    let cp = IVec3::new(x, y, z);
                    // The cubes it changes and the ones next to them.
                    if !fill.near(cp, 17.0) || d.has(cp) || !in_view(cp) {
                        continue;
                    }
                    if fill.covers(cp) && !keeps {
                        whole.push(cp);
                    } else {
                        load.push(cp);
                    }
                }
            }
        }
        let (seed, storage, edits) = (self.cfg.seed, self.storage.clone(), &self.dims[dim as usize].edits);
        let mut made: Vec<(IVec3, Cube, Vec<u8>, u16)> = load
            .par_iter()
            .map(|&pos| {
                let loaded = storage.as_ref().and_then(|s| s.lock().load(dim, pos));
                let (mut cube, entities) = loaded.unwrap_or_else(|| (dim::generate(c, dim, seed, pos), Vec::new()));
                dim::prepare(c, dim, edits, pos, &mut cube);
                let conn = dim::see_through(c, &cube);
                (pos, cube, entities, conn)
            })
            .collect();
        made.sort_unstable_by_key(|m| (-m.0.y, m.0.x, m.0.z));
        let loaded = made.len();
        for (pos, cube, entities, conn) in made {
            self.dims[dim as usize].insert(c, pos, cube, &entities, conn, true);
        }
        let size = hi - lo + 1;
        log::info!(
            "area fill of {} cubes across: loaded {loaded} cubes it opens up, made {} it fills whole, {:.0} ms",
            size.max_element(),
            whole.len(),
            start.elapsed().as_secs_f64() * 1000.0
        );
        self.dims[dim as usize].area_fill(c, fill, &whole);
        if let Some(e) = self.dims[dim as usize].edits.last() {
            let spec = e.spec(c);
            for p in self.players.iter_mut().filter(|p| p.joined && p.dim == dim) {
                p.conn
                    .send(ToClient::Edits { dim, specs: vec![spec.clone()], all: false });
            }
        }
    }
}

/// The cubes in a player's view that differ from generation: told about first (the client
/// does not generate them), then sent, loaded ones as they are and the rest from storage with
/// the area edits made since they were saved.
pub(super) fn send_far(
    c: &'static Content,
    p: &mut Player,
    dim: &mut DimSet,
    storage: Option<&Arc<Mutex<Storage>>>,
    view: View,
    tick: u64,
) {
    // What the client generates: the view and the sky column above it.
    let def = &c.dims[p.dim as usize];
    let center = view.center;
    let in_view = |q: IVec3| view.loaded(def, q);
    if tick % 10 == 0 || p.announced.is_empty() {
        let mut new: Vec<IVec3> = dim
            .modified()
            .filter(|&q| in_view(q) && !p.announced.contains(&q))
            .collect();
        if let Some(s) = storage {
            new.extend(
                s.lock()
                    .saved(p.dim)
                    .iter()
                    .copied()
                    .filter(|&q| in_view(q) && !p.announced.contains(&q)),
            );
        }
        new.sort_unstable_by_key(|q| (q.x, q.y, q.z));
        new.dedup();
        if !new.is_empty() {
            p.announced.extend(new.iter().copied());
            p.conn
                .send(ToClient::Saved { dim: p.dim, cubes: new.clone() });
            p.to_send_far.extend(new);
            p.to_send_far
                .sort_unstable_by_key(|q| std::cmp::Reverse((*q - center).length_squared()));
        }
    }
    let mut n = 0;
    while n < FAR_PER_TICK {
        let Some(pos) = p.to_send_far.pop() else { break };
        if p.sent.contains(&pos) || !in_view(pos) {
            continue;
        }
        let cube = match dim.snapshot(pos) {
            Some(cube) => cube,
            None => {
                n += 1;
                let Some((mut cube, _)) = storage.and_then(|s| s.lock().load(p.dim, pos)) else { continue };
                dim.catch_up_edits(pos, &mut cube);
                Arc::new(cube)
            }
        };
        p.conn
            .send(ToClient::Cube { dim: p.dim, frame: 0, pos, cube, edit: false });
        p.send_levels(dim, pos);
        p.sent.insert(pos);
    }
}
