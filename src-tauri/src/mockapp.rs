//! Tests that need a real `AppHandle` — the orchestration that has no value to
//! assert on, only something that must *happen*.
//!
//! Everything else in this crate is tested the way `merge_sweep_results`,
//! `bulk::tally` and `sweep::decide` were: pull the decision out as a pure
//! function and pin that. What is left here cannot be pulled out, because it
//! is not a decision. `write_and_refresh` publishing a status after a failed
//! write is nothing more than where a call sits relative to an `if`, and that a
//! prune is still *called* is only true of the caller. Each was a known hole in
//! docs/AUDIT.md (T2, A5, A3) for as long as no test could build an app.
//!
//! The app is `tauri::test::mock_app` with the real `AppState` managed on it
//! via `AppState::new` — the constructor `setup` uses, so these run against the
//! state the app runs with. It needs `build.rs`'s manifest handling to start at
//! all on Windows; `a_mock_app_can_start` is the canary for that.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use tauri::test::{mock_app, MockRuntime};
use tauri::{App, AppHandle, Listener, Manager};

use crate::settings::Settings;
use crate::testrepo::TempRepo;
use crate::{AppState, Repo, RootState};

struct Harness {
    app: App<MockRuntime>,
    dir: PathBuf,
}

impl Harness {
    /// A mock app with a fresh `AppState` whose config and cache directories
    /// are this test's own, so `roots/` and `cache/` writes land somewhere
    /// disposable rather than in the developer's real profile.
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("corgit-test-mockapp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("could not create the test directory");

        let app = mock_app();
        app.manage(AppState::new(
            dir.join("config"),
            dir.join("cache"),
            Settings::default(),
            Arc::new(tokio::sync::OnceCell::new()),
        ));
        Self { app, dir }
    }

    fn handle(&self) -> &AppHandle<MockRuntime> {
        self.app.handle()
    }

    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    /// Installs `repos` as the open root, the way `open_root` would minus the
    /// scan — the tests choose exactly which repos exist.
    fn open(&self, repos: Vec<Repo>) {
        let root = RootState {
            generation: 1,
            path: self.dir.join("root"),
            repos,
            statuses: HashMap::new(),
            errors: HashMap::new(),
            last_fetch_at: HashMap::new(),
            auth_needed: HashSet::new(),
            pins: HashSet::new(),
            unwatched: HashSet::new(),
            selected: None,
        };
        *self.state().root.lock().unwrap() = Some(root);
    }

    fn close(&self) {
        *self.state().root.lock().unwrap() = None;
    }

    /// Every payload emitted under `event` from now on, as raw JSON.
    fn record(&self, event: &str) -> Arc<Mutex<Vec<serde_json::Value>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        self.app.listen_any(event, move |event| {
            let payload = serde_json::from_str(event.payload()).expect("event payload is not JSON");
            sink.lock().unwrap().push(payload);
        });
        seen
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn repo(id: &str, path: &Path) -> Repo {
    Repo { id: id.to_string(), name: id.to_string(), path: path.to_path_buf() }
}

/// Not a test of Corgit — a test that tests *can* drive Corgit. For a long
/// time a test binary containing `mock_app` would not even start on Windows
/// (`STATUS_ENTRYPOINT_NOT_FOUND`); `build.rs` explains the cause. Should that
/// regress, this fails alone and by name, rather than as every test in this
/// file dying at once with a loader error that says nothing about why.
#[test]
fn a_mock_app_can_start() {
    let _app = mock_app();
}

/// CLAUDE.md's "regardless of whether the operation succeeded", and the reason
/// it is a rule at all: a write that fails can still have changed the repo.
/// The op here does exactly that — stages a file, then reports failure — and
/// the row must show the staged file anyway, both in Rust's state and in the
/// event the frontend mirrors it from.
#[test]
fn a_failed_write_still_publishes_the_repo_status() {
    let h = Harness::new("publish-regardless");
    let git = TempRepo::new("publish-regardless");
    git.write("a.txt", "a\n");
    h.open(vec![repo("r", git.path())]);
    let published = h.record(crate::sweep::REPO_STATUS_EVENT);

    let result = tauri::async_runtime::block_on(crate::write_and_refresh(
        h.handle(),
        "r".to_string(),
        "Stage",
        |path| async move {
            let staged = Command::new("git").args(["add", "a.txt"]).current_dir(&path).status().unwrap();
            assert!(staged.success());
            Err("failed after the index had already changed".to_string())
        },
    ));

    assert!(result.is_err(), "the op's failure must reach the caller unchanged");

    {
        let state = h.state();
        let root = state.root.lock().unwrap();
        let status = root.as_ref().unwrap().statuses.get("r").expect("no status recorded after a failed write");
        assert_eq!(status.staged, 1, "the recorded status predates the failed write");
    }

    let published = published.lock().unwrap();
    assert_eq!(published.len(), 1, "expected exactly one status event, got {published:?}");
    assert_eq!(published[0]["repoId"], "r");
    assert_eq!(published[0]["status"]["staged"], 1);
}

/// §13's *Work in progress*: the row spins from `write:begin` to `write:end`,
/// so an end that is not sent is a row that spins forever. `WriteMarker` is a
/// `Drop` guard so that a failing op cannot skip it — this is that case.
#[test]
fn a_failed_write_still_ends_its_busy_marker() {
    let h = Harness::new("busy-marker");
    let git = TempRepo::new("busy-marker");
    h.open(vec![repo("r", git.path())]);
    let begun = h.record(crate::WRITE_BEGIN_EVENT);
    let ended = h.record(crate::WRITE_END_EVENT);

    let _ = tauri::async_runtime::block_on(crate::write_and_refresh(h.handle(), "r".to_string(), "Stage", |_| async {
        Err("refused".to_string())
    }));

    assert_eq!(begun.lock().unwrap().len(), 1);
    assert_eq!(ended.lock().unwrap().len(), 1, "a failed write left its row marked busy");
}

/// A5's wiring: `WriteQueues::retain_known` and `RepoWatchers::retain_reads`
/// are tested in their own files, and this checks `sync_watchers` still calls
/// both. A root swap that drops a repo must drop its queue and its throttle
/// entry; a repo that stays keeps both.
///
/// The repo paths have no `.git`, so nothing is actually watched — `start`
/// turns them down as unwatchable, which is all `sync` needs to run its prune.
#[test]
fn a_root_swap_prunes_the_queues_and_throttles_of_dropped_repos() {
    let h = Harness::new("sync-prune");
    let (kept, dropped) = (h.dir.join("kept"), h.dir.join("dropped"));
    h.open(vec![repo("kept", &kept), repo("dropped", &dropped)]);

    let state = h.state();
    drop(state.write_queues.try_read("kept"));
    drop(state.write_queues.try_read("dropped"));
    state.watchers.note_read("kept");
    state.watchers.note_read("dropped");
    assert_eq!(state.write_queues.len(), 2);

    h.open(vec![repo("kept", &kept)]);
    crate::sync_watchers(h.handle());

    assert_eq!(state.write_queues.len(), 1, "a dropped repo kept its write queue");
    assert!(!state.watchers.throttles("dropped"), "a dropped repo kept its throttle entry");
    assert!(state.watchers.throttles("kept"), "a repo still in the root lost its throttle entry");
}

/// The other branch of `sync_watchers`: the root closed outright. Everything
/// goes, because nothing is left that could need it.
#[test]
fn closing_the_root_prunes_every_queue_and_throttle() {
    let h = Harness::new("sync-close");
    let path = h.dir.join("repo");
    h.open(vec![repo("r", &path)]);

    let state = h.state();
    drop(state.write_queues.try_read("r"));
    state.watchers.note_read("r");

    h.close();
    crate::sync_watchers(h.handle());

    assert_eq!(state.write_queues.len(), 0, "a closed root's write queue survived");
    assert!(!state.watchers.throttles("r"), "a closed root's throttle entry survived");
}

/// A3's wiring: `cache::prune` is tested in `cache.rs`, and this checks
/// `remember_root` still calls it. Opening enough new roots pushes the first
/// off the recent list, and its cache file must go with it — while the
/// newest root, still on the list, keeps its own.
#[test]
fn a_root_pushed_off_the_recent_list_loses_its_cache_file() {
    let h = Harness::new("remember-prune");
    let cache_dir = h.state().cache_dir.clone();
    let empty = crate::cache::RootCache { version: crate::cache::CACHE_VERSION, ..Default::default() };

    let oldest = h.dir.join("oldest");
    crate::cache::save(&cache_dir, &oldest, &empty).unwrap();
    crate::remember_root(h.handle(), &oldest);

    // The newest root's file is written just before its own `remember_root`,
    // the order `open_root` has: the cache is read, then the root is
    // remembered. Written any earlier it is rightly pruned by an earlier call,
    // since the root is not on the list yet — which is what the first draft of
    // this test did.
    let newest = h.dir.join(format!("root-{}", crate::MAX_RECENT_ROOTS));
    for n in 1..=crate::MAX_RECENT_ROOTS {
        let root = h.dir.join(format!("root-{n}"));
        if root == newest {
            crate::cache::save(&cache_dir, &newest, &empty).unwrap();
        }
        crate::remember_root(h.handle(), &root);
    }

    let file = |root: &Path| cache_dir.join(format!("{}.json", crate::cache::hash_root(root)));
    assert!(!file(&oldest).exists(), "a root off the recent list kept its cache file");
    assert!(file(&newest).exists(), "a recent root lost its cache file");
}
