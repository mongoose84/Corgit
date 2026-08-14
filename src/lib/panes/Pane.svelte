<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    title: string;
    children: Snippet;
    actions?: Snippet;
    /** Extra class for the root element — a styling hook for a caller-owned
     *  `:global()` rule, since scoped CSS in the caller can't otherwise reach
     *  into a child component's own template. */
    class?: string;
  }

  let { title, children, actions, class: className = '' }: Props = $props();
</script>

<section class="pane {className}">
  <header>
    <h2>{title}</h2>
    {#if actions}
      <div class="actions">{@render actions()}</div>
    {/if}
  </header>
  <div class="body">
    {@render children()}
  </div>
</section>

<style>
  .pane {
    display: flex;
    min-width: 0;
    flex-direction: column;
    /* Elevation comes from surface lightness, not shadows — shadows read
       poorly on dark (SPEC.md §11). */
    background: var(--bg-surface);
    height: 100%;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    height: var(--header-height);
    padding: 0 var(--space-3);
    border-bottom: 1px solid var(--border);
    flex: 0 0 auto;
  }

  h2 {
    margin: 0;
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 0 0 auto;
  }

  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
  }
</style>
