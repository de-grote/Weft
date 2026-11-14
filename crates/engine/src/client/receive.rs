//! What the server said and what the terrain thread finished, taken once a frame.

use super::*;

impl Client {
    pub(super) fn receive(&mut self) {
        // World messages go to the terrain thread, all of a frame together.
        let mut fwd = Vec::new();
        while let Some(msg) = self.conn.recv() {
            self.receive_message(msg, &mut fwd);
        }
        if !fwd.is_empty() {
            let _ = self.terrain.tx.send(terrain::In::Many(fwd));
        }
        self.take_terrain();
        if self.conn.is_closed() && self.disconnected.is_none() {
            self.disconnected = Some("connection lost".into());
        }
        for pos in std::mem::take(&mut self.world.removed) {
            self.render.remove((0, pos));
        }
    }

    fn receive_message(&mut self, msg: ToClient, fwd: &mut Vec<terrain::In>) {
        match msg {
            ToClient::Welcome { player, dim, pos, seed, look, selected, flying } => {
                self.player.flying = flying;
                self.player_id = player;
                (self.player.yaw, self.player.pitch) = (look[0], look[1]);
                self.selected = selected as usize;
                self.joined = true;
                self.player.pos = pos;
                self.player.prev = pos;
                self.seed = seed;
                self.reset_terrain(dim, fwd);
            }
            ToClient::Cube { dim, frame: 0, pos, cube, edit } if dim == self.world.dim => fwd.push(terrain::In::Cube { pos, cube, edit }),
            ToClient::Cube { dim, frame, pos, cube, .. } if dim == self.world.dim => {
                if let Some(e) = self.entities.get_mut(&frame) {
                    if e.structure.is_none() {
                        e.structure = Some(Box::new(ClientWorld::structure(dim)));
                        self.render.remove_frame(frame);
                    }
                    let s = e.structure.as_mut().unwrap();
                    s.insert(pos, cube, Default::default());
                    s.scan_bounds();
                }
            }
            ToClient::Unload { dim, pos } if dim == self.world.dim => {
                self.wires.cube_gone(0, pos);
                fwd.push(terrain::In::Unload(pos));
            }
            ToClient::Blocks { dim, frame: 0, changes, extras } if dim == self.world.dim => fwd.push(terrain::In::Blocks(changes, extras)),
            ToClient::Blocks { dim, frame, changes, extras } if dim == self.world.dim => {
                if let Some(s) = self
                    .entities
                    .get_mut(&frame)
                    .and_then(|e| e.structure.as_mut())
                {
                    for (p, state) in changes {
                        s.set_block(p, state);
                    }
                    for (p, parts) in extras {
                        s.set_extras(p, parts);
                    }
                    s.scan_bounds();
                }
            }
            ToClient::Saved { dim, cubes } if dim == self.world.dim => fwd.push(terrain::In::Saved(cubes)),
            ToClient::Edits { dim, specs, all } if dim == self.world.dim => fwd.push(terrain::In::Edits { specs, all }),
            ToClient::Signals { dim, frame, changes } if dim == self.world.dim => self.wires.power_changed(frame, &changes),
            ToClient::BodyGlow { dim, cells } if dim == self.world.dim => self.world.body_glow_changed(cells),
            ToClient::Clock { dim, time, ticks, weather, rate } => {
                if dim == self.world.dim {
                    self.world.time = time;
                    self.world.ticks = ticks;
                    self.since_time = 0.0;
                }
                self.weather_id = (weather as usize).min(content().game.weathers.len() - 1) as u16;
                self.tick_rate = rate.max(1) as f64;
            }
            ToClient::Teleport { dim, pos, frame } => self.teleported(dim, pos, frame, fwd),
            ToClient::Spawn(list) => self.spawn_entities(list),
            ToClient::Moves { tick, moves } => self.receive_moves(tick, moves),
            ToClient::BodyLight { frame, lights } => {
                if let Some(e) = self.entities.get_mut(&frame) {
                    e.world_light = lights.into_iter().collect();
                }
            }
            ToClient::Swing { used_item, off_hand } => {
                self.swing_arm();
                self.hand_swing_off = off_hand;
                if used_item && !off_hand && !self.held().is_empty() {
                    self.hand_height = 0.0;
                }
            }
            ToClient::Taken(list) => {
                for (item, by) in list {
                    if let Some(e) = self.entities.get_mut(&item) {
                        e.pulled = Some((e.pos, by, 0.0));
                    }
                }
            }
            ToClient::Despawn(ids) => {
                let arriving = self.arriving_tick();
                self.entity_components
                    .retain(|(e, _), _| !ids.iter().any(|g| g.0 == *e));
                for &(id, killed) in &ids {
                    self.environment.remote.remove(&id);
                    self.environment.static_entities.remove(&id);
                    self.showing_blocks.remove(&id);
                    // What is stuck in one that died goes down with it.
                    let host = self.entities.get(&id).and_then(|e| e.stuck).map(|s| s.host);
                    let host_dies = host.is_some_and(|h| {
                        ids.contains(&(h, true))
                            || self.entities.get(&h).is_some_and(|h| h.dying.is_some())
                            || (h == self.player_id && self.me.dying.is_some())
                    });
                    // Killed mobs stay a moment for the death animation, taken items until
                    // they reach the taker.
                    match self.entities.get_mut(&id) {
                        Some(e) if e.pulled.is_some() => {}
                        Some(e) if host_dies => {
                            if let Some(s) = &mut e.stuck {
                                s.leaving = true;
                            }
                        }
                        Some(e) if (killed || e.health <= 0.0) && e.item.is_empty() && e.dying.is_none() => {
                            (e.dying, e.died_at) = (Some(0.0), arriving);
                        }
                        _ => {
                            self.entities.remove(&id);
                        }
                    }
                }
            }
            ToClient::Players(names) => self.players = names,
            ToClient::Inventory(slots) => {
                for (i, s) in slots {
                    if let Some(dst) = self.inventory.get_mut(i as usize) {
                        *dst = s;
                    }
                }
            }
            ToClient::Components(list) => {
                for (k, v) in list {
                    self.engine_field(self.player_id, k, &v);
                    self.components.insert(k, v);
                }
            }
            ToClient::OpenMenu { kind, window, slots, owner, block, name } => {
                self.menu = Some(ClientMenu {
                    kind,
                    window,
                    owner,
                    block,
                    name,
                    slots: vec![ItemStack::EMPTY; slots as usize],
                    cursor: ItemStack::EMPTY,
                });
                self.events.push(ClientEvent::OpenMenu);
            }
            ToClient::MenuSlots(list) => {
                if let Some(m) = &mut self.menu {
                    for (i, s) in list {
                        if let Some(dst) = m.slots.get_mut(i as usize) {
                            *dst = s;
                        }
                    }
                }
            }
            ToClient::Cursor(s) => {
                if let Some(m) = &mut self.menu {
                    m.cursor = s;
                }
            }
            ToClient::CloseMenu => {
                self.menu = None;
                self.drag = None;
                self.sweep.clear();
                self.picking.clear();
                self.events.push(ClientEvent::CloseMenu);
            }
            ToClient::BreakRefused { frame, pos } => {
                self.refused_break = Some((frame, pos));
                self.breaking = None;
            }
            ToClient::Using(ticks) => {
                self.using = ticks.map(|(t, _)| (0.0, t as f32));
                self.using_off = ticks.is_some_and(|(_, off)| off);
            }

            ToClient::EntitySwing { id, off } => {
                let arriving = self.arriving_tick();
                if let Some(e) = self.entities.get_mut(&id) {
                    // Past halfway it starts over, as vanilla's.
                    if e.arm_swing.is_none_or(|p| p >= 0.5) {
                        e.swing_at = Some(arriving);
                    }
                    e.swing_off = off;
                }
            }
            ToClient::Cracks(list) => self.others_mining = list,
            ToClient::StopSprint => self.sprint_stopped = true,
            ToClient::LodColumns { dim, all, cols } => self.lod.set_columns(dim, all, &cols),
            ToClient::Camera { mode, locked, transition, easing } => self.camera_from_server(mode, locked, transition, easing),
            ToClient::Knock { push } => {
                let push = Vec3::from_array(push).as_dvec3();
                let resist = self.attribute(crate::registry::attr::KNOCKBACK_RESISTANCE) as f64;
                self.player.vel = physics::knock(self.player.vel, push, self.player.on_ground, resist);
            }
            ToClient::Velocity { add } => self.player.vel += Vec3::from_array(add).as_dvec3(),
            ToClient::SetVelocity(v) => self.player.vel = Vec3::from_array(v).as_dvec3(),
            ToClient::Look([yaw, pitch]) => (self.player.yaw, self.player.pitch) = (yaw, pitch),
            ToClient::Flying(on) => self.player.flying = on,
            ToClient::Selected(slot) => self.selected = (slot as usize).min(content().game.player_layout.hotbar.saturating_sub(1) as usize),
            ToClient::Died => self.dead = true,
            ToClient::Show(s) => {
                let in_chat = s.place == weft_abi::show_place::CHAT;
                if in_chat {
                    log::info!("[chat] {} {:?}", s.msg.key, s.msg.args);
                }
                let hidden = match crate::settings::chat_visibility() {
                    crate::settings::ChatVisibility::All => false,
                    crate::settings::ChatVisibility::System => s.from == crate::net::Shown::PLAYER,
                    crate::settings::ChatVisibility::Hidden => s.from != crate::net::Shown::REPLY,
                };
                if !(in_chat && hidden) {
                    self.show(s);
                }
            }
            ToClient::CommandReply { id, lines } => {
                self.command_replies.insert(id, lines);
            }
            ToClient::Disconnect(reason) => self.disconnected = Some(reason),
            ToClient::Gizmos(shapes) => self.gizmos.receive(shapes),
            ToClient::Packet { channel, data } => self.packets.push((channel, data)),
            ToClient::EntityComponents(list) => {
                for (id, comp, bytes) in list {
                    self.engine_field(id, comp, &bytes);
                    self.entity_components.insert((id, comp), bytes);
                }
            }
            // The same values in one process; a remote server's win while joined.
            ToClient::Configs(list) => {
                for (id, v) in list {
                    crate::config::put_from_server(id, v);
                }
            }
            ToClient::Skin { entity, png } => self.entity_render.skin_pending.push((entity, png)),
            ToClient::Avatar { entity, avatar } => self
                .entity_render
                .avatar_pending
                .push((entity, std::sync::Arc::new(avatar))),
            ToClient::PlayerGone { entity } => self.entity_render.gone_pending.push(entity),
            ToClient::PlayAction(list) => {
                let arriving = self.arriving_tick();
                for (id, name, looping) in list {
                    if id == self.player_id {
                        self.me.action = (!name.is_empty()).then(|| (name, self.me.age, looping));
                    } else if let Some(e) = self.entities.get_mut(&id) {
                        let start = ((arriving - e.born).max(0.0) / crate::world::TPS as f64) as f32;
                        e.action = (!name.is_empty()).then(|| (name, start, looping));
                    }
                }
            }
            ToClient::Sounds(list) => {
                self.sounds.extend(
                    list.into_iter()
                        .map(|(sound, pos, volume, pitch)| crate::audio::Play { sound, pos, volume, pitch }),
                );
            }
            ToClient::Particles(list) => {
                for b in &list {
                    self.particles.spawn(b, &self.world);
                }
            }
            _ => {}
        }
    }

    fn teleported(&mut self, dim: u32, pos: DVec3, frame: Option<(u64, DVec3)>, fwd: &mut Vec<terrain::In>) {
        if dim != self.world.dim {
            self.reset_terrain(dim, fwd);
            self.entities.clear();
            self.showing_blocks.clear();
            self.particles.clear();
            self.entity_components.clear();
        }
        let at = self.body_tick(self.body_clock);
        let on = frame.and_then(|(id, rel)| self.entities.get(&id).map(|b| (id, b.known(at) + rel)));
        let pos = on.map_or(pos, |o| o.1);
        self.player.support = on.map(|o| o.0);
        // Off whatever it rode, or the next step would put it back there.
        self.player.riding = None;
        self.player.pos = pos;
        self.player.prev = pos;
        self.player.vel = DVec3::ZERO;
    }

    fn spawn_entities(&mut self, list: Vec<crate::net::EntityView>) {
        for e in list {
            if content()
                .components
                .id(weft_abi::environment::COMPONENT)
                .is_some_and(|id| {
                    content()
                        .components
                        .bytes(crate::components::ENTITY, e.kind, id)
                        .is_some()
                })
            {
                self.environment.static_entities.insert(e.id);
            }
            let to = if content().game.is_block_entity(e.kind) { &mut self.showing_blocks } else { &mut self.entities };
            self.entity_render.changed_items.insert(e.id);
            let mut shown = ClientEntity::new(e.kind, e.pos, e.yaw, e.pitch);
            (shown.born, shown.body_yaw, shown.body_target) = (e.born as f64, e.body_yaw, e.body_yaw);
            (shown.walk, shown.walk_target, shown.swing, shown.swing_target) = (e.strode, e.strode, e.stride, e.stride);
            to.insert(e.id, shown);
        }
    }

    fn receive_moves(&mut self, tick: u64, list: Vec<crate::net::Move>) {
        // A long stall (or the first move) starts the timeline over.
        let sample = self.body_clock - STEP + self.accum - tick as f64 / self.tick_rate;
        match self.tick_offset {
            Some(o) if (sample - o).abs() < 0.25 => self.offset_target += (sample - self.offset_target) * 0.1,
            _ => (self.tick_offset, self.offset_target) = (Some(sample), sample),
        }
        let solid = |k: u32| content().game.entities[k as usize].is(crate::tag_bits::entity::SOLID);
        // Physics looks a step of its own back, many ticks at a high `/tick rate`.
        let keep = ((self.tick_rate * STEP).ceil() as usize * 2 + 4).max(8);
        // The mount this client steers goes where it moves it.
        let driving = self.me.vehicle.map(|v| v.0);
        // Bodies turn by the tick from these moves (`physics::turn_body`), as the server turned
        // them, by how they moved on what they stand on.
        let deltas: rustc_hash::FxHashMap<u64, DVec3> = list
            .iter()
            .filter_map(|&(id, pos, ..)| self.entities.get(&id).map(|e| (id, pos - e.target)))
            .collect();
        // Limbs swing and crouching turns by the tick too, the clips' clock.
        for &(id, _, yaw, _, pose, _, frame) in &list {
            let Some(&d) = deltas.get(&id) else { continue };
            let carried = deltas
                .get(&frame)
                .filter(|_| frame != 0)
                .copied()
                .unwrap_or_default();
            if let Some(e) = self.entities.get_mut(&id) {
                let walked = d - carried;
                e.body_target = crate::physics::turn_body(e.body_target, yaw, walked);
                let speed = (walked.with_y(0.0).length() * crate::world::TPS as f64) as f32;
                crate::model::walk_limbs(&mut e.walk_target, &mut e.swing_target, speed, crate::world::TICK as f32);
                let crouching = pose == Pose::CROUCH;
                if crouching != e.crouch_at.2 {
                    e.crouch_at = (tick as f64, e.crouched, crouching);
                }
            }
        }
        for &(id, pos, yaw, pitch, pose, sprinting, frame) in &list {
            if Some(id) == driving {
                continue;
            }
            if let Some(e) = self.entities.get_mut(&id) {
                (e.pose, e.sprinting) = (pose, sprinting);
                let was = e.target;
                e.target = pos;
                e.target_yaw = yaw;
                e.target_pitch = pitch;
                e.frame = frame;
                if solid(e.kind) {
                    // A paused server keeps sending the same tick: one sample per
                    // tick, or two at the same time make an endless speed.
                    let t = tick as f64;
                    // It starts from where it appeared, or its first move is a jump.
                    if e.track.is_empty() {
                        e.track.push_back((t - 1.0, was));
                    }
                    match e.track.back_mut() {
                        Some(last) if last.0 == t => last.1 = pos,
                        Some(last) if last.0 > t => {
                            e.track.clear();
                            e.track.push_back((t, pos));
                        }
                        _ => e.track.push_back((t, pos)),
                    }
                    while e.track.len() > keep {
                        e.track.pop_front();
                    }
                }
            }
        }
        // Riders relative to their body, both where the server had them this tick.
        for &(id, pos, .., frame) in &list {
            let body = self
                .entities
                .get(&frame)
                .filter(|_| frame != 0)
                .map(|b| (b.target, b.pos));
            let Some(e) = self.entities.get_mut(&id) else { continue };
            match body {
                Some((target, shown)) => {
                    if e.rel_target.is_none() {
                        e.rel = e.pos - shown;
                    }
                    e.rel_target = Some(pos - target);
                }
                None => e.rel_target = None,
            }
        }
    }

    /// What the terrain thread finished, a batch at a time (shown together), for a few
    /// milliseconds a frame: taking all that came at once froze frames while flying. A loading
    /// batch may be cut there (a step of the view unloads a slice of the world); one with edits
    /// is taken whole.
    fn take_terrain(&mut self) {
        let start = Instant::now();
        while start.elapsed() < self.budget {
            let batch = match self.terrain_left.take() {
                Some(b) => b,
                None => match self.terrain.rx.try_recv() {
                    Ok(b) => b,
                    Err(_) => break,
                },
            };
            if batch.resets != self.terrain_resets {
                continue;
            }
            let (resets, whole) = (batch.resets, batch.whole);
            self.world.begin_batch(whole);
            let mut items = batch.out.into_iter();
            while let Some(o) = items.next() {
                match o {
                    terrain::Out::Cube { pos, cube, around } => self.world.insert(pos, cube, around),
                    terrain::Out::Blocks(changes, extras) => {
                        for (p, s) in changes {
                            self.world.set_block(p, s);
                        }
                        for (p, parts) in extras {
                            self.world.set_extras(p, parts);
                        }
                    }
                    terrain::Out::Unload(pos) => self.world.remove(pos),
                    terrain::Out::Range { lo, hi } => self.world.set_range(lo, hi),
                }
                if !whole && start.elapsed() >= self.budget {
                    let out: Vec<terrain::Out> = items.collect();
                    if !out.is_empty() {
                        self.terrain_left = Some(terrain::Batch { resets, whole, out });
                    }
                    break;
                }
            }
            self.world.end_batch();
        }
    }
}
