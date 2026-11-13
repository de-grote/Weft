# Particles and sounds



## Particles

Spawn them with `w.spawn_particles(&SPARK, pos).count(20).speed(2.0).color(0xFF4020)`, from the client too. Particles live on the GPU, so what happens to one is data on its kind, not code: `on_land`, `on_wall`, `on_ceiling`, `on_enter_liquid`, `on_leave_liquid` and `on_expire` each take `When::any()`, `When::liquid()` or `When::tag("water")` and `Then::die()`, `stop()`, `slide()`, `bounce(0.4)` or `turn_into(&OTHER)` / `turn_into_still(&OTHER)`; the first rule that holds wins. Rain that rings on water and is gone elsewhere is `.on_enter_liquid(When::tag("water"), Then::turn_into_still(&RIPPLE)).on_land(When::any(), Then::die())`, an ember that bounces and dies into smoke `.on_land(When::any(), Then::bounce(0.4)).on_expire(When::any(), Then::turn_into(&SMOKE))`. At most 5 different tags over all particle rules of the game

## Sounds

`w.play_sound(&S, pos, volume)` plays one, and `.pitch(2.0)` on it sets an exact playback speed. `r.sound(&S).falloff(4.0, Falloff::Late(2))` keeps a sound loud further out.
