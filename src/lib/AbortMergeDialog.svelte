<script lang="ts">
  // Abort-merge confirmation (SPEC.md §13).
  //
  // §13 reports failures in a banner and never in a modal — but the *recovery*
  // is a different act from the report, and this one throws work away: any
  // conflict already resolved in the working tree goes back to where the merge
  // started. That is the same category as `DiscardDialog`, so it gets the same
  // treatment, down to the reasons: no scrim-click close, Escape and Cancel
  // only, and Cancel takes focus so the destructive half is never one reflex
  // keystroke away.
  interface Props {
    repoName: string;
    /** Resolves once the abort has settled; the dialog stays up until then so
     *  the button can say what is happening. A failure is reported by the
     *  banner, like every other git write (§13). */
    onAbort: () => Promise<void>;
    onClose: () => void;
  }

  let { repoName, onAbort, onClose }: Props = $props();

  let busy = $state(false);
  let cancelEl: HTMLButtonElement | undefined = $state();

  $effect(() => {
    cancelEl?.focus();
  });

  async function confirm() {
    if (busy) return;
    busy = true;
    try {
      await onAbort();
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
    aria-label="Abort merge"
    onkeydown={onKeydown}
  >
    <p class="title">Abort the merge in {repoName}?</p>

    <p class="consequence">
      The working tree goes back to where the merge started. Any conflicts you have already
      resolved are thrown away; commits on either branch are untouched.
    </p>

    <div class="buttons">
      <button bind:this={cancelEl} type="button" disabled={busy} onclick={onClose}>Cancel</button>
      <button type="button" class="danger" disabled={busy} onclick={confirm}>
        {busy ? 'Aborting…' : 'Abort merge'}
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

  .consequence {
    margin: 0 0 var(--space-3);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

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
