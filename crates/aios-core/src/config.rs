//! Configuration and the on-disk layout of `~/.aios`.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Everything AIOS owns lives under one directory, overridable with `AIOS_HOME`
/// so tests and throwaway instances never touch the real registry.
pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os("AIOS_HOME") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aios")
}

pub fn state_db_path() -> PathBuf {
    home().join("state.db")
}

pub fn config_path() -> PathBuf {
    home().join("config.toml")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Obsidian vault root. Its own git repo; see plan §5.
    pub vault: PathBuf,
    /// Port the daemon will listen on for non-local transports. Unused until
    /// the daemon lands in phase 4; recorded now so the file shape is stable.
    pub port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            vault: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("vault"),
            port: 7777,
        }
    }
}

impl Config {
    /// Load config, falling back to defaults when the file does not exist. A
    /// missing config is normal; a malformed one is not, and is reported with
    /// its path rather than swallowed.
    pub fn load() -> Result<Self> {
        Self::load_from(&config_path())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|source| Error::Config {
                path: path.display().to_string(),
                source,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Create `~/.aios` if absent. Mode 0700: the Unix socket and state database
/// live here, and same-machine access is authenticated by filesystem
/// permissions alone (plan §3.2).
pub fn ensure_home() -> Result<PathBuf> {
    let dir = home();
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&dir)?.permissions();
        if perms.mode() & 0o777 != 0o700 {
            perms.set_mode(0o700);
            std::fs::set_permissions(&dir, perms)?;
        }
    }
    Ok(dir)
}
