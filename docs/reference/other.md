# Everything else reference

Types that belong to no topic above.

## Client

- <a id="Client-environment"></a>`.environment(layer: &Environment) -> bool`: Adds or replaces a persistent local source. Repeated calls replace its target without restarting its fade. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/environment.rs#L153)
- <a id="Client-clear_environment"></a>`.clear_environment(name: &str)`: Removes a local source, retaining its configured fade-out. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/environment.rs#L158)
- <a id="Client-environment_values"></a>`.environment_values() -> EnvironmentValues`: The final parameters from the last environment evaluation. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/environment.rs#L162)

## DeclarationBuilder

A component's declaration being registered (`Registry::entity_component`, `stack_component`, `storage`); another mod's is `r.edit(COMP.declaration())`.

- <a id="DeclarationBuilder-stack"></a>`.stack(flags: u32)`: Stacks carry it, with `stack_flags`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/game.rs#L2109)

## Generation

The completed stages before this callback, plus its own earlier writes. Reads in the same stage never depend on another start's execution order. This view cannot outlive the callback it was borrowed from.

- <a id="Generation-read"></a>`.read(pos: impl Into<BlockPos>) -> u32`: A state at any world coordinate, including across cube boundaries; air outside the dimension. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/generation.rs#L18)
- <a id="Generation-column"></a>`.column(x: i32, z: i32) -> Column`: The terrain's reference height and biome. This is metadata, before features change blocks. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/generation.rs#L22)
- <a id="Generation-find"></a>`.find(from: impl Into<BlockPos>, dir: Dir, distance: u32, mut test: impl FnMut(u32) -> bool) -> Option<BlockPos>`: First cell matching `test`, starting at `from`, then at most `distance` steps in `dir`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/generation.rs#L28)
- <a id="Generation-all"></a>`.all(from: impl Into<BlockPos>, to: impl Into<BlockPos>, mut test: impl FnMut(u32) -> bool) -> bool`: Every cell in the box between two opposite corners matches `test`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/generation.rs#L43)
- <a id="Generation-surface_block"></a>`.surface_block(x: i32, z: i32) -> u32`: What is now at the terrain's reference surface; air where it was carved away. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/generation.rs#L57)
- <a id="Generation-surface_carved"></a>`.surface_carved(x: i32, z: i32) -> bool`: Whether the terrain's reference surface is now empty. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/generation.rs#L62)
- <a id="Generation-fit"></a>`.fit(x: i32, z: i32, size: impl Into<BlockPos>, max_slope: i32) -> Option<BlockPos>`: Fits a footprint at the terrain's reference height, rejecting missing ground and steep slopes. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/generation.rs#L66)
