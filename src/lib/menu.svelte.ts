import { listen } from '@tauri-apps/api/event';

import { needsPublish, repos } from './repos.svelte';
import { settings, DEFAULT_PANE_WIDTHS } from './settings.svelte';
import { inTauri } from './tauri';

/**
 * Native menu bar (SPEC.md §4.1).
 *
 * Items whose logic already lives elsewhere (Open Folder, Open Recent,
 * Fetch, Pull, Push, Reset Pane Sizes) arrive here as one `menu:action`
 * event from `menu.rs` and are dispatched to the store methods that already
 * exist for them — this file adds no new behaviour, just routes an event to
 * it. Items Rust owns outright (View's two checkboxes) arrive as
 * `pane:visibility` and are mirrored into `paneVisibility` below.
 */

type MenuAction =
  | { kind: 'open-folder' }
  | { kind: 'open-recent'; path: string }
  | { kind: 'reset-pane-sizes' }
  | { kind: 'fetch' }
  | { kind: 'pull' }
  | { kind: 'push' };

interface PaneVisibilityEvent {
  repoList: boolean;
  commitPane: boolean;
}

class PaneVisibilityStore {
  repoList = $state(true);
  commitPane = $state(true);
}

export const paneVisibility = new PaneVisibilityStore();

export async function startMenuListener(): Promise<void> {
  if (!inTauri) return;

  await listen<MenuAction>('menu:action', (event) => {
    dispatch(event.payload);
  });

  await listen<PaneVisibilityEvent>('pane:visibility', (event) => {
    paneVisibility.repoList = event.payload.repoList;
    paneVisibility.commitPane = event.payload.commitPane;
  });
}

function dispatch(action: MenuAction): void {
  switch (action.kind) {
    case 'open-folder':
      void repos.openFolder();
      break;
    case 'open-recent':
      void repos.open(action.path);
      break;
    case 'reset-pane-sizes':
      settings.paneWidths = { ...DEFAULT_PANE_WIDTHS };
      void settings.flush();
      break;
    case 'fetch':
      void repos.fetch();
      break;
    case 'pull':
      void repos.pull();
      break;
    case 'push':
      // Mirrors CommitPane's own Push/Publish branch swap (§8.7) — exactly
      // one of the two ever applies.
      void pushOrPublish();
      break;
  }
}

async function pushOrPublish(): Promise<void> {
  const id = repos.selectedId;
  const status = id ? repos.status(id) : undefined;
  if (status !== undefined && needsPublish(status)) {
    await repos.publish();
  } else {
    await repos.push();
  }
}
