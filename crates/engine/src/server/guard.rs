use glam::DVec3;
use weft_abi as abi;

use super::dim::Dim;
use crate::content::Content;

pub(super) enum DamageOutcome {
    Missed,
    Hurt,
    Blocked(u32),
}

impl Dim {
    pub(super) fn blocking_item(&self, c: &Content, stack: &abi::ItemStack) -> Option<abi::Blocking> {
        let comp = c.components.id(abi::comp::BLOCKS_ATTACKS)?;
        let bytes = crate::stack::value(stack.data, comp).or_else(|| {
            c.components
                .bytes(crate::components::ITEM, stack.item, comp)
        })?;
        abi::data::from_bytes(bytes)
    }

    pub(super) fn block_damage(&mut self, c: &'static Content, id: u64, d: &abi::Damage) -> Option<u32> {
        let e = self.entities.get(id)?;
        if d.amount <= 0.0 || e.blocking_disabled_until > self.time {
            return None;
        }
        let (item, left, slot) = e.use_done?;
        let stack = self.entities.inventory_slot(id, slot);
        if stack.item != item {
            return None;
        }
        let blocking = self.blocking_item(c, &stack)?;
        if e.using.0.saturating_sub(left) < blocking.delay_ticks {
            return None;
        }
        let cause = c.game.damage_causes.get(d.cause as usize)?;
        if cause.has(crate::tag_bits::damage::BYPASSES_BLOCKING) {
            return None;
        }
        let direct = if d.direct != 0 { d.direct } else { d.source };
        let toward = (d.knock != abi::Vec3::ZERO)
            .then(|| -DVec3::from_array(d.knock.to_array()))
            .or_else(|| self.entities.get(direct).map(|s| s.pos - e.pos))?
            .with_y(0.0)
            .try_normalize()?;
        let look = crate::physics::look_dir(e.yaw, 0.0);
        if toward.dot(look) <= (blocking.arc.to_radians() as f64 * 0.5).cos() {
            return None;
        }
        let creative = e.mode.has(crate::tag_bits::game_mode::NO_COSTS);
        let disable = c
            .stack_value::<f32>(&d.item, c.components.id(abi::comp::DISABLE_BLOCKING))
            .unwrap_or(0.0);
        if !creative && d.amount > blocking.wear_threshold {
            let wear = blocking
                .wear_base
                .saturating_add((d.amount * blocking.wear_factor).floor().max(0.0) as u32);
            self.entities
                .set_slot(id, slot, crate::stack::worn(c, stack, wear));
        }
        if disable > 0.0 {
            if let Some(e) = self.entities.get_mut(id) {
                e.blocking_disabled_until = self.time + (disable * crate::world::TPS as f32).ceil() as u64;
            }
            self.stop_using(id);
        } else if self.entities.inventory_slot(id, slot).is_empty() {
            self.stop_using(id);
        }
        Some(slot)
    }

    pub(super) fn catch_projectile(&mut self, c: &Content, target: u64, slot: u32, projectile: u64) {
        let stack = self.entities.inventory_slot(target, slot);
        if stack.is_empty() {
            return;
        }
        let Some(comp) = c.components.id(abi::comp::CATCH_PROJECTILES) else { return };
        let Some(catch) = crate::stack::value(stack.data, comp)
            .or_else(|| {
                c.components
                    .bytes(crate::components::ITEM, stack.item, comp)
            })
            .and_then(abi::data::from_bytes::<abi::ProjectileCatch>)
        else {
            return;
        };
        let Some(comp) = c.components.id(abi::comp::EMBEDDED_PROJECTILES) else { return };
        let Some(e) = self.entities.get(projectile) else { return };
        let kind = c.game.entities[e.kind as usize].name.clone();
        let mut arrows: Vec<abi::EmbeddedProjectile> = crate::stack::value(stack.data, comp)
            .and_then(abi::data::from_bytes)
            .unwrap_or_default();
        // Bound stack size even when an invulnerable player blocks indefinitely.
        if arrows.len() == 32 {
            arrows.remove(0);
        }
        let position = std::array::from_fn(|i| catch.min[i] + (catch.max[i] - catch.min[i]) * self.random_unit());
        arrows.push(abi::EmbeddedProjectile { kind, bone: catch.bone, position, direction: catch.direction });
        let bytes = abi::data::bytes_of(&arrows);
        let data = crate::stack::with(stack.data, comp, Some(&bytes));
        self.entities
            .set_slot(target, slot, abi::ItemStack { data, ..stack });
    }
}
