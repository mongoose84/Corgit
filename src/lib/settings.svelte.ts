import { invoke } from '@tauri-apps/api/core';

import { inTauri } from './tauri';

/**
 * Settings mirror (SPEC.md §9).
 *
 * Rust owns the truth; this is a view that reads once at startup and writes
 * back debounced. When the frontend runs outside Tauri (plain `npm run dev` in
 * a browser) it falls back to localStorage so the layout is still workable
 * without the Rust toolchain installed.
 */

export interface PaneWidths {
  /** Fraction of usable width, not pixels — survives window resizing. */
  left: number;
  middle: number;
}

export interface Settings {
  version: number;
  paneWidths: PaneWidths;
  /** The diff view's old/new split (§5.4), as a fraction of that pane's width. */
  diffSplit: number;
  scanDepth: number;
  statusSweepSecs: number;
  fetchSweepSecs: number;
  recentRoots: string[];
  /** Rule ids from `gitErrors.ts` the user has ticked *Don't show this again*
   *  on (§13). Only the banner is silenced by these — the row badge and the
   *  Recent Problems entry are unaffected, which is what makes suppressing one
   *  safe to offer at all. */
  suppressedNotices: string[];
}

export const DEFAULT_PANE_WIDTHS: PaneWidths = { left: 0.25, middle: 0.2 };
/** Even, because neither side of a diff is the more important one by default. */
export const DEFAULT_DIFF_SPLIT = 0.5;

const DEFAULTS: Settings = {
  version: 1,
  paneWidths: { ...DEFAULT_PANE_WIDTHS },
  diffSplit: DEFAULT_DIFF_SPLIT,
  scanDepth: 1,
  statusSweepSecs: 60,
  fetchSweepSecs: 300,
  recentRoots: [],
  suppressedNotices: [],
};

const SAVE_DEBOUNCE_MS = 500;
const STORAGE_KEY = 'corgit.settings';

class SettingsStore {
  loaded = $state(false);
  data = $state<Settings>(structuredClone(DEFAULTS));

  #timer: ReturnType<typeof setTimeout> | undefined;
  #loading = false;

  get paneWidths(): PaneWidths {
    return this.data.paneWidths;
  }

  set paneWidths(value: PaneWidths) {
    this.data.paneWidths = value;
    this.queueSave();
  }

  /** Read defensively: a settings.json written before this field existed
   *  parses without it, and `undefined.includes` would take down every banner
   *  rather than the one being asked about. */
  isSuppressed(ruleId: string): boolean {
    return this.data.suppressedNotices?.includes(ruleId) ?? false;
  }

  suppress(ruleId: string): void {
    if (this.isSuppressed(ruleId)) return;
    this.data.suppressedNotices = [...(this.data.suppressedNotices ?? []), ruleId];
    // Flushed rather than debounced: this is a decision, not a drag, and the
    // user's next act after ticking the box may well be closing the window.
    void this.flush();
  }

  /** *Help ▸ Reset Dismissed Warnings* (§4.1). §13 requires this to exist: a
   *  suppression with no way back is a trap, and the user who set it is by
   *  definition no longer seeing the thing that would remind them. */
  resetSuppressed(): void {
    this.data.suppressedNotices = [];
    void this.flush();
  }

  get diffSplit(): number {
    return this.data.diffSplit;
  }

  set diffSplit(value: number) {
    this.data.diffSplit = value;
    this.queueSave();
  }

  /** *View ▸ Reset Pane Sizes* and a divider's double-click (§4.1). Shared so
   *  the menu item and the dividers cannot reset different sets of things —
   *  every draggable boundary in the window goes back to its default here. */
  resetLayout(): void {
    this.data.paneWidths = { ...DEFAULT_PANE_WIDTHS };
    this.data.diffSplit = DEFAULT_DIFF_SPLIT;
    void this.flush();
  }

  /** Re-read what the backend holds. Opening a folder appends to the
   *  recent-roots list back there, which the welcome screen renders. */
  async reload(): Promise<void> {
    this.loaded = false;
    await this.load();
  }

  async load(): Promise<void> {
    if (this.loaded || this.#loading) return;
    this.#loading = true;
    try {
      this.data = inTauri ? await invoke<Settings>('get_settings') : readLocal();
    } catch (err) {
      // Settings are advisory. A broken file must never block startup (§9.5).
      console.warn('corgit: could not load settings, using defaults', err);
      this.data = structuredClone(DEFAULTS);
    } finally {
      this.#loading = false;
      this.loaded = true;
    }
  }

  /** Coalesce rapid changes — a pane drag fires on every pointer move. */
  queueSave(): void {
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => void this.persist(), SAVE_DEBOUNCE_MS);
  }

  /** Write now, skipping the debounce (e.g. on pointer release). */
  async flush(): Promise<void> {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    await this.persist();
  }

  private async persist(): Promise<void> {
    // $state.snapshot strips the reactive proxy; a proxy cannot cross the
    // Tauri IPC boundary or reach structuredClone intact.
    const settings = $state.snapshot(this.data) as Settings;
    try {
      if (inTauri) {
        await invoke('save_settings', { settings });
      } else {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
      }
    } catch (err) {
      console.warn('corgit: could not save settings', err);
    }
  }
}

function readLocal(): Settings {
  const raw = localStorage.getItem(STORAGE_KEY);
  if (!raw) return structuredClone(DEFAULTS);
  try {
    return { ...structuredClone(DEFAULTS), ...(JSON.parse(raw) as Partial<Settings>) };
  } catch {
    return structuredClone(DEFAULTS);
  }
}

export const settings = new SettingsStore();
