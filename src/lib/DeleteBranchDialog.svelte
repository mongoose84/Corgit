<script lang="ts">
  // Delete branch (§8.3) — opened from a local ref badge's right-click menu in
  // the graph. Centred modal with no scrim-click close, following
  // DiscardDialog rather than CreateBranchDialog: this is the second thing in
  // Corgit that can destroy work, so the stray outside click that dismisses a
  // menu must not also dismiss the question.
  //
  // The dialog is deliberately two-stepped, and the two steps are the same
  // dialog rather than two: the first press runs the safe `git branch -d`, and
  // *Delete anyway* only exists once git has come back refusing. That means
  // the destructive button is never the one on screen when the dialog opens,
  // and the words above it are git's own reason rather than a warning Corgit
  // wrote in advance and might have been wrong about.

  interface Props {
    /** The local branch being deleted, exactly as its badge names it. */
    name: string;
    /** Git's "not fully merged" refusal, once one has come back — the switch
     *  into the second step. Held by the pane, not here, because the write it
     *  came from is the pane's (`busy`, `actionError`) like every other. */
    refusal: string | null;
    /** True while a delete is in flight; the dialog stays up throughout so the
     *  button can say what is happening. */
    busy: boolean;
    /** Runs the delete. The pane closes this dialog on success and on any
     *  failure that is not the unmerged refusal — those surface as an ordinary
     *  write error (§13), which is what every other graph write does. */
    onDelete: (force: boolean) => void;
    onClose: () => void;
  }

  let { name, refusal, busy, onDelete, onClose }: Props = $props();

  let cancelEl: HTMLButtonElement | undefined = $state();

  // Cancel holds focus, and takes it again when the second step appears: Enter
  // and Space press whatever is focused, and *Delete anyway* must not inherit
  // the focus ring from the button that was standing in that spot a moment
  // earlier. Reading `refusal` is what re-runs this on the step change.
  $effect(() => {
    refusal;
    cancelEl?.focus();
  });

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
    aria-label="Delete branch"
    onkeydown={onKeydown}
  >
    <p class="title">Delete branch?</p>
    <p class="branch selectable">{name}</p>

    {#if refusal === null}
      <p class="consequence">
        The local branch only — a remote branch of the same name is left alone. Git refuses if it
        holds commits that are not merged anywhere else.
      </p>
    {:else}
      <!-- Git's own words, not a translation: the sentence names the branch and
           says precisely what is unmerged, and this is the moment the user is
           deciding whether that matters. -->
      <pre class="refusal selectable">{refusal}</pre>
      <p class="consequence">
        Deleting anyway drops those commits from every branch. They stay in the reflog for a
        while, but nothing points at them.
      </p>
    {/if}

    <div class="buttons">
      <button bind:this={cancelEl} type="button" disabled={busy} onclick={onClose}>Cancel</button>
      {#if refusal === null}
        <button type="button" class="danger" disabled={busy} onclick={() => onDelete(false)}>
          {busy ? 'Deleting…' : 'Delete'}
        </button>
      {:else}
        <button type="button" class="danger" disabled={busy} onclick={() => onDelete(true)}>
          {busy ? 'Deleting…' : 'Delete anyway'}
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    /* Above ContextMenu's 100 — the menu that opened this is still unwinding
       its own close as the dialog first paints. */
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
    margin: 0;
    font-size: var(--text-md);
    font-weight: 600;
    color: var(--text-primary);
  }

  /* Monospace and on its own line for the same reason the discard dialog
     lists paths: the name is the thing being checked before something
     irreversible, so it must not read as prose. */
  .branch {
    margin: var(--space-1) 0 var(--space-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .refusal {
    max-height: 120px;
    margin: 0 0 var(--space-2);
    padding: var(--space-2);
    overflow: auto;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    white-space: pre-wrap;
    color: var(--text-secondary);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .consequence {
    margin: 0 0 var(--space-3);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  /* Same pairing as DiscardDialog: --danger where a primary action would use
     --accent, since the accent means selection and primary action (§11) and
     this is neither. */
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
