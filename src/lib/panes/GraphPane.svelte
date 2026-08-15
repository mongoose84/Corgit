<script lang="ts">
  import Pane from './Pane.svelte';
  import GraphRow from './GraphRow.svelte';
  import EmptyState from '../EmptyState.svelte';
  import ContextMenu from '../ContextMenu.svelte';
  import GitErrorNotice from '../GitErrorNotice.svelte';
  import { repos, isDirty } from '../repos.svelte';
  import { graph, type RefBadge } from '../graph.svelte';
  import { laneCount as computeLaneCount, laneColorVar, ROW_HEIGHT, LANE_WIDTH } from '../graphLayout';

  // Commit selection drives the middle pane's Mode B in build step 7 (§5.2);
  // until then, clicking a row only highlights it here.
  const hasRepo = $derived(repos.selectedId !== undefined);
  const status = $derived(repos.selectedId ? repos.status(repos.selectedId) : undefined);
  const dirty = $derived(status !== undefined && isDirty(status));

  $effect(() => {
    const id = repos.selectedId;
    if (id) void graph.loadFor(id);
    else graph.clear();
  });

  const lanes = $derived(Math.max(1, computeLaneCount(graph.rows)));

  // Virtualized rows (§5.3): only the rows within the scrolled viewport (plus
  // a small overscan) exist in the DOM. Lane layout runs once over the whole
  // loaded set in graph.svelte.ts; this only slices the precomputed result.
  const OVERSCAN = 8;
  let scrollEl: HTMLElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportHeight = $state(0);

  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN));
  const endIndex = $derived(
    Math.min(graph.rows.length, Math.ceil((scrollTop + viewportHeight) / ROW_HEIGHT) + OVERSCAN),
  );
  const visibleRows = $derived(graph.rows.slice(startIndex, endIndex));
  const topOffset = $derived(startIndex * ROW_HEIGHT);
  const totalHeight = $derived(graph.rows.length * ROW_HEIGHT);

  function onScroll() {
    if (scrollEl) scrollTop = scrollEl.scrollTop;
  }

  // Branch switching (§8.3, §8.4, build step 8) — double-click a ref badge
  // or pick one from a row's right-click menu; both funnel through here.
  let switching = $state(false);
  let switchError = $state<string | null>(null);
  // Only true alongside `switchError` when the tree was dirty at the moment
  // of failure — the one case §8.3 says to offer *Open in VS Code* for.
  // Never force-checkout, ever, so there is no other action to offer here.
  let switchErrorDirty = $state(false);

  let menu = $state<{ x: number; y: number; refs: RefBadge[] } | null>(null);

  async function switchTo(ref: RefBadge) {
    if (switching) return;
    switching = true;
    switchError = null;
    switchErrorDirty = false;
    const ok = await repos.switchBranch(ref.name, ref.kind);
    if (!ok) {
      switchError = repos.writeError;
      switchErrorDirty = dirty;
    }
    switching = false;
  }

  function openMenu(event: MouseEvent, refs: RefBadge[]) {
    if (refs.length === 0) return;
    event.preventDefault();
    menu = { x: event.clientX, y: event.clientY, refs };
  }

  function menuItems(refs: RefBadge[]) {
    return refs.map((ref) => ({
      label: ref.kind === 'local' ? `Switch to ${ref.name}` : `Switch to ${ref.name} (new local branch)`,
      onSelect: () => switchTo(ref),
    }));
  }
</script>

<Pane title="Graph">
  {#if !hasRepo}
    <EmptyState message="No repository selected" hint="Select a repository to see its history" />
  {:else if graph.loading && graph.rows.length === 0}
    <EmptyState message="Reading history…" />
  {:else if graph.error}
    <EmptyState message="Could not read history" hint={graph.error} />
  {:else}
    <!-- Its own flex column, mirroring Pane's internal layout, so `.scroll`
         gets a definite height to virtualize against regardless of whether
         the Uncommitted Changes node is showing above it. -->
    <div class="graph-body">
      {#if switchError}
        <div class="switch-error">
          <GitErrorNotice
            error={switchError}
            forceAction={switchErrorDirty ? 'open-vscode' : null}
            onOpenVSCode={() => repos.openInVSCode()}
            onDismiss={() => (switchError = null)}
          />
        </div>
      {/if}

      {#if dirty}
        <button
          type="button"
          class="uncommitted"
          class:selected={graph.selection === 'working-tree'}
          onclick={() => graph.select('working-tree')}
        >
          <svg class="lanes" width={LANE_WIDTH} height={ROW_HEIGHT} viewBox="0 0 {LANE_WIDTH} {ROW_HEIGHT}">
            {#if graph.rows.length > 0}
              <line
                x1={LANE_WIDTH / 2}
                y1={ROW_HEIGHT / 2}
                x2={LANE_WIDTH / 2}
                y2={ROW_HEIGHT}
                stroke={laneColorVar(0)}
              />
            {/if}
            <circle cx={LANE_WIDTH / 2} cy={ROW_HEIGHT / 2} r="4" fill={laneColorVar(0)} />
          </svg>
          <span class="subject">Uncommitted Changes</span>
        </button>
      {/if}

      {#if graph.rows.length === 0}
        <EmptyState message="No commits yet" hint="Make the first commit to see history here" />
      {:else}
        <div class="scroll" bind:this={scrollEl} bind:clientHeight={viewportHeight} onscroll={onScroll}>
          <div class="spacer" style="height: {totalHeight}px">
            <div class="window" style="transform: translateY({topOffset}px)">
              {#each visibleRows as row (row.commit.hash)}
                <GraphRow
                  {row}
                  laneCount={lanes}
                  refs={graph.refsByHash.get(row.commit.hash) ?? []}
                  selected={graph.selection === row.commit.hash}
                  currentBranch={status?.branch ?? null}
                  onSelect={() => graph.select(row.commit.hash)}
                  onSwitchBranch={switchTo}
                  onContextMenu={openMenu}
                />
              {/each}
            </div>
          </div>
          {#if graph.hasMore}
            <button type="button" class="load-more" disabled={graph.loadingMore} onclick={() => graph.loadMore()}>
              {graph.loadingMore ? 'Loading…' : 'Load more'}
            </button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</Pane>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.refs)} onClose={() => (menu = null)} />
{/if}

<style>
  .graph-body {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .switch-error {
    flex: 0 0 auto;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
    background: var(--bg-raised);
  }

  .uncommitted {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: var(--row-height);
    flex: 0 0 auto;
    padding: 0 var(--space-3);
    border: 0;
    border-bottom: 1px solid var(--border);
    background: none;
    text-align: left;
    cursor: default;
  }

  .uncommitted:hover {
    background: var(--bg-hover);
  }

  .uncommitted.selected {
    background: var(--accent-muted);
  }

  .uncommitted .lanes {
    flex: 0 0 auto;
  }

  .uncommitted .subject {
    font-size: var(--text-sm);
    color: var(--text-primary);
  }

  .scroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .spacer {
    position: relative;
  }

  .window {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
  }

  .load-more {
    display: block;
    width: 100%;
    height: var(--row-height);
    border: 0;
    border-top: 1px solid var(--border);
    background: none;
    color: var(--text-muted);
    font-size: var(--text-sm);
    cursor: default;
  }

  .load-more:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .load-more:disabled {
    color: var(--text-disabled);
  }
</style>
