use std::{
    env,
    io::{self, IsTerminal, Read},
    path::Path,
    str::FromStr,
};

use git_smee_core::{config::LifeCyclePhase, executor, repository};

use crate::config_path::read_config_file;

const DEFAULT_MAX_HOOK_STDIN_BYTES: u64 = 10 * 1024 * 1024;
const HOOK_STDIN_LIMIT_ENV: &str = "GIT_SMEE_HOOK_STDIN_LIMIT_BYTES";
const DEFAULT_HOOK_STDIN_LIMIT_DISPLAY: &str = "10 MiB";

pub(crate) fn run_hook(
    config_path: &Path,
    hook: &str,
    hook_args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    repository::ensure_in_repo_root()?;
    let phase = LifeCyclePhase::from_str(hook)?;
    let stdin_payload = read_hook_stdin_for_phase(phase)?;
    let config = read_config_file(config_path)?;
    let summary =
        executor::execute_hook_with_summary(&config, phase, hook_args, stdin_payload.as_deref())?;
    for line in summary.text_lines(phase) {
        println!("{line}");
    }
    if let Some(error) = summary.error() {
        return Err(Box::new(error));
    }
    Ok(())
}

fn read_hook_stdin_for_phase(phase: LifeCyclePhase) -> io::Result<Option<Vec<u8>>> {
    // proc-receive is an interactive pkt-line protocol: Git waits for the hook to
    // answer before closing stdin, so buffering until EOF would deadlock before
    // the configured command is spawned. Let the command inherit stdin instead.
    if phase == LifeCyclePhase::ProcReceive {
        return Ok(None);
    }
    read_hook_stdin()
}

fn read_hook_stdin() -> io::Result<Option<Vec<u8>>> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }

    let max_hook_stdin_bytes = max_hook_stdin_bytes()?;
    let mut payload = Vec::new();
    stdin
        .lock()
        .take(stdin_sentinel_read_limit(max_hook_stdin_bytes))
        .read_to_end(&mut payload)?;
    if payload.len() as u64 > max_hook_stdin_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "hook stdin exceeds the {} limit",
                hook_stdin_limit_display(max_hook_stdin_bytes)
            ),
        ));
    }
    Ok(Some(payload))
}

fn max_hook_stdin_bytes() -> io::Result<u64> {
    let Some(value) = env::var_os(HOOK_STDIN_LIMIT_ENV) else {
        return Ok(DEFAULT_MAX_HOOK_STDIN_BYTES);
    };
    let value = value.to_string_lossy();
    value.parse::<u64>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{HOOK_STDIN_LIMIT_ENV} must be an unsigned byte count: {error}"),
        )
    })
}

fn hook_stdin_limit_display(limit: u64) -> String {
    if limit == DEFAULT_MAX_HOOK_STDIN_BYTES {
        DEFAULT_HOOK_STDIN_LIMIT_DISPLAY.to_string()
    } else {
        format!("{limit} bytes")
    }
}

fn stdin_sentinel_read_limit(max_hook_stdin_bytes: u64) -> u64 {
    max_hook_stdin_bytes.saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limit_uses_human_readable_display() {
        assert_eq!(
            hook_stdin_limit_display(DEFAULT_MAX_HOOK_STDIN_BYTES),
            "10 MiB"
        );
    }

    #[test]
    fn custom_limit_uses_byte_display() {
        assert_eq!(hook_stdin_limit_display(42), "42 bytes");
    }

    #[test]
    fn sentinel_read_limit_saturates_at_u64_max() {
        assert_eq!(stdin_sentinel_read_limit(41), 42);
        assert_eq!(stdin_sentinel_read_limit(u64::MAX), u64::MAX);
    }
}
