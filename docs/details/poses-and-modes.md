# Poses and game modes



## Poses

Poses are values too: `r.pose(&SIT).height(0.7).eye(0.62).speed(0.0).action("sit")` registers one (`tilt(pose_tilt::FACE_DOWN)` lays the model down), and `w.set_pose(id, Some(&SIT), None)` holds a mob or a player in it until `None` (with a facing in degrees it also stays still and turned so, the way a bed lays a player down). The engine still picks standing, crouching, swimming and crawling (`keys::CRAWL` held, or no room for more) by itself while nothing holds the pose, for a mob too if its kind has `.poses()`

## Game modes

Game modes are values with abilities, the engine's three as well as yours: `r.game_mode(&ADVENTURE).breaks(Breaking::None).no_placing()` is an adventure mode, with `no_costs`, `invulnerable`, `flight(Flight::Allowed)`, `noclip`, `hidden`, `no_interacting`, `no_hud`, `creative_inventory`, `with(flags)` and `.modifier(Modifier::add(&attribute::BLOCK_REACH, 1.0))` for the rest. `/gamemode mymod:adventure` and `{if:mymod:adventure}` know it at once, and it is saved by name
