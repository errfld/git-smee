# Git Smee – Copilot Instructions

These guidelines help AI coding agents work effectively in this repo.

## 1. Project overview

- Language: Rust (2024 edition).
- Layout: Cargo workspace with two crates:
  - `crates/git-smee-cli`: binary crate, user-facing CLI.
  - `crates/git-smee-core`: library crate for reusable/core logic.
- Domain: Manage Git hooks from a version-controlled `.smee.toml` in the repo root.
- Goal: Provide a `git smee` CLI that installs and runs hooks defined in `.smee.toml` across platforms.

## 2. Architecture & patterns

- Treat `git-smee-core` as the single source of domain logic. Any new feature should primarily be implemented here.
- Keep `git-smee-cli` responsible for:
  - Parsing CLI arguments (use `clap` derives) and exposing a `git smee` UX.
  - Converting CLI args into typed configs/requests for `git-smee-core`.
  - Handling user-facing output and process exit codes.
- Model core concepts from `Git Smee.md` explicitly in `git-smee-core`:
  - `.smee.toml` config and hook definitions (config DSL).
  - Installer that writes idempotent scripts into `.git/hooks`.
  - Executor that runs configured commands and propagates exit codes.
  - Platform abstraction for cross-platform hook script behavior.
- Prefer small, composable modules in `git-smee-core` (e.g. `config`, `installer`, `executor`, `platform`, `error`) over complex logic in the binary.

## 3. Dependencies & conventions

- Use `clap` (with `derive`) for CLI parsing in `git-smee-cli`.
- In `git-smee-core`, prefer:
  - `serde` + `toml` for `.smee.toml` parsing.
  - `thiserror` for domain error types (e.g. `ConfigError`, `InstallError`).
  - `anyhow` only at the CLI/app boundary when needed.
  - `which` for resolving executables when appropriate.
- Keep domain-centric crates clean: core logic in `git-smee-core`, presentation/UX in `git-smee-cli`.
- Use path dependencies for internal crates (`git-smee-core = { path = "../git-smee-core" }`).
- Keep editions consistent across crates.

## 4. Build, run, and test

- From repo root:
  - Build workspace: `cargo build`
  - Run CLI: `cargo run -p git-smee-cli -- [args]`
  - Run tests: `cargo test`
- When adding new crates or binaries, wire them into the `[workspace]` `members` list.

## 5. Implementation guidance for agents

- Primary goal: support the human author’s Rust learning. Prefer:
  - Explaining options, trade-offs, and idioms.
  - Proposing designs, tasks, and examples.
  - Letting the user implement core code; avoid large unsolicited code drops.
- Follow the architecture from `Git Smee.md`:
  - Implement config parsing, installer, executor, platform, and error handling modules in `git-smee-core` first.
  - Expose a small, well-typed API from `git-smee-core` that `git-smee-cli` calls.
- When adding CLI behaviors:
  - Suggest `clap`-based subcommands (`init`, `add`, `install`, `run`) and how they map into core APIs.
- For cross-platform behavior:
  - Guide design of a `platform` abstraction in `git-smee-core`.
  - Ensure hook scripts in `.git/hooks` are idempotent and delegate to `git smee run <hook>`.
- When the human author changes the architecture (e.g. crate layout, module boundaries), agents must infer the new patterns from the codebase and update this file (and `Git Smee.md` if needed) to match actual practice.

## 6. Style & quality

- Prefer explicit types and clear domain names: `SmeeConfig`, `HookDefinition`, `Installer`, `Executor`, `Platform`.
- Keep `git-smee-core` APIs small and purposeful; expose only what the CLI and tests need.
- Add tests in `git-smee-core` close to the implemented modules (e.g. config parsing, installer idempotence, executor behavior).
- Keep error handling consistent: domain errors via `thiserror` in core, human-friendly messages in CLI.
- Name error enums simply `Error` within each module (`config::Error`, `installer::Error`, etc.); refer to them via qualified paths when needed.
- Module naming: use flat `mod.rs`-less files (`config.rs`, `installer.rs`, etc.) at the crate root; only create subfolders when there are actual submodules.

## 7. Architecture snapshot (current)

- Workspace: root `[workspace]` with `crates/git-smee-cli` (bin) and `crates/git-smee-core` (lib).
- Data flow:
  - `.smee.toml` in repo root → parsed into `SmeeConfig`-like types in `git-smee-core`.
  - `git-smee-cli` parses subcommands (`init`, `add`, `install`, `run`) → calls core APIs.
  - Core installer writes idempotent hook scripts into `.git/hooks` → scripts delegate to `git smee run <hook>`.
  - Core executor runs configured commands with proper exit codes and cross-platform handling.
- Key modules to evolve in `git-smee-core`: `config.rs`, `installer.rs`, `executor.rs`, `platform.rs`, `error.rs`.
- On architectural changes: update this snapshot and `Git Smee.md` so agents use the actual current design.

If any of these assumptions appear incorrect as the project evolves, update this file to match actual patterns before adding new guidelines.

## 8. Git & PR workflow for agents

- When asked to create a PR:
  - Create a dedicated branch with a concise, descriptive name.
  - Make focused commits with clear messages that explain the change.
  - Push the branch to the GitHub remote.
  - Open a pull request targeting `main` with a concise, structured description (summary, changes, motivation, notes).
- Keep changes small and well-scoped; avoid mixing unrelated refactors with feature/CI changes.
