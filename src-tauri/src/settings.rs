//! Global settings (SPEC.md §9.5).
//!
//! Global here means "not tied to a root": pane widths, sweep intervals, the
//! recent-roots list. Per-root state (pins, last selected repo, window
//! geometry) lands in `roots/<hash>.json` alongside the per-root status cache
//! in `cache/<hash>.json` — build step 3, when there is state worth caching.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomicfile;

/// Bumped when the on-disk shape changes incompatibly.
pub const SETTINGS_VERSION: u32 = 1;

const FILE_NAME: &str = "settings.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneWidths {
    /// Fraction of usable width, not pixels — survives window resizing.
    pub left: f64,
    pub middle: f64,
}

impl Default for PaneWidths {
    fn default() -> Self {
        Self {
            left: 0.25,
            middle: 0.20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub pane_widths: PaneWidths,
    /// The diff view's old/new split (§5.4), as a fraction of that pane's
    /// width. Stored beside the pane widths and for the same reason: it is a
    /// layout preference, and re-dragging it every session is exactly the kind
    /// of small friction the persisted widths exist to avoid.
    pub diff_split: f64,
    /// Direct children of a root only (§8.1). Present so the value is
    /// inspectable, not because deeper scanning is supported.
    pub scan_depth: u32,
    pub status_sweep_secs: u64,
    pub fetch_sweep_secs: u64,
    pub recent_roots: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            pane_widths: PaneWidths::default(),
            diff_split: 0.5,
            scan_depth: 1,
            status_sweep_secs: 60,
            fetch_sweep_secs: 300,
            recent_roots: Vec::new(),
        }
    }
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

/// Settings are advisory: every failure falls back to defaults rather than
/// surfacing an error, because a corrupt file must never block startup.
pub fn load(config_dir: &Path) -> Settings {
    atomicfile::prune_stale_temps(config_dir);

    let Ok(raw) = fs::read_to_string(path(config_dir)) else {
        return Settings::default();
    };

    match serde_json::from_str::<Settings>(&raw) {
        Ok(settings) if settings.version == SETTINGS_VERSION => settings,
        Ok(settings) => migrate(settings),
        Err(err) => {
            log::warn!("settings unreadable, using defaults ({err})");
            Settings::default()
        }
    }
}

/// No older versions exist yet, so anything unrecognised resets. Real
/// migrations land here as the schema grows.
fn migrate(old: Settings) -> Settings {
    log::warn!("settings version {} not understood, using defaults", old.version);
    Settings::default()
}

/// Atomic write (§9.5 rule 1) — see [`atomicfile`]. A menu toggle and a
/// frontend `save_settings` can both land here at once.
pub fn save(config_dir: &Path, settings: &Settings) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;

    atomicfile::write(&path(config_dir), &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = std::env::temp_dir().join("corgit-test-missing");
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(load(&dir).scan_depth, 1);
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = std::env::temp_dir().join("corgit-test-corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(path(&dir), b"{not json").unwrap();

        let loaded = load(&dir);

        assert_eq!(loaded.version, SETTINGS_VERSION);
        assert_eq!(loaded.pane_widths.left, 0.25);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join("corgit-test-roundtrip");
        let _ = fs::remove_dir_all(&dir);

        let mut settings = Settings::default();
        settings.pane_widths.left = 0.31;
        settings.recent_roots.push(PathBuf::from(r"C:\dev\code"));
        save(&dir, &settings).unwrap();

        let loaded = load(&dir);

        assert_eq!(loaded.pane_widths.left, 0.31);
        assert_eq!(loaded.recent_roots.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// The container-level `#[serde(default)]` is what lets a field be added
    /// without a version bump: a settings file written before `diff_split`
    /// existed still loads, with the new field defaulted rather than the whole
    /// file rejected.
    #[test]
    fn a_file_predating_a_field_keeps_the_rest_and_defaults_the_new_one() {
        let dir = std::env::temp_dir().join("corgit-test-added-field");
        fs::create_dir_all(&dir).unwrap();
        fs::write(path(&dir), br#"{"version":1,"paneWidths":{"left":0.31,"middle":0.2}}"#).unwrap();

        let loaded = load(&dir);

        assert_eq!(loaded.pane_widths.left, 0.31);
        assert_eq!(loaded.diff_split, 0.5);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_version_resets() {
        let dir = std::env::temp_dir().join("corgit-test-version");
        fs::create_dir_all(&dir).unwrap();
        fs::write(path(&dir), br#"{"version":99,"scanDepth":7}"#).unwrap();

        assert_eq!(load(&dir).scan_depth, 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
