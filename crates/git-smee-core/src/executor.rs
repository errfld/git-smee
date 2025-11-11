use std::process::Command;

use thiserror::Error;

use crate::{SmeeConfig, config::LifeCyclePhase};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Hook execution failed with exit code {0}")]
    ExecutionFailed(i32),
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
    match smee_config.hooks.get(&phase) {
        None => Err(Error::NoHooksConfigured(phase)),
        Some(hooks) => hooks.iter().try_for_each(|hook| {
            let command_parts: Vec<&str> = hook.command.split_whitespace().collect();
            let command = *command_parts.first().ok_or(Error::NoCommandDefined)?;
            Command::new(command)
                .args(command_parts.iter().skip(1))
                .status()?;
            Ok(())
        }),
    }
}

#[cfg(test)]
mod tests {
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
        assert!(matches!(result, Err(Error::NonSuccessfulExitStatus(_))));
    }
}
