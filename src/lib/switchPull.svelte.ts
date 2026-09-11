/**
 * Whether the multi-repo *Switch & pull* dialog is open (SPEC.md §5.1, §8.3).
 *
 * The same three-line shape as `multiBranch.svelte.ts`, and for the same
 * reason: the dialog has two doors — the repo list's *All* band, which owns the
 * component, and Repository ▸ *Switch & Pull in All (N)…*, dispatched from
 * `menu.svelte.ts`, which is not a component and nowhere near the tree.
 *
 * Two stores rather than one flag with a discriminant, because the two dialogs
 * are two components and nothing about opening one has any bearing on the
 * other. A shared `openDialog: 'branch' | 'switch' | null` would be the same
 * amount of code plus a way for the pane to render both.
 *
 * Nothing else lives here on purpose. The dialog owns its branch, its ticked
 * set and its search; none of that should survive being closed, and a store is
 * exactly where state accidentally survives things.
 */
class SwitchPullStore {
  open = $state(false);

  show(): void {
    this.open = true;
  }

  close(): void {
    this.open = false;
  }
}

export const switchPull = new SwitchPullStore();
