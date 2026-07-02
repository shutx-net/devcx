use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Current cache file schema version (D-06).
const SCHEMA_VERSION: u32 = 1;

/// Persisted selection for one workspace.
#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    schema_version: u32,
    workspace: PathBuf,
    config: PathBuf,
}

/// Cache root: $DEVCX_CACHE_DIR override, else <user cache dir>/devcx.
pub fn cache_root() -> anyhow::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("DEVCX_CACHE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let base = dirs::cache_dir().context("could not determine the user cache directory")?;
    Ok(base.join("devcx"))
}

/// Cache file for the given workspace: <root>/projects/<sha256-hex>.json.
#[allow(dead_code)] // planned public API; resolver uses cache_file_path_at with an injected root
pub fn cache_file_path(workspace: &Path) -> anyhow::Result<PathBuf> {
    Ok(cache_file_path_at(&cache_root()?, workspace))
}

/// Load the cached config for the workspace if it passes validation
/// (workspace match, config exists, config under workspace).
pub fn load_valid(workspace: &Path) -> Option<PathBuf> {
    load_valid_at(&cache_root().ok()?, workspace)
}

/// Persist the selected config for the workspace.
#[allow(dead_code)] // planned public API; resolver uses save_at with an injected root
pub fn save(workspace: &Path, config: &Path) -> anyhow::Result<()> {
    save_at(&cache_root()?, workspace, config)
}

/// Remove the cache entry for the workspace. Returns true if a file was deleted.
pub fn clear(workspace: &Path) -> anyhow::Result<bool> {
    clear_at(&cache_root()?, workspace)
}

/// [`cache_file_path`] against an explicit cache root. The file name is the
/// full lowercase SHA-256 hex digest of the workspace path bytes.
pub(crate) fn cache_file_path_at(root: &Path, workspace: &Path) -> PathBuf {
    let digest = Sha256::digest(workspace.as_os_str().as_encoded_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    root.join("projects").join(format!("{hex}.json"))
}

/// [`load_valid`] against an explicit cache root. Any IO, parse, or
/// validation failure yields None; never panics, never propagates errors.
pub(crate) fn load_valid_at(root: &Path, workspace: &Path) -> Option<PathBuf> {
    let bytes = fs::read(cache_file_path_at(root, workspace)).ok()?;
    let entry: CacheEntry = serde_json::from_slice(&bytes).ok()?;
    let valid = entry.schema_version == SCHEMA_VERSION
        && entry.workspace.as_path() == workspace
        && entry.config.is_file()
        && entry.config.starts_with(workspace);
    if valid { Some(entry.config) } else { None }
}

/// [`save`] against an explicit cache root: write to a sibling temp file,
/// then rename over the final path (atomic on the same filesystem).
pub(crate) fn save_at(root: &Path, workspace: &Path, config: &Path) -> anyhow::Result<()> {
    let path = cache_file_path_at(root, workspace);
    let dir = path
        .parent()
        .context("cache file path has no parent directory")?;
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create cache directory: {}", dir.display()))?;

    let entry = CacheEntry {
        schema_version: SCHEMA_VERSION,
        workspace: workspace.to_path_buf(),
        config: config.to_path_buf(),
    };
    let mut json =
        serde_json::to_string_pretty(&entry).context("failed to serialize cache entry")?;
    json.push('\n');

    let tmp = {
        let mut name = path
            .file_name()
            .context("cache file path has no file name")?
            .to_os_string();
        name.push(".tmp");
        path.with_file_name(name)
    };
    fs::write(&tmp, json)
        .with_context(|| format!("failed to write cache file: {}", tmp.display()))?;
    fs::rename(&tmp, &path)
        .with_context(|| format!("failed to move cache file into place: {}", path.display()))?;
    Ok(())
}

/// [`clear`] against an explicit cache root. Ok(false) when no entry existed.
pub(crate) fn clear_at(root: &Path, workspace: &Path) -> anyhow::Result<bool> {
    let path = cache_file_path_at(root, workspace);
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => {
            Err(err).with_context(|| format!("failed to remove cache file: {}", path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        tmp: tempfile::TempDir,
        root: PathBuf,
        workspace: PathBuf,
        config: PathBuf,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let root = tmp.path().join("cache-root");
        let workspace = tmp.path().join("workspace");
        let config = workspace.join(".devcontainer").join("devcontainer.json");
        fs::create_dir_all(config.parent().expect("config parent")).expect("create config dir");
        fs::write(&config, "{}").expect("write config");
        Fixture {
            tmp,
            root,
            workspace,
            config,
        }
    }

    /// Write an arbitrary entry into the cache slot that belongs to `slot_workspace`.
    fn write_entry(root: &Path, slot_workspace: &Path, entry: &CacheEntry) {
        let path = cache_file_path_at(root, slot_workspace);
        fs::create_dir_all(path.parent().expect("cache parent")).expect("create cache dir");
        fs::write(
            &path,
            serde_json::to_string_pretty(entry).expect("serialize"),
        )
        .expect("write entry");
    }

    #[test]
    fn save_then_load_roundtrip() {
        let f = fixture();
        save_at(&f.root, &f.workspace, &f.config).expect("save");
        assert_eq!(load_valid_at(&f.root, &f.workspace), Some(f.config.clone()));
    }

    #[test]
    fn load_rejects_workspace_mismatch() {
        let f = fixture();
        write_entry(
            &f.root,
            &f.workspace,
            &CacheEntry {
                schema_version: SCHEMA_VERSION,
                workspace: f.tmp.path().join("other-workspace"),
                config: f.config.clone(),
            },
        );
        assert_eq!(load_valid_at(&f.root, &f.workspace), None);
    }

    #[test]
    fn load_rejects_schema_version_mismatch() {
        let f = fixture();
        write_entry(
            &f.root,
            &f.workspace,
            &CacheEntry {
                schema_version: SCHEMA_VERSION + 1,
                workspace: f.workspace.clone(),
                config: f.config.clone(),
            },
        );
        assert_eq!(load_valid_at(&f.root, &f.workspace), None);
    }

    #[test]
    fn load_rejects_missing_config_file() {
        let f = fixture();
        save_at(&f.root, &f.workspace, &f.config).expect("save");
        fs::remove_file(&f.config).expect("delete config");
        assert_eq!(load_valid_at(&f.root, &f.workspace), None);
    }

    #[test]
    fn load_rejects_config_outside_workspace() {
        let f = fixture();
        let outside = f.tmp.path().join("outside").join("devcontainer.json");
        fs::create_dir_all(outside.parent().expect("outside parent")).expect("create outside dir");
        fs::write(&outside, "{}").expect("write outside config");
        save_at(&f.root, &f.workspace, &outside).expect("save");
        assert_eq!(load_valid_at(&f.root, &f.workspace), None);
    }

    #[test]
    fn load_rejects_corrupted_json() {
        let f = fixture();
        let path = cache_file_path_at(&f.root, &f.workspace);
        fs::create_dir_all(path.parent().expect("cache parent")).expect("create cache dir");
        fs::write(&path, "{ this is not json").expect("write garbage");
        assert_eq!(load_valid_at(&f.root, &f.workspace), None);
    }

    #[test]
    fn clear_removes_entry_once() {
        let f = fixture();
        save_at(&f.root, &f.workspace, &f.config).expect("save");
        assert!(clear_at(&f.root, &f.workspace).expect("first clear"));
        assert!(!clear_at(&f.root, &f.workspace).expect("second clear"));
        assert_eq!(load_valid_at(&f.root, &f.workspace), None);
    }

    #[test]
    fn cache_file_path_at_is_deterministic_and_hex_named() {
        let root = Path::new("/cache-root");
        let workspace = Path::new("/home/user/project");
        let first = cache_file_path_at(root, workspace);
        let second = cache_file_path_at(root, workspace);
        assert_eq!(first, second);
        assert!(first.starts_with(root.join("projects")));
        assert_eq!(first.extension().and_then(|e| e.to_str()), Some("json"));
        let stem = first
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("utf-8 file stem");
        assert_eq!(stem.len(), 64);
        assert!(stem.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
        assert_ne!(
            first,
            cache_file_path_at(root, Path::new("/home/user/other"))
        );
    }
}
