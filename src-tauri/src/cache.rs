//! Per-root status cache (SPEC.md §6, §9.5).
//!
//! What makes cold start paint instantly: `open_root` seeds the new root's
//! statuses from here before the sweep has run at all, so rows arrive filled
//! in from disk and the sweep only corrects them (§1 — first paint never
//! waits on git). Written after every sweep that lands, so a later launch
//! never starts from a blank slate.
//!
//! Keyed by a hash of the canonicalised root path, one file per root — a
//! single shared `cache.json` would have two windows on different roots
//! overwrite each other's state (§9.2, §9.5).

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::status::RepoStatus;

pub const CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RootCache {
    pub version: u32,
    /// Keyed by repo id, same as the live `RootState.statuses` it seeds.
    pub statuses: HashMap<String, RepoStatus>,
    /// Unix seconds of each repo's last fetch attempt (§6, §9.5), keyed by
    /// repo id. Lets the fetch sweep skip a repo fetched within the last
    /// interval even across a restart, rather than re-fetching everything
    /// the moment the app launches.
    pub last_fetch_at: HashMap<String, i64>,
}

/// Shared with `roots.rs` so a root's cache file and its pins file are keyed
/// identically — both are per-root, both hashed the same way.
pub fn hash_root(root: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    // Lower-cased so the same folder reached with different casing — which
    // Windows treats as identical — still hits one file.
    root.to_string_lossy().to_lowercase().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn path(cache_dir: &Path, root: &Path) -> PathBuf {
    cache_dir.join(format!("{}.json", hash_root(root)))
}

/// A cache, never truth (§9.5 rule 3): any parse failure deletes the file and
/// rebuilds silently. The caller gets an empty cache either way, exactly what
/// a first-ever open of this root would produce.
pub fn load(cache_dir: &Path, root: &Path) -> RootCache {
    let target = path(cache_dir, root);
    let Ok(raw) = fs::read_to_string(&target) else {
        return RootCache::default();
    };

    match serde_json::from_str::<RootCache>(&raw) {
        Ok(cache) if cache.version == CACHE_VERSION => cache,
        _ => {
            let _ = fs::remove_file(&target);
            RootCache::default()
        }
    }
}

/// Atomic write: temp file + rename, so a crash mid-write cannot leave a
/// truncated cache behind (§9.5 rule 1).
pub fn save(cache_dir: &Path, root: &Path, cache: &RootCache) -> std::io::Result<()> {
    fs::create_dir_all(cache_dir)?;

    let target = path(cache_dir, root);
    let temp = target.with_extension("json.tmp");

    let json = serde_json::to_vec(cache)
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
    fn missing_file_yields_an_empty_cache() {
        let dir = TempDir::new("corgit-test-cache-missing");
        let cache = load(&dir.0, Path::new(r"C:\dev\code"));
        assert!(cache.statuses.is_empty());
    }

    #[test]
    fn corrupt_file_yields_an_empty_cache_and_is_removed() {
        let dir = TempDir::new("corgit-test-cache-corrupt");
        let root = Path::new(r"C:\dev\code");
        fs::create_dir_all(&dir.0).unwrap();
        fs::write(path(&dir.0, root), b"{not json").unwrap();

        let cache = load(&dir.0, root);

        assert!(cache.statuses.is_empty());
        assert!(!path(&dir.0, root).exists());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new("corgit-test-cache-roundtrip");
        let root = Path::new(r"C:\dev\code");

        let mut cache = RootCache { version: CACHE_VERSION, statuses: HashMap::new(), last_fetch_at: HashMap::new() };
        cache.statuses.insert(
            "repo-1".to_string(),
            RepoStatus { branch: Some("main".to_string()), ahead: 2, ..Default::default() },
        );
        save(&dir.0, root, &cache).unwrap();

        let loaded = load(&dir.0, root);

        assert_eq!(loaded.statuses.len(), 1);
        assert_eq!(loaded.statuses["repo-1"].branch.as_deref(), Some("main"));
        assert_eq!(loaded.statuses["repo-1"].ahead, 2);
    }

    #[test]
    fn different_roots_hash_to_different_files() {
        let dir = TempDir::new("corgit-test-cache-hash");
        let a = path(&dir.0, Path::new(r"C:\dev\a"));
        let b = path(&dir.0, Path::new(r"C:\dev\b"));
        assert_ne!(a, b);
    }

    #[test]
    fn same_root_different_case_hashes_the_same() {
        let dir = TempDir::new("corgit-test-cache-case");
        let a = path(&dir.0, Path::new(r"C:\dev\Code"));
        let b = path(&dir.0, Path::new(r"C:\DEV\code"));
        assert_eq!(a, b);
    }

    #[test]
    fn unknown_version_yields_an_empty_cache() {
        let dir = TempDir::new("corgit-test-cache-version");
        let root = Path::new(r"C:\dev\code");
        fs::create_dir_all(&dir.0).unwrap();
        fs::write(path(&dir.0, root), br#"{"version":99,"statuses":{}}"#).unwrap();

        assert!(load(&dir.0, root).statuses.is_empty());
    }
}
