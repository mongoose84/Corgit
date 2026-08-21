//! Recent Problems (SPEC.md §13).
//!
//! The user-facing half of the logging pair. `git.rs` writes every non-zero
//! git exit to `corgit.log`, including the ones Corgit expects and handles;
//! this ring holds only the failures that were actually *returned* to someone,
//! which is what "Problems" has to mean if the window is going to be worth
//! opening.
//!
//! It exists because §13 lets the UI throw failure notices away — a dismissed
//! banner, a suppressed rule, a background sweep that failed while nobody was
//! looking. Every one of those is only defensible if the record survives
//! somewhere reachable, and *Help ▸ Open Log Folder* is not reachable enough
//! when the question is "what just went wrong". Dismissal is a promise that
//! this list is keeping.
//!
//! A process-wide static rather than `AppState`, matching `git.rs`'s semaphore
//! and for the same reason: the record has to be whole across every window
//! (§9.2), and a per-window ring would show each window a different history of
//! the same herd.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

/// §13 says ~50. That is roughly a bad morning's worth of failures — long
/// enough that the thing you are hunting is still in it after you notice you
/// need it, short enough to hand to the webview whole on every open rather
/// than paginating a debugging aid.
const CAPACITY: usize = 50;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// Monotonic within the process. The frontend keys its list on this: two
    /// identical failures a second apart are two entries, and a timestamp in
    /// whole seconds cannot tell them apart.
    pub seq: u64,
    /// Unix seconds, formatted frontend-side by `dateFormat.ts` so the fixed
    /// `dd-MM-yyyy HH:mm:ss` holds here too.
    pub at: i64,
    /// `None` for a failure that belongs to no single repo — opening a root,
    /// say. The list shows the repo's name when it has one, because a herd of
    /// 77 makes an unattributed error nearly useless.
    pub repo_id: Option<String>,
    /// What the user asked for, in their words: "Push", "Pull", "Commit".
    /// Not the git argv — that is in the log, next to the stderr this holds.
    pub operation: String,
    /// The raw error, untruncated (§13). This list is where someone goes when
    /// the headline was not enough, so it must not repeat the headline's edit.
    pub message: String,
}

struct Ring {
    entries: VecDeque<Problem>,
    next_seq: u64,
}

fn ring() -> &'static Mutex<Ring> {
    static RING: OnceLock<Mutex<Ring>> = OnceLock::new();
    RING.get_or_init(|| {
        Mutex::new(Ring { entries: VecDeque::with_capacity(CAPACITY), next_seq: 1 })
    })
}

/// Record a failure that was surfaced to the user, newest last.
///
/// Returns the entry so the caller can emit it — the window that triggered the
/// operation already knows about this failure, but the *other* windows do not,
/// and a Problems list that is only correct in one of them is a trap of the
/// kind this module exists to prevent.
pub fn record(repo_id: Option<String>, operation: &str, message: &str) -> Problem {
    let mut ring = ring().lock().expect("problems ring poisoned");

    let seq = ring.next_seq;
    ring.next_seq += 1;

    let problem = Problem {
        seq,
        at: super::now_unix(),
        repo_id,
        operation: operation.to_string(),
        message: message.to_string(),
    };

    if ring.entries.len() == CAPACITY {
        ring.entries.pop_front();
    }
    ring.entries.push_back(problem.clone());
    problem
}

/// Newest first — the order the list is read in, resolved here rather than in
/// the webview so every window agrees without having to.
pub fn recent() -> Vec<Problem> {
    let ring = ring().lock().expect("problems ring poisoned");
    ring.entries.iter().rev().cloned().collect()
}

/// *Clear* in the Problems window. Deliberately does not touch `corgit.log`:
/// clearing a view of the record must never destroy the record, which is the
/// same rule that keeps §13's suppression from hiding a condition.
pub fn clear() {
    ring().lock().expect("problems ring poisoned").entries.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    // Serialised against each other: the ring is process-wide by design, so
    // two `#[test]`s on the same static would race under the default harness.
    #[test]
    fn ring_evicts_oldest_and_reports_newest_first() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|err| err.into_inner());
        clear();

        for n in 0..CAPACITY + 10 {
            record(Some(format!("repo-{n}")), "Push", "rejected");
        }

        let recent = recent();
        assert_eq!(recent.len(), CAPACITY, "the ring must not grow past its cap");
        // Newest first, and the ten oldest are gone rather than the ten newest.
        assert_eq!(recent[0].repo_id.as_deref(), Some("repo-59"));
        assert_eq!(recent[CAPACITY - 1].repo_id.as_deref(), Some("repo-10"));
    }

    #[test]
    fn seq_distinguishes_identical_failures() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|err| err.into_inner());
        clear();

        // Same repo, same operation, same message, same second — the case the
        // frontend's list key has to survive.
        let first = record(Some("a".into()), "Pull", "boom");
        let second = record(Some("a".into()), "Pull", "boom");
        assert_ne!(first.seq, second.seq);
    }

    static TEST_LOCK: Mutex<()> = Mutex::new(());
}
