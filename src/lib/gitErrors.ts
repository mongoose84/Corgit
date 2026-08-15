/**
 * Plain-language translation of common git failures (SPEC.md §13).
 *
 * The backend now hands over the whole trimmed stderr rather than just its
 * first line, so "Details" always has the full raw text to show — this only
 * picks a short headline out of it, and only for the cases the table lists.
 * Checkout-blocked-by-dirty-tree is deliberately absent: §13 says to show
 * "git's own stderr" for that one, not a translation.
 */

export type GitErrorAction = 'pull' | 'open-vscode' | 'retry' | null;

export interface TranslatedGitError {
  /** Plain-language headline, or the raw text itself when nothing matched. */
  message: string;
  action: GitErrorAction;
  /** The untranslated stderr, always available for a "Details" disclosure. */
  raw: string;
}

interface Rule {
  match: (lower: string) => boolean;
  message: string;
  action: GitErrorAction;
}

const RULES: Rule[] = [
  {
    match: (lower) => lower.includes('non-fast-forward') || lower.includes('fetch first'),
    message: "Remote has commits you don't have",
    action: 'pull',
  },
  {
    match: (lower) => lower.includes('unable to create') && lower.includes('index.lock'),
    message: 'Another git process is running',
    action: 'retry',
  },
  {
    match: (lower) =>
      lower.includes('please commit your changes or stash them') ||
      (lower.includes('your local changes') && lower.includes('would be overwritten')),
    message: 'Commit or discard your changes first',
    action: 'open-vscode',
  },
];

export function translateGitError(raw: string): TranslatedGitError {
  const lower = raw.toLowerCase();
  const rule = RULES.find((candidate) => candidate.match(lower));
  return rule
    ? { message: rule.message, action: rule.action, raw }
    : { message: raw, action: null, raw };
}
