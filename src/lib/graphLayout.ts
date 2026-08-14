/**
 * Lane assignment for the commit graph (SPEC.md §5.3, §8.4).
 *
 * "Lane layout is implemented in-house. Do not parse `git log --graph` ASCII
 * output" — this is that layout. It is pure and framework-free so it can run
 * incrementally as pages load, independent of virtualization or rendering.
 *
 * `git log --all --date-order` guarantees no parent is shown before all of
 * its children, so by the time a commit is processed, every lane that could
 * be waiting for it already is. Each lane tracks the hash it is "waiting"
 * for (its next commit); a commit either lands in the lane already waiting
 * for it, or opens a new one.
 */

/** Must match app.css's `--row-height` — an SVG viewBox can't read a CSS
 *  custom property, and virtualization math needs the same number. */
export const ROW_HEIGHT = 30;
export const LANE_WIDTH = 16;

export interface Commit {
  hash: string;
  parents: string[];
  timestamp: number;
  author: string;
  subject: string;
}

export type EdgeKind = 'through' | 'merge-in' | 'branch-out';

/** A line segment drawn in a row, in addition to the commit's own straight
 *  vertical (top→dot→bottom in its own lane), which every row implicitly
 *  carries via `hasIncomingSameLane`/`hasOutgoingSameLane` below. */
export interface Edge {
  /** The *other* lane this edge touches — never the row's own lane. */
  lane: number;
  kind: EdgeKind;
}

export interface RowLayout {
  commit: Commit;
  /** This commit's own column. */
  lane: number;
  edges: Edge[];
  /** A line should be drawn from the top edge down to this row's dot. False
   *  only when nothing was ever waiting for this hash — the very top of a
   *  branch nothing else has referenced yet. */
  hasIncomingSameLane: boolean;
  /** A line should be drawn from this row's dot down to the bottom edge —
   *  true whenever the commit has a first parent, whether or not that parent
   *  has loaded yet (a later page, or none — the line simply runs off the
   *  last loaded row). */
  hasOutgoingSameLane: boolean;
}

/** `lanes[i]` is the hash lane `i` is waiting to draw down to, or `null` if
 *  the lane is free. Carried across "Load more" calls so layout resumes
 *  exactly where the previous page left off. */
export interface LaneState {
  lanes: (string | null)[];
}

export function emptyLaneState(): LaneState {
  return { lanes: [] };
}

export function layoutRows(commits: Commit[], state: LaneState): { rows: RowLayout[]; state: LaneState } {
  const lanes = state.lanes.slice();
  const rows: RowLayout[] = [];

  for (const commit of commits) {
    const waitingLanes: number[] = [];
    for (let i = 0; i < lanes.length; i++) {
      if (lanes[i] === commit.hash) waitingLanes.push(i);
    }

    const lane = waitingLanes.length > 0 ? waitingLanes[0] : firstFreeLane(lanes);
    const edges: Edge[] = [];

    // Every other occupied lane not involved with this commit runs straight
    // through the row untouched.
    for (let i = 0; i < lanes.length; i++) {
      if (i === lane || waitingLanes.includes(i) || lanes[i] === null) continue;
      edges.push({ lane: i, kind: 'through' });
    }

    // Extra lanes that were also waiting for this hash converge into it —
    // two branches meeting at a shared ancestor — and are freed.
    for (const waiting of waitingLanes) {
      if (waiting === lane) continue;
      edges.push({ lane: waiting, kind: 'merge-in' });
      lanes[waiting] = null;
    }

    const [first, ...rest] = commit.parents;
    lanes[lane] = first ?? null;

    // Additional parents (a merge commit) each open their own lane, diverging
    // from this row's dot.
    for (const parent of rest) {
      const parentLane = firstFreeLane(lanes, lane);
      lanes[parentLane] = parent;
      edges.push({ lane: parentLane, kind: 'branch-out' });
    }

    rows.push({
      commit,
      lane,
      edges,
      hasIncomingSameLane: waitingLanes.length > 0,
      hasOutgoingSameLane: first !== undefined,
    });
  }

  return { rows, state: { lanes } };
}

function firstFreeLane(lanes: (string | null)[], avoid = -1): number {
  for (let i = 0; i < lanes.length; i++) {
    if (i !== avoid && lanes[i] === null) return i;
  }
  lanes.push(null);
  return lanes.length - 1;
}

/** Widest lane column touched by any row — sizes the fixed-width lanes SVG. */
export function laneCount(rows: RowLayout[]): number {
  let max = 0;
  for (const row of rows) {
    max = Math.max(max, row.lane);
    for (const edge of row.edges) max = Math.max(max, edge.lane);
  }
  return max + 1;
}

/** Cycled by lane index (§11) — never introduces a ninth hue, just repeats. */
export function laneColorVar(lane: number): string {
  return `var(--lane-${(lane % 8) + 1})`;
}
