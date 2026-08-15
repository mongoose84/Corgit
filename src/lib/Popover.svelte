<script lang="ts">
  // Generic anchored popover — `ContextMenu.svelte`'s viewport-clamping
  // positioning logic, factored out for a caller that needs arbitrary
  // content (e.g. a row's error detail) rather than a menu-item list.
  import type { Snippet } from 'svelte';

  interface Props {
    /** Viewport coordinates of the click that opened this popover. */
    x: number;
    y: number;
    children: Snippet;
    onClose: () => void;
  }

  let { x, y, children, onClose }: Props = $props();

  let popoverEl: HTMLElement | undefined = $state();
  // Painted at the click position first, then nudged on-screen once the
  // popover's real width/height can be measured (same reasoning as
  // ContextMenu's identical effect).
  $effect(() => {
    if (!popoverEl) return;
    const { innerWidth, innerHeight } = window;
    const rect = popoverEl.getBoundingClientRect();
    popoverEl.style.left = `${Math.max(0, Math.min(x, innerWidth - rect.width))}px`;
    popoverEl.style.top = `${Math.max(0, Math.min(y, innerHeight - rect.height))}px`;
  });

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') onClose();
  }
</script>

<svelte:window onmousedown={onClose} onkeydown={onKeydown} onblur={onClose} />

<div
  class="popover"
  role="dialog"
  tabindex="-1"
  bind:this={popoverEl}
  style="left: {x}px; top: {y}px"
  onmousedown={(event) => event.stopPropagation()}
>
  {@render children()}
</div>

<style>
  .popover {
    position: fixed;
    z-index: 100;
    width: 320px;
    padding: var(--space-2) var(--space-3);
    background: var(--bg-raised);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
  }
</style>
