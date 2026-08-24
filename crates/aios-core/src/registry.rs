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

/// Something wrong with a hand-edited store, described well enough to fix.
///
/// Not a wire type yet — this is diagnostic output for `aios doctor`. If it
/// ever becomes a capability it moves to `aios-types` like everything else that
/// crosses a boundary (§15).
#[derive(Debug, Clone)]
pub struct Problem {
    /// The file it concerns, relative to `~/.aios`.
    pub file: String,
    pub detail: String,
    /// What to do about it.
    pub fix: String,
}

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

    /// Resolve, returning the storage id alongside the project.
    ///
    /// The storage id is the *filename*, which is what every lookup and write
    /// must use. It normally equals `project.slug`, but these files are
    /// hand-editable and the two can disagree — writing to `project.slug`
    /// would then create a second document instead of updating this one, and
    /// deleting by it would silently miss. `validate` reports the mismatch;
    /// this makes sure it stays a report rather than corruption.
    pub fn locate(&self, needle: &str) -> Result<(String, Project)> {
        if detect::slugify(needle) == needle
            && let Some(p) = self.store.get::<Project>(COLLECTION, needle)?
        {
            return Ok((needle.to_string(), p));
        }
        let all = self.store.list_with_ids::<Project>(COLLECTION)?;
        if let Some((id, p)) = all.iter().find(|(_, p)| p.id.as_str() == needle) {
            return Ok((id.clone(), p.clone()));
        }
        if let Ok(canonical) = std::fs::canonicalize(needle) {
            let canonical = canonical.display().to_string();
            if let Some((id, p)) = all.iter().find(|(_, p)| p.path == canonical) {
                return Ok((id.clone(), p.clone()));
            }
        }
        Err(Error::ProjectNotFound(needle.to_string()))
    }

    /// Where a project's document lives on disk. These files are meant to be
    /// opened and edited, so knowing this is part of the public surface.
    pub fn document_path(&self, needle: &str) -> Result<std::path::PathBuf> {
        let (id, _) = self.locate(needle)?;
        Ok(crate::config::projects_dir().join(format!("{id}.json")))
    }

    /// Resolve by slug, then id, then path — so `aios project show .` works
    /// from inside a project without knowing its slug.
    pub fn resolve(&self, needle: &str) -> Result<Project> {
        Ok(self.locate(needle)?.1)
    }

    pub fn remove(&self, needle: &str) -> Result<Project> {
        self.store.with_lock(|| {
            let (id, project) = self.locate(needle)?;
            self.store.delete(COLLECTION, &id)?;
            Ok(project)
        })
    }

    /// Re-run detection over a registered project and store the result.
    ///
    /// Separate from `add` because detection is cheap and drifts: branches get
    /// renamed, beads gets initialized later, a lockfile changes.
    pub fn refresh(&self, needle: &str) -> Result<Project> {
        self.store.with_lock(|| {
            let (id, mut project) = self.locate(needle)?;
            let d = detect::detect(std::path::Path::new(&project.path));
            project.git_remote = d.git_remote;
            project.default_branch = d.default_branch;
            project.languages = d.languages;
            project.package_manager = d.package_manager;
            project.issue_prefix = d.issue_prefix;
            project.updated_at = OffsetDateTime::now_utc();
            self.store.put(COLLECTION, &id, &project)?;
            Ok(project)
        })
    }

    pub fn count(&self) -> Result<usize> {
        Ok(self.all()?.len())
    }

    /// Check the store for the mistakes hand-editing actually produces.
    ///
    /// Reports everything it finds rather than failing on the first problem: if
    /// you edited three files, you want all three answers, not three
    /// invocations. Read-only — nothing here changes anything on disk.
    pub fn validate(&self) -> Vec<Problem> {
        let mut problems = Vec::new();

        let projects = match self.store.list_with_ids::<Project>(COLLECTION) {
            Ok(p) => p,
            Err(e) => {
                problems.push(Problem {
                    file: format!("{COLLECTION}/"),
                    detail: e.to_string(),
                    fix: "fix the JSON, or move the file aside".into(),
                });
                return problems;
            }
        };

        let mut seen_paths: std::collections::HashMap<&str, &str> =
            std::collections::HashMap::new();

        for (filename, project) in &projects {
            let file = format!("{COLLECTION}/{filename}.json");

            // Lookups go by filename; `list` reports the field. When they
            // disagree the project answers to two different names, so say which
            // to change rather than silently picking one.
            if filename != &project.slug {
                problems.push(Problem {
                    file: file.clone(),
                    detail: format!(
                        "filename says {filename:?} but the `slug` field says {:?}",
                        project.slug
                    ),
                    fix: format!(
                        "make them match — rename the file to {}.json, or set `slug` to {filename:?}",
                        project.slug
                    ),
                });
            }

            // The slug is both the filename and a field. They must agree, and
            // guessing which one you meant would be worse than saying so.
            if detect::slugify(&project.slug) != project.slug {
                problems.push(Problem {
                    file: file.clone(),
                    detail: format!("slug {:?} is not slug-shaped", project.slug),
                    fix: format!(
                        "rename it to {:?} in both the filename and the `slug` field",
                        detect::slugify(&project.slug)
                    ),
                });
            }

            if !std::path::Path::new(&project.path).is_dir() {
                problems.push(Problem {
                    file: file.clone(),
                    detail: format!("path {} no longer exists", project.path),
                    fix: format!("update `path`, or run `aios project rm {}`", project.slug),
                });
            }

            if let Some(other) = seen_paths.insert(&project.path, &project.slug) {
                problems.push(Problem {
                    file: file.clone(),
                    detail: format!("path {} is also registered as {:?}", project.path, other),
                    fix: "remove one of them; a directory is one project".into(),
                });
            }
        }
        problems
    }
}
