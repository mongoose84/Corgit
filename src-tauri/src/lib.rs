mod atomicfile;
mod branch;
mod cache;
mod commit;
mod diff;
mod discovery;
mod git;
mod graph;
mod menu;
mod remote;
mod roots;
mod settings;
mod status;
mod watch;
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
/// The global git semaphore lives in `git.rs` instead, as a static — it has to
/// cover every git spawn in the process (§7.3, §9.2), and routing it through
/// app state would only make that easier to get wrong.
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
    /// One write queue per repo, shared process-wide (§7, §9.2). First
    /// used in build step 4, where staging and commit are the first writes.
    /// `Arc`-wrapped so the sweep can clone a handle into its per-repo tasks
    /// without needing an `AppHandle` there too (§6, §7 rule 2).
    write_queues: Arc<WriteQueues>,
    /// FS watchers on the hot set — pinned ∪ selected (§6, build step 9).
    hot_watchers: watch::HotWatchers,
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
        // Unlike a pin, the selection is not the user's authored state — it is
        // where they happen to be — and a selection naming a repo the list no
        // longer shows would leave the middle pane describing nothing.
        if self.selected.as_ref().is_some_and(|id| !known(id)) {
            self.selected = None;
        }

        self.repos = repos;
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
    start_sweep(&app, generation, repos.clone());
    sync_hot_watchers(&app);

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
#[tauri::command]
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

    start_sweep(&app, generation, view.repos.clone());
    sync_hot_watchers(&app);
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

/// Discard the unstaged changes to these paths (§5.2, §8.6). The same shape as
/// every other write — the destructive part lives entirely in
/// `commit::discard`'s flags, and the confirmation entirely in the frontend.
#[tauri::command]
async fn discard_paths(repo_id: String, paths: Vec<String>, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, |path| async move { commit::discard(&path, &paths).await })
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
    write_and_refresh(&app, repo_id, |path| async move {
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
    write_and_refresh(&app, repo_id, |path| async move {
        branch::create(&path, &name, &start_point, checkout).await
    })
    .await
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

    // VS Code's Windows launcher is a `.cmd` shim; `Command::new("code")` alone
    // does not resolve it (Windows does not walk PATHEXT for a bare child
    // process the way a shell does), so it has to run through one.
    let mut command = if cfg!(windows) {
        let mut command = tokio::process::Command::new("cmd");
        command.args(["/C", "code"]);
        command
    } else {
        tokio::process::Command::new("code")
    };
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
#[tauri::command]
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
    sync_hot_watchers(&app);
    Ok(pins)
}

/// Unpin everything in one go (§5.1). A loop of `toggle_pin` from the
/// frontend would do the same thing, but it would write `roots/<hash>.json`
/// and resync the watchers once per pin — this is one write and one resync,
/// and it cannot leave a half-cleared set behind if a call in the middle
/// fails.
#[tauri::command]
fn clear_pins(app: AppHandle) -> Result<HashSet<String>, String> {
    let state = app.state::<AppState>();
    let (root_path, selected) = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let root = current.as_mut().ok_or_else(|| "No folder is open".to_string())?;
        root.pins.clear();
        (root.path.clone(), root.selected.clone())
    };

    persist_root_settings(&app, &root_path, HashSet::new(), selected);
    sync_hot_watchers(&app);
    Ok(HashSet::new())
}

/// The frontend's current selection, mirrored server-side (§9.5's persisted
/// `last_selected`, and the input to the hot set in §6/build step 9's
/// watchers). `None` when nothing is selected.
#[tauri::command]
fn set_selected_repo(repo_id: Option<String>, app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (root_path, pins, selected) = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let root = current.as_mut().ok_or_else(|| "No folder is open".to_string())?;
        root.selected = repo_id;
        (root.path.clone(), root.pins.clone(), root.selected.clone())
    };

    persist_root_settings(&app, &root_path, pins, selected.clone());
    sync_hot_watchers(&app);
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

/// §13's merge-conflict banner: "Abort merge", one of its exactly two ways
/// out (the other is *Open in VS Code*).
#[tauri::command]
async fn merge_abort(repo_id: String, app: AppHandle) -> Result<(), String> {
    write_and_refresh(&app, repo_id, |path| async move { remote::merge_abort(&path).await }).await
}

/// A branch with no upstream configured (§8.7). `remote::publish` pushes
/// `HEAD`, so the known branch name is only a guard here — it turns the one
/// case git would refuse anyway, a detached HEAD, into a sentence that says so.
#[tauri::command]
async fn publish_branch(repo_id: String, app: AppHandle) -> Result<(), String> {
    current_branch(&app, &repo_id)?;
    write_and_refresh(&app, repo_id, |path| async move { remote::publish(&path).await }).await
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

    write_and_refresh(&app, repo_id, |path| async move {
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
/// what a full sweep does for all of them. `pub(crate)` so `watch.rs`'s
/// debounced FS-watcher callbacks (§6) can call it directly, the same way
/// `write_and_refresh` does after every mutating command.
pub(crate) async fn emit_repo_status(app: &AppHandle, repo_id: &str, path: &Path) {
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
        log::warn!("could not publish repo status ({err})");
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
        log::warn!("could not save status cache ({err})");
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
                log::warn!("could not publish sweep results ({err})");
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

/// Recomputes the hot set — pinned ∪ selected (§6) — from whatever root is
/// currently open, and syncs the FS watchers to match. Called after anything
/// that can change either half: `open_root`, `toggle_pin`,
/// `set_selected_repo`, `refresh_root`.
fn sync_hot_watchers(app: &AppHandle) {
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    let Some(root) = current.as_ref() else {
        drop(current);
        state.hot_watchers.clear();
        return;
    };

    let hot: Vec<(String, PathBuf)> = root
        .repos
        .iter()
        .filter(|repo| root.pins.contains(&repo.id) || root.selected.as_deref() == Some(repo.id.as_str()))
        .map(|repo| (repo.id.clone(), repo.path.clone()))
        .collect();
    drop(current);

    state.hot_watchers.sync(app, &hot);
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
            log::warn!("could not publish fetch sweep results ({err})");
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
            let config_dir = app.path().app_config_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            let settings = settings::load(&config_dir);

            // One git spawn on the startup path, ~20 ms. It buys a single
            // honest answer to "is git here?" instead of every later command
            // failing in its own way (§3). Revisit if step 3's measurement
            // shows the 500 ms budget is tight.
            let git = tauri::async_runtime::block_on(git::probe());
            if !git.available {
                log::error!("no usable git on PATH");
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
                hot_watchers: watch::HotWatchers::default(),
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
            stage_all,
            unstage_all,
            commit_repo,
            fetch_repo,
            pull_repo,
            push_repo,
            merge_abort,
            publish_branch,
            commit_and_push,
            switch_branch,
            create_branch,
            open_in_vscode,
            open_in_terminal,
            toggle_pin,
            clear_pins,
            set_selected_repo,
            menu::menu_command,
            menu::publish_pane_visibility,
        ])
        .run(tauri::generate_context!())
        .expect("corgit: fatal error while running the application");
}

#[cfg(test)]
mod tests {
    use super::*;

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