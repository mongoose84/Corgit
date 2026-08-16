//! Running git (SPEC.md §3, §7, §8).
//!
//! Every git invocation in Corgit goes through here, for two reasons: the
//! global in-flight cap lives in one place, and so does the Windows-specific
//! "don't flash a console window" flag.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

use serde::Serialize;
use tokio::process::Command;
use tokio::sync::Semaphore;

/// Git for Windows installs a small launcher at `<install>\cmd\git.exe` whose
/// only job is to exec the real binary. Measured on a 69-repo sweep, that hop
/// costs ~75 ms per call — more than `git status` itself spends working. The
/// real binary lives under one of these, depending on the build.
#[cfg(windows)]
const REAL_BINARY_DIRS: [&str; 3] = ["mingw64", "mingw32", "clangarm64"];

/// Global cap on concurrent git processes (§7.3). Without it the status sweep
/// spawns one `git.exe` per repo at once and Defender melts the machine. It is
/// a process-wide static precisely because the guarantee has to hold across
/// every window (§9.2).
const MAX_INFLIGHT: usize = 8;

/// Built-in FSMonitor, which is what makes `git status` fast on Windows (§6).
const FSMONITOR_MIN_VERSION: (u32, u32) = (2, 37);

fn inflight() -> &'static Semaphore {
    static SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| Semaphore::new(MAX_INFLIGHT))
}

/// Two entry points to the same git, chosen by what the command needs.
///
/// `write` is whatever `git` resolves to on PATH — the documented entry point,
/// which is where credential helpers, hooks and LFS are guaranteed to behave.
/// The whole reason Corgit shells out rather than linking libgit2 (§3) is to
/// inherit those, so nothing that fetches, pulls, pushes or commits may take a
/// shortcut around it.
///
/// `read` skips the launcher where one exists. Status, log and for-each-ref
/// need no credentials and fire no hooks, and they are the commands that run
/// 77 at a time, so this is where the saving is worth having.
struct Binaries {
    read: PathBuf,
    write: PathBuf,
}

fn binaries() -> &'static Binaries {
    static BINARIES: OnceLock<Binaries> = OnceLock::new();
    BINARIES.get_or_init(|| {
        let write = on_path().unwrap_or_else(|| PathBuf::from("git"));
        Binaries {
            read: unshimmed(&write).unwrap_or_else(|| write.clone()),
            write,
        }
    })
}

/// A PATH walk rather than a `which` crate: one dependency avoided, and the
/// answer is wanted exactly once per process.
fn on_path() -> Option<PathBuf> {
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(windows)]
fn unshimmed(git: &Path) -> Option<PathBuf> {
    // <install>\cmd\git.exe → <install>
    let install = git.parent()?.parent()?;
    REAL_BINARY_DIRS
        .iter()
        .map(|dir| install.join(dir).join("bin").join("git.exe"))
        .find(|candidate| candidate.is_file())
}

#[cfg(not(windows))]
fn unshimmed(_git: &Path) -> Option<PathBuf> {
    // Only Git for Windows has the launcher.
    None
}

/// What we know about the git binary, resolved once at startup. `available:
/// false` drives a blocking first-run screen rather than a failure per
/// operation (§3).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    pub available: bool,
    /// Raw `git --version` output, e.g. "2.51.0.windows.1".
    pub version: Option<String>,
    /// Whether `core.fsmonitor` is supported (§6).
    pub supports_fsmonitor: bool,
    /// The binary the status sweep actually runs. Worth surfacing: it is not
    /// always the `git` on PATH, and a sweep three times slower than expected
    /// is the first thing you would want to check.
    pub read_binary: Option<String>,
}

pub struct Output {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Ask git who it is. Runs once during setup; the result is stored in
/// `AppState` so no other code path has to care whether git exists.
pub async fn probe() -> GitInfo {
    // Probes the write binary deliberately: it is the one whose absence would
    // be fatal, and the read binary is derived from it.
    // `--version` needs no repo, so any working directory will do.
    let Ok(output) = run_in(&binaries().write, Path::new("."), &["--version"], None, &[]).await else {
        return GitInfo::default();
    };
    if !output.ok {
        return GitInfo::default();
    }

    let version = output
        .stdout
        .trim()
        .strip_prefix("git version ")
        .unwrap_or_else(|| output.stdout.trim())
        .to_string();

    GitInfo {
        available: true,
        supports_fsmonitor: at_least(&version, FSMONITOR_MIN_VERSION),
        version: Some(version),
        read_binary: Some(binaries().read.to_string_lossy().into_owned()),
    }
}

/// Compare only the leading `major.minor` — the Windows builds carry suffixes
/// (`2.51.0.windows.1`) that no general version parser handles usefully.
fn at_least(version: &str, min: (u32, u32)) -> bool {
    let mut parts = version.split('.').map(|part| part.parse::<u32>().ok());
    let (Some(Some(major)), Some(Some(minor))) = (parts.next(), parts.next()) else {
        return false;
    };
    (major, minor) >= min
}

/// Run a read-only command. `--no-optional-locks` keeps git from writing the
/// index while we are only looking (§8) — it is the difference between a sweep
/// that is safe to run every 60 s and one that fights the user's terminal.
pub async fn read(cwd: &Path, args: &[&str]) -> Result<Output, String> {
    let mut full = Vec::with_capacity(args.len() + 1);
    full.push("--no-optional-locks");
    full.extend_from_slice(args);
    run_in(&binaries().read, cwd, &full, None, &[]).await
}

/// Run a mutating command through the documented `git` entry point (§3) — the
/// one credential helpers, hooks and LFS expect. Nothing that stages, commits,
/// fetches, pulls or pushes may take the `read` shortcut.
pub async fn write(cwd: &Path, args: &[&str]) -> Result<Output, String> {
    run_in(&binaries().write, cwd, args, None, &[]).await
}

/// Like [`write`], but pipes `input` to the child's stdin — `git commit -F -`
/// takes its message this way specifically to avoid arg-escaping pain (§8.6).
pub async fn write_stdin(cwd: &Path, args: &[&str], input: &str) -> Result<Output, String> {
    run_in(&binaries().write, cwd, args, Some(input), &[]).await
}

/// Like [`write`], but with `envs` set on the child — the background fetch
/// sweep's way of disabling credential prompts (§8.7) without a prompt-free
/// git existing anywhere else. A **manual** fetch/push must never go through
/// this: the user is sitting right there and is allowed to be prompted.
pub async fn write_noninteractive(
    cwd: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<Output, String> {
    run_in(&binaries().write, cwd, args, None, envs).await
}

async fn run_in(
    program: &Path,
    cwd: &Path,
    args: &[&str],
    stdin: Option<&str>,
    envs: &[(&str, &str)],
) -> Result<Output, String> {
    // Held for the lifetime of the child process, not just the spawn.
    let _permit = inflight()
        .acquire()
        .await
        .map_err(|_| "git semaphore closed".to_string())?;

    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .envs(envs.iter().copied())
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // A console window per git process is unusable at 77 repos. cfg-gated
    // rather than unconditional so the Linux build (§10) stays clean.
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|err| format!("could not run git: {err}"))?;

    if let Some(input) = stdin {
        use tokio::io::AsyncWriteExt;
        // Taking the handle (rather than borrowing) drops and closes it at
        // the end of this block, so git sees EOF instead of hanging on stdin.
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(input.as_bytes())
                .await
                .map_err(|err| format!("could not write to git: {err}"))?;
        }
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|err| format!("could not run git: {err}"))?;

    // Git emits paths as bytes and porcelain v2 with -z does not quote them, so
    // a path that is not valid UTF-8 is possible. Lossy conversion keeps the
    // record parseable; the alternative is dropping the whole repo's status.
    Ok(Output {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_gate_reads_major_minor() {
        assert!(at_least("2.51.0.windows.1", FSMONITOR_MIN_VERSION));
        assert!(at_least("2.37.0", FSMONITOR_MIN_VERSION));
        assert!(at_least("3.0.0", FSMONITOR_MIN_VERSION));
    }

    #[test]
    fn version_gate_rejects_older_and_unparseable() {
        assert!(!at_least("2.36.9", FSMONITOR_MIN_VERSION));
        assert!(!at_least("1.9.5", FSMONITOR_MIN_VERSION));
        assert!(!at_least("", FSMONITOR_MIN_VERSION));
        assert!(!at_least("banana", FSMONITOR_MIN_VERSION));
    }
}
