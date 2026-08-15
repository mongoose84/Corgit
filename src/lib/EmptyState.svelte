<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    message: string;
    hint?: string;
    /** Optional decoration above the copy. Reserved for the *resting* empty
     *  states — never the loading or error ones, where art would compete with
     *  the thing the user actually needs to read. */
    art?: Snippet;
  }

  let { message, hint, art }: Props = $props();
</script>

<div class="empty">
  {#if art}
    <div class="art">{@render art()}</div>
  {/if}
  <p class="message">{message}</p>
  {#if hint}
    <p class="hint">{hint}</p>
  {/if}
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    height: 100%;
    padding: var(--space-5) var(--space-4);
    text-align: center;
  }

  .art {
    margin-bottom: var(--space-3);
  }

  .message {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-md);
  }

  .hint {
    margin: 0;
    color: var(--text-disabled);
    font-size: var(--text-sm);
  }
</style>
