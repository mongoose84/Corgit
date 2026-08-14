import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { settings } from './settings.svelte';
import { inTauri } from './tauri';

/**
 * Repo state (SPEC.md §9.3).
 *
 * Rust owns the truth. This holds no derived git state of its own — it copies
 * what arrives and renders it. Anything computed here would become a second
 * source of truth that has to be reconciled, and the cache in build step 3
 * would make it a third.
 */

export interface Repo {
  /** Canonicalised path; stable across roots that overlap. */
  id: string;
  name: string;
  path: string;
}

export interface RepoStatus {
  /** `null` when HEAD is detached. */
  branch: string | null;
  head: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  staged: number;
  unstaged: number;
  untracked: number;
  conflicted: number;
}

export interface GitInfo {
  available: boolean;
  version: string | null;
  supportsFsmonitor: boolean;
  /** The binary the sweep runs — not always the `git` on PATH. */
  readBinary: string | null;
}

/** One row in the middle pane's file lists (§5.2). */
export interface FileEntry {
  path: string;
  /** Git's own status letter (M/A/D/R/C/T/U/?), rendered as-is. */
  status: string;
}

/** The selected repo's file list — fetched on demand, never for all 77 (§5.2). */
export interface FileChanges {
  staged: FileEntry[];
  stagedTotal: number;
  unstaged: FileEntry[];
  unstagedTotal: number;
  conflicted: FileEntry[];
}

interface RootView {
  path: string;
  repos: Repo[];
  statuses: Record<string, RepoStatus>;
  errors: Record<string, string>;
}

interface SweepEvent {
  /** Echoed back so results for a replaced root can be dropped. */
  root: string;
  statuses: Record<string, RepoStatus>;
  errors: Record<string, string>;
  elapsedMs: number;
}

/** Emitted after a stage, unstage or commit lands — updates one row and the
 *  middle pane immediately rather than waiting up to 60 s for the next sweep. */
interface RepoStatusEvent {
  root: string;
  repoId: string;
  status: RepoStatus | null;
  error: string | null;
}

/** The dirty dot is one state — the row answers "does this need me?", nothing
 *  finer (§5.1). Detail belongs in the middle pane. */
export function isDirty(status: RepoStatus): boolean {
  return status.staged + status.unstaged + status.untracked + status.conflicted > 0;
}

class RepoStore {
  root = $state<string | null>(null);
  repos = $state<Repo[]>([]);
  statuses = $state<Record<string, RepoStatus>>({});
  /** Keyed by repo id. A repo whose status failed is unknown, not clean. */
  errors = $state<Record<string, string>>({});

  git = $state<GitInfo>({
    available: true,
    version: null,
    supportsFsmonitor: false,
    readBinary: null,
  });
  /** A sweep is in flight — the rows are painted but not yet filled in. */
  sweeping = $state(false);
  /** Wall clock for the last sweep, against the 300 ms budget in §1. */
  lastSweepMs = $state<number | null>(null);
  /** Set when opening a folder failed, e.g. a disconnected drive (§9.1). */
  openError = $state<string | null>(null);
  ready = $state(false);

  /**
   * Selection is a set with a v1 invariant of at most one member (§9.4). It
   * costs nothing now and keeps the v2 "one commit message across N repos"
   * from needing a rewrite.
   */
  selected = $state<Set<string>>(new Set());

  /** The selected repo's file list (§5.2) — fetched on demand, not part of
   *  the sweep. `null` before the first fetch lands or when nothing's selected. */
  files = $state<FileChanges | null>(null);
  loadingFiles = $state(false);
  filesError = $state<string | null>(null);
  /** Set when a stage/unstage/commit attempt fails; cleared on the next one. */
  writeError = $state<string | null>(null);

  get selectedId(): string | undefined {
    return this.selected.values().next().value;
  }

  select(id: string): void {
    this.selected = new Set([id]);
    void this.loadFiles();
  }

  status(id: string): RepoStatus | undefined {
    return this.statuses[id];
  }

  error(id: string): string | undefined {
    return this.errors[id];
  }

  /** Re-fetch the selected repo's file list — after selecting a repo, and
   *  after any stage/unstage/commit attempt so the pane reflects reality. */
  async loadFiles(): Promise<void> {
    const id = this.selectedId;
    if (!id) {
      this.files = null;
      this.filesError = null;
      return;
    }

    this.loadingFiles = true;
    try {
      const files = await invoke<FileChanges>('repo_files', { repoId: id });
      // The selection moved on while this was in flight.
      if (this.selectedId !== id) return;
      this.files = files;
      this.filesError = null;
    } catch (err) {
      if (this.selectedId !== id) return;
      this.files = null;
      this.filesError = String(err);
    } finally {
      if (this.selectedId === id) this.loadingFiles = false;
    }
  }

  async stagePaths(paths: string[]): Promise<boolean> {
    return this.write('stage_paths', { paths });
  }

  async unstagePaths(paths: string[]): Promise<boolean> {
    return this.write('unstage_paths', { paths });
  }

  async stageAll(): Promise<boolean> {
    return this.write('stage_all', {});
  }

  async unstageAll(): Promise<boolean> {
    return this.write('unstage_all', {});
  }

  async commit(message: string): Promise<boolean> {
    return this.write('commit_repo', { message });
  }

  /** Every mutating command shares this shape: resolve the selected repo,
   *  invoke, refresh the file list, surface a failure in `writeError`. The
   *  row/status side updates itself via the `status:repo` event (§7). Returns
   *  whether it succeeded, so e.g. the commit box only clears on success. */
  private async write(command: string, args: Record<string, unknown>): Promise<boolean> {
    const id = this.selectedId;
    if (!id) return false;

    this.writeError = null;
    try {
      await invoke(command, { repoId: id, ...args });
      return true;
    } catch (err) {
      this.writeError = String(err);
      return false;
    } finally {
      await this.loadFiles();
    }
  }

  /**
   * Startup (§9.1): reopen the last root if it still exists, otherwise leave
   * `root` null so the welcome screen shows. Never an empty repo list.
   */
  async start(): Promise<void> {
    if (!inTauri) {
      this.ready = true;
      return;
    }

    try {
      this.git = await invoke<GitInfo>('git_info');
      await listen<SweepEvent>('status:sweep', (event) => this.applySweep(event.payload));
      await listen<RepoStatusEvent>('status:repo', (event) => this.applyRepoStatus(event.payload));

      // A reload lands here with the backend's root still open; reuse it
      // rather than sweeping again.
      const current = await invoke<RootView | null>('current_root');
      if (current) {
        this.applyRoot(current);
      } else {
        const initial = await invoke<string | null>('initial_root');
        if (initial) await this.open(initial);
      }
    } catch (err) {
      console.warn('twogit: could not restore the last folder', err);
    } finally {
      this.ready = true;
    }
  }

  /** File → Open Folder…, which replaces the root in this window (§9.1). */
  async openFolder(): Promise<void> {
    if (!inTauri) {
      this.openError = 'Opening a folder needs the desktop app — run `npm run tauri:dev`.';
      return;
    }

    const picked = await invoke<string | null>('pick_root');
    if (picked) await this.open(picked);
  }

  async open(path: string): Promise<void> {
    this.openError = null;
    try {
      const view = await invoke<RootView>('open_root', { path });
      this.applyRoot(view);
      // open_root appended to the backend's recent-roots list; the welcome
      // screen renders that list, so our snapshot of it is now stale.
      void settings.reload();
      // Discovery returns before git runs, so the list paints now and the
      // sweep fills it in (§1).
      this.sweeping = view.repos.length > 0;
    } catch (err) {
      this.openError = String(err);
      this.root = null;
      this.repos = [];
      this.statuses = {};
      this.errors = {};
    }
  }

  async refresh(): Promise<void> {
    if (!inTauri || !this.root) return;
    this.sweeping = true;
    try {
      // Discovery runs again: a repo may have been cloned or deleted since
      // the folder was opened.
      this.applyRoot(await invoke<RootView>('refresh_root'));
    } catch (err) {
      this.sweeping = false;
      console.warn('twogit: refresh failed', err);
    }
  }

  private applyRoot(view: RootView): void {
    // A selection is meaningless once the repo it names may be gone.
    if (view.path !== this.root) {
      this.selected = new Set();
      this.files = null;
      this.filesError = null;
    }
    this.root = view.path;
    this.repos = view.repos;
    this.statuses = view.statuses;
    this.errors = view.errors;
  }

  private applySweep(event: SweepEvent): void {
    // Results for a folder this window no longer shows.
    if (event.root !== this.root) return;

    this.statuses = event.statuses;
    this.errors = event.errors;
    this.lastSweepMs = event.elapsedMs;
    this.sweeping = false;
  }

  private applyRepoStatus(event: RepoStatusEvent): void {
    if (event.root !== this.root) return;

    if (event.status) {
      this.statuses = { ...this.statuses, [event.repoId]: event.status };
      if (event.repoId in this.errors) {
        const { [event.repoId]: _removed, ...rest } = this.errors;
        this.errors = rest;
      }
    } else if (event.error) {
      this.errors = { ...this.errors, [event.repoId]: event.error };
      if (event.repoId in this.statuses) {
        const { [event.repoId]: _removed, ...rest } = this.statuses;
        this.statuses = rest;
      }
    }
  }
}

export const repos = new RepoStore();
