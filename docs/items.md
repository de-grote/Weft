# Items

`r.item(&ITEM)` and then the calls below. Stacks that carry values of their own are at the end. Every method is in the [reference](reference/items.md).

## Settings

- `texture`
- `max_stack(N)`
- `durability`
- `tool("pickaxe", tier)`, `mining_speed(speed)`
- `attack_damage`, `attack_speed(per_second)` (hitting before it has recharged does less; a sword is 1.6, and without it an item swings at an empty hand's 4) and `sweeps()` (a full swing on the ground also hits what stands next to the target: 1, plus `attribute::SWEEPING_DAMAGE_RATIO` of the hit)
- Advancements read tags, so yours count by tagging them: `cooked_meat` on food, `hostile` and `livestock` on mobs, `logs`, `torches`, `furnaces`, etc etc
- `no_tab()`: in no creative tab nor its search, still obtainable (a variant you hand out, a spoiler); another mod brings it back with `.untag(abi::comp::NO_TAB)`
- `no_creative_break()`: in creative it doesn't break blocks, like a sword. Both sides know, so nothing flickers; a rule that depends on more is `actions::BREAK_BLOCK`, which is asked when mining starts and tells the client no right away
- `spawns(&ENTITY)`: used on a block it puts the entity there. Every entity already has an egg of its own, see below
- `places(&BLOCK)`
- `handheld()`: held by its end and pointing ahead in third person, like a stick. `tool(..)` does it already, and so does the tag `handheld`
- `wearable(&HEAD, inflate, texture)` for armor and anything else worn in an equipment slot
- `light(14)`, `light_rgb(r, g, b)`: lights up around whoever holds or wears it, or where it lies. Every client draws that light itself, so it doesn't spawn-proof or grow anything. An item that places a block already gives that block's light

## Using an item

- `two_handed()`, `one_handed_action(action)`, `use_priority(n)`: [what it does in the hands](models.md#items-in-the-hand)
- `on_use`: what using it does. Before it runs anyone can say no to `actions::USE_ITEM`, and after it the same action's event tells every mod (testmod heals with base's food through it). `base_api::eat` is the base game's eating: `.set(&FOOD, ..).use_ticks(32).on_use(base_api::eat)`
- `use_until_released()` and `on_release(|u| ..)`: held as long as the button is, and letting go does something, `u.held_ticks()` saying after how long. Base's bow is that: `.use_until_released().on_release(shoot_bow)`, the draw's strength from the ticks. While it is held, `w.entity(id)` says what is being used and for how long (`using`, `using_ticks`): base's bow narrows the view by a step on the `fov` attribute that reads them, and its `use` clip (arms, the string, the nocked arrow) plays by the seconds held. `on_release` also hears an item with `use_ticks` let go too early. Anything finer reads the keys themselves, see Client side

## Stacks with their own data

A stack is an item, a count, and whatever values of components it carries of its own, like the damage a tool took. You never touch `data` yourself, it's only a handle to those values:
```rust
value! { pub struct Quality { pub grade: u32, pub maker: u32 } }
static QUALITY: Component<Quality> = Component::named("quality");

// While registering, once:
r.stack_component(&QUALITY, 0);

// Making one, a custom workbench's output for example:
let out = PICKAXE.stack(1)
    .with(&QUALITY, Quality { grade: 3, maker: 7 })
    .with_wear(10);
inventory.set(slot, out);

// Reading it:
QUALITY.on_stack(&stack);   // its own value, else what the item has
QUALITY.on_stack_own(&stack); // only its own
stack.wear();
let plain = stack.without(&QUALITY);
```
A stack's own value wins over its item's for everything the engine reads too: give one stack its own `DURABILITY`, `MINING_SPEED`, `TOOL_LEVEL`, `ATTACK_DAMAGE`, `ATTACK_SPEED`, `USE_TICKS` or a `tools/<class>` tag and it digs, wears and hits by those, and `stack.with_modifiers(&[Modifier::add(&attribute::ATTACK_DAMAGE, 2.0)])` gives it modifiers of its own instead of its item's (an enchanted sword). Anything about a stack can be changed from outside: its item, count and any value, with `inventory.set(slot, ..)` and `.with(..)`.

A tool made of parts keeps them in a storage (`r.storage(&PARTS, 3)`, see [block entities](block_entities.md)): `stack.stacks(&PARTS)` and `stack.with_stacks(&PARTS, &[handle, head, binding])` read and write them right on the stack, no slot needed, and each part is a stack with its own values and parts, as deep as you go. Add the parts' values up when you make it and write the result onto the stack. `stack.with_layers(Layers::new().layer("rod").layer("pick_head").tint(0xB0B0B0))` makes it show its parts' pictures one over the other (`tint` colors the layer just added), in slots, in the hand and on the ground.

Stacks with different data don't merge. `r.stack_component` says a component exists for stacks, its flags are `abi::stack_flags`: `SERVER_ONLY` (clients get the stack without it), `NO_SAVE`, or 0 for neither. `.with` on a component nobody declared does nothing and says so in the log. Ids in a value shift when a mod comes or goes, the same as in a `SAVE` component (see [components](modding.md#components-and-tags-as-inter-mod-communication)).
