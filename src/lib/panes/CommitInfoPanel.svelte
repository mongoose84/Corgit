<script lang="ts">
  import Pane from './Pane.svelte';
  import FileRow from './FileRow.svelte';
  import EmptyState from '../EmptyState.svelte';
  import Glyph from '../Glyph.svelte';
  import { graph, type RefBadge } from '../graph.svelte';
  import { diff } from '../diff.svelte';
  import { repos } from '../repos.svelte';
  import { formatCommitDate } from '../dateFormat';
  import { laneColorVar } from '../graphLayout';

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
  // Grouped rather than one flat wrapped row (§8.3's Local/Remote split,
  // pulled forward here too) — a commit several branches converge on reads
  // as two short lists instead of a jumble once there are more than a couple.
  const localBadges = $derived(badges.filter((ref) => ref.kind === 'local'));
  const remoteBadges = $derived(badges.filter((ref) => ref.kind === 'remote'));
  // Same "which one is HEAD" emphasis a graph row gives its badges (§8.3).
  const currentBranch = $derived(graph.repoId ? (repos.status(graph.repoId)?.branch ?? null) : null);
  const isCurrent = (ref: RefBadge) => ref.kind === 'local' && ref.name === currentBranch;
  // The commit's own lane in the graph (GraphRow.svelte's `currentBadgeStyle`
  // twin) — this panel never lays out lanes itself, but the selected commit
  // is always one of the currently rendered rows, so the lookup always hits.
  const currentLane = $derived(details ? (graph.rows.find((row) => row.commit.hash === details.hash)?.lane ?? 0) : 0);

  function currentBadgeStyle(lane: number): string {
    const color = laneColorVar(lane);
    return `color: ${color}; border-color: ${color}; background: color-mix(in srgb, ${color} 22%, var(--bg-raised));`;
  }

  function close() {
    graph.select('working-tree');
  }

  /** A commit's file rows open the same diff view the working-tree rows do
   *  (§5.4), against that commit's parent. Read-only either way — the only
   *  difference is which two sides get compared. */
  function openDiff(path: string, hash: string) {
    if (graph.repoId) diff.show(graph.repoId, path, { kind: 'commit', hash });
  }

  function isOpen(path: string, hash: string): boolean {
    return graph.repoId !== null && diff.isOpen(graph.repoId, path, { kind: 'commit', hash });
  }
</script>

<Pane title="Commit" class="info-panel">
  {#snippet actions()}
    <button type="button" class="close" title="Close" aria-label="Close" onclick={close}>
      <Glyph kind="cross" />
    </button>
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
    {#if localBadges.length > 0 || remoteBadges.length > 0}
      <div class="ref-groups">
        {#if localBadges.length > 0}
          <div class="ref-group">
            <span class="ref-group-label">Local</span>
            <div class="refs">
              {#each localBadges as ref (ref.name)}
                <span
                  class="ref ref-local"
                  class:current={isCurrent(ref)}
                  style={isCurrent(ref) ? currentBadgeStyle(currentLane) : undefined}
                >{ref.name}</span>
              {/each}
            </div>
          </div>
        {/if}
        {#if remoteBadges.length > 0}
          <div class="ref-group">
            <span class="ref-group-label">Remote</span>
            <div class="refs">
              {#each remoteBadges as ref (ref.name)}
                <span class="ref ref-remote">{ref.name}</span>
              {/each}
            </div>
          </div>
        {/if}
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
          <li>
            <FileRow
              {entry}
              onOpen={() => openDiff(entry.path, details.hash)}
              selected={isOpen(entry.path, details.hash)}
            />
          </li>
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

  .ref-groups {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: var(--space-2) var(--space-3) 0;
  }

  .ref-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  /* Same treatment as `.section-title` below — kept smaller and without its
     own row, since a group of badges is lighter-weight than a Files section. */
  .ref-group-label {
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .refs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
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

  /* Matches GraphRow.svelte's `.ref.current` — same badge, same emphasis.
     Colour comes from the inline style (`currentBadgeStyle`), not here. */
  .ref.current {
    padding: 2px var(--space-2);
    font-size: var(--text-sm);
    font-weight: 700;
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
