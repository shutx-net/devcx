use std::path::{Path, PathBuf};

use anyhow::Context;
use dialoguer::Select;

/// Show an interactive picker over the discovered configs, labelled by
/// workspace-relative path. Returns Ok(None) when the user cancels (Esc / q).
pub fn select_config(workspace: &Path, configs: &[PathBuf]) -> anyhow::Result<Option<PathBuf>> {
    let labels = relative_labels(workspace, configs);
    let selection = Select::new()
        .with_prompt("Select devcontainer.json")
        .items(&labels)
        .default(0)
        .interact_opt()
        .context("devcontainer.json selection prompt failed")?;
    Ok(selection.map(|index| configs[index].clone()))
}

/// Build the picker labels: workspace-relative paths, falling back to the
/// absolute path when a config is not under the workspace.
fn relative_labels(workspace: &Path, configs: &[PathBuf]) -> Vec<String> {
    configs
        .iter()
        .map(|config| {
            config
                .strip_prefix(workspace)
                .unwrap_or(config)
                .display()
                .to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_workspace_relative() {
        let workspace = Path::new("/home/user/project");
        let configs = [
            PathBuf::from("/home/user/project/.devcontainer/devcontainer.json"),
            PathBuf::from("/home/user/project/.devcontainer/rust/devcontainer.json"),
            PathBuf::from("/home/user/project/.devcontainer.json"),
        ];
        assert_eq!(
            relative_labels(workspace, &configs),
            vec![
                ".devcontainer/devcontainer.json",
                ".devcontainer/rust/devcontainer.json",
                ".devcontainer.json",
            ]
        );
    }

    #[test]
    fn labels_fall_back_to_absolute_outside_workspace() {
        let workspace = Path::new("/home/user/project");
        let configs = [PathBuf::from("/elsewhere/devcontainer.json")];
        assert_eq!(
            relative_labels(workspace, &configs),
            vec!["/elsewhere/devcontainer.json"]
        );
    }
}
