//! Turning a repo-relative path from the frontend into one on disk (audit S5).
//!
//! Every path a file row carries is relative to its repo — it came out of
//! `git status` — and three commands join one onto the repo root: the
//! untracked-file diff (which *reads the file and returns its contents*),
//! Open in VS Code, and Reveal in File Explorer. `Path::join` is the wrong tool
//! for that on its own: an absolute argument *replaces* the base rather than
//! extending it, and on Windows so does a root-relative one (`\Windows\x`
//! keeps only the drive) or a drive-relative one (`C:x`). `..` needs no special
//! form at all. Git never emits any of these, so a path that has one did not
//! come from a file row, and the right answer is to refuse it rather than
//! clean it up.
//!
//! Lexical, deliberately — no `canonicalize`. That would resolve symlinks, and
//! an untracked symlink inside the repo pointing elsewhere is a real file row
//! git does report; deciding what that row may show is a different question
//! from whether the frontend can name a path git never reported.

use std::path::{Component, Path, PathBuf};

/// `root` joined with `rel`, or an error if `rel` would land anywhere other
/// than under `root`.
pub fn join(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let inside = Path::new(rel)
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if !inside {
        return Err(format!("{rel} is not a path inside the repository"));
    }
    Ok(root.join(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> &'static Path {
        Path::new(r"C:\dev\code\repo")
    }

    /// What `git status` actually sends — forward slashes on every platform.
    #[test]
    fn a_path_from_git_status_is_joined() {
        assert_eq!(join(root(), "src/a.rs").unwrap(), root().join("src/a.rs"));
        assert_eq!(join(root(), "notes.txt").unwrap(), root().join("notes.txt"));
        assert_eq!(join(root(), "./notes.txt").unwrap(), root().join("./notes.txt"));
    }

    /// The case S5 was written about: `join` with an absolute argument
    /// discards the repo root entirely.
    #[test]
    fn an_absolute_path_is_refused() {
        assert!(join(root(), r"C:\Windows\win.ini").is_err());
        assert!(join(root(), "/etc/hosts").is_err());
        assert!(join(root(), r"\\server\share\f").is_err());
    }

    /// The two Windows forms that are not absolute and escape anyway —
    /// neither looks like an absolute path to anyone checking for `C:\`.
    #[test]
    fn a_root_relative_or_drive_relative_path_is_refused() {
        assert!(join(root(), r"\Windows\win.ini").is_err());
        assert!(join(root(), "C:Windows").is_err());
    }

    /// Including the one that starts inside and walks out, which is the shape
    /// a prefix check (`starts_with(root)`) on the joined path would miss —
    /// `C:\dev\code\repo\a\..\..\x` starts with the root and is not under it.
    #[test]
    fn a_parent_step_is_refused_wherever_it_appears() {
        assert!(join(root(), r"..\secret").is_err());
        assert!(join(root(), "a/../../x").is_err());
    }
}
