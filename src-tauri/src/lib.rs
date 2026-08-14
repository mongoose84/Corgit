mod cache;
mod commit;
mod discovery;
mod git;
mod graph;
mod remote;
mod settings;
mod status;
mod writequeue;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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

const SWEEP_EVENT: &str = "status:sweep";
const REPO_STATUS_EVENT: &str = "status:repo";

/// Rust owns the state; the frontend is a view over it (SPEC.md §9.3).
///
/// The global git semaphore lives in `git.rs` instead, as a static — it has
/// to hold across every window (§9.2), and routing it through app state would
/// only make that easier to get wrong.
struct AppState {
    config_dir: PathBuf,
    cache_dir: PathBuf,
    settings: Mutex<Settings>,
    /// Resolved once at startup: git either exists or the UI says so (§3).
    git: GitInfo,
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
    fetch_ticker: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// One write queue per repo, shared across every window (§7, §9.2). First
    /// used in build step 4, where staging and commit are the first writes.
    /// `Arc`-wrapped so the sweep can clone a handle into its per-repo tasks
    /// without needing an `AppHandle` there too (§6, §7 rule 2).
    write_queues: Arc<WriteQueues>,
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
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SweepEvent {
    /// Echoed back so a window can ignore results for a root it no longer shows.
    root: PathBuf,
    statuses: HashMap<String, RepoStatus>,
    errors: HashMap<String, String>,
    /// Measured against the 300 ms budget in §1. It is the number the whole
    /// project is justified by, so it is reported, not guessed at.
    elapsed_ms: u64,
}

/// Emitted after a stage, unstage or commit lands, so the row and middle pane
/// update immediately rather than waiting up to 60 s for the next sweep tick.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoStatusEvent {
    root: PathBuf,
    repo_id: String,
    status: Option<RepoStatus>,
    error: Option<String>,
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state
        .settings
        .lock()
        .expect("settings mutex poisoned")
        .clone()
}

#[tauri::command]
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

#[tauri::command]
fn git_info(state: State<'_, AppState>) -> GitInfo {
    state.git.clone()
}

/// The root to reopen on launch: the most recent one that still exists. A
/// renamed folder or a disconnected drive yields `None`, and the frontend
/// shows the welcome screen — never an empty repo list, never a crash (§9.1).
#[tauri::command]
fn initial_root(state: State<'_, AppState>) -> Option<PathBuf> {
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
#[tauri::command]
fn open_root(path: PathBuf, app: AppHandle) -> Result<RootView, String> {
    let root = discovery::canonicalize(&path);
    if !root.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }

    let repos = discovery::scan(&root);
    let state = app.state::<AppState>();

    let mut cached = cache::load(&state.cache_dir, &root);
    // A repo that no longer exists under this root has nothing left to
    // correct it, so it must not linger in the view.
    cached.statuses.retain(|id, _| repos.iter().any(|repo| &repo.id == id));
    cached.last_fetch_at.retain(|id, _| repos.iter().any(|repo| &repo.id == id));

    let generation = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let generation = current.as_ref().map_or(0, |root| root.generation) + 1;
        *current = Some(RootState {
            generation,
            path: root.clone(),
            repos: repos.clone(),
            statuses: cached.statuses.clone(),
            errors: HashMap::new(),
            last_fetch_at: cached.last_fetch_at.clone(),
            auth_needed: HashSet::new(),
        });
        generation
    };

    remember_root(&state, &root);
    start_sweep(&app, generation, repos.clone());

    Ok(RootView {
        path: root,
        repos,
        statuses: cached.statuses,
        errors: HashMap::new(),
        last_fetch_at: cached.last_fetch_at,
        auth_needed: HashSet::new(),
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
    })
}

/// Rescan the root and sweep again. Discovery is repeated because a repo may
/// have been cloned or deleted since the folder was opened, so the caller gets
/// the new repo list back rather than only the statuses the sweep will emit.
#[tauri::command]
fn refresh_root(app: AppHandle) -> Result<RootView, String> {
    let state = app.state::<AppState>();

    let (generation, view) = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let Some(root) = current.as_mut() else {
            return Err("No folder is open".to_string());
        };

        let repos = discovery::scan(&root.path);
        // Otherwise a deleted repo keeps its last known status forever.
        root.statuses.retain(|id, _| repos.iter().any(|repo| &repo.id == id));
        root.errors.retain(|id, _| repos.iter().any(|repo| &repo.id == id));
        root.last_fetch_at.retain(|id, _| repos.iter().any(|repo| &repo.id == id));
        root.auth_needed.retain(|id| repos.iter().any(|repo| &repo.id == id));
        root.repos = repos;

        (
            root.generation,
            RootView {
                path: root.path.clone(),
                repos: root.repos.clone(),
                statuses: root.statuses.clone(),
                errors: root.errors.clone(),
                last_fetch_at: root.last_fetch_at.clone(),
                auth_needed: root.auth_needed.clone(),
            },
        )
    };

    start_sweep(&app, generation, view.repos.clone());
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
    write_and_refresh(&app, repo_id, |path| async move { commit::stage(&path, &paths).await })
        .await
}

#[tauri::command]
async fn unstage_paths(repo_id: String, paths: Vec<String>, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, |path| async move { commit::unstage(&path, &paths).await })
        .await
}

#[tauri::command]
async fn stage_all(repo_id: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, |path| async move { commit::stage_all(&path).await }).await
}

#[tauri::command]
async fn unstage_all(repo_id: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, |path| async move { commit::unstage_all(&path).await }).await
}

#[tauri::command]
async fn commit_repo(repo_id: String, message: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, |path| async move { commit::commit(&path, &message).await })
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

/// A manual, user-triggered fetch — allowed to prompt interactively, unlike
/// the background fetch sweep (§8.7). Clears "auth needed" regardless of
/// outcome: the user is sitting right there, and this attempt's own result is
/// the freshest signal about whether the repo still needs attention.
#[tauri::command]
async fn fetch_repo(repo_id: String, app: AppHandle) -> Result<(), String> {
    let result = write_and_refresh(&app, repo_id.clone(), |path| async move { remote::fetch(&path).await }).await;
    record_fetch_attempt(&app, &repo_id);
    result
}

#[tauri::command]
async fn pull_repo(repo_id: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, |path| async move { remote::pull(&path).await }).await
}

#[tauri::command]
async fn push_repo(repo_id: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, |path| async move { remote::push(&path).await }).await
}

/// A branch with no upstream configured (§8.7) — the branch name comes from
/// the repo's currently known status rather than a frontend-supplied
/// argument, so there is one source of truth for what "the current branch"
/// means.
#[tauri::command]
async fn publish_branch(repo_id: String, app: AppHandle) -> Result<(), String> {
    let branch = current_branch(&app, &repo_id)?;
    write_and_refresh(&app, repo_id, |path| async move { remote::publish(&path, &branch).await }).await
}

/// Commit, then push in one step. Whether that push needs `-u origin
/// <branch>` is decided up front from the known status, before the commit
/// runs, since a fresh commit does not change whether an upstream exists.
#[tauri::command]
async fn commit_and_push(repo_id: String, message: String, app: AppHandle) -> Result<(), String> {
    let branch = if has_upstream(&app, &repo_id) { None } else { Some(current_branch(&app, &repo_id)?) };

    write_and_refresh(&app, repo_id, |path| async move {
        commit::commit(&path, &message).await?;
        match branch {
            Some(branch) => remote::publish(&path, &branch).await,
            None => remote::push(&path).await,
        }
    })
    .await
}

/// Read from the currently known status rather than querying git fresh —
/// good enough for "which branch am I about to publish", and avoids a spawn
/// on a command that already has a status right there.
fn current_branch(app: &AppHandle, repo_id: &str) -> Result<String, String> {
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    let root = current.as_ref().ok_or_else(|| "No folder is open".to_string())?;
    root.statuses
        .get(repo_id)
        .and_then(|status| status.branch.clone())
        .ok_or_else(|| "No branch to publish (detached HEAD)".to_string())
}

fn has_upstream(app: &AppHandle, repo_id: &str) -> bool {
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    current
        .as_ref()
        .and_then(|root| root.statuses.get(repo_id))
        .is_some_and(|status| status.upstream.is_some())
}

/// Records this repo's fetch attempt and clears any "auth needed" flag (§8.7,
/// §13) — shared by the manual `fetch_repo` command. Persists immediately,
/// mirroring `emit_repo_status`'s per-write cache save: a manual fetch is a
/// user action, not a batch the fetch sweep already throttles.
fn record_fetch_attempt(app: &AppHandle, repo_id: &str) {
    let state = app.state::<AppState>();
    let published = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let Some(root) = current.as_mut() else { return };
        root.last_fetch_at.insert(repo_id.to_string(), now_unix());
        root.auth_needed.remove(repo_id);
        (root.path.clone(), root.statuses.clone(), root.last_fetch_at.clone())
    };
    let (root_path, statuses, last_fetch_at) = published;
    persist_cache(app, &root_path, statuses, last_fetch_at);
}

/// Shared shape for every mutating command (§7 rule 1): resolve the repo,
/// hold its write-queue lock for the duration of `op`, then refresh and
/// publish its status regardless of whether `op` succeeded — a failed stage
/// or commit can still have changed something (e.g. a partial index update),
/// and the row must never show data staler than the attempt just made.
async fn write_and_refresh<F, Fut>(app: &AppHandle, repo_id: String, op: F) -> Result<(), String>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let path = repo_path(app, &repo_id)?;

    let result = {
        let _write_guard = app.state::<AppState>().write_queues.write(&repo_id).await;
        op(path.clone()).await
    };

    emit_repo_status(app, &repo_id, &path).await;
    result
}

/// Re-reads one repo's status outside any write lock, updates it in the open
/// root (if that root and repo are still current), saves the cache, and
/// notifies every window watching this root — the single-repo counterpart to
/// what a full sweep does for all of them.
async fn emit_repo_status(app: &AppHandle, repo_id: &str, path: &Path) {
    let status = status::query(path).await;
    let state = app.state::<AppState>();

    let published = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let Some(root) = current.as_mut() else { return };
        if !root.repos.iter().any(|repo| repo.id == repo_id) {
            return;
        }

        match &status {
            Ok(s) => {
                root.statuses.insert(repo_id.to_string(), s.clone());
                root.errors.remove(repo_id);
            }
            Err(err) => {
                root.errors.insert(repo_id.to_string(), err.clone());
                root.statuses.remove(repo_id);
            }
        }

        (root.path.clone(), root.statuses.clone(), root.last_fetch_at.clone())
    };

    let (root_path, statuses, last_fetch_at) = published;
    persist_cache(app, &root_path, statuses, last_fetch_at);

    let event = RepoStatusEvent {
        root: root_path,
        repo_id: repo_id.to_string(),
        status: status.as_ref().ok().cloned(),
        error: status.as_ref().err().cloned(),
    };
    if let Err(err) = app.emit(REPO_STATUS_EVENT, event) {
        eprintln!("twogit: could not publish repo status ({err})");
    }
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
        eprintln!("twogit: could not save status cache ({err})");
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn start_sweep(app: &AppHandle, generation: u64, repos: Vec<Repo>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { sweep(app, generation, repos).await });
}

async fn sweep(app: AppHandle, generation: u64, repos: Vec<Repo>) {
    let write_queues = {
        let state = app.state::<AppState>();
        if state.sweeping.swap(true, Ordering::SeqCst) {
            return;
        }
        state.write_queues.clone()
    };

    let started = Instant::now();
    let (statuses, errors) = collect(write_queues, repos).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let state = app.state::<AppState>();

    let outcome = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        match current.as_mut() {
            Some(root) if root.generation == generation => {
                // A merge, not a replace: repos skipped this round because
                // their write lock was held (§6, §7 rule 2) are absent from
                // both maps and must keep whatever they last had, not vanish.
                for (id, status) in &statuses {
                    root.statuses.insert(id.clone(), status.clone());
                    root.errors.remove(id);
                }
                for (id, err) in &errors {
                    root.errors.insert(id.clone(), err.clone());
                    root.statuses.remove(id);
                }
                Outcome::Publish(
                    SweepEvent {
                        root: root.path.clone(),
                        statuses: root.statuses.clone(),
                        errors: root.errors.clone(),
                        elapsed_ms,
                    },
                    root.last_fetch_at.clone(),
                )
            }
            // The root was replaced while we were out, so these results
            // describe a folder nobody is looking at any more. The sweep the
            // new root asked for was turned away by the guard above, which
            // makes redoing it our job — otherwise its rows never fill in.
            Some(root) => Outcome::Restart(root.generation, root.repos.clone()),
            None => Outcome::Nothing,
        }
    };

    match outcome {
        Outcome::Publish(event, last_fetch_at) => {
            // Saved after every sweep rather than on a separate debounce
            // timer: sweeps are already throttled to the configured interval
            // (§6), so this already satisfies "not on every status change"
            // (§9.5 rule 4) without a second timer to keep in sync with the
            // first. Errors are deliberately not cached — a repo that failed
            // this round keeps whatever the *previous* successful sweep or
            // cache load left behind, until it succeeds again.
            persist_cache(&app, &event.root, event.statuses.clone(), last_fetch_at);

            if let Err(err) = app.emit(SWEEP_EVENT, event) {
                eprintln!("twogit: could not publish sweep results ({err})");
            }

            // Cleared only now: the guard has to cover the cache write and
            // event emit too, not just the git spawns in `collect`, or a
            // sweep triggered in that window races `cache::save` against
            // this one for the same root file (§6 — "a sweep never starts
            // while one is in flight").
            state.sweeping.store(false, Ordering::SeqCst);
        }
        Outcome::Restart(generation, repos) => {
            // Cleared before restarting, not after: `start_sweep` spawns a
            // fresh sweep that immediately re-swaps the guard to `true`, so
            // it must find it `false` here or the restart silently no-ops.
            state.sweeping.store(false, Ordering::SeqCst);
            start_sweep(&app, generation, repos);
        }
        Outcome::Nothing => {
            state.sweeping.store(false, Ordering::SeqCst);
        }
    }
}

/// Sweep the currently open root, if any — a no-op with no root open. Shared
/// by the focus-gained handler and the periodic ticker, both of which sweep
/// whatever is open rather than a fixed repo list captured at start time.
fn trigger_sweep(app: &AppHandle) {
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    let Some(root) = current.as_ref() else { return };
    let (generation, repos) = (root.generation, root.repos.clone());
    drop(current);
    start_sweep(app, generation, repos);
}

/// Window gained focus (§6): sweep immediately rather than waiting for the
/// next tick, and (re)start the tickers that were aborted on the last blur.
/// Unlike the status sweep, fetch does not also run immediately — it is a
/// background convenience on a multi-minute interval (§6), and firing one on
/// every alt-tab back into the window would contend with the status sweep
/// for the shared 8-process cap (§7.3) right when the UI most wants that
/// budget for itself.
fn on_focus(app: &AppHandle) {
    trigger_sweep(app);
    start_ticker(app);
    start_fetch_ticker(app);
}

/// Window lost focus (§6): stop ticking entirely. Not "skip the next tick" —
/// an unfocused window has no sweep timer running at all, which is what
/// makes background CPU zero rather than merely low.
fn on_blur(app: &AppHandle) {
    let state = app.state::<AppState>();

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
        loop {
            tokio::time::sleep(interval).await;
            trigger_sweep(&app);
        }
    }));
}

/// Fetch the currently open root, if any — the fetch-sweep counterpart to
/// `trigger_sweep`.
fn trigger_fetch_sweep(app: &AppHandle) {
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    let Some(root) = current.as_ref() else { return };
    let (generation, repos) = (root.generation, root.repos.clone());
    drop(current);
    start_fetch_sweep(app, generation, repos);
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
            trigger_fetch_sweep(&app);
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

/// What a finished sweep does next. Split out so the decision is made while
/// the root lock is held and acted on after it is released.
enum Outcome {
    Publish(SweepEvent, HashMap<String, i64>),
    Restart(u64, Vec<Repo>),
    Nothing,
}

/// Every repo is dispatched at once; the semaphore in `git.rs` is what
/// actually holds concurrency to 8 (§7.3). One batched result rather than 77
/// events keeps the IPC cost off the sweep's measured time. A repo whose
/// write lock is currently held is skipped rather than queried — a
/// non-blocking `try_read`, not a wait, so one busy repo never holds up the
/// other 76 (§6, §7 rule 2) — and is simply absent from both maps; `sweep`
/// merges results in rather than replacing wholesale, so a skipped repo keeps
/// its last known status until the next tick.
async fn collect(
    write_queues: Arc<WriteQueues>,
    repos: Vec<Repo>,
) -> (HashMap<String, RepoStatus>, HashMap<String, String>) {
    let tasks: Vec<_> = repos
        .into_iter()
        .map(|repo| {
            let write_queues = write_queues.clone();
            tauri::async_runtime::spawn(async move {
                let result = match write_queues.try_read(&repo.id) {
                    Some(_read_guard) => Some(status::query(&repo.path).await),
                    None => None,
                };
                (repo.id, result)
            })
        })
        .collect();

    let mut statuses = HashMap::new();
    let mut errors = HashMap::new();

    for task in tasks {
        // A panicked task is a bug in the parser, not a reason to lose the
        // other 76 repos' results.
        let Ok((id, result)) = task.await else { continue };
        match result {
            Some(Ok(status)) => {
                statuses.insert(id, status);
            }
            Some(Err(err)) => {
                errors.insert(id, err);
            }
            None => {}
        }
    }

    (statuses, errors)
}

/// How many `git fetch` processes the fetch sweep runs at once (§6) — on top
/// of, not instead of, the global 8-process cap in `git.rs` (§7.3). Lower
/// than the status sweep's implicit 8, because a fetch is 0.5–2 s of network
/// time rather than a local read, and status reads should not have to queue
/// behind a batch of them.
const FETCH_CONCURRENCY: usize = 4;

const FETCH_SWEEP_EVENT: &str = "fetch:sweep";

/// The background fetch sweep's counterpart to `SweepEvent` — deliberately
/// separate from it (§6: "these are different mechanisms"). Carries no
/// status data of its own; a fetch changes `refs/remotes/*`, and it is the
/// status sweep that turns that into ahead/behind counts.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FetchSweepEvent {
    root: PathBuf,
    last_fetch_at: HashMap<String, i64>,
    auth_needed: HashSet<String>,
    elapsed_ms: u64,
}

fn start_fetch_sweep(app: &AppHandle, generation: u64, repos: Vec<Repo>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { fetch_sweep(app, generation, repos).await });
}

/// Unlike the status sweep, a fetch sweep that finds its root replaced
/// mid-flight simply drops its results rather than restarting for the new
/// root (§6's "must not vanish" guarantee is about currently-displayed rows,
/// which fetch does not drive) — the next periodic tick picks the new root
/// up on schedule, which is soon enough for a background convenience.
async fn fetch_sweep(app: AppHandle, generation: u64, repos: Vec<Repo>) {
    let (write_queues, interval_secs) = {
        let state = app.state::<AppState>();
        if state.fetch_sweeping.swap(true, Ordering::SeqCst) {
            return;
        }
        let interval_secs = state.settings.lock().expect("settings mutex poisoned").fetch_sweep_secs.max(1);
        (state.write_queues.clone(), interval_secs)
    };

    let (known_last_fetch_at, auth_needed) = {
        let state = app.state::<AppState>();
        let current = state.root.lock().expect("root mutex poisoned");
        match current.as_ref() {
            Some(root) if root.generation == generation => {
                (root.last_fetch_at.clone(), root.auth_needed.clone())
            }
            _ => {
                state.fetch_sweeping.store(false, Ordering::SeqCst);
                return;
            }
        }
    };

    let started = Instant::now();
    let now = now_unix();

    // Repos this tick actually has work for: not currently flagged
    // "auth needed" (§8.7, §13 — a manual fetch is what clears that), and not
    // fetched within the last interval already, whether that timestamp came
    // from this session or the cache seeded at open (§9.5).
    let due: Vec<Repo> = repos
        .into_iter()
        .filter(|repo| !auth_needed.contains(&repo.id))
        .filter(|repo| {
            known_last_fetch_at
                .get(&repo.id)
                .map_or(true, |last| now - last >= interval_secs as i64)
        })
        .collect();

    let (attempted, newly_auth_needed) = fetch_many(write_queues, due).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let state = app.state::<AppState>();
    let published = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        match current.as_mut() {
            Some(root) if root.generation == generation => {
                for id in &attempted {
                    root.last_fetch_at.insert(id.clone(), now);
                    root.auth_needed.remove(id);
                }
                for id in &newly_auth_needed {
                    root.last_fetch_at.insert(id.clone(), now);
                    root.auth_needed.insert(id.clone());
                }
                Some((
                    root.path.clone(),
                    root.statuses.clone(),
                    root.last_fetch_at.clone(),
                    root.auth_needed.clone(),
                ))
            }
            _ => None,
        }
    };

    if let Some((root_path, statuses, last_fetch_at, auth_needed)) = published {
        persist_cache(&app, &root_path, statuses, last_fetch_at.clone());

        let event = FetchSweepEvent { root: root_path, last_fetch_at, auth_needed, elapsed_ms };
        if let Err(err) = app.emit(FETCH_SWEEP_EVENT, event) {
            eprintln!("twogit: could not publish fetch sweep results ({err})");
        }

        // A fetch just moved refs/remotes/*, which is what the status
        // sweep's ahead/behind reads (§8.2) — trigger one now rather than
        // leaving badges stale for up to 60 s (§6).
        trigger_sweep(&app);
    }

    state.fetch_sweeping.store(false, Ordering::SeqCst);
}

/// Dispatches up to [`FETCH_CONCURRENCY`] fetches at once. Returns the ids
/// that were actually attempted (success or a non-auth failure — either way
/// `last_fetch_at` should move forward so a repo that is, say, offline is not
/// retried every tick) separately from the ids whose failure looked like an
/// auth problem, which the caller marks "auth needed" instead (§8.7, §13).
/// A repo with no remote, or whose write lock is currently held by something
/// else, is skipped silently — absent from both lists, so its `last_fetch_at`
/// is untouched and it is reconsidered next tick.
async fn fetch_many(write_queues: Arc<WriteQueues>, repos: Vec<Repo>) -> (Vec<String>, Vec<String>) {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(FETCH_CONCURRENCY));

    let tasks: Vec<_> = repos
        .into_iter()
        .map(|repo| {
            let write_queues = write_queues.clone();
            let semaphore = semaphore.clone();
            tauri::async_runtime::spawn(async move {
                if !remote::has_remote(&repo.path).await {
                    return (repo.id, None);
                }

                let Ok(_permit) = semaphore.acquire().await else {
                    return (repo.id, None);
                };
                // Skip rather than block (§6, §7 rule 2): a repo mid-write
                // this round just waits for the next tick instead of holding
                // up the other three fetch slots.
                let Some(_write_guard) = write_queues.try_write(&repo.id) else {
                    return (repo.id, None);
                };

                (repo.id, Some(remote::fetch_background(&repo.path).await))
            })
        })
        .collect();

    let mut attempted = Vec::new();
    let mut auth_needed = Vec::new();

    for task in tasks {
        let Ok((id, result)) = task.await else { continue };
        match result {
            Some(Ok(())) => attempted.push(id),
            Some(Err(err)) if remote::looks_like_auth_failure(&err) => auth_needed.push(id),
            Some(Err(_)) => attempted.push(id),
            None => {}
        }
    }

    (attempted, auth_needed)
}

/// Most-recent-first, deduplicated, capped. Saved immediately rather than on
/// the settings debounce: opening a folder is exactly the moment a crash would
/// be most annoying to lose.
fn remember_root(state: &AppState, root: &Path) {
    let mut settings = state.settings.lock().expect("settings mutex poisoned");

    settings.recent_roots.retain(|recent| recent != root);
    settings.recent_roots.insert(0, root.to_path_buf());
    settings.recent_roots.truncate(MAX_RECENT_ROOTS);

    if let Err(err) = settings::save(&state.config_dir, &settings) {
        eprintln!("twogit: could not save recent roots ({err})");
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            let settings = settings::load(&config_dir);

            // One git spawn on the startup path, ~20 ms. It buys a single
            // honest answer to "is git here?" instead of every later command
            // failing in its own way (§3). Revisit if step 3's measurement
            // shows the 500 ms budget is tight.
            let git = tauri::async_runtime::block_on(git::probe());
            if !git.available {
                eprintln!("twogit: no usable git on PATH");
            }

            app.manage(AppState {
                config_dir,
                cache_dir,
                settings: Mutex::new(settings),
                git,
                root: Mutex::new(None),
                sweeping: AtomicBool::new(false),
                ticker: Mutex::new(None),
                fetch_sweeping: AtomicBool::new(false),
                fetch_ticker: Mutex::new(None),
                write_queues: Arc::new(WriteQueues::default()),
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
            stage_paths,
            unstage_paths,
            stage_all,
            unstage_all,
            commit_repo,
            fetch_repo,
            pull_repo,
            push_repo,
            publish_branch,
            commit_and_push,
        ])
        .run(tauri::generate_context!())
        .expect("twogit: fatal error while running the application");
}

#[cfg(test)]
mod tests {
    use super::*;

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
    //! $env:TWOGIT_BENCH_ROOT = 'C:\dev\code'
    //! cargo test --release --lib -- --ignored --nocapture bench_status_sweep
    //! ```
    use super::*;

    /// Separates "this machine creates processes slowly" from "twogit creates
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
    #[ignore = "measurement, not a test: needs TWOGIT_BENCH_ROOT"]
    fn bench_status_sweep() {
        let root = std::env::var("TWOGIT_BENCH_ROOT")
            .expect("set TWOGIT_BENCH_ROOT to a folder containing repositories");
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

        // The first sweep pays for cold file caches. Real cold start pays that
        // too, but only once, and from build step 3 it paints from cache while
        // it happens — so the steady-state number is the one under budget.
        let warm = tauri::async_runtime::block_on(collect(write_queues.clone(), repos.clone()));
        println!("warm-up:   {} ok, {} failed", warm.0.len(), warm.1.len());

        for round in 1..=6 {
            let started = Instant::now();
            let (statuses, errors) =
                tauri::async_runtime::block_on(collect(write_queues.clone(), repos.clone()));
            println!(
                "round {round}:   {} repos in {} ms ({} ok, {} failed)",
                repos.len(),
                started.elapsed().as_millis(),
                statuses.len(),
                errors.len(),
            );
            for (id, err) in errors.iter().take(3) {
                println!("           {id}: {err}");
            }
        }
    }
}