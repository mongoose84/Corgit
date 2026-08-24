//! Running git (SPEC.md §3, §7, §8).
//!
//! Every git invocation in Corgit goes through here, for two reasons: the
//! global in-flight cap lives in one place, and so does the Windows-specific
//! "don't flash a console window" flag.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

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
/// spawns one `git.exe` per repo at once and Defender melts the machine. A
/// process-wide static because the cap has to cover every git spawn in the
/// process at once — which is the whole cap only because §9.2 guarantees there
/// is exactly one process.
const MAX_INFLIGHT: usize = 8;

/// Built-in FSMonitor, which is what makes `git status` fast on Windows (§6).
const FSMONITOR_MIN_VERSION: (u32, u32) = (2, 37);

// How long a git process may run before Corgit presumes it hung and kills it.
//
// None of these are performance targets — they are the point past which a
// process is stuck rather than slow, and they exist because a child that never
// exits is not merely slow, it is permanent. Such a child holds a semaphore
// permit (§7.3) and, for a write, its repo's write-queue guard (§7) with
// nobody left to release either: that repo stops sweeping (`try_read` fails
// every tick), selecting it blocks the middle pane and graph forever on the
// blocking `write_queues.read()`, and the 8-process budget shrinks for good.
//
// Being killed is a normal git failure like any other — the error surfaces
// through the same path as a rejected push (§13), and the operation is
// retryable. Erring generous therefore costs a wait; erring tight costs a
// legitimately slow operation. Hence the spread below.

/// Local reads — status, log, for-each-ref, remote. Bounded by repo size and
/// nothing else: no network, no hooks, no credential prompt. The §1 budget for
/// *all 77* is 300 ms, so a single read reaching 30 s is not slow, it is stuck.
/// `pub(crate)` for one assertion: the status sweep publishes without a repo
/// that outruns `SWEEP_PATIENCE`, and that only means anything while a read is
/// allowed to outlive it.
pub(crate) const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The startup probe, and the tightest budget of the lot because it is the
/// only one that blocks the app from existing: `run`'s setup hook waits on it
/// (§3, ~20 ms expected). §1 gives cold start 500 ms total, so anything near
/// this is already a failure — the point is only that the user gets a window
/// saying "no usable git" instead of an app that never opens.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Local writes — add, restore, commit, switch, branch. Deliberately the most
/// generous of the three: §3 shells out to system git precisely so that hooks
/// run, and a pre-commit hook running a test suite is doing its job rather
/// than hanging. Ten minutes is long enough that reaching it means something
/// is genuinely wedged, not thorough.
const LOCAL_WRITE_TIMEOUT: Duration = Duration::from_secs(600);

/// Network writes — fetch, pull, push. Long enough for an incremental fetch
/// over a bad link, short enough that the background fetch sweep cannot wedge
/// its four slots (§6) — and the eight global ones under them — indefinitely.
/// This is the timeout that matters most: an unreachable host is the failure
/// that actually happens, and TCP alone can take far longer to notice.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);

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
    let Ok(output) =
        run_in(&binaries().write, Path::new("."), &["--version"], None, &[], PROBE_TIMEOUT).await
    else {
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
    run_in(&binaries().read, cwd, &full, None, &[], READ_TIMEOUT).await
}

/// Run a mutating command through the documented `git` entry point (§3) — the
/// one credential helpers, hooks and LFS expect. Nothing that stages, commits,
/// fetches, pulls or pushes may take the `read` shortcut.
///
/// For a command that talks to a remote use [`write_network`] instead: same
/// binary, different budget for how long it is allowed to say nothing.
pub async fn write(cwd: &Path, args: &[&str]) -> Result<Output, String> {
    run_in(&binaries().write, cwd, args, None, &[], LOCAL_WRITE_TIMEOUT).await
}

/// Like [`write`], but for a command that reaches the network — fetch, pull,
/// push. Split out for [`NETWORK_TIMEOUT`] alone: an unreachable host is the
/// hang that actually happens in practice, and it deserves a far tighter leash
/// than a commit whose pre-commit hook is running tests.
pub async fn write_network(cwd: &Path, args: &[&str]) -> Result<Output, String> {
    run_in(&binaries().write, cwd, args, None, &[], NETWORK_TIMEOUT).await
}

/// Like [`write`], but pipes `input` to the child's stdin — `git commit -F -`
/// takes its message this way specifically to avoid arg-escaping pain (§8.6).
/// Commit is the hook-firing command, so this takes the local-write budget.
pub async fn write_stdin(cwd: &Path, args: &[&str], input: &str) -> Result<Output, String> {
    run_in(&binaries().write, cwd, args, Some(input), &[], LOCAL_WRITE_TIMEOUT).await
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
    run_in(&binaries().write, cwd, args, None, envs, NETWORK_TIMEOUT).await
}

async fn run_in(
    program: &Path,
    cwd: &Path,
    args: &[&str],
    stdin: Option<&str>,
    envs: &[(&str, &str)],
    budget: Duration,
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
        .stderr(Stdio::piped())
        // What actually enforces `budget` below, and the only thing that can:
        // `wait_with_output` consumes the `Child`, so once it is in flight
        // there is no handle left to call `kill` on. Dropping the future drops
        // the child, and this turns that drop into a kill. It also covers the
        // case no timeout can — a task aborted from outside, e.g. a ticker
        // cancelled on blur (§6) — which would otherwise orphan the process.
        .kill_on_drop(true);

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

    // Feeding stdin is inside the budget rather than before it: a child that
    // never reads its stdin would otherwise block `write_all` forever on a
    // full pipe, which is the one hang a timeout around the wait alone would
    // still miss.
    let run = async move {
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

        child
            .wait_with_output()
            .await
            .map_err(|err| format!("could not run git: {err}"))
    };

    let Ok(waited) = tokio::time::timeout(budget, run).await else {
        // `run` is dropped here, taking the `Child` with it; `kill_on_drop`
        // above makes that an actual kill rather than an orphan.
        //
        // Logged as well as returned: the caller surfaces this to whoever
        // triggered it, but a fetch killed by the background sweep (§6) has no
        // one watching, and "which repo, which command" is exactly what you
        // would want to know afterwards.
        log::warn!("killed `git {}` in {} after {:?}", args.join(" "), cwd.display(), budget);
        return Err(timed_out_message(budget));
    };
    let output = waited?;

    // Git emits paths as bytes and porcelain v2 with -z does not quote them, so
    // a path that is not valid UTF-8 is possible. Lossy conversion keeps the
    // record parseable; the alternative is dropping the whole repo's status.
    let output = Output {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };

    // §13: a failing git command used to be the one thing the log did not
    // have. A non-zero exit is a *successful* call down here — the child ran
    // and said no — so the timeout kill above was the only failure anything
    // ever recorded, and "why was my push rejected" was unanswerable after the
    // fact. This is the single place that sees every invocation.
    //
    // Unconditional rather than only for failures the user is shown:
    // `switch_remote_tracking` deliberately probes with `switch -c --track`
    // and falls back when git says "already exists", so a handled, expected
    // failure lands here too. That is the right trade for a *log* — what
    // Corgit actually ran is what you came looking for. The user-facing ring
    // (`problems.rs`) is fed from where an error is returned instead, and so
    // stays free of failures nobody needed to know about.
    if !output.ok {
        log::warn!(
            "`git {}` in {} exited non-zero: {}",
            args.join(" "),
            cwd.display(),
            output.stderr.trim()
        );
    }

    Ok(output)
}

/// Phrased as something Corgit did rather than something git reported, because
/// that is what happened — git did not fail, it was stopped.
///
/// The substring "timed out" is load-bearing: `gitErrors.ts` keys its §13
/// translation off it, so this wording is a contract across the IPC boundary
/// rather than only prose. The test below is what keeps the two in step.
fn timed_out_message(budget: Duration) -> String {
    format!("git timed out after {}s and was stopped", budget.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `gitErrors.ts` matches `lower.includes('timed out')`. If that substring
    /// ever leaves this message the frontend silently stops translating a
    /// timeout — it degrades to raw text with no Retry action, which is easy
    /// to miss by eye and impossible to miss here.
    #[test]
    fn a_timeout_message_stays_translatable_by_the_frontend() {
        let message = timed_out_message(NETWORK_TIMEOUT);
        assert!(message.to_lowercase().contains("timed out"), "{message}");
    }

    #[test]
    fn a_timeout_message_names_the_budget_it_exceeded() {
        assert!(timed_out_message(Duration::from_secs(120)).contains("120"));
    }

    /// Not arbitrary ordering: each budget is bounded by what that class of
    /// work can legitimately take (§7.3's comment block). A read outliving a
    /// fetch, or a fetch outliving a hook-firing commit, would mean one of the
    /// constants was edited without its reasoning.
    #[test]
    fn budgets_are_ordered_by_how_long_the_work_can_honestly_take() {
        assert!(PROBE_TIMEOUT < READ_TIMEOUT, "the probe blocks startup; it must be the tightest");
        assert!(READ_TIMEOUT < NETWORK_TIMEOUT, "a local read must not outlive a fetch");
        assert!(NETWORK_TIMEOUT < LOCAL_WRITE_TIMEOUT, "hooks make commit the most generous case");
    }

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
