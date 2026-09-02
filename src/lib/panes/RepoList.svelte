<script lang="ts">
  import Pane from './Pane.svelte';
  import RepoRow from './RepoRow.svelte';
  import EmptyState from '../EmptyState.svelte';
  import Mascot from '../Mascot.svelte';
  import { repos } from '../repos.svelte';
  import { filterTerms, matchesFilter } from '../repoFilter';

  let filter = $state('');

  // Substring on repo name only — not branch, not path (SPEC.md §5.1) — with
  // a comma-separated value matching any of its terms. The rule lives in
  // `repoFilter.ts` with the banner's half of the same contract; see there for
  // why the box learned to hold a list.
  const terms = $derived(filterTerms(filter));

  const shown = $derived.by(() => {
    if (terms.length === 0) return repos.repos;
    return repos.repos.filter((repo) => matchesFilter(repo.name, terms));
  });

  // The banner's *Show the N* (§5.1, §13). A one-shot signal, consumed as it
  // is applied: left set, a later sweep or repo selection would re-apply a
  // filter the user has since cleared, which is a list that will not let go.
  $effect(() => {
    const requested = repos.filterRequest;
    if (requested === null) return;
    filter = requested;
    repos.clearFilterRequest();
  });

  const bulk = $derived(repos.bulk);
  const behind = $derived(repos.behindCount);

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
    {#if repos.sweeping}
      <!-- Dead time, and chrome rather than decoration — the same reasoning
           that lets a mascot glyph be a button icon (SPEC §14.1). He trots for
           as long as the sweep runs; the timing readout takes the slot back
           the moment it lands, so the two never fight for the space. -->
      <Mascot pose="mini-working" height={18} />
    {:else if repos.lastSweepMs !== null}
      <!-- The 300 ms status-sweep budget in §1 is the reason this project
           exists, so the measurement is on screen, not in a log. -->
      <span class="timing" title="Last status sweep across {repos.repos.length} repositories"
        >{repos.lastSweepMs} ms</span
      >
    {/if}
    <!-- The only icon in this header. ⟳ used to sit beside it and moved to
         Repository ▸ Rescan Folder (§5.1): two circular arrows differing by
         stroke direction cannot be told apart, and rediscovery is rare enough
         for a menu now that the watchers keep the rows current (§6).

         Same glyph and same 22px button as *Changes*' Fetch, one column over —
         the icon means one thing everywhere, and only the column it sits in
         says what it applies to. -->
    <button
      type="button"
      class="icon-action"
      onclick={() => void repos.fetchAll()}
      disabled={bulk !== null || !repos.root || repos.repos.length === 0}
      title="Fetch every repository"
      aria-label="Fetch all"
    >
      ↻
    </button>
  {/snippet}

  <!-- §5.1's root strip. Reserved rather than conditional, and that is the
       whole point: shown only when something was behind, it would be inserted
       by the *fetch sweep* — unprompted, on its own schedule — putting a
       button that writes to every behind working tree exactly where a repo row
       was a moment ago. Reserving it means nothing ever moves.

       Above the filter box, not below: actions on top, list manipulation
       underneath. Everything below the box is what the box scopes, and this
       strip deliberately ignores it. -->
  {#if repos.repos.length > 0}
    <div class="root-strip">
      {#if bulk}
        <span class="summary" aria-live="polite">
          {bulk.operation === 'Pull' ? 'Pulling' : 'Fetching'}… {bulk.done} of {bulk.total}
        </span>
        <!-- Honest about what it can do (§5.1): a `git pull` cannot be
             abandoned mid-merge without leaving a tree to repair by hand, so
             this stops the queue, and the repos already running still land.
             The count keeps moving afterwards, which is the visible proof. -->
        <button
          type="button"
          class="stop"
          onclick={() => repos.stopBulk()}
          disabled={repos.bulkStopping}
        >
          {repos.bulkStopping ? 'Stopping…' : 'Stop'}
        </button>
      {:else}
        {#if behind > 0}
          <span class="badge behind" aria-hidden="true">↓</span>
          <span class="summary">{behind} behind</span>
        {:else if repos.allClean}
          <span class="summary idle">All {repos.repos.length} in sync</span>
        {:else}
          <!-- `allClean` is strict — it counts dirty and ahead too — and this
               is the branch where nothing is behind but something else needs
               the user. Saying "in sync" there would be a small lie told by
               the one line whose job is being trusted at a glance. -->
          <span class="summary idle">Nothing to pull</span>
        {/if}
        <!-- Neutral at rest by inheritance, not by a second rule: `.primary`
             carries the accent only `:not(:disabled)`, matching the Commit
             button it borrows its styling from (§5.2). -->
        <button
          type="button"
          class="pull-all primary"
          onclick={() => void repos.pullAllBehind()}
          disabled={behind === 0}
          title={behind > 0 ? `Pull ${behind} repositories that are behind` : 'Nothing is behind'}
        >
          Pull all
        </button>
      {/if}
    </div>
  {/if}

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
      hint="Corgit looks one level down — open the folder that contains them"
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

  /* Deliberately identical to CommitPane's `.icon-action`, down to the
     values: §5.1's Fetch all and §5.2's Fetch are the same button doing the
     same thing at two scopes, and two copies that merely look alike is how
     they stop looking alike. */
  .icon-action {
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
    font-size: var(--text-md);
    line-height: 1;
    cursor: default;
  }

  .icon-action:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .icon-action:disabled {
    color: var(--text-disabled);
  }

  /* The root strip (§5.1). `--bg-app` rather than the pane's own surface, so
     it reads as a band across the top of the list rather than as its first
     row — the distinction the whole placement rests on. */
  .root-strip {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    background: var(--bg-app);
    border-bottom: 1px solid var(--border);
  }

  .badge.behind {
    font-size: var(--text-xs);
    line-height: 1;
    color: var(--status-behind);
  }

  .summary {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .summary.idle {
    color: var(--text-disabled);
  }

  .pull-all {
    flex: 0 0 auto;
    margin-left: auto;
    height: 22px;
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: default;
  }

  /* The same two rules CommitPane's Commit button uses, and the same reason
     they are written `:not(:disabled)`: a disabled primary falls back to the
     neutral fill on its own, which is exactly the resting state §5.1 wants
     when nothing is behind. No second rule, no second colour. */
  .pull-all.primary:not(:disabled) {
    color: var(--accent-text);
    background: var(--accent-muted);
    border-color: var(--accent);
  }

  .pull-all.primary:hover:not(:disabled) {
    color: var(--accent-text);
    background: var(--accent);
    border-color: var(--accent-hover);
  }

  .pull-all:disabled {
    color: var(--text-disabled);
  }

  .stop {
    flex: 0 0 auto;
    margin-left: auto;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-muted);
    cursor: default;
  }

  .stop:hover:not(:disabled) {
    color: var(--text-primary);
    text-decoration: underline;
  }

  .stop:disabled {
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
