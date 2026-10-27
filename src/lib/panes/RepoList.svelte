<script lang="ts">
  import Pane from './Pane.svelte';
  import RepoRow from './RepoRow.svelte';
  import RootStrip from './RootStrip.svelte';
  import EmptyState from '../EmptyState.svelte';
  import Mascot from '../Mascot.svelte';
  import MultiBranchDialog from '../MultiBranchDialog.svelte';
  import SwitchPullDialog from '../SwitchPullDialog.svelte';
  import { repos } from '../repos.svelte';
  import { filterTerms, matchesFilter } from '../repoFilter';
  import { multiBranch } from '../multiBranch.svelte';
  import { switchPull } from '../switchPull.svelte';

  let filter = $state('');

  // Substring on repo name only — not branch, not path (SPEC.md §5.1) — with
  // a comma-separated value matching any of its terms. The rule lives in
  // `repoFilter.ts` with the banner's half of the same contract; see there for
  // why the box learned to hold a list.
  const terms = $derived(filterTerms(filter));

  const shown = $derived.by(() => {
    if (terms.length === 0) return repos.repos;
    return repos.repos.filter((repo) => matchesFilter(repo.name, terms));
  });

  // The banner's *Show the N* (§5.1, §13). A one-shot signal, consumed as it
  // is applied: left set, a later sweep or repo selection would re-apply a
  // filter the user has since cleared, which is a list that will not let go.
  $effect(() => {
    const requested = repos.filterRequest;
    if (requested === null) return;
    filter = requested;
    repos.clearFilterRequest();
  });

  const bulk = $derived(repos.bulk);

  // Two sections, each alphabetical (§5.1) — discovery already returns repos
  // pre-sorted by name, so this is a partition of `shown`, not a re-sort.
  const pinned = $derived(shown.filter((repo) => repos.pins.has(repo.id)));
  const unpinned = $derived(shown.filter((repo) => !repos.pins.has(repo.id)));

  const count = $derived(
    shown.length === repos.repos.length
      ? `${repos.repos.length}`
      : `${shown.length} of ${repos.repos.length}`,
  );
</script>

<Pane title="Repositories ({count})">
  {#snippet actions()}
    {#if repos.sweeping}
      <!-- Dead time, and chrome rather than decoration — the same reasoning
           that lets a mascot glyph be a button icon (SPEC §14.1). He trots for
           as long as the sweep runs; the timing readout takes the slot back
           the moment it lands, so the two never fight for the space. -->
      <Mascot pose="mini-working" height={18} />
    {:else if repos.lastSweepMs !== null}
      <!-- The 300 ms status-sweep budget in §1 is the reason this project
           exists, so the measurement is on screen, not in a log. -->
      <span class="timing" title="Last status sweep across {repos.repos.length} repositories"
        >{repos.lastSweepMs} ms</span
      >
    {/if}
    <!-- The only icon in this header. ⟳ used to sit beside it and moved to
         Repository ▸ Rescan Folder (§5.1): two circular arrows differing by
         stroke direction cannot be told apart, and rediscovery is rare enough
         for a menu now that the watchers keep the rows current (§6).

         Same glyph and same 22px button as *Changes*' Fetch, one column over —
         the icon means one thing everywhere, and only the column it sits in
         says what it applies to. -->
    <button
      type="button"
      class="icon-action"
      onclick={() => void repos.fetchAll()}
      disabled={bulk !== null || !repos.root || repos.repos.length === 0}
      title="Fetch every repository"
      aria-label="Fetch all"
    >
      ↻
    </button>
  {/snippet}

  <!-- §5.1's root strip. Reserved rather than conditional, and that is the
       whole point: shown only when something was behind, it would be inserted
       by the *fetch sweep* — unprompted, on its own schedule — putting a
       button that writes to every behind working tree exactly where a repo row
       was a moment ago. Reserving it means nothing ever moves.

       Above the filter box, not below: actions on top, list manipulation
       underneath. Everything below the box is what the box scopes, and this
       strip deliberately ignores it. -->
  {#if repos.repos.length > 0}
    <RootStrip />
  {/if}

  <div class="filter">
    <input
      type="search"
      placeholder="Filter repositories…"
      bind:value={filter}
      disabled={repos.repos.length === 0}
      aria-label="Filter repositories by name"
    />
  </div>

  {#if repos.repos.length === 0}
    <EmptyState
      message="No repositories here"
      hint="Corgit looks one level down — open the folder that contains them"
    />
  {:else if shown.length === 0}
    <EmptyState message="No matches" hint="Filter matches repository names only" />
  {:else}
    {#if pinned.length > 0}
      <!-- The pinned band (§5.1, §11.1). Not a section header with a button
           dropped into it — §11.1 puts buttons in a strip or a pane header and
           nowhere else, and one sitting loose in a micro-label row belongs to
           no family in this app. So the header becomes the root strip's own
           shape one level down: the same --bg-app band, the same 4px padding
           around a 22px button, at the scope the pins actually have.

           That scope is the whole reason it is not simply added to the root
           strip above, where it would fit perfectly well. §11.1: the container
           declares the scope, and the strip's container means the folder. Two
           buttons there would make the folder and the user's curated set look
           like siblings, which they are not.

           It is also where the §2 bulk-across-the-pinned-set actions go when
           they arrive, which is what makes a band worth its two pixels over a
           label. -->
      <div class="pinned-band">
        <span class="band-label">Pinned ({pinned.length})</span>
        <span class="band-actions">
          <!-- Emptying the hot set in one click matters as much as filling it:
               the set is meant to track what you are working on this week, and
               a set that is tedious to clear stops tracking anything. -->
          {#if filter.trim() === ''}
            <!-- Hidden while filtering: the section then shows a subset, and a
                 button that quietly unpins repos the user cannot see is a trap. -->
            <button type="button" class="clear" onclick={() => void repos.clearPins()}>
              Unpin all
            </button>
          {/if}
          <!-- Rightmost, where *Pull all* sits in the strip above: the band's
               own primary, and the reason the band exists.

               Drawn at all times, unlike *Unpin all* beside it, and the split
               is §11.1's weight rule — branching the set is what the pins were
               *for*, unpinning is housekeeping. It also stays put while
               filtering, because the rule that hides *Unpin all* is about
               acting silently on rows the user cannot see, and this acts on
               nothing: it opens a dialog listing every repo it would touch,
               with a checkbox on each. -->
          <button
            type="button"
            class="branch"
            disabled={bulk !== null}
            onclick={() => multiBranch.show()}
          >
            Branch…
          </button>
        </span>
      </div>
      <ul>
        {#each pinned as repo (repo.id)}
          <li>
            <RepoRow {repo} status={repos.status(repo.id)} error={repos.error(repo.id)} />
          </li>
        {/each}
      </ul>
    {/if}

    <!-- The *All* band (§5.1). Built the same way the pinned band above is,
         because it is the same object one section over: the same --bg-app fill,
         the same 4px padding around a 22px control, the same micro-label type.
         The asymmetry this replaced was never the goal — a band exists because
         it holds a control, and until *Switch & pull…* there was nothing to
         hold.

         Drawn whenever there are repositories, pinned or not, which is the one
         way it differs from the band above. With nothing pinned it sits
         directly under a root strip covering the same repos, and that is fine:
         §11.1's sibling problem is two containers claiming the same act, and
         these are two acts — the strip pulls each repo's *current* branch, this
         moves every repo onto *one named* branch. Hiding it until something is
         pinned would put the feature behind a gesture nobody has to make. -->
    <div class="section-band">
      <span class="band-label">All ({unpinned.length})</span>
      <span class="band-actions">
        <!-- Neutral, never `.primary`: §11.1 allows one accent per surface and
             *Pull all* is already wearing it in the strip above. Same rule that
             keeps *Branch…* neutral.

             Stays put while filtering, like *Branch…* and unlike *Unpin all*:
             that rule is about acting silently on rows the user cannot see, and
             this acts on nothing — it opens a dialog listing every repository
             it would touch, with a checkbox on each. -->
        <button
          type="button"
          class="branch"
          disabled={bulk !== null}
          onclick={() => switchPull.show()}
        >
          Switch &amp; pull…
        </button>
      </span>
    </div>
    <ul>
      {#each unpinned as repo (repo.id)}
        <li>
          <RepoRow {repo} status={repos.status(repo.id)} error={repos.error(repo.id)} />
        </li>
      {/each}
    </ul>
  {/if}
</Pane>

<!-- Hosted here rather than in `App.svelte` because this pane owns the pinned
     set the dialog acts on. The menu's second route opens the same store flag
     (§4.1), so there is still exactly one dialog. -->
{#if multiBranch.open}
  <MultiBranchDialog
    onCreate={(repoIds, name, checkout) => void repos.branchAll(repoIds, name, checkout)}
    onClose={() => multiBranch.close()}
  />
{/if}

<!-- Hosted here for the same reason, and separately: the two dialogs are two
     components over two sections, and nothing about opening one bears on the
     other. -->
{#if switchPull.open}
  <SwitchPullDialog
    onSwitch={(repoIds, name, pull) => void repos.switchPullAll(repoIds, name, pull)}
    onClose={() => switchPull.close()}
  />
{/if}

<style>
  .filter {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
  }

  input {
    width: 100%;
    height: 26px;
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  input::placeholder {
    color: var(--text-disabled);
  }

  input:disabled {
    color: var(--text-disabled);
    cursor: default;
  }

  input:focus-visible {
    border-color: var(--accent);
    outline: none;
  }

  .timing {
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    color: var(--text-disabled);
  }

  /* Deliberately identical to CommitPane's `.icon-action`, down to the
     values: §5.1's Fetch all and §5.2's Fetch are the same button doing the
     same thing at two scopes, and two copies that merely look alike is how
     they stop looking alike. */
  .icon-action {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    font-size: var(--text-md);
    line-height: 1;
    cursor: default;
  }

  .icon-action:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .icon-action:disabled {
    color: var(--text-disabled);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* Built the way RootStrip's `.root-strip` is built, rather than to a
     matching height: the same --bg-app fill and the same 4px padding around a
     22px control, so all three bands are the same object at three scopes and
     stay that way if the button height ever moves.

     One rule for both section bands rather than two that happen to match. They
     are the same object one section apart — the moment they are two rules is
     the moment they stop looking alike, which is the mistake `.icon-action`
     records having made once already.

     No border-top. The band is darker than the filter row above it, so the
     value change already draws the edge — and a border there would sit on top
     of the filter's own border-bottom as a 2px line. `.root-strip` omits it
     for the same reason. */
  .pinned-band,
  .section-band {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    padding: var(--space-1) var(--space-3);
    background: var(--bg-app);
    border-bottom: 1px solid var(--border);
  }

  /* Still the section header's type, not the strip's: this band replaced
     *Pinned (5)* and has to keep reading as the pair of *All (72)* below. Only
     the chrome around it changed. */
  .band-label {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .band-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-left: auto;
    flex: 0 0 auto;
  }

  /* The same box as RootStrip's *Pull all*, per §11.1: 22px, --bg-raised on a
     --bg-app band, a 1px --border and --radius-sm.

     Neutral, never `.primary`. §11.1 allows one accent per surface and *Pull
     all* is already wearing it forty pixels above; a second accented button
     within sight of the first leaves neither reading as primary.

     The font is reset rather than inherited: the band's label type is
     uppercase, 600 and letter-spaced, and no button in the app is any of
     those. */
  .branch {
    flex: 0 0 auto;
    height: 22px;
    padding: 0 var(--space-2);
    font-family: inherit;
    font-size: var(--text-sm);
    font-weight: 400;
    letter-spacing: normal;
    text-transform: none;
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: default;
  }

  .branch:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: var(--border-strong);
  }

  /* Unavailable while a bulk run is going — the same lockout the strip's own
     controls take, since a second run cannot start on top of the first. Colour
     only, matching RootStrip's `.pull-all:disabled`: the box stays, so the
     band does not change height when a run starts. */
  .branch:disabled {
    color: var(--text-disabled);
  }

  .clear {
    padding: 0;
    border: 0;
    background: none;
    font-size: var(--text-xs);
    letter-spacing: 0.04em;
    color: var(--text-disabled);
    cursor: default;
    /* Transparent rather than hidden, so it stays reachable by keyboard. */
    opacity: 0;
  }

  /* Revealed with the band, not the individual row — it acts on the whole
     set, and the band is that set's own chrome. */
  .pinned-band:hover .clear,
  .clear:focus-visible {
    opacity: 1;
  }

  .clear:hover {
    color: var(--text-primary);
    text-decoration: underline;
  }

</style>
