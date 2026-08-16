//! `git status --porcelain=v2 --branch -z` (SPEC.md §8.2).
//!
//! One command yields branch, upstream, ahead/behind and every changed path, so
//! it populates the repo row and — from build step 4 — the middle pane. Step 2
//! keeps only the counts: 77 repos' worth of full file lists would eat the
//! 150 MB budget (§1) to render a single dot.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::git;

/// The row's whole vocabulary (§5.1): branch, one dirty dot, ahead/behind.
///
/// Also the shape persisted to the per-root cache (§9.5): a status read from
/// disk on a previous run is indistinguishable from one the sweep just
/// produced, which is what makes cache-first paint (build step 3) free.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
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

/// Whether a branch must be **published** (`push -u origin HEAD`) rather than
/// plainly pushed (§8.7).
///
/// Two states qualify. No upstream at all is the obvious one. The second is an
/// upstream whose *branch name differs from the local branch's* — `feature-x`
/// tracking `origin/main` — because git's default `push.default = simple`
/// refuses a bare `push` outright there, making it the one operation
/// guaranteed to fail, while publish succeeds and re-points the upstream.
///
/// **This duplicates `needsPublish` in `repos.svelte.ts`, and has to.** The
/// frontend decides which button to *show*; `commit_and_push` decides, in
/// Rust, which command to *run* — one button press, two decisions, on opposite
/// sides of the IPC boundary with no way to share the code. They were allowed
/// to drift once already: the Rust half checked only `upstream.is_some()`, so
/// Commit & Push ran a `push` that could not succeed on exactly the branches
/// Corgit itself had created (see `branch.rs`'s `--no-track`). Tests pin both
/// halves to the same cases; change one and change the other.
pub fn needs_publish(branch: &str, upstream: Option<&str>) -> bool {
    match upstream {
        None => true,
        Some(upstream) => upstream_branch(upstream) != branch,
    }
}

/// `origin/feature/x` → `feature/x`. Only the first segment is the remote, so
/// a branch whose own name contains a `/` survives — the same rule, and the
/// same assumption about remote names, as `branch.rs`'s `local_name`.
fn upstream_branch(upstream: &str) -> &str {
    upstream.split_once('/').map_or(upstream, |(_, rest)| rest)
}

pub async fn query(repo: &Path) -> Result<RepoStatus, String> {
    let output = git::read(repo, &["status", "--porcelain=v2", "--branch", "-z"]).await?;
    if !output.ok {
        return Err(full_message(&output.stderr));
    }
    Ok(parse(&output.stdout))
}

/// Rows capped per §5.2: at 100 the header must read `Changes (100 of 3,412)`,
/// and "stage all" still has to reach the other 3,312 — so the cap lives here,
/// on the list the UI renders, not on what a stage-all pathspec touches.
pub const MAX_FILES_PER_SECTION: usize = 100;

/// One row in the middle pane's file lists (§5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    /// Git's own status letter (M/A/D/R/C/T/U/?) for that side, shown as-is.
    pub status: char,
}

/// The selected repo's full file list — fetched on demand, never for all 77
/// repos at once, which is exactly what keeps `RepoStatus` (§1's 150 MB
/// budget) to counts alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChanges {
    pub staged: Vec<FileEntry>,
    pub staged_total: usize,
    pub unstaged: Vec<FileEntry>,
    pub unstaged_total: usize,
    pub conflicted: Vec<FileEntry>,
}

pub async fn query_files(repo: &Path) -> Result<FileChanges, String> {
    let output = git::read(repo, &["status", "--porcelain=v2", "-z"]).await?;
    if !output.ok {
        return Err(full_message(&output.stderr));
    }
    Ok(parse_files(&output.stdout))
}

fn parse_files(raw: &str) -> FileChanges {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut conflicted = Vec::new();
    let mut records = raw.split('\0');

    while let Some(record) = records.next() {
        match record.as_bytes().first() {
            Some(b'1') => changed_entry(record, 9, &mut staged, &mut unstaged),
            Some(b'2') => {
                changed_entry(record, 10, &mut staged, &mut unstaged);
                // A rename/copy carries its original path as a second
                // NUL-terminated field — see `status::tally`'s note.
                records.next();
            }
            Some(b'u') => {
                if let Some(path) = record.splitn(11, ' ').last() {
                    conflicted.push(FileEntry { path: path.to_string(), status: 'U' });
                }
            }
            Some(b'?') => {
                if let Some(path) = record.strip_prefix("? ") {
                    unstaged.push(FileEntry { path: path.to_string(), status: '?' });
                }
            }
            _ => {}
        }
    }

    FileChanges {
        staged_total: staged.len(),
        unstaged_total: unstaged.len(),
        staged: capped(staged),
        unstaged: capped(unstaged),
        conflicted,
    }
}

/// A changed record is `<type> <XY> … <path>`, X the staged state and Y the
/// unstaged one, `.` meaning unmodified — same layout `status::tally` counts,
/// just kept as paths here instead of tallies. `field_count` is how many
/// space-separated fields precede the path (9 for a plain change, 10 for a
/// rename/copy's extra similarity-score field), so a path itself containing
/// spaces still splits correctly.
fn changed_entry(record: &str, field_count: usize, staged: &mut Vec<FileEntry>, unstaged: &mut Vec<FileEntry>) {
    let bytes = record.as_bytes();
    if bytes.len() < 4 {
        return;
    }
    let Some(path) = record.splitn(field_count, ' ').last() else { return };
    let (x, y) = (bytes[2] as char, bytes[3] as char);

    if x != '.' {
        staged.push(FileEntry { path: path.to_string(), status: x });
    }
    if y != '.' {
        unstaged.push(FileEntry { path: path.to_string(), status: y });
    }
}

fn capped(mut entries: Vec<FileEntry>) -> Vec<FileEntry> {
    entries.truncate(MAX_FILES_PER_SECTION);
    entries
}

/// The whole trimmed stderr, not just its first line — §13's "raw stderr
/// always available in a collapsible Details" needs the whole thing; the
/// frontend's `translateGitError` picks a plain-language headline out of it.
fn full_message(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() { "git status failed".to_string() } else { trimmed.to_string() }
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

    /*
     * These mirror `repoStatus.test.ts`'s `needsPublish` cases one for one, on
     * purpose: the two are the same rule applied on either side of the IPC
     * boundary, and the only thing keeping them in step is that both suites
     * cover the same table. A case added on one side belongs on the other.
     */

    #[test]
    fn no_upstream_needs_publishing() {
        assert!(needs_publish("enhance-quality", None));
    }

    #[test]
    fn tracking_its_own_name_is_already_published() {
        assert!(!needs_publish("feature-x", Some("origin/feature-x")));
        assert!(!needs_publish("feature-x", Some("fork/feature-x")));
    }

    /// The state Corgit created itself until `branch.rs` grew `--no-track`,
    /// and the one that made Commit & Push run a `git push` that could not
    /// succeed. Nothing repairs an existing branch but a publish.
    #[test]
    fn tracking_a_differently_named_branch_needs_publishing() {
        assert!(needs_publish("Update_the_titlebar", Some("origin/main")));
    }

    /// Only the first segment is the remote, so a slash in the branch's own
    /// name must not read as a mismatch — `jk/thing` is a common shape, and
    /// getting it wrong would force a publish on every one of them.
    #[test]
    fn a_slash_in_the_branch_name_is_not_a_mismatch() {
        assert!(!needs_publish("jk/retry", Some("origin/jk/retry")));
    }

    #[test]
    fn a_shared_prefix_is_still_a_mismatch() {
        assert!(needs_publish("retry", Some("origin/jk/retry")));
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

    #[test]
    fn file_lists_split_by_staged_and_unstaged_side() {
        let files = parse_files(&joined(&[
            "1 M. N... 100644 100644 100644 aaa bbb src/main.rs",
            "1 .M N... 100644 100644 100644 ccc ddd README.md",
            "1 MM N... 100644 100644 100644 eee fff src/lib.rs",
            "? notes.txt",
        ]));

        assert_eq!(
            files.staged,
            vec![
                FileEntry { path: "src/main.rs".into(), status: 'M' },
                FileEntry { path: "src/lib.rs".into(), status: 'M' },
            ]
        );
        assert_eq!(
            files.unstaged,
            vec![
                FileEntry { path: "README.md".into(), status: 'M' },
                FileEntry { path: "src/lib.rs".into(), status: 'M' },
                FileEntry { path: "notes.txt".into(), status: '?' },
            ]
        );
        assert_eq!((files.staged_total, files.unstaged_total), (2, 3));
    }

    #[test]
    fn rename_reads_the_new_path_not_the_score_field() {
        let files = parse_files(&joined(&[
            "2 R. N... 100644 100644 100644 aaa bbb R100 src/new.rs",
            "1-old-name.rs",
        ]));

        assert_eq!(files.staged, vec![FileEntry { path: "src/new.rs".into(), status: 'R' }]);
    }

    #[test]
    fn unmerged_paths_are_listed_as_conflicts() {
        let files = parse_files(&joined(&[
            "u UU N... 100644 100644 100644 100644 aaa bbb ccc src/conflict.rs",
        ]));

        assert_eq!(files.conflicted, vec![FileEntry { path: "src/conflict.rs".into(), status: 'U' }]);
        assert!(files.staged.is_empty());
        assert!(files.unstaged.is_empty());
    }

    #[test]
    fn file_lists_are_capped_but_totals_are_not() {
        let records: Vec<String> = (0..150)
            .map(|n| format!("1 M. N... 100644 100644 100644 aaa bbb file-{n}.rs"))
            .collect();
        let raw: String = records.iter().map(|r| format!("{r}\0")).collect();

        let files = parse_files(&raw);

        assert_eq!(files.staged.len(), MAX_FILES_PER_SECTION);
        assert_eq!(files.staged_total, 150);
    }

    #[test]
    fn paths_containing_spaces_are_not_truncated() {
        let files = parse_files(&joined(&["1 M. N... 100644 100644 100644 aaa bbb my notes.txt"]));
        assert_eq!(files.staged, vec![FileEntry { path: "my notes.txt".into(), status: 'M' }]);
    }
}
