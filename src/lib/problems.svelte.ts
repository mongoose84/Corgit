/**
 * Recent Problems (SPEC.md §13) — the frontend mirror of `problems.rs`.
 *
 * Rust owns the ring, as it owns every other piece of application state (§9.3);
 * this reads it on demand and follows the `problem:recorded` event so a window
 * that is merely watching stays in step with one that is acting.
 *
 * This list is what makes the rest of §13 defensible. Dismissing a banner,
 * ticking *Don't warn me again*, and a background sweep failing while nobody is
 * looking all discard a notification — and every one of them is only reasonable
 * because the failure is still here afterwards. Treat it as load-bearing rather
 * than as a debugging nicety: weaken it and the suppression checkbox becomes a
 * way to lose information permanently.
 */

import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

import { inTauri } from './tauri';

export interface Problem {
  /** Monotonic within the backend process, and the list's key: two identical
   *  failures a second apart are two entries, which a whole-second timestamp
   *  cannot distinguish. */
  seq: number;
  /** Unix seconds — rendered through `dateFormat.ts` so this obeys the fixed
   *  `dd-MM-yyyy HH:mm:ss` like every other date in the app. */
  at: number;
  repoId: string | null;
  /** The user's word for what they asked: "Push", "Pull", "Commit". */
  operation: string;
  /** Raw and untruncated (§13). Someone reading this list has already found
   *  the headline insufficient. */
  message: string;
}

class ProblemStore {
  entries = $state<Problem[]>([]);
  /** Whether the window is up. Held here rather than in `App.svelte` so the
   *  Help menu can open it without the component owning menu state. */
  open = $state(false);

  async load(): Promise<void> {
    if (!inTauri) return;
    try {
      this.entries = await invoke<Problem[]>('recent_problems');
    } catch (err) {
      // A failure to read the failure list is not worth a banner of its own —
      // that way lies a loop, and the log still has everything.
      console.warn('corgit: could not read recent problems', err);
    }
  }

  async show(): Promise<void> {
    await this.load();
    this.open = true;
  }

  async clear(): Promise<void> {
    if (!inTauri) return;
    try {
      await invoke('clear_problems');
      this.entries = [];
    } catch (err) {
      console.warn('corgit: could not clear recent problems', err);
    }
  }

  /** Follows the backend's ring rather than appending locally: the window may
   *  be showing failures from a *different* window (§9.2), and re-reading is
   *  cheap next to keeping two copies of a 50-entry list in agreement. */
  async start(): Promise<void> {
    if (!inTauri) return;
    await listen('problem:recorded', () => {
      if (this.open) void this.load();
    });
  }
}

export const problems = new ProblemStore();
