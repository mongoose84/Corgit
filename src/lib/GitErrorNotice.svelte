<script lang="ts">
  // Shared error surface (SPEC.md §13): a plain-language headline, an
  // action button when one applies, and raw stderr behind a collapsible
  // "Details" toggle — "raw stderr always available" without dumping it in
  // the user's face by default.
  import Mascot from './Mascot.svelte';
  import { translateGitError, type GitErrorAction } from './gitErrors';

  interface Props {
    error: string;
    onPull?: () => void;
    onOpenVSCode?: () => void;
    onRetry?: () => void;
    onDismiss?: () => void;
    /** Overrides the action `translateGitError` would otherwise suggest —
     *  for callers that already know the right action from context the raw
     *  text alone doesn't carry (e.g. §8.3's dirty-tree checkout failure,
     *  which stays untranslated per §13 but still offers Open in VS Code
     *  when the tree was dirty at the moment of failure). */
    forceAction?: GitErrorAction;
  }

  let { error, onPull, onOpenVSCode, onRetry, onDismiss, forceAction }: Props = $props();

  const translated = $derived(translateGitError(error));
  const action = $derived(forceAction !== undefined ? forceAction : translated.action);
  const hasDetails = $derived(translated.raw !== translated.message);
  let detailsOpen = $state(false);
</script>

<div class="notice">
  <div class="row">
    <!-- Sorry, not alarmed — softening git's worst moments is this pose's
         whole job (docs/mascot.md §5). Kept to 20px because the narrowest
         place this notice appears is the 240px commit pane, where every pixel
         he takes is one the message wraps out of. -->
    <Mascot pose="mini-sorry" height={20} />
    <p class="message selectable">{translated.message}</p>
    <div class="actions">
      {#if action === 'pull' && onPull}
        <button type="button" onclick={onPull}>Pull</button>
      {:else if action === 'open-vscode' && onOpenVSCode}
        <button type="button" onclick={onOpenVSCode}>Open in VS Code</button>
      {:else if action === 'retry' && onRetry}
        <button type="button" onclick={onRetry}>Retry</button>
      {/if}
      {#if onDismiss}
        <button type="button" onclick={onDismiss}>Dismiss</button>
      {/if}
    </div>
  </div>

  {#if hasDetails}
    <button type="button" class="details-toggle" onclick={() => (detailsOpen = !detailsOpen)}>
      {detailsOpen ? 'Hide details' : 'Show details'}
    </button>
    {#if detailsOpen}
      <pre class="raw selectable">{translated.raw}</pre>
    {/if}
  {/if}
</div>

<style>
  .notice {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .message {
    /* Takes the slack, so the dog stays tucked against the left edge instead
       of being pushed apart from the text by `space-between`. */
    flex: 1 1 auto;
    margin: 0;
    min-width: 0;
    overflow: hidden;
    font-size: var(--text-sm);
    color: var(--status-error);
  }

  .actions {
    display: flex;
    flex: 0 0 auto;
    gap: var(--space-2);
  }

  .actions button {
    height: 22px;
    padding: 0 var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-primary);
    background: var(--bg-hover);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
  }

  .actions button:hover {
    background: var(--bg-active);
  }

  .details-toggle {
    align-self: flex-start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }

  .details-toggle:hover {
    color: var(--text-primary);
  }

  .raw {
    margin: 0;
    padding: var(--space-2);
    max-height: 160px;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text-muted);
    background: var(--bg-app);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }
</style>
