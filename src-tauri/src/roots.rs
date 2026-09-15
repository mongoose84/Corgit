//! Per-root settings (SPEC.md §9.5): pins and the last-selected repo.
//!
//! Deliberately separate from `cache.rs` even though both are per-root files
//! keyed the same way (`cache::hash_root`) — the cache is disposable (rule 3:
//! any parse failure deletes and rebuilds silently), but pins are the user's
//! own choices with no other source of truth, so deleting the cache must
//! never lose them (rule 5). That is the whole reason there are two files
//! instead of one.
//!
//! Window size and position are deliberately *not* here. They were in §9.5's
//! table while multi-window was still a v1 goal, on the reasoning that geometry
//! belongs to the window and a window belongs to a root. With one window (§9.2)
//! that no longer holds — geometry would be global, not per-root — so it was
//! dropped from the spec rather than stored in the wrong file.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomicfile;
use crate::cache::{hash_root, legacy_hash_root};

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

/// Where this root's pins lived before `hash_root` became FNV-1a.
fn legacy_path(config_dir: &Path, root: &Path) -> PathBuf {
    dir(config_dir).join(format!("{}.json", legacy_hash_root(root)))
}

/// Move a pins file written under the old `DefaultHasher` key to the current
/// one, returning its contents so the caller can carry on as if it had always
/// been there.
///
/// Rule 5 says pins are truth, and the whole argument for making `hash_root`
/// stable was that a changed key silently orphans them. Changing the key
/// *without* this would have done precisely that, once, to every existing
/// install — a fix that ships the bug it fixes. This is the cost of having had
/// an unstable key at all, and it is paid here rather than by the user.
///
/// Copy-then-remove, not `fs::rename`: the two sit in one directory so rename
/// would normally do, but a failed remove must not be able to lose the file —
/// leaving both copies is a harmless orphan, whereas a half-done rename is the
/// loss. The remove is best-effort for the same reason; a stale legacy file
/// is never read again, because the current key now resolves first.
///
/// `atomicfile::write` keeps §9.5 rule 1 across the migration itself: a crash
/// mid-adoption leaves the legacy file untouched and the new one absent, so
/// the next launch simply migrates again.
fn adopt_legacy(config_dir: &Path, root: &Path) -> Option<String> {
    let legacy = legacy_path(config_dir, root);
    let target = path(config_dir, root);
    // Belt and braces: if the two keys ever agreed, adopting would be a
    // self-overwrite that then deletes the file it just wrote.
    if legacy == target {
        return None;
    }

    let raw = fs::read_to_string(&legacy).ok()?;
    atomicfile::write(&target, raw.as_bytes()).ok()?;
    let _ = fs::remove_file(&legacy);

    Some(raw)
}

/// Pins are truth, not a cache (§9.5 rule 5) — but a corrupt file still has no
/// other source to recover from, so this falls back to empty exactly like
/// `settings.rs` falls back to defaults. The failure mode that rule 5 actually
/// guards against is `cache.json` deletion, not this file's own corruption.
pub fn load(config_dir: &Path, root: &Path) -> RootSettings {
    atomicfile::prune_stale_temps(&dir(config_dir));

    // A miss here is either a root never opened before or one whose pins are
    // still under the pre-FNV key; `adopt_legacy` tells the two apart, and
    // only on the miss, so the common path stays one read.
    let raw = match fs::read_to_string(path(config_dir, root)) {
        Ok(raw) => raw,
        Err(_) => match adopt_legacy(config_dir, root) {
            Some(raw) => raw,
            None => return RootSettings::default(),
        },
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

    /// The migration that stops the `hash_root` fix from wiping the pins it
    /// exists to protect. Writes a pins file under the old `DefaultHasher` key
    /// exactly as an installed build would have, then asserts a plain `load`
    /// returns them.
    #[test]
    fn pins_written_under_the_legacy_key_are_adopted() {
        let dir = TempDir::new("corgit-test-roots-legacy");
        let root = Path::new(r"C:\dev\code");
        fs::create_dir_all(dir.0.join("roots")).unwrap();

        let mut settings = RootSettings::default();
        settings.pins.insert("repo-1".to_string());
        settings.last_selected = Some("repo-1".to_string());
        fs::write(legacy_path(&dir.0, root), serde_json::to_vec(&settings).unwrap()).unwrap();

        let loaded = load(&dir.0, root);

        assert!(loaded.pins.contains("repo-1"), "pins lost across the key change");
        assert_eq!(loaded.last_selected.as_deref(), Some("repo-1"));
        // Adopted, not merely read: the next load must hit the current key
        // directly, and the old file must not linger to be re-adopted over a
        // newer one.
        assert!(path(&dir.0, root).exists(), "not rewritten under the current key");
        assert!(!legacy_path(&dir.0, root).exists(), "legacy file left behind");
    }

    /// A root whose pins already live under the current key must not have them
    /// replaced by a stale legacy file — the adoption is a miss-path only.
    #[test]
    fn a_current_file_wins_over_a_legacy_one() {
        let dir = TempDir::new("corgit-test-roots-legacy-precedence");
        let root = Path::new(r"C:\dev\code");
        fs::create_dir_all(dir.0.join("roots")).unwrap();

        let mut current = RootSettings::default();
        current.pins.insert("current".to_string());
        save(&dir.0, root, &current).unwrap();

        let mut stale = RootSettings::default();
        stale.pins.insert("stale".to_string());
        fs::write(legacy_path(&dir.0, root), serde_json::to_vec(&stale).unwrap()).unwrap();

        let loaded = load(&dir.0, root);

        assert!(loaded.pins.contains("current"));
        assert!(!loaded.pins.contains("stale"), "a stale legacy file overwrote live pins");
    }

    /// Nothing to adopt is the ordinary first-open case, not an error.
    #[test]
    fn no_legacy_file_still_yields_defaults() {
        let dir = TempDir::new("corgit-test-roots-legacy-absent");
        fs::create_dir_all(dir.0.join("roots")).unwrap();

        assert!(load(&dir.0, Path::new(r"C:\dev\code")).pins.is_empty());
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
