//! The one way this app writes a JSON file to disk (SPEC.md §9.5 rule 1).
//!
//! Every persisted file — global settings, per-root settings, the per-root
//! status cache — is overwritten wholesale, so a crash mid-write must not be
//! able to leave a truncated file where a complete one used to be. Hence:
//! write a sibling temp file, fsync it, rename it over the target. Both paths
//! sit in the same directory, so the rename stays on one volume, and on
//! Windows `fs::rename` replaces an existing destination.
//!
//! The temp name is unique per call, never a fixed `<target>.tmp`. Writers of
//! one file genuinely overlap here — a status sweep, a fetch sweep and a
//! single-repo write each persist the whole snapshot from their own thread —
//! and with a shared temp path one writer renames the file out from under
//! another, which then fails with "the system cannot find the file
//! specified". Each caller writes a complete snapshot, so whoever renames
//! last simply wins; the file on disk is always one writer's whole output.

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// Extension marking a half-written file. Chosen so it never collides with a
/// real `.json` target in the same directory.
const TEMP_EXT: &str = "tmp";

/// Creates `target`'s parent directory if needed, then replaces `target` with
/// `bytes` atomically.
pub fn write(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp = target.with_extension(format!("{}.{}.{TEMP_EXT}", std::process::id(), next_id()));

    // Any failure past this point strands a temp file that no later call will
    // ever reuse, so each call cleans up after itself.
    let written = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();

    if let Err(err) = written.and_then(|()| fs::rename(&temp, target)) {
        let _ = fs::remove_file(&temp);
        return Err(err);
    }

    Ok(())
}

fn next_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Only a killed process can strand a temp file now — `write` cleans up its
/// own failures — which is rare but never self-corrects, so the load paths
/// sweep the directory as they go. The age floor is what keeps this from
/// deleting a temp that another window is writing at this very instant.
pub fn prune_stale_temps(dir: &Path) {
    const MAX_AGE: Duration = Duration::from_secs(60 * 60);

    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension() != Some(OsStr::new(TEMP_EXT)) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .and_then(|modified| SystemTime::now().duration_since(modified).map_err(std::io::Error::other))
            .is_ok_and(|age| age > MAX_AGE);
        if stale {
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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

    fn temp_files(dir: &Path) -> usize {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == TEMP_EXT))
            .count()
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = TempDir::new("corgit-test-atomic-parents");
        let target = dir.0.join("nested").join("file.json");

        write(&target, b"{}").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "{}");
    }

    #[test]
    fn replaces_an_existing_file_and_leaves_no_temp_behind() {
        let dir = TempDir::new("corgit-test-atomic-replace");
        let target = dir.0.join("file.json");

        write(&target, b"first").unwrap();
        write(&target, b"second").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "second");
        assert_eq!(temp_files(&dir.0), 0);
    }

    /// The regression this module exists for: with a shared `<target>.tmp`,
    /// overlapping writers renamed each other's temp away and every loser
    /// returned "the system cannot find the file specified" (os error 2).
    #[test]
    fn concurrent_writers_of_one_file_all_succeed() {
        let dir = TempDir::new("corgit-test-atomic-concurrent");
        let target = dir.0.join("file.json");
        fs::create_dir_all(&dir.0).unwrap();

        let errors: Vec<String> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|n| {
                    let target = target.clone();
                    scope.spawn(move || {
                        let body = format!("{{\"writer\":{n}}}");
                        (0..40)
                            .filter_map(|_| write(&target, body.as_bytes()).err())
                            .map(|err| err.to_string())
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles.into_iter().flat_map(|handle| handle.join().unwrap()).collect()
        });

        assert!(errors.is_empty(), "{} write(s) failed: {:?}", errors.len(), errors);
        assert_eq!(temp_files(&dir.0), 0, "temp files left behind");
        // Whoever renamed last wins, but the file is always one writer's whole output.
        let final_contents = fs::read_to_string(&target).unwrap();
        assert!(final_contents.starts_with("{\"writer\":") && final_contents.ends_with('}'), "{final_contents}");
    }

    #[test]
    fn pruning_removes_stranded_temps_but_spares_fresh_ones() {
        let dir = TempDir::new("corgit-test-atomic-prune");
        fs::create_dir_all(&dir.0).unwrap();

        let stranded = dir.0.join("deadbeef.999.0.tmp");
        let fresh = dir.0.join("deadbeef.999.1.tmp");
        let unrelated = dir.0.join("deadbeef.json");
        for file in [&stranded, &fresh, &unrelated] {
            fs::write(file, b"{}").unwrap();
        }
        // Only mtime decides, so an old temp is simulated rather than waited for.
        let old = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
        fs::File::options().write(true).open(&stranded).unwrap().set_modified(old).unwrap();

        prune_stale_temps(&dir.0);

        assert!(!stranded.exists());
        assert!(fresh.exists());
        assert!(unrelated.exists());
    }
}
