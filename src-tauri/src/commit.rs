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
        return Err(first_line(&output.stderr));
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
        return Err(first_line(&output.stderr));
    }
    Ok(())
}

fn first_line(stderr: &str) -> String {
    stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("git failed")
        .trim()
        .to_string()
}
