//! FS watchers on hot repos (SPEC.md §6).
//!
//! Hot = pinned ∪ selected repos — instant feedback for the small set the
//! user actually cares about right now; everything else relies on the 60 s
//! sweep. Holding a repo's debouncer *is* the watch: dropping the map entry
//! stops it (RAII), which is what makes `sync` a plain diff-and-drop/add
//! rather than anything more stateful.
//!
//! Never watches the working tree — every path watched here lives inside
//! `.git`, so 77 repos' worth of `node_modules` never enters the picture.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use tauri::AppHandle;

use crate::emit_repo_status;

/// "~200 ms" per §6 — git writes several files per operation (HEAD, index,
/// a ref), so this coalesces a burst into one refresh instead of several.
const DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Default)]
pub struct HotWatchers {
    watchers: Mutex<HashMap<String, Debouncer<RecommendedWatcher>>>,
}

impl HotWatchers {
    /// Diffs the desired hot set against what's currently watched: drops
    /// entries no longer hot (which stops their watch on drop) and starts new
    /// ones. Called after every change to pins or selection (§6).
    pub fn sync(&self, app: &AppHandle, hot: &[(String, PathBuf)]) {
        let mut watchers = self.watchers.lock().expect("hot-watchers mutex poisoned");

        watchers.retain(|id, _| hot.iter().any(|(hot_id, _)| hot_id == id));

        for (id, path) in hot {
            if watchers.contains_key(id) {
                continue;
            }
            if let Some(debouncer) = start(app.clone(), id.clone(), path.clone()) {
                watchers.insert(id.clone(), debouncer);
            }
        }
    }

    /// Drops every watcher outright — the root was replaced or closed, so
    /// nothing in the old hot set is worth watching any more.
    pub fn clear(&self) {
        self.watchers.lock().expect("hot-watchers mutex poisoned").clear();
    }
}

/// `None` when the repo can't be watched at all (its `.git` is a file, not a
/// directory — a linked worktree or submodule) rather than an error: this is
/// a known, documented scope limit (§6), not a failure worth surfacing.
fn start(app: AppHandle, repo_id: String, repo_path: PathBuf) -> Option<Debouncer<RecommendedWatcher>> {
    let git_dir = repo_path.join(".git");
    if !git_dir.is_dir() {
        return None;
    }

    let callback_path = repo_path.clone();
    let mut debouncer = new_debouncer(DEBOUNCE, move |result: DebounceEventResult| {
        if result.is_err() {
            return;
        }
        // Runs on the debouncer's own thread, not async — hand off to the
        // async runtime the same way `write_and_refresh` publishes a status
        // after any other mutation.
        let app = app.clone();
        let repo_id = repo_id.clone();
        let path = callback_path.clone();
        tauri::async_runtime::spawn(async move {
            emit_repo_status(&app, &repo_id, &path).await;
        });
    })
    .ok()?;

    // Non-recursive on `.git` itself covers HEAD/index/ORIG_HEAD/etc. in one
    // watch; `refs` needs its own recursive watch since it's a subdirectory.
    // Broader than the spec's three literal paths, but every path inside
    // `.git` is itself a legitimate reason to refresh (a commit's
    // COMMIT_EDITMSG, say), so this isn't over-triggering in practice.
    debouncer.watcher().watch(&git_dir, RecursiveMode::NonRecursive).ok()?;
    let refs_dir = git_dir.join("refs");
    if refs_dir.is_dir() {
        let _ = debouncer.watcher().watch(&refs_dir, RecursiveMode::Recursive);
    }

    Some(debouncer)
}
