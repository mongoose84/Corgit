import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { inTauri } from './tauri';
import {
  emptyLaneState,
  layoutRows,
  type Commit,
  type LaneState,
  type RowLayout,
} from './graphLayout';
import type { FileEntry } from './repos.svelte';

/**
 * Graph state for the selected repo (SPEC.md §5.3).
 *
 * Selected-repo only, one repo at a time (§5.3) — unlike `repos.svelte.ts`
 * this holds no data for the other 76. Mirrors that store's shape: Rust owns
 * the git data, this copies what arrives and derives lane layout from it.
 */

export type RefKind = 'local' | 'remote';

export interface RefBadge {
  name: string;
  commit: string;
  kind: RefKind;
}

interface GraphPage {
  commits: Commit[];
  hasMore: boolean;
}

/** Mirrors `RepoStatusEvent` in repos.svelte.ts — the same single-repo event
 *  a stage/unstage/commit/fetch/pull/push emits, reused here to reload the
 *  graph immediately rather than waiting on the next selection. */
interface RepoStatusEvent {
  root: string;
  repoId: string;
}

/** `working-tree` is the synthetic "Uncommitted Changes" node (§5.3); a real
 *  selection is a full commit hash. */
export type GraphSelection = 'working-tree' | string;

/** The commit info panel (§5.2 revised, §8.5) — read-only, so unlike
 *  `FileChanges` the file list here is never capped. */
export interface CommitDetails {
  hash: string;
  author: string;
  email: string;
  timestamp: number;
  message: string;
  files: CommitFileEntry[];
}

/** A changed file plus its line-change stats (GitHub-style per-file +/−).
 *  `insertions`/`deletions` are `null` for a binary file, where git reports
 *  `-` instead of a count. */
export interface CommitFileEntry extends FileEntry {
  insertions: number | null;
  deletions: number | null;
}

class GraphStore {
  repoId = $state<string | null>(null);
  rows = $state<RowLayout[]>([]);
  refs = $state<RefBadge[]>([]);
  hasMore = $state(false);
  loading = $state(false);
  loadingMore = $state(false);
  error = $state<string | null>(null);
  selection = $state<GraphSelection>('working-tree');

  /** The selected commit's details (§5.2 Mode B) — `null` in working-tree
   *  mode or before the fetch for the current selection lands. */
  details = $state<CommitDetails | null>(null);
  loadingDetails = $state(false);
  detailsError = $state<string | null>(null);

  private laneState: LaneState = emptyLaneState();

  refsByHash = $derived.by(() => {
    const map = new Map<string, RefBadge[]>();
    for (const ref of this.refs) {
      const list = map.get(ref.commit);
      if (list) list.push(ref);
      else map.set(ref.commit, [ref]);
    }
    return map;
  });

  async start(): Promise<void> {
    if (!inTauri) return;
    await listen<RepoStatusEvent>('status:repo', (event) => {
      if (event.payload.repoId === this.repoId) void this.reload();
    });
  }

  /** Repo selection changed — reset and load its first page. */
  async loadFor(repoId: string): Promise<void> {
    if (this.repoId === repoId) return;
    this.repoId = repoId;
    this.rows = [];
    this.refs = [];
    this.hasMore = false;
    this.error = null;
    this.selection = 'working-tree';
    this.clearDetails();
    this.laneState = emptyLaneState();
    await this.reload();
  }

  clear(): void {
    this.repoId = null;
    this.rows = [];
    this.refs = [];
    this.hasMore = false;
    this.error = null;
    this.selection = 'working-tree';
    this.clearDetails();
    this.laneState = emptyLaneState();
  }

  /** Drives the middle pane's mode (§5.2): `working-tree` clears Mode B's
   *  data, any other value is a commit hash whose details get fetched. */
  select(selection: GraphSelection): void {
    this.selection = selection;
    if (selection === 'working-tree') {
      this.clearDetails();
    } else {
      void this.loadDetails(selection);
    }
  }

  private clearDetails(): void {
    this.details = null;
    this.loadingDetails = false;
    this.detailsError = null;
  }

  private async loadDetails(hash: string): Promise<void> {
    const id = this.repoId;
    if (!id) return;

    this.loadingDetails = true;
    this.details = null;
    this.detailsError = null;
    try {
      const details = await invoke<CommitDetails>('commit_details', { repoId: id, hash });
      // The selection moved on while this was in flight.
      if (this.repoId !== id || this.selection !== hash) return;
      this.details = details;
    } catch (err) {
      if (this.repoId !== id || this.selection !== hash) return;
      this.detailsError = String(err);
    } finally {
      if (this.repoId === id && this.selection === hash) this.loadingDetails = false;
    }
  }

  /** Re-reads the first page and refs for the current repo — after a
   *  mutating op lands, or right after `loadFor`. Discards any pages loaded
   *  past the first, the same way a fresh commit resets what "top of the
   *  graph" means. */
  private async reload(): Promise<void> {
    const id = this.repoId;
    if (!id) return;

    this.loading = true;
    this.laneState = emptyLaneState();
    try {
      const [page, refs] = await Promise.all([
        invoke<GraphPage>('graph_page', { repoId: id, skip: 0 }),
        invoke<RefBadge[]>('graph_refs', { repoId: id }),
      ]);
      if (this.repoId !== id) return;

      const laid = layoutRows(page.commits, this.laneState);
      this.rows = laid.rows;
      this.laneState = laid.state;
      this.hasMore = page.hasMore;
      this.refs = refs;
      this.error = null;
    } catch (err) {
      if (this.repoId !== id) return;
      this.error = String(err);
    } finally {
      if (this.repoId === id) this.loading = false;
    }
  }

  /** The "Load more" row (§5.3) — appends the next 300 rather than reloading
   *  what is already on screen. */
  async loadMore(): Promise<void> {
    const id = this.repoId;
    if (!id || this.loadingMore || !this.hasMore) return;

    this.loadingMore = true;
    try {
      const page = await invoke<GraphPage>('graph_page', { repoId: id, skip: this.rows.length });
      if (this.repoId !== id) return;

      const laid = layoutRows(page.commits, this.laneState);
      this.rows = [...this.rows, ...laid.rows];
      this.laneState = laid.state;
      this.hasMore = page.hasMore;
      this.error = null;
    } catch (err) {
      if (this.repoId === id) this.error = String(err);
    } finally {
      if (this.repoId === id) this.loadingMore = false;
    }
  }
}

export const graph = new GraphStore();
