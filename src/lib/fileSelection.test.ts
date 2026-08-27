import { describe, expect, it } from 'vitest';

import {
  extend,
  isSelected,
  prune,
  selectOne,
  selectedRows,
  step,
  toggle,
  type FileSelection,
} from './fileSelection';

/**
 * The selection is invisible state: nothing on screen says which rows a
 * right-click is about to stage except the highlight, so an off-by-one in a
 * shift range or a path left behind after a stage would send a batch write at
 * files the user never picked. That is the class of bug these cover.
 */

const ROWS = [{ path: 'a.txt' }, { path: 'b.txt' }, { path: 'c.txt' }, { path: 'd.txt' }];

function paths(selection: FileSelection | null): string[] {
  return selection ? [...selection.paths] : [];
}

describe('toggle', () => {
  it('adds to the set and removes on a second click', () => {
    let selection = selectOne('unstaged', 'a.txt');
    selection = toggle(selection, 'unstaged', 'c.txt')!;
    expect(paths(selection).sort()).toEqual(['a.txt', 'c.txt']);

    selection = toggle(selection, 'unstaged', 'a.txt')!;
    expect(paths(selection)).toEqual(['c.txt']);
  });

  it('clears rather than leaving an empty set', () => {
    expect(toggle(selectOne('staged', 'a.txt'), 'staged', 'a.txt')).toBeNull();
  });

  it('starts over when the click crosses into the other section', () => {
    const selection = toggle(selectOne('unstaged', 'a.txt'), 'staged', 'b.txt');
    expect(selection).toEqual({ section: 'staged', paths: new Set(['b.txt']), anchor: 'b.txt' });
  });
});

describe('extend', () => {
  it('spans the anchor and the click, in either direction', () => {
    const down = extend(selectOne('unstaged', 'b.txt'), 'unstaged', 'd.txt', ROWS);
    expect(paths(down)).toEqual(['b.txt', 'c.txt', 'd.txt']);

    const up = extend(selectOne('unstaged', 'd.txt'), 'unstaged', 'b.txt', ROWS);
    expect(paths(up)).toEqual(['b.txt', 'c.txt', 'd.txt']);
  });

  it('keeps the anchor so a second shift-click re-measures from it', () => {
    const first = extend(selectOne('unstaged', 'b.txt'), 'unstaged', 'd.txt', ROWS);
    const second = extend(first, 'unstaged', 'c.txt', ROWS);
    expect(paths(second)).toEqual(['b.txt', 'c.txt']);
  });

  it('falls back to a plain click when the anchor is gone from the list', () => {
    const stale: FileSelection = { section: 'unstaged', paths: new Set(['x.txt']), anchor: 'x.txt' };
    expect(paths(extend(stale, 'unstaged', 'c.txt', ROWS))).toEqual(['c.txt']);
  });
});

describe('prune', () => {
  it('drops paths the list no longer has', () => {
    const selection = extend(selectOne('unstaged', 'a.txt'), 'unstaged', 'c.txt', ROWS);
    expect(paths(prune(selection, [{ path: 'b.txt' }, { path: 'd.txt' }]))).toEqual(['b.txt']);
  });

  it('clears once the whole selection has been staged away', () => {
    const selection = selectOne('unstaged', 'a.txt');
    expect(prune(selection, [{ path: 'b.txt' }])).toBeNull();
    expect(prune(selection, undefined)).toBeNull();
  });

  it('moves the anchor onto a surviving row', () => {
    const selection = extend(selectOne('unstaged', 'a.txt'), 'unstaged', 'b.txt', ROWS);
    expect(prune(selection, [{ path: 'b.txt' }])?.anchor).toBe('b.txt');
  });

  it('leaves an untouched selection identical, so nothing re-renders', () => {
    const selection = selectOne('unstaged', 'a.txt');
    expect(prune(selection, ROWS)).toBe(selection);
  });
});

describe('reading a selection', () => {
  const selection = extend(selectOne('unstaged', 'a.txt'), 'unstaged', 'c.txt', ROWS);

  it('never reports a row in the other section', () => {
    expect(isSelected(selection, 'unstaged', 'a.txt')).toBe(true);
    expect(isSelected(selection, 'staged', 'a.txt')).toBe(false);
    expect(selectedRows(selection, 'staged', ROWS)).toEqual([]);
  });

  it('returns the rows in list order, not click order', () => {
    const clicked = toggle(selectOne('unstaged', 'd.txt'), 'unstaged', 'a.txt')!;
    expect(selectedRows(clicked, 'unstaged', ROWS).map((row) => row.path)).toEqual([
      'a.txt',
      'd.txt',
    ]);
  });
});

describe('step', () => {
  const STAGED = [{ path: 's1.txt' }, { path: 's2.txt' }];

  /** `section/path`, because a step that lands on the right filename in the
   *  wrong section asks git for the wrong diff and looks correct doing it. */
  function at(target: ReturnType<typeof step> | null): string | null {
    return target && `${target.section}/${target.row.path}`;
  }

  it('walks the two sections as one list, in the order they are drawn', () => {
    expect(at(step(selectOne('staged', 's2.txt'), 1, STAGED, ROWS))).toBe('unstaged/a.txt');
    expect(at(step(selectOne('unstaged', 'a.txt'), -1, STAGED, ROWS))).toBe('staged/s2.txt');
  });

  it('clamps at both ends rather than wrapping', () => {
    expect(step(selectOne('unstaged', 'd.txt'), 1, STAGED, ROWS)).toBeNull();
    expect(step(selectOne('staged', 's1.txt'), -1, STAGED, ROWS)).toBeNull();
  });

  it('enters at the near end when nothing is picked', () => {
    expect(at(step(null, 1, STAGED, ROWS))).toBe('staged/s1.txt');
    expect(at(step(null, -1, STAGED, ROWS))).toBe('unstaged/d.txt');
    expect(step(null, 1, [], [])).toBeNull();
  });

  it('enters at the near end when the selection has been staged away', () => {
    const stale: FileSelection = { section: 'unstaged', paths: new Set(['x.txt']), anchor: 'x.txt' };
    expect(at(step(stale, 1, STAGED, ROWS))).toBe('staged/s1.txt');
  });

  it('steps past a range rather than back into the middle of it', () => {
    const range = extend(selectOne('unstaged', 'b.txt'), 'unstaged', 'c.txt', ROWS);
    expect(at(step(range, 1, STAGED, ROWS))).toBe('unstaged/d.txt');
    expect(at(step(range, -1, STAGED, ROWS))).toBe('unstaged/a.txt');
  });

  it('measures a scattered ctrl-selection from its outermost row', () => {
    const scattered = toggle(selectOne('unstaged', 'a.txt'), 'unstaged', 'c.txt')!;
    expect(at(step(scattered, 1, STAGED, ROWS))).toBe('unstaged/d.txt');
    expect(at(step(scattered, -1, STAGED, ROWS))).toBe('staged/s2.txt');
  });

  it('ignores a same-named row in the other section', () => {
    const both = [{ path: 'a.txt' }];
    expect(at(step(selectOne('staged', 'a.txt'), 1, both, ROWS))).toBe('unstaged/a.txt');
  });
});
