/**
 * The repo list's filter box (SPEC.md §5.1).
 *
 * Substring on **repo name only** — not branch, not path — with one addition:
 * a comma-separated value matches any of its terms. The same rule, made
 * plural, and the reason it is plural is §13's bulk-run banner. Two repos that
 * failed a *Pull all* share no substring, so *Show the 2* had nothing it could
 * write into a box that took a single needle; the alternatives were a
 * predicate syntax (`is:failed`) or a second way to narrow the list, and both
 * cost more than teaching the one box to hold a list.
 *
 * Extracted from `RepoList.svelte` rather than left inline because it is the
 * one piece of that component with cases worth pinning — the trailing comma
 * especially, which is a state the box is in for as long as it takes to type
 * the next character.
 */

/**
 * Splits what the user typed into the terms to match against.
 *
 * Empty terms are dropped rather than treated as matching everything. That is
 * what makes typing a list bearable: `"corgit,"` is a comma the user has not
 * finished, and a filter that flashed the full 77 rows between keystrokes
 * would be unusable in exactly the case this feature exists for.
 */
export function filterTerms(filter: string): string[] {
  return filter
    .toLowerCase()
    .split(',')
    .map((term) => term.trim())
    .filter((term) => term.length > 0);
}

/**
 * Whether a repo name survives the filter. No terms means no filtering, which
 * keeps an empty box and a box holding only separators behaving identically.
 */
export function matchesFilter(name: string, terms: string[]): boolean {
  if (terms.length === 0) return true;
  const lowered = name.toLowerCase();
  return terms.some((term) => lowered.includes(term));
}

/**
 * The value the bulk banner's *Show the N* writes into the box (§5.1).
 *
 * Here rather than in the store so the format and the parser above cannot
 * drift apart: this is the one string in the app that has to survive a
 * round trip through `filterTerms` and come back as the same set of repos.
 */
export function filterForNames(names: string[]): string {
  return names.join(', ');
}
