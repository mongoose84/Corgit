<script lang="ts">
  // Discard confirmation (§5.2). Modelled on CreateBranchDialog — a centred
  // modal rather than an anchored popover, for a stronger reason here: this is
  // the only thing in Corgit that destroys work no git command can bring back,
  // so it must not be dismissible by the same stray outside click that closes
  // a menu. Hence no scrim-click close either; Cancel and Escape only.
  //
  // §8.3 refuses force-checkout because it "silently discards work". This is
  // the same act done *loudly*: every path is listed, and the sentence under
  // the list says exactly which half of the file's changes goes and which
  // stays. A count alone would not be enough to check the list against.
  import type { FileEntry } from './repos.svelte';

  interface Props {
    /** Exactly what will be discarded, captured when the dialog was opened —
     *  never re-derived while it is up. A sweep, an FS watcher or a terminal
     *  can change the file list underneath a modal, and confirming must
     *  discard what the user was shown, not what the list says by then. */
    entries: readonly FileEntry[];
    /** Resolves once the write has settled; the dialog stays up until then so
     *  the button can say what is happening. Failures are reported by the
     *  pane, alongside every other git write's (§13). */
    onDiscard: () => Promise<void>;
    onClose: () => void;
  }

  let { entries, onDiscard, onClose }: Props = $props();

  let busy = $state(false);
  let cancelEl: HTMLButtonElement | undefined = $state();

  // Cancel takes focus, not the confirm button: Enter and Space are how a
  // focused button is pressed, and a dialog that destroys files must not have
  // its destructive half one reflex keystroke away.
  $effect(() => {
    cancelEl?.focus();
  });

  const title = $derived(
    entries.length === 1 ? 'Discard changes to this file?' : `Discard changes to ${entries.length} files?`,
  );

  async function confirm() {
    if (busy) return;
    busy = true;
    try {
      await onDiscard();
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
    aria-label="Discard changes"
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

    <p class="consequence">
      Their unstaged changes are thrown away and cannot be recovered. Anything already staged for
      these files is kept.
    </p>

    <div class="buttons">
      <button bind:this={cancelEl} type="button" disabled={busy} onclick={onClose}>Cancel</button>
      <button type="button" class="danger" disabled={busy} onclick={confirm}>
        {busy ? 'Discarding…' : 'Discard changes'}
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
