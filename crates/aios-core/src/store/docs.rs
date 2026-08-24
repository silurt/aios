//! A JSON document store: one file per document, one directory per collection.

use crate::error::{Error, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::File;
use std::path::{Path, PathBuf};

/// The envelope every document is stored in.
///
/// The payload is wrapped rather than flattened so that `schemaVersion` never
/// appears in the wire type. A stored document can then gain a migration hook
/// without `Project` growing a field that every client would see in the
/// OpenAPI spec.
#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope<T> {
    schema_version: u32,
    kind: String,
    data: T,
}

/// Bumped when a stored shape changes incompatibly. Reading a document from the
/// future is refused rather than guessed at: a newer build wrote it, and
/// silently dropping fields it added would lose data on the next write.
pub const SCHEMA_VERSION: u32 = 1;

pub struct DocStore {
    root: PathBuf,
}

impl DocStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn collection_dir(&self, collection: &str) -> PathBuf {
        self.root.join(collection)
    }

    /// Document ids become filenames, so they are constrained to a safe
    /// alphabet. A slug like `../../etc/passwd` must not be able to place a
    /// file outside the store — ids reach here from user and agent input.
    fn path(&self, collection: &str, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || id == "."
            || id == ".."
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            || id.contains("..")
        {
            return Err(Error::Invalid(format!(
                "{id:?} is not a valid document id; use [A-Za-z0-9._-]"
            )));
        }
        Ok(self.collection_dir(collection).join(format!("{id}.json")))
    }

    pub fn get<T: DeserializeOwned>(&self, collection: &str, id: &str) -> Result<Option<T>> {
        let path = self.path(collection, id)?;
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        Ok(Some(decode(&path, &text)?))
    }

    /// Every document in a collection, in filename order.
    ///
    /// A document that fails to parse aborts the read rather than being
    /// skipped: silently dropping a project from the registry because someone
    /// hand-edited it badly would be far more confusing than an error naming
    /// the file.
    pub fn list<T: DeserializeOwned>(&self, collection: &str) -> Result<Vec<T>> {
        let dir = self.collection_dir(collection);
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        paths.sort();

        paths
            .iter()
            .map(|p| decode(p, &std::fs::read_to_string(p)?))
            .collect()
    }

    /// Write a document atomically.
    ///
    /// Temp file → fsync → rename. `rename(2)` is atomic on POSIX, so a
    /// concurrent reader sees the old document or the new one and never a
    /// truncated file. Without the fsync the rename can land before the data
    /// does, which turns a power loss into an empty document.
    pub fn put<T: Serialize>(&self, collection: &str, id: &str, doc: &T) -> Result<()> {
        let path = self.path(collection, id)?;
        let dir = self.collection_dir(collection);
        std::fs::create_dir_all(&dir)?;

        let envelope = Envelope {
            schema_version: SCHEMA_VERSION,
            kind: collection.to_string(),
            data: doc,
        };
        let mut body = serde_json::to_string_pretty(&envelope)?;
        body.push('\n');

        let tmp = path.with_extension("json.tmp");
        {
            let mut file = File::create(&tmp)?;
            std::io::Write::write_all(&mut file, body.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Remove a document. Returns whether it existed.
    pub fn delete(&self, collection: &str, id: &str) -> Result<bool> {
        match std::fs::remove_file(self.path(collection, id)?) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Run `f` holding an exclusive advisory lock on the store.
    ///
    /// Check-then-write sequences — "is this slug taken? no, take it" — are
    /// otherwise racy between two concurrent `aios` invocations. The daemon
    /// will be the single writer once it exists, but the CLI can run without
    /// it, and a lock is cheaper than the class of bug it removes.
    pub fn with_lock<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        std::fs::create_dir_all(&self.root)?;
        // `File::lock` is std as of Rust 1.89 — no dependency needed for an
        // advisory exclusive lock.
        let lock = File::create(self.root.join(".lock"))?;
        lock.lock()?;
        let outcome = f();
        // Explicit unlock so the error path releases too. Dropping the file
        // would also release it, but relying on that is easy to break later.
        let _ = lock.unlock();
        outcome
    }
}

fn decode<T: DeserializeOwned>(path: &Path, text: &str) -> Result<T> {
    let envelope: Envelope<T> = serde_json::from_str(text).map_err(|e| {
        Error::Invalid(format!(
            "{} is not a valid AIOS document: {e}",
            path.display()
        ))
    })?;
    if envelope.schema_version > SCHEMA_VERSION {
        return Err(Error::Invalid(format!(
            "{} was written by a newer aios (schema {} > {}); upgrade rather than downgrade",
            path.display(),
            envelope.schema_version,
            SCHEMA_VERSION
        )));
    }
    Ok(envelope.data)
}
