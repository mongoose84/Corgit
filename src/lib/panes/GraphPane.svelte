<script lang="ts">
  import Pane from './Pane.svelte';
  import GraphRow from './GraphRow.svelte';
  import DiffView from './DiffView.svelte';
  import EmptyState from '../EmptyState.svelte';
  import Glyph from '../Glyph.svelte';
  import Mascot from '../Mascot.svelte';
  import ContextMenu from '../ContextMenu.svelte';
  import CreateBranchDialog from '../CreateBranchDialog.svelte';
  import GitErrorNotice from '../GitErrorNotice.svelte';
  import { repos, isDirty } from '../repos.svelte';
  import { graph, type RefBadge } from '../graph.svelte';
  import { diff } from '../diff.svelte';
  import { laneCount as computeLaneCount, laneColorVar, ROW_HEIGHT, LANE_WIDTH } from '../graphLayout';

  // Commit selection drives the middle pane's Mode B in build step 7 (§5.2);
  // until then, clicking a row only highlights it here.
  const hasRepo = $derived(repos.selectedId !== undefined);
  const status = $derived(repos.selectedId ? repos.status(repos.selectedId) : undefined);
  const dirty = $derived(status !== undefined && isDirty(status));

  // Guards the diff against this effect's own re-runs. The effect re-fires
  // whenever anything `graph.loadFor` touches changes, not only on a repo
  // change — harmless for the graph, since `loadFor` returns early, but a
  // bare `diff.close()` here would shut a diff the user had just opened.
  let lastRepoId: string | undefined = undefined;

  $effect(() => {
    const id = repos.selectedId;
    if (id) void graph.loadFor(id);
    else graph.clear();
    if (id !== lastRepoId) {
      lastRepoId = id;
      // The open diff names a file in the repo we just left; it may not even
      // exist in this one (§5.4).
      diff.close();
    }
  });

  // The tab strip (§5.4). Only ever labelled by the filename — the directory
  // is in the diff's own header, where there is room for it.
  const fileName = $derived(diff.open ? (diff.open.path.split('/').pop() ?? diff.open.path) : '');

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
  // Shared by switching and branch creation: both are `write()` calls whose
  // failure is a line of git stderr, and only one of them can be in flight.
  let actionError = $state<string | null>(null);
  // Only true alongside `actionError` when the tree was dirty at the moment
  // of failure — the one case §8.3 says to offer *Open in VS Code* for.
  // Never force-checkout, ever, so there is no other action to offer here.
  let actionErrorDirty = $state(false);

  let menu = $state<{ x: number; y: number; refs: RefBadge[] } | null>(null);
  // Non-null while the Create Branch dialog is up; the value is the start
  // point the new branch will be cut from (§8.3).
  let createFrom = $state<string | null>(null);

  // Only local names: git rejects a new branch that collides with one, and the
  // remote-tracking badges sharing the graph are a different namespace.
  const localBranchNames = $derived(
    graph.refs.filter((ref) => ref.kind === 'local').map((ref) => ref.name),
  );

  async function switchTo(ref: RefBadge) {
    if (switching) return;
    switching = true;
    actionError = null;
    actionErrorDirty = false;
    const ok = await repos.switchBranch(ref.name, ref.kind);
    if (!ok) {
      actionError = repos.writeError;
      actionErrorDirty = dirty;
    }
    switching = false;
  }

  async function createBranch(name: string, checkout: boolean): Promise<boolean> {
    const startPoint = createFrom;
    if (!startPoint) return false;

    actionError = null;
    actionErrorDirty = false;
    const ok = await repos.createBranch(name, startPoint, checkout);
    if (!ok) {
      actionError = repos.writeError;
      // Only a checkout can fail on a dirty tree; a plain `git branch` never
      // touches the working tree, so offering VS Code there would be noise.
      actionErrorDirty = checkout && dirty;
    }
    return ok;
  }

  function openMenu(event: MouseEvent, refs: RefBadge[]) {
    if (refs.length === 0) return;
    event.preventDefault();
    menu = { x: event.clientX, y: event.clientY, refs };
  }

  function menuItems(refs: RefBadge[]) {
    return refs.flatMap((ref) => [
      {
        label: ref.kind === 'local' ? `Switch to ${ref.name}` : `Switch to ${ref.name} (new local branch)`,
        onSelect: () => switchTo(ref),
      },
      {
        label: `Create branch from ${ref.name}…`,
        onSelect: () => (createFrom = ref.name),
      },
    ]);
  }
</script>

<Pane title="Graph">
  {#snippet tabs()}
    <button
      type="button"
      class="tab"
      class:active={diff.view === 'graph'}
      role="tab"
      aria-selected={diff.view === 'graph'}
      onclick={() => diff.select('graph')}
    >Graph</button>
    {#if diff.open}
      <!-- Two buttons rather than a close button nested inside the tab: a
           button inside a button is invalid, and the close target has to be
           separately clickable anyway. -->
      <div class="tab tab-file" class:active={diff.view === 'diff'}>
        <button
          type="button"
          class="tab-label"
          role="tab"
          aria-selected={diff.view === 'diff'}
          title={diff.open.path}
          onclick={() => diff.select('diff')}
        >{fileName}</button>
        <button
          type="button"
          class="tab-close"
          title="Close diff"
          aria-label="Close diff"
          onclick={() => diff.close()}
        >
          <Glyph kind="cross" />
        </button>
      </div>
    {/if}
  {/snippet}

  <!-- Both views stay mounted and laid out (§5.4), stacked rather than swapped:
       each is a scroll container, and the graph carries scroll position and
       however many pages the user loaded. Unmounting it — or even `display:
       none`, which destroys the scroll box — would make a glance at a diff
       cost a scroll back down to where they were. -->
  <div class="views">
  <div class="view" class:hidden={diff.view !== 'graph'}>
    {#if !hasRepo}
      <!-- The dog lives here (SPEC §14.1). The commit pane sits empty at the
           same moment and is deliberately left bare, because two of him on
           screen at once stops being charming.

           Two poses share the slot: he lies down once the whole herd is clean
           and in sync, and sits up waiting otherwise (docs/mascot.md §5). -->
      {#if repos.allClean}
        <EmptyState message="All in sync" hint="Nothing needs you — select a repository to browse its history">
          {#snippet art()}
            <Mascot pose="content" height={112} />
          {/snippet}
        </EmptyState>
      {:else}
        <EmptyState message="Nothing to herd" hint="Select a repository to see its history">
          {#snippet art()}
            <Mascot pose="resting" height={132} />
          {/snippet}
        </EmptyState>
      {/if}
    {:else if graph.loading && graph.rows.length === 0}
      <EmptyState message="Reading history…" />
    {:else if graph.error}
      <EmptyState message="Could not read history" hint={graph.error} />
    {:else}
      <!-- Its own flex column, mirroring Pane's internal layout, so `.scroll`
           gets a definite height to virtualize against regardless of whether
           the Uncommitted Changes node is showing above it. -->
      <div class="graph-body">
        {#if actionError}
          <div class="action-error">
            <GitErrorNotice
              error={actionError}
              forceAction={actionErrorDirty ? 'open-vscode' : null}
              onOpenVSCode={() => repos.openInVSCode()}
              onDismiss={() => (actionError = null)}
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
                    headHash={status?.head ?? null}
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
  </div>

  {#if diff.open}
    <div class="view" class:hidden={diff.view !== 'diff'}>
      <DiffView />
    </div>
  {/if}
  </div>
</Pane>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.refs)} onClose={() => (menu = null)} />
{/if}

{#if createFrom}
  <CreateBranchDialog
    startPoint={createFrom}
    existingLocal={localBranchNames}
    onCreate={createBranch}
    onClose={() => (createFrom = null)}
  />
{/if}

<style>
  .views {
    position: relative;
    height: 100%;
  }

  /* Stacked, not swapped — see the markup. `visibility: hidden` is doing real
     work here: it keeps the box laid out (so the hidden view's scroll offset
     and virtualization measurements survive) while taking it out of the paint,
     out of hit-testing and out of the tab order. */
  .view {
    position: absolute;
    inset: 0;
  }

  .view.hidden {
    visibility: hidden;
  }

  /* Tabs read as a segmented control rather than browser tabs — the pane
     header is 34 px and already carries a border, so a full tab shape would
     be two competing edges. The active one is marked by surface lightness,
     matching §11 rule 2, not by the accent (rule 3: selection in the *lists*
     is what the accent is for). */
  .tab {
    display: flex;
    align-items: center;
    flex: 0 1 auto;
    min-width: 0;
    height: 22px;
    padding: 0 var(--space-2);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .tab:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .tab.active {
    color: var(--text-primary);
    background: var(--bg-raised);
    border-color: var(--border-strong);
  }

  /* The filename keeps its own case — it is a name, not a label. */
  .tab-file {
    gap: var(--space-1);
    padding-right: var(--space-1);
    text-transform: none;
    letter-spacing: 0;
  }

  .tab-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    font-family: var(--font-mono);
  }

  /* A drawn cross, not a `×` character — see Glyph.svelte for why. Centring a
     block child is exact; centring a glyph's line box is not. */
  .tab-close {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
  }

  .tab-close:hover {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  .graph-body {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .action-error {
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
