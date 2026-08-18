//! Commit graph (SPEC.md §5.3, §8.4).
//!
//! Lane layout is deliberately *not* done here — this module only fetches and
//! parses `git log`/`for-each-ref` output. Turning that into lanes and SVG
//! paths is in-house frontend work (§5.3: "do not parse `git log --graph`
//! ASCII output"), and belongs beside the rendering it feeds.

use std::path::Path;

use serde::Serialize;

use crate::git;

/// One page of history (§5.3: "Loads 300 commits at a time").
pub const PAGE_SIZE: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub hash: String,
    /// Full parent hashes, in order — drives lane layout client-side. Empty
    /// for a root commit.
    pub parents: Vec<String>,
    /// `%ct`, Unix seconds. Rendered client-side as `dd-MM-yyyy HH:mm:ss` in
    /// local time (§5.3) — never formatted here, since the frontend owns the
    /// fixed, non-locale-dependent format string.
    pub timestamp: i64,
    pub author: String,
    pub subject: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphPage {
    pub commits: Vec<Commit>,
    /// A full page came back. Good enough for the "Load more" row (§5.3): the
    /// next request may still find nothing, but that is cheap to discover and
    /// simpler than a second `git rev-list --count` just to know for certain.
    pub has_more: bool,
}

/// `git log --all --date-order -z -n 300 --skip=<offset> --format=…` (§8.4).
///
/// `--all` includes `HEAD` itself as well as every ref under `refs/`, so the
/// current branch's tip is always reachable even from a detached HEAD. An
/// empty repository (no commits yet) is not an error here: git prints nothing
/// and exits 0, since `--all` simply expands to no refs.
pub async fn log(repo: &Path, skip: usize) -> Result<GraphPage, String> {
    let skip_arg = skip.to_string();
    let n_arg = PAGE_SIZE.to_string();
    let output = git::read(
        repo,
        &[
            "log",
            "--all",
            "--date-order",
            "-z",
            "-n",
            &n_arg,
            "--skip",
            &skip_arg,
            "--format=%H%x1f%P%x1f%ct%x1f%an%x1f%s",
        ],
    )
    .await?;

    if !output.ok {
        return Err(first_line(&output.stderr));
    }

    let commits = parse_log(&output.stdout);
    let has_more = commits.len() == PAGE_SIZE;
    Ok(GraphPage { commits, has_more })
}

/// Records are NUL-terminated (`-z`), so the final split yields an empty
/// string — same shape as `status::parse` (§8.2).
fn parse_log(raw: &str) -> Vec<Commit> {
    raw.split('\0').filter_map(parse_commit).collect()
}

fn parse_commit(record: &str) -> Option<Commit> {
    if record.is_empty() {
        return None;
    }
    let mut fields = record.splitn(5, '\u{1f}');
    let hash = fields.next()?.to_string();
    let parents = fields.next()?.split_whitespace().map(str::to_string).collect();
    let timestamp = fields.next()?.parse().ok()?;
    let author = fields.next()?.to_string();
    let subject = fields.next().unwrap_or("").to_string();
    Some(Commit { hash, parents, timestamp, author, subject })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefKind {
    Local,
    Remote,
}

/// One ref badge (§5.3: "Ref badges come from `for-each-ref` (§8.3), not
/// `%d`"). `commit` is the full hash it points at, matched against `Commit::hash`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefBadge {
    pub name: String,
    pub commit: String,
    pub kind: RefKind,
}

/// `git for-each-ref` over `refs/heads` and `refs/remotes` (§8.3) — the same
/// two namespaces the branch switcher will use in build step 8. Tags are out
/// of scope for v1 (§2).
pub async fn refs(repo: &Path) -> Result<Vec<RefBadge>, String> {
    let output = git::read(
        repo,
        &["for-each-ref", "--format=%(refname)%1f%(objectname)", "refs/heads", "refs/remotes"],
    )
    .await?;

    if !output.ok {
        return Err(first_line(&output.stderr));
    }

    Ok(parse_refs(&output.stdout))
}

fn parse_refs(raw: &str) -> Vec<RefBadge> {
    raw.lines().filter_map(parse_ref).collect()
}

fn parse_ref(line: &str) -> Option<RefBadge> {
    let (refname, commit) = line.split_once('\u{1f}')?;

    // clippy::question_mark wants the last arm folded into a `?` on the
    // `refs/remotes/` prefix. That would work, but it breaks the symmetry the
    // three arms are readable *because of*: two namespaces map to their kind,
    // and anything else — a tag, a note, `refs/stash` — is not a badge (§2).
    // Kept as a dispatch, silenced deliberately rather than by loosening the
    // lint level for the whole crate.
    #[allow(clippy::question_mark)]
    let (short, kind) = if let Some(name) = refname.strip_prefix("refs/heads/") {
        (name, RefKind::Local)
    } else if let Some(name) = refname.strip_prefix("refs/remotes/") {
        (name, RefKind::Remote)
    } else {
        return None;
    };

    // `refs/remotes/<remote>/HEAD` is a symbolic ref pointing at that
    // remote's default branch, not a branch of its own — it would otherwise
    // badge every commit twice.
    if short.rsplit('/').next() == Some("HEAD") {
        return None;
    }

    Some(RefBadge { name: short.to_string(), commit: commit.to_string(), kind })
}

/// The middle pane's Mode B (§5.2, §8.5) — read-only, so unlike `FileChanges`
/// (§5.2's 100-entry cap) the file list here is never capped: a single
/// commit's diff is bounded by what that commit actually touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDetails {
    pub hash: String,
    pub author: String,
    pub email: String,
    /// `%ct`, Unix seconds — rendered client-side the same way `Commit::timestamp` is (§5.3).
    pub timestamp: i64,
    /// Full raw message (`%B`: subject + body) — Mode B shows it verbatim
    /// rather than re-deriving a subject line from the graph row.
    pub message: String,
    pub files: Vec<CommitFileEntry>,
}

/// One changed file with its line-change stats — GitHub-style per-file +/−
/// alongside the usual status letter. `insertions`/`deletions` are `None` for
/// a binary file, where git reports `-` instead of a count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitFileEntry {
    pub path: String,
    pub status: char,
    pub insertions: Option<u32>,
    pub deletions: Option<u32>,
}

/// `git diff-tree --no-commit-id -m --first-parent --root --raw --numstat -r -z <hash>`
/// plus `git show -s --format=… <hash>` (§8.5), run concurrently since neither
/// depends on the other's result. `--raw` and `--numstat` combine into a
/// single call (unlike `--name-status`, which git treats as exclusive with
/// `--numstat`) — the status letter from one block, the +/− counts from the
/// other.
///
/// **`-m --first-parent` and `--root` are what make merges and the root commit
/// show anything at all.** Bare `diff-tree` prints *nothing* for either: a
/// merge because git will not pick a parent to diff against on its own, a root
/// commit because it has no parent to diff against. The graph feeds this from
/// a plain `git log --all` (no `--no-merges`), so both are selectable rows, and
/// without these flags every merge in a PR-merging repo opened a details pane
/// reading "No files changed" — the panel confidently lying about history it
/// had read correctly.
///
/// `--cc` is the other way to make a merge non-empty and is deliberately *not*
/// used: on a merge that resolved trivially it emits the numstat block with no
/// `--raw` block at all, and `parse_raw_and_numstat` zips the two by position,
/// so the file list would come back empty again — the same bug wearing a
/// different flag. `-m --first-parent` also answers the question the pane is
/// actually asking: "what did this merge bring in", not "how was it resolved".
pub async fn details(repo: &Path, hash: &str) -> Result<CommitDetails, String> {
    let files_args = diff_tree_args(hash);
    let show_args = ["show", "-s", "--format=%H%x1f%an%x1f%ae%x1f%ct%x1f%B", hash];
    let (files_result, meta_result) =
        tokio::join!(git::read(repo, &files_args), git::read(repo, &show_args));

    let files_output = files_result?;
    if !files_output.ok {
        return Err(first_line(&files_output.stderr));
    }
    let meta_output = meta_result?;
    if !meta_output.ok {
        return Err(first_line(&meta_output.stderr));
    }

    let files = parse_raw_and_numstat(&files_output.stdout);
    parse_show(&meta_output.stdout, files)
}

/// Its own function, and tested, for the same reason `branch::create_args` is:
/// the flags *are* the behaviour here, and three of them look droppable to
/// anyone who only ever selects an ordinary commit while checking.
fn diff_tree_args(hash: &str) -> [&str; 10] {
    [
        "diff-tree",
        "--no-commit-id",
        // Split a merge into one diff per parent, then keep only the first —
        // together they are one diff, not N. An octopus merge would otherwise
        // repeat every path once per parent.
        "-m",
        "--first-parent",
        "--root",
        "--raw",
        "--numstat",
        "-r",
        "-z",
        hash,
    ]
}

/// `--raw --numstat -r -z` prints two format blocks back to back, describing
/// the same changed files in the same order: a `:mode mode sha sha status`
/// header (status is its last whitespace-separated field, with a trailing
/// similarity score for `R`/`C`) followed by a path, then — once the raw
/// block runs out (the next token stops starting with `:`) — an
/// `added\tdeleted\tpath` record per file, in that same order. Zipped by
/// position rather than by path, which sidesteps matching rename pairs
/// across the two blocks entirely.
fn parse_raw_and_numstat(raw: &str) -> Vec<CommitFileEntry> {
    let mut tokens = raw.split('\0').filter(|token| !token.is_empty()).peekable();

    let mut entries: Vec<(char, String)> = Vec::new();
    while let Some(&token) = tokens.peek() {
        if !token.starts_with(':') {
            break;
        }
        tokens.next();
        let Some(status) = token.split_whitespace().last().and_then(|s| s.chars().next()) else {
            continue;
        };
        if status == 'R' || status == 'C' {
            let _old_path = tokens.next();
            if let Some(new_path) = tokens.next() {
                entries.push((status, new_path.to_string()));
            }
        } else if let Some(path) = tokens.next() {
            entries.push((status, path.to_string()));
        }
    }

    let mut stats: Vec<(Option<u32>, Option<u32>)> = Vec::new();
    while let Some(token) = tokens.next() {
        let mut fields = token.splitn(3, '\t');
        let added = fields.next().unwrap_or("");
        let deleted = fields.next().unwrap_or("");
        let path_field = fields.next().unwrap_or("");
        if path_field.is_empty() {
            // A rename/copy defers its path the same way the raw block does:
            // two more NUL-terminated tokens follow. Already reflected in
            // `entries`, so just consumed here to stay in sync.
            tokens.next();
            tokens.next();
        }
        stats.push((added.parse().ok(), deleted.parse().ok()));
    }

    entries
        .into_iter()
        .zip(stats)
        .map(|((status, path), (insertions, deletions))| CommitFileEntry {
            path,
            status,
            insertions,
            deletions,
        })
        .collect()
}

/// `%H%x1f%an%x1f%ae%x1f%ct%x1f%B` — `%B` is last and unbounded (it can
/// contain its own newlines), so it takes everything `splitn` leaves over.
fn parse_show(raw: &str, files: Vec<CommitFileEntry>) -> Result<CommitDetails, String> {
    let mut fields = raw.splitn(5, '\u{1f}');
    let hash = fields.next().unwrap_or("").to_string();
    if hash.is_empty() {
        return Err("git show returned no output".to_string());
    }
    let author = fields.next().unwrap_or("").to_string();
    let email = fields.next().unwrap_or("").to_string();
    let timestamp = fields.next().unwrap_or("").trim().parse().unwrap_or(0);
    // Trailing newline `%B` always ends with, trimmed so the pane doesn't
    // render one extra blank line under the message.
    let message = fields.next().unwrap_or("").trim_end().to_string();

    Ok(CommitDetails { hash, author, email, timestamp, message, files })
}

fn first_line(stderr: &str) -> String {
    stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("git log failed")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(records: &[&str]) -> String {
        records.iter().map(|r| format!("{r}\0")).collect()
    }

    #[test]
    fn parses_a_normal_commit() {
        let commits = parse_log(&joined(&[
            "a3f9c21ee0c1a5b8d4e7f2039182736451a9c0de\u{1f}bc0debc0debc0debc0debc0debc0debc0debc0de\u{1f}1786744977\u{1f}Jeppe\u{1f}feat: add retry logic",
        ]));

        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "a3f9c21ee0c1a5b8d4e7f2039182736451a9c0de");
        assert_eq!(commits[0].parents, vec!["bc0debc0debc0debc0debc0debc0debc0debc0de".to_string()]);
        assert_eq!(commits[0].timestamp, 1786744977);
        assert_eq!(commits[0].author, "Jeppe");
        assert_eq!(commits[0].subject, "feat: add retry logic");
    }

    #[test]
    fn root_commit_has_no_parents() {
        let commits = parse_log(&joined(&[
            "a3f9c21ee0c1a5b8d4e7f2039182736451a9c0de\u{1f}\u{1f}1786744977\u{1f}Jeppe\u{1f}initial commit",
        ]));

        assert!(commits[0].parents.is_empty());
    }

    #[test]
    fn merge_commit_has_two_parents() {
        let commits = parse_log(&joined(&[
            "a3f9c21ee0c1a5b8d4e7f2039182736451a9c0de\u{1f}bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ccccccccccccccccccccccccccccccccccccccc\u{1f}1786744977\u{1f}Jeppe\u{1f}Merge branch 'feature'",
        ]));

        assert_eq!(commits[0].parents.len(), 2);
    }

    #[test]
    fn a_subject_containing_the_field_separator_is_not_possible_but_missing_trailing_fields_do_not_panic() {
        assert!(parse_log("").is_empty());
        assert!(parse_commit("not-enough-fields").is_none());
    }

    #[test]
    fn empty_output_yields_no_commits() {
        assert!(parse_log("").is_empty());
    }

    #[test]
    fn a_full_page_signals_there_is_more() {
        let records: Vec<String> = (0..PAGE_SIZE)
            .map(|n| format!("hash{n}\u{1f}\u{1f}1700000000\u{1f}Jeppe\u{1f}commit {n}"))
            .collect();
        let raw: String = records.iter().map(|r| format!("{r}\0")).collect();

        let commits = parse_log(&raw);
        assert_eq!(commits.len(), PAGE_SIZE);
    }

    #[test]
    fn reads_local_and_remote_branches() {
        let refs = parse_refs(
            "refs/heads/main\u{1f}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
             refs/remotes/origin/main\u{1f}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
        );

        assert_eq!(
            refs,
            vec![
                RefBadge {
                    name: "main".into(),
                    commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                    kind: RefKind::Local,
                },
                RefBadge {
                    name: "origin/main".into(),
                    commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                    kind: RefKind::Remote,
                },
            ]
        );
    }

    #[test]
    fn a_remote_head_symbolic_ref_is_skipped() {
        let refs = parse_refs(
            "refs/remotes/origin/HEAD\u{1f}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
        );
        assert!(refs.is_empty());
    }

    #[test]
    fn empty_ref_output_is_not_an_error() {
        assert!(parse_refs("").is_empty());
    }

    fn nul_joined(tokens: &[&str]) -> String {
        tokens.iter().map(|t| format!("{t}\0")).collect()
    }

    #[test]
    fn raw_and_numstat_zip_by_position() {
        let files = parse_raw_and_numstat(&nul_joined(&[
            ":100644 100644 aaaaaaa bbbbbbb M",
            "src/main.rs",
            ":000000 100644 0000000 ccccccc A",
            "src/retry.rs",
            "5\t0\tsrc/main.rs",
            "12\t0\tsrc/retry.rs",
        ]));

        assert_eq!(
            files,
            vec![
                CommitFileEntry {
                    path: "src/main.rs".into(),
                    status: 'M',
                    insertions: Some(5),
                    deletions: Some(0),
                },
                CommitFileEntry {
                    path: "src/retry.rs".into(),
                    status: 'A',
                    insertions: Some(12),
                    deletions: Some(0),
                },
            ]
        );
    }

    #[test]
    fn raw_and_numstat_rename_reads_the_new_path_and_stays_in_sync() {
        let files = parse_raw_and_numstat(&nul_joined(&[
            ":100644 100644 aaaaaaa bbbbbbb R100",
            "src/old.rs",
            "src/new.rs",
            "10\t2\t",
            "src/old.rs",
            "src/new.rs",
        ]));

        assert_eq!(
            files,
            vec![CommitFileEntry {
                path: "src/new.rs".into(),
                status: 'R',
                insertions: Some(10),
                deletions: Some(2),
            }]
        );
    }

    #[test]
    fn raw_and_numstat_binary_file_has_no_counts() {
        let files = parse_raw_and_numstat(&nul_joined(&[
            ":100644 100644 aaaaaaa bbbbbbb M",
            "image.png",
            "-\t-\timage.png",
        ]));

        assert_eq!(files, vec![CommitFileEntry { path: "image.png".into(), status: 'M', insertions: None, deletions: None }]);
    }

    #[test]
    fn raw_and_numstat_empty_output_is_not_an_error() {
        assert!(parse_raw_and_numstat("").is_empty());
    }

    #[test]
    fn show_parses_metadata_and_a_multiline_message() {
        let raw = "a3f9c21ee0c1a5b8d4e7f2039182736451a9c0de\u{1f}Jeppe Kronborg\u{1f}jeppe@example.com\u{1f}1786744977\u{1f}feat: add retry logic\n\nLonger body here.\n";
        let details = parse_show(raw, vec![]).unwrap();

        assert_eq!(details.hash, "a3f9c21ee0c1a5b8d4e7f2039182736451a9c0de");
        assert_eq!(details.author, "Jeppe Kronborg");
        assert_eq!(details.email, "jeppe@example.com");
        assert_eq!(details.timestamp, 1786744977);
        assert_eq!(details.message, "feat: add retry logic\n\nLonger body here.");
    }

    #[test]
    fn show_with_no_output_is_an_error() {
        assert!(parse_show("", vec![]).is_err());
    }

    /// Guards the three flags that make a merge and a root commit list their
    /// files instead of coming back empty. Bare `diff-tree` prints nothing for
    /// either, and the graph (`git log --all`, no `--no-merges`) makes both
    /// selectable rows — so dropping any of these silently returns the pane to
    /// reading "No files changed" over history it read fine.
    #[test]
    fn diff_tree_covers_merges_and_the_root_commit() {
        let args = diff_tree_args("a3f9c21");

        assert!(args.contains(&"-m"), "a merge diffs against no parent without -m");
        assert!(args.contains(&"--first-parent"), "-m alone repeats every path once per parent");
        assert!(args.contains(&"--root"), "the root commit has no parent to diff against");
        // `--cc` would make merges non-empty too, but emits no `--raw` block on
        // a trivially-resolved merge, and `parse_raw_and_numstat` zips the two
        // blocks by position — so it lands back on an empty file list.
        assert!(!args.contains(&"--cc"), "--cc breaks the raw/numstat positional zip");
        assert_eq!(args.last(), Some(&"a3f9c21"), "the revision stays last");
    }
}
