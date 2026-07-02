use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use anyhow::{Context, bail};

/// Locate the devcontainer CLI on PATH.
pub fn ensure_devcontainer_cli() -> anyhow::Result<PathBuf> {
    which::which("devcontainer").map_err(|_| {
        anyhow::anyhow!(
            "devcontainer CLI was not found in PATH.\nPlease install it first:\n  npm install -g @devcontainers/cli"
        )
    })
}

/// Reject user-supplied --config / --workspace-folder (managed by devcx).
pub fn forbid_managed_flags(user_args: &[String]) -> anyhow::Result<()> {
    for arg in user_args {
        if arg == "--config" || arg.starts_with("--config=") {
            bail!(
                "--config is managed by devcx. Use `devcx select` to change the selected config."
            );
        }
        if arg == "--workspace-folder" || arg.starts_with("--workspace-folder=") {
            bail!("--workspace-folder is managed by devcx.");
        }
    }
    Ok(())
}

/// Build the devcontainer argv:
/// <subcmd> --workspace-folder <abs> --config <abs> <user-args...>.
/// Managed flags always precede user args because `devcontainer exec` stops
/// parsing options at the first positional token.
pub fn build_args(
    subcmd: &str,
    workspace: &Path,
    config: &Path,
    user_args: &[String],
) -> Vec<String> {
    let mut args = vec![
        subcmd.to_string(),
        "--workspace-folder".to_string(),
        workspace.display().to_string(),
        "--config".to_string(),
        config.display().to_string(),
    ];
    args.extend(user_args.iter().cloned());
    args
}

/// Run the devcontainer CLI with the prebuilt argv (see [`build_args`]),
/// inheriting stdio, and return its exit code (on Unix, 128 + signal number
/// when the child was signal-killed).
pub fn run(cli_path: &Path, args: &[String]) -> anyhow::Result<i32> {
    let status = Command::new(cli_path)
        .args(args)
        .status()
        .with_context(|| format!("failed to run {}", cli_path.display()))?;
    Ok(exit_code(status))
}

/// Render the argv as one shell-safe `devcontainer ...` line for --dry-run
/// (stdout) and --verbose (stderr) output.
pub fn format_command(args: &[String]) -> String {
    let mut line = String::from("devcontainer");
    for arg in args {
        line.push(' ');
        line.push_str(&shell_quote(arg));
    }
    line
}

/// Quote one token for shell display: tokens made only of harmless bytes
/// stay bare, everything else is single-quoted with embedded single quotes
/// escaped as '\''.
fn shell_quote(token: &str) -> String {
    let bare = |b: u8| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b);
    if !token.is_empty() && token.bytes().all(bare) {
        token.to_string()
    } else {
        format!("'{}'", token.replace('\'', "'\\''"))
    }
}

/// Map an exit status to a shell-convention code: the child's own code, or
/// 128 + signal number on Unix when it was signal-killed.
fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn build_args_puts_managed_flags_before_user_args() {
        let args = build_args(
            "exec",
            Path::new("/ws"),
            Path::new("/ws/.devcontainer/devcontainer.json"),
            &strings(&["bash", "-lc", "echo hi"]),
        );
        assert_eq!(
            args,
            [
                "exec",
                "--workspace-folder",
                "/ws",
                "--config",
                "/ws/.devcontainer/devcontainer.json",
                "bash",
                "-lc",
                "echo hi",
            ]
        );
    }

    #[test]
    fn build_args_up_without_user_args() {
        let args = build_args(
            "up",
            Path::new("/ws"),
            Path::new("/ws/.devcontainer.json"),
            &[],
        );
        assert_eq!(
            args,
            [
                "up",
                "--workspace-folder",
                "/ws",
                "--config",
                "/ws/.devcontainer.json",
            ]
        );
    }

    #[test]
    fn forbids_config_flag_in_both_forms() {
        for args in [strings(&["--config"]), strings(&["--config=/x.json"])] {
            let err = forbid_managed_flags(&args).unwrap_err();
            assert_eq!(
                err.to_string(),
                "--config is managed by devcx. Use `devcx select` to change the selected config."
            );
        }
    }

    #[test]
    fn forbids_workspace_folder_flag_in_both_forms() {
        for args in [
            strings(&["--workspace-folder"]),
            strings(&["--workspace-folder=/x"]),
        ] {
            let err = forbid_managed_flags(&args).unwrap_err();
            assert_eq!(err.to_string(), "--workspace-folder is managed by devcx.");
        }
    }

    #[test]
    fn allows_other_flags_and_commands() {
        assert!(forbid_managed_flags(&strings(&["--remove-existing-container"])).is_ok());
        assert!(forbid_managed_flags(&strings(&["bash", "-lc", "echo hi"])).is_ok());
        assert!(forbid_managed_flags(&[]).is_ok());
    }

    #[test]
    fn shell_quote_leaves_harmless_tokens_bare() {
        for token in [
            "up",
            "--workspace-folder",
            "/ws/.devcontainer/devcontainer.json",
            "a_b@c%d+e=f:g,h.i/j-k",
        ] {
            assert_eq!(shell_quote(token), token);
        }
    }

    #[test]
    fn shell_quote_single_quotes_special_tokens() {
        assert_eq!(shell_quote("echo hi"), "'echo hi'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a;b"), "'a;b'");
        assert_eq!(shell_quote("$HOME"), "'$HOME'");
    }

    #[test]
    fn format_command_renders_one_shell_safe_line() {
        let args = strings(&[
            "exec",
            "--workspace-folder",
            "/w s",
            "bash",
            "-lc",
            "echo 'hi'",
        ]);
        assert_eq!(
            format_command(&args),
            r"devcontainer exec --workspace-folder '/w s' bash -lc 'echo '\''hi'\'''"
        );
    }

    #[test]
    fn exit_code_passes_through_child_code() {
        let status = Command::new("/bin/sh")
            .args(["-c", "exit 42"])
            .status()
            .unwrap();
        assert_eq!(exit_code(status), 42);
    }

    #[cfg(unix)]
    #[test]
    fn exit_code_maps_signal_death_to_128_plus_signal() {
        let status = Command::new("/bin/sh")
            .args(["-c", "kill -TERM $$"])
            .status()
            .unwrap();
        assert_eq!(exit_code(status), 128 + 15);
    }
}
