import { describe, expect, it } from 'vitest';

import type { RefBadge } from './graph.svelte';
import { publishedBranches } from './publishedBranches';

function ref(name: string, kind: RefBadge['kind'], upstream: string | null = null): RefBadge {
  return { name, commit: 'a'.repeat(40), kind, timestamp: 0, subject: '', upstream };
}

describe('publishedBranches', () => {
  it('counts a branch with an upstream as published', () => {
    expect(publishedBranches([ref('main', 'local', 'origin/main'), ref('origin/main', 'remote')])).toEqual(
      new Set(['main']),
    );
  });

  it('counts a branch pushed without -u, which has a remote badge but no upstream', () => {
    expect(publishedBranches([ref('feature', 'local'), ref('origin/feature', 'remote')])).toEqual(
      new Set(['feature']),
    );
  });

  it('counts a branch that tracks a remote branch of a different name', () => {
    expect(publishedBranches([ref('mine', 'local', 'origin/theirs'), ref('origin/theirs', 'remote')])).toEqual(
      new Set(['mine']),
    );
  });

  it('leaves out a branch that exists only locally', () => {
    expect(publishedBranches([ref('wip', 'local'), ref('origin/main', 'remote')])).toEqual(new Set());
  });

  it('reads slashes past the remote name as part of the branch name', () => {
    expect(publishedBranches([ref('feature/x', 'local'), ref('origin/feature/x', 'remote')])).toEqual(
      new Set(['feature/x']),
    );
  });

  it('matches a remote other than origin', () => {
    expect(publishedBranches([ref('fix', 'local'), ref('fork/fix', 'remote')])).toEqual(new Set(['fix']));
  });

  it('never lists a remote badge itself', () => {
    expect(publishedBranches([ref('origin/main', 'remote')])).toEqual(new Set());
  });
});
