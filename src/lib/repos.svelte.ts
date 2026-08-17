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
  lastFetchAt: Record<string, number>;
  authNeeded: string[];
  pins: string[];
  lastSelected: string | null;
}

interface SweepEvent {
  /** Echoed back so results for a replaced root can be dropped. */
  root: string;
  statuses: Record<string, RepoStatus>;
  errors: Record<string, string>;
  elapsedMs: number;
}

/** The background fetch sweep's own event (SPEC.md §6) — separate from the
 *  status sweep's, since fetch and status are different mechanisms. Carries
 *  no status data of its own; a fetch moves refs/remotes/*, and it is the
 *  status sweep that turns that into ahead/behind counts. */
interface FetchSweepEvent {
  root: string;
  lastFetchAt: Record<string, number>;
  authNeeded: string[];
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

/** A branch git cannot plainly `push` (§8.7): the middle pane's Push becomes
 *  "Publish branch", and the row marks its branch name as local-only.
 *
 *  Shared rather than derived in each place so the two can never disagree —
 *  a row promising something the button then doesn't offer is worse than
 *  either signal alone. Detached HEAD is excluded: it has no upstream either,
 *  but there is nothing there to publish, and the row already says so by
 *  showing the oid instead of a name.
 *
 *  Two states qualify, not one. The obvious one is *no upstream at all*. The
 *  second is an upstream whose **branch name differs from the local branch's**
 *  — `feature-x` tracking `origin/main`, say. Under git's default
 *  `push.default = simple` a bare `git push` refuses that outright, so Push is
 *  the one button guaranteed to fail, while Publish (`push -u origin HEAD`)
 *  both succeeds and re-points the upstream at the matching remote branch.
 *  Offering Push there left such a branch permanently stuck in the UI.
 *
 *  Corgit created these itself until `branch.rs` grew `--no-track` (git's
 *  `branch.autoSetupMerge` sets the upstream from a remote-tracking start
 *  point on its own), so any branch cut from `origin/…` by an older build is
 *  in this state and cannot be repaired by that fix — only by a publish.
 *
 *  The cost, accepted: a *deliberately* mismatched upstream — local `feature`
 *  tracking `origin/jk/feature` — is re-pointed by the next publish. That is a
 *  legitimate setup, and this quietly normalises it. Judged the better trade
 *  for a four-verb dashboard, where a button that cannot work is worse than
 *  one that tidies an unusual config. */
export type PublishReason = 'no-upstream' | 'upstream-name-mismatch';

/** Which of the two states applies, for callers that have to *say* which —
 *  the row's tooltip would otherwise tell someone with a mismatched upstream
 *  that they have no upstream. `needsPublish` is this, asked as a yes/no. */
export function publishReason(status: RepoStatus): PublishReason | null {
  if (status.branch === null) return null;
  if (status.upstream === null) return 'no-upstream';
  return upstreamBranch(status.upstream) === status.branch ? null : 'upstream-name-mismatch';
}

export function needsPublish(status: RepoStatus): boolean {
  return publishReason(status) !== null;
}

/** `origin/feature/x` → `feature/x`. Only the first segment is the remote, so
 *  a branch whose own name contains a `/` survives — the same rule, and the
 *  same assumption about remote names, as `branch.rs`'s `local_name`. */
function upstreamBranch(upstream: string): string {
  const slash = upstream.indexOf('/');
  return slash === -1 ? upstream : upstream.slice(slash + 1);
}

class RepoStore {
  root = $state<string | null>(null);
  repos = $state<Repo[]>([]);
  statuses = $state<Record<string, RepoStatus>>({});
  /** Keyed by repo id. A repo whose status failed is unknown, not clean. */
  errors = $state<Record<string, string>>({});
  /** Unix seconds of each repo's last fetch attempt (§6), keyed by repo id. */
  lastFetchAt = $state<Record<string, number>>({});
  /** Repos whose background fetch most recently failed on what looks like an
   *  auth problem (§8.7, §13) — the background sweep stops retrying these
   *  until a manual fetch. */
  authNeeded = $state<Set<string>>(new Set());
  /** User-pinned repos (§5.1) — the hot set's other half is whichever repo
   *  is selected (§6). Persisted server-side per root. */
  pins = $state<Set<string>>(new Set());
  /** Failures from a row-triggered write (Fetch or Pull from a row's context
   *  menu / hover affordance) — keyed by repo id rather than the single
   *  `writeError`, since the row that failed may not be selected (§5.1,
   *  §13: "the row must be able to carry an error badge"). */
  rowErrors = $state<Record<string, string>>({});
  /** Whichever of `fetchRepo`/`pullRow` most recently failed for a repo, so
   *  the row's error popover's "Retry" re-runs the operation that actually
   *  failed rather than guessing. */
  #rowRetry: Record<string, () => Promise<void>> = {};

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
    // Mirrored server-side for §9.5 persistence and the hot-set watchers
    // (§6). Not for the Repository menu any more — that reads `selectedId`
    // straight off this store (§4.1).
    void invoke('set_selected_repo', { repoId: id }).catch(() => {});
  }

  async togglePin(id: string): Promise<void> {
    try {
      const pins = await invoke<string[]>('toggle_pin', { repoId: id });
      this.pins = new Set(pins);
    } catch (err) {
      console.warn('corgit: could not toggle pin', err);
    }
  }

  /** Empty the hot set (§5.1) — one backend call rather than a loop of
   *  `togglePin`, so it is one file write and one watcher resync. */
  async clearPins(): Promise<void> {
    try {
      const pins = await invoke<string[]>('clear_pins');
      this.pins = new Set(pins);
    } catch (err) {
      console.warn('corgit: could not clear pins', err);
    }
  }

  status(id: string): RepoStatus | undefined {
    return this.statuses[id];
  }

  /**
   * Every repository clean, in sync, and known to be so — the payoff state the
   * *content* mascot reports (SPEC §14.1, docs/mascot.md §5). Deliberately
   * strict: a sweep in flight, a repo whose status failed, or one not yet
   * swept all count as not-yet-known rather than clean, because claiming
   * "all in sync" over stale or missing data is the one way this state can
   * lie to the user.
   */
  get allClean(): boolean {
    if (this.sweeping || this.repos.length === 0) return false;
    return this.repos.every((repo) => {
      if (this.errors[repo.id]) return false;
      const status = this.statuses[repo.id];
      return status !== undefined && !isDirty(status) && status.ahead === 0 && status.behind === 0;
    });
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

  /** Throw away the *unstaged* changes to these paths (§5.2, §8.6) — the one
   *  write in the app that destroys work rather than moving it between the
   *  index and the working tree, which is why the pane confirms it first.
   *
   *  Untracked paths must never reach here: git has nothing to restore them
   *  from, so the command would fail on the pathspec — and because git rejects
   *  a pathspec list wholesale rather than per path, one stray untracked entry
   *  would take a whole multi-file discard down with it. `CommitPane` is what
   *  keeps them out. */
  async discardPaths(paths: string[]): Promise<boolean> {
    return this.write('discard_paths', { paths });
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

  /** Manual, user-triggered fetch — unlike the background sweep, this one is
   *  allowed to prompt for credentials (§8.7). */
  async fetch(): Promise<boolean> {
    return this.write('fetch_repo', {});
  }

  async pull(): Promise<boolean> {
    return this.write('pull_repo', {});
  }

  /** `git push`, or "Publish branch" (`push -u origin <branch>`) when the
   *  selected repo's current status has no upstream (§8.7) — the caller
   *  decides which button to show; both land here as one command each. */
  async push(): Promise<boolean> {
    return this.write('push_repo', {});
  }

  async publish(): Promise<boolean> {
    return this.write('publish_branch', {});
  }

  /** §13's merge-conflict banner "Abort merge" — the selected repo only,
   *  same as commit/push; the row-level conflict badge is driven purely by
   *  status and needs no store method of its own. */
  async mergeAbort(): Promise<boolean> {
    return this.write('merge_abort', {});
  }

  async commitAndPush(message: string): Promise<boolean> {
    return this.write('commit_and_push', { message });
  }

  /** Branch switching from the graph (§8.3, §8.4) — `kind` mirrors the ref
   *  badge that was double-clicked or picked from its context menu. */
  async switchBranch(name: string, kind: 'local' | 'remote'): Promise<boolean> {
    return this.write('switch_branch', { name, kind });
  }

  /** Branch creation from the graph (§8.3) — `startPoint` is the ref badge or
   *  commit hash that was right-clicked, so the new branch starts there rather
   *  than at HEAD. */
  async createBranch(name: string, startPoint: string, checkout: boolean): Promise<boolean> {
    return this.write('create_branch', { name, startPoint, checkout });
  }

  /** The dirty-tree checkout failure's other half (§8.3) — not routed through
   *  `write()`, since it never mutates repo state and has nothing to refresh.
   *  Defaults to the selected repo; the row context menu (§5.1) passes an
   *  explicit id for a row that may not be selected.
   *
   *  `file` opens one file alongside the repo (§5.4). The repo is opened
   *  either way: a file on its own gives a VS Code window with no source
   *  control and no search around it, which is most of what made offering
   *  VS Code worth doing. */
  async openInVSCode(id?: string, file?: { path: string; line?: number }): Promise<void> {
    const target = id ?? this.selectedId;
    if (!target) return;
    try {
      await invoke('open_in_vscode', {
        repoId: target,
        file: file?.path ?? null,
        line: file?.line ?? null,
      });
    } catch (err) {
      this.writeError = String(err);
    }
  }

  /** Right-click → Open in Terminal (§5.1). Fire-and-forget, like
   *  `openInVSCode` — nothing in Corgit's own state changes because of it. */
  async openInTerminal(id: string): Promise<void> {
    try {
      await invoke('open_in_terminal', { repoId: id });
    } catch (err) {
      console.warn('corgit: could not open a terminal', err);
    }
  }

  /** Right-click → Fetch on a row that may not be the selected repo (§5.1).
   *  Goes through the same write queue as every other write; failures land in
   *  `rowErrors` rather than the compose pane's `writeError`, since the row
   *  that failed is not necessarily the one currently shown there. */
  async fetchRepo(id: string): Promise<void> {
    await this.rowWrite(id, 'fetch_repo', () => this.fetchRepo(id));
  }

  /** Row-level Pull (§5.1) — the dashboard's whole thesis is acting without
   *  navigating, so a behind row gets its own hover-revealed Pull rather than
   *  forcing a select-then-cross-the-window trip. Same `rowErrors` reasoning
   *  as `fetchRepo`. */
  async pullRow(id: string): Promise<void> {
    await this.rowWrite(id, 'pull_repo', () => this.pullRow(id));
  }

  /** The row error popover's "Retry" (§13's `index.lock` case) — re-runs
   *  whichever of `fetchRepo`/`pullRow` most recently failed for this repo. */
  async retryRow(id: string): Promise<void> {
    await this.#rowRetry[id]?.();
  }

  private async rowWrite(id: string, command: string, retry: () => Promise<void>): Promise<void> {
    delete this.rowErrors[id];
    this.#rowRetry[id] = retry;
    try {
      await invoke(command, { repoId: id });
      delete this.#rowRetry[id];
    } catch (err) {
      this.rowErrors = { ...this.rowErrors, [id]: String(err) };
    }
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
      await listen<FetchSweepEvent>('fetch:sweep', (event) => this.applyFetchSweep(event.payload));

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
      console.warn('corgit: could not restore the last folder', err);
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
      console.warn('corgit: refresh failed', err);
    }
  }

  private applyRoot(view: RootView): void {
    // A selection is meaningless once the repo it names may be gone.
    const isNewRoot = view.path !== this.root;
    if (isNewRoot) {
      this.selected = new Set();
      this.files = null;
      this.filesError = null;
    }
    this.root = view.path;
    this.repos = view.repos;
    this.statuses = view.statuses;
    this.errors = view.errors;
    this.lastFetchAt = view.lastFetchAt;
    this.authNeeded = new Set(view.authNeeded);
    this.pins = new Set(view.pins);

    // Restores where a relaunch left off (§9.5) — only on a genuinely new
    // root; a reload of the one already open must not fight the frontend's
    // own in-memory selection.
    if (isNewRoot && view.lastSelected) this.select(view.lastSelected);
  }

  private applySweep(event: SweepEvent): void {
    // Results for a folder this window no longer shows.
    if (event.root !== this.root) return;

    this.statuses = event.statuses;
    this.errors = event.errors;
    this.lastSweepMs = event.elapsedMs;
    this.sweeping = false;
  }

  private applyFetchSweep(event: FetchSweepEvent): void {
    if (event.root !== this.root) return;

    this.lastFetchAt = event.lastFetchAt;
    this.authNeeded = new Set(event.authNeeded);
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
