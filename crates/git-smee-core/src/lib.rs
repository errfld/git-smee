pub mod config;
pub mod installer;
pub use crate::config::Error;
pub use crate::config::SmeeConfig;
pub use crate::installer::install_hooks;

#[cfg(test)]
mod tests {}
