<script lang="ts">
  import { isDirty, repos, type Repo, type RepoStatus } from '../repos.svelte';

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
  // The background fetch sweep stopped retrying this repo (§8.7, §13) — a
  // manual fetch is what clears it. Shown alongside the other badges rather
  // than replacing them: unlike a status-read failure, this repo's status is
  // still known and current, just possibly stale on the "behind" count.
  const authNeeded = $derived(repos.authNeeded.has(repo.id));
</script>

<button
  type="button"
  class="row"
  class:selected
  aria-current={selected}
  title={repo.path}
  onclick={() => repos.select(repo.id)}
>
  <span class="name">{repo.name}</span>

  <span class="meta">
    {#if error}
      <!-- A repo whose status could not be read is unknown, not clean, and
           must never render as a clean row (§5.1). -->
      <span class="badge error" title={error}>!</span>
    {:else if status}
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

  .conflict {
    color: var(--status-conflict);
  }

  .auth {
    color: var(--status-dirty);
  }
</style>
