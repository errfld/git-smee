use std::{io, time::Duration};

use crate::config::LifeCyclePhase;

use super::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPhase {
    Sequential,
    Parallel,
}

impl CommandPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Sequential => "sequential",
            Self::Parallel => "parallel",
        }
    }
}

#[derive(Debug)]
pub struct HookRunSummary {
    pub(super) total_configured: usize,
    pub(super) total_duration: Duration,
    pub(super) sequential_duration: Duration,
    pub(super) parallel_duration: Duration,
    pub(super) command_runs: Vec<CommandRun>,
}

impl HookRunSummary {
    pub fn total_configured(&self) -> usize {
        self.total_configured
    }

    pub fn attempted_count(&self) -> usize {
        self.command_runs.len()
    }

    pub fn skipped_count(&self) -> usize {
        self.total_configured.saturating_sub(self.attempted_count())
    }

    pub fn failed_count(&self) -> usize {
        self.command_runs
            .iter()
            .filter(|run| run.outcome.is_failure())
            .count()
    }

    pub fn phase_attempted_count(&self, phase: CommandPhase) -> usize {
        self.command_runs
            .iter()
            .filter(|run| run.phase == phase)
            .count()
    }

    pub fn phase_failed_count(&self, phase: CommandPhase) -> usize {
        self.command_runs
            .iter()
            .filter(|run| run.phase == phase && run.outcome.is_failure())
            .count()
    }

    pub fn first_failure(&self) -> Option<&CommandRun> {
        self.command_runs
            .iter()
            .filter(|run| run.outcome.is_failure())
            .min_by_key(|run| (run.phase_sort_key(), run.index))
    }

    pub fn error(&self) -> Option<Error> {
        self.first_failure().and_then(CommandRun::to_error)
    }

    pub fn text_lines(&self, phase: LifeCyclePhase) -> Vec<String> {
        let mut lines = vec![
            format!("Hook summary: {phase}"),
            format!(
                "  total: {} attempted, {} skipped, {} failed in {}",
                self.attempted_count(),
                self.skipped_count(),
                self.failed_count(),
                format_duration(self.total_duration),
            ),
            format!(
                "  sequential: {} attempted, {} failed in {}",
                self.phase_attempted_count(CommandPhase::Sequential),
                self.phase_failed_count(CommandPhase::Sequential),
                format_duration(self.sequential_duration),
            ),
            format!(
                "  parallel: {} attempted, {} failed in {}",
                self.phase_attempted_count(CommandPhase::Parallel),
                self.phase_failed_count(CommandPhase::Parallel),
                format_duration(self.parallel_duration),
            ),
        ];
        for run in &self.command_runs {
            lines.push(format!(
                "  - {} command #{}: {} in {}",
                run.phase.as_str(),
                run.index + 1,
                run.status_display(),
                format_duration(run.duration),
            ));
        }
        if let Some(first_failure) = self.first_failure() {
            lines.push(format!(
                "  first failure: {}",
                first_failure.failure_display()
            ));
        }
        lines
    }
}

#[derive(Debug)]
pub struct CommandRun {
    pub(super) phase: CommandPhase,
    pub(super) index: usize,
    pub(super) duration: Duration,
    pub(super) outcome: CommandOutcome,
}

impl CommandRun {
    const fn phase_sort_key(&self) -> usize {
        match self.phase {
            CommandPhase::Sequential => 0,
            CommandPhase::Parallel => 1,
        }
    }

    pub(super) fn status_display(&self) -> String {
        match &self.outcome {
            CommandOutcome::Success => "ok".to_string(),
            CommandOutcome::Exit(code) => format!("failed with code {code}"),
            CommandOutcome::Signal => "terminated by signal".to_string(),
            CommandOutcome::SpawnFailed { .. } => "spawn failed".to_string(),
            CommandOutcome::NoCommandDefined => "no command defined".to_string(),
        }
    }

    pub(super) fn failure_display(&self) -> String {
        let prefix = format!("{} command #{}", self.phase.as_str(), self.index + 1);
        match &self.outcome {
            CommandOutcome::Success => format!("{prefix} succeeded"),
            CommandOutcome::Exit(code) => format!("{prefix} exited with code {code}"),
            CommandOutcome::Signal => format!("{prefix} was terminated by a signal"),
            CommandOutcome::SpawnFailed {
                command,
                shell,
                source,
            } => {
                format!("{prefix} failed to spawn '{command}' via '{shell}': {source}")
            }
            CommandOutcome::NoCommandDefined => format!("{prefix} had no command defined"),
        }
    }

    pub(super) fn to_error(&self) -> Option<Error> {
        match &self.outcome {
            CommandOutcome::Success => None,
            CommandOutcome::Exit(code) => Some(Error::ExecutionFailed(*code)),
            CommandOutcome::Signal => Some(Error::ExecutionTerminatedBySignal),
            CommandOutcome::SpawnFailed {
                command,
                shell,
                source,
            } => Some(Error::CommandSpawnFailed {
                command: command.clone(),
                shell: shell.clone(),
                source: io::Error::new(source.kind(), source.to_string()),
            }),
            CommandOutcome::NoCommandDefined => Some(Error::NoCommandDefined),
        }
    }
}

#[derive(Debug)]
pub(super) enum CommandOutcome {
    Success,
    Exit(i32),
    Signal,
    SpawnFailed {
        command: String,
        shell: String,
        source: io::Error,
    },
    NoCommandDefined,
}

impl CommandOutcome {
    pub(super) const fn is_failure(&self) -> bool {
        !matches!(self, Self::Success)
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}
