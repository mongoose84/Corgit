import { describe, expect, it } from 'vitest';

import type { RefBadge } from './graph.svelte';
import { publishedBranches } from './publishedBranches';

function ref(name: string, kind: RefBadge['kind']): RefBadge {
  return { name, commit: 'a'.repeat(40), kind, timestamp: 0, subject: '' };
}

describe('publishedBranches', () => {
  it('counts a branch with a remote badge of the same name', () => {
    expect(publishedBranches([ref('main', 'local'), ref('origin/main', 'remote')])).toEqual(new Set(['main']));
  });

  // The regression: `-u` leaves an upstream configured after the server branch
  // is deleted and pruned, and that alone must not hold Delete back.
  it('drops a branch once its remote badge is pruned, whatever it tracks', () => {
    expect(publishedBranches([ref('feature', 'local'), ref('origin/main', 'remote')])).toEqual(new Set());
  });

  // `feature-x` tracking `origin/main`: never pushed under its own name, and
  // `needsPublish` says so — the graph must not call it published either.
  it('leaves out a branch whose only remote counterpart has another name', () => {
    expect(publishedBranches([ref('feature-x', 'local'), ref('origin/main', 'remote')])).toEqual(new Set());
  });

  it('reads slashes past the remote name as part of the branch name', () => {
    expect(publishedBranches([ref('feature/x', 'local'), ref('origin/feature/x', 'remote')])).toEqual(
      new Set(['feature/x']),
    );
  });

  it('matches a remote other than origin', () => {
    expect(publishedBranches([ref('fix', 'local'), ref('fork/fix', 'remote')])).toEqual(new Set(['fix']));
  });

  it('matches a remote whose own name contains a slash', () => {
    expect(publishedBranches([ref('fix', 'local'), ref('jk/fork/fix', 'remote')])).toEqual(new Set(['fix']));
  });

  // The suffix match's one wrong answer, pinned so it stays the cautious one.
  it('holds back a branch whose name is only the tail of a remote branch', () => {
    expect(publishedBranches([ref('x', 'local'), ref('origin/feature/x', 'remote')])).toEqual(new Set(['x']));
  });

  it('does not match on a partial segment', () => {
    expect(publishedBranches([ref('fix', 'local'), ref('origin/hotfix', 'remote')])).toEqual(new Set());
  });

  it('never lists a remote badge itself', () => {
    expect(publishedBranches([ref('origin/main', 'remote')])).toEqual(new Set());
  });
});
