<script lang="ts">
  /**
   * §5.1's root strip — the folder-wide band at the top of the repo list:
   * *Pull all* and the behind count at rest, the bulk run's progress and its
   * Stop while one is going.
   *
   * Cut out of RepoList because it is the one part of that pane that
   * scopes to the *folder* rather than to the rows: it reads the bulk run and
   * the behind count, never the filter, the pins or the list itself. That is
   * the same boundary the comment on its placement always drew ("this strip
   * deliberately ignores it"), now drawn by the component instead.
   *
   * Reads `repos` directly, like every pane — there is nothing the parent
   * knows that the store does not.
   */
  import { bulkProgressText, repos } from '../repos.svelte';

  const bulk = $derived(repos.bulk);
  const behind = $derived(repos.behindCount);

  /**
   * The strip's progress bar (§5.1), as two overlaid segments rather than a
   * track and a fill: solid for repos that have landed, dim for the ones with
   * a git process running right now.
   *
   * The dim segment is what makes the bar honest at the start of a run. With
   * four pulls in flight and none finished, a plain `done / total` bar sits at
   * zero for several seconds while the app is visibly busy — so it shows work
   * happening without claiming work is done, and the gap between the two
   * segments is the concurrency cap made visible.
   *
   * Percentages rather than pixels because the strip is the pane's width, and
   * the pane is resizable down to 190px (§4).
   */
  const progress = $derived.by(() => {
    if (bulk === null || bulk.total === 0) return { done: 0, reached: 0 };
    return {
      done: (bulk.done / bulk.total) * 100,
      // Clamped for the one frame where a start and a finish can both be in
      // flight across the IPC boundary: a segment wider than its own strip
      // would paint over the pane's edge.
      reached: Math.min(100, ((bulk.done + bulk.running) / bulk.total) * 100),
    };
  });
</script>

<div class="root-strip">
  {#if bulk}
    <span class="summary" aria-live="polite">
      {bulkProgressText(bulk)}
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
    <!-- Sits *on* the border the strip already draws, which is the only
         reason it is affordable: a bar that took a row of its own would be
         spending a second repo row on chrome (§14.1). Decorative — the
         counter beside it is the accessible reading, and `aria-live` is
         already on that. -->
    <span class="bar reached" style="width: {progress.reached}%" aria-hidden="true"></span>
    <span class="bar done" style="width: {progress.done}%" aria-hidden="true"></span>
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

<style>
  /* `--bg-app` rather than the pane's own surface, so it reads as a band
     across the top of the list rather than as its first row — the
     distinction the whole placement rests on. */
  .root-strip {
    position: relative;
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

  /* `bottom: -1px` puts the bar over the strip's own 1px border rather than
     above it, so the running state costs one pixel of height, not two. */
  .bar {
    position: absolute;
    left: 0;
    bottom: -1px;
    height: 2px;
    /* Only the width animates, and only while a run is going: the value
       arrives in steps of one repo, which at ten repos is a 10% jump. */
    transition: width 180ms ease;
  }

  /* Same accent as the Pull all button this replaced, so the bar reads as that
     button running rather than as a new colour with a new meaning (§11). */
  .bar.done {
    background: var(--accent);
  }

  .bar.reached {
    background: var(--accent-muted);
  }
</style>
