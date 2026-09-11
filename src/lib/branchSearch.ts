import type { RefBadge } from './graph.svelte';

/**
 * The graph pane's branch search (SPEC.md §5.3).
 *
 * Plain case-insensitive substring on the whole ref name, which for a remote
 * badge includes its remote: typing `origin/` narrows to remote-tracking
 * branches, and typing `release` finds them under any remote. The same
 * predicate the *Switch & pull* branch picker uses (§5.1), and deliberately
 * **not** `repoFilter.ts`'s comma-separated terms — that plural exists for
 * §13's bulk banner, which writes the names of two failed repos into the
 * filter box as *Show the 2*. Nothing inside a single repo asks the same
 * question, and a comma is a legal character in a branch name.
 *
 * Extracted from `GraphPane.svelte` for the reason `repoFilter.ts` was: the
 * *ordering* is the part with cases worth pinning, and it is the part a
 * reasonable person would change first.
 */

/** Whether one ref name survives the query. An empty or whitespace-only query
 *  matches everything, which keeps a box the user has just cleared and a box
 *  they have not typed in behaving identically. */
export function matchesBranch(name: string, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (needle.length === 0) return true;
  return name.toLowerCase().includes(needle);
}

/**
 * The result list: matching refs, local before remote, newest tip first.
 *
 * **Local before remote** because the branch being hunted for is usually one
 * you had checked out once, and because the pair `x` / `origin/x` is the
 * common case — listing the local one first puts the branch you can switch to
 * above the one that has to be created to switch to it (§8.3).
 *
 * **Newest first inside each group**, not alphabetical, even though
 * `for-each-ref` hands them over sorted by name and alphabetical is what every
 * other list in Corgit uses. The repo list is alphabetical because you are
 * looking for a name you know; here you have already typed the name you know,
 * and what is left to choose between is `release/R2026-08` against
 * `release/R2024-11`. Date is the axis the remaining decision is actually on.
 *
 * Ties break on name so the order is total: two branches on one commit — a
 * branch just cut from HEAD, `main` and its tag-shaped sibling — otherwise
 * swap places between renders for no visible reason.
 */
export function searchBranches(refs: RefBadge[], query: string): RefBadge[] {
  const hits = refs.filter((ref) => matchesBranch(ref.name, query));

  const byRecency = (a: RefBadge, b: RefBadge) =>
    b.timestamp - a.timestamp || a.name.localeCompare(b.name);

  return [
    ...hits.filter((ref) => ref.kind === 'local').sort(byRecency),
    ...hits.filter((ref) => ref.kind === 'remote').sort(byRecency),
  ];
}
