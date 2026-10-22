//! Concrete ordinary runtime providers.
pub mod docker;
#[cfg(target_os = "linux")]
pub mod process;

use crate::WireFailure;
use std::{fmt, io, net::SocketAddr};

/// One owned Sandbox of either runtime.
#[derive(Debug)]
pub enum Runtime {
    Docker(docker::Sandbox),
    #[cfg(target_os = "linux")]
    Process(process::Sandbox),
}
/// A runtime call's original failure.
#[derive(Debug)]
pub enum Failure {
    Docker(Box<WireFailure>),
    Process(io::Error),
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Docker(failure) => write!(f, "{failure:?}"),
            Self::Process(error) => write!(f, "daemon process: {error}"),
        }
    }
}
impl Failure {
    /// The Sandbox exists but its daemon no longer runs: only its delete is left.
    pub fn stopped(&self) -> bool {
        match self {
            Self::Docker(failure) => failure.container_stopped(),
            Self::Process(error) => error.kind() == io::ErrorKind::NotFound,
        }
    }
    /// The Sandbox itself is gone (a Docker container the Engine no longer has).
    pub fn gone(&self) -> bool {
        match self {
            Self::Docker(failure) => failure.container_gone(),
            Self::Process(_) => false,
        }
    }
}
impl Runtime {
    /// The container id, or `pid:<pid>@<start>` of a daemon process.
    pub fn identity(&self) -> String {
        match self {
            Self::Docker(sandbox) => sandbox.identity().to_string(),
            #[cfg(target_os = "linux")]
            Self::Process(sandbox) => sandbox.identity(),
        }
    }
    /// The daemon's address on this host's loopback, while it runs.
    pub fn endpoint(&self) -> Result<SocketAddr, Failure> {
        match self {
            Self::Docker(sandbox) => sandbox.endpoint().map_err(Failure::Docker),
            #[cfg(target_os = "linux")]
            Self::Process(sandbox) => sandbox.endpoint().map_err(Failure::Process),
        }
    }
    /// The daemon's last output, at most `cap` bytes (`lines` lines of a container log).
    pub fn log_tail(&self, lines: u16, cap: usize) -> Result<Vec<u8>, Failure> {
        match self {
            Self::Docker(sandbox) => sandbox.log_tail(lines, cap).map_err(Failure::Docker),
            #[cfg(target_os = "linux")]
            Self::Process(sandbox) => sandbox.log_tail(cap).map_err(Failure::Process),
        }
    }
    /// One stop with a `seconds` grace; a daemon that already exited is stopped.
    pub fn stop(&mut self, seconds: u16) -> Result<(), Failure> {
        match self {
            Self::Docker(sandbox) => sandbox.stop(seconds).map_err(Failure::Docker),
            #[cfg(target_os = "linux")]
            Self::Process(sandbox) => sandbox
                .stop(std::time::Duration::from_secs(seconds.into()))
                .map_err(Failure::Process),
        }
    }
    /// One delete after a known stop. The shared Store is never deleted.
    pub fn delete(&mut self) -> Result<(), Failure> {
        match self {
            Self::Docker(sandbox) => sandbox.delete().map_err(Failure::Docker),
            #[cfg(target_os = "linux")]
            Self::Process(sandbox) => sandbox.delete().map_err(Failure::Process),
        }
    }
}
