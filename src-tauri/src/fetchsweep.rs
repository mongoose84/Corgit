//! The background fetch sweep (SPEC.md §6, §8.7).
//!
//! A different mechanism from the status sweep, and §6 is explicit that the
//! two "must not be conflated" — so they are no longer neighbours in one file
//! arguing it in comments. This one runs on a multi-minute jittered interval
//! rather than a tick, caps itself at four processes rather than leaning on
//! the global eight, takes each repo's write lock (a fetch writes `.git`, so
//! it is not a read — §7 rule 1), and must never prompt for credentials:
//! nobody is watching it, so a credential dialog is a hang with nothing on
//! screen to explain it.
//!
//! `record_fetch_attempt` lives here rather than with the single-repo commands
//! for the same reason: `last_fetch_at` and the ⚿ "auth needed" set are this
//! sweep's state, and the manual *Fetch now* is the one other thing allowed to
//! move them. Two writers, one file, one set of rules.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::discovery::Repo;
use crate::sweep;
use crate::writequeue::WriteQueues;
use crate::{now_unix, persist_cache, remote, AppState};

/// Fetch the currently open root, if any — the fetch-sweep counterpart to
/// `trigger_sweep`.
pub(crate) fn trigger_fetch_sweep(app: &AppHandle) {
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    let Some(root) = current.as_ref() else { return };
    let (generation, repos) = (root.generation, root.repos.clone());
    drop(current);
    start_fetch_sweep(app, generation, repos);
}

/// How many `git fetch` processes the fetch sweep runs at once (§6) — on top
/// of, not instead of, the global 8-process cap in `git.rs` (§7.3). Lower
/// than the status sweep's implicit 8, because a fetch is 0.5–2 s of network
/// time rather than a local read, and status reads should not have to queue
/// behind a batch of them.
const FETCH_CONCURRENCY: usize = 4;

pub(crate) const FETCH_SWEEP_EVENT: &str = "fetch:sweep";

/// The background fetch sweep's counterpart to `SweepEvent` — deliberately
/// separate from it (§6: "these are different mechanisms"). Carries no
/// status data of its own; a fetch changes `refs/remotes/*`, and it is the
/// status sweep that turns that into ahead/behind counts.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FetchSweepEvent {
    root: PathBuf,
    last_fetch_at: HashMap<String, i64>,
    auth_needed: HashSet<String>,
    elapsed_ms: u64,
}

fn start_fetch_sweep(app: &AppHandle, generation: u64, repos: Vec<Repo>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { fetch_sweep(app, generation, repos).await });
}

/// Unlike the status sweep, a fetch sweep that finds its root replaced
/// mid-flight simply drops its results rather than restarting for the new
/// root (§6's "must not vanish" guarantee is about currently-displayed rows,
/// which fetch does not drive) — the next periodic tick picks the new root
/// up on schedule, which is soon enough for a background convenience.
async fn fetch_sweep(app: AppHandle, generation: u64, repos: Vec<Repo>) {
    let (write_queues, interval_secs) = {
        let state = app.state::<AppState>();
        if state.fetch_sweeping.swap(true, Ordering::SeqCst) {
            return;
        }
        let interval_secs = state.settings.lock().expect("settings mutex poisoned").fetch_sweep_secs.max(1);
        (state.write_queues.clone(), interval_secs)
    };

    let (known_last_fetch_at, auth_needed) = {
        let state = app.state::<AppState>();
        let current = state.root.lock().expect("root mutex poisoned");
        match current.as_ref() {
            Some(root) if root.generation == generation => {
                (root.last_fetch_at.clone(), root.auth_needed.clone())
            }
            _ => {
                state.fetch_sweeping.store(false, Ordering::SeqCst);
                return;
            }
        }
    };

    let started = Instant::now();
    let now = now_unix();

    // Repos this tick actually has work for: not currently flagged
    // "auth needed" (§8.7, §13 — a manual fetch is what clears that), and not
    // fetched within the last interval already, whether that timestamp came
    // from this session or the cache seeded at open (§9.5).
    let due: Vec<Repo> = repos
        .into_iter()
        .filter(|repo| !auth_needed.contains(&repo.id))
        .filter(|repo| {
            known_last_fetch_at
                .get(&repo.id)
                .map_or(true, |last| now - last >= interval_secs as i64)
        })
        .collect();

    let (attempted, newly_auth_needed) = fetch_many(write_queues, due).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let state = app.state::<AppState>();
    let published = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        match current.as_mut() {
            Some(root) if root.generation == generation => {
                for id in &attempted {
                    root.last_fetch_at.insert(id.clone(), now);
                    root.auth_needed.remove(id);
                }
                for id in &newly_auth_needed {
                    root.last_fetch_at.insert(id.clone(), now);
                    root.auth_needed.insert(id.clone());
                }
                Some((
                    root.path.clone(),
                    root.statuses.clone(),
                    root.last_fetch_at.clone(),
                    root.auth_needed.clone(),
                ))
            }
            _ => None,
        }
    };

    if let Some((root_path, statuses, last_fetch_at, auth_needed)) = published {
        persist_cache(&app, &root_path, statuses, last_fetch_at.clone());

        let event = FetchSweepEvent { root: root_path, last_fetch_at, auth_needed, elapsed_ms };
        if let Err(err) = app.emit(FETCH_SWEEP_EVENT, event) {
            log::warn!("could not publish fetch sweep results ({err})");
        }

        // A fetch just moved refs/remotes/*, which is what the status
        // sweep's ahead/behind reads (§8.2) — trigger one now rather than
        // leaving badges stale until the next reconciliation pass (§6).
        //
        // `sweep::Scope::All` and not `Unwatched`: the watchers do cover this, since
        // a fetch writes `.git/refs/remotes` inside every tree we watch, but
        // it writes them for as many repos as the sweep just fetched at once,
        // and each of those refreshes is subject to the per-repo throttle in
        // `watch.rs`. One pass over the repos we know just changed is both
        // cheaper and less racy than waiting for up to that many debounces.
        sweep::trigger_sweep(&app, sweep::Scope::All);
    }

    state.fetch_sweeping.store(false, Ordering::SeqCst);
}

/// Dispatches up to [`FETCH_CONCURRENCY`] fetches at once. Returns the ids
/// that were actually attempted (success or a non-auth failure — either way
/// `last_fetch_at` should move forward so a repo that is, say, offline is not
/// retried every tick) separately from the ids whose failure looked like an
/// auth problem, which the caller marks "auth needed" instead (§8.7, §13).
/// A repo with no remote, or whose write lock is currently held by something
/// else, is skipped silently — absent from both lists, so its `last_fetch_at`
/// is untouched and it is reconsidered next tick.
async fn fetch_many(write_queues: Arc<WriteQueues>, repos: Vec<Repo>) -> (Vec<String>, Vec<String>) {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(FETCH_CONCURRENCY));

    let tasks: Vec<_> = repos
        .into_iter()
        .map(|repo| {
            let write_queues = write_queues.clone();
            let semaphore = semaphore.clone();
            tauri::async_runtime::spawn(async move {
                if !remote::has_remote(&repo.path).await {
                    return (repo.id, None);
                }

                let Ok(_permit) = semaphore.acquire().await else {
                    return (repo.id, None);
                };
                // Skip rather than block (§6, §7 rule 2): a repo mid-write
                // this round just waits for the next tick instead of holding
                // up the other three fetch slots.
                let Some(_write_guard) = write_queues.try_write(&repo.id) else {
                    return (repo.id, None);
                };

                (repo.id, Some(remote::fetch_background(&repo.path).await))
            })
        })
        .collect();

    let mut attempted = Vec::new();
    let mut auth_needed = Vec::new();

    for task in tasks {
        let Ok((id, result)) = task.await else { continue };
        match result {
            Some(Ok(())) => attempted.push(id),
            Some(Err(err)) if remote::looks_like_auth_failure(&err) => auth_needed.push(id),
            Some(Err(_)) => attempted.push(id),
            None => {}
        }
    }

    (attempted, auth_needed)
}

/// Records this repo's fetch attempt and clears any "auth needed" flag (§8.7,
/// §13) — shared by the manual `fetch_repo` command. Persists immediately,
/// mirroring `emit_repo_status`'s per-write cache save: a manual fetch is a
/// user action, not a batch the fetch sweep already throttles.
pub(crate) fn record_fetch_attempt(app: &AppHandle, repo_id: &str) {
    let state = app.state::<AppState>();
    let published = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let Some(root) = current.as_mut() else { return };
        root.last_fetch_at.insert(repo_id.to_string(), now_unix());
        root.auth_needed.remove(repo_id);
        (root.path.clone(), root.statuses.clone(), root.last_fetch_at.clone())
    };
    let (root_path, statuses, last_fetch_at) = published;
    persist_cache(app, &root_path, statuses, last_fetch_at);
}


/// Publishes `last_fetch_at` and `auth_needed` outside a sweep, and saves the
/// cache once — what a finished bulk fetch (§5.1) owes the rows it moved.
///
///
/// Lives here rather than in `bulk.rs`, which is its only caller, so that
/// `FetchSweepEvent` has exactly one place that builds it. Reusing the event
/// is deliberate: what changed is exactly what it exists to carry, and that
/// event exists to carry, and a second event shape saying the same thing is
/// how the row's ⚿ badge ends up with two ways to be set that disagree.
/// `elapsed_ms` is zero — the run's own timing belongs to the strip, and the
/// repo list's header readout is the *status* sweep's number (§1).
pub(crate) fn publish_state(app: &AppHandle) {
    let state = app.state::<AppState>();
    let published = {
        let current = state.root.lock().expect("root mutex poisoned");
        let Some(root) = current.as_ref() else { return };
        (root.path.clone(), root.statuses.clone(), root.last_fetch_at.clone(), root.auth_needed.clone())
    };
    let (root_path, statuses, last_fetch_at, auth_needed) = published;
    persist_cache(app, &root_path, statuses, last_fetch_at.clone());

    let event = FetchSweepEvent { root: root_path, last_fetch_at, auth_needed, elapsed_ms: 0 };
    if let Err(err) = app.emit(FETCH_SWEEP_EVENT, event) {
        log::warn!("could not publish bulk fetch results ({err})");
    }
}
