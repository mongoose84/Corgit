<script lang="ts">
  interface Props {
    message: string;
    hint?: string;
  }

  let { message, hint }: Props = $props();
</script>

<div class="empty">
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
