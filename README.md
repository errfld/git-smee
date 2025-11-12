# git-smee

[![CI](https://github.com/errfld/git-smee/actions/workflows/ci.yml/badge.svg)](https://github.com/errfld/git-smee/actions/workflows/ci.yml)

git-smee is a lightweight Rust CLI that manages Git hooks from a version-controlled configuration file, `.smee.toml`, in your repository.

Instead of copying hook scripts around or relying on heavy external tooling, git-smee installs small, idempotent hook wrappers that delegate to commands defined in your config—making hook behavior consistent across all contributors.

## Why git-smee?

- Version-controlled: Define hooks once in `.smee.toml`, commit it, share it.
- Cross-platform minded: Designed to work on Unix and Windows (Git Bash) via a clear platform abstraction.
- Small and focused: Single-purpose binary, no plugin ecosystem or extra runtime.

## How it works (high level)

1. You declare hooks in `.smee.toml`:

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
    - `.smee.toml` parsing (`SmeeConfig`, `HookDefinition`)
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
