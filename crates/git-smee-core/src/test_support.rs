use std::sync::{Mutex, MutexGuard};

static PROCESS_STATE_MUTEX: Mutex<()> = Mutex::new(());

/// Serializes tests that mutate process-global state such as environment
/// variables or the current working directory.
///
/// Rust 2024 makes environment mutation unsafe because other threads can read or
/// mutate the same process state concurrently. Hold this guard for the full
/// setup/exercise/restore window whenever a test changes env vars or cwd.
pub(crate) fn process_state_lock() -> MutexGuard<'static, ()> {
    PROCESS_STATE_MUTEX.lock().unwrap()
}
