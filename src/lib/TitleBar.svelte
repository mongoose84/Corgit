<script lang="ts">
  /*
   * The one top row (SPEC.md §4.1): app mark, menus, then the caption buttons.
   *
   * This replaces two rows the OS used to draw — its caption and its menu bar,
   * stacked — with one row in the client area, worth about 30px of vertical
   * space in a window whose whole job is showing as many repository rows as
   * possible. The cost is recorded in §4.1: Snap Layouts' hover flyout goes
   * with the native frame, and every other thing the frame did for free is now
   * ours.
   *
   * `data-tauri-drag-region` is what makes the bar behave like a caption:
   * press-and-drag moves the window, double-click maximizes it. It only
   * applies to the element the press actually lands on, so it goes on the bar
   * and on the spacer — the two places with nothing in them — and never on a
   * button, where it would swallow the click.
   */
  import Mascot from './Mascot.svelte';
  import MenuBar from './MenuBar.svelte';
  import WindowControls from './WindowControls.svelte';
  import { chooseMenuItem } from './menu.svelte';

  /*
   * Accelerators (§4.1's table). The native menu bound these as part of
   * declaring the item; an HTML menu has to bind them separately, which means
   * the key and the label in `menuModel.ts` can now drift apart —
   * `menuModel.test.ts` is what stops that.
   *
   * Deliberately not guarded on the focused element. Both of these are
   * window-level operations that mean the same thing wherever the caret is,
   * and Ctrl+O and Ctrl+W do nothing in a textarea worth preserving. A filter
   * box that swallowed Ctrl+W would be the surprising behaviour, not this.
   */
  const ACCELERATORS: Record<string, string> = {
    o: 'open-folder',
    w: 'close-window',
  };

  function onKeydown(event: KeyboardEvent) {
    if (!event.ctrlKey || event.altKey || event.shiftKey || event.metaKey) return;

    const id = ACCELERATORS[event.key.toLowerCase()];
    if (id === undefined) return;

    // Ctrl+W would otherwise reach the webview, which treats it as "close the
    // tab" — in a Tauri window that is a blank page with no way back.
    event.preventDefault();
    chooseMenuItem(id);
  }
</script>

<svelte:window onkeydown={onKeydown} />

<header class="titlebar" data-tauri-drag-region>
  <!-- The app mark's first use in the UI: docs/mascot.md §7 had it as
       icon-and-installer-only, because until now the window's icon was drawn
       by Windows from `icon.ico` and there was no other place for it. It is
       below the 24px floor that section sets for the full poses, which is the
       case the head crop exists for — the taskbar has been rendering it at
       16px all along. -->
  <div class="mark" data-tauri-drag-region>
    <Mascot pose="mark" height={20} />
  </div>

  <MenuBar />

  <div class="spacer" data-tauri-drag-region></div>

  <WindowControls />
</header>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    height: 100%;
    /* Its own surface rather than --bg-app: the bar is the window's edge, and
       a caption that reads as part of the first pane makes the window look
       like it has no top. */
    background: var(--bg-surface);
    border-bottom: 1px solid var(--border);
  }

  .mark {
    display: flex;
    align-items: center;
    /* Windows leaves its icon about this far from the window edge; the eye is
       calibrated to it from every other title bar on the desktop. */
    padding: 0 var(--space-2) 0 var(--space-3);
  }

  /* The draggable middle. It has to be an element of its own and it has to
     grow: the caption's usable drag area is whatever is left after the menus,
     and on a narrow window that can be nothing at all — which is fine, but it
     must not be *negative*, i.e. must not push the buttons off the edge. */
  .spacer {
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
  }
</style>
