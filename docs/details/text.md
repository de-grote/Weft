# Text on the screen



```rust
w.hint("mymod.no_mana", &[]).to(player);                          // a short line above the hotbar
w.chat("mymod.boss_up", &[Arg::Text(name)]).to(Who::All);           // a line in the chat
w.title("mymod.round", &[Arg::Int(3)]);                             // big, over the crosshair
w.subtitle("mymod.get_ready", &[]).to(Who::Near(w.dimension(), pos, 32.0)); // under the crosshair
w.show("mymod.timer", &[Arg::Int(left)])                            // by itself, anywhere
    .to(player)
    .id("mymod:timer")
    .at(0.98, 0.02)
    .scale(2.0);
```

`.to(..)` takes a player's id or a `Who`; without it the text goes to everyone in the dimension. It is sent at the end of the statement, and everything can be changed on any of them:
- `.at(x, y)` in fractions of the window. That point of the text lies at the same point of the window, so `.at(1.0, 0.0)` is exactly the top right corner. A hint with `.at` is no longer in the HUD's flow.
- `.scale(s)`: 1 is menu text, a subtitle is 2, a title 4.
- `.times(fade_in, stay, fade_out)` in seconds.
- `.color(0xRRGGBB)` for what doesn't color itself with `{#color}`.
- `.id("mymod:timer")`: new text with the same place and id replaces the old one, so a timer counts down in one chat line instead of filling the chat. An empty key with no args takes it away.

Text, sounds, particles and gizmo shapes are one `Output` that the server's `World` and a `Client` both have, with the same calls. On the server each goes to whoever it reaches by itself unless `.to(..)` says otherwise: a sound to the players in its range, particles to those near them, a gizmo to those showing its group, text to the dimension. On a client it goes to that client alone, and only if its `.to(..)` takes that player in, so code both sides run writes it once: `out.play_sound(&CHIME, None, 1.0).to(player)` dings for that player whichever side calls it.

Players move or hide hints and titles in the HUD editor (`core:hint`, `core:title`, `core:subtitle`), so use those for anything a player might want elsewhere, and `.at` when the position is the point. A place of your own works the same: `r.text_place(&QUEST).hud(hud_anchor::TOP_RIGHT, 0).stacks()` makes one more element of the HUD, and `w.show_at(player, &QUEST, key, args)` puts text there; the engine's are in `text_places`, and `hint`, `title` and the rest are `show_at` with one of them. The client has the same calls for its own screen: `c.show`, `c.show_at`, `c.hint`, `c.title`, `c.subtitle`, `c.chat` (`c.say` sends a line as if typed). A check that reads the world can serve both sides: written over `&impl View` (`raining`, `day_time`, `block`, `light`, `entity`, `component_of`), it takes the server's `w` and the client's `c.view()` alike. `c.draw_screen_text` is different: it draws for one frame, for what you animate yourself.

A player's message goes through one mod, the last loaded that asked. Pass what they typed as `Arg::Text`, or players get to format the line:
```rust
r.chat_format(|f| f.set("mymod.chat", &[Arg::Text(rank(f.player())), Arg::Text(f.text())]));
```
Commands: `/title <who> title|subtitle|hint <text>`, `/title <who> show <x> <y> <size> <text>`, `/title <who> times <in> <stay> <out>`, `/title <who> clear|reset`, `/say`, `/msg`, `/tellraw <who> <text>`. Text in `/title` and `/tellraw` can use `{#color}` and the rest; what `/say`, `/msg` and the chat get stays as typed.
