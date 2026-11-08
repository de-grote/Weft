//! `/attribute <attribute> <value> [who]` sets an attribute of players apart from what the rules and
//! everything else make it; `/attribute <attribute> reset [who]` takes that back.

use weft_sdk::*;

const MODIFIER: &str = "base:attribute_command";

pub fn register(r: &mut Registry) {
    r.command("attribute", "<attribute:attribute> reset [who:targets] | <attribute:attribute> <value:float> [who:targets]", |cmd| {
        let w = cmd.world();
        let Some(name) = cmd.text("attribute").map(String::from) else { return };
        let attr = Attribute::generated(name.clone());
        let targets = if cmd.given("who") { cmd.targets("who") } else { vec![cmd.player()] };
        match cmd.float("value") {
            Some(value) => {
                for &id in &targets {
                    w.attribute_add_modifier(id, &attr, MODIFIER, attr_op::SET, value, 0);
                }
                cmd.reply("command.base.attribute.done", &[Arg::Text(&name), Arg::Float(value, 2), Arg::Int(targets.len() as i64)]);
            }
            None => {
                for &id in &targets {
                    w.attribute_remove_modifier(id, &attr, MODIFIER);
                }
                cmd.reply("command.base.attribute.cleared", &[Arg::Text(&name), Arg::Int(targets.len() as i64)]);
            }
        }
    });
}
