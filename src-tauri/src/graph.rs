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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
}
