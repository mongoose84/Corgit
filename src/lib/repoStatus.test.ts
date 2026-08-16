import { describe, expect, test } from 'vitest';

import { isDirty, needsPublish, type RepoStatus } from './repos.svelte';

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
});

describe('isDirty (§5.1)', () => {
  test('a clean tree is not dirty', () => {
    expect(isDirty(status())).toBe(false);
  });

  test.each(['staged', 'unstaged', 'untracked', 'conflicted'] as const)('%s alone is dirty', (field) => {
    expect(isDirty(status({ [field]: 1 }))).toBe(true);
  });
});
