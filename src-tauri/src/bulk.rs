//! Multi-repo runs (SPEC.md §5.1's *Fetch all*, *Pull all*, *Branch…* and
//! *Switch & pull*; §4.1's Repository ▸ root group).
//!
//! Split out of `lib.rs` rather than living beside the single-repo commands
//! because a run owns state none of them do — the cancellation flag, its own
//! concurrency cap, and a progress event with a lifetime of its own — and
//! because the one rule that makes a run safe is easiest to keep when it is
//! the only rule in the file: **every repo goes through `write_and_refresh`
//! exactly as a single-repo command does**. The per-repo write queue (§7 rule
//! 1), the Problems record and the status republish are all things a run
//! needs, and the moment this hand-rolls that sequence is the moment the two
//! drift apart.
//!
//! What is deliberately *not* here: `repo_branches`, which reads branch names
//! for the same multi-repo dialogs. It takes no write lock, spawns no run and
//! has no cap of its own — it is a fan-out read that happens to serve the same
//! screens, and filing it here would make "bulk" mean two things.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::discovery::Repo;
use crate::fetchsweep::{self, record_fetch_attempt};
use crate::sweep::{trigger_sweep, Scope};
use crate::{
    branch, now_unix, open_repos, remote, repos_by_id, status,
    write_and_refresh, AppState,
};

/// How many of the global eight (§7 rule 3) a bulk run may hold at once.
///
/// One click here queues dozens of writes, and left uncapped they take every
/// slot: the status sweep, the graph read behind the next click, and the repo
/// the user gives up and selects instead all end up queued behind a run whose
/// end they cannot see. Keeping half the budget free is what leaves the window
/// answering while the herd comes down (§7 rule 4).
///
/// Deliberately the same 4 as `fetchsweep::FETCH_CONCURRENCY` rather than a
/// second number to defend: the fetch sweep arrived at it by this exact
/// argument, and a pull is that work with a merge on the end, holding its
/// slot longer.
const BULK_CONCURRENCY: usize = 4;

const BULK_PROGRESS_EVENT: &str = "bulk:progress";

/// The bulk runs (§5.1's *Fetch all*, *Pull all* and *Branch…*, §4.1's
/// Repository ▸ root group). An enum rather than a closure passed into the
/// runner: they differ only in which git call they make, and the interactivity
/// decision below is the one thing about them worth reading in a single place.
///
/// `Clone` rather than `Copy` since `Branch` carries a name. It is an
/// `Arc<str>` so the per-task clones share one allocation — the whole point of
/// the run is that a single name reaches every repo, and copying it per repo
/// would be the one place this file pretended otherwise.
#[derive(Clone)]
enum BulkOp {
    Fetch,
    Pull,
    Branch { name: Arc<str>, checkout: bool },
    /// §5.1's *Switch & pull* — one existing branch, checked out in every repo
    /// the dialog listed, and pulled unless the user unticked it.
    SwitchPull { name: Arc<str>, pull: bool },
}

impl BulkOp {
    /// The user's word for the operation, shared with `write_and_refresh` so
    /// the busy row, the Problems record and the failure banner all name the
    /// same act (§13).
    fn label(&self) -> &'static str {
        match self {
            Self::Fetch => "Fetch",
            Self::Pull => "Pull",
            Self::Branch { .. } => "Branch",
            // Two labels for one variant, because the checkbox changes what the
            // run actually does and this word is what the busy row, the
            // Problems record and the failure banner all print (§13). A repo
            // that failed to switch, in a run that was never going to pull,
            // must not be recorded as having failed to "Switch & pull".
            Self::SwitchPull { pull, .. } => {
                if *pull {
                    "Switch & pull"
                } else {
                    "Switch"
                }
            }
        }
    }

    async fn run(self, path: &Path) -> Result<(), String> {
        match self {
            // The one place §8.7's "the user is sitting right there, so let it
            // prompt" stops holding. It is true of a fetch on one repo and
            // false of a fetch on seventy-seven: a queue of credential dialogs
            // is not a fetch, it is a hang with extra steps, and the user
            // cannot tell which repo each one is even for. So a bulk fetch is
            // the non-interactive one, and a repo whose auth fails takes the ⚿
            // badge that already means exactly this — with the row's own
            // *Fetch now* as the place a prompt is welcome (§13).
            Self::Fetch => remote::fetch_background(path).await,
            // Pull stays interactive. It only ever runs on repos we believe
            // are behind, and believing that means a fetch reached their
            // remote recently — so a prompt here is both rare and worth
            // answering, unlike the fetch case above.
            Self::Pull => remote::pull(path).await,
            // `HEAD`, never the branch name the dialog printed beside the row.
            // That name came from a status read that can be a whole sweep old
            // (§5.1), and the repo may have been switched in a terminal since
            // — so the start point is resolved by git, in the repo, at the
            // moment the branch is cut. Same rule, and the same reason, as
            // `remote::publish` pushing `HEAD` rather than a cached name.
            //
            // No network, so this is `git::write` like the single-repo create:
            // it writes to `.git`, which makes it a write (§7 rule 1), but it
            // needs no credential helper.
            Self::Branch { name, checkout } => branch::create(path, &name, "HEAD", checkout).await,
            // Two git commands, one operation — and deliberately not two
            // queued writes. `write_and_refresh` holds this repo's write lock
            // around the whole closure (§7 rule 1), so nothing can land between
            // the checkout and the pull: another window's fetch, the row's own
            // Pull, or a second bulk run would each be a merge starting from a
            // tree that is no longer the one that was just checked out.
            //
            // The pull is skipped rather than the switch on `!pull`, because
            // the checkbox is *Pull after switching* — the switch is the part
            // the user always asked for.
            Self::SwitchPull { name, pull } => {
                branch::switch_to(path, &name).await?;
                if pull {
                    // Interactive, like every other pull that is not the fetch
                    // sweep: this ran because the user pressed a button and is
                    // watching the strip count, so a credential prompt is
                    // worth answering (§8.7). The cap of four keeps it to four
                    // prompts at worst rather than sixty.
                    remote::pull(path).await?;
                }
                Ok(())
            }
        }
    }
}

/// Progress for the repo-list strip's "Pulling… 4 of 7" (§5.1). Sent on every
/// completion rather than batched: the count is the only thing on screen that
/// moves during a run, and a run is seconds long per repo.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BulkProgress {
    operation: String,
    /// Repos that have landed, success or failure. The solid half of the
    /// strip's bar.
    done: u32,
    /// Repos with a git process running right now — never more than
    /// [`BULK_CONCURRENCY`]. Drawn as a dimmer segment ahead of `done`, which
    /// is what stops the bar sitting at zero for the first few seconds while
    /// four pulls work invisibly: it shows work happening without claiming
    /// work is finished. The gap between the two segments is the concurrency
    /// cap, made visible for free.
    running: u32,
    total: u32,
}

/// What the run is handed back with. §13's error model names one repo at a
/// time, and a bulk run cannot — so the failures come back as a list and the
/// frontend raises **one** notice for the run, with each repo still carrying
/// its own `!` badge from `write_and_refresh`'s Problems record.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkOutcome {
    operation: String,
    total: u32,
    succeeded: u32,
    /// Repos *Stop* got to before they started. Reported separately from
    /// failures because they are not one: nothing was attempted, nothing
    /// changed, and the repo still carries whatever badge it had.
    skipped: u32,
    failed: Vec<BulkFailure>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkFailure {
    repo_id: String,
    /// Carried rather than looked up by the frontend: the banner names these
    /// repos and writes them into the filter box (§5.1), and by the time it
    /// renders, a rescan may have removed the row it would have read.
    name: String,
    message: String,
}

/// Runs `op` across `targets`, at most [`BULK_CONCURRENCY`] at a time.
///
/// Every repo goes through `write_and_refresh` exactly as a single-repo
/// command does — the per-repo write queue (§7 rule 1), the Problems record
/// and the status republish are all things a bulk run needs, and the moment
/// this hand-rolls that sequence is the moment the two drift apart.
async fn run_bulk(app: &AppHandle, op: BulkOp, targets: Vec<Repo>) -> BulkOutcome {
    let operation = op.label();
    let total = targets.len() as u32;
    app.state::<AppState>().bulk_cancelled.store(false, Ordering::SeqCst);
    emit_bulk_progress(app, operation, 0, 0, total);

    let semaphore = Arc::new(tokio::sync::Semaphore::new(BULK_CONCURRENCY));
    let done = Arc::new(AtomicU32::new(0));
    // Counted here rather than read back off the semaphore's available
    // permits: a permit is taken slightly before the git process starts and
    // released slightly after it ends, and this number is drawn on screen —
    // it should count repos actually being worked on, not permits in hand.
    let running = Arc::new(AtomicU32::new(0));

    let tasks: Vec<_> = targets
        .into_iter()
        .map(|repo| {
            let app = app.clone();
            let semaphore = semaphore.clone();
            let done = done.clone();
            let running = running.clone();
            let op = op.clone();
            tauri::async_runtime::spawn(async move {
                let Ok(_permit) = semaphore.acquire().await else {
                    return BulkResult::Skipped;
                };

                // *Stop*, and the whole of what it can promise (§5.1). A
                // `git pull` mid-merge cannot be cancelled — killing it is how
                // you get a half-merged tree — so Stop is about the queue, and
                // this is the queue's last honest moment: the permit is held
                // but no process has been spawned.
                if app.state::<AppState>().bulk_cancelled.load(Ordering::SeqCst) {
                    return BulkResult::Skipped;
                }

                // Emitted on the way in as well as on the way out, so the bar
                // moves the moment the run starts rather than after the first
                // repo lands.
                let started = running.fetch_add(1, Ordering::SeqCst) + 1;
                emit_bulk_progress(&app, operation, done.load(Ordering::SeqCst), started, total);

                // Read before `op` is moved into the closure below. Only the
                // fetch bookkeeping needs it, and it is one bit rather than a
                // second clone of the op.
                let is_fetch = matches!(op, BulkOp::Fetch);
                let result =
                    write_and_refresh(&app, repo.id.clone(), operation, |path| async move { op.run(&path).await }).await;

                // Progress counts *completions*, including failures: the strip
                // is telling the user how much of the run is left, not how
                // much of it worked. The banner at the end is where the
                // difference is reported.
                let finished = done.fetch_add(1, Ordering::SeqCst) + 1;
                let still_running = running.fetch_sub(1, Ordering::SeqCst) - 1;
                emit_bulk_progress(&app, operation, finished, still_running, total);

                match result {
                    Ok(()) => {
                        if is_fetch {
                            record_fetch_attempt(&app, &repo.id).await;
                        }
                        BulkResult::Ok
                    }
                    Err(message) => {
                        if is_fetch {
                            record_bulk_fetch_failure(&app, &repo.id, remote::looks_like_auth_failure(&message));
                        }
                        BulkResult::Failed(BulkFailure { repo_id: repo.id, name: repo.name, message })
                    }
                }
            })
        })
        .collect();

    let mut results = Vec::with_capacity(tasks.len());
    for task in tasks {
        // A task that panicked is counted as skipped rather than silently
        // dropped: the three numbers have to add up to `total`, or the banner
        // is arithmetic the user can see is wrong. Kept here rather than in
        // `tally` because a panic arrives as a `JoinError`, which only the
        // await can see.
        results.push(task.await.unwrap_or(BulkResult::Skipped));
    }

    // A fetch moved `refs/remotes/*` for every repo it reached, which is what
    // ahead/behind is read from (§8.2) — and the strip's own count is the
    // first thing that has to be right afterwards.
    if matches!(op, BulkOp::Fetch) {
        fetchsweep::publish_state(app).await;
        trigger_sweep(app, Scope::All);
    }

    tally(operation, total, results)
}

/// Fold one result per repo into the run's outcome.
///
/// Named and pure for the same reason `RootState::merge_sweep_results` is: the
/// rule it carries — **`succeeded + skipped + failed.len() == total`** — is
/// what the banner turns into a sentence, and it is invisible in the loop that
/// produces it. A run that quietly lost a repo reads as a run that had fewer
/// repos in it.
///
/// *Skipped* is counted apart from *failed* because it is not a failure:
/// **Stop** reached those repos before anything was attempted, so nothing
/// changed and each still carries whatever badge it already had (§5.1).
fn tally(operation: &str, total: u32, results: Vec<BulkResult>) -> BulkOutcome {
    let mut failed = Vec::new();
    let mut succeeded = 0;
    let mut skipped = 0;
    for result in results {
        match result {
            BulkResult::Ok => succeeded += 1,
            BulkResult::Skipped => skipped += 1,
            BulkResult::Failed(failure) => failed.push(failure),
        }
    }

    BulkOutcome { operation: operation.to_string(), total, succeeded, skipped, failed }
}

/// One repo's share of a bulk run. Three outcomes rather than a `Result`,
/// because *skipped* is neither half of one: nothing ran, so there is no error
/// to report and no success to count.
enum BulkResult {
    Ok,
    Skipped,
    Failed(BulkFailure),
}

/// §5.1's *Stop*. Stops the run from starting anything further; the repos
/// already in flight finish, because there is no way to abandon a `git pull`
/// part-way through that does not leave a tree someone has to repair by hand.
#[tauri::command]
pub fn stop_bulk(app: AppHandle) {
    app.state::<AppState>().bulk_cancelled.store(true, Ordering::SeqCst);
}

fn emit_bulk_progress(app: &AppHandle, operation: &str, done: u32, running: u32, total: u32) {
    let event = BulkProgress { operation: operation.to_string(), done, running, total };
    if let Err(err) = app.emit(BULK_PROGRESS_EVENT, event) {
        log::warn!("could not publish bulk progress ({err})");
    }
}

/// A bulk fetch's counterpart to [`record_fetch_attempt`] — same bookkeeping,
/// the opposite decision about the badge.
///
/// The per-repo manual fetch clears "auth needed" whatever happens, because
/// the user is watching that one repo and may have just typed a password into
/// it. A bulk fetch never prompts (see [`BulkOp::run`]), so a repo that failed
/// on auth has learned nothing new — clearing its badge there would hide the
/// one thing the run actually discovered about it.
fn record_bulk_fetch_failure(app: &AppHandle, repo_id: &str, auth_failed: bool) {
    let state = app.state::<AppState>();
    let mut current = state.root.lock().expect("root mutex poisoned");
    let Some(root) = current.as_mut() else { return };
    root.last_fetch_at.insert(repo_id.to_string(), now_unix());
    if auth_failed {
        root.auth_needed.insert(repo_id.to_string());
    }
}

/// Every repo in the open root that has a remote worth fetching (§5.1's
/// *Fetch all*). A repo with no remote is dropped here rather than failing
/// inside the run and landing in the banner as a failure it is not.
#[tauri::command]
pub async fn fetch_all(app: AppHandle) -> Result<BulkOutcome, String> {
    let repos = open_repos(&app)?;

    let mut targets = Vec::with_capacity(repos.len());
    for repo in repos {
        if remote::has_remote(&repo.path).await {
            targets.push(repo);
        }
    }

    Ok(run_bulk(&app, BulkOp::Fetch, targets).await)
}

/// The repos the *last* status read said were behind (§5.1's *Pull all*).
///
/// Reading the decision out of cached status is safe in both directions, which
/// is the test CLAUDE.md sets for anything that decides from the cache: a repo
/// that has since caught up gets a `git pull` that is a no-op, and one that has
/// since fallen behind is picked up by the next run. Neither is a wrong answer
/// the user pays for — unlike, say, pushing a cached branch name (§8.7).
///
/// Deliberately blind to the filter box (§5.1): the strip counts the whole
/// root, so this must pull the whole root, or the number the user pressed and
/// the work that happened are two different things.
#[tauri::command]
pub async fn pull_all_behind(app: AppHandle) -> Result<BulkOutcome, String> {
    let targets = {
        let state = app.state::<AppState>();
        let current = state.root.lock().expect("root mutex poisoned");
        let root = current.as_ref().ok_or_else(|| "No folder is open".to_string())?;
        root.repos
            .iter()
            .filter(|repo| root.statuses.get(&repo.id).is_some_and(status::can_pull))
            .cloned()
            .collect::<Vec<_>>()
    };

    Ok(run_bulk(&app, BulkOp::Pull, targets).await)
}

/// §5.1's *Branch…* — one branch name, cut in every repo the dialog listed.
///
/// The dialog has already dropped the repos that cannot take it (the name is
/// already there, or a merge is in progress and checkout is on), and this is
/// deliberately not a second gate on the same question: ids in, branches out.
/// Re-deciding here from cached status would add nothing but a way for the two
/// halves to disagree, and anything that still fails surfaces as git's own
/// error in the run's banner like every other write (§13).
#[tauri::command]
pub async fn branch_all(
    app: AppHandle,
    repo_ids: Vec<String>,
    name: String,
    checkout: bool,
) -> Result<BulkOutcome, String> {
    let targets = repos_by_id(&app, &repo_ids)?;
    if targets.is_empty() {
        return Err("None of those repositories are open any more".to_string());
    }
    Ok(run_bulk(&app, BulkOp::Branch { name: name.into(), checkout }, targets).await)
}

/// §5.1's *Switch & pull* — one existing branch, checked out across the *All*
/// section and then brought up to date.
///
/// Same contract as `branch_all` above and for the same reason: the dialog has
/// already dropped the repos that cannot take it (no branch of that name, or a
/// merge in progress), and re-deciding that here from cached status would add
/// nothing but a way for the two halves to disagree. Ids in, branches switched.
///
/// A dirty tree is deliberately *not* filtered here either, on either side of
/// the boundary. Git switches through uncommitted changes unless they collide
/// with what differs between the branches, and which of the two it is cannot be
/// known without trying — §8.3 forbids force-checkout, so the honest thing is
/// to let git refuse and surface its own stderr in the run's banner (§13).
#[tauri::command]
pub async fn switch_pull_all(
    app: AppHandle,
    repo_ids: Vec<String>,
    name: String,
    pull: bool,
) -> Result<BulkOutcome, String> {
    let targets = repos_by_id(&app, &repo_ids)?;
    if targets.is_empty() {
        return Err("None of those repositories are open any more".to_string());
    }
    Ok(run_bulk(&app, BulkOp::SwitchPull { name: name.into(), pull }, targets).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(repo_id: &str) -> BulkFailure {
        BulkFailure {
            repo_id: repo_id.to_string(),
            name: repo_id.to_string(),
            message: "boom".to_string(),
        }
    }

    /// The arithmetic the banner shows. Every repo dispatched has to come back
    /// as exactly one of the three, or the run reports on fewer repos than it
    /// touched — and the one number the user can check is the one that is
    /// wrong.
    #[test]
    fn every_repo_lands_in_exactly_one_column() {
        let outcome = tally(
            "Pull",
            5,
            vec![
                BulkResult::Ok,
                BulkResult::Failed(failure("api")),
                BulkResult::Skipped,
                BulkResult::Ok,
                BulkResult::Skipped,
            ],
        );

        assert_eq!(outcome.succeeded, 2);
        assert_eq!(outcome.skipped, 2);
        assert_eq!(outcome.failed.len(), 1);
        assert_eq!(
            outcome.succeeded + outcome.skipped + outcome.failed.len() as u32,
            outcome.total,
            "the three columns do not add up to the run",
        );
    }

    /// *Stop* is the case this exists for: it lands on a run where most repos
    /// never started, and a tally that folded those into failures would raise
    /// a banner naming repos that nothing was done to.
    #[test]
    fn a_stopped_run_reports_skips_rather_than_failures() {
        let outcome = tally("Fetch", 4, vec![BulkResult::Ok, BulkResult::Skipped, BulkResult::Skipped, BulkResult::Skipped]);

        assert_eq!(outcome.skipped, 3);
        assert!(outcome.failed.is_empty(), "a repo Stop got to first was reported as a failure");
    }

    /// The failures carry their own name and message rather than an id the
    /// frontend looks up later — by the time the banner renders, a rescan may
    /// have removed the row it would have read (§5.1).
    #[test]
    fn a_failure_carries_what_the_banner_needs_to_name_it() {
        let outcome = tally("Pull", 1, vec![BulkResult::Failed(failure("api"))]);

        let reported = &outcome.failed[0];
        assert_eq!(reported.repo_id, "api");
        assert_eq!(reported.name, "api");
        assert_eq!(reported.message, "boom");
        assert_eq!(outcome.operation, "Pull");
    }

    /// A run with nothing to do is still a run, and still has to add up.
    #[test]
    fn an_empty_run_is_not_a_failure() {
        let outcome = tally("Fetch", 0, Vec::new());

        assert_eq!(outcome.total, 0);
        assert_eq!(outcome.succeeded, 0);
        assert_eq!(outcome.skipped, 0);
        assert!(outcome.failed.is_empty());
    }
}
