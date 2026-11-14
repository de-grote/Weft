//! A player coming and going: its file, joining, the mode it left in.

use super::*;

impl Server {
    pub(super) fn join(&mut self, c: &'static Content, pi: usize, name: String, key: String) {
        let key = if key.is_empty() { name.clone() } else { key };
        if self.players.iter().any(|p| p.joined && p.key == key) {
            self.players[pi]
                .conn
                .send(ToClient::Disconnect(format!("{name} is already playing")));
            return;
        }
        // Read each time, so a key added takes effect with no restart. The key is what the
        // connection proved, not what the client claims.
        if let Some(Ok(list)) = self.cfg.whitelist.as_ref().map(std::fs::read_to_string) {
            if !list.lines().any(|l| l.trim() == key) {
                log::info!("{name} is not on the whitelist; to let them in, add this line: {key}");
                self.players[pi]
                    .conn
                    .send(ToClient::Disconnect("not on this server's whitelist".into()));
                return;
            }
        }
        let saved = self
            .storage
            .as_ref()
            .and_then(|s| load_player(c, &s.lock(), &key));
        let spawn = || (c.spawn_dim, self.dims[c.spawn_dim as usize].spawn_point(c));
        let (dim, pos, health, inv, comps, look, selected, flying, fire) = match saved {
            // A dimension that is gone (its mod removed), or the void while a real one exists:
            // everything else is kept, the player starts at the spawn.
            Some(s) => {
                let ((dim, pos), keep) = match s.0 {
                    Some(d) if d != c.void_dim || c.spawn_dim == c.void_dim => ((d, s.1), true),
                    _ => (spawn(), false),
                };
                if !keep {
                    log::warn!("player {name} was in a dimension that is not there now, placed at the spawn");
                }
                (dim, pos, s.2, s.3, s.4, s.5, s.6, s.7, s.8)
            }
            None => {
                let (d, p) = spawn();
                (d, p, -1.0, None, Vec::new(), [0.0, 0.0], 0, false, 0)
            }
        };
        let r = self.dims[dim as usize].ensure_region(c, pos);
        let id = r.new_id();
        let mut e = Entity::new(id, c.game.player_kind, pos, health);
        e.player = true;
        let (mode, deaths) = self.storage.as_ref().map_or((None, None), |s| {
            let s = s.lock();
            (saved_field(&s, &key, "mode").and_then(|m| GameMode::parse(&m)), saved_field(&s, &key, "deaths").and_then(|d| d.parse().ok()))
        });
        // A new player gets the mode the world was made with.
        e.mode = mode.unwrap_or_else(|| {
            self.storage
                .as_ref()
                .map_or(GameMode::SURVIVAL, |s| s.lock().meta.game_mode)
        });
        (e.yaw, e.pitch) = (look[0], look[1]);
        (e.flying, e.fire) = (flying, fire);
        // The first connection is the one playing on this machine, the world's owner, with
        // commands if the world was made with cheats.
        let cheats = self.storage.as_ref().is_none_or(|s| s.lock().meta.cheats);
        let permission = if pi == 0 && !self.cfg.dedicated && cheats {
            abi::permission::OWNER
        } else {
            self.storage
                .as_ref()
                .and_then(|s| s.lock().meta.ops.iter().find(|o| o.0 == key).map(|o| o.1))
                .unwrap_or(abi::permission::EVERYONE)
        };
        e.permission = permission;
        e.selected = selected.min(c.game.player_layout.hotbar.saturating_sub(1));
        let selected = e.selected;
        // Effects come back with the ticks they had left, by this dimension's clock.
        let comps =
            comps
                .into_iter()
                .map(|(k, v)| {
                    if k == c.game.fields.effect {
                        (k, super::persist::effects_from_save(c, &v, r.time).into_boxed_slice())
                    } else {
                        (k, v)
                    }
                })
                .collect();
        r.entities.insert_with(c, e, inv, comps);
        r.keep_sources(c, id);
        // Known only now that the modifiers are back; a new player (no saved health) starts full.
        let max = r.max_health(c, id);
        let mut dead = false;
        if let Some(e) = r.entities.get_mut(id) {
            if e.health < 0.0 {
                e.health = max;
            }
            dead = !e.alive();
        }
        r.emit_engine(c, abi::event::PLAYER_JOINED, &abi::PlayerEvent { player: id });
        let p = &mut self.players[pi];
        p.name = name;
        p.key = key;
        p.joined = true;
        p.dim = dim;
        p.entity = id;
        p.last_place = (None, pos);
        p.deaths = deaths.unwrap_or(0);
        p.first_sync = true;
        p.glow_sent = false;
        log::info!("{} joined", p.name);
        let line = Msg::new("multiplayer.joined", vec![Arg::Text(p.name.clone())]);
        for q in self.players.iter_mut().filter(|q| q.joined) {
            q.conn
                .send(ToClient::Show(crate::net::Shown::chat(line.clone())));
        }
        let p = &mut self.players[pi];
        p.conn
            .send(ToClient::Welcome { player: id, dim, pos, seed: self.cfg.seed, look, selected, flying });
        self.send_lod_columns(pi, dim);
        let p = &mut self.players[pi];
        p.conn.send(ToClient::Edits {
            dim,
            specs: self.dims[dim as usize]
                .edits
                .iter()
                .map(|e| e.spec(c))
                .collect(),
            all: true,
        });
        p.conn.send(ToClient::Configs(crate::config::synced()));
        // Left while dead: still dead, as in vanilla.
        if dead {
            p.conn.send(ToClient::Died);
        }
        if dim == c.void_dim {
            p.conn
                .send(ToClient::Show(crate::net::Shown::chat(void_note(c))));
        }
        // The skins of who is here already.
        let skins: Vec<(u64, Arc<Vec<u8>>)> = self
            .players
            .iter()
            .filter(|o| o.joined && o.entity != id)
            .filter_map(|o| o.skin.clone().map(|s| (o.entity, s)))
            .collect();
        for (entity, png) in skins {
            self.players[pi]
                .conn
                .send(ToClient::Skin { entity, png: png.to_vec() });
        }
        let avatars: Vec<(u64, Arc<crate::net::AvatarData>)> = self
            .players
            .iter()
            .filter(|o| o.joined && o.entity != id)
            .filter_map(|o| o.avatar.clone().map(|a| (o.entity, a)))
            .collect();
        for (entity, avatar) in avatars {
            self.players[pi]
                .conn
                .send(ToClient::Avatar { entity, avatar: (*avatar).clone() });
        }
    }
}

pub(super) fn save_player(s: &Storage, p: &Player, d: &Dim) {
    let c = content();
    let path = s.player_path(&p.key);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Some(e) = d.entities.get(p.entity) else { return };
    let mut text = format!(
        "name = {}\ndim = {}\npos = {} {} {}\nlook = {} {}\nselected = {}\nhealth = {}\n",
        p.name, c.dims[p.dim as usize].name, e.pos.x, e.pos.y, e.pos.z, e.yaw, e.pitch, e.selected, e.health
    );
    text.push_str(&format!("flying = {}\nfire = {}\n", e.flying as u8, e.fire));
    if e.mode != GameMode::SURVIVAL {
        text.push_str(&format!("mode = {}\n", e.mode.name()));
    }
    if p.deaths > 0 {
        text.push_str(&format!("deaths = {}\n", p.deaths));
    }
    if let Some(inv) = d.entities.inventories.get(&p.entity) {
        let items: Vec<String> = inv
            .iter()
            .map(|s| {
                if s.is_empty() {
                    "-".into()
                } else {
                    format!("{}*{}*{}", c.items[s.item as usize].name, s.count, crate::stack::to_text(crate::stack::for_save(s.data)))
                }
            })
            .collect();
        text.push_str(&format!("inventory = {}\n", items.join(" ")));
    }
    for ((id, comp), bytes) in &d.entities.comps {
        if *id == p.entity && c.game.components[*comp as usize].flags & cf::SAVE != 0 {
            let named = if crate::stack::holds_stacks(c, *comp) {
                Some(crate::stack::contents_to_names(c, bytes))
            } else {
                (*comp == c.game.fields.effect).then(|| super::persist::effects_to_save(c, bytes, d.time))
            };
            let bytes = named.as_deref().unwrap_or(bytes);
            text.push_str(&format!("comp.{} = {}\n", c.game.components[*comp as usize].name, crate::kv::hex(bytes)));
        }
    }
    // Whole or not at all: a crash in the middle must not leave half a player.
    let tmp = path.with_extension("new");
    if let Err(err) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path)) {
        log::error!("saving player {}: {err}", p.name);
    }
}

/// What a player's file says of `field`.
fn saved_field(s: &Storage, key: &str, field: &str) -> Option<String> {
    let src = std::fs::read_to_string(s.player_path(key)).ok()?;
    crate::kv::get(&src, field).map(str::to_string)
}

/// Dimension (`None`: unknown now), position, health, inventory, components.
pub(super) type PlayerData = (Option<u32>, DVec3, f32, Option<Vec<ItemStack>>, Vec<(u32, Box<[u8]>)>, [f32; 2], u32, bool, u32);

/// A saved player. Content that no longer exists is dropped with a log line; a file that cannot
/// be read is kept as `.bak` before it gets overwritten by a fresh player.
pub(super) fn load_player(c: &Content, s: &Storage, key: &str) -> Option<PlayerData> {
    let path = s.player_path(key);
    let src = match std::fs::read_to_string(&path) {
        Ok(src) => src,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            log::error!("reading player {key}: {e}; kept as .bak, the player starts anew");
            let _ = std::fs::copy(&path, path.with_extension("bak"));
            return None;
        }
    };
    let (mut dim, mut pos, mut health, mut inv, mut comps) = (None, None, -1.0, None, Vec::new());
    let (mut look, mut selected, mut flying, mut fire) = ([0.0f32; 2], 0u32, false, 0u32);
    for (_, k, v) in crate::kv::pairs(&src) {
        match k {
            "dim" => {
                dim = c.dim_names.get(v).copied();
                if dim.is_none() {
                    log::warn!("player {key}: dimension {v} does not exist now");
                }
            }
            "look" => {
                let n: Vec<f32> = v
                    .split_whitespace()
                    .filter_map(|x| x.parse().ok())
                    .collect();
                if n.len() == 2 {
                    look = [n[0], n[1]];
                }
            }
            "selected" => selected = v.parse().unwrap_or(0),
            "flying" => flying = v == "1",
            "fire" => fire = v.parse().unwrap_or(0),
            "pos" => {
                let n: Vec<f64> = v
                    .split_whitespace()
                    .filter_map(|x| x.parse().ok())
                    .collect();
                if n.len() == 3 {
                    pos = Some(DVec3::new(n[0], n[1], n[2]));
                }
            }
            "health" => health = v.parse().unwrap_or(-1.0),
            "inventory" => {
                let mut slots = vec![ItemStack::EMPTY; c.game.player_layout.slots as usize];
                for (i, tok) in v.split_whitespace().enumerate().take(slots.len()) {
                    let parts: Vec<&str> = tok.split('*').collect();
                    if parts.len() == 3 {
                        let item = c.item_names.get(parts[0]).copied();
                        if item.is_none() {
                            log::warn!("player {key}: dropped {} {} (no such item now)", parts[1], parts[0]);
                        }
                        if let Some(item) = item {
                            slots[i] = ItemStack { item, count: parts[1].parse().unwrap_or(1), data: crate::stack::from_text(parts[2]) };
                        }
                    }
                }
                inv = Some(slots);
            }
            _ => {
                if let Some(name) = k.strip_prefix("comp.") {
                    let id = c.game.component(name);
                    if id.is_none() {
                        log::warn!("player {key}: dropped component {name} (not registered now)");
                    }
                    if let Some(id) = id {
                        let bytes = crate::kv::unhex(v);
                        let bytes = if crate::stack::holds_stacks(c, id) { crate::stack::contents_from_names(c, &bytes) } else { bytes };
                        comps.push((id, c.game.fit_component(id, bytes)));
                    }
                }
            }
        }
    }
    let Some(pos) = pos else {
        log::error!("player {key}: no position in the file; kept as .bak, the player starts anew");
        let _ = std::fs::copy(&path, path.with_extension("bak"));
        return None;
    };
    Some((dim, pos, health, inv, comps, look, selected, flying, fire))
}
