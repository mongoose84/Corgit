import { describe, expect, test } from 'vitest';

import {
  emptyLaneState,
  laneColorVar,
  laneCount,
  layoutRows,
  visibleWindow,
  OVERSCAN,
  ROW_HEIGHT,
  type Commit,
  type LaneState,
} from './graphLayout';

/**
 * Lane layout is the only algorithm in the frontend that can be *silently*
 * wrong: a mis-assigned lane still renders, it just draws the wrong history.
 * There is no git error to surface and nothing throws, so these tests are the
 * only thing standing between a bad edge case and a graph that quietly lies.
 *
 * `layoutRows` is pure — `Commit[]` plus carried lane state in, rows out — so
 * none of this needs a DOM, a repo, or a mocked IPC boundary.
 */

/** Only the fields layout reads; author/subject/timestamp never affect lanes. */
function commit(hash: string, ...parents: string[]): Commit {
  return { hash, parents, timestamp: 1_700_000_000, author: 'Jeppe', subject: hash };
}

function lay(commits: Commit[], state: LaneState = emptyLaneState()) {
  return layoutRows(commits, state);
}

describe('linear history', () => {
  test('every commit stays in one lane and draws no extra edges', () => {
    const { rows } = lay([commit('a', 'b'), commit('b', 'c'), commit('c')]);

    expect(rows.map((row) => row.lane)).toEqual([0, 0, 0]);
    expect(rows.flatMap((row) => row.edges)).toEqual([]);
  });

  test('the first row has nothing above it, the rest continue a lane', () => {
    const { rows } = lay([commit('a', 'b'), commit('b', 'c')]);

    // Nothing was ever waiting for `a` — it is the tip of a branch, so no line
    // is drawn from the top edge down into it.
    expect(rows[0].hasIncomingSameLane).toBe(false);
    expect(rows[1].hasIncomingSameLane).toBe(true);
  });

  test('a root commit ends its lane rather than running off the bottom', () => {
    const { rows } = lay([commit('a', 'b'), commit('b')]);

    expect(rows[0].hasOutgoingSameLane).toBe(true);
    expect(rows[1].hasOutgoingSameLane).toBe(false);
  });

  test('a first parent that has not loaded yet still draws downward', () => {
    // The line simply runs off the last loaded row — the parent may be on the
    // next page, or nowhere at all.
    const { rows } = lay([commit('a', 'not-loaded')]);

    expect(rows[0].hasOutgoingSameLane).toBe(true);
  });
});

describe('merges and branches', () => {
  test('a second parent opens its own lane, diverging from this row', () => {
    const { rows } = lay([commit('m', 'p1', 'p2')]);

    expect(rows[0].lane).toBe(0);
    expect(rows[0].edges).toEqual([{ lane: 1, kind: 'branch-out' }]);
  });

  test('an octopus merge opens one lane per extra parent', () => {
    const { rows } = lay([commit('m', 'p1', 'p2', 'p3')]);

    expect(rows[0].edges).toEqual([
      { lane: 1, kind: 'branch-out' },
      { lane: 2, kind: 'branch-out' },
    ]);
  });

  test('two lanes waiting on one hash converge into it and free the spare', () => {
    // Both lanes are waiting for `shared` — two branches meeting at a common
    // ancestor. It lands in the lower lane; the other merges in and is freed.
    const { rows, state } = lay([commit('shared', 'older')], { lanes: ['shared', 'shared'] });

    expect(rows[0].lane).toBe(0);
    expect(rows[0].edges).toEqual([{ lane: 1, kind: 'merge-in' }]);
    expect(state.lanes).toEqual(['older', null]);
  });

  test('an unrelated occupied lane runs straight through the row', () => {
    const { rows } = lay([commit('a', 'a-parent')], { lanes: ['a', 'unrelated'] });

    expect(rows[0].edges).toEqual([{ lane: 1, kind: 'through' }]);
  });

  test('an empty lane draws nothing at all', () => {
    // A freed lane is `null`, not merely unreferenced — it must not render as
    // a through-line, or the graph grows phantom vertical strokes.
    const { rows } = lay([commit('a', 'a-parent')], { lanes: ['a', null, 'other'] });

    expect(rows[0].edges).toEqual([{ lane: 2, kind: 'through' }]);
  });

  test('a freed lane is reused by the next branch that needs one', () => {
    const { state } = lay([commit('shared', 'older'), commit('tip', 'tip-parent')], {
      lanes: ['shared', 'shared'],
    });

    // Lane 1 was freed by the convergence above, so `tip` reuses it rather
    // than growing the graph a third column.
    expect(state.lanes).toEqual(['older', 'tip-parent']);
  });
});

describe('paging', () => {
  test('a commit resumes the lane its child left waiting across pages', () => {
    // The property that makes "Load more" correct: layout runs incrementally,
    // so page 2 must continue page 1's lanes rather than restarting at zero.
    const first = lay([commit('a', 'b')]);
    const second = layoutRows([commit('b', 'c')], first.state);

    expect(second.rows[0].lane).toBe(0);
    expect(second.rows[0].hasIncomingSameLane).toBe(true);
  });

  test('laying out a page does not mutate the state handed in', () => {
    // `loadMore` keeps its own reference; mutating it in place would corrupt
    // any retry of the same page.
    const state: LaneState = { lanes: ['a'] };
    const before = [...state.lanes];

    lay([commit('a', 'b')], state);

    expect(state.lanes).toEqual(before);
  });

  test('an empty page leaves the lanes exactly as they were', () => {
    const { rows, state } = lay([], { lanes: ['a', null] });

    expect(rows).toEqual([]);
    expect(state.lanes).toEqual(['a', null]);
  });
});

describe('lane metrics', () => {
  test('the count covers edge lanes, not just the lanes commits sit in', () => {
    // Sizing the SVG off row lanes alone would clip a merge's diverging edge.
    const { rows } = lay([commit('m', 'p1', 'p2')]);

    expect(rows[0].lane).toBe(0);
    expect(laneCount(rows)).toBe(2);
  });

  test('an empty graph still reports a lane rather than a zero-width column', () => {
    expect(laneCount([])).toBe(1);
  });

  test('colours cycle through eight hues and never invent a ninth', () => {
    expect(laneColorVar(0)).toBe('var(--lane-1)');
    expect(laneColorVar(7)).toBe('var(--lane-8)');
    expect(laneColorVar(8)).toBe('var(--lane-1)');
    expect(laneColorVar(21)).toBe('var(--lane-6)');
  });
});

describe('virtualization window', () => {
  // A 20-row viewport, the shape the pane actually runs in.
  const VIEWPORT = 20 * ROW_HEIGHT;

  test('a fresh graph starts at the first row', () => {
    const { start, end, topOffset } = visibleWindow(300, 0, VIEWPORT);

    expect(start).toBe(0);
    expect(topOffset).toBe(0);
    expect(end).toBe(20 + OVERSCAN);
  });

  test('the window follows the offset with overscan either side', () => {
    const { start, end, topOffset } = visibleWindow(300, 100 * ROW_HEIGHT, VIEWPORT);

    expect(start).toBe(100 - OVERSCAN);
    expect(topOffset).toBe((100 - OVERSCAN) * ROW_HEIGHT);
    expect(end).toBe(120 + OVERSCAN);
  });

  test('an offset left over from a longer history still draws rows', () => {
    // The bug this exists for: a rebuilt scroll box, or a page replaced under
    // a scrolled one, leaves `scrollTop` pointing past the end of the new
    // content. Unclamped, `start` ran past `rowCount` and the slice was empty
    // — a graph that drew nothing and never recovered.
    const { start, end, topOffset } = visibleWindow(12, 3000, VIEWPORT);

    expect(start).toBe(0);
    expect(topOffset).toBe(0);
    expect(end).toBe(12);
  });

  test('a history shorter than the viewport is never scrolled off the top', () => {
    // No scrollbar means no scroll event to correct a stale offset, which is
    // what made the blank graph stick.
    expect(visibleWindow(5, 900, VIEWPORT).start).toBe(0);
    expect(visibleWindow(5, 900, VIEWPORT).end).toBe(5);
  });

  test('the last row survives being scrolled to the very bottom', () => {
    // `maxScroll` deliberately ignores the "Load more" row below the spacer,
    // so the clamp undershoots by one row height; the overscan has to absorb
    // that without dropping the end of the history.
    const total = 300 * ROW_HEIGHT;
    const { end } = visibleWindow(300, total - VIEWPORT, VIEWPORT);

    expect(end).toBe(300);
  });

  test('an empty graph asks for no rows and no spacer', () => {
    expect(visibleWindow(0, 0, VIEWPORT)).toEqual({
      start: 0,
      end: 0,
      topOffset: 0,
      totalHeight: 0,
    });
  });
});
