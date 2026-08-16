//! Per-repo write queue (SPEC.md §7).
//!
//! One queue per repo, keyed by canonicalised path, living on `AppState` so
//! every caller in the process shares one — the guarantee §7 depends on being
//! process-local, which §9.2's single-instance rule is what makes safe (two
//! processes would each get their own set). Modelled as an `RwLock<()>` rather
//! than a literal queue: every mutating operation needs exclusive access and
//! runs one at a time, but reads (the status sweep, an on-demand file list)
//! only need to know that no write is in flight, and may run concurrently
//! with each other. An `RwLock` gives both for the price of one map.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

#[derive(Default)]
pub struct WriteQueues {
    locks: Mutex<HashMap<String, Arc<RwLock<()>>>>,
}

impl WriteQueues {
    fn get(&self, repo_id: &str) -> Arc<RwLock<()>> {
        let mut locks = self.locks.lock().expect("write-queue mutex poisoned");
        locks
            .entry(repo_id.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(())))
            .clone()
    }

    /// Acquired by every mutating operation (§7 rule 1): stage, unstage,
    /// commit, and — once they exist — fetch, pull, push, checkout. Waits for
    /// any earlier write on this repo to finish first.
    pub async fn write(&self, repo_id: &str) -> OwnedRwLockWriteGuard<()> {
        self.get(repo_id).write_owned().await
    }

    /// Acquired by an on-demand read, e.g. the per-file listing behind the
    /// middle pane. Blocks until any in-flight write on this repo finishes —
    /// never parse a repo mid-mutation (§7 rule 2) — but multiple readers run
    /// concurrently with each other.
    pub async fn read(&self, repo_id: &str) -> OwnedRwLockReadGuard<()> {
        self.get(repo_id).read_owned().await
    }

    /// Acquired by the status sweep. `None` means a write currently holds the
    /// repo; the sweep skips it for this round rather than waiting (§6),
    /// leaving its last known status in place until the next tick.
    pub fn try_read(&self, repo_id: &str) -> Option<OwnedRwLockReadGuard<()>> {
        self.get(repo_id).try_read_owned().ok()
    }

    /// Acquired by the background fetch sweep. `None` means either a write or
    /// an in-flight read (e.g. the middle pane's file list) currently holds
    /// the repo; the sweep skips it for this round rather than blocking the
    /// other repos behind a busy one, mirroring `try_read`'s non-blocking
    /// spirit for the write side.
    pub fn try_write(&self, repo_id: &str) -> Option<OwnedRwLockWriteGuard<()>> {
        self.get(repo_id).try_write_owned().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_held_write_lock_fails_try_read() {
        let queues = WriteQueues::default();
        let _write_guard = queues.write("repo-1").await;
        assert!(queues.try_read("repo-1").is_none());
    }

    #[tokio::test]
    async fn different_repos_do_not_contend() {
        let queues = WriteQueues::default();
        let _a = queues.write("repo-a").await;
        assert!(queues.try_read("repo-b").is_some());
    }

    #[tokio::test]
    async fn releasing_a_write_lock_unblocks_reads() {
        let queues = WriteQueues::default();
        {
            let _write_guard = queues.write("repo-1").await;
            assert!(queues.try_read("repo-1").is_none());
        }
        assert!(queues.try_read("repo-1").is_some());
    }

    #[tokio::test]
    async fn a_held_read_lock_fails_try_write() {
        let queues = WriteQueues::default();
        let _read_guard = queues.read("repo-1").await;
        assert!(queues.try_write("repo-1").is_none());
    }

    #[tokio::test]
    async fn a_held_write_lock_fails_try_write() {
        let queues = WriteQueues::default();
        let _write_guard = queues.write("repo-1").await;
        assert!(queues.try_write("repo-1").is_none());
    }

    #[tokio::test]
    async fn an_idle_repo_permits_try_write() {
        let queues = WriteQueues::default();
        assert!(queues.try_write("repo-1").is_some());
    }
}
