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

#[derive(Default)]
pub struct RepoWatchers {
    watchers: Mutex<HashMap<String, Debouncer<RecommendedWatcher>>>,
    /// When each repo last had a status read triggered from here — the state
    /// behind `MIN_INTERVAL`. Kept beside the watchers rather than inside each
    /// debouncer's closure because a repo that is dropped and re-watched (a
    /// blur, a `refresh_root`) should not get a free pass through the throttle
    /// it was already subject to.
    last_read: Mutex<HashMap<String, Instant>>,
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
    /// window lost focus (§6). `last_read` survives deliberately: the throttle
    /// is about how often a repo is read, and an alt-tab is not a reason to
    /// forget that.
    pub fn clear(&self) {
        self.watchers.lock().expect("watchers mutex poisoned").clear();
    }

    /// Whether this repo may be read again yet, recording the read if so.
    /// One lock does both because a burst of events arrives on several
    /// debouncer threads at once, and a check separate from the record would
    /// let all of them through.
    fn may_read(&self, repo_id: &str) -> bool {
        let mut last_read = self.last_read.lock().expect("last-read mutex poisoned");
        let now = Instant::now();
        match last_read.get(repo_id) {
            Some(previous) if now.duration_since(*previous) < MIN_INTERVAL => false,
            _ => {
                last_read.insert(repo_id.to_string(), now);
                true
            }
        }
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
            if !app.state::<crate::AppState>().watchers.may_read(&repo_id) {
                return;
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
        assert!(watchers.may_read("repo-a"));
        assert!(!watchers.may_read("repo-a"));
        assert!(!watchers.may_read("repo-a"));
        // A different repo has its own budget — one noisy repo must not be
        // able to starve the rest of the list.
        assert!(watchers.may_read("repo-b"));
    }

    #[test]
    fn clearing_the_watchers_does_not_reset_the_throttle() {
        let watchers = RepoWatchers::default();
        assert!(watchers.may_read("repo-a"));
        watchers.clear();
        assert!(!watchers.may_read("repo-a"), "a blur is not a reason to forget a recent read");
    }
}
