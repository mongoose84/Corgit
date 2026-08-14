//! Global settings (SPEC.md §9.5).
//!
//! Global here means "not tied to a root": pane widths, sweep intervals, the
//! recent-roots list. Per-root state (pins, last selected repo, window
//! geometry) lands in `roots/<hash>.json` alongside the per-root status cache
//! in `cache/<hash>.json` — build step 3, when there is state worth caching.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
    let Ok(raw) = fs::read_to_string(path(config_dir)) else {
        return Settings::default();
    };

    match serde_json::from_str::<Settings>(&raw) {
        Ok(settings) if settings.version == SETTINGS_VERSION => settings,
        Ok(settings) => migrate(settings),
        Err(err) => {
            eprintln!("twogit: settings unreadable, using defaults ({err})");
            Settings::default()
        }
    }
}

/// No older versions exist yet, so anything unrecognised resets. Real
/// migrations land here as the schema grows.
fn migrate(old: Settings) -> Settings {
    eprintln!(
        "twogit: settings version {} not understood, using defaults",
        old.version
    );
    Settings::default()
}

/// Write to a sibling temp file and rename over the target, so a crash
/// mid-write cannot leave a truncated file. `fs::rename` replaces an existing
/// destination on Windows, and both paths are in the same directory, so the
/// rename stays on one volume.
pub fn save(config_dir: &Path, settings: &Settings) -> std::io::Result<()> {
    fs::create_dir_all(config_dir)?;

    let target = path(config_dir);
    let temp = target.with_extension("json.tmp");

    let json = serde_json::to_vec_pretty(settings)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;

    let mut file = fs::File::create(&temp)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);

    fs::rename(&temp, &target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = std::env::temp_dir().join("twogit-test-missing");
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(load(&dir).scan_depth, 1);
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = std::env::temp_dir().join("twogit-test-corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(path(&dir), b"{not json").unwrap();

        let loaded = load(&dir);

        assert_eq!(loaded.version, SETTINGS_VERSION);
        assert_eq!(loaded.pane_widths.left, 0.25);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join("twogit-test-roundtrip");
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

    #[test]
    fn unknown_version_resets() {
        let dir = std::env::temp_dir().join("twogit-test-version");
        fs::create_dir_all(&dir).unwrap();
        fs::write(path(&dir), br#"{"version":99,"scanDepth":7}"#).unwrap();

        assert_eq!(load(&dir).scan_depth, 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
