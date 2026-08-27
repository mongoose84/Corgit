<script lang="ts">
  import Pane from './Pane.svelte';
  import FileRow from './FileRow.svelte';
  import EmptyState from '../EmptyState.svelte';
  import Mascot from '../Mascot.svelte';
  import Glyph from '../Glyph.svelte';
  import DiscardDialog from '../DiscardDialog.svelte';
  import ContextMenu from '../ContextMenu.svelte';
  import { hasConflict, needsPublish, repos, type FileEntry } from '../repos.svelte';
  import { diff, sourceForRow } from '../diff.svelte';
  import {
    extend,
    isSelected,
    prune,
    selectOne,
    selectedRows,
    step,
    toggle,
    type FileSection,
    type FileSelection,
  } from '../fileSelection';
  import { exactPattern, ignoreCandidates } from '../ignorePatterns';
  import { tick } from 'svelte';

  // Mode A (working tree) — SPEC.md §5.2. Commit details (Mode B) live in
  // their own panel (graph.svelte.ts + CommitInfoPanel.svelte) rather than
  // taking over this pane, so staging/commit stays visible at all times.
  let message = $state('');
  let busy = $state(false);

  const hasRepo = $derived(repos.selectedId !== undefined);
  const files = $derived(repos.files);
  const status = $derived(repos.selectedId ? repos.status(repos.selectedId) : undefined);
  // No upstream configured (§8.7) — "Push" becomes "Publish branch" rather
  // than a separate control, since exactly one of the two ever applies. Shared
  // with the repo row's local-only branch marking (§5.1).
  const publishable = $derived(status !== undefined && needsPublish(status));
  // §13: an unresolved merge conflict blocks commit and push for this repo
  // until it's resolved or aborted — exactly two ways out, never a third.
  //
  // The pane no longer *draws* the conflict: that is the blocking banner in
  // the app chrome, which can hold a headline and both buttons on one line at
  // window width and could not at this pane's 240px minimum (§4). What stays
  // here is the part that was always this pane's own — the guard on its two
  // buttons. Via the shared predicate, so the banner and the disabled Commit
  // cannot disagree about whether this repo is wedged.
  const conflicted = $derived(status !== undefined && hasConflict(status));

  const canCommit = $derived(
    hasRepo && !busy && !conflicted && message.trim().length > 0 && (files?.stagedTotal ?? 0) > 0,
  );
  const canPush = $derived(hasRepo && !busy && !conflicted);

  function sectionLabel(shown: number, total: number): string {
    return shown === total ? `${total}` : `${shown} of ${total}`;
  }

  // The payoff state (SPEC.md §14.1, docs/mascot-clean-pane.md): a selected
  // repo with genuinely nothing to commit. `content` is wired to the herd-wide
  // version of this in the graph pane, which is unreachable while you are
  // actually working — select a clean repo and it goes away.
  //
  // Judged from `files`, never from `isDirty(status)`. `status` comes from the
  // sweep cache and the cache is never truth (§5.1) — it can be a sweep behind
  // the rows this pane just drew, and a dog lying down over changes that are
  // on screen is the one way this state can lie. This is the rare place where
  // *not* reusing the shared predicate is the correct call.
  //
  // Totals, not `staged.length`/`unstaged.length`: those lists are capped, and
  // that is what `sectionLabel` above exists for.
  const atRest = $derived(
    !conflicted &&
      !repos.loadingFiles &&
      files !== null &&
      files.stagedTotal === 0 &&
      files.unstagedTotal === 0,
  );

  // Discard (§5.2) — the only thing in the pane that destroys work, so it is
  // scoped as narrowly as it can honestly be and confirmed every time.
  //
  // *Changes* only, and only its tracked rows. A staged row's Discard could
  // only mean "throw away the staged work as well", which is not what a button
  // sitting beside − reads as; unstage first and the row appears here, where
  // discarding it means one plain thing. Untracked files are excluded because
  // git has nothing to restore them from: discarding one could only be `git
  // clean` deleting it outright, and Corgit does not delete files.
  //
  // Still no tick column — that is what made *Changes* read as a form to be
  // filled in rather than a list of what changed. A batch is built by
  // ctrl/shift-clicking rows instead (§5.2), which costs the list nothing when
  // nobody is using it, and reaches discard through the same dialog.
  /** Non-null while the confirmation is up; the value is the exact list the
   *  dialog is showing. */
  let confirming = $state<FileEntry[] | null>(null);

  /** Ctrl/shift-click selection over one section's rows (§5.2). Lives here and
   *  not in `repos.svelte.ts`: it is about what is on screen, and the store
   *  mirrors backend state rather than holding state of its own. */
  let selection = $state<FileSelection | null>(null);

  /** The right-click menu, holding the rows it was opened on rather than
   *  re-deriving them: a sweep or an FS watcher can rewrite the file list while
   *  a menu is up (§7), and *Stage 3 files* has to stage the three that were
   *  counted. */
  let menu = $state<{ x: number; y: number; section: FileSection; entries: FileEntry[] } | null>(
    null,
  );

  function rowsFor(section: FileSection): readonly FileEntry[] {
    return (section === 'staged' ? files?.staged : files?.unstaged) ?? [];
  }

  // Staging a selection is the ordinary end of it: the rows leave the section
  // and the selection empties itself, rather than lingering as a highlight over
  // whatever slid up into their place. Also covers switching repos, and another
  // window or a terminal changing the tree underneath us.
  $effect(() => {
    const pruned = prune(selection, rowsFor(selection?.section ?? 'unstaged'));
    if (pruned !== selection) selection = pruned;
  });

  function openDiff(section: FileSection, entry: FileEntry) {
    const id = repos.selectedId;
    if (!id) return;
    diff.show(id, entry.path, sourceForRow(section, entry));
  }

  function isOpen(section: FileSection, entry: FileEntry): boolean {
    const id = repos.selectedId;
    return id !== undefined && diff.isOpen(id, entry.path, sourceForRow(section, entry));
  }

  /** A modified click builds the selection; a plain one is what it always was,
   *  a way into the diff. Deliberately not both: opening a diff per ctrl-click
   *  would spawn a `git diff` for every row picked on the way to staging six of
   *  them, and leave the right pane showing whichever one happened to be
   *  last. */
  function rowClick(section: FileSection, entry: FileEntry, event: MouseEvent) {
    // `metaKey` for the Mac this does not ship on yet (§10) — one clause now
    // costs less than the bug report later.
    if (event.ctrlKey || event.metaKey) {
      selection = toggle(selection, section, entry.path);
      return;
    }
    if (event.shiftKey) {
      selection = extend(selection, section, entry.path, rowsFor(section));
      return;
    }
    selection = selectOne(section, entry.path);
    cancelPendingDiff();
    openDiff(section, entry);
  }

  /** The two `<ul>`s, so a step can put focus on the row it moved to. Either
   *  can be undefined — an empty section draws a `<p>` instead of a list. */
  let stagedList = $state<HTMLUListElement>();
  let unstagedList = $state<HTMLUListElement>();

  /** Trailing coalesce for a *held* arrow key. Each row's diff is a `git diff`
   *  process, and on this platform the spawn alone costs more than the diff
   *  (CLAUDE.md, §1) — holding ↓ through thirty files would queue thirty of
   *  them through the global semaphore to show the user one. A deliberate press
   *  is never delayed: `event.repeat` separates the two exactly, so this timer
   *  only ever exists while the key is down. */
  let pendingDiff: ReturnType<typeof setTimeout> | null = null;
  /** Long enough to swallow Windows' default key-repeat interval (~50 ms) and
   *  short enough to land the read as the key comes up. */
  const HELD_ARROW_DIFF_MS = 80;

  function cancelPendingDiff() {
    if (pendingDiff !== null) clearTimeout(pendingDiff);
    pendingDiff = null;
  }

  // A key held as the window closes, or as the pane is torn down by a repo with
  // no files — the timer outlives the component otherwise.
  $effect(() => cancelPendingDiff);

  /** ↑/↓ walk the rows and bring each one's diff up as they go, so reviewing a
   *  commit's worth of files is one key rather than a click per file (§5.2).
   *
   *  Bound to the lists rather than the window: the arrows belong to whatever
   *  has focus, and the commit message textarea two elements up is a place
   *  where they have to keep moving the caret. Clicking a row focuses its
   *  button, so the ordinary path — click the first file, then arrow down —
   *  arrives here by bubbling. */
  async function onListKeydown(event: KeyboardEvent) {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    // Modified arrows are left alone rather than claimed and ignored: shift-↓
    // is a range in every file list that has one, and this pane does not have
    // it yet. Taking the key now would make adding it later a regression.
    if (event.shiftKey || event.ctrlKey || event.metaKey || event.altKey) return;

    const target = step(
      selection,
      event.key === 'ArrowDown' ? 1 : -1,
      files?.staged ?? [],
      files?.unstaged ?? [],
    );
    // At either end. Unclaimed, so the pane scrolls the way it would have.
    if (!target) return;
    event.preventDefault();

    selection = selectOne(target.section, target.row.path);
    cancelPendingDiff();
    if (event.repeat) {
      const repoId = repos.selectedId;
      pendingDiff = setTimeout(() => {
        pendingDiff = null;
        // The repo cannot change from the keyboard, but a click elsewhere
        // during a held key can, and this diff would then be of a file in a
        // repo nobody is looking at.
        if (repos.selectedId === repoId) openDiff(target.section, target.row);
      }, HELD_ARROW_DIFF_MS);
    } else {
      openDiff(target.section, target.row);
    }

    // Focus follows the highlight, which is also what scrolls the new row into
    // view — doing it by hand would mean reimplementing "scroll the least
    // possible" against `Pane`'s `.body`. After the flush, because the row may
    // be in the other section's list and that list may have just been created.
    await tick();
    const list = target.section === 'staged' ? stagedList : unstagedList;
    for (const row of list?.querySelectorAll<HTMLElement>('.file-row') ?? []) {
      if (row.dataset.path !== target.row.path) continue;
      row.querySelector<HTMLButtonElement>('button.open')?.focus();
      break;
    }
  }

  /** Right-click acts on the selection when the row is in it, and on that row
   *  alone otherwise — right-clicking outside a selection replaces it, the way
   *  every file list does, so the menu can never act on rows the user cannot
   *  see are picked. */
  function openMenu(section: FileSection, entry: FileEntry, event: MouseEvent) {
    event.preventDefault();
    if (!isSelected(selection, section, entry.path)) selection = selectOne(section, entry.path);
    menu = {
      x: event.clientX,
      y: event.clientY,
      section,
      entries: selectedRows(selection, section, rowsFor(section)),
    };
  }

  function plural(count: number): string {
    return `${count} file${count === 1 ? '' : 's'}`;
  }

  /** Built per section, because the two do not share a verb: `+` and `−` are
   *  opposites, and discard means one plain thing in *Changes* and something
   *  else entirely beside a staged row (§5.2). */
  function menuItems(section: FileSection, entries: FileEntry[]) {
    const paths = entries.map((entry) => entry.path);
    const items = [];

    if (section === 'staged') {
      items.push({ label: `Unstage ${plural(paths.length)}`, onSelect: () => void unstage(paths) });
    } else {
      items.push({ label: `Stage ${plural(paths.length)}`, onSelect: () => void stage(paths) });
      // Untracked rows are dropped rather than blocking the item: git has
      // nothing to restore them from, and it rejects a pathspec list wholesale,
      // so one `?` row would take the whole discard down with it. The label
      // says how many survive when that is fewer than were picked, and the
      // dialog then lists exactly those paths.
      const tracked = entries.filter((entry) => entry.status !== '?');
      if (tracked.length > 0) {
        const label =
          tracked.length === entries.length
            ? `Discard changes to ${plural(tracked.length)}…`
            : `Discard changes to ${plural(tracked.length)} (skipping untracked)…`;
        items.push({ label, onSelect: () => (confirming = tracked) });
      }
    }

    // One row only: `explorer /select,` takes a single path, so N files would
    // mean N windows rather than one window with them all picked out. Hiding it
    // beats offering something that misbehaves at three files.
    if (paths.length === 1) {
      items.push({
        label: 'Reveal in File Explorer',
        onSelect: () => void repos.revealInExplorer(paths[0]),
      });
    }

    items.push(...ignoreItems(section, entries));

    return items;
  }

  /** The ignore entries (§5.2), on **untracked rows only**. A `.gitignore`
   *  line for a tracked file does nothing whatsoever — git keeps tracking what
   *  it already tracks — so the row would sit exactly where it was while the
   *  entry that produced it reported success. That also rules the group out of
   *  *Staged Changes* wholesale: a staged row is in the index by definition.
   *
   *  Behind a separator, because this is where the menu stops being about
   *  these files and starts being about what git sees at all — the one entry
   *  here that outlives the selection, the pane, and the session, since it
   *  ends up committed and then applies to everyone on the repo.
   *
   *  The full file/extension/folder set is offered only for a selection that
   *  *is* one row, not for one that merely has one untracked row left in it.
   *  There is no honest single extension or folder for six files, and a rich
   *  menu built from the survivor of a three-row selection would be describing
   *  rows the user can see are picked and it is not acting on. */
  function ignoreItems(section: FileSection, entries: FileEntry[]) {
    if (section !== 'unstaged') return [];
    const untracked = entries.filter((entry) => entry.status === '?');
    if (untracked.length === 0) return [];

    const items =
      entries.length === 1
        ? ignoreCandidates(untracked[0].path).map((candidate) => ({
            label: `Ignore ${candidate.label}`,
            onSelect: () => void ignore([candidate.pattern]),
          }))
        : [
            {
              // The same "say how many survive" rule Discard follows above,
              // with the filter running the other way round.
              label:
                untracked.length === entries.length
                  ? `Ignore ${plural(untracked.length)}`
                  : `Ignore ${plural(untracked.length)} (skipping tracked)`,
              onSelect: () => void ignore(untracked.map((entry) => exactPattern(entry.path))),
            },
          ];

    return [{ separator: true as const }, ...items];
  }

  async function doCommit() {
    if (!canCommit) return;
    busy = true;
    try {
      if (await repos.commit(message)) message = '';
    } finally {
      busy = false;
    }
  }

  async function doFetch() {
    busy = true;
    try {
      await repos.fetch();
    } finally {
      busy = false;
    }
  }

  async function doPull() {
    busy = true;
    try {
      await repos.pull();
    } finally {
      busy = false;
    }
  }

  async function doPush() {
    busy = true;
    try {
      await (publishable ? repos.publish() : repos.push());
    } finally {
      busy = false;
    }
  }

  async function doCommitAndPush() {
    if (!canCommit) return;
    busy = true;
    try {
      if (await repos.commitAndPush(message)) message = '';
    } finally {
      busy = false;
    }
  }

  async function stage(paths: string[]) {
    if (paths.length === 0) return;
    busy = true;
    try {
      await repos.stagePaths(paths);
    } finally {
      busy = false;
    }
  }

  async function unstage(paths: string[]) {
    if (paths.length === 0) return;
    busy = true;
    try {
      await repos.unstagePaths(paths);
    } finally {
      busy = false;
    }
  }

  async function stageAll() {
    busy = true;
    try {
      await repos.stageAll();
    } finally {
      busy = false;
    }
  }

  async function unstageAll() {
    busy = true;
    try {
      await repos.unstageAll();
    } finally {
      busy = false;
    }
  }

  /** Appends the lines the menu entry named. Unconfirmed, unlike discard: this
   *  destroys nothing — the file stays on disk untouched, it only stops being
   *  listed — and the `.gitignore` edit itself arrives in *Changes* as an
   *  ordinary row the user can read, discard or commit. That row is the
   *  confirmation, after the fact and reversible, which is the right shape for
   *  an act this cheap.
   *
   *  Deliberately not staged afterwards. A write that both edits a file and
   *  puts it in the index would be doing a second thing the menu entry never
   *  said, and `+` is right there on the row it creates. */
  async function ignore(patterns: string[]) {
    if (patterns.length === 0) return;

    busy = true;
    try {
      await repos.ignorePatterns(patterns);
    } finally {
      busy = false;
    }
  }

  /** Runs what the dialog confirmed. The row list refreshes itself off the
   *  write's status publish (§7), so nothing here has to be cleaned up on
   *  either outcome — a failed discard simply leaves the row where it was. */
  async function discard(entries: readonly FileEntry[]) {
    const paths = entries.map((entry) => entry.path);
    if (paths.length === 0) return;

    busy = true;
    try {
      await repos.discardPaths(paths);
    } finally {
      busy = false;
    }
  }
</script>

<Pane title="Changes" class="commit-pane">
  {#snippet actions()}
    <!-- Icon-only and hover-revealed per feedback, rather than a full button
         row, since they act on the selected repo the same way the menu bar's
         Repository ▸ Fetch/Pull do (§4.1). -->
    <button
      type="button"
      class="icon-action"
      disabled={!hasRepo || busy}
      title="Fetch"
      aria-label="Fetch"
      onclick={doFetch}
    >↻</button>
    <button
      type="button"
      class="icon-action"
      disabled={!hasRepo || busy}
      title="Pull"
      aria-label="Pull"
      onclick={doPull}
    >⇩</button>
  {/snippet}

  {#if !hasRepo}
    <EmptyState message="No repository selected" hint="Select a repository to stage and commit changes" />
  {:else}
    <div class="compose">
      <div class="message-field">
        <textarea
          bind:value={message}
          placeholder="Commit message"
          rows="3"
          disabled={busy}
          aria-label="Commit message"
        ></textarea>
        {#if message.length > 0}
          <button
            type="button"
            class="clear-message"
            disabled={busy}
            title="Clear commit message"
            aria-label="Clear commit message"
            onclick={() => (message = '')}
          >
            <Glyph kind="cross" />
          </button>
        {/if}
      </div>

      <div class="buttons">
        <button class="primary" disabled={!canCommit} onclick={doCommit}>Commit</button>
        <button disabled={!canPush} onclick={doPush}>
          {publishable ? 'Publish branch' : 'Push'}
        </button>
      </div>

      <button
        type="button"
        class="primary wide"
        disabled={!canCommit}
        title="Commit, then push in one step"
        onclick={doCommitAndPush}
      >
        Commit + Push
      </button>
    </div>

    {#if repos.loadingFiles && !files}
      <EmptyState message="Reading repository…" />
    {:else if repos.filesError}
      <EmptyState message="Could not read this repository" hint={repos.filesError} />
    {:else if files}
      <div class="section">
        <span class="section-title">Staged Changes</span>
        <span class="section-actions">
          <span class="count">{sectionLabel(files.staged.length, files.stagedTotal)}</span>
          {#if files.staged.length > 0}
            <button type="button" class="link" disabled={busy} onclick={unstageAll}>
              − unstage all
            </button>
          {/if}
        </span>
      </div>
      {#if files.staged.length === 0}
        <p class="section-empty">Nothing staged</p>
      {:else}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <ul bind:this={stagedList} onkeydown={onListKeydown}>
          {#each files.staged as entry (entry.path)}
            <li>
              <!-- The row's own − unstages that row and no other, even with
                   several selected: it is attached to one row and points at one
                   row, and a button that quietly acted on five would be §5.2's
                   tick column back in a worse form. The menu is where a batch
                   is asked for out loud. -->
              <FileRow
                {entry}
                action="unstage"
                disabled={busy}
                onToggle={() => unstage([entry.path])}
                onOpen={(event) => rowClick('staged', entry, event)}
                onContextMenu={(event) => openMenu('staged', entry, event)}
                selected={isSelected(selection, 'staged', entry.path)}
                showingDiff={isOpen('staged', entry)}
              />
            </li>
          {/each}
        </ul>
      {/if}

      <div class="section">
        <span class="section-title">Changes</span>
        <span class="section-actions">
          <span class="count">{sectionLabel(files.unstaged.length, files.unstagedTotal)}</span>
          {#if files.unstaged.length > 0}
            <button
              type="button"
              class="link"
              disabled={busy}
              onclick={stageAll}
              title="Stages every change, including any hidden by the cap above"
            >
              + stage all
            </button>
          {/if}
        </span>
      </div>
      {#if files.unstaged.length === 0}
        <p class="section-empty">No changes</p>
      {:else}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <ul bind:this={unstagedList} onkeydown={onListKeydown}>
          {#each files.unstaged as entry (entry.path)}
            <li>
              <FileRow
                {entry}
                action="stage"
                disabled={busy}
                onToggle={() => stage([entry.path])}
                onOpen={(event) => rowClick('unstaged', entry, event)}
                onContextMenu={(event) => openMenu('unstaged', entry, event)}
                selected={isSelected(selection, 'unstaged', entry.path)}
                showingDiff={isOpen('unstaged', entry)}
                onDiscard={entry.status === '?' ? undefined : () => (confirming = [entry])}
              />
            </li>
          {/each}
        </ul>
      {/if}

      {#if atRest}
        <!-- A sibling of both sections rather than nested in either: he
             reports on the pair of them. The two grey lines above stay — they
             carry the meaning, and the dog is `aria-hidden` decoration
             (Mascot.svelte), so removing them would put semantic weight on an
             image.

             Not `EmptyState`: that is `height: 100%` and centres against the
             whole pane, which would fight the sections above it. -->
        <div class="rest">
          <Mascot pose="content" height={75} />
          <p>Nothing to commit</p>
        </div>
      {/if}
    {/if}
  {/if}
</Pane>

{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    items={menuItems(menu.section, menu.entries)}
    onClose={() => (menu = null)}
  />
{/if}

{#if confirming}
  <DiscardDialog
    entries={confirming}
    onDiscard={() => discard(confirming ?? [])}
    onClose={() => (confirming = null)}
  />
{/if}

<style>
  /* `Pane`'s `.body` is a plain block box, so nothing in this pane knows how
     tall the pane is: a mascot appended after the sections would land directly
     under "No changes" with a void beneath it, which reads as a rendering bug
     rather than a rest state. A flex column gives `.rest` below something to
     claim the leftover space with.

     `.body` is `Pane`'s markup, hence the `class` prop and the `:global()` —
     that prop exists for exactly this. */
  :global(.commit-pane .body) {
    display: flex;
    flex-direction: column;
  }

  /* The cost of the rule above: every child of `.body` is now a flex item, and
     flex items shrink by default. In a scrolling container that lets the file
     lists compress below their content height instead of letting `.body`
     scroll. Every stacking child needs this, and missing one only shows up on
     a repo with enough files to overflow — not the repo you test on.

     `.rest` is deliberately absent: it is the one thing here that *should*
     flex. `EmptyState` is too — it is never a sibling, only ever the sole
     child of its own branch. */
  .compose,
  .section,
  .section-empty,
  ul {
    flex-shrink: 0;
  }

  /* Claims whatever is left below the two sections and sits him at the foot of
     it — `flex-end`, not `center`. Centring floated him in the middle of the
     void on a tall window, which reads as "placed nowhere"; resting on the
     floor of the pane is a position with a reason, and it holds still as the
     file lists above grow.
     No `min-height: 0`: on a short window he should push `.body` into a
     scroll rather than be squashed. */
  .rest {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
    /* Bottom padding is much smaller than the top: the artwork is drawn with
       its own floor shadow, so a generous gap under it reads as him hovering
       rather than as breathing room. `--space-1` is close to the floor of what
       is available — the caption sits below the dog, so this is clearance for
       a line of text, and taking it to zero would crowd its descenders against
       the pane edge. If he needs to sit lower still, the gap to spend is the
       `gap` above, not this. */
    padding: var(--space-4) var(--space-3) var(--space-1);
  }

  /* `content` is the widest, shortest pose (1.57:1), so 75px tall is 118px
     wide against a 240px `MIN_MIDDLE` (App.svelte) — the guard below is now
     slack at every width the pane can reach, but it stays: it costs nothing
     and the size is the kind of number that gets revisited.
     `Mascot.svelte` sets a height with `width: auto` and cannot answer a
     narrow pane on its own, so the guard lives here rather than there —
     keeping it out is what keeps its height-only API, and with it the poses
     looking like one set. */
  .rest :global(img) {
    max-width: 100%;
    height: auto;
  }

  /* Muted rather than `--text-disabled`: it is the caption to the artwork, not
     a third grey line like "No changes" above it. Same size, so it does not
     compete with the pane's own section headings either. */
  .rest p {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--text-muted);
  }

  .compose {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border-bottom: 1px solid var(--border);
  }

  .message-field {
    position: relative;
  }

  textarea {
    width: 100%;
    padding: var(--space-2);
    /* Room for the clear button so it never overlaps typed text. */
    padding-right: calc(var(--space-2) + 18px + var(--space-1));
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    resize: vertical;
  }

  .clear-message {
    position: absolute;
    top: var(--space-1);
    right: var(--space-1);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
  }

  .clear-message:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .clear-message:disabled {
    color: var(--text-disabled);
  }

  textarea::placeholder {
    color: var(--text-disabled);
  }

  textarea:disabled {
    color: var(--text-disabled);
  }

  textarea:focus-visible {
    border-color: var(--accent);
    outline: none;
  }

  .buttons {
    display: flex;
    gap: var(--space-2);
  }

  button {
    flex: 1 1 0;
    height: 28px;
    padding: 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: var(--border-strong);
  }

  /* Tinted at rest rather than a solid accent fill — full brightness read as
     mismatched next to the neutral Push button. Brightens to the solid fill
     on hover, so it still reads as the primary action. */
  button.primary:not(:disabled) {
    color: var(--accent-text);
    background: var(--accent-muted);
    border-color: var(--accent);
  }

  button.primary:hover:not(:disabled) {
    color: var(--accent-text);
    background: var(--accent);
    border-color: var(--accent-hover);
  }

  button.wide {
    /* Overrides the base rule's `flex: 1 1 0` — this button is a direct flex
       item of `.compose` (a column flex container), not of `.buttons`, so
       without this it stretches to fill the leftover vertical space instead
       of staying the same height as Commit/Push. */
    flex: 0 0 auto;
    width: 100%;
  }

  button:disabled {
    color: var(--text-disabled);
    cursor: default;
  }

  /* Header icon actions (Fetch/Pull) — hover-revealed, matching the repo
     list's refresh button (RepoList.svelte's `.refresh`). */
  .icon-action {
    flex: 0 0 auto;
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
  }

  .icon-action:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .icon-action:disabled {
    color: var(--text-disabled);
  }

  .section {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3) var(--space-1);
  }

  .section-title {
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .section-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .count {
    font-size: var(--text-xs);
    color: var(--text-disabled);
    font-variant-numeric: tabular-nums;
  }

  .link {
    flex: 0 0 auto;
    height: auto;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }

  .link:hover:not(:disabled) {
    color: var(--text-primary);
    background: none;
    border-color: transparent;
  }

  .section-empty {
    margin: 0;
    padding: 0 var(--space-3) var(--space-2);
    font-size: var(--text-sm);
    color: var(--text-disabled);
  }

  ul {
    margin: 0;
    padding: 0 0 var(--space-2);
    list-style: none;
  }
</style>
