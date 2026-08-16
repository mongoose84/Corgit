//! Per-root settings (SPEC.md §9.5): pins and the last-selected repo.
//!
//! Deliberately separate from `cache.rs` even though both are per-root files
//! keyed the same way (`cache::hash_root`) — the cache is disposable (rule 3:
//! any parse failure deletes and rebuilds silently), but pins are the user's
//! own choices with no other source of truth, so deleting the cache must
//! never lose them (rule 5). That is the whole reason there are two files
//! instead of one.
//!
//! Window size and position (also listed in §9.5's table for this file) land
//! here once multi-window ships; a single-window app has nowhere meaningful
//! to restore geometry *to* yet.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomicfile;
use crate::cache::hash_root;

pub const ROOTS_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RootSettings {
    pub version: u32,
    pub pins: HashSet<String>,
    pub last_selected: Option<String>,
}

impl Default for RootSettings {
    fn default() -> Self {
        Self { version: ROOTS_VERSION, pins: HashSet::new(), last_selected: None }
    }
}

fn dir(config_dir: &Path) -> PathBuf {
    config_dir.join("roots")
}

fn path(config_dir: &Path, root: &Path) -> PathBuf {
    dir(config_dir).join(format!("{}.json", hash_root(root)))
}

/// Pins are truth, not a cache (§9.5 rule 5) — but a corrupt file still has no
/// other source to recover from, so this falls back to empty exactly like
/// `settings.rs` falls back to defaults. The failure mode that rule 5 actually
/// guards against is `cache.json` deletion, not this file's own corruption.
pub fn load(config_dir: &Path, root: &Path) -> RootSettings {
    atomicfile::prune_stale_temps(&dir(config_dir));

    let Ok(raw) = fs::read_to_string(path(config_dir, root)) else {
        return RootSettings::default();
    };

    match serde_json::from_str::<RootSettings>(&raw) {
        Ok(settings) if settings.version == ROOTS_VERSION => settings,
        _ => RootSettings::default(),
    }
}

/// Atomic write (§9.5 rule 1) — see [`atomicfile`]. Pins can be toggled faster
/// than a save completes, so two of these overlapping is ordinary.
pub fn save(config_dir: &Path, root: &Path, settings: &RootSettings) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;

    atomicfile::write(&path(config_dir, root), &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(name);
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_file_yields_no_pins() {
        let dir = TempDir::new("corgit-test-roots-missing");
        let loaded = load(&dir.0, Path::new(r"C:\dev\code"));
        assert!(loaded.pins.is_empty());
        assert_eq!(loaded.last_selected, None);
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = TempDir::new("corgit-test-roots-corrupt");
        let root = Path::new(r"C:\dev\code");
        fs::create_dir_all(dir.0.join("roots")).unwrap();
        fs::write(path(&dir.0, root), b"{not json").unwrap();

        let loaded = load(&dir.0, root);
        assert!(loaded.pins.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new("corgit-test-roots-roundtrip");
        let root = Path::new(r"C:\dev\code");

        let mut settings = RootSettings::default();
        settings.pins.insert("repo-1".to_string());
        settings.last_selected = Some("repo-1".to_string());
        save(&dir.0, root, &settings).unwrap();

        let loaded = load(&dir.0, root);
        assert!(loaded.pins.contains("repo-1"));
        assert_eq!(loaded.last_selected.as_deref(), Some("repo-1"));
    }

    #[test]
    fn different_roots_hash_to_different_files() {
        let dir = TempDir::new("corgit-test-roots-hash");
        assert_ne!(
            path(&dir.0, Path::new(r"C:\dev\a")),
            path(&dir.0, Path::new(r"C:\dev\b")),
        );
    }

    #[test]
    fn unknown_version_yields_defaults() {
        let dir = TempDir::new("corgit-test-roots-version");
        let root = Path::new(r"C:\dev\code");
        fs::create_dir_all(dir.0.join("roots")).unwrap();
        fs::write(path(&dir.0, root), br#"{"version":99,"pins":["x"]}"#).unwrap();

        assert!(load(&dir.0, root).pins.is_empty());
    }
}
