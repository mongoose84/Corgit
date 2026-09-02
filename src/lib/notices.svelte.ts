/**
 * The error banner's state (SPEC.md §13).
 *
 * §13 splits failures into three tiers and this store owns exactly one of
 * them. The split is worth restating here, because which tier a failure is
 * decides *where its state lives*, not merely how it is painted:
 *
 * - **Warning** — a row badge, owned by `repos.svelte.ts`. Never reaches here.
 * - **Error** — an operation failed and the repo is unchanged. Nothing in the
 *   repository records that it happened, so the event has to be held
 *   somewhere: that is `raised` below.
 * - **Blocking** — the repo is in a state git will not leave on its own. That
 *   *is* recorded in the repository, so it is derived from status rather than
 *   stored, and it must be: a conflict created in a terminal, or one that
 *   outlived a restart, raised no event here and still has to put the banner
 *   up. It is therefore not in this store at all — `hasConflict` in
 *   `repos.svelte.ts` is the predicate, alongside `isDirty` and `needsPublish`
 *   for the same reason those are shared, and `App.svelte` derives the banner
 *   from it. Keeping it out also keeps this module from importing `repos`,
 *   which imports this one.
 *
 * The practical consequence is that a blocking banner cannot be dismissed and
 * cannot go stale, and neither property needed enforcing.
 */

import { settings } from './settings.svelte';
import { translateGitError, type GitErrorAction, type TranslatedGitError } from './gitErrors';

export interface RaisedNotice {
  /** The repo the failure belongs to, or `null` for one that belongs to no
   *  single repo. The banner names it: row-level Pull (§5.1) can fail in a
   *  repo that is not selected, and an unattributed headline is ambiguous
   *  across a 77-row list. */
  repoId: string | null;
  /** What the user asked for — "Push", "Pull". Matches the `operation` the
   *  backend records in its Problems ring, so the banner and the list say the
   *  same word about the same failure. */
  operation: string;
  translated: TranslatedGitError;
  /** Re-runs the exact operation that failed. Carried on the notice rather
   *  than reconstructed from `operation`, because the arguments are gone by
   *  then — §13's `index.lock` case wants *this* commit retried, not a guess
   *  at which command the word "Commit" meant. */
  retry?: () => void;
  /** Overrides the action the rule would suggest, for the caller that knows
   *  something the stderr does not carry. §8.3's dirty-tree checkout failure
   *  is the whole reason it exists: git's refusal there stays untranslated by
   *  §13's own instruction, but the pane knows the tree was dirty at the
   *  moment it failed and can therefore still offer *Open in VS Code*. */
  forceAction?: GitErrorAction;
  /** A bulk run's failed repo names, as the filter box would take them (§5.1).
   *  Present only on a run summary, and what makes the banner offer *Show the
   *  N* — carried as data rather than as a callback so the notice stays a
   *  plain description of what happened, with the acting left to the chrome
   *  that renders it. */
  repoFilter?: string;
}

class NoticeStore {
  /**
   * At most one raised error at a time, newest wins.
   *
   * A stack was the obvious alternative and is wrong for this app: the banner
   * is app chrome above a three-pane layout whose whole promise is density
   * (§1), and a background sweep failing across a herd could push five of them
   * on screen at once. Nothing is lost by holding one — every failure is in
   * Recent Problems (§13) whether its banner was shown, replaced, dismissed or
   * suppressed. That list is what makes keeping only the newest defensible.
   */
  raised = $state<RaisedNotice | null>(null);

  /**
   * Report a failed operation. Called from the store methods that own the
   * writes, not from components, so a failure surfaces identically whether it
   * came from the row, the button or the menu — which is the defect §13 exists
   * to fix.
   */
  raise(repoId: string | null, operation: string, error: string, retry?: () => void): void {
    const translated = translateGitError(error);

    // A blocking failure is *already* on screen by the time this runs:
    // `write_and_refresh` republishes the repo's status regardless of outcome,
    // so `hasConflict` has already picked the conflict up. Storing a second
    // copy here would put two banners on one condition, and the stored one
    // would be the one that could go stale.
    if (translated.tier === 'blocking') return;

    // Suppression silences the banner and nothing else — the row badge still
    // marks the repo and the backend has already recorded the problem. §13:
    // a notification may be suppressed, a condition may never be.
    if (translated.id !== null && settings.isSuppressed(translated.id)) return;

    this.raised = { repoId, operation, translated, retry };
  }

  /**
   * A bulk run's summary (§5.1's *Pull all*). One banner for the run, not one
   * per failed repo — §13's rule is that the banner holds the newest failure,
   * and a run that fails in three repos would otherwise show the third and
   * silently drop the other two.
   *
   * Deliberately **not** routed through `translateGitError`. Every other
   * caller hands this store git's stderr and wants a headline picked out of
   * it; this one has already written the sentence, and passing Corgit's own
   * prose through rules aimed at git's would let a rule matching, say,
   * "conflict" rewrite a summary that merely mentions one. `raw` carries the
   * per-repo stderr for the *Details* disclosure, so nothing is truncated at
   * the boundary — and each failed repo still has its own `!` badge and its
   * own Problems record, which is where §13 actually keeps them.
   *
   * `id: null` because there is nothing here to suppress: "don't show this
   * again" applies to a recurring condition, and this is a report on one run
   * the user started.
   */
  raiseRunSummary(operation: string, message: string, raw: string, repoFilter: string): void {
    this.raised = {
      repoId: null,
      operation,
      translated: { id: null, message, action: null, tier: 'error', raw },
      repoFilter,
    };
  }

  dismiss(): void {
    this.raised = null;
  }

  /** Narrow the action on whatever was just raised. Separate from `raise` so
   *  the store methods that own the writes do not have to take a parameter
   *  only one caller in the app ever has an answer for. */
  overrideAction(action: GitErrorAction): void {
    if (this.raised !== null) this.raised = { ...this.raised, forceAction: action };
  }

  /** *Don't show this again*. Only ever reachable on a notice that matched a
   *  rule — an unrecognised failure has no id and the checkbox is not drawn. */
  suppress(): void {
    const id = this.raised?.translated.id;
    if (id != null) settings.suppress(id);
    this.dismiss();
  }
}

export const notices = new NoticeStore();
