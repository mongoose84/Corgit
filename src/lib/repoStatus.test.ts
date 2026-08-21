import { describe, expect, test } from 'vitest';

import { hasConflict, isDirty, needsPublish, type RepoStatus } from './repos.svelte';

function status(overrides: Partial<RepoStatus> = {}): RepoStatus {
  return {
    branch: 'main',
    head: 'a1b2c3d',
    upstream: 'origin/main',
    ahead: 0,
    behind: 0,
    staged: 0,
    unstaged: 0,
    untracked: 0,
    conflicted: 0,
    changedFiles: 0,
    ...overrides,
  };
}

describe('needsPublish (§8.7)', () => {
  test('a branch with an upstream is already published', () => {
    expect(needsPublish(status())).toBe(false);
  });

  test('a branch with no upstream needs publishing', () => {
    expect(needsPublish(status({ branch: 'enhance-quality', upstream: null }))).toBe(true);
  });

  /** The case that makes this worth marking on a row at all: git only emits
   *  `# branch.ab` when an upstream exists, so an unpublished branch reports
   *  ahead 0 and would otherwise render exactly like a synced repo (§5.1). */
  test('an unpublished branch reports no divergence to give it away', () => {
    const s = status({ branch: 'enhance-quality', upstream: null });
    expect(s.ahead).toBe(0);
    expect(s.behind).toBe(0);
    expect(needsPublish(s)).toBe(true);
  });

  /** Detached HEAD has no upstream either, but nothing to publish — the
   *  backend refuses it, so the row must not promise it. */
  test('detached HEAD is not publishable', () => {
    expect(needsPublish(status({ branch: null, upstream: null }))).toBe(false);
  });

  /*
   * A mismatched upstream. Corgit produced these itself until `branch.rs`
   * grew `--no-track`, and no code change repairs one that already exists —
   * only a publish does. Treated as needing publish because a bare `git push`
   * cannot succeed under the default `push.default = simple`, so Push would
   * be the one button guaranteed to fail.
   */
  test('a branch tracking a differently-named upstream needs publishing', () => {
    expect(needsPublish(status({ branch: 'Update_the_titlebar', upstream: 'origin/main' }))).toBe(true);
  });

  test('a branch tracking its own name on any remote is already published', () => {
    expect(needsPublish(status({ branch: 'feature-x', upstream: 'origin/feature-x' }))).toBe(false);
    expect(needsPublish(status({ branch: 'feature-x', upstream: 'fork/feature-x' }))).toBe(false);
  });

  /** Only the first segment is the remote, so a slash in the branch's own
   *  name must not read as a mismatch — this is the common `jk/thing` shape,
   *  and getting it wrong would push a publish on every such branch. */
  test('a slash in the branch name is not a mismatch', () => {
    expect(needsPublish(status({ branch: 'jk/retry', upstream: 'origin/jk/retry' }))).toBe(false);
  });

  test('a differently-named upstream is a mismatch even sharing a prefix', () => {
    expect(needsPublish(status({ branch: 'retry', upstream: 'origin/jk/retry' }))).toBe(true);
  });

  /** Detached HEAD wins over the mismatch check too: there is no branch name
   *  to compare against, and nothing to publish either way. */
  test('detached HEAD with an upstream set is still not publishable', () => {
    expect(needsPublish(status({ branch: null, upstream: 'origin/main' }))).toBe(false);
  });
});

describe('isDirty (§5.1)', () => {
  test('a clean tree is not dirty', () => {
    expect(isDirty(status())).toBe(false);
  });

  test.each(['staged', 'unstaged', 'untracked', 'conflicted'] as const)('%s alone is dirty', (field) => {
    expect(isDirty(status({ [field]: 1 }))).toBe(true);
  });

  /** `changedFiles` is what the badge prints, but it must never be what
   *  decides whether the badge appears: a status missing it — an older cache,
   *  a partial fixture — has to still read as dirty rather than render a repo
   *  full of work as clean. The parser keeps the two in step (`status.rs`'s
   *  `nothing_is_dirty_without_a_changed_file`); this pins which way the
   *  frontend fails if they ever part. */
  test('a status with counts but no changedFiles is still dirty', () => {
    expect(isDirty(status({ unstaged: 2, changedFiles: 0 }))).toBe(true);
  });
});

describe('hasConflict (§13)', () => {
  /**
   * The blocking tier's whole predicate. It decides three things that must
   * never disagree: the row's ⚠, the banner above the panes, and whether
   * Commit and Push are usable at all — which is why it is shared rather than
   * re-derived at each of the three.
   */
  test('a clean repo is not conflicted', () => {
    expect(hasConflict(status())).toBe(false);
  });

  test('any unmerged path is a conflict', () => {
    expect(hasConflict(status({ conflicted: 1, changedFiles: 1 }))).toBe(true);
  });

  test('an ordinarily dirty repo is not blocked', () => {
    // The distinction the tiers rest on: work in progress is not a state git
    // refuses to leave, so it must not raise a banner that cannot be
    // dismissed or block the two buttons.
    expect(hasConflict(status({ unstaged: 4, changedFiles: 4 }))).toBe(false);
  });
});
