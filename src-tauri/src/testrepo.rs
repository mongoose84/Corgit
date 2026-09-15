//! A real git repository in a temp directory, for tests that cannot honestly
//! be written against a fixture.
//!
//! Every parser and every argv in this crate was, until now, tested against
//! hand-written strings. That catches an edit to the code; it cannot catch a
//! misreading of git, because the fixture was written from the same
//! understanding that produced the parser. The flag constants are the sharpest
//! case: `DISCARD_FLAGS` is asserted equal to `["--worktree", "--"]`, which
//! pins the *value* while saying nothing about the claim the comment above it
//! makes — that a partly-staged file keeps its staged half.
//!
//! So this exists for the beliefs rather than the code: the small number of
//! places where Corgit asserts something about what git *does*, and where
//! being wrong is silent. It is not a second test suite for things a fixture
//! already covers.
//!
//! Cheap enough to run unconditionally — a handful of `git` invocations per
//! test, and git is a hard requirement of the app (§3). Not `#[ignore]`d like
//! the benches, which need a prepared root and a release build.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

/// Keeps concurrently-running tests out of each other's directories. The
/// existing helpers (`discovery.rs`, `ignore.rs`) key on a caller-supplied
/// name alone, which is fine while one test uses each; a counter costs nothing
/// and removes the rule that the names must stay unique by inspection.
static NEXT: AtomicU32 = AtomicU32::new(0);

pub struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    /// A fresh repository with an initial commit's worth of configuration but
    /// no commits yet.
    ///
    /// The config is all about making the test independent of whoever runs it:
    /// an identity so `commit` works on a machine that has none, no signing
    /// (a developer with `commit.gpgsign = true` globally would otherwise wait
    /// on a passphrase prompt that never comes), no autocrlf so file contents
    /// compare byte for byte, and `--template=` so the user's own init
    /// templates cannot drop hooks into a repository this test then runs git
    /// in.
    pub fn new(name: &str) -> Self {
        let unique = NEXT.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("corgit-test-{name}-{unique}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("could not create the test directory");

        let repo = Self { path };
        repo.git(&["init", "--quiet", "--template=", "--initial-branch=main"]);
        repo.git(&["config", "user.email", "test@corgit.invalid"]);
        repo.git(&["config", "user.name", "Corgit Test"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.git(&["config", "core.autocrlf", "false"]);
        repo
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run git in this repo, panicking with its stderr on failure — a failed
    /// *setup* command is a broken test, not a result to assert on.
    pub fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .unwrap_or_else(|err| panic!("could not run git {args:?}: {err}"));
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub fn write(&self, rel: &str, contents: &str) {
        let target = self.path.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("could not create the parent directory");
        }
        fs::write(target, contents).expect("could not write the test file");
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path.join(rel)).expect("could not read the test file")
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.path.join(rel).exists()
    }

    /// Stage everything and commit it — the usual way a test gets to a known
    /// starting point.
    pub fn commit_all(&self, message: &str) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--message", message]);
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        // Best-effort: on Windows a lingering handle (an antivirus scan of the
        // objects just written) can hold a file open past the test. Leaving a
        // directory in %TEMP% is not worth failing a passing test over.
        let _ = fs::remove_dir_all(&self.path);
    }
}
