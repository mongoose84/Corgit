/**
 * Which local branches are *published* — pushed to a remote (SPEC.md §8.3).
 *
 * The graph's Rename and Delete entries are offered only for branches that are
 * not, and both read this one function so the two can never disagree about
 * the same badge. Pure and rune-free, like `branchName.ts`, so it is testable
 * without a pane.
 *
 * A branch counts as published on either of two signals, because each one
 * misses a case the other catches:
 *
 * - **A remote badge of the same name** (`origin/foo` for `foo`). This is
 *   "pushed" as the user means it, and it catches a branch pushed without
 *   `-u`, which has no upstream configured at all.
 * - **An upstream configured.** This catches a branch that tracks a remote
 *   branch under a *different* name, which no name comparison can see.
 *
 * Both come from the last fetch, so the answer can be stale (§5.1) — but only
 * in the cautious direction. A branch deleted on the server keeps its remote
 * badge until a `--prune` fetch removes it (§8.7), so for one sweep the entries
 * are missing where they could be offered; they are never offered where they
 * should not be.
 */

import type { RefBadge } from './graph.svelte';

/** The names of the local branches that are published, out of every ref the
 *  graph holds. A set rather than a per-badge predicate: the menu asks about
 *  each badge on a row, and the remote names are the same for all of them. */
export function publishedBranches(refs: readonly RefBadge[]): ReadonlySet<string> {
  // Only the first segment is the remote's name, as `branch::local_name` reads
  // it on the Rust side: `origin/feature/x` is the branch `feature/x`.
  const remoteNames = new Set(
    refs.filter((ref) => ref.kind === 'remote').map((ref) => ref.name.slice(ref.name.indexOf('/') + 1)),
  );

  return new Set(
    refs
      .filter((ref) => ref.kind === 'local' && (ref.upstream !== null || remoteNames.has(ref.name)))
      .map((ref) => ref.name),
  );
}
