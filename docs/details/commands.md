# Commands



`r.command("heal", "[amount:int] [who:targets]", |cmd| ..)`\
The usage string is the whole spec: the engine parses it, checks types, completes it with Tab and shows it in `/help`.\
Reading values by name: `cmd.int("amount")`, `cmd.targets("who")`. `[seconds:int(infinite)]` takes a number or the word `infinite`, offered by Tab like `word(a,b)` lists; `cmd.text("seconds")` has the word.\
Answering: `cmd.reply("mymod.healed", &[Arg::Int(amount)])` with `mymod.healed = healed by {0}` in your lang files. The reply travels as the key and its arguments and each player's client formats it in its own language, so a server never sends English text. The same goes for gizmo labels (`w.draw_text(Some(&GROUP), pos, key, &args, ..)`) and the client's `draw_screen_text`.\
Who may run it goes by permission levels, the same for the engine's commands and yours: 0 everyone, 1 moderator, 2 game master, 3 admin, 4 the host. A command asks for level 2 unless you say otherwise, `r.command(..).permission(permission::EVERYONE)`, and players below it are refused and not offered it. `/op <who> [level]` hands them out, and for anything else your mod does, `w.permission(player)` tells a player's level and `w.set_permission(player, level)` changes it.

The callback gets a `ServerCommand`: `cmd.world()` is where it runs, and what it changes settles when it returns (neighbors updated, events delivered), like `/setblock`.`r.client_command` is the same for commands that run on the client that typed them: a `ClientCommand` with `cmd.client()` and no world.`r.command_branch("time", "dawn", ..)` adds a branch to someone else's command (`r.client_command_branch` for client ones like `/gizmo`), and a `r.command` named like one of the engine's (`/give`, `/tp`...) replaces it.
