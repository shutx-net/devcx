mod cache;
mod cli;
mod discovery;
mod resolver;
mod runner;
mod selector;
mod workspace;

use std::path::Path;

use clap::Parser;

use crate::cli::{Cli, Commands, RunFlags};
use crate::resolver::ResolveOptions;

fn main() {
    match real_main() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}

/// Dispatch the parsed command and return the process exit code. Errors are
/// printed to stderr by `main` and exit with 1 (D-08); only which / list and
/// the --dry-run command line write data to stdout (D-10).
fn real_main() -> anyhow::Result<i32> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Up { flags, args } => run_devcontainer("up", &flags, &[], &args),
        Commands::Rebuild { flags, args } => {
            // rebuild = up --remove-existing-container: the extra flag goes
            // after the managed flags and before the user args (D-01). A
            // duplicate from the user is left alone (harmless boolean).
            run_devcontainer("up", &flags, &["--remove-existing-container"], &args)
        }
        Commands::Exec { flags, args } => run_devcontainer("exec", &flags, &[], &args),
        Commands::Select => select(),
        Commands::Which => which(),
        Commands::List => list(),
        Commands::Clear => clear(),
    }
}

/// up / rebuild / exec: resolve the config for the workspace, then delegate
/// to the devcontainer CLI and propagate its exit code. `devcx_args` are
/// devcx-injected passthrough args placed before the user args. With
/// --dry-run the command is printed to stdout instead of executed, and the
/// devcontainer CLI does not need to be installed.
fn run_devcontainer(
    subcmd: &str,
    flags: &RunFlags,
    devcx_args: &[&str],
    user_args: &[String],
) -> anyhow::Result<i32> {
    let cli_path = if flags.dry_run {
        None
    } else {
        Some(runner::ensure_devcontainer_cli()?)
    };
    let workspace = workspace::detect_workspace()?;
    if flags.verbose {
        eprintln!("devcx: workspace: {}", workspace.display());
    }
    runner::forbid_managed_flags(user_args)?;
    let config = resolver::resolve_config(
        &workspace,
        ResolveOptions {
            force_select: flags.select,
            no_cache: flags.no_cache,
            verbose: flags.verbose,
        },
    )?;

    let mut passthrough: Vec<String> = devcx_args.iter().map(|s| s.to_string()).collect();
    passthrough.extend(user_args.iter().cloned());
    let args = runner::build_args(subcmd, &workspace, &config, &passthrough);

    if flags.verbose {
        eprintln!("devcx: command: {}", runner::format_command(&args));
    }
    if flags.dry_run {
        println!("{}", runner::format_command(&args));
        return Ok(0);
    }
    let cli_path = cli_path.expect("devcontainer CLI path is resolved unless --dry-run");
    runner::run(&cli_path, &args)
}

/// Force an interactive (re-)selection and report the chosen config.
fn select() -> anyhow::Result<i32> {
    let workspace = workspace::detect_workspace()?;
    let config = resolver::resolve_config(
        &workspace,
        ResolveOptions {
            force_select: true,
            ..ResolveOptions::default()
        },
    )?;
    eprintln!("Selected: {}", display_path(&workspace, &config));
    Ok(0)
}

/// Print the cached selection to stdout, or fail when nothing is selected.
fn which() -> anyhow::Result<i32> {
    let workspace = workspace::detect_workspace()?;
    match cache::load_valid(&workspace) {
        Some(config) => {
            println!("{}", display_path(&workspace, &config));
            Ok(0)
        }
        None => {
            anyhow::bail!("No devcontainer.json selected for this workspace.\nRun: devcx select")
        }
    }
}

/// List every devcontainer.json under the workspace on stdout (D-10).
fn list() -> anyhow::Result<i32> {
    let workspace = workspace::detect_workspace()?;
    let configs = discovery::discover_configs(&workspace)?;
    if configs.is_empty() {
        return Err(resolver::no_config_error(&workspace));
    }
    for config in &configs {
        println!("{}", display_path(&workspace, config));
    }
    Ok(0)
}

/// Drop the cached selection for the workspace (exit 0 either way).
fn clear() -> anyhow::Result<i32> {
    let workspace = workspace::detect_workspace()?;
    if cache::clear(&workspace)? {
        eprintln!("Cleared cached selection.");
    } else {
        eprintln!("No cached selection for this workspace.");
    }
    Ok(0)
}

/// Workspace-relative display path, falling back to the absolute path when
/// the config is not under the workspace.
fn display_path(workspace: &Path, config: &Path) -> String {
    config
        .strip_prefix(workspace)
        .unwrap_or(config)
        .display()
        .to_string()
}
