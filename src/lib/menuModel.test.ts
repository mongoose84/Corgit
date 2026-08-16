import { describe, expect, it } from 'vitest';

import { buildMenus, recentLabel, type MenuItem, type MenuState } from './menuModel';

/**
 * The menu's logic is small but it is the kind that goes wrong quietly: an
 * item enabled when it cannot act, or Push offered on a branch that has no
 * upstream to push to. None of that shows up until someone picks it.
 */

const BASE: MenuState = {
  recentRoots: [],
  repoSelected: false,
  publishing: false,
  repoListVisible: true,
  commitPaneVisible: true,
};

function menu(state: Partial<MenuState>, id: string) {
  const found = buildMenus({ ...BASE, ...state }).find((entry) => entry.id === id);
  if (found === undefined) throw new Error(`no ${id} menu`);
  return found;
}

function items(state: Partial<MenuState>, menuId: string): MenuItem[] {
  return menu(state, menuId).entries.filter((entry): entry is MenuItem => entry.kind === 'item');
}

function item(state: Partial<MenuState>, menuId: string, itemId: string): MenuItem {
  const found = items(state, menuId).find((entry) => entry.id === itemId);
  if (found === undefined) throw new Error(`no ${itemId} item`);
  return found;
}

describe('menu bar', () => {
  it('has exactly the four menus §4.1 lists, in order', () => {
    expect(buildMenus(BASE).map((entry) => entry.label)).toEqual(['File', 'View', 'Repository', 'Help']);
  });
});

describe('Repository', () => {
  // §4.1: "Disabled when no repo is selected". These three act on the
  // selection and have nothing to act on without one.
  it('disables all three items with no repo selected', () => {
    expect(items({ repoSelected: false }, 'repository').map((entry) => entry.enabled)).toEqual([
      false,
      false,
      false,
    ]);
  });

  it('enables all three once a repo is selected', () => {
    expect(items({ repoSelected: true }, 'repository').map((entry) => entry.enabled)).toEqual([
      true,
      true,
      true,
    ]);
  });

  // §8.7: the menu is a second route to the button in CommitPane, and the two
  // must not describe the same operation differently.
  it('offers Push on a branch with an upstream', () => {
    expect(item({ repoSelected: true, publishing: false }, 'repository', 'push').label).toBe('Push');
  });

  it('offers Publish Branch when the branch has no upstream', () => {
    expect(item({ repoSelected: true, publishing: true }, 'repository', 'push').label).toBe('Publish Branch');
  });

  it('keeps the same id either way, so the label never changes what is invoked', () => {
    const published = item({ repoSelected: true, publishing: false }, 'repository', 'push');
    const unpublished = item({ repoSelected: true, publishing: true }, 'repository', 'push');
    expect(published.id).toBe(unpublished.id);
  });
});

describe('View', () => {
  it('checks each toggle from its own pane visibility', () => {
    const state = { repoListVisible: true, commitPaneVisible: false };
    expect(item(state, 'view', 'toggle-repo-list').checked).toBe(true);
    expect(item(state, 'view', 'toggle-commit-pane').checked).toBe(false);
  });

  // The check column is reserved on every row of a panel, so an item that can
  // never carry a mark must be distinguishable from one that is merely
  // unchecked — `undefined` rather than `false`.
  it('leaves non-checkbox items without a checked state at all', () => {
    expect(item({}, 'view', 'reset-pane-sizes').checked).toBeUndefined();
    expect(item({}, 'file', 'open-folder').checked).toBeUndefined();
  });
});

describe('File ▸ Open Recent', () => {
  const submenu = (state: Partial<MenuState>) => {
    const found = menu(state, 'file').entries.find((entry) => entry.kind === 'submenu');
    if (found === undefined || found.kind !== 'submenu') throw new Error('no Open Recent');
    return found;
  };

  it('is present but empty on first run, rather than absent', () => {
    expect(submenu({ recentRoots: [] }).items).toEqual([]);
  });

  it('labels each root by its trailing segment and keeps the path in the id', () => {
    const entries = submenu({ recentRoots: ['C:\\dev\\code', 'D:\\work\\repos'] }).items;
    expect(entries.map((entry) => entry.label)).toEqual(['code', 'repos']);
    expect(entries[0].id).toBe('open-recent|C:\\dev\\code');
  });

  it('preserves order, so most-recent-first survives into the menu', () => {
    const roots = ['C:\\a', 'C:\\b', 'C:\\c'];
    expect(submenu({ recentRoots: roots }).items.map((entry) => entry.id.split('|')[1])).toEqual(roots);
  });
});

describe('recentLabel', () => {
  it('takes the trailing segment of either separator', () => {
    expect(recentLabel('C:\\dev\\code')).toBe('code');
    expect(recentLabel('/home/me/code')).toBe('code');
  });

  it('ignores a trailing separator rather than returning an empty label', () => {
    expect(recentLabel('C:\\dev\\code\\')).toBe('code');
  });

  it('falls back to the whole path when there is no segment to take', () => {
    expect(recentLabel('')).toBe('');
  });
});

describe('accelerators', () => {
  /*
   * A native menu item declared its key and its label together, so they could
   * not disagree. Here the label lives in `menuModel.ts` and the binding in
   * `TitleBar.svelte`'s ACCELERATORS, and nothing but this test connects them.
   * Change one and this fails — which is the point.
   */
  const BOUND: Record<string, string> = { 'open-folder': 'Ctrl+O', 'close-window': 'Ctrl+W' };

  it('labels exactly the items that are actually bound', () => {
    const labelled = buildMenus(BASE)
      .flatMap((entry) => entry.entries)
      .filter((entry): entry is MenuItem => entry.kind === 'item' && entry.accelerator !== undefined);

    expect(Object.fromEntries(labelled.map((entry) => [entry.id, entry.accelerator]))).toEqual(BOUND);
  });
});
