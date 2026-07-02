use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "devcx",
    version,
    about = "A smart wrapper for Dev Container CLI",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// devcx-owned flags shared by up / rebuild / exec.
///
/// They are recognized only until the first passthrough token: once the
/// trailing capture starts (at the first token devcx does not own), anything
/// later — including e.g. `--select` — is forwarded verbatim.
#[derive(Args)]
pub struct RunFlags {
    /// Ignore the cached selection and choose interactively
    #[arg(long)]
    pub select: bool,

    /// Resolve the config without reading or writing the selection cache
    #[arg(long = "no-cache")]
    pub no_cache: bool,

    /// Print the devcontainer command instead of running it
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Explain workspace detection, discovery, and the final command on stderr
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the Dev Container (wraps `devcontainer up`)
    Up {
        #[command(flatten)]
        flags: RunFlags,

        /// Extra arguments passed through to `devcontainer up`
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Recreate the Dev Container (up --remove-existing-container)
    Rebuild {
        #[command(flatten)]
        flags: RunFlags,

        /// Extra arguments passed through to `devcontainer up`
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run a command inside the Dev Container (wraps `devcontainer exec`)
    Exec {
        #[command(flatten)]
        flags: RunFlags,

        /// Command and arguments to run inside the container
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        args: Vec<String>,
    },
    /// Interactively choose the devcontainer.json to use
    Select,
    /// Show the currently selected devcontainer.json
    Which,
    /// List devcontainer.json files found under the workspace
    List,
    /// Remove the cached selection for this workspace
    Clear,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(argv)
    }

    fn parse_up(argv: &[&str]) -> (RunFlags, Vec<String>) {
        match parse(argv).unwrap().command {
            Commands::Up { flags, args } => (flags, args),
            _ => panic!("expected Up"),
        }
    }

    #[test]
    fn exec_captures_hyphen_values_verbatim() {
        let cli = parse(&["devcx", "exec", "bash", "-lc", "echo hi"]).unwrap();
        match cli.command {
            Commands::Exec { args, .. } => assert_eq!(args, ["bash", "-lc", "echo hi"]),
            _ => panic!("expected Exec"),
        }
    }

    #[test]
    fn up_passes_through_flags() {
        let (flags, args) = parse_up(&["devcx", "up", "--remove-existing-container"]);
        assert!(!flags.select);
        assert_eq!(args, ["--remove-existing-container"]);
    }

    #[test]
    fn exec_requires_a_command() {
        assert!(parse(&["devcx", "exec"]).is_err());
    }

    #[test]
    fn plain_subcommands_parse() {
        assert!(matches!(
            parse(&["devcx", "select"]).unwrap().command,
            Commands::Select
        ));
        assert!(matches!(
            parse(&["devcx", "which"]).unwrap().command,
            Commands::Which
        ));
        assert!(matches!(
            parse(&["devcx", "list"]).unwrap().command,
            Commands::List
        ));
        assert!(matches!(
            parse(&["devcx", "clear"]).unwrap().command,
            Commands::Clear
        ));
    }

    #[test]
    fn bare_invocation_is_an_error() {
        assert!(parse(&["devcx"]).is_err());
    }

    // --- devcx-owned flag / trailing capture boundary (pinned behavior) ---

    #[test]
    fn devcx_flag_right_after_subcommand_is_recognized() {
        let (flags, args) = parse_up(&["devcx", "up", "--select"]);
        assert!(flags.select);
        assert!(!flags.no_cache && !flags.dry_run && !flags.verbose);
        assert!(args.is_empty());
    }

    #[test]
    fn devcx_flag_before_first_passthrough_token_is_recognized() {
        let (flags, args) = parse_up(&["devcx", "up", "--select", "--remove-existing-container"]);
        assert!(flags.select);
        assert_eq!(args, ["--remove-existing-container"]);
    }

    #[test]
    fn devcx_flag_after_trailing_capture_started_is_passed_through() {
        let (flags, args) = parse_up(&["devcx", "up", "--remove-existing-container", "--select"]);
        assert!(
            !flags.select,
            "a --select after the trailing capture started must be passthrough"
        );
        assert_eq!(args, ["--remove-existing-container", "--select"]);
    }

    #[test]
    fn all_run_flags_parse_on_rebuild() {
        let cli = parse(&["devcx", "rebuild", "--no-cache", "--dry-run", "--verbose"]).unwrap();
        match cli.command {
            Commands::Rebuild { flags, args } => {
                assert!(flags.no_cache && flags.dry_run && flags.verbose);
                assert!(!flags.select);
                assert!(args.is_empty());
            }
            _ => panic!("expected Rebuild"),
        }
    }

    #[test]
    fn exec_flags_precede_the_container_command() {
        let cli = parse(&["devcx", "exec", "--no-cache", "bash", "-lc", "echo hi"]).unwrap();
        match cli.command {
            Commands::Exec { flags, args } => {
                assert!(flags.no_cache);
                assert_eq!(args, ["bash", "-lc", "echo hi"]);
            }
            _ => panic!("expected Exec"),
        }
    }

    #[test]
    fn rebuild_passes_through_unknown_flags() {
        let cli = parse(&["devcx", "rebuild", "--log-level", "trace"]).unwrap();
        match cli.command {
            Commands::Rebuild { flags, args } => {
                assert!(!flags.select && !flags.no_cache);
                assert_eq!(args, ["--log-level", "trace"]);
            }
            _ => panic!("expected Rebuild"),
        }
    }
}
