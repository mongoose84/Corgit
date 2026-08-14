//! `git status --porcelain=v2 --branch -z` (SPEC.md §8.2).
//!
//! One command yields branch, upstream, ahead/behind and every changed path, so
//! it populates the repo row and — from build step 4 — the middle pane. Step 2
//! keeps only the counts: 77 repos' worth of full file lists would eat the
//! 150 MB budget (§1) to render a single dot.

use std::path::Path;

use serde::Serialize;

use crate::git;

/// The row's whole vocabulary (§5.1): branch, one dirty dot, ahead/behind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    /// `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Short HEAD oid. `None` in a repo with no commits yet.
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    /// Unmerged paths — a merge conflict, which blocks commit and push (§13).
    pub conflicted: u32,
}

pub async fn query(repo: &Path) -> Result<RepoStatus, String> {
    let output = git::read(repo, &["status", "--porcelain=v2", "--branch", "-z"]).await?;
    if !output.ok {
        return Err(first_line(&output.stderr));
    }
    Ok(parse(&output.stdout))
}

fn first_line(stderr: &str) -> String {
    stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("git status failed")
        .trim()
        .to_string()
}

/// Records are NUL-terminated rather than NUL-separated, so the final split
/// yields an empty string; unknown record types are skipped rather than
/// treated as errors, so a future git version adding one cannot break a sweep.
pub fn parse(raw: &str) -> RepoStatus {
    let mut status = RepoStatus::default();
    let mut records = raw.split('\0');

    while let Some(record) = records.next() {
        match record.as_bytes().first() {
            Some(b'#') => header(record, &mut status),
            Some(b'1') => tally(record, &mut status),
            Some(b'2') => {
                tally(record, &mut status);
                // A rename/copy carries its original path as a second
                // NUL-terminated field. Miss this and every field afterwards
                // is read as a record type — a path beginning with "1" would
                // be counted as a change.
                records.next();
            }
            Some(b'u') => status.conflicted += 1,
            Some(b'?') => status.untracked += 1,
            _ => {}
        }
    }

    status
}

fn header(record: &str, status: &mut RepoStatus) {
    let Some((key, value)) = record.strip_prefix("# ").unwrap_or(record).split_once(' ') else {
        return;
    };

    match key {
        // Literally "(initial)" in a repo with no commits, not an oid.
        "branch.oid" => {
            status.head = (value != "(initial)")
                .then(|| value.get(..7).map(str::to_string))
                .flatten();
        }
        "branch.head" => status.branch = (value != "(detached)").then(|| value.to_string()),
        "branch.upstream" => status.upstream = Some(value.to_string()),
        // "+1 -2", always both, always in that order.
        "branch.ab" => {
            for field in value.split_whitespace() {
                match (field.split_at(1), field[1..].parse::<u32>()) {
                    (("+", _), Ok(n)) => status.ahead = n,
                    (("-", _), Ok(n)) => status.behind = n,
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// A changed entry is `<type> <XY> …`, where X is the staged state and Y the
/// unstaged one, `.` meaning unmodified. A single path can be both.
fn tally(record: &str, status: &mut RepoStatus) {
    let field = record.as_bytes();
    if field.len() < 4 {
        return;
    }
    if field[2] != b'.' {
        status.staged += 1;
    }
    if field[3] != b'.' {
        status.unstaged += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the NUL-terminated shape git actually emits.
    fn joined(records: &[&str]) -> String {
        records.iter().map(|r| format!("{r}\0")).collect()
    }

    #[test]
    fn reads_branch_upstream_and_divergence() {
        let status = parse(&joined(&[
            "# branch.oid a3f9c21ee0c1a5b8d4e7f2039182736451a9c0de",
            "# branch.head feature/retry",
            "# branch.upstream origin/feature/retry",
            "# branch.ab +3 -12",
        ]));

        assert_eq!(status.branch.as_deref(), Some("feature/retry"));
        assert_eq!(status.head.as_deref(), Some("a3f9c21"));
        assert_eq!(status.upstream.as_deref(), Some("origin/feature/retry"));
        assert_eq!((status.ahead, status.behind), (3, 12));
        assert!(!is_dirty(&status));
    }

    #[test]
    fn detached_head_has_no_branch() {
        let status = parse(&joined(&["# branch.head (detached)"]));
        assert_eq!(status.branch, None);
    }

    #[test]
    fn repo_with_no_commits_has_no_head() {
        let status = parse(&joined(&["# branch.oid (initial)", "# branch.head main"]));
        assert_eq!(status.head, None);
        assert_eq!(status.branch.as_deref(), Some("main"));
    }

    #[test]
    fn counts_staged_unstaged_and_untracked_separately() {
        let status = parse(&joined(&[
            "1 M. N... 100644 100644 100644 aaa bbb src/main.rs",
            "1 .M N... 100644 100644 100644 ccc ddd README.md",
            "1 MM N... 100644 100644 100644 eee fff src/lib.rs",
            "? notes.txt",
        ]));

        assert_eq!(status.staged, 2);
        assert_eq!(status.unstaged, 2);
        assert_eq!(status.untracked, 1);
        assert!(is_dirty(&status));
    }

    #[test]
    fn rename_original_path_is_not_read_as_a_record() {
        // The original path deliberately starts with "1" — read as a record it
        // would be counted as another change.
        let status = parse(&joined(&[
            "2 R. N... 100644 100644 100644 aaa bbb R100 src/new.rs",
            "1-old-name.rs",
            "? notes.txt",
        ]));

        assert_eq!(status.staged, 1);
        assert_eq!(status.unstaged, 0);
        assert_eq!(status.untracked, 1);
    }

    #[test]
    fn unmerged_paths_are_conflicts() {
        let status = parse(&joined(&[
            "u UU N... 100644 100644 100644 100644 aaa bbb ccc src/conflict.rs",
        ]));

        assert_eq!(status.conflicted, 1);
        assert_eq!(status.staged, 0);
    }

    #[test]
    fn clean_repo_parses_to_defaults() {
        let status = parse(&joined(&["# branch.head main", "# branch.ab +0 -0"]));
        assert_eq!(status, RepoStatus { branch: Some("main".into()), ..Default::default() });
    }

    #[test]
    fn empty_output_does_not_panic() {
        assert_eq!(parse(""), RepoStatus::default());
    }

    fn is_dirty(status: &RepoStatus) -> bool {
        status.staged + status.unstaged + status.untracked + status.conflicted > 0
    }
}
