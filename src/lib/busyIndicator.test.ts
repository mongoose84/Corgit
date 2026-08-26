import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';

import { BusyIndicator, MINIMUM_SHOW_MS, REVEAL_DELAY_MS } from './busyIndicator';

/** Records every change rather than the latest one: the bug this class exists
 *  to prevent is a *pair* of changes nobody asked for, which a "current value"
 *  assertion cannot see. */
function track() {
  const changes: boolean[] = [];
  const indicator = new BusyIndicator((shown) => changes.push(shown));
  return { changes, indicator };
}

describe('BusyIndicator', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('says nothing about a write that finishes before the reveal delay', () => {
    const { changes, indicator } = track();

    indicator.set(true);
    vi.advanceTimersByTime(REVEAL_DELAY_MS - 1);
    indicator.set(false);
    vi.advanceTimersByTime(10_000);

    expect(changes).toEqual([]);
  });

  it('reveals once the wait is long enough to notice', () => {
    const { changes, indicator } = track();

    indicator.set(true);
    vi.advanceTimersByTime(REVEAL_DELAY_MS);

    expect(changes).toEqual([true]);
  });

  /** The flash arriving by the other road: revealed at 150 ms, done at 160 ms.
   *  Without the hold this is 10 ms of spinner. */
  it('holds a just-revealed indicator for the minimum', () => {
    const { changes, indicator } = track();

    indicator.set(true);
    vi.advanceTimersByTime(REVEAL_DELAY_MS);
    indicator.set(false);

    expect(changes).toEqual([true]);

    vi.advanceTimersByTime(MINIMUM_SHOW_MS - 1);
    expect(changes).toEqual([true]);

    vi.advanceTimersByTime(1);
    expect(changes).toEqual([true, false]);
  });

  it('takes a long write down as soon as it ends', () => {
    const { changes, indicator } = track();

    indicator.set(true);
    vi.advanceTimersByTime(REVEAL_DELAY_MS + MINIMUM_SHOW_MS + 5_000);
    indicator.set(false);

    expect(changes).toEqual([true, false]);
  });

  /** Two writes queued on one repo (§7 rule 1) arrive as begin/end/begin/end
   *  with a gap between. The indicator must not blink in the gap. */
  it('stays up across a second write that starts before the hold expires', () => {
    const { changes, indicator } = track();

    indicator.set(true);
    vi.advanceTimersByTime(REVEAL_DELAY_MS);
    indicator.set(false);
    vi.advanceTimersByTime(50);
    indicator.set(true);
    vi.advanceTimersByTime(5_000);

    expect(changes).toEqual([true]);
  });

  it('ignores a repeated value rather than restarting its timers', () => {
    const { changes, indicator } = track();

    indicator.set(true);
    vi.advanceTimersByTime(REVEAL_DELAY_MS - 20);
    indicator.set(true);
    vi.advanceTimersByTime(20);

    expect(changes).toEqual([true]);
  });

  it('drops a pending reveal when disposed', () => {
    const { changes, indicator } = track();

    indicator.set(true);
    indicator.dispose();
    vi.advanceTimersByTime(10_000);

    expect(changes).toEqual([]);
  });
});
