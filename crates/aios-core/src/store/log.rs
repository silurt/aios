//! An append-only JSONL log with monotonic sequence numbers.
//!
//! This is the primitive a relational store would have been best at, so it is
//! the one that has to earn the decision. It backs run events (§13.2), which
//! need three things a directory of JSON files cannot give: cheap appends,
//! monotonic ordering, and `since` replay so a client that dropped its
//! connection can catch up rather than losing state.
//!
//! One line per record, `{"seq":N,"at":"…","data":{…}}`. Line-oriented storage
//! makes appending O(1) with no rewrite, makes `tail -f` work, and makes a
//! partially written trailing record — the shape a power loss leaves — a single
//! unparseable line rather than a corrupt file.

use crate::error::Result;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// A record and its position in the stream.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sequenced<T> {
    /// 1-based, monotonic, gapless within a log. This is the cursor a client
    /// sends back as `?since=`.
    pub seq: u64,
    pub at: String,
    pub data: T,
}

pub struct AppendLog {
    path: PathBuf,
}

impl AppendLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append a record, returning the sequence number it was given.
    ///
    /// The next sequence number is derived from the last valid line rather than
    /// held in memory, so two writers cannot silently diverge and a restarted
    /// process resumes correctly.
    pub fn append<T: Serialize>(&self, data: &T, at: &str) -> Result<u64> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let seq = self.last_seq()? + 1;
        let record = Sequenced {
            seq,
            at: at.to_string(),
            data,
        };
        let mut line = serde_json::to_string(&record)?;
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        // Events are the record of what an agent did to a repository. Losing
        // the tail of that to a crash is worse than the cost of the fsync.
        file.sync_data()?;
        Ok(seq)
    }

    /// Highest sequence number in the log, or 0 when it is empty or absent.
    pub fn last_seq(&self) -> Result<u64> {
        let Some(reader) = self.reader()? else {
            return Ok(0);
        };
        let mut last = 0;
        for line in reader.lines() {
            let line = line?;
            if let Some(seq) = peek_seq(&line) {
                last = last.max(seq);
            }
        }
        Ok(last)
    }

    /// Records with `seq > since`, up to `limit`.
    ///
    /// `since = 0` reads from the beginning. Unparseable lines are skipped, not
    /// fatal: the only way one occurs is a torn trailing write, and refusing to
    /// serve the entire history because the last record was cut short would
    /// turn a recoverable crash into an unusable log.
    pub fn read_since<T: DeserializeOwned>(
        &self,
        since: u64,
        limit: usize,
    ) -> Result<Vec<Sequenced<T>>> {
        let Some(reader) = self.reader()? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Sequenced<T>>(&line) {
                Ok(record) if record.seq > since => {
                    out.push(record);
                    if out.len() >= limit {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => continue,
            }
        }
        Ok(out)
    }

    fn reader(&self) -> Result<Option<BufReader<File>>> {
        match File::open(&self.path) {
            Ok(f) => Ok(Some(BufReader::new(f))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

/// Read just the `seq` field without deserializing the payload, so scanning for
/// the tail does not need to know the record type.
fn peek_seq(line: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()?
        .get("seq")?
        .as_u64()
}
