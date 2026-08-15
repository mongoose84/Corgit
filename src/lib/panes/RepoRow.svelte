<script lang="ts">
  import { isDirty, repos, type Repo, type RepoStatus } from '../repos.svelte';
  import ContextMenu from '../ContextMenu.svelte';
  import Popover from '../Popover.svelte';
  import GitErrorNotice from '../GitErrorNotice.svelte';

  interface Props {
    repo: Repo;
    status?: RepoStatus;
    error?: string;
  }

  let { repo, status, error }: Props = $props();

  const selected = $derived(repos.selected.has(repo.id));
  const dirty = $derived(status !== undefined && isDirty(status));
  // Detached HEAD has no branch name; the short oid is the honest substitute.
  const branch = $derived(status?.branch ?? status?.head ?? '');
  const pinned = $derived(repos.pins.has(repo.id));
  // The background fetch sweep stopped retrying this repo (§8.7, §13) — a
  // manual fetch is what clears it. Shown alongside the other badges rather
  // than replacing them: unlike a status-read failure, this repo's status is
  // still known and current, just possibly stale on the "behind" count.
  const authNeeded = $derived(repos.authNeeded.has(repo.id));
  // A row-triggered write (Fetch/Pull from this very row) failing is the
  // freshest, most actionable signal — it wins over a stale status-read
  // failure when both happen to be present (§5.1, §13).
  const rowError = $derived(repos.rowErrors[repo.id] ?? error);
  const canPullRow = $derived(status !== undefined && status.behind > 0 && status.conflicted === 0);

  let menuPos = $state<{ x: number; y: number } | null>(null);
  let errorPopoverPos = $state<{ x: number; y: number } | null>(null);
  let pulling = $state(false);

  function openMenu(event: MouseEvent) {
    event.preventDefault();
    menuPos = { x: event.clientX, y: event.clientY };
  }

  function openErrorPopover(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    errorPopoverPos = { x: event.clientX, y: event.clientY };
  }

  async function pullRow(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    if (pulling) return;
    pulling = true;
    try {
      await repos.pullRow(repo.id);
    } finally {
      pulling = false;
    }
  }

  const menuItems = $derived([
    { label: pinned ? 'Unpin' : 'Pin', onSelect: () => void repos.togglePin(repo.id) },
    { label: 'Fetch', onSelect: () => void repos.fetchRepo(repo.id) },
    { label: 'Open in VS Code', onSelect: () => void repos.openInVSCode(repo.id) },
    { label: 'Open in Terminal', onSelect: () => void repos.openInTerminal(repo.id) },
    { label: 'Copy path', onSelect: () => void navigator.clipboard.writeText(repo.path) },
  ]);
</script>

<button
  type="button"
  class="row"
  class:selected
  aria-current={selected}
  title={repo.path}
  onclick={() => repos.select(repo.id)}
  oncontextmenu={openMenu}
>
  <span class="name">{repo.name}</span>

  <span class="meta">
    {#if canPullRow}
      <!-- Hover-revealed, and only on rows that are behind (§5.1) — acting
           without a select-then-cross-the-window trip is the whole point. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <span
        role="button"
        tabindex="-1"
        class="pull-action"
        class:pulling
        title="Pull"
        aria-label="Pull"
        onclick={pullRow}
      >{pulling ? '…' : '⇩'}</span>
    {/if}

    {#if rowError}
      <!-- A repo whose status could not be read — or whose row-triggered
           write just failed — is unknown/needs-attention, not clean, and
           must never render as a clean row (§5.1). Click opens the raw
           detail (§13). -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <span
        role="button"
        tabindex="-1"
        class="badge error"
        title={rowError}
        onclick={openErrorPopover}
      >!</span>
    {/if}
    {#if status}
      <span class="branch" class:detached={!status.branch}>{branch}</span>

      {#if status.conflicted > 0}
        <span class="badge conflict" title="Merge conflict">⚠</span>
      {:else if dirty}
        <!-- One dot, one state. The row answers "does this need me?" (§5.1);
             staged versus unstaged is the middle pane's job. -->
        <span class="dot" title="Uncommitted changes" aria-label="Uncommitted changes"></span>
      {/if}

      {#if status.ahead > 0}
        <span class="badge ahead" title="{status.ahead} commit(s) to push">↑{status.ahead}</span>
      {/if}
      {#if status.behind > 0}
        <span class="badge behind" title="{status.behind} commit(s) to pull">↓{status.behind}</span>
      {/if}
      {#if authNeeded}
        <span class="badge auth" title="Authentication needed — fetch manually to sign in">⚿</span>
      {/if}
    {/if}
  </span>
</button>

{#if menuPos}
  <ContextMenu x={menuPos.x} y={menuPos.y} items={menuItems} onClose={() => (menuPos = null)} />
{/if}

{#if errorPopoverPos && rowError}
  <Popover x={errorPopoverPos.x} y={errorPopoverPos.y} onClose={() => (errorPopoverPos = null)}>
    <GitErrorNotice
      error={rowError}
      onOpenVSCode={() => repos.openInVSCode(repo.id)}
      onRetry={() => void repos.retryRow(repo.id)}
    />
  </Popover>
{/if}

<style>
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: var(--row-height);
    padding: 0 var(--space-3);
    border: 0;
    background: none;
    text-align: left;
    cursor: default;
  }

  .row:hover {
    background: var(--bg-hover);
  }

  /* The accent is for selection only — status colours must stay distinct from
     it or the list stops being scannable, which is the point of this pane. */
  .row.selected {
    background: var(--accent-muted);
  }

  .name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--text-primary);
  }

  /* Everything after the name hugs the right edge; the slack sits between. */
  .meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 0 1 auto;
    min-width: 0;
    margin-left: auto;
  }

  .branch {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-muted);
  }

  .branch.detached {
    font-family: var(--font-mono);
  }

  .dot {
    flex: 0 0 auto;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--status-dirty);
  }

  .badge {
    flex: 0 0 auto;
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }

  /* Row-level Pull (§5.1): hidden until the row is hovered/focused, and only
     ever rendered on rows that are behind in the first place. */
  .pull-action {
    display: none;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    font-size: var(--text-sm);
    line-height: 1;
  }

  .row:hover .pull-action,
  .row:focus-visible .pull-action {
    display: flex;
  }

  .pull-action:hover {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  .pull-action.pulling {
    display: flex;
    color: var(--text-disabled);
  }

  .ahead {
    color: var(--status-ahead);
  }

  .behind {
    color: var(--status-behind);
  }

  .error {
    color: var(--status-error);
    font-weight: 700;
  }

  .badge.error:hover {
    text-decoration: underline;
  }

  .conflict {
    color: var(--status-conflict);
  }

  .auth {
    color: var(--status-dirty);
  }
</style>
