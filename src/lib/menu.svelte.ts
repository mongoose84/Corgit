import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { buildMenus, type Menu } from './menuModel';
import { needsPublish, repos } from './repos.svelte';
import { settings } from './settings.svelte';
import { inTauri } from './tauri';

/**
 * Menu bar wiring (SPEC.md §4.1).
 *
 * The bar is drawn by `MenuBar.svelte` and described by `menuModel.ts`; this
 * is the part in between — it assembles the model from live store state, and
 * it routes a chosen id to whoever owns that item.
 *
 * The routing split is the same one the native menu had, just travelling the
 * other way. Items that only touch process lifecycle or a boolean Rust owns
 * (§9.3) go out to `menu_command`; everything else is a store method that has
 * existed all along, and is now called directly rather than round-tripping
 * through Rust to come back as a `menu:action` event. That event is gone —
 * with the menu itself in the webview there was nothing left for it to carry.
 */

/** Ids `menu.rs` handles. Everything else is dispatched locally, below. */
const RUST_IDS = new Set(['close-window', 'exit', 'toggle-repo-list', 'toggle-commit-pane', 'about', 'open-logs']);

interface PaneVisibilityEvent {
  repoList: boolean;
  commitPane: boolean;
}

class PaneVisibilityStore {
  repoList = $state(true);
  commitPane = $state(true);
}

export const paneVisibility = new PaneVisibilityStore();

/**
 * The live menu. A `$derived` so the parts that change while a menu is open —
 * a sweep finishing and making Push into Publish Branch, a pane toggling —
 * are reflected without the bar having to subscribe to anything.
 */
export function menus(): Menu[] {
  const id = repos.selectedId;
  const status = id === undefined ? undefined : repos.status(id);

  return buildMenus({
    recentRoots: settings.data.recentRoots,
    repoSelected: id !== undefined,
    publishing: status !== undefined && needsPublish(status),
    repoListVisible: paneVisibility.repoList,
    commitPaneVisible: paneVisibility.commitPane,
  });
}

export async function startMenuListener(): Promise<void> {
  if (!inTauri) return;

  await listen<PaneVisibilityEvent>('pane:visibility', (event) => {
    paneVisibility.repoList = event.payload.repoList;
    paneVisibility.commitPane = event.payload.commitPane;
  });

  // Rust owns these two booleans and outlives the webview, so after a reload
  // the menu's checkmarks would otherwise show their defaults rather than the
  // truth. The native menu never had this problem — it was not inside the
  // thing being reloaded. Asking once at startup is the whole fix.
  await invoke('publish_pane_visibility').catch(() => {});
}

export function chooseMenuItem(id: string): void {
  if (RUST_IDS.has(id)) {
    void invoke('menu_command', { id }).catch(() => {});
    return;
  }

  const recent = id.match(/^open-recent\|(.+)$/);
  if (recent !== null) {
    void repos.open(recent[1]);
    return;
  }

  switch (id) {
    case 'open-folder':
      void repos.openFolder();
      break;
    case 'reset-pane-sizes':
      settings.resetLayout();
      break;
    // Rust used to do this by evaluating `location.reload()` in this very
    // webview. With the caller already in the webview, the round trip was the
    // only part of it that was doing anything.
    case 'reload':
      location.reload();
      break;
    case 'fetch':
      void repos.fetch();
      break;
    case 'pull':
      void repos.pull();
      break;
    case 'push':
      // Mirrors CommitPane's own Push/Publish branch swap (§8.7) — exactly
      // one of the two ever applies. The label was chosen the same way, in
      // `menuModel.ts`; this picks the matching call.
      void pushOrPublish();
      break;
    default:
      console.warn(`unhandled menu item (${id})`);
  }
}

async function pushOrPublish(): Promise<void> {
  const id = repos.selectedId;
  const status = id === undefined ? undefined : repos.status(id);
  if (status !== undefined && needsPublish(status)) {
    await repos.publish();
  } else {
    await repos.push();
  }
}
