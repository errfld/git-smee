use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use rayon::iter::IntoParallelRefIterator;
use rayon::prelude::*;

use crate::config::HookDefinition;

use super::{
    Error,
    redaction::redact_command,
    runner::CommandRunner,
    summary::{CommandOutcome, CommandPhase, CommandRun, HookRunSummary},
};

type IndexedHook<'a> = (usize, &'a HookDefinition);

pub(super) fn run_hooks_with_runner<R: CommandRunner>(
    hooks: &[HookDefinition],
    runner: &R,
    hook_args: &[String],
    stdin_payload: Option<&[u8]>,
) -> Result<(), Error> {
    let summary = run_hooks_with_runner_with_summary(hooks, runner, hook_args, stdin_payload);
    match summary.error() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub(super) fn run_hooks_with_runner_with_summary<R: CommandRunner>(
    hooks: &[HookDefinition],
    runner: &R,
    hook_args: &[String],
    stdin_payload: Option<&[u8]>,
) -> HookRunSummary {
    let started = Instant::now();
    let (parallel_hooks, sequential_hooks): (Vec<IndexedHook<'_>>, Vec<IndexedHook<'_>>) = hooks
        .iter()
        .enumerate()
        .partition(|(_, hook)| hook.parallel_execution_allowed);

    let mut command_runs = Vec::new();
    let mut failed = false;
    let sequential_started = Instant::now();
    for (phase_index, (_, hook)) in sequential_hooks.into_iter().enumerate() {
        if failed {
            break;
        }
        let run = execute_command_record(
            CommandPhase::Sequential,
            phase_index,
            &hook.command,
            runner,
            hook_args,
            stdin_payload,
        );
        failed = run.outcome.is_failure();
        command_runs.push(run);
    }
    let sequential_duration = sequential_started.elapsed();

    let mut parallel_duration = Duration::ZERO;
    if !failed {
        let parallel_started = Instant::now();
        let parallel_runs = Mutex::new(Vec::new());
        let _ = parallel_hooks
            .par_iter()
            .enumerate()
            .try_for_each(|(phase_index, (_, hook))| {
                let run = execute_command_record(
                    CommandPhase::Parallel,
                    phase_index,
                    &hook.command,
                    runner,
                    hook_args,
                    stdin_payload,
                );
                let failed = run.outcome.is_failure();
                lock_command_runs(&parallel_runs).push(run);
                if failed { Err(()) } else { Ok(()) }
            });
        let mut parallel_runs = match parallel_runs.into_inner() {
            Ok(runs) => runs,
            Err(poisoned) => poisoned.into_inner(),
        };
        parallel_duration = parallel_started.elapsed();
        parallel_runs.sort_by_key(|run| run.index);
        command_runs.extend(parallel_runs);
    }

    HookRunSummary {
        total_configured: hooks.len(),
        total_duration: started.elapsed(),
        sequential_duration,
        parallel_duration,
        command_runs,
    }
}

fn lock_command_runs(
    command_runs: &Mutex<Vec<CommandRun>>,
) -> std::sync::MutexGuard<'_, Vec<CommandRun>> {
    match command_runs.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn execute_command_record(
    phase: CommandPhase,
    index: usize,
    command: &str,
    runner: &impl CommandRunner,
    hook_args: &[String],
    stdin_payload: Option<&[u8]>,
) -> CommandRun {
    let started = Instant::now();
    let outcome = if command.trim().is_empty() {
        CommandOutcome::NoCommandDefined
    } else {
        match runner.run(command, hook_args, stdin_payload) {
            Ok(Some(0)) => CommandOutcome::Success,
            Ok(Some(exit_status_code)) => CommandOutcome::Exit(exit_status_code),
            Ok(None) => CommandOutcome::Signal,
            Err(source) => CommandOutcome::SpawnFailed {
                command: redact_command(command),
                shell: runner.shell_display().to_string(),
                source,
            },
        }
    };
    CommandRun {
        phase,
        index,
        duration: started.elapsed(),
        outcome,
    }
}

#[cfg(test)]
pub(super) fn execute_command(
    command: &str,
    runner: &impl CommandRunner,
    hook_args: &[String],
    stdin_payload: Option<&[u8]>,
) -> Result<(), Error> {
    if command.trim().is_empty() {
        return Err(Error::NoCommandDefined);
    }
    let exit_code = runner
        .run(command, hook_args, stdin_payload)
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
