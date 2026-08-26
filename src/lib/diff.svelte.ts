import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { inTauri } from './tauri';
import { layoutDiff, type DiffLayout, type FileDiff } from './diffLayout';
import type { FileChanges, FileEntry } from './repos.svelte';

/**
 * The right pane's second view (SPEC.md §5.4).
 *
 * Same shape as `graph.svelte.ts`: Rust owns the git data, this copies what
 * arrives and derives the side-by-side layout from it. Selected repo, one file
 * at a time — a diff is never fetched for a file nobody is looking at.
 */

/** Which two sides to compare — mirrors `DiffSource` in `diff.rs`.
 *
 *  It comes from the *section* a row was clicked in rather than from the file
 *  itself, because that is the only thing that knows: the same path can sit in
 *  both *Staged Changes* and *Changes* with a different diff on each side. */
export type DiffSource =
  | { kind: 'unstaged' }
  | { kind: 'staged' }
  | { kind: 'untracked' }
  | { kind: 'commit'; hash: string };

export interface OpenDiff {
  repoId: string;
  path: string;
  source: DiffSource;
}

/** Which of the right pane's two views the tab strip has selected (§5.4). */
export type PaneView = 'graph' | 'diff';

/** Mirrors `RepoStatusEvent` in repos.svelte.ts — the same single-repo event a
 *  stage/unstage/commit emits, reused here to re-read an open working-tree
 *  diff immediately rather than leaving it describing a stale index. */
interface RepoStatusEvent {
  root: string;
  repoId: string;
  /** The middle pane's rows, carried only for the selected repo (§5.2) and
   *  `null` on every other event. Its absence is "nothing to decide from",
   *  never "the file list is empty" — see `reconcile`. */
  files: FileChanges | null;
}

/** Which two sides a middle-pane row compares (§5.2, §5.4). It has to come from
 *  the section rather than from the entry: the same path sits in both lists
 *  whenever a file is partly staged, with a different diff on each side. An
 *  untracked file has no other side at all, so it gets its own source rather
 *  than a `git diff` that would correctly report nothing.
 *
 *  Shared with `sourcesFor` below rather than left inline in the pane — the row
 *  a click opens and the row `reconcile` looks for afterwards must be the same
 *  row, and two copies of this are two chances for them to disagree. */
export function sourceForRow(section: 'staged' | 'unstaged', entry: FileEntry): DiffSource {
  if (section === 'staged') return { kind: 'staged' };
  return entry.status === '?' ? { kind: 'untracked' } : { kind: 'unstaged' };
}

/** Where the middle pane lists this path now, as the sources a row click would
 *  have produced. Empty means the file has no row at all any more.
 *
 *  A partly-staged file is in both lists with a different diff on each side, so
 *  this returns both. Staged is first because the one case where the open side
 *  is neither of them is an untracked file that was just `git add`ed and then
 *  edited again: the staged row holds the whole file, which is what the tab was
 *  showing, and the unstaged one holds only the edit since.
 *
 *  `files.conflicted` is deliberately not consulted. Those are not rows in the
 *  pane (§13 sends conflicts to VS Code instead), and the rule this serves is
 *  about rows. */
export function sourcesFor(files: FileChanges, path: string): DiffSource[] {
  const sources: DiffSource[] = [];
  for (const entry of files.staged) {
    if (entry.path === path) sources.push(sourceForRow('staged', entry));
  }
  for (const entry of files.unstaged) {
    if (entry.path === path) sources.push(sourceForRow('unstaged', entry));
  }
  return sources;
}

/** A commit is immutable, so its diff never needs re-reading; everything else
 *  is a view onto the working tree or the index, both of which a write moves. */
function isLive(source: DiffSource): boolean {
  return source.kind !== 'commit';
}

/** What an open diff should do about a fresh file list — see `DiffStore.reconcile`. */
export type Reconciliation =
  | { kind: 'keep' }
  | { kind: 'close' }
  | { kind: 'repoint'; source: DiffSource };

/**
 * The tab's lifetime is the middle pane's rows (§5.2, §5.4): a live diff is a
 * view of a row, so when the row goes the tab goes with it, and when the row
 * moves section the tab follows it.
 *
 * Pure, and separate from the store, because this is the decision that can be
 * *silently wrong* in both directions — a tab closed under a reader, or one
 * left sitting over a file that is no longer there — and neither shows up as an
 * error anywhere. Same reasoning as `prune` in fileSelection.ts.
 */
export function reconcileOpen(open: OpenDiff, files: FileChanges): Reconciliation {
  // A commit's two sides are immutable; nothing a working tree does can move
  // them, and its file rows are the info panel's, not this pane's.
  if (!isLive(open.source)) return { kind: 'keep' };

  // Both lists stop at `MAX_FILES_PER_SECTION` (status.rs). Past that a path's
  // absence is not evidence that it is gone — the row may simply have been
  // pushed off the end — and closing a tab the user is reading on a guess is
  // worse than leaving one open a beat too long.
  if (files.staged.length < files.stagedTotal || files.unstaged.length < files.unstagedTotal) {
    return { kind: 'keep' };
  }

  const sources = sourcesFor(files, open.path);
  if (sources.length === 0) return { kind: 'close' };
  if (sources.some((source) => source.kind === open.source.kind)) return { kind: 'keep' };

  // Still a row, just a different section's — staged, unstaged, or added out of
  // untracked. Re-pointed rather than closed: it is the same file and, for a
  // whole-file stage, the same lines, so a tab that vanished on the + button
  // would make staging look like it lost something. The column captions change
  // to `HEAD`/`Index`, which is the view saying what it swapped to.
  return { kind: 'repoint', source: sources[0] };
}

class DiffStore {
  /** `null` when no file is open — the Diff tab is absent entirely then. */
  open = $state<OpenDiff | null>(null);
  /**
   * The tab strip's selection. Deliberately independent of `open`: switching
   * back to the graph must not discard the diff, or the tab it would return
   * through disappears as it is clicked.
   */
  view = $state<PaneView>('graph');

  file = $state<FileDiff | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);

  layout = $derived.by<DiffLayout>(() =>
    this.file ? layoutDiff(this.file) : { rows: [], maxWidth: 0 },
  );

  async start(): Promise<void> {
    if (!inTauri) return;
    await listen<RepoStatusEvent>('status:repo', (event) => {
      const open = this.open;
      if (!open || open.repoId !== event.payload.repoId || !isLive(open.source)) return;
      // Reconcile first: a re-read of a file that is no longer there returns a
      // perfectly valid empty diff, and rendering that is how the tab ends up
      // sitting over "No changes" after a discard or a commit.
      const files = event.payload.files;
      if (files && this.reconcile(files)) return;
      void this.reload();
    });
  }

  /** Clicking a file row (§5.2, §5.4) — opens it *and* brings the view
   *  forward, since a diff opening behind the graph would be invisible. */
  show(repoId: string, path: string, source: DiffSource): void {
    this.open = { repoId, path, source };
    this.view = 'diff';
    void this.reload();
  }

  /** The tab strip. Selecting *Graph* keeps `open` so the Diff tab stays. */
  select(view: PaneView): void {
    if (view === 'diff' && this.open === null) return;
    this.view = view;
  }

  /** The Diff tab's × and Esc, and any repo change — the file named here may
   *  not even exist in the newly selected repo. */
  close(): void {
    this.open = null;
    this.file = null;
    this.loading = false;
    this.error = null;
    this.view = 'graph';
  }

  /** Whether a given row is the one currently open, so the file lists can mark
   *  it. Shared rather than re-derived per list, the same reasoning as
   *  `isDirty`/`needsPublish` in repos.svelte.ts. */
  isOpen(repoId: string, path: string, source: DiffSource): boolean {
    const open = this.open;
    if (!open) return false;
    if (open.repoId !== repoId || open.path !== path) return false;
    if (open.source.kind !== source.kind) return false;
    if (open.source.kind === 'commit' && source.kind === 'commit') {
      return open.source.hash === source.hash;
    }
    return true;
  }

  /**
   * Applies `reconcileOpen` to a fresh file list.
   *
   * Without it, discarding or committing the open file leaves the tab selected
   * over an empty diff — git compares the two sides, finds them identical, and
   * says so, which is true and useless. Keeping the last content instead was
   * the other option and is worse: a working-tree diff that no longer describes
   * the working tree, with nothing on screen saying so.
   *
   * Returns whether it handled the event, so the caller's plain re-read is
   * skipped for a tab that just closed or re-pointed.
   */
  private reconcile(files: FileChanges): boolean {
    const open = this.open;
    if (!open) return true;

    const next = reconcileOpen(open, files);
    switch (next.kind) {
      case 'keep':
        return false;
      case 'close':
        this.close();
        return true;
      case 'repoint':
        this.open = { ...open, source: next.source };
        void this.reload();
        return true;
    }
  }

  private async reload(): Promise<void> {
    const open = this.open;
    if (!open) return;

    this.loading = true;
    this.error = null;
    try {
      const file = await invoke<FileDiff>('file_diff', {
        repoId: open.repoId,
        path: open.path,
        source: open.source,
      });
      // The selection moved on while this was in flight.
      if (this.open !== open) return;
      this.file = file;
    } catch (err) {
      if (this.open !== open) return;
      this.file = null;
      this.error = String(err);
    } finally {
      if (this.open === open) this.loading = false;
    }
  }
}

export const diff = new DiffStore();
