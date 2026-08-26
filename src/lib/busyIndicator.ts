/**
 * When a busy indicator may be drawn (SPEC.md §13, *Work in progress*).
 *
 * Two thresholds, and they answer different complaints. The **reveal delay**
 * exists because most writes finish faster than anyone wonders whether they
 * started: a spinner that appears and vanishes inside 80 ms is a flash of
 * movement in the corner of a dashboard, which reads as a glitch rather than
 * as progress. The **minimum hold** exists for the case that lands just past
 * the delay — without it, a write finishing at 160 ms would draw three
 * indicators for a tenth of a second, which is the same flash arriving by the
 * other road.
 *
 * Deliberately not a rune, and deliberately not aware of the store: this is
 * the piece with timing bugs in it, and `vite.config.ts` only tests pure
 * logic. The component owns the boolean and feeds `set` from whatever it is
 * watching; this owns nothing but the two timers.
 *
 * Note what it does *not* do: acknowledge. The badge under the pointer and
 * the Pull chevron mark themselves the instant they are clicked, without
 * passing through here (§13: "reveal after ~150 ms" is about narration, and
 * delaying acknowledgement re-creates the bug the whole section exists for).
 */

export const REVEAL_DELAY_MS = 150;
export const MINIMUM_SHOW_MS = 300;

export class BusyIndicator {
  #active = false;
  #shown = false;
  #shownAt = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;

  /** Called with `true` when the indicator may be drawn, `false` when it must
   *  come down. Never called with the value it already has. */
  constructor(private readonly onChange: (shown: boolean) => void) {}

  set(active: boolean): void {
    if (active === this.#active) return;
    this.#active = active;
    this.#clearTimer();

    if (active) {
      // Already on screen — a second write started before the first one's
      // hold expired, so there is nothing to reveal and nothing to wait for.
      if (this.#shown) return;
      this.#timer = setTimeout(() => this.#reveal(), REVEAL_DELAY_MS);
      return;
    }

    // Never made it onto the screen, so there is nothing to take down.
    if (!this.#shown) return;

    const elapsed = Date.now() - this.#shownAt;
    if (elapsed >= MINIMUM_SHOW_MS) {
      this.#hide();
      return;
    }
    this.#timer = setTimeout(() => this.#hide(), MINIMUM_SHOW_MS - elapsed);
  }

  /** Component teardown. A pending reveal outliving its row would call back
   *  into a component that no longer exists. */
  dispose(): void {
    this.#clearTimer();
    this.#active = false;
    this.#shown = false;
  }

  #reveal(): void {
    this.#timer = undefined;
    this.#shown = true;
    this.#shownAt = Date.now();
    this.onChange(true);
  }

  #hide(): void {
    this.#timer = undefined;
    this.#shown = false;
    this.onChange(false);
  }

  #clearTimer(): void {
    if (this.#timer !== undefined) clearTimeout(this.#timer);
    this.#timer = undefined;
  }
}
