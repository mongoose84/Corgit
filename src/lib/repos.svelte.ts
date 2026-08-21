import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { settings } from './settings.svelte';
import { inTauri } from './tauri';
import { notices } from './notices.svelte';

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
  /** Distinct changed paths — what the row's badge shows. Not the sum of the
   *  four above: a file staged and then edited again is counted on both sides
   *  there, so only the backend, which has the per-path records, can say how
   *  many *files* are involved (§5.1). */
  changedFiles: number;
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
  /** The pane's rows, sent only when this repo is the selected one — the
   *  backend already had them, having read the same `git status` this event's
   *  counts came from (§8.2). `null` for every other repo. */
  files: FileChanges | null;
}

/** Whether the working tree has anything in it at all — the row answers "does
 *  this need me?", and the badge's count answers "how much?" (§5.1). Staged
 *  versus unstaged is still the middle pane's job.
 *
 *  Kept as the sum of the four rather than `changedFiles > 0` so that a status
 *  from anywhere — an older cache, a hand-built fixture — cannot render a repo
 *  holding work as clean. `parse` in `status.rs` guarantees the two agree, and
 *  a test there pins it; this is the direction to be wrong in if they ever
 *  don't. */
/** The blocking state of §13: git stopped mid-merge and will not move on
 *  until someone resolves or aborts. Shared for the same reason `isDirty` is —
 *  the row's `⚠`, the banner, and the commit/push guards must not be able to
 *  disagree about whether this repo is wedged.
 *
 *  Note what it is *not* derived from: any record that a merge failed. A
 *  conflict made in a terminal, or one that outlived a restart, raised no
 *  event inside Corgit and still has to show. Reading the condition itself is
 *  the only version of this that cannot go stale or be dismissed into a lie. */
export function hasConflict(status: RepoStatus): boolean {
  return status.conflicted > 0;
}

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
   *  menu / hover affordance) — keyed by repo id rather than held as one
   *  value, since the row that failed may not be selected (§5.1, §13: "the row
   *  must be able to carry an error badge").
   *
   *  This is the badge's backing store, not the banner's: the banner is one
   *  announcement of the newest failure (`notices.svelte.ts`), and this is the
   *  per-row record that outlives it. */
  rowErrors = $state<Record<string, string>>({});
  /** Whichever of `fetchRepo`/`pullRow` most recently failed for a repo, so
   *  the row's "Retry" re-runs the operation that actually failed rather than
   *  guessing. */
  #rowRetry: Record<string, () => Promise<void>> = {};
  /** Which operation each `rowErrors` entry came from, so re-raising the
   *  banner from the badge says "Fetch" rather than something vague. Private
   *  and parallel to `rowErrors` rather than folded into it: that map is read
   *  as a plain message in three places, and widening it to an object to carry
   *  one label would be the bigger change. */
  #rowOperation: Record<string, string> = {};
  /** The raw stderr from the most recent selected-repo write, kept for the one
   *  caller that has to *classify* a failure rather than display it: §8.3's
   *  unmerged-branch refusal, which grows a *Delete anyway* button instead of
   *  a headline. Not the banner's state — that is `notices.svelte.ts` — and
   *  not for display, or the two would be free to disagree. */
  #lastWriteError: string | null = null;

  get lastWriteError(): string | null {
    return this.#lastWriteError;
  }

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
    return this.write('stage_paths', { paths }, 'Stage');
  }

  async unstagePaths(paths: string[]): Promise<boolean> {
    return this.write('unstage_paths', { paths }, 'Unstage');
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
    return this.write('discard_paths', { paths }, 'Discard');
  }

  async stageAll(): Promise<boolean> {
    return this.write('stage_all', {}, 'Stage all');
  }

  async unstageAll(): Promise<boolean> {
    return this.write('unstage_all', {}, 'Unstage all');
  }

  async commit(message: string): Promise<boolean> {
    return this.write('commit_repo', { message }, 'Commit');
  }

  /** Manual, user-triggered fetch — unlike the background sweep, this one is
   *  allowed to prompt for credentials (§8.7). */
  async fetch(): Promise<boolean> {
    return this.write('fetch_repo', {}, 'Fetch');
  }

  async pull(): Promise<boolean> {
    return this.write('pull_repo', {}, 'Pull');
  }

  /** `git push`, or "Publish branch" (`push -u origin <branch>`) when the
   *  selected repo's current status has no upstream (§8.7) — the caller
   *  decides which button to show; both land here as one command each. */
  async push(): Promise<boolean> {
    return this.write('push_repo', {}, 'Push');
  }

  async publish(): Promise<boolean> {
    return this.write('publish_branch', {}, 'Publish branch');
  }

  /** §13's merge-conflict banner "Abort merge" — the selected repo only,
   *  same as commit/push; the row-level conflict badge is driven purely by
   *  status and needs no store method of its own. */
  async mergeAbort(): Promise<boolean> {
    return this.write('merge_abort', {}, 'Abort merge');
  }

  async commitAndPush(message: string): Promise<boolean> {
    return this.write('commit_and_push', { message }, 'Commit + Push');
  }

  /** Branch switching from the graph (§8.3, §8.4) — `kind` mirrors the ref
   *  badge that was double-clicked or picked from its context menu. */
  async switchBranch(name: string, kind: 'local' | 'remote'): Promise<boolean> {
    return this.write('switch_branch', { name, kind }, 'Switch branch');
  }

  /** Branch creation from the graph (§8.3) — `startPoint` is the ref badge or
   *  commit hash that was right-clicked, so the new branch starts there rather
   *  than at HEAD. */
  async createBranch(name: string, startPoint: string, checkout: boolean): Promise<boolean> {
    return this.write('create_branch', { name, startPoint, checkout }, 'Create branch');
  }

  /** Merging a branch into the checked-out one (§8.3) — `name` is the ref
   *  badge that was right-clicked; the destination is always HEAD, so there is
   *  nothing else to pass. A conflict comes back as a failed write whose
   *  status refresh raises §13's conflict banner, which is where the way out
   *  of it lives. */
  async mergeBranch(name: string): Promise<boolean> {
    return this.write('merge_branch', { name }, 'Merge');
  }

  /** Deleting a local branch from the graph (§8.3) — `name` is the local ref
   *  badge that was right-clicked. `force` is `git branch -D`, and only the
   *  delete dialog's second step ever passes it: the first attempt is always
   *  the safe `-d`, so the unsafe one is reachable only through git's own
   *  "not fully merged" refusal on screen. */
  async deleteBranch(name: string, force: boolean): Promise<boolean> {
    return this.write('delete_branch', { name, force }, 'Delete branch');
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
      // Not a git write, but it is the recovery §13 offers when git has
      // failed — so a launcher that itself fails must say so somewhere, or
      // *Open in VS Code* silently does nothing and the user is stranded,
      // which is the exact outcome §13's rule exists to forbid.
      notices.raise(target, 'Open in VS Code', String(err));
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

  /** A file row's right-click ▸ Reveal in File Explorer (§5.2). Always the
   *  selected repo — the file lists only ever show that one — and
   *  fire-and-forget like `openInTerminal`: nothing in Corgit's own state
   *  changes because a shell window opened, and a failure here has no place in
   *  the error banner, which is about operations the user asked git for. */
  async revealInExplorer(path: string): Promise<void> {
    const id = this.selectedId;
    if (!id) return;
    try {
      await invoke('reveal_in_explorer', { repoId: id, path });
    } catch (err) {
      console.warn('corgit: could not reveal the file', err);
    }
  }

  /** Right-click → Fetch on a row that may not be the selected repo (§5.1).
   *  Goes through the same write queue as every other write; failures badge
   *  the row as well as raising the banner, since the row that failed is not
   *  necessarily the one currently shown. */
  async fetchRepo(id: string): Promise<void> {
    await this.rowWrite(id, 'fetch_repo', 'Fetch', () => this.fetchRepo(id));
  }

  /** Row-level Pull (§5.1) — the dashboard's whole thesis is acting without
   *  navigating, so a behind row gets its own hover-revealed Pull rather than
   *  forcing a select-then-cross-the-window trip. Same `rowErrors` reasoning
   *  as `fetchRepo`. */
  async pullRow(id: string): Promise<void> {
    await this.rowWrite(id, 'pull_repo', 'Pull', () => this.pullRow(id));
  }

  /** The row error's "Retry" (§13's `index.lock` case) — re-runs whichever of
   *  `fetchRepo`/`pullRow` most recently failed for this repo. */
  async retryRow(id: string): Promise<void> {
    await this.#rowRetry[id]?.();
  }

  /** Right-click ▸ Dismiss on a row carrying an error badge (§13).
   *
   *  This is the *only* badge that may be dismissed, and the reason is in
   *  where it lives rather than in what it says: `rowErrors` is owned outright
   *  by the frontend and nothing regenerates it until the operation is tried
   *  again. `errors` and `authNeeded` are replaced wholesale from sweep events
   *  (see `applySweep`), so dismissing either would be a lie the next tick
   *  contradicts — §13's rule that an event may be dismissed and a state may
   *  not, read off the code rather than imposed on it. */
  dismissRowError(id: string): void {
    const { [id]: _removed, ...rest } = this.rowErrors;
    this.rowErrors = rest;
    delete this.#rowOperation[id];
  }

  /** What the row's error badge is about. `undefined` when the badge is being
   *  drawn for a failed *status read* instead, which belongs to no operation
   *  the user asked for. */
  rowOperation(id: string): string | undefined {
    return this.#rowOperation[id];
  }

  private async rowWrite(
    id: string,
    command: string,
    operation: string,
    retry: () => Promise<void>,
  ): Promise<void> {
    delete this.rowErrors[id];
    this.#rowRetry[id] = retry;
    try {
      await invoke(command, { repoId: id });
      delete this.#rowRetry[id];
    } catch (err) {
      // Both, and they are not redundant (§13): the badge is this row's
      // durable record that something failed here, the banner is the one
      // announcement — and the banner may be suppressed or replaced by a newer
      // failure, at which point the badge is all that is left pointing at it.
      this.rowErrors = { ...this.rowErrors, [id]: String(err) };
      this.#rowOperation[id] = operation;
      notices.raise(id, operation, String(err), () => void retry());
    }
  }

  /** Every mutating command shares this shape: resolve the selected repo,
   *  invoke, raise a banner on failure (§13). Both the row *and* the file
   *  list update themselves from the `status:repo` event (§7), which the
   *  backend emits before the command returns — including after a failure,
   *  since a failed stage can still have moved the index.
   *
   *  That event is why there is no `loadFiles()` here any more. There used to
   *  be, and it made every stage, unstage, discard and commit read the same
   *  repo's status twice: once in `write_and_refresh`, once again for the
   *  paths. Returns whether it succeeded, so e.g. the commit box only clears
   *  on success. */
  private async write(
    command: string,
    args: Record<string, unknown>,
    operation: string,
  ): Promise<boolean> {
    const id = this.selectedId;
    if (!id) return false;

    this.#lastWriteError = null;
    try {
      await invoke(command, { repoId: id, ...args });
      return true;
    } catch (err) {
      this.#lastWriteError = String(err);
      // §13: one surface, chosen by tier rather than by which pane called
      // this. The compose pane used to render its own copy inline, which is
      // how the same push rejection came to look like two different failures
      // depending on whether it was started from the row or the button.
      notices.raise(id, operation, String(err), () => void this.write(command, args, operation));
      return false;
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

    // Carried on the event rather than fetched: `repo_files` would re-run the
    // very `git status` this event was built from, and on the bench machine
    // that second spawn costs 60–340 ms of which almost all is process
    // creation. Guarded on the selection anyway — the backend decides using
    // the selection it knows about, and this one is the truth the pane is
    // painted from, so a selection that moved while the event was in flight
    // drops it here rather than showing another repo's files.
    if (event.files && event.repoId === this.selectedId) {
      this.files = event.files;
      this.filesError = null;
      this.loadingFiles = false;
    }

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
