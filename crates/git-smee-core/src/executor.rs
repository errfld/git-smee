use rayon::prelude::*;

use rayon::iter::IntoParallelRefIterator;
use thiserror::Error;

use crate::{
    SmeeConfig,
    config::{HookDefinition, LifeCyclePhase},
    platform::Platform,
};

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
    execute_hook_with_platform(smee_config, phase, Platform::current())
}

pub fn execute_hook_with_platform(
    smee_config: &SmeeConfig,
    phase: LifeCyclePhase,
    platform: Platform,
) -> Result<(), Error> {
    let runner = PlatformCommandRunner {
        platform: &platform,
    };
    execute_hook_with_runner(smee_config, phase, &runner)
}

fn execute_hook_with_runner<R: CommandRunner>(
    smee_config: &SmeeConfig,
    phase: LifeCyclePhase,
    runner: &R,
) -> Result<(), Error> {
    match smee_config.hooks.get(&phase) {
        None => Err(Error::NoHooksConfigured(phase)),
        Some(hooks) => run_hooks_with_runner(hooks, runner),
    }
}

trait CommandRunner: Sync {
    fn run(&self, command: &str) -> Result<Option<i32>, std::io::Error>;
    fn shell_display(&self) -> &'static str;
}

struct PlatformCommandRunner<'a> {
    platform: &'a Platform,
}

impl CommandRunner for PlatformCommandRunner<'_> {
    fn run(&self, command: &str) -> Result<Option<i32>, std::io::Error> {
        self.platform
            .create_command()
            .arg(command)
            .status()
            .map(|status| status.code())
    }

    fn shell_display(&self) -> &'static str {
        self.platform.shell_display()
    }
}

fn run_hooks_with_runner<R: CommandRunner>(
    hooks: &[HookDefinition],
    runner: &R,
) -> Result<(), Error> {
    let (parallel_hooks, sequential_hooks): (Vec<&HookDefinition>, Vec<&HookDefinition>) = (
        hooks
            .iter()
            .filter(|hook| hook.parallel_execution_allowed)
            .collect(),
        hooks
            .iter()
            .filter(|hook| !hook.parallel_execution_allowed)
            .collect(),
    );

    sequential_hooks
        .iter()
        .try_for_each(|&hook| execute_command(&hook.command, runner))?;
    parallel_hooks
        .par_iter()
        .try_for_each(|&hook| execute_command(&hook.command, runner))?;
    Ok(())
}

fn execute_command(command: &str, runner: &impl CommandRunner) -> Result<(), Error> {
    if command.trim().is_empty() {
        return Err(Error::NoCommandDefined);
    }
    let exit_code = runner
        .run(command)
        .map_err(|source| Error::CommandSpawnFailed {
            command: redact_command(command),
            shell: runner.shell_display().to_string(),
            source,
        })?;
    match exit_code {
        Some(0) => Ok(()),
        Some(exit_status_code) => Err(Error::ExecutionFailed(exit_status_code)),
        None => Err(Error::ExecutionTerminatedBySignal),
    }
}

fn redact_command(command: &str) -> String {
    let mut tokens = command.split_whitespace();
    let executable = tokens
        .by_ref()
        .find(|token| !is_inline_env_assignment(token))
        .unwrap_or("<redacted>");
    let mut redacted = executable.to_string();
    if redacted.len() > 80 {
        redacted.truncate(77);
        redacted.push_str("...");
    }
    if tokens.next().is_some() {
        redacted.push_str(" <args redacted>");
    }
    redacted
}

fn is_inline_env_assignment(token: &str) -> bool {
    token.contains('=') && !token.starts_with('-') && !token.contains('/') && !token.contains('\\')
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        io,
        sync::Mutex,
    };

    use assert2::assert;

    use crate::config::HookDefinition;

    use super::*;

    enum PlannedResult {
        Exit(Option<i32>),
        SpawnError(io::ErrorKind),
    }

    #[derive(Default)]
    struct FakeRunnerState {
        outcomes_by_command: HashMap<String, VecDeque<PlannedResult>>,
        default_outcomes: VecDeque<PlannedResult>,
        calls: Vec<String>,
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
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, command: &str) -> Result<Option<i32>, io::Error> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(command.to_string());
            let outcome = state
                .outcomes_by_command
                .get_mut(command)
                .and_then(VecDeque::pop_front)
                .or_else(|| state.default_outcomes.pop_front())
                .unwrap_or_else(|| panic!("no fake outcome configured for command '{command}'"));
            match outcome {
                PlannedResult::Exit(code) => Ok(code),
                PlannedResult::SpawnError(kind) => Err(io::Error::new(kind, "spawn failed")),
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

        let result = execute_hook_with_runner(&config, LifeCyclePhase::PreCommit, &runner);
        assert!(result.is_ok());
        assert_eq!(runner.calls(), vec!["run-pre-commit"]);
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

        let result = execute_hook_with_runner(&config, LifeCyclePhase::PreCommit, &runner);
        assert!(matches!(result, Err(Error::ExecutionFailed(127))));
    }

    #[test]
    fn given_spawn_error_when_executing_then_command_spawn_failed_error_contains_redacted_command()
    {
        let runner = FakeRunner::with_default_outcomes(vec![PlannedResult::SpawnError(
            io::ErrorKind::NotFound,
        )]);

        let result = execute_command("deploy --token super-secret-value", &runner);

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

        let result = execute_command("TOKEN=super-secret API_KEY=123 deploy --arg value", &runner);

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
    fn given_empty_command_when_executing_then_no_command_defined_error() {
        let runner = FakeRunner::with_default_outcomes(vec![]);
        let result = execute_command("   ", &runner);
        assert!(matches!(result, Err(Error::NoCommandDefined)));
    }

    #[test]
    fn given_missing_exit_code_when_executing_then_terminated_by_signal_error() {
        let runner = FakeRunner::with_default_outcomes(vec![PlannedResult::Exit(None)]);
        let result = execute_command("run-hook", &runner);
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

        let result = execute_hook_with_runner(&config, LifeCyclePhase::PreCommit, &runner);

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

        let result = execute_hook_with_runner(&config, LifeCyclePhase::PreCommit, &runner);
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

        let result = run_hooks_with_runner(&hooks, &runner);

        assert!(matches!(result, Err(Error::ExecutionFailed(10))));
        assert_eq!(runner.calls(), vec!["sequential"]);
    }

    #[test]
    fn given_failed_parallel_hook_when_executing_then_error_is_propagated() {
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
            ("parallel-ok", vec![PlannedResult::Exit(Some(0))]),
            ("parallel-fail", vec![PlannedResult::Exit(Some(23))]),
        ]);

        let result = run_hooks_with_runner(&hooks, &runner);
        let calls = runner.calls();

        assert!(matches!(result, Err(Error::ExecutionFailed(23))));
        assert_eq!(calls[0], "sequential");
        assert!(calls.iter().any(|call| call == "parallel-fail"));
    }
}
