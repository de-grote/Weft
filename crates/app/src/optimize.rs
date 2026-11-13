//! `weft optimize-assets`: the game's pictures and sounds made smaller where it can't be told.
//! A PNG is written again in the smallest form that the game reads back pixel for pixel the
//! same (a palette of its colors, gray, no alpha where all of it is opaque); a WAV becomes Ogg
//! Vorbis at quality 6 through ffmpeg, which must be on the PATH.

use std::path::{Path, PathBuf};

use weft_engine::assets::decode_png;

/// Folders of the source tree holding assets.
const DIRS: [&str; 3] = ["crates/engine/assets", "mods", "examples"];
const VORBIS_QUALITY: &str = "6";

pub fn run(root: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    for dir in DIRS {
        walk(&root.join(dir), &mut files);
    }
    let (mut pngs, mut png_saved, mut sounds, mut sound_saved) = (0, 0u64, 0, 0u64);
    for path in &files {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let before = std::fs::metadata(path).map_or(0, |m| m.len());
        match ext.as_str() {
            "png" => {
                let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
                if let Some(smaller) = smaller_png(&bytes) {
                    std::fs::write(path, &smaller).map_err(|e| format!("{}: {e}", path.display()))?;
                    pngs += 1;
                    png_saved += before - smaller.len() as u64;
                }
            }
            "wav" => {
                let out = path.with_extension("ogg");
                let done = std::process::Command::new("ffmpeg")
                    .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                    .arg(path)
                    .args(["-c:a", "libvorbis", "-q:a", VORBIS_QUALITY])
                    .arg(&out)
                    .status()
                    .map_err(|e| format!("ffmpeg is needed for sounds ({e}): https://ffmpeg.org/download.html"))?;
                if !done.success() {
                    return Err(format!("ffmpeg could not convert {}", path.display()));
                }
                let after = std::fs::metadata(&out).map_or(0, |m| m.len());
                std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
                sounds += 1;
                sound_saved += before.saturating_sub(after);
            }
            _ => {}
        }
    }
    let kb = |b: u64| b / 1024;
    Ok(format!("{pngs} pictures {} KB smaller, {sounds} sounds to Ogg {} KB smaller", kb(png_saved), kb(sound_saved)))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "target") {
                walk(&p, out);
            }
        } else {
            out.push(p);
        }
    }
}

/// `bytes` written again in its smallest form the game reads the same, if smaller. Left alone:
/// 16 bits a channel (the game drops the low bits, a writer shouldn't) and animated PNGs.
fn smaller_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let depth = *bytes.get(24)?;
    if depth == 16 || bytes.windows(4).any(|w| w == b"acTL") {
        return None;
    }
    let img = decode_png(bytes)?;
    let (w, h, rgba) = (img.width, img.height, &img.rgba);
    let px = || rgba.chunks_exact(4);
    let opaque = px().all(|p| p[3] == 255);
    let gray = px().all(|p| p[0] == p[1] && p[1] == p[2]);
    let mut tries = Vec::new();
    let channels = |keep: &[usize]| -> Vec<u8> {
        px().flat_map(|p| keep.iter().map(|&i| p[i]).collect::<Vec<_>>())
            .collect()
    };
    match (opaque, gray) {
        (true, true) => tries.push(encode(w, h, png::ColorType::Grayscale, 8, &channels(&[0]), None)),
        (true, false) => tries.push(encode(w, h, png::ColorType::Rgb, 8, &channels(&[0, 1, 2]), None)),
        (false, true) => tries.push(encode(w, h, png::ColorType::GrayscaleAlpha, 8, &channels(&[0, 3]), None)),
        (false, false) => tries.push(encode(w, h, png::ColorType::Rgba, 8, rgba, None)),
    }
    if let Some((palette, indices)) = palette(rgba) {
        let bits = match palette.len() {
            ..=2 => 1,
            ..=4 => 2,
            ..=16 => 4,
            _ => 8,
        };
        let packed = pack(&indices, w as usize, bits);
        let rgb: Vec<u8> = palette.iter().flat_map(|c| [c[0], c[1], c[2]]).collect();
        let alpha: Option<Vec<u8>> = (!opaque).then(|| palette.iter().map(|c| c[3]).collect());
        tries.push(encode(w, h, png::ColorType::Indexed, bits, &packed, Some((&rgb, alpha.as_deref()))));
    }
    let best = tries.into_iter().flatten().min_by_key(|b| b.len())?;
    (best.len() < bytes.len() && decode_png(&best).is_some_and(|back| back.rgba == *rgba)).then_some(best)
}

/// The colors of `rgba` and each pixel's index among them, when there are 256 or fewer.
fn palette(rgba: &[u8]) -> Option<(Vec<[u8; 4]>, Vec<u8>)> {
    let mut colors: Vec<[u8; 4]> = Vec::new();
    let mut index = rustc_hash::FxHashMap::default();
    let mut out = Vec::with_capacity(rgba.len() / 4);
    for p in rgba.chunks_exact(4) {
        let c = [p[0], p[1], p[2], p[3]];
        let i = *index.entry(c).or_insert_with(|| {
            colors.push(c);
            colors.len() - 1
        });
        if i > 255 {
            return None;
        }
        out.push(i as u8);
    }
    Some((colors, out))
}

/// Indices `bits` each, rows starting on a byte.
fn pack(indices: &[u8], width: usize, bits: u8) -> Vec<u8> {
    if bits == 8 {
        return indices.to_vec();
    }
    let per = 8 / bits as usize;
    indices
        .chunks(width)
        .flat_map(|row| {
            row.chunks(per).map(|group| {
                group
                    .iter()
                    .enumerate()
                    .fold(0u8, |b, (k, &i)| b | i << (8 - bits as usize * (k + 1)))
            })
        })
        .collect()
}

fn encode(w: u32, h: u32, color: png::ColorType, bits: u8, data: &[u8], palette: Option<(&[u8], Option<&[u8]>)>) -> Option<Vec<u8>> {
    let depth = match bits {
        1 => png::BitDepth::One,
        2 => png::BitDepth::Two,
        4 => png::BitDepth::Four,
        _ => png::BitDepth::Eight,
    };
    // Rows of indices usually pack best unfiltered, the rest with a filter chosen per row.
    let filters: &[png::Filter] = if color == png::ColorType::Indexed {
        &[png::Filter::NoFilter, png::Filter::Adaptive]
    } else {
        &[png::Filter::Adaptive, png::Filter::NoFilter]
    };
    filters
        .iter()
        .filter_map(|&filter| {
            let mut out = Vec::new();
            let mut e = png::Encoder::new(&mut out, w, h);
            e.set_color(color);
            e.set_depth(depth);
            e.set_compression(png::Compression::High);
            e.set_filter(filter);
            if let Some((rgb, alpha)) = palette {
                e.set_palette(rgb);
                if let Some(a) = alpha {
                    e.set_trns(a);
                }
            }
            let mut writer = e.write_header().ok()?;
            writer.write_image_data(data).ok()?;
            writer.finish().ok()?;
            Some(out)
        })
        .min_by_key(|b| b.len())
}
