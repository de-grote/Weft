//! What players ask for, one list of what each message takes (`allowed`), then the actions.

use super::*;

impl Server {
    pub(super) fn handle_messages(&mut self, c: &'static Content) {
        for pi in 0..self.players.len() {
            while let Some(msg) = self.players[pi].conn.recv() {
                self.handle(c, pi, msg);
                self.settle(c);
            }
        }
    }

    /// Items held long enough take effect; another item in hand ends it.
    pub(super) fn finish_uses(&mut self, c: &'static Content) {
        for pi in 0..self.players.len() {
            let Some((item, done, slot)) = self.players[pi].using else { continue };
            let held = self
                .region(pi)
                .map_or(ItemStack::EMPTY, |(r, id)| r.entities.inventory_slot(id, slot));
            let selected = self
                .region(pi)
                .and_then(|(r, id)| r.entities.get(id).map(|e| e.selected));
            let off = c.game.player_layout.off == Some(slot);
            if held.item != item || held.is_empty() || (!off && selected != Some(slot)) {
                self.stop_using(pi);
            } else if self.tick >= done {
                self.stop_using(pi);
                self.use_in_hand(c, pi, slot, None, true);
            }
        }
    }

    fn stop_using(&mut self, pi: usize) {
        self.players[pi].using = None;
        self.players[pi].conn.send(ToClient::Using(None));
        if let Some(e) = self.region(pi).and_then(|(r, id)| r.entities.get_mut(id)) {
            e.using = (0, false);
        }
    }

    /// The hands to try, in order, for the player's right click.
    fn hands(&mut self, c: &'static Content, pi: usize) -> Vec<u32> {
        self.region(pi).map_or(Vec::new(), |(r, id)| {
            let sel = r.entities.get(id).map_or(0, |e| e.selected);
            r.entities
                .inventories
                .get(&id)
                .map_or(Vec::new(), |i| crate::content::hands_in_order(c, i, sel))
        })
    }

    /// A click on a block with the item in `slot`: places it if it is a block, uses it if not.
    #[allow(clippy::too_many_arguments)]
    fn place_or_use(
        &mut self,
        c: &'static Content,
        pi: usize,
        slot: u32,
        frame: u64,
        pos: IVec3,
        face: u8,
        look: [f32; 3],
        hit: [f32; 3],
    ) -> bool {
        let off_hand = c.game.player_layout.off == Some(slot);
        let Some((r, entity)) = self.region(pi) else { return false };
        let Some(e) = r.entities.get(entity) else { return false };
        let creative = e.mode.has(crate::tag_bits::game_mode::NO_COSTS);
        let no_place = e.mode.has(crate::tag_bits::game_mode::NO_PLACE);
        let held = r.entities.inventory_slot(entity, slot);
        let Some(state) = c
            .items
            .get(held.item as usize)
            .and_then(|i| i.block)
            .filter(|_| !held.is_empty() && !no_place)
        else {
            // Items used on a block (buckets) work on the world's blocks only, so far.
            return self.use_in_hand(c, pi, slot, (frame == 0).then_some((pos, face)), false);
        };
        let placed = r.with_space(c, frame, |d, _| {
            let hit = hit.map(|v| v.clamp(0.0, 1.0));
            let target = d.place_clicked(c, pos, face, state, look, hit, entity, held.item)?;
            // What the stack carries goes onto the block entity it becomes (a renamed chest).
            if let Some(id) = d.anchored_part(c, target, state) {
                for &comp in &c.game.carried {
                    if let Some(v) = crate::stack::value(held.data, comp) {
                        d.entities.comps.insert((id, comp), v.into());
                    }
                }
            }
            Some(())
        });
        // Where it can't be placed, it is used like any item (a seed's own use, a bucket of it).
        if !matches!(placed, Some(Some(()))) {
            return self.use_in_hand(c, pi, slot, (frame == 0).then_some((pos, face)), false);
        }
        if !creative {
            r.entities
                .set_slot(entity, slot, ItemStack { count: held.count - 1, ..held });
        }
        r.events
            .push(DimEvent::EntitySwing { id: entity, off: off_hand });
        self.players[pi]
            .conn
            .send(ToClient::Swing { used_item: true, off_hand });
        true
    }

    /// Uses the item in `slot` on what was clicked: true if it did something, or started to.
    fn use_in_hand(&mut self, c: &'static Content, pi: usize, slot: u32, target: Option<(IVec3, u8)>, finishing: bool) -> bool {
        let off_hand = c.game.player_layout.off == Some(slot);
        let Some((r, entity)) = self.region(pi) else { return false };
        let held = r.entities.inventory_slot(entity, slot);
        let Some(def) = c.items.get(held.item as usize) else { return false };
        let (pos, face) = target.unwrap_or((IVec3::ZERO, 0));
        let state = target.and_then(|_| r.get(pos)).unwrap_or(0);
        let mut q = abi::Query { entity, item: held.item, pos: pos.to_array().into(), state, ..abi::Query::DEFAULT };
        if !r.ask(c, abi::action::USE_ITEM, &mut q) {
            return false;
        }
        // An item that takes time starts here, and comes back through this when it is done.
        let ticks = crate::content::use_ticks(c, &held);
        if ticks > 0 && !finishing {
            if let Some(e) = r.entities.get_mut(entity) {
                e.using = (ticks, off_hand);
            }
            let done = self.tick + ticks as u64;
            self.players[pi].using = Some((held.item, done, slot));
            self.players[pi]
                .conn
                .send(ToClient::Using(Some((ticks, off_hand))));
            return true;
        }
        let Some((r, entity)) = self.region(pi) else { return false };
        let mut used = false;
        if def.on_use.is_none() {
            if let Some(after) = r.use_container(c, entity, held) {
                if after != held {
                    r.entities.set_slot(entity, slot, after);
                    used = true;
                }
            }
        } else if let Some((by, cb)) = def.on_use {
            let a = r.call(by, cb, |w| abi::ItemUseArgs {
                world: w,
                player: entity,
                stack: held,
                slot,
                pos: pos.to_array().into(),
                face: face as u32,
                has_target: target.is_some() as u32,
                handled: 0,
            });
            if a.handled != 0 && a.stack != held && crate::host::known_item("on_use", &a.stack) {
                r.entities.set_slot(entity, slot, a.stack);
            }
            used = a.handled != 0;
        }
        if used {
            r.emit_engine(
                c,
                abi::action::USE_ITEM,
                &abi::ItemUsed { player: entity, stack: held, pos: pos.to_array().into(), has_target: target.is_some() as u32 },
            );
            r.events
                .push(DimEvent::EntitySwing { id: entity, off: off_hand });
            self.players[pi]
                .conn
                .send(ToClient::Swing { used_item: true, off_hand });
        }
        used
    }

    /// The region holding player `pi`'s entity, with the entity id.
    pub(super) fn region(&mut self, pi: usize) -> Option<(&mut Dim, u64)> {
        let p = &self.players[pi];
        let id = p.entity;
        self.dims[p.dim as usize].entity_region(id).map(|r| (r, id))
    }

    /// Whether player `pi` may send this at all. One list before the actions, so a new action
    /// can't forget that its sender may be dead, a spectator or not in the game yet.
    pub(super) fn allowed(&self, pi: usize, msg: &ToServer) -> bool {
        use ToServer as M;
        let p = &self.players[pi];
        if matches!(msg, M::Join { .. } | M::Leave) {
            return true;
        }
        let Some(e) = self.dims[p.dim as usize]
            .entity(p.entity)
            .filter(|_| p.joined)
        else {
            return false;
        };
        match msg {
            // Sent by a modified client only; they would index out of bounds.
            M::Use { face, .. } | M::UseItem { target: Some((_, face)) } if *face >= 6 => false,
            M::Respawn => !e.alive(),
            M::Break { .. } | M::Use { .. } | M::UseItem { .. } | M::Attack(_) | M::Swing | M::Interact(_) | M::Drop { .. } => {
                e.alive() && !e.mode.has(crate::tag_bits::game_mode::NO_INTERACT)
            }
            M::Move { .. } | M::OpenInventory | M::Click { .. } | M::Drag { .. } | M::CreativeTake { .. } | M::Pick { .. } => e.alive(),
            _ => true,
        }
    }

    pub(super) fn handle(&mut self, c: &'static Content, pi: usize, msg: ToServer) {
        if !self.allowed(pi, &msg) {
            log::debug!(target: "net", "refused {} from {}", crate::net::Message::kind(&msg), self.players[pi].name);
            return;
        }
        match msg {
            ToServer::Join { name, key } => self.join(c, pi, name, key),
            ToServer::StartBreak { pos, part, frame } => {
                let Some((r, _)) = self.region(pi) else { return };
                let (frame, pos) = r.landed_as(frame, pos);
                self.players[pi].mining = Some(Mining { frame, pos, part, progress: 0.0 });
                let Some((r, entity)) = self.region(pi) else { return };
                let held = r.entities.held(entity).item;
                // Said now, or it would mine all that time for nothing.
                let refused = r.with_space(c, frame, |d, _| {
                    d.get(pos).is_some() && {
                        let state = d.aimed_part(c, pos, part);
                        !d.may_break(c, pos, state, held, entity)
                    }
                });
                if refused == Some(true) {
                    self.players[pi].mining = None;
                    self.players[pi]
                        .conn
                        .send(ToClient::BreakRefused { frame, pos });
                }
            }
            ToServer::StopBreak => self.players[pi].mining = None,
            ToServer::StopUsing => self.stop_using(pi),
            ToServer::SwapHands => {
                let Some(off) = c.game.player_layout.off else { return };
                let Some((r, entity)) = self.region(pi) else { return };
                let Some(e) = r
                    .entities
                    .get(entity)
                    .filter(|e| !e.mode.has(crate::tag_bits::game_mode::NO_INTERACT))
                else {
                    return;
                };
                let sel = e.selected;
                let (main, other) = (r.entities.inventory_slot(entity, sel), r.entities.inventory_slot(entity, off));
                if crate::content::fits_one_hand(c, main.item) {
                    r.entities.set_slot(entity, sel, other);
                    r.entities.set_slot(entity, off, main);
                }
            }
            ToServer::Move { pos, yaw, pitch, on_ground, pose, sprint, flying, jumped, frame } => {
                if !pos.is_finite() {
                    return;
                }
                // On a body: where the client has the player on it, on the body as it is here.
                let dim = &self.dims[self.players[pi].dim as usize];
                let on = frame.filter(|f| f.1.is_finite()).and_then(|(b, rel)| {
                    dim.entity_region_ref(b)
                        .and_then(|r| r.entities.get(b))
                        .map(|e| (b, e.pos + rel, rel))
                });
                let pos = on.map_or(pos, |o| o.1);
                let place = on.map_or((None, pos), |o| (Some(o.0), o.2));
                let p = &mut self.players[pi];
                if let Some(t) = p.teleported {
                    if (pos - t).length_squared() > 16.0 {
                        return;
                    }
                    p.teleported = None;
                    p.last_place = place;
                }
                // Measured on what it rides, like a mob's: a lift going down under a jump isn't a
                // fall. Getting on or off isn't a step.
                let walked = if p.last_place.0 == place.0 { place.1 - p.last_place.1 } else { DVec3::ZERO };
                p.last_place = place;
                let Some((r, entity)) = self.region(pi) else { return };
                let Some(e) = r.entities.get_mut(entity) else { return };
                // On a mount it is where its seat is; only where it looks is its own.
                if e.vehicle.is_some() {
                    (e.yaw, e.pitch) = (yaw, pitch);
                    return;
                }
                e.pose = pose;
                let here = crate::physics::around(c, &*r.space, pos, e.height(c));
                let splash = entity::splashes(e.in_liquid, here.in_liquid, -walked.y);
                let fell = entity::track_fall(&mut e.fall, walked.y, on_ground, here.in_liquid || here.climbing);
                let land = fell.and_then(|f| entity::fall_damage(&c.game.entities[e.kind as usize], f));
                e.sprinting = sprint;
                e.flying = flying;
                e.jumped |= jumped;
                e.moved = walked.with_y(0.0).length() as f32;
                e.pos = pos;
                e.frame = on.map(|o| o.0);
                e.yaw = yaw;
                e.pitch = pitch;
                e.on_ground = on_ground;
                e.in_liquid = here.in_liquid;
                r.fall_effects(c, entity, fell, splash);
                if let Some(amount) = land {
                    r.damage(c, entity, amount, 0, abi::damage_cause::FALL);
                }
            }

            ToServer::MountMove { pos, yaw, vel, on_ground } => {
                if !pos.is_finite() || !vel.is_finite() || !yaw.is_finite() {
                    return;
                }
                let Some((r, entity)) = self.region(pi) else { return };
                r.mount_moved(c, entity, pos, yaw, vel, on_ground);
            }
            ToServer::Dismount => {
                let Some((r, entity)) = self.region(pi) else { return };
                r.dismount(c, entity);
            }
            ToServer::Camera(mode) => {
                let Some(m) = c.game.cameras.get(mode as usize) else { return };
                let Some((r, entity)) = self.region(pi) else { return };
                let pos = r.entities.get(entity).map_or(DVec3::ZERO, |e| e.pos);
                let ask = crate::conditions::Ask { pos, entity, ..Default::default() };
                let mut q = weft_abi::Query { entity, amount: mode as f64, ..weft_abi::Query::DEFAULT };
                let holds = m
                    .when
                    .is_none_or(|w| crate::conditions::on_server(r, c, w, &ask));
                let may = r.ask_or(c, weft_abi::action::CAMERA, &mut q, holds);
                if may {
                    self.players[pi]
                        .conn
                        .send(ToClient::Camera { mode, locked: false, transition: f32::NAN, easing: 0 });
                }
            }

            ToServer::Break { pos, part, face, hit, frame } => {
                let Some((r, _)) = self.region(pi) else { return };
                let (frame, pos) = r.landed_as(frame, pos);
                let mining = self.players[pi].mining.take();
                let Some((r, entity)) = self.region(pi) else { return };
                let Some(e) = r.entities.get(entity) else { return };
                let (eye, sel, creative) = (e.pos + DVec3::Y * e.eye(c), e.selected, e.mode.has(crate::tag_bits::game_mode::INSTANT_BREAK));
                let no_break = e.mode.has(crate::tag_bits::game_mode::NO_BREAK);
                let reach = r.block_reach(c, entity);
                let rate = r.mining_rate_of(c, entity, frame, pos, part);
                let progress = mining
                    .filter(|m| m.frame == frame && m.pos == pos && m.part == part)
                    .map_or(0.0, |m| m.progress);
                let mut refused = false;
                let held = r.entities.held(entity);
                let worn = r.with_space(c, frame, |d, o| {
                    // Done by what the server added up, with room for lag: the client's word is
                    // only that it is, and a modified one could say anything.
                    let part = d.aimed_part(c, pos, part);
                    let too_soon = rate < 1.0 && progress + 2.0 * rate < 0.7;
                    let keeps = (creative && c.no_creative_break(held.item)) || no_break;
                    let free = c
                        .states
                        .get(part as usize)
                        .is_some_and(|s| s.hardness <= 0.0);
                    let done = within(o.to_local(eye), pos, reach)
                        && !too_soon
                        && !keeps
                        && d.break_piece(c, pos, part, face, hit, held, entity, !creative);
                    // The client already shows air: a refused break sends the block back.
                    if !done && d.get(pos).is_some() {
                        d.resend_cell(pos);
                        refused = true;
                    }
                    done && !free
                });
                if refused {
                    self.players[pi]
                        .conn
                        .send(ToClient::BreakRefused { frame, pos });
                }
                let Some((r, _)) = self.region(pi) else { return };
                if worn == Some(true) && !r.entities.held(entity).is_empty() {
                    wear(c, r, entity, sel, 1);
                }
            }

            ToServer::Use { pos, part, face, look, hit, sneak, frame } => {
                let Some((r, _)) = self.region(pi) else { return };
                let (frame, pos) = r.landed_as(frame, pos);
                let Some((r, entity)) = self.region(pi) else { return };
                let Some(e) = r.entities.get(entity) else { return };
                let eye = e.pos + DVec3::Y * e.eye(c);
                let reach = r.block_reach(c, entity);
                // The block clicked goes first; only then what is in the hands.
                let used = r.with_space(c, frame, |d, o| {
                    if !within(o.to_local(eye), pos, reach) {
                        return None;
                    }
                    Some(!sneak && d.player_use(c, pos, part, face, entity))
                });
                match used {
                    Some(Some(true)) => {
                        r.events
                            .push(DimEvent::EntitySwing { id: entity, off: false });
                        self.players[pi]
                            .conn
                            .send(ToClient::Swing { used_item: false, off_hand: false });
                    }
                    Some(Some(false)) => {
                        for slot in self.hands(c, pi) {
                            if self.place_or_use(c, pi, slot, frame, pos, face, look, hit) {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }

            ToServer::UseItem { target } => {
                for slot in self.hands(c, pi) {
                    if self.use_in_hand(c, pi, slot, target, false) {
                        break;
                    }
                }
            }
            ToServer::Pick { item } => {
                let Some((r, entity)) = self.region(pi) else { return };
                let Some(e) = r.entities.get(entity) else { return };
                let (sel, creative) = (e.selected, e.mode.has(crate::tag_bits::game_mode::NO_COSTS));
                let Some(inv) = r.entities.inventories.get(&entity) else { return };
                let current = inv.get(sel as usize).copied().unwrap_or_default();
                if let Some(j) = c
                    .game
                    .player_layout
                    .main()
                    .map(|j| j as usize)
                    .find(|&j| inv.get(j).is_some_and(|s| s.item == item && !s.is_empty()))
                {
                    let found = inv[j];
                    r.entities.set_slot(entity, j as u32, current);
                    r.entities.set_slot(entity, sel, found);
                } else if creative && c.items.get(item as usize).is_some() {
                    r.entities.set_slot(entity, sel, ItemStack::new(item, 1));
                }
            }
            ToServer::Select(slot) => {
                if slot < c.game.player_layout.hotbar {
                    if let Some((r, entity)) = self.region(pi) {
                        if let Some(e) = r.entities.get_mut(entity) {
                            e.selected = slot;
                        }
                        let held = r.entities.held(entity);
                        if let Some(e) = r.entities.get_mut(entity) {
                            e.notice_hand(held);
                        }
                    }
                }
            }

            ToServer::Attack(target) => {
                let Some((r, entity)) = self.region(pi) else { return };
                let (Some(me), Some(t)) = (r.entities.get(entity), r.entities.get(target)) else { return };
                if !in_reach(c, r, me, t) {
                    return;
                }
                r.melee(c, entity, target);
            }

            ToServer::Swing => {
                if let Some(e) = self.region(pi).and_then(|(r, id)| r.entities.get_mut(id)) {
                    e.swing_ticks = 0;
                }
            }

            ToServer::Interact(target) => {
                let Some((r, entity)) = self.region(pi) else { return };
                let Some(kind) = r.entities.get(target).map(|e| e.kind) else { return };
                if !r
                    .entities
                    .get(entity)
                    .zip(r.entities.get(target))
                    .is_some_and(|(me, t)| in_reach(c, r, me, t))
                {
                    return;
                }
                let held = r.entities.held(entity);
                let mut q = abi::Query { entity: target, other: entity, item: held.item, ..abi::Query::DEFAULT };
                if !r.ask(c, abi::action::INTERACT_ENTITY, &mut q) {
                    return;
                }
                r.emit_engine(
                    c,
                    abi::action::INTERACT_ENTITY,
                    &abi::Interaction { player: entity, entity: target, item: held.item, pad: 0 },
                );
                let mut handled = false;
                for &(owner, cb) in &c.game.entities[kind as usize].on_interact {
                    handled |= r
                        .call(owner, cb, |w| abi::EntityEventArgs { world: w, entity: target, other: entity, handled: 0 })
                        .handled
                        != 0;
                }
                if handled {
                    self.players[pi]
                        .conn
                        .send(ToClient::Swing { used_item: false, off_hand: false });
                }
            }
            ToServer::OpenInventory => {
                if let Some(m) = c.game.player_menu {
                    self.open_menu(c, pi, m, None, 0);
                }
            }
            ToServer::Click { slot, button, shift } => {
                let Server { players, dims, .. } = self;
                let p = &mut players[pi];
                if let (Some(m), Some(r)) = (&mut p.menu, dims[p.dim as usize].entity_region(p.entity)) {
                    menu::click(m, r, c, p.entity, slot, button, shift);
                }
            }
            ToServer::CreativeTake { item, button } => {
                let Server { players, dims, .. } = self;
                let p = &mut players[pi];
                let creative = dims[p.dim as usize]
                    .entity(p.entity)
                    .is_some_and(|e| e.mode.has(crate::tag_bits::game_mode::CREATIVE_INVENTORY));
                if let (true, Some(m), true) = (creative, &mut p.menu, item != 0 && (item as usize) < c.items.len()) {
                    // Like vanilla: the same item adds one, anything else held is thrown away
                    // first, an empty hand takes a stack (left) or one (right).
                    let max = c.max_stack(item);
                    m.cursor = if m.cursor.is_empty() {
                        ItemStack::new(item, if button == 1 { 1 } else { max })
                    } else if m.cursor.item == item && m.cursor.data == 0 {
                        ItemStack { count: (m.cursor.count + 1).min(max), ..m.cursor }
                    } else {
                        ItemStack::EMPTY
                    };
                }
            }
            ToServer::Drag { slots, button } => {
                let Server { players, dims, .. } = self;
                let p = &mut players[pi];
                if let (Some(m), Some(r)) = (&mut p.menu, dims[p.dim as usize].entity_region(p.entity)) {
                    menu::drag(m, r, c, p.entity, &slots, button);
                }
            }
            ToServer::CloseMenu => {
                let Server { players, dims, .. } = self;
                let p = &mut players[pi];
                if let (Some(m), Some(r)) = (p.menu.take(), dims[p.dim as usize].entity_region(p.entity)) {
                    menu::close(m, r, c, p.entity);
                }
            }

            ToServer::Drop { all } => {
                let Some((r, entity)) = self.region(pi) else { return };
                let Some(sel) = r.entities.get(entity).map(|e| e.selected) else { return };
                // A two-handed item that does nothing can still be thrown away.
                let held = r.entities.in_selected_slot(entity);
                if held.is_empty() {
                    return;
                }
                let n = if all { held.count } else { 1 };
                r.entities
                    .set_slot(entity, sel, ItemStack { count: held.count - n, ..held });
                r.throw(c, entity, ItemStack { count: n, ..held });
            }
            ToServer::Respawn => {
                if let Some((r, id)) = self.region(pi) {
                    if let Some(e) = r.entities.get_mut(id) {
                        e.fall = 0.0;
                    }
                }
                let spawn = self.dims[c.spawn_dim as usize].spawn_point(c);
                self.change_dim(c, pi, c.spawn_dim, spawn);
                if let Some((r, entity)) = self.region(pi) {
                    r.entities.reset_components(c, entity);
                    // The game mode's modifiers went with the rest.
                    r.keep_sources(c, entity);
                    // After the components: modifiers are gone with them. Sent as any change.
                    let max = r.max_health(c, entity);
                    if let Some(e) = r.entities.get_mut(entity) {
                        e.health = max;
                        e.fire = 0;
                        e.hurt_cooldown = 0;
                    }
                    r.emit_engine(c, abi::event::PLAYER_RESPAWNED, &abi::PlayerEvent { player: entity });
                }
                // Out of lives: on as a spectator, as in vanilla's hardcore.
                let lives = crate::config::get(c.game.lives) as u32;
                if lives > 0 && self.players[pi].deaths >= lives {
                    self.set_mode(pi, crate::world::GameMode::SPECTATOR);
                }
            }
            ToServer::Chat(text) => {
                let name = self.players[pi].name.clone();
                if let Some(cmd) = text.strip_prefix('/') {
                    for line in self.command(c, pi, cmd) {
                        let from = crate::net::Shown::REPLY;
                        self.players[pi]
                            .conn
                            .send(ToClient::Show(crate::net::Shown { from, ..crate::net::Shown::chat(line) }));
                    }
                } else {
                    let text: String = text.chars().take(256).collect();
                    let mut line = None;
                    if let Some((r, player)) = self.region(pi) {
                        let mut q = abi::Query { entity: player, text: abi::Str::new(&text), ..abi::Query::DEFAULT };
                        if !r.ask(c, abi::action::CHAT, &mut q) {
                            return;
                        }
                        if let Some((owner, cb)) = c.game.chat_format {
                            let out = &raw mut line as *mut std::ffi::c_void;
                            r.call(owner, cb, |world| abi::ChatFormatArgs { world, player, text: abi::Str::new(&text), out });
                        }
                        r.emit_engine(c, abi::action::CHAT, &abi::ChatMessage::new(player, &text));
                    }
                    log::info!("<{name}> {text}");
                    let line = line.unwrap_or_else(|| Msg::new("chat.message", vec![Arg::Text(name), Arg::Text(text)]));
                    let shown = crate::net::Shown { from: crate::net::Shown::PLAYER, ..crate::net::Shown::chat(line) };
                    self.show(abi::Who::All, shown);
                }
            }
            ToServer::Command { id, line } => {
                let lines = self.command(c, pi, line.trim_start_matches('/'));
                self.players[pi]
                    .conn
                    .send(ToClient::CommandReply { id, lines });
            }
            ToServer::Packet { channel, data } => {
                let Some((owner, cb)) = c.game.packets.get(channel as usize).and_then(|d| d.server) else { return };
                let Some((r, player)) = self.region(pi) else { return };
                let slice = abi::Slice::new(&data);
                r.call(owner, cb, |w| abi::PacketArgs { world: w, player, data: slice });
            }
            ToServer::Skin(png) => {
                // Only a skin's size of PNG: everyone else stores and uploads it.
                let size = crate::assets::png_size(&png).filter(|_| png.len() <= 65536);
                if !matches!(size, Some((64, 64) | (64, 32))) {
                    return;
                }
                let png = Arc::new(png);
                let entity = self.players[pi].entity;
                self.players[pi].skin = Some(png.clone());
                for p in self.players.iter_mut().filter(|p| p.joined) {
                    p.conn.send(ToClient::Skin { entity, png: png.to_vec() });
                }
            }
            ToServer::Avatar(avatar) => {
                // Once a stay, and only what fits: everyone else keeps it for as long as they play.
                let allowed = crate::config::by_name("core:allow_avatars").is_none_or(|id| crate::config::get(id) != 0.0);
                if self.players[pi].avatar.is_some() || avatar.size() > crate::net::AVATAR_BYTES || !allowed {
                    return;
                }
                let avatar = Arc::new(avatar);
                let entity = self.players[pi].entity;
                self.players[pi].avatar = Some(avatar.clone());
                for p in self.players.iter_mut().filter(|p| p.joined) {
                    p.conn
                        .send(ToClient::Avatar { entity, avatar: (*avatar).clone() });
                }
            }
            ToServer::View(r) => {
                let c = content();
                let def = crate::config::by_name("core:view_distance").map(|id| &c.game.configs[id as usize]);
                self.players[pi].view_radius = def.map_or(r as f64, |d| d.fit(r as f64)) as i32;
            }
            ToServer::Gizmos(mut names) => {
                names.retain(|n| n.len() <= 64);
                names.truncate(64);
                self.players[pi].gizmos = names;
            }
            ToServer::Leave => {
                if !self.players[pi].joined {
                    return;
                }
                log::info!("{} left", self.players[pi].name);
                let line = Msg::new("multiplayer.left", vec![Arg::Text(self.players[pi].name.clone())]);
                let entity = self.players[pi].entity;
                for q in self.players.iter_mut().filter(|q| q.joined) {
                    q.conn
                        .send(ToClient::Show(crate::net::Shown::chat(line.clone())));
                    q.conn.send(ToClient::PlayerGone { entity });
                }
                let Server { players, dims, storage, .. } = self;
                let p = &mut players[pi];
                if let Some(r) = dims[p.dim as usize].entity_region(p.entity) {
                    if let Some(m) = p.menu.take() {
                        menu::close(m, r, c, p.entity);
                    }
                    if let Some(s) = storage {
                        save_player(&s.lock(), p, r);
                    }
                    r.entities.remove(p.entity);
                }
                p.conn.send(ToClient::Disconnect("bye".into()));
                p.joined = false;
            }
        }
    }
}

/// Within its `ENTITY_REACH`, from the eyes to the nearest point of the target's box, like
/// modern's; a player's with room for what its client saw last.
pub(crate) fn in_reach(c: &Content, d: &Dim, me: &Entity, target: &Entity) -> bool {
    let eye = me.pos + DVec3::Y * me.eye(c);
    let b = target.aabb(c);
    let slack = if me.player { ENTITY_SLACK } else { 0.0 };
    let reach = d.attribute(c, me.id, extend::ENTITY_REACH, false) + slack;
    (eye.clamp(b.min, b.max) - eye).length_squared() <= reach * reach
}

pub(super) fn within(eye: DVec3, block: IVec3, reach: f64) -> bool {
    (block.as_dvec3() + DVec3::splat(0.5) - eye).length_squared() < reach * reach
}

/// Damages the held tool; breaks it when used up. Creative players' tools never wear.
pub(super) fn wear(c: &Content, d: &mut Dim, player: u64, slot: u32, amount: u32) {
    if d.entities
        .get(player)
        .is_some_and(|e| e.mode.has(crate::tag_bits::game_mode::NO_COSTS))
    {
        return;
    }
    let stack = d.entities.inventory_slot(player, slot);
    if stack.is_empty() {
        return;
    }
    let next = crate::stack::worn(c, stack, amount);
    if next != stack {
        d.entities.set_slot(player, slot, next);
    }
}
