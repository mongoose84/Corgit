//! Staging and commit (SPEC.md §8.6).
//!
//! Every function here is a write. Acquiring that repo's write-queue lock
//! (§7) is the caller's job, in `lib.rs` — kept out of this module so the
//! lock's scope stays visible at the call site instead of hidden in here.

use std::path::Path;

use crate::git;

pub async fn stage(repo: &Path, paths: &[String]) -> Result<(), String> {
    run_pathspec(repo, "add", &["--"], paths).await
}

pub async fn unstage(repo: &Path, paths: &[String]) -> Result<(), String> {
    run_pathspec(repo, "restore", &["--staged", "--"], paths).await
}

/// Discard the *unstaged* changes to these paths (§8.6): restore the working
/// tree from the index, leaving whatever is staged for them untouched.
///
/// The flags are the entire safety property of this function, which is why
/// they are a named constant with a test on them. `--worktree` alone takes its
/// source from the index; adding `--staged` silently moves that source to HEAD
/// and takes the staged work with it — unrecoverable, and reported as success,
/// which is the same class of failure §8.3 refuses force-checkout for.
///
/// Untracked paths cannot be discarded and must not be passed: git has nothing
/// to restore them from, so the only way to remove one would be `git clean`
/// deleting it outright. It rejects an unmatched pathspec by failing the whole
/// invocation rather than skipping that path, so a single untracked entry
/// would abandon the entire discard — nothing is half-done, which is the
/// failure mode to want, but the caller is still what keeps them out (§5.2).
pub async fn discard(repo: &Path, paths: &[String]) -> Result<(), String> {
    run_pathspec(repo, "restore", DISCARD_FLAGS, paths).await
}

/// Named so the "working tree only" rule above is something a test can hold
/// onto — the failure it guards against destroys work and reports success.
const DISCARD_FLAGS: &[&str] = &["--worktree", "--"];

/// Delete these *untracked* paths from the working tree (§5.2, §8.6).
///
/// `git clean`, never `std::fs::remove_file`, and that is this function's
/// safety property rather than a stylistic preference: clean removes only what
/// git considers untracked, so a tracked path arriving here through a frontend
/// bug is skipped instead of unlinked. With `remove_file` the pane's filter
/// would be the only thing that ever stood between a bug and someone's tracked
/// work.
///
/// The flags are the rest of it, hence a named constant with a test on them,
/// the same as [`DISCARD_FLAGS`]:
///
/// - `-d` is absent because Corgit never shows a folder row (§5.2), so there is
///   never a directory here to recurse into.
/// - `-x` and `-X` are absent because an ignored file is not what any row in
///   this pane represents. Either one turns a two-row selection into a sweep
///   that can take `node_modules`, `target` and every build artefact on the
///   disk with it — the accident this list exists to make unreachable.
///
/// Unlike [`discard`], an unmatched pathspec is not fatal here: `git clean`
/// skips what it cannot find rather than failing the whole invocation, so a
/// file already gone by the time the user confirms is a no-op rather than an
/// error on the rest of the batch.
pub async fn delete_untracked(repo: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let specs: Vec<String> = paths.iter().map(|path| literal_pathspec(path)).collect();
    let mut args: Vec<&str> = Vec::with_capacity(1 + CLEAN_FLAGS.len() + specs.len());
    args.push("clean");
    args.extend_from_slice(CLEAN_FLAGS);
    args.extend(specs.iter().map(String::as_str));
    run(repo, &args).await
}

/// See [`delete_untracked`] for why each absent flag is absent. `--force`
/// is required for `clean` to do anything at all unless the repo sets
/// `clean.requireForce=false`, so it is not optional here.
const CLEAN_FLAGS: &[&str] = &["--force", "--"];

/// Git reads a pathspec as a **glob**, so a file honestly named `report[1].txt`
/// or `draft*.md` is a *pattern* that can match files nobody picked. For a
/// delete that is the difference between removing the one row the user
/// confirmed and removing everything sitting beside it, so each path is wrapped
/// in `:(literal)`, which switches off pathspec magic and globbing for that
/// entry alone.
///
/// It also neutralises a leading `:`, which would otherwise make the path
/// itself read as pathspec magic and fail — or worse, parse as some other
/// directive entirely.
fn literal_pathspec(path: &str) -> String {
    format!(":(literal){path}")
}

/// "Stage all" must reach files hidden by the middle pane's 100-entry cap
/// (§5.2), so it stages the whole tree rather than a path list the frontend
/// gathered from what it could see.
pub async fn stage_all(repo: &Path) -> Result<(), String> {
    run(repo, &["add", "--all"]).await
}

pub async fn unstage_all(repo: &Path) -> Result<(), String> {
    run(repo, &["restore", "--staged", "--", "."]).await
}

/// Message via stdin, not an argument — avoids arg-escaping pain for
/// arbitrary commit messages (§8.6).
pub async fn commit(repo: &Path, message: &str) -> Result<(), String> {
    let output = git::write_stdin(repo, &["commit", "-F", "-"], message).await?;
    if !output.ok {
        return Err(full_message(&output.stderr));
    }
    Ok(())
}

async fn run_pathspec(
    repo: &Path,
    subcommand: &str,
    flags: &[&str],
    paths: &[String],
) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = Vec::with_capacity(1 + flags.len() + paths.len());
    args.push(subcommand);
    args.extend_from_slice(flags);
    args.extend(paths.iter().map(String::as_str));
    run(repo, &args).await
}

async fn run(repo: &Path, args: &[&str]) -> Result<(), String> {
    let output = git::write(repo, args).await?;
    if !output.ok {
        return Err(full_message(&output.stderr));
    }
    Ok(())
}

/// The whole trimmed stderr, not just its first line — §13's "raw stderr
/// always available in a collapsible Details" needs the whole thing; the
/// frontend's `translateGitError` picks a plain-language headline out of it.
fn full_message(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() { "git failed".to_string() } else { trimmed.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of [`DISCARD_FLAGS`], and the reason it is a constant:
    /// `--worktree` restores from the *index*, so a partly-staged file keeps
    /// its staged half. `--staged --worktree` would restore from HEAD instead
    /// and destroy that half too — a one-word edit, no visible difference in
    /// the UI, and no way to get the work back.
    #[test]
    fn discard_restores_the_worktree_from_the_index_only() {
        assert_eq!(DISCARD_FLAGS, ["--worktree", "--"]);
        assert!(
            !DISCARD_FLAGS.contains(&"--staged"),
            "discard must never reach the index: that restores from HEAD and takes staged work"
        );
    }

    /// A `restore` with no paths would be a no-op at best; `git restore` with
    /// only `--` and nothing after it errors. Either way an empty selection is
    /// the caller's business, not git's.
    #[tokio::test]
    async fn an_empty_path_list_runs_no_git_at_all() {
        // No repo needed: the guard is hit before anything is spawned, so a
        // path that does not exist proves the early return rather than luck.
        assert!(discard(Path::new("\\\\?\\nonexistent"), &[]).await.is_ok());
    }

    /// The same guard on the delete path, and it matters more here: `git clean
    /// --force --` with no pathspec at all is not an error, it is *clean the
    /// entire working tree*. An empty selection reaching git would be the
    /// worst single command this app could issue.
    #[tokio::test]
    async fn an_empty_path_list_never_reaches_clean() {
        assert!(delete_untracked(Path::new("\\\\?\\nonexistent"), &[]).await.is_ok());
    }

    /// The delete counterpart of the [`DISCARD_FLAGS`] test above. Each of
    /// these turns a confirmed two-file delete into something much larger, and
    /// none of them changes anything the user can see before it happens.
    #[test]
    fn clean_never_recurses_into_directories_or_reaches_ignored_files() {
        assert_eq!(CLEAN_FLAGS, ["--force", "--"]);
        for flag in ["-d", "-x", "-X", "--directory"] {
            assert!(
                !CLEAN_FLAGS.contains(&flag),
                "clean must stay file-scoped and never touch ignored files: {flag} would take \
                 whole build directories off disk from a two-row selection"
            );
        }
    }

    /// Without `:(literal)` these are globs, and `git clean --force` would
    /// delete every file they happen to match rather than the one row the user
    /// confirmed. This is the single most dangerous line in the module.
    #[test]
    fn every_path_is_passed_as_a_literal_pathspec() {
        assert_eq!(literal_pathspec("report[1].txt"), ":(literal)report[1].txt");
        assert_eq!(literal_pathspec("draft*.md"), ":(literal)draft*.md");
        // A leading colon would otherwise be read as pathspec magic itself.
        assert_eq!(literal_pathspec(":weird.txt"), ":(literal):weird.txt");
    }
}
