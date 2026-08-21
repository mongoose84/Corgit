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

/**
 * How loudly this failure is shown (§13) — decided by what the user has to do
 * about it, never by which pane ran the command.
 *
 * `error`   the operation failed and the repo is unchanged; dismissible.
 * `blocking` the repo is in a state git will not leave on its own. The banner
 *            renders the condition, so it has no *Dismiss* and no suppression
 *            checkbox: both would only produce a UI that disagrees with the
 *            repository until the next sweep repaints it.
 *
 * There is no `warning` member. Warnings are badge-only (§13) and never reach
 * a translation — nothing here can produce one, so nothing here can name one.
 */
export type GitErrorTier = 'error' | 'blocking';

export interface TranslatedGitError {
  /** The matched rule's id, or `null` when nothing matched.
   *
   *  This is what *Don't show this again* keys on, and the null carries real
   *  weight: a failure Corgit has no rule for cannot be suppressed, because
   *  there is no id to suppress it by. §13 wants exactly that, and getting it
   *  from the shape of the data beats getting it from a rule someone has to
   *  remember. */
  id: string | null;
  /** Plain-language headline, or the raw text itself when nothing matched. */
  message: string;
  action: GitErrorAction;
  tier: GitErrorTier;
  /** The untranslated stderr, always available for a "Details" disclosure. */
  raw: string;
}

interface Rule {
  /** Stable across git versions and across rewordings of `message` — it is
   *  persisted in settings as a suppression, so renaming one silently un-mutes
   *  whatever the user had muted. */
  id: string;
  match: (lower: string) => boolean;
  message: string;
  action: GitErrorAction;
  tier: GitErrorTier;
}

const RULES: Rule[] = [
  {
    // Both Corgit giving up on a wedged git process (`git.rs`'s budgets) and
    // git's own "Connection timed out" land here. The advice is the same
    // either way, and so is the only useful next step.
    id: 'timed-out',
    match: (lower) => lower.includes('timed out'),
    message: 'Git stopped responding, so Corgit cancelled it',
    action: 'retry',
    tier: 'error',
  },
  {
    id: 'non-fast-forward',
    match: (lower) => lower.includes('non-fast-forward') || lower.includes('fetch first'),
    message: "Remote has commits you don't have",
    action: 'pull',
    tier: 'error',
  },
  {
    id: 'index-lock',
    match: (lower) => lower.includes('unable to create') && lower.includes('index.lock'),
    message: 'Another git process is running',
    action: 'retry',
    tier: 'error',
  },
  {
    id: 'dirty-tree',
    match: (lower) =>
      lower.includes('please commit your changes or stash them') ||
      (lower.includes('your local changes') && lower.includes('would be overwritten')),
    message: 'Commit or discard your changes first',
    action: 'open-vscode',
    tier: 'error',
  },
  {
    // A merge that stopped in conflict (§13). Below the dirty-tree rule on
    // purpose: a merge git *refused* never reached the working tree, and
    // "commit or discard your changes first" is the more actionable of the
    // two headlines when both could match. The way out is the middle pane's
    // conflict banner — *Abort merge* — which is already up by the time this
    // renders, so the action here only offers the banner's other half.
    id: 'merge-conflict',
    match: (lower) => lower.includes('automatic merge failed') || lower.includes('conflict ('),
    message: 'Merge stopped with conflicts',
    action: 'open-vscode',
    // The only blocking rule: git has left the tree half-merged and will not
    // move on until someone resolves or aborts. The banner adds *Abort merge*
    // as the primary and drops *Dismiss*, which is what the comment above
    // means by "the banner's other half".
    tier: 'blocking',
  },
];

export function translateGitError(raw: string): TranslatedGitError {
  const lower = raw.toLowerCase();
  const rule = RULES.find((candidate) => candidate.match(lower));
  return rule
    ? { id: rule.id, message: rule.message, action: rule.action, tier: rule.tier, raw }
    // Unmatched: git's own words, no action invented, and no id — so the
    // banner draws no suppression checkbox. §13's "checkout blocked by local
    // changes" is deliberately one of these.
    : { id: null, message: raw, action: null, tier: 'error', raw };
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
