mod settings;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{Manager, State};

use crate::settings::Settings;

/// Rust owns the state; the frontend is a view over it (SPEC.md §9.3).
///
/// The repo map, per-repo write queues and the global git semaphore join this
/// struct in later build steps — keeping them behind one owner is what makes
/// multi-window safe (§9.2).
struct AppState {
    config_dir: PathBuf,
    settings: Mutex<Settings>,
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
fn save_settings(settings: Settings, state: State<'_, AppState>) -> Result<(), String> {
    settings::save(&state.config_dir, &settings).map_err(|err| err.to_string())?;
    *state.settings.lock().expect("settings mutex poisoned") = settings;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let settings = settings::load(&config_dir);

            app.manage(AppState {
                config_dir,
                settings: Mutex::new(settings),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_settings, save_settings])
        .run(tauri::generate_context!())
        .expect("twogit: fatal error while running the application");
}
