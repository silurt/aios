//! Terminal formatting.
//!
//! Colour is suppressed when stdout is not a terminal or `NO_COLOR` is set, so
//! piped and captured output stays clean without a flag.

use std::io::IsTerminal;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal())
}

fn paint(code: &str, s: &str) -> String {
    if enabled() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn red(s: &str) -> String {
    paint("31", s)
}
pub fn green(s: &str) -> String {
    paint("32", s)
}
pub fn yellow(s: &str) -> String {
    paint("33", s)
}
pub fn bold(s: &str) -> String {
    paint("1", s)
}
pub fn dim(s: &str) -> String {
    paint("2", s)
}

/// Shorten a path for display by collapsing the home directory to `~`.
pub fn tilde(path: &str) -> String {
    match dirs_home() {
        Some(home) if path.starts_with(&home) => path.replacen(&home, "~", 1),
        _ => path.to_string(),
    }
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}
