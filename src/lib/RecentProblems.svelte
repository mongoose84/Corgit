<script lang="ts">
  // Help ▸ Recent Problems… (SPEC.md §13, §4.1).
  //
  // The counterweight to everything §13 lets the UI throw away. A dismissed
  // banner, a suppressed rule and a background sweep that failed unwatched all
  // leave nothing on screen; this is where they still are. It is why the
  // *Don't warn me again* checkbox is safe to offer at all.
  //
  // A modal, unlike the banner — and for the opposite reason the banner is
  // not one. Nothing is wrong at the moment this opens; it is a thing the user
  // deliberately went looking for, and it wants the width to show whole stderr
  // rather than a strip above the panes.
  import EmptyState from './EmptyState.svelte';
  import Mascot from './Mascot.svelte';
  import { formatCommitDate } from './dateFormat';
  import { problems } from './problems.svelte';
  import { repos } from './repos.svelte';

  interface Props {
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  let closeEl: HTMLButtonElement | undefined = $state();

  $effect(() => {
    closeEl?.focus();
  });

  /** Repo *name*, not id — the id is a canonicalised path and this is a list
   *  meant to be skimmed. Falls back to the id when the repo is no longer in
   *  the open root: the entry outlives the folder being closed, and dropping
   *  the attribution would be worse than showing a path. */
  function repoLabel(repoId: string | null): string | null {
    if (repoId === null) return null;
    return repos.repos.find((repo) => repo.id === repoId)?.name ?? repoId;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.stopPropagation();
      onClose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" role="presentation" onclick={onClose}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="dialog"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-label="Recent problems"
    onclick={(event) => event.stopPropagation()}
    onkeydown={onKeydown}
  >
    <div class="head">
      <p class="title">Recent problems</p>
      <div class="head-actions">
        {#if problems.entries.length > 0}
          <!-- Clears the view, never `corgit.log` — the same rule that keeps
               §13's suppression from silencing a condition. The hint below
               says so, because a *Clear* that quietly kept everything would be
               just as confusing as one that quietly destroyed it. -->
          <button type="button" class="link" onclick={() => void problems.clear()}>Clear</button>
        {/if}
        <button bind:this={closeEl} type="button" onclick={onClose}>Close</button>
      </div>
    </div>

    {#if problems.entries.length === 0}
      <EmptyState message="Nothing has gone wrong" hint="Failed git operations are listed here">
        {#snippet art()}
          <Mascot pose="content" height={96} />
        {/snippet}
      </EmptyState>
    {:else}
      <ul class="entries">
        {#each problems.entries as problem (problem.seq)}
          {@const repo = repoLabel(problem.repoId)}
          <li>
            <div class="meta">
              <span class="operation">{problem.operation}</span>
              {#if repo}<span class="repo" title={problem.repoId}>{repo}</span>{/if}
              <span class="at">{formatCommitDate(problem.at)}</span>
            </div>
            <pre class="message selectable">{problem.message}</pre>
          </li>
        {/each}
      </ul>
    {/if}

    <p class="hint">
      The last {problems.entries.length === 1 ? 'failure' : 'few failures'} only. Every git
      command Corgit runs — including the ones that worked — is in the log folder, under Help.
    </p>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 200;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.45);
  }

  /* Wider than the confirmation dialogs, because unlike them this exists to
     show text that arrived at whatever width git wrote it. */
  .dialog {
    display: flex;
    flex-direction: column;
    width: 640px;
    max-width: calc(100vw - var(--space-5));
    max-height: calc(100vh - var(--space-5));
    padding: var(--space-3);
    background: var(--bg-raised);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .title {
    margin: 0;
    font-size: var(--text-md);
    font-weight: 600;
    color: var(--text-primary);
  }

  .head-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .head-actions button {
    height: 24px;
    padding: 0 var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-primary);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .head-actions button:hover {
    background: var(--bg-hover);
    border-color: var(--border-strong);
  }

  .head-actions button.link {
    border-color: transparent;
    background: none;
    color: var(--text-muted);
  }

  .head-actions button.link:hover {
    color: var(--text-primary);
  }

  .entries {
    flex: 1 1 auto;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .entries li {
    padding: var(--space-2);
    border-bottom: 1px solid var(--border);
  }

  .entries li:last-child {
    border-bottom: 0;
  }

  .meta {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
  }

  .operation {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text-primary);
  }

  .repo {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  /* Pushed right and fixed-width, like the graph's date column (§5.3) —
     scanning down the timestamps is how you find the run you remember. */
  .at {
    margin-left: auto;
    flex: 0 0 auto;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
  }

  /* Not clamped. This list is the end of the line for a failure's text, and
     the only reason to open it is that the headline was not enough (§13). */
  .message {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--status-error);
  }

  .hint {
    margin: var(--space-2) 0 0;
    font-size: var(--text-xs);
    color: var(--text-muted);
  }
</style>
