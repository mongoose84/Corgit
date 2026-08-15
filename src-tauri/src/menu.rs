//! Native menu bar (SPEC.md §4.1) — via `tauri::menu`, part of the core
//! `tauri` crate, not a plugin (no capability/ACL entry needed).
//!
//! Items that only touch process lifecycle or a boolean Rust already owns
//! (Close Window, Exit, Reload, the View checkboxes, About) are handled
//! directly here. Everything else — anything that already has a home in
//! `repos.svelte.ts` (Open Folder, Open Recent, Fetch, Pull, Push, Reset Pane
//! Sizes) — is forwarded to the frontend as one `menu:action` event rather
//! than reimplemented, so the git/selection logic stays in the one place it
//! already lives (§9.3: Rust owns state, but *behaviour* the frontend already
//! has shouldn't grow a second copy here).
//!
//! "New Window" is intentionally absent — multi-window is a deferred,
//! separate piece of work. "Check for Updates" (§12, build step 10) and
//! "Open Log Folder" (no log file exists yet) are omitted for the same
//! reason: a menu item with nothing behind it is worse than no item.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, Menu, MenuBuilder, MenuItem, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_dialog::DialogExt;

const MENU_ACTION_EVENT: &str = "menu:action";
const PANE_VISIBILITY_EVENT: &str = "pane:visibility";

/// Rust-owned state for the two View-menu checkboxes (§9.3). Not persisted —
/// ephemeral per session, same as any other purely-visual toggle.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneVisibility {
    pub repo_list: bool,
    pub commit_pane: bool,
}

impl Default for PaneVisibility {
    fn default() -> Self {
        Self { repo_list: true, commit_pane: true }
    }
}

/// Handles kept around after the menu is built, for the mutations that
/// happen later: enabling/disabling Fetch/Pull/Push on selection change,
/// flipping a View checkbox, and repopulating Open Recent. Each field is a
/// handle to a native menu object (mutation methods take `&self`), not owned
/// UI state, so no `Mutex` wrapper is needed around the struct itself —
/// `PaneVisibility`'s actual booleans live separately, in `AppState`.
pub struct MenuHandles {
    pub open_recent: tauri::menu::Submenu<Wry>,
    pub fetch_item: MenuItem<Wry>,
    pub pull_item: MenuItem<Wry>,
    pub push_item: MenuItem<Wry>,
    pub repo_list_check: CheckMenuItem<Wry>,
    pub commit_pane_check: CheckMenuItem<Wry>,
}

/// What gets forwarded to the frontend for items whose logic already lives in
/// `repos.svelte.ts` / `settings.svelte.ts`.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum MenuAction {
    OpenFolder,
    OpenRecent { path: PathBuf },
    ResetPaneSizes,
    Fetch,
    Pull,
    Push,
}

/// Builds the menu bar and installs it, returning the handles later
/// mutations need. Called once at startup with whatever `settings` and
/// `PaneVisibility` are current at that point.
pub fn install(
    app: &AppHandle,
    recent_roots: &[PathBuf],
    repo_selected: bool,
    visibility: PaneVisibility,
) -> tauri::Result<MenuHandles> {
    let (menu, handles) = build(app, recent_roots, repo_selected, visibility)?;
    app.set_menu(menu)?;
    register_event_handler(app);
    Ok(handles)
}

fn build(
    app: &AppHandle,
    recent_roots: &[PathBuf],
    repo_selected: bool,
    visibility: PaneVisibility,
) -> tauri::Result<(Menu<Wry>, MenuHandles)> {
    let open_folder = MenuItemBuilder::with_id("open-folder", "Open Folder…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let open_recent = SubmenuBuilder::new(app, "Open Recent").build()?;
    fill_open_recent(&open_recent, recent_roots)?;
    let close_window = MenuItemBuilder::with_id("close-window", "Close Window")
        .accelerator("CmdOrCtrl+W")
        .build(app)?;
    let exit = MenuItemBuilder::with_id("exit", "Exit").build(app)?;

    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&open_folder)
        .item(&open_recent)
        .separator()
        .item(&close_window)
        .item(&exit)
        .build()?;

    let repo_list_check = CheckMenuItemBuilder::with_id("toggle-repo-list", "Toggle Repo List")
        .checked(visibility.repo_list)
        .build(app)?;
    let commit_pane_check = CheckMenuItemBuilder::with_id("toggle-commit-pane", "Toggle Commit Pane")
        .checked(visibility.commit_pane)
        .build(app)?;
    let reset_pane_sizes = MenuItemBuilder::with_id("reset-pane-sizes", "Reset Pane Sizes").build(app)?;
    let reload = MenuItemBuilder::with_id("reload", "Reload").build(app)?;

    let view_menu = SubmenuBuilder::new(app, "View")
        .item(&repo_list_check)
        .item(&commit_pane_check)
        .separator()
        .item(&reset_pane_sizes)
        .item(&reload)
        .build()?;

    // Disabled with no repo selected (§4.1's table) — kept in sync by
    // `set_repo_selected`, called from the `set_selected_repo` command.
    let fetch_item = MenuItemBuilder::with_id("fetch", "Fetch").enabled(repo_selected).build(app)?;
    let pull_item = MenuItemBuilder::with_id("pull", "Pull").enabled(repo_selected).build(app)?;
    let push_item = MenuItemBuilder::with_id("push", "Push").enabled(repo_selected).build(app)?;

    let repository_menu = SubmenuBuilder::new(app, "Repository")
        .item(&fetch_item)
        .item(&pull_item)
        .item(&push_item)
        .build()?;

    let about = MenuItemBuilder::with_id("about", "About").build(app)?;
    let help_menu = SubmenuBuilder::new(app, "Help").item(&about).build()?;

    let menu = MenuBuilder::new(app)
        .item(&file_menu)
        .item(&view_menu)
        .item(&repository_menu)
        .item(&help_menu)
        .build()?;

    Ok((
        menu,
        MenuHandles { open_recent, fetch_item, pull_item, push_item, repo_list_check, commit_pane_check },
    ))
}

fn register_event_handler(app: &AppHandle) {
    app.on_menu_event(|app, event| {
        let id = event.id().as_ref();

        match id {
            "close-window" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.close();
                }
            }
            "exit" => app.exit(0),
            "reload" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.eval("location.reload()");
                }
            }
            "toggle-repo-list" => toggle_visibility(app, |v| &mut v.repo_list),
            "toggle-commit-pane" => toggle_visibility(app, |v| &mut v.commit_pane),
            "about" => show_about(app),
            "open-folder" => emit_action(app, MenuAction::OpenFolder),
            "reset-pane-sizes" => emit_action(app, MenuAction::ResetPaneSizes),
            "fetch" => emit_action(app, MenuAction::Fetch),
            "pull" => emit_action(app, MenuAction::Pull),
            "push" => emit_action(app, MenuAction::Push),
            _ => {
                if let Some(raw) = id.strip_prefix("open-recent|") {
                    emit_action(app, MenuAction::OpenRecent { path: PathBuf::from(raw) });
                }
            }
        }
    });
}

fn emit_action(app: &AppHandle, action: MenuAction) {
    if let Err(err) = app.emit(MENU_ACTION_EVENT, action) {
        eprintln!("twogit: could not forward menu action ({err})");
    }
}

/// Flips one field of the Rust-owned `PaneVisibility` (via the `AppState`
/// this module doesn't itself define — `lib.rs` owns that struct), updates
/// the checkbox that triggered this, and tells the frontend which pane to
/// hide or show.
fn toggle_visibility(app: &AppHandle, field: impl Fn(&mut PaneVisibility) -> &mut bool) {
    let state = app.state::<crate::AppState>();
    let visibility = {
        let mut current = state.pane_visibility.lock().expect("pane-visibility mutex poisoned");
        *field(&mut current) = !*field(&mut current);
        *current
    };

    let _ = state.menu.repo_list_check.set_checked(visibility.repo_list);
    let _ = state.menu.commit_pane_check.set_checked(visibility.commit_pane);

    if let Err(err) = app.emit(PANE_VISIBILITY_EVENT, visibility) {
        eprintln!("twogit: could not publish pane visibility ({err})");
    }
}

fn show_about(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let git_line = match &state.git.version {
        Some(version) => format!("Git {version} ({})", state.git.read_binary.as_deref().unwrap_or("git")),
        None => "Git not found".to_string(),
    };
    let message = format!("twogit {}\n{git_line}", env!("CARGO_PKG_VERSION"));

    app.dialog().message(message).title("About twogit").show(|_| {});
}

/// Called from `set_selected_repo` (§9.3: the frontend's selection, mirrored
/// server-side) to keep Fetch/Pull/Push's enabled state matching whether a
/// repo is selected (§4.1's table: "Disabled when no repo is selected").
pub fn set_repo_selected(app: &AppHandle, selected: bool) {
    let state = app.state::<crate::AppState>();
    let _ = state.menu.fetch_item.set_enabled(selected);
    let _ = state.menu.pull_item.set_enabled(selected);
    let _ = state.menu.push_item.set_enabled(selected);
}

/// Called from `remember_root` whenever `settings.recent_roots` changes —
/// repopulates the one submenu in place rather than rebuilding the whole
/// menu bar, which stays untouched.
pub fn refresh_open_recent(app: &AppHandle, recent_roots: &[PathBuf]) {
    let state = app.state::<crate::AppState>();
    if let Err(err) = fill_open_recent(&state.menu.open_recent, recent_roots) {
        eprintln!("twogit: could not refresh Open Recent menu ({err})");
    }
}

fn fill_open_recent(open_recent: &tauri::menu::Submenu<Wry>, recent_roots: &[PathBuf]) -> tauri::Result<()> {
    for item in open_recent.items()? {
        open_recent.remove(&item)?;
    }
    for root in recent_roots {
        let id = format!("open-recent|{}", root.display());
        let item = MenuItemBuilder::with_id(id, label_for(root)).build(open_recent.app_handle())?;
        open_recent.append(&item)?;
    }
    Ok(())
}

fn label_for(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string())
}
