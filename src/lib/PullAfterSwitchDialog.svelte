<script lang="ts">
  // "You switched to a branch that is behind — pull it?" (SPEC.md §8.3, §8.7).
  //
  // The one prompt in Corgit that is *offered* rather than demanded. Git Graph
  // asks the same question for the same reason: a checkout is nearly always
  // the start of working on that branch, and the branch you have just landed
  // on being N commits stale is knowable at that exact moment and forgettable
  // one second later. The alternative Corgit already had — switch, notice the
  // ↓ badge, cross the window to Pull — is the trip the dashboard exists to
  // remove (§5.1).
  //
  // Modelled on CreateBranchDialog rather than DiscardDialog, and the
  // differences are all the same difference: this destroys nothing, so the
  // scrim closes it, *Pull* takes focus, and the affirmative half is the
  // accent button. Escape and *Not now* mean "leave it behind" — a real
  // answer, not a dismissal, which is why the prompt never comes back on its
  // own: the switch that raised it is over.
  //
  // Nothing here is suppressible (§13's checkbox). That rule is about
  // silencing *failures*; this is an offer, and one made only in response to a
  // gesture the user just made.
  interface Props {
    /** The branch now checked out — the one the pull would move. Not the badge
     *  that was double-clicked: switching to a remote-tracking badge creates a
     *  local branch of a different name (`origin/x` → `x`), and the sentence
     *  must name what HEAD is actually on. */
    branch: string;
    /** Its upstream, short form (`origin/main`) — the other end of the count,
     *  and worth naming: a branch tracking something other than the remote of
     *  the same name is exactly when "3 behind" is surprising. */
    upstream: string;
    behind: number;
    /** Resolves once the pull has settled; the dialog stays up until then so
     *  the button can say what is happening, and a failure surfaces in the
     *  banner like every other git write (§13). */
    onPull: () => Promise<void>;
    onClose: () => void;
  }

  let { branch, upstream, behind, onPull, onClose }: Props = $props();

  let busy = $state(false);
  let pullEl: HTMLButtonElement | undefined = $state();

  // The affirmative button takes focus, the reverse of DiscardDialog — there
  // the reflex Enter had to be harmless, here it is the answer most people
  // want, and a pull can be undone by anything git offers for a merge.
  $effect(() => {
    pullEl?.focus();
  });

  const commits = $derived(behind === 1 ? '1 commit' : `${behind} commits`);

  async function confirm() {
    if (busy) return;
    busy = true;
    try {
      await onPull();
      onClose();
    } finally {
      busy = false;
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      // Stopped rather than left to bubble, like every other dialog here:
      // GraphPane's window-level Escape would otherwise also close the info
      // column behind the scrim.
      event.stopPropagation();
      if (!busy) onClose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="scrim" role="presentation" onmousedown={() => !busy && onClose()}>
  <div
    class="dialog"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-label="Pull after switching branch"
    onmousedown={(event) => event.stopPropagation()}
    onkeydown={onKeydown}
  >
    <p class="title">Pull {branch}?</p>

    <p class="detail">
      It is {commits} behind <span class="ref">{upstream}</span>. Pulling merges them in now
      (<span class="ref">git pull --no-rebase</span>); the count comes from the last fetch, so
      there may be more on the server.
    </p>

    <div class="buttons">
      <button type="button" disabled={busy} onclick={onClose}>Not now</button>
      <button bind:this={pullEl} type="button" class="primary" disabled={busy} onclick={confirm}>
        {busy ? 'Pulling…' : 'Pull'}
      </button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    /* Above ContextMenu's 100 — the menu whose *Switch to …* started this may
       still be unwinding its close when the dialog first paints. */
    z-index: 200;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.45);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: 340px;
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

  .detail {
    margin: 0 0 var(--space-3);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .ref {
    font-family: var(--font-mono);
    color: var(--text-secondary);
  }

  .buttons {
    display: flex;
    gap: var(--space-2);
  }

  /* Same treatment as CreateBranchDialog's pair, which is the compose pane's
     (§11). */
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
