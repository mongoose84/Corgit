<script lang="ts">
  import Divider from './lib/Divider.svelte';
  import TitleBar from './lib/TitleBar.svelte';
  import Welcome from './lib/Welcome.svelte';
  import RepoList from './lib/panes/RepoList.svelte';
  import CommitPane from './lib/panes/CommitPane.svelte';
  import GraphPane from './lib/panes/GraphPane.svelte';
  import CommitInfoPanel from './lib/panes/CommitInfoPanel.svelte';
  import NoticeBanner from './lib/NoticeBanner.svelte';
  import AbortMergeDialog from './lib/AbortMergeDialog.svelte';
  import RecentProblems from './lib/RecentProblems.svelte';
  import { hasConflict, repos } from './lib/repos.svelte';
  import { notices } from './lib/notices.svelte';
  import { problems } from './lib/problems.svelte';
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
  void problems.start();

  // §13's banner, and the one place the three tiers are resolved into what is
  // actually on screen.
  //
  // Blocking outranks a raised error rather than the two stacking: the banner
  // is a single strip above a layout whose whole promise is density (§1), and
  // a repo that cannot commit or push is the more urgent of the two by
  // definition. Nothing is lost by hiding the error — it is in Recent Problems
  // either way, which is what lets this be a choice rather than a compromise.
  //
  // Derived from status, never from "a merge failed here earlier": a conflict
  // made in a terminal or one that outlived a restart has to raise this too.
  const conflictedRepo = $derived.by(() => {
    const id = repos.selectedId;
    if (id === undefined) return null;
    const status = repos.status(id);
    return status !== undefined && hasConflict(status) ? id : null;
  });

  const raised = $derived(notices.raised);
  /** The repo the banner is talking about, whichever tier is showing. */
  const noticeRepoId = $derived(conflictedRepo ?? raised?.repoId ?? null);
  const noticeRepoName = $derived(
    noticeRepoId === null
      ? undefined
      : (repos.repos.find((repo) => repo.id === noticeRepoId)?.name ?? undefined),
  );

  let abortingMerge = $state(false);

  // The conflict can clear without the dialog being the thing that cleared it
  // — resolved in a terminal, aborted from another window (§9.2), or simply a
  // different repo selected. Left latched, this would spring a stale "Abort
  // the merge?" open over the *next* conflict the user hits.
  $effect(() => {
    if (conflictedRepo === null) abortingMerge = false;
  });

  function selectNoticeRepo() {
    if (noticeRepoId !== null && noticeRepoId !== repos.selectedId) repos.select(noticeRepoId);
  }

  /**
   * *Pull* on a rejected push, aimed at the repo the banner names rather than
   * at the selected one.
   *
   * They are not always the same repo, which is the whole reason the banner
   * carries a name: row-level Pull and row-level Fetch (§5.1) fail in repos
   * that were never selected, and a recovery button that silently acted on
   * whatever happened to be highlighted would be the worst possible way to
   * honour §13's "every failure path ends in a recovery action".
   */
  async function pullNoticeRepo(): Promise<void> {
    const id = notices.raised?.repoId;
    if (id == null || id === repos.selectedId) {
      await repos.pull();
    } else {
      await repos.pullRow(id);
    }
  }
</script>

<!-- The title bar is outside the three states below, not repeated inside each
     one: with `decorations: false` it is the only thing that can move, close
     or restore the window, so it has to exist before settings have loaded and
     on the welcome screen — not only once there is a repository list to sit
     above. -->
<div class="shell">
  <TitleBar />

  <!-- Always rendered, empty or not, so the grid keeps three items for its
       three tracks — an `auto` row with nothing in it is 0px, whereas dropping
       the element would let the panes auto-place into the banner's row. -->
  <div class="notice-slot">
    {#if conflictedRepo !== null}
      <NoticeBanner
        tier="blocking"
        message="Merge conflict — commit and push are blocked until it's resolved or aborted"
        repoName={noticeRepoName}
        onAbortMerge={() => (abortingMerge = true)}
        onOpenVSCode={() => void repos.openInVSCode(conflictedRepo)}
      />
    {:else if raised}
      <!-- `forceAction` wins over the rule's own suggestion: §8.3's dirty-tree
           checkout failure stays untranslated by §13's instruction, so only
           the pane that ran the command knows the tree was dirty at the moment
           git refused — and only it can turn that into a way out. -->
      <NoticeBanner
        tier={raised.translated.tier}
        message={raised.translated.message}
        repoName={noticeRepoName}
        details={raised.translated.raw === raised.translated.message
          ? undefined
          : raised.translated.raw}
        action={raised.forceAction ?? raised.translated.action}
        canSuppress={raised.translated.id !== null}
        onPull={() => void pullNoticeRepo()}
        onOpenVSCode={() => void repos.openInVSCode(raised.repoId ?? undefined)}
        onRetry={raised.retry}
        onSelectRepo={noticeRepoId !== null ? selectNoticeRepo : undefined}
        onDismiss={() => notices.dismiss()}
        onSuppress={() => notices.suppress()}
      />
    {/if}
  </div>

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

{#if abortingMerge && conflictedRepo !== null}
  <AbortMergeDialog
    repoName={noticeRepoName ?? conflictedRepo}
    onAbort={async () => {
      await repos.mergeAbort();
    }}
    onClose={() => (abortingMerge = false)}
  />
{/if}

{#if problems.open}
  <RecentProblems onClose={() => (problems.open = false)} />
{/if}

<style>
  /* Title bar over content. The second track is `minmax(0, 1fr)` for the same
     reason `main`'s row is, one level down: `1fr` alone has an auto minimum,
     so a tall pane would push the grid past the window instead of scrolling
     inside it — and here that would push the title bar off the top. */
  .shell {
    display: grid;
    /* The column needs the same bound as the rows above, for a failure that
       is worse than the one they prevent. The implicit track is `auto`, whose
       base size is its widest item's *minimum* contribution — so a banner
       that cannot squeeze below the window (§13 gives it a repo name, a
       message and up to four controls on one line) widens the track, and
       `main` stretches to that track and reports the oversized width back
       through `bind:clientWidth`. With `body { overflow: hidden }` the excess
       is not scrollable, it is simply gone: the banner's rightmost control is
       the Dismiss ✕, so the one banner too wide to fit is also the one that
       cannot be dismissed. */
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: var(--titlebar-height) auto minmax(0, 1fr);
    height: 100%;
  }

  /* Bounding the track is only half of it — a grid item's own automatic
     minimum size can still overflow the track it sits in. Both children opt
     out so the window stays the outer limit for everything below it. */
  .notice-slot,
  main {
    min-width: 0;
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
