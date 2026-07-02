use std::path::{Path, PathBuf};
use std::process::Command;

/// Detect the workspace root: the enclosing git repository's toplevel,
/// falling back to the current directory when not inside a git repo.
pub fn detect_workspace() -> anyhow::Result<PathBuf> {
    detect_workspace_from(&std::env::current_dir()?)
}

/// Detect the workspace root for `dir`.
///
/// Runs `git -C <dir> rev-parse --show-toplevel` and returns the reported
/// toplevel on success. On any failure (git not installed, `dir` outside a
/// repository, non-zero exit, empty output) falls back to `dir` as-is.
fn detect_workspace_from(dir: &Path) -> anyhow::Result<PathBuf> {
    Ok(git_toplevel(dir).unwrap_or_else(|| dir.to_path_buf()))
}

/// Ask git for the repository toplevel containing `dir`.
///
/// Runs without a shell, capturing stdout and stderr so nothing leaks to the
/// terminal. Returns `None` on any failure so callers can fall back quietly.
fn git_toplevel(dir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let toplevel = stdout.trim_end_matches(['\r', '\n']);
    if toplevel.is_empty() {
        return None;
    }
    Some(PathBuf::from(toplevel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// `git init` needs no user configuration as long as no commits are made.
    fn git_init(dir: &Path) {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .arg("init")
            .output()
            .expect("failed to run git init");
        assert!(
            output.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Compare canonicalized paths: temp dirs can involve symlinks
    /// (e.g. /tmp -> /private/tmp), which git resolves but we do not.
    fn canon(path: &Path) -> PathBuf {
        path.canonicalize().expect("canonicalize")
    }

    #[test]
    fn returns_repo_root_inside_git_repo() {
        let tmp = tempfile::tempdir().unwrap();
        git_init(tmp.path());

        let detected = detect_workspace_from(tmp.path()).unwrap();

        assert_eq!(canon(&detected), canon(tmp.path()));
    }

    #[test]
    fn returns_repo_root_from_nested_subdirectory() {
        let tmp = tempfile::tempdir().unwrap();
        git_init(tmp.path());
        let nested = tmp.path().join("a").join("b");
        fs::create_dir_all(&nested).unwrap();

        let detected = detect_workspace_from(&nested).unwrap();

        assert_eq!(canon(&detected), canon(tmp.path()));
    }

    #[test]
    fn falls_back_to_given_dir_outside_git_repo() {
        let tmp = tempfile::tempdir().unwrap();

        let detected = detect_workspace_from(tmp.path()).unwrap();

        assert_eq!(canon(&detected), canon(tmp.path()));
    }
}
