//! Managed regions in files AIOS does not own.
//!
//! `CLAUDE.md`, `AGENTS.md` and friends belong to the project and to whoever
//! else edits them. AIOS writes a fenced block into them and rewrites only
//! that block, so regeneration never destroys hand-written content — the
//! failure mode this exists to prevent (plan §4).

pub const BEGIN: &str = "<!-- BEGIN AIOS -->";
pub const END: &str = "<!-- END AIOS -->";

/// Replace the managed region in `existing`, appending one if absent.
///
/// Returns `None` when nothing would change, so callers can avoid rewriting a
/// file — and dirtying a git working tree — for no reason.
pub fn upsert(existing: &str, body: &str) -> Option<String> {
    let block = format!("{BEGIN}\n{}\n{END}", body.trim_end());

    let updated = match (existing.find(BEGIN), existing.find(END)) {
        (Some(start), Some(end)) if end > start => {
            let mut out = String::with_capacity(existing.len() + block.len());
            out.push_str(&existing[..start]);
            out.push_str(&block);
            out.push_str(&existing[end + END.len()..]);
            out
        }
        // A begin without a matching end means someone truncated the file or
        // edited inside our block. Appending a fresh one would leave two
        // BEGINs and make the next rewrite ambiguous, so refuse instead.
        (Some(_), _) => return None,
        _ => {
            // Land the block one blank line below whatever is already there,
            // without stacking blank lines when the file already ends in one.
            let separator = match () {
                _ if existing.trim_end().is_empty() || existing.ends_with("\n\n") => "",
                _ if existing.ends_with('\n') => "\n",
                _ => "\n\n",
            };
            format!("{existing}{separator}{block}\n")
        }
    };

    (updated != existing).then_some(updated)
}

/// Remove the managed region, if present.
pub fn remove(existing: &str) -> Option<String> {
    let (start, end) = (existing.find(BEGIN)?, existing.find(END)?);
    if end < start {
        return None;
    }
    let mut out = String::with_capacity(existing.len());
    out.push_str(existing[..start].trim_end());
    let rest = existing[end + END.len()..].trim_start_matches('\n');
    if !rest.is_empty() {
        out.push_str("\n\n");
        out.push_str(rest);
    } else {
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_when_absent_and_replaces_in_place_after() {
        let original = "# Project\n\nHand-written notes.\n";
        let once = upsert(original, "generated v1").unwrap();
        assert!(once.starts_with("# Project"));
        assert!(once.contains("Hand-written notes."));
        assert!(once.contains("generated v1"));

        let twice = upsert(&once, "generated v2").unwrap();
        assert!(
            twice.contains("Hand-written notes."),
            "hand edits must survive"
        );
        assert!(twice.contains("generated v2"));
        assert!(!twice.contains("generated v1"));
        assert_eq!(
            twice.matches(BEGIN).count(),
            1,
            "must not accumulate blocks"
        );
    }

    #[test]
    fn preserves_content_written_after_the_block() {
        let original = format!("intro\n\n{BEGIN}\nold\n{END}\n\ntrailing notes\n");
        let updated = upsert(&original, "new").unwrap();
        assert!(updated.contains("intro"));
        assert!(updated.contains("trailing notes"));
        assert!(updated.contains("new") && !updated.contains("old"));
    }

    #[test]
    fn no_change_returns_none_so_we_do_not_dirty_the_tree() {
        let once = upsert("# P\n", "same").unwrap();
        assert!(upsert(&once, "same").is_none());
    }

    #[test]
    fn refuses_a_file_with_an_unterminated_block() {
        // Better to do nothing than to leave two BEGIN markers behind.
        let broken = format!("intro\n{BEGIN}\nhalf a block\n");
        assert!(upsert(&broken, "new").is_none());
    }

    #[test]
    fn remove_takes_the_block_and_leaves_the_rest() {
        let with_block = upsert("# P\n\nkeep me\n", "generated").unwrap();
        let cleaned = remove(&with_block).unwrap();
        assert!(cleaned.contains("keep me"));
        assert!(!cleaned.contains(BEGIN) && !cleaned.contains("generated"));
    }
}
