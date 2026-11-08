//! `weft --submit-mod <dir> [--owner <login>]`: sends a mod's pushed commit to the mod index as
//! one new file, `mods/<id>/<version>.toml`. The index builds the mod itself from that commit, so
//! nothing is uploaded from here. With the GitHub CLI (`gh`) logged in the pull request is opened
//! from here; otherwise the browser opens GitHub's page for the new file, filled in.

use std::path::Path;
use std::process::Command;

const INDEX: &str = "Weft-Rust/Weft-Mods-Index";
const SDK_GIT: &str = "https://github.com/Weft-Rust/Weft";

pub fn run(dir: &Path, owner: Option<String>) -> Result<String, String> {
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).map_err(|e| format!("{}: {e}", dir.join("Cargo.toml").display()))?;
    let id = package_field(&manifest, "name").ok_or("Cargo.toml has no package name")?;
    let version = package_field(&manifest, "version").ok_or("Cargo.toml has no package version")?;
    let valid = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    };
    if !valid(&id) {
        return Err(format!("the package name {id} is the mod id: lowercase letters, digits and _ only"));
    }
    let weft = sdk_tag(&manifest).ok_or(format!(
        "weft_sdk must come from the game's repository at a release tag, for the index to build it:\nweft_sdk = {{ git = \"{SDK_GIT}\", tag = \"v26.11.3\" }}"
    ))?;
    if !git(dir, &["status", "--porcelain"])?.is_empty() {
        return Err("commit your changes first: the index builds the commit, not what is on disk".into());
    }
    let commit = git(dir, &["rev-parse", "HEAD"])?;
    if git(dir, &["branch", "-r", "--contains", &commit])?.is_empty() {
        return Err("push this commit first: the index builds it from your repository".into());
    }
    let repo = https_url(&git(dir, &["remote", "get-url", "origin"])?).ok_or("origin is not a GitHub repository")?;
    let sub = git(dir, &["rev-parse", "--show-prefix"])?
        .trim_end_matches('/')
        .to_string();
    let owner = owner
        .or_else(|| gh(&["api", "user", "-q", ".login"]))
        .or_else(|| {
            repo.trim_start_matches("https://github.com/")
                .split('/')
                .next()
                .map(String::from)
        })
        .ok_or("say who owns the mod id: --owner <your GitHub login>")?;
    // Already escaped as TOML in Cargo.toml.
    let summary = package_field(&manifest, "description").unwrap_or_default();

    println!("building {id} {version} against Weft {weft} to be sure it does");
    let built = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(dir)
        .status()
        .map_err(|e| format!("cargo: {e}"))?;
    if !built.success() {
        return Err("it doesn't build here, so it wouldn't on the index either".into());
    }

    let path = format!("mods/{id}/{version}.toml");
    let file = format!(
        "# Shown in the launcher. The newest version's say what the mod is.\nname = \"{id}\"\nsummary = \"{summary}\"\nauthor = \"{owner}\"\n\n# What is built: the crate in `dir` of `repo` at `commit`, against the SDK of `weft`.\nrepo = \"{repo}\"\ndir = \"{sub}\"\ncommit = \"{commit}\"\nweft = \"{weft}\"\n# Mod ids it needs besides base, installed with it.\ndeps = []\n# The GitHub login the id belongs to: only they can add versions.\nowner = \"{owner}\"\n# Set by a maintainer who read the source of this commit.\nreviewed = \"\"\n"
    );
    if gh_ok(&["auth", "status"]) {
        return pull_request(&id, &version, &path, &file);
    }
    let url = format!("https://github.com/{INDEX}/new/main?filename={}&value={}", encode(&path), encode(&file));
    open(&url);
    Ok(format!(
        "GitHub opened in the browser: fill in its name and summary if you like, then Propose new file and Create pull request.\nIf it didn't open:\n{url}"
    ))
}

/// Through `gh`: a fork of the index, a branch with the file, a pull request.
fn pull_request(id: &str, version: &str, path: &str, file: &str) -> Result<String, String> {
    let tmp = std::env::temp_dir().join(format!("weft-submit-{}", std::process::id()));
    let clone = Command::new("gh")
        .args(["repo", "fork", INDEX, "--clone", "--default-branch-only", "--", "--depth=1"])
        .arg(&tmp)
        .status()
        .map_err(|e| format!("gh: {e}"))?;
    if !clone.success() {
        return Err("gh couldn't fork the index".into());
    }
    let branch = format!("{id}-{version}");
    let target = tmp.join(path);
    std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&target, file).map_err(|e| e.to_string())?;
    let title = format!("{id} {version}");
    for args in [vec!["checkout", "-b", &branch], vec!["add", path], vec!["commit", "-m", &title], vec!["push", "-u", "origin", &branch]] {
        git(&tmp, &args)?;
    }
    let pr = Command::new("gh")
        .args(["pr", "create", "--repo", INDEX, "--title", &title, "--body", "Submitted with weft --submit-mod."])
        .current_dir(&tmp)
        .output()
        .map_err(|e| format!("gh: {e}"))?;
    let _ = std::fs::remove_dir_all(&tmp);
    if !pr.status.success() {
        return Err(String::from_utf8_lossy(&pr.stderr).into_owned());
    }
    Ok(format!("pull request opened: {}", String::from_utf8_lossy(&pr.stdout).trim()))
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// What `gh` prints, if it is there and that worked.
fn gh(args: &[&str]) -> Option<String> {
    let out = Command::new("gh").args(args).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

fn gh_ok(args: &[&str]) -> bool {
    Command::new("gh")
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `key = "value"` under `[package]`.
fn package_field(manifest: &str, key: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// The tag of `weft_sdk = { git = "<the game's repository>", tag = "..." }`.
fn sdk_tag(manifest: &str) -> Option<String> {
    let line = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("weft_sdk") && l.contains(SDK_GIT))?;
    let after = &line[line.find("tag")? + 3..];
    let start = after.find('"')? + 1;
    let end = start + after[start..].find('"')?;
    Some(after[start..end].to_string())
}

/// `git@github.com:a/b.git` or `https://github.com/a/b.git` as `https://github.com/a/b`.
fn https_url(remote: &str) -> Option<String> {
    let path = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("https://github.com/"))?;
    Some(format!("https://github.com/{}", path.trim_end_matches(".git").trim_end_matches('/')))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn open(url: &str) {
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = Command::new(opener).arg(url).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_says_what_to_build() {
        let m = "[package]\nname = \"cogwork\"\nversion = \"0.2.1\"\n\n[dependencies]\nweft_sdk = { git = \"https://github.com/Weft-Rust/Weft\", tag = \"v26.11.3\" }\n";
        assert_eq!(package_field(m, "name").as_deref(), Some("cogwork"));
        assert_eq!(package_field(m, "version").as_deref(), Some("0.2.1"));
        assert_eq!(sdk_tag(m).as_deref(), Some("v26.11.3"));
        assert_eq!(https_url("git@github.com:a/b.git").as_deref(), Some("https://github.com/a/b"));
        assert_eq!(encode("a b&c"), "a%20b%26c");
    }
}
