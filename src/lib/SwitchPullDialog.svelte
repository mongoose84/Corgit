<script lang="ts">
  // Switch & pull across the *All* section (SPEC.md §5.1, §8.3) — opened from
  // the repo list's *All* band, or Repository ▸ Switch & Pull in All.
  //
  // `MultiBranchDialog`'s shape reused rather than its code: same 400px, same
  // framed list, same checkbox per repo, same excluded-before-anything-runs
  // column. Folding the two together was considered and rejected for the reason
  // that one gives for not folding into `CreateBranchDialog` — a dialog whose
  // every second line branches on which surface opened it. What they share is
  // the chrome and the *rules*, and the rules that are actually shared live in
  // `switchTargets.ts`, which is where the interesting half of this file went.
  //
  // Four things differ, and each is in `switchTargets.ts` or below:
  //   · the branch is picked, not typed — you can only switch to one that exists
  //   · the per-repo column says what will happen, not where it starts from
  //   · a dirty tree is a caution, not an exclusion (§8.3 forbids force-checkout)
  //   · a tri-state check-all heads the checkbox column
  import { filterTerms, matchesFilter } from './repoFilter';
  import { hasConflict, isDirty, repos, type Repo } from './repos.svelte';
  import {
    blockerLabel,
    branchCoverage,
    coverageSplit,
    planSwitch,
    type BranchSets,
    type SwitchContext,
  } from './switchTargets';

  interface Props {
    /** Fires with the repos to act on. The dialog closes on the click rather
     *  than waiting for the run: the strip is where a bulk run narrates itself
     *  (§5.1), and a modal over the list would cover the rows whose new
     *  branches are the result (§14.1). */
    onSwitch: (repoIds: string[], name: string, pull: boolean) => void;
    onClose: () => void;
  }

  let { onSwitch, onClose }: Props = $props();

  let branch = $state('');
  // Checked by default: *switch and then pull* is the whole gesture, and the
  // checkbox exists to refuse the second half rather than to opt into it. Same
  // default and same reasoning as `CreateBranchDialog`'s *Check out after
  // creating*.
  let pull = $state(true);
  let pickerOpen = $state(false);
  let branchQuery = $state('');
  let query = $state('');
  let branchFieldEl: HTMLButtonElement | undefined = $state();

  /** Every listed repo's branch names, read once when the dialog opens (§8.3,
   *  §1). Keyed by repo id; a repo whose refs could not be read maps to `null`,
   *  which blocks it rather than waving it through. */
  let branches = $state<Map<string, BranchSets | null>>(new Map());
  let loading = $state(true);

  /** The *All* section is the dialog's subject, and it is the whole section
   *  whatever the filter box behind it is showing (§5.1): the band's label
   *  counts what is on screen, this counts what would be touched, and the
   *  ticked set below is the consent. It is live — opting a pinned repo in
   *  below unpins nothing, so this list only changes if the root does. */
  const section = $derived(repos.unpinnedRepos);

  /** Repos opted in from the search box — pinned ones, since everything
   *  unpinned is already a row above. Held as ids rather than repos so a rescan
   *  cannot leave a stale object in the run. */
  let added = $state<Set<string>>(new Set());
  const addedRepos = $derived(repos.pinnedRepos.filter((repo) => added.has(repo.id)));

  /** Every row the dialog draws, in the list's own order. */
  const listed = $derived([...section, ...addedRepos]);

  /** Ticked repos. Seeded from the section once the rows are known, then owned
   *  by the user. */
  let selected = $state<Set<string>>(new Set());
  let seeded = $state(false);

  $effect(() => {
    branchFieldEl?.focus();
  });

  // Reads `section` only to seed, and only once: re-running it on every change
  // would re-tick a row the user had just cleared.
  $effect(() => {
    if (seeded) return;
    selected = new Set(section.map((repo) => repo.id));
    seeded = true;
  });

  // One read for the whole dialog, on open. Every pick after this is resolved
  // against what came back, never against git — a `for-each-ref` per repo per
  // keystroke is what the spawn-bound path (§1) will not spend.
  $effect(() => {
    const ids = listed.map((repo) => repo.id);
    if (ids.length === 0) {
      loading = false;
      return;
    }
    // Re-runs when a repo is opted in below, which is what covers it: it
    // arrives here and is read like every other row, rather than being the one
    // repo in the dialog whose branches are unknown. `loading` goes back up for
    // that read, so the button cannot fire against a half-known set.
    let cancelled = false;
    loading = true;
    void repos.repoBranches(ids).then((entries) => {
      if (cancelled) return;
      // Rebuilt rather than merged: this is the whole answer for the current
      // list, and a merge would keep names for a repo that has since left it.
      const next = new Map<string, BranchSets | null>();
      for (const entry of entries) {
        next.set(entry.repoId, entry.local === null || entry.remote === null ? null : { local: entry.local, remote: entry.remote });
      }
      branches = next;
      loading = false;
    });
    return () => {
      cancelled = true;
    };
  });

  /** The order the picker reads them in — `null` for a repo not read yet, so a
   *  half-loaded dialog cannot report a branch as absent. */
  const sets = $derived(listed.map((repo) => branches.get(repo.id) ?? null));
  const coverage = $derived(branchCoverage(sets));
  const split = $derived(coverageSplit(branch, sets));

  const branchTerms = $derived(branchQuery.trim().toLowerCase());
  const pickable = $derived(
    branchTerms === ''
      ? coverage
      : coverage.filter((entry) => entry.name.toLowerCase().includes(branchTerms)),
  );

  /** What the run will do to one repo, from cached status plus the one ref
   *  read. Assembled here so `switchTargets.ts` never has to know what a
   *  `RepoStatus` is. */
  function contextFor(repo: Repo): SwitchContext {
    const status = repos.status(repo.id);
    return {
      branches: branches.get(repo.id) ?? null,
      // The oid is not a branch name, so a detached repo reports `''` and is
      // switched like any other — its column just has nothing to print on the
      // left of the arrow.
      current: status?.branch ?? '',
      behind: status?.behind ?? 0,
      conflicted: status !== undefined && hasConflict(status),
      dirty: status !== undefined && isDirty(status),
    };
  }

  const plans = $derived(new Map(listed.map((repo) => [repo.id, planSwitch(branch, contextFor(repo))])));

  const blocked = $derived(
    new Set(listed.filter((repo) => plans.get(repo.id)?.kind === 'blocked').map((repo) => repo.id)),
  );

  /** Ticked, minus anything that cannot take it. The button's count, the scope
   *  line and the ids handed to the run are all this one list.
   *
   *  Derived from the *rows* rather than from `selected`, which is why it is a
   *  filter over `listed`: a repo pinned from the list behind the dialog leaves
   *  `selected` untouched, and switching a repo whose row is not on screen is
   *  the one outcome this dialog must not have. */
  const targets = $derived(
    listed.filter((repo) => selected.has(repo.id) && !blocked.has(repo.id)).map((repo) => repo.id),
  );

  const selectable = $derived(listed.filter((repo) => !blocked.has(repo.id)));
  /** The column head's three states. It can never reach the section's own
   *  count — the blocked rows are not selectable — so "all" means all it can
   *  take, which is why the middle state exists at all. */
  const allChecked = $derived(selectable.length > 0 && targets.length === selectable.length);
  const someChecked = $derived(targets.length > 0 && !allChecked);

  const excluded = $derived(blocked.size);
  const canSwitch = $derived(!loading && branch !== '' && targets.length > 0);

  const terms = $derived(filterTerms(query));
  /** Repos the search offers to add — pinned only, since everything unpinned is
   *  already a row above. Same predicate as the repo list's own filter box, so
   *  the two boxes cannot disagree about what matches. */
  const matches = $derived(
    terms.length === 0
      ? []
      : repos.pinnedRepos.filter((repo) => !added.has(repo.id) && matchesFilter(repo.name, terms)),
  );

  function toggle(id: string): void {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }

  /** The head of the column, not a button in the header: it toggles every row
   *  it *can*, and clearing beats filling when the set is mixed — the mixed
   *  state is reached by unticking, so the gesture that produced it repeated is
   *  most likely meant to finish the job. */
  function toggleAll(): void {
    if (targets.length > 0) {
      selected = new Set();
      return;
    }
    selected = new Set(selectable.map((repo) => repo.id));
  }

  /** Opting a pinned repo in adds it to the run and **changes no pins** — the
   *  mirror image of `MultiBranchDialog`, deliberately. There, ticking a repo
   *  pins it because the ticked set and the pin set mean the same thing for
   *  that gesture. Here the section is defined by *not* being pinned, so
   *  pinning one would move it out of the very list it was just added to. */
  function add(repo: Repo): void {
    added = new Set(added).add(repo.id);
    selected = new Set(selected).add(repo.id);
  }

  function pick(name: string): void {
    branch = name;
    pickerOpen = false;
    branchQuery = '';
    branchFieldEl?.focus();
  }

  function confirm(): void {
    if (!canSwitch) return;
    onSwitch(targets, branch, pull);
    onClose();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      // The picker first, then the dialog — Escape closes the thing that is on
      // top, which is the only reading that is never surprising.
      if (pickerOpen) {
        event.stopPropagation();
        pickerOpen = false;
        branchFieldEl?.focus();
        return;
      }
      // Stopped rather than left to bubble, like every other dialog here.
      event.stopPropagation();
      onClose();
    }
  }

  /** The right-hand column, and the reason it is text rather than a badge: it
   *  is a sentence about one repo, and five of them read down the list. */
  function planLabel(repoId: string): string {
    const plan = plans.get(repoId);
    if (plan === undefined) return '';
    switch (plan.kind) {
      case 'blocked':
        return blockerLabel(plan.blocker, branch);
      case 'stay':
        // The one row that can carry a count, because status only knows
        // ahead/behind for the branch the repo is on (§8.2). Promising a number
        // for any other row would be inventing one.
        return plan.behind > 0 ? `${branch} ↓${plan.behind}` : `${branch} · up to date`;
      case 'switch':
        return `${plan.from} → ${branch}`;
      case 'create':
        return `${plan.from} → ${branch}`;
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="scrim" role="presentation" onmousedown={onClose}>
  <div
    class="dialog"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-label="Switch branch and pull in several repositories"
    onmousedown={(event) => event.stopPropagation()}
    onkeydown={onKeydown}
  >
    <p class="title">Switch &amp; pull</p>
    <p class="scope">
      in {targets.length} of {listed.length}
      {listed.length === 1 ? 'repository' : 'repositories'}
    </p>

    <div class="branch-row">
      <span class="branch-label" id="switch-branch-label">Branch</span>
      <div class="picker">
        <button
          bind:this={branchFieldEl}
          type="button"
          class="field mono"
          class:empty={branch === ''}
          aria-labelledby="switch-branch-label"
          aria-expanded={pickerOpen}
          onclick={() => (pickerOpen = !pickerOpen)}
        >
          <span class="field-name">{branch === '' ? 'Pick a branch…' : branch}</span>
          <span class="caret" aria-hidden="true">▾</span>
        </button>

        {#if pickerOpen}
          <div class="menu">
            <!-- Required, not polish (§8.3): the row-level switcher has the
                 same box for the same reason, and a root of seventy-seven
                 repos has more branch names in it than any one of them does. -->
            <div class="menu-search">
              <!-- svelte-ignore a11y_autofocus -->
              <input
                bind:value={branchQuery}
                type="text"
                placeholder="Filter branches…"
                spellcheck="false"
                autocapitalize="off"
                autocorrect="off"
                autofocus
                aria-label="Filter branches"
              />
            </div>
            <div class="section">
              <span>Across all {listed.length}</span>
              <span class="count-note">repos with it</span>
            </div>
            {#if pickable.length === 0}
              <p class="no-matches">
                {loading ? 'Reading branches…' : 'No branch matches'}
              </p>
            {:else}
              <div class="menu-list">
                {#each pickable as entry (entry.name)}
                  <button
                    type="button"
                    class="branch-option"
                    class:picked={entry.name === branch}
                    onclick={() => pick(entry.name)}
                  >
                    <span class="option-name mono">{entry.name}</span>
                    <span class="option-count">{entry.repos}</span>
                  </button>
                {/each}
              </div>
            {/if}
            <p class="menu-foot">Every branch name here, local or on a remote.</p>
          </div>
        {/if}
      </div>
    </div>

    <!-- Always in the layout, only sometimes saying something — otherwise
         picking a branch makes the whole dialog jump. -->
    <p class="split" class:shown={branch !== ''}>
      {#if branch !== ''}
        local in {split.local} · on the remote only in {split.remote} · absent in {split.absent}
      {/if}
    </p>

    <div class="repos">
      <div class="section">
        <!-- The head of the checkbox column rather than a button in the header:
             it sits where the checkboxes are, so nothing has to say what it
             toggles (§11.1's container rule, one level down). -->
        <label class="check-all">
          <input
            type="checkbox"
            checked={allChecked}
            indeterminate={someChecked}
            disabled={selectable.length === 0}
            onchange={toggleAll}
            aria-label="Select every repository that can take the branch"
          />
          <span>All ({listed.length})</span>
        </label>
        <span class="count-note">{targets.length} selected</span>
      </div>

      {#each listed as repo (repo.id)}
        {@const plan = plans.get(repo.id)}
        {@const isBlocked = plan?.kind === 'blocked'}
        {@const context = contextFor(repo)}
        <label class="repo" class:blocked={isBlocked}>
          <input
            type="checkbox"
            checked={selected.has(repo.id)}
            disabled={isBlocked}
            onchange={() => toggle(repo.id)}
            aria-label={repo.name}
          />
          <span class="name">{repo.name}</span>
          {#if isBlocked}
            <!-- The reason takes the plan's slot: a repo that cannot take the
                 branch has no plan worth reading. -->
            <span class="reason">{planLabel(repo.id)}</span>
          {:else if context.dirty}
            <!-- Not an exclusion (§8.3): git switches through uncommitted
                 changes unless they collide, which cannot be known without
                 trying, and force-checkout is never offered. So the row stays
                 ticked and says what might happen instead. -->
            <span class="caution">uncommitted · may refuse</span>
          {:else}
            <span class="plan">
              {#if plan?.kind === 'create'}<span class="new">new</span>{/if}{planLabel(repo.id)}
            </span>
          {/if}
        </label>
      {/each}

      <div class="search">
        <span class="plus" aria-hidden="true">+</span>
        <input
          bind:value={query}
          type="text"
          placeholder="Add a pinned repository by name…"
          spellcheck="false"
          autocapitalize="off"
          autocorrect="off"
          aria-label="Add a pinned repository by name"
        />
      </div>

      {#if terms.length > 0}
        <div class="section">
          <span>Pinned ({matches.length} {matches.length === 1 ? 'match' : 'matches'})</span>
          <!-- The rule, where it happens. The mirror image of
               `MultiBranchDialog`'s, and it has to be said for the same reason:
               there, ticking a repo pins it; here, pinning one would move it
               out of the section this dialog is about. -->
          <span class="count-note">pins are left alone</span>
        </div>
        {#if matches.length === 0}
          <p class="no-matches">No pinned repository matches</p>
        {:else}
          {#each matches as repo (repo.id)}
            <label class="repo">
              <input type="checkbox" checked={false} onchange={() => add(repo)} aria-label={repo.name} />
              <span class="name">{repo.name}</span>
            </label>
          {/each}
        {/if}
      {/if}
    </div>

    <!-- The arithmetic in front of the user, the way the run's own banner does
         it (§5.1). Reserved, so a branch that excludes more repos does not make
         the buttons jump. -->
    <p class="summary" class:shown={excluded > 0}>
      {excluded}
      {excluded === 1 ? 'repository is' : 'repositories are'} excluded.
    </p>

    <label class="after">
      <input type="checkbox" bind:checked={pull} />
      Pull after switching
    </label>

    <div class="buttons">
      <button type="button" onclick={onClose}>Cancel</button>
      <button type="button" class="primary" disabled={!canSwitch} onclick={confirm}>
        {#if loading}
          Reading…
        {:else if pull}
          Switch &amp; pull {targets.length}
        {:else}
          Switch {targets.length}
        {/if}
      </button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    /* Above ContextMenu's 100, matching the other dialogs: the band or menu
       that opened this is still unwinding its own close when it first paints. */
    z-index: 200;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.45);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    /* `MultiBranchDialog`'s width, and for a related reason: the extra 80 over
       `CreateBranchDialog` is a per-repo column, and this one's says what will
       happen rather than where it starts. */
    width: 400px;
    padding: var(--space-3);
    background: var(--bg-raised);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .title {
    margin: 0;
    font-size: var(--text-md);
    font-weight: 600;
    color: var(--text-primary);
  }

  .scope {
    margin: var(--space-1) 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--text-muted);
  }

  .branch-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .branch-label {
    flex: 0 0 auto;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  /* The anchor the menu hangs off. `relative` here rather than on the dialog so
     the menu tracks the field if the label's width ever changes. */
  .picker {
    position: relative;
    flex: 1 1 0;
    min-width: 0;
  }

  .field {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: 26px;
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    text-align: left;
    cursor: default;
  }

  /* Branch names are compared character for character, so the picked one shows
     in the font that makes a doubled hyphen or an l/1 visible. */
  .mono {
    font-family: var(--font-mono);
  }

  /* The placeholder is UI text, not a ref — it should not be monospaced. */
  .field.empty {
    font-family: var(--font-ui);
    color: var(--text-disabled);
  }

  .field:hover {
    border-color: var(--border-strong);
  }

  .field-name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .caret {
    margin-left: auto;
    font-family: var(--font-ui);
    color: var(--text-muted);
  }

  .menu {
    position: absolute;
    z-index: 1;
    top: calc(26px + var(--space-1));
    left: 0;
    right: 0;
    display: flex;
    flex-direction: column;
    /* One step lighter than the dialog it floats over (§11.1). */
    background: var(--bg-hover);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }

  .menu-search {
    padding: 6px var(--space-2);
    border-bottom: 1px solid var(--border);
  }

  .menu-search input {
    width: 100%;
    height: 24px;
  }

  .menu-list {
    max-height: 180px;
    overflow-y: auto;
  }

  .branch-option {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: 26px;
    padding: 0 var(--space-2);
    border: 0;
    background: none;
    font: inherit;
    text-align: left;
    cursor: default;
  }

  .branch-option:hover {
    background: var(--bg-active);
  }

  /* The accent, because this is a selection — the one thing §11 reserves it
     for besides a primary button. */
  .branch-option.picked {
    background: var(--accent-muted);
  }

  .option-name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-primary);
  }

  .option-count {
    flex: 0 0 auto;
    margin-left: auto;
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
  }

  .menu-foot {
    margin: 0;
    padding: var(--space-1) var(--space-2) 6px;
    border-top: 1px solid var(--border);
    font-size: var(--text-xs);
    color: var(--text-disabled);
  }

  .split {
    min-height: 16px;
    margin: var(--space-1) 0 var(--space-3);
    font-size: var(--text-xs);
    color: var(--text-muted);
    visibility: hidden;
  }

  .split.shown {
    visibility: visible;
  }

  input[type='text'] {
    width: 100%;
    height: 28px;
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  input[type='text']:focus-visible {
    border-color: var(--accent);
    outline: none;
  }

  input[type='text']::placeholder {
    color: var(--text-disabled);
  }

  /* Framed, so it reads as a list inside a form rather than as the form's own
     rows. Scrolls rather than growing: this section is usually the whole root. */
  .repos {
    max-height: 240px;
    overflow-y: auto;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .section {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 6px var(--space-2) var(--space-1);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  /* Aligned with the row checkboxes below, which is the whole point of it
     sitting here rather than in the dialog's header. */
  .check-all {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }

  .check-all input {
    margin: 0;
    accent-color: var(--accent);
  }

  .count-note {
    text-transform: none;
    letter-spacing: 0.04em;
    font-weight: 400;
    color: var(--text-disabled);
  }

  .repo {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 26px;
    padding: 0 var(--space-2);
  }

  .repo:hover {
    background: var(--bg-hover);
  }

  .repo input {
    flex: 0 0 auto;
    margin: 0;
    accent-color: var(--accent);
  }

  .name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-primary);
  }

  .repo.blocked .name {
    color: var(--text-disabled);
  }

  .plan {
    flex: 0 0 auto;
    margin-left: auto;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-muted);
    white-space: nowrap;
  }

  /* The one row that leaves a branch behind that was not there before, said in
     the UI face so it reads as a note about the plan rather than part of it. */
  .new {
    margin-right: var(--space-2);
    font-family: var(--font-ui);
    color: var(--text-disabled);
  }

  /* --status-dirty, matching the row badge that means the same thing in the
     pane behind this (§11): the tree has uncommitted work in it. Not
     --status-error, which is the column's "this will not run" colour — this
     row is still ticked and still going. */
  .caution {
    flex: 0 0 auto;
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--status-dirty);
    white-space: nowrap;
  }

  /* --status-error, not --status-conflict: nothing has failed here. This is the
     dialog saying so before anything runs, which is the quieter of §13's two
     reds. */
  .reason {
    flex: 0 0 auto;
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--status-error);
    white-space: nowrap;
  }

  /* Bordered on top only, so it still reads as part of the list it extends
     rather than as a second field. */
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 28px;
    padding: 0 var(--space-2);
    border-top: 1px solid var(--border);
    background: var(--bg-raised);
  }

  .search .plus {
    flex: 0 0 auto;
    color: var(--text-muted);
    font-size: var(--text-md);
    line-height: 1;
  }

  .search input {
    height: 100%;
    border: 0;
    background: none;
    border-radius: 0;
  }

  .search input:focus-visible {
    outline: none;
  }

  .no-matches {
    margin: 0;
    padding: var(--space-1) var(--space-2) 6px;
    font-size: var(--text-xs);
    color: var(--text-disabled);
  }

  .summary {
    min-height: 16px;
    margin: var(--space-2) 0 0;
    font-size: var(--text-xs);
    color: var(--text-muted);
    visibility: hidden;
  }

  .summary.shown {
    visibility: visible;
  }

  .after {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-2) 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .after input {
    accent-color: var(--accent);
  }

  /* Same button treatment as the compose pane's Commit/Push pair (§11). */
  .buttons {
    display: flex;
    gap: var(--space-2);
  }

  .buttons button {
    flex: 1 1 0;
    height: 28px;
    padding: 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .buttons button:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: var(--border-strong);
  }

  .buttons button.primary:not(:disabled) {
    color: var(--accent-text);
    background: var(--accent-muted);
    border-color: var(--accent);
  }

  .buttons button.primary:hover:not(:disabled) {
    color: var(--accent-text);
    background: var(--accent);
    border-color: var(--accent-hover);
  }

  .buttons button:disabled {
    color: var(--text-disabled);
    cursor: default;
  }
</style>
