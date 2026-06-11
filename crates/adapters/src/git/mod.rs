//! [`GitCliAdapter`] — the [`GitPort`] implementation backed by the `git` CLI (D35, WU-7).
//!
//! ## Why shell-git, not `git2`
//!
//! The design (D35) permits EITHER `git2` OR shell-git. We choose shell-git
//! (`std::process::Command`) deliberately: `diff_head` only needs the unified-diff TEXT,
//! which `git diff` already emits verbatim, so `git2` (a heavy libgit2 C build dependency
//! with its own diff-formatting API) buys us nothing here. Shell-git keeps the adapter
//! dependency surface at ZERO new crates and matches the diff format VibeLens expects
//! (raw `git diff HEAD` output). The struct is named `GitCliAdapter` (honest) rather than
//! the tasks.md placeholder `Git2Adapter`.
//!
//! ## Empty-repo handling
//!
//! `git diff HEAD` fails in a repo with NO commits (the `HEAD` ref is unborn). The adapter
//! detects that via `git rev-parse --verify HEAD` and falls back to diffing against git's
//! well-known EMPTY TREE object (`4b825dc642cb6eb9a060e54bf8d69288fbee4904`), so a brand-new
//! repo yields the "add-all" diff of its staged content instead of an error (diff-pipeline
//! spec: "Empty-repo MUST diff vs the empty tree").

use std::path::Path;
use std::process::Command;

use spectty_core::ports::{GitError, GitPort};

/// Git's canonical empty-tree object hash. Diffing against it in a commit-less repo yields
/// the add-all diff of the index without needing a `HEAD` commit.
const EMPTY_TREE_HASH: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// A [`GitPort`] backed by the system `git` binary (shell-git).
#[derive(Debug, Default, Clone)]
pub struct GitCliAdapter;

impl GitCliAdapter {
    /// Build the adapter. Stateless — the workspace is passed per call.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Run `git <args>` in `workspace`, returning stdout on success or a [`GitError`].
    fn run(workspace: &Path, args: &[&str]) -> Result<std::process::Output, GitError> {
        Command::new("git")
            .args(args)
            .current_dir(workspace)
            .output()
            .map_err(|e| GitError::Backend(format!("failed to spawn git: {e}")))
    }

    /// Whether the repo has a committed `HEAD` (false in a fresh, commit-less repo).
    fn has_head(workspace: &Path) -> bool {
        Self::run(workspace, &["rev-parse", "--verify", "HEAD"])
            .map(|out| out.status.success())
            .unwrap_or(false)
    }
}

impl GitPort for GitCliAdapter {
    fn diff_head(&self, workspace: &Path) -> Result<String, GitError> {
        // Pick the base: the HEAD commit if one exists, else git's empty-tree object so a
        // commit-less repo still produces the add-all diff rather than an "unborn HEAD" error.
        let base = if Self::has_head(workspace) {
            "HEAD"
        } else {
            EMPTY_TREE_HASH
        };

        let output = Self::run(workspace, &["diff", base])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::Backend(format!(
                "git diff {base} failed: {}",
                stderr.trim()
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|e| GitError::Backend(format!("git diff output was not utf-8: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// A throwaway git repo in a unique temp dir; cleaned up on drop.
    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new(label: &str) -> Self {
            let mut path = std::env::temp_dir();
            let unique = format!(
                "spectty-git-{label}-{}-{:?}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            path.push(unique);
            fs::create_dir_all(&path).expect("create temp repo dir");
            Self { path }
        }

        fn git(&self, args: &[&str]) {
            let status = Command::new("git")
                .args(args)
                .current_dir(&self.path)
                .output()
                .expect("run git in temp repo");
            assert!(
                status.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&status.stderr)
            );
        }

        fn init(&self) {
            self.git(&["init", "-q"]);
            // Deterministic identity so commits succeed in CI without global config.
            self.git(&["config", "user.email", "test@spectty.local"]);
            self.git(&["config", "user.name", "Spectty Test"]);
        }

        fn write(&self, name: &str, contents: &str) {
            fs::write(self.path.join(name), contents).expect("write file");
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    // WU-7.1: a populated repo with a staged change → diff_head returns the unified diff.
    #[test]
    fn git_adapter_diff_head_on_populated_repo() {
        let repo = TempRepo::new("populated");
        repo.init();
        repo.write("a.txt", "one\n");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-q", "-m", "initial"]);
        // Now modify the committed file (unstaged working-tree change).
        repo.write("a.txt", "one\ntwo\n");

        let adapter = GitCliAdapter::new();
        let diff = adapter.diff_head(&repo.path).expect("diff_head ok");

        assert!(
            diff.contains("a.txt"),
            "diff must name the changed file: {diff}"
        );
        assert!(
            diff.contains("+two"),
            "diff must show the added line: {diff}"
        );
    }

    // WU-7.2: a repo with NO commits → diff_head diffs against the empty tree (no error,
    // returns the add-all diff of the staged content).
    #[test]
    fn git_adapter_diff_head_empty_repo_uses_empty_tree() {
        let repo = TempRepo::new("emptyrepo");
        repo.init();
        repo.write("new.txt", "hello\n");
        repo.git(&["add", "."]); // stage so the empty-tree diff has content

        let adapter = GitCliAdapter::new();
        let diff = adapter
            .diff_head(&repo.path)
            .expect("diff_head on a commit-less repo must NOT error");

        assert!(
            diff.contains("new.txt"),
            "empty-repo diff vs empty tree must name the staged file: {diff}"
        );
        assert!(
            diff.contains("+hello"),
            "must show the add-all content: {diff}"
        );
    }

    // WU-7.3: a truly empty working tree (no changes) → empty diff string. The pipeline
    // (WU-8) maps this to DiffExplanation::empty().
    #[test]
    fn git_adapter_truly_empty_workspace_returns_empty_string() {
        let repo = TempRepo::new("clean");
        repo.init();
        repo.write("a.txt", "one\n");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-q", "-m", "initial"]);
        // No working-tree changes after the commit → clean.

        let adapter = GitCliAdapter::new();
        let diff = adapter.diff_head(&repo.path).expect("diff_head ok");

        assert!(
            diff.is_empty(),
            "a clean working tree must yield an empty diff string, got: {diff:?}"
        );
    }

    // Object-safety / port-conformance: usable behind Arc<dyn GitPort>.
    #[test]
    fn git_adapter_is_usable_behind_arc_dyn_port() {
        use std::sync::Arc;
        let _port: Arc<dyn GitPort> = Arc::new(GitCliAdapter::new());
    }
}
