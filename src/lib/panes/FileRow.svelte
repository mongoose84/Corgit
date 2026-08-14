<script lang="ts">
  import type { FileEntry } from '../repos.svelte';

  interface Props {
    entry: FileEntry;
    action: 'stage' | 'unstage';
    onToggle: () => void;
    disabled?: boolean;
  }

  let { entry, action, onToggle, disabled = false }: Props = $props();

  const label = $derived(action === 'stage' ? 'Stage' : 'Unstage');
  const symbol = $derived(action === 'stage' ? '+' : '−');

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

<div class="file-row">
  <span class="status status-{badgeTone}" title={entry.status}>{entry.status}</span>
  <span class="path selectable" title={entry.path}>{shown}</span>
  <button
    type="button"
    class="toggle"
    {disabled}
    onclick={onToggle}
    title="{label} {entry.path}"
    aria-label="{label} {entry.path}"
  >
    {symbol}
  </button>
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
    font-size: var(--text-md);
    line-height: 1;
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
</style>
