import { getCurrentWindow } from '@tauri-apps/api/window';

import { inTauri } from './tauri';

/**
 * Whether the window is maximized, for the caption's middle button
 * (SPEC.md §4.1).
 *
 * It lives in a store rather than in `WindowControls.svelte` because the
 * window can be maximized without that button ever being pressed — a
 * double-click on the drag region, Win+Up, a drag to the top edge, or the
 * shell restoring the window on its own — so the glyph has to follow the
 * window, not the click, and anything else that ever needs the same answer
 * must get it from the same place.
 *
 * **There is no maximized inset, and adding one is a mistake.** An undecorated
 * window keeps `WS_THICKFRAME` (which is what still gives it grabbable resize
 * edges), and maximized, its *window rect* is the work area inflated by the
 * border thickness — measured here as 1938×1158 against a 1920×1140 work area,
 * 9px past every edge. That looks exactly like the well-known bug where a
 * custom title bar's close button ends up off-screen, and the usual fix is to
 * pad the content by the border thickness. It does not apply: tao handles
 * `WM_NCCALCSIZE`, so the *client* rect is already 1920×1140 at screen 0,0.
 * The overhang is entirely outside the client area and cannot be seen. Padding
 * it away costs a visible 9px band on all four sides and buys nothing —
 * measure the client rect, not the window rect, before believing otherwise.
 */
class WindowFrame {
  maximized = $state(false);
}

export const windowFrame = new WindowFrame();

export async function startWindowFrame(): Promise<void> {
  if (!inTauri) return;

  const window = getCurrentWindow();
  const sync = () => void window.isMaximized().then((value) => (windowFrame.maximized = value));

  sync();
  await window.onResized(sync);
}
