//! Branch switching (SPEC.md §8.3, build step 8).
//!
//! Two shapes only, matching the two ref kinds the graph already badges
//! (`graph::RefKind`): a local branch is a plain `git switch`, a
//! remote-tracking one creates and tracks a local branch of the same name —
//! unless that local branch already exists (the graph, unlike the switcher,
//! shows every ref regardless of whether its local counterpart is also
//! visible), in which case this just switches to it instead.

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
}
