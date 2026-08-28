//! `Vcs` backed by the `git` binary.
//!
//! Shelling out rather than linking libgit2: it keeps the single-static-binary
//! goal intact (no C dependency), and it guarantees we agree with whatever git
//! the user actually has, including their config and credential helpers.

use aios_caps::ports::Vcs;
use aios_core::{Error, Result};
use aios_types::{Commit, Diff, FileChange, RepoStatus};
use std::path::Path;
use std::process::Command;

pub struct Git;

impl Git {
    pub fn new() -> Self {
        Self
    }

    fn run(&self, repo: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => Error::ToolMissing { tool: "git".into() },
                _ => Error::Io(e),
            })?;
        if !output.status.success() {
            return Err(Error::ToolFailed {
                tool: "git".into(),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl Default for Git {
    fn default() -> Self {
        Self::new()
    }
}

impl Vcs for Git {
    fn backend(&self) -> &'static str {
        "git"
    }

    fn status(&self, repo: &Path) -> Result<RepoStatus> {
        // `-b` gives the branch and upstream tracking line; `--porcelain=v1` is
        // a stable machine format, unlike the human output.
        let raw = self.run(
            repo,
            &["status", "--porcelain=v1", "-b", "--untracked-files=normal"],
        )?;
        let mut branch = None;
        let mut ahead = None;
        let mut behind = None;
        let (mut staged, mut unstaged, mut untracked) = (0u32, 0u32, 0u32);
        let mut changed_files = Vec::new();

        for line in raw.lines() {
            if let Some(header) = line.strip_prefix("## ") {
                // Forms: `main`, `main...origin/main`, `main...origin/main [ahead 1, behind 2]`
                let name = header
                    .split("...")
                    .next()
                    .unwrap_or(header)
                    .split(' ')
                    .next()
                    .unwrap_or(header);
                branch = (!name.is_empty() && name != "HEAD").then(|| name.to_string());
                if let Some(tracking) = header.split_once('[').map(|(_, t)| t.trim_end_matches(']'))
                {
                    for part in tracking.split(", ") {
                        if let Some(n) = part.strip_prefix("ahead ") {
                            ahead = n.parse().ok();
                        } else if let Some(n) = part.strip_prefix("behind ") {
                            behind = n.parse().ok();
                        }
                    }
                }
                continue;
            }
            if line.len() < 4 {
                continue;
            }
            let code = &line[..2];
            let path = line[3..].to_string();
            match code {
                "??" => untracked += 1,
                _ => {
                    // Column 1 is the index, column 2 the working tree; a file
                    // can be both, which is why these are not exclusive.
                    if !code.starts_with(' ') {
                        staged += 1;
                    }
                    if !code.ends_with(' ') {
                        unstaged += 1;
                    }
                }
            }
            changed_files.push(FileChange {
                path,
                status: code.trim().to_string(),
            });
        }

        Ok(RepoStatus {
            branch,
            clean: changed_files.is_empty(),
            changed_files,
            staged,
            unstaged,
            untracked,
            ahead,
            behind,
        })
    }

    fn log(&self, repo: &Path, limit: usize) -> Result<Vec<Commit>> {
        // Unit separator between fields and record separator between commits, so
        // a subject containing tabs or pipes cannot corrupt the parse.
        let format = "--pretty=format:%H\x1f%s\x1f%an\x1f%aI\x1e";
        let raw = self.run(repo, &["log", &format!("-{limit}"), format])?;
        Ok(raw
            .split('\x1e')
            .filter(|r| !r.trim().is_empty())
            .filter_map(|record| {
                let mut f = record.trim_start_matches('\n').split('\x1f');
                Some(Commit {
                    sha: f.next()?.to_string(),
                    subject: f.next()?.to_string(),
                    author: f.next()?.to_string(),
                    date: f.next()?.to_string(),
                })
            })
            .collect())
    }

    fn diff(
        &self,
        repo: &Path,
        base: Option<&str>,
        staged: bool,
        max_bytes: usize,
    ) -> Result<Diff> {
        let (against, range): (String, Vec<&str>) = match base {
            Some(base) => (base.to_string(), vec![base]),
            // No base: the working tree. `--staged` alone would *exclude*
            // unstaged edits, which is the opposite of what someone reviewing
            // "what did the agent change" wants, so HEAD is the comparison and
            // staged decides whether the index is included.
            None if staged => ("HEAD".to_string(), vec!["HEAD"]),
            None => ("working tree".to_string(), Vec::new()),
        };

        let mut args = vec!["diff"];
        args.extend(range.iter().copied());
        let patch = self.run(repo, &args)?;

        let mut name_args = vec!["diff", "--name-only"];
        name_args.extend(range.iter().copied());
        let files = self
            .run(repo, &name_args)?
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_owned)
            .collect();

        let truncated = patch.len() > max_bytes;
        let patch = if truncated {
            // Cut on a char boundary; a patch sliced mid-codepoint would not be
            // valid UTF-8 and would fail to serialize.
            let mut end = max_bytes;
            while end > 0 && !patch.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}\n… truncated", &patch[..end])
        } else {
            patch
        };

        Ok(Diff {
            against,
            patch,
            files,
            truncated,
        })
    }
}
