/**
 * What *Switch & pull* (SPEC.md §5.1) will do to each repository, decided
 * before anything runs.
 *
 * Deliberately free of `repos.svelte.ts` and of runes: the dialog maps a
 * `RepoStatus` and a `RepoBranches` onto [`SwitchContext`] and everything
 * interesting happens here, in plain functions a test can call. The same split
 * `branchName.ts` and `repoFilter.ts` already use, and for the same reason —
 * this is the code that decides whether sixty working trees get checked out.
 *
 * The rule that shapes all of it: **only refuse what is knowable locally.**
 * Two conditions qualify — the repo has no branch of that name at all, and a
 * merge is in progress, which git will not switch through. A dirty working tree
 * is *not* one of them: git switches through uncommitted changes unless they
 * collide with what differs between the branches, which cannot be known without
 * trying, and §8.3 forbids force-checkout. So it is a caution carried alongside
 * the plan rather than a reason to exclude a row.
 */

/** One repo's branch names, split by namespace — `branch::BranchSets` as it
 *  crosses the boundary. Remote names have their remote stripped: `develop`,
 *  not `origin/develop`, because that is the name being switched to. */
export interface BranchSets {
  local: string[];
  remote: string[];
}

/** Everything about one repo that the decision depends on. Assembled by the
 *  dialog so this file never has to know what a `RepoStatus` looks like. */
export interface SwitchContext {
  /** `null` when the repo's refs could not be read — which blocks the row
   *  rather than waving it through, the same call `MultiBranchDialog` makes
   *  about a name it could not check for collisions. */
  branches: BranchSets | null;
  /** The branch the repo is on now, `''` when HEAD is detached or its status
   *  has not arrived yet. Read from cached status and possibly a sweep old
   *  (§5.1), so it is *shown* and never *used* — the run passes the target name
   *  and lets git resolve where it is coming from. */
  current: string;
  /** Commits behind the upstream *of the branch it is on now*. Only meaningful
   *  when that is already the target branch: status carries ahead/behind for
   *  the checked-out branch and nothing else (§8.2), so for any other repo the
   *  honest answer is "not known yet" rather than a number. */
  behind: number;
  conflicted: boolean;
  dirty: boolean;
}

/** Why a repo cannot take the switch. Ordered as the checks run — see
 *  [`planSwitch`] for why absence is reported ahead of a merge. */
export type SwitchBlocker = 'unreadable' | 'absent' | 'merging';

export type SwitchPlan =
  | { kind: 'blocked'; blocker: SwitchBlocker }
  /** Already on the target branch: the switch is a no-op and the pull is the
   *  whole of the work. */
  | { kind: 'stay'; behind: number }
  /** A local branch of that name exists — a plain `git switch`. */
  | { kind: 'switch'; from: string }
  /** The name exists only on a remote, so the switch creates the local side on
   *  the way (`switch --guess`, see `branch::switch_to`). Worth its own kind
   *  because it is the one plan that leaves a branch behind that was not there
   *  before, and the row says so. */
  | { kind: 'create'; from: string };

/**
 * What will happen to one repo if the run goes ahead.
 *
 * Absence is checked before the merge, and the order is not arbitrary: a repo
 * both mid-merge and lacking the branch has two problems, and the one worth
 * naming is the one that changes as the user picks a different branch. Naming
 * the merge instead would leave a row that stays red however they re-pick, for
 * a reason that has nothing to do with the choice they are making.
 */
export function planSwitch(name: string, context: SwitchContext): SwitchPlan {
  if (context.branches === null) return { kind: 'blocked', blocker: 'unreadable' };
  if (name === '') return { kind: 'blocked', blocker: 'absent' };

  // The no-op case comes first, and it is checked against the *branch the repo
  // is on* rather than against the name lists: a repo already on `develop`
  // needs no switch even in the odd case where the ref read and the status read
  // disagree about what exists.
  if (context.current === name) return { kind: 'stay', behind: context.behind };

  const local = context.branches.local.includes(name);
  const remote = context.branches.remote.includes(name);
  if (!local && !remote) return { kind: 'blocked', blocker: 'absent' };

  // Only after we know there is something to switch to. A repo mid-merge that
  // does not have the branch anyway is better described by the first reason.
  if (context.conflicted) return { kind: 'blocked', blocker: 'merging' };

  return local ? { kind: 'switch', from: context.current } : { kind: 'create', from: context.current };
}

export function isBlocked(plan: SwitchPlan): plan is { kind: 'blocked'; blocker: SwitchBlocker } {
  return plan.kind === 'blocked';
}

/** The row's reason text. `--status-error` rather than `--status-conflict` in
 *  the dialog (§13): nothing has failed, this is the dialog saying so in
 *  advance. */
export function blockerLabel(blocker: SwitchBlocker, name: string): string {
  switch (blocker) {
    case 'unreadable':
      return 'could not read its branches';
    case 'absent':
      return name === '' ? 'no branch picked' : `no ${name} branch`;
    case 'merging':
      return 'merge in progress';
  }
}

/**
 * One branch name as the picker offers it, with the repos it covers.
 *
 * `local` and `remote` count repositories, not refs, and a repo holding the
 * branch on both sides counts as local — that is the case that needs no branch
 * created, and the split under the field is there to say how much of the run
 * will create one.
 */
export interface BranchCoverage {
  name: string;
  local: number;
  remote: number;
  /** `local + remote` — the number the picker prints, and the one the field's
   *  note calls "present in N". */
  repos: number;
}

/**
 * Every branch name in the section, most widely held first.
 *
 * Sorted by coverage rather than alphabetically because the whole gesture is
 * "put the herd on one branch", and the branches that can hold the herd are the
 * answer — `main` and `develop` at the top, one repo's `spike/queue` at the
 * bottom. Ties break by name so the order is stable between two opens of the
 * dialog over an unchanged root.
 */
export function branchCoverage(entries: readonly (BranchSets | null)[]): BranchCoverage[] {
  const found = new Map<string, BranchCoverage>();

  const bump = (name: string, side: 'local' | 'remote') => {
    const existing = found.get(name);
    if (existing === undefined) {
      found.set(name, { name, local: side === 'local' ? 1 : 0, remote: side === 'remote' ? 1 : 0, repos: 1 });
      return;
    }
    existing[side] += 1;
    existing.repos += 1;
  };

  for (const sets of entries) {
    if (sets === null) continue;
    for (const name of sets.local) bump(name, 'local');
    // A repo with the branch on both sides is counted once, as local: it is
    // the side that decides whether the switch creates anything, and counting
    // it twice would make `repos` exceed the number of repositories.
    for (const name of sets.remote) {
      if (!sets.local.includes(name)) bump(name, 'remote');
    }
  }

  return [...found.values()].sort((a, b) => b.repos - a.repos || a.name.localeCompare(b.name));
}

/** The line under the branch field — how the pick lands across the section.
 *  `absent` is what the excluded count is built from, so the two can never
 *  disagree about the same run. */
export interface CoverageSplit {
  local: number;
  remote: number;
  absent: number;
}

export function coverageSplit(name: string, entries: readonly (BranchSets | null)[]): CoverageSplit {
  const split: CoverageSplit = { local: 0, remote: 0, absent: 0 };
  for (const sets of entries) {
    // A repo whose refs could not be read is counted as absent: it is excluded
    // from the run either way, and inventing a fourth number for it would put
    // arithmetic on screen that does not add up to the section.
    if (sets === null || name === '') {
      split.absent += 1;
    } else if (sets.local.includes(name)) {
      split.local += 1;
    } else if (sets.remote.includes(name)) {
      split.remote += 1;
    } else {
      split.absent += 1;
    }
  }
  return split;
}
