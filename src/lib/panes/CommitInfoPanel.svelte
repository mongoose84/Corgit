<script lang="ts">
  import Pane from './Pane.svelte';
  import FileRow from './FileRow.svelte';
  import EmptyState from '../EmptyState.svelte';
  import { graph } from '../graph.svelte';
  import { formatCommitDate } from '../dateFormat';

  // A fourth column, not a mode of CommitPane (SPEC.md §5.2 revised): opens
  // beside the graph when a commit is selected, closes back to nothing
  // selected. Read-only.
  const details = $derived(graph.details);
  // `%B`'s first line, the same "subject" a graph row shows — split by hand
  // rather than trusting the first `\n` never to be missing on an empty message.
  const subject = $derived(details ? (details.message.split('\n')[0] ?? '') : '');
  // The same ref badges a graph row shows (§5.3) — reused as-is rather than
  // fetched again, since `graph.refs` already covers the whole loaded page.
  const badges = $derived(details ? (graph.refsByHash.get(details.hash) ?? []) : []);

  function close() {
    graph.select('working-tree');
  }
</script>

<Pane title="Commit" class="info-panel">
  {#snippet actions()}
    <button type="button" class="close" title="Close" aria-label="Close" onclick={close}>×</button>
  {/snippet}

  {#if graph.loadingDetails && !details}
    <EmptyState message="Reading commit…" />
  {:else if graph.detailsError}
    <EmptyState message="Could not read this commit" hint={graph.detailsError} />
  {:else if details}
    <div class="commit-header">
      <span class="hash">{details.hash.slice(0, 7)}</span>
      <span class="subject">{subject}</span>
    </div>
    {#if badges.length > 0}
      <div class="refs">
        {#each badges as ref (ref.kind + ref.name)}
          <span class="ref ref-{ref.kind}">{ref.name}</span>
        {/each}
      </div>
    {/if}
    <p class="commit-meta">{details.author} · {formatCommitDate(details.timestamp)}</p>
    <pre class="commit-message selectable">{details.message}</pre>

    <div class="section">
      <span class="section-title">Files</span>
      <span class="count">{details.files.length}</span>
    </div>
    {#if details.files.length === 0}
      <p class="section-empty">No files changed</p>
    {:else}
      <ul>
        {#each details.files as entry (entry.path)}
          <li><FileRow {entry} /></li>
        {/each}
      </ul>
    {/if}
  {/if}
</Pane>

<style>
  /* No Divider sits between the graph and this panel (it isn't resizable),
     so the border that would normally come from a Divider goes here instead.
     `:global` because Pane.svelte, not this component, renders the element
     the class lands on. */
  :global(.info-panel) {
    border-left: 1px solid var(--border);
  }

  .close {
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
  }

  .close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .commit-header {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    margin: var(--space-3) var(--space-3) 0;
    min-width: 0;
  }

  .commit-header .hash {
    flex: 0 0 auto;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .commit-header .subject {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-primary);
  }

  .refs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin: var(--space-2) var(--space-3) 0;
  }

  /* Same badge look as a graph row's (GraphRow.svelte's `.ref`) — this panel
     always names a real commit, so there is no "Uncommitted Changes" case to
     distinguish from. */
  .ref {
    flex: 0 0 auto;
    max-width: 100%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    padding: 1px var(--space-1);
    font-size: var(--text-xs);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
  }

  /* Neither badge kind may borrow the accent — it is reserved for selection
     and primary actions only (SPEC.md §11 rule 3). */
  .ref-local {
    color: var(--text-secondary);
  }

  .ref-remote {
    color: var(--text-muted);
    font-style: italic;
  }

  .commit-meta {
    margin: var(--space-1) var(--space-3) 0;
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .commit-message {
    margin: var(--space-2) var(--space-3);
    padding: var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    white-space: pre-wrap;
    word-break: break-word;
  }

  .section {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3) var(--space-1);
  }

  .section-title {
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .count {
    font-size: var(--text-xs);
    color: var(--text-disabled);
    font-variant-numeric: tabular-nums;
  }

  .section-empty {
    margin: 0;
    padding: 0 var(--space-3) var(--space-2);
    font-size: var(--text-sm);
    color: var(--text-disabled);
  }

  ul {
    margin: 0;
    padding: 0 0 var(--space-2);
    list-style: none;
  }
</style>
