/**
 * Which local branches are *published* — pushed to a remote (SPEC.md §8.3).
 *
 * The graph's Rename and Delete entries are offered only for branches that are
 * not, and both read this one function so the two can never disagree about
 * the same badge. Pure and rune-free, like `branchName.ts`, so it is testable
 * without a pane.
 *
 * Published means one thing: **a remote badge of the same branch name
 * exists** (`origin/foo` for `foo`). That is the server still holding a copy
 * under this name, which is exactly what a rename would strand and what a
 * delete should wait for.
 *
 * The configured upstream is deliberately not a second signal, though it once
 * was. `%(upstream:short)` keeps printing after the server branch is gone —
 * `branch.<name>.merge` outlives a `--prune` — so a branch pushed with `-u`
 * stayed "published" forever and Delete never came back after its PR was
 * merged. And a branch tracking a *different* name (`feature-x` →
 * `origin/main`, which older builds created via `branch.autoSetupMerge`) was
 * never pushed under its own name at all: `needsPublish` in `repos.svelte.ts`
 * calls it unpublished, and so must this. Renaming one is harmless anyway —
 * `git branch -m` carries the tracking config across to the new name.
 *
 * This is not `needsPublish`, and the two can differ on purpose. That one asks
 * "can a plain `git push` work?", this one "does the server hold a copy under
 * this name?". A branch pushed without `-u` is both: Push says *Publish*,
 * because there is no upstream, and the graph holds back Rename, because
 * there is a server copy.
 *
 * The badges are as of the last fetch (§5.1), and stale only in the cautious
 * direction: a branch deleted on the server keeps its remote badge until a
 * `--prune` fetch (§8.7) removes it, which is what makes the entries appear.
 */

import type { RefBadge } from './graph.svelte';

/** The names of the local branches that are published, out of every ref the
 *  graph holds. A set rather than a per-badge predicate: the menu asks about
 *  each badge on a row, and the remote badges are the same for all of them.
 *
 *  Matched as a `/`-bounded suffix of the remote badge, not by cutting off its
 *  first segment as `upstreamBranch` and `branch::local_name` do. Those only
 *  have to name *a* branch; this decides whether to offer a write, and a
 *  remote named `jk/fork` would make the cut read `jk/fork/fix` as `fork/fix`
 *  and offer a rename on a pushed `fix`. The suffix reads it right, and where
 *  it can be wrong — local `x` beside `origin/feature/x` — it errs towards
 *  holding the entries back, which is the side to be wrong on. */
export function publishedBranches(refs: readonly RefBadge[]): ReadonlySet<string> {
  const remoteNames = refs.filter((ref) => ref.kind === 'remote').map((ref) => ref.name);

  return new Set(
    refs
      .filter((ref) => ref.kind === 'local' && remoteNames.some((remote) => remote.endsWith(`/${ref.name}`)))
      .map((ref) => ref.name),
  );
}
