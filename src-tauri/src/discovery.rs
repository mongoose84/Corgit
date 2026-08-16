//! Depth-1 repo discovery (SPEC.md §8.1).
//!
//! Direct children of the root only: one directory read plus an `exists()` per
//! child. No recursion, no skip-list, no `node_modules` traversal — which is
//! why discovery over 77 repos is sub-millisecond and needs no progress UI.
//! Repos nested deeper are out of scope by design; open that folder as its own
//! root instead (§9.1).

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    /// Canonicalised path. Write queues are keyed by it, so the same repo
    /// reached through two overlapping roots is one repo (§9.2).
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

/// Sorted by name, case-insensitively — the list never reorders itself at
/// runtime (§5.1), so the order it is built in is the order it keeps.
pub fn scan(root: &Path) -> Vec<Repo> {
    let mut repos = Vec::new();

    // Opening a repo folder directly is the obvious mistake to make, and
    // "0 repositories" would be a puzzling answer to it. One extra exists().
    if let Some(repo) = as_repo(root) {
        repos.push(repo);
    }

    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            // is_dir() rather than the entry's file type, so a junction or
            // symlink pointing at a repo still counts.
            if path.is_dir() {
                if let Some(repo) = as_repo(&path) {
                    repos.push(repo);
                }
            }
        }
    }

    repos.sort_by_key(|repo| repo.name.to_lowercase());
    repos.dedup_by(|a, b| a.id == b.id);
    repos
}

/// A repo is any directory containing `.git` — a directory normally, a file in
/// a linked worktree or a submodule.
fn as_repo(path: &Path) -> Option<Repo> {
    if !path.join(".git").exists() {
        return None;
    }

    let canonical = canonicalize(path);
    Some(Repo {
        id: canonical.to_string_lossy().into_owned(),
        name: canonical
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            // A drive root has no file name, and is a legal thing to open.
            .unwrap_or_else(|| canonical.to_string_lossy().into_owned()),
        path: canonical,
    })
}

pub fn canonicalize(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(name);
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn repo(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(path.join(".git")).unwrap();
            path
        }

        fn plain_dir(&self, name: &str) {
            fs::create_dir_all(self.0.join(name)).unwrap();
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_child_repos_and_ignores_plain_directories() {
        let root = TempRoot::new("corgit-test-scan");
        root.repo("billing");
        root.repo("api-gateway");
        root.plain_dir("notes");

        let names: Vec<_> = scan(&root.0).into_iter().map(|r| r.name).collect();

        assert_eq!(names, vec!["api-gateway", "billing"]);
    }

    #[test]
    fn a_dot_git_file_counts_as_a_repo() {
        let root = TempRoot::new("corgit-test-worktree");
        let worktree = root.0.join("linked");
        fs::create_dir_all(&worktree).unwrap();
        fs::write(worktree.join(".git"), b"gitdir: ../main/.git/worktrees/linked").unwrap();

        assert_eq!(scan(&root.0).len(), 1);
    }

    #[test]
    fn does_not_recurse_past_depth_one() {
        let root = TempRoot::new("corgit-test-depth");
        let nested = root.0.join("outer").join("inner");
        fs::create_dir_all(nested.join(".git")).unwrap();

        assert!(scan(&root.0).is_empty());
    }

    #[test]
    fn the_root_itself_can_be_the_repo() {
        let root = TempRoot::new("corgit-test-self");
        fs::create_dir_all(root.0.join(".git")).unwrap();
        root.repo("child");

        let names: Vec<_> = scan(&root.0).into_iter().map(|r| r.name).collect();

        assert_eq!(names, vec!["child", "corgit-test-self"]);
    }

    #[test]
    fn a_missing_root_yields_nothing_rather_than_failing() {
        assert!(scan(Path::new(r"C:\corgit\definitely\not\here")).is_empty());
    }
}
