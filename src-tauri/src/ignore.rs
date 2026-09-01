//! Appending patterns to a repo's root `.gitignore` (SPEC.md §5.2).
//!
//! The one write in the app that is not a git command. Corgit shells out for
//! everything else (§3) precisely so credential helpers, hooks and LFS keep
//! working, but there is no `git ignore` to shell out to — `git check-ignore`
//! only *asks*. So this edits a text file, which makes the file's existing
//! contents something to preserve rather than something git will merge for us.
//!
//! Acquiring that repo's write-queue lock (§7) is the caller's job, in
//! `lib.rs`, exactly as it is for `commit.rs`. It matters as much here as
//! there despite no git process being involved: a read-modify-write of a file
//! two windows can both reach is the same race as two `git add`s.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// Always the repo root's, never the nearest one to the file. A repo may have
/// a `.gitignore` per directory and git reads them all, but a dashboard that
/// picked one by proximity would write to a file the user never opened and
/// cannot see from the row they clicked. One file, always the same file, is
/// the version of this that can be explained in a menu label.
const GITIGNORE: &str = ".gitignore";

/// Append `patterns` to the repo's root `.gitignore`, creating it if it is not
/// there, and skipping any line the file already carries.
///
/// Only ever appends. Rewriting or sorting the file would be Corgit editing
/// something the user wrote — a `.gitignore` is usually grouped and commented
/// by hand, and losing that is not recoverable from the UI that did it.
pub fn append(repo: &Path, patterns: &[String]) -> Result<(), String> {
    if patterns.is_empty() {
        return Ok(());
    }

    let target = repo.join(GITIGNORE);
    let existing = match fs::read_to_string(&target) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => String::new(),
        // Including invalid UTF-8, which lands here as `InvalidData`. Reading
        // it lossily and writing the result back would silently mangle bytes
        // the user has committed, so this stops instead and says so.
        Err(err) => return Err(format!("Could not read {GITIGNORE}: {err}")),
    };

    let Some(text) = with_patterns(&existing, patterns) else {
        return Ok(());
    };

    // Deliberately not `atomicfile::write`, which every other file this app
    // writes goes through (§9.5). That one renames a temp sibling over the
    // target, and the sibling lives in the directory being written to — here
    // that is the user's repo root, where a process killed mid-write would
    // strand a `.gitignore.1234.0.tmp` that shows up as an untracked row in
    // this very pane and that nothing ever cleans up. The failure it protects
    // against is a truncated `.gitignore`, which is one `git restore` away for
    // a file that is almost always tracked. Debris in someone's repo is the
    // worse of the two for an app whose whole job is showing them what is in
    // it.
    fs::write(&target, text.as_bytes()).map_err(|err| format!("Could not write {GITIGNORE}: {err}"))
}

/// The file's new contents, or `None` when every pattern is already in it and
/// there is nothing to write — an appended duplicate changes no behaviour but
/// does put a spurious modification in the diff view.
fn with_patterns(existing: &str, patterns: &[String]) -> Option<String> {
    // CRLF if the file is already CRLF: a `.gitignore` last touched by Notepad
    // is, and appending LF lines to it makes every tool that reads the file
    // report mixed endings — including Corgit's own diff view, two panes away.
    let newline = if existing.contains("\r\n") { "\r\n" } else { "\n" };

    let mut text = existing.to_string();
    let mut appended = false;

    for pattern in patterns {
        if already_ignored(&text, pattern) {
            continue;
        }
        // A file that does not end in a newline would otherwise get the first
        // pattern welded onto its last line, turning two rules into one that
        // matches nothing. Checked inside the loop against the growing text so
        // it is right for an empty file, an unterminated one, and a normal one
        // alike.
        if !text.is_empty() && !text.ends_with('\n') {
            text.push_str(newline);
        }
        text.push_str(pattern);
        text.push_str(newline);
        appended = true;
    }

    appended.then_some(text)
}

/// Whether this exact line is already in the file. Line-by-line and exact,
/// never an attempt to work out whether some *other* pattern already covers
/// this path: that is `git check-ignore`'s job and it needs a process, and a
/// wrong answer here would drop a rule the user asked for on the floor.
///
/// `str::lines` strips the `\r` of a CRLF file; `trim_end` catches the
/// trailing whitespace git ignores anyway.
fn already_ignored(text: &str, pattern: &str) -> bool {
    text.lines().any(|line| line.trim_end() == pattern.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(name);
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn patterns(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| (*line).to_string()).collect()
    }

    /// The whole reason `with_patterns` is separate from `append`: everything
    /// interesting about this module is string surgery on a file someone else
    /// wrote, and none of it needs a filesystem to test.
    #[test]
    fn appends_to_a_file_that_ends_in_a_newline() {
        let text = with_patterns("target/\n", &patterns(&["/notes.txt"])).unwrap();
        assert_eq!(text, "target/\n/notes.txt\n");
    }

    /// Without the guard the two rules become `target//notes.txt`, one line
    /// that matches nothing — and the user is looking at a menu entry that
    /// reported success.
    #[test]
    fn terminates_a_file_that_does_not_end_in_a_newline() {
        let text = with_patterns("target/", &patterns(&["/notes.txt"])).unwrap();
        assert_eq!(text, "target/\n/notes.txt\n");
    }

    #[test]
    fn starts_an_empty_file_without_a_leading_blank_line() {
        let text = with_patterns("", &patterns(&["/notes.txt", "*.log"])).unwrap();
        assert_eq!(text, "/notes.txt\n*.log\n");
    }

    #[test]
    fn keeps_a_crlf_file_crlf() {
        let text = with_patterns("target/\r\n", &patterns(&["/notes.txt"])).unwrap();
        assert_eq!(text, "target/\r\n/notes.txt\r\n");
    }

    #[test]
    fn terminates_an_unterminated_crlf_file_with_crlf() {
        let text = with_patterns("a/\r\nb/", &patterns(&["/c.txt"])).unwrap();
        assert_eq!(text, "a/\r\nb/\r\n/c.txt\r\n");
    }

    #[test]
    fn skips_a_pattern_the_file_already_carries() {
        assert!(with_patterns("target/\n/notes.txt\n", &patterns(&["/notes.txt"])).is_none());
    }

    /// A CRLF file's lines carry a trailing `\r`, so a naive comparison would
    /// never match and every ignore would append a duplicate.
    #[test]
    fn matches_an_existing_line_across_line_endings_and_trailing_space() {
        assert!(with_patterns("/notes.txt\r\n", &patterns(&["/notes.txt"])).is_none());
        assert!(with_patterns("/notes.txt  \n", &patterns(&["/notes.txt"])).is_none());
    }

    #[test]
    fn appends_only_the_patterns_that_are_missing() {
        let text = with_patterns("/a.txt\n", &patterns(&["/a.txt", "/b.txt"])).unwrap();
        assert_eq!(text, "/a.txt\n/b.txt\n");
    }

    /// Comments and blank lines are structure the user put there by hand, and
    /// the file is only ever appended to, so they come back out untouched.
    #[test]
    fn leaves_existing_comments_and_spacing_alone() {
        let existing = "# build output\ntarget/\n\n# editors\n.vscode/\n";
        let text = with_patterns(existing, &patterns(&["/notes.txt"])).unwrap();
        assert_eq!(text, format!("{existing}/notes.txt\n"));
    }

    #[test]
    fn creates_the_file_when_the_repo_has_none() {
        let dir = TempDir::new("corgit-test-ignore-create");
        append(&dir.0, &patterns(&["/notes.txt"])).unwrap();
        assert_eq!(fs::read_to_string(dir.0.join(GITIGNORE)).unwrap(), "/notes.txt\n");
    }

    /// The counterpart to `atomicfile`'s temp files, and the reason this
    /// module does not use it: nothing but `.gitignore` may appear in the repo
    /// root, or the next status sweep shows the user a file Corgit left there.
    #[test]
    fn leaves_nothing_else_in_the_repo_root() {
        let dir = TempDir::new("corgit-test-ignore-debris");
        append(&dir.0, &patterns(&["/notes.txt"])).unwrap();
        append(&dir.0, &patterns(&["*.log"])).unwrap();

        let entries: Vec<_> =
            fs::read_dir(&dir.0).unwrap().flatten().map(|entry| entry.file_name()).collect();
        assert_eq!(entries, [GITIGNORE]);
    }

    /// An empty list is the caller's business, not the filesystem's — and it
    /// must not create a `.gitignore` in a repo that had none.
    #[test]
    fn an_empty_pattern_list_writes_nothing() {
        let dir = TempDir::new("corgit-test-ignore-empty");
        append(&dir.0, &[]).unwrap();
        assert!(!dir.0.join(GITIGNORE).exists());
    }

    /// A second ignore of the same row — a double-click on the menu entry, or
    /// two windows on one root — must not rewrite the file: a no-op write
    /// still bumps the mtime and wakes every watcher on it (§6).
    #[test]
    fn re_ignoring_the_same_pattern_does_not_touch_the_file() {
        let dir = TempDir::new("corgit-test-ignore-idempotent");
        append(&dir.0, &patterns(&["/notes.txt"])).unwrap();

        let target = dir.0.join(GITIGNORE);
        let before = fs::metadata(&target).unwrap().modified().unwrap();

        append(&dir.0, &patterns(&["/notes.txt"])).unwrap();
        assert_eq!(fs::metadata(&target).unwrap().modified().unwrap(), before);
        assert_eq!(fs::read_to_string(&target).unwrap(), "/notes.txt\n");
    }
}
