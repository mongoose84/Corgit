<script lang="ts">
  import Pane from './Pane.svelte';
  import FileRow from './FileRow.svelte';
  import EmptyState from '../EmptyState.svelte';
  import GitErrorNotice from '../GitErrorNotice.svelte';
  import Glyph from '../Glyph.svelte';
  import DiscardDialog from '../DiscardDialog.svelte';
  import ContextMenu from '../ContextMenu.svelte';
  import { needsPublish, repos, type FileEntry } from '../repos.svelte';
  import { diff, type DiffSource } from '../diff.svelte';
  import {
    extend,
    isSelected,
    prune,
    selectOne,
    selectedRows,
    toggle,
    type FileSection,
    type FileSelection,
  } from '../fileSelection';

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
  const conflicted = $derived(status !== undefined && status.conflicted > 0);

  const canCommit = $derived(
    hasRepo && !busy && !conflicted && message.trim().length > 0 && (files?.stagedTotal ?? 0) > 0,
  );
  const canPush = $derived(hasRepo && !busy && !conflicted);

  function sectionLabel(shown: number, total: number): string {
    return shown === total ? `${total}` : `${shown} of ${total}`;
  }

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

  /** Which two sides the right pane compares (§5.4). It has to come from the
   *  section rather than from the entry: the same path sits in both lists
   *  whenever a file is partly staged, with a different diff on each side.
   *  An untracked file has no other side at all, so it gets its own source
   *  rather than a `git diff` that would correctly report nothing. */
  function sourceFor(section: 'staged' | 'unstaged', entry: FileEntry): DiffSource {
    if (section === 'staged') return { kind: 'staged' };
    return entry.status === '?' ? { kind: 'untracked' } : { kind: 'unstaged' };
  }

  function openDiff(section: FileSection, entry: FileEntry) {
    const id = repos.selectedId;
    if (!id) return;
    diff.show(id, entry.path, sourceFor(section, entry));
  }

  function isOpen(section: FileSection, entry: FileEntry): boolean {
    const id = repos.selectedId;
    return id !== undefined && diff.isOpen(id, entry.path, sourceFor(section, entry));
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
    openDiff(section, entry);
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

    return items;
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

  async function doMergeAbort() {
    busy = true;
    try {
      await repos.mergeAbort();
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

<Pane title="Changes">
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
    {#if conflicted}
      <!-- §13: exactly two buttons, never a third — never force-anything. -->
      <div class="conflict-banner">
        <p class="selectable">This repository has a merge conflict. Commit and push are blocked until it's resolved or aborted.</p>
        <div class="conflict-actions">
          <button type="button" disabled={busy} onclick={doMergeAbort}>Abort merge</button>
          <button type="button" disabled={busy} onclick={() => repos.openInVSCode()}>Open in VS Code</button>
        </div>
      </div>
    {/if}

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

      {#if repos.writeError}
        <GitErrorNotice
          error={repos.writeError}
          onPull={doPull}
          onOpenVSCode={() => repos.openInVSCode()}
          onDismiss={() => (repos.writeError = null)}
        />
      {/if}
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
        <ul>
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
        <ul>
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
  .conflict-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
    background: var(--bg-raised);
  }

  .conflict-banner p {
    margin: 0;
    min-width: 0;
    font-size: var(--text-sm);
    color: var(--status-conflict);
  }

  .conflict-actions {
    display: flex;
    flex: 0 0 auto;
    gap: var(--space-2);
  }

  .conflict-actions button {
    height: 22px;
    padding: 0 var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-primary);
    background: var(--bg-hover);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
  }

  .conflict-actions button:hover:not(:disabled) {
    background: var(--bg-active);
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
