/**
 * The menu bar's contents as data (SPEC.md §4.1's table).
 *
 * Pure: it takes the state the menu depends on and returns the menus, so the
 * one part of the menu with real logic in it — which items are enabled, which
 * are checked, whether Push says Push — is testable without a window. The
 * component that renders it (`MenuBar.svelte`) then has no conditionals of its
 * own, which is the whole reason for splitting it out.
 *
 * Every item carries an `id`, and it is the same id the native menu used. Ids
 * Rust still handles are passed straight to its `menu_command` (see
 * `menu.rs`); the rest are dispatched in `menu.svelte.ts` to the store methods
 * that already own them.
 */

export interface MenuItem {
  kind: 'item';
  id: string;
  label: string;
  /** Displayed right-aligned, and *only* displayed — the key itself is bound
   *  in `TitleBar.svelte`. A native menu made these the same declaration; here
   *  they are two, and they can drift. `menuModel.test.ts` pins the pair. */
  accelerator?: string;
  enabled: boolean;
  /** Present on a checkbox item, absent on a plain one. */
  checked?: boolean;
}

export interface MenuSubmenu {
  kind: 'submenu';
  id: string;
  label: string;
  /** An empty submenu renders disabled rather than opening onto nothing. */
  items: MenuItem[];
}

export interface MenuSeparator {
  kind: 'separator';
}

export type MenuEntry = MenuItem | MenuSubmenu | MenuSeparator;

export interface Menu {
  id: string;
  label: string;
  entries: MenuEntry[];
}

export interface MenuState {
  recentRoots: string[];
  /** Whether a repository is selected — the only thing Repository's *lower*
   *  group depends on (§4.1: "Disabled when no repo is selected"). The root
   *  group above the separator is not about a selection and must never be
   *  disabled by one. */
  repoSelected: boolean;
  /** Whether a folder is open with repositories in it — what the root group
   *  needs instead. */
  rootOpen: boolean;
  /** How many repos *Pull All Behind* would act on, shown in its label. The
   *  strip already prints this number (§5.1) and the menu is a second route to
   *  the same button, so it must not quietly name a different set. Zero
   *  disables the item, matching the strip's greyed Pull all. */
  behindCount: number;
  /** A bulk run is in flight — both root actions are unavailable until it
   *  finishes, the same way the strip replaces itself with a progress line. */
  bulkRunning: boolean;
  /** True when the selected repo's branch has no upstream, so Push must read
   *  *Publish Branch* — the same swap `CommitPane` makes (§8.7). The menu is
   *  a second route to the same button and must not describe it differently. */
  publishing: boolean;
  repoListVisible: boolean;
  commitPaneVisible: boolean;
}

/** Trailing path segment, matching how the native submenu labelled a root and
 *  how `Welcome.svelte` labels the same list. A full path is unreadable in a
 *  menu and the paths mostly differ only in their last segment anyway. */
export function recentLabel(root: string): string {
  const parts = root.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? root;
}

export function buildMenus(state: MenuState): Menu[] {
  return [
    {
      id: 'file',
      label: 'File',
      entries: [
        { kind: 'item', id: 'open-folder', label: 'Open Folder…', accelerator: 'Ctrl+O', enabled: true },
        {
          kind: 'submenu',
          id: 'open-recent',
          label: 'Open Recent',
          items: state.recentRoots.map((root) => ({
            kind: 'item',
            // The path is in the id because it *is* the argument — the native
            // menu encoded it the same way, for the same reason: a menu item
            // has an id and a label and nowhere else to put anything.
            id: `open-recent|${root}`,
            label: recentLabel(root),
            enabled: true,
          })),
        },
        { kind: 'separator' },
        { kind: 'item', id: 'close-window', label: 'Close Window', accelerator: 'Ctrl+W', enabled: true },
        { kind: 'item', id: 'exit', label: 'Exit', enabled: true },
      ],
    },
    {
      id: 'view',
      label: 'View',
      entries: [
        {
          kind: 'item',
          id: 'toggle-repo-list',
          label: 'Toggle Repo List',
          enabled: true,
          checked: state.repoListVisible,
        },
        {
          kind: 'item',
          id: 'toggle-commit-pane',
          label: 'Toggle Commit Pane',
          enabled: true,
          checked: state.commitPaneVisible,
        },
        { kind: 'separator' },
        { kind: 'item', id: 'reset-pane-sizes', label: 'Reset Pane Sizes', enabled: true },
        { kind: 'item', id: 'reload', label: 'Reload', enabled: true },
      ],
    },
    {
      id: 'repository',
      label: 'Repository',
      entries: [
        // Root scope above the separator, selected repo below (§4.1). The
        // separator is the only thing saying which items act on what, which is
        // why the labels carry "All" rather than relying on position alone.
        {
          kind: 'item',
          id: 'fetch-all',
          label: 'Fetch All',
          enabled: state.rootOpen && !state.bulkRunning,
        },
        {
          kind: 'item',
          id: 'pull-all',
          label:
            state.behindCount > 0 ? `Pull All Behind (${state.behindCount})` : 'Pull All Behind',
          enabled: state.rootOpen && state.behindCount > 0 && !state.bulkRunning,
        },
        // The ⟳ that left the repo list's header (§5.1). It sits with the root
        // group because the group is "acts on every repository" and rediscovery
        // is exactly that — even though it is the one item here that runs no
        // git at all.
        {
          kind: 'item',
          id: 'rescan',
          label: 'Rescan Folder',
          enabled: state.rootOpen && !state.bulkRunning,
        },
        { kind: 'separator' },
        { kind: 'item', id: 'fetch', label: 'Fetch', enabled: state.repoSelected },
        { kind: 'item', id: 'pull', label: 'Pull', enabled: state.repoSelected },
        {
          kind: 'item',
          id: 'push',
          label: state.publishing ? 'Publish Branch' : 'Push',
          enabled: state.repoSelected,
        },
      ],
    },
    {
      id: 'help',
      label: 'Help',
      entries: [
        { kind: 'item', id: 'about', label: 'About', enabled: true },
        { kind: 'separator' },
        // The two halves of §13's record, in the order you reach for them:
        // the ring first, because it is the one that answers "what just went
        // wrong" without leaving the app.
        { kind: 'item', id: 'recent-problems', label: 'Recent Problems…', enabled: true },
        { kind: 'item', id: 'open-logs', label: 'Open Log Folder', enabled: true },
        { kind: 'separator' },
        // §13 requires a way back from *Don't warn me again*. Always enabled:
        // the menu would otherwise have to know the suppression set to grey
        // itself out, and an item that is disabled precisely when you have
        // forgotten what you muted is the wrong half of the trap to keep.
        {
          kind: 'item',
          id: 'reset-suppressed',
          label: 'Reset Dismissed Warnings',
          enabled: true,
        },
      ],
    },
  ];
}
