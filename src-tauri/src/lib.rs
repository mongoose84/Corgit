mod cache;
mod commit;
mod discovery;
mod git;
mod settings;
mod status;
mod writequeue;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RootView {
    path: PathBuf,
    repos: Vec<Repo>,
    statuses: HashMap<String, RepoStatus>,
    errors: HashMap<String, String>,
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

    let generation = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let generation = current.as_ref().map_or(0, |root| root.generation) + 1;
        *current = Some(RootState {
            generation,
            path: root.clone(),
            repos: repos.clone(),
            statuses: cached.statuses.clone(),
            errors: HashMap::new(),
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
        root.repos = repos;

        (
            root.generation,
            RootView {
                path: root.path.clone(),
                repos: root.repos.clone(),
                statuses: root.statuses.clone(),
                errors: root.errors.clone(),
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

        (root.path.clone(), root.statuses.clone())
    };

    let (root_path, statuses) = published;
    let on_disk = RootCache { version: cache::CACHE_VERSION, statuses };
    if let Err(err) = cache::save(&state.cache_dir, &root_path, &on_disk) {
        eprintln!("twogit: could not save status cache ({err})");
    }

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
                Outcome::Publish(SweepEvent {
                    root: root.path.clone(),
                    statuses: root.statuses.clone(),
                    errors: root.errors.clone(),
                    elapsed_ms,
                })
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
        Outcome::Publish(event) => {
            // Saved after every sweep rather than on a separate debounce
            // timer: sweeps are already throttled to the configured interval
            // (§6), so this already satisfies "not on every status change"
            // (§9.5 rule 4) without a second timer to keep in sync with the
            // first. Errors are deliberately not cached — a repo that failed
            // this round keeps whatever the *previous* successful sweep or
            // cache load left behind, until it succeeds again.
            let on_disk = RootCache {
                version: cache::CACHE_VERSION,
                statuses: event.statuses.clone(),
            };
            if let Err(err) = cache::save(&state.cache_dir, &event.root, &on_disk) {
                eprintln!("twogit: could not save status cache ({err})");
            }

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
/// next tick, and (re)start the ticker that was aborted on the last blur.
fn on_focus(app: &AppHandle) {
    trigger_sweep(app);
    start_ticker(app);
}

/// Window lost focus (§6): stop ticking entirely. Not "skip the next tick" —
/// an unfocused window has no sweep timer running at all, which is what
/// makes background CPU zero rather than merely low.
fn on_blur(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut ticker = state.ticker.lock().expect("ticker mutex poisoned");
    if let Some(handle) = ticker.take() {
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

/// What a finished sweep does next. Split out so the decision is made while
/// the root lock is held and acted on after it is released.
enum Outcome {
    Publish(SweepEvent),
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
            stage_paths,
            unstage_paths,
            stage_all,
            unstage_all,
            commit_repo,
        ])
        .run(tauri::generate_context!())
        .expect("twogit: fatal error while running the application");
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