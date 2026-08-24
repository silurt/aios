//! Storage: JSON documents and append-only JSONL logs.
//!
//! There is no embedded SQL engine. Two primitives cover everything AIOS
//! stores, and both are plain JSON on disk:
//!
//! - [`docs::DocStore`] — one JSON file per document, for configuration-shaped
//!   data that is small, read often, written rarely, and worth being able to
//!   read with `cat`: the project registry, and later agent profiles.
//! - [`log::AppendLog`] — newline-delimited JSON, for high-volume append-only
//!   streams with monotonic sequence numbers and `since` replay: run events,
//!   and later session transcripts.
//!
//! **Why not SQLite.** Three reasons, none of them fashion. The stored form
//! becomes the same `serde` type as the wire form, which removes a hand-written
//! row-mapping layer that could drift from it (§15). It drops a C dependency,
//! which the single-static-binary goal cares about. And `~/.aios` becomes
//! inspectable and editable by the agents this system exists to serve.
//!
//! **What we give up, and why it is affordable.** Multi-document transactions:
//! the daemon is the single writer by design (§3), and every operation here
//! touches one document. Referential integrity: tags live *inside* the project
//! document rather than in a join table, which is the more natural model
//! anyway. Indexed queries: the registry is tens of documents, so a full scan
//! is faster than the syscalls needed to consult an index would be.
//!
//! **What we do not give up.** Durability. Every document write goes to a
//! temporary file, is fsynced, and is renamed into place — `rename(2)` is
//! atomic on POSIX, so a reader sees either the old document or the new one,
//! never a half-written file. Appends are fsynced too, and a torn trailing line
//! from a power loss is detected and skipped on read rather than aborting the
//! whole log.

pub mod docs;
pub mod log;

pub use docs::DocStore;
pub use log::{AppendLog, Sequenced};
