import { describe, expect, it } from 'vitest';

import { exactPattern, ignoreCandidates } from './ignorePatterns';

/**
 * These cover the two ways this can go wrong quietly. A pattern that is *too
 * narrow* does nothing and the user sees the row stay put, which is annoying
 * but obvious. A pattern that is *too broad* hides files nobody asked to hide,
 * in a file that gets committed and then applies to everyone on the repo —
 * and nothing on screen says it happened. The escaping tests are the second
 * kind: `logs[1].txt` written verbatim is a character class.
 */

function patterns(path: string): string[] {
  return ignoreCandidates(path).map((candidate) => candidate.pattern);
}

function labels(path: string): string[] {
  return ignoreCandidates(path).map((candidate) => candidate.label);
}

describe('exactPattern', () => {
  it('anchors to the repo root', () => {
    expect(exactPattern('docs/notes.txt')).toBe('/docs/notes.txt');
  });

  it('anchors a name that would otherwise be a comment or a negation', () => {
    // Unanchored these would be `#notes.txt` (a comment, so the file stays
    // visible) and `!notes.txt` (a negation, which can un-ignore something
    // matched by an earlier rule — a line that does the opposite of its label).
    expect(exactPattern('#notes.txt')).toBe('/#notes.txt');
    expect(exactPattern('!notes.txt')).toBe('/!notes.txt');
  });

  it('escapes the characters git reads as glob syntax', () => {
    expect(exactPattern('logs[1].txt')).toBe('/logs\\[1].txt');
    expect(exactPattern('a?b.txt')).toBe('/a\\?b.txt');
    expect(exactPattern('star*.txt')).toBe('/star\\*.txt');
  });

  it('escapes a backslash once, not twice', () => {
    // The replacement introduces backslashes of its own, so a class that
    // handled `\` after `*` would escape the escapes.
    expect(exactPattern('a\\b.txt')).toBe('/a\\\\b.txt');
  });

  it('escapes a trailing space, which git would otherwise strip', () => {
    expect(exactPattern('trailing ')).toBe('/trailing\\ ');
  });
});

describe('ignoreCandidates', () => {
  it('offers the file, its extension and its folder', () => {
    expect(patterns('docs/notes.txt')).toEqual(['/docs/notes.txt', '*.txt', '/docs/']);
    expect(labels('docs/notes.txt')).toEqual(['notes.txt', '*.txt', 'docs/']);
  });

  it('offers the parent and the top-level folder when they differ', () => {
    // The `node_modules` case §5.2's `-uall` creates: the parent alone would
    // leave the other few hundred rows exactly where they were.
    expect(patterns('node_modules/react/index.js')).toEqual([
      '/node_modules/react/index.js',
      '*.js',
      '/node_modules/react/',
      '/node_modules/',
    ]);
  });

  it('lists the narrow folder before the broad one', () => {
    const folders = labels('a/b/c/d.txt').filter((label) => label.endsWith('/'));
    expect(folders).toEqual(['a/b/c/', 'a/']);
  });

  it('offers no folder for a file at the repo root', () => {
    expect(patterns('README.md')).toEqual(['/README.md', '*.md']);
  });

  it('does not treat a dotfile name as an extension', () => {
    // `*.env` would match `production.env` too — a pattern for a suffix that
    // is really the whole name.
    expect(patterns('.env')).toEqual(['/.env']);
    expect(patterns('config/.env')).toEqual(['/config/.env', '/config/']);
  });

  it('takes the last extension of a multi-part one', () => {
    expect(labels('archive.tar.gz')).toEqual(['archive.tar.gz', '*.gz']);
  });

  it('offers no extension for a name ending in a dot', () => {
    expect(patterns('weird.')).toEqual(['/weird.']);
  });

  it('escapes folder patterns too, and keeps the trailing slash outside', () => {
    // The slash is punctuation this code adds, not part of the name, so it
    // must land after the escaping rather than be fed through it.
    expect(patterns('build[x]/out.txt')).toEqual([
      '/build\\[x]/out.txt',
      '*.txt',
      '/build\\[x]/',
    ]);
  });
});
