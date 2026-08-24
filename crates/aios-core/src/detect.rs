//! Inspecting a directory to fill in what we can without asking.
//!
//! Git is driven by shelling out rather than linking libgit2: it keeps the
//! `VCS` port honest (plan §2 lists it as "git + gh"), avoids a C dependency in
//! a crate that wants to stay a single static binary, and means we always agree
//! with whatever git the user actually has.

use aios_types::ProjectDetection;
use std::path::Path;
use std::process::Command;

pub fn detect(path: &Path) -> ProjectDetection {
    ProjectDetection {
        git_remote: git(path, &["remote", "get-url", "origin"]),
        default_branch: default_branch(path),
        languages: languages(path),
        package_manager: package_manager(path),
        issue_prefix: issue_prefix(path),
    }
}

fn git(path: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

pub fn is_git_repo(path: &Path) -> bool {
    git(path, &["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true")
}

fn default_branch(path: &Path) -> Option<String> {
    // The remote's HEAD is the real answer, but it is only present if someone
    // has fetched it. Fall back to the checked-out branch.
    git(
        path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .and_then(|s| s.rsplit('/').next().map(str::to_owned))
    .or_else(|| git(path, &["branch", "--show-current"]))
}

/// Marker files, most specific first. Presence is evidence, not proof — this is
/// display metadata, and nothing downstream should make decisions on it.
const LANGUAGE_MARKERS: &[(&str, &str)] = &[
    ("Cargo.toml", "rust"),
    ("go.mod", "go"),
    ("package.json", "typescript"),
    ("pyproject.toml", "python"),
    ("requirements.txt", "python"),
    ("Gemfile", "ruby"),
    ("pom.xml", "java"),
    ("build.gradle", "java"),
    ("build.gradle.kts", "kotlin"),
    ("Package.swift", "swift"),
    ("composer.json", "php"),
    ("CMakeLists.txt", "cpp"),
];

fn languages(path: &Path) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for (marker, lang) in LANGUAGE_MARKERS {
        if path.join(marker).exists() && !found.iter().any(|l| l == lang) {
            found.push((*lang).to_string());
        }
    }
    // An Xcode project is a strong signal even without Package.swift.
    if found.iter().all(|l| l != "swift")
        && read_dir_names(path)
            .iter()
            .any(|n| n.ends_with(".xcodeproj") || n.ends_with(".xcworkspace"))
    {
        found.push("swift".to_string());
    }
    found
}

fn package_manager(path: &Path) -> Option<String> {
    // Lockfiles first: they say what is actually in use, where a manifest only
    // says what could be.
    let by_lockfile = [
        ("pnpm-lock.yaml", "pnpm"),
        ("bun.lockb", "bun"),
        ("yarn.lock", "yarn"),
        ("package-lock.json", "npm"),
        ("Cargo.lock", "cargo"),
        ("poetry.lock", "poetry"),
        ("uv.lock", "uv"),
        ("go.sum", "go"),
    ];
    for (file, pm) in by_lockfile {
        if path.join(file).exists() {
            return Some(pm.to_string());
        }
    }
    for (file, pm) in [
        ("Cargo.toml", "cargo"),
        ("package.json", "npm"),
        ("go.mod", "go"),
    ] {
        if path.join(file).exists() {
            return Some(pm.to_string());
        }
    }
    None
}

/// Ask beads for this project's issue prefix, returning `None` when the project
/// has no beads database.
///
/// This goes through `bd` rather than reading `.beads/` because the on-disk
/// layout is beads' business and changes (plan §6.1) — a fact this function
/// learned the hard way, having first parsed `config.yaml` looking for a key
/// named `prefix` when the key is `issue-prefix` and is commented out by default
/// anyway. `bd config list` reports the effective value from all sources.
fn issue_prefix(path: &Path) -> Option<String> {
    if !path.join(".beads").is_dir() {
        return None;
    }
    let out = Command::new("bd")
        .arg("-C")
        .arg(path)
        .args(["config", "list"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "issue_prefix").then(|| value.trim().to_string())
        })
}

fn read_dir_names(path: &Path) -> Vec<String> {
    std::fs::read_dir(path)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Derive a slug from a directory name: lowercase, non-alphanumerics collapsed
/// to single hyphens.
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = true; // leading separators are dropped
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}
