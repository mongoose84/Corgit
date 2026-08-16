//! Branch switching and creation (SPEC.md §8.3, build step 8).
//!
//! Switching has two shapes only, matching the two ref kinds the graph already
//! badges (`graph::RefKind`): a local branch is a plain `git switch`, a
//! remote-tracking one creates and tracks a local branch of the same name —
//! unless that local branch already exists (the graph, unlike the switcher,
//! shows every ref regardless of whether its local counterpart is also
//! visible), in which case this just switches to it instead.
//!
//! Creation (§8.3, right-click a ref badge in the graph) takes an explicit
//! start point — the badge or commit that was right-clicked — so it never
//! depends on what happens to be checked out.

use std::path::Path;

use crate::git;

pub async fn switch_local(repo: &Path, branch: &str) -> Result<(), String> {
    let output = git::write(repo, &["switch", branch]).await?;
    if !output.ok {
        return Err(full_message(&output.stderr));
    }
    Ok(())
}

/// `remote_ref` is the short name `for-each-ref` gave the badge, e.g.
/// `origin/feature-x` — `local_name` strips the leading remote, e.g.
/// `feature-x`.
pub async fn switch_remote_tracking(repo: &Path, remote_ref: &str) -> Result<(), String> {
    let local = local_name(remote_ref);

    let create = git::write(repo, &["switch", "-c", &local, "--track", remote_ref]).await?;
    if create.ok {
        return Ok(());
    }
    if !create.stderr.contains("already exists") {
        return Err(full_message(&create.stderr));
    }

    // A local branch of that name already exists elsewhere in the graph —
    // just switch to it, the same as double-clicking its own local badge would.
    let fallback = git::write(repo, &["switch", &local]).await?;
    if !fallback.ok {
        return Err(full_message(&fallback.stderr));
    }
    Ok(())
}

/// New branch at `start_point` (a ref name or commit hash — whatever the graph
/// badge or row that was right-clicked names).
///
/// `checkout` picks the command rather than adding a second switch afterwards:
/// `git switch -c` is atomic where create-then-switch can leave a branch behind
/// on a checkout that fails against a dirty tree. Nothing here ever sets an
/// upstream — a branch created off `origin/foo` is deliberately *not* tracking
/// it, since that is a different intent from "switch to that remote branch"
/// (which `switch_remote_tracking` above already covers).
pub async fn create(repo: &Path, name: &str, start_point: &str, checkout: bool) -> Result<(), String> {
    let output = git::write(repo, &create_args(name, start_point, checkout)).await?;
    if !output.ok {
        return Err(create_message(&output.stderr));
    }
    Ok(())
}

fn create_args<'a>(name: &'a str, start_point: &'a str, checkout: bool) -> Vec<&'a str> {
    if checkout {
        vec!["switch", "-c", name, start_point]
    } else {
        vec!["branch", name, start_point]
    }
}

/// `origin/feature-x` → `feature-x`. Only the first path segment is treated as
/// the remote name, so a branch whose own name contains a `/` (e.g.
/// `origin/feature/x`) still yields `feature/x`.
fn local_name(remote_ref: &str) -> String {
    remote_ref.split_once('/').map_or(remote_ref, |(_, rest)| rest).to_string()
}

/// The whole trimmed stderr, not just its first line — §13's "raw stderr
/// always available in a collapsible Details" needs the whole thing; the
/// frontend's `translateGitError` picks a plain-language headline out of it.
fn full_message(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() { "git switch failed".to_string() } else { trimmed.to_string() }
}

/// Same whole-stderr rule as [`full_message`], with the fallback naming the
/// operation that actually ran.
fn create_message(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() { "could not create the branch".to_string() } else { trimmed.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_name_strips_the_remote() {
        assert_eq!(local_name("origin/feature-x"), "feature-x");
    }

    #[test]
    fn local_name_keeps_slashes_past_the_first() {
        assert_eq!(local_name("origin/feature/x"), "feature/x");
    }

    #[test]
    fn local_name_with_no_slash_is_returned_as_is() {
        assert_eq!(local_name("main"), "main");
    }

    #[test]
    fn creating_without_checkout_leaves_head_alone() {
        assert_eq!(create_args("feature-x", "main", false), ["branch", "feature-x", "main"]);
    }

    #[test]
    fn creating_with_checkout_is_one_atomic_switch() {
        assert_eq!(create_args("feature-x", "main", true), ["switch", "-c", "feature-x", "main"]);
    }

    /// Never `--track`: a branch cut from a remote badge is not the same
    /// intent as switching to that remote branch (see `create`'s docs).
    #[test]
    fn creating_from_a_remote_ref_does_not_set_an_upstream() {
        assert!(!create_args("feature-x", "origin/feature-x", true).contains(&"--track"));
    }
}
