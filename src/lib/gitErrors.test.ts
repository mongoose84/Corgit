import { describe, expect, test } from 'vitest';

import { translateGitError } from './gitErrors';

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
