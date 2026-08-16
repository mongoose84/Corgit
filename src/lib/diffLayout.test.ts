import { describe, expect, test } from 'vitest';

import { layoutDiff, type DiffHunk, type DiffLine, type FileDiff } from './diffLayout';

/**
 * Side-by-side alignment is the frontend's other silently-wrong algorithm (see
 * `graphLayout.test.ts`): a mis-paired row still renders, it just shows the
 * wrong old line opposite the new one. Nothing throws, no git error surfaces,
 * and the reader has no way to tell. Hence tests rather than a look at it.
 *
 * `layoutDiff` is pure — parsed hunks in, rows out — so none of this needs a
 * DOM, a repo, or a mocked IPC boundary.
 */

function lines(spec: string): DiffLine[] {
  // "-a +A  b" is unreadable; one line per entry, first char is git's own.
  return spec
    .split('\n')
    .filter((line) => line.length > 0)
    .map((line) => ({ kind: line[0] as DiffLine['kind'], text: line.slice(1) }));
}

function hunk(oldStart: number, newStart: number, spec: string): DiffHunk {
  const body = lines(spec);
  return {
    oldStart,
    newStart,
    oldCount: body.filter((line) => line.kind !== '+').length,
    newCount: body.filter((line) => line.kind !== '-').length,
    lines: body,
  };
}

function diff(...hunks: DiffHunk[]): FileDiff {
  return { path: 'src/main.rs', hunks, binary: false, truncated: false, insertions: 0, deletions: 0 };
}

describe('pairing', () => {
  test('a modified line puts old opposite new', () => {
    const { rows } = layoutDiff(diff(hunk(1, 1, ' a\n-b\n+B\n c')));

    expect(rows).toEqual([
      { kind: 'pair', oldNo: 1, oldText: 'a', newNo: 1, newText: 'a', changed: false },
      { kind: 'pair', oldNo: 2, oldText: 'b', newNo: 2, newText: 'B', changed: true },
      { kind: 'pair', oldNo: 3, oldText: 'c', newNo: 3, newText: 'c', changed: false },
    ]);
  });

  test('a pure insertion leaves the old side empty and does not consume an old number', () => {
    const { rows } = layoutDiff(diff(hunk(1, 1, ' a\n+new\n b')));

    expect(rows[1]).toEqual({
      kind: 'pair',
      oldNo: null,
      oldText: null,
      newNo: 2,
      newText: 'new',
      changed: true,
    });
    // `b` is still old line 2 — the insertion did not exist on that side.
    expect(rows[2]).toMatchObject({ oldNo: 2, newNo: 3 });
  });

  test('a pure deletion leaves the new side empty', () => {
    const { rows } = layoutDiff(diff(hunk(1, 1, ' a\n-gone\n b')));

    expect(rows[1]).toEqual({
      kind: 'pair',
      oldNo: 2,
      oldText: 'gone',
      newNo: null,
      newText: null,
      changed: true,
    });
    expect(rows[2]).toMatchObject({ oldNo: 3, newNo: 2 });
  });

  test('uneven runs pair as far as they can, then fill', () => {
    const { rows } = layoutDiff(diff(hunk(10, 10, '-a\n-b\n-c\n+A')));

    expect(rows).toEqual([
      { kind: 'gap', skipped: 9 },
      { kind: 'pair', oldNo: 10, oldText: 'a', newNo: 10, newText: 'A', changed: true },
      { kind: 'pair', oldNo: 11, oldText: 'b', newNo: null, newText: null, changed: true },
      { kind: 'pair', oldNo: 12, oldText: 'c', newNo: null, newText: null, changed: true },
    ]);
  });

  /** The case a naive "collect all -, collect all +" pass gets wrong: without
   *  flushing on the second `-`, the four lines collapse into one block and
   *  `a` ends up opposite `B`. */
  test('alternating change blocks stay separate', () => {
    const { rows } = layoutDiff(diff(hunk(1, 1, '-a\n+A\n-b\n+B')));

    expect(rows).toEqual([
      { kind: 'pair', oldNo: 1, oldText: 'a', newNo: 1, newText: 'A', changed: true },
      { kind: 'pair', oldNo: 2, oldText: 'b', newNo: 2, newText: 'B', changed: true },
    ]);
  });
});

describe('gaps', () => {
  test('the unchanged lines before the first hunk become one gap row', () => {
    const { rows } = layoutDiff(diff(hunk(12, 12, '-a\n+A')));

    expect(rows[0]).toEqual({ kind: 'gap', skipped: 11 });
  });

  test('a hunk starting at line 1 has no leading gap', () => {
    const { rows } = layoutDiff(diff(hunk(1, 1, '-a\n+A')));

    expect(rows[0]).toMatchObject({ kind: 'pair' });
  });

  test('the distance between two hunks is measured, not assumed', () => {
    const { rows } = layoutDiff(diff(hunk(1, 1, ' a\n-b\n+B'), hunk(20, 20, '-x\n+X')));

    expect(rows.filter((row) => row.kind === 'gap')).toEqual([{ kind: 'gap', skipped: 17 }]);
  });

  /** A new file's only hunk is `@@ -0,0 +1,n @@`: the old side never had a
   *  line 1, so measuring the gap from it would report a negative skip. */
  test('a new file opens with no gap', () => {
    const whole: DiffHunk = {
      oldStart: 0,
      oldCount: 0,
      newStart: 1,
      newCount: 2,
      lines: lines('+one\n+two'),
    };

    const { rows } = layoutDiff(diff(whole));

    expect(rows).toEqual([
      { kind: 'pair', oldNo: null, oldText: null, newNo: 1, newText: 'one', changed: true },
      { kind: 'pair', oldNo: null, oldText: null, newNo: 2, newText: 'two', changed: true },
    ]);
  });
});

describe('width', () => {
  /** Virtualized rows make `max-content` meaningless — it would be computed
   *  from whichever rows happen to be mounted, so the horizontal scroll extent
   *  would shift while scrolling vertically. */
  test('the longest line in the whole diff is reported, not the longest visible one', () => {
    const { maxWidth } = layoutDiff(diff(hunk(1, 1, ' short\n-a much longer line here\n+x')));

    expect(maxWidth).toBe('a much longer line here'.length);
  });

  test('an empty diff lays out to nothing', () => {
    expect(layoutDiff(diff())).toEqual({ rows: [], maxWidth: 0 });
  });
});
