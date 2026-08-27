<script lang="ts">
  // Confirmation for the two acts in the pane that destroy work (§5.2).
  // Modelled on CreateBranchDialog — a centred modal rather than an anchored
  // popover, for a stronger reason here: these are the only things in Corgit
  // that destroy work no git command can bring back, so it must not be
  // dismissible by the same stray outside click that closes a menu. Hence no
  // scrim-click close either; Cancel and Escape only.
  //
  // §8.3 refuses force-checkout because it "silently discards work". This is
  // the same act done *loudly*: every path is listed, and the sentence under
  // the list says what happens to those exact bytes. A count alone would not
  // be enough to check the list against.
  //
  // It serves both destructive acts in the pane, because the *modal* is the
  // safety property — Cancel focused, no scrim close, every path listed — and
  // two copies of that would be two places for it to rot. Only the words
  // change with `mode`, and they change completely: "discard" and "delete" must
  // never be able to read as each other.
  import type { FileEntry } from './repos.svelte';

  /** `discard` restores tracked files from the index and git can bring the
   *  result back. `delete` is `git clean` on untracked files and nothing can —
   *  they have never been in the index, so no object git holds has a copy. */
  export type DestructiveMode = 'discard' | 'delete';

  interface Props {
    mode: DestructiveMode;
    /** Exactly what will be acted on, captured when the dialog was opened —
     *  never re-derived while it is up. A sweep, an FS watcher or a terminal
     *  can change the file list underneath a modal, and confirming must act on
     *  what the user was shown, not on what the list says by then. */
    entries: readonly FileEntry[];
    /** Resolves once the write has settled; the dialog stays up until then so
     *  the button can say what is happening. Failures are reported by the
     *  pane, alongside every other git write's (§13). */
    onConfirm: () => Promise<void>;
    onClose: () => void;
  }

  let { mode, entries, onConfirm, onClose }: Props = $props();

  let busy = $state(false);
  let cancelEl: HTMLButtonElement | undefined = $state();

  // Cancel takes focus, not the confirm button: Enter and Space are how a
  // focused button is pressed, and a dialog that destroys files must not have
  // its destructive half one reflex keystroke away.
  $effect(() => {
    cancelEl?.focus();
  });

  const one = $derived(entries.length === 1);

  const title = $derived(
    mode === 'discard'
      ? one
        ? 'Discard changes to this file?'
        : `Discard changes to ${entries.length} files?`
      : one
        ? 'Delete this file?'
        : `Delete ${entries.length} files?`,
  );

  // The sentence the user actually reads before pressing the red button, so it
  // says what happens to the bytes rather than naming the git command. The
  // delete wording leads with the irreversibility because that is the whole
  // difference between the two modes: everywhere else in Corgit, "cannot be
  // recovered" still means git has it and the UI does not.
  const consequence = $derived(
    mode === 'discard'
      ? 'Their unstaged changes are thrown away and cannot be recovered. Anything already staged for these files is kept.'
      : one
        ? 'This file is deleted from disk. It has never been committed, so git has no copy and nothing can bring it back.'
        : 'These files are deleted from disk. They have never been committed, so git has no copy and nothing can bring them back.',
  );

  const action = $derived(mode === 'discard' ? 'Discard changes' : 'Delete files');
  const pending = $derived(mode === 'discard' ? 'Discarding…' : 'Deleting…');

  async function confirm() {
    if (busy) return;
    busy = true;
    try {
      await onConfirm();
      onClose();
    } finally {
      busy = false;
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.stopPropagation();
      if (!busy) onClose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" role="presentation">
  <div
    class="dialog"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-label={action}
    onkeydown={onKeydown}
  >
    <p class="title">{title}</p>

    <ul class="files">
      {#each entries as entry (entry.path)}
        <li>
          <span class="status">{entry.status}</span>
          <span class="path selectable" title={entry.path}>{entry.path}</span>
        </li>
      {/each}
    </ul>

    <p class="consequence">{consequence}</p>

    <div class="buttons">
      <button bind:this={cancelEl} type="button" disabled={busy} onclick={onClose}>Cancel</button>
      <button type="button" class="danger" disabled={busy} onclick={confirm}>
        {busy ? pending : action}
      </button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 200;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.45);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: 360px;
    max-width: calc(100vw - var(--space-5));
    padding: var(--space-3);
    background: var(--bg-raised);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .title {
    margin: 0 0 var(--space-2);
    font-size: var(--text-md);
    font-weight: 600;
    color: var(--text-primary);
  }

  /* Scrolls rather than growing: the *Changes* list caps at 100 entries (§5.2)
     and every one of them can be selected at once, which is taller than any
     screen. The count in the title stays the authoritative number either way. */
  .files {
    max-height: 180px;
    margin: 0 0 var(--space-2);
    padding: var(--space-1) 0;
    overflow-y: auto;
    list-style: none;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .files li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 2px var(--space-2);
  }

  .status {
    flex: 0 0 auto;
    width: 12px;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    text-align: center;
    color: var(--text-muted);
  }

  /* Tail-ellipsized, unlike the file rows' head-first trim: here the list is
     the thing being checked before something irreversible, and two files in
     the same directory must not both read as "…/config.ts". */
  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .consequence {
    margin: 0 0 var(--space-3);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  /* Same treatment as the compose pane's Commit/Push pair (§11), with --danger
     standing in for --accent: the accent means selection and primary action,
     and this is neither. */
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

  .buttons button.danger:not(:disabled) {
    color: var(--danger-hover);
    background: var(--danger-muted);
    border-color: var(--danger);
  }

  .buttons button.danger:hover:not(:disabled) {
    color: var(--danger-text);
    background: var(--danger);
    border-color: var(--danger-hover);
  }

  .buttons button:disabled {
    color: var(--text-disabled);
    cursor: default;
  }
</style>
