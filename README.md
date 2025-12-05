# git-smee

[![CI](https://github.com/errfld/git-smee/actions/workflows/ci.yml/badge.svg)](https://github.com/errfld/git-smee/actions/workflows/ci.yml)

> Smee - the right hand of (Git) hooks

git-smee is a lightweight Rust CLI that manages Git hooks from a version-controlled configuration file, `.git-smee.toml`, in your repository.

Instead of copying hook scripts around or relying on heavy external tooling, git-smee installs small, idempotent hook wrappers that delegate to commands defined in your config—making hook behavior consistent across all contributors.

## Why git-smee?

- **Version-controlled**: Define hooks once in `.git-smee.toml`, commit it, share it.
- **Cross-platform minded**: Designed to work on Unix and Windows (Git Bash) via a clear platform abstraction.
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

That's it! Your hooks are now active. When Git triggers a hook, it calls `git smee run <hook>`, which executes the configured commands in order.

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
| `parallel_execution_allowed` | bool | no | *(Planned)* Allow parallel execution of commands (default: `false`) |

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
| `reference-transaction` | Run when reference transaction state changes |
| `push-to-checkout` | Run when a push tries to update the checked-out branch |
| `pre-auto-gc` | Run before automatic garbage collection |
| `post-update` | Run after refs are updated (server-side) |
| `fsmonitor-watchman` | Integration with watchman file monitor |
| `post-index-change` | Run after the index is written |

## CLI Commands

```bash
git smee init      # Initialize a .git-smee.toml configuration file
git smee install   # Install git hooks from .git-smee.toml into .git/hooks
git smee run <hook> # Run a specific git hook (called by installed hook scripts)
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

3. The installer will write idempotent scripts into `.git/hooks`:

   - Each script calls `git smee run <hook>`.

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

## License

MIT © 2025 Eran Riesenfeld
