/**
 * Multi-select for the middle pane's file lists (SPEC.md §5.2).
 *
 * Kept here rather than inline in `CommitPane` because the interesting part is
 * not the rendering: it is what a modifier click does to a set, which is the
 * part that is worth testing and the part that got the pane's earlier tick
 * column removed. The pane holds one value of this type; every click replaces
 * it, so there is no half-updated selection to reason about.
 *
 * **A selection belongs to one section.** *Staged* and *Changes* answer
 * different questions — `−` versus `+`, discard applying to one and not the
 * other — so a set spanning both would leave the context menu with no honest
 * verb to offer. Ctrl-clicking across the divide starts a new selection rather
 * than growing the old one.
 */

export type FileSection = 'staged' | 'unstaged';

/** Anything with a path — `FileEntry` and `CommitFileEntry` both qualify, and
 *  nothing here needs the status letter. */
export interface PathRow {
  path: string;
}

export interface FileSelection {
  section: FileSection;
  paths: ReadonlySet<string>;
  /** The row a shift-click measures its range from — the last row the user
   *  picked deliberately, not the last one to end up in the set. */
  anchor: string;
}

/** Plain click: the selection becomes exactly this row. */
export function selectOne(section: FileSection, path: string): FileSelection {
  return { section, paths: new Set([path]), anchor: path };
}

/** Ctrl/Cmd-click: add or remove one row, keeping the rest. Removing the last
 *  member clears the selection outright rather than leaving an empty set that
 *  every caller would then have to treat as "nothing" anyway. */
export function toggle(
  current: FileSelection | null,
  section: FileSection,
  path: string,
): FileSelection | null {
  if (!current || current.section !== section) return selectOne(section, path);

  const paths = new Set(current.paths);
  if (!paths.delete(path)) paths.add(path);
  if (paths.size === 0) return null;
  // The anchor follows the click even when it removed the row: a shift-click
  // afterwards should span from where the user last was, not from wherever the
  // selection happens to have started.
  return { section, paths, anchor: path };
}

/** Shift-click: replace the selection with the run between the anchor and this
 *  row, inclusive and in either direction. Falls back to a plain click when
 *  there is nothing to measure from, which is also what happens when the
 *  anchor has since been staged away. */
export function extend(
  current: FileSelection | null,
  section: FileSection,
  path: string,
  rows: readonly PathRow[],
): FileSelection {
  if (!current || current.section !== section) return selectOne(section, path);

  const from = rows.findIndex((row) => row.path === current.anchor);
  const to = rows.findIndex((row) => row.path === path);
  if (from < 0 || to < 0) return selectOne(section, path);

  const [start, end] = from <= to ? [from, to] : [to, from];
  // The anchor is deliberately kept, not moved to `path`: dragging the shift
  // key up and down a list must keep measuring from the same end.
  return {
    section,
    paths: new Set(rows.slice(start, end + 1).map((row) => row.path)),
    anchor: current.anchor,
  };
}

/** Drop paths the file list no longer has (§7 — a stage, a commit, an FS
 *  watcher tick or another window can all rewrite it underneath us), and with
 *  them the whole selection once nothing is left. Staging the selection is the
 *  ordinary case: the rows move to the other section and this clears itself,
 *  which is what the user means by "and then stage them".
 *
 *  An anchor that has gone is replaced rather than kept, so a later shift-click
 *  extends from a row that is actually on screen. */
export function prune(
  current: FileSelection | null,
  rows: readonly PathRow[] | undefined,
): FileSelection | null {
  if (!current) return null;

  const live = new Set(rows?.map((row) => row.path) ?? []);
  const paths = new Set([...current.paths].filter((path) => live.has(path)));
  if (paths.size === 0) return null;
  if (paths.size === current.paths.size && live.has(current.anchor)) return current;

  const anchor = live.has(current.anchor) ? current.anchor : [...paths][0];
  return { section: current.section, paths, anchor };
}

export function isSelected(
  current: FileSelection | null,
  section: FileSection,
  path: string,
): boolean {
  return current !== null && current.section === section && current.paths.has(path);
}

/** The rows a menu opened in this section acts on, in list order — the order
 *  matters because the discard dialog lists them and a set does not promise
 *  the order they were clicked in. */
export function selectedRows<T extends PathRow>(
  current: FileSelection | null,
  section: FileSection,
  rows: readonly T[],
): T[] {
  if (!current || current.section !== section) return [];
  return rows.filter((row) => current.paths.has(row.path));
}
