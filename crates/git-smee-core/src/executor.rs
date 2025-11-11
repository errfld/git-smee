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
