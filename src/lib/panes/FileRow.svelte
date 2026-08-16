<script lang="ts">
  import Glyph from '../Glyph.svelte';
  import type { FileEntry } from '../repos.svelte';
  import type { CommitFileEntry } from '../graph.svelte';

  interface Props {
    entry: FileEntry | CommitFileEntry;
    /** Omitted for a read-only row (commit info panel) — no toggle button. */
    action?: 'stage' | 'unstage';
    onToggle?: () => void;
    disabled?: boolean;
    /** Opens this file's diff in the right pane (§5.4). Required, not optional:
     *  every file row in the app is a way into the diff view, and a row that
     *  silently was not would be indistinguishable from one that failed. */
    onOpen: () => void;
    /** This row's diff is the one currently open — marked so the file lists and
     *  the right pane's tab cannot disagree about what is being shown. */
    selected?: boolean;
  }

  let { entry, action, onToggle, disabled = false, onOpen, selected = false }: Props = $props();

  // Per-file +/− (§5.2 revised, §8.5) — present only on a `CommitFileEntry`,
  // so this doubles as the "am I a commit-info row" check.
  const stats = $derived('insertions' in entry ? entry : null);

  const label = $derived(action === 'stage' ? 'Stage' : 'Unstage');

  /** M/A/D/R/C/T untouched, but git's own letters don't map to CSS-safe class
   *  names (`?`), so this buckets them into a handful of semantic tones. */
  function tone(status: string): string {
    switch (status) {
      case 'A':
        return 'added';
      case 'D':
        return 'deleted';
      case 'R':
      case 'C':
        return 'renamed';
      case '?':
        return 'untracked';
      case 'U':
        return 'conflict';
      default:
        return 'modified';
    }
  }

  const badgeTone = $derived(tone(entry.status));

  /** Ellipsized head-first so the filename stays visible (§5.2) — CSS
   *  `text-overflow: ellipsis` only ever trims the tail, so the head has to
   *  be trimmed by hand. */
  function headEllipsis(path: string, max: number): string {
    if (path.length <= max) return path;
    return `…${path.slice(path.length - (max - 1))}`;
  }

  const shown = $derived(headEllipsis(entry.path, 44));
</script>

<div class="file-row" class:selected>
  <!-- The path is deliberately not `.selectable` here, unlike everywhere else
       readable in the app: `cursor: text` on something that opens a diff when
       clicked is a lie, and the full path is on the title attribute anyway. -->
  <button type="button" class="open" onclick={onOpen} title="Show the diff for {entry.path}">
    <span class="status status-{badgeTone}" title={entry.status}>{entry.status}</span>
    <span class="path">{shown}</span>
    {#if stats}
      <span class="stats">
        {#if stats.insertions === null && stats.deletions === null}
          <span class="stat-na">binary</span>
        {:else}
          <span class="stat-add">+{stats.insertions ?? 0}</span>
          <span class="stat-del">−{stats.deletions ?? 0}</span>
        {/if}
      </span>
    {/if}
  </button>
  {#if action && onToggle}
    <button
      type="button"
      class="toggle"
      {disabled}
      onclick={onToggle}
      title="{label} {entry.path}"
      aria-label="{label} {entry.path}"
    >
      <Glyph kind={action === 'stage' ? 'plus' : 'minus'} />
    </button>
  {/if}
</div>

<style>
  .file-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: var(--row-height);
    padding: 0 var(--space-3);
  }

  .file-row:hover {
    background: var(--bg-hover);
  }

  /* Declared after the hover rule so the open file stays marked while the
     pointer is elsewhere in the list — which file is showing is a standing
     fact, not a transient one (§5.3's HEAD-row reasoning). */
  .file-row.selected {
    background: var(--accent-muted);
  }

  /* The row itself (§5.4). A button rather than a click handler on the div, so
     it is keyboard-reachable and announced as an action; the stage/unstage
     toggle stays a sibling, since a button inside a button is not valid. */
  .open {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
  }

  .status {
    flex: 0 0 auto;
    width: 14px;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    text-align: center;
    color: var(--text-muted);
  }

  .status-added {
    color: var(--status-ahead);
  }

  .status-deleted,
  .status-conflict {
    color: var(--status-error);
  }

  .status-untracked {
    color: var(--status-dirty);
  }

  .path {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    font-size: var(--text-sm);
    color: var(--text-primary);
  }

  .toggle {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    opacity: 0;
  }

  .file-row:hover .toggle,
  .toggle:focus-visible {
    opacity: 1;
  }

  .toggle:hover:not(:disabled) {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  .toggle:disabled {
    color: var(--text-disabled);
    cursor: default;
  }

  .stats {
    flex: 0 0 auto;
    display: flex;
    gap: var(--space-1);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }

  .stat-add {
    color: var(--status-ahead);
  }

  .stat-del {
    color: var(--status-error);
  }

  .stat-na {
    color: var(--text-disabled);
  }
</style>
