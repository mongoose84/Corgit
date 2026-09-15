mod atomicfile;
mod bulk;
mod branch;
mod cache;
mod commit;
mod diff;
mod discovery;
mod fetchsweep;
mod git;
mod graph;
mod ignore;
mod inflight;
mod menu;
mod problems;
mod remote;
mod roots;
mod settings;
mod status;
mod sweep;
#[cfg(test)]
mod testrepo;
mod watch;
mod writequeue;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;

use crate::cache::RootCache;
use crate::discovery::Repo;
use crate::git::GitInfo;
use crate::settings::Settings;
use crate::status::{FileChanges, RepoStatus};
use crate::writequeue::WriteQueues;

/// Long enough to be useful in *File → Open Recent*, short enough that the
/// welcome screen stays a list rather than a search problem.
const MAX_RECENT_ROOTS: usize = 10;

/// A failure was added to the Recent Problems ring (§13). Emitted so the
/// *other* windows learn about it — the one that triggered the operation
/// already has the error in hand.
const PROBLEM_EVENT: &str = "problem:recorded";

/// A write started or finished on a repo (§13, *Work in progress*). Unlike
/// `PROBLEM_EVENT`, the window that started the operation needs these too: it
/// knows it called `switch_branch`, but not that the call has stopped waiting
/// on the write queue and started doing something, and the row it has to mark
/// may not be the selected one.
const WRITE_BEGIN_EVENT: &str = "write:begin";
const WRITE_END_EVENT: &str = "write:end";

/// Rust owns the state; the frontend is a view over it (SPEC.md §9.3).
///
/// The global git semaphore lives in `git.rs` instead, as a static — it has to
/// cover every git spawn in the process (§7.3, §9.2), and routing it through
/// app state would only make that easier to get wrong.
struct AppState {
    config_dir: PathBuf,
    cache_dir: PathBuf,
    settings: Mutex<Settings>,
    /// Whether git exists, resolved *off* the startup path (§3).
    ///
    /// A cell rather than a value because the probe is a process spawn, and a
    /// process spawn is the one startup cost with no upper bound worth
    /// trusting: an anti-malware scan of a rarely-run binary, a revocation
    /// check that cannot reach its responder. Blocking `setup` on it — which
    /// this used to do — spent that time with the event loop stopped, so the
    /// window did not exist yet, and hitting `PROBE_TIMEOUT` meant five
    /// seconds of nothing followed by the wrong screen.
    ///
    /// `OnceCell` and not a plain spawn-and-store because `git_info` must be
    /// able to *wait* for the answer without racing it: whoever asks first
    /// runs the probe, everyone else awaits that same run.
    git: Arc<tokio::sync::OnceCell<GitInfo>>,
    root: Mutex<Option<RootState>>,
    /// Re-entrancy guard (§6): a sweep never starts while one is in flight.
    /// The tick is skipped, not queued.
    sweeping: AtomicBool,
    /// The periodic status-sweep ticker (§6 focus gating). `Some` only while
    /// the window is focused — aborted on blur so an unfocused window truly
    /// goes idle rather than merely skipping its own ticks, and restarted
    /// (with an immediate sweep) on refocus.
    ticker: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// The fetch sweep's own re-entrancy guard and ticker — a separate
    /// mechanism from the status sweep (§6: "these are different mechanisms
    /// and must not be conflated"), with its own interval, concurrency and
    /// focus gating.
    fetch_sweeping: AtomicBool,
    /// Set by `stop_bulk` and cleared at the start of every run (§5.1's Stop).
    /// A bulk run dispatches all of its tasks at once and they queue on the
    /// bulk semaphore, so this is checked *after* a task takes its permit —
    /// the last moment before the git process would be spawned, and the only
    /// point at which "has not started yet" is still true.
    bulk_cancelled: AtomicBool,
    fetch_ticker: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// One write queue per repo, shared process-wide (§7, §9.2). First
    /// used in build step 4, where staging and commit are the first writes.
    /// `Arc`-wrapped so the sweep can clone a handle into its per-repo tasks
    /// without needing an `AppHandle` there too (§6, §7 rule 2).
    write_queues: Arc<WriteQueues>,
    /// Which repos have a write running, for §13's *Work in progress*. Beside
    /// `write_queues` rather than inside it on purpose — see `inflight.rs`:
    /// the queue is a correctness guarantee, this is something to draw.
    in_flight: Arc<inflight::InFlightWrites>,
    /// FS watchers, one per repo (§6). No longer the hot set: on Windows a
    /// subtree watch is one handle whatever its depth, so every repo gets one
    /// and the sweep stops being the refresh mechanism.
    watchers: watch::RepoWatchers,
    /// The View menu's two checkboxes' actual state — the checkboxes
    /// themselves only mirror this (§9.3: Rust owns state). The only piece of
    /// the menu bar still held here now that it is drawn in the webview
    /// (§4.1); the rest of what a menu needs — the recent roots, whether a
    /// repo is selected — the frontend already has.
    pane_visibility: Mutex<menu::PaneVisibility>,
}

/// One window, one root (§9.1).
struct RootState {
    /// Bumped on every open. A sweep that outlives its root must not write its
    /// results over the new one's.
    generation: u64,
    path: PathBuf,
    repos: Vec<Repo>,
    statuses: HashMap<String, RepoStatus>,
    /// Keyed by repo id. A repo whose status could not be read is not clean —
    /// it is unknown, and the row has to say so rather than show a clean dot.
    errors: HashMap<String, String>,
    /// Unix seconds of each repo's last fetch attempt (§6, §9.5) — read by the
    /// fetch sweep to skip repos fetched within the last interval, persisted
    /// to the cache alongside `statuses`.
    last_fetch_at: HashMap<String, i64>,
    /// Repos whose background fetch most recently failed on what looks like
    /// an auth failure (§8.7, §13). The fetch sweep stops retrying a repo in
    /// this set; a manual fetch clears it, since the user is sitting right
    /// there and may resolve it (or fail again honestly). Not persisted — a
    /// fresh launch is a fair reason to try again.
    auth_needed: HashSet<String>,
    /// The hot set's user-controlled half (§5.1, §6) — the other half is
    /// whichever repo is currently selected. Persisted to `roots/<hash>.json`
    /// (§9.5), not the status cache: pins are truth, not something a sweep
    /// can regenerate.
    pins: HashSet<String>,
    /// Repos no FS watcher could be established for (§6) — a network share,
    /// a permissions failure, a `.git` file rather than a directory. These are
    /// the only ones the frequent sweep still has to cover, because everything
    /// else reports its own changes. Not persisted: whether a watch succeeds
    /// is a fact about right now, and a relaunch is entitled to try again.
    unwatched: HashSet<String>,
    /// Mirrors `pins` and the frontend's current selection so a relaunch can
    /// restore it (§9.5). Kept here rather than only in `roots.rs`'s on-disk
    /// copy so `set_selected_repo` has somewhere in memory to read it back
    /// from without a file read on every selection change.
    selected: Option<String>,
}

impl RootState {
    /// Adopt a fresh discovery scan, dropping what a repo that is no longer
    /// there leaves behind. Everything pruned here is regenerable — a status,
    /// an error, a fetch timestamp — so dropping it costs at most one sweep.
    ///
    /// **`pins` is deliberately not pruned**, and that omission is the whole
    /// reason this is a named method rather than a run of inline `retain`
    /// calls. A repo missing from *one* scan is not necessarily gone: a
    /// disconnected network drive, a folder briefly held by another process,
    /// and a repo mid-re-clone all look exactly like deletion from here. A
    /// pin is the user's own choice with no other source (§9.5 rule 5), so
    /// guessing wrong about it is not recoverable — and `set_selected_repo`
    /// writes this whole file on every selection change, which would turn the
    /// guess permanent on the user's very next click.
    ///
    /// A pin left behind for a repo that really is gone costs one path string
    /// and renders as nothing: the repo list and the hot-set watchers both
    /// walk `repos` and look pins up, never the reverse.
    fn adopt_repos(&mut self, repos: Vec<Repo>) {
        let known = |id: &String| repos.iter().any(|repo| &repo.id == id);

        self.statuses.retain(|id, _| known(id));
        self.errors.retain(|id, _| known(id));
        self.last_fetch_at.retain(|id, _| known(id));
        self.auth_needed.retain(known);
        self.unwatched.retain(known);
        // Unlike a pin, the selection is not the user's authored state — it is
        // where they happen to be — and a selection naming a repo the list no
        // longer shows would leave the middle pane describing nothing.
        if self.selected.as_ref().is_some_and(|id| !known(id)) {
            self.selected = None;
        }

        self.repos = repos;
    }

    /// Fold one sweep's results into the open root (§6).
    ///
    /// **A merge, not a replace**, and that is the whole of why this is a
    /// named method rather than two loops inside `sweep`. A repo whose write
    /// lock was held when the sweep reached it is skipped rather than waited
    /// for (§6, §7 rule 2), which means it is absent from *both* maps — so
    /// anything that assigned these wholesale would erase a repo's status for
    /// no reason other than that it was busy, and the row would blank every
    /// time the user staged something in it.
    ///
    /// The two maps are kept mutually exclusive in both directions, because
    /// the row draws from both: a repo that has just succeeded must lose the
    /// error it used to carry, and one that has just failed must lose the
    /// status, or `errors` says unknown while `statuses` still shows a clean
    /// dot and the row picks whichever it happens to read first.
    pub(crate) fn merge_sweep_results(
        &mut self,
        statuses: &HashMap<String, RepoStatus>,
        errors: &HashMap<String, String>,
    ) {
        for (id, status) in statuses {
            self.statuses.insert(id.clone(), status.clone());
            self.errors.remove(id);
        }
        for (id, err) in errors {
            self.errors.insert(id.clone(), err.clone());
            self.statuses.remove(id);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RootView {
    path: PathBuf,
    repos: Vec<Repo>,
    statuses: HashMap<String, RepoStatus>,
    errors: HashMap<String, String>,
    last_fetch_at: HashMap<String, i64>,
    auth_needed: HashSet<String>,
    pins: HashSet<String>,
    /// The repo selected when this root was last open, if it still exists
    /// (§9.5) — the frontend selects it on load so a relaunch drops you back
    /// where you left off.
    last_selected: Option<String>,
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state
        .settings
        .lock()
        .expect("settings mutex poisoned")
        .clone()
}

/// `(async)` for the reason in §1: this writes a file, and a command without
/// it is dispatched on the thread running the event loop. `%APPDATA%` is
/// normally a local disk and this is normally sub-millisecond — but it is
/// redirectable to a network share by policy, and the failure that buys is a
/// UI that freezes on a settings write rather than one that is slow to save.
/// The same applies to `toggle_pin`, `clear_pins` and `set_selected_repo`
/// below, which write the per-root file on a pin toggle and on every click in
/// the repo list.
#[tauri::command(async)]
fn save_settings(mut settings: Settings, state: State<'_, AppState>) -> Result<(), String> {
    let mut current = state.settings.lock().expect("settings mutex poisoned");

    // Recent roots are the backend's to write — `open_root` appends to them.
    // The frontend holds a snapshot taken at startup, so honouring its copy
    // here would silently drop every folder opened since.
    settings.recent_roots = current.recent_roots.clone();

    settings::save(&state.config_dir, &settings).map_err(|err| err.to_string())?;
    *current = settings;
    Ok(())
}

/// Waits for the probe rather than reporting "unknown", because the one caller
/// is the frontend deciding whether to show §3's blocking first-run screen and
/// there is no honest third answer to draw. It costs the frontend nothing to
/// wait: `repos.svelte.ts` starts optimistic and no longer holds up first paint
/// for this.
///
/// `AppHandle` rather than `State<'_, _>`: an async command taking a reference
/// is required to return `Result`, and there is nothing here that can fail.
#[tauri::command]
async fn git_info(app: AppHandle) -> GitInfo {
    // Cloned out before the await so no `State` guard is held across it.
    let git = app.state::<AppState>().git.clone();
    git.get_or_init(git::probe).await.clone()
}

/// The root to reopen on launch: the most recent one that still exists. A
/// renamed folder or a disconnected drive yields `None`, and the frontend
/// shows the welcome screen — never an empty repo list, never a crash (§9.1).
///
/// `(async)`, and taking an `AppHandle` so it can be: `is_dir` is a stat per
/// remembered root, and a disconnected drive is precisely the case where that
/// stat waits on a timeout rather than on a disk. On the main thread that wait
/// is the window not appearing.
#[tauri::command(async)]
fn initial_root(app: AppHandle) -> Option<PathBuf> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().expect("settings mutex poisoned");
    settings
        .recent_roots
        .iter()
        .find(|root| root.is_dir())
        .cloned()
}

/// The native folder picker, driven from Rust so no dialog permission has to
/// be handed to the webview.
#[tauri::command]
async fn pick_root(app: AppHandle) -> Option<PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();

    app.dialog()
        .file()
        .set_title("Open folder")
        .pick_folder(move |picked| {
            let _ = tx.send(picked);
        });

    rx.await.ok().flatten().and_then(|path| path.into_path().ok())
}

/// Discovery is synchronous and sub-millisecond (§8.1), so this returns a
/// paintable repo list immediately and leaves git to the sweep. First paint
/// never waits on git (§1) — rows arrive filled in from the on-disk cache,
/// stale by at most one sweep interval, and the sweep corrects them (§6).
/// `(async)` on a synchronous body, which is the whole of the fix: a
/// `#[tauri::command]` without it is dispatched on the thread running the
/// event loop, so every stat below — one directory read plus two per child
/// (§8.1), a cache file, a subtree handle per repo (§6) — was time the window
/// spent not painting and not accepting input. Warm that is ~120 ms and
/// invisible. Cold, with each of those opens going through a filter driver, it
/// is the whole of "Corgit took ages to open".
///
/// It still blocks *a* thread, now one of the async runtime's, which is the
/// honest cost of not restructuring a body that is all `std::fs`. That thread
/// is one of several and none of them paint.
#[tauri::command(async)]
fn open_root(path: PathBuf, app: AppHandle) -> Result<RootView, String> {
    let opened = Instant::now();

    let root = discovery::canonicalize(&path);
    if !root.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }

    let repos = discovery::scan(&root);
    let scanned = opened.elapsed();
    let state = app.state::<AppState>();

    let mut cached = cache::load(&state.cache_dir, &root);
    let loaded = opened.elapsed();
    // A repo that no longer exists under this root has nothing left to
    // correct it, so it must not linger in the view.
    cached.statuses.retain(|id, _| repos.iter().any(|repo| &repo.id == id));
    cached.last_fetch_at.retain(|id, _| repos.iter().any(|repo| &repo.id == id));

    // Pins are carried over exactly as stored — never filtered against this
    // scan. Same reasoning as `RootState::adopt_repos`: a repo missing right
    // now may be a disconnected drive rather than a deletion, and this value
    // is what the next `set_selected_repo` writes back to disk.
    let root_settings = roots::load(&state.config_dir, &root);
    let last_selected = root_settings
        .last_selected
        .clone()
        .filter(|id| repos.iter().any(|repo| &repo.id == id));

    let generation = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let generation = current.as_ref().map_or(0, |root| root.generation) + 1;
        *current = Some(RootState {
            // Filled in by `sync_watchers` below, once we know which repos
            // would actually take a watch. Empty means "everything is
            // watched", so the first frequent tick before that call is a
            // no-op rather than a full sweep.
            unwatched: HashSet::new(),
            generation,
            path: root.clone(),
            repos: repos.clone(),
            statuses: cached.statuses.clone(),
            errors: HashMap::new(),
            last_fetch_at: cached.last_fetch_at.clone(),
            auth_needed: HashSet::new(),
            pins: root_settings.pins.clone(),
            selected: last_selected.clone(),
        });
        generation
    };

    remember_root(&app, &root);
    sweep::start_sweep(&app, generation, repos.clone());
    sync_watchers(&app);

    // Split rather than a single total, because the three phases fail for
    // different reasons: `scan` is one directory read plus two stats per child
    // (§8.1), `cache` is one file, and `watchers` is a subtree handle per repo
    // (§6). A cold start that is slow in only one of them says which.
    let watched = opened.elapsed();
    log::info!(
        "open_root {}: scan {} ms ({} repos), cache {} ms, sweep+watchers {} ms, total {} ms",
        root.display(),
        scanned.as_millis(),
        repos.len(),
        (loaded - scanned).as_millis(),
        (watched - loaded).as_millis(),
        watched.as_millis(),
    );

    Ok(RootView {
        path: root,
        repos,
        statuses: cached.statuses,
        errors: HashMap::new(),
        last_fetch_at: cached.last_fetch_at,
        auth_needed: HashSet::new(),
        pins: root_settings.pins,
        last_selected,
    })
}

/// What the frontend asks for when it reloads — the whole current view,
/// statuses included, so a reload does not have to re-sweep.
#[tauri::command]
fn current_root(state: State<'_, AppState>) -> Option<RootView> {
    let current = state.root.lock().expect("root mutex poisoned");
    current.as_ref().map(|root| RootView {
        path: root.path.clone(),
        repos: root.repos.clone(),
        statuses: root.statuses.clone(),
        errors: root.errors.clone(),
        last_fetch_at: root.last_fetch_at.clone(),
        auth_needed: root.auth_needed.clone(),
        pins: root.pins.clone(),
        // A reload keeps whatever is already selected in the frontend rather
        // than re-forcing the on-open default — this field only matters for a
        // fresh `open_root`.
        last_selected: root.selected.clone(),
    })
}

/// Rescan the root and sweep again. Discovery is repeated because a repo may
/// have been cloned or deleted since the folder was opened, so the caller gets
/// the new repo list back rather than only the statuses the sweep will emit.
///
/// `(async)` for the same reason as `open_root`: it repeats that scan and
/// rebuilds the watchers, so on the main thread it froze the window on every
/// F5 rather than only at launch.
#[tauri::command(async)]
fn refresh_root(app: AppHandle) -> Result<RootView, String> {
    let state = app.state::<AppState>();

    let (generation, view) = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let Some(root) = current.as_mut() else {
            return Err("No folder is open".to_string());
        };

        // Otherwise a deleted repo keeps its last known status forever. See
        // `adopt_repos` for why pins are the one thing this does not touch.
        root.adopt_repos(discovery::scan(&root.path));

        (
            root.generation,
            RootView {
                path: root.path.clone(),
                repos: root.repos.clone(),
                statuses: root.statuses.clone(),
                errors: root.errors.clone(),
                last_fetch_at: root.last_fetch_at.clone(),
                auth_needed: root.auth_needed.clone(),
                pins: root.pins.clone(),
                last_selected: root.selected.clone(),
            },
        )
    };

    sweep::start_sweep(&app, generation, view.repos.clone());
    sync_watchers(&app);
    Ok(view)
}

/// Resolve a repo id against the currently open root's repo list — every
/// mutating command and `repo_files` starts here.
fn repo_path(app: &AppHandle, repo_id: &str) -> Result<PathBuf, String> {
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    let root = current.as_ref().ok_or_else(|| "No folder is open".to_string())?;
    root.repos
        .iter()
        .find(|repo| repo.id == repo_id)
        .map(|repo| repo.path.clone())
        .ok_or_else(|| "That repository is no longer open".to_string())
}

/// The middle pane's file list (§5.2), fetched on demand for the selected
/// repo only — never swept for all 77, which is what keeps `RepoStatus`
/// (§1's 150 MB budget) to counts alone. Waits for any in-flight write on
/// this repo rather than skipping (§7 rule 2): unlike the sweep, this is a
/// one-off user-triggered read with nothing sensible to show if it bails.
#[tauri::command]
async fn repo_files(repo_id: String, app: AppHandle) -> Result<FileChanges, String> {
    let path = repo_path(&app, &repo_id)?;
    let _read_guard = app.state::<AppState>().write_queues.read(&repo_id).await;
    status::query_files(&path).await
}

#[tauri::command]
async fn stage_paths(repo_id: String, paths: Vec<String>, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Stage", |path| async move { commit::stage(&path, &paths).await })
        .await
}

#[tauri::command]
async fn unstage_paths(repo_id: String, paths: Vec<String>, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Unstage", |path| async move { commit::unstage(&path, &paths).await })
        .await
}

/// Discard the unstaged changes to these paths (§5.2, §8.6). The same shape as
/// every other write — the destructive part lives entirely in
/// `commit::discard`'s flags, and the confirmation entirely in the frontend.
#[tauri::command]
async fn discard_paths(repo_id: String, paths: Vec<String>, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Discard", |path| async move { commit::discard(&path, &paths).await })
        .await
}

/// Delete these untracked paths from the working tree (§5.2, §8.6) — the only
/// thing Corgit does that git cannot undo *at all*, since an untracked file has
/// never been in the index and so exists in no object git holds. Everything
/// that makes that survivable is elsewhere: `commit::delete_untracked`'s flags
/// and literal pathspecs, the frontend's untracked-only filter, and a
/// confirmation modal listing every path.
///
/// Same shape as every other write regardless, which is the point of §7 rule 1
/// — the most dangerous command in the app gets no bespoke handling.
#[tauri::command]
async fn delete_paths(repo_id: String, paths: Vec<String>, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Delete", |path| async move {
        commit::delete_untracked(&path, &paths).await
    })
    .await
}

/// Right-click ▸ Ignore on an untracked file row (§5.2). Goes through
/// `write_and_refresh` like every other mutation despite spawning no git at
/// all: it holds the repo's write lock, which is what keeps a `.gitignore`
/// read-modify-write from racing a second window (§7 rule 1), and it publishes
/// the repo's status afterwards, which is what makes the newly-ignored rows
/// leave the pane and the `.gitignore` change appear in it.
///
/// `patterns`, not paths. The frontend derives both the menu label and the
/// line from one function (`ignorePatterns.ts`) so the two cannot say
/// different things, and gitignore's escaping rules belong next to the label
/// that promises what they do.
#[tauri::command]
async fn append_gitignore(repo_id: String, patterns: Vec<String>, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Ignore", |path| async move { ignore::append(&path, &patterns) }).await
}

#[tauri::command]
async fn stage_all(repo_id: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Stage all", |path| async move { commit::stage_all(&path).await }).await
}

#[tauri::command]
async fn unstage_all(repo_id: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Unstage all", |path| async move { commit::unstage_all(&path).await }).await
}

#[tauri::command]
async fn commit_repo(repo_id: String, message: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Commit", |path| async move { commit::commit(&path, &message).await })
        .await
}

/// The graph pane's history, one page at a time (§5.3, §8.4). Waits for any
/// in-flight write like `repo_files` does — a one-off read with nothing
/// sensible to show if it raced a commit landing.
#[tauri::command]
async fn graph_page(repo_id: String, skip: usize, app: AppHandle) -> Result<graph::GraphPage, String> {
    let path = repo_path(&app, &repo_id)?;
    let _read_guard = app.state::<AppState>().write_queues.read(&repo_id).await;
    graph::log(&path, skip).await
}

/// Ref badges for the graph pane (§5.3, §8.3) — fetched alongside each reload
/// rather than swept for all 77, same reasoning as `repo_files`.
#[tauri::command]
async fn graph_refs(repo_id: String, app: AppHandle) -> Result<Vec<graph::RefBadge>, String> {
    let path = repo_path(&app, &repo_id)?;
    let _read_guard = app.state::<AppState>().write_queues.read(&repo_id).await;
    graph::refs(&path).await
}

/// A single commit's details for the middle pane's Mode B (§5.2, §8.5). Waits
/// for any in-flight write like `graph_page`/`graph_refs` — a one-off read
/// with nothing sensible to show if it raced a commit landing.
#[tauri::command]
async fn commit_details(repo_id: String, hash: String, app: AppHandle) -> Result<graph::CommitDetails, String> {
    let path = repo_path(&app, &repo_id)?;
    let _read_guard = app.state::<AppState>().write_queues.read(&repo_id).await;
    graph::details(&path, &hash).await
}

/// One file's diff for the right pane's second view (§5.4, §8.8). Waits for
/// any in-flight write like `repo_files` and `commit_details` do — a diff read
/// mid-`git add` would describe an index that no longer exists by the time it
/// painted.
#[tauri::command]
async fn file_diff(
    repo_id: String,
    path: String,
    source: diff::DiffSource,
    app: AppHandle,
) -> Result<diff::FileDiff, String> {
    let repo = repo_path(&app, &repo_id)?;
    let _read_guard = app.state::<AppState>().write_queues.read(&repo_id).await;
    diff::file(&repo, &path, &source).await
}

/// Branch switching (§8.3, §8.4 badges — build step 8): the graph shows every
/// ref, so unlike the switcher (deferred), this takes whichever badge the
/// user double-clicked or picked from its context menu and dispatches on its
/// kind. A dirty-tree failure lands here the same as any other — the frontend
/// already knows the selected repo's dirty state from its status and decides
/// whether to offer *Open in VS Code* from that, rather than this command
/// trying to classify git's stderr (§8.3: never force-checkout).
#[tauri::command]
async fn switch_branch(repo_id: String, name: String, kind: graph::RefKind, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Switch branch", |path| async move {
        match kind {
            graph::RefKind::Local => branch::switch_local(&path, &name).await,
            graph::RefKind::Remote => branch::switch_remote_tracking(&path, &name).await,
        }
    })
    .await
}

/// Branch creation from the graph (§8.3) — right-click a ref badge or a commit
/// row. `start_point` is whatever that badge/row names (a branch name or a
/// commit hash), never HEAD, so the branch starts where the user pointed.
/// `checkout` mirrors the dialog's checkbox; when it is set, a dirty-tree
/// failure surfaces exactly like a plain switch's does.
#[tauri::command]
async fn create_branch(
    repo_id: String,
    name: String,
    start_point: String,
    checkout: bool,
    app: AppHandle,
) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Create branch", |path| async move {
        branch::create(&path, &name, &start_point, checkout).await
    })
    .await
}

/// Deleting a local branch (§8.3) — right-click its badge in the graph. The
/// menu only offers this for local badges that are not the checked-out branch;
/// `force` is never set by that first click, only by the *Delete anyway* the
/// dialog grows once git has refused an unmerged branch.
///
/// Nothing here re-validates either condition. The frontend's checks are about
/// which entries to *show*, and re-deriving them from cached status on this
/// side would be deciding from the cache (§5.1) what git already knows for
/// certain — a delete that should not happen fails as git's own error, which
/// is the outcome anyway.
#[tauri::command]
async fn delete_branch(repo_id: String, name: String, force: bool, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Delete branch", |path| async move { branch::delete(&path, &name, force).await }).await
}

/// Merging a branch into the checked-out one (§8.3) — right-click a ref badge
/// in the graph. Only the source is named: the destination is HEAD, decided by
/// git at the moment it runs rather than by anything Corgit has cached, for the
/// same reason `remote::publish` pushes `HEAD` (§5.1 — the cache is never truth).
///
/// A conflict fails here like any other error, and that is the whole recovery
/// story: `write_and_refresh` republishes the status regardless of outcome, so
/// §13's conflict banner — *Abort merge* and *Open in VS Code* — is already on
/// screen by the time the frontend shows the message.
#[tauri::command]
async fn merge_branch(repo_id: String, name: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Merge", |path| async move { branch::merge(&path, &name).await }).await
}

/// The dirty-tree checkout failure's other half (§8.3): launches VS Code on
/// the repo so the user can resolve things by hand. Fire-and-forget — nothing
/// in Corgit's own state changes because of it.
///
/// `file` opens one file *as well as* the repo (§5.4's escape hatch): the
/// folder always goes on the command line first so VS Code opens it as the
/// workspace, because a lone file argument gives a window with no repo around
/// it — no source control, no search, which is the context that made opening
/// VS Code worth offering. `line` is appended as `-g <file>:<line>` so the
/// editor lands on the first change rather than at the top of the file.
#[tauri::command]
async fn open_in_vscode(
    repo_id: String,
    file: Option<String>,
    line: Option<u32>,
    app: AppHandle,
) -> Result<(), String> {
    let path = repo_path(&app, &repo_id)?;

    // VS Code's Windows entry point is `code.cmd`, a shim, and Windows does not
    // walk PATHEXT for a bare child process the way a shell does — so the name
    // has to be resolved before spawning. This used to hand it to `cmd /C`
    // instead, which resolved it by putting a shell between Corgit and these
    // arguments; see `program_on_path` for why that was not survivable.
    //
    // A missing VS Code is now an error rather than a spawn that succeeds and
    // does nothing: `cmd` exists whether or not `code` does, so the old form
    // reported success and left the user looking at a window that never
    // opened — precisely what §13 forbids, and what `openInVSCode`'s own
    // error handling in `repos.svelte.ts` was already written to catch.
    #[cfg(windows)]
    let mut command = tokio::process::Command::new(program_on_path("code").ok_or_else(|| {
        "could not find VS Code — install it, or re-run its installer with \
         \"Add to PATH\" ticked"
            .to_string()
    })?);
    #[cfg(not(windows))]
    let mut command = tokio::process::Command::new("code");

    command.arg(&path);

    if let Some(file) = file {
        // Joined here rather than trusted from the frontend: `file` is a
        // repo-relative path out of `git status`, and the repo root is the
        // only thing that can turn it into something VS Code can open.
        let target = path.join(&file);
        let target = target.to_string_lossy();
        command.arg("-g");
        match line {
            Some(line) => command.arg(format!("{target}:{line}")),
            None => command.arg(target.as_ref()),
        };
    }

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .map_err(|err| format!("could not launch VS Code: {err}"))?;
    Ok(())
}

/// Right-click → Open in Terminal (§5.1). Unlike every git spawn, this one is
/// meant to leave a visible window behind, so `CREATE_NO_WINDOW` is
/// deliberately never set here.
#[tauri::command]
async fn open_in_terminal(repo_id: String, app: AppHandle) -> Result<(), String> {
    let path = repo_path(&app, &repo_id)?;

    let mut command = if cfg!(windows) {
        match windows_terminal_on_path() {
            // `wt -d <path>` opens straight into the repo; no separate `cd`.
            Some(wt) => {
                let mut command = tokio::process::Command::new(wt);
                command.arg("-d").arg(&path);
                command
            }
            None => {
                let mut command = tokio::process::Command::new("cmd");
                command.arg("/K").current_dir(&path);
                command
            }
        }
    } else {
        let mut command = tokio::process::Command::new("sh");
        command.current_dir(&path);
        command
    };

    command
        .spawn()
        .map_err(|err| format!("could not open a terminal: {err}"))?;
    Ok(())
}

/// Right-click ▸ Reveal in File Explorer on a file row (§5.2). Fire-and-forget
/// like `open_in_terminal`, and `cfg`-gated for the same reason as `menu.rs`'s
/// log folder — there is no portable "reveal this file".
///
/// `path` is repo-relative, straight off `git status`, so the repo root is what
/// turns it into something the shell can open — the same join `open_in_vscode`
/// does, and for the same reason.
///
/// A deleted file has nothing to select, and `explorer` given a path that does
/// not exist silently opens Documents instead, which reads as the app having
/// gone wrong rather than as "that file is gone". So the target falls back to
/// the nearest ancestor that does exist — worst case the repo root, which is
/// always there.
#[tauri::command]
async fn reveal_in_explorer(repo_id: String, path: String, app: AppHandle) -> Result<(), String> {
    let root = repo_path(&app, &repo_id)?;
    let target = root.join(&path);

    if target.exists() {
        return reveal_existing(&target);
    }

    let fallback = target
        .ancestors()
        .skip(1)
        .find(|dir| dir.exists())
        .unwrap_or(root.as_path())
        .to_path_buf();
    open_folder(&fallback)
}

/// `git status` reports paths with forward slashes on every platform, so
/// joining one onto a Windows root leaves `C:\repo/src/file.rs`. Everything
/// that *opens* a path takes that as-is — `Path::exists` and `open_in_vscode`
/// both do — but `explorer` does not open its argument, it parses it, and one
/// forward slash makes it give up and show the user's home folder instead.
/// That is the same silent wrong-window failure this pair already guards
/// against for missing files, arriving by another route, so the separators are
/// flipped here rather than at the join: explorer is the only thing that cares.
#[cfg(windows)]
fn explorer_arg(path: &Path) -> String {
    path.display().to_string().replace('/', "\\")
}

/// `explorer /select,<file>` opens the containing folder with the file
/// highlighted. The argument is written raw because explorer parses its own
/// command line rather than reading argv: Rust's quoting would hand it
/// `"/select,C:\a b\f.txt"` as a single quoted token, which it takes for a
/// folder name and gives up on. Quoting the path alone is the form it does
/// understand.
#[cfg(windows)]
fn reveal_existing(target: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    // `explorer` exits non-zero even when it succeeds (menu.rs's log folder
    // says the same), so spawning is the only thing worth checking here.
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", explorer_arg(target)))
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not open File Explorer: {err}"))
}

/// No portable "select this file" (§10) — the containing folder is as close as
/// this gets off Windows, and v1 ships Windows.
#[cfg(not(windows))]
fn reveal_existing(target: &Path) -> Result<(), String> {
    open_folder(target.parent().unwrap_or(target))
}

fn open_folder(dir: &Path) -> Result<(), String> {
    // Same separator trap as `reveal_existing`, and the fallback path is the
    // one most likely to carry a slash: it is an ancestor of a git-reported
    // path. Explorer takes a mixed-separator folder to mean Documents.
    #[cfg(windows)]
    let (opener, arg) = ("explorer", explorer_arg(dir));
    #[cfg(not(windows))]
    let (opener, arg) = ("xdg-open", dir.display().to_string());

    std::process::Command::new(opener)
        .arg(arg)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not open the folder: {err}"))
}

/// Resolve a bare program name against `PATH` the way a shell would, trying
/// each `PATHEXT` extension in turn.
///
/// This exists so VS Code is spawned *directly* rather than through
/// `cmd /C code`, which is what `open_in_vscode` used to do. cmd re-parses the
/// command line it is handed, and Rust quotes an argument only when it
/// contains a space or a tab — so a repo folder or a file named `build&deploy`
/// reached cmd unquoted and the `&` in it started a second command. `&`, `|`,
/// `^`, `<` and `>` are all legal in Windows filenames, and the name can
/// arrive in a repo someone cloned, which made a right-click on a file row
/// enough to run it. Resolving the shim here takes the shell out of the path
/// entirely, which is the only fix that holds: no amount of quoting survives
/// a second parser with different rules.
///
/// Spawning the resolved `.cmd` is then safe on its own terms — CVE-2024-24576
/// is this same hazard from the other direction, and std has escaped a batch
/// file's arguments since the `rust-version = "1.77.2"` floor in Cargo.toml.
#[cfg(windows)]
fn program_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let extensions = path_extensions(std::env::var("PATHEXT").ok().as_deref());

    for dir in std::env::split_paths(&path) {
        for extension in &extensions {
            let candidate = dir.join(format!("{name}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The extensions Windows tries for a bare program name, in the order its
/// shell tries them. Read from `PATHEXT` rather than hard-coded so a machine
/// that has added one still resolves it, falling back to the documented
/// default that every Windows install has shipped since NT.
///
/// `.CMD` is the entry that matters and the reason this is not simply
/// `[".EXE"]`: VS Code's Windows entry point is `code.cmd`. A resolver that
/// missed it would leave *Open in VS Code* permanently unavailable, which is
/// the failure this whole change was made to stop having a silent version of.
///
/// Takes the value rather than reading the environment so the list is testable
/// without setting a process-wide variable.
#[cfg(windows)]
fn path_extensions(pathext: Option<&str>) -> Vec<String> {
    const DEFAULT: &str = ".COM;.EXE;.BAT;.CMD";

    pathext
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT)
        .split(';')
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(windows)]
fn windows_terminal_on_path() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("wt.exe"))
        .find(|candidate| candidate.is_file())
}

/// Only Windows has Windows Terminal to look for (§10 — the `not(windows)`
/// half exists purely so `open_in_terminal` above can call this
/// unconditionally rather than branching on `cfg!` around the call itself).
#[cfg(not(windows))]
fn windows_terminal_on_path() -> Option<PathBuf> {
    None
}

/// Pin/unpin a repo (§5.1) — persisted immediately, same reasoning as
/// `remember_root`: a pin toggle is a deliberate, infrequent user action, not
/// something worth debouncing.
#[tauri::command(async)]
fn toggle_pin(repo_id: String, app: AppHandle) -> Result<HashSet<String>, String> {
    let state = app.state::<AppState>();
    let (root_path, pins, selected) = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let root = current.as_mut().ok_or_else(|| "No folder is open".to_string())?;
        if !root.repos.iter().any(|repo| repo.id == repo_id) {
            return Err("That repository is no longer open".to_string());
        }
        if !root.pins.remove(&repo_id) {
            root.pins.insert(repo_id);
        }
        (root.path.clone(), root.pins.clone(), root.selected.clone())
    };

    persist_root_settings(&app, &root_path, pins.clone(), selected);
    Ok(pins)
}

/// Unpin everything in one go (§5.1). A loop of `toggle_pin` from the
/// frontend would do the same thing, but it would write `roots/<hash>.json`
/// once per pin — this is one write, and it cannot leave a half-cleared set
/// behind if a call in the middle fails.
#[tauri::command(async)]
fn clear_pins(app: AppHandle) -> Result<HashSet<String>, String> {
    let state = app.state::<AppState>();
    let (root_path, selected) = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let root = current.as_mut().ok_or_else(|| "No folder is open".to_string())?;
        root.pins.clear();
        (root.path.clone(), root.selected.clone())
    };

    persist_root_settings(&app, &root_path, HashSet::new(), selected);
    Ok(HashSet::new())
}

/// *Help ▸ Recent Problems…* (§13, §4.1). Reads the process-wide ring, so
/// every window sees the same history of the same herd (§9.2).
#[tauri::command]
fn recent_problems() -> Vec<problems::Problem> {
    problems::recent()
}

/// *Clear* in the Problems window. Empties the ring only — `corgit.log` keeps
/// everything, because clearing a view of the record must not destroy the
/// record. Same rule as §13's suppression, which silences a notification and
/// never a condition.
#[tauri::command]
fn clear_problems(app: AppHandle) {
    problems::clear();
    // The list is process-wide, so a second window showing the old entries
    // after this would be showing something that no longer exists.
    if let Err(err) = app.emit(PROBLEM_EVENT, ()) {
        log::warn!("could not publish the problems clear ({err})");
    }
}

/// The frontend's current selection, mirrored server-side (§9.5's persisted
/// `last_selected`, and the input to the hot set in §6/build step 9's
/// watchers). `None` when nothing is selected.
#[tauri::command(async)]
fn set_selected_repo(repo_id: Option<String>, app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (root_path, pins, selected) = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let root = current.as_mut().ok_or_else(|| "No folder is open".to_string())?;
        root.selected = repo_id;
        (root.path.clone(), root.pins.clone(), root.selected.clone())
    };

    persist_root_settings(&app, &root_path, pins, selected.clone());
    // Selection used to resync the watchers from here, because it was half of
    // the hot set that defined them (§6). Every repo is watched now, so there
    // is nothing to resync — and `emit_repo_status` reads this field to decide
    // whether to carry the file list, which is the only thing selection still
    // changes on this side.
    //
    // Repository ▸ Fetch/Pull/Push used to be enabled and disabled from here,
    // because a native menu item's enabled state is a thing you set. The
    // frontend menu derives it from `repos.selectedId` instead (§4.1's table),
    // which is the same selection this command is mirroring — so there is
    // nothing left to push.
    Ok(())
}

/// The one place that writes `roots/<hash>.json` (§9.5): every caller hands
/// over the whole current snapshot, same reasoning as `persist_cache` below —
/// the file is overwritten wholesale, so a partial write would silently erase
/// the other half on disk.
fn persist_root_settings(app: &AppHandle, root_path: &Path, pins: HashSet<String>, last_selected: Option<String>) {
    let state = app.state::<AppState>();
    let settings = roots::RootSettings { version: roots::ROOTS_VERSION, pins, last_selected };
    if let Err(err) = roots::save(&state.config_dir, root_path, &settings) {
        // `error`, not `warn`: unlike the status cache this file is the only
        // copy of the user's pins (§9.5 rule 5), so a failed write here is
        // silent data loss rather than a cache miss.
        log::error!("could not save root settings ({err})");
    }
}

/// A manual, user-triggered fetch — allowed to prompt interactively, unlike
/// the background fetch sweep (§8.7). Clears "auth needed" regardless of
/// outcome: the user is sitting right there, and this attempt's own result is
/// the freshest signal about whether the repo still needs attention.
#[tauri::command]
async fn fetch_repo(repo_id: String, app: AppHandle) -> Result<(), String> {
    let result = write_and_refresh(&app, repo_id.clone(), "Fetch", |path| async move { remote::fetch(&path).await }).await;
    fetchsweep::record_fetch_attempt(&app, &repo_id);
    result
}

#[tauri::command]
async fn pull_repo(repo_id: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Pull", |path| async move { remote::pull(&path).await }).await
}

#[tauri::command]
async fn push_repo(repo_id: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Push", |path| async move { remote::push(&path).await }).await
}

/// Branch names per repo, for both multi-repo dialogs — Create Branch's
/// duplicate check (§8.3) and Switch & pull's picker (§5.1). Read concurrently,
/// and each behind that repo's read guard like every other one-off read (§7
/// rule 2) — a branch list read mid-`switch` would describe refs that have
/// already moved.
///
/// Uncapped on purpose, unlike the sweep: this runs over the repos in one
/// dialog, not the root, and the global semaphore of eight is still the ceiling
/// (§7 rule 3).
#[tauri::command]
async fn repo_branches(app: AppHandle, repo_ids: Vec<String>) -> Vec<RepoBranches> {
    let tasks: Vec<_> = repo_ids
        .into_iter()
        .map(|repo_id| {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let read = match repo_path(&app, &repo_id) {
                    Ok(path) => {
                        let _read_guard = app.state::<AppState>().write_queues.read(&repo_id).await;
                        branch::name_sets(&path).await
                    }
                    Err(message) => Err(message),
                };
                match read {
                    Ok(sets) => RepoBranches {
                        repo_id,
                        local: Some(sets.local),
                        remote: Some(sets.remote),
                        error: None,
                    },
                    Err(message) => {
                        RepoBranches { repo_id, local: None, remote: None, error: Some(message) }
                    }
                }
            })
        })
        .collect();

    let mut out = Vec::with_capacity(tasks.len());
    for task in tasks {
        if let Ok(entry) = task.await {
            out.push(entry);
        }
    }
    out
}

/// One repo's branch names. The two lists move together and are exclusive with
/// `error`, and the dialogs need both halves: a repo whose refs could not be
/// read must not silently render as a repo with no branches, which would be a
/// green light to create a name that is already there — and, in Switch & pull,
/// a row excluded for lacking a branch it may well have.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoBranches {
    repo_id: String,
    local: Option<Vec<String>>,
    /// Remote-only membership is a real answer rather than a footnote: it is
    /// what makes a row read `new  main → develop` instead of being excluded.
    remote: Option<Vec<String>>,
    error: Option<String>,
}

/// The open root's repos whose ids are in `repo_ids`, in the **root's** order
/// rather than the caller's.
///
/// That is the order the list is drawn in, so it is the order the run's
/// failures should be named in when they reach the banner (§5.1). Ids the root
/// no longer lists are dropped rather than erroring: a rescan between opening
/// the dialog and pressing Create is a repo that went away, not a failure.
fn repos_by_id(app: &AppHandle, repo_ids: &[String]) -> Result<Vec<Repo>, String> {
    let wanted: HashSet<&str> = repo_ids.iter().map(String::as_str).collect();
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    let root = current.as_ref().ok_or_else(|| "No folder is open".to_string())?;
    Ok(root.repos.iter().filter(|repo| wanted.contains(repo.id.as_str())).cloned().collect())
}

/// The open root's repo list, cloned out from under the lock so the caller can
/// await without holding it.
fn open_repos(app: &AppHandle) -> Result<Vec<Repo>, String> {
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    let root = current.as_ref().ok_or_else(|| "No folder is open".to_string())?;
    Ok(root.repos.clone())
}

/// §13's merge-conflict banner: "Abort merge", one of its exactly two ways
/// out (the other is *Open in VS Code*).
#[tauri::command]
async fn merge_abort(repo_id: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, "Abort merge", |path| async move { remote::merge_abort(&path).await }).await
}

/// A branch with no upstream configured (§8.7). `remote::publish` pushes
/// `HEAD`, so the known branch name is only a guard here — it turns the one
/// case git would refuse anyway, a detached HEAD, into a sentence that says so.
#[tauri::command]
async fn publish_branch(repo_id: String, app: AppHandle) -> Result<(), String> {
    current_branch(&app, &repo_id)?;
    write_and_refresh(&app, repo_id, "Publish branch", |path| async move { remote::publish(&path).await }).await
}

/// Commit, then push in one step. Whether that push needs `-u origin` is
/// decided up front from the known status, before the commit runs, since a
/// fresh commit does not change what the branch tracks.
///
/// That decision is `status::needs_publish`, the same rule the frontend's
/// `needsPublish` uses to label the button — one press, two decisions, and the
/// only reason they are two is that this one happens on the far side of the
/// IPC boundary. Deciding it here with a *narrower* rule than the label used
/// is what made this the last path still failing on a branch whose upstream
/// name did not match: the button read "Publish Branch" and this ran `push`.
///
/// A stale answer costs nothing worse than a clear error: `push` on a branch
/// that turns out to have no upstream stops with git's own "no upstream"
/// message, and `publish` on one that turns out to have an upstream re-points
/// it at the branch's own name. Neither can push a branch other than the one
/// just committed to, because both refspecs resolve `HEAD` when git runs.
#[tauri::command]
async fn commit_and_push(repo_id: String, message: String, app: AppHandle) -> Result<(), String> {
    // Detached HEAD fails here rather than after `commit::commit` has already
    // written a commit that the push would then not carry. It is its own check
    // rather than a consequence of `needs_publish`, which reports `false` for
    // a detached HEAD (there is no branch to publish) and would otherwise send
    // it down the plain-push path to fail in git's words instead of ours.
    let needs_publish = publish_needed(&app, &repo_id)?;

    write_and_refresh(&app, repo_id, "Commit + Push", |path| async move {
        commit::commit(&path, &message).await?;
        if needs_publish { remote::publish(&path).await } else { remote::push(&path).await }
    })
    .await
}

/// Read from the currently known status rather than querying git fresh — this
/// only ever decides whether to *refuse*, never what gets pushed, so a stale
/// answer cannot send a commit somewhere unintended.
fn current_branch(app: &AppHandle, repo_id: &str) -> Result<String, String> {
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    let root = current.as_ref().ok_or_else(|| "No folder is open".to_string())?;
    root.statuses
        .get(repo_id)
        .and_then(|status| status.branch.clone())
        .ok_or_else(|| "No branch to publish (detached HEAD)".to_string())
}

/// Push or publish, for Commit & Push — `status::needs_publish` asked against
/// the cached status, erroring on the detached HEAD that neither can serve.
///
/// Read from the currently known status for the same reason `current_branch`
/// is: this only picks between two commands that both push `HEAD`, so a stale
/// answer cannot send a commit somewhere unintended. The worst case is a clear
/// error — `push` on a branch that turns out to need publishing stops with
/// git's own message, and `publish` on one that did not re-points an upstream
/// at the branch it was already tracking.
fn publish_needed(app: &AppHandle, repo_id: &str) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    let root = current.as_ref().ok_or_else(|| "No folder is open".to_string())?;
    let status = root.statuses.get(repo_id).ok_or_else(|| "That repository has no status yet".to_string())?;
    let branch = status
        .branch
        .as_deref()
        .ok_or_else(|| "No branch to publish (detached HEAD)".to_string())?;

    Ok(status::needs_publish(branch, status.upstream.as_deref()))
}

/// §13's *Work in progress*, published for one repo. `operation` is the
/// user's word for what they asked — "Switch branch", "Pull" — the same one
/// the Problems record and the error banner use, so the row's tooltip and the
/// failure that may follow it name the same thing.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WriteBeginEvent {
    repo_id: String,
    operation: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WriteEndEvent {
    repo_id: String,
}

/// Marks a repo busy for as long as it is alive (§13, *Work in progress*).
///
/// A guard rather than a pair of calls in `write_and_refresh`, for the same
/// reason the Problems record lives there rather than at each call site: the
/// end must be unmissable. An early `?`, a panic inside the operation, a
/// future dropped because the window closed mid-write — every one of those
/// has to clear the row, and a `Drop` impl is the only version of that which
/// cannot be forgotten by a future write command.
struct WriteMarker {
    app: AppHandle,
    repo_id: String,
}

impl WriteMarker {
    fn begin(app: &AppHandle, repo_id: &str, operation: &str) -> Self {
        if app.state::<AppState>().in_flight.begin(repo_id) {
            let event = WriteBeginEvent { repo_id: repo_id.to_string(), operation: operation.to_string() };
            if let Err(err) = app.emit(WRITE_BEGIN_EVENT, event) {
                log::warn!("could not publish the start of a write ({err})");
            }
        }
        Self { app: app.clone(), repo_id: repo_id.to_string() }
    }
}

impl Drop for WriteMarker {
    fn drop(&mut self) {
        if self.app.state::<AppState>().in_flight.end(&self.repo_id) {
            let event = WriteEndEvent { repo_id: self.repo_id.clone() };
            if let Err(err) = self.app.emit(WRITE_END_EVENT, event) {
                // Nothing to recover with — but a lost end is a row that spins
                // forever, so it must not be silent in the log either.
                log::warn!("could not publish the end of a write ({err})");
            }
        }
    }
}

/// Shared shape for every mutating command (§7 rule 1): resolve the repo,
/// hold its write-queue lock for the duration of `op`, then refresh and
/// publish its status regardless of whether `op` succeeded — a failed stage
/// or commit can still have changed something (e.g. a partial index update),
/// and the row must never show data staler than the attempt just made.
///
/// It is also where §13's *Work in progress* is published, and the marker is
/// taken *before* the write queue rather than after: a write queued behind an
/// earlier one on the same repo is time the user spends waiting with nothing
/// happening on screen, which is precisely the case the indicator exists for.
/// It lives until this function returns, so the status refresh below is inside
/// the window too — the row must not go quiet while it is still being redrawn.
async fn write_and_refresh<F, Fut>(
    app: &AppHandle,
    repo_id: String,
    operation: &str,
    op: F,
) -> Result<(), String>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let path = repo_path(app, &repo_id)?;
    let _busy = WriteMarker::begin(app, &repo_id, operation);

    let result = {
        let _write_guard = app.state::<AppState>().write_queues.write(&repo_id).await;
        op(path.clone()).await
    };

    // Recorded here rather than at each call site for the same reason the
    // status refresh below is: this is the one shape every mutating command
    // has, so a new write command cannot forget to be in the record (§13).
    //
    // `operation` is the user's word for what they asked — "Push", "Merge" —
    // not the git argv, which `git.rs` already logged next to this stderr.
    // The Problems list answers "what of mine failed"; the log answers "with
    // what command", and they are different questions.
    if let Err(err) = &result {
        let problem = problems::record(Some(repo_id.clone()), operation, err);
        if let Err(err) = app.emit(PROBLEM_EVENT, problem) {
            log::warn!("could not publish a recorded problem ({err})");
        }
    }

    sweep::emit_repo_status(app, &repo_id, &path).await;
    result
}

/// The one place that writes the per-root cache file (§9.5): every caller —
/// a single-repo write, a status sweep, a fetch sweep — hands over the whole
/// current snapshot, because the file is overwritten wholesale each time and
/// a partial write (e.g. statuses without `last_fetch_at`) would silently
/// erase the other half on disk.
fn persist_cache(
    app: &AppHandle,
    root_path: &Path,
    statuses: HashMap<String, RepoStatus>,
    last_fetch_at: HashMap<String, i64>,
) {
    let state = app.state::<AppState>();
    let on_disk = RootCache { version: cache::CACHE_VERSION, statuses, last_fetch_at };
    if let Err(err) = cache::save(&state.cache_dir, root_path, &on_disk) {
        log::warn!("could not save status cache ({err})");
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Points a watcher at every repo in the open root (§6) and records the ones
/// that would not take a watch, which are the only repos the frequent sweep
/// still has to visit.
///
/// Called from `open_root`, `refresh_root` and focus-gain — the three things
/// that change *which repos exist* or whether we are watching at all. Pinning
/// and selection no longer call it: they used to define the watched set, and
/// now every repo is in it regardless (§6), so a pin toggle costs nothing here
/// where it used to cost a watcher teardown and rebuild.
fn sync_watchers(app: &AppHandle) {
    let state = app.state::<AppState>();

    let repos: Vec<(String, PathBuf)> = {
        let current = state.root.lock().expect("root mutex poisoned");
        let Some(root) = current.as_ref() else {
            drop(current);
            state.watchers.clear();
            return;
        };
        root.repos
            .iter()
            .map(|repo| (repo.id.clone(), repo.path.clone()))
            .collect()
    };

    let unwatched: HashSet<String> = state.watchers.sync(app, &repos).into_iter().collect();
    if !unwatched.is_empty() {
        // Worth a line in the log: a repo here refreshes on the sweep interval
        // instead of instantly, and "why is this one row slow" has no other
        // visible explanation.
        log::info!("{} repo(s) could not be watched; they stay on the sweep", unwatched.len());
    }

    let mut current = state.root.lock().expect("root mutex poisoned");
    if let Some(root) = current.as_mut() {
        root.unwatched = unwatched;
    }
}

/// Window gained focus (§6): sweep immediately rather than waiting for the
/// next tick, and (re)start the tickers that were aborted on the last blur.
/// Unlike the status sweep, fetch does not also run immediately — it is a
/// background convenience on a multi-minute interval (§6), and firing one on
/// every alt-tab back into the window would contend with the status sweep
/// for the shared 8-process cap (§7.3) right when the UI most wants that
/// budget for itself.
fn on_focus(app: &AppHandle) {
    // Watchers first, then the sweep: the sweep is what covers the gap the
    // dropped watchers left, and re-establishing them before it runs means
    // nothing that changes *during* the sweep falls between the two.
    sync_watchers(app);
    sweep::trigger_sweep(app, sweep::Scope::All);
    start_ticker(app);
    start_fetch_ticker(app);
}

/// Window lost focus (§6): stop ticking entirely. Not "skip the next tick" —
/// an unfocused window has no sweep timer running at all, which is what
/// makes background CPU zero rather than merely low.
fn on_blur(app: &AppHandle) {
    let state = app.state::<AppState>();

    // Dropped, not merely ignored — §6's promise is that an unfocused window
    // costs *no* background CPU, and watchers left running would wake us on
    // every file a background build writes. That is low, not none. Rebuilding
    // all of them on focus costs 17 ms, and the focus-gain sweep covers
    // whatever happened while they were gone.
    state.watchers.clear();

    let ticker = state.ticker.lock().expect("ticker mutex poisoned").take();
    if let Some(handle) = ticker {
        handle.abort();
    }

    let fetch_ticker = state.fetch_ticker.lock().expect("fetch ticker mutex poisoned").take();
    if let Some(handle) = fetch_ticker {
        handle.abort();
    }
}

fn start_ticker(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut ticker = state.ticker.lock().expect("ticker mutex poisoned");
    if ticker.is_some() {
        return;
    }

    let interval = {
        let settings = state.settings.lock().expect("settings mutex poisoned");
        Duration::from_secs(settings.status_sweep_secs.max(1))
    };

    let app = app.clone();
    *ticker = Some(tauri::async_runtime::spawn(async move {
        let mut tick: u32 = 0;
        loop {
            tokio::time::sleep(interval).await;
            tick = tick.wrapping_add(1);
            let scope = if tick % sweep::RECONCILE_EVERY == 0 { sweep::Scope::All } else { sweep::Scope::Unwatched };
            sweep::trigger_sweep(&app, scope);
        }
    }));
}

/// A separate ticker from the status sweep's (§6: "these are different
/// mechanisms and must not be conflated"), on its own — much longer,
/// jittered — interval, and focus-gated the same way.
fn start_fetch_ticker(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut ticker = state.fetch_ticker.lock().expect("fetch ticker mutex poisoned");
    if ticker.is_some() {
        return;
    }

    let base_secs = {
        let settings = state.settings.lock().expect("settings mutex poisoned");
        settings.fetch_sweep_secs.max(1)
    };

    let app = app.clone();
    *ticker = Some(tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(jittered_interval(base_secs)).await;
            fetchsweep::trigger_fetch_sweep(&app);
        }
    }));
}

/// `base_secs` to `2 * base_secs` — with the default 300 s setting, that is
/// 5 to 10 minutes, matching §6's "5–10 min, jittered" exactly without a
/// `rand` dependency: the low-order bits of the current epoch time change
/// every tick, which is all a polling interval's jitter needs (this is not a
/// security context).
fn jittered_interval(base_secs: u64) -> Duration {
    let base = base_secs.max(1);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let extra_secs = (nanos % (base as u128 * 1_000_000_000)) / 1_000_000_000;
    Duration::from_secs(base + extra_secs as u64)
}

/// Most-recent-first, deduplicated, capped. Saved immediately rather than on
/// the settings debounce: opening a folder is exactly the moment a crash would
/// be most annoying to lose.
///
/// This used to also repopulate the native File ▸ Open Recent submenu, being
/// the one place the list actually changes. The frontend menu (§4.1) renders
/// that submenu from `settings.data.recentRoots`, which the frontend refreshes
/// on the same open this is called from — so the list now follows without
/// being pushed.
fn remember_root(app: &AppHandle, root: &Path) {
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().expect("settings mutex poisoned");

    settings.recent_roots.retain(|recent| recent != root);
    settings.recent_roots.insert(0, root.to_path_buf());
    settings.recent_roots.truncate(MAX_RECENT_ROOTS);

    if let Err(err) = settings::save(&state.config_dir, &settings) {
        log::warn!("could not save recent roots ({err})");
    }
}

/// Routes a second launch into the running process rather than starting a
/// second one (§9.2). This is not a nicety: the global git semaphore (§7.3),
/// the per-repo write queues (§7) and the single cache writer (§9.5) are all
/// process-local, so two processes mean 16 concurrent `git.exe`, two
/// independent write queues racing `index.lock` on the same repo, and two
/// writers on one cache file.
///
/// Registered before every other plugin, which the plugin requires.
///
/// §9.2 also calls for a second launch to *spawn a window* in the running
/// process. Until multi-window ships there is only ever "main" to raise, so
/// this surfaces that instead — the half of §9.2 that prevents corruption
/// rather than the half that adds windows. `args`/`cwd` are ignored because
/// Corgit takes no command line yet; a future `corgit <path>` opens here.
fn focus_existing_window(app: &AppHandle, _args: Vec<String>, _cwd: String) {
    let Some(window) = app.get_webview_window("main") else { return };
    // Minimised first, then focus: `set_focus` on a minimised window raises it
    // in the taskbar without actually restoring it on Windows.
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// Where a failure goes when there is no console to print it to.
///
/// `main.rs` builds release with `windows_subsystem = "windows"`, so every
/// `eprintln!` in a shipped build writes to a closed handle — a cache that
/// silently stops saving, a root-settings write that silently fails, and a git
/// process killed at its budget (§7.3) all left no trace whatsoever. A file in
/// the log dir is what makes those diagnosable after the fact; Help ▸ Open Log
/// Folder (§4.1) is what makes it reachable without knowing where `%APPDATA%`
/// keeps it.
///
/// `Info` rather than `Debug`: `tao`/`wry` are extremely chatty below Info, and
/// a log nobody can skim is one nobody reads. Corgit's own messages are all
/// warnings or errors, so none of them are lost to this floor.
fn logging() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let mut builder = tauri_plugin_log::Builder::new()
        .level(log::LevelFilter::Info)
        .target(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
            file_name: Some("corgit".to_string()),
        }));

    // Only useful where a console exists to read it, which by construction is
    // never the case in the builds this plugin is here for.
    if cfg!(debug_assertions) {
        builder = builder.target(tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::Stderr,
        ));
    }

    builder.build()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(focus_existing_window))
        .plugin(logging())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // The window exists by the time `setup` runs, but the thread that
            // paints it is this one — so every branch below is time the user
            // spends looking at no window at all, not at an empty one.
            let started = Instant::now();

            let config_dir = app.path().app_config_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            let settings = settings::load(&config_dir);

            // Started here, waited for nowhere: the answer is wanted once, by
            // `git_info`, and everything else on the startup path — reading
            // the cache, painting the last root — is true whether or not git
            // exists (§3). Warming it now rather than leaving it to the first
            // `git_info` only means the frontend usually finds it already
            // there.
            let git: Arc<tokio::sync::OnceCell<GitInfo>> = Arc::new(tokio::sync::OnceCell::new());
            let warming = git.clone();
            tauri::async_runtime::spawn(async move {
                let probed = Instant::now();
                let info = warming.get_or_init(git::probe).await;
                if !info.available {
                    log::error!("no usable git on PATH");
                }
                // The one startup cost that is a process spawn rather than a
                // file read, and so the one with no upper bound worth
                // trusting. It no longer blocks anything, but it is still the
                // number that explains a slow first paint of the repo list.
                log::info!("startup: git probe {} ms", probed.elapsed().as_millis());
            });

            app.manage(AppState {
                config_dir,
                cache_dir,
                settings: Mutex::new(settings),
                git,
                root: Mutex::new(None),
                sweeping: AtomicBool::new(false),
                ticker: Mutex::new(None),
                fetch_sweeping: AtomicBool::new(false),
                bulk_cancelled: AtomicBool::new(false),
                fetch_ticker: Mutex::new(None),
                write_queues: Arc::new(WriteQueues::default()),
                in_flight: Arc::new(inflight::InFlightWrites::default()),
                watchers: watch::RepoWatchers::default(),
                // No repo is open yet, so the panes default to visible.
                pane_visibility: Mutex::new(menu::PaneVisibility::default()),
            });

            // Focus gating (§6): a window is focused when created, and Tauri
            // does not replay that as a `Focused(true)` event, so the ticker
            // is started here rather than waiting for one.
            if let Some(window) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::Focused(focused) = event {
                        if *focused {
                            on_focus(&handle);
                        } else {
                            on_blur(&handle);
                        }
                    }
                });
            }
            on_focus(app.handle());

            // The handover point: after this the event loop runs and the
            // window can paint, so anything still slow from here on is the
            // webview booting or a command blocking the main thread — which
            // `open_root` now says for itself.
            log::info!("startup: main thread free after {} ms", started.elapsed().as_millis());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            git_info,
            initial_root,
            pick_root,
            open_root,
            current_root,
            refresh_root,
            repo_files,
            graph_page,
            graph_refs,
            commit_details,
            file_diff,
            stage_paths,
            unstage_paths,
            discard_paths,
            delete_paths,
            append_gitignore,
            stage_all,
            unstage_all,
            commit_repo,
            fetch_repo,
            pull_repo,
            push_repo,
            bulk::fetch_all,
            bulk::pull_all_behind,
            bulk::branch_all,
            bulk::switch_pull_all,
            repo_branches,
            bulk::stop_bulk,
            merge_abort,
            publish_branch,
            commit_and_push,
            switch_branch,
            create_branch,
            merge_branch,
            delete_branch,
            open_in_vscode,
            open_in_terminal,
            reveal_in_explorer,
            toggle_pin,
            clear_pins,
            set_selected_repo,
            recent_problems,
            clear_problems,
            menu::menu_command,
            menu::publish_pane_visibility,
        ])
        .run(tauri::generate_context!())
        .expect("corgit: fatal error while running the application");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of `explorer_arg`: a repo-relative path off `git status`
    /// joined onto a Windows root is mixed-separator, and explorer answers that
    /// by opening the home folder rather than failing.
    #[cfg(windows)]
    #[test]
    fn explorer_arg_flips_git_slashes() {
        let target = PathBuf::from(r"C:\dev\corgit").join("src/lib/repos.svelte.ts");
        assert_eq!(explorer_arg(&target), r"C:\dev\corgit\src\lib\repos.svelte.ts");
    }

    /// The reason `program_on_path` reads `PATHEXT` at all. VS Code's entry
    /// point is `code.cmd`, so an extension list without `.CMD` resolves
    /// nothing and *Open in VS Code* is dead on every machine — a failure with
    /// no symptom other than the feature never working.
    #[cfg(windows)]
    #[test]
    fn the_extension_list_can_always_find_a_cmd_shim() {
        for pathext in [None, Some(""), Some("   "), Some(".COM;.EXE;.BAT;.CMD")] {
            let extensions = path_extensions(pathext);
            assert!(
                extensions.iter().any(|extension| extension.eq_ignore_ascii_case(".cmd")),
                "{pathext:?} resolved to {extensions:?}, which cannot find code.cmd"
            );
        }
    }

    /// Order is the machine's to decide, not ours: `PATHEXT` is what the shell
    /// would try first, and a resolver that reordered it could pick a
    /// different program than the one the user gets by typing the name.
    #[cfg(windows)]
    #[test]
    fn the_extension_list_keeps_the_order_pathext_gave() {
        assert_eq!(path_extensions(Some(".CMD;.EXE")), [".CMD", ".EXE"]);
        assert_eq!(path_extensions(Some(".EXE;.CMD")), [".EXE", ".CMD"]);
    }

    /// A real `PATHEXT` picks up debris — a trailing separator from an
    /// installer that appended carelessly, spaces from one that used a GUI.
    /// An empty entry would build the bare name `code`, which is the one
    /// candidate Windows will not execute.
    #[cfg(windows)]
    #[test]
    fn the_extension_list_drops_blank_entries_and_surrounding_space() {
        assert_eq!(path_extensions(Some(".EXE; .CMD ;;")), [".EXE", ".CMD"]);
    }

    fn repo(id: &str) -> Repo {
        Repo { id: id.to_string(), name: id.to_string(), path: PathBuf::from(id) }
    }

    fn root_with(repos: &[&str], pins: &[&str], selected: Option<&str>) -> RootState {
        RootState {
            generation: 1,
            path: PathBuf::from("root"),
            repos: repos.iter().map(|id| repo(id)).collect(),
            statuses: repos
                .iter()
                .map(|id| (id.to_string(), RepoStatus::default()))
                .collect(),
            errors: HashMap::new(),
            last_fetch_at: repos.iter().map(|id| (id.to_string(), 1_700_000_000)).collect(),
            auth_needed: HashSet::new(),
            unwatched: HashSet::new(),
            pins: pins.iter().map(|id| id.to_string()).collect(),
            selected: selected.map(str::to_string),
        }
    }

    /// The bug this method exists to prevent: a repo can vanish from a scan
    /// without being deleted — a disconnected network drive is the usual way —
    /// and `set_selected_repo` persists `pins` wholesale on the user's next
    /// click. Pruning here made that transient miss permanent (§9.5 rule 5).
    #[test]
    fn a_repo_missing_from_a_rescan_keeps_its_pin() {
        let mut root = root_with(&["api", "billing"], &["api", "billing"], None);

        root.adopt_repos(vec![repo("billing")]);

        assert!(root.pins.contains("api"), "a pin is the user's own choice, not a cache");
        assert!(root.pins.contains("billing"));
    }

    /// The other half: everything a sweep can regenerate *is* dropped, so a
    /// genuinely deleted repo does not keep a stale status forever.
    #[test]
    fn a_repo_missing_from_a_rescan_loses_its_regenerable_state() {
        let mut root = root_with(&["api", "billing"], &[], None);
        root.errors.insert("api".to_string(), "boom".to_string());
        root.auth_needed.insert("api".to_string());

        root.adopt_repos(vec![repo("billing")]);

        assert!(!root.statuses.contains_key("api"));
        assert!(!root.errors.contains_key("api"));
        assert!(!root.last_fetch_at.contains_key("api"));
        assert!(!root.auth_needed.contains("api"));
        assert_eq!(root.repos, vec![repo("billing")]);
    }

    /// Unlike a pin, a selection naming a repo the list no longer shows would
    /// leave the middle pane describing nothing.
    #[test]
    fn a_selection_on_a_vanished_repo_is_cleared_but_one_still_present_is_kept() {
        let mut root = root_with(&["api", "billing"], &[], Some("api"));
        root.adopt_repos(vec![repo("billing")]);
        assert_eq!(root.selected, None);

        let mut root = root_with(&["api", "billing"], &[], Some("billing"));
        root.adopt_repos(vec![repo("billing")]);
        assert_eq!(root.selected.as_deref(), Some("billing"));
    }

    /// A pin surviving for a repo that really is gone has to be harmless, or
    /// the rule above would trade one bug for another. Both consumers walk
    /// `repos` and look pins up, so an unmatched pin contributes nothing.
    #[test]
    fn a_pin_with_no_matching_repo_contributes_nothing_to_the_hot_set() {
        let root = root_with(&["billing"], &["api", "billing"], None);

        let hot: Vec<&str> = root
            .repos
            .iter()
            .filter(|repo| root.pins.contains(&repo.id) || root.selected.as_deref() == Some(&repo.id))
            .map(|repo| repo.id.as_str())
            .collect();

        assert_eq!(hot, vec!["billing"]);
    }

    fn status_with(branch: &str) -> RepoStatus {
        RepoStatus { branch: Some(branch.to_string()), ..RepoStatus::default() }
    }

    /// The rule `merge_sweep_results` exists for, and the one that is invisible
    /// when it breaks. A repo whose write lock was held is skipped by the sweep
    /// (§7 rule 2) and so appears in neither map — assigning the maps wholesale
    /// would blank its row every time the user staged something in it, which
    /// reads as the repo having vanished rather than as the sweep having
    /// politely stayed out of the way.
    #[test]
    fn a_repo_the_sweep_skipped_keeps_the_status_it_had() {
        let mut root = root_with(&["api", "billing"], &[], None);
        root.statuses.insert("api".to_string(), status_with("main"));
        root.statuses.insert("billing".to_string(), status_with("main"));

        // Only `billing` was read this round; `api` was busy.
        let swept = HashMap::from([("billing".to_string(), status_with("release"))]);
        root.merge_sweep_results(&swept, &HashMap::new());

        assert_eq!(root.statuses["api"].branch.as_deref(), Some("main"), "a skipped repo must not be erased");
        assert_eq!(root.statuses["billing"].branch.as_deref(), Some("release"));
    }

    /// Both directions of the statuses/errors exclusion. The row draws from
    /// both maps, so a repo left in both says "unknown" and shows a clean dot
    /// at the same time — and which one the user sees depends on read order,
    /// which is the shape of bug that survives a whole release.
    #[test]
    fn a_repo_is_never_in_both_statuses_and_errors() {
        let mut root = root_with(&["api"], &[], None);

        // Succeeded, having previously failed: the stale error has to go.
        root.errors.insert("api".to_string(), "boom".to_string());
        root.merge_sweep_results(&HashMap::from([("api".to_string(), status_with("main"))]), &HashMap::new());
        assert!(root.errors.is_empty(), "a repo that just succeeded still carries its old error");
        assert!(root.statuses.contains_key("api"));

        // Failed, having previously succeeded: the stale status has to go, or
        // the row shows a clean dot for a repo nothing could read.
        root.merge_sweep_results(&HashMap::new(), &HashMap::from([("api".to_string(), "boom".to_string())]));
        assert!(root.statuses.is_empty(), "a repo that just failed still shows its old status");
        assert!(root.errors.contains_key("api"));
    }

    /// An empty sweep is the normal case for a `Scope::Unwatched` tick, which
    /// is four ticks in five (§6, `sweep::RECONCILE_EVERY`) and usually covers
    /// no repos at all. It must be a no-op, not a clear.
    #[test]
    fn a_sweep_that_read_nothing_changes_nothing() {
        let mut root = root_with(&["api"], &[], None);
        root.statuses.insert("api".to_string(), status_with("main"));
        root.errors.insert("billing".to_string(), "boom".to_string());

        root.merge_sweep_results(&HashMap::new(), &HashMap::new());

        assert_eq!(root.statuses["api"].branch.as_deref(), Some("main"));
        assert_eq!(root.errors["billing"], "boom");
    }

    /// §6: "5–10 min, jittered" — verified against the default 300 s setting
    /// rather than a mocked clock, since the jitter source is real wall-clock
    /// time and the property under test is the range, not a specific value.
    #[test]
    fn fetch_jitter_stays_within_one_to_two_times_the_base_interval() {
        for _ in 0..20 {
            let interval = jittered_interval(300);
            assert!(interval.as_secs() >= 300, "{interval:?} is below the base interval");
            assert!(interval.as_secs() < 600, "{interval:?} is at or beyond double the base interval");
        }
    }

    #[test]
    fn fetch_jitter_never_divides_by_zero_at_a_zero_base() {
        let interval = jittered_interval(0);
        assert!(interval.as_secs() >= 1, "base_secs is floored at 1");
    }
}

#[cfg(test)]
mod bench {
    //! Not a test — the §1 status-sweep measurement, kept beside the code it
    //! measures because §16 says to take it again at build steps 3 and 6.
    //!
    //! ```text
    //! $env:CORGIT_BENCH_ROOT = 'C:\dev\code'
    //! cargo test --release --lib -- --ignored --nocapture bench_status_sweep
    //! ```
    use super::*;

    /// Separates "this machine creates processes slowly" from "Corgit creates
    /// them one at a time". `git --version` does no repository work, so its
    /// wall clock is pure spawn cost: 16 of them should cost about two rounds
    /// of the semaphore, not sixteen.
    #[test]
    #[ignore = "measurement, not a test"]
    fn bench_spawn_concurrency() {
        let cwd = std::env::current_dir().unwrap();

        for count in [1usize, 8, 16, 32] {
            let started = Instant::now();
            tauri::async_runtime::block_on(async {
                let tasks: Vec<_> = (0..count)
                    .map(|_| {
                        let cwd = cwd.clone();
                        tauri::async_runtime::spawn(async move {
                            git::read(&cwd, &["--version"]).await.map(|out| out.ok)
                        })
                    })
                    .collect();
                for task in tasks {
                    let _ = task.await;
                }
            });
            let total = started.elapsed().as_millis();
            println!(
                "{count:>3} × `git --version`: {total:>5} ms total, {:>5.1} ms each",
                total as f64 / count as f64,
            );
        }
    }

    #[test]
    #[ignore = "measurement, not a test: needs CORGIT_BENCH_ROOT"]
    fn bench_status_sweep() {
        let root = std::env::var("CORGIT_BENCH_ROOT")
            .expect("set CORGIT_BENCH_ROOT to a folder containing repositories");
        let root = discovery::canonicalize(Path::new(&root));

        let discovery_started = Instant::now();
        let repos = discovery::scan(&root);
        let discovery_us = discovery_started.elapsed().as_micros();

        assert!(!repos.is_empty(), "no repositories under {}", root.display());
        println!("discovery: {} repos in {discovery_us} µs", repos.len());

        let info = tauri::async_runtime::block_on(git::probe());
        println!(
            "git:       {} via {}",
            info.version.as_deref().unwrap_or("unavailable"),
            info.read_binary.as_deref().unwrap_or("-"),
        );

        // No repo is write-locked in this harness, so an empty registry
        // behaves like every repo being free to read — same as production.
        let write_queues = Arc::new(WriteQueues::default());

        // Longer than any read can live — `git.rs`'s `READ_TIMEOUT` kills one
        // at 30 s — so nothing here becomes a straggler and each round is the
        // cost of the *whole* pass. That is the number §1 budgets. Production
        // uses `sweep::SWEEP_PATIENCE` instead and publishes without its slowest
        // repo, which is a different question from how long the pass takes.
        let no_stragglers = Duration::from_secs(60);

        // The first sweep pays for cold file caches. Real cold start pays that
        // too, but only once, and from build step 3 it paints from cache while
        // it happens — so the steady-state number is the one under budget.
        let warm =
            tauri::async_runtime::block_on(sweep::collect(write_queues.clone(), repos.clone(), no_stragglers));
        println!("warm-up:   {} ok, {} failed", warm.0.len(), warm.1.len());

        for round in 1..=6 {
            let started = Instant::now();
            let (statuses, errors, _) = tauri::async_runtime::block_on(sweep::collect(
                write_queues.clone(),
                repos.clone(),
                no_stragglers,
            ));
            let elapsed = started.elapsed();
            println!(
                "round {round}:   {} repos in {} ms ({} ok, {} failed){}",
                repos.len(),
                elapsed.as_millis(),
                statuses.len(),
                errors.len(),
                if elapsed > sweep::SWEEP_PATIENCE { " — would have published early" } else { "" },
            );
            for (id, err) in errors.iter().take(3) {
                println!("           {id}: {err}");
            }
        }
    }
}