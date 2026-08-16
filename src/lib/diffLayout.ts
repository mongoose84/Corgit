/**
 * Unified hunks → two aligned columns (SPEC.md §5.4).
 *
 * The mirror of `graphLayout.ts`: Rust parses git's output (`diff.rs`), and the
 * part that only exists because of how it gets drawn lives here, beside the
 * component that draws it and testable without a browser.
 *
 * There is no diff *algorithm* here and there must not be one — git already
 * decided which lines changed. All this does is decide which left row sits
 * opposite which right row.
 */

/** Git's own leading character, transcribed rather than interpreted. */
export interface DiffLine {
  kind: ' ' | '+' | '-';
  text: string;
}

export interface DiffHunk {
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  lines: DiffLine[];
}

/** `FileDiff` in `diff.rs`. */
export interface FileDiff {
  path: string;
  hunks: DiffHunk[];
  /** Nothing to render side by side — the view offers VS Code instead. */
  binary: boolean;
  /** Hit the backend's line cap: the hunks are real but stop early. */
  truncated: boolean;
  insertions: number;
  deletions: number;
}

/**
 * One rendered row. A `gap` stands in for the unchanged lines between two
 * hunks — git never sent them, so the row says how many rather than pretending
 * the file is contiguous.
 *
 * A `pair` carries both sides at once because they share a row element: that
 * is what makes the two columns scroll together with no synchronisation code,
 * and what keeps row height uniform enough to virtualize.
 */
export type DiffRow =
  | { kind: 'gap'; skipped: number }
  | {
      kind: 'pair';
      /** `null` on the side that has no line here — a filler cell. */
      oldNo: number | null;
      oldText: string | null;
      newNo: number | null;
      newText: string | null;
      /** False for a context line, where both sides are the same text. */
      changed: boolean;
    };

export interface DiffLayout {
  rows: DiffRow[];
  /**
   * The longest rendered line, in characters. The scroll container needs an
   * explicit width: virtualized rows mean `max-content` is computed from
   * whichever rows happen to be mounted, so the horizontal scroll extent would
   * shift under the user as they scroll vertically.
   */
  maxWidth: number;
}

/** Uniform, and tighter than `--row-height` — this is code, not a list row. */
export const DIFF_ROW_HEIGHT = 20;

interface Numbered {
  no: number;
  text: string;
}

export function layoutDiff(diff: FileDiff): DiffLayout {
  const rows: DiffRow[] = [];
  let maxWidth = 0;
  // The last line number actually rendered on each side, so a gap can be
  // measured without knowing how long the file is.
  let lastOld = 0;
  let lastNew = 0;

  const widen = (text: string) => {
    if (text.length > maxWidth) maxWidth = text.length;
  };

  for (const hunk of diff.hunks) {
    // Measured on both sides and the larger taken: a hunk that only adds lines
    // has an old side that did not move, and vice versa.
    const skipped = Math.max(hunk.oldStart - lastOld - 1, hunk.newStart - lastNew - 1);
    if (skipped > 0) rows.push({ kind: 'gap', skipped });

    let oldNo = hunk.oldStart;
    let newNo = hunk.newStart;
    let removed: Numbered[] = [];
    let added: Numbered[] = [];

    const flush = () => {
      if (removed.length === 0 && added.length === 0) return;
      // Zipped by position, which is the whole alignment rule: git decided a
      // run of removals is replaced by a run of additions, and the shorter side
      // simply runs out. Anything cleverer would be a second diff algorithm
      // disagreeing with the one that produced this patch.
      const height = Math.max(removed.length, added.length);
      for (let i = 0; i < height; i += 1) {
        const left = removed[i];
        const right = added[i];
        rows.push({
          kind: 'pair',
          oldNo: left?.no ?? null,
          oldText: left?.text ?? null,
          newNo: right?.no ?? null,
          newText: right?.text ?? null,
          changed: true,
        });
      }
      removed = [];
      added = [];
    };

    for (const line of hunk.lines) {
      widen(line.text);
      if (line.kind === '-') {
        // A removal after additions ends the previous block — otherwise
        // `-a +A -b +B` would collapse into one four-row block whose halves
        // no longer line up.
        if (added.length > 0) flush();
        removed.push({ no: oldNo, text: line.text });
        oldNo += 1;
      } else if (line.kind === '+') {
        added.push({ no: newNo, text: line.text });
        newNo += 1;
      } else {
        flush();
        rows.push({
          kind: 'pair',
          oldNo,
          oldText: line.text,
          newNo,
          newText: line.text,
          changed: false,
        });
        oldNo += 1;
        newNo += 1;
      }
    }
    flush();

    // Clamped forward: a hunk with an empty old side (a new file) leaves
    // `oldNo` at the header's 0, and a gap must never be measured from -1.
    lastOld = Math.max(lastOld, oldNo - 1);
    lastNew = Math.max(lastNew, newNo - 1);
  }

  return { rows, maxWidth };
}
