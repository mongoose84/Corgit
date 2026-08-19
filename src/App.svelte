<script lang="ts">
  import Divider from './lib/Divider.svelte';
  import TitleBar from './lib/TitleBar.svelte';
  import Welcome from './lib/Welcome.svelte';
  import RepoList from './lib/panes/RepoList.svelte';
  import CommitPane from './lib/panes/CommitPane.svelte';
  import GraphPane from './lib/panes/GraphPane.svelte';
  import CommitInfoPanel from './lib/panes/CommitInfoPanel.svelte';
  import { repos } from './lib/repos.svelte';
  import { graph } from './lib/graph.svelte';
  import { diff } from './lib/diff.svelte';
  import { settings } from './lib/settings.svelte';
  import { paneVisibility, startMenuListener } from './lib/menu.svelte';
  import { startWindowFrame } from './lib/windowFrame.svelte';

  // Minimum usable widths (SPEC.md §4). Below these the panes stop being
  // readable, so they win over the stored fractions.
  const MIN_LEFT = 190;
  const MIN_MIDDLE = 240;
  const MIN_GRAPH = 320;
  const DIVIDER = 5;

  // The commit info panel (§5.2 revised): a fourth, fixed-width column that
  // opens beside the graph on a row's right-click ▸ Info — not a resizable
  // pane, no stored fraction, matching the fixed width of a Google
  // Photos-style info panel. The graph's `1fr` track absorbs the difference on
  // its own; the only extra bookkeeping needed is making sure `resolve()`
  // still leaves it MIN_GRAPH when the panel is open (see `reserved` below).
  const INFO_WIDTH = 320;
  const infoOpen = $derived(graph.infoOpen);
  const infoWidth = $derived(infoOpen ? INFO_WIDTH : 0);

  let container: HTMLElement | undefined = $state();
  let width = $state(0);

  // View ▸ Toggle Repo List / Toggle Commit Pane (§4.1) — Rust owns the
  // actual booleans (§9.3); a hidden pane and its divider are dropped from
  // the layout entirely rather than squeezed to zero, so `usable` and
  // `resolve()` only ever reserve space for panes that are actually shown.
  const showLeft = $derived(paneVisibility.repoList);
  const showMiddle = $derived(paneVisibility.commitPane);

  const usable = $derived(
    Math.max(1, width - (showLeft ? DIVIDER : 0) - (showMiddle ? DIVIDER : 0)),
  );
  const px = $derived(
    resolve(usable, settings.paneWidths.left, settings.paneWidths.middle, infoWidth, showLeft, showMiddle),
  );

  /**
   * Widths are stored as fractions so they survive window resizing, but they
   * are applied as pixels under the minimums. When the window is too narrow to
   * honour both, the middle pane yields first, then the left. `reserved` is
   * the info panel's current width — left/middle keep their stored fractions
   * exactly (opening the panel must not itself resize them), but the excess
   * check additionally protects the graph's minimum against it. A hidden pane
   * contributes nothing and is never shrunk for one still visible.
   */
  function resolve(
    total: number,
    leftFrac: number,
    middleFrac: number,
    reserved: number,
    showLeft: boolean,
    showMiddle: boolean,
  ) {
    let left = showLeft ? Math.max(MIN_LEFT, Math.round(total * leftFrac)) : 0;
    let middle = showMiddle ? Math.max(MIN_MIDDLE, Math.round(total * middleFrac)) : 0;

    let excess = left + middle + MIN_GRAPH + reserved - total;
    if (excess > 0) {
      if (showMiddle) {
        const fromMiddle = Math.min(excess, middle - MIN_MIDDLE);
        middle -= fromMiddle;
        excess -= fromMiddle;
      }
      if (showLeft) {
        left -= Math.min(excess, left - MIN_LEFT);
      }
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
    settings.resetLayout();
  }

  // Settings first: the pane widths are needed for the very first paint, and
  // the welcome screen reads the recent-roots list from them.
  void settings.load().then(() => repos.start());
  void graph.start();
  void diff.start();
  void startMenuListener();
  void startWindowFrame();
</script>

<!-- The title bar is outside the three states below, not repeated inside each
     one: with `decorations: false` it is the only thing that can move, close
     or restore the window, so it has to exist before settings have loaded and
     on the welcome screen — not only once there is a repository list to sit
     above. -->
<div class="shell">
  <TitleBar />

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
      style="--pane-left: {px.left}px; --divider-left: {showLeft ? DIVIDER : 0}px; --pane-middle: {px.middle}px; --divider-mid: {showMiddle ? DIVIDER : 0}px; --pane-info: {infoWidth}px"
    >
      {#if showLeft}
        <RepoList />
        <Divider
          label="Resize repository list"
          value={Math.round((px.left / usable) * 100)}
          ondrag={dragLeft}
          onrelease={() => void settings.flush()}
          onreset={reset}
        />
      {/if}
      {#if showMiddle}
        <CommitPane />
        <Divider
          label="Resize commit pane"
          value={Math.round(((px.left + px.middle) / usable) * 100)}
          ondrag={dragMiddle}
          onrelease={() => void settings.flush()}
          onreset={reset}
        />
      {/if}
      <GraphPane />
      {#if infoOpen}
        <CommitInfoPanel />
      {/if}
    </main>
  {/if}
</div>

<style>
  /* Title bar over content. The second track is `minmax(0, 1fr)` for the same
     reason `main`'s row is, one level down: `1fr` alone has an auto minimum,
     so a tall pane would push the grid past the window instead of scrolling
     inside it — and here that would push the title bar off the top. */
  .shell {
    display: grid;
    grid-template-rows: var(--titlebar-height) minmax(0, 1fr);
    height: 100%;
  }

  main {
    display: grid;
    grid-template-columns: var(--pane-left) var(--divider-left) var(--pane-middle) var(--divider-mid) 1fr var(--pane-info);
    /* The single row must be pinned to the container's height. Left implicit
       it sizes to `auto`, whose max-content contribution is the tallest pane's
       full content height — so with 69 repositories the row became 9094px in
       an 870px window, every `.pane { height: 100% }` resolved against *that*,
       and the panes' own `overflow-y: auto` had nothing left to scroll.
       `minmax(0, 1fr)` refuses to grow past the container, which is what gives
       the panes a bounded height to scroll within. */
    grid-template-rows: minmax(0, 1fr);
    /* Only the info column's track reliably animates at runtime (0 ↔ 320px);
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
