//! Concrete ordinary runtime providers.
pub mod docker;
#[cfg(target_os = "linux")]
pub mod process;
#[cfg(target_os = "linux")]
mod process_host;
mod runtime;
pub mod stop_rules;

pub use runtime::{Failure, Runtime};
