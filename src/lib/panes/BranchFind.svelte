<script lang="ts">
  /**
   * Find a branch in this repository (§5.3) — the box, its results, and the
   * keyboard cursor that walks them.
   *
   * Cut out of GraphPane, which had grown six independent concerns sharing one
   * `<script>`. This one never interacted with the other five except through
   * `repos.selectedId`, and the tell was the branch-search feature itself: its
   * pure logic went to `branchSearch.ts` with tests, while its UI state stayed
   * in the pane because there was nowhere else for it to go. This is that
   * somewhere.
   *
   * What makes the feature possible at all is an asymmetry worth restating:
   * the rows on screen are one page of a long history, but `graph.refs` is
   * *every* ref `for-each-ref` returned, tip loaded or not. So the search can
   * answer instantly about a branch cut two years ago; it is only the jump
   * that has to go and read pages.
   *
   * Reads `graph` directly rather than taking refs and rows as props. The pane
   * does the same, and it keeps `loadedTips` able to see whether the box is
   * open — a prop would have the parent build that Set for a closed box on
   * every sweep-triggered reload.
   */
  import { tick } from 'svelte';

  import Glyph from '../Glyph.svelte';
  import { graph, type RefBadge } from '../graph.svelte';
  import { searchBranches } from '../branchSearch';
  import { formatCommitDate } from '../dateFormat';

  interface Props {
    /** Bindable, because the pane owns the two ways in — the header's icon
     *  button (which paints itself pressed) and Ctrl+F — while the box owns
     *  everything behind them. */
    open: boolean;
    /** The search does not survive a repo change (§5.3); this is what tells it
     *  one happened. */
    repoId: string | undefined;
    /** Marked in the results the way GraphRow marks it on a row. */
    currentBranch: string | null;
    /** Read pages until `ref`'s tip is loaded, select it, and answer with its
     *  row index — or -1 if it gave up inside §5.3's page cap. The pane's job,
     *  because the pages and the selection belong to the graph. */
    reveal: (ref: RefBadge) => Promise<number>;
    /** Centre that row in the viewport. Deliberately separate from `reveal`:
     *  it has to run *after* this box closes, because closing hands the scroll
     *  container its 186px back and centring against the shorter viewport puts
     *  the row three rows high — wrong every time a jump had to read pages,
     *  which is the case the centring exists for. */
    centre: (index: number) => Promise<void>;
  }

  let { open = $bindable(), repoId, currentBranch, reveal, centre }: Props = $props();

  let query = $state('');
  let inputEl: HTMLInputElement | undefined = $state();
  let resultsEl: HTMLElement | undefined = $state();

  /** Which hit the keyboard cursor is on. An index rather than a ref, because
   *  the list it indexes into is rebuilt on every keystroke and a held ref
   *  would survive its own disappearance from the results. */
  let cursor = $state(0);

  /** Set when a jump gave up before finding its commit (§5.3's page cap), and
   *  cleared by the next query or the next jump. Answered here rather than
   *  through `notices.raise`: that store translates git's stderr for §13's
   *  banner, and nothing failed — a branch simply turned out to be further
   *  back than one gesture is allowed to walk. §11.1's container rule puts the
   *  answer where the question was asked. */
  let outOfReach = $state<string | null>(null);

  const hits = $derived(searchBranches(graph.refs, query));

  /** Clamped rather than stored clamped, so a shrinking result list can never
   *  point the cursor past the end between a keystroke and its `$derived`. */
  const cursorAt = $derived(hits.length === 0 ? 0 : cursor % hits.length);

  /** Which tips are already among the loaded rows — the one thing the search
   *  knows that the graph cannot show, and the difference between a jump that
   *  is instant and one that reads pages. Built only while the box is open:
   *  `graph.rows` changes on every sweep-triggered reload, and a Set over
   *  3000 rows rebuilt for a closed box is work nobody asked for. */
  const loadedTips = $derived.by(() => {
    if (!open) return new Set<string>();
    return new Set(graph.rows.map((row) => row.commit.hash));
  });

  const results = $derived(
    hits.map((ref) => ({
      ref,
      loaded: loadedTips.has(ref.commit),
      date: formatCommitDate(ref.timestamp),
    })),
  );

  /** Every part of the search is about the repo being left: the query was
   *  aimed at that repo's branches, the cursor indexes a list that no longer
   *  exists, and the out-of-reach line names a branch the new repo may not
   *  have. A box reappearing over a different repo's history, already narrowed
   *  by something typed about another one, is a search that has quietly
   *  changed its subject.
   *
   *  Only what this box owns, though. Whether it is *open* is the pane's, and
   *  the pane clears that in the same effect it closes the diff in —
   *  deliberately not from here, because this component unmounts while a new
   *  repo's first page loads (`graph-body` sits behind a `{#if}`), and a reset
   *  written on the way out of a component that is about to be rebuilt is a
   *  reset written twice. */
  let lastRepoId: string | undefined | symbol = Symbol('before the first repo');
  $effect(() => {
    if (repoId === lastRepoId) return;
    lastRepoId = repoId;
    query = '';
    cursor = 0;
    outOfReach = null;
  });

  /** Opening, and re-opening while already open — Ctrl+F in the box itself
   *  lands here, which is why this selects rather than merely focuses: a
   *  previous query should be typed over, not appended to. The one case where
   *  keeping the text earns itself is coming back to the same hunt, and that
   *  is still one keystroke away. */
  export async function openBox() {
    open = true;
    cursor = 0;
    outOfReach = null;
    await tick();
    inputEl?.select();
  }

  export function closeBox() {
    open = false;
    outOfReach = null;
  }

  /** Whether an event landed in this box's input. The pane's Ctrl+F declines
   *  to steal focus out of somewhere the user is typing, and this box is the
   *  one exemption. */
  export function ownsTarget(target: EventTarget | null): boolean {
    return target === inputEl;
  }

  /** One jump at a time. `graph.reveal` walks pages sequentially, so a second
   *  jump started mid-walk would interleave two `loadMore` chains against one
   *  lane state — the exact aliasing `loadToken` exists to catch, arrived at
   *  from inside the pane instead.
   *
   *  A plain `let`, unlike the `busy` flag the switch gesture uses: nothing
   *  reads this from the template. What the user sees during a jump is
   *  `graph.revealing`, which the store owns because the pages are its. */
  let jumping = false;

  /**
   * Go to a branch: select its tip commit and scroll the graph to it.
   *
   * **Not a checkout.** The graph already has a switch gesture — double-click
   * a badge, or the row menu (§8.3) — and a search result that quietly wrote
   * to the working tree would be the one place in Corgit where finding
   * something changed it. Landing on the row leaves every one of those
   * gestures one click away, on a row that is now on screen.
   */
  async function jumpTo(ref: RefBadge) {
    if (jumping) return;
    jumping = true;
    outOfReach = null;
    try {
      const at = await reveal(ref);
      if (at === -1) {
        // The box stays open, because the sentence needs somewhere to live and
        // because the next thing the user does is probably narrow the query.
        outOfReach = ref.name;
        return;
      }
      open = false;
      await centre(at);
    } finally {
      jumping = false;
    }
  }

  // The results box is 186px and the cursor walks the whole list, so ↓ runs
  // off the bottom of it within six presses. `block: 'nearest'` rather than
  // 'center': a cursor already in view must not shunt the list on every
  // keystroke, which is the difference between following the selection and
  // fighting it. Queried rather than kept in a per-row binding array, because
  // the list is rebuilt on every keystroke and a stale array of elements is a
  // second thing to keep correct for no gain.
  $effect(() => {
    if (!open) return;
    // Both dependencies earn themselves. The cursor is the obvious one; `hits`
    // is the case where it is *not* moving — narrowing the query while
    // scrolled halfway down leaves the box scrolled there over a list that is
    // now three rows long, with the cursor sitting on a row above the fold.
    void cursorAt;
    void hits;
    resultsEl?.querySelector('.hit.on')?.scrollIntoView({ block: 'nearest' });
  });

  function onFindKeys(event: KeyboardEvent) {
    if (hits.length === 0) return;
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      cursor = (cursorAt + 1) % hits.length;
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      cursor = (cursorAt + hits.length - 1) % hits.length;
    } else if (event.key === 'Enter') {
      event.preventDefault();
      void jumpTo(hits[cursorAt]);
    }
    // Esc is deliberately absent: it bubbles to the window handler, which
    // already decides what Esc closes when several things are open (§5.2).
  }
</script>

{#if open}
  <!-- A --bg-app band cut into the pane, which is the shape §5.1 already uses
       for chrome that scopes what sits under it — the root strip and both
       section bands. Above the *Uncommitted Changes* node because that node is
       a row, and this is chrome over the rows. -->
  <div class="find">
    <input
      bind:this={inputEl}
      bind:value={query}
      type="text"
      placeholder="Find a branch…"
      spellcheck="false"
      autocapitalize="off"
      autocorrect="off"
      aria-label="Find a branch in this repository"
      oninput={() => {
        cursor = 0;
        outOfReach = null;
      }}
      onkeydown={onFindKeys}
    />
    <span class="find-count">
      {query.trim() === ''
        ? `${graph.refs.length} branches`
        : `${hits.length} of ${graph.refs.length}`}
    </span>
    <button
      type="button"
      class="find-close"
      onclick={closeBox}
      title="Close (Esc)"
      aria-label="Close branch search"
    >
      <Glyph kind="cross" />
    </button>
  </div>

  <!-- In flow, not floating. Pane.svelte's `.body` is a scroll container, so
       an absolutely-positioned popover anchored here would be clipped by it
       and would scroll away with the pane — which is why ContextMenu.svelte is
       `position: fixed` with viewport coordinates and a measure-then-nudge
       pass. A strip that shrinks the graph needs none of that machinery, and
       the rows are virtualized, so the height costs only rows you can see. -->
  <div class="results" bind:this={resultsEl}>
    {#if results.length === 0}
      <p class="no-hits">No branch matches</p>
    {:else}
      <div class="group">
        <span>{query.trim() === '' ? 'Every branch' : `Matching “${query.trim()}”`}</span>
        <span class="note">tip commit</span>
      </div>
      {#each results as hit, index (hit.ref.kind + hit.ref.name)}
        <button
          type="button"
          class="hit"
          class:on={index === cursorAt}
          onclick={() => void jumpTo(hit.ref)}
          title={hit.loaded
            ? `Go to ${hit.ref.name}`
            : `Go to ${hit.ref.name} — its tip is not in the loaded history yet`}
        >
          <span
            class="name"
            class:remote={hit.ref.kind === 'remote'}
            class:current={hit.ref.kind === 'local' && hit.ref.name === currentBranch}
            >{hit.ref.name}</span
          >
          <span class="tip">{hit.ref.subject}</span>
          <!-- The one thing the search knows that the graph cannot show.
               Muted, because it is a fact about the pane and not a state of
               the branch — the status colours all mean something about a
               repo (§11). -->
          {#if !hit.loaded}
            <span class="unloaded">not loaded</span>
          {/if}
          <span class="when">{hit.date}</span>
        </button>
      {/each}
    {/if}
    <!-- One slot, three sentences, so the list never changes height under the
         pointer while a jump runs. -->
    <p class="foot" class:said={graph.revealing !== null || outOfReach !== null}>
      {#if graph.revealing !== null}
        Reading history to reach {graph.revealing}…
      {:else if outOfReach !== null}
        {outOfReach} is further back than one jump reads — load more history, then search again.
      {:else}
        Enter goes to the branch tip; it does not switch to it.
      {/if}
    </p>
  </div>
{/if}

<style>
  /* A --bg-app band, like the root strip and the section bands (§5.1, §11.1):
     it reads as a band cut across the pane rather than as its first row. */
  .find {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 0 0 auto;
    padding: var(--space-1) var(--space-3);
    background: var(--bg-app);
    border-bottom: 1px solid var(--border);
  }

  /* RepoList's filter input, to the value: 26px, --bg-raised on a --bg-app
     band, 1px --border, --radius-sm. One box for "type to narrow a list",
     whichever pane it is in. */
  .find input {
    flex: 1 1 auto;
    min-width: 0;
    height: 26px;
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .find input::placeholder {
    color: var(--text-disabled);
  }

  .find input:focus-visible {
    border-color: var(--accent);
    outline: none;
  }

  .find-count {
    flex: 0 0 auto;
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    color: var(--text-disabled);
  }

  .find-close {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    cursor: default;
  }

  .find-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* Bounded so the graph never disappears behind its own search: at 186px the
     list holds five results and scrolls, and eight rows of history stay
     visible in the shortest pane §4 allows. */
  .results {
    flex: 0 0 auto;
    max-height: 186px;
    overflow-y: auto;
    background: var(--bg-app);
    border-bottom: 1px solid var(--border);
  }

  /* The picker's section head (SwitchPullDialog), at the pane's own 12px
     gutter rather than the dialog's 8px. */
  .group {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 6px var(--space-3) var(--space-1);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .group .note {
    text-transform: none;
    letter-spacing: 0.04em;
    font-weight: 400;
    color: var(--text-disabled);
  }

  .hit {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: 26px;
    padding: 0 var(--space-3);
    border: 0;
    background: none;
    text-align: left;
    cursor: default;
  }

  .hit:hover {
    background: var(--bg-hover);
  }

  /* The keyboard cursor, and deliberately not the accent: §11 rule 3 keeps
     that for selection, and nothing is selected until Enter — pressing it is
     what paints a row in --accent-muted, one list down. */
  .hit.on {
    background: var(--bg-active);
  }

  /* GraphRow's `.ref` box, so a branch name means the same thing in the search
     as it does on the row the search is about to land on. Wider, because the
     column it sits in is the whole list rather than a row already carrying a
     message, an author and a date. */
  .hit .name {
    flex: 0 0 auto;
    max-width: 250px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    padding: 1px var(--space-1);
    font-size: var(--text-xs);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
    color: var(--text-secondary);
  }

  .hit .name.remote {
    color: var(--text-muted);
    font-style: italic;
  }

  /* Bold like GraphRow's `.current`, but without the lane colour: lane hues
     belong to a row's dot, and there is no dot here to tie one to. */
  .hit .name.current {
    color: var(--text-primary);
    border-color: var(--text-muted);
    font-weight: 700;
  }

  .hit .tip {
    flex: 1 1 6rem;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-muted);
  }

  .hit .unloaded {
    flex: 0 0 auto;
    font-size: var(--text-xs);
    color: var(--text-disabled);
  }

  /* The graph's own date column, at the same width and the same alignment, so
     the two lists read as one surface (§5.3). */
  .hit .when {
    flex: 0 0 auto;
    width: 140px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    font-size: var(--text-xs);
    color: var(--text-disabled);
  }

  /* The picker's own words and its own metrics, kept (SwitchPullDialog). */
  .no-hits {
    margin: 0;
    padding: var(--space-1) var(--space-3) 6px;
    font-size: var(--text-xs);
    color: var(--text-disabled);
  }

  .foot {
    margin: 0;
    padding: var(--space-1) var(--space-3) 6px;
    border-top: 1px solid var(--border);
    font-size: var(--text-xs);
    color: var(--text-disabled);
  }

  /* One step up when the line stops being a standing hint and starts being an
     answer to something the user just did. Not a status colour: every hue in
     §11 already means something about a repo, and this is the pane talking
     about itself. */
  .foot.said {
    color: var(--text-secondary);
  }
</style>
