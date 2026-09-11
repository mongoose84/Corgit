/**
 * Whether the multi-repo Create Branch dialog is open (SPEC.md §5.1, §8.3).
 *
 * A three-line store rather than a `$state` inside `RepoList` because the
 * dialog has two doors: the repo list's *Pinned* header, which owns the
 * component, and Repository ▸ *Create Branch in Pinned*, which is dispatched
 * from `menu.svelte.ts` — not a component, and nowhere near the tree. The same
 * shape and the same reason as `problems.svelte.ts`.
 *
 * Nothing else lives here on purpose. The dialog owns its own name, selection
 * and search; none of that should survive being closed, and a store is exactly
 * where state accidentally survives things.
 */
class MultiBranchStore {
  open = $state(false);

  show(): void {
    this.open = true;
  }

  close(): void {
    this.open = false;
  }
}

export const multiBranch = new MultiBranchStore();
