<script lang="ts">
  /**
   * A draggable vertical pane divider.
   *
   * Reports the pointer's absolute x position and lets the parent decide what
   * it means — the parent is the only place that knows the pane geometry and
   * minimum widths.
   */
  interface Props {
    label: string;
    /** Current position as a percentage of the container, for assistive tech. */
    value: number;
    ondrag: (clientX: number) => void;
    onrelease?: () => void;
    onreset?: () => void;
  }

  let { label, value, ondrag, onrelease, onreset }: Props = $props();

  let dragging = $state(false);

  function pointerdown(event: PointerEvent) {
    if (event.button !== 0) return;
    dragging = true;
    // Pointer capture keeps events coming even when the cursor outruns the
    // 5px divider, which it will.
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  function pointermove(event: PointerEvent) {
    if (dragging) ondrag(event.clientX);
  }

  function pointerup(event: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    const el = event.currentTarget as HTMLElement;
    if (el.hasPointerCapture(event.pointerId)) el.releasePointerCapture(event.pointerId);
    onrelease?.();
  }

  function keydown(event: KeyboardEvent) {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    const step = event.shiftKey ? 40 : 8;
    const edge = (event.currentTarget as HTMLElement).getBoundingClientRect().left;
    ondrag(edge + (event.key === 'ArrowLeft' ? -step : step));
    onrelease?.();
    event.preventDefault();
  }
</script>

<!--
  A focusable separator is the ARIA window-splitter pattern: role="separator"
  plus tabindex and value attributes. Svelte's a11y lint only knows the
  non-focusable form of the role, so the two warnings it raises are wrong here.
-->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="divider"
  class:dragging
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={value}
  aria-valuemin={0}
  aria-valuemax={100}
  tabindex="0"
  onpointerdown={pointerdown}
  onpointermove={pointermove}
  onpointerup={pointerup}
  onpointercancel={pointerup}
  ondblclick={() => onreset?.()}
  onkeydown={keydown}
></div>

<style>
  .divider {
    position: relative;
    height: 100%;
    background: var(--border);
    cursor: col-resize;
    touch-action: none;
  }

  /* Widen the hit area beyond the visible line without widening the layout. */
  .divider::after {
    content: '';
    position: absolute;
    inset: 0 -3px;
  }

  .divider:hover,
  .divider.dragging {
    background: var(--accent);
  }

  .divider:focus-visible {
    outline: none;
    background: var(--accent);
  }
</style>
