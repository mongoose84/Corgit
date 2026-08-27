/**
 * Turning a file row into a `.gitignore` line (SPEC.md §5.2).
 *
 * Kept here rather than inline in `CommitPane` for the same reason
 * `fileSelection` is: the rendering is not the interesting part. The
 * interesting part is that a menu label and the line that gets appended must
 * describe the same thing, and gitignore's pattern language is not the path
 * language — `logs[1].txt` written verbatim is a character class matching
 * nothing at all, and the user would be looking at a menu entry that quietly
 * did nothing. One function produces both halves so they cannot drift.
 *
 * Every pattern this produces is **anchored** with a leading `/`, so it means
 * the path from the repo root rather than "any file of that name at any
 * depth" — the row the user right-clicked is one file, and ignoring six others
 * they cannot see is not what the label says. The one exception is the
 * extension pattern, which is deliberately repo-wide.
 *
 * The anchor pays for itself twice: a line starting with `#` is a comment and
 * one starting with `!` is a negation, so a file literally named `#notes.txt`
 * would otherwise produce a line that either vanishes or un-ignores something.
 * Behind a leading `/` neither character is ever first.
 */

export interface IgnoreCandidate {
  /** What the menu says is being ignored, e.g. `notes.txt`, `*.txt`, `docs/`.
   *  Not the pattern itself: `/docs/notes.txt` reads as a path someone typed
   *  wrong, and the row above it already says which file this is about. */
  label: string;
  /** The exact line appended to `.gitignore`. */
  pattern: string;
}

/** Characters git's matcher reads as syntax rather than as part of a name, and
 *  which a literal path therefore has to escape. `]` is deliberately absent:
 *  outside a bracket expression git treats it as an ordinary character, and
 *  with `[` escaped there is never one open. `\` is first in the class because
 *  the replacement itself introduces backslashes — escaping it last would
 *  double the ones this pass just added. */
const SPECIAL = /[\\*?[]/g;

/** A trailing space is stripped by git unless it is escaped, so a file whose
 *  name ends in one would get a pattern that matches a *different* name.
 *  Windows will not create such a name (§10 — v1 is Windows-only), which is
 *  exactly why this is handled here rather than trusted to never happen: the
 *  day Corgit reads a repo that came from elsewhere, this is not the bug
 *  anyone would think to look for. */
function escapeLiteral(text: string): string {
  return text.replace(SPECIAL, '\\$&').replace(/ $/, '\\ ');
}

/** The line that ignores exactly this file and nothing else. Exported on its
 *  own because a multi-row selection gets only this form — there is no honest
 *  single extension or folder for six files picked out of a list. */
export function exactPattern(path: string): string {
  return `/${escapeLiteral(path)}`;
}

/**
 * The ignore entries offered for a *single* untracked row, narrowest first, so
 * the menu reads as an escalation and the broadest thing it can do is never
 * the first thing under the pointer.
 *
 * 1. the file itself;
 * 2. its extension, repo-wide — the one unanchored pattern here, because
 *    `*.log` anchored to the root would be the least useful reading of it;
 * 3. its folder, and then its top-level folder when that is a different one.
 *
 * The folder entries exist because of §5.2's "never a folder row": status is
 * read with `-uall`, so a wholly-untracked `node_modules` arrives as several
 * hundred individual file rows. Offering only the file and the extension would
 * mean ignoring that one row at a time, which is not a feature. The top-level
 * entry is what actually answers that case — `node_modules/react/index.js` has
 * an immediate parent of `node_modules/react/`, which solves nothing — and it
 * is offered *as well as* the parent rather than instead of it, because the
 * same shape reaches `src/generated/out.js`, where the broad reading would
 * ignore the whole source tree. Both are on the menu, both say in full what
 * they cover, and the narrow one is the one on top.
 */
export function ignoreCandidates(path: string): IgnoreCandidate[] {
  const segments = path.split('/');
  const name = segments[segments.length - 1];

  const candidates: IgnoreCandidate[] = [{ label: name, pattern: exactPattern(path) }];

  // `dot > 0` excludes a dotfile: `.env`'s only dot is its first character, and
  // `*.env` would be a pattern for a suffix that is really the whole name. The
  // upper bound excludes a trailing dot, whose "extension" is empty.
  const dot = name.lastIndexOf('.');
  if (dot > 0 && dot < name.length - 1) {
    const extension = name.slice(dot);
    candidates.push({ label: `*${extension}`, pattern: `*${escapeLiteral(extension)}` });
  }

  // A file at the repo root has no folder to offer, and `segments.length` is 1.
  // The trailing slash on each pattern means "a directory of this name", not
  // whatever happens to match — the difference is real for a `dist` that is a
  // folder in one repo and a build script in another.
  for (const depth of folderDepths(segments.length - 1)) {
    const folder = segments.slice(0, depth).join('/');
    candidates.push({ label: `${folder}/`, pattern: `${exactPattern(folder)}/` });
  }

  return candidates;
}

/** Which ancestor folders to offer, given how many the path has. One folder
 *  deep, the parent and the top-level folder are the same directory and must
 *  not be listed twice; deeper, the parent comes first. Nothing between the two
 *  is offered — a menu that grew a line per path segment would be a folder
 *  picker, and the two ends are the two questions anyone actually has. */
function folderDepths(folders: number): number[] {
  if (folders <= 0) return [];
  if (folders === 1) return [1];
  return [folders, 1];
}
