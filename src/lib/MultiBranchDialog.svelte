<script lang="ts">
  // Create Branch across the pinned set (SPEC.md §5.1, §8.3) — opened from the
  // repo list's *Pinned* section header, or Repository ▸ Create Branch in
  // Pinned.
  //
  // Deliberately not a second copy of `CreateBranchDialog`, and deliberately
  // not a generalisation of it either. That one is opened from a ref badge and
  // takes its start point from the badge that was right-clicked; this one is
  // opened from a *set*, and its start point is per repo. They share the name
  // rules (`branchName.ts`) and the chrome, which is where the overlap ends —
  // folding them together would mean a dialog whose every second line is a
  // branch on which surface opened it.
  //
  // 400px rather than that dialog's 320: the extra 80 is the start-point
  // column, which is the whole reason this one exists. Same modal treatment
  // for the same reason — a form you are halfway through typing into must not
  // close on the next outside mousedown.
  import { validateBranchName } from './branchName';
  import { filterTerms, matchesFilter } from './repoFilter';
  import { hasConflict, repos, type Repo } from './repos.svelte';

  interface Props {
    /** Fires with the repos to act on. The dialog closes on the click rather
     *  than waiting for the run: the strip is where a bulk run narrates itself
     *  (§5.1), and a modal over the list would cover the rows whose new branch
     *  names are the result (§14.1). */
    onCreate: (repoIds: string[], name: string, checkout: boolean) => void;
    onClose: () => void;
  }

  let { onCreate, onClose }: Props = $props();

  let name = $state('');
  // Checked by default, matching `CreateBranchDialog`: creating a branch you
  // do not then work in is the rarer of the two intents. It carries more
  // weight here — it is also what decides whether a repo mid-merge can be
  // included at all (see `blockerFor`).
  let checkout = $state(true);
  let query = $state('');
  let inputEl: HTMLInputElement | undefined = $state();

  /** Every repo's local branch names, read once when the dialog opens (§8.3).
   *  Keyed by repo id; a repo whose refs could not be read maps to `null`,
   *  which blocks it rather than waving it through. */
  let branchNames = $state<Map<string, string[] | null>>(new Map());
  let loading = $state(true);

  /** The pinned set is the dialog's subject, and it is live: opting a repo in
   *  below pins it, so it leaves the matches and arrives here. */
  const pinned = $derived(repos.pinnedRepos);

  /** Ticked repos. Seeded from the pinned set once the rows are known, then
   *  owned by the user — unticking one here must not unpin it (§5.1). */
  let selected = $state<Set<string>>(new Set());
  let seeded = $state(false);

  $effect(() => {
    inputEl?.focus();
  });

  // Reads `pinned` only to seed, and only once: re-running it on every change
  // would re-tick a row the user had just cleared.
  $effect(() => {
    if (seeded) return;
    selected = new Set(pinned.map((repo) => repo.id));
    seeded = true;
  });

  // One read for the whole dialog, on open. Every keystroke after this is
  // checked against what came back, never against git — five processes per
  // character is the one thing the spawn-bound path (§1) will not spend.
  $effect(() => {
    const ids = pinned.map((repo) => repo.id);
    if (ids.length === 0) {
      loading = false;
      return;
    }
    // Re-runs when `pinned` grows, which is what covers a repo opted in below:
    // it arrives here and is read like every other row, rather than being the
    // one repo in the dialog whose collisions are invisible. `loading` goes
    // back up for that read, so *Create* cannot fire against a half-known set.
    let cancelled = false;
    loading = true;
    void repos.localBranches(ids).then((entries) => {
      if (cancelled) return;
      // Rebuilt rather than merged: this is the whole answer for the current
      // list, and a merge would keep names for a repo that has since been
      // unpinned out from under the dialog.
      const next = new Map<string, string[] | null>();
      for (const entry of entries) next.set(entry.repoId, entry.names);
      branchNames = next;
      loading = false;
    });
    return () => {
      cancelled = true;
    };
  });

  const trimmed = $derived(name.trim());
  // Format only. The duplicate half of `validateBranchName` is per repo here,
  // and belongs on the rows that actually have the collision — one repo out of
  // five already holding the name is not a reason to refuse the whole run.
  const problem = $derived(validateBranchName(name, []));

  const terms = $derived(filterTerms(query));
  /** Repos the search offers to add — unpinned only, since everything pinned
   *  is already a row above. Same predicate as the repo list's own filter box,
   *  so the two boxes cannot disagree about what matches. */
  const matches = $derived(
    terms.length === 0
      ? []
      : repos.repos.filter((repo) => !repos.pins.has(repo.id) && matchesFilter(repo.name, terms)),
  );

  /**
   * Why this repo cannot take the branch, or `null` if it can.
   *
   * Both answers are known before anything runs, which is the point: a run
   * that would fail in one repo says so while the name is still being typed,
   * rather than coming back with one red row in the banner (§13).
   */
  function blockerFor(repo: Repo): string | null {
    const names = branchNames.get(repo.id);
    if (names === undefined) return null; // Not read yet — `loading` holds the button.
    if (names === null) return 'could not read its branches';
    if (trimmed.length > 0 && names.includes(trimmed)) return 'already has this branch';

    // Not a fact about the repo but about the checkbox below: git will cut a
    // branch mid-merge quite happily, it just will not switch onto it. So the
    // reason names the thing the user can actually change.
    const status = repos.status(repo.id);
    if (checkout && status !== undefined && hasConflict(status)) {
      return 'merge in progress — cannot check out';
    }
    return null;
  }

  /** What the branch will be cut from, as the row prints it. The run itself
   *  passes `HEAD` and lets git resolve it in the repo (§5.1's cache rule) —
   *  this is the same answer, read from a status that may be a sweep old, and
   *  it is shown rather than used. */
  function startPointFor(repo: Repo): string {
    const status = repos.status(repo.id);
    return status?.branch ?? status?.head ?? '';
  }

  const blocked = $derived(new Set(pinned.filter((repo) => blockerFor(repo) !== null).map((repo) => repo.id)));
  /** Ticked, minus anything that cannot take it. The button's count, the title
   *  and the ids handed to the run are all this one list.
   *
   *  Derived from the *rows* rather than from `selected`, which is why it is a
   *  filter over `pinned` and not over the set: a repo unpinned from the list
   *  behind the dialog leaves `selected` untouched, and branching a repo whose
   *  row is not on screen is the one outcome this dialog must not have. */
  const targets = $derived(
    pinned.filter((repo) => selected.has(repo.id) && !blocked.has(repo.id)).map((repo) => repo.id),
  );
  const canCreate = $derived(!loading && trimmed.length > 0 && problem === null && targets.length > 0);

  const excluded = $derived(pinned.filter((repo) => blocked.has(repo.id)).length);

  function toggle(id: string): void {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }

  /** Ticking a repo out of the matches adds it to the run *and* pins it — the
   *  one asymmetry in this dialog, and a deliberate one: adding pins, and
   *  unticking never unpins. Opting out of one run must not cost a pin that
   *  was set last week (§5.1). */
  async function add(repo: Repo): Promise<void> {
    toggle(repo.id);
    // Pinning is what moves the row up into the list above, which is what
    // makes the branch-name read above re-run over it. Nothing to do here.
    await repos.pin(repo.id);
  }

  function create(): void {
    if (!canCreate) return;
    onCreate(targets, trimmed, checkout);
    onClose();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      // Stopped rather than left to bubble, like every other dialog here.
      event.stopPropagation();
      onClose();
    } else if (event.key === 'Enter' && event.target === inputEl) {
      // Only from the name field: Enter on Cancel must still cancel, and
      // preventing its default here would swallow that activation.
      event.preventDefault();
      create();
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
    aria-label="Create branch in several repositories"
    onmousedown={(event) => event.stopPropagation()}
    onkeydown={onKeydown}
  >
    <p class="title">Create branch</p>
    <p class="scope">
      in {targets.length}
      {targets.length === 1 ? 'repository' : 'repositories'}
    </p>

    <input
      bind:this={inputEl}
      bind:value={name}
      type="text"
      class="mono"
      placeholder="Branch name"
      spellcheck="false"
      autocapitalize="off"
      autocorrect="off"
      aria-label="Branch name"
      aria-invalid={problem !== null}
    />

    <!-- Always in the layout, only sometimes visible — otherwise typing a bad
         character makes the whole dialog jump. -->
    <p class="problem" class:shown={problem !== null}>{problem ?? ''}</p>

    <!-- One control, because "which commit" is one decision even across five
         repos — but the answer differs per repo, so every row below prints its
         own resolved value rather than trusting this label. -->
    <div class="from-row">
      <label class="from-label" for="branch-start-point">From</label>
      <select id="branch-start-point">
        <option>Current branch in each repo</option>
      </select>
    </div>

    <div class="repos">
      <div class="section">
        <span>Pinned ({pinned.length})</span>
        <span class="count-note">{targets.length} selected</span>
      </div>

      {#each pinned as repo (repo.id)}
        {@const blocker = blockerFor(repo)}
        <label class="repo" class:blocked={blocker !== null}>
          <input
            type="checkbox"
            checked={selected.has(repo.id)}
            disabled={blocker !== null}
            onchange={() => toggle(repo.id)}
            aria-label={repo.name}
          />
          <span class="pin" class:dim={blocker !== null} aria-hidden="true">
            <svg viewBox="0 0 12 12" focusable="false">
              <path d="M4 1h4v1l-1 1v2.5l2 1.5v1H6.5V11h-1V8H3V7l2-1.5V3L4 2z" />
            </svg>
          </span>
          <span class="name">{repo.name}</span>
          {#if blocker !== null}
            <!-- The reason takes the start point's slot: a repo that cannot
                 take the branch has no start point worth reading. -->
            <span class="reason">{blocker}</span>
          {:else}
            <span class="start">{startPointFor(repo)}</span>
          {/if}
        </label>
      {/each}

      <div class="search">
        <span class="plus" aria-hidden="true">+</span>
        <input
          bind:value={query}
          type="text"
          placeholder="Add a repository by name…"
          spellcheck="false"
          autocapitalize="off"
          autocorrect="off"
          aria-label="Add a repository by name"
        />
      </div>

      {#if terms.length > 0}
        <div class="section">
          <span>All ({matches.length} {matches.length === 1 ? 'match' : 'matches'})</span>
          <!-- The rule, where it happens, rather than in a tooltip. -->
          <span class="count-note">checking one pins it</span>
        </div>
        {#if matches.length === 0}
          <p class="no-matches">No unpinned repository matches</p>
        {:else}
          {#each matches as repo (repo.id)}
            <label class="repo">
              <input type="checkbox" checked={false} onchange={() => void add(repo)} aria-label={repo.name} />
              <span class="pin off" aria-hidden="true">
                <svg viewBox="0 0 12 12" focusable="false">
                  <path d="M4 1h4v1l-1 1v2.5l2 1.5v1H6.5V11h-1V8H3V7l2-1.5V3L4 2z" />
                </svg>
              </span>
              <span class="name">{repo.name}</span>
              <span class="start">{startPointFor(repo)}</span>
            </label>
          {/each}
        {/if}
      {/if}
    </div>

    <!-- The arithmetic in front of the user, the way the run's own banner does
         it (§5.1). Reserved, so a name that starts colliding does not make the
         buttons jump. -->
    <p class="summary" class:shown={excluded > 0}>
      {excluded}
      {excluded === 1 ? 'repository is' : 'repositories are'} excluded.
    </p>

    <label class="checkout">
      <input type="checkbox" bind:checked={checkout} />
      Check out after creating
    </label>

    <div class="buttons">
      <button type="button" onclick={onClose}>Cancel</button>
      <button type="button" class="primary" disabled={!canCreate} onclick={create}>
        {loading ? 'Reading…' : `Create in ${targets.length}`}
      </button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    /* Above ContextMenu's 100, matching CreateBranchDialog: the menu or header
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

  /* The name is compared against refs character for character, so it is shown
     in the font that makes a doubled hyphen or an l/1 visible. */
  input.mono {
    font-family: var(--font-mono);
  }

  input[type='text']:focus-visible {
    border-color: var(--accent);
    outline: none;
  }

  input[type='text'][aria-invalid='true'] {
    border-color: var(--status-conflict);
  }

  input[type='text']::placeholder {
    color: var(--text-disabled);
  }

  .problem {
    min-height: 16px;
    margin: var(--space-1) 0 0;
    font-size: var(--text-xs);
    color: var(--status-conflict);
    visibility: hidden;
  }

  .problem.shown {
    visibility: visible;
  }

  .from-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-1) 0 var(--space-3);
  }

  .from-label {
    flex: 0 0 auto;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  select {
    flex: 1 1 0;
    min-width: 0;
    height: 26px;
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  /* Framed, so it reads as a list inside a form rather than as the form's own
     rows. Scrolls rather than growing: a hot set is three to five repos, but
     the search can put a dozen matches under it. */
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

  /* The pin is in the row so that "checking a repo also pins it" is visible
     rather than explained. */
  .pin {
    flex: 0 0 auto;
    width: 11px;
    height: 11px;
    color: var(--text-secondary);
  }

  .pin.off {
    color: var(--text-disabled);
    opacity: 0.55;
  }

  .pin.dim {
    color: var(--text-disabled);
  }

  .pin svg {
    display: block;
    width: 11px;
    height: 11px;
    fill: currentColor;
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

  .start {
    flex: 0 0 auto;
    margin-left: auto;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  /* --status-error, not --status-conflict: nothing has failed here. This is
     the dialog saying so before anything runs, which is the quieter of §13's
     two reds. */
  .reason {
    flex: 0 0 auto;
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--status-error);
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

  .checkout {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-2) 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .checkout input {
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
