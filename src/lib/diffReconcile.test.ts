import { describe, expect, it } from 'vitest';

import { reconcileOpen, sourceForRow, type OpenDiff } from './diff.svelte';
import type { FileChanges, FileEntry } from './repos.svelte';

/**
 * The rule under test is "the tab lives as long as its row" (§5.2, §5.4), and
 * it is wrong in two directions that both fail silently: a tab closed under
 * someone reading it, and a tab left over a file that was discarded or
 * committed away. Nothing on screen contradicts either one — which is exactly
 * the reason the decision was pulled out of the store to be pinned here.
 */

const OPEN: OpenDiff = { repoId: 'r1', path: 'src/main.rs', source: { kind: 'unstaged' } };

function changes(staged: FileEntry[], unstaged: FileEntry[]): FileChanges {
  return {
    staged,
    stagedTotal: staged.length,
    unstaged,
    unstagedTotal: unstaged.length,
    conflicted: [],
  };
}

const modified = (path: string): FileEntry => ({ path, status: 'M' });
const untracked = (path: string): FileEntry => ({ path, status: '?' });

describe('reconcileOpen', () => {
  it('keeps a diff whose row is still in the section it was opened from', () => {
    expect(reconcileOpen(OPEN, changes([], [modified('src/main.rs')]))).toEqual({ kind: 'keep' });
  });

  it('closes a diff whose file was discarded or committed away', () => {
    expect(reconcileOpen(OPEN, changes([], [modified('other.rs')]))).toEqual({ kind: 'close' });
  });

  it('closes on an empty working tree — the commit case', () => {
    expect(reconcileOpen(OPEN, changes([], []))).toEqual({ kind: 'close' });
  });

  it('follows the file into Staged Changes rather than closing on the + button', () => {
    expect(reconcileOpen(OPEN, changes([modified('src/main.rs')], []))).toEqual({
      kind: 'repoint',
      source: { kind: 'staged' },
    });
  });

  it('follows it back out again on unstage', () => {
    const open: OpenDiff = { ...OPEN, source: { kind: 'staged' } };
    expect(reconcileOpen(open, changes([], [modified('src/main.rs')]))).toEqual({
      kind: 'repoint',
      source: { kind: 'unstaged' },
    });
  });

  it('keeps a partly staged file on the side it was opened from', () => {
    const files = changes([modified('src/main.rs')], [modified('src/main.rs')]);
    expect(reconcileOpen(OPEN, files)).toEqual({ kind: 'keep' });
  });

  // `git add` on an untracked file leaves no untracked row, but the staged one
  // holds the same all-additions content the tab was already showing.
  it('follows an untracked file into the index', () => {
    const open: OpenDiff = { ...OPEN, source: { kind: 'untracked' } };
    expect(reconcileOpen(open, changes([{ path: 'src/main.rs', status: 'A' }], []))).toEqual({
      kind: 'repoint',
      source: { kind: 'staged' },
    });
  });

  it("never touches a commit's diff, whatever the working tree does", () => {
    const open: OpenDiff = { ...OPEN, source: { kind: 'commit', hash: 'abc1234' } };
    expect(reconcileOpen(open, changes([], []))).toEqual({ kind: 'keep' });
  });

  // The lists are capped at 100 rows per section. Absence from a capped list is
  // not absence from the working tree, and this is the direction to be wrong in.
  it('keeps the tab when the list it would have to check is truncated', () => {
    const files: FileChanges = { ...changes([], [modified('other.rs')]), unstagedTotal: 412 };
    expect(reconcileOpen(OPEN, files)).toEqual({ kind: 'keep' });
  });

  it('is not fooled by a path that only shares a prefix', () => {
    expect(reconcileOpen(OPEN, changes([], [modified('src/main.rs.bak')]))).toEqual({
      kind: 'close',
    });
  });
});

describe('sourceForRow', () => {
  it('reads the section, not the entry, for a tracked row', () => {
    expect(sourceForRow('staged', modified('a.txt'))).toEqual({ kind: 'staged' });
    expect(sourceForRow('unstaged', modified('a.txt'))).toEqual({ kind: 'unstaged' });
  });

  // A `?` row has no other side for `git diff` to compare against, so it gets
  // its own source rather than a diff that would correctly report nothing.
  it('gives an untracked row its own source', () => {
    expect(sourceForRow('unstaged', untracked('a.txt'))).toEqual({ kind: 'untracked' });
  });
});
