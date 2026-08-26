//! Which repos have a write running right now (SPEC.md §13, *Work in progress*).
//!
//! Deliberately separate from `writequeue.rs`, which it looks like a sibling
//! of and is not: that file exists so two writes never touch one repo at the
//! same time, and its lock is held for correctness. This one exists so the UI
//! can say something is happening, and nothing depends on it being accurate to
//! survive. Folding the count into `WriteQueues` would mean a display concern
//! reaching into the type §7's guarantees rest on.
//!
//! A count rather than a flag, because a second write on a repo waits on the
//! write queue rather than being rejected: `begin, begin, end, end` for two
//! queued writes has to publish one begin and one end, or the first `end`
//! clears an indicator the second write still needs. Only the 0→1 and 1→0
//! transitions are worth an event; the ones in between are noise the frontend
//! would have to de-duplicate itself.

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct InFlightWrites {
    depth: Mutex<HashMap<String, usize>>,
}

impl InFlightWrites {
    /// `true` when this is the first write on the repo — i.e. when the
    /// frontend needs telling.
    pub fn begin(&self, repo_id: &str) -> bool {
        let mut depth = self.depth.lock().expect("in-flight mutex poisoned");
        let count = depth.entry(repo_id.to_string()).or_insert(0);
        *count += 1;
        *count == 1
    }

    /// `true` when the last write on the repo finished. The entry is removed
    /// rather than left at zero — a root can be swapped for another (§9.1) and
    /// a map that only ever grows would keep every repo ever written to.
    pub fn end(&self, repo_id: &str) -> bool {
        let mut depth = self.depth.lock().expect("in-flight mutex poisoned");
        let Some(count) = depth.get_mut(repo_id) else { return false };
        *count -= 1;
        if *count == 0 {
            depth.remove(repo_id);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_write_signals_and_the_last_one_does_too() {
        let writes = InFlightWrites::default();
        assert!(writes.begin("repo-1"));
        assert!(writes.end("repo-1"));
    }

    /// The case the count exists for: a second write queued behind the first
    /// must not produce a second begin, and the first one to finish must not
    /// clear an indicator the other still needs.
    #[test]
    fn a_queued_second_write_publishes_nothing_until_both_are_done() {
        let writes = InFlightWrites::default();
        assert!(writes.begin("repo-1"));
        assert!(!writes.begin("repo-1"), "the row is already showing busy");
        assert!(!writes.end("repo-1"), "one write is still running");
        assert!(writes.end("repo-1"));
    }

    #[test]
    fn repos_are_counted_apart() {
        let writes = InFlightWrites::default();
        assert!(writes.begin("repo-a"));
        assert!(writes.begin("repo-b"));
        assert!(writes.end("repo-a"));
        assert!(writes.end("repo-b"));
    }

    /// Nothing should be able to emit a stray "done" for a repo that was never
    /// busy — the guard's `Drop` is the only caller, but an unbalanced end
    /// must be inert rather than underflowing the count.
    #[test]
    fn an_unbalanced_end_is_inert() {
        let writes = InFlightWrites::default();
        assert!(!writes.end("repo-1"));
        assert!(writes.begin("repo-1"), "and the map is still usable afterwards");
    }
}
