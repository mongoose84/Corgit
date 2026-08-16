<script lang="ts">
  import EmptyState from '../EmptyState.svelte';
  import Divider from '../Divider.svelte';
  import { diff, type DiffSource } from '../diff.svelte';
  import { repos } from '../repos.svelte';
  import { settings } from '../settings.svelte';
  import { DIFF_ROW_HEIGHT, type DiffRow } from '../diffLayout';

  // The right pane's second view (SPEC.md §5.4). Read-only, always — Corgit
  // does not edit diffs or resolve conflicts (§2, permanent non-goal), so
  // every case this cannot render ends at the same Open in VS Code button
  // rather than at a half-measure.

  const file = $derived(diff.file);
  const layout = $derived(diff.layout);
  const open = $derived(diff.open);

  /** Which two things are being compared, named rather than left as
   *  "old"/"new" — "index vs working tree" is the whole reason a file can be
   *  in both middle-pane sections at once with a different diff on each. */
  function sides(source: DiffSource): [string, string] {
    switch (source.kind) {
      case 'staged':
        return ['HEAD', 'Index'];
      case 'unstaged':
        return ['Index', 'Working tree'];
      case 'untracked':
        return ['Not in git', 'Working tree'];
      case 'commit':
        return [`${source.hash.slice(0, 7)}~`, source.hash.slice(0, 7)];
    }
  }

  const captions = $derived(open ? sides(open.source) : ['', '']);

  /**
   * Open this file in VS Code (§5.4) — the escape hatch for everything this
   * viewer deliberately does not do: editing, resolving, and the binary and
   * oversized files it cannot render at all.
   *
   * The line number is only passed for a working-tree source, where the new
   * side is the file on disk. A commit's new side is some older revision, so
   * its line numbers would land the reader on whatever happens to sit there
   * today — worse than opening at the top and saying nothing.
   */
  function openInVSCode() {
    if (!open) return;
    const line = open.source.kind === 'commit' ? undefined : firstChangedLine(diff.layout.rows);
    void repos.openInVSCode(open.repoId, { path: open.path, line });
  }

  function firstChangedLine(rows: DiffRow[]): number | undefined {
    for (const row of rows) {
      if (row.kind === 'pair' && row.changed && row.newNo !== null) return row.newNo;
    }
    return undefined;
  }

  // Virtualized exactly like the graph's rows (§5.3): a 20k-line diff is the
  // case the backend's cap exists for, and even a legitimate 5k-line one is
  // 5k DOM rows. Uniform height is what makes this possible, which is in turn
  // why lines never wrap — see `.text` below.
  const OVERSCAN = 12;
  let scrollEl: HTMLElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportHeight = $state(0);
  // `clientWidth`, so it excludes the vertical scrollbar. The sash is placed
  // against this same number, which is what keeps the line it draws sitting
  // exactly on the boundary between the two columns rather than ~5px off it.
  let viewportWidth = $state(0);

  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / DIFF_ROW_HEIGHT) - OVERSCAN));
  const endIndex = $derived(
    Math.min(
      layout.rows.length,
      Math.ceil((scrollTop + viewportHeight) / DIFF_ROW_HEIGHT) + OVERSCAN,
    ),
  );
  const visibleRows = $derived(layout.rows.slice(startIndex, endIndex));
  const topOffset = $derived(startIndex * DIFF_ROW_HEIGHT);
  const totalHeight = $derived(layout.rows.length * DIFF_ROW_HEIGHT);

  // The old/new split (§5.4). Both columns are sized against the *pane*, never
  // against the longest line: sizing them to the content is what pushed the new
  // side off the right edge entirely on any file with real code in it, so the
  // two columns could not be read together — which is the only reason to show
  // them side by side.
  const MIN_COLUMN = 120;
  const leftColumn = $derived(
    viewportWidth > 0 ? Math.round(clampColumn(settings.diffSplit * viewportWidth)) : 0,
  );

  /** The upper bound is itself floored at `MIN_COLUMN`: below 240px of pane
   *  the two minimums cannot both hold, and letting the clamp invert would
   *  hand CSS a negative flex-basis it silently drops. */
  function clampColumn(px: number): number {
    return clamp(px, MIN_COLUMN, Math.max(MIN_COLUMN, viewportWidth - MIN_COLUMN));
  }

  /** Whichever column is smaller bounds how far the shared offset has to be
   *  able to travel, since the longest line must be reachable in both. */
  const narrowColumn = $derived(Math.min(leftColumn, Math.max(0, viewportWidth - leftColumn)));

  function clamp(value: number, min: number, max: number): number {
    return Math.min(Math.max(value, min), max);
  }

  function dragSplit(clientX: number) {
    if (!scrollEl || viewportWidth <= 0) return;
    const origin = scrollEl.getBoundingClientRect().left;
    settings.diffSplit = clampColumn(clientX - origin) / viewportWidth;
  }

  // Long lines are reached by scrolling, not by widening the column, so the
  // horizontal scrollbar is its own strip rather than the row container's:
  // each column clips its text, and one shared offset moves both. Shared
  // deliberately — comparing two lines means seeing the same columns of
  // characters on each side, and independent offsets break exactly that.
  let hbarEl: HTMLElement | undefined = $state();
  let hOffset = $state(0);

  // Scroll back to the start of both axes whenever the open file changes — a
  // new diff inheriting the last one's scroll position lands the reader
  // mid-file with no idea why.
  $effect(() => {
    void open?.path;
    void open?.source;
    if (scrollEl) scrollEl.scrollTop = 0;
    if (hbarEl) hbarEl.scrollLeft = 0;
    scrollTop = 0;
    hOffset = 0;
  });

  function onScroll() {
    if (scrollEl) scrollTop = scrollEl.scrollTop;
  }

  function onHScroll() {
    if (hbarEl) hOffset = hbarEl.scrollLeft;
  }

  // Scrolling sideways over the rows themselves, which is what anyone tries
  // first and what the strip alone does not serve. Both gestures have to be
  // handled: a trackpad swipe arrives as `deltaX`, while shift+wheel arrives
  // as `deltaY` on some platforms and as `deltaX` on others — reading only one
  // of them makes the feature look broken on whichever input you own.
  function onWheel(event: WheelEvent) {
    if (!hbarEl) return;
    const delta = event.deltaX !== 0 ? event.deltaX : event.shiftKey ? event.deltaY : 0;
    if (delta === 0) return;

    const before = hbarEl.scrollLeft;
    hbarEl.scrollLeft += delta;
    // Only swallow the event if it actually moved something; at either end of
    // the range the gesture belongs to whatever is behind us.
    if (hbarEl.scrollLeft !== before) event.preventDefault();
  }

  // Guarded on the view: this component stays mounted behind the graph tab
  // (§5.4), and Esc while reading the graph must not quietly close a diff that
  // is not even on screen.
  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && diff.view === 'diff') diff.close();
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if diff.loading && !file}
  <EmptyState message="Reading diff…" />
{:else if diff.error}
  <EmptyState message="Could not read this diff" hint={diff.error} />
{:else if file}
  <!-- Row height comes from the layout module rather than the stylesheet: the
       virtualization arithmetic above and the rendered height must be the same
       number, and two places to edit is one place to get it wrong. -->
  <div
    class="diff-body"
    style="--col-left: {leftColumn}px; --col-narrow: {narrowColumn}px; --h-offset: {hOffset}px; --line-width: {layout.maxWidth}ch; --diff-row-height: {DIFF_ROW_HEIGHT}px"
  >
    <div class="file-header">
      <span class="path selectable" title={file.path}>{file.path}</span>
      <span class="stats">
        <span class="stat-add">+{file.insertions}</span>
        <span class="stat-del">−{file.deletions}</span>
      </span>
      <!-- Always here, not only in the states this viewer cannot render: it is
           the one action a read-only view can offer, and moving it around
           between states would make it something to look for. -->
      <button type="button" class="vscode" title="Open {file.path} in VS Code" onclick={openInVSCode}>
        Open in VS Code
      </button>
    </div>

    {#if file.binary}
      <EmptyState
        message="Binary file"
        hint="There is nothing to compare line by line — open it in VS Code instead."
      />
    {:else if layout.rows.length === 0}
      <EmptyState message="No changes" hint="This file matches the other side exactly." />
    {:else}
      {#if file.truncated}
        <div class="notice">
          <p>This diff is too large to show in full — it stops partway through the file.</p>
        </div>
      {/if}

      <div class="split">
        <div class="captions">
          <span class="caption">{captions[0]}</span>
          <span class="caption">{captions[1]}</span>
        </div>

        <div
          class="scroll"
          bind:this={scrollEl}
          bind:clientHeight={viewportHeight}
          bind:clientWidth={viewportWidth}
          onscroll={onScroll}
          onwheel={onWheel}
        >
          <div class="spacer" style="height: {totalHeight}px">
            <div class="window" style="transform: translateY({topOffset}px)">
              {#each visibleRows as row, index (startIndex + index)}
                {#if row.kind === 'gap'}
                  <div class="row gap">
                    <span class="gap-label"
                      >⋯ {row.skipped} unchanged {row.skipped === 1 ? 'line' : 'lines'}</span
                    >
                  </div>
                {:else}
                  <div class="row">
                    <div class="side" class:removed={row.changed && row.oldText !== null} class:filler={row.oldText === null}>
                      <span class="gutter">{row.oldNo ?? ''}</span>
                      <!-- The clip is its own element so the horizontal offset
                           cannot slide text over the line numbers, which stay
                           pinned to the column's left edge. -->
                      <span class="clip"><span class="text selectable">{row.oldText ?? ''}</span></span>
                    </div>
                    <div class="side right" class:added={row.changed && row.newText !== null} class:filler={row.newText === null}>
                      <span class="gutter">{row.newNo ?? ''}</span>
                      <span class="clip"><span class="text selectable">{row.newText ?? ''}</span></span>
                    </div>
                  </div>
                {/if}
              {/each}
            </div>
          </div>
        </div>

        <!-- Overlaid rather than a real column, because the columns live inside
             the scroller and a divider in there would scroll away with them. -->
        <div class="sash" style="left: {leftColumn}px">
          <Divider
            label="Resize the diff columns"
            value={Math.round((leftColumn / Math.max(1, viewportWidth)) * 100)}
            ondrag={dragSplit}
            onrelease={() => void settings.flush()}
            onreset={() => settings.resetLayout()}
          />
        </div>
      </div>

      <!-- A scrollbar with nothing in it: the strip exists only to give the
           shared horizontal offset a native control and a native thumb. Its
           range is the longest line minus the *narrower* column, so the end of
           that line is reachable in whichever column is smaller. -->
      <div class="hbar" bind:this={hbarEl} onscroll={onHScroll}>
        <div class="hbar-content"></div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .diff-body {
    --diff-gutter: 48px;
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .file-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    flex: 0 0 auto;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
  }

  .path {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    /* Tail-trimmed here, unlike a file row's head-first ellipsis (§5.2): the
       tab already carries the filename, so what this line adds is the
       directory it lives in. */
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .stats {
    display: flex;
    flex: 0 0 auto;
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

  .notice {
    flex: 0 0 auto;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
    background: var(--bg-raised);
  }

  .notice p {
    margin: 0;
    min-width: 0;
    font-size: var(--text-sm);
    color: var(--status-dirty);
  }

  /* Same treatment as the conflict banner's buttons (CommitPane.svelte) —
     neutral, not the accent, which is reserved for selection and primary
     actions (§11 rule 3). Opening an external editor is neither. */
  .vscode {
    flex: 0 0 auto;
    height: 22px;
    padding: 0 var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-primary);
    background: var(--bg-hover);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
  }

  .vscode:hover {
    background: var(--bg-active);
  }

  /* The positioning context for the sash, and the only element that spans
     exactly the captions plus the rows — which is the height the divider has
     to cover for the boundary to read as one line. */
  .split {
    position: relative;
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
  }

  .captions {
    display: flex;
    flex: 0 0 auto;
    border-bottom: 1px solid var(--border);
  }

  .caption {
    /* Sized off the same `--col-left` the rows use, so the captions stay over
       their own columns as the split moves. */
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    padding: 2px var(--space-3);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .caption:first-child {
    flex: 0 0 var(--col-left);
  }

  .sash {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    /* Above the rows so its widened hit area is reachable, but it is one pixel
       wide and the rows underneath stay selectable either side of it. */
    z-index: 1;
  }

  .scroll {
    flex: 1 1 auto;
    min-height: 0;
    /* Vertical only. Horizontal lives on `.hbar`, because the columns have to
       stay at their pane-relative widths rather than growing to fit a line. */
    overflow-y: auto;
    overflow-x: hidden;
  }

  .spacer {
    position: relative;
    width: 100%;
  }

  .window {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
  }

  .row {
    display: flex;
    height: var(--diff-row-height);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: var(--diff-row-height);
  }

  .side {
    display: flex;
    /* The dragged split, in pixels of the pane — never of the content. The
       right column takes whatever is left, so the two always add up to the
       viewport exactly and no rounding gap opens between them. */
    flex: 0 0 var(--col-left);
    min-width: 0;
    overflow: hidden;
  }

  .side.right {
    flex: 1 1 auto;
  }

  .gutter {
    flex: 0 0 var(--diff-gutter);
    padding-right: var(--space-2);
    text-align: right;
    font-size: var(--text-xs);
    color: var(--diff-gutter-text);
    font-variant-numeric: tabular-nums;
    user-select: none;
  }

  .clip {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
  }

  .text {
    /* `block`, not the default inline: a transform does not apply to a
       non-replaced inline box, so the offset would silently do nothing. */
    display: block;
    width: max-content;
    transform: translateX(calc(-1 * var(--h-offset)));
    /* Never wraps: uniform row height is what makes virtualization possible,
       and a wrapped row would silently break the two columns' alignment. The
       horizontal offset above is how a long line is read instead. */
    white-space: pre;
    color: var(--text-primary);
  }

  .side.removed {
    background: var(--diff-del-bg);
  }

  .side.removed .gutter {
    background: var(--diff-del-gutter);
  }

  .side.added {
    background: var(--diff-add-bg);
  }

  .side.added .gutter {
    background: var(--diff-add-gutter);
  }

  /* No line exists on this side at all — it has to read as absent rather than
     as an empty line of code, so it is flat and has no line number. */
  .side.filler {
    background: var(--diff-filler-bg);
  }

  .row.gap {
    align-items: center;
    justify-content: center;
    background: var(--bg-raised);
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }

  .gap-label {
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .hbar {
    flex: 0 0 auto;
    /* 11px, not 10: `box-sizing: border-box` is global, so the 1px border
       would otherwise eat into the 10px the scrollbar itself needs and the
       thumb would render clipped. */
    height: 11px;
    overflow-x: auto;
    overflow-y: hidden;
    border-top: 1px solid var(--border);
  }

  .hbar-content {
    /* `ch` is resolved against *this* element's font, so it has to be the font
       the rows are drawn in — inheriting the UI font here would measure the
       longest line in the wrong units and stop the strip short of the end of
       it. Nothing is drawn; only the width matters. */
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    height: 1px;
    /* The strip is the pane's width, so its own scrollable overhang is exactly
       how far the text must travel: the longest line, less the narrower of the
       two columns. `max()` keeps it at zero — no thumb at all — for a file
       whose lines already fit. */
    width: calc(
      100% +
        max(
          0px,
          var(--diff-gutter) + var(--line-width) + var(--space-3) - var(--col-narrow)
        )
    );
  }
</style>
