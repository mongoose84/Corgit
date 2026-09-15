//! The status sweep (SPEC.md §6) — the reconciliation pass behind the repo
//! list, and the per-repo status publish every other part of the app refreshes
//! through.
//!
//! Split out of `lib.rs` because the sweep owns a mechanism, not just a
//! function: a re-entrancy flag, a generation counter, a patience budget, and
//! a straggler hand-off that keeps the flag held after the event has already
//! gone out. Those four only make sense read together, and interleaved with
//! the command handlers they were four unrelated-looking pieces of `AppState`.
//!
//! The ordering rule that everything here serves: **a sweep never starts while
//! one is in flight** (§6). `AppState::sweeping` is what enforces it, and the
//! reason it is not simply cleared when `sweep` returns is `finish_stragglers`
//! — publishing early moved the *event*, not the end of the sweep.
//!
//! What is deliberately *not* here: the tickers and the focus gating that
//! decide *when* to sweep. They start the fetch sweep too, so they belong to
//! neither mechanism (§6: "these are different mechanisms and must not be
//! conflated") and stay in `lib.rs` as wiring.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::discovery::Repo;
use crate::status::{self, FileChanges, RepoStatus};
use crate::writequeue::WriteQueues;
use crate::{persist_cache, AppState};

/// A full sweep landed; every row in the list is refreshed from it.
pub(crate) const SWEEP_EVENT: &str = "status:sweep";
/// One repo's status arrived on its own — a watcher fired, a write landed, or
/// a straggler finished after the batch had already gone out.
pub(crate) const REPO_STATUS_EVENT: &str = "status:repo";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SweepEvent {
    /// Echoed back so a window can ignore results for a root it no longer shows.
    root: PathBuf,
    statuses: HashMap<String, RepoStatus>,
    errors: HashMap<String, String>,
    /// Measured against the 300 ms budget in §1. It is the number the whole
    /// project is justified by, so it is reported, not guessed at.
    elapsed_ms: u64,
}

/// Emitted after a stage, unstage or commit lands, so the row and middle pane
/// update immediately rather than waiting up to 60 s for the next sweep tick.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoStatusEvent {
    root: PathBuf,
    repo_id: String,
    status: Option<RepoStatus>,
    error: Option<String>,
    /// The pane's rows, carried only when this repo is the selected one —
    /// `None` for every other repo, which is what keeps the §1 memory budget
    /// to counts alone for the other 76.
    ///
    /// This is what lets the frontend stop calling `repo_files` after every
    /// write: that call re-ran the same `git status` this event was already
    /// built from (§8.2). It also closes a staleness gap that had nothing to
    /// do with speed — a stage done in the user's terminal reaches here
    /// through `watch.rs`, and used to refresh the row's badge while leaving
    /// the pane underneath it showing the pre-stage list until the repo was
    /// reselected.
    files: Option<FileChanges>,
}

/// Which repos a sweep covers (§6). Two answers, because the watchers made
/// the question worth asking: most ticks only owe something to the repos
/// nothing is watching.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// Every repo — the reconciliation pass. Repairs whatever the watchers
    /// could have missed: a dropped-event overflow, a change made while the
    /// window was blurred and the watchers were down.
    All,
    /// Only repos with no working watcher, which is usually none of them and
    /// therefore usually free.
    Unwatched,
}

/// Sweep the currently open root, if any — a no-op with no root open. Shared
/// by the focus-gained handler and the periodic ticker, both of which sweep
/// whatever is open rather than a fixed repo list captured at start time.
pub(crate) fn trigger_sweep(app: &AppHandle, scope: Scope) {
    let state = app.state::<AppState>();
    let current = state.root.lock().expect("root mutex poisoned");
    let Some(root) = current.as_ref() else { return };
    let generation = root.generation;
    let repos: Vec<Repo> = match scope {
        Scope::All => root.repos.clone(),
        Scope::Unwatched => root
            .repos
            .iter()
            .filter(|repo| root.unwatched.contains(&repo.id))
            .cloned()
            .collect(),
    };
    drop(current);

    // Not merely wasteful — `sweep` publishes an event and rewrites the cache
    // on every run, so an empty sweep would repaint the list every 60 s to say
    // nothing. The common case with everything watched is exactly this.
    if repos.is_empty() {
        return;
    }

    start_sweep(app, generation, repos);
}

pub(crate) fn start_sweep(app: &AppHandle, generation: u64, repos: Vec<Repo>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { sweep(app, generation, repos).await });
}

async fn sweep(app: AppHandle, generation: u64, repos: Vec<Repo>) {
    let (write_queues, reconcile) = {
        let state = app.state::<AppState>();
        if state.sweeping.swap(true, Ordering::SeqCst) {
            return;
        }

        // Which repo this sweep owes a *full* republish to when it lands, if
        // it is covering that repo at all. Taken here because `collect` below
        // consumes the list, and because the answer is only interesting for
        // the one repo whose paths anybody is looking at.
        let reconcile = {
            let current = state.root.lock().expect("root mutex poisoned");
            current.as_ref().and_then(|root| {
                let id = root.selected.as_ref()?;
                let repo = repos.iter().find(|repo| &repo.id == id)?;
                Some((repo.id.clone(), repo.path.clone()))
            })
        };

        (state.write_queues.clone(), reconcile)
    };

    let started = Instant::now();
    let (statuses, errors, stragglers) = collect(write_queues, repos, SWEEP_PATIENCE).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    // Only the slow ones. A sweep runs every 60 s and normally lands inside
    // §1's 300 ms budget, so logging each would bury the launch it is here to
    // explain; a sweep past this threshold is one that held the 8-process cap
    // (§7.3) long enough for clicking a repo to feel stuck.
    const SLOW_SWEEP_MS: u64 = 2_000;
    if elapsed_ms >= SLOW_SWEEP_MS || !stragglers.is_empty() {
        log::info!(
            "slow sweep: {} ok, {} failed in {} ms, {} still reading",
            statuses.len(),
            errors.len(),
            elapsed_ms,
            stragglers.len(),
        );
    }

    let state = app.state::<AppState>();

    let outcome = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        match current.as_mut() {
            Some(root) if root.generation == generation => {
                root.merge_sweep_results(&statuses, &errors);
                Outcome::Publish(
                    SweepEvent {
                        root: root.path.clone(),
                        statuses: root.statuses.clone(),
                        errors: root.errors.clone(),
                        elapsed_ms,
                    },
                    root.last_fetch_at.clone(),
                )
            }
            // The root was replaced while we were out, so these results
            // describe a folder nobody is looking at any more. The sweep the
            // new root asked for was turned away by the guard above, which
            // makes redoing it our job — otherwise its rows never fill in.
            Some(root) => Outcome::Restart(root.generation, root.repos.clone()),
            None => Outcome::Nothing,
        }
    };

    match outcome {
        Outcome::Publish(event, last_fetch_at) => {
            // Saved after every sweep rather than on a separate debounce
            // timer: sweeps are already throttled to the configured interval
            // (§6), so this already satisfies "not on every status change"
            // (§9.5 rule 4) without a second timer to keep in sync with the
            // first. Errors are deliberately not cached — a repo that failed
            // this round keeps whatever the *previous* successful sweep or
            // cache load left behind, until it succeeds again.
            persist_cache(&app, &event.root, event.statuses.clone(), last_fetch_at);

            if let Err(err) = app.emit(SWEEP_EVENT, event) {
                log::warn!("could not publish sweep results ({err})");
            }

            // Cleared only now: the guard has to cover the cache write and
            // event emit too, not just the git spawns in `collect`, or a
            // sweep triggered in that window races `cache::save` against
            // this one for the same root file (§6 — "a sweep never starts
            // while one is in flight").
            //
            // And not here at all when repos are still reading: publishing
            // early moved the *event*, not the end of the sweep. §6's "a sweep
            // never starts while one is in flight" is what keeps a straggler
            // from being overtaken by the next tick's read of the same repo
            // and publishing the older answer second, so `finish_stragglers`
            // owns the flag from here.
            if stragglers.is_empty() {
                state.sweeping.store(false, Ordering::SeqCst);
            } else {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    finish_stragglers(app, generation, stragglers).await;
                });
            }

            // A sweep publishes counts, and counts are all the *rows* need.
            // The middle pane's file list and the open diff are fed by
            // `status:repo` instead (§5.2, §5.4), so reconciling the rows
            // without reconciling the selected repo in full leaves those two
            // describing the working tree as it was when a watcher last
            // fired. The case that makes it visible is the one where the
            // sweep itself has nothing to show: editing a file that was
            // already modified moves no count, so the row is right, the list
            // is right, and the diff under them is stale.
            //
            // This is what covers the window with no watchers at all — §6
            // drops them on blur, so *every* change made in an editor while
            // Corgit is in the background arrives through the focus-gain
            // sweep and nothing else. It also covers repos that could never
            // be watched (a linked worktree, a network share), which have no
            // other path to a fresh diff than this one.
            //
            // One extra `git status` for one repo, on sweeps that covered it.
            if let Some((repo_id, path)) = reconcile {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    emit_repo_status(&app, &repo_id, &path).await;
                });
            }
        }
        // Both remaining arms describe a root nobody is looking at, so a
        // straggler still reading one of its repos has no one to publish to.
        // Aborted rather than left to finish: `kill_on_drop` (§7.3) turns that
        // into killing the git process, which is the whole point — the results
        // are worthless and the semaphore permits are not.
        Outcome::Restart(generation, repos) => {
            for task in stragglers {
                task.abort();
            }
            // Cleared before restarting, not after: `start_sweep` spawns a
            // fresh sweep that immediately re-swaps the guard to `true`, so
            // it must find it `false` here or the restart silently no-ops.
            state.sweeping.store(false, Ordering::SeqCst);
            start_sweep(&app, generation, repos);
        }
        Outcome::Nothing => {
            for task in stragglers {
                task.abort();
            }
            state.sweeping.store(false, Ordering::SeqCst);
        }
    }
}

/// What a finished sweep does next. Split out so the decision is made while
/// the root lock is held and acted on after it is released.
enum Outcome {
    Publish(SweepEvent, HashMap<String, i64>),
    Restart(u64, Vec<Repo>),
    Nothing,
}

/// Every repo is dispatched at once; the semaphore in `git.rs` is what
/// actually holds concurrency to 8 (§7.3). One batched result rather than 77
/// events keeps the IPC cost off the sweep's measured time. A repo whose
/// write lock is currently held is skipped rather than queried — a
/// non-blocking `try_read`, not a wait, so one busy repo never holds up the
/// other 76 (§6, §7 rule 2) — and is simply absent from both maps; `sweep`
/// merges results in rather than replacing wholesale, so a skipped repo keeps
/// its last known status until the next tick.
pub(crate) async fn collect(
    write_queues: Arc<WriteQueues>,
    repos: Vec<Repo>,
    patience: Duration,
) -> (HashMap<String, RepoStatus>, HashMap<String, String>, Vec<StatusTask>) {
    let tasks: Vec<_> = repos
        .into_iter()
        .map(|repo| {
            let write_queues = write_queues.clone();
            tauri::async_runtime::spawn(async move {
                let result = match write_queues.try_read(&repo.id) {
                    Some(_read_guard) => Some(status::query(&repo.path).await),
                    None => None,
                };
                (repo.id, result)
            })
        })
        .collect();

    let mut statuses = HashMap::new();
    let mut errors = HashMap::new();
    let mut stragglers = Vec::new();

    // One deadline for the batch, not one per repo: what the user is waiting
    // for is the *list*, and the list arrives when the last repo in it does.
    let deadline = tokio::time::Instant::now() + patience;

    for mut task in tasks {
        // `&mut task`, so a repo that runs past the deadline keeps running and
        // keeps its read guard. Taking the handle by value would drop it, and
        // a dropped `JoinHandle` detaches the task — the read would still cost
        // its process and its semaphore permit, and then throw the answer
        // away. `timeout_at` polls the task before it looks at the clock, so a
        // task that finished while we waited on an earlier one is still
        // collected here rather than needlessly deferred.
        match tokio::time::timeout_at(deadline, &mut task).await {
            // A panicked task is a bug in the parser, not a reason to lose the
            // other 76 repos' results.
            Ok(Err(_)) => continue,
            Ok(Ok((id, result))) => match result {
                Some(Ok(status)) => {
                    statuses.insert(id, status);
                }
                Some(Err(err)) => {
                    errors.insert(id, err);
                }
                None => {}
            },
            Err(_) => stragglers.push(task),
        }
    }

    (statuses, errors, stragglers)
}

/// One repo's in-flight status read. `None` in the payload means the repo was
/// skipped because a write held it, which is not an outcome a straggler can
/// have — a skipped repo never reaches the deadline — but the type is the same
/// one `collect` dispatches, so both arms exist.
type StatusTask = tauri::async_runtime::JoinHandle<(String, Option<Result<RepoStatus, String>>)>;

/// How long the batch waits for its slowest repo before publishing without it
/// (§1, §6).
///
/// A full pass over 69 repos costs about 1.2 s at best, so this is not a
/// budget the healthy case is meant to feel — it is the point past which one
/// repo is no longer merely slow. The case it exists for is real and
/// repeatable: on a cold file cache a large repo can take the whole 30 s
/// `READ_TIMEOUT` and then be killed, and holding all 69 rows and the sweeping
/// indicator for that is what made a launch look like a hang. The stragglers
/// publish themselves as they land, through the same per-repo event a watcher
/// uses (§5.2), so nothing is lost — it arrives separately.
pub(crate) const SWEEP_PATIENCE: Duration = Duration::from_secs(3);

/// How many status ticks pass between full reconciliation sweeps (§6).
///
/// Every tick sweeps the repos nothing is watching, which is normally none of
/// them and therefore free; every fifth also sweeps the rest, to repair what a
/// watcher can miss — a dropped-event overflow, a change that landed while the
/// window was blurred. At the default 60 s interval that is a full pass every
/// five minutes.
///
/// The full pass is the expensive one — one process per repo, 1.2 s at best
/// for 69 of them — which is precisely why it is no longer what keeps the rows
/// current. Lowering this number gives back the cost the watchers removed.
pub(crate) const RECONCILE_EVERY: u32 = 5;

/// Publishes the repos that outran [`SWEEP_PATIENCE`], one at a time as they
/// land, and only then lets the next sweep start.
///
/// Per-repo events rather than a second batch: this *is* the "one repo's
/// status arrived on its own" case, which `status:repo` already exists for and
/// which the frontend already merges without disturbing the other 68 rows
/// (§5.2). Carrying the status the read already produced rather than calling
/// `emit_repo_status` matters for the same reason `query_with_files` exists —
/// re-reading would spend another process to learn what is in hand.
async fn finish_stragglers(app: AppHandle, generation: u64, stragglers: Vec<StatusTask>) {
    let state = app.state::<AppState>();
    let mut published = 0usize;

    let mut remaining = stragglers.into_iter();
    while let Some(task) = remaining.next() {
        let Ok((repo_id, Some(status))) = task.await else { continue };

        let root_path = {
            let mut current = state.root.lock().expect("root mutex poisoned");
            // Same guard as the batch: a result whose root was replaced while
            // it was reading must not be written over the new one.
            match current.as_mut() {
                Some(root) if root.generation == generation => {
                    match &status {
                        Ok(fresh) => {
                            root.statuses.insert(repo_id.clone(), fresh.clone());
                            root.errors.remove(&repo_id);
                        }
                        Err(err) => {
                            root.errors.insert(repo_id.clone(), err.clone());
                            root.statuses.remove(&repo_id);
                        }
                    }
                    root.path.clone()
                }
                // The root was replaced under us. Every straggler still out
                // describes it, so they go the same way the `Restart` arm
                // sends them rather than being awaited for nothing.
                _ => {
                    remaining.by_ref().for_each(|task| task.abort());
                    break;
                }
            }
        };

        published += 1;
        let event = RepoStatusEvent {
            root: root_path,
            repo_id,
            status: status.as_ref().ok().cloned(),
            error: status.as_ref().err().cloned(),
            // Counts only, exactly as the batch would have carried. A straggler
            // that happens to be the selected repo gets its file list from the
            // reconciling read the sweep already scheduled (§5.2, §5.4).
            files: None,
        };
        if let Err(err) = app.emit(REPO_STATUS_EVENT, event) {
            log::warn!("could not publish a late repo status ({err})");
        }
    }

    // Once, not per repo: the file is rewritten wholesale (§9.5), and these
    // arrived seconds apart at most.
    if published > 0 {
        let snapshot = {
            let current = state.root.lock().expect("root mutex poisoned");
            current
                .as_ref()
                .filter(|root| root.generation == generation)
                .map(|root| (root.path.clone(), root.statuses.clone(), root.last_fetch_at.clone()))
        };
        if let Some((root_path, statuses, last_fetch_at)) = snapshot {
            persist_cache(&app, &root_path, statuses, last_fetch_at);
        }
    }

    state.sweeping.store(false, Ordering::SeqCst);
}

/// Re-reads one repo's status outside any write lock, updates it in the open
/// root (if that root and repo are still current), saves the cache, and
/// notifies every window watching this root — the single-repo counterpart to
/// what a full sweep does for all of them. `pub(crate)` so `watch.rs`'s
/// debounced FS-watcher callbacks (§6) can call it directly, the same way
/// `write_and_refresh` does after every mutating command.
pub(crate) async fn emit_repo_status(app: &AppHandle, repo_id: &str, path: &Path) {
    let state = app.state::<AppState>();

    // Whether the middle pane is looking at this repo decides how much of the
    // read to keep: paths for the selected repo, counts for everyone else.
    // Asked *before* the git call rather than after, because the answer picks
    // the command — deciding afterwards would mean a second `git status` for
    // the paths, which is precisely the spawn this exists to remove.
    //
    // Racing the user's selection here is harmless in the direction that
    // matters: a selection that moves on mid-read publishes file lists the
    // frontend drops, whereas one that arrives late leaves the pane to
    // `repo_files`, which selection calls anyway.
    let selected = {
        let current = state.root.lock().expect("root mutex poisoned");
        current
            .as_ref()
            .is_some_and(|root| root.selected.as_deref() == Some(repo_id))
    };

    // §7 rule 2, and it matters far more now than when this was reached only
    // by the hot set: every repo is watched, and a commit writes `.git` a
    // dozen times, so an unguarded read here would parse repos mid-mutation
    // routinely rather than rarely. Blocking rather than `try_read` because
    // unlike the sweep this refresh has no next tick to fall back on — it is
    // the only thing that will publish this change.
    //
    // No deadlock with `write_and_refresh`: that releases its write guard
    // before calling here, precisely so this can be taken.
    let _read_guard = state.write_queues.read(repo_id).await;

    let (status, files) = if selected {
        match status::query_with_files(path).await {
            Ok((status, files)) => (Ok(status), Some(files)),
            Err(err) => (Err(err), None),
        }
    } else {
        (status::query(path).await, None)
    };

    let published = {
        let mut current = state.root.lock().expect("root mutex poisoned");
        let Some(root) = current.as_mut() else { return };
        if !root.repos.iter().any(|repo| repo.id == repo_id) {
            return;
        }

        match &status {
            Ok(s) => {
                root.statuses.insert(repo_id.to_string(), s.clone());
                root.errors.remove(repo_id);
            }
            Err(err) => {
                root.errors.insert(repo_id.to_string(), err.clone());
                root.statuses.remove(repo_id);
            }
        }

        (root.path.clone(), root.statuses.clone(), root.last_fetch_at.clone())
    };

    let (root_path, statuses, last_fetch_at) = published;
    persist_cache(app, &root_path, statuses, last_fetch_at);

    let event = RepoStatusEvent {
        root: root_path,
        repo_id: repo_id.to_string(),
        status: status.as_ref().ok().cloned(),
        error: status.as_ref().err().cloned(),
        files,
    };
    if let Err(err) = app.emit(REPO_STATUS_EVENT, event) {
        log::warn!("could not publish repo status ({err})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both halves of what makes publishing early worth having. Raised to meet
    /// `READ_TIMEOUT` the mechanism is dead code — no read can outlive its own
    /// kill — and the 30 s wait it exists to remove comes straight back.
    /// Lowered under a healthy full pass (about 1.2 s over 69 repos, per
    /// `RECONCILE_EVERY`) it fires every time, and the batch the frontend
    /// applies wholesale is routinely split for no reason.
    #[test]
    fn sweep_patience_sits_between_a_healthy_pass_and_a_killed_read() {
        assert!(
            SWEEP_PATIENCE < crate::git::READ_TIMEOUT,
            "a read that cannot outlive the patience can never straggle",
        );
        assert!(
            SWEEP_PATIENCE >= Duration::from_secs(2),
            "a full pass over 69 repos costs ~1.2 s; splitting that one is noise, not news",
        );
    }
}
