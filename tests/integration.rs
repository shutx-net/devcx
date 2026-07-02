//! End-to-end tests driving the devcx binary against a fake devcontainer CLI.
//!
//! Every test builds its own fixture: a workspace tempdir (not a git repo, so
//! devcx falls back to the command's cwd), a private DEVCX_CACHE_DIR, and a
//! bin dir prepended to PATH holding a fake `devcontainer` script that logs
//! its argv to $FAKE_LOG and exits with $FAKE_EXIT. No state is shared
//! between tests and no environment variable is mutated in-process.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;
use sha2::{Digest, Sha256};

const SINGLE: &[&str] = &[".devcontainer/devcontainer.json"];
const MULTI: &[&str] = &[
    ".devcontainer/a/devcontainer.json",
    ".devcontainer/b/devcontainer.json",
];
const CONFIG_A: &str = ".devcontainer/a/devcontainer.json";

struct Fixture {
    tmp: tempfile::TempDir,
    workspace: PathBuf,
    cache_dir: PathBuf,
    log: PathBuf,
    path_env: OsString,
}

/// Build a fixture whose workspace contains the given config files.
fn fixture(configs: &[&str]) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    for rel in configs {
        let path = workspace.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{}").unwrap();
    }

    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("devcontainer");
    fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FAKE_LOG\"\nexit \"${FAKE_EXIT:-0}\"\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

    let mut path_env = OsString::from(bin_dir.as_os_str());
    path_env.push(":");
    path_env.push(std::env::var_os("PATH").unwrap_or_default());

    Fixture {
        workspace,
        cache_dir: tmp.path().join("cache"),
        log: tmp.path().join("fake.log"),
        path_env,
        tmp,
    }
}

impl Fixture {
    /// A devcx Command wired to this fixture (cwd, cache dir, fake CLI).
    fn devcx(&self) -> Command {
        let mut cmd = Command::cargo_bin("devcx").unwrap();
        cmd.current_dir(&self.workspace)
            .env("DEVCX_CACHE_DIR", &self.cache_dir)
            .env("PATH", &self.path_env)
            .env("FAKE_LOG", &self.log)
            .timeout(Duration::from_secs(60));
        cmd
    }

    /// The workspace as a displayable absolute path.
    fn ws(&self) -> String {
        self.workspace.display().to_string()
    }

    /// Absolute path of a workspace-relative config, as a displayable string.
    fn abs(&self, rel: &str) -> String {
        self.workspace.join(rel).display().to_string()
    }

    /// The cache file devcx uses for this workspace, hashed like cache.rs:
    /// lowercase-hex SHA-256 of the workspace path bytes.
    fn cache_file(&self) -> PathBuf {
        let digest = Sha256::digest(self.workspace.as_os_str().as_encoded_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        self.cache_dir.join("projects").join(format!("{hex}.json"))
    }

    /// Seed the selection cache directly (bypasses the interactive prompt).
    fn seed_cache(&self, rel: &str) {
        let file = self.cache_file();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let entry = serde_json::json!({
            "schema_version": 1,
            "workspace": self.workspace,
            "config": self.workspace.join(rel),
        });
        fs::write(&file, serde_json::to_string(&entry).unwrap()).unwrap();
    }

    /// Everything the fake devcontainer logged so far (one line per call).
    fn log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }
}

#[test]
fn up_uses_cached_selection_and_manages_flags() {
    let f = fixture(MULTI);
    f.seed_cache(CONFIG_A);

    f.devcx().arg("up").assert().success();

    assert_eq!(
        f.log(),
        format!(
            "up --workspace-folder {} --config {}\n",
            f.ws(),
            f.abs(CONFIG_A)
        )
    );
}

#[test]
fn up_forwards_user_args_after_managed_flags() {
    let f = fixture(MULTI);
    f.seed_cache(CONFIG_A);

    f.devcx()
        .args(["up", "--remove-existing-container"])
        .assert()
        .success();

    assert_eq!(
        f.log(),
        format!(
            "up --workspace-folder {} --config {} --remove-existing-container\n",
            f.ws(),
            f.abs(CONFIG_A)
        )
    );
}

#[test]
fn exec_propagates_exit_code_and_forwards_command() {
    let f = fixture(MULTI);
    f.seed_cache(CONFIG_A);

    f.devcx()
        .env("FAKE_EXIT", "42")
        .args(["exec", "bash", "-lc", "echo hi"])
        .assert()
        .code(42);

    assert_eq!(
        f.log(),
        format!(
            "exec --workspace-folder {} --config {} bash -lc echo hi\n",
            f.ws(),
            f.abs(CONFIG_A)
        )
    );
}

#[test]
fn rebuild_runs_up_with_remove_existing_container() {
    let f = fixture(MULTI);
    f.seed_cache(CONFIG_A);

    f.devcx().arg("rebuild").assert().success();

    assert_eq!(
        f.log(),
        format!(
            "up --workspace-folder {} --config {} --remove-existing-container\n",
            f.ws(),
            f.abs(CONFIG_A)
        )
    );
}

#[test]
fn rebuild_puts_user_args_after_remove_existing_container() {
    let f = fixture(MULTI);
    f.seed_cache(CONFIG_A);

    f.devcx()
        .args(["rebuild", "--log-level", "trace"])
        .assert()
        .success();

    assert_eq!(
        f.log(),
        format!(
            "up --workspace-folder {} --config {} --remove-existing-container --log-level trace\n",
            f.ws(),
            f.abs(CONFIG_A)
        )
    );
}

#[test]
fn up_single_config_auto_selects_saves_cache_and_which_reports_it() {
    let f = fixture(SINGLE);

    f.devcx().arg("up").assert().success();

    assert!(
        f.cache_file().exists(),
        "up must save the auto-selected config"
    );
    f.devcx()
        .arg("which")
        .assert()
        .success()
        .stdout(".devcontainer/devcontainer.json\n");
}

#[test]
fn which_without_selection_fails_with_hint() {
    let f = fixture(MULTI);

    f.devcx()
        .arg("which")
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains(
            "No devcontainer.json selected for this workspace.",
        ))
        .stderr(predicate::str::contains("Run: devcx select"));
}

#[test]
fn clear_reports_removal_then_absence() {
    let f = fixture(MULTI);
    f.seed_cache(CONFIG_A);

    f.devcx()
        .arg("clear")
        .assert()
        .success()
        .stderr(predicate::str::contains("Cleared cached selection."));
    assert!(!f.cache_file().exists());

    f.devcx()
        .arg("clear")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "No cached selection for this workspace.",
        ));
}

#[test]
fn list_prints_sorted_workspace_relative_paths() {
    let f = fixture(MULTI);

    f.devcx()
        .arg("list")
        .assert()
        .success()
        .stdout(".devcontainer/a/devcontainer.json\n.devcontainer/b/devcontainer.json\n");
}

#[test]
fn list_without_configs_fails() {
    let f = fixture(&[]);

    f.devcx()
        .arg("list")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "No devcontainer.json found under workspace: {}",
            f.ws()
        )));
}

#[test]
fn up_rejects_user_supplied_managed_flags() {
    let f = fixture(MULTI);
    f.seed_cache(CONFIG_A);

    f.devcx()
        .args(["up", "--config", "/x.json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "--config is managed by devcx. Use `devcx select` to change the selected config.",
        ));
    f.devcx()
        .args(["up", "--workspace-folder", "/elsewhere"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "--workspace-folder is managed by devcx.",
        ));
    assert_eq!(
        f.log(),
        "",
        "rejected commands must never reach the devcontainer CLI"
    );
}

#[test]
fn exec_rejects_managed_flags_even_inside_container_command() {
    // Policy A (D-03) scans all passthrough args, so --config is rejected
    // even when it was meant for the command run inside the container.
    let f = fixture(MULTI);
    f.seed_cache(CONFIG_A);

    f.devcx()
        .args(["exec", "bash", "--config", "x"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "--config is managed by devcx. Use `devcx select` to change the selected config.",
        ));
    assert_eq!(f.log(), "");
}

#[test]
fn dry_run_prints_command_without_executing() {
    let f = fixture(MULTI);
    f.seed_cache(CONFIG_A);

    f.devcx()
        .args(["up", "--dry-run", "--remove-existing-container"])
        .assert()
        .success()
        .stdout(format!(
            "devcontainer up --workspace-folder {} --config {} --remove-existing-container\n",
            f.ws(),
            f.abs(CONFIG_A)
        ));

    assert_eq!(
        f.log(),
        "",
        "--dry-run must not invoke the devcontainer CLI"
    );
}

#[test]
fn dry_run_works_without_devcontainer_cli_on_path() {
    let f = fixture(MULTI);
    f.seed_cache(CONFIG_A);
    // PATH without the fake devcontainer (and without git: devcx then falls
    // back to the cwd as the workspace).
    let empty_bin = f.tmp.path().join("empty-bin");
    fs::create_dir_all(&empty_bin).unwrap();

    let mut cmd = Command::cargo_bin("devcx").unwrap();
    cmd.current_dir(&f.workspace)
        .env("DEVCX_CACHE_DIR", &f.cache_dir)
        .env("PATH", &empty_bin)
        .timeout(Duration::from_secs(60))
        .args(["up", "--dry-run"]);

    cmd.assert().success().stdout(format!(
        "devcontainer up --workspace-folder {} --config {}\n",
        f.ws(),
        f.abs(CONFIG_A)
    ));
}

#[test]
fn no_cache_resolves_without_writing_cache() {
    let f = fixture(SINGLE);

    f.devcx().args(["up", "--no-cache"]).assert().success();

    assert!(
        !f.cache_file().exists(),
        "--no-cache must not write a cache entry"
    );
    assert_eq!(
        f.log(),
        format!(
            "up --workspace-folder {} --config {}\n",
            f.ws(),
            f.abs(".devcontainer/devcontainer.json")
        )
    );
}

#[test]
fn up_multi_config_without_cache_or_tty_fails_at_prompt() {
    let f = fixture(MULTI);

    f.devcx().arg("up").assert().failure();

    assert_eq!(
        f.log(),
        "",
        "an unresolved selection must not reach the devcontainer CLI"
    );
}

#[test]
fn verbose_writes_diagnostics_to_stderr_only() {
    let f = fixture(MULTI);
    f.seed_cache(CONFIG_A);

    let assert = f
        .devcx()
        .args(["up", "--verbose"])
        .assert()
        .success()
        .stdout("");
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    assert!(
        stderr.contains(&format!("devcx: workspace: {}", f.ws())),
        "stderr must report the detected workspace, was:\n{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "devcx: command: devcontainer up --workspace-folder {} --config {}",
            f.ws(),
            f.abs(CONFIG_A)
        )),
        "stderr must report the final command line, was:\n{stderr}"
    );
}
