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
    /// Run control. Behind a trait so `aios-caps` does not depend on
    /// `aios-runs`, which depends on it — the same acyclicity the ports exist
    /// to preserve.
    pub runs: Box<dyn RunControl>,
}

/// The slice of run supervision capabilities need.
pub trait RunControl: Send + Sync {
    fn interrupt(&self, id: &str) -> Result<aios_types::Run>;
}

/// Used where run control is not wired up — tests, and any surface that has no
/// business stopping a run.
pub struct NoRunControl;

impl RunControl for NoRunControl {
    fn interrupt(&self, _id: &str) -> Result<aios_types::Run> {
        Err(Error::Invalid("run control is not available here".into()))
    }
}

impl Context {
    pub fn new(config: Config, registry: Registry, ports: Ports) -> Self {
        Self {
            config,
            registry,
            ports,
            runs: Box::new(NoRunControl),
        }
    }

    /// Give this context real run control.
    ///
    /// Only the daemon does this. Everything else gets [`NoRunControl`], so a
    /// surface that has no business stopping a run cannot accidentally acquire
    /// the ability.
    pub fn with_runs(mut self, runs: Box<dyn RunControl>) -> Self {
        self.runs = runs;
        self
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
