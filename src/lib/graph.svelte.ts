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
  /** The tip commit's `%ct`, and `subject` its first line — both carried for
   *  the branch search (§5.3), which describes branches whose tip is not among
   *  the loaded rows and so cannot look either up from `rows`. */
  timestamp: number;
  subject: string;
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

/**
 * How many extra pages one branch-search jump may read before giving up
 * (§5.3). Ten pages is 3000 commits, and the number is a *time* budget wearing
 * a commit count: the pages are sequential — each one's `skip` is the row count
 * the last produced — so this is ten spawns end to end, ~85 ms each on the
 * bench machine before git does any work. A second of chasing is defensible for
 * something the user explicitly asked for; a minute of it, which is what an
 * uncapped walk costs on a monorepo whose release branch is 40k commits back,
 * is not. Past the cap the pane says how far it looked and leaves *Load more*
 * where it was, which is the same answer at the user's own pace.
 */
const REVEAL_PAGE_LIMIT = 10;

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

  /** Whether the info column is on screen (§5.2). Deliberately *not* derived
   *  from `selection`: selecting a row is how you read the graph, and a
   *  320 px column that appears and reflows the pane every time you click a
   *  commit is not something the user asked for each time. It opens only from
   *  a row's right-click ▸ Info, and once open it follows the selection like
   *  any other detail view. Nothing is fetched while it is shut. */
  infoOpen = $state(false);

  /** The selected commit's details (§5.2 Mode B) — `null` while the info
   *  column is shut, in working-tree mode, or before the fetch lands. */
  details = $state<CommitDetails | null>(null);
  loadingDetails = $state(false);
  detailsError = $state<string | null>(null);

  /** The hash `details` is showing *or fetching*. Not `details.hash`, which
   *  is null for the whole of a fetch — this is what makes a second ask for
   *  the same commit inert rather than a re-spawn. */
  private detailsFor: string | null = null;

  private laneState: LaneState = emptyLaneState();

  /** Bumped by every first-page load; a response carrying a stale token is
   *  dropped. Two `reload`s really do overlap in normal use — a commit or a
   *  pull emits `status:repo` from `write_and_refresh`, and the hot repo's FS
   *  watcher emits a second one ~200 ms later (§6), both while `graph_page` is
   *  still running. Without this the later one read `this.laneState` back
   *  *after* its await, by which point the earlier reload had published the
   *  lane state left over at the bottom of its page, and laid page 1 out as if
   *  it continued those lanes: every still-open lane drew a `through` line up
   *  and off the top of a graph with nothing above it. */
  private loadToken = 0;

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
    this.infoOpen = false;
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
    this.infoOpen = false;
    this.clearDetails();
    this.laneState = emptyLaneState();
  }

  /** Which row is highlighted (§5.3). On its own this paints a row and
   *  nothing else — no fetch, no column — which is what makes clicking down
   *  a graph cost nothing.
   *
   *  `working-tree` additionally shuts the info column: the *Uncommitted
   *  Changes* node means "back to the working tree", and there is no commit
   *  left for the column to be about. */
  select(selection: GraphSelection): void {
    this.selection = selection;
    if (selection === 'working-tree') this.infoOpen = false;
    this.syncDetails();
  }

  /** Right-click a row ▸ Info (§5.2) — the only way the column opens. */
  showInfo(hash: string): void {
    this.selection = hash;
    this.infoOpen = true;
    this.syncDetails();
  }

  closeInfo(): void {
    this.infoOpen = false;
    this.syncDetails();
  }

  /** The single place that decides what the column should be showing, run
   *  after every change to either input. Asking twice for the same commit is
   *  inert: `loadDetails` nulls `details` *before* it fetches, so a repeat
   *  would flash the panel back to "Reading commit…", throw away its scroll
   *  position and shell out to git again for a commit that cannot have
   *  changed. (Immutability is the same reason `diff.svelte.ts`'s `isLive`
   *  never reloads a commit's diff.) A failed fetch is the one case where
   *  asking again means something, so it retries. */
  private syncDetails(): void {
    const hash = this.selection;
    if (!this.infoOpen || hash === 'working-tree') {
      this.clearDetails();
      return;
    }
    if (this.detailsFor === hash && this.detailsError === null) return;
    void this.loadDetails(hash);
  }

  private clearDetails(): void {
    this.detailsFor = null;
    this.details = null;
    this.loadingDetails = false;
    this.detailsError = null;
  }

  private async loadDetails(hash: string): Promise<void> {
    const id = this.repoId;
    if (!id) return;

    this.detailsFor = hash;
    this.loadingDetails = true;
    this.details = null;
    this.detailsError = null;
    try {
      const details = await invoke<CommitDetails>('commit_details', { repoId: id, hash });
      // The panel moved on — or shut — while this was in flight.
      if (this.repoId !== id || this.detailsFor !== hash) return;
      this.details = details;
    } catch (err) {
      if (this.repoId !== id || this.detailsFor !== hash) return;
      this.detailsError = String(err);
    } finally {
      if (this.repoId === id && this.detailsFor === hash) this.loadingDetails = false;
    }
  }

  /** Re-reads the first page and refs for the current repo — after a
   *  mutating op lands, or right after `loadFor`. Discards any pages loaded
   *  past the first, the same way a fresh commit resets what "top of the
   *  graph" means. */
  private async reload(): Promise<void> {
    const id = this.repoId;
    if (!id) return;
    const token = ++this.loadToken;

    this.loading = true;
    try {
      const [page, refs] = await Promise.all([
        invoke<GraphPage>('graph_page', { repoId: id, skip: 0 }),
        invoke<RefBadge[]>('graph_refs', { repoId: id }),
      ]);
      if (this.repoId !== id || this.loadToken !== token) return;

      // Always from an empty state, never from `this.laneState`: a first page
      // continues nothing, and reading the field back after an await is what
      // let a concurrent reload's leftovers become this page's phantom lanes.
      const laid = layoutRows(page.commits, emptyLaneState());
      this.rows = laid.rows;
      this.laneState = laid.state;
      this.hasMore = page.hasMore;
      this.refs = refs;
      this.error = null;
    } catch (err) {
      if (this.repoId !== id || this.loadToken !== token) return;
      this.error = String(err);
    } finally {
      // Only the newest load owns the spinner; an outdated one clearing it
      // would say "loaded" over rows still being replaced.
      if (this.repoId === id && this.loadToken === token) this.loading = false;
    }
  }

  /** The "Load more" row (§5.3) — appends the next 300 rather than reloading
   *  what is already on screen. */
  async loadMore(): Promise<void> {
    const id = this.repoId;
    if (!id || this.loadingMore || !this.hasMore) return;

    // Both captured before the await for the same reason `reload` no longer
    // reads them after one: this page continues *these* rows, and a reload
    // landing meanwhile replaces both the rows and the lanes they left open.
    const token = this.loadToken;
    const base = this.laneState;

    this.loadingMore = true;
    try {
      const page = await invoke<GraphPage>('graph_page', { repoId: id, skip: this.rows.length });
      // A reload landed while this was in flight — these commits continue rows
      // that are no longer on screen, so appending them would duplicate or
      // misplace them. The reload already published a correct first page.
      if (this.repoId !== id || this.loadToken !== token) return;

      const laid = layoutRows(page.commits, base);
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

  /** The branch the search is reading pages to reach (§5.3), `null` when no
   *  jump is running. The name rather than a boolean because the narration
   *  names it — "Reading history to reach release/R2026-08…" is the sentence
   *  that makes a two-second pause legible instead of alarming. */
  revealing = $state<string | null>(null);

  /**
   * Brings a commit into the loaded rows and returns its index, or `-1` if it
   * could not be reached (§5.3). The branch search's jump.
   *
   * A branch the user went looking for is usually one whose tip the first page
   * never reached — that is the whole shape of the feature — and there is no
   * cheaper way to get a row for it than to read the pages in between. `git
   * log` has no "which index is this commit at" to ask, and the rows have to
   * exist regardless: the list is virtualized against `rows.length`, so a
   * selection outside it is a selection with nowhere to scroll.
   *
   * Deliberately not a fresh `--ancestry-path` query for just that commit: the
   * graph's lane layout is a fold over the page sequence (`laneState`), so a
   * row spliced in out of order would draw its lanes against a state that
   * never ran.
   */
  async reveal(hash: string, label: string): Promise<number> {
    const indexOf = () => this.rows.findIndex((row) => row.commit.hash === hash);

    const already = indexOf();
    if (already !== -1) return already;

    const id = this.repoId;
    if (!id) return -1;

    this.revealing = label;
    try {
      for (let page = 0; page < REVEAL_PAGE_LIMIT; page += 1) {
        if (!this.hasMore) break;
        const before = this.rows.length;
        await this.loadMore();
        // Three bail-outs in one test, and they want to be one: the repo
        // changed under us, a `status:repo` reload replaced the rows this walk
        // was extending, or the page simply failed. In every case the rows are
        // no longer the ones the last iteration measured, and walking on would
        // be counting pages against a graph that restarted.
        if (this.repoId !== id || this.rows.length <= before) break;

        const at = indexOf();
        if (at !== -1) return at;
      }
      return -1;
    } finally {
      // Same guard as everywhere else in here: a jump that outlived its repo
      // must not clear a label belonging to the repo now on screen.
      if (this.repoId === id) this.revealing = null;
    }
  }
}

export const graph = new GraphStore();
