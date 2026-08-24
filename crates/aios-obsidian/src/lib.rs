//! `Knowledge` backed by a directory of markdown files — i.e. an Obsidian vault.
//!
//! Nothing here talks to Obsidian the application. The vault is plain markdown
//! with YAML frontmatter and `[[wikilinks]]`, which is exactly why it was chosen
//! (plan §5): agents read and write it with ordinary file operations, and a
//! human can open the same tree in any editor.
//!
//! Search is grep-shaped rather than embedding-based, deliberately. For a
//! personal vault it is fast, exact, and needs no index to keep in sync; §5
//! says to add embeddings only if this stops being enough.

mod frontmatter;

use aios_caps::ports::Knowledge;
use aios_core::{Error, Result};
use aios_types::{Note, NoteHit, NoteRef, Scope, WriteNote};
use std::path::{Path, PathBuf};

pub struct Vault {
    root: PathBuf,
}

impl Vault {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolve a vault-relative path, refusing anything that escapes the vault.
    ///
    /// Agents supply these paths, so `../../.ssh/id_rsa` is a real input, not a
    /// hypothetical. The check is on the *lexical* path rather than the
    /// canonicalized one because the target may not exist yet on write.
    fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let rel = rel.trim_start_matches('/');
        let candidate = Path::new(rel);
        if candidate.is_absolute()
            || candidate
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(Error::Invalid(format!(
                "{rel:?} escapes the vault; paths must be vault-relative"
            )));
        }
        let mut path = self.root.join(candidate);
        if path.extension().is_none() {
            path.set_extension("md");
        }
        Ok(path)
    }

    fn rel_path(&self, abs: &Path) -> String {
        abs.strip_prefix(&self.root)
            .unwrap_or(abs)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Walk markdown files under a scope, skipping `.obsidian/` and other dot
    /// directories — vault configuration is not knowledge.
    fn walk(&self, scope: &Scope) -> Result<Vec<PathBuf>> {
        let base = match scope.subdir() {
            Some(sub) => self.root.join(sub),
            None => self.root.clone(),
        };
        let mut out = Vec::new();
        if base.is_dir() {
            collect(&base, &mut out)?;
        }
        out.sort();
        Ok(out)
    }

    fn note_ref(&self, path: &Path) -> Result<NoteRef> {
        let text = std::fs::read_to_string(path)?;
        let parsed = frontmatter::parse(&text);
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
        Ok(NoteRef {
            path: self.rel_path(path),
            title: parsed
                .title
                .or_else(|| frontmatter::first_heading(parsed.body))
                .or(stem)
                .unwrap_or_else(|| "untitled".into()),
            tags: parsed.tags,
            modified_at: modified_at(path),
        })
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
    Ok(())
}

fn modified_at(path: &Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let ts = time::OffsetDateTime::from(modified);
    ts.format(&time::format_description::well_known::Rfc3339)
        .ok()
}

impl Knowledge for Vault {
    fn backend(&self) -> &'static str {
        "obsidian"
    }

    fn available(&self) -> bool {
        self.root.is_dir()
    }

    fn list(&self, scope: &Scope) -> Result<Vec<NoteRef>> {
        if !self.available() {
            return Err(Error::NoVault(self.root.display().to_string()));
        }
        self.walk(scope)?.iter().map(|p| self.note_ref(p)).collect()
    }

    fn search(&self, scope: &Scope, query: &str, limit: usize) -> Result<Vec<NoteHit>> {
        if !self.available() {
            return Err(Error::NoVault(self.root.display().to_string()));
        }
        let needle = query.to_lowercase();
        let mut hits = Vec::new();
        for path in self.walk(scope)? {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            // First match per note only: ten hits in one file is one result to a
            // reader, and pushes more useful notes off the end of the list.
            if let Some((idx, line)) = text
                .lines()
                .enumerate()
                .find(|(_, l)| l.to_lowercase().contains(&needle))
            {
                hits.push(NoteHit {
                    meta: self.note_ref(&path)?,
                    line: idx as u32 + 1,
                    excerpt: line.trim().chars().take(200).collect(),
                });
                if hits.len() >= limit {
                    break;
                }
            }
        }
        Ok(hits)
    }

    fn read(&self, path: &str) -> Result<Note> {
        let abs = self.resolve(path)?;
        if !abs.is_file() {
            return Err(Error::NotFound {
                kind: "note",
                id: path.to_string(),
            });
        }
        let text = std::fs::read_to_string(&abs)?;
        let parsed = frontmatter::parse(&text);
        Ok(Note {
            meta: self.note_ref(&abs)?,
            links: frontmatter::wikilinks(parsed.body),
            body: parsed.body.to_string(),
        })
    }

    fn write(&self, req: &WriteNote) -> Result<Note> {
        if !self.available() {
            return Err(Error::NoVault(self.root.display().to_string()));
        }
        let abs = self.resolve(&req.path)?;
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if req.append && abs.is_file() {
            // Append below whatever is there, keeping existing frontmatter
            // intact — rewriting it would silently drop fields we do not model.
            let existing = std::fs::read_to_string(&abs)?;
            let separator = if existing.ends_with('\n') { "" } else { "\n" };
            std::fs::write(
                &abs,
                format!("{existing}{separator}\n{}\n", req.body.trim_end()),
            )?;
        } else {
            let front = frontmatter::render(req.title.as_deref(), &req.tags);
            std::fs::write(&abs, format!("{front}{}\n", req.body.trim_end()))?;
        }
        self.read(&self.rel_path(&abs))
    }
}
