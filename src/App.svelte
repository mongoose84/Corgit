<script lang="ts">
  import Divider from './lib/Divider.svelte';
  import RepoList from './lib/panes/RepoList.svelte';
  import CommitPane from './lib/panes/CommitPane.svelte';
  import GraphPane from './lib/panes/GraphPane.svelte';
  import { settings, DEFAULT_PANE_WIDTHS } from './lib/settings.svelte';

  // Minimum usable widths (SPEC.md §4). Below these the panes stop being
  // readable, so they win over the stored fractions.
  const MIN_LEFT = 190;
  const MIN_MIDDLE = 240;
  const MIN_GRAPH = 320;
  const DIVIDER = 5;

  let container: HTMLElement | undefined = $state();
  let width = $state(0);

  const usable = $derived(Math.max(1, width - DIVIDER * 2));
  const px = $derived(resolve(usable, settings.paneWidths.left, settings.paneWidths.middle));

  /**
   * Widths are stored as fractions so they survive window resizing, but they
   * are applied as pixels under the minimums. When the window is too narrow to
   * honour both, the middle pane yields first, then the left.
   */
  function resolve(total: number, leftFrac: number, middleFrac: number) {
    let left = Math.max(MIN_LEFT, Math.round(total * leftFrac));
    let middle = Math.max(MIN_MIDDLE, Math.round(total * middleFrac));

    let excess = left + middle + MIN_GRAPH - total;
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
    const max = Math.max(MIN_LEFT, usable - px.middle - MIN_GRAPH);
    const left = clamp(clientX - originX(), MIN_LEFT, max);
    settings.paneWidths = { ...settings.paneWidths, left: left / usable };
  }

  function dragMiddle(clientX: number) {
    const max = Math.max(MIN_MIDDLE, usable - px.left - MIN_GRAPH);
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

  void settings.load();
</script>

<main
  bind:this={container}
  bind:clientWidth={width}
  style="--pane-left: {px.left}px; --pane-middle: {px.middle}px; --divider: {DIVIDER}px"
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
</main>

<style>
  main {
    display: grid;
    grid-template-columns: var(--pane-left) var(--divider) var(--pane-middle) var(--divider) 1fr;
    height: 100%;
    background: var(--bg-app);
  }
</style>
