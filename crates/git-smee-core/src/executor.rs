use thiserror::Error;

mod redaction;
mod runner;
mod scheduler;
mod summary;

use crate::{SmeeConfig, config::LifeCyclePhase, platform::Platform};

use runner::{CommandRunner, PlatformCommandRunner};
use scheduler::{run_hooks_with_runner, run_hooks_with_runner_with_summary};
pub use summary::{CommandPhase, CommandRun, HookRunSummary};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Hook execution failed with exit code {0}")]
    ExecutionFailed(i32),
    #[error("Hook execution was terminated by a signal")]
    ExecutionTerminatedBySignal,
    #[error("No hooks configured for lifecycle phase: {0}")]
    NoHooksConfigured(LifeCyclePhase),
    #[error("No command defined")]
    NoCommandDefined,
    #[error("Failed to spawn hook command '{command}' via '{shell}': {source}")]
    CommandSpawnFailed {
        command: String,
        shell: String,
        source: std::io::Error,
    },
}

pub fn execute_hook(smee_config: &SmeeConfig, phase: LifeCyclePhase) -> Result<(), Error> {
    execute_hook_with_args_and_stdin(smee_config, phase, &[], None)
}

pub fn execute_hook_with_args(
    smee_config: &SmeeConfig,
    phase: LifeCyclePhase,
    hook_args: &[String],
) -> Result<(), Error> {
    execute_hook_with_args_and_stdin(smee_config, phase, hook_args, None)
}

pub fn execute_hook_with_args_and_stdin(
    smee_config: &SmeeConfig,
    phase: LifeCyclePhase,
    hook_args: &[String],
    stdin_payload: Option<&[u8]>,
) -> Result<(), Error> {
    execute_hook_with_platform_and_args_and_stdin(
        smee_config,
        phase,
        Platform::current(),
        hook_args,
        stdin_payload,
    )
}

pub fn execute_hook_with_summary(
    smee_config: &SmeeConfig,
    phase: LifeCyclePhase,
    hook_args: &[String],
    stdin_payload: Option<&[u8]>,
) -> Result<HookRunSummary, Error> {
    let platform = Platform::current();
    let runner = PlatformCommandRunner {
        platform: &platform,
    };
    execute_hook_with_runner_and_summary(smee_config, phase, &runner, hook_args, stdin_payload)
}

pub fn execute_hook_with_platform(
    smee_config: &SmeeConfig,
    phase: LifeCyclePhase,
    platform: Platform,
) -> Result<(), Error> {
    execute_hook_with_platform_and_args_and_stdin(smee_config, phase, platform, &[], None)
}

pub fn execute_hook_with_platform_and_args(
    smee_config: &SmeeConfig,
    phase: LifeCyclePhase,
    platform: Platform,
    hook_args: &[String],
) -> Result<(), Error> {
    execute_hook_with_platform_and_args_and_stdin(smee_config, phase, platform, hook_args, None)
}

pub fn execute_hook_with_platform_and_args_and_stdin(
    smee_config: &SmeeConfig,
    phase: LifeCyclePhase,
    platform: Platform,
    hook_args: &[String],
    stdin_payload: Option<&[u8]>,
) -> Result<(), Error> {
    let runner = PlatformCommandRunner {
        platform: &platform,
    };
    execute_hook_with_runner(smee_config, phase, &runner, hook_args, stdin_payload)
}

fn execute_hook_with_runner<R: CommandRunner>(
    smee_config: &SmeeConfig,
    phase: LifeCyclePhase,
    runner: &R,
    hook_args: &[String],
    stdin_payload: Option<&[u8]>,
) -> Result<(), Error> {
    match smee_config.hooks.get(&phase) {
        None => Err(Error::NoHooksConfigured(phase)),
        Some(hooks) => run_hooks_with_runner(hooks, runner, hook_args, stdin_payload),
    }
}

fn execute_hook_with_runner_and_summary<R: CommandRunner>(
    smee_config: &SmeeConfig,
    phase: LifeCyclePhase,
    runner: &R,
    hook_args: &[String],
    stdin_payload: Option<&[u8]>,
) -> Result<HookRunSummary, Error> {
    match smee_config.hooks.get(&phase) {
        None => Err(Error::NoHooksConfigured(phase)),
        Some(hooks) => Ok(run_hooks_with_runner_with_summary(
            hooks,
            runner,
            hook_args,
            stdin_payload,
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        env,
        ffi::OsString,
        io,
        process::Command,
        sync::{Arc, Barrier, Mutex},
        time::Duration,
    };

    use assert2::assert;
    use proptest::prelude::*;

    use crate::{config::HookDefinition, test_support::process_state_lock};

    use super::redaction::redact_command;
    use super::runner::{
        apply_hook_arg_env, is_hook_arg_env_key, windows_cmd_quote_hook_arg, windows_command_script,
    };
    use super::scheduler::execute_command;
    use super::summary::{CommandOutcome, CommandRun};
    use super::*;

    enum PlannedResult {
        Exit(Option<i32>),
        SpawnError(io::ErrorKind),
        Barrier(Arc<Barrier>, Option<i32>),
    }

    #[derive(Default)]
    struct FakeRunnerState {
        outcomes_by_command: HashMap<String, VecDeque<PlannedResult>>,
        default_outcomes: VecDeque<PlannedResult>,
        calls: Vec<String>,
        hook_args_calls: Vec<Vec<String>>,
        stdin_calls: Vec<Option<Vec<u8>>>,
    }

    struct FakeRunner {
        state: Mutex<FakeRunnerState>,
        shell_display: &'static str,
    }

    impl FakeRunner {
        fn with_default_outcomes(outcomes: Vec<PlannedResult>) -> Self {
            Self {
                state: Mutex::new(FakeRunnerState {
                    default_outcomes: outcomes.into(),
                    ..Default::default()
                }),
                shell_display: "test-shell -c",
            }
        }

        fn with_command_outcomes(outcomes_by_command: Vec<(&str, Vec<PlannedResult>)>) -> Self {
            Self {
                state: Mutex::new(FakeRunnerState {
                    outcomes_by_command: outcomes_by_command
                        .into_iter()
                        .map(|(command, outcomes)| (command.to_string(), outcomes.into()))
                        .collect(),
                    ..Default::default()
                }),
                shell_display: "test-shell -c",
            }
        }

        fn calls(&self) -> Vec<String> {
            self.state.lock().unwrap().calls.clone()
        }

        fn hook_args_calls(&self) -> Vec<Vec<String>> {
            self.state.lock().unwrap().hook_args_calls.clone()
        }

        fn stdin_calls(&self) -> Vec<Option<Vec<u8>>> {
            self.state.lock().unwrap().stdin_calls.clone()
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(
            &self,
            command: &str,
            hook_args: &[String],
            stdin_payload: Option<&[u8]>,
        ) -> Result<Option<i32>, io::Error> {
            let outcome = {
                let mut state = self.state.lock().unwrap();
                state.calls.push(command.to_string());
                state.hook_args_calls.push(hook_args.to_vec());
                state.stdin_calls.push(stdin_payload.map(Vec::from));
                state
                    .outcomes_by_command
                    .get_mut(command)
                    .and_then(VecDeque::pop_front)
                    .or_else(|| state.default_outcomes.pop_front())
                    .unwrap_or_else(|| panic!("no fake outcome configured for command '{command}'"))
            };
            match outcome {
                PlannedResult::Exit(code) => Ok(code),
                PlannedResult::SpawnError(kind) => Err(io::Error::new(kind, "spawn failed")),
                PlannedResult::Barrier(barrier, code) => {
                    barrier.wait();
                    Ok(code)
                }
            }
        }

        fn shell_display(&self) -> &'static str {
            self.shell_display
        }
    }

    #[test]
    fn given_empty_smee_config_when_executing_hook_then_no_hooks_configured_error() {
        let config = SmeeConfig {
            hooks: std::collections::HashMap::new(),
        };

        let result = execute_hook(&config, LifeCyclePhase::PreCommit);
        assert!(matches!(
            result,
            Err(Error::NoHooksConfigured(LifeCyclePhase::PreCommit))
        ));
    }

    #[test]
    fn given_single_hook_when_executing_then_command_runs() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            LifeCyclePhase::PreCommit,
            vec![crate::config::HookDefinition {
                command: "run-pre-commit".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };
        let runner = FakeRunner::with_default_outcomes(vec![PlannedResult::Exit(Some(0))]);

        let result =
            execute_hook_with_runner(&config, LifeCyclePhase::PreCommit, &runner, &[], None);
        assert!(result.is_ok());
        assert_eq!(runner.calls(), vec!["run-pre-commit"]);
    }

    #[test]
    fn given_hook_args_when_executing_then_all_commands_receive_forwarded_args() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            LifeCyclePhase::CommitMsg,
            vec![crate::config::HookDefinition {
                command: "check-commit-message".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };
        let runner = FakeRunner::with_default_outcomes(vec![PlannedResult::Exit(Some(0))]);
        let hook_args = vec!["COMMIT_EDITMSG".to_string(), "message".to_string()];

        let result = execute_hook_with_runner(
            &config,
            LifeCyclePhase::CommitMsg,
            &runner,
            &hook_args,
            None,
        );

        assert!(result.is_ok());
        assert_eq!(runner.calls(), vec!["check-commit-message"]);
        assert_eq!(runner.hook_args_calls(), vec![hook_args]);
    }

    #[test]
    fn given_windows_command_script_when_building_then_command_can_read_batch_parameters() {
        let script = windows_command_script("if \"%1\"==\"alpha\" exit /b 0");

        assert_eq!(script, "@echo off\r\nif \"%1\"==\"alpha\" exit /b 0\r\n");
    }

    #[test]
    fn given_windows_hook_arg_with_cmd_metachar_when_quoting_then_it_is_wrapped() {
        assert_eq!(
            windows_cmd_quote_hook_arg("caret^bang!percent%"),
            "\"caret^^bang^!percent%%\""
        );
        assert_eq!(windows_cmd_quote_hook_arg("plain-ref"), "plain-ref");
    }

    #[test]
    fn given_windows_hook_arg_with_expansion_syntax_when_quoting_then_it_stays_literal() {
        assert_eq!(windows_cmd_quote_hook_arg("%PATH%"), "\"%%PATH%%\"");
        assert_eq!(windows_cmd_quote_hook_arg("!PATH!"), "\"^!PATH^!\"");
    }

    #[test]
    fn given_windows_hook_arg_with_space_when_quoting_then_it_is_wrapped() {
        assert_eq!(windows_cmd_quote_hook_arg("space value"), "\"space value\"");
    }

    #[test]
    fn given_summary_success_when_rendering_then_counts_phases_and_durations() {
        let hooks = vec![
            HookDefinition {
                command: "seq-ok".to_string(),
                parallel_execution_allowed: false,
            },
            HookDefinition {
                command: "parallel-ok".to_string(),
                parallel_execution_allowed: true,
            },
        ];
        let runner = FakeRunner::with_command_outcomes(vec![
            ("seq-ok", vec![PlannedResult::Exit(Some(0))]),
            ("parallel-ok", vec![PlannedResult::Exit(Some(0))]),
        ]);

        let summary = run_hooks_with_runner_with_summary(&hooks, &runner, &[], None);

        assert_eq!(summary.total_configured(), 2);
        assert_eq!(summary.attempted_count(), 2);
        assert_eq!(summary.skipped_count(), 0);
        assert_eq!(summary.failed_count(), 0);
        assert_eq!(summary.phase_attempted_count(CommandPhase::Sequential), 1);
        assert_eq!(summary.phase_failed_count(CommandPhase::Sequential), 0);
        assert_eq!(summary.phase_attempted_count(CommandPhase::Parallel), 1);
        assert_eq!(summary.phase_failed_count(CommandPhase::Parallel), 0);
        assert!(summary.first_failure().is_none());
        assert!(summary.error().is_none());

        let lines = summary.text_lines(LifeCyclePhase::PreCommit).join("\n");
        assert!(lines.contains("Hook summary: pre-commit"));
        assert!(lines.contains("total: 2 attempted, 0 skipped, 0 failed in"));
        assert!(lines.contains("sequential: 1 attempted, 0 failed in"));
        assert!(lines.contains("parallel: 1 attempted, 0 failed in"));
        assert!(lines.contains("sequential command #1: ok in"));
        assert!(lines.contains("parallel command #1: ok in"));
    }

    #[test]
    fn given_summary_sequential_failure_when_rendering_then_reports_skipped_and_first_failure() {
        let hooks = vec![
            HookDefinition {
                command: "seq-fail".to_string(),
                parallel_execution_allowed: false,
            },
            HookDefinition {
                command: "seq-skipped".to_string(),
                parallel_execution_allowed: false,
            },
            HookDefinition {
                command: "parallel-skipped".to_string(),
                parallel_execution_allowed: true,
            },
        ];
        let runner = FakeRunner::with_command_outcomes(vec![(
            "seq-fail",
            vec![PlannedResult::Exit(Some(9))],
        )]);

        let summary = run_hooks_with_runner_with_summary(&hooks, &runner, &[], None);

        assert_eq!(summary.total_configured(), 3);
        assert_eq!(summary.attempted_count(), 1);
        assert_eq!(summary.skipped_count(), 2);
        assert_eq!(summary.failed_count(), 1);
        assert_eq!(summary.phase_attempted_count(CommandPhase::Sequential), 1);
        assert_eq!(summary.phase_failed_count(CommandPhase::Sequential), 1);
        assert_eq!(summary.phase_attempted_count(CommandPhase::Parallel), 0);
        assert!(matches!(summary.error(), Some(Error::ExecutionFailed(9))));

        let first_failure = summary.first_failure().expect("missing first failure");
        assert_eq!(first_failure.status_display(), "failed with code 9");
        assert_eq!(
            first_failure.failure_display(),
            "sequential command #1 exited with code 9"
        );
    }

    #[test]
    fn given_summary_parallel_failures_when_rendering_then_first_failure_is_phase_ordered() {
        let summary = HookRunSummary {
            total_configured: 2,
            total_duration: Duration::ZERO,
            sequential_duration: Duration::ZERO,
            parallel_duration: Duration::ZERO,
            command_runs: vec![
                CommandRun {
                    phase: CommandPhase::Parallel,
                    index: 1,
                    duration: Duration::ZERO,
                    outcome: CommandOutcome::Exit(7),
                },
                CommandRun {
                    phase: CommandPhase::Parallel,
                    index: 0,
                    duration: Duration::ZERO,
                    outcome: CommandOutcome::Exit(5),
                },
            ],
        };

        assert_eq!(summary.failed_count(), 2);
        assert_eq!(summary.phase_failed_count(CommandPhase::Parallel), 2);
        let first_failure = summary.first_failure().expect("missing first failure");
        assert_eq!(
            first_failure.failure_display(),
            "parallel command #1 exited with code 5"
        );
        let lines = summary.text_lines(LifeCyclePhase::PreCommit).join("\n");
        assert!(lines.contains("first failure: parallel command #1 exited with code 5"));
    }

    #[test]
    fn given_spawn_failure_when_rendering_summary_then_status_and_error_are_redacted() {
        let hooks = vec![HookDefinition {
            command: "SECRET=value deploy --token super-secret-value".to_string(),
            parallel_execution_allowed: false,
        }];
        let runner = FakeRunner::with_default_outcomes(vec![PlannedResult::SpawnError(
            io::ErrorKind::NotFound,
        )]);

        let summary = run_hooks_with_runner_with_summary(&hooks, &runner, &[], None);

        let first_failure = summary.first_failure().expect("missing first failure");
        assert_eq!(first_failure.status_display(), "spawn failed");
        assert!(
            first_failure
                .failure_display()
                .contains("deploy <args redacted>")
        );
        assert!(
            !first_failure
                .failure_display()
                .contains("super-secret-value")
        );
        assert!(matches!(
            summary.error(),
            Some(Error::CommandSpawnFailed { .. })
        ));
    }

    #[test]
    fn given_stdin_payload_when_executing_then_each_command_receives_the_same_bytes() {
        let hooks = vec![
            HookDefinition {
                command: "first".to_string(),
                parallel_execution_allowed: false,
            },
            HookDefinition {
                command: "second".to_string(),
                parallel_execution_allowed: false,
            },
        ];
        let runner = FakeRunner::with_command_outcomes(vec![
            ("first", vec![PlannedResult::Exit(Some(0))]),
            ("second", vec![PlannedResult::Exit(Some(0))]),
        ]);
        let stdin_payload = b"refs/heads/main 0123456789 refs/heads/main abcdef0123\n";

        let result = run_hooks_with_runner(&hooks, &runner, &[], Some(stdin_payload));

        assert!(result.is_ok());
        assert_eq!(
            runner.stdin_calls(),
            vec![Some(stdin_payload.to_vec()), Some(stdin_payload.to_vec()),]
        );
    }

    #[test]
    fn given_mixed_case_hook_arg_env_when_applying_then_existing_entries_are_removed() {
        let _guard = process_state_lock();
        // SAFETY: test serializes process environment mutation via process_state_lock.
        unsafe {
            env::set_var("Git_Smee_Hook_Arg_2", "stale");
            env::set_var("GIT_SMEE_HOOK_ARGC", "7");
        }

        let mut command = Command::new("echo");
        let hook_args = vec!["alpha".to_string(), "beta".to_string()];
        apply_hook_arg_env(&mut command, &hook_args);

        let envs: Vec<(OsString, Option<OsString>)> = command
            .get_envs()
            .map(|(key, value)| (key.to_os_string(), value.map(OsString::from)))
            .collect();

        assert!(!envs.iter().any(|(key, value)| {
            is_hook_arg_env_key(&key.to_string_lossy())
                && value.as_ref() == Some(&OsString::from("stale"))
        }));
        assert!(envs.iter().any(|(key, value)| {
            key == "GIT_SMEE_HOOK_ARGC" && value.as_ref() == Some(&OsString::from("2"))
        }));
        assert!(envs.iter().any(|(key, value)| {
            key == "GIT_SMEE_HOOK_ARG_1" && value.as_ref() == Some(&OsString::from("alpha"))
        }));
        assert!(envs.iter().any(|(key, value)| {
            key == "GIT_SMEE_HOOK_ARG_2" && value.as_ref() == Some(&OsString::from("beta"))
        }));

        // SAFETY: test serializes process environment mutation via process_state_lock.
        unsafe {
            env::remove_var("Git_Smee_Hook_Arg_2");
            env::remove_var("GIT_SMEE_HOOK_ARGC");
        }
    }

    #[test]
    fn given_user_prefixed_hook_arg_env_when_applying_then_unrelated_entries_are_preserved() {
        let _guard = process_state_lock();
        // SAFETY: test serializes process environment mutation via process_state_lock.
        unsafe {
            env::set_var("GIT_SMEE_HOOK_ARGS_FILE", "user-owned");
            env::set_var("GIT_SMEE_HOOK_ARGUMENT_MODE", "strict");
            env::set_var("GIT_SMEE_HOOK_ARG_2", "stale");
        }

        let mut command = Command::new("echo");
        let hook_args = vec!["fresh".to_string()];
        apply_hook_arg_env(&mut command, &hook_args);

        let envs: Vec<(OsString, Option<OsString>)> = command
            .get_envs()
            .map(|(key, value)| (key.to_os_string(), value.map(OsString::from)))
            .collect();

        assert!(
            !envs
                .iter()
                .any(|(key, value)| { key == "GIT_SMEE_HOOK_ARGS_FILE" && value.is_none() })
        );
        assert!(
            !envs
                .iter()
                .any(|(key, value)| { key == "GIT_SMEE_HOOK_ARGUMENT_MODE" && value.is_none() })
        );
        assert!(
            envs.iter()
                .any(|(key, value)| { key == "GIT_SMEE_HOOK_ARG_2" && value.is_none() })
        );

        // SAFETY: test serializes process environment mutation via process_state_lock.
        unsafe {
            env::remove_var("GIT_SMEE_HOOK_ARGS_FILE");
            env::remove_var("GIT_SMEE_HOOK_ARGUMENT_MODE");
            env::remove_var("GIT_SMEE_HOOK_ARG_2");
        }
    }

    #[test]
    fn given_hook_arg_contract_keys_when_matching_then_only_argc_and_numbered_args_match() {
        assert!(is_hook_arg_env_key("GIT_SMEE_HOOK_ARGC"));
        assert!(is_hook_arg_env_key("git_smee_hook_arg_12"));
        assert!(!is_hook_arg_env_key("GIT_SMEE_HOOK_ARG"));
        assert!(!is_hook_arg_env_key("GIT_SMEE_HOOK_ARG_"));
        assert!(!is_hook_arg_env_key("GIT_SMEE_HOOK_ARG_0"));
        assert!(!is_hook_arg_env_key("GIT_SMEE_HOOK_ARG_01"));
        assert!(!is_hook_arg_env_key("GIT_SMEE_HOOK_ARG_0x1"));
        assert!(!is_hook_arg_env_key("GIT_SMEE_HOOK_ARGS_FILE"));
        assert!(!is_hook_arg_env_key("GIT_SMEE_HOOK_ARGUMENT_MODE"));
    }

    #[test]
    fn given_empty_hook_args_when_applying_then_argc_is_zero_and_no_indexed_args_are_added() {
        let _guard = process_state_lock();
        let mut command = Command::new("echo");

        apply_hook_arg_env(&mut command, &[]);

        let envs: Vec<(OsString, Option<OsString>)> = command
            .get_envs()
            .map(|(key, value)| (key.to_os_string(), value.map(OsString::from)))
            .collect();

        assert!(envs.iter().any(|(key, value)| {
            key == "GIT_SMEE_HOOK_ARGC" && value.as_ref() == Some(&OsString::from("0"))
        }));
        assert!(!envs.iter().any(|(key, value)| {
            value.is_some()
                && is_hook_arg_env_key(&key.to_string_lossy())
                && key != "GIT_SMEE_HOOK_ARGC"
        }));
    }

    #[test]
    fn given_non_zero_exit_when_executing_then_execution_failed_error() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            LifeCyclePhase::PreCommit,
            vec![crate::config::HookDefinition {
                command: "hook command".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };
        let runner = FakeRunner::with_default_outcomes(vec![PlannedResult::Exit(Some(127))]);

        let result =
            execute_hook_with_runner(&config, LifeCyclePhase::PreCommit, &runner, &[], None);
        assert!(matches!(result, Err(Error::ExecutionFailed(127))));
    }

    #[test]
    fn given_spawn_error_when_executing_then_command_spawn_failed_error_contains_redacted_command()
    {
        let runner = FakeRunner::with_default_outcomes(vec![PlannedResult::SpawnError(
            io::ErrorKind::NotFound,
        )]);

        let result = execute_command("deploy --token super-secret-value", &runner, &[], None);

        match result {
            Err(Error::CommandSpawnFailed {
                command,
                shell,
                source,
            }) => {
                assert_eq!(command, "deploy <args redacted>");
                assert_eq!(shell, "test-shell -c");
                assert_eq!(source.kind(), io::ErrorKind::NotFound);
            }
            _ => panic!("expected CommandSpawnFailed"),
        }
    }

    #[test]
    fn given_spawn_error_with_env_prefix_when_executing_then_redaction_hides_env_assignments() {
        let runner = FakeRunner::with_default_outcomes(vec![PlannedResult::SpawnError(
            io::ErrorKind::NotFound,
        )]);

        let result = execute_command(
            "TOKEN=super-secret API_KEY=123 deploy --arg value",
            &runner,
            &[],
            None,
        );

        match result {
            Err(Error::CommandSpawnFailed { command, shell, .. }) => {
                assert_eq!(command, "deploy <args redacted>");
                assert_eq!(shell, "test-shell -c");
                assert!(!command.contains("super-secret"));
                assert!(!command.contains("API_KEY"));
            }
            _ => panic!("expected CommandSpawnFailed"),
        }
    }

    #[test]
    fn given_spawn_error_with_quoted_env_assignment_then_redaction_keeps_executable_only() {
        let runner = FakeRunner::with_default_outcomes(vec![PlannedResult::SpawnError(
            io::ErrorKind::NotFound,
        )]);

        let result = execute_command(
            "TOKEN=\"super secret\" API_KEY='another secret' ./deploy --arg value",
            &runner,
            &[],
            None,
        );

        match result {
            Err(Error::CommandSpawnFailed { command, .. }) => {
                assert_eq!(command, "./deploy <args redacted>");
                assert!(!command.contains("super secret"));
                assert!(!command.contains("another secret"));
                assert!(!command.contains("API_KEY"));
            }
            _ => panic!("expected CommandSpawnFailed"),
        }
    }

    #[test]
    fn given_command_without_env_prefix_when_redacting_then_executable_is_preserved() {
        let redacted = redact_command("deploy --token super-secret-value");
        assert_eq!(redacted, "deploy <args redacted>");
    }

    #[test]
    fn given_long_executable_when_redacting_then_name_is_truncated() {
        let executable = "a".repeat(120);
        let command = format!("{executable} --flag");

        let redacted = redact_command(&command);

        assert_eq!(redacted, format!("{}... <args redacted>", "a".repeat(77)));
    }

    #[test]
    fn given_long_unicode_executable_when_redacting_then_name_is_truncated_without_panicking() {
        let executable = "ä".repeat(120);
        let command = format!("{executable} --flag");

        let redacted = redact_command(&command);

        assert_eq!(redacted, format!("{}... <args redacted>", "ä".repeat(77)));
    }

    #[test]
    fn given_windows_style_path_when_redacting_then_backslashes_are_preserved() {
        let redacted =
            redact_command(r#""C:\Program Files\Git\bin\bash.exe" -lc "echo secret-value""#);

        assert_eq!(
            redacted,
            r#"C:\Program Files\Git\bin\bash.exe <args redacted>"#
        );
    }

    #[test]
    fn given_escaped_spaces_when_redacting_then_backslashes_are_preserved() {
        let redacted = redact_command(r#"./path\ with\ spaces/tool --token secret-value"#);

        assert_eq!(redacted, r#"./path\ with\ spaces/tool <args redacted>"#);
    }

    #[test]
    fn given_empty_command_when_executing_then_no_command_defined_error() {
        let runner = FakeRunner::with_default_outcomes(vec![]);
        let result = execute_command("   ", &runner, &[], None);
        assert!(matches!(result, Err(Error::NoCommandDefined)));
    }

    #[test]
    fn given_missing_exit_code_when_executing_then_terminated_by_signal_error() {
        let runner = FakeRunner::with_default_outcomes(vec![PlannedResult::Exit(None)]);
        let result = execute_command("run-hook", &runner, &[], None);
        assert!(matches!(result, Err(Error::ExecutionTerminatedBySignal)));
    }

    #[test]
    fn given_multiple_commands_when_parallel_execution_then_execution_succeeds() {
        let mut hooks_map = HashMap::new();
        hooks_map.insert(
            LifeCyclePhase::PreCommit,
            ["parallel-1", "parallel-2", "parallel-3", "parallel-4"]
                .iter()
                .map(|command| HookDefinition {
                    command: command.to_string(),
                    parallel_execution_allowed: true,
                })
                .collect(),
        );
        let config = SmeeConfig { hooks: hooks_map };
        let runner = FakeRunner::with_command_outcomes(vec![
            ("parallel-1", vec![PlannedResult::Exit(Some(0))]),
            ("parallel-2", vec![PlannedResult::Exit(Some(0))]),
            ("parallel-3", vec![PlannedResult::Exit(Some(0))]),
            ("parallel-4", vec![PlannedResult::Exit(Some(0))]),
        ]);

        let result =
            execute_hook_with_runner(&config, LifeCyclePhase::PreCommit, &runner, &[], None);

        assert!(result.is_ok());
        let mut calls = runner.calls();
        calls.sort();
        assert_eq!(
            calls,
            vec![
                "parallel-1".to_string(),
                "parallel-2".to_string(),
                "parallel-3".to_string(),
                "parallel-4".to_string()
            ]
        );
    }

    #[test]
    fn given_multiple_commands_when_parallel_and_sequential_execution_then_sequential_runs_first() {
        let mut hooks_map = HashMap::new();
        let mut hook_definitions: Vec<HookDefinition> = ["parallel-1", "parallel-2", "parallel-3"]
            .iter()
            .map(|command| HookDefinition {
                command: command.to_string(),
                parallel_execution_allowed: true,
            })
            .collect();
        hook_definitions.push(HookDefinition {
            command: "sequential-1".to_string(),
            parallel_execution_allowed: false,
        });
        hook_definitions.push(HookDefinition {
            command: "sequential-2".to_string(),
            parallel_execution_allowed: false,
        });

        hooks_map.insert(LifeCyclePhase::PreCommit, hook_definitions);
        let config = SmeeConfig { hooks: hooks_map };
        let runner = FakeRunner::with_command_outcomes(vec![
            ("sequential-1", vec![PlannedResult::Exit(Some(0))]),
            ("sequential-2", vec![PlannedResult::Exit(Some(0))]),
            ("parallel-1", vec![PlannedResult::Exit(Some(0))]),
            ("parallel-2", vec![PlannedResult::Exit(Some(0))]),
            ("parallel-3", vec![PlannedResult::Exit(Some(0))]),
        ]);

        let result =
            execute_hook_with_runner(&config, LifeCyclePhase::PreCommit, &runner, &[], None);
        let calls = runner.calls();

        assert!(result.is_ok());
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[0], "sequential-1");
        assert_eq!(calls[1], "sequential-2");

        let mut parallel_calls = calls[2..].to_vec();
        parallel_calls.sort();
        assert_eq!(
            parallel_calls,
            vec![
                "parallel-1".to_string(),
                "parallel-2".to_string(),
                "parallel-3".to_string()
            ]
        );
    }

    #[test]
    fn given_failed_sequential_hook_when_executing_then_parallel_hooks_do_not_run() {
        let hooks = vec![
            HookDefinition {
                command: "sequential".to_string(),
                parallel_execution_allowed: false,
            },
            HookDefinition {
                command: "parallel".to_string(),
                parallel_execution_allowed: true,
            },
        ];
        let runner = FakeRunner::with_command_outcomes(vec![
            ("sequential", vec![PlannedResult::Exit(Some(10))]),
            ("parallel", vec![PlannedResult::Exit(Some(0))]),
        ]);

        let result = run_hooks_with_runner(&hooks, &runner, &[], None);

        assert!(matches!(result, Err(Error::ExecutionFailed(10))));
        assert_eq!(runner.calls(), vec!["sequential"]);
    }

    #[test]
    fn given_failed_parallel_hook_when_executing_then_in_flight_parallel_hooks_may_complete() {
        let barrier = Arc::new(Barrier::new(2));
        let hooks = vec![
            HookDefinition {
                command: "sequential".to_string(),
                parallel_execution_allowed: false,
            },
            HookDefinition {
                command: "parallel-ok".to_string(),
                parallel_execution_allowed: true,
            },
            HookDefinition {
                command: "parallel-fail".to_string(),
                parallel_execution_allowed: true,
            },
        ];
        let runner = FakeRunner::with_command_outcomes(vec![
            ("sequential", vec![PlannedResult::Exit(Some(0))]),
            (
                "parallel-ok",
                vec![PlannedResult::Barrier(barrier.clone(), Some(0))],
            ),
            (
                "parallel-fail",
                vec![PlannedResult::Barrier(barrier.clone(), Some(23))],
            ),
        ]);

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .expect("test thread pool should build");
        let result = pool.install(|| run_hooks_with_runner(&hooks, &runner, &[], None));
        let calls = runner.calls();

        assert!(matches!(result, Err(Error::ExecutionFailed(23))));
        assert_eq!(calls[0], "sequential");
        assert!(calls.iter().any(|call| call == "parallel-ok"));
        assert!(calls.iter().any(|call| call == "parallel-fail"));
    }

    proptest! {
        #[test]
        fn redact_command_never_panics_for_arbitrary_input(command in any::<String>()) {
            let _ = redact_command(&command);
        }

        #[test]
        fn redact_command_hides_inline_env_secret_values(
            key in "[A-Z_][A-Z0-9_]{0,7}",
            secret_segments in prop::collection::vec("[qxz]{3,8}", 1..4),
            executable_suffix in "[A-Z0-9_./:\\\\-]{1,24}"
        ) {
            let secret = secret_segments.join(" ");
            let executable = format!("CMD_{executable_suffix}");
            let command = format!(r#"{key}="{secret}" {executable} --flag trailing"#);

            let redacted = redact_command(&command);

            prop_assert!(!redacted.contains(&secret));
            prop_assert!(redacted.ends_with(" <args redacted>"));
        }
    }
}
