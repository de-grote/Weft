//! What the editor makes and the code for it, both ways. A thing is a handle and the builder
//! calls made on it, each argument kept as the code writes it; which calls there are comes from
//! the SDK itself (`METHODS`, listed by the build script), so nothing here knows any one method.

use crate::ui::text::Span;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum What {
    Block,
    Item,
}

impl What {
    /// Its bit in `Method::on`.
    fn bit(self) -> u8 {
        match self {
            What::Block => 1,
            What::Item => 2,
        }
    }

    /// The folder its pictures are under.
    pub fn folder(self) -> &'static str {
        match self {
            What::Block => "block",
            What::Item => "item",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Call {
    pub method: String,
    /// Each argument as code (`"base:stone"`, `1.5`, `[0.0, 0.0, 0.0]`).
    pub args: Vec<String>,
}

impl Call {
    pub fn new(method: &str, args: &[&str]) -> Call {
        Call { method: method.into(), args: args.iter().map(|a| a.to_string()).collect() }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Thing {
    pub kind: What,
    pub name: String,
    pub calls: Vec<Call>,
}

impl Thing {
    /// A new one from the editor's buttons: something to start from.
    pub fn new(kind: What, name: &str) -> Thing {
        let calls = match kind {
            What::Block => vec![
                Call::new("texture", &["\"base:stone\""]),
                Call::new("sound_group", &["\"base:block.stone\""]),
                Call::new("hardness", &["1.5"]),
                Call::new("mined_by", &["\"pickaxe\"", "abi::tool_tier::WOOD"]),
            ],
            What::Item => vec![Call::new("texture", &["\"base:diamond\""])],
        };
        Thing { kind, name: name.into(), calls }
    }

    /// The arguments of its calls to `method`, in order.
    pub fn args_of<'a>(&'a self, method: &'a str) -> impl Iterator<Item = &'a [String]> + 'a {
        self.calls
            .iter()
            .filter(move |c| c.method == method)
            .map(|c| c.args.as_slice())
    }

    /// The picture its `texture` call names, as the content lists it (`base:block/stone`).
    pub fn picture(&self) -> Option<String> {
        let t = unquote(self.args_of("texture").next()?.first()?)?;
        picture_name(&t, self.kind.folder())
    }

    /// Its model's boxes, `[min, max]` in 1/16; a whole cube without any.
    pub fn boxes(&self) -> Vec<[f32; 6]> {
        let v: Vec<[f32; 6]> = self
            .args_of("model_box")
            .filter_map(|a| {
                let (lo, hi) = (three(a.first()?)?, three(a.get(1)?)?);
                Some([lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]])
            })
            .collect();
        if v.is_empty() { vec![[0.0, 0.0, 0.0, 16.0, 16.0, 16.0]] } else { v }
    }
}

/// The tool tiers `mined_by` names, as `abi::tool_tier::<name>`.
pub const TIERS: [&str; 5] = ["HAND", "WOOD", "STONE", "IRON", "DIAMOND"];

/// A builder's picture argument (`base:stone`) as the content names it (`base:block/stone`).
/// Relative names are the mod's own, which the game running the editor doesn't have.
pub fn picture_name(arg: &str, folder: &str) -> Option<String> {
    let (ns, path) = arg.split_once(':')?;
    Some(format!("{ns}:{folder}/{path}"))
}

/// A parameter of a builder method, as the SDK writes it.
pub struct Param {
    pub name: &'static str,
    pub ty: &'static str,
    /// The folder of the picture it names (`block`, `item`, `entity`), empty if it names none.
    pub picture: &'static str,
}

/// A builder method: `on` is which builders have it (1 blocks, 2 items).
pub struct Method {
    pub on: u8,
    pub name: &'static str,
    pub params: &'static [Param],
    pub doc: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/sdk_methods.rs"));

/// What `kind`'s builder can be told, by name.
pub fn methods(kind: What) -> impl Iterator<Item = &'static Method> {
    METHODS.iter().filter(move |m| m.on & kind.bit() != 0)
}

pub fn method(kind: What, name: &str) -> Option<&'static Method> {
    methods(kind).find(|m| m.name == name)
}

/// A value of a parameter's type to start a new call with.
pub fn default_arg(p: &Param) -> String {
    match p.ty {
        "&str" => "\"\"".into(),
        "f32" | "f64" => "1.0".into(),
        "bool" => "true".into(),
        // A box's far corner starts at the block's, so a new box is the whole block.
        "[f32;3]" if p.name == "max" => "[16.0, 16.0, 16.0]".into(),
        "[f32;3]" => "[0.0, 0.0, 0.0]".into(),
        t if t.starts_with('[') => "[]".into(),
        t if t.starts_with("&[") => "&[]".into(),
        "&CreativeTab" => "&CreativeTab::named(\"base:building\")".into(),
        t if t.starts_with('u') || t.starts_with('i') => "0".into(),
        _ => String::new(),
    }
}

pub fn unquote(a: &str) -> Option<String> {
    a.trim()
        .strip_prefix('"')?
        .strip_suffix('"')
        .map(String::from)
}

pub fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A number as code writes it: `1.5`, `2.0`, `0.25`; no float noise (`7.500005` is `7.5`).
pub fn num(v: f32) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    let s = format!("{r}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

pub fn three(a: &str) -> Option<[f32; 3]> {
    let inner = a.trim().strip_prefix('[')?.strip_suffix(']')?;
    if let Some((v, n)) = inner.split_once(';') {
        return (n.trim() == "3")
            .then(|| v.trim().parse().ok().map(|v| [v; 3]))
            .flatten();
    }
    let v: Vec<f32> = inner
        .split(',')
        .map(|x| x.trim().trim_end_matches("f32").parse().ok())
        .collect::<Option<_>>()?;
    (v.len() == 3).then(|| [v[0], v[1], v[2]])
}

fn handle(t: &Thing) -> String {
    t.name.to_uppercase()
}

/// The code for everything: the handles, then each one's calls in the order they were made.
pub fn write(things: &[Thing]) -> String {
    let mut out = String::from("// Made in the editor\n");
    for t in things {
        let ty = if t.kind == What::Block { "Block" } else { "Item" };
        out += &format!("pub static {}: {ty} = {ty}::named({});\n", handle(t), quote(&t.name));
    }
    for t in things {
        out += &format!("\nr.{}(&{})", t.kind.folder(), handle(t));
        for c in &t.calls {
            out += &format!("\n    .{}({})", c.method, c.args.join(", "));
        }
        out += ";\n";
    }
    out
}

/// Code without its comments, strings kept whole.
fn strip_comments(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines() {
        let mut in_str = false;
        let mut prev = ' ';
        let mut cut = line.len();
        for (i, c) in line.char_indices() {
            if c == '"' && prev != '\\' {
                in_str = !in_str;
            }
            if !in_str && c == '/' && prev == '/' {
                cut = i - 1;
                break;
            }
            prev = c;
        }
        out += &line[..cut];
        out.push('\n');
    }
    out
}

/// Reads the code back. Each `r.block(&X)` / `r.item(&X)` chain becomes a thing, named by its
/// `static X: Block = Block::named("x")`; a mistake says which line.
pub fn read(src: &str) -> Result<Vec<Thing>, String> {
    let src = strip_comments(src);
    let line_of = |at: usize| src[..at].matches('\n').count() + 1;
    let mut names: Vec<(String, String)> = Vec::new();
    for decl in src.split(';') {
        let Some(i) = decl.find("static ") else { continue };
        let decl = &decl[i + 7..];
        if let (Some((h, _)), Some(name)) = (decl.split_once(':'), decl.split('"').nth(1)) {
            names.push((h.trim().to_string(), name.to_string()));
        }
    }
    let mut things = Vec::new();
    let mut at = 0;
    while let Some(i) = src[at..].find("r.").map(|i| at + i) {
        let open = &src[i + 2..];
        let (kind, skip) = if open.starts_with("block(") {
            (What::Block, 8)
        } else if open.starts_with("item(") {
            (What::Item, 7)
        } else {
            at = i + 2;
            continue;
        };
        let close = src[i + skip..]
            .find(')')
            .map(|c| i + skip + c)
            .ok_or_else(|| format!("line {}: no `)`", line_of(i)))?;
        let h = src[i + skip..close].trim().trim_start_matches('&').trim();
        let name = names
            .iter()
            .find(|n| n.0 == h)
            .map(|n| n.1.clone())
            .ok_or_else(|| format!("line {}: no `static {h}` above", line_of(i)))?;
        let mut t = Thing { kind, name, calls: Vec::new() };
        let mut pos = close + 1;
        loop {
            let s = &src[pos..];
            let trimmed = s.trim_start();
            pos += s.len() - trimmed.len();
            if trimmed.starts_with(';') {
                pos += 1;
                break;
            }
            if !trimmed.starts_with('.') {
                return Err(format!("line {}: expected `.call(...)` or `;`", line_of(pos)));
            }
            let (call, len) = one_call(trimmed).ok_or_else(|| format!("line {}: unfinished call", line_of(pos)))?;
            t.calls.push(call);
            pos += len;
        }
        things.push(t);
        at = pos;
    }
    if things.is_empty() {
        return Err("nothing registered: no `r.block(&X)` or `r.item(&X)`".into());
    }
    Ok(things)
}

/// `.name(args)` at the start of `s`, its arguments split at their top commas, and how long it is.
fn one_call(s: &str) -> Option<(Call, usize)> {
    let open = s.find('(')?;
    let method = s[1..open].trim().to_string();
    if method.is_empty() || !method.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let (mut depth, mut in_str, mut prev) = (0, false, ' ');
    let mut args = Vec::new();
    let mut cur = String::new();
    for (i, c) in s[open..].char_indices() {
        if c == '"' && prev != '\\' {
            in_str = !in_str;
        }
        prev = c;
        if !in_str {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                if !cur.trim().is_empty() {
                    args.push(cur.trim().to_string());
                }
                return Some((Call { method, args }, open + i + 1));
            }
            if depth == 1 && (c == ',' || (c == '(' && i == 0)) {
                if c == ',' {
                    args.push(cur.trim().to_string());
                    cur.clear();
                }
                continue;
            }
        }
        cur.push(c);
    }
    None
}

/// Colors for `highlight`, `None` for the field's own.
#[derive(Clone, Copy, Default)]
pub struct Colors {
    pub keyword: Option<u32>,
    pub call: Option<u32>,
    pub string: Option<u32>,
    pub number: Option<u32>,
    pub comment: Option<u32>,
}

/// Code as spans, every glyph the same width (`mono`), so a field puts its caret by counting.
pub fn highlight(src: &str, c: &Colors) -> Vec<Span> {
    const KEYWORDS: [&str; 8] = ["pub", "static", "let", "fn", "use", "mut", "move", "as"];
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        let ch = chars[i];
        let take = |i: &mut usize, f: &dyn Fn(char) -> bool| {
            while *i < chars.len() && f(chars[*i]) {
                *i += 1;
            }
        };
        let color = if ch == '/' && chars.get(i + 1) == Some(&'/') {
            take(&mut i, &|c| c != '\n');
            c.comment
        } else if ch == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' && chars[i] != '\n' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            i = (i + 1).min(chars.len());
            c.string
        } else if ch.is_ascii_digit() {
            take(&mut i, &|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
            c.number
        } else if ch.is_alphabetic() || ch == '_' {
            take(&mut i, &|c| c.is_alphanumeric() || c == '_');
            let word: String = chars[start..i].iter().collect();
            if KEYWORDS.contains(&word.as_str()) {
                c.keyword
            } else if chars.get(i) == Some(&'(') {
                c.call
            } else {
                None
            }
        } else {
            i += 1;
            take(&mut i, &|c| !(c.is_alphanumeric() || c == '_' || c == '"' || c == '/'));
            None
        };
        // Text drops spaces at the start of a line: these never break, and are as wide.
        let s: String = chars[start..i]
            .iter()
            .map(|&c| if c == ' ' { '\u{a0}' } else { c })
            .collect();
        let mut span = Span::text(s, color);
        span.mono = true;
        out.push(span);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_reads_back_the_same() {
        let mut b = Thing::new(What::Block, "ruby_block");
        b.calls
            .push(Call::new("model_box", &["[0.0, 0.0, 0.0]", "[16.0, 8.0, 16.0]"]));
        b.calls
            .push(Call::new("rule", &["Rule::random(4.0).becomes(\"base:stone\")"]));
        b.calls.push(Call::new("falling", &[]));
        b.calls
            .push(Call::new("tab", &["&CreativeTab::named(\"base:building\")"]));
        let things = vec![b, Thing::new(What::Item, "ruby")];
        let code = write(&things);
        assert_eq!(read(&code).unwrap(), things, "{code}");
        assert_eq!(things[0].boxes(), vec![[0.0, 0.0, 0.0, 16.0, 8.0, 16.0]]);
        assert_eq!(things[0].picture().as_deref(), Some("base:block/stone"));
        // Written by hand: comments, other spacing.
        let hand = "pub static X: Block = Block::named(\"x\"); // mine\nr.block(&X).hardness(7.5) .light(15)\n  .tag(\"a, b\");";
        let t = &read(hand).unwrap()[0];
        assert_eq!(t.calls, vec![Call::new("hardness", &["7.5"]), Call::new("light", &["15"]), Call::new("tag", &["\"a, b\""])]);
        assert!(
            read("r.block(&Y).hardness(1.0);")
                .unwrap_err()
                .contains("line 1")
        );
        assert!(
            read("pub static X: Block = Block::named(\"x\");\nr.block(&X)\n    .hardness(1.0)\n    oops;")
                .unwrap_err()
                .contains("line 4")
        );
    }

    #[test]
    fn the_sdk_is_listed() {
        let tex = method(What::Block, "texture").unwrap();
        assert_eq!((tex.params[0].ty, tex.params[0].picture), ("&str", "block"));
        assert!(method(What::Block, "tint").is_some() && method(What::Item, "max_stack").is_some());
        assert!(method(What::Item, "hardness").is_none());
    }

    #[test]
    fn numbers_without_noise() {
        assert_eq!(num(7.500005), "7.5");
        assert_eq!(num(2.0), "2.0");
        assert_eq!(three("[0.0; 3]"), Some([0.0; 3]));
    }
}
