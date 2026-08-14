import { invoke } from '@tauri-apps/api/core';

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
  scanDepth: number;
  statusSweepSecs: number;
  fetchSweepSecs: number;
  recentRoots: string[];
}

export const DEFAULT_PANE_WIDTHS: PaneWidths = { left: 0.25, middle: 0.2 };

const DEFAULTS: Settings = {
  version: 1,
  paneWidths: { ...DEFAULT_PANE_WIDTHS },
  scanDepth: 1,
  statusSweepSecs: 60,
  fetchSweepSecs: 300,
  recentRoots: [],
};

const SAVE_DEBOUNCE_MS = 500;
const STORAGE_KEY = 'twogit.settings';

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

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

  async load(): Promise<void> {
    if (this.loaded || this.#loading) return;
    this.#loading = true;
    try {
      this.data = inTauri ? await invoke<Settings>('get_settings') : readLocal();
    } catch (err) {
      // Settings are advisory. A broken file must never block startup (§9.5).
      console.warn('twogit: could not load settings, using defaults', err);
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
      console.warn('twogit: could not save settings', err);
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
