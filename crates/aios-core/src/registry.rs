//! The project registry: the thing phase 0 exists to prove.

use crate::detect;
use crate::error::{Error, Result};
use aios_types::{NewProject, Project, ProjectId, ProjectSummary};
use rusqlite::{Connection, OptionalExtension, Row, params};
use time::OffsetDateTime;

pub struct Registry {
    conn: Connection,
}

impl Registry {
    pub fn open() -> Result<Self> {
        Ok(Self {
            conn: crate::db::open()?,
        })
    }

    pub fn from_conn(conn: Connection) -> Self {
        Self { conn }
    }

    /// Register a directory.
    ///
    /// The path is canonicalized first, so `.`, `~`, symlinks and trailing
    /// slashes all collapse to one identity — otherwise the same project could
    /// be registered several times under names that only differ cosmetically.
    pub fn add(&self, req: NewProject) -> Result<Project> {
        let path =
            std::fs::canonicalize(&req.path).map_err(|_| Error::NotADirectory(req.path.clone()))?;
        if !path.is_dir() {
            return Err(Error::NotADirectory(path.display().to_string()));
        }
        let path_str = path.display().to_string();

        if let Some(existing) = self.find_by_path(&path_str)? {
            return Err(Error::PathAlreadyRegistered {
                path: path_str,
                slug: existing.slug,
            });
        }

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
        if self.find_by_slug(&slug)?.is_some() {
            return Err(Error::SlugTaken(slug));
        }

        let d = detect::detect(&path);
        let now = OffsetDateTime::now_utc();
        let project = Project {
            id: ProjectId(ulid::Ulid::from_datetime(std::time::SystemTime::now()).to_string()),
            slug,
            name: req.name.unwrap_or(dir_name),
            path: path_str,
            git_remote: d.git_remote,
            default_branch: d.default_branch,
            languages: d.languages,
            package_manager: d.package_manager,
            issue_prefix: d.issue_prefix,
            tags: {
                let mut t = req.tags;
                t.sort();
                t.dedup();
                t
            },
            created_at: now,
            updated_at: now,
        };

        self.conn.execute(
            "INSERT INTO projects (id, slug, name, path, git_remote, default_branch,
                 languages, package_manager, issue_prefix, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                project.id.as_str(),
                project.slug,
                project.name,
                project.path,
                project.git_remote,
                project.default_branch,
                serde_json::to_string(&project.languages)?,
                project.package_manager,
                project.issue_prefix,
                project.created_at,
                project.updated_at,
            ],
        )?;
        for tag in &project.tags {
            self.conn.execute(
                "INSERT INTO project_tags (project_id, tag) VALUES (?1, ?2)",
                params![project.id.as_str(), tag],
            )?;
        }
        Ok(project)
    }

    /// List projects, optionally filtered to those carrying `tag`.
    pub fn list(&self, tag: Option<&str>) -> Result<Vec<ProjectSummary>> {
        let mut out = Vec::new();
        match tag {
            Some(tag) => {
                let mut stmt = self.conn.prepare(
                    "SELECT p.* FROM projects p
                     JOIN project_tags t ON t.project_id = p.id
                     WHERE t.tag = ?1 ORDER BY p.slug",
                )?;
                let rows = stmt.query_map(params![tag], row_to_project)?;
                for r in rows {
                    out.push(self.with_tags(r?)?.into());
                }
            }
            None => {
                let mut stmt = self.conn.prepare("SELECT * FROM projects ORDER BY slug")?;
                let rows = stmt.query_map([], row_to_project)?;
                for r in rows {
                    out.push(self.with_tags(r?)?.into());
                }
            }
        }
        Ok(out)
    }

    /// Resolve by slug, then by id, then by path — so `aios project show .`
    /// works from inside a project without knowing its slug.
    pub fn resolve(&self, needle: &str) -> Result<Project> {
        if let Some(p) = self.find_by_slug(needle)? {
            return Ok(p);
        }
        if let Some(p) = self.find_by_id(needle)? {
            return Ok(p);
        }
        if let Ok(canonical) = std::fs::canonicalize(needle)
            && let Some(p) = self.find_by_path(&canonical.display().to_string())?
        {
            return Ok(p);
        }
        Err(Error::ProjectNotFound(needle.to_string()))
    }

    pub fn remove(&self, needle: &str) -> Result<Project> {
        let project = self.resolve(needle)?;
        self.conn.execute(
            "DELETE FROM projects WHERE id = ?1",
            params![project.id.as_str()],
        )?;
        Ok(project)
    }

    /// Re-run detection over a registered project and store the result. Kept
    /// separate from `add` because detection is cheap and drifts: branches get
    /// renamed, beads gets initialized later, a lockfile changes.
    pub fn refresh(&self, needle: &str) -> Result<Project> {
        let mut project = self.resolve(needle)?;
        let d = detect::detect(std::path::Path::new(&project.path));
        project.git_remote = d.git_remote;
        project.default_branch = d.default_branch;
        project.languages = d.languages;
        project.package_manager = d.package_manager;
        project.issue_prefix = d.issue_prefix;
        project.updated_at = OffsetDateTime::now_utc();
        self.conn.execute(
            "UPDATE projects SET git_remote=?2, default_branch=?3, languages=?4,
                 package_manager=?5, issue_prefix=?6, updated_at=?7 WHERE id=?1",
            params![
                project.id.as_str(),
                project.git_remote,
                project.default_branch,
                serde_json::to_string(&project.languages)?,
                project.package_manager,
                project.issue_prefix,
                project.updated_at,
            ],
        )?;
        Ok(project)
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    }

    fn find_by(&self, column: &str, value: &str) -> Result<Option<Project>> {
        let sql = format!("SELECT * FROM projects WHERE {column} = ?1");
        let found = self
            .conn
            .query_row(&sql, params![value], row_to_project)
            .optional()?;
        found.map(|p| self.with_tags(p)).transpose()
    }

    fn find_by_slug(&self, slug: &str) -> Result<Option<Project>> {
        self.find_by("slug", slug)
    }

    fn find_by_id(&self, id: &str) -> Result<Option<Project>> {
        self.find_by("id", id)
    }

    fn find_by_path(&self, path: &str) -> Result<Option<Project>> {
        self.find_by("path", path)
    }

    fn with_tags(&self, mut project: Project) -> Result<Project> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM project_tags WHERE project_id = ?1 ORDER BY tag")?;
        project.tags = stmt
            .query_map(params![project.id.as_str()], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(project)
    }
}

fn row_to_project(row: &Row<'_>) -> rusqlite::Result<Project> {
    let languages: String = row.get("languages")?;
    Ok(Project {
        id: ProjectId(row.get("id")?),
        slug: row.get("slug")?,
        name: row.get("name")?,
        path: row.get("path")?,
        git_remote: row.get("git_remote")?,
        default_branch: row.get("default_branch")?,
        languages: serde_json::from_str(&languages).unwrap_or_default(),
        package_manager: row.get("package_manager")?,
        issue_prefix: row.get("issue_prefix")?,
        tags: Vec::new(), // filled by `with_tags`
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}
