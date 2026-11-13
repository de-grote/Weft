# Commands reference

Command definitions and what a command callback gets.

## ClientCommand

A command run on the client that typed it (`Registry::client_command`), never sent.

- <a id="ClientCommand-client"></a>`.client() -> crate::Client` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L55)

## Command

What every command gets: its arguments and a way to answer.

- <a id="Command-player"></a>`.player() -> u64`: Entity id of the player who ran the command. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L85)
- <a id="Command-int"></a>`.int(name: &str) -> Option<i64>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L96)
- <a id="Command-float"></a>`.float(name: &str) -> Option<f64>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L102)
- <a id="Command-text"></a>`.text(name: &str) -> Option<&str>`: Words, text, player names, and the full names of items, blocks, entities, dimensions, attributes. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L110)
- <a id="Command-id"></a>`.id(name: &str) -> Option<u32>`: Registry id of an item, block state, entity kind, dimension or attribute parameter. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L117)
- <a id="Command-targets"></a>`.targets(name: &str) -> Vec<u64>`: Entities a `targets` parameter selected (`@e[type=zombie]`, a player name, ...). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L124)
- <a id="Command-pos"></a>`.pos(name: &str) -> Option<Vec3>` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L135)
- <a id="Command-given"></a>`.given(name: &str) -> bool`: Whether a literal word of the usage (or an optional parameter) was given. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L142)
- <a id="Command-reply"></a>`.reply(key: &str, args: &[crate::Arg])`: A line for whoever ran the command: the translation `key` with `args`, formatted by their client in its own language. Call again for more lines. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L148)

## CommandOptions

What `Registry::command` gives back, to say more about the command.

- <a id="CommandOptions-permission"></a>`.permission(level: u32)`: The `abi::permission` level it asks for. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L30)

## ServerCommand

A command run on the server (`Registry::command`).

- <a id="ServerCommand-world"></a>`.world() -> World`: The world it runs in. What it changes settles when it returns (neighbors, events). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/command.rs#L46)
