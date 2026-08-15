<script lang="ts">
  import Pane from './Pane.svelte';
  import RepoRow from './RepoRow.svelte';
  import EmptyState from '../EmptyState.svelte';
  import { repos } from '../repos.svelte';

  let filter = $state('');

  // Substring on repo name only — not branch, not path (SPEC.md §5.1).
  const shown = $derived.by(() => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return repos.repos;
    return repos.repos.filter((repo) => repo.name.toLowerCase().includes(needle));
  });

  // Two sections, each alphabetical (§5.1) — discovery already returns repos
  // pre-sorted by name, so this is a partition of `shown`, not a re-sort.
  const pinned = $derived(shown.filter((repo) => repos.pins.has(repo.id)));
  const unpinned = $derived(shown.filter((repo) => !repos.pins.has(repo.id)));

  const count = $derived(
    shown.length === repos.repos.length
      ? `${repos.repos.length}`
      : `${shown.length} of ${repos.repos.length}`,
  );
</script>

<Pane title="Repositories ({count})">
  {#snippet actions()}
    {#if repos.lastSweepMs !== null && !repos.sweeping}
      <!-- The 300 ms status-sweep budget in §1 is the reason this project
           exists, so the measurement is on screen, not in a log. -->
      <span class="timing" title="Last status sweep across {repos.repos.length} repositories"
        >{repos.lastSweepMs} ms</span
      >
    {/if}
    <button
      type="button"
      class="refresh"
      onclick={() => void repos.refresh()}
      disabled={repos.sweeping || !repos.root}
      title="Rescan and refresh status"
      aria-label="Refresh"
    >
      ⟳
    </button>
  {/snippet}

  <div class="filter">
    <input
      type="search"
      placeholder="Filter repositories…"
      bind:value={filter}
      disabled={repos.repos.length === 0}
      aria-label="Filter repositories by name"
    />
  </div>

  {#if repos.repos.length === 0}
    <EmptyState
      message="No repositories here"
      hint="twogit looks one level down — open the folder that contains them"
    />
  {:else if shown.length === 0}
    <EmptyState message="No matches" hint="Filter matches repository names only" />
  {:else}
    {#if pinned.length > 0}
      <div class="section-header">
        <span>Pinned ({pinned.length})</span>
        <!-- Emptying the hot set in one click matters as much as filling it:
             the set is meant to track what you are working on this week, and
             a set that is tedious to clear stops tracking anything. -->
        {#if filter.trim() === ''}
          <!-- Hidden while filtering: the section then shows a subset, and a
               button that quietly unpins repos the user cannot see is a trap. -->
          <button type="button" class="clear" onclick={() => void repos.clearPins()}>
            Unpin all
          </button>
        {/if}
      </div>
      <ul>
        {#each pinned as repo (repo.id)}
          <li>
            <RepoRow {repo} status={repos.status(repo.id)} error={repos.error(repo.id)} />
          </li>
        {/each}
      </ul>
    {/if}

    {#if pinned.length > 0}
      <div class="section-header">All ({unpinned.length})</div>
    {/if}
    <ul>
      {#each unpinned as repo (repo.id)}
        <li>
          <RepoRow {repo} status={repos.status(repo.id)} error={repos.error(repo.id)} />
        </li>
      {/each}
    </ul>
  {/if}
</Pane>

<style>
  .filter {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
  }

  input {
    width: 100%;
    height: 26px;
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  input::placeholder {
    color: var(--text-disabled);
  }

  input:disabled {
    color: var(--text-disabled);
    cursor: default;
  }

  input:focus-visible {
    border-color: var(--accent);
    outline: none;
  }

  .timing {
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    color: var(--text-disabled);
  }

  .refresh {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    cursor: default;
  }

  .refresh:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .refresh:disabled {
    color: var(--text-disabled);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3) var(--space-1);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .clear {
    padding: 0;
    border: 0;
    background: none;
    font-size: var(--text-xs);
    letter-spacing: 0.04em;
    color: var(--text-disabled);
    cursor: default;
    /* Transparent rather than hidden, so it stays reachable by keyboard. */
    opacity: 0;
  }

  /* Revealed with the section, not the individual row — it acts on the whole
     set, so hovering any part of that set is the right trigger. */
  .section-header:hover .clear,
  .clear:focus-visible {
    opacity: 1;
  }

  .clear:hover {
    color: var(--text-primary);
    text-decoration: underline;
  }
</style>
