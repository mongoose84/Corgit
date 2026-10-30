//! Branch switching, creation, merging and deletion (SPEC.md §8.3, build step 8).
//!
//! Switching has two shapes only, matching the two ref kinds the graph already
//! badges (`graph::RefKind`): a local branch is a plain `git switch`, a
//! remote-tracking one creates and tracks a local branch of the same name —
//! unless that local branch already exists (the graph, unlike the switcher,
//! shows every ref regardless of whether its local counterpart is also
//! visible), in which case this just switches to it instead.
//!
//! Creation (§8.3, right-click a ref badge in the graph) takes an explicit
//! start point — the badge or commit that was right-clicked — so it never
//! depends on what happens to be checked out.
//!
//! Merging (§8.3, the same menu) is the mirror image: the badge names the
//! *source*, and the destination is always whatever is checked out — Corgit
//! never merges two branches neither of which you are on.
//!
//! Deletion (§8.3, the same menu) is local-only, and safe-first: `-d` always
//! runs before `-D`, so the only way to reach the forceful one is through
//! git's own refusal shown on screen.

use std::path::Path;

use crate::git::{self, END_OF_OPTIONS};

pub async fn switch_local(repo: &Path, branch: &str) -> Result<(), String> {
    let output = git::write(repo, &["switch", END_OF_OPTIONS, branch]).await?;
    if !output.ok {
        return Err(full_message(&output.stderr));
    }
    Ok(())
}

/// `remote_ref` is the short name `for-each-ref` gave the badge, e.g.
/// `origin/feature-x` — `local_name` strips the leading remote, e.g.
/// `feature-x`.
pub async fn switch_remote_tracking(repo: &Path, remote_ref: &str) -> Result<(), String> {
    let local = local_name(remote_ref);

    // `END_OF_OPTIONS` in both commands below for uniformity rather than
    // because a test can show it doing anything: any option-shaped
    // `remote_ref` yields an option-shaped `local`, which git rejects as the
    // `-c` value before it reads the rest — so the create fails on the name,
    // and that failure is not "already exists", so the fallback never runs.
    // Mutation-checked, and both deletions survive for exactly that reason.
    let create = git::write(repo, &["switch", "-c", &local, "--track", END_OF_OPTIONS, remote_ref]).await?;
    if create.ok {
        return Ok(());
    }
    if !create.stderr.contains("already exists") {
        return Err(full_message(&create.stderr));
    }

    // A local branch of that name already exists elsewhere in the graph —
    // just switch to it, the same as double-clicking its own local badge would.
    let fallback = git::write(repo, &["switch", END_OF_OPTIONS, &local]).await?;
    if !fallback.ok {
        return Err(full_message(&fallback.stderr));
    }
    Ok(())
}

/// Switch to `name` wherever it lives — §5.1's *Switch & pull*, run across a
/// whole section at once.
///
/// One command for both of the cases that dialog draws, a local branch and one
/// that exists only on a remote, because *which* it is has to be resolved in
/// the repo at the moment of the switch and never from the answer the dialog
/// printed, which is read from cached status and can be a sweep old (§5.1).
/// `--guess` is git's own resolution of exactly that: a local branch is checked
/// out, and a name matching exactly one remote gets the
/// `-c <name> --track <remote>/<name>` that `switch_remote_tracking` above
/// spells out by hand.
///
/// Passed explicitly rather than left to the default. `checkout.guess = false`
/// is a real setting, and a repo carrying it would fail every remote-only row
/// in a run the dialog had just said would work — the failure mode being one
/// repo in sixty behaving differently from the other fifty-nine for a reason
/// nothing on screen can show.
pub async fn switch_to(repo: &Path, name: &str) -> Result<(), String> {
    let output = git::write(repo, &switch_args(name)).await?;
    if !output.ok {
        return Err(full_message(&output.stderr));
    }
    Ok(())
}

fn switch_args(name: &str) -> Vec<&str> {
    vec!["switch", "--guess", END_OF_OPTIONS, name]
}

/// New branch at `start_point` (a ref name or commit hash — whatever the graph
/// badge or row that was right-clicked names).
///
/// `checkout` picks the command rather than adding a second switch afterwards:
/// `git switch -c` is atomic where create-then-switch can leave a branch behind
/// on a checkout that fails against a dirty tree. Nothing here ever sets an
/// upstream — a branch created off `origin/foo` is deliberately *not* tracking
/// it, since that is a different intent from "switch to that remote branch"
/// (which `switch_remote_tracking` above already covers).
///
/// `--no-track` is what actually enforces that, and it is not optional: git's
/// `branch.autoSetupMerge` defaults to `true`, so a *remote-tracking* start
/// point silently configures the new branch to track it. Off `origin/main`
/// that produced a branch whose upstream was `origin/main` — which made
/// `needsPublish` (§8.7) see an upstream and offer **Push** instead of
/// **Publish Branch**, and a bare `git push` then fails under the default
/// `push.default = simple` because the names don't match. It fails safe there,
/// but only by luck: under `push.default = upstream` the same button would
/// have pushed a feature branch straight onto `main`.
pub async fn create(repo: &Path, name: &str, start_point: &str, checkout: bool) -> Result<(), String> {
    let output = git::write(repo, &create_args(name, start_point, checkout)).await?;
    if !output.ok {
        return Err(create_message(&output.stderr));
    }
    Ok(())
}

fn create_args<'a>(name: &'a str, start_point: &'a str, checkout: bool) -> Vec<&'a str> {
    if checkout {
        vec!["switch", "--no-track", "-c", name, END_OF_OPTIONS, start_point]
    } else {
        vec!["branch", "--no-track", END_OF_OPTIONS, name, start_point]
    }
}

/// `origin/feature-x` → `feature-x`. Only the first path segment is treated as
/// the remote name, so a branch whose own name contains a `/` (e.g.
/// `origin/feature/x`) still yields `feature/x`.
fn local_name(remote_ref: &str) -> String {
    remote_ref.split_once('/').map_or(remote_ref, |(_, rest)| rest).to_string()
}

/// The whole trimmed stderr, not just its first line — §13's "raw stderr
/// always available in a collapsible Details" needs the whole thing; the
/// frontend's `translateGitError` picks a plain-language headline out of it.
fn full_message(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() { "git switch failed".to_string() } else { trimmed.to_string() }
}

/// Same whole-stderr rule as [`full_message`], with the fallback naming the
/// operation that actually ran.
fn create_message(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() { "could not create the branch".to_string() } else { trimmed.to_string() }
}

/// One repo's branch names, for both multi-repo dialogs (§5.1).
///
/// Fetched once, when a dialog opens, for every repo it lists. The checks then
/// happen in the frontend against this — Create Branch's duplicate check on
/// each keystroke, exactly as `validateBranchName` does it for the single-repo
/// dialog, and Switch & pull's per-row resolution on each pick. A git call per
/// keystroke per repo would be five processes a character, which on the
/// spawn-bound Windows path (§1) is the one thing this codebase will not spend.
pub async fn name_sets(repo: &Path) -> Result<BranchSets, String> {
    let output = git::read(repo, &names_args()).await?;
    if !output.ok {
        return Err(names_message(&output.stderr));
    }
    Ok(parse_names(&output.stdout))
}

/// Split the way the two dialogs need it. Create Branch reads `local` alone —
/// a name that exists on the remote but not here is one that repo can still
/// create, and refusing it would be Corgit inventing a rule git does not have.
/// Switch & pull needs both: a name that exists only on the remote is still a
/// branch you can switch onto, it is just a `switch --guess` that creates the
/// local side on the way (see `switch_to`).
///
/// Both come out of **one** `for-each-ref`, which is why this replaced a
/// locals-only read rather than sitting beside one. The path is spawn-bound
/// (§1): a second ref namespace is free next to the ~85 ms the process itself
/// costs, and a second command would have doubled the cost of opening a dialog
/// over seventy-seven repos to avoid listing refs one of them does not want.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct BranchSets {
    pub local: Vec<String>,
    /// Short names with the remote stripped — `origin/develop` arrives here as
    /// `develop`, deduplicated across remotes, because that is the name the
    /// picker shows and the name `switch --guess` takes.
    pub remote: Vec<String>,
}

/// `for-each-ref`, not `git branch --list`: the porcelain marks the current
/// branch with a leading `* ` and can be reshaped by user config, and this
/// list is compared against typed text character for character.
///
/// `%(refname)` rather than `%(refname:short)`, which this used to be. The
/// short form of `refs/remotes/origin/develop` is `origin/develop`, separable
/// from a local `develop` by looking for a slash — except that a local branch
/// *called* `origin/develop` is perfectly legal, so the cheap test is wrong on
/// exactly the repo where being wrong matters. The full ref name says which
/// namespace a line came from without anything having to guess.
fn names_args() -> Vec<&'static str> {
    vec!["for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes"]
}

fn parse_names(stdout: &str) -> BranchSets {
    let mut sets = BranchSets::default();
    for line in stdout.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(name) = line.strip_prefix("refs/heads/") {
            sets.local.push(name.to_string());
        } else if let Some(rest) = line.strip_prefix("refs/remotes/") {
            // `refs/remotes/origin/HEAD` is a symbolic pointer at the remote's
            // default branch, not a branch of its own. Left in, the picker
            // would offer a branch named `HEAD` that is really a second copy
            // of `main`, and switching onto it would detach every repo it
            // reached.
            let Some((_remote, name)) = rest.split_once('/') else { continue };
            if name == "HEAD" {
                continue;
            }
            if !sets.remote.iter().any(|existing| existing == name) {
                sets.remote.push(name.to_string());
            }
        }
    }
    sets
}

/// Same whole-stderr rule again (§13). This one reaches the user only through
/// the dialog's own "could not read" row, but the raw text still travels.
fn names_message(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() { "could not list branches".to_string() } else { trimmed.to_string() }
}

/// Deleting a local branch (§8.3, the same menu). Only local ones: a
/// remote-tracking badge names a branch on the server, and removing that is a
/// `push --delete` — a network write with a different blast radius, not this.
/// The menu narrows it further to unpublished ones, the same rule as `rename`.
///
/// `force` picks `-D` over `-d`, and the two are a *sequence*, not a choice
/// the caller makes up front: the frontend always tries the safe one first and
/// only offers the forceful one after git has said the branch is not fully
/// merged. That ordering is why this does not fall foul of §8.3's
/// "never force-checkout" rule — the point there is that nothing may discard
/// work *silently*, and here git's own refusal is what puts the question on
/// screen. Squash-merged branches make the unsafe path unavoidable: to git
/// they are unmerged forever, so `-d` alone would mean the most common
/// deletion could never be done from Corgit at all.
///
/// Deleting the checked-out branch is git's own error, and the menu does not
/// offer it — but nothing here re-checks that. HEAD as this process sees it is
/// the only authority (§5.1), and git is holding it.
pub async fn delete(repo: &Path, name: &str, force: bool) -> Result<(), String> {
    let output = git::write(repo, &delete_args(name, force)).await?;
    if !output.ok {
        return Err(delete_message(&output.stderr));
    }
    Ok(())
}

fn delete_args(name: &str, force: bool) -> Vec<&str> {
    vec!["branch", if force { "-D" } else { "-d" }, END_OF_OPTIONS, name]
}

/// Same whole-stderr rule as [`full_message`] (§13). The "not fully merged"
/// refusal has to survive intact in particular — the frontend reads it to
/// decide whether to offer the forceful delete, so trimming it to a headline
/// here would take that branch away.
fn delete_message(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() { "could not delete the branch".to_string() } else { trimmed.to_string() }
}

/// Renaming a local branch (§8.3, the same menu). `-m`, never `-M`: the
/// capital one overwrites a branch that already has the new name, and the
/// dialog's duplicate check is a check against the graph as it was painted —
/// git refusing is the one that cannot be stale.
///
/// The menu offers this only for branches that are not published, and nothing
/// here re-checks it, for the same reason `delete` does not re-check HEAD: the
/// frontend's condition is about which entries to *show*. Unlike `delete`, the
/// checked-out branch is fine — git renames it and moves HEAD along with it.
pub async fn rename(repo: &Path, from: &str, to: &str) -> Result<(), String> {
    let output = git::write(repo, &rename_args(from, to)).await?;
    if !output.ok {
        return Err(rename_message(&output.stderr));
    }
    Ok(())
}

fn rename_args<'a>(from: &'a str, to: &'a str) -> Vec<&'a str> {
    vec!["branch", "-m", END_OF_OPTIONS, from, to]
}

/// Same whole-stderr rule as [`full_message`] (§13).
fn rename_message(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() { "could not rename the branch".to_string() } else { trimmed.to_string() }
}

/// Merge another branch into the checked-out one (§8.3, right-click a ref
/// badge in the graph). `source` is whatever that badge names — a local
/// branch or a remote-tracking one, since merging `origin/main` into the
/// branch you are on is the same gesture and git treats both as commits.
///
/// `--no-edit` for the same reason `pull` passes `--no-rebase`: user config
/// decides otherwise. Git normally skips the editor when it has no terminal,
/// but `merge.edit`/`GIT_MERGE_AUTOEDIT` can force one, and an editor spawned
/// by a process with no console is a hang with nothing on screen to explain
/// it. Nothing here ever passes `--no-ff` — a fast-forward where one is
/// possible is what the user asked for, not a merge commit recording that
/// Corgit was involved.
///
/// A conflict comes back as an `Err` like any other failure, which is
/// deliberate: `write_and_refresh` republishes the repo's status either way,
/// so §13's conflict banner and its *Abort merge* button appear at the same
/// moment the error notice does. The error is what says *why* the tree
/// suddenly has conflicts in it.
pub async fn merge(repo: &Path, source: &str) -> Result<(), String> {
    let output = git::write(repo, &merge_args(source)).await?;
    if !output.ok {
        return Err(merge_message(&output.stdout, &output.stderr));
    }
    Ok(())
}

fn merge_args(source: &str) -> Vec<&str> {
    vec!["merge", "--no-edit", END_OF_OPTIONS, source]
}

/// The one failure in this file that has to read stdout as well as stderr,
/// and not as a nicety: measured on Git 2.51, a conflicting `git merge` exits
/// non-zero with **stderr empty** — `CONFLICT (content): …` and `Automatic
/// merge failed; fix conflicts and then commit the result.` both go to stdout.
/// A stderr-only message would leave the notice blank in exactly the case the
/// user most needs a sentence. The refusals do use stderr (`Your local changes
/// … would be overwritten by merge`), so both streams have to be joined, in
/// the order git emitted them.
fn merge_message(stdout: &str, stderr: &str) -> String {
    let joined =
        [stdout.trim(), stderr.trim()].iter().filter(|part| !part.is_empty()).copied().collect::<Vec<_>>().join("\n");
    if joined.is_empty() { "git merge failed".to_string() } else { joined }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_name_strips_the_remote() {
        assert_eq!(local_name("origin/feature-x"), "feature-x");
    }

    #[test]
    fn local_name_keeps_slashes_past_the_first() {
        assert_eq!(local_name("origin/feature/x"), "feature/x");
    }

    #[test]
    fn local_name_with_no_slash_is_returned_as_is() {
        assert_eq!(local_name("main"), "main");
    }

    #[test]
    fn creating_without_checkout_leaves_head_alone() {
        assert_eq!(
            create_args("feature-x", "main", false),
            ["branch", "--no-track", "--end-of-options", "feature-x", "main"]
        );
    }

    #[test]
    fn creating_with_checkout_is_one_atomic_switch() {
        assert_eq!(
            create_args("feature-x", "main", true),
            ["switch", "--no-track", "-c", "feature-x", "--end-of-options", "main"]
        );
    }

    /// A branch cut from a remote badge is not the same intent as switching to
    /// that remote branch (see `create`'s docs), so it must come out with no
    /// upstream at all.
    ///
    /// This test used to assert the *absence of `--track`*, and passed
    /// throughout the period the behaviour was broken: nothing was ever adding
    /// `--track`, and it was never how the upstream got set. Git's
    /// `branch.autoSetupMerge` default does it on its own for a
    /// remote-tracking start point, so only an explicit `--no-track` prevents
    /// it. Assert the flag that has to be *present*; the one that has to be
    /// absent was never in danger of appearing.
    #[test]
    fn creating_from_a_remote_ref_does_not_set_an_upstream() {
        for checkout in [true, false] {
            let args = create_args("feature-x", "origin/feature-x", checkout);
            assert!(args.contains(&"--no-track"), "{args:?} would inherit origin/feature-x as upstream");
        }
    }

    /// The multi-repo dialog (§5.1) cuts every branch from `HEAD` rather than
    /// from the branch name its row printed — that name can be a sweep old, and
    /// git resolving HEAD in the repo is the only reading that cannot be stale.
    #[test]
    fn creating_from_head_names_no_branch() {
        assert_eq!(
            create_args("fix/auth", "HEAD", true),
            ["switch", "--no-track", "-c", "fix/auth", "--end-of-options", "HEAD"]
        );
        assert_eq!(
            create_args("fix/auth", "HEAD", false),
            ["branch", "--no-track", "--end-of-options", "fix/auth", "HEAD"]
        );
    }

    /// The porcelain would mark the checked-out branch with `* `, and this
    /// list is compared against typed text character for character.
    #[test]
    fn branch_names_are_read_from_plumbing() {
        assert_eq!(names_args(), ["for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes"]);
    }

    /// The two namespaces are told apart by their full ref name, never by
    /// looking for a slash in the short one — a local branch called
    /// `origin/develop` is legal, and it must not be read as a remote.
    #[test]
    fn a_local_branch_named_like_a_remote_one_stays_local() {
        let sets = parse_names("refs/heads/origin/develop\nrefs/remotes/origin/develop\n");
        assert_eq!(sets.local, ["origin/develop"]);
        assert_eq!(sets.remote, ["develop"]);
    }

    /// The picker names a branch once however many remotes carry it: it offers
    /// a name to switch to, not a ref to check out.
    #[test]
    fn a_branch_on_two_remotes_is_named_once() {
        assert_eq!(
            parse_names("refs/remotes/origin/develop\nrefs/remotes/upstream/develop\n").remote,
            ["develop"]
        );
    }

    /// `refs/remotes/origin/HEAD` is a symbolic pointer at the remote's default
    /// branch. Offered as a branch, switching onto it would detach HEAD in
    /// every repo the run reached.
    #[test]
    fn the_remotes_head_pointer_is_not_a_branch() {
        assert_eq!(parse_names("refs/remotes/origin/HEAD\nrefs/remotes/origin/main\n").remote, ["main"]);
    }

    /// Remote-only is the case `switch_to` exists for, and `--guess` is what
    /// resolves it in the repo rather than from the dialog's cached answer —
    /// explicitly, because `checkout.guess = false` would otherwise turn every
    /// remote-only row in a run into a failure the dialog said would not happen.
    #[test]
    fn switching_asks_git_to_resolve_the_name() {
        assert_eq!(switch_args("develop"), ["switch", "--guess", "--end-of-options", "develop"]);
    }

    #[test]
    fn branch_names_drop_blank_lines_and_surrounding_space() {
        let sets = parse_names("refs/heads/main\nrefs/heads/feature/x\n\n  refs/heads/release/3.2  \n");
        assert_eq!(sets.local, ["main", "feature/x", "release/3.2"]);
        assert!(sets.remote.is_empty());
    }

    /// A repo with no commits has no branches, and that is not an error — the
    /// dialog must read it as "nothing to collide with", not as a failure.
    #[test]
    fn a_repo_with_no_branches_parses_to_an_empty_list() {
        assert_eq!(parse_names(""), BranchSets::default());
    }

    #[test]
    fn a_silent_branch_listing_failure_still_says_something() {
        assert_eq!(names_message("  \n"), "could not list branches");
    }

    #[test]
    fn a_plain_delete_is_the_safe_one() {
        assert_eq!(delete_args("feature-x", false), ["branch", "-d", "--end-of-options", "feature-x"]);
    }

    #[test]
    fn a_forced_delete_is_the_capital_one() {
        assert_eq!(delete_args("feature-x", true), ["branch", "-D", "--end-of-options", "feature-x"]);
    }

    /// The frontend decides whether to offer *Delete anyway* by looking for
    /// git's own words in this message, so it must come through whole.
    #[test]
    fn the_not_fully_merged_refusal_survives_whole() {
        let message = delete_message("error: the branch 'feature-x' is not fully merged.
");
        assert_eq!(message, "error: the branch 'feature-x' is not fully merged.");
    }

    #[test]
    fn a_silent_delete_failure_still_says_something() {
        assert_eq!(delete_message("  
"), "could not delete the branch");
    }

    #[test]
    fn merging_never_opens_an_editor() {
        assert_eq!(merge_args("feature-x"), ["merge", "--no-edit", "--end-of-options", "feature-x"]);
    }

    /// The case that made `merge_message` read stdout at all: git puts the
    /// whole conflict report there and leaves stderr empty, so a stderr-only
    /// message would show the user an empty error notice.
    #[test]
    fn a_conflict_report_survives_an_empty_stderr() {
        let message = merge_message("CONFLICT (content): Merge conflict in f.txt\n", "");
        assert_eq!(message, "CONFLICT (content): Merge conflict in f.txt");
    }

    /// The refusals go the other way — stderr only — and both streams have to
    /// come through when git uses both.
    #[test]
    fn both_streams_are_kept_in_the_order_git_wrote_them() {
        let message = merge_message("Auto-merging f.txt\n", "error: Your local changes would be overwritten\n");
        assert_eq!(message, "Auto-merging f.txt\nerror: Your local changes would be overwritten");
    }

    #[test]
    fn a_silent_merge_failure_still_says_something() {
        assert_eq!(merge_message("", "  \n"), "git merge failed");
    }

    use crate::testrepo::TempRepo;

    /// `switch_remote_tracking` decides between creating a tracking branch and
    /// falling back to a plain switch by looking for the words "already
    /// exists" in git's stderr. A fixture cannot check that — it would be
    /// written from the same guess that produced the match. So this drives the
    /// collision in a real repository: if git ever rewords the refusal, the
    /// create's failure is shown to the user instead of being handled, and
    /// this is the test that says so.
    #[tokio::test]
    async fn switching_to_a_remote_ref_falls_back_when_the_local_branch_exists() {
        let (work, _origin) = repo_with_a_pushed_feature_branch("branch-fallback");
        work.git(&["switch", "--quiet", "main"]);

        // `feature` now exists both locally and as `origin/feature` — exactly
        // the collision the fallback exists for.
        switch_remote_tracking(work.path(), "origin/feature").await.unwrap();

        assert_eq!(work.git_stdout(&["rev-parse", "--abbrev-ref", "HEAD"]), "feature");
    }

    /// The path the fallback is a fallback *from*, kept beside it so that a
    /// change breaking the create — and quietly sending every switch through
    /// the fallback — cannot pass by still looking like a success.
    #[tokio::test]
    async fn switching_to_a_remote_ref_creates_a_tracking_branch() {
        let (work, _origin) = repo_with_a_pushed_feature_branch("branch-create");
        work.git(&["switch", "--quiet", "main"]);
        work.git(&["branch", "--quiet", "-D", "feature"]);

        switch_remote_tracking(work.path(), "origin/feature").await.unwrap();

        assert_eq!(work.git_stdout(&["rev-parse", "--abbrev-ref", "HEAD"]), "feature");
        assert_eq!(
            work.git_stdout(&["rev-parse", "--abbrev-ref", "feature@{upstream}"]),
            "origin/feature",
            "the branch was created without an upstream"
        );
    }

    // A name the frontend supplies is a name, never a flag. Each case uses
    // a name that git would *accept* as an option, so without
    // `END_OF_OPTIONS` the call does something — succeeds, or fails for a
    // different reason — and the assertion sees it. A name git rejects as an
    // option anyway would pass with the marker deleted — a test that cannot
    // fail.

    /// The one reproduced before the fix: `--detach` in the branch slot
    /// detached HEAD.
    #[tokio::test]
    async fn a_switch_target_is_never_read_as_a_flag() {
        let repo = repo_with_one_commit("s6-switch");

        assert!(switch_to(repo.path(), "--detach").await.is_err());
        assert!(switch_local(repo.path(), "--detach").await.is_err());

        assert_eq!(repo.git_stdout(&["rev-parse", "--abbrev-ref", "HEAD"]), "main", "HEAD was detached");
    }

    /// `--quiet` as the start point: unguarded, both commands take it as a
    /// flag, default the start point to HEAD, and create the branch anyway.
    #[tokio::test]
    async fn a_start_point_is_never_read_as_a_flag() {
        let repo = repo_with_one_commit("s6-create");

        for checkout in [true, false] {
            assert!(create(repo.path(), "made", "--quiet", checkout).await.is_err());
            assert!(
                repo.try_git(&["rev-parse", "--verify", "--quiet", "refs/heads/made"]).stdout.is_empty(),
                "a branch was created from a flag (checkout: {checkout})"
            );
        }
    }

    /// Unguarded, `merge --no-edit --squash` is a squash merge of the
    /// upstream — refused here only because there is none.
    #[tokio::test]
    async fn a_merge_source_is_never_read_as_a_flag() {
        let repo = repo_with_one_commit("s6-merge");

        let message = merge(repo.path(), "--squash").await.unwrap_err();
        assert!(message.contains("not something we can merge"), "{message}");
    }

    /// The one case here where an unguarded flag cannot *do* anything: with a
    /// single positional, `branch -d <flag>` fails whatever the flag is
    /// (checked: `--list`, `--quiet`, `-v`, `--force`). What the marker changes
    /// is the failure — unguarded the user is told "branch name required" or
    /// shown git's usage text, which is git describing a command Corgit never
    /// meant to run. Guarded, git names the branch it could not find.
    #[tokio::test]
    async fn a_branch_to_delete_is_never_read_as_a_flag() {
        let repo = repo_with_one_commit("s6-delete");

        let message = delete(repo.path(), "--quiet", false).await.unwrap_err();
        assert!(message.contains("'--quiet' not found"), "{message}");
    }

    #[test]
    fn a_rename_never_overwrites() {
        assert_eq!(rename_args("old", "new"), ["branch", "-m", "--end-of-options", "old", "new"]);
    }

    #[test]
    fn a_silent_rename_failure_still_says_something() {
        assert_eq!(rename_message("  \n"), "could not rename the branch");
    }

    #[tokio::test]
    async fn renaming_moves_the_branch_and_drops_the_old_name() {
        let repo = repo_with_one_commit("rename-plain");
        repo.git(&["branch", "old"]);

        rename(repo.path(), "old", "new").await.unwrap();

        assert!(!repo.try_git(&["rev-parse", "--verify", "--quiet", "refs/heads/new"]).stdout.is_empty());
        assert!(repo.try_git(&["rev-parse", "--verify", "--quiet", "refs/heads/old"]).stdout.is_empty());
    }

    /// The case `delete` cannot do: HEAD follows the rename, so the user is
    /// still on the branch they were on, under its new name.
    #[tokio::test]
    async fn renaming_the_checked_out_branch_keeps_head_on_it() {
        let repo = repo_with_one_commit("rename-head");
        repo.git(&["switch", "--quiet", "-c", "old"]);

        rename(repo.path(), "old", "new").await.unwrap();

        let current = repo.try_git(&["branch", "--show-current"]).stdout;
        assert_eq!(String::from_utf8_lossy(&current).trim(), "new");
    }

    /// `-m` rather than `-M`: a name that is already taken is git's refusal,
    /// and the branch that held it is untouched.
    #[tokio::test]
    async fn renaming_onto_an_existing_branch_is_refused() {
        let repo = repo_with_one_commit("rename-taken");
        repo.git(&["branch", "old"]);
        repo.git(&["branch", "taken"]);

        let message = rename(repo.path(), "old", "taken").await.unwrap_err();
        assert!(message.contains("already exists"), "{message}");
        assert!(!repo.try_git(&["rev-parse", "--verify", "--quiet", "refs/heads/old"]).stdout.is_empty());
    }

    fn repo_with_one_commit(name: &str) -> TempRepo {
        let repo = TempRepo::new(name);
        repo.write("a.txt", "one\n");
        repo.commit_all("initial");
        repo
    }

    /// A repo with `main` and `feature`, both pushed to an `origin` that is
    /// itself a temp repo.
    ///
    /// The origin is handed back rather than dropped here: dropping a
    /// `TempRepo` deletes its directory, and that directory is what `origin`
    /// points at. Bound as `_origin` at each call site so it lives to the end
    /// of the test rather than to the end of this function.
    fn repo_with_a_pushed_feature_branch(name: &str) -> (TempRepo, TempRepo) {
        let upstream = TempRepo::new(&format!("{name}-upstream"));
        upstream.allow_pushes_to_checked_out_branch();

        let work = TempRepo::new(name);
        work.write("a.txt", "one\n");
        work.commit_all("initial");
        work.git(&["remote", "add", "origin", &upstream.remote_url()]);
        work.git(&["push", "--quiet", "origin", "main"]);

        work.git(&["switch", "--quiet", "-c", "feature"]);
        work.write("b.txt", "two\n");
        work.commit_all("feature work");
        work.git(&["push", "--quiet", "origin", "feature"]);

        (work, upstream)
    }
}
