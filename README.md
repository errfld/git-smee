# git-smee

[![CI](https://github.com/errfld/git-smee/actions/workflows/ci.yml/badge.svg)](https://github.com/errfld/git-smee/actions/workflows/ci.yml)

> Smee - the right hand of (Git) hooks

git-smee is a lightweight Rust CLI that manages Git hooks from a version-controlled configuration file, `.git-smee.toml`, in your repository.

Instead of copying hook scripts around or relying on heavy external tooling, git-smee installs small, idempotent hook wrappers that delegate to commands defined in your config—making hook behavior consistent across all contributors.

## Why git-smee?

- **Version-controlled**: Define hooks once in `.git-smee.toml`, commit it, share it.
- **Cross-platform minded**: Designed to work on Unix and Windows (Git Bash) via a clear platform abstraction.
- **Portable Unix execution**: Hook commands run through POSIX `sh -c` on Unix-like systems (not Bash-specific).
- **Small and focused**: Single-purpose binary, no plugin ecosystem or extra runtime.

## Installation

### Homebrew

```bash
brew tap errfld/git-smee
brew install git-smee
```

### Cargo

```bash
cargo install git-smee-cli
```

### From source

```bash
git clone https://github.com/errfld/git-smee.git
cd git-smee
cargo install --path crates/git-smee-cli
```

## Quick Start

1. Initialize a configuration file in your repository:

   ```bash
   git smee init
   ```

   This creates a `.git-smee.toml` file with a default pre-commit hook.
   If the file already exists, `init` refuses to overwrite it unless you pass `--force`.

2. Edit `.git-smee.toml` to define your hooks:

   ```toml
   [[pre-commit]]
   command = "cargo fmt --check"

   [[pre-commit]]
   command = "cargo test"

   [[pre-push]]
   command = "cargo test --all-targets"
   ```

3. Install the hooks into `.git/hooks`:

   ```bash
   git smee install
   ```

   By default, `install` only overwrites hook files previously managed by git-smee.
   Existing unmanaged hook files are preserved unless you pass `--force`.

That's it! Your hooks are now active. When Git triggers a hook, the installed wrapper runs the `git-smee` executable directly and executes the configured commands in order.

### Alternate config paths

By default, git-smee reads `.git-smee.toml` from the repository root.

You can override the config path in two ways:

- CLI flag: `--config <path>`
- Environment variable: `GIT_SMEE_CONFIG=<path>`

Precedence is explicit: `--config` > `GIT_SMEE_CONFIG` > `.git-smee.toml`.

Examples:

```bash
git smee install --config .config/git-smee.toml
GIT_SMEE_CONFIG=.config/git-smee.toml git smee run pre-commit
```

## Configuration

The `.git-smee.toml` file uses TOML format. Each hook is defined as an array of tables:

```toml
[[hook-name]]
command = "your command here"
```

### Hook Definition Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `command` | string | yes | The command to execute |
| `parallel_execution_allowed` | bool | no | Allow parallel execution with other parallel-enabled commands (default: `false`) |

### Execution Order

When running hooks, git-smee executes commands in two phases:

1. **Sequential phase**: All commands with `parallel_execution_allowed = false` (or omitted) run one at a time, in the order they appear in the config.
2. **Parallel phase**: All commands with `parallel_execution_allowed = true` run concurrently using a thread pool.

Sequential commands always complete before parallel commands begin. If any command fails, execution stops immediately (fail-fast behavior).

**Example:**

```toml
[[pre-commit]]
command = "cargo fmt --check"
# Sequential (default) - runs first

[[pre-commit]]
command = "cargo clippy"
parallel_execution_allowed = true
# Parallel - runs concurrently with other parallel commands

[[pre-commit]]
command = "cargo test --lib"
parallel_execution_allowed = true
# Parallel - runs concurrently with clippy

[[pre-commit]]
command = "echo 'Setup complete'"
# Sequential - runs before parallel commands
```

In this example, the two sequential commands (`cargo fmt --check` and `echo 'Setup complete'`) run first in order, then `cargo clippy` and `cargo test --lib` run in parallel.

### Supported Git Hooks

git-smee supports all standard Git lifecycle hooks:

| Hook | Description |
|------|-------------|
| `applypatch-msg` | Edit the commit message of a patch |
| `pre-applypatch` | Run before a patch is applied |
| `post-applypatch` | Run after a patch is applied |
| `pre-commit` | Run before a commit is created |
| `prepare-commit-msg` | Prepare the default commit message |
| `commit-msg` | Validate or modify the commit message |
| `post-commit` | Run after a commit is created |
| `pre-merge-commit` | Run before a merge commit is created |
| `pre-rebase` | Run before a rebase starts |
| `post-checkout` | Run after a checkout |
| `post-merge` | Run after a merge |
| `post-rewrite` | Run after commands that rewrite commits |
| `pre-push` | Run before a push |
| `pre-receive` | Run before refs are updated (server-side) |
| `update` | Run once per ref update (server-side) |
| `proc-receive` | Handle receive-pack commands (server-side) |
| `post-receive` | Run after refs are updated (server-side) |
| `reference-transaction` | Run when reference transaction state changes |
| `push-to-checkout` | Run when a push tries to update the checked-out branch |
| `pre-auto-gc` | Run before automatic garbage collection |
| `post-update` | Run after refs are updated (server-side) |
| `fsmonitor-watchman` | Integration with watchman file monitor |
| `post-index-change` | Run after the index is written |

### Hook argument forwarding

When Git invokes a hook with positional arguments (for example `commit-msg <path>` or
`post-checkout <old> <new> <flag>`), installed git-smee wrappers forward those arguments to
`git smee run`.

On Unix, forwarded hook arguments are available as shell positional parameters inside configured
commands (`$1`, `$2`, ...). Example:

```toml
[[commit-msg]]
command = "test -n \"$1\""
```

## CLI Commands

```bash
git smee init [--force] [--config <path>]       # Initialize a config file
git smee install [--force] [--config <path>]    # Install hooks from the selected config
git smee [--config <path>] run <hook> [hook-args...]           # Run a specific git hook
```

## How it works (high level)

1. You declare hooks in `.git-smee.toml`:

   ```toml
   [[pre-commit]]
   command = "cargo fmt --check"

   [[pre-commit]]
   command = "cargo test"

   [[pre-push]]
   command = "cargo test --all-targets"
   ```

2. git-smee reads this config and knows:

   - which Git hook name (e.g. `pre-commit`) maps to which commands,
   - that each `[[hook-name]]` entry is an ordered `HookDefinition`.

3. The installer will write idempotent scripts into Git's effective hooks directory:

   - Each script runs the installed `git-smee` executable directly with `--config <resolved path> run <hook>`,
     forwarding original Git hook positional arguments.

4. The executor will run the configured commands for that hook and propagate exit codes back to Git.

## Project structure

This repo is a Cargo workspace with two crates:

- `crates/git-smee-core`
  - Library crate with all domain logic:
    - `.git-smee.toml` parsing (`SmeeConfig`, `HookDefinition`)
    - Error types using `thiserror`
    - Installer, executor, platform abstraction
- `crates/git-smee-cli`
  - Binary crate providing the `git smee` CLI:
    - Uses `clap` for argument parsing
    - Thin wrapper that delegates to `git-smee-core`

## Status

Early-stage / experimental. Expect breaking changes while the design settles.

## Development

From the repo root:

```bash
cargo build
cargo test
cargo run -p git-smee-cli -- --help
```

GitHub Actions CI validates Linux (stable/beta/nightly) and Windows (stable) build/test compatibility.

## License

MIT © 2025 Eran Riesenfeld
