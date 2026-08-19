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
    /// Distinct paths git reported anything about — the number the row's badge
    /// shows (§5.1). Deliberately *not* `staged + unstaged + untracked +
    /// conflicted`: git reports one record per path with a state on each side,
    /// so a file edited, staged, then edited again (`MM`) lands in both
    /// `staged` and `unstaged`. Summing those is right for "is anything going
    /// on here", which is all the dot ever asked, and wrong the moment the row
    /// prints a number — "3 files" for two files is the kind of small lie that
    /// makes someone open the repo to check. One record, one file, counted
    /// here where the records are already in front of us.
    pub changed_files: u32,
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
    Ok(parse(&read_status(repo).await?))
}

/// Both shapes from **one** `git status` (§8.2 — "this single call populates the
/// repo row *and* the middle pane", which until now it did not).
///
/// The two reads it replaces were the same command run twice: every stage,
/// unstage, discard and commit ran `emit_repo_status`'s counts read and then
/// the frontend's `repo_files` read, back to back, on the same repo. Measured
/// on the 69-repo bench root, a `git status` spawn costs 60–340 ms of which
/// 85–95 % is process creation and 2–10 ms is git reading the repository — so
/// the second spawn cost about as much as the first and learned nothing new.
///
/// Parsing the output twice rather than in one walk is deliberate. `parse` and
/// `parse_files` disagree about what a record means — one counts a path once,
/// the other files it under each side it appears on — and both are pinned by
/// their own tests. A merged walk would have to keep both rules in one body to
/// save microseconds of string scanning next to a spawn measured in tens of
/// milliseconds. The spawn was the cost; it is the one that is gone.
pub async fn query_with_files(repo: &Path) -> Result<(RepoStatus, FileChanges), String> {
    let raw = read_status(repo).await?;
    Ok((parse(&raw), parse_files(&raw)))
}

/// `--branch` is carried even when only the file lists are wanted: the header
/// records cost one line of output each and `parse_files` skips them, which is
/// cheaper than having two call sites that could drift into two commands.
async fn read_status(repo: &Path) -> Result<String, String> {
    let output = git::read(repo, &["status", "--porcelain=v2", "--branch", "-z"]).await?;
    if !output.ok {
        return Err(full_message(&output.stderr));
    }
    Ok(output.stdout)
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
    Ok(parse_files(&read_status(repo).await?))
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
            Some(b'u') => {
                status.conflicted += 1;
                status.changed_files += 1;
            }
            Some(b'?') => {
                status.untracked += 1;
                status.changed_files += 1;
            }
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
/// unstaged one, `.` meaning unmodified. A single path can be both — which is
/// why `changed_files` is incremented once here rather than derived from the
/// two sides afterwards.
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
    status.changed_files += 1;
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

    /// The case the row's badge exists to get right, and the one the per-side
    /// totals cannot express: `src/lib.rs` is staged *and* modified again, so
    /// `staged + unstaged + untracked` is 5 for four files. The badge must say
    /// four.
    #[test]
    fn a_partly_staged_file_counts_as_one_changed_file() {
        let status = parse(&joined(&[
            "1 M. N... 100644 100644 100644 aaa bbb src/main.rs",
            "1 .M N... 100644 100644 100644 ccc ddd README.md",
            "1 MM N... 100644 100644 100644 eee fff src/lib.rs",
            "? notes.txt",
        ]));

        assert_eq!(status.staged + status.unstaged + status.untracked, 5);
        assert_eq!(status.changed_files, 4);
    }

    /// Conflicts are files too. The row draws ⚠ instead of the count while any
    /// exist (§5.1), but `changed_files` is the working tree's size, not the
    /// badge's, and `is_dirty` reads it in the frontend.
    #[test]
    fn conflicts_and_untracked_files_are_counted() {
        let status = parse(&joined(&[
            "u UU N... 100644 100644 100644 100644 aaa bbb ccc src/conflict.rs",
            "? notes.txt",
        ]));

        assert_eq!(status.changed_files, 2);
    }

    /// The invariant the frontend's `isDirty` leans on: anything that makes a
    /// repo dirty by the old per-side sum also shows up as at least one
    /// changed file. Were the two able to disagree, a repo would render clean
    /// while holding uncommitted work — §5.1's one unforgivable failure.
    #[test]
    fn nothing_is_dirty_without_a_changed_file() {
        for record in [
            "1 M. N... 100644 100644 100644 aaa bbb src/main.rs",
            "1 .M N... 100644 100644 100644 aaa bbb src/main.rs",
            "1 MM N... 100644 100644 100644 aaa bbb src/main.rs",
            "u UU N... 100644 100644 100644 100644 aaa bbb ccc src/main.rs",
            "? notes.txt",
        ] {
            let status = parse(&joined(&[record]));
            assert!(is_dirty(&status), "{record}");
            assert!(status.changed_files > 0, "{record}");
        }
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
        // The rename is one file, not two: the original path is a record of
        // its own, and counting it would inflate the row's badge.
        assert_eq!(status.changed_files, 2);
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

    /// `query_with_files` hands one read to both parsers, so the badge and
    /// the pane below it are now literally the same bytes — this is what says
    /// so. `changed_files` counts a path once; the file lists file it under
    /// each side it appears on, so the union of the two lists is the set the
    /// badge counts. Were these to drift, a row would say "4 files" over a
    /// pane listing three.
    #[test]
    fn the_badge_counts_exactly_the_paths_the_pane_lists() {
        let raw = joined(&[
            "# branch.head main",
            "1 M. N... 100644 100644 100644 aaa bbb src/main.rs",
            "1 .M N... 100644 100644 100644 ccc ddd README.md",
            "1 MM N... 100644 100644 100644 eee fff src/lib.rs",
            "2 R. N... 100644 100644 100644 aaa bbb R100 src/new.rs",
            "1-old-name.rs",
            "u UU N... 100644 100644 100644 100644 aaa bbb ccc src/conflict.rs",
            "? notes.txt",
        ]);

        let status = parse(&raw);
        let files = parse_files(&raw);

        let listed: std::collections::HashSet<&str> = files
            .staged
            .iter()
            .chain(&files.unstaged)
            .chain(&files.conflicted)
            .map(|entry| entry.path.as_str())
            .collect();

        assert_eq!(status.changed_files as usize, listed.len());
    }

    /// The header records `parse_files` has to walk past now that both
    /// parsers read one `--branch` invocation. A `#` line read as a change
    /// would put "branch.head main" in the middle pane.
    #[test]
    fn branch_headers_are_not_read_as_files() {
        let files = parse_files(&joined(&[
            "# branch.oid a3f9c21ee0c1a5b8d4e7f2039182736451a9c0de",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +3 -12",
            "? notes.txt",
        ]));

        assert_eq!(files.unstaged, vec![FileEntry { path: "notes.txt".into(), status: '?' }]);
        assert!(files.staged.is_empty());
    }

    #[test]
    fn paths_containing_spaces_are_not_truncated() {
        let files = parse_files(&joined(&["1 M. N... 100644 100644 100644 aaa bbb my notes.txt"]));
        assert_eq!(files.staged, vec![FileEntry { path: "my notes.txt".into(), status: 'M' }]);
    }
}
