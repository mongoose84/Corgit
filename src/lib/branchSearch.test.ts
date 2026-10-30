import { describe, expect, test } from 'vitest';

import { matchesBranch, searchBranches } from './branchSearch';
import type { RefBadge } from './graph.svelte';

/**
 * The predicate is one line and barely worth pinning; the *ordering* is the
 * reason this file exists. It encodes three decisions that a reader would
 * otherwise have to take on trust — local first, newest first, ties on name —
 * and each of them is a decision someone will reasonably want to change.
 */

let seq = 0;

function ref(name: string, kind: 'local' | 'remote', timestamp: number): RefBadge {
  seq += 1;
  return { name, commit: `hash${seq}`, kind, timestamp, subject: `subject ${seq}` };
}

const names = (refs: RefBadge[]) => refs.map((r) => r.name);

describe('matching', () => {
  test('substring, anywhere in the name, either case', () => {
    expect(matchesBranch('release/R2026-08', 'rel')).toBe(true);
    expect(matchesBranch('release/R2026-08', 'R2026')).toBe(true);
    expect(matchesBranch('release/R2026-08', 'RELEASE')).toBe(true);
    expect(matchesBranch('Releases/hotfix', 'releases')).toBe(true);
    expect(matchesBranch('main', 'rel')).toBe(false);
  });

  test('a remote badge matches on its remote too', () => {
    // The name a remote badge carries is `origin/x`, so typing the remote is
    // how you narrow to remote-tracking branches — no separate syntax.
    expect(matchesBranch('origin/release/R2026-08', 'origin/')).toBe(true);
    expect(matchesBranch('release/R2026-08', 'origin/')).toBe(false);
  });

  test('an empty or whitespace-only query matches everything', () => {
    // A box just cleared and a box never typed in must behave identically.
    expect(matchesBranch('anything', '')).toBe(true);
    expect(matchesBranch('anything', '   ')).toBe(true);
  });

  test('surrounding whitespace is trimmed, not matched on', () => {
    expect(matchesBranch('main', '  main  ')).toBe(true);
  });

  test('a comma is an ordinary character, unlike in the repo filter', () => {
    // repoFilter.ts splits on commas for §13's bulk banner. Branch names may
    // legally contain one, and nothing here asks for a list — so `a,b` is a
    // needle, not two needles.
    expect(matchesBranch('odd,name', 'odd,name')).toBe(true);
    expect(matchesBranch('odd', 'odd,name')).toBe(false);
  });
});

describe('ordering', () => {
  test('local branches come before remote ones', () => {
    // Even when the remote is newer: `x` can be switched to, `origin/x` has to
    // be created first (§8.3).
    const refs = [ref('origin/main', 'remote', 300), ref('main', 'local', 100)];

    expect(names(searchBranches(refs, ''))).toEqual(['main', 'origin/main']);
  });

  test('newest tip first within each group, not alphabetical', () => {
    const refs = [
      ref('release/R2024-11', 'local', 100),
      ref('release/R2026-08', 'local', 300),
      ref('release/R2025-06', 'local', 200),
    ];

    expect(names(searchBranches(refs, 'release'))).toEqual([
      'release/R2026-08',
      'release/R2025-06',
      'release/R2024-11',
    ]);
  });

  test('branches sharing a tip commit order by name, so the list is stable', () => {
    const refs = [ref('zeta', 'local', 500), ref('alpha', 'local', 500)];

    expect(names(searchBranches(refs, ''))).toEqual(['alpha', 'zeta']);
  });

  test('an undatable ref sorts last rather than disappearing', () => {
    // `timestamp: 0` is graph.rs's fallback for a ref whose date git could not
    // print. It is still findable by name, which is what the search is for.
    const refs = [ref('broken', 'local', 0), ref('main', 'local', 100)];

    expect(names(searchBranches(refs, ''))).toEqual(['main', 'broken']);
  });

  test('the query filters before the ordering runs', () => {
    const refs = [
      ref('main', 'local', 400),
      ref('release/R2026-08', 'local', 300),
      ref('origin/release/R2026-08', 'remote', 300),
      ref('origin/main', 'remote', 400),
    ];

    expect(names(searchBranches(refs, 'release'))).toEqual([
      'release/R2026-08',
      'origin/release/R2026-08',
    ]);
  });

  test('no matches is an empty list, not everything', () => {
    const refs = [ref('main', 'local', 100)];

    expect(searchBranches(refs, 'nothing-like-this')).toEqual([]);
  });

  test('the input array is not reordered in place', () => {
    // `sort` mutates, and the array handed in is `graph.refs` — a $state array
    // the ref badges on every row are read from.
    const refs = [ref('b', 'local', 100), ref('a', 'local', 200)];
    const before = names(refs);

    searchBranches(refs, '');

    expect(names(refs)).toEqual(before);
  });
});
