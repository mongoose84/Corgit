import { describe, expect, test } from 'vitest';

import { validateBranchName } from './branchName';

/**
 * These rules mirror `git check-ref-format --branch`, which stays the
 * authority — this only lets the dialog answer while the name is being typed.
 * So the thing worth testing is not "does it match git exactly" (it cannot,
 * and does not claim to) but that it never *accepts* something git will
 * reject, since that is the case where the dialog closes and the failure
 * arrives too late to be useful.
 */

const NO_EXISTING: readonly string[] = [];

function rejects(name: string): boolean {
  return validateBranchName(name, NO_EXISTING) !== null;
}

describe('names the dialog accepts', () => {
  test('ordinary names, including slashes and dots mid-name', () => {
    expect(validateBranchName('feature-x', NO_EXISTING)).toBeNull();
    expect(validateBranchName('feature/retry-logic', NO_EXISTING)).toBeNull();
    expect(validateBranchName('release/v1.2.3', NO_EXISTING)).toBeNull();
    expect(validateBranchName('fix_123', NO_EXISTING)).toBeNull();
  });

  test('surrounding whitespace is trimmed rather than rejected', () => {
    expect(validateBranchName('  feature-x  ', NO_EXISTING)).toBeNull();
  });

  test('an empty box is not an error, just not submittable yet', () => {
    // The dialog distinguishes these: null means "no complaint to show",
    // and the Create button is gated on the name being non-empty separately.
    expect(validateBranchName('', NO_EXISTING)).toBeNull();
    expect(validateBranchName('   ', NO_EXISTING)).toBeNull();
  });
});

describe('names git would reject', () => {
  test('characters git forbids outright', () => {
    for (const name of ['has space', 'a~b', 'a^b', 'a:b', 'a?b', 'a*b', 'a[b', 'a\\b']) {
      expect(rejects(name)).toBe(true);
    }
  });

  test('control characters', () => {
    // Built rather than written literally: a raw control byte in a source
    // file is invisible in review and mangled by half the tools that touch it.
    const control = (code: number) => 'a' + String.fromCharCode(code) + 'b';
    expect(rejects(control(0x00))).toBe(true);
    expect(rejects(control(0x1f))).toBe(true);
    expect(rejects(control(0x7f))).toBe(true);
  });

  test('sequences with special meaning to git', () => {
    expect(rejects('a..b')).toBe(true);
    expect(rejects('a@{b')).toBe(true);
    expect(rejects('@')).toBe(true);
  });

  test('malformed path segments', () => {
    expect(rejects('/leading')).toBe(true);
    expect(rejects('trailing/')).toBe(true);
    expect(rejects('a//b')).toBe(true);
    expect(rejects('.hidden')).toBe(true);
    expect(rejects('a/.hidden')).toBe(true);
    expect(rejects('a/b.lock')).toBe(true);
  });

  test('names ending in a dot or .lock', () => {
    expect(rejects('trailing.')).toBe(true);
    expect(rejects('branch.lock')).toBe(true);
  });

  /**
   * Load-bearing beyond ref-format: a name starting with `-` reaches git as
   * an *option*, not an argument (`git switch -c <name> <start>` has no `--`
   * to stop parsing). Until the backend rejects these too, this check is the
   * only thing between the dialog and a misparsed command line.
   */
  test('a leading dash, which git would read as an option', () => {
    expect(rejects('-D')).toBe(true);
    expect(rejects('--orphan')).toBe(true);
    expect(rejects('-')).toBe(true);
  });
});

describe('collisions with an existing branch', () => {
  test('an existing local name is refused with its own message', () => {
    const error = validateBranchName('main', ['main', 'develop']);

    expect(error).toBe('A branch named "main" already exists');
  });

  test('the comparison is exact, not case-insensitive or fuzzy', () => {
    // Git refs are case-sensitive, so `Main` alongside `main` is legal even
    // though it is a poor idea — refusing it here would block a valid name.
    expect(validateBranchName('Main', ['main'])).toBeNull();
  });

  test('the collision check runs on the trimmed name', () => {
    expect(validateBranchName('  main  ', ['main'])).not.toBeNull();
  });
});
