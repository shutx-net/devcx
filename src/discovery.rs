use std::path::{Path, PathBuf};

use walkdir::{DirEntry, WalkDir};

pub const EXCLUDED_DIRS: &[&str] = &[".git", "node_modules", "target", "build", "dist", ".cache"];

/// File names the devcontainer CLI accepts for `--config` (D-05).
const CONFIG_FILE_NAMES: &[&str] = &["devcontainer.json", ".devcontainer.json"];

/// Recursively find devcontainer.json / .devcontainer.json files under the
/// workspace, pruning EXCLUDED_DIRS. Returns absolute paths sorted by their
/// workspace-relative path.
pub fn discover_configs(workspace: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut configs: Vec<PathBuf> = WalkDir::new(workspace)
        .into_iter()
        .filter_entry(|entry| !is_excluded_dir(entry))
        // Skip unreadable entries (permissions etc.) and keep walking.
        .filter_map(Result::ok)
        .filter(is_config_file)
        .map(DirEntry::into_path)
        .collect();

    configs.sort_by(|a, b| {
        let a = a.strip_prefix(workspace).unwrap_or(a.as_path());
        let b = b.strip_prefix(workspace).unwrap_or(b.as_path());
        a.cmp(b)
    });

    Ok(configs)
}

/// True for directories below the workspace root whose name is in
/// EXCLUDED_DIRS; `filter_entry` then prunes the whole subtree. Depth 0 always
/// passes so a workspace itself named e.g. `target` is still searched.
fn is_excluded_dir(entry: &DirEntry) -> bool {
    entry.depth() > 0
        && entry.file_type().is_dir()
        && EXCLUDED_DIRS.iter().any(|name| entry.file_name() == *name)
}

/// True for files named exactly `devcontainer.json` or `.devcontainer.json`.
fn is_config_file(entry: &DirEntry) -> bool {
    entry.file_type().is_file()
        && CONFIG_FILE_NAMES
            .iter()
            .any(|name| entry.file_name() == *name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create `path` (and its parent directories) with dummy JSON content.
    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "{}").unwrap();
    }

    /// Workspace-relative views of `paths` for compact assertions.
    fn relative(paths: &[PathBuf], workspace: &Path) -> Vec<PathBuf> {
        paths
            .iter()
            .map(|p| p.strip_prefix(workspace).unwrap().to_path_buf())
            .collect()
    }

    #[test]
    fn finds_configs_prunes_excluded_dirs_and_sorts() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path();
        touch(&ws.join(".devcontainer/ansible/devcontainer.json"));
        touch(&ws.join(".devcontainer/java/devcontainer.json"));
        touch(&ws.join(".devcontainer.json"));
        touch(&ws.join("node_modules/pkg/devcontainer.json"));
        touch(&ws.join("target/x/devcontainer.json"));
        touch(&ws.join("src/tool/devcontainer.json"));
        // Not an accepted config file name: must not be picked up.
        touch(&ws.join("src/devcontainer.jsonc"));

        let found = discover_configs(ws).unwrap();

        assert!(found.iter().all(|p| p.is_absolute()));
        assert_eq!(
            relative(&found, ws),
            [
                PathBuf::from(".devcontainer/ansible/devcontainer.json"),
                PathBuf::from(".devcontainer/java/devcontainer.json"),
                PathBuf::from(".devcontainer.json"),
                PathBuf::from("src/tool/devcontainer.json"),
            ]
        );
    }

    #[test]
    fn workspace_root_named_like_excluded_dir_is_searched() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().join("target");
        touch(&ws.join(".devcontainer/devcontainer.json"));

        let found = discover_configs(&ws).unwrap();

        assert!(found.iter().all(|p| p.is_absolute()));
        assert_eq!(
            relative(&found, &ws),
            [PathBuf::from(".devcontainer/devcontainer.json")]
        );
    }
}
