import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { buildMenus, type Menu } from './menuModel';
import { needsPublish, repos } from './repos.svelte';
import { problems } from './problems.svelte';
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
    rootOpen: repos.root !== null && repos.repos.length > 0,
    // The same getter the strip's count comes from (§5.1), not a second walk
    // over the statuses: the menu is another route to the same button, and two
    // counts derived separately are two counts that can disagree.
    behindCount: repos.behindCount,
    bulkRunning: repos.bulk !== null,
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
    // Both are frontend state, so both are dispatched here rather than out to
    // `menu_command` — the split §4.1 describes, unchanged: Rust handles only
    // what Rust owns, and it owns neither the Problems window nor the
    // suppression set (which lives in settings and is mirrored, §9.3).
    case 'recent-problems':
      void problems.show();
      break;
    case 'reset-suppressed':
      settings.resetSuppressed();
      break;
    // Rust used to do this by evaluating `location.reload()` in this very
    // webview. With the caller already in the webview, the round trip was the
    // only part of it that was doing anything.
    case 'reload':
      location.reload();
      break;
    case 'fetch-all':
      void repos.fetchAll();
      break;
    case 'pull-all':
      void repos.pullAllBehind();
      break;
    // The repo list's old ⟳, unchanged behaviour and a label that finally says
    // what it does: rediscovery first, status as a consequence (§5.1).
    case 'rescan':
      void repos.refresh();
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
