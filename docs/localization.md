# Localization guide

All the text in the game comes from the language files, tooltips included (the boxes that follow the
cursor over items, creative tabs and settings). There is no code for the usual case, and any mod or
resource pack can add or change text, even for things in other mods.

## Note
The text is a normal translation, so everything from the rich text list below works in it.\
Examples to copy from: `mods/base/assets/lang/en_us.lang` (`block.base.furnace.tooltip`)\
and `mods/testmod/assets/lang/en_us.lang` (`item.testmod.rune.tooltip`, with effects and conditions).

## Quick links
[rich text](#rich-text) | [effects](#effects) | [conditions](#conditions) | [icons and meters](#icons-and-meters) | [item tooltips](#item-tooltips) | [lines by tag](#lines-by-tag) | [other tooltips](#other-tooltips) | [adding from outside](#adding-from-outside) | [look](#look) | [limits](#limits)

## Rich text
Tags in the translation color the text, show keys and icons, hide parts or animate them.\
They work in any translation, not only in tooltips:

| tag | what it does |
|---|---|
| `{#rrggbb}text{/}` | colors the text, `{/}` goes back to the default |
| `{item:base:coal}` | draws the item's icon inline |
| `{block:base:furnace[lit=true]}` | draws a block state like a block item; properties not named keep their defaults |
| `{img:base:arrow}` | draws a texture from `gui/` at its own size, `{img:base:arrow 32x16}` at another (GUI pixels); the line grows to fit |
| `{icons:base:food_full value.hunger/2}` | a row of a picture, as many as the number says, see [icons and meters](#icons-and-meters) |
| `{value}`, `{value.hunger}`, `{value/200}`, `{value:food.hunger}` | a number from the item the text is about, see [lines by tag](#lines-by-tag) |
| `{align:center}text{/align}` | lays out the lines inside centered (`left`, `right`); a picture alone on such a line is a centered picture |
| `{key:core:sneak}` | the key the player has bound to that action |
| `{lang:some.key}` | inserts another translation, so a shared phrase is written once |
| `{name:item:base:coal}` | the display name of something, colors and all: `item:`, `block:`, `liquid:`, `tab:`, `mod:`, `recipe:`, `config:`, `key:`. Without a translation it shows the raw name. From code it's `Arg::Name("item:base:coal")` |
| `{on_key:core:sneak}text{/on_key}` | shown only while the key is held; `!` in front inverts it; raw keys work too, but it's recommended to not use them |
| `{if:creative}text{/if}` | shown while a condition holds, `!` inverts it: `{if:!creative}text{/if}`; a comparison works too: `{if:value.toughness>0}` (`<`, `<=`, `>`, `>=`, `==`, `!=`) |
| `{fx:core:wave}text{/fx}` | animates the text with an effect |
| `{0}` ... `{9}` | arguments given by the code in order (durability uses them) |
| `{{` | a literal `{` |

Hidden parts (`on_key`, `if`) are basically instant, the text is just laid out again when they change.

### Effects
`{fx:...}` animates the letters of the text. The engine has `core:rainbow`, `core:wave`, `core:pulse`
and `core:shake`.

A new one is a file `shaders/fx/<name>.wgsl` in any mod or pack, used as `{fx:mymod:<name>}`.\
The file has one function that gets a letter and returns it moved and/or tinted. What a letter carries (its offset, color, position in the text and so on) is listed in the [shaders guide](shaders.md#text-effects), so here is just a small example, letters that jump sideways now and then (the real one is `mods/testmod/assets/shaders/fx/glitch.wgsl`):
```wgsl
fn effect(g: Glyph) -> Glyph {
    var o = g;
    let t = floor(g.time * 8.0);
    let h = fract(sin(g.index * 91.7 + t * 13.1) * 43758.5453);
    if h > 0.8 {
        o.offset.x = (h - 0.9) * 20.0 * g.scale;
    }
    return o;
}
```

### Conditions
`{if:name}` shows text while a condition holds.\
The engine's are every game mode by its name (`creative`, `survival`, `spectator`), `day`, `night`, `underground`, `surface`, `light(..7)`, `debug` and `dev`, and they go together with `&`, `|` and `!`: `{if:night & surface}`. Your own are registered in the mod, and in text they are asked on the client, so they see the local player only:
```rust
static CHARGED: Condition = Condition::named("charged");
r.condition(&CHARGED, |c| c.client().is_some_and(|v| v.component(&CHARGE).is_some_and(|c| c > 0)));
```
then `{if:mymod:charged}Charged!{/if}` in any translation.

## Icons and meters
`{icons:<picture> <amount>}` draws a row of a picture from `gui/`. The amount is a number or a value, and it doesn't have to be whole, `2.8` is two pictures and most of a third. Options go after it:
```
{icons:base:food_full value.hunger/2}
{icons:mymod:star 2.8 dir:left size:16x16}
```
- `dir:` is where the part of a picture fills toward: `right` (default), `left`, `up`, `down`. With `left` the part comes first in the row
- `size:` is the size of each in GUI pixels, by default the picture's own

What the part of a picture looks like is up to the files next to it:
- **Nothing else:** it's cut off, like a progress bar.
- **`<picture>_<share>`:** the nearest one is drawn whole. `hunger_0.5.png` and `hunger_0.25.png` next to `hunger.png` give quarter steps, a share nearer to nothing than to the smallest one draws nothing.
- **`<picture>_<frame>`:** when a number is over 1 they are frames of one turn, the share picks the nearest. `clock_0.png` to `clock_23.png` is a clock: 0.5 is `clock_12`.
- **`<stem>_half`** is vanilla's name for the half of `<stem>_full`, so `food_full` and `food_half` work as they are.

The same goes for the HUD's bars (`ui.icons(..)`, with `.from_right()` or `.dir(abi::ui_dir::UP)`) and for the fill of a progress bar. They have a direction of their own instead of a mirrored picture.

## Item tooltips
Hovering an item or block shows its name, and `<name key>.tooltip` under it if the language has one.\
The name line is white, the tooltip lines are gray by default (a color you set wins).\
The same tooltip shows in the inventory, the creative list and the recipe viewer. A block's is
`block.<modname>.<name>.tooltip`.

```
item.examplemod.ruby = Ruby
item.examplemod.ruby.tooltip = A red gem that smelts {#ffe070}8{/} items.
```

A new line is `\n`, or an indented line which continues the previous value on a new line:
```
block.examplemod.lamp.tooltip = Glows warmly.
    Does not burn.
```

The box is at most 320 GUI pixels wide, longer lines wrap.\
Items that wear get a `Durability: 12 / 250` line by themselves (its translation is `tooltip.durability`).

Long details usually hide behind the sneak key, so the tooltip stays short until the player wants to read that.\
`{lang:tooltip.more}` is the ready phrase "Hold <key> for details":
```
block.base.furnace.tooltip = Smelts ores and cooks food.
    {on_key:!core:sneak}{lang:tooltip.more}{/on_key}{on_key:core:sneak}Burns {item:base:coal} coal, {item:base:oak_planks} planks or other {#ffe070}fuel{/} in the bottom slot.{/on_key}
```

## Lines by tag
A line for every item with a tag or component, in one place, is `tagged.<name>`. It goes under the item's own tooltip, and a new item gets it just by having the tag:
```
tagged.food = {icons:base:food_full value.hunger/2}
tagged.armor = {#5555ff}+{value.points} Armor{/}{if:value.toughness>0}\n{#5555ff}+{value.toughness} Armor Toughness{/}{/if}
tagged.attack_damage = {#00aa00}{value} Attack Damage{/}
tagged.mymod.magic = {#ff60ff}Magical{/}
```
`<name>` is the tag's or component's, with `.` for `:`.\
`{value}` in it is that component's own number, `{value.field}` a field of it, and `/n` or `*n` scale it (`{value/200}`).\
Anywhere in an item's tooltip `{value:food.hunger}` reads any component of the item by name. The stack's own value goes before its item's.\
Numbers come from the item, so nothing in the language file changes when a mod adds a stronger sword.

A line all behind a condition (`{on_key:core:sneak}...{/on_key}`, `{if:..}`) leaves no empty line while hidden.\
Another mod adds its line to the same tag without replacing it, as with tooltips: `tagged.food.mymod = ...`.

## Other tooltips
- **Settings:** `config.<mod>.<key>.tooltip` is shown over the setting's row in Mod Settings. Settings that the
  server overrides or that need a restart get a note added by the engine.
- **Creative tabs:** show their name from `tab.<mod>.<name>`, or the mod's display name when there is no key.
- **Your own screens:** any part with a style takes one, `S::column().tooltip("examplemod.hint")`.
- **Slots:** a slot group's `tooltip(key)` shows over its empty slots ([block entities guide](block_entities.md#creating-one)).

## Adding from outside
Translations are one shared table: every `lang/<code>.lang` of every mod and pack is read into it, so the
key of an item is the same whoever wrote the file.\
A mod can add a tooltip to an item of another mod just by having the key in its own lang file.

### Adding a line
The nice way, keeps the original: put your mod's id at the end of the tooltip key.\
After the item's own tooltip the game adds every `<tooltip key>.<mod id>` it finds, one line group per mod,
in the mods' load order:
```
# mods/mymod/assets/lang/en_us.lang
item.base.coal.tooltip.mymod = Also burns in my {#ffe070}boiler{/}.
```
That way coal shows its own text and your line under it.\
It works for any item or block and doesn't need the item to have a tooltip of its own.
Several mods can do it to the same item, they don't interfere with each other.

### Replacing
Without the id the key is the original's, and your text takes its place:
```
item.base.coal.tooltip = Burns, but only in my {#ffe070}boiler{/}.
```
- if the same key is in several mods, the one whose mod id sorts last alphabetically wins; for your own items there is no
  conflict, for someone else's it's a replacement: the old text is gone, so `{lang:}` can't pull it
  back in (a key that points to itself shows nothing), the new text has to be written in full
- a resource pack changes a tooltip the same way, with the key in the `lang` file of the mod that owns it:
  `<pack>/<mod>/lang/en_us.lang`. The file needs only the keys the pack changes, every other key of the
  mod stays; added lines (`<tooltip key>.<mod id>`) work in packs too
- other languages work with their own file (`ru_ru.lang`), a missing key falls back to English

## Look
The box is the class `tooltip`: `gui/tooltip.png` with its `.gui` (base's is a 9-slice), and
`color.tooltip` in the language files is the default text color, `color.tooltip_dim` the gray of
the lines under the name. A resource pack replaces the picture or the colors like anything else.

## Limits
- own text is the same for every stack of an item; conditions (`{if:}`, `{on_key:}`) and numbers (`{value..}`, the stack's value first) vary it
- no tooltips changed by code
- text and inline pictures only, the box does not scroll: keep it to a few lines and put the rest behind sneak
- conditions are calculated during the creation of tooltip so it can't be dynamic
