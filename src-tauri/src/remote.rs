//! Remote operations (SPEC.md §8.7): fetch, pull, push, publish.
//!
//! Every function here is a write, same as `commit.rs` — acquiring the
//! repo's write-queue lock (§7) is the caller's job in `lib.rs`.

use std::path::Path;

use crate::git;

/// A manual, user-triggered fetch. Allowed to prompt interactively — the user
/// is sitting right there (§8.7).
pub async fn fetch(repo: &Path) -> Result<(), String> {
    run_network(repo, &["fetch", "--prune", "--no-tags", "--quiet"]).await
}

/// The background fetch sweep's fetch. Must never prompt — a background sweep
/// blocking on a credential dialog would look like a hang (§8.7).
pub async fn fetch_background(repo: &Path) -> Result<(), String> {
    let output = git::write_noninteractive(
        repo,
        &["-c", "credential.interactive=never", "fetch", "--prune", "--no-tags", "--quiet"],
        &[
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_ASKPASS", "echo"),
            ("SSH_ASKPASS", "echo"),
            ("SSH_ASKPASS_REQUIRE", "never"),
        ],
    )
    .await?;
    if !output.ok {
        return Err(full_message(&output.stderr));
    }
    Ok(())
}

/// `--no-rebase` is explicit: user config may set `pull.rebase=true`, and pull
/// in Corgit is always a merge (§2 — rebase is out of scope for v1).
pub async fn pull(repo: &Path) -> Result<(), String> {
    run_network(repo, &["pull", "--no-rebase"]).await
}

pub async fn push(repo: &Path) -> Result<(), String> {
    run_network(repo, &["push"]).await
}

/// §13's merge-conflict banner: exactly two ways out, this is one of them —
/// never a third option, matching §8.3's "never force-checkout" precedent.
///
/// The one function here that touches no remote, so it takes the local write
/// budget rather than the network one despite living in this module.
pub async fn merge_abort(repo: &Path) -> Result<(), String> {
    finish(git::write(repo, &["merge", "--abort"]).await?)
}

/// "Publish branch" — pushes a branch with no upstream configured and sets one.
pub async fn publish(repo: &Path, branch: &str) -> Result<(), String> {
    run_network(repo, &["push", "-u", "origin", branch]).await
}

/// `git remote` is a local config read — no network, no lock needed — so it
/// goes through the read path. The fetch sweep uses this to skip repos with
/// no remote configured at all (§6), rather than spawning a fetch that can
/// only ever fail.
pub async fn has_remote(repo: &Path) -> bool {
    match git::read(repo, &["remote"]).await {
        Ok(output) => output.ok && !output.stdout.trim().is_empty(),
        Err(_) => false,
    }
}

/// Heuristic over stderr text rather than exit-code alone, because git gives
/// every failure the same exit code. Used only to decide whether the
/// background fetch sweep should stop retrying this repo (§8.7, §13) — a
/// false negative just means one more retry next tick, so this stays a rough
/// match rather than a exhaustive parser.
pub fn looks_like_auth_failure(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    [
        "authentication failed",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "permission denied (publickey)",
        "the requested url returned error: 401",
        "the requested url returned error: 403",
        "fatal: access denied",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Everything here but `merge_abort` reaches a remote, so this is the module's
/// default shape — see [`git::write_network`] for why that is its own entry
/// point rather than a plain write.
async fn run_network(repo: &Path, args: &[&str]) -> Result<(), String> {
    finish(git::write_network(repo, args).await?)
}

fn finish(output: git::Output) -> Result<(), String> {
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

    #[test]
    fn recognises_common_auth_failure_messages() {
        assert!(looks_like_auth_failure("fatal: Authentication failed for 'https://example.com/repo.git'"));
        assert!(looks_like_auth_failure(
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled"
        ));
        assert!(looks_like_auth_failure("Permission denied (publickey).\nfatal: Could not read from remote repository."));
        assert!(looks_like_auth_failure("remote: The requested URL returned error: 403"));
    }

    #[test]
    fn does_not_flag_unrelated_failures() {
        assert!(!looks_like_auth_failure("fatal: unable to access: Could not resolve host"));
        assert!(!looks_like_auth_failure("error: failed to push some refs"));
    }
}
