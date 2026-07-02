# devcx

A smart wrapper for Dev Container CLI.

Repositories with multiple `devcontainer.json` files force you to pass
`--config .devcontainer/<name>/devcontainer.json` on every `devcontainer`
invocation: the Dev Container CLI only auto-detects
`.devcontainer/devcontainer.json` and `.devcontainer.json`, so subfolder
configs always require `--config`. `devcx` discovers every config in your
workspace, lets you pick one once, caches the choice per workspace, and then
invokes `devcontainer` with the right `--workspace-folder` and `--config`
injected for you.

## Requirements

- Docker
- [Dev Container CLI](https://github.com/devcontainers/cli) (Node.js >= 20)

```bash
npm install -g @devcontainers/cli
```

## Install

### Quick install (Linux x86_64 / aarch64)

```bash
curl -fsSL https://raw.githubusercontent.com/shutx-net/devcx/main/install.sh | bash
```

Installs a static (musl) binary to `/usr/local/bin` when run as root,
otherwise to `~/.local/bin`. Overrides: `DEVCX_INSTALL_DIR` (target
directory), `DEVCX_VERSION` (specific tag, e.g. `v0.1.0`).

### Manual install

Download the archive for your target from
[Releases](https://github.com/shutx-net/devcx/releases), verify the checksum,
extract, and put the binary on your `PATH`. For `x86_64-unknown-linux-musl`:

```bash
curl -fsSLO https://github.com/shutx-net/devcx/releases/latest/download/devcx-x86_64-unknown-linux-musl.tar.gz
curl -fsSLO https://github.com/shutx-net/devcx/releases/latest/download/devcx-x86_64-unknown-linux-musl.tar.gz.sha256
sha256sum -c devcx-x86_64-unknown-linux-musl.tar.gz.sha256
tar -xzf devcx-x86_64-unknown-linux-musl.tar.gz
sudo mv devcx /usr/local/bin/
```

### From source

```bash
cargo install --git https://github.com/shutx-net/devcx
```

Requires Rust 1.85 or newer.

## Usage

| Command | Description |
| --- | --- |
| `devcx up [args...]` | Start the Dev Container (wraps `devcontainer up`; extra args are passed through) |
| `devcx rebuild [args...]` | Recreate the container (`devcontainer up --remove-existing-container`) |
| `devcx exec <cmd> [args...]` | Run a command inside the container (wraps `devcontainer exec`) |
| `devcx select` | Interactively choose the `devcontainer.json` to use and cache it |
| `devcx which` | Print the currently selected `devcontainer.json` (relative path, stdout) |
| `devcx list` | List all `devcontainer.json` files found in the workspace |
| `devcx clear` | Remove the cached selection for this workspace |

Flags for `up` / `rebuild` / `exec` (place them right after the subcommand):

| Flag | Description |
| --- | --- |
| `--select` | Ignore the cache and pick interactively; the new choice is saved |
| `--no-cache` | Resolve without reading or writing the cache |
| `--dry-run` | Print the `devcontainer` command that would run, without running it |
| `--verbose` | Log workspace, discovery, cache, and command details to stderr |

## Example

Given a repository with several configs:

```text
project-root/
├── .devcontainer/
│   ├── ansible/
│   │   └── devcontainer.json
│   ├── java/
│   │   └── devcontainer.json
│   └── node/
│       └── devcontainer.json
├── src/
└── README.md
```

The first `devcx up` asks which config to use:

```text
$ devcx up
? Select devcontainer.json
> .devcontainer/ansible/devcontainer.json
  .devcontainer/java/devcontainer.json
  .devcontainer/node/devcontainer.json
```

After you pick one, devcx runs:

```bash
devcontainer up \
  --workspace-folder /path/to/project-root \
  --config /path/to/project-root/.devcontainer/ansible/devcontainer.json
```

Subsequent runs reuse the cached choice — no prompt:

```bash
devcx up
devcx exec ansible --version
devcx exec bash
```

Switch to another config at any time:

```bash
devcx select
```

## Notes and constraints

- devcx-owned flags (`--select`, `--no-cache`, `--dry-run`, `--verbose`) must
  come right after the subcommand, **before** any passthrough arguments.
  Everything after the first passthrough token is forwarded to
  `devcontainer` verbatim.
- `--config` and `--workspace-folder` are managed by devcx. Passing them
  yourself is an error — use `devcx select` to change the selected config.
- The selection cache lives at `~/.cache/devcx` on Linux (the OS cache
  directory on other platforms). Set `DEVCX_CACHE_DIR` to override.
- Exit codes: for `up` / `rebuild` / `exec`, the exit code of the underlying
  `devcontainer` process is propagated unchanged.

## How it works

1. Detects the workspace: the git repository root (`git rev-parse
   --show-toplevel`), or the current directory outside git.
2. Recursively discovers `devcontainer.json` / `.devcontainer.json` under the
   workspace, skipping `.git`, `node_modules`, `target`, `build`, `dist`, and
   `.cache`.
3. Picks a config: automatically when there is exactly one, from the
   per-workspace cache when it is still valid, interactively otherwise.
4. Executes `devcontainer <subcommand> --workspace-folder <abs>
   --config <abs> <your args...>` and propagates its exit code.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
