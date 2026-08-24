<script lang="ts">
  import { isDirty, publishReason, repos, type Repo, type RepoStatus } from '../repos.svelte';
  import ContextMenu from '../ContextMenu.svelte';
  import { notices } from '../notices.svelte';

  interface Props {
    repo: Repo;
    status?: RepoStatus;
    error?: string;
  }

  let { repo, status, error }: Props = $props();

  const selected = $derived(repos.selected.has(repo.id));
  const dirty = $derived(status !== undefined && isDirty(status));
  // Detached HEAD has no branch name; the short oid is the honest substitute.
  const branch = $derived(status?.branch ?? status?.head ?? '');
  // Ahead/behind come from `# branch.ab`, which git only emits when an
  // upstream exists — so a branch with twelve unpushed commits and no upstream
  // reports ahead 0 and renders exactly like a synced repo. Marking the name
  // rather than adding a badge keeps this off the badge strip, which §5.1
  // reserves for "does this need me?": unpublished is a fact about the branch,
  // not a repo that needs attention. It also costs no width, and the strip is
  // already what squeezes the name to an ellipsis on a narrow pane.
  const publish = $derived(status === undefined ? null : publishReason(status));
  const unpublished = $derived(publish !== null);
  // Both states underline the name, but they are not the same fact and the
  // tooltip is the only place that can say which. Telling someone whose
  // branch tracks `origin/main` that it has no upstream would send them
  // looking for the wrong problem.
  const publishHint = $derived(
    publish === 'upstream-name-mismatch'
      ? `${branch} tracks ${status?.upstream} — a differently-named branch, so Push cannot work. Publish will point it at origin/${branch}.`
      : `${branch} is not published — no upstream branch on the remote`,
  );
  // The badge shows a bare number, so the accessible name and the tooltip are
  // the only places that can say what it counts.
  const changedLabel = $derived(
    `${status?.changedFiles ?? 0} file${status?.changedFiles === 1 ? '' : 's'} with uncommitted changes`,
  );
  const pinned = $derived(repos.pins.has(repo.id));
  // The background fetch sweep stopped retrying this repo (§8.7, §13) — a
  // manual fetch is what clears it. Shown alongside the other badges rather
  // than replacing them: unlike a status-read failure, this repo's status is
  // still known and current, just possibly stale on the "behind" count.
  const authNeeded = $derived(repos.authNeeded.has(repo.id));
  // A row-triggered write (Fetch/Pull from this very row) failing is the
  // freshest, most actionable signal — it wins over a stale status-read
  // failure when both happen to be present (§5.1, §13).
  const rowError = $derived(repos.rowErrors[repo.id] ?? error);
  // Only the write failure may be dismissed (§13's event-versus-state rule).
  // `error` is the sweep's status-read failure, republished on every tick, so
  // a Dismiss on it would be undone within one interval — the button would be
  // broken by construction rather than merely unhelpful.
  const canDismissError = $derived(repos.rowErrors[repo.id] !== undefined);
  const canPullRow = $derived(status !== undefined && status.behind > 0 && status.conflicted === 0);

  let menuPos = $state<{ x: number; y: number } | null>(null);
  let pulling = $state(false);
  let rowEl: HTMLButtonElement | undefined = $state();

  /**
   * Keep the selected row on screen. Pinning moves a repo between the two
   * sections of the list (§5.1), which destroys this component and builds it
   * again in the other one — at a scroll offset that, over a folder of 77
   * repos, is usually nowhere near the viewport. Nothing about the selection
   * changed, but the only thing showing it just left the screen, which reads
   * exactly like the selection was cleared.
   *
   * The effect belongs on the row rather than on the list because the row is
   * what remounts: a list-level effect would have to watch for a reorder it
   * cannot see. It also covers the restored selection on startup (§9.5),
   * which has the same problem for the same reason.
   *
   * `nearest` deliberately: a row already in view must not jump, and the
   * common case — clicking a row you can see — has to be a no-op.
   */
  $effect(() => {
    if (selected) rowEl?.scrollIntoView({ block: 'nearest' });
  });

  function openMenu(event: MouseEvent) {
    event.preventDefault();
    menuPos = { x: event.clientX, y: event.clientY };
  }

  /** The badge points at the error; it does not render a second copy of it
   *  (§5.1, §13). Selecting the repo is what raises its banner, which is the
   *  one surface wide enough for a headline, an action and *Details*.
   *
   *  Cheap and non-destructive: selection drives the middle pane and graph,
   *  and the compose pane's message is component-local, so nothing typed is
   *  lost by arriving here. */
  function showError(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    if (rowError !== undefined) {
      // "Status" when the badge is a failed sweep read rather than a write the
      // user asked for — there is no operation to name in that case, and
      // Retry would re-run nothing.
      const operation = repos.rowOperation(repo.id);
      notices.raise(
        repo.id,
        operation ?? 'Status',
        rowError,
        operation === undefined ? undefined : () => void repos.retryRow(repo.id),
      );
    }
    if (!selected) repos.select(repo.id);
  }

  function togglePin(event: MouseEvent) {
    // The pin lives inside the row's click target, and clicking it must not
    // also select the repo — pinning a repo you are not working in right now
    // is a perfectly ordinary thing to do.
    event.preventDefault();
    event.stopPropagation();
    void repos.togglePin(repo.id);
  }

  async function pullRow(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    if (pulling) return;
    pulling = true;
    try {
      await repos.pullRow(repo.id);
    } finally {
      pulling = false;
    }
  }

  const menuItems = $derived([
    { label: pinned ? 'Unpin' : 'Pin', onSelect: () => void repos.togglePin(repo.id) },
    // "Fetch now" rather than "Dismiss" on an auth-badged row, and the label
    // is the honest one (§13): `authNeeded` is a *scheduling* flag — the fetch
    // sweep skips these repos until a manual fetch clears it — so making the
    // badge go away and retrying the fetch are the same act. Calling it
    // Dismiss would promise silence and deliver a retry that re-badges within
    // one sweep.
    { label: authNeeded ? 'Fetch now' : 'Fetch', onSelect: () => void repos.fetchRepo(repo.id) },
    { label: 'Open in VS Code', onSelect: () => void repos.openInVSCode(repo.id) },
    { label: 'Open in Terminal', onSelect: () => void repos.openInTerminal(repo.id) },
    { label: 'Copy path', onSelect: () => void navigator.clipboard.writeText(repo.path) },
    ...(canDismissError
      ? [{ label: 'Dismiss error', onSelect: () => repos.dismissRowError(repo.id) }]
      : []),
  ]);
</script>

<button
  bind:this={rowEl}
  type="button"
  class="row"
  class:selected
  aria-current={selected}
  title={repo.path}
  onclick={() => repos.select(repo.id)}
  oncontextmenu={openMenu}
>
  <!-- The hot set is only worth having if putting a repo in it costs one
       click (§5.1), so the pin is on the row, not only in the context menu.
       The gutter is always reserved — revealing it on hover must not shuffle
       the names sideways. -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <span
    role="button"
    tabindex="-1"
    class="pin"
    class:pinned
    aria-pressed={pinned}
    title={pinned ? 'Unpin' : 'Pin to the top'}
    aria-label={pinned ? `Unpin ${repo.name}` : `Pin ${repo.name}`}
    onclick={togglePin}
  >
    <svg viewBox="0 0 12 12" aria-hidden="true" focusable="false">
      <path d="M4 1h4v1l-1 1v2.5l2 1.5v1H6.5V11h-1V8H3V7l2-1.5V3L4 2z" />
    </svg>
  </span>

  <span class="name">{repo.name}</span>

  <span class="meta">
    {#if canPullRow}
      <!-- Hover-revealed, and only on rows that are behind (§5.1) — acting
           without a select-then-cross-the-window trip is the whole point. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <span
        role="button"
        tabindex="-1"
        class="pull-action"
        class:pulling
        title="Pull"
        aria-label="Pull"
        onclick={pullRow}
      >{pulling ? '…' : '⇩'}</span>
    {/if}

    {#if rowError}
      <!-- A repo whose status could not be read — or whose row-triggered
           write just failed — is unknown/needs-attention, not clean, and
           must never render as a clean row (§5.1). Click selects the repo and
           raises its banner (§13) rather than opening a popover with a second
           copy of the notice in it. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <span
        role="button"
        tabindex="-1"
        class="badge error"
        title={rowError}
        onclick={showError}
      >!</span>
    {/if}
    {#if status}
      <span
        class="branch"
        class:detached={!status.branch}
        class:unpublished
        title={unpublished ? publishHint : undefined}
      >{branch}</span>

      {#if status.conflicted > 0}
        <span class="badge conflict" title="Merge conflict">⚠</span>
      {:else if dirty}
        <!-- One number, one state. The row still answers "does this need me?"
             (§5.1) — staged versus unstaged remains the middle pane's job —
             but the count is what turns "something changed here" into "this is
             a two-line fix, that one is an afternoon" without a click. -->
        <span class="count" title={changedLabel} aria-label={changedLabel}>{status.changedFiles}</span>
      {/if}

      {#if status.ahead > 0}
        <span class="badge ahead" title="{status.ahead} commit(s) to push">↑{status.ahead}</span>
      {/if}
      {#if status.behind > 0}
        <span class="badge behind" title="{status.behind} commit(s) to pull">↓{status.behind}</span>
      {/if}
      {#if authNeeded}
        <span class="badge auth" title="Authentication needed — fetch manually to sign in">⚿</span>
      {/if}
    {/if}
  </span>
</button>

{#if menuPos}
  <ContextMenu x={menuPos.x} y={menuPos.y} items={menuItems} onClose={() => (menuPos = null)} />
{/if}

<style>
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: var(--row-height);
    padding: 0 var(--space-3) 0 var(--space-1);
    border: 0;
    background: none;
    text-align: left;
    cursor: default;
  }

  .row:hover {
    background: var(--bg-hover);
  }

  /* The accent is for selection only — status colours must stay distinct from
     it or the list stops being scannable, which is the point of this pane. */
  .row.selected {
    background: var(--accent-muted);
  }

  /* Reserved on every row, drawn only when the repo is pinned or the row is
     hovered — `visibility` rather than `display` so the names stay aligned. */
  .pin {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
    border-radius: var(--radius-sm);
    visibility: hidden;
    color: var(--text-disabled);
  }

  .pin svg {
    width: 11px;
    height: 11px;
    fill: currentColor;
  }

  .row:hover .pin,
  .row:focus-visible .pin,
  .pin.pinned {
    visibility: visible;
  }

  /* Pinned reads as an established state, not an available action — hence
     the step up in contrast rather than the accent, which §11 reserves for
     selection and primary buttons. */
  .pin.pinned {
    color: var(--text-secondary);
  }

  .pin:hover {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  .name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--text-primary);
  }

  /* Everything after the name hugs the right edge; the slack sits between. */
  .meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 0 1 auto;
    min-width: 0;
    margin-left: auto;
  }

  .branch {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-muted);
  }

  .branch.detached {
    font-family: var(--font-mono);
  }

  /* No upstream (§8.7) — the branch exists only on this machine. Underlined
     in the ahead badge's colour because that is the same condition taken to
     its limit: nothing on an unpublished branch has been pushed. A decoration
     rather than a badge, so the row gains a state without gaining a column.
     Mutually exclusive with `.detached`, which has no branch to publish. */
  .branch.unpublished {
    color: var(--text-secondary);
    text-decoration: underline dashed var(--status-ahead);
    text-decoration-thickness: 1px;
    text-underline-offset: 3px;
  }

  /* The dirty dot, grown enough to hold its own count — VS Code's SCM badge.
     Filled rather than coloured text like the ahead/behind badges: those are
     read after you have already decided a row is interesting, this one is what
     makes you decide, and a solid shape survives being scanned at 77 rows.
     Neutral rather than a status hue — see --count-bg for why a number is not
     a state.
     A circle at one digit, stretching to a pill at two or three — `min-width`
     with a symmetric `padding` and a radius past half the height, so 3 and 128
     are the same object rather than two differently-shaped ones. */
  .count {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    min-width: 16px;
    height: 16px;
    padding: 0 5px;
    border-radius: 8px;
    background: var(--count-bg);
    color: var(--count-text);
    font-size: var(--text-xs);
    /* Digits share a width, so counts line up down the strip and a row does
       not twitch sideways when 9 becomes 10. */
    font-variant-numeric: tabular-nums;
    font-weight: 600;
    line-height: 1;
  }

  .badge {
    flex: 0 0 auto;
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }

  /* Row-level Pull (§5.1): hidden until the row is hovered/focused, and only
     ever rendered on rows that are behind in the first place. */
  .pull-action {
    display: none;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    font-size: var(--text-sm);
    line-height: 1;
  }

  .row:hover .pull-action,
  .row:focus-visible .pull-action {
    display: flex;
  }

  .pull-action:hover {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  .pull-action.pulling {
    display: flex;
    color: var(--text-disabled);
  }

  .ahead {
    color: var(--status-ahead);
  }

  .behind {
    color: var(--status-behind);
  }

  .error {
    color: var(--status-error);
    font-weight: 700;
  }

  .badge.error:hover {
    text-decoration: underline;
  }

  .conflict {
    color: var(--status-conflict);
  }

  .auth {
    color: var(--status-dirty);
  }
</style>
