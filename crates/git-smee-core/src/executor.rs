use std::{
    env,
    io::{self, ErrorKind, Write},
    process::Stdio,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::path::PathBuf;

use rayon::iter::IntoParallelRefIterator;
use rayon::prelude::*;
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
    total_configured: usize,
    total_duration: Duration,
    command_runs: Vec<CommandRun>,
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
                "  sequential: {} attempted, {} failed",
                self.phase_attempted_count(CommandPhase::Sequential),
                self.phase_failed_count(CommandPhase::Sequential),
            ),
            format!(
                "  parallel: {} attempted, {} failed",
                self.phase_attempted_count(CommandPhase::Parallel),
                self.phase_failed_count(CommandPhase::Parallel),
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
    phase: CommandPhase,
    index: usize,
    duration: Duration,
    outcome: CommandOutcome,
}

impl CommandRun {
    const fn phase_sort_key(&self) -> usize {
        match self.phase {
            CommandPhase::Sequential => 0,
            CommandPhase::Parallel => 1,
        }
    }

    fn status_display(&self) -> String {
        match &self.outcome {
            CommandOutcome::Success => "ok".to_string(),
            CommandOutcome::Exit(code) => format!("failed with code {code}"),
            CommandOutcome::Signal => "terminated by signal".to_string(),
            CommandOutcome::SpawnFailed { .. } => "spawn failed".to_string(),
            CommandOutcome::NoCommandDefined => "no command defined".to_string(),
        }
    }

    fn failure_display(&self) -> String {
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

    fn to_error(&self) -> Option<Error> {
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
enum CommandOutcome {
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
    const fn is_failure(&self) -> bool {
        !matches!(self, Self::Success)
    }
}

trait CommandRunner: Sync {
    fn run(
        &self,
        command: &str,
        hook_args: &[String],
        stdin_payload: Option<&[u8]>,
    ) -> Result<Option<i32>, std::io::Error>;
    fn shell_display(&self) -> &'static str;
}

struct PlatformCommandRunner<'a> {
    platform: &'a Platform,
}

impl CommandRunner for PlatformCommandRunner<'_> {
    fn run(
        &self,
        command: &str,
        hook_args: &[String],
        stdin_payload: Option<&[u8]>,
    ) -> Result<Option<i32>, std::io::Error> {
        let mut shell_command = self.platform.create_command();
        shell_command.arg(command);
        apply_hook_arg_env(&mut shell_command, hook_args);
        match self.platform {
            Platform::Unix => {
                shell_command.arg("--");
                shell_command.args(hook_args);
            }
            Platform::Windows => {}
        }
        if stdin_payload.is_some() {
            shell_command.stdin(Stdio::piped());
        }

        #[cfg(windows)]
        if let Some(current_dir) = cmd_compatible_current_dir()? {
            shell_command.current_dir(current_dir);
        }

        let mut child = shell_command.spawn()?;
        if let Some(stdin_payload) = stdin_payload {
            let Some(mut stdin) = child.stdin.take() else {
                return child.wait().map(|status| status.code());
            };
            let stdin_payload = stdin_payload.to_vec();
            let stdin_writer = thread::spawn(move || {
                match stdin.write_all(&stdin_payload) {
                    // Hook commands are allowed to ignore or close stdin early. If the
                    // command exits successfully, a broken pipe while replaying the
                    // buffered hook payload should not fail the hook run.
                    Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
                    result => result,
                }
            });
            let wait_result = child.wait().map(|status| status.code());
            let stdin_result = stdin_writer
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("stdin writer thread panicked")));
            stdin_result?;
            wait_result
        } else {
            child.wait().map(|status| status.code())
        }
    }

    fn shell_display(&self) -> &'static str {
        self.platform.shell_display()
    }
}

fn apply_hook_arg_env(shell_command: &mut std::process::Command, hook_args: &[String]) {
    for (key, _) in env::vars_os() {
        if is_hook_arg_env_key(&key.to_string_lossy()) {
            #[cfg(windows)]
            shell_command.env_remove(key.to_string_lossy().to_ascii_uppercase());
            #[cfg(not(windows))]
            shell_command.env_remove(key);
        }
    }

    shell_command.env("GIT_SMEE_HOOK_ARGC", hook_args.len().to_string());
    for (index, arg) in hook_args.iter().enumerate() {
        shell_command.env(format!("GIT_SMEE_HOOK_ARG_{}", index + 1), arg);
    }
}

#[cfg(windows)]
fn cmd_compatible_current_dir() -> Result<Option<PathBuf>, std::io::Error> {
    let current_dir = env::current_dir()?;
    let current_dir = current_dir.to_string_lossy();
    Ok(current_dir.strip_prefix(r"\\?\").map(PathBuf::from))
}

fn is_hook_arg_env_key(key: &str) -> bool {
    if key.eq_ignore_ascii_case("GIT_SMEE_HOOK_ARGC") {
        return true;
    }

    let prefix = "GIT_SMEE_HOOK_ARG_";
    let Some(prefix_part) = key.get(..prefix.len()) else {
        return false;
    };
    if !prefix_part.eq_ignore_ascii_case(prefix) {
        return false;
    }
    let suffix = &key[prefix.len()..];
    !suffix.is_empty() && !suffix.starts_with('0') && suffix.chars().all(|ch| ch.is_ascii_digit())
}

type IndexedHook<'a> = (usize, &'a HookDefinition);

fn run_hooks_with_runner<R: CommandRunner>(
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

fn run_hooks_with_runner_with_summary<R: CommandRunner>(
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

    if !failed {
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
        parallel_runs.sort_by_key(|run| run.index);
        command_runs.extend(parallel_runs);
    }

    HookRunSummary {
        total_configured: hooks.len(),
        total_duration: started.elapsed(),
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

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

#[cfg(test)]
fn execute_command(
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

fn redact_command(command: &str) -> String {
    let tokens = tokenize_command(command);
    let executable_index = tokens
        .iter()
        .position(|token| !is_inline_env_assignment(token));
    let mut redacted = executable_index
        .and_then(|index| tokens.get(index))
        .cloned()
        .unwrap_or_else(|| "<redacted>".to_string());
    if redacted.chars().count() > 80 {
        redacted = redacted.chars().take(77).collect();
        redacted.push_str("...");
    }
    if let Some(index) = executable_index
        && tokens.len() > index + 1
    {
        redacted.push_str(" <args redacted>");
    }
    redacted
}

fn is_inline_env_assignment(token: &str) -> bool {
    let Some((key, _)) = token.split_once('=') else {
        return false;
    };
    is_valid_env_var_name(key)
}

fn is_valid_env_var_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, 'A'..='Z' | 'a'..='z' | '_') {
        return false;
    }
    chars.all(|ch| matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_'))
}

fn tokenize_command(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' if !in_single_quotes => {
                current.push(ch);
                if let Some(next) = chars.peek().copied()
                    && (next.is_whitespace() || matches!(next, '\\' | '\'' | '"'))
                {
                    current.push(chars.next().expect("peeked char should exist"));
                }
            }
            '\'' if !in_double_quotes => {
                in_single_quotes = !in_single_quotes;
            }
            '"' if !in_single_quotes => {
                in_double_quotes = !in_double_quotes;
            }
            ch if ch.is_whitespace() && !in_single_quotes && !in_double_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        ffi::OsString,
        io,
        process::Command,
        sync::{Arc, Barrier, Mutex},
    };

    use assert2::assert;
    use proptest::prelude::*;

    use crate::{config::HookDefinition, test_support::process_state_lock};

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
