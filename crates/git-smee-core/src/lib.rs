pub mod config;
pub mod executor;
pub mod installer;
pub mod platform;
pub mod repository;
pub use crate::config::Error;
pub use crate::config::SmeeConfig;
pub use crate::installer::install_hooks;
pub use crate::repository::{ensure_in_repo_root, find_git_root};

#[cfg(test)]
mod tests {}
