//! Generation invariants: phase inputs, local edits and cross-cube reads.
use weft_sdk::*;
pub static DIM: Dimension = Dimension::named("read_world");
static STONE: Block = Block::named("base:stone");
static GOLD: Block = Block::named("base:gold_block");
static COBBLE: Block = Block::named("base:cobblestone");
static BAD: Block = Block::named("base:red_lamp");
static RAW: Feature = Feature::named("read_raw");
static PLAN: Feature = Feature::named("read_plan");
static CLEAR: Feature = Feature::named("read_clear");
static POST: Feature = Feature::named("read_post");
static PIECE: Template = Template::named("read_piece");

pub fn register(r: &mut Registry) {
    r.terrain(&DIM).stone("base:stone").base_height(7.0);
    r.dimension(&DIM)
        .bare()
        .height_cubes(-1, 1)
        .sky_above(1)
        .no_block_updates()
        .generator(|g| {
            let o = g.origin();
            if o.y != 0 {
                return;
            }
            let raw = g.read([o.x - 1, 7, o.z]) == STONE.id();
            for z in 0..16 {
                for x in 0..16 {
                    g.set(x, 7, z, GOLD.id());
                    g.set(x, 6, z, if raw { GOLD.id() } else { BAD.id() });
                }
            }
        });
    r.template(&PIECE)
        .layer(&["gg"])
        .key('g', "base:gold_block");
    r.feature(&RAW)
        .dimension(&DIM)
        .stage(abi::stage::UNDERGROUND)
        .generate(|g| {
            let o = g.origin();
            if o.y != 0 {
                return;
            }
            let good = g.read([o.x - 1, 7, o.z]) == GOLD.id();
            for z in 0..16 {
                for x in 0..16 {
                    g.set(x, 8, z, if good { COBBLE.id() } else { BAD.id() });
                }
            }
        });
    r.scatter(&PLAN)
        .dimension(&DIM)
        .stage(abi::stage::POST_PROCESS)
        .spacing(8)
        .reach(4)
        .plan(|p| {
            let at = p.pos();
            let mut good = p.read(at) == COBBLE.id();
            let above = [at.x, at.y + 1, at.z];
            p.set(above, GOLD.id());
            good &= p.read(above) == GOLD.id();
            p.put(above, BAD.id());
            good &= p.read(above) == GOLD.id();
            p.fill([at.x, at.y + 2, at.z], [at.x + 1, at.y + 2, at.z], STONE.id());
            p.replace([at.x, at.y + 2, at.z], [at.x + 1, at.y + 2, at.z], "base:stone", GOLD.id());
            good &= p
                .view()
                .all([at.x, at.y + 2, at.z], [at.x + 1, at.y + 2, at.z], |s| s == GOLD.id());
            p.template(&PIECE, [at.x + 2, at.y + 1, at.z], Transform::turned(1))
                .put();
            good &= p.read([at.x + 2, at.y + 1, at.z]) == GOLD.id();
            p.set([at.x, 32, at.z], GOLD.id());
            good &= p.read([at.x, 32, at.z]) == 0;
            p.set(at, if good { GOLD.id() } else { BAD.id() });
        });
    r.feature(&POST)
        .dimension(&DIM)
        .stage(abi::stage::POST_PROCESS)
        .generate(|g| {
            let o = g.origin();
            if o.y != 0 {
                return;
            }
            // A peer's plan writes in this phase must be absent, even inside this cube.
            let good = g
                .view()
                .all([o.x - 1, 8, o.z], [o.x + 16, 8, o.z], |s| s == COBBLE.id());
            g.set(0, 12, 0, if good { GOLD.id() } else { BAD.id() });
            g.set(1, 12, 0, GOLD.id());
        });
    r.feature(&CLEAR)
        .dimension(&DIM)
        .stage(abi::stage::POST_PROCESS)
        .after(&POST)
        .generate(|g| {
            if g.origin().y == 0 {
                g.set(1, 12, 0, 0);
            }
        });
}
