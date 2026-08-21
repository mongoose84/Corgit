<script lang="ts">
  import Pane from './Pane.svelte';
  import GraphRow from './GraphRow.svelte';
  import DiffView from './DiffView.svelte';
  import EmptyState from '../EmptyState.svelte';
  import Glyph from '../Glyph.svelte';
  import Mascot from '../Mascot.svelte';
  import ContextMenu from '../ContextMenu.svelte';
  import CreateBranchDialog from '../CreateBranchDialog.svelte';
  import DeleteBranchDialog from '../DeleteBranchDialog.svelte';
  import { repos, isDirty } from '../repos.svelte';
  import { graph, type RefBadge } from '../graph.svelte';
  import { isUnmergedBranchRefusal } from '../gitErrors';
  import { notices } from '../notices.svelte';
  import { diff } from '../diff.svelte';
  import { laneCount as computeLaneCount, laneColorVar, ROW_HEIGHT, LANE_WIDTH } from '../graphLayout';

  // Commit selection drives the middle pane's Mode B in build step 7 (§5.2);
  // until then, clicking a row only highlights it here.
  const hasRepo = $derived(repos.selectedId !== undefined);
  const status = $derived(repos.selectedId ? repos.status(repos.selectedId) : undefined);
  const dirty = $derived(status !== undefined && isDirty(status));
  // Merging needs a destination, and Corgit's is always what is checked out.
  // On a detached HEAD there is no branch to merge into and no name to put in
  // the menu label, so the entry is simply not offered there.
  const currentBranch = $derived(status?.branch ?? null);

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
  let spacerEl: HTMLElement | undefined = $state();
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

  // Back to the working tree, which also shuts the info column (§5.2) — there
  // is no commit left for it to be about. Two ways in, because the
  // *Uncommitted Changes* node is the semantic one but only exists
  // `{#if dirty}`, so on a clean repo clicking past the last row is the only
  // one there is.
  function deselect() {
    graph.select('working-tree');
  }

  // Only a click that landed on the scroll container or the virtualization
  // spacer, i.e. the empty space past the last row. Testing the target rather
  // than relying on rows to stop propagation keeps this independent of
  // GraphRow's internals — and of the "Load more" button, which lives in the
  // same scroll box.
  function onBackgroundClick(event: MouseEvent) {
    if (event.target === scrollEl || event.target === spacerEl) deselect();
  }

  // Branch switching (§8.3, §8.4, build step 8) — double-click a ref badge
  // or pick one from a row's right-click menu; both funnel through here.
  //
  // One in-flight write at a time from this pane. The per-repo write queue
  // (§7) already serialises them on the Rust side; this exists so a second
  // click cannot queue a switch behind a merge whose result is not on screen
  // yet, which would leave the two failures fighting over one banner.
  let busy = $state(false);

  let menu = $state<{ x: number; y: number; refs: RefBadge[]; hash: string } | null>(null);
  // Non-null while the Create Branch dialog is up; the value is the start
  // point the new branch will be cut from (§8.3).
  let createFrom = $state<string | null>(null);
  // Non-null while the Delete Branch dialog is up (§8.3). `refusal` carries
  // git's "not fully merged" text once a safe delete has come back with one,
  // which is what turns the dialog's second step on; it lives here rather than
  // in the dialog because it *is* the failed write's error, and every other
  // write's error is the pane's to hold.
  let deleting = $state<{ name: string; refusal: string | null } | null>(null);

  // Esc shuts the info panel (§5.2) and leaves the row selected — it undoes
  // the *Show info* that opened the column, not the click that picked the row.
  // Guarded three ways: the panel has to be open; the graph has to be the view
  // on screen, since DiffView owns Esc while the diff tab is showing and one
  // press must not close two things; and the context menu gets it first. The
  // menu is the only overlay needing that check — both dialogs handle Esc on
  // the dialog element and stop it propagating, so it never reaches this
  // window listener while one is up.
  function onKeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape') return;
    if (diff.view !== 'graph' || menu !== null) return;
    if (graph.infoOpen) graph.closeInfo();
  }

  // Only local names: git rejects a new branch that collides with one, and the
  // remote-tracking badges sharing the graph are a different namespace.
  const localBranchNames = $derived(
    graph.refs.filter((ref) => ref.kind === 'local').map((ref) => ref.name),
  );

  async function switchTo(ref: RefBadge) {
    if (busy) return;
    busy = true;
    const ok = await repos.switchBranch(ref.name, ref.kind);
    // The banner is already up (§13); this only tells it something git's
    // stderr does not carry — that the tree was dirty when the checkout was
    // refused, which is what makes *Open in VS Code* the right way out.
    if (!ok && dirty) notices.overrideAction('open-vscode');
    busy = false;
  }

  // Merging from the graph (§8.3) — the badge names the source, the
  // destination is always the checked-out branch.
  //
  // No action override here, unlike `switchTo`: a merge tells you why it
  // failed in its own words every time — either git's "your local changes
  // would be overwritten", which `translateGitError` already answers with
  // *Open in VS Code*, or a conflict, which raises the blocking banner from
  // the status refresh instead. Forcing the dirty-tree action would override
  // the conflict case, and a conflict leaves the tree dirty by definition, so
  // it would override it exactly when it is wrong.
  async function mergeInto(ref: RefBadge) {
    if (busy) return;
    busy = true;
    await repos.mergeBranch(ref.name);
    busy = false;
  }

  async function createBranch(name: string, checkout: boolean): Promise<boolean> {
    const startPoint = createFrom;
    if (!startPoint) return false;

    const ok = await repos.createBranch(name, startPoint, checkout);
    // Only a checkout can fail on a dirty tree; a plain `git branch` never
    // touches the working tree, so offering VS Code there would be noise.
    if (!ok && checkout && dirty) notices.overrideAction('open-vscode');
    return ok;
  }

  // Deleting a local branch (§8.3). Safe-first: the dialog's first button
  // passes `force: false`, and the only way to reach `true` is through the
  // *Delete anyway* that appears once git has refused an unmerged branch.
  //
  // That refusal is the one write failure this pane takes off the banner: the
  // dialog is still up and already showing it, and a banner above the scrim
  // saying the same thing would be the same error twice. Every other failure
  // closes the dialog and leaves the banner to do its job.
  async function deleteBranch(force: boolean) {
    const target = deleting;
    if (!target || busy) return;
    busy = true;

    const ok = await repos.deleteBranch(target.name, force);
    // Classified, not displayed — hence `lastWriteError` rather than anything
    // the banner owns (§8.3: this is the one git failure Corgit answers with a
    // different button instead of a headline).
    const raw = repos.lastWriteError;
    if (ok) {
      deleting = null;
    } else if (!force && raw !== null && isUnmergedBranchRefusal(raw)) {
      notices.dismiss();
      deleting = { name: target.name, refusal: raw };
    } else {
      deleting = null;
    }
    busy = false;
  }

  // Every row has a menu now, badges or not: *Show info* is the only way into
  // the info column (§5.2), so a plain commit with no refs on it must still
  // open one. That is why the old `refs.length === 0` bail is gone.
  function openMenu(event: MouseEvent, refs: RefBadge[], hash: string) {
    event.preventDefault();
    menu = { x: event.clientX, y: event.clientY, refs, hash };
  }

  // *Show info* leads because it is the one entry every row has; the branch
  // entries below it exist only on the rows carrying a badge.
  function menuItems(refs: RefBadge[], hash: string) {
    return [
      { label: 'Show info', onSelect: () => graph.showInfo(hash) },
      ...refs.flatMap((ref) => [
        {
          label: ref.kind === 'local' ? `Switch to ${ref.name}` : `Switch to ${ref.name} (new local branch)`,
          onSelect: () => switchTo(ref),
        },
        {
          label: `Create branch from ${ref.name}…`,
          onSelect: () => (createFrom = ref.name),
        },
        // Merging a branch into itself is git's own no-op, so the badge for
        // the branch you are on does not offer it. Remote-tracking badges do:
        // merging `origin/main` into your branch is the same gesture, and the
        // one Pull does not cover when the branch you want is not upstream.
        ...(currentBranch !== null && !(ref.kind === 'local' && ref.name === currentBranch)
          ? [
              {
                label: `Merge ${ref.name} into ${currentBranch}`,
                onSelect: () => mergeInto(ref),
              },
            ]
          : []),
        // Local badges only, and never the one you are standing on: git
        // refuses to delete the checked-out branch, and deleting a remote
        // badge would be a `push --delete` — a network write with a different
        // blast radius, so it is not folded into the same entry (§8.3).
        ...(ref.kind === 'local' && ref.name !== currentBranch
          ? [
              {
                label: `Delete ${ref.name}`,
                onSelect: () => (deleting = { name: ref.name, refusal: null }),
              },
            ]
          : []),
      ]),
    ];
  }
</script>

<svelte:window onkeydown={onKeydown} />

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
           screen at once stops being charming. Its own `content` placement is
           the opposite case — a repo selected and clean — so the two can never
           both be showing.

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
            <Mascot pose="resting" height={132} gaze />
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
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <!-- The keyboard equivalent is Esc (`onKeydown`), which is why this
               needs no key handler of its own: there is nothing to focus here,
               the click target *is* the absence of a row. -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="scroll"
            bind:this={scrollEl}
            bind:clientHeight={viewportHeight}
            onscroll={onScroll}
            onclick={onBackgroundClick}
          >
            <div class="spacer" bind:this={spacerEl} style="height: {totalHeight}px">
              <div class="window" style="transform: translateY({topOffset}px)">
                {#each visibleRows as row (row.commit.hash)}
                  <GraphRow
                    {row}
                    laneCount={lanes}
                    refs={graph.refsByHash.get(row.commit.hash) ?? []}
                    selected={graph.selection === row.commit.hash}
                    {currentBranch}
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
  <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.refs, menu.hash)} onClose={() => (menu = null)} />
{/if}

{#if createFrom}
  <CreateBranchDialog
    startPoint={createFrom}
    existingLocal={localBranchNames}
    onCreate={createBranch}
    onClose={() => (createFrom = null)}
  />
{/if}

{#if deleting}
  <DeleteBranchDialog
    name={deleting.name}
    refusal={deleting.refusal}
    {busy}
    onDelete={deleteBranch}
    onClose={() => (deleting = null)}
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
