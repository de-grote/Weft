//! Stairs, slabs, vertical slabs, beams and eighths of any block, by tag or name: `r.stairs("#has_shapes")`. Each
//! block found gets its own, with its textures and what it is like to mine, also from mods this
//! one has never heard of. `.tab(..)` and the rest go on what they return, as on any block. And a
//! torch's shape for any torch (`torch_state`).

use crate::abi::BlockPos;
use crate::component::Tag;
use crate::content::{Block, BlockBuilder, Place, Props, Registry, State, WithComponents};
use crate::world::state_prop;
use crate::{Dir, Facings, Orientation, Toward};

/// How neighboring stairs find each other: every stair has it.
static STAIRS: Tag = Tag::named("stairs");

fn pos_step(pos: BlockPos, side: Dir) -> BlockPos {
    let o = side.offset();
    [pos.x + o.x, pos.y + o.y, pos.z + o.z].into()
}

/// Whether what is placed goes in the top half.
fn upper_half(p: &Place) -> bool {
    p.hit().y > 0.5
}

/// A stair's shape from the stairs around it, as vanilla has it.
fn stair_shape(w: &crate::World, pos: BlockPos) -> &'static str {
    let (Some(facing), Some(half)) = (w.prop(pos, "facing").and_then(Dir::from_name), w.prop(pos, "half")) else { return "straight" };
    let stair_at = |at: BlockPos| {
        let s = w.block(at);
        (STAIRS.on_block(s) && state_prop(s, "half") == Some(half))
            .then(|| state_prop(s, "facing").and_then(Dir::from_name))
            .flatten()
    };
    let across = |side: Dir| side != facing && side != facing.opposite();
    // Nothing the same way on that side to keep in line with.
    let free = |side: Dir| stair_at(pos_step(pos, side)) != Some(facing);
    if let Some(back) = stair_at(pos_step(pos, facing)).filter(|&b| across(b) && free(b.opposite())) {
        return if back == facing.counter_clockwise() { "outer_left" } else { "outer_right" };
    }
    if let Some(front) = stair_at(pos_step(pos, facing.opposite())).filter(|&f| across(f) && free(f)) {
        return if front == facing.counter_clockwise() { "inner_left" } else { "inner_right" };
    }
    "straight"
}

impl Registry {
    /// Blocks made of others: `sources` are names and tags (`#has_shapes`, `#planks/*`, `base:stone`),
    /// and each full cube among them, of any mod, gets a block `<its name><suffix>` from this one.
    /// It shows the source's textures on what `.states(..)` leaves to it, and is mined and
    /// sounds like it. The lang key `shape.<name>` names them, `{0}` being the source. Each is
    /// tagged `<name>/<source>` and its source `<name>_of/<source>`, for one recipe to cover all:
    /// `r.shaped(C, "#wall/{m}", 6, &["###", "###"], &[('#', "#wall_of/{m}")])`.
    pub fn shapes_of<'a>(&'a mut self, name: &str, sources: &str, suffix: &str) -> BlockBuilder<'a> {
        self.block(Block::generated(name.to_string()))
            .shapes_of(sources, suffix)
    }

    /// Stairs of `sources` (see `shapes_of`): `<name>_stairs`, turned the way the player looks,
    /// joining into corners with those around them.
    pub fn stairs<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a> {
        self.shapes_of("stairs", sources, "_stairs")
            .tag("stairs")
            .signal_blocker()
            .shares_cell_with("#water")
            .facing(Facings::HORIZONTAL, Toward::Away)
            .prop("half", ["bottom", "top"])
            .prop("shape", ["straight", "inner_left", "inner_right", "outer_left", "outer_right"])
            .states(|p: &Props, mut s| {
                // Written going up to the north; the top half is the same turned over about that
                // way, which swaps its left and right.
                let (shape, top) = (p.get("shape"), p.is("half", "top"));
                if top {
                    s = s.orientation(Orientation::new(Dir::Up, p.facing()));
                }
                let x = if shape.ends_with("left") != top { [0.0, 8.0] } else { [8.0, 16.0] };
                let s = s.model_box([0.0; 3], [16.0, 8.0, 16.0]);
                match shape {
                    "outer_left" | "outer_right" => s.model_box([x[0], 8.0, 0.0], [x[1], 16.0, 8.0]),
                    "inner_left" | "inner_right" => s
                        .model_box([0.0, 8.0, 0.0], [16.0, 16.0, 8.0])
                        .model_box([x[0], 8.0, 8.0], [x[1], 16.0, 16.0]),
                    _ => s.model_box([0.0, 8.0, 0.0], [16.0, 16.0, 8.0]),
                }
            })
            .placement(|p, props| props.set("half", if upper_half(p) { "top" } else { "bottom" }))
            .derived("shape", |n| stair_shape(&n.world(), n.pos()))
    }

    /// Slabs of `sources` (see `shapes_of`): `<name>_slab`, in the half of the block clicked. The
    /// other half takes one of any block, the same one making it a whole block of it.
    pub fn slabs<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a> {
        self.shapes_of("slab", sources, "_slab")
            .tag("slabs")
            .shares_cell_with("#water")
            .prop("type", ["bottom", "top", "double"])
            .states(|p: &Props, s| match p.get("type") {
                "bottom" => s
                    .model_box([0.0, 0.0, 0.0], [16.0, 8.0, 16.0])
                    .signal_blocker(),
                "top" => s
                    .model_box([0.0, 8.0, 0.0], [16.0, 16.0, 16.0])
                    .signal_blocker(),
                _ => s
                    .loot_rolls(2)
                    .piece([0.0; 3], [16.0, 8.0, 16.0], "type=top", "type=bottom")
                    .piece([0.0, 8.0, 0.0], [16.0; 3], "type=bottom", "type=top"),
            })
            .placement(|p, props| props.set("type", if upper_half(p) { "top" } else { "bottom" }))
            .merge(|p, props| {
                if p.placed("type") != Some(props.get("type")) {
                    props.set("type", "double");
                }
            })
    }

    /// Vertical slabs of `sources` (see `shapes_of`): `<name>_vertical_slab`, against what was
    /// clicked. On a floor or a ceiling, on the side of the block nearest to where it was
    /// clicked. The other side takes one of any block, the same one making it a whole block of it.
    pub fn vertical_slabs<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a> {
        self.shapes_of("vertical_slab", sources, "_vertical_slab")
            .tag("vertical_slabs")
            .shares_cell_with("#water")
            .facing(Facings::HORIZONTAL, Toward::Hit)
            .prop("double", ["false", "true"])
            .states(|p: &Props, s| match p.is("double", "true") {
                true => {
                    let other = format!("facing={},double=false", p.facing().opposite().name());
                    s.loot_rolls(2)
                        .piece([0.0, 0.0, 8.0], [16.0; 3], &other, "double=false")
                        .piece([0.0; 3], [16.0, 16.0, 8.0], "double=false", &other)
                }
                // Written against the south side of its cell, facing away from it.
                false => s
                    .model_box([0.0, 0.0, 8.0], [16.0, 16.0, 16.0])
                    .signal_blocker(),
            })
            .merge(|p, props| {
                if p.placed("facing").and_then(Dir::from_name) == Some(props.facing().opposite()) {
                    props.set("double", "true");
                }
            })
    }
}

impl Registry {
    /// Beams of `sources` (see `shapes_of`), a quarter of a block (1 x 1/2 x 1/2): `<name>_beam`,
    /// across where the player looks, in the half of the cell clicked, high or low. Several share a
    /// cell, as eighths do.
    pub fn beams<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a> {
        self.shapes_of("beam", sources, "_beam")
            .tag("beams")
            .shares_cell_with("#water")
            .facing(Facings::HORIZONTAL, Toward::Away)
            .prop("half", ["bottom", "top"])
            // Written along the north side of its cell.
            .states(|p: &Props, s| match p.get("half") {
                "top" => s.model_box([0.0, 8.0, 0.0], [16.0, 16.0, 8.0]),
                _ => s.model_box([0.0; 3], [16.0, 8.0, 8.0]),
            })
            .placement(|p, props| {
                props.set("half", if upper_half(p) { "top" } else { "bottom" });
                // On the side of the cell clicked, along the look.
                let d = props.facing();
                let (o, hit) = (d.offset(), p.hit());
                let along = if o.x != 0 { (hit.x - 0.5) * o.x as f64 } else { (hit.z - 0.5) * o.z as f64 };
                if along < 0.0 {
                    props.set("facing", d.opposite().name());
                }
            })
    }

    /// Eighths of `sources` (see `shapes_of`), half a block each way: `<name>_eighth`, in the corner
    /// of the cell clicked. Eight fill a cell, each broken alone.
    pub fn eighths<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a> {
        self.shapes_of("eighth", sources, "_eighth")
            .tag("eighths")
            .shares_cell_with("#water")
            .facing(Facings::HORIZONTAL, Toward::Away)
            .prop("half", ["bottom", "top"])
            // Written in the north-west corner; each facing turns it to the next corner.
            .states(|p: &Props, s| match p.get("half") {
                "top" => s.model_box([0.0, 8.0, 0.0], [8.0, 16.0, 8.0]),
                _ => s.model_box([0.0; 3], [8.0; 3]),
            })
            .placement(|p, props| {
                let hit = p.hit();
                props.set("half", if upper_half(p) { "top" } else { "bottom" });
                let corner = match (hit.x >= 0.5, hit.z >= 0.5) {
                    (false, false) => Dir::North,
                    (true, false) => Dir::East,
                    (true, true) => Dir::South,
                    (false, true) => Dir::West,
                };
                props.set("facing", corner.name());
            })
    }
}

/// Torches stand on the floor or hang on a wall, facing away from it.
pub const TORCH_FACINGS: Facings = Facings::only(&[Dir::Up, Dir::North, Dir::East, Dir::South, Dir::West]);

/// A stick's picture on a post, as a torch's: the middle of it from the side, its tip on top.
pub fn stick_faces(t: &str) -> [(&str, [f32; 4]); 6] {
    let side = [7.0, 6.0, 9.0, 16.0];
    [(t, side), (t, side), (t, [7.0, 13.0, 9.0, 15.0]), (t, [7.0, 6.0, 9.0, 8.0]), (t, side), (t, side)]
}

/// A torch facing `d`: on the floor (`Up`), hanging (`Down`), or leaning out from the wall behind
/// it, by vanilla's numbers.
pub fn torch_state<'a>(d: Dir, s: State<'a>, texture: &str) -> State<'a> {
    let faces = stick_faces(texture);
    let s = s.texture(texture);
    if d.horizontal() {
        // Written against a wall to its west.
        s.orientation(crate::Orientation::leaning(d.opposite()))
            .needs(Dir::West, "")
            .model_box_faces([-1.0, 3.5, 7.0], [1.0, 13.5, 9.0], faces)
            .tilted([0.0, 3.5, 8.0], Dir::South, -22.5)
            .hitbox([0.0, 3.0, 5.5], [5.0, 13.0, 10.5])
    } else {
        s.orientation(crate::Orientation::new(d.opposite(), Dir::North))
            .needs(Dir::Down, "")
            .model_box_faces([7.0, 0.0, 7.0], [9.0, 10.0, 9.0], faces)
            .hitbox([6.0, 0.0, 6.0], [10.0, 10.0, 10.0])
    }
}

impl BlockBuilder<'_> {
    /// A layer one pixel thick against sides of its cell (vines, ladders, glow lichen, a wall
    /// sign's plate): a property per side in `allowed` (`north` .. `down`, `"true"` where it lies).
    /// It goes against the face clicked, or the allowed side nearest the look when that one
    /// isn't; its own item put into its cell adds a side, up to every allowed one; a side with
    /// nothing solid behind it falls off, and breaking takes one side at a time. Redstone wire
    /// is the same with a drawing of its own. `Facings::HORIZONTAL` for ladders (walls only),
    /// `Facings::ALL` for vines everywhere.
    pub fn on_sides(self, allowed: Facings) -> Self {
        self.sides(allowed, true)
    }

    /// `on_sides`, with a plate of the block's textures on each side or not (wire draws itself).
    pub(crate) fn sides(mut self, allowed: Facings, plates: bool) -> Self {
        self.sides = Some(allowed);
        let sides: Vec<Dir> = allowed.dirs().collect();
        // With plates, the first state, the item's, lies on the first side.
        let mut b = sides.iter().enumerate().fold(self, |b, (i, d)| {
            let values = if plates && i == 0 { ["true", "false"] } else { ["false", "true"] };
            b.prop(d.name(), values)
        });
        // Each side is a piece of its own, and wire is picked side by side as it is drawn.
        for &d in &sides {
            let (min, max) = plate(d);
            let alone: Vec<String> = sides
                .iter()
                .filter(|&&o| o != d)
                .map(|o| format!("{}=false", o.name()))
                .collect();
            let left = format!("{}=false", d.name());
            b = b.state_where(&format!("{}=true", d.name()), move |s| {
                let s = s.piece(min, max, &left, &alone.join(","));
                if plates { s.model_box(min, max) } else { s.hitbox(min, max) }
            });
        }
        if plates {
            let none = sides
                .iter()
                .map(|d| format!("{}=false", d.name()))
                .collect::<Vec<_>>()
                .join(",");
            b = b.state_where(&none, |s| s.air());
        }
        b.tag(crate::abi::comp::block::ON_SIDES)
    }
}

/// The plate against side `d`, in 1/16 of a block.
fn plate(d: Dir) -> ([f32; 3], [f32; 3]) {
    let (mut min, mut max) = ([0.0; 3], [16.0; 3]);
    let axis = d.face() as usize / 2;
    if d.face() % 2 == 0 {
        max[axis] = 1.0;
    } else {
        min[axis] = 15.0;
    }
    (min, max)
}
