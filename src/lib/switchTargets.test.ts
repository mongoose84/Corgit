import { describe, expect, it } from 'vitest';

import {
  blockerLabel,
  branchCoverage,
  coverageSplit,
  planSwitch,
  type BranchSets,
  type SwitchContext,
} from './switchTargets';

function context(overrides: Partial<SwitchContext> = {}): SwitchContext {
  return {
    branches: { local: ['main', 'develop'], remote: ['develop', 'release/4.0'] },
    current: 'main',
    behind: 0,
    conflicted: false,
    dirty: false,
    ...overrides,
  };
}

describe('planSwitch', () => {
  it('switches plainly when the branch is already local', () => {
    expect(planSwitch('develop', context())).toEqual({ kind: 'switch', from: 'main' });
  });

  it('creates the local side when the branch is only on a remote', () => {
    expect(planSwitch('release/4.0', context())).toEqual({ kind: 'create', from: 'main' });
  });

  /** The whole of the work for this repo is the pull, and the row has a real
   *  count to print because status carries ahead/behind for the branch it is
   *  actually on (§8.2). */
  it('stays put when the repo is already on the branch, and keeps its count', () => {
    expect(planSwitch('main', context({ behind: 3 }))).toEqual({ kind: 'stay', behind: 3 });
  });

  /** A repo on both sides needs no branch created — the local one wins. */
  it('prefers the local branch when the name is on both sides', () => {
    expect(planSwitch('develop', context())).toEqual({ kind: 'switch', from: 'main' });
  });

  it('blocks a repo with no branch of that name', () => {
    expect(planSwitch('spike/queue', context())).toEqual({ kind: 'blocked', blocker: 'absent' });
  });

  /** Refs that could not be read must never read as "no collisions here" — the
   *  same call `MultiBranchDialog` makes about a name it could not check. */
  it('blocks a repo whose refs could not be read', () => {
    expect(planSwitch('develop', context({ branches: null }))).toEqual({
      kind: 'blocked',
      blocker: 'unreadable',
    });
  });

  it('blocks a repo mid-merge, which git will not switch through', () => {
    expect(planSwitch('develop', context({ conflicted: true }))).toEqual({
      kind: 'blocked',
      blocker: 'merging',
    });
  });

  /**
   * The order the two blockers are reported in, and the reason for it: absence
   * changes as the user picks a different branch, a merge does not. Naming the
   * merge would leave a row that stays red however they re-pick, for a reason
   * that has nothing to do with the choice being made.
   */
  it('names the missing branch rather than the merge when a repo has both problems', () => {
    const both = context({ branches: { local: ['main'], remote: [] }, conflicted: true });
    expect(planSwitch('develop', both)).toEqual({ kind: 'blocked', blocker: 'absent' });
  });

  /** A merge in progress on the branch you are already on does not stop the
   *  pull being attempted — `git pull` refuses on its own, with its own words,
   *  and §13 would rather show that than pre-empt it. But there is no switch to
   *  block, so the row is not excluded for one. */
  it('does not block a mid-merge repo that is already on the branch', () => {
    expect(planSwitch('main', context({ conflicted: true, behind: 2 }))).toEqual({
      kind: 'stay',
      behind: 2,
    });
  });

  /**
   * The rule the whole design rests on (§8.3 forbids force-checkout): git
   * switches through uncommitted changes unless they collide, which is not
   * knowable locally, so a dirty tree is a caution the row carries and never a
   * reason to drop it from the run.
   */
  it('never blocks a repo for having uncommitted changes', () => {
    expect(planSwitch('develop', context({ dirty: true }))).toEqual({ kind: 'switch', from: 'main' });
  });

  it('blocks every repo while no branch is picked', () => {
    expect(planSwitch('', context())).toEqual({ kind: 'blocked', blocker: 'absent' });
  });
});

describe('blockerLabel', () => {
  it('names the branch that is missing', () => {
    expect(blockerLabel('absent', 'develop')).toBe('no develop branch');
  });

  it('says nothing about a branch when none is picked', () => {
    expect(blockerLabel('absent', '')).toBe('no branch picked');
  });

  it('names the merge rather than the repo, which is the thing that can change', () => {
    expect(blockerLabel('merging', 'develop')).toBe('merge in progress');
  });
});

describe('branchCoverage', () => {
  const sets = (local: string[], remote: string[]): BranchSets => ({ local, remote });

  /** The gesture is "put the herd on one branch", so the branches that can hold
   *  the herd have to be the top of the list. */
  it('orders by how many repositories hold the branch', () => {
    const coverage = branchCoverage([
      sets(['main', 'develop'], []),
      sets(['main', 'develop'], []),
      sets(['main'], []),
      sets(['main', 'spike/queue'], []),
    ]);
    expect(coverage.map((entry) => [entry.name, entry.repos])).toEqual([
      ['main', 4],
      ['develop', 2],
      ['spike/queue', 1],
    ]);
  });

  /** Two opens of the dialog over an unchanged root must produce the same list,
   *  and coverage alone does not settle a tie. */
  it('breaks ties by name so the order is stable', () => {
    const coverage = branchCoverage([sets(['zebra', 'alpha'], [])]);
    expect(coverage.map((entry) => entry.name)).toEqual(['alpha', 'zebra']);
  });

  /** `repos` is a count of repositories; a repo holding the branch on both
   *  sides counting twice would let it exceed the size of the section. */
  it('counts a repo holding the branch on both sides once, as local', () => {
    const coverage = branchCoverage([sets(['develop'], ['develop'])]);
    expect(coverage).toEqual([{ name: 'develop', local: 1, remote: 0, repos: 1 }]);
  });

  it('counts a remote-only branch as remote', () => {
    const coverage = branchCoverage([sets(['main'], ['develop'])]);
    expect(coverage.find((entry) => entry.name === 'develop')).toEqual({
      name: 'develop',
      local: 0,
      remote: 1,
      repos: 1,
    });
  });

  it('skips repos whose refs could not be read rather than counting them as empty', () => {
    expect(branchCoverage([null, sets(['main'], [])])).toEqual([
      { name: 'main', local: 1, remote: 0, repos: 1 },
    ]);
  });
});

describe('coverageSplit', () => {
  const sets = (local: string[], remote: string[]): BranchSets => ({ local, remote });

  it('splits the section three ways', () => {
    const split = coverageSplit('develop', [
      sets(['develop'], []),
      sets(['develop'], ['develop']),
      sets(['main'], ['develop']),
      sets(['main'], []),
    ]);
    expect(split).toEqual({ local: 2, remote: 1, absent: 1 });
  });

  /** The three numbers are what the excluded count is built from, so they have
   *  to add up to the section however a repo failed to answer. */
  it('counts an unreadable repo as absent so the arithmetic still adds up', () => {
    const entries = [null, sets(['develop'], []), sets(['main'], [])];
    const split = coverageSplit('develop', entries);
    expect(split).toEqual({ local: 1, remote: 0, absent: 2 });
    expect(split.local + split.remote + split.absent).toBe(entries.length);
  });

  it('counts everything absent while no branch is picked', () => {
    expect(coverageSplit('', [sets(['main'], []), sets(['develop'], [])])).toEqual({
      local: 0,
      remote: 0,
      absent: 2,
    });
  });
});
