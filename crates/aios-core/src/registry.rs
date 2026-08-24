//! The project registry, stored as JSON documents.
//!
//! One document per project at `~/.aios/projects/<slug>.json`, holding the
//! `Project` wire type verbatim. There is no row mapping and no join table:
//! tags live inside the document, which is both the natural model and one
//! fewer thing to keep consistent.
//!
//! The registry is tens of documents, so every query is a full scan. That is
//! not a compromise at this size — loading the whole collection costs less than
//! the syscalls consulting an index would.

use crate::detect;
use crate::error::{Error, Result};
use crate::store::DocStore;
use aios_types::{NewProject, Project, ProjectId, ProjectSummary};
use time::OffsetDateTime;

const COLLECTION: &str = "projects";

pub struct Registry {
    store: DocStore,
}

impl Registry {
    pub fn open() -> Result<Self> {
        let home = crate::config::ensure_home()?;
        Ok(Self {
            store: DocStore::new(home),
        })
    }

    /// A registry rooted at an arbitrary directory. Used by tests so they never
    /// touch the real `~/.aios`.
    pub fn at(root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            store: DocStore::new(root),
        }
    }

    /// Register a directory.
    ///
    /// Runs under the store lock: "is this slug taken? no, take it" is
    /// otherwise racy between two concurrent `aios` invocations.
    pub fn add(&self, req: NewProject) -> Result<Project> {
        // Canonicalize first, so `.`, `~`, symlinks and trailing slashes all
        // collapse to one identity — otherwise the same project could be
        // registered several times under names differing only cosmetically.
        let path =
            std::fs::canonicalize(&req.path).map_err(|_| Error::NotADirectory(req.path.clone()))?;
        if !path.is_dir() {
            return Err(Error::NotADirectory(path.display().to_string()));
        }
        let path_str = path.display().to_string();

        let dir_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".to_string());

        let slug = match req.slug {
            Some(s) => {
                let normalized = detect::slugify(&s);
                if normalized != s {
                    return Err(Error::Invalid(format!(
                        "slug {s:?} is not slug-shaped; did you mean {normalized:?}?"
                    )));
                }
                normalized
            }
            None => detect::slugify(&dir_name),
        };
        if slug.is_empty() {
            return Err(Error::Invalid(format!(
                "could not derive a slug from {dir_name:?}; pass --slug"
            )));
        }

        self.store.with_lock(|| {
            if let Some(existing) = self.all()?.into_iter().find(|p| p.path == path_str) {
                return Err(Error::PathAlreadyRegistered {
                    path: path_str.clone(),
                    slug: existing.slug,
                });
            }
            if self.store.get::<Project>(COLLECTION, &slug)?.is_some() {
                return Err(Error::SlugTaken(slug.clone()));
            }

            let d = detect::detect(&path);
            let now = OffsetDateTime::now_utc();
            let project = Project {
                id: ProjectId(ulid::Ulid::from_datetime(std::time::SystemTime::now()).to_string()),
                slug: slug.clone(),
                name: req.name.clone().unwrap_or_else(|| dir_name.clone()),
                path: path_str.clone(),
                git_remote: d.git_remote,
                default_branch: d.default_branch,
                languages: d.languages,
                package_manager: d.package_manager,
                issue_prefix: d.issue_prefix,
                tags: {
                    let mut t = req.tags.clone();
                    t.sort();
                    t.dedup();
                    t
                },
                created_at: now,
                updated_at: now,
            };
            self.store.put(COLLECTION, &project.slug, &project)?;
            Ok(project)
        })
    }

    /// Every registered project, sorted by slug.
    ///
    /// Documents are named after the slug, so filename order *is* slug order
    /// and the store's ordering carries through.
    pub fn all(&self) -> Result<Vec<Project>> {
        self.store.list::<Project>(COLLECTION)
    }

    pub fn list(&self, tag: Option<&str>) -> Result<Vec<ProjectSummary>> {
        Ok(self
            .all()?
            .into_iter()
            .filter(|p| tag.is_none_or(|t| p.tags.iter().any(|x| x == t)))
            .map(ProjectSummary::from)
            .collect())
    }

    /// Resolve by slug, then id, then path — so `aios project show .` works
    /// from inside a project without knowing its slug.
    pub fn resolve(&self, needle: &str) -> Result<Project> {
        // Only try the direct document lookup when the needle could *be* a
        // slug. A path contains separators, which the store rejects as an
        // unsafe document id — that rejection is correct, but here it means
        // "not a slug", not "invalid input", so it must not surface.
        if detect::slugify(needle) == needle
            && let Some(p) = self.store.get::<Project>(COLLECTION, needle)?
        {
            return Ok(p);
        }
        let all = self.all()?;
        if let Some(p) = all.iter().find(|p| p.id.as_str() == needle) {
            return Ok(p.clone());
        }
        if let Ok(canonical) = std::fs::canonicalize(needle) {
            let canonical = canonical.display().to_string();
            if let Some(p) = all.iter().find(|p| p.path == canonical) {
                return Ok(p.clone());
            }
        }
        Err(Error::ProjectNotFound(needle.to_string()))
    }

    pub fn remove(&self, needle: &str) -> Result<Project> {
        self.store.with_lock(|| {
            let project = self.resolve(needle)?;
            self.store.delete(COLLECTION, &project.slug)?;
            Ok(project)
        })
    }

    /// Re-run detection over a registered project and store the result.
    ///
    /// Separate from `add` because detection is cheap and drifts: branches get
    /// renamed, beads gets initialized later, a lockfile changes.
    pub fn refresh(&self, needle: &str) -> Result<Project> {
        self.store.with_lock(|| {
            let mut project = self.resolve(needle)?;
            let d = detect::detect(std::path::Path::new(&project.path));
            project.git_remote = d.git_remote;
            project.default_branch = d.default_branch;
            project.languages = d.languages;
            project.package_manager = d.package_manager;
            project.issue_prefix = d.issue_prefix;
            project.updated_at = OffsetDateTime::now_utc();
            self.store.put(COLLECTION, &project.slug, &project)?;
            Ok(project)
        })
    }

    pub fn count(&self) -> Result<usize> {
        Ok(self.all()?.len())
    }
}
