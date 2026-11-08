//! Terrain described as data (`entry_kind::TERRAIN`): a base height plus height features, biome
//! rules that pick each column's biome and surface, water and caves. The dimension's mod
//! describes it, other mods edit it, and the merged result is evaluated here, so structures,
//! decorations and spawn search all read the same columns from one cache.
//!
//! Heights, masks and the fields rules look at are expressions (`abi::ExprStep`), run here and
//! by `shaders/terrain.wgsl` with the same integer math.

use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use weft_abi::{self as abi, BiomeRule, ExprStep, TerrainColumn, expr as ek, rule_field as rf};
use weft_noise::fixed::{ONE, div_floor, fbm, hash, perlin2, perlin3, q4, q12, ramp, salt, seed32, site};

use crate::entries::Entry;
use crate::gpu_layout::terrain::limit;

fn step(kind: u32, a: i32) -> ExprStep {
    ExprStep { kind, a, ..Default::default() }
}

fn noise(salt: u64, size: i32, octaves: i32) -> ExprStep {
    ExprStep { salt, b: octaves, ..step(ek::NOISE, size) }
}

/// The fields `edit.fields` names replace those of `rule`.
pub(crate) fn merge(rule: &mut BiomeRule, edit: &BiomeRule) {
    let has = |f: u32| edit.fields & f != 0;
    if has(rf::BIOME) {
        rule.biome.clone_from(&edit.biome);
    }
    if has(rf::RANGES) {
        for r in &edit.ranges {
            match rule.ranges.iter_mut().find(|x| x.name == r.name) {
                Some(x) => *x = r.clone(),
                None => rule.ranges.push(r.clone()),
            }
        }
    }
    if has(rf::HEIGHT) {
        rule.height = edit.height;
    }
    if has(rf::SURFACE) {
        (rule.top, rule.filler, rule.depth) = (edit.top.clone(), edit.filler.clone(), edit.depth);
    }
    if has(rf::UNDER) {
        (rule.under, rule.under_layers) = (edit.under.clone(), edit.under_layers);
    }
    if has(rf::DEEP_TOP) {
        (rule.deep_top, rule.deep_below) = (edit.deep_top.clone(), edit.deep_below);
    }
    if has(rf::BARE_ABOVE) {
        rule.bare_above = edit.bare_above;
    }
    if has(rf::FROZEN) {
        rule.ice.clone_from(&edit.ice);
    }
}

/// The most steps an expression may have.
const MAX_STEPS: usize = 256;

#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub kind: u32,
    pub salt: u64,
    /// `ek::FIELD`: the field's index.
    pub a: i32,
    pub b: i32,
    pub c: i32,
}

/// An expression checked and with its field names turned into indices; empty means 0.
#[derive(Clone, Debug, Default)]
pub struct Expr(pub Vec<Step>);

/// The largest factor of a product, so it fits 32 bits.
const MUL_LIMIT: i32 = 46340;

impl Expr {
    /// `raw` checked; `field` finds the fields it may use by name.
    fn resolve(raw: &[ExprStep], field: &dyn Fn(&str) -> Option<usize>) -> Result<Expr, String> {
        if raw.len() > MAX_STEPS {
            return Err(format!("{} steps, at most {MAX_STEPS}", raw.len()));
        }
        let (mut depth, mut deepest) = (0usize, 0usize);
        let mut out = Vec::with_capacity(raw.len());
        for (i, s) in raw.iter().enumerate() {
            let pops = match s.kind {
                ek::CONST | ek::X | ek::Z | ek::FIELD => 0,
                ek::ABS | ek::RAMP => 1,
                ek::ADD | ek::SUB | ek::MUL | ek::DIV | ek::REM | ek::MIN | ek::MAX | ek::NOISE | ek::SITE => 2,
                kind => return Err(format!("step {i} is of unknown kind {kind}")),
            };
            depth = depth
                .checked_sub(pops)
                .ok_or_else(|| format!("step {i} has too few values to work on"))?
                + 1;
            deepest = deepest.max(depth);
            let a = match s.kind {
                ek::FIELD => field(&s.name).ok_or_else(|| format!("no field named {}", s.name))? as i32,
                _ => s.a,
            };
            out.push(Step { kind: s.kind, salt: s.salt, a, b: s.b, c: s.c });
        }
        match depth {
            1 if deepest <= limit::DEPTH as usize => Ok(Expr(out)),
            1 => Err(format!("needs room for {deepest} values at once, at most {}", limit::DEPTH)),
            0 => Err("has no steps".into()),
            n => Err(format!("leaves {n} values instead of one")),
        }
    }

    /// Its value at (`x`, `z`) (Q4) given the values of the fields.
    pub fn eval(&self, fields: &[i32], seed: u64, x: i32, z: i32) -> i32 {
        let mut st = [0i32; limit::DEPTH as usize];
        let mut sp = 0usize;
        for s in &self.0 {
            let v = match s.kind {
                ek::CONST => s.a,
                ek::X => x,
                ek::Z => z,
                ek::FIELD => fields[s.a as usize],
                ek::ABS => {
                    sp -= 1;
                    st[sp].wrapping_abs()
                }
                ek::RAMP => {
                    sp -= 1;
                    ramp(st[sp], s.a, s.b)
                }
                ek::NOISE => {
                    sp -= 2;
                    fbm(seed32(seed, s.salt), st[sp], st[sp + 1], s.a, s.b as u32)
                }
                ek::SITE => {
                    sp -= 2;
                    site(seed32(seed, s.salt), st[sp], st[sp + 1], s.a, s.b, s.c)
                }
                kind => {
                    sp -= 2;
                    let (l, r) = (st[sp], st[sp + 1]);
                    match kind {
                        ek::ADD => l.wrapping_add(r),
                        ek::SUB => l.wrapping_sub(r),
                        ek::MUL => (l.clamp(-MUL_LIMIT, MUL_LIMIT) * r.clamp(-MUL_LIMIT, MUL_LIMIT)) >> 12,
                        ek::DIV if r == 0 => 0,
                        ek::DIV => l.wrapping_div(r),
                        ek::REM => div_floor(l, r.max(1)).1,
                        ek::MIN => l.min(r),
                        _ => l.max(r),
                    }
                }
            };
            st[sp] = v;
            sp += 1;
        }
        if sp == 0 { 0 } else { st[0] }
    }

    /// Whether it uses the field with index `field`.
    fn uses(&self, field: usize) -> bool {
        self.0
            .iter()
            .any(|s| s.kind == ek::FIELD && s.a as usize == field)
    }
}

/// A named value over the world's columns that rules can ask for and expressions can use.
pub struct Field {
    pub name: String,
    pub expr: Expr,
}

/// The height a feature adds is held to this (Q4, 16384 blocks) before its strength scales it.
const HEIGHT_LIMIT: i32 = 1 << 18;

pub struct Feature {
    pub name: String,
    /// Q4.
    pub height: Expr,
    /// 0 to `ONE`; nothing is added where it isn't above 0. `None`: `ONE` everywhere.
    pub mask: Option<Expr>,
}

/// A biome rule with names resolved: block states and biome ids.
pub struct Rule {
    pub biome: u32,
    /// Index into `Terrain::fields`, then the range the field must be in (Q12).
    pub ranges: Vec<(usize, i32, i32)>,
    pub height: (i32, i32),
    pub top: Option<u32>,
    pub filler: Option<u32>,
    pub depth: i32,
    pub under: Option<(u32, i32)>,
    pub deep_top: Option<(u32, i32)>,
    pub bare_above: Option<i32>,
    pub ice: Option<u32>,
}

pub struct Caves {
    pub lava: u32,
    pub lava_from: i32,
    pub lava_to: i32,
}

pub struct CaveBiome {
    pub biome: u32,
    pub depth: i32,
    /// Q4, Q12.
    pub size: i32,
    pub threshold: i32,
}

/// The columns of one column of cubes.
pub struct Columns {
    pub cols: [TerrainColumn; 256],
    pub rules: [u32; 256],
}

pub struct Terrain {
    pub stone: u32,
    /// Q4.
    pub base: i32,
    pub sea: Option<(i32, u32)>,
    /// Computed in this order, each only from those before it. Those nothing uses are empty.
    pub fields: Vec<Field>,
    pub features: Vec<Feature>,
    pub rules: Vec<Rule>,
    pub caves: Option<Caves>,
    /// Biomes of cubes deep underground.
    pub cave_biomes: Vec<CaveBiome>,
    cache: ColumnCache<Columns>,
    /// Column heights every 4 blocks over a cube's columns and its +X/+Z edge (5 x 5): caves
    /// measure depth from this smoothed surface, so steep slopes do not leave single-column
    /// pillars.
    corners: ColumnCache<[i32; 25]>,
}

/// The fields every terrain has, climate and continentalness (about -1.4..1.4, Q12), which
/// a terrain may declare again.
fn standard_fields() -> Vec<abi::TerrainField> {
    let of = |salt, size: i32, octaves, scale| {
        vec![step(ek::X, 0), step(ek::Z, 0), noise(salt, size * 16, octaves), step(ek::CONST, scale), step(ek::MUL, 0)]
    };
    let mut continent = of(salt::CONTINENT, 900, 4, 6554);
    continent.extend([step(ek::CONST, 614), step(ek::ADD, 0)]);
    let field = |name: &str, value| abi::TerrainField { name: name.into(), value };
    vec![
        field("temperature", of(salt::TEMPERATURE, 1100, 3, 5734)),
        field("humidity", of(salt::HUMIDITY, 800, 3, 5734)),
        field("continent", continent),
    ]
}

/// Looks up content by full name while terrains are built.
pub struct Names<'a> {
    /// Name to first block state.
    pub block: &'a dyn Fn(&str) -> Option<u32>,
    pub biome: &'a dyn Fn(&str) -> Option<u32>,
    /// A biome's own climate and surface, which its rules start from.
    pub biome_rule: &'a dyn Fn(&str) -> Option<&'a BiomeRule>,
}

/// The terrain a folded `entry_kind::TERRAIN` entry describes; missing names are `errors`.
pub fn build(dimension: &str, e: &Entry, names: &Names, errors: &mut Vec<String>) -> Terrain {
    use abi::comp::terrain as t;
    let mut field_defs = standard_fields();
    for f in e.or::<Vec<abi::TerrainField>>(t::FIELDS, Vec::new()) {
        match field_defs.iter_mut().find(|x| x.name == f.name) {
            Some(x) => {
                log::info!("the terrain of {dimension} gets another {} field", f.name);
                *x = f;
            }
            None => field_defs.push(f),
        }
    }
    let mut feature_defs: Vec<abi::TerrainFeature> = Vec::new();
    for f in e.or::<Vec<abi::TerrainFeature>>(t::FEATURES, Vec::new()) {
        if !f.name.is_empty() && feature_defs.iter().any(|x| x.name == f.name) {
            errors.push(format!("the terrain of {dimension} has two features named {}", f.name));
        } else {
            feature_defs.push(f);
        }
    }
    for m in e.or::<Vec<abi::FeatureMask>>(t::MASKS, Vec::new()) {
        match feature_defs.iter_mut().find(|f| f.name == m.name) {
            Some(f) if f.mask.is_empty() => f.mask = m.mask,
            Some(f) => {
                f.mask.extend(m.mask);
                f.mask.push(step(ek::MUL, 0));
            }
            None => errors.push(format!("the terrain of {dimension} has no feature {} to mask", m.name)),
        }
    }
    let mut rule_defs: Vec<BiomeRule> = Vec::new();
    for r in e.or::<Vec<BiomeRule>>(t::RULES, Vec::new()) {
        if rule_defs.iter().any(|x| x.name == r.name) {
            errors.push(format!("the terrain of {dimension} has two biome rules named {} (name the new one, or replace the old)", r.name));
        } else {
            rule_defs.push(r);
        }
    }
    for edit in e.or::<Vec<BiomeRule>>(t::RULE_EDITS, Vec::new()) {
        match rule_defs.iter_mut().find(|r| r.name == edit.name) {
            Some(r) => merge(r, &edit),
            None => errors.push(format!("the terrain of {dimension} has no biome rule {} to edit", edit.name)),
        }
    }

    let block = |name: &str, what: &str, errors: &mut Vec<String>| -> Option<u32> {
        if name.is_empty() {
            return None;
        }
        let b = (names.block)(name);
        if b.is_none() {
            errors.push(format!("terrain of {dimension}: unknown block {name} ({what})"));
        }
        b
    };
    let stone = match e.text(t::STONE).filter(|s| !s.is_empty()) {
        Some(s) => block(s, "stone", errors).unwrap_or(0),
        None => {
            errors.push(format!("terrain of {dimension}: no stone block (`.stone(..)`)"));
            0
        }
    };
    let sea = e
        .get::<abi::TerrainSea>(t::SEA)
        .and_then(|s| Some((s.level, block(&s.block, "sea", errors)?)));
    let caves = e
        .get::<abi::TerrainCaves>(t::CAVES)
        .and_then(|c| Some(Caves { lava: block(&c.lava, "lava", errors)?, lava_from: c.from, lava_to: c.to }));
    if field_defs.len() > limit::FIELDS as usize {
        errors.push(format!("terrain of {dimension}: {} fields, at most {}", field_defs.len(), limit::FIELDS));
        field_defs.truncate(limit::FIELDS as usize);
    }
    let mut fields: Vec<Field> = Vec::with_capacity(field_defs.len());
    for (i, f) in field_defs.iter().enumerate() {
        let expr = Expr::resolve(&f.value, &|n| field_defs[..i].iter().position(|x| x.name == n)).unwrap_or_else(|e| {
            errors.push(format!("terrain of {dimension}: field {}: {e}", f.name));
            Expr::default()
        });
        fields.push(Field { name: f.name.clone(), expr });
    }
    let field_at = |n: &str| field_defs.iter().position(|f| f.name == n);
    let mut features: Vec<Feature> = Vec::with_capacity(feature_defs.len());
    for f in &feature_defs {
        let what = format!("terrain of {dimension}: feature {}", f.name);
        let height = Expr::resolve(&f.height, &field_at);
        let mask = (!f.mask.is_empty())
            .then(|| Expr::resolve(&f.mask, &field_at))
            .transpose();
        match (height, mask) {
            (Ok(height), Ok(mask)) => features.push(Feature { name: f.name.clone(), height, mask }),
            (h, m) => errors.extend(
                h.err()
                    .map(|e| format!("{what}: height {e}"))
                    .into_iter()
                    .chain(m.err().map(|e| format!("{what}: mask {e}"))),
            ),
        }
    }
    let mut rules = Vec::with_capacity(rule_defs.len());
    for own in &rule_defs {
        let mut merged = (names.biome_rule)(&own.biome).cloned();
        let r = match &mut merged {
            Some(m) => {
                merge(m, own);
                m.name.clone_from(&own.name);
                &*m
            }
            None => own,
        };
        let biome = (names.biome)(&r.biome).unwrap_or_else(|| {
            errors.push(format!("terrain of {dimension}: unknown biome {}", r.biome));
            0
        });
        let ranges = r
            .ranges
            .iter()
            .filter_map(|x| {
                let i = field_at(&x.name);
                if i.is_none() {
                    errors.push(format!("terrain of {dimension}: biome rule {} asks for field {}, which it doesn't have", r.name, x.name));
                }
                Some((i?, q12(x.min), q12(x.max)))
            })
            .collect();
        let what = format!("biome rule {}", r.name);
        rules.push(Rule {
            biome,
            ranges,
            height: (r.height[0], r.height[1]),
            top: block(&r.top, &what, errors),
            filler: block(&r.filler, &what, errors),
            depth: r.depth,
            under: block(&r.under, &what, errors).map(|b| (b, r.under_layers)),
            deep_top: block(&r.deep_top, &what, errors).map(|b| (b, r.deep_below)),
            bare_above: (r.bare_above != i32::MAX).then_some(r.bare_above),
            ice: block(&r.ice, &what, errors),
        });
    }
    let cave_biomes = e
        .or::<Vec<abi::CaveBiome>>(t::CAVE_BIOMES, Vec::new())
        .iter()
        .map(|c| CaveBiome {
            biome: (names.biome)(&c.name).unwrap_or_else(|| {
                errors.push(format!("terrain of {dimension}: unknown cave biome {}", c.name));
                0
            }),
            depth: c.depth,
            size: q4(c.size),
            threshold: q12(c.threshold),
        })
        .collect();
    // What no feature or rule looks at, directly or through another field, is not computed.
    let mut needed: Vec<bool> = (0..fields.len())
        .map(|i| {
            features
                .iter()
                .any(|f| f.height.uses(i) || f.mask.as_ref().is_some_and(|m| m.uses(i)))
                || rules.iter().any(|r| r.ranges.iter().any(|x| x.0 == i))
        })
        .collect();
    for i in (0..fields.len()).rev() {
        for j in 0..i {
            needed[j] |= needed[i] && fields[i].expr.uses(j);
        }
    }
    for (f, needed) in fields.iter_mut().zip(needed) {
        if !needed {
            f.expr = Expr::default();
        }
    }
    Terrain::new(stone, q4(e.or(t::BASE_HEIGHT, 64.0)), sea, fields, features, rules, caves, cave_biomes)
}

impl Terrain {
    fn new(
        stone: u32,
        base: i32,
        sea: Option<(i32, u32)>,
        fields: Vec<Field>,
        features: Vec<Feature>,
        rules: Vec<Rule>,
        caves: Option<Caves>,
        cave_biomes: Vec<CaveBiome>,
    ) -> Terrain {
        Terrain {
            stone,
            base,
            sea,
            fields,
            features,
            rules,
            caves,
            cave_biomes,
            cache: ColumnCache::new(32768),
            corners: ColumnCache::new(32768),
        }
    }

    pub fn sea_level(&self) -> Option<i32> {
        self.sea.map(|s| s.0)
    }

    /// Height, biome and rule of column (`x`, `z`).
    fn compute(&self, seed: u64, x: i32, z: i32) -> (TerrainColumn, u32) {
        let (fx, fz) = (x * 16, z * 16);
        let mut v = [0i32; limit::FIELDS as usize];
        for (i, f) in self.fields.iter().enumerate() {
            v[i] = f.expr.eval(&v, seed, fx, fz);
        }
        let mut h = self.base;
        for feat in &self.features {
            let w = feat
                .mask
                .as_ref()
                .map_or(ONE, |m| m.eval(&v, seed, fx, fz).min(ONE));
            if w > 0 {
                h = h.wrapping_add(
                    (w * feat
                        .height
                        .eval(&v, seed, fx, fz)
                        .clamp(-HEIGHT_LIMIT, HEIGHT_LIMIT))
                        >> 12,
                );
            }
        }
        let height = h >> 4;
        let rule = self
            .rules
            .iter()
            .position(|b| height >= b.height.0 && height <= b.height.1 && b.ranges.iter().all(|&(f, lo, hi)| v[f] >= lo && v[f] < hi))
            .unwrap_or(self.rules.len().saturating_sub(1));
        (TerrainColumn { height, biome: self.rules.get(rule).map_or(0, |r| r.biome) }, rule as u32)
    }

    /// Column (`x`, `z`) from the cache when its cube column is there, computed otherwise (a
    /// single lookup should not cost a whole cube column).
    pub fn column(&self, seed: u64, x: i32, z: i32) -> TerrainColumn {
        match self.cache.peek(seed, x >> 4, z >> 4) {
            Some(c) => c.cols[((z & 15) * 16 + (x & 15)) as usize],
            None => self.compute(seed, x, z).0,
        }
    }

    /// The columns of cube column (`cx`, `cz`).
    pub fn columns(&self, seed: u64, cx: i32, cz: i32) -> Arc<Columns> {
        self.cache.get(seed, cx, cz, || {
            let mut c = Columns { cols: [TerrainColumn::default(); 256], rules: [0; 256] };
            for i in 0..256 {
                (c.cols[i], c.rules[i]) = self.compute(seed, cx * 16 + (i % 16) as i32, cz * 16 + (i / 16) as i32);
            }
            c
        })
    }

    /// Whether caves took the block at the surface of column (`x`, `z`): decorations placed from
    /// outside the cube (trees) and spawn search check it so they do not float over pits.
    pub fn surface_carved(&self, seed: u64, x: i32, z: i32) -> bool {
        if self.caves.is_none() {
            return false;
        }
        let h = self.column(seed, x, z).height;
        if self.sea.is_some_and(|s| h <= s.0) {
            return false;
        }
        let (gx, gy, gz) = (x.div_euclid(4) * 4, h.div_euclid(4) * 4, z.div_euclid(4) * 4);
        let (t, c) = interpolate(
            |dy, dz, dx| cave_node(seed, gx + dx as i32 * 4, gy + dy as i32 * 4, gz + dz as i32 * 4),
            (x - gx) as usize,
            (h - gy) as usize,
            (z - gz) as usize,
        );
        let corners: [i32; 4] = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dz)| self.column(seed, gx + dx * 4, gz + dz * 4).height);
        let smooth = smooth4(corners, (x - gx) as i32, (z - gz) as i32);
        carves(t > 0, c, cavern_roof(seed, x, z), smooth - h * 16)
    }

    /// What rule `rule` puts at the top of a column `h` high, before caves.
    fn surface_of(&self, rule: u32, h: i32) -> u32 {
        let Some(r) = self.rules.get(rule as usize) else { return self.stone };
        if r.bare_above.is_some_and(|y| h > y) {
            return self.stone;
        }
        match r.deep_top {
            Some((b, below)) if h < below => b,
            _ => r.top.unwrap_or(self.stone),
        }
    }

    /// The block generation leaves at the top of column (`x`, `z`), 0 where caves took it: the
    /// ground of a decoration on the lowest layer of a cube is in the cube below.
    pub fn surface_block(&self, seed: u64, x: i32, z: i32) -> u32 {
        if self.surface_carved(seed, x, z) {
            return 0;
        }
        let cols = self.columns(seed, x.div_euclid(16), z.div_euclid(16));
        let i = (z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize;
        self.surface_of(cols.rules[i], cols.cols[i].height)
    }

    /// Smoothed surface height (Q4) at local column (x, z) from the cube's corner grid.
    fn smooth(corners: &[i32; 25], x: usize, z: usize) -> i32 {
        let (cx, cz) = (x / 4, z / 4);
        let c = |dx: usize, dz: usize| corners[(cz + dz) * 5 + cx + dx];
        smooth4([c(0, 0), c(1, 0), c(0, 1), c(1, 1)], (x % 4) as i32, (z % 4) as i32)
    }

    /// Fills cube `pos` with stone, surface layers, water and caves and sets its biomes; returns
    /// its columns.
    pub fn generate(&self, seed: u64, pos: [i32; 3], blocks: &mut [u32], biomes: &mut [u16; 256]) -> Arc<Columns> {
        let [cx, cy, cz] = pos;
        let (ox, oy, oz) = (cx * 16, cy * 16, cz * 16);
        let cols = self.columns(seed, cx, cz);
        for (i, c) in cols.cols.iter().enumerate() {
            let (x, z) = ((ox + (i % 16) as i32) * 16, (oz + (i / 16) as i32) * 16);
            let deep = self.cave_biomes.iter().enumerate().find(|&(k, b)| {
                oy + 15 < c.height - b.depth
                    && perlin3(seed32(seed, salt::CAVE_BIOME ^ k as u64), x, (oy + 8) * 16, z, b.size, b.size, b.size) > b.threshold
            });
            biomes[i] = deep.map_or(c.biome, |(_, b)| b.biome) as u16;
        }
        let max_h = cols.cols.iter().map(|c| c.height).max().unwrap_or(0);
        let sea = self.sea.map_or(i32::MIN, |s| s.0);
        if oy > max_h.max(sea) + 16 {
            return cols;
        }
        // Decided per block from its column alone, so neighboring cubes agree: tunnels may open
        // onto the surface, caverns close up under it except where `cavern_roof` lets them out.
        let cave = match &self.caves {
            Some(_) if oy <= max_h => Some(cave_fields(seed, ox, oy, oz)),
            _ => None,
        };
        let corners = self.corners.get(seed, cx, cz, || {
            std::array::from_fn(|i| {
                self.column(seed, ox + (i % 5) as i32 * 4, oz + (i / 5) as i32 * 4)
                    .height
            })
        });
        let stone = self.stone;
        let water = self.sea.map_or(0, |s| s.1);
        for z in 0..16usize {
            for x in 0..16usize {
                let i = z * 16 + x;
                let h = cols.cols[i].height;
                let rule = self.rules.get(cols.rules[i] as usize);
                let bare = rule.and_then(|r| r.bare_above).is_some_and(|y| h > y);
                let top = self.surface_of(cols.rules[i], h);
                let filler = rule.and_then(|r| r.filler).unwrap_or(stone);
                let depth = rule.map_or(0, |r| r.depth);
                let under = rule.and_then(|r| r.under);
                let ice = rule.and_then(|r| r.ice);
                let closed = if cave.is_some() { cavern_roof(seed, ox + x as i32, oz + z as i32) } else { 0 };
                let smooth = Self::smooth(&corners, x, z);
                // Keeps a seal under water so seas and canyons do not drain into caves.
                let carved = |ly: usize| {
                    let Some(cave) = &cave else { return false };
                    let wy = oy + ly as i32;
                    let (tunnel, cavern) = cave[(ly * 16 + z) * 16 + x];
                    wy <= h && !(h <= sea && wy > h - 4) && carves(tunnel, cavern, closed, smooth - wy * 16)
                };
                for y in 0..16usize {
                    let wy = oy + y as i32;
                    let mut s = if wy > h {
                        if wy <= sea { if wy == sea { ice.unwrap_or(water) } else { water } } else { 0 }
                    } else if bare {
                        stone
                    } else if wy == h {
                        top
                    } else if wy >= h - depth {
                        match under {
                            Some((b, layers)) if wy < h - depth + layers => b,
                            _ => filler,
                        }
                    } else {
                        stone
                    };
                    if let Some(c) = self.caves.as_ref().filter(|_| carved(y)) {
                        s = match lake(seed, wy).filter(|_| (c.lava_from..c.lava_to).contains(&wy)) {
                            Some(true) => c.lava,
                            Some(false) => stone,
                            None => 0,
                        };
                    }
                    blocks[(y * 16 + z) * 16 + x] = s;
                }
            }
        }
        cols
    }
}

/// Cave fields for the cube, sampled every 4 blocks on a grid aligned to the world and
/// trilinearly interpolated: whether a tunnel passes, and how far inside a cavern (> 0, Q12)
/// the block is.
fn cave_fields(seed: u64, ox: i32, oy: i32, oz: i32) -> Vec<(bool, i32)> {
    const G: usize = 5;
    let mut grid = [[[(0i32, 0i32); G]; G]; G];
    for (gy, row) in grid.iter_mut().enumerate() {
        for (gz, line) in row.iter_mut().enumerate() {
            for (gx, v) in line.iter_mut().enumerate() {
                *v = cave_node(seed, ox + gx as i32 * 4, oy + gy as i32 * 4, oz + gz as i32 * 4);
            }
        }
    }
    let mut out = vec![(false, 0i32); 4096];
    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                let (t, c) = interpolate(|dy, dz, dx| grid[y / 4 + dy][z / 4 + dz][x / 4 + dx], x % 4, y % 4, z % 4);
                out[(y * 16 + z) * 16 + x] = (t > 0, c);
            }
        }
    }
    out
}

/// Trilinear blend of grid nodes (`node(dy, dz, dx)`) at offset (x, y, z) inside a 4-block cell.
fn interpolate(node: impl Fn(usize, usize, usize) -> (i32, i32), x: usize, y: usize, z: usize) -> (i32, i32) {
    let (fx, fy, fz) = (x as i32 * 1024, y as i32 * 1024, z as i32 * 1024);
    let l = |a: i32, b: i32, t: i32| a + (((b - a) * t) >> 12);
    let field = |k: usize| {
        let c = |dy, dz, dx| {
            let v = node(dy, dz, dx);
            if k == 0 { v.0 } else { v.1 }
        };
        l(
            l(l(c(0, 0, 0), c(0, 0, 1), fx), l(c(0, 1, 0), c(0, 1, 1), fx), fz),
            l(l(c(1, 0, 0), c(1, 0, 1), fx), l(c(1, 1, 0), c(1, 1, 1), fx), fz),
            fy,
        )
    };
    (field(0), field(1))
}

/// Bilinear blend (Q4) of corner heights 4 blocks apart at offset (x, z) inside the cell.
fn smooth4(c: [i32; 4], x: i32, z: i32) -> i32 {
    (c[0] * (4 - x) + c[1] * x) * (4 - z) + (c[2] * (4 - x) + c[3] * x) * z
}

/// Whether a cave takes a block `depth` (Q4) under the smoothed surface. Near the surface a
/// cavern needs to be ever deeper inside its noise, so its ceiling domes over instead of breaking
/// through; where `cavern_roof` opens (`closed` < 1) it widens towards the surface into a pit.
fn carves(tunnel: bool, cavern: i32, closed: i32, depth: i32) -> bool {
    let d = depth.clamp(-(1 << 16), 1 << 16);
    let near = (ONE - d * ONE / (12 * 16)).max(0);
    let pit = (ONE - d * ONE / (24 * 16)).max(0);
    let need = ((((closed * 1024) >> 12) * near) >> 12) - (((((ONE - closed) * PIT) >> 12) * pit) >> 12);
    tunnel || cavern > need
}

/// What a cave holds at height `wy` instead of air: in about half of the 64-block bands of height
/// a lake crosses every cave at one level, 4 blocks of lava on 2 of stone, so lakes are flat,
/// span the whole cave and never hang over open space. `Some(true)` lava, `Some(false)` stone.
fn lake(seed: u64, wy: i32) -> Option<bool> {
    let band = div_floor(wy, 64).0;
    let h = hash(seed32(seed, salt::LAVA), 0, band, 0);
    if h & 1 == 0 {
        return None;
    }
    let level = band * 64 + 16 + ((h >> 8) % 32) as i32;
    match level - wy {
        1..=4 => Some(true),
        5..=6 => Some(false),
        _ => None,
    }
}

/// Cave fields at a grid node (every 4 blocks): tunnels, caverns (Q12).
fn cave_node(seed: u64, x: i32, y: i32, z: i32) -> (i32, i32) {
    let (x, y, z) = (x * 16, y * 16, z * 16);
    let a = perlin3(seed32(seed, salt::CAVE_A), x, y, z, 48 * 16, 32 * 16, 48 * 16);
    let b = perlin3(seed32(seed, salt::CAVE_B), x, y, z, 48 * 16, 32 * 16, 48 * 16);
    // Two thin noise zero-crossings intersect into winding tunnels.
    let spaghetti = 328 - (a.abs() + b.abs());
    let cheese = perlin3(seed32(seed, salt::CAVERN), x, y, z, 90 * 16, 50 * 16, 90 * 16) - 1720;
    (spaghetti, cheese)
}

/// 1 where caverns stay under a roof, falling to 0 in patches where they may open into wide pits
/// like vanilla's cave entrances (Q12).
const PIT: i32 = 492;

fn cavern_roof(seed: u64, x: i32, z: i32) -> i32 {
    ramp(perlin2(seed32(seed, salt::CAVERN_ROOF), x * 16, z * 16, 120 * 16), 1638, 1024)
}

const SHARDS: usize = 32;

/// Data computed once per column of cubes and shared by every cube of it. Generation runs on
/// many threads at once, so the map is split into locked shards; a shard that grows past its
/// share of the capacity drops a quarter of its entries (clearing it whole made a view of 64,
/// some 13k columns, compute most columns twice).
pub struct ColumnCache<T> {
    shards: [Mutex<FxHashMap<(u64, i32, i32), Arc<std::sync::OnceLock<Arc<T>>>>>; SHARDS],
    per_shard: usize,
}

impl<T> ColumnCache<T> {
    pub fn new(capacity: usize) -> ColumnCache<T> {
        ColumnCache { shards: std::array::from_fn(|_| Mutex::new(FxHashMap::default())), per_shard: capacity.div_ceil(SHARDS).max(1) }
    }

    fn shard(&self, x: i32, z: i32) -> &Mutex<FxHashMap<(u64, i32, i32), Arc<std::sync::OnceLock<Arc<T>>>>> {
        let h = weft_noise::hash(0, x as i64, 0, z as i64) as u32;
        &self.shards[(h >> 27) as usize % SHARDS]
    }

    fn peek(&self, seed: u64, x: i32, z: i32) -> Option<Arc<T>> {
        self.shard(x, z)
            .lock()
            .get(&(seed, x, z))
            .and_then(|c| c.get().cloned())
    }

    /// The value for cube column (`x`, `z`) under `seed`, computing it with `make` on a miss.
    /// Threads asking while it is computed wait for it: the cubes of a column come together,
    /// and each computed it again.
    pub fn get(&self, seed: u64, x: i32, z: i32, make: impl FnOnce() -> T) -> Arc<T> {
        let cell = {
            let mut map = self.shard(x, z).lock();
            match map.get(&(seed, x, z)) {
                Some(cell) => cell.clone(),
                None => {
                    if map.len() >= self.per_shard {
                        let mut n = 0usize;
                        map.retain(|_, _| {
                            n += 1;
                            n % 4 != 0
                        });
                    }
                    map.entry((seed, x, z)).or_default().clone()
                }
            }
        };
        cell.get_or_init(|| Arc::new(make())).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::entries::RawEntry;
    use abi::{comp::terrain as t, value_op as vo};

    fn expr(steps: Vec<ExprStep>) -> Expr {
        Expr::resolve(&steps, &|_| None).unwrap()
    }

    /// `noise * height`, Q4 sizes and heights from blocks.
    fn hills(height: i32, size: i32) -> Feature {
        let height =
            expr(vec![step(ek::X, 0), step(ek::Z, 0), noise(salt::HILLS, size * 16, 4), step(ek::CONST, height * 16), step(ek::MUL, 0)]);
        Feature { name: String::new(), height, mask: None }
    }

    /// Ridge lines `width` blocks apart, squared, with rough detail on top.
    fn mountains(height: i32, width: i32) -> Feature {
        let ridge = |out: &mut Vec<ExprStep>| {
            out.extend([
                step(ek::CONST, ONE),
                step(ek::X, 0),
                step(ek::Z, 0),
                noise(salt::MOUNTAINS, width * 16, 4),
                step(ek::CONST, 2 * ONE),
                step(ek::MUL, 0),
                step(ek::ABS, 0),
                step(ek::CONST, ONE),
                step(ek::MIN, 0),
                step(ek::SUB, 0),
            ]);
        };
        let mut steps = Vec::new();
        ridge(&mut steps);
        ridge(&mut steps);
        steps.extend([step(ek::MUL, 0), step(ek::CONST, height * 16), step(ek::MUL, 0)]);
        steps.extend([
            step(ek::X, 0),
            step(ek::Z, 0),
            noise(salt::HILLS, 60 * 16, 4),
            step(ek::CONST, height * 16),
            step(ek::MUL, 0),
            step(ek::CONST, 7),
            step(ek::DIV, 0),
            step(ek::ADD, 0),
        ]);
        Feature { name: String::new(), height: expr(steps), mask: None }
    }

    fn rule(biome: u32) -> Rule {
        Rule {
            biome,
            ranges: Vec::new(),
            height: (i32::MIN, i32::MAX),
            top: None,
            filler: None,
            depth: 3,
            under: None,
            deep_top: None,
            bare_above: None,
            ice: None,
        }
    }

    fn terrain(features: Vec<Feature>, rules: Vec<Rule>, lava: Option<u32>, base: f64) -> Terrain {
        Terrain::new(
            1,
            q4(base),
            None,
            Vec::new(),
            features,
            rules,
            lava.map(|lava| Caves { lava, lava_from: if lava == 1 { 1 } else { i32::MIN }, lava_to: 11 }),
            Vec::new(),
        )
    }

    fn cube(t: &Terrain, seed: u64, pos: [i32; 3]) -> Vec<u32> {
        let mut blocks = vec![0u32; 4096];
        t.generate(seed, pos, &mut blocks, &mut [0; 256]);
        blocks
    }

    #[test]
    fn masks_ramp_both_ways() {
        assert_eq!(ramp(2048, 0, ONE), 2048);
        assert_eq!(ramp(-819, 0, -1638), 2048);
        assert_eq!(ramp(2 * ONE, 0, ONE), ONE);
    }

    /// Caverns end under a roof, not at the bottom of the surface cube (that leaves flat stone
    /// slabs hanging over open space).
    #[test]
    fn caverns_keep_a_roof() {
        let t = terrain(vec![hills(16, 110), mountains(150, 260)], Vec::new(), Some(1), 64.0);
        let (mut columns, mut breached) = (0, 0);
        for cx in -3..3 {
            for cz in -3..3 {
                let heights: Vec<i32> = (0..256)
                    .map(|i| {
                        t.column(7, cx * 16 + (i % 16) as i32, cz * 16 + (i / 16) as i32)
                            .height
                    })
                    .collect();
                let top = heights.iter().max().unwrap() >> 4;
                let bottom = (heights.iter().min().unwrap() >> 4) - 1;
                // Air within the column, by height.
                let mut air = std::collections::HashSet::new();
                for cy in bottom..=top {
                    for (i, &b) in cube(&t, 7, [cx, cy, cz]).iter().enumerate() {
                        if b == 0 {
                            air.insert((i % 256, cy * 16 + (i / 256) as i32));
                        }
                    }
                }
                for (i, &h) in heights.iter().enumerate() {
                    if cavern_roof(7, cx * 16 + (i % 16) as i32, cz * 16 + (i / 16) as i32) < ONE {
                        continue;
                    }
                    columns += 1;
                    // Open space in the 8 blocks under the surface means the cave reached up.
                    let open = (h - 8..h).filter(|&y| air.contains(&(i, y))).count();
                    if open >= 6 {
                        breached += 1;
                    }
                }
            }
        }
        assert!(breached * 50 < columns, "{breached} of {columns} columns open under the surface");
    }

    /// The point check trees use agrees with what generation carved.
    #[test]
    fn surface_carved_matches_generation() {
        let t = terrain(vec![hills(16, 110)], Vec::new(), Some(1), 64.0);
        let mut checked = 0;
        for cx in -4..4 {
            for cz in -4..4 {
                for i in 0..256 {
                    let (x, z) = (cx * 16 + (i % 16) as i32, cz * 16 + (i / 16) as i32);
                    let h = t.column(3, x, z).height;
                    let air = cube(&t, 3, [cx, h >> 4, cz])[((h & 15) as usize * 16 + i / 16) * 16 + i % 16] == 0;
                    assert_eq!(air, t.surface_carved(3, x, z), "at {x} {z}");
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 16384);
    }

    /// The point check decorations use agrees with what generation put there.
    #[test]
    fn surface_block_matches_generation() {
        let t = terrain(vec![hills(16, 110)], Vec::new(), Some(1), 64.0);
        for cx in -2..2 {
            for cz in -2..2 {
                for i in 0..256 {
                    let (x, z) = (cx * 16 + (i % 16) as i32, cz * 16 + (i / 16) as i32);
                    let h = t.column(3, x, z).height;
                    let made = cube(&t, 3, [cx, h >> 4, cz])[((h & 15) as usize * 16 + i / 16) * 16 + i % 16];
                    assert_eq!(made, t.surface_block(3, x, z), "at {x} {z}");
                }
            }
        }
    }

    /// Lava lakes lie on stone, not in every open block between two heights (lava hanging
    /// over caves below).
    #[test]
    fn lava_lakes_rest_on_stone() {
        let t = terrain(vec![hills(16, 110)], Vec::new(), Some(7), 64.0);
        let (mut lava, mut hanging) = (0, 0);
        for cx in -3..3 {
            for cz in -3..3 {
                let column: FxHashMap<i32, Vec<u32>> = (-32..1).map(|cy| (cy, cube(&t, 9, [cx, cy, cz]))).collect();
                let at = |i: usize, y: i32| {
                    column
                        .get(&(y >> 4))
                        .map(|b| b[((y & 15) as usize * 16 + i / 16) * 16 + i % 16])
                };
                for i in 0..256 {
                    for y in -511..16 {
                        if at(i, y) == Some(7) {
                            lava += 1;
                            if at(i, y - 1) == Some(0) {
                                hanging += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(lava > 0, "no lakes at all");
        assert_eq!(hanging, 0, "{hanging} of {lava} lava blocks over air");
    }

    #[test]
    fn first_matching_rule_wins() {
        let low = |b| Rule { height: (i32::MIN, 60), ..rule(b) };
        assert_eq!(
            terrain(Vec::new(), vec![low(1), rule(2)], None, 70.0)
                .column(1, 0, 0)
                .biome,
            2
        );
        assert_eq!(
            terrain(Vec::new(), vec![low(1), rule(2)], None, 50.0)
                .column(1, 0, 0)
                .biome,
            1
        );
    }

    fn biome_rule(name: &str, biome: &str) -> BiomeRule {
        BiomeRule {
            name: if name.is_empty() { biome } else { name }.into(),
            fields: rf::BIOME,
            biome: biome.into(),
            height: [i32::MIN, i32::MAX],
            depth: 3,
            deep_below: i32::MIN,
            bare_above: i32::MAX,
            ..Default::default()
        }
    }

    /// A terrain entry of stone `a:stone`, then `ops` (operation, component, value, anchor) in order.
    struct Ops(Entry, Vec<String>);

    impl Ops {
        fn new() -> Ops {
            Ops(Entry::default(), Vec::new()).op(vo::SET, t::STONE, "a:stone".to_string(), "")
        }

        fn op<V: abi::data::Data>(mut self, op: u32, name: &str, v: V, anchor: &str) -> Ops {
            let mut o = RawEntry::engine(abi::entry_kind::TERRAIN, "")
                .set(name, v)
                .ops
                .remove(0);
            (o.op, o.anchor) = (op, anchor.into());
            self.0.apply(&o, weft_abi::ModId(0), &mut self.1);
            self
        }

        fn push<V: abi::data::Data>(self, name: &str, v: V) -> Ops {
            self.op(vo::PUSH, name, v, "")
        }

        fn build(mut self, names: &Names) -> (Terrain, Vec<String>) {
            let t = build("a:dim", &self.0, names, &mut self.1);
            (t, self.1)
        }
    }

    fn built(ops: Ops) -> (Vec<u32>, Vec<String>) {
        let biomes = ["a:plains", "a:desert", "b:savanna", "b:dunes", "a:snow"];
        let names = Names {
            block: &|n| (n == "a:stone").then_some(1),
            biome: &|n| biomes.iter().position(|b| *b == n).map(|i| i as u32),
            biome_rule: &|_| None,
        };
        let (t, errors) = ops.build(&names);
        (t.rules.iter().map(|r| r.biome).collect(), errors)
    }

    /// Edits find rules by name (a biome's own name by default) and keep the owner's order around them.
    #[test]
    fn edits_insert_by_name() {
        let base = || {
            Ops::new()
                .push(t::RULES, biome_rule("", "a:desert"))
                .push(t::RULES, biome_rule("", "a:plains"))
        };
        let ops = base()
            .op(vo::INSERT_BEFORE, t::RULES, biome_rule("", "b:savanna"), "a:desert")
            .op(vo::INSERT_AFTER, t::RULES, biome_rule("", "b:dunes"), "a:desert");
        assert_eq!(built(ops), (vec![2, 1, 3, 0], vec![]));

        let ops = base()
            .op(vo::REPLACE_ITEM, t::RULES, biome_rule("", "b:dunes"), "a:desert")
            .op(vo::REMOVE_ITEM, t::RULES, BiomeRule::default(), "a:plains");
        assert_eq!(built(ops).0, vec![3]);

        // A second rule of the same biome needs a name of its own.
        let ops = base().op(vo::INSERT_BEFORE, t::RULES, biome_rule("", "a:desert"), "a:plains");
        assert_eq!(built(ops).1.len(), 1);
        let ops = base().op(vo::INSERT_BEFORE, t::RULES, biome_rule("b:hot_coast", "a:desert"), "a:plains");
        assert_eq!(built(ops), (vec![1, 1, 0], vec![]));

        // An edit whose target is missing says so, like any list edit, and changes nothing.
        let ops = base().op(vo::INSERT_BEFORE, t::RULES, biome_rule("", "b:savanna"), "a:jungle");
        let (rules, errors) = built(ops);
        assert_eq!((rules, errors.len()), (vec![1, 0], 1));

        // One rule edited: only the fields the edit gives.
        let mut taller = biome_rule("a:desert", "");
        (taller.fields, taller.height) = (rf::HEIGHT, [0, 99]);
        let (t, errors) = base().push(t::RULE_EDITS, taller).build(&Names {
            block: &|_| Some(1),
            biome: &|n| (n == "a:desert").then_some(7),
            biome_rule: &|_| None,
        });
        assert_eq!((t.rules[0].biome, t.rules[0].height, errors.len()), (7, (0, 99), 1), "a:plains is unknown here");
    }

    /// A rule starts from its biome's climate and surface and overrides only what it sets.
    #[test]
    fn rules_inherit_the_biome() {
        let range = |name: &str, min, max| abi::RuleRange { name: name.into(), min, max };
        let mut desert = biome_rule("", "a:desert");
        desert.fields = rf::RANGES | rf::SURFACE;
        desert.ranges = vec![range("temperature", 0.35, f64::INFINITY)];
        (desert.top, desert.filler, desert.depth) = ("a:sand".into(), "a:sand".into(), 3);
        let mut coast = biome_rule("a:coast", "a:desert");
        coast.fields |= rf::RANGES | rf::HEIGHT;
        coast.ranges = vec![range("temperature", 0.0, 1.0)];
        coast.height = [0, 64];
        let names = Names {
            block: &|n| Some(if n == "a:sand" { 2 } else { 1 }),
            biome: &|_| Some(0),
            biome_rule: &|n| (n == "a:desert").then_some(&desert),
        };
        let (t, errors) = Ops::new()
            .push(t::RULES, coast)
            .push(t::RULES, biome_rule("", "a:desert"))
            .build(&names);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!((&t.rules[0].ranges[..], t.rules[0].height, t.rules[0].top), (&[(0, 0, ONE)][..], (0, 64), Some(2)));
        assert_eq!(
            (&t.rules[1].ranges[..], t.rules[1].height, t.rules[1].top),
            (&[(0, q12(0.35), 1 << 18)][..], (i32::MIN, i32::MAX), Some(2))
        );
    }

    #[test]
    fn expressions_are_checked() {
        let none = |_: &str| None;
        let ok = |steps: Vec<ExprStep>| Expr::resolve(&steps, &none).map(|_| ());
        assert!(ok(vec![step(ek::X, 0), step(ek::Z, 0), step(ek::ADD, 0)]).is_ok());
        assert!(ok(vec![step(ek::ADD, 0)]).is_err(), "nothing to add");
        assert!(ok(vec![step(ek::X, 0), step(ek::Z, 0)]).is_err(), "two values left");
        assert!(ok(vec![]).is_err());
        assert!(ok(vec![step(99, 0)]).is_err(), "unknown step");
        assert!(ok(vec![step(ek::NOISE, 0)]).is_err(), "noise needs x and z");
        let deep = vec![step(ek::X, 0); limit::DEPTH as usize + 1];
        let mut deep: Vec<ExprStep> = deep;
        deep.extend(vec![step(ek::ADD, 0); limit::DEPTH as usize]);
        assert!(ok(deep).is_err(), "too deep");
        let mut by_name = step(ek::FIELD, 0);
        by_name.name = "wet".into();
        assert!(ok(vec![by_name.clone()]).is_err(), "no such field");
        assert_eq!(
            Expr::resolve(&[by_name], &|n| (n == "wet").then_some(5))
                .unwrap()
                .0[0]
                .a,
            5
        );
    }

    #[test]
    fn expressions_compute() {
        let at = |steps: Vec<ExprStep>| expr(steps).eval(&[], 5, 160, -48);
        assert_eq!(at(vec![step(ek::X, 0), step(ek::Z, 0), step(ek::SUB, 0)]), 208);
        // Fixed point: 2.5 * 1.5, and the factors held where the product still fits.
        assert_eq!(at(vec![step(ek::CONST, 10240), step(ek::CONST, 6144), step(ek::MUL, 0)]), 15360);
        assert_eq!(at(vec![step(ek::CONST, i32::MAX), step(ek::CONST, i32::MAX), step(ek::MUL, 0)]), (MUL_LIMIT * MUL_LIMIT) >> 12);
        // Integers round toward zero, a zero divisor gives 0, a remainder is never negative.
        assert_eq!(at(vec![step(ek::CONST, -7), step(ek::CONST, 2), step(ek::DIV, 0)]), -3);
        assert_eq!(at(vec![step(ek::CONST, 7), step(ek::CONST, 0), step(ek::DIV, 0)]), 0);
        assert_eq!(at(vec![step(ek::CONST, -7), step(ek::CONST, 5), step(ek::REM, 0)]), 3);
        assert_eq!(at(vec![step(ek::CONST, i32::MIN), step(ek::ABS, 0)]), i32::MIN);
        let ramp = |from, to| ExprStep { b: to, ..step(ek::RAMP, from) };
        assert_eq!(at(vec![step(ek::CONST, 2048), ramp(0, ONE)]), 2048);
        assert_eq!(at(vec![step(ek::CONST, -819), ramp(0, -1638)]), 2048);
        // Noise takes the position from the stack, so it can be moved (a warp).
        let noise = |x, z| expr(vec![step(ek::CONST, x), step(ek::CONST, z), noise(7, 100 * 16, 3)]).eval(&[], 5, 0, 0);
        assert_eq!(noise(160, -48), fbm(seed32(5, 7), 160, -48, 100 * 16, 3));
        let hills = hills(16, 110).height.eval(&[], 5, 160, -48);
        assert_eq!(hills, (fbm(seed32(5, salt::HILLS), 160, -48, 110 * 16, 4) * 16 * 16) >> 12);
    }

    /// Fields and features see the fields they name, later ones see earlier ones, and the standard
    /// ones nothing asks for are never computed.
    #[test]
    fn fields_are_resolved_and_pruned() {
        let field = |name: &str, value: Vec<ExprStep>| abi::TerrainField { name: name.into(), value };
        let by_name = |n: &str| ExprStep { name: n.into(), ..step(ek::FIELD, 0) };
        let wet_range = |name: &str| {
            let mut r = biome_rule("", "a:desert");
            r.fields |= rf::RANGES;
            r.ranges = vec![abi::RuleRange { name: name.into(), min: 0.5, max: f64::INFINITY }];
            r
        };
        let names = Names { block: &|_| Some(1), biome: &|_| Some(0), biome_rule: &|_| None };
        // `wet` is twice the humidity; `damp` uses `wet`; only `damp` is asked for.
        let (t, errors) = Ops::new()
            .push(t::FIELDS, field("wet", vec![by_name("humidity"), step(ek::CONST, 2 * ONE), step(ek::MUL, 0)]))
            .push(t::FIELDS, field("damp", vec![by_name("wet"), step(ek::CONST, 100), step(ek::ADD, 0)]))
            .push(t::RULES, wet_range("damp"))
            .build(&names);
        assert!(errors.is_empty(), "{errors:?}");
        let used: Vec<(&str, bool)> = t
            .fields
            .iter()
            .map(|f| (f.name.as_str(), !f.expr.0.is_empty()))
            .collect();
        assert_eq!(used, [("temperature", false), ("humidity", true), ("continent", false), ("wet", true), ("damp", true)]);
        // A field can't use one declared after it, and a rule can't ask for one that is not there.
        let (_, errors) = Ops::new()
            .push(t::FIELDS, field("early", vec![by_name("late")]))
            .push(t::FIELDS, field("late", vec![step(ek::X, 0)]))
            .build(&names);
        assert_eq!(errors.len(), 1, "{errors:?}");
        let (_, errors) = Ops::new()
            .push(t::RULES, wet_range("nothing"))
            .build(&names);
        assert_eq!(errors.len(), 1, "{errors:?}");
        // Declared again, the standard temperature is replaced where it stands.
        let (t, _) = Ops::new()
            .push(t::FIELDS, field("temperature", vec![step(ek::CONST, 7)]))
            .push(t::RULES, wet_range("temperature"))
            .build(&names);
        assert_eq!(t.fields[0].expr.eval(&[], 1, 0, 0), 7);
    }
}
