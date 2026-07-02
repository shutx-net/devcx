use std::path::{Path, PathBuf};

use anyhow::bail;

use crate::{cache, discovery, selector};

/// Per-run knobs for [`resolve_config`], mapped from the up / rebuild / exec
/// flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolveOptions {
    /// Force the interactive chooser for multi-config workspaces even when a
    /// valid cache entry exists. Single-config workspaces still auto-resolve.
    pub force_select: bool,
    /// Neither read nor write the selection cache (--no-cache).
    pub no_cache: bool,
    /// Report discovery and cache details on stderr (--verbose).
    pub verbose: bool,
}

/// The error for a workspace without any devcontainer.json, shared by the
/// resolve flow and `devcx list`.
pub fn no_config_error(workspace: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "No devcontainer.json found under workspace: {}",
        workspace.display()
    )
}

/// Decide which devcontainer.json to use for the workspace.
///
/// 0 found: error. 1 found: auto-select. N found: use the valid cache entry
/// unless force_select / no_cache, otherwise select interactively. The
/// resolved config is saved to the cache unless no_cache (D-09). Interactive
/// cancel is reported as an error ("Selection cancelled.").
pub fn resolve_config(workspace: &Path, opts: ResolveOptions) -> anyhow::Result<PathBuf> {
    resolve_with(
        workspace,
        opts,
        &cache::cache_root()?,
        selector::select_config,
    )
}

/// [`resolve_config`] with the cache root and the interactive chooser
/// injected so tests can use a temp cache and a closure instead of dialoguer.
///
/// `choose` receives the workspace and the discovered configs; it returns
/// Ok(None) when the user cancels.
fn resolve_with(
    workspace: &Path,
    opts: ResolveOptions,
    cache_root: &Path,
    choose: impl FnOnce(&Path, &[PathBuf]) -> anyhow::Result<Option<PathBuf>>,
) -> anyhow::Result<PathBuf> {
    let configs = discovery::discover_configs(workspace)?;
    if opts.verbose {
        eprintln!(
            "devcx: discovered {} devcontainer.json file(s)",
            configs.len()
        );
    }

    // Cache write policy (D-09): every resolved config is saved, unless the
    // cache is disabled for this run.
    let save = |config: &Path| -> anyhow::Result<()> {
        if !opts.no_cache {
            cache::save_at(cache_root, workspace, config)?;
        }
        Ok(())
    };

    match configs.as_slice() {
        [] => Err(no_config_error(workspace)),
        [only] => {
            save(only)?;
            Ok(only.clone())
        }
        _ => {
            // Cache read policy: only an implicit resolution may reuse the
            // cached choice.
            if !opts.force_select && !opts.no_cache {
                let cache_file = cache::cache_file_path_at(cache_root, workspace);
                if opts.verbose {
                    eprintln!("devcx: cache file: {}", cache_file.display());
                }
                if let Some(cached) = cache::load_valid_at(cache_root, workspace) {
                    return Ok(cached);
                }
                // A cache file that exists but failed validation is stale
                // (e.g. its config was deleted): tell the user why they are
                // being asked to select again.
                if cache_file.exists() {
                    eprintln!("Cached devcontainer.json no longer exists. Please select again.");
                }
            }
            match choose(workspace, &configs)? {
                Some(config) => {
                    save(&config)?;
                    Ok(config)
                }
                None => bail!("Selection cancelled."),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Fixture {
        _tmp: tempfile::TempDir,
        cache_root: PathBuf,
        workspace: PathBuf,
    }

    /// Tempdir fixture: a private cache root plus a workspace containing the
    /// given configs (workspace-relative paths, "{}" contents).
    fn fixture(configs: &[&str]) -> Fixture {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let cache_root = tmp.path().join("cache-root");
        let workspace = tmp.path().join("workspace");
        fs::create_dir_all(&workspace).expect("create workspace");
        for rel in configs {
            let path = workspace.join(rel);
            fs::create_dir_all(path.parent().expect("config parent")).expect("create config dir");
            fs::write(&path, "{}").expect("write config");
        }
        Fixture {
            _tmp: tmp,
            cache_root,
            workspace,
        }
    }

    const SINGLE: &[&str] = &[".devcontainer/devcontainer.json"];
    const MULTI: &[&str] = &[
        ".devcontainer/a/devcontainer.json",
        ".devcontainer/b/devcontainer.json",
    ];

    fn opts(force_select: bool, no_cache: bool) -> ResolveOptions {
        ResolveOptions {
            force_select,
            no_cache,
            verbose: false,
        }
    }

    /// Chooser that fails the test if the resolver prompts.
    fn forbidden_chooser(_: &Path, _: &[PathBuf]) -> anyhow::Result<Option<PathBuf>> {
        panic!("interactive chooser must not be called");
    }

    #[test]
    fn zero_configs_is_an_error() {
        let f = fixture(&[]);

        let err = resolve_with(
            &f.workspace,
            opts(false, false),
            &f.cache_root,
            forbidden_chooser,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            format!(
                "No devcontainer.json found under workspace: {}",
                f.workspace.display()
            )
        );
    }

    #[test]
    fn single_config_auto_selects_and_saves() {
        let f = fixture(SINGLE);
        let expected = f.workspace.join(".devcontainer/devcontainer.json");

        let resolved = resolve_with(
            &f.workspace,
            opts(false, false),
            &f.cache_root,
            forbidden_chooser,
        )
        .unwrap();

        assert_eq!(resolved, expected);
        assert_eq!(
            cache::load_valid_at(&f.cache_root, &f.workspace),
            Some(expected)
        );
    }

    #[test]
    fn single_config_with_force_select_skips_prompt_and_saves() {
        let f = fixture(SINGLE);
        let expected = f.workspace.join(".devcontainer/devcontainer.json");

        let resolved = resolve_with(
            &f.workspace,
            opts(true, false),
            &f.cache_root,
            forbidden_chooser,
        )
        .unwrap();

        assert_eq!(resolved, expected);
        assert_eq!(
            cache::load_valid_at(&f.cache_root, &f.workspace),
            Some(expected)
        );
    }

    #[test]
    fn multi_with_valid_cache_returns_cached_without_prompting() {
        let f = fixture(MULTI);
        let cached = f.workspace.join(".devcontainer/b/devcontainer.json");
        cache::save_at(&f.cache_root, &f.workspace, &cached).unwrap();

        let resolved = resolve_with(
            &f.workspace,
            opts(false, false),
            &f.cache_root,
            forbidden_chooser,
        )
        .unwrap();

        assert_eq!(resolved, cached);
    }

    #[test]
    fn multi_with_stale_cache_falls_through_to_chooser() {
        let f = fixture(MULTI);
        // The cached config no longer exists: the cache file is on disk but
        // fails validation.
        let ghost = f.workspace.join(".devcontainer/ghost/devcontainer.json");
        cache::save_at(&f.cache_root, &f.workspace, &ghost).unwrap();
        assert!(cache::cache_file_path_at(&f.cache_root, &f.workspace).exists());
        let chosen = f.workspace.join(".devcontainer/a/devcontainer.json");

        let mut called = false;
        let resolved = resolve_with(
            &f.workspace,
            opts(false, false),
            &f.cache_root,
            |ws, configs| {
                called = true;
                assert_eq!(ws, f.workspace.as_path());
                assert_eq!(configs.len(), 2);
                Ok(Some(chosen.clone()))
            },
        )
        .unwrap();

        assert!(called, "stale cache must fall through to the chooser");
        assert_eq!(resolved, chosen);
        assert_eq!(
            cache::load_valid_at(&f.cache_root, &f.workspace),
            Some(chosen)
        );
    }

    #[test]
    fn force_select_bypasses_valid_cache() {
        let f = fixture(MULTI);
        let a = f.workspace.join(".devcontainer/a/devcontainer.json");
        let b = f.workspace.join(".devcontainer/b/devcontainer.json");
        cache::save_at(&f.cache_root, &f.workspace, &a).unwrap();

        let resolved = resolve_with(&f.workspace, opts(true, false), &f.cache_root, |_, _| {
            Ok(Some(b.clone()))
        })
        .unwrap();

        assert_eq!(resolved, b);
        assert_eq!(cache::load_valid_at(&f.cache_root, &f.workspace), Some(b));
    }

    #[test]
    fn chooser_cancel_is_a_selection_cancelled_error() {
        let f = fixture(MULTI);

        let err = resolve_with(&f.workspace, opts(false, false), &f.cache_root, |_, _| {
            Ok(None)
        })
        .unwrap_err();

        assert_eq!(err.to_string(), "Selection cancelled.");
        // A cancelled selection must not create a cache entry.
        assert!(!cache::cache_file_path_at(&f.cache_root, &f.workspace).exists());
    }

    #[test]
    fn chosen_config_is_saved_to_cache() {
        let f = fixture(MULTI);
        let b = f.workspace.join(".devcontainer/b/devcontainer.json");

        let resolved = resolve_with(&f.workspace, opts(false, false), &f.cache_root, |_, _| {
            Ok(Some(b.clone()))
        })
        .unwrap();

        assert_eq!(resolved, b);
        assert_eq!(cache::load_valid_at(&f.cache_root, &f.workspace), Some(b));
    }

    // --- --no-cache behavior ---

    #[test]
    fn no_cache_skips_a_valid_cache_and_prompts() {
        let f = fixture(MULTI);
        let a = f.workspace.join(".devcontainer/a/devcontainer.json");
        let b = f.workspace.join(".devcontainer/b/devcontainer.json");
        cache::save_at(&f.cache_root, &f.workspace, &a).unwrap();

        let mut called = false;
        let resolved = resolve_with(&f.workspace, opts(false, true), &f.cache_root, |_, _| {
            called = true;
            Ok(Some(b.clone()))
        })
        .unwrap();

        assert!(called, "--no-cache must ignore the valid cache and prompt");
        assert_eq!(resolved, b);
        // ...and it must not overwrite the existing cache entry either.
        assert_eq!(cache::load_valid_at(&f.cache_root, &f.workspace), Some(a));
    }

    #[test]
    fn no_cache_single_config_does_not_create_a_cache_file() {
        let f = fixture(SINGLE);
        let expected = f.workspace.join(".devcontainer/devcontainer.json");

        let resolved = resolve_with(
            &f.workspace,
            opts(false, true),
            &f.cache_root,
            forbidden_chooser,
        )
        .unwrap();

        assert_eq!(resolved, expected);
        assert!(!cache::cache_file_path_at(&f.cache_root, &f.workspace).exists());
    }

    #[test]
    fn no_cache_chosen_multi_does_not_create_a_cache_file() {
        let f = fixture(MULTI);
        let b = f.workspace.join(".devcontainer/b/devcontainer.json");

        let resolved = resolve_with(&f.workspace, opts(false, true), &f.cache_root, |_, _| {
            Ok(Some(b.clone()))
        })
        .unwrap();

        assert_eq!(resolved, b);
        assert!(!cache::cache_file_path_at(&f.cache_root, &f.workspace).exists());
    }

    #[test]
    fn select_with_no_cache_prompts_and_writes_nothing() {
        let f = fixture(MULTI);
        let a = f.workspace.join(".devcontainer/a/devcontainer.json");
        let b = f.workspace.join(".devcontainer/b/devcontainer.json");
        cache::save_at(&f.cache_root, &f.workspace, &a).unwrap();

        let mut called = false;
        let resolved = resolve_with(&f.workspace, opts(true, true), &f.cache_root, |_, _| {
            called = true;
            Ok(Some(b.clone()))
        })
        .unwrap();

        assert!(called, "--select --no-cache must prompt");
        assert_eq!(resolved, b);
        assert_eq!(
            cache::load_valid_at(&f.cache_root, &f.workspace),
            Some(a),
            "--no-cache must leave the cache untouched even with --select"
        );
    }
}
