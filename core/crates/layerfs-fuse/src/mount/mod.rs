//! Linux mount profile, first-party mount controls and negotiation facts.
pub mod copies;
mod profile;
pub(crate) mod syscalls;

pub(crate) use profile::negotiate;
pub use profile::Negotiation;
pub use syscalls::{AbortWrite, MountEntry};
