<script lang="ts">
  import Divider from './lib/Divider.svelte';
  import Welcome from './lib/Welcome.svelte';
  import RepoList from './lib/panes/RepoList.svelte';
  import CommitPane from './lib/panes/CommitPane.svelte';
  import GraphPane from './lib/panes/GraphPane.svelte';
  import CommitInfoPanel from './lib/panes/CommitInfoPanel.svelte';
  import { repos } from './lib/repos.svelte';
  import { graph } from './lib/graph.svelte';
  import { settings, DEFAULT_PANE_WIDTHS } from './lib/settings.svelte';

  // Minimum usable widths (SPEC.md §4). Below these the panes stop being
  // readable, so they win over the stored fractions.
  const MIN_LEFT = 190;
  const MIN_MIDDLE = 240;
  const MIN_GRAPH = 320;
  const DIVIDER = 5;

  // The commit info panel (§5.2 revised): a fourth, fixed-width column that
  // opens beside the graph on a commit selection — not a resizable pane, no
  // stored fraction, matching the fixed width of a Google Photos-style info
  // panel. The graph's `1fr` track absorbs the difference on its own; the
  // only extra bookkeeping needed is making sure `resolve()` still leaves it
  // MIN_GRAPH when the panel is open (see `reserved` below).
  const INFO_WIDTH = 320;
  const infoOpen = $derived(graph.selection !== 'working-tree');
  const infoWidth = $derived(infoOpen ? INFO_WIDTH : 0);

  let container: HTMLElement | undefined = $state();
  let width = $state(0);

  const usable = $derived(Math.max(1, width - DIVIDER * 2));
  const px = $derived(resolve(usable, settings.paneWidths.left, settings.paneWidths.middle, infoWidth));

  /**
   * Widths are stored as fractions so they survive window resizing, but they
   * are applied as pixels under the minimums. When the window is too narrow to
   * honour both, the middle pane yields first, then the left. `reserved` is
   * the info panel's current width — left/middle keep their stored fractions
   * exactly (opening the panel must not itself resize them), but the excess
   * check additionally protects the graph's minimum against it.
   */
  function resolve(total: number, leftFrac: number, middleFrac: number, reserved: number) {
    let left = Math.max(MIN_LEFT, Math.round(total * leftFrac));
    let middle = Math.max(MIN_MIDDLE, Math.round(total * middleFrac));

    let excess = left + middle + MIN_GRAPH + reserved - total;
    if (excess > 0) {
      const fromMiddle = Math.min(excess, middle - MIN_MIDDLE);
      middle -= fromMiddle;
      excess -= fromMiddle;
      left -= Math.min(excess, left - MIN_LEFT);
    }
    return { left, middle };
  }

  function originX(): number {
    return container?.getBoundingClientRect().left ?? 0;
  }

  function dragLeft(clientX: number) {
    const max = Math.max(MIN_LEFT, usable - px.middle - MIN_GRAPH - infoWidth);
    const left = clamp(clientX - originX(), MIN_LEFT, max);
    settings.paneWidths = { ...settings.paneWidths, left: left / usable };
  }

  function dragMiddle(clientX: number) {
    const max = Math.max(MIN_MIDDLE, usable - px.left - MIN_GRAPH - infoWidth);
    const middle = clamp(clientX - originX() - px.left - DIVIDER, MIN_MIDDLE, max);
    settings.paneWidths = { ...settings.paneWidths, middle: middle / usable };
  }

  function clamp(value: number, min: number, max: number): number {
    return Math.min(Math.max(value, min), max);
  }

  function reset() {
    settings.paneWidths = { ...DEFAULT_PANE_WIDTHS };
    void settings.flush();
  }

  // Settings first: the pane widths are needed for the very first paint, and
  // the welcome screen reads the recent-roots list from them.
  void settings.load().then(() => repos.start());
  void graph.start();
</script>

{#if !repos.ready}
  <!-- Deliberately blank rather than a spinner. Startup is a few milliseconds
       of reading two files; a spinner would only ever be seen as a flash. -->
  <div class="booting"></div>
{:else if repos.root === null}
  <Welcome />
{:else}
  <main
    bind:this={container}
    bind:clientWidth={width}
    style="--pane-left: {px.left}px; --pane-middle: {px.middle}px; --pane-info: {infoWidth}px; --divider: {DIVIDER}px"
  >
    <RepoList />
    <Divider
      label="Resize repository list"
      value={Math.round((px.left / usable) * 100)}
      ondrag={dragLeft}
      onrelease={() => void settings.flush()}
      onreset={reset}
    />
    <CommitPane />
    <Divider
      label="Resize commit pane"
      value={Math.round(((px.left + px.middle) / usable) * 100)}
      ondrag={dragMiddle}
      onrelease={() => void settings.flush()}
      onreset={reset}
    />
    <GraphPane />
    {#if infoOpen}
      <CommitInfoPanel />
    {/if}
  </main>
{/if}

<style>
  main {
    display: grid;
    grid-template-columns: var(--pane-left) var(--divider) var(--pane-middle) var(--divider) 1fr var(--pane-info);
    /* Only the info column's track actually changes at runtime (0 ↔ 320px);
       the graph's `1fr` reflows for free as a side effect of that track
       changing, no JS width recalculation needed for it (§5.2 revised). */
    transition: grid-template-columns 180ms ease;
    height: 100%;
    background: var(--bg-app);
  }

  .booting {
    height: 100%;
    background: var(--bg-app);
  }
</style>
