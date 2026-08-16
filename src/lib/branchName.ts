/**
 * Branch-name checking for the Create Branch dialog (§8.3).
 *
 * `git check-ref-format --branch` is the authority and still gets the final
 * say — every failure git raises surfaces as its own stderr like any other
 * write. This is only here so the dialog can say "no, and here's why" while
 * the name is being typed, instead of letting an obviously bad name make a
 * round trip to git and come back as a shell error.
 */

/** `null` means the name is usable. */
export function validateBranchName(name: string, existingLocal: readonly string[]): string | null {
  const trimmed = name.trim();
  if (trimmed.length === 0) return null; // Nothing typed yet is not an error, just not submittable.

  // The rules below are git-check-ref-format's, in the order it lists them.
  if (/[\s~^:?*[\\]/.test(trimmed)) return 'No spaces or any of ~ ^ : ? * [ \\';
  if (/[\x00-\x1f\x7f]/.test(trimmed)) return 'No control characters';
  if (trimmed.includes('..')) return 'No ".." anywhere in the name';
  if (trimmed.includes('@{')) return 'No "@{" anywhere in the name';
  if (trimmed === '@') return '"@" alone is not a branch name';
  if (trimmed.startsWith('/') || trimmed.endsWith('/')) return 'Cannot start or end with "/"';
  if (trimmed.includes('//')) return 'No empty path segment ("//")';
  if (trimmed.startsWith('-')) return 'Cannot start with "-"';
  if (trimmed.endsWith('.') || trimmed.endsWith('.lock')) return 'Cannot end with "." or ".lock"';
  if (trimmed.split('/').some((segment) => segment.startsWith('.') || segment.endsWith('.lock'))) {
    return 'No path segment may start with "." or end with ".lock"';
  }

  // Not a git rule but the most likely mistake by far, and the one whose git
  // error ("a branch named 'x' already exists") arrives after the dialog has
  // already closed.
  if (existingLocal.includes(trimmed)) return `A branch named "${trimmed}" already exists`;

  return null;
}
