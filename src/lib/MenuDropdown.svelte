<script lang="ts">
  /*
   * One open menu's panel (SPEC.md §4.1).
   *
   * Not `ContextMenu.svelte`: that one is anchored to a click point anywhere
   * in the viewport and clamped against it, and it has no notion of a disabled
   * item, a checkmark, an accelerator column or a submenu. This one hangs off
   * a known button, so its position is a left edge and a top edge rather than
   * a measurement — and everything it adds, the context menu has no use for.
   * They share the surface tokens and nothing else.
   */
  import type { MenuEntry, MenuItem } from './menuModel';

  interface Props {
    entries: MenuEntry[];
    /** Distance from the bar's left edge to the owning button's left edge. */
    left: number;
    onChoose: (id: string) => void;
  }

  let { entries, left, onChoose }: Props = $props();

  // Which submenu is open, by id. At most one: the only submenu in the spec's
  // table is File ▸ Open Recent, and a second level of nesting is not
  // something this menu should ever grow.
  let openSubmenu: string | null = $state(null);

  function choose(item: MenuItem) {
    if (!item.enabled) return;
    onChoose(item.id);
  }
</script>

<div class="dropdown" role="menu" style="left: {left}px">
  {#each entries as entry, index (entry.kind === 'separator' ? `sep-${index}` : entry.id)}
    {#if entry.kind === 'separator'}
      <div class="separator" role="separator"></div>
    {:else if entry.kind === 'submenu'}
      <!-- An empty Open Recent is disabled rather than hidden: the item
           missing entirely on first run makes File look like a different menu
           than the one the user will see afterwards. -->
      <div
        class="row submenu"
        class:open={openSubmenu === entry.id}
        role="menuitem"
        tabindex="-1"
        aria-haspopup="menu"
        aria-expanded={openSubmenu === entry.id}
        aria-disabled={entry.items.length === 0}
        onmouseenter={() => (openSubmenu = entry.items.length > 0 ? entry.id : null)}
      >
        <span class="check"></span>
        <span class="label" class:disabled={entry.items.length === 0}>{entry.label}</span>
        <span class="arrow" aria-hidden="true">›</span>

        {#if openSubmenu === entry.id}
          <div class="dropdown nested" role="menu">
            {#each entry.items as item (item.id)}
              <button type="button" class="row" role="menuitem" onclick={() => choose(item)}>
                <span class="check"></span>
                <span class="label">{item.label}</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <button
        type="button"
        class="row"
        role={entry.checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
        aria-checked={entry.checked}
        disabled={!entry.enabled}
        onmouseenter={() => (openSubmenu = null)}
        onclick={() => choose(entry)}
      >
        <!-- The check column is reserved on every row, not only the two that
             can carry a mark. Otherwise toggling View's checkboxes shifts the
             other three items sideways. -->
        <span class="check">{entry.checked === true ? '✓' : ''}</span>
        <span class="label">{entry.label}</span>
        {#if entry.accelerator !== undefined}
          <span class="accelerator">{entry.accelerator}</span>
        {/if}
      </button>
    {/if}
  {/each}
</div>

<style>
  .dropdown {
    position: absolute;
    top: 100%;
    z-index: 100;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: var(--space-1);
    background: var(--bg-raised);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
  }

  .nested {
    /* Beside its parent row, not below it, and overlapping the panel edge by
       the padding so there is no gap for the pointer to fall through on the
       way across. */
    top: calc(var(--space-1) * -1);
    left: calc(100% - var(--space-1));
  }

  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-primary);
    font-size: var(--text-sm);
    text-align: left;
    cursor: default;
  }

  .row:hover:not(:disabled),
  .submenu.open {
    background: var(--bg-hover);
  }

  .row:disabled .label,
  .label.disabled {
    color: var(--text-disabled);
  }

  .check {
    flex: 0 0 auto;
    width: 12px;
    color: var(--text-secondary);
  }

  .label {
    flex: 1 1 auto;
  }

  /* Right-aligned and quieter than the label: it is a reminder, not a second
     thing to read on the way to picking the item. */
  .accelerator {
    flex: 0 0 auto;
    padding-left: var(--space-4);
    color: var(--text-muted);
  }

  .arrow {
    flex: 0 0 auto;
    color: var(--text-muted);
  }

  .separator {
    height: 1px;
    margin: var(--space-1) var(--space-2);
    background: var(--border);
  }
</style>
