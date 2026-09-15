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
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomicfile;
use crate::status::RepoStatus;

/// Bumped to 2 when `RepoStatus` gained `changed_files`. `RepoStatus` is
/// `#[serde(default)]`, so a v1 file would have loaded happily — and painted a
/// dirty repo's badge as "0 files" until the first sweep corrected it. Cheaper
/// to throw the old file away: the cost is one cold sweep, once, which is the
/// state every first-ever open of a root starts from anyway. A new count field
/// is exactly what this constant is for.
pub const CACHE_VERSION: u32 = 2;

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
///
/// FNV-1a, in-house, and pinned by a known-answer test so CI holds the value
/// rather than the compiler. This was `DefaultHasher`, which std is explicit
/// must not be relied on: the algorithm "is not specified, and so it and its
/// hashes should not be relied upon over releases" — and it has already
/// changed once (SipHash-2-4 -> 1-3). That is survivable for `cache/<hash>.json`,
/// where a re-key costs one cold sweep. It is not survivable for
/// `roots/<hash>.json`, which holds pins and last-selected: a toolchain bump
/// would orphan every pin the user has ever set, in every root, at once and
/// with nothing to connect the loss to. §9.5 rule 5 spends a whole second file
/// preventing exactly that, so the key cannot be the compiler's to change.
///
/// Six lines and no dependency — the requirements here are stable output and
/// a spread across sibling paths, not collision resistance against an
/// adversary. A cache file is not a security boundary.
pub fn hash_root(root: &Path) -> String {
    // Lower-cased so the same folder reached with different casing — which
    // Windows treats as identical — still hits one file.
    let key = root.to_string_lossy().to_lowercase();

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The pre-FNV key: `DefaultHasher` over the same lower-cased path.
///
/// Kept for one reason — [`roots::load`](crate::roots::load) adopts a pins
/// file written under it, once, so that fixing [`hash_root`] does not itself
/// inflict the loss [`hash_root`] exists to prevent. Every installed build
/// wrote its pins under this key; changing the key without a migration would
/// orphan all of them on upgrade, which is the bug, not a fix for it.
///
/// This reaches files written by a build whose `DefaultHasher` matches the one
/// compiling *this* line, which is every build Corgit has shipped. It cannot
/// reach a file written by a differently-compiled build — and the fact that
/// such a file is already unreachable is the finding.
///
/// Deletable once no installed build predates the FNV key. Nothing else may
/// call it: it is the old key, not a fallback.
pub fn legacy_hash_root(root: &Path) -> String {
    let mut hasher = DefaultHasher::new();
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
    atomicfile::prune_stale_temps(cache_dir);

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

/// Overlapping saves for one root are normal here — a status sweep, a fetch
/// sweep and a single-repo write each hand over the whole snapshot from their
/// own thread, and only sweep-vs-sweep is guarded — which is exactly what
/// [`atomicfile::write`] is built to survive (§9.5 rule 1).
pub fn save(cache_dir: &Path, root: &Path, cache: &RootCache) -> std::io::Result<()> {
    let json = serde_json::to_vec(cache)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;

    atomicfile::write(&path(cache_dir, root), &json)
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
    fn concurrent_saves_of_one_root_all_succeed() {
        let dir = TempDir::new("corgit-test-cache-concurrent");
        let root = Path::new(r"C:\dev\code");

        let errors: Vec<String> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|n| {
                    let dir = dir.0.clone();
                    scope.spawn(move || {
                        let mut errors = Vec::new();
                        for _ in 0..40 {
                            let mut cache = RootCache { version: CACHE_VERSION, ..Default::default() };
                            cache
                                .statuses
                                .insert(format!("repo-{n}"), RepoStatus { ahead: n, ..Default::default() });
                            if let Err(err) = save(&dir, root, &cache) {
                                errors.push(err.to_string());
                            }
                        }
                        errors
                    })
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
        });

        assert!(errors.is_empty(), "{} save(s) failed: {:?}", errors.len(), errors);
        assert_eq!(temp_files(&dir.0), 0, "temp files left behind");
        assert_eq!(load(&dir.0, root).version, CACHE_VERSION, "cache unreadable after the storm");
    }

    fn temp_files(dir: &Path) -> usize {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "tmp"))
            .count()
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

    /// The point of the whole exercise: these two strings are the key, and a
    /// toolchain bump must not move them. If this test ever fails, every
    /// installed user's pins just became unreachable — the fix is to restore
    /// the constant, not to update the expectation.
    #[test]
    fn hash_root_is_pinned_to_known_answers() {
        assert_eq!(hash_root(Path::new(r"C:\dev\code")), "1e1d7bda2697f2ec");
        assert_eq!(hash_root(Path::new(r"C:\dev\a")), "807c970460408824");
    }

    /// Sibling paths differing in one byte must not land in one file. FNV-1a
    /// avalanches poorly in its low bits, and these roots differ only there —
    /// the reason to check rather than assume.
    #[test]
    fn sibling_roots_do_not_collide() {
        assert_ne!(hash_root(Path::new(r"C:\dev\a")), hash_root(Path::new(r"C:\dev\b")));
    }

    /// The migration in `roots::load` is only worth its lines if the two keys
    /// actually differ. If a toolchain ever made them agree this would pass
    /// vacuously, so it asserts the difference directly.
    #[test]
    fn the_legacy_key_is_not_the_current_key() {
        let root = Path::new(r"C:\dev\code");
        assert_ne!(hash_root(root), legacy_hash_root(root));
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
