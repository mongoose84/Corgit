import { describe, expect, test } from 'vitest';

import { isUnmergedBranchRefusal, translateGitError } from './gitErrors';

/**
 * §13's rule is "never strand the user in a state Corgit can't get them out
 * of", and this module is where that is decided: a matched rule carries an
 * action, an unmatched one carries none. So the failure mode worth guarding
 * is a message that *should* have offered a way out and silently didn't —
 * which looks like a plain error string and is easy to miss by eye.
 *
 * The backend hands over whole stderr, not a first line, so every case here
 * is fed realistic multi-line text rather than a tidy sentence.
 */

describe('cases §13 lists', () => {
  test('a rejected push offers Pull', () => {
    const result = translateGitError(
      'To github.com:acme/api.git\n' +
        ' ! [rejected]        main -> main (non-fast-forward)\n' +
        "error: failed to push some refs to 'github.com:acme/api.git'",
    );

    expect(result.message).toBe("Remote has commits you don't have");
    expect(result.action).toBe('pull');
  });

  test('"fetch first" is the same case under a different wording', () => {
    const result = translateGitError(
      ' ! [rejected]        main -> main (fetch first)\nhint: Updates were rejected',
    );

    expect(result.action).toBe('pull');
  });

  test('a held index.lock offers Retry', () => {
    const result = translateGitError(
      "fatal: Unable to create '/repo/.git/index.lock': File exists.\n\n" +
        'Another git process seems to be running in this repository.',
    );

    expect(result.message).toBe('Another git process is running');
    expect(result.action).toBe('retry');
  });

  test('a pull blocked by local changes offers VS Code', () => {
    const result = translateGitError(
      'error: Your local changes to the following files would be overwritten by merge:\n' +
        '\tsrc/main.rs\nPlease commit your changes or stash them before you merge.',
    );

    expect(result.message).toBe('Commit or discard your changes first');
    expect(result.action).toBe('open-vscode');
  });

  test('a merge that stopped in conflict gets a headline of its own', () => {
    // Exactly what a conflicting `git merge` writes — and it writes all of it
    // to *stdout*, which is why `branch::merge` joins both streams: the text
    // arriving here has no stderr in it at all.
    const result = translateGitError(
      'Auto-merging src/main.rs\n' +
        'CONFLICT (content): Merge conflict in src/main.rs\n' +
        'Automatic merge failed; fix conflicts and then commit the result.',
    );

    expect(result.message).toBe('Merge stopped with conflicts');
    expect(result.action).toBe('open-vscode');
  });

  test('a merge git refused outright is the dirty-tree case, not the conflict one', () => {
    // Both rules can match text with "merge" in it, and this one has to win:
    // nothing was merged, so there is no conflict to abort — the user has to
    // deal with their working tree first.
    const result = translateGitError(
      'error: Your local changes to the following files would be overwritten by merge:\n' +
        '\tsrc/main.rs\nPlease commit your changes or stash them before you merge.\nAborting',
    );

    expect(result.message).toBe('Commit or discard your changes first');
  });
});

describe('timeouts', () => {
  /**
   * The exact wording `git.rs` produces when it kills a process at its budget.
   * A Rust test asserts the message keeps the "timed out" substring; this is
   * the other end of that contract.
   */
  test('a git process Corgit killed offers Retry', () => {
    const result = translateGitError('git timed out after 120s and was stopped');

    expect(result.message).toBe('Git stopped responding, so Corgit cancelled it');
    expect(result.action).toBe('retry');
  });

  test("git's own connection timeout lands in the same place", () => {
    // Different origin, same advice — and Retry is the only useful next step
    // for either, which is why one rule covers both.
    const result = translateGitError(
      "ssh: connect to host github.com port 22: Connection timed out\n" +
        'fatal: Could not read from remote repository.',
    );

    expect(result.action).toBe('retry');
  });
});

describe('everything else', () => {
  test('an unrecognised failure is shown verbatim with no action invented', () => {
    // §13 deliberately has no translation for a blocked checkout — git's own
    // stderr is the message. Inventing an action here would be worse than none.
    const raw = "error: pathspec 'nope' did not match any file(s) known to git";
    const result = translateGitError(raw);

    expect(result.message).toBe(raw);
    expect(result.action).toBeNull();
  });

  test('matching is case-insensitive, since git is not consistent about case', () => {
    expect(translateGitError('FATAL: Authentication failed').raw).toBe(
      'FATAL: Authentication failed',
    );
    expect(translateGitError('Unable To Create index.lock').action).toBe('retry');
  });

  test('raw stderr survives translation for the Details disclosure', () => {
    // §13: "raw stderr always available in a collapsible Details" — a
    // translated headline must never be the only thing left.
    const raw = ' ! [rejected]  main -> main (non-fast-forward)\nhint: see git-push(1)';
    const result = translateGitError(raw);

    expect(result.message).not.toBe(raw);
    expect(result.raw).toBe(raw);
  });

  test('an empty message does not pretend to be a known failure', () => {
    const result = translateGitError('');

    expect(result.message).toBe('');
    expect(result.action).toBeNull();
  });
});

describe('isUnmergedBranchRefusal', () => {
  // The one failure that changes which button the user is offered (§8.3), so
  // a miss here does not just word the error badly — it hides *Delete anyway*
  // and leaves a squash-merged branch undeletable from Corgit.
  test("matches git's refusal whatever it capitalises", () => {
    expect(isUnmergedBranchRefusal("error: The branch 'feature-x' is not fully merged.")).toBe(true);
    expect(isUnmergedBranchRefusal("error: the branch 'feature-x' is not fully merged.")).toBe(true);
  });

  test('does not match the other ways a delete fails', () => {
    expect(isUnmergedBranchRefusal("error: branch 'feature-x' not found.")).toBe(false);
    // Git 2.53's wording for deleting the branch you are on — the menu never
    // offers that, but the classifier must not read it as the refusal that
    // grows a *Delete anyway* button.
    expect(
      isUnmergedBranchRefusal(
        "error: cannot delete branch 'main' used by worktree at 'C:/dev/repo'",
      ),
    ).toBe(false);
    expect(isUnmergedBranchRefusal('')).toBe(false);
  });
});

describe('ids and tiers (§13)', () => {
  /**
   * Ids are persisted in settings as suppressions, so they are a stable
   * contract rather than an implementation detail: renaming one silently
   * un-mutes whatever the user had muted, and the failure is invisible — a
   * warning they told Corgit to stop showing starts showing again.
   */
  test('each translated case carries its id', () => {
    expect(translateGitError('(non-fast-forward)').id).toBe('non-fast-forward');
    expect(translateGitError("unable to create '.git/index.lock'").id).toBe('index-lock');
    expect(translateGitError('git timed out after 120s and was stopped').id).toBe('timed-out');
    expect(translateGitError('Please commit your changes or stash them').id).toBe('dirty-tree');
    expect(translateGitError('Automatic merge failed; fix conflicts').id).toBe('merge-conflict');
  });

  /**
   * The property §13 leans on rather than enforces: with no id there is
   * nothing for *Don't warn me again* to key on, so an error Corgit does not
   * recognise cannot be silenced — and the checkbox is simply not drawn.
   * Checkout-blocked-by-local-changes is deliberately one of these.
   */
  test('an unrecognised failure has no id, and so cannot be suppressed', () => {
    expect(translateGitError('error: some new thing git learned to say').id).toBeNull();
  });

  test('a stopped merge is the only blocking case', () => {
    expect(translateGitError('Automatic merge failed; fix conflicts').tier).toBe('blocking');

    // Everything else leaves the repo as it was, so its banner stays
    // dismissible and suppressible.
    for (const raw of [
      '(non-fast-forward)',
      "unable to create '.git/index.lock'",
      'git timed out after 120s and was stopped',
      'Please commit your changes or stash them',
      'error: something unrecognised',
    ]) {
      expect(translateGitError(raw).tier).toBe('error');
    }
  });

  test('a merge git refused outright is an error, not a block', () => {
    // Nothing was merged, so there is no half-finished state for git to be
    // stuck in — the dirty-tree rule wins and the banner stays dismissible.
    const result = translateGitError(
      [
        'error: Your local changes to the following files would be overwritten by merge:',
        '\tsrc/main.rs',
        'Please commit your changes or stash them before you merge.',
      ].join('\n'),
    );

    expect(result.id).toBe('dirty-tree');
    expect(result.tier).toBe('error');
  });
});
