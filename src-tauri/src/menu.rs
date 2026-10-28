//! The menu's Rust half (SPEC.md §4.1).
//!
//! The menu bar itself is drawn in the webview now — `MenuBar.svelte` — because
//! it shares one row with the app mark and the caption buttons, and a native
//! Win32 menu can only be its own row below the caption. That inverted which
//! side of the IPC boundary the menu *lives* on, but deliberately not which
//! side each item's **behaviour** lives on: that split was right before and is
//! unchanged.
//!
//! So what is left here is exactly the items that only touch process lifecycle
//! or a boolean Rust already owns (§9.3) — Close Window, Exit, the two View
//! checkboxes, About, Open Log Folder. They arrive as one `menu_command`
//! invoke instead of one `on_menu_event`, and their bodies are the same bodies.
//! Everything else — Open Folder, Open Recent, Fetch, Pull, Push, Reset Pane
//! Sizes, Reload — is now handled entirely in the frontend, where it always
//! already lived; it used to make a round trip out to Rust and straight back
//! as a `menu:action` event, and with a frontend menu that event has no reason
//! to exist.
//!
//! Gone with the native menu, and worth knowing were once free: accelerators,
//! Alt mnemonics, and the menu's own screen-reader semantics. The first are
//! re-registered in `TitleBar.svelte`; the second is not (§4.1 records the
//! omission); the third is hand-built out of ARIA roles.
//!
//! "New Window" is still intentionally absent — multi-window is deferred to v2
//! (§2, §9.2). "Check for Updates" (§12, build step 10) is omitted for the
//! same reason: a menu item with nothing behind it is worse than no item.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::git::GitInfo;

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

/// One entry point for every menu item Rust still owns, keyed by the same ids
/// the native menu used. A single command rather than one per item because
/// the set is small, closed, and defined by one table in the spec — seven
/// separate commands would put that table in seven places.
///
/// An unknown id is logged and dropped rather than returning an error: the
/// caller is our own menu model, so an unknown id is a bug in this repository,
/// not something a user can provoke or a dialog can help with. That also makes
/// a mismatch silent — a menu item that does nothing — which is why the id
/// table is `MenuCommand::from_id`, pinned against the frontend's own list.
#[tauri::command]
pub fn menu_command(app: AppHandle, id: String) {
    let Some(command) = MenuCommand::from_id(&id) else {
        log::warn!("unknown menu command ({id})");
        return;
    };
    match command {
        MenuCommand::CloseWindow => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.close();
            }
        }
        MenuCommand::Exit => app.exit(0),
        MenuCommand::ToggleRepoList => toggle_visibility(&app, |v| &mut v.repo_list),
        MenuCommand::ToggleCommitPane => toggle_visibility(&app, |v| &mut v.commit_pane),
        MenuCommand::About => show_about(&app),
        MenuCommand::OpenLogs => open_log_folder(&app),
    }
}

/// The items `menu_command` handles. The ids are a contract with `RUST_IDS` in
/// `menu.svelte.ts`, which decides what is sent here rather than handled in
/// the webview. The two lists are in different languages, so nothing but
/// `rust_ids_match_the_frontend` keeps them the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuCommand {
    CloseWindow,
    Exit,
    ToggleRepoList,
    ToggleCommitPane,
    About,
    OpenLogs,
}

impl MenuCommand {
    fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "close-window" => Self::CloseWindow,
            "exit" => Self::Exit,
            "toggle-repo-list" => Self::ToggleRepoList,
            "toggle-commit-pane" => Self::ToggleCommitPane,
            "about" => Self::About,
            "open-logs" => Self::OpenLogs,
            _ => return None,
        })
    }
}

/// Flips one field of the Rust-owned `PaneVisibility` (via the `AppState`
/// this module doesn't itself define — `lib.rs` owns that struct) and tells
/// the frontend which pane to hide or show.
///
/// The checkmark is not set here any more: with the menu in the webview, the
/// checkbox is rendered from the same `pane:visibility` event the panes react
/// to, so there is no second copy of the boolean left to keep in sync.
fn toggle_visibility(app: &AppHandle, field: impl Fn(&mut PaneVisibility) -> &mut bool) {
    let state = app.state::<crate::AppState>();
    let visibility = {
        let mut current = state.pane_visibility.lock().expect("pane-visibility mutex poisoned");
        *field(&mut current) = !*field(&mut current);
        *current
    };

    if let Err(err) = app.emit(PANE_VISIBILITY_EVENT, visibility) {
        log::warn!("could not publish pane visibility ({err})");
    }
}

/// Publishes the current visibility without changing it, so a webview that
/// has just (re)loaded can render the two checkmarks correctly. The native
/// menu never needed this — it was built with the right state and outlived
/// every reload, being outside the webview. An HTML menu is rebuilt by every
/// reload and starts from its own defaults, which are only right until the
/// first toggle.
#[tauri::command]
pub fn publish_pane_visibility(app: AppHandle) {
    let visibility = {
        let state = app.state::<crate::AppState>();
        let current = state.pane_visibility.lock().expect("pane-visibility mutex poisoned");
        *current
    };

    if let Err(err) = app.emit(PANE_VISIBILITY_EVENT, visibility) {
        log::warn!("could not publish pane visibility ({err})");
    }
}

/// Help ▸ Open Log Folder — reveals the directory `lib.rs`'s `logging` writes
/// to. The folder is created eagerly rather than assumed: on a clean install
/// nothing has been logged yet, and a menu item that opens nothing reads as
/// broken rather than as "no problems so far".
///
/// Shells out the same way `open_in_vscode` does, and is `cfg`-gated for the
/// same reason (§10) — there is no portable "reveal this folder".
fn open_log_folder(app: &AppHandle) {
    let Ok(dir) = app.path().app_log_dir() else {
        log::warn!("no log directory to open");
        return;
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        log::warn!("could not create the log folder ({err})");
        return;
    }

    let opener = if cfg!(windows) { "explorer" } else { "xdg-open" };
    // `explorer` exits non-zero even when it succeeds, so the spawn is all
    // that can be checked here — the status is deliberately not waited on.
    if let Err(err) = std::process::Command::new(opener).arg(&dir).spawn() {
        log::warn!("could not open the log folder ({err})");
    }
}

fn show_about(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    // `get` rather than awaiting the probe: this is a synchronous menu
    // handler, and by the time anyone reaches Help ▸ About the answer has been
    // in the cell for minutes.
    let message = format!("Corgit {}\n{}", env!("CARGO_PKG_VERSION"), about_git_line(state.git.get()));

    app.dialog().message(message).title("About Corgit").show(|_| {});
}

/// About's git line. The unresolved arm exists so that the one case where the
/// probe has not answered — still stuck in an anti-malware scan — says so
/// instead of claiming git is missing (§3).
fn about_git_line(git: Option<&GitInfo>) -> String {
    match git {
        Some(git) => match &git.version {
            Some(version) => format!("Git {version} ({})", git.read_binary.as_deref().unwrap_or("git")),
            None => "Git not found".to_string(),
        },
        None => "Still looking for git".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend's `RUST_IDS`, read out of its source. The Rust half of a
    /// cross-language contract has no other way to see the other half, and a
    /// mismatch either way fails silently: an id Rust dropped is logged and
    /// ignored, and one the frontend dropped is dispatched locally, where its
    /// `switch` has no case for it either.
    fn frontend_rust_ids() -> Vec<String> {
        let source = include_str!("../../src/lib/menu.svelte.ts");
        let start = source.find("const RUST_IDS = new Set([").expect("RUST_IDS not found in menu.svelte.ts");
        let list = &source[start..];
        let list = &list[list.find('[').unwrap() + 1..list.find(']').unwrap()];
        list.split(',').map(|id| id.trim().trim_matches('\'').to_string()).filter(|id| !id.is_empty()).collect()
    }

    #[test]
    fn rust_ids_match_the_frontend() {
        let ids = frontend_rust_ids();
        for id in &ids {
            assert!(MenuCommand::from_id(id).is_some(), "the frontend sends {id:?}, which menu.rs does not handle");
        }

        // The other direction: every command is reachable from some id the
        // frontend sends, checked per command so two ids landing on one arm
        // cannot stand in for an arm nothing reaches.
        let reached: Vec<MenuCommand> = ids.iter().filter_map(|id| MenuCommand::from_id(id)).collect();
        let all = [
            MenuCommand::CloseWindow,
            MenuCommand::Exit,
            MenuCommand::ToggleRepoList,
            MenuCommand::ToggleCommitPane,
            MenuCommand::About,
            MenuCommand::OpenLogs,
        ];
        for command in all {
            assert!(reached.contains(&command), "{command:?} has no id in the frontend's RUST_IDS");
        }
    }

    #[test]
    fn an_unknown_id_is_not_a_command() {
        assert_eq!(MenuCommand::from_id("open-folder"), None);
        assert_eq!(MenuCommand::from_id(""), None);
    }

    fn git(version: Option<&str>, read_binary: Option<&str>) -> GitInfo {
        GitInfo {
            available: version.is_some(),
            version: version.map(str::to_string),
            supports_fsmonitor: true,
            read_binary: read_binary.map(str::to_string),
        }
    }

    #[test]
    fn about_names_the_version_and_the_binary_the_sweep_runs() {
        let info = git(Some("2.53.0.windows.1"), Some(r"C:\Program Files\Git\mingw64\bin\git.exe"));
        assert_eq!(about_git_line(Some(&info)), r"Git 2.53.0.windows.1 (C:\Program Files\Git\mingw64\bin\git.exe)");
    }

    /// §3: a probe that has not answered yet is not a missing git.
    #[test]
    fn about_tells_a_pending_probe_from_a_missing_git() {
        assert_eq!(about_git_line(None), "Still looking for git");
        assert_eq!(about_git_line(Some(&git(None, None))), "Git not found");
    }
}
