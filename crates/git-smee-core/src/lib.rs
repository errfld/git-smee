pub mod config;
pub mod executor;
pub mod installer;
pub mod platform;
pub use crate::config::Error;
pub use crate::config::SmeeConfig;
pub use crate::installer::install_hooks;

#[cfg(test)]
mod tests {}
