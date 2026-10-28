<script lang="ts">
  /**
   * The middle pane's two file lists (§5.2) — *Staged Changes* and *Changes*,
   * and everything that acts on their rows: the ctrl/shift-click selection,
   * ↑/↓ with its held-key coalesce, the right-click menu, and the
   * discard/delete confirmation.
   *
   * Cut out of CommitPane for the reason BranchFind was cut out of GraphPane:
   * the pane had two concerns sharing one `<script>`, and they met in
   * exactly one place. The compose box above never reads the selection, the
   * menu or the keyboard; these lists never read the message. What they share
   * is `busy` — one write at a time from this pane, whichever half started
   * it — so that is the one bindable, and everything else stays on its side.
   *
   * Mounted only while there is a file list to draw. The menu and the dialog
   * live here too, so they go when the list does: a repo whose files can no
   * longer be read has no rows for a confirmation to be about, and the dialog
   * would otherwise still be offering to discard the rows it captured.
   */
  import { tick } from 'svelte';

  import FileRow from './FileRow.svelte';
  import DiscardDialog, { type DestructiveMode } from '../DiscardDialog.svelte';
  import ContextMenu from '../ContextMenu.svelte';
  import { repos, type FileChanges, type FileEntry } from '../repos.svelte';
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

  interface Props {
    /** The pane's narrowed `repos.files`: it draws the loading and error
     *  states itself and only mounts this once there is a list. */
    files: FileChanges;
    /** Bindable, because the pane's Commit and Push and these rows' + and −
     *  must not run over each other — either half sets it, both halves are
     *  disabled by it. */
    busy: boolean;
  }

  let { files, busy = $bindable() }: Props = $props();

  function sectionLabel(shown: number, total: number): string {
    return shown === total ? `${total}` : `${shown} of ${total}`;
  }

  // The two acts in the pane that destroy work (§5.2), both *Changes* only,
  // both confirmed every time, and deliberately never merged into one entry —
  // they are not two labels for one thing and the row's status letter decides
  // which applies:
  //
  // - **Discard**, on tracked rows: `git restore --worktree`, so a partly
  //   staged file keeps its staged half. A staged row's Discard could only mean
  //   "throw away the staged work as well", which is not what a button sitting
  //   beside − reads as; unstage first and the row appears here, where
  //   discarding it means one plain thing.
  // - **Delete**, on untracked (`?`) rows: `git clean`. Git has nothing to
  //   restore an untracked file *from*, so the only honest thing the pane can
  //   offer is removing it — and the entry says "Delete" rather than "Discard"
  //   for exactly that reason. It is the one act here git cannot undo at all.
  //
  // A selection spanning both gets both entries, each scoped to the rows it
  // applies to, rather than one entry quietly doing two different irreversible
  // things to different halves of the list.
  //
  // Still no tick column — that is what made *Changes* read as a form to be
  // filled in rather than a list of what changed. A batch is built by
  // ctrl/shift-clicking rows instead (§5.2), which costs the list nothing when
  // nobody is using it, and reaches both through the same dialog.
  /** Non-null while the confirmation is up; the value is the exact list the
   *  dialog is showing, and the act it is about to perform on it. */
  let confirming = $state<{ mode: DestructiveMode; entries: FileEntry[] } | null>(null);

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
    return section === 'staged' ? files.staged : files.unstaged;
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

  // A key held as the window closes, or as the list is torn down by a repo
  // whose files cannot be read — the timer outlives the component otherwise.
  $effect(() => cancelPendingDiff);

  /** ↑/↓ walk the rows and bring each one's diff up as they go, so reviewing a
   *  commit's worth of files is one key rather than a click per file (§5.2).
   *
   *  Bound to the lists rather than the window: the arrows belong to whatever
   *  has focus, and the commit message textarea just above is a place where
   *  they have to keep moving the caret. Clicking a row focuses its button, so
   *  the ordinary path — click the first file, then arrow down — arrives here
   *  by bubbling. */
  async function onListKeydown(event: KeyboardEvent) {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    // Modified arrows are left alone rather than claimed and ignored: shift-↓
    // is a range in every file list that has one, and this pane does not have
    // it yet. Taking the key now would make adding it later a regression.
    if (event.shiftKey || event.ctrlKey || event.metaKey || event.altKey) return;

    const target = step(selection, event.key === 'ArrowDown' ? 1 : -1, files.staged, files.unstaged);
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
        items.push({ label, onSelect: () => (confirming = { mode: 'discard', entries: tracked }) });
      }

      // The other half of the same selection, and a separate entry rather than
      // a broader Discard: this one removes the file from disk with nothing in
      // git able to bring it back, so it must not share a word with the act
      // above that git can undo. "(skipping tracked)" is never needed here —
      // whatever it skipped, the Discard entry directly above is offering.
      const untracked = entries.filter((entry) => entry.status === '?');
      if (untracked.length > 0) {
        items.push({
          label: `Delete ${plural(untracked.length)}…`,
          onSelect: () => (confirming = { mode: 'delete', entries: untracked }),
        });
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

  /** Runs the delete the dialog confirmed. `remove`, not `delete` — the latter
   *  is a reserved word — and the same shape as `discard` above right down to
   *  needing no cleanup on failure: the rows come back from the write's status
   *  publish (§7), so a file that could not be removed simply stays listed. */
  async function remove(entries: readonly FileEntry[]) {
    const paths = entries.map((entry) => entry.path);
    if (paths.length === 0) return;

    busy = true;
    try {
      await repos.deletePaths(paths);
    } finally {
      busy = false;
    }
  }
</script>

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
          onDiscard={entry.status === '?'
            ? undefined
            : () => (confirming = { mode: 'discard', entries: [entry] })}
        />
      </li>
    {/each}
  </ul>
{/if}
<!-- No rest-state artwork here: the clean-repo mascot (docs/mascot-clean-pane.md)
     was removed from this pane deliberately. The two grey lines above are what
     report the state — they always carried the meaning, and the dog was
     `aria-hidden` decoration, so nothing legible went with it. -->

{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    items={menuItems(menu.section, menu.entries)}
    onClose={() => (menu = null)}
  />
{/if}

{#if confirming}
  <!-- `confirming` is read once into a local so the confirm handler cannot see
       a different value than the list on screen: the dialog stays up across the
       await, and a second menu action landing in between would otherwise send
       the write at rows the user never looked at. -->
  {@const pending = confirming}
  <DiscardDialog
    mode={pending.mode}
    entries={pending.entries}
    onConfirm={() => (pending.mode === 'discard' ? discard(pending.entries) : remove(pending.entries))}
    onClose={() => (confirming = null)}
  />
{/if}

<style>
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

  /* Written out in full rather than inherited: these used to sit under
     CommitPane's base `button` rule and override most of it, and what they
     silently kept from it — the pointer, the disabled colour — has to be said
     here now that the scope boundary is between them. */
  .link {
    flex: 0 0 auto;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .link:hover:not(:disabled) {
    color: var(--text-primary);
  }

  .link:disabled {
    color: var(--text-disabled);
    cursor: default;
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
