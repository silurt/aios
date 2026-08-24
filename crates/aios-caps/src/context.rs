//! What a capability handler is given to work with.

use crate::ports::{IssueTracker, Knowledge, Vcs};
use aios_core::{Config, Error, Registry, Result};
use aios_types::Project;
use std::path::PathBuf;

/// The adapters in use. Assembled by whoever is hosting capabilities — the CLI
/// today, the daemon from phase 4 — so `aios-caps` never depends on a concrete
/// adapter crate and the dependency graph stays acyclic.
pub struct Ports {
    pub issues: Box<dyn IssueTracker>,
    pub knowledge: Box<dyn Knowledge>,
    pub vcs: Box<dyn Vcs>,
}

pub struct Context {
    pub config: Config,
    pub registry: Registry,
    pub ports: Ports,
}

impl Context {
    pub fn new(config: Config, registry: Registry, ports: Ports) -> Self {
        Self {
            config,
            registry,
            ports,
        }
    }

    /// Resolve a project reference to its working tree.
    ///
    /// `None` means "the current directory", which is what makes
    /// `aios issue list` work from inside a repo without naming it. An
    /// unregistered directory still resolves: capabilities that only need a
    /// path should not demand registration first.
    pub fn repo_path(&self, project: Option<&str>) -> Result<PathBuf> {
        match project {
            Some(needle) => Ok(PathBuf::from(self.registry.resolve(needle)?.path)),
            None => Ok(std::env::current_dir()?),
        }
    }

    /// Resolve to a *registered* project. Used by capabilities that need the
    /// slug — knowledge scoping, for instance, where the vault directory is
    /// named after it.
    pub fn project(&self, project: Option<&str>) -> Result<Project> {
        match project {
            Some(needle) => self.registry.resolve(needle),
            None => {
                let cwd = std::env::current_dir()?;
                self.registry
                    .resolve(&cwd.display().to_string())
                    .map_err(|_| {
                        Error::ProjectNotFound(format!(
                            "{} is not registered; run `aios project add`",
                            cwd.display()
                        ))
                    })
            }
        }
    }
}
