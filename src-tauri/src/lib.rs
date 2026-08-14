mod discovery;
mod git;
mod settings;
mod status;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::discovery::Repo;
use crate::git::GitInfo;
use crate::settings::Settings;
use crate::status::RepoStatus;

/// Long enough to be useful in *File → Open Recent*, short enough that the
/// welcome screen stays a list rather than a search problem.
const MAX_RECENT_ROOTS: usize = 10;

const SWEEP_EVENT: &str = "status:sweep";

/// Rust owns the state; the frontend is a view over it (SPEC.md §9.3).
///
/// The per-repo write queues join this struct in build step 4, when there is
/// finally something to write. The global git semaphore lives in `git.rs`
/// instead, as a static — it has to hold across every window (§9.2), and
/// routing it through app state would only make that easier to get wrong.
struct AppState {
    config_dir: PathBuf,
    settings: Mutex<Settings>,
    /// Resolved once at startup: git either exists or the UI says so (§3).
    git: GitInfo,
    root: Mutex<Option<RootState>>,
    /// Re-entrancy guard (§6): a sweep never starts while one is in flight.
    /// The tick is skipped, not queued.
    sweeping: AtomicBool,
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
/// never waits on git (§1).
#[tauri::command]
fn open_root(path: PathBuf, app: AppHandle) -> Result<RootView, String> {
    let root = discovery::canonicalize(&path);
    if !root.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }

    let repos = discovery::scan(&root);
    let state = app.state::<AppState>();

    let generation = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let generation = current.as_ref().map_or(0, |root| root.generation) + 1;
        *current = Some(RootState {
            generation,
            path: root.clone(),
            repos: repos.clone(),
            statuses: HashMap::new(),
            errors: HashMap::new(),
        });
        generation
    };

    remember_root(&state, &root);
    start_sweep(&app, generation, repos.clone());

    Ok(RootView {
        path: root,
        repos,
        statuses: HashMap::new(),
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

fn start_sweep(app: &AppHandle, generation: u64, repos: Vec<Repo>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { sweep(app, generation, repos).await });
}

async fn sweep(app: AppHandle, generation: u64, repos: Vec<Repo>) {
    if app
        .state::<AppState>()
        .sweeping
        .swap(true, Ordering::SeqCst)
    {
        return;
    }

    let started = Instant::now();
    let (statuses, errors) = collect(repos).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let state = app.state::<AppState>();
    state.sweeping.store(false, Ordering::SeqCst);

    let outcome = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        match current.as_mut() {
            Some(root) if root.generation == generation => {
                root.statuses = statuses.clone();
                root.errors = errors.clone();
                Outcome::Publish(SweepEvent {
                    root: root.path.clone(),
                    statuses,
                    errors,
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
            if let Err(err) = app.emit(SWEEP_EVENT, event) {
                eprintln!("twogit: could not publish sweep results ({err})");
            }
        }
        Outcome::Restart(generation, repos) => start_sweep(&app, generation, repos),
        Outcome::Nothing => {}
    }
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
/// events keeps the IPC cost off the sweep's measured time.
async fn collect(repos: Vec<Repo>) -> (HashMap<String, RepoStatus>, HashMap<String, String>) {
    let tasks: Vec<_> = repos
        .into_iter()
        .map(|repo| {
            tauri::async_runtime::spawn(async move {
                let result = status::query(&repo.path).await;
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
            Ok(status) => {
                statuses.insert(id, status);
            }
            Err(err) => {
                errors.insert(id, err);
            }
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
                settings: Mutex::new(settings),
                git,
                root: Mutex::new(None),
                sweeping: AtomicBool::new(false),
            });

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

        // The first sweep pays for cold file caches. Real cold start pays that
        // too, but only once, and from build step 3 it paints from cache while
        // it happens — so the steady-state number is the one under budget.
        let warm = tauri::async_runtime::block_on(collect(repos.clone()));
        println!("warm-up:   {} ok, {} failed", warm.0.len(), warm.1.len());

        for round in 1..=6 {
            let started = Instant::now();
            let (statuses, errors) = tauri::async_runtime::block_on(collect(repos.clone()));
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