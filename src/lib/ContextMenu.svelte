<script lang="ts">
  // Generic right-click menu (§9 "context menus", pulled forward for branch
  // switching in §8.3 — the first caller, not the last: any future row action
  // reuses this rather than growing its own popup).

  interface ContextMenuItem {
    label: string;
    onSelect: () => void;
  }

  interface Props {
    /** Viewport coordinates of the click that opened this menu. */
    x: number;
    y: number;
    items: ContextMenuItem[];
    onClose: () => void;
  }

  let { x, y, items, onClose }: Props = $props();

  let menuEl: HTMLElement | undefined = $state();
  // Painted at the click position first, then nudged on-screen once the
  // menu's real width/height can be measured — there is no way to know
  // either before the browser has laid it out once. Written straight to the
  // element rather than through `$state`, since this is a one-shot
  // adjustment per open, not something that should re-run reactively.
  $effect(() => {
    if (!menuEl) return;
    const { innerWidth, innerHeight } = window;
    const rect = menuEl.getBoundingClientRect();
    menuEl.style.left = `${Math.max(0, Math.min(x, innerWidth - rect.width))}px`;
    menuEl.style.top = `${Math.max(0, Math.min(y, innerHeight - rect.height))}px`;
  });

  function select(item: ContextMenuItem) {
    item.onSelect();
    onClose();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') onClose();
  }
</script>

<svelte:window onmousedown={onClose} onkeydown={onKeydown} onblur={onClose} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="menu"
  role="menu"
  tabindex="-1"
  bind:this={menuEl}
  style="left: {x}px; top: {y}px"
  onmousedown={(event) => event.stopPropagation()}
>
  {#each items as item (item.label)}
    <button type="button" role="menuitem" onclick={() => select(item)}>{item.label}</button>
  {/each}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 100;
    display: flex;
    flex-direction: column;
    min-width: 160px;
    padding: var(--space-1);
    background: var(--bg-raised);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
  }

  button {
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-primary);
    font-size: var(--text-sm);
    text-align: left;
  }

  button:hover {
    background: var(--bg-hover);
  }
</style>
