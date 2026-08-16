<script lang="ts">
  /*
   * Minimize / maximize-restore / close, for a window with `decorations: false`
   * (SPEC.md §4.1). Removing the native frame is what makes one combined row
   * possible at all; the cost is that these three buttons — and the caption's
   * whole hit-test behaviour — become ours to draw.
   *
   * Drawn as SVG rather than typed as characters or set in Segoe Fluent Icons,
   * for `Glyph.svelte`'s reason and one more: the chrome glyphs moved between
   * MDL2 Assets and Fluent Icons, and which font is present depends on the
   * Windows build. A shape we draw is the same shape on every machine.
   *
   * The 46×32 button and the red close hover are Windows' own metrics, kept
   * deliberately — these three controls are the one part of the app where
   * matching the OS beats matching the app, because the muscle memory being
   * served was trained by every other window on the desktop.
   */
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { inTauri } from './tauri';
  import { windowFrame } from './windowFrame.svelte';

  // Read, never written here: the window can be maximized without this button
  // ever being pressed, so the glyph follows the window rather than the click
  // (see `windowFrame.svelte.ts`).
  const maximized = $derived(windowFrame.maximized);

  // Outside Tauri (`npm run dev`) there is no window to command. The buttons
  // still render so the bar can be laid out in a browser; they just do
  // nothing, which is the same bargain the rest of the frontend makes.
  function minimize() {
    if (inTauri) void getCurrentWindow().minimize();
  }

  function toggleMaximize() {
    if (inTauri) void getCurrentWindow().toggleMaximize();
  }

  function close() {
    if (inTauri) void getCurrentWindow().close();
  }
</script>

<div class="controls">
  <button type="button" class="control" aria-label="Minimize" onclick={minimize}>
    <svg viewBox="0 0 10 10" aria-hidden="true">
      <path d="M0 5.5 h10" />
    </svg>
  </button>

  <button
    type="button"
    class="control"
    aria-label={maximized ? 'Restore' : 'Maximize'}
    onclick={toggleMaximize}
  >
    {#if maximized}
      <!-- Two offset rectangles, the back one clipped by the front: the
           restore glyph reads as "there is another size behind this one". -->
      <svg viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0.5 3 h6.5 v6.5 h-6.5 z" />
        <path d="M3 3 v-2.5 h6.5 v6.5 h-2.5" />
      </svg>
    {:else}
      <svg viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0.5 0.5 h9 v9 h-9 z" />
      </svg>
    {/if}
  </button>

  <button type="button" class="control close" aria-label="Close" onclick={close}>
    <svg viewBox="0 0 10 10" aria-hidden="true">
      <path d="M0.5 0.5 l9 9 M9.5 0.5 l-9 9" />
    </svg>
  </button>
</div>

<style>
  .controls {
    display: flex;
    /* Flush to the top-right corner. The close button must reach the very
       last pixel of the window or the Fitts's-law throw into the corner —
       the fastest target on the screen — stops landing on it. */
    align-self: stretch;
  }

  .control {
    display: grid;
    place-items: center;
    /* Windows' own caption-button box. Wider than it is tall, and wider than
       anything else in this bar. */
    width: 46px;
    height: 100%;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-secondary);
    cursor: default;
  }

  .control:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .control:active {
    background: var(--bg-active);
  }

  .close:hover,
  .close:active {
    background: var(--titlebar-close-hover);
    color: var(--accent-text);
  }

  svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1;
  }
</style>
