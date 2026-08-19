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
     *  silently was not would be indistinguishable from one that failed.
     *
     *  Handed the click rather than called bare, because in the working-tree
     *  lists the modifier keys decide whether this is an open at all: ctrl and
     *  shift build a selection instead (§5.2). */
    onOpen: (event: MouseEvent) => void;
    /** Part of the pane's current selection — the highlight a batch action
     *  will act on. In the read-only commit info panel there is no selection
     *  to build, so it marks the open row instead. */
    selected?: boolean;
    /** This row's diff is the one the right pane is showing — a separate fact
     *  from `selected` now that a selection can be several rows and can leave
     *  the open one out, and marked separately so the two cannot be confused
     *  for each other. */
    showingDiff?: boolean;
    /** Right-click. Absent on a read-only row, which has no menu. */
    onContextMenu?: (event: MouseEvent) => void;
    /** Absent on an untracked row — git has nothing to restore it from, so
     *  discard does not apply to it (§5.2) — and on every staged row, where
     *  discarding would have to mean throwing away the staged work too, which
     *  is not what the button says. */
    onDiscard?: () => void;
  }

  let {
    entry,
    action,
    onToggle,
    disabled = false,
    onOpen,
    selected = false,
    showingDiff = false,
    onContextMenu,
    onDiscard,
  }: Props = $props();

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

  /** Filename and the directory it sits in, split so they can be styled and
   *  trimmed apart (§5.2). Git reports POSIX separators even on Windows, so
   *  one split character is enough. A file at the repo root has no directory
   *  half, and nothing is drawn for it. */
  const name = $derived(entry.path.slice(entry.path.lastIndexOf('/') + 1));
  const dir = $derived(entry.path.slice(0, Math.max(entry.path.lastIndexOf('/'), 0)));
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="file-row" class:selected class:showing={showingDiff} oncontextmenu={onContextMenu}>
  <!-- The path is deliberately not `.selectable` here, unlike everywhere else
       readable in the app: `cursor: text` on something that opens a diff when
       clicked is a lie, and the full path is on the title attribute anyway. -->
  <button type="button" class="open" onclick={onOpen} title="Show the diff for {entry.path}">
    <span class="status status-{badgeTone}" title={entry.status}>{entry.status}</span>
    <span class="name">{name}</span>
    {#if dir}
      <span class="dir">{dir}</span>
    {/if}
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
  {#if onDiscard}
    <!-- A character rather than a drawn Glyph: Glyph exists because `+`/`−`/`×`
         are *math* glyphs sitting off the em box's centre, and an arrow is
         not — the pane header's ↻/⇩ are the same call. Left of the stage
         toggle so `+` keeps the rightmost slot it has always had. -->
    <button
      type="button"
      class="toggle discard"
      {disabled}
      onclick={onDiscard}
      title="Discard changes to {entry.path}"
      aria-label="Discard changes to {entry.path}"
    >↺</button>
  {/if}
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

  /* Declared after the hover rule so a selected file stays marked while the
     pointer is elsewhere in the list — what is selected is a standing fact,
     not a transient one (§5.3's HEAD-row reasoning). */
  .file-row.selected {
    background: var(--accent-muted);
  }

  /* The row whose diff is up. A bar rather than a second background, because
     it has to be readable *on top of* the selection fill: with a multi-row
     selection every row is already accent-muted, and a fill that only differed
     in shade would make the open row indistinguishable from the rest. Drawn
     with a border so it costs no layout — the padding compensates so text does
     not shift by 2px as the diff moves down the list. */
  .file-row.showing {
    border-left: 2px solid var(--accent);
    padding-left: calc(var(--space-3) - 2px);
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

  /* The filename is what the row is *for*, so it gets full contrast and only
     shrinks once the directory beside it has nothing left to give — hence the
     lopsided shrink factors below rather than a plain `1`. Both halves are
     tail-trimmed by CSS: a path that runs out of room should lose its deepest
     folder, not the segment that says which project it is in. */
  .name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-sm);
    color: var(--text-primary);
  }

  .dir {
    flex: 0 100 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-xs);
    color: var(--text-muted);
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

  /* The one control in the file lists that destroys work rather than moving it
     between the index and the tree, so it is the one that goes red under the
     pointer — the confirmation (§5.2) is the actual guard, this is only the
     warning that one is coming. */
  .discard {
    font-size: var(--text-md);
    line-height: 1;
  }

  .discard:hover:not(:disabled) {
    background: var(--bg-active);
    color: var(--danger-hover);
  }

  /* Pushed to the right edge by its own margin rather than by a growing path:
     neither the name nor the directory grows any more, and a row whose file
     sits at the repo root has no directory element at all to do it. */
  .stats {
    flex: 0 0 auto;
    margin-left: auto;
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
