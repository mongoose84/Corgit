import { describe, expect, it } from 'vitest';

import { filterForNames, filterTerms, matchesFilter } from './repoFilter';

/**
 * §5.1's rule is "substring on repo name only", and everything here is about
 * keeping the plural version of it honest — a filter that quietly matched more
 * than the user typed would be worse than one that could not hold a list at
 * all, because the list is what they navigate 77 repos with.
 */

function shown(names: string[], filter: string): string[] {
  const terms = filterTerms(filter);
  return names.filter((name) => matchesFilter(name, terms));
}

const REPOS = ['billing-worker', 'checkout-web', 'identity', 'docs-site'];

describe('a single term', () => {
  it('matches by substring, as it always did', () => {
    expect(shown(REPOS, 'work')).toEqual(['billing-worker']);
  });

  it('ignores case in both directions', () => {
    expect(shown(['Corgit'], 'CORGIT')).toEqual(['Corgit']);
  });

  it('shows everything on an empty box', () => {
    expect(shown(REPOS, '')).toEqual(REPOS);
  });

  it('shows everything on a box holding only whitespace', () => {
    expect(shown(REPOS, '   ')).toEqual(REPOS);
  });
});

describe('a comma-separated list', () => {
  it('matches any of its terms', () => {
    expect(shown(REPOS, 'billing-worker, identity')).toEqual(['billing-worker', 'identity']);
  });

  it('tolerates missing spaces after the commas', () => {
    expect(shown(REPOS, 'billing-worker,identity')).toEqual(['billing-worker', 'identity']);
  });

  /*
   * The state the box is in for as long as it takes to type the next
   * character. An empty term treated as "matches everything" would flash all
   * 77 rows between keystrokes, which is unusable in exactly the case the
   * feature exists for.
   */
  it('ignores a trailing comma rather than matching everything', () => {
    expect(shown(REPOS, 'identity,')).toEqual(['identity']);
  });

  it('ignores empty terms in the middle of a list', () => {
    expect(shown(REPOS, 'identity, , docs')).toEqual(['identity', 'docs-site']);
  });

  it('never widens a term — a list of substrings is still a list of substrings', () => {
    expect(shown(REPOS, 'checkout, nothing-matches-this')).toEqual(['checkout-web']);
  });
});

describe('the banner round trip', () => {
  /*
   * §13's *Show the N* writes names into the box and the box parses them back
   * out. These are the two halves of one contract: if they disagree, the
   * button shows a different set than the banner named, in the one moment the
   * user is being told something went wrong.
   */
  it('shows exactly the repos the banner named', () => {
    const failed = ['billing-worker', 'identity'];
    expect(shown(REPOS, filterForNames(failed))).toEqual(failed);
  });

  it('survives a single failure as well as several', () => {
    expect(shown(REPOS, filterForNames(['identity']))).toEqual(['identity']);
  });
});
