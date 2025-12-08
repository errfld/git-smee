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
    #[error("Hook execution did not return a code")]
    NoCodeReturned,
    #[error("Non successful exit status: {0}")]
    NonSuccessfulExitStatus(#[from] std::io::Error),
}

pub fn execute_hook(smee_config: &SmeeConfig, phase: LifeCyclePhase) -> Result<(), Error> {
    execute_hook_with_platform(smee_config, phase, Platform::current())
}

pub fn execute_hook_with_platform(
    smee_config: &SmeeConfig,
    phase: LifeCyclePhase,
    platform: Platform,
) -> Result<(), Error> {
    match smee_config.hooks.get(&phase) {
        None => Err(Error::NoHooksConfigured(phase)),
        Some(hooks) => run_hooks(hooks, platform),
    }
}

fn run_hooks(hooks: &[HookDefinition], platform: Platform) -> Result<(), Error> {
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
        .try_for_each(|&hook| execute_command(&hook.command, &platform))?;
    parallel_hooks
        .par_iter()
        .try_for_each(|&hook| execute_command(&hook.command, &platform))?;
    Ok(())
}

fn execute_command(command: &str, platform: &Platform) -> Result<(), Error> {
    if command.is_empty() {
        return Err(Error::NoCommandDefined);
    }
    let exit_status = platform.create_command().arg(command).status()?;
    if !exit_status.success() {
        return match exit_status.code() {
            Some(exit_status_code) => Err(Error::ExecutionFailed(exit_status_code)),
            None => Err(Error::ExecutionTerminatedBySignal),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        time::{Duration, Instant},
    };

    use assert2::assert;

    use crate::config::HookDefinition;

    use super::*;

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
    fn given_simple_smee_config_when_executing_hook_then_command_executed() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            LifeCyclePhase::PreCommit,
            vec![crate::config::HookDefinition {
                command: "echo Pre-commit hook executed".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };

        let result = execute_hook(&config, LifeCyclePhase::PreCommit);
        assert!(result.is_ok());
    }

    #[test]
    fn given_invalid_command_when_executing_hook_then_execution_failed_error() {
        let mut hooks_map = std::collections::HashMap::new();
        hooks_map.insert(
            LifeCyclePhase::PreCommit,
            vec![crate::config::HookDefinition {
                command: "nonexistent_command".to_string(),
                parallel_execution_allowed: false,
            }],
        );
        let config = SmeeConfig { hooks: hooks_map };

        let result = execute_hook(&config, LifeCyclePhase::PreCommit);
        assert!(matches!(result, Err(Error::ExecutionFailed(_))));
    }

    #[test]
    fn given_multiple_commands_when_parallel_execution_then_execution_succeeds() {
        let mut hooks_map = HashMap::new();
        hooks_map.insert(
            LifeCyclePhase::PreCommit,
            (1..10)
                .map(|_| HookDefinition {
                    command: "sleep 0.1".to_string(),
                    parallel_execution_allowed: true,
                })
                .collect(),
        );
        let config = SmeeConfig { hooks: hooks_map };

        let start_time = Instant::now();
        let result = execute_hook(&config, LifeCyclePhase::PreCommit);
        let end_time = Instant::now();

        assert!(result.is_ok());
        assert!((end_time - start_time) < Duration::from_millis(500));
        assert!((end_time - start_time) > Duration::from_millis(100));
    }

    #[test]
    fn given_multiple_commands_when_parallel_and_sequential_execution_then_execution_succeeds() {
        let mut hooks_map = HashMap::new();
        let mut hook_definitions: Vec<HookDefinition> = (1..10)
            .map(|_| HookDefinition {
                command: "sleep 0.1".to_string(),
                parallel_execution_allowed: true,
            })
            .collect();
        hook_definitions.push(HookDefinition {
            command: "sleep 0.5".to_string(),
            parallel_execution_allowed: false,
        });

        hooks_map.insert(LifeCyclePhase::PreCommit, hook_definitions);
        let config = SmeeConfig { hooks: hooks_map };

        let start_time = Instant::now();
        let result = execute_hook(&config, LifeCyclePhase::PreCommit);
        let end_time = Instant::now();

        assert!(result.is_ok());
        assert!((end_time - start_time) < Duration::from_millis(1000));
        assert!((end_time - start_time) > Duration::from_millis(500));
    }
}
