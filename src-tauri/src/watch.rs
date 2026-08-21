//! FS watchers, one per repo (SPEC.md §6).
//!
//! **Every repo is watched, working tree included** — on Windows.
//! `ReadDirectoryChangesW` watches a subtree through a single handle no matter
//! how deep it goes, so 77 repos cost 77 handles rather than 77 × every
//! directory beneath them; measured over the 69-repo bench root, the whole set
//! costs 17.3 ms to establish, +73 handles and +4.4 MB. §6's original "never
//! watch the working tree" was not wrong, it was reasoned about inotify, where
//! a recursive watch really does cost a descriptor per directory. Hence
//! `WATCH_WORKING_TREE` below rather than a change of rule for everyone.
//!
//! This is what makes the status sweep a reconciliation pass instead of the
//! refresh mechanism (§6). The sweep spawns one `git status` per repo and
//! process creation is 85–95 % of that cost, so the way under §1's 300 ms
//! budget is not to spawn faster but to spawn only for repos that changed.
//!
//! Holding a repo's debouncer *is* the watch: dropping the map entry stops it
//! (RAII), which is what makes `sync` a plain diff-and-drop/add rather than
//! anything more stateful — and what makes dropping the lot on blur (§6) a
//! one-liner rather than a shutdown protocol.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use tauri::{AppHandle, Manager};

use crate::emit_repo_status;

/// Whether this platform gives subtree watches cheaply enough to point one at
/// a working tree (§6). Windows does; inotify does not, and there the watch
/// stays inside `.git` and working-tree changes wait for the sweep.
///
/// A `const` rather than `#[cfg]` at each use site so both arms keep compiling
/// on both platforms — the Linux build is a v2 deliverable (§10) that nobody
/// runs today, which is exactly how it would rot.
const WATCH_WORKING_TREE: bool = cfg!(windows);

/// "~200 ms" per §6 — git writes several files per operation (HEAD, index, a
/// ref), so this coalesces a burst into one refresh instead of several. This
/// is the delay the user actually feels: it is what stands between staging in
/// a terminal and the row updating.
const DEBOUNCE: Duration = Duration::from_millis(200);

/// The floor between two status reads of the same repo, however many events
/// arrive (§6). A build writes thousands of files that git will never report,
/// and without this each one would be worth a `git status` — the sweep's whole
/// per-tick cost, repeatedly, for one repo.
///
/// 2 s is chosen against the debounce above rather than against human
/// patience: a refresh that lands within a couple of seconds of a build
/// finishing is indistinguishable from an instant one, because the build is
/// what the user was watching.
///
/// Events arriving inside the window are *deferred*, never dropped — see
/// `Claim`. A leading-edge-only throttle is fine for a row, which the next
/// event or the sweep will put right, but it silently loses the one thing the
/// user is actually reading: the last write of a burst is what describes the
/// file as it now stands, and it is the write most likely to land inside its
/// predecessor's window.
const MIN_INTERVAL: Duration = Duration::from_secs(2);

/// Directory names whose contents are skipped before an event is even
/// debounced (§6).
///
/// **This list is allowed to be wrong.** A repo that really does track its
/// `dist/` gets its refresh from the reconciliation sweep instead of from a
/// watcher, which makes the failure mode "late", never "wrong" — the property
/// that makes a guess like this safe to ship. Ordered roughly by how much
/// churn each one produces.
const IGNORED_DIRS: [&str; 8] = [
    "node_modules",
    "target",
    ".next",
    "dist",
    "build",
    "obj",
    "bin",
    ".venv",
];

/// What the throttle says about a repo an event has just arrived for.
enum Claim {
    /// Far enough since the last read; it has been recorded, go ahead.
    Now,
    /// Too soon, and nothing was waiting yet. Read once this much more of
    /// `MIN_INTERVAL` has passed — the caller owns that deferred read.
    After(Duration),
    /// Too soon, and a deferred read is already queued. That one will see
    /// whatever this event described, so this one has nothing left to do.
    Queued,
}

/// One repo's throttle state. `pending` is the whole of what makes this a
/// leading-*and*-trailing edge throttle rather than a leading-edge one.
struct ReadState {
    last: Instant,
    pending: bool,
}

#[derive(Default)]
pub struct RepoWatchers {
    watchers: Mutex<HashMap<String, Debouncer<RecommendedWatcher>>>,
    /// When each repo last had a status read triggered from here, and whether
    /// one is already waiting out the throttle — the state behind
    /// `MIN_INTERVAL`. Kept beside the watchers rather than inside each
    /// debouncer's closure because a repo that is dropped and re-watched (a
    /// blur, a `refresh_root`) should not get a free pass through the throttle
    /// it was already subject to.
    reads: Mutex<HashMap<String, ReadState>>,
}

impl RepoWatchers {
    /// Diffs the desired set against what's currently watched: drops entries
    /// no longer wanted (which stops their watch on drop) and starts new ones.
    ///
    /// Returns the ids it could **not** watch. Those repos are not covered by
    /// anything and must stay on the sweep interval (§6) — a network share or
    /// a linked worktree is a normal case here, not an error path, and
    /// silently treating one as watched is how a row goes stale for good.
    pub fn sync(&self, app: &AppHandle, repos: &[(String, PathBuf)]) -> Vec<String> {
        let mut watchers = self.watchers.lock().expect("watchers mutex poisoned");

        watchers.retain(|id, _| repos.iter().any(|(repo_id, _)| repo_id == id));

        let mut unwatchable = Vec::new();
        for (id, path) in repos {
            if watchers.contains_key(id) {
                continue;
            }
            match start(app.clone(), id.clone(), path.clone()) {
                Some(debouncer) => {
                    watchers.insert(id.clone(), debouncer);
                }
                None => unwatchable.push(id.clone()),
            }
        }

        unwatchable
    }

    /// Drops every watcher outright — the root was replaced or closed, or the
    /// window lost focus (§6). `reads` survives deliberately: the throttle is
    /// about how often a repo is read, and an alt-tab is not a reason to
    /// forget that. A deferred read still waiting finds its repo unwatched and
    /// stands down — see `finish_deferred`.
    pub fn clear(&self) {
        self.watchers.lock().expect("watchers mutex poisoned").clear();
    }

    /// Whether this repo may be read again yet, and if not, when — recording
    /// the read, or the intent to defer one, under the same lock as the check.
    /// One lock does both because a burst of events arrives on several
    /// debouncer threads at once, and a check separate from the record would
    /// let all of them through — and, now that a refusal hands out the wait
    /// rather than ending the matter, would let each of them queue a deferred
    /// read of its own.
    fn claim(&self, repo_id: &str) -> Claim {
        let mut reads = self.reads.lock().expect("reads mutex poisoned");
        let now = Instant::now();
        match reads.get_mut(repo_id) {
            Some(state) => {
                let since = now.duration_since(state.last);
                if since >= MIN_INTERVAL {
                    state.last = now;
                    state.pending = false;
                    Claim::Now
                } else if state.pending {
                    Claim::Queued
                } else {
                    state.pending = true;
                    Claim::After(MIN_INTERVAL - since)
                }
            }
            None => {
                reads.insert(repo_id.to_string(), ReadState { last: now, pending: false });
                Claim::Now
            }
        }
    }

    /// Ends the wait a `Claim::After` handed out. Returns whether the deferred
    /// read should still happen: a repo whose watcher went away while we
    /// waited belongs to a root that was closed, or to a window that lost
    /// focus, and reading it here is precisely the background work §6 promises
    /// an unfocused window does not do — the focus-gain sweep covers it.
    ///
    /// The slot is released either way. Leaving `pending` set on a read that
    /// never happens would wedge this repo out of the throttle for good.
    fn finish_deferred(&self, repo_id: &str) -> bool {
        let watched =
            self.watchers.lock().expect("watchers mutex poisoned").contains_key(repo_id);

        let mut reads = self.reads.lock().expect("reads mutex poisoned");
        if let Some(state) = reads.get_mut(repo_id) {
            state.pending = false;
            // Only a read that actually happens resets the floor.
            if watched {
                state.last = Instant::now();
            }
        }

        watched
    }
}

/// `None` when the repo can't be watched at all — its `.git` is a file rather
/// than a directory (a linked worktree or submodule), or the platform refused
/// the watch (a network share, a permissions failure). The caller puts these
/// back on the sweep rather than surfacing an error: it is a documented scope
/// limit (§6), and the row still updates, just on the slower path.
fn start(app: AppHandle, repo_id: String, repo_path: PathBuf) -> Option<Debouncer<RecommendedWatcher>> {
    let git_dir = repo_path.join(".git");
    if !git_dir.is_dir() {
        return None;
    }

    let callback_path = repo_path.clone();
    let callback_root = repo_path.clone();
    let mut debouncer = new_debouncer(DEBOUNCE, move |result: DebounceEventResult| {
        // An error here is the platform saying it dropped events, not that the
        // repo is gone (§6) — so it is a reason to read the repo, not to skip
        // it. Falling through to the refresh below is the whole handling: we
        // no longer know what changed, so we ask git.
        let interesting = match &result {
            Ok(events) => events.iter().any(|event| !is_noise(&callback_root, &event.path)),
            Err(_) => true,
        };
        if !interesting {
            return;
        }

        // Runs on the debouncer's own thread, not async — hand off to the
        // async runtime the same way `write_and_refresh` publishes a status
        // after any other mutation.
        let app = app.clone();
        let repo_id = repo_id.clone();
        let path = callback_path.clone();
        tauri::async_runtime::spawn(async move {
            // Bound before the `match` rather than matched on directly: the
            // `State` temporary would otherwise live to the end of the match
            // and so be held across the `.await` inside it.
            let claim = app.state::<crate::AppState>().watchers.claim(&repo_id);
            match claim {
                Claim::Now => {}
                Claim::Queued => return,
                Claim::After(wait) => {
                    tokio::time::sleep(wait).await;
                    if !app.state::<crate::AppState>().watchers.finish_deferred(&repo_id) {
                        return;
                    }
                }
            }
            emit_repo_status(&app, &repo_id, &path).await;
        });
    })
    .ok()?;

    if WATCH_WORKING_TREE {
        // One recursive watch covering `.git` and the working tree together.
        // Every path inside `.git` is a legitimate reason to refresh (a
        // commit's COMMIT_EDITMSG, say) and so is every tracked file, so the
        // filtering that matters is `is_noise`, not the watch's scope.
        debouncer.watcher().watch(&repo_path, RecursiveMode::Recursive).ok()?;
    } else {
        // Non-recursive on `.git` itself covers HEAD/index/ORIG_HEAD/etc. in
        // one watch; `refs` needs its own recursive watch since it's a
        // subdirectory. Working-tree changes wait for the sweep here (§6).
        debouncer.watcher().watch(&git_dir, RecursiveMode::NonRecursive).ok()?;
        let refs_dir = git_dir.join("refs");
        if refs_dir.is_dir() {
            let _ = debouncer.watcher().watch(&refs_dir, RecursiveMode::Recursive);
        }
    }

    Some(debouncer)
}

/// Whether a changed path is one git will never report anyway (§6).
///
/// Matched on path *components* below the repo root rather than on the whole
/// string, so a repo that happens to live in `C:\build\thing` is not filtered
/// into silence by its own parent directory — the bug the obvious `contains`
/// spelling would have.
fn is_noise(repo_root: &Path, changed: &Path) -> bool {
    let Ok(relative) = changed.strip_prefix(repo_root) else {
        // Outside the repo entirely: not ours to reason about, and refreshing
        // on it is the safe direction.
        return false;
    };

    relative.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| IGNORED_DIRS.contains(&name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_output_is_ignored() {
        let root = Path::new("C:/dev/code/thing");
        assert!(is_noise(root, &root.join("node_modules/react/index.js")));
        assert!(is_noise(root, &root.join("target/debug/app.exe")));
        assert!(is_noise(root, &root.join("src/../obj/gen.cs")));
    }

    #[test]
    fn source_and_git_metadata_are_not_ignored() {
        let root = Path::new("C:/dev/code/thing");
        assert!(!is_noise(root, &root.join("src/main.rs")));
        assert!(!is_noise(root, &root.join(".git/index")));
        assert!(!is_noise(root, &root.join(".git/refs/heads/main")));
    }

    /// The bug a `path.to_string().contains("build")` spelling would have: the
    /// repo's own ancestors are not part of what changed inside it, and a repo
    /// checked out under `C:\build\` would otherwise never refresh at all.
    #[test]
    fn an_ignored_name_above_the_repo_root_does_not_silence_it() {
        let root = Path::new("C:/build/node_modules/thing");
        assert!(!is_noise(root, &root.join("src/main.rs")));
    }

    /// A path outside the repo cannot be classified, and the safe direction is
    /// to refresh rather than to drop it.
    #[test]
    fn a_path_outside_the_repo_is_not_noise() {
        assert!(!is_noise(Path::new("C:/dev/code/thing"), Path::new("C:/elsewhere/file.txt")));
    }

    /// Both halves of the throttle in one: the first read through, and the
    /// burst behind it turned away. `MIN_INTERVAL` is what stands between a
    /// `cargo build` and thousands of `git status` spawns (§6).
    #[test]
    fn one_repo_is_read_once_per_interval() {
        let watchers = RepoWatchers::default();
        assert!(matches!(watchers.claim("repo-a"), Claim::Now));
        assert!(matches!(watchers.claim("repo-a"), Claim::After(_)));
        // The rest of the burst queues nothing further — the deferred read
        // one of them already booked will see whatever they describe.
        assert!(matches!(watchers.claim("repo-a"), Claim::Queued));
        // A different repo has its own budget — one noisy repo must not be
        // able to starve the rest of the list.
        assert!(matches!(watchers.claim("repo-b"), Claim::Now));
    }

    /// What the trailing edge is for: the last write of a burst is the one
    /// describing the file as it now stands, and turning it away outright
    /// left an open diff (§5.4) showing the previous save until something
    /// unrelated happened to touch the repo.
    #[test]
    fn a_throttled_change_is_deferred_not_dropped() {
        let watchers = RepoWatchers::default();
        assert!(matches!(watchers.claim("repo-a"), Claim::Now));

        let Claim::After(wait) = watchers.claim("repo-a") else {
            panic!("a refusal must hand out the wait, not end the matter");
        };
        assert!(wait <= MIN_INTERVAL && !wait.is_zero());

        // Nothing is watching `repo-a` in this test, so the deferred read
        // stands down (§6: an unfocused window reads nothing). It must still
        // release the slot — a `pending` left set would wedge this repo out
        // of the throttle for the rest of the session.
        assert!(!watchers.finish_deferred("repo-a"));
        assert!(matches!(watchers.claim("repo-a"), Claim::After(_)));
    }

    #[test]
    fn clearing_the_watchers_does_not_reset_the_throttle() {
        let watchers = RepoWatchers::default();
        assert!(matches!(watchers.claim("repo-a"), Claim::Now));
        watchers.clear();
        assert!(
            matches!(watchers.claim("repo-a"), Claim::After(_)),
            "a blur is not a reason to forget a recent read",
        );
    }
}
