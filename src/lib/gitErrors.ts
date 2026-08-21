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
    // Both Corgit giving up on a wedged git process (`git.rs`'s budgets) and
    // git's own "Connection timed out" land here. The advice is the same
    // either way, and so is the only useful next step.
    match: (lower) => lower.includes('timed out'),
    message: 'Git stopped responding, so Corgit cancelled it',
    action: 'retry',
  },
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
  {
    // A merge that stopped in conflict (§13). Below the dirty-tree rule on
    // purpose: a merge git *refused* never reached the working tree, and
    // "commit or discard your changes first" is the more actionable of the
    // two headlines when both could match. The way out is the middle pane's
    // conflict banner — *Abort merge* — which is already up by the time this
    // renders, so the action here only offers the banner's other half.
    match: (lower) => lower.includes('automatic merge failed') || lower.includes('conflict ('),
    message: 'Merge stopped with conflicts',
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

/**
 * Whether a failed `git branch -d` was git refusing to drop commits, rather
 * than any of the other ways a delete can fail (§8.3). This is the one git
 * failure Corgit answers with a *different button* instead of a headline, so
 * it is a predicate here rather than a `RULES` entry: the delete dialog uses
 * it to decide whether to offer *Delete anyway*, and everything it does not
 * match is surfaced as an ordinary write error.
 *
 * Matched on git's own wording, lowercased — the capitalisation of "The
 * branch" changed between git versions, the four words after it have not.
 */
export function isUnmergedBranchRefusal(raw: string): boolean {
  return raw.toLowerCase().includes('not fully merged');
}
