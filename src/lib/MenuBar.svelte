<script lang="ts">
  /*
   * File · View · Repository · Help, in the title bar (SPEC.md §4.1).
   *
   * The behaviours below are not decoration — they are what makes a menu bar
   * read as one rather than as four independent popup buttons, and a native
   * menu supplied all of them:
   *
   *   - click to open, click the same top-level again to close;
   *   - once *something* is open, hovering another top-level switches to it
   *     without a second click. This is the one people notice by its absence;
   *   - Escape closes and returns focus to the button, so the keyboard is not
   *     stranded inside a menu that is no longer visible;
   *   - a click anywhere else closes, including in the pane underneath;
   *   - ← and → move between menus while open, ↑ and ↓ within one.
   *
   * Alt-key mnemonics (Alt+F for File) are deliberately not here. They want
   * underlined letters, an Alt-held state, and a rule for what happens when a
   * mnemonic collides with a shortcut — §4.1 records the omission rather than
   * half-building it.
   */
  import MenuDropdown from './MenuDropdown.svelte';
  import { chooseMenuItem, menus } from './menu.svelte';

  const open = $derived(menus());

  let openId: string | null = $state(null);
  let barEl: HTMLElement | undefined = $state();

  function toggle(id: string) {
    openId = openId === id ? null : id;
  }

  function hover(id: string) {
    // Only switch, never open: hovering across the bar with nothing open must
    // not spring a menu on someone on their way to the drag region.
    if (openId !== null) openId = id;
  }

  function choose(id: string) {
    openId = null;
    chooseMenuItem(id);
  }

  function close() {
    openId = null;
  }

  function onKeydown(event: KeyboardEvent) {
    if (openId === null) return;

    if (event.key === 'Escape') {
      event.preventDefault();
      focusButton(openId);
      openId = null;
      return;
    }

    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      event.preventDefault();
      const index = open.findIndex((menu) => menu.id === openId);
      const step = event.key === 'ArrowRight' ? 1 : -1;
      // Wraps, like a native menu bar: the ends of the row are not walls.
      const next = open[(index + step + open.length) % open.length];
      openId = next.id;
      focusButton(next.id);
    }
  }

  function focusButton(id: string) {
    barEl?.querySelector<HTMLButtonElement>(`[data-menu="${id}"]`)?.focus();
  }
</script>

<!-- `onmousedown` rather than `onclick` so a press that starts outside closes
     the menu before anything under it can act on the release, and `onblur` so
     alt-tabbing away does not leave a menu hanging over an unfocused window
     the way the native one never would. -->
<svelte:window onmousedown={close} onkeydown={onKeydown} onblur={close} />

<!-- A plain `div`, not a `nav`: `nav` already carries a landmark role, and
     overriding it with `menubar` is what the a11y lint objects to. The menu
     bar is not navigation in the landmark sense anyway — it acts on the app
     rather than moving around it.

     `tabindex="-1"` because the `menubar` role must be focusable, but the bar
     itself is never a tab stop — the four buttons inside it are, and they are
     real `<button>`s already. -->
<div
  class="menubar"
  bind:this={barEl}
  role="menubar"
  aria-label="Main menu"
  tabindex="-1"
  onmousedown={(event) => event.stopPropagation()}
>
  {#each open as menu (menu.id)}
    <div class="slot">
      <button
        type="button"
        class="top"
        class:open={openId === menu.id}
        data-menu={menu.id}
        role="menuitem"
        aria-haspopup="menu"
        aria-expanded={openId === menu.id}
        onclick={() => toggle(menu.id)}
        onmouseenter={() => hover(menu.id)}
      >
        {menu.label}
      </button>

      {#if openId === menu.id}
        <MenuDropdown entries={menu.entries} left={0} onChoose={choose} />
      {/if}
    </div>
  {/each}
</div>

<style>
  .menubar {
    display: flex;
    align-items: stretch;
    height: 100%;
  }

  /* Each top-level gets a positioned box so its panel can hang from it
     directly. That is the whole reason this menu needs no viewport
     measurement, unlike the context menu: the anchor is an element. */
  .slot {
    position: relative;
    display: flex;
  }

  .top {
    padding: 0 var(--space-3);
    border: 0;
    background: none;
    color: var(--text-secondary);
    font-size: var(--text-sm);
    white-space: nowrap;
    cursor: default;
  }

  .top:hover,
  .top.open {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
</style>
