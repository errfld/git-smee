use std::{
    env,
    io::{self, ErrorKind, Write},
    process::Stdio,
    thread,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::path::PathBuf;

use crate::platform::Platform;

pub(super) trait CommandRunner: Sync {
    fn run(
        &self,
        command: &str,
        hook_args: &[String],
        stdin_payload: Option<&[u8]>,
    ) -> Result<Option<i32>, std::io::Error>;
    fn shell_display(&self) -> &'static str;
}

pub(super) struct PlatformCommandRunner<'a> {
    pub(super) platform: &'a Platform,
}

impl CommandRunner for PlatformCommandRunner<'_> {
    fn run(
        &self,
        command: &str,
        hook_args: &[String],
        stdin_payload: Option<&[u8]>,
    ) -> Result<Option<i32>, std::io::Error> {
        let mut shell_command = self.platform.create_command();
        apply_hook_arg_env(&mut shell_command, hook_args);
        let mut _windows_command_script = None;
        match self.platform {
            Platform::Unix => {
                shell_command.arg(command);
                shell_command.arg("--");
                shell_command.args(hook_args);
            }
            Platform::Windows => {
                let command_script = create_windows_command_script(command)?;
                shell_command.arg(&command_script);
                #[cfg(windows)]
                append_windows_hook_args(&mut shell_command, hook_args);
                #[cfg(not(windows))]
                shell_command.args(hook_args);
                _windows_command_script = Some(command_script);
            }
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

pub(super) fn create_windows_command_script(
    command: &str,
) -> Result<tempfile::TempPath, io::Error> {
    let mut script = tempfile::Builder::new()
        .prefix("git-smee-command-")
        .suffix(".cmd")
        .tempfile()?;
    script.write_all(windows_command_script(command).as_bytes())?;
    script.flush()?;
    Ok(script.into_temp_path())
}

#[cfg(windows)]
fn append_windows_hook_args(shell_command: &mut std::process::Command, hook_args: &[String]) {
    for arg in hook_args {
        shell_command.raw_arg(" ");
        shell_command.raw_arg(windows_cmd_quote_hook_arg(arg));
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn windows_cmd_quote_hook_arg(arg: &str) -> String {
    let needs_quotes = arg.is_empty()
        || arg.chars().any(|ch| {
            matches!(
                ch,
                ' ' | '\t' | '&' | '|' | '^' | '<' | '>' | '(' | ')' | '!' | '%'
            )
        });
    if !needs_quotes {
        return arg.to_string();
    }

    let escaped = arg.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

pub(super) fn windows_command_script(command: &str) -> String {
    format!("@echo off\r\n{command}\r\n")
}

pub(super) fn apply_hook_arg_env(shell_command: &mut std::process::Command, hook_args: &[String]) {
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
pub(super) fn cmd_compatible_current_dir() -> Result<Option<PathBuf>, std::io::Error> {
    let current_dir = env::current_dir()?;
    let current_dir = current_dir.to_string_lossy();
    Ok(current_dir.strip_prefix(r"\\?\").map(PathBuf::from))
}

pub(super) fn is_hook_arg_env_key(key: &str) -> bool {
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
