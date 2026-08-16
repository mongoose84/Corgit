import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { inTauri } from './tauri';
import { layoutDiff, type DiffLayout, type FileDiff } from './diffLayout';

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
}

/** A commit is immutable, so its diff never needs re-reading; everything else
 *  is a view onto the working tree or the index, both of which a write moves. */
function isLive(source: DiffSource): boolean {
  return source.kind !== 'commit';
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
      if (open && open.repoId === event.payload.repoId && isLive(open.source)) {
        void this.reload();
      }
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
