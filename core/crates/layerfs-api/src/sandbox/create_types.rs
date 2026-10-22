//! Sandbox startup inputs, owners and failure custody.
use crate::{internal::Control, OperationFailure};
use layerfs_bridge::{
    control::{ControlError, DaemonStatus},
    daemon_setup::DaemonSetup,
    duplex::{ChannelError, ChannelWork},
};
use layerfs_sandbox::backend::{
    docker::{Docker, ListenerFailure, SandboxCreateFailure, SandboxRequest, UploadFailure},
    Failure, Runtime,
};
use std::{fmt, io, path::PathBuf, time::Duration};
/// Where one Sandbox runs: a new Docker container, or a daemon process on this Linux host.
#[derive(Clone, Debug)]
pub enum Deployment {
    /// Exact immutable image and borrowed shared named volume.
    Docker(SandboxRequest),
    /// A new private Sandbox directory; the Store is the setup's own file.
    #[cfg(target_os = "linux")]
    Process(layerfs_sandbox::backend::process::Layout),
}
/// One actual stopped-container deployment, private archive and authenticated startup.
#[derive(Clone)]
pub struct SandboxCreate {
    /// The runtime and its placement inputs.
    pub deployment: Deployment,
    /// Explicit private daemon resources and Disposable install/open selection.
    pub setup: DaemonSetup,
    /// Sealed Linux executable supplied by the caller; streamed once, never read as Store data.
    pub executable: PathBuf,
    /// Controller Noise secret; redacted from Debug and never sent in env/argv.
    pub controller_private: [u8; 32],
    /// Expected actual daemon identity, checked against private deployment config.
    pub daemon_peer: [u8; 32],
    /// Explicit listener event wait; does not apply to ordinary commands.
    pub startup_wait: Duration,
    /// Host to dial for the daemon's loopback-published port, when this
    /// process does not share the Docker host's loopback (a client in a
    /// container: `host.docker.internal`). `None` dials the observed
    /// 127.0.0.1 endpoint. The publish and its check are unchanged.
    pub connect_host: Option<String>,
}
impl fmt::Debug for SandboxCreate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxCreate")
            .field("deployment", &self.deployment)
            .field("setup", &self.setup)
            .field("executable", &self.executable)
            .field("controller_private", &"[redacted]")
            .field("daemon_peer", &self.daemon_peer)
            .field("startup_wait", &self.startup_wait)
            .field("connect_host", &self.connect_host)
            .finish()
    }
}
/// Concrete boundary reached by one original lifecycle operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SandboxPhase {
    /// Local validation, no container effect.
    Validate,
    /// Caller executable open/metadata.
    Executable,
    /// Volume observation and original Create.
    Create,
    /// Original stopped-container archive extraction.
    Upload,
    /// Original container Start.
    Start,
    /// Actual original stdout listener event wait.
    Listener,
    /// Exact container endpoint observation.
    Endpoint,
    /// One TCP connection attempt.
    Connect,
    /// One expected-peer duplex authentication.
    Authenticate,
    /// Clears only explicit handshake waits after authentication; owner remains retained on failure.
    Policy,
    /// One correlated Hello through the retained Control owner.
    Hello,
    /// One original sealed Project installation.
    Install,
    /// One explicit whole-Sandbox stop attempt.
    Stop,
    /// One delete attempt after acknowledged stop.
    Delete,
}
/// Exact original failure, independent of owned resource disposition.
#[derive(Debug)]
pub enum SandboxCause {
    /// Local filesystem/socket error.
    Io(io::Error),
    /// Local config or correlated startup validation.
    Setup(ControlError),
    /// Actual original runtime Create including partial decoded ID.
    Create(Box<SandboxCreateFailure>),
    /// Actual incremental archive extraction failure and work.
    Upload(Box<UploadFailure>),
    /// Original listener wait/progress/fence failure.
    Listener(Box<ListenerFailure>),
    /// Original runtime call (Engine transfer or daemon process), no replay.
    Runtime(Failure),
    /// Daemon process start: directory, spawn or listen line.
    #[cfg(target_os = "linux")]
    Process(layerfs_sandbox::backend::process::StartPhase, io::Error),
    /// Original authentication failure; handshake work remains in the receipt.
    Channel(ChannelError),
    /// Original correlated daemon control exchange/refusal.
    Control(Box<OperationFailure>),
    /// Original streaming installation failure and exact publication knowledge.
    Install(Box<crate::InstallFailure>),
}
/// Caller retains exact acknowledged owner, connection and original deployment inputs.
pub struct SandboxFailure {
    /// Original inputs with secrets retained privately.
    pub request: SandboxCreate,
    /// Original reached boundary.
    pub phase: SandboxPhase,
    /// Exact acknowledged owned Sandbox; no automatically adopted partial ID.
    pub sandbox: Option<Runtime>,
    /// Original authenticated control owner, if established.
    pub control: Option<Control>,
    /// Actual original handshake work, including failure.
    pub handshake: ChannelWork,
    /// Original correlated Hello, even if its phase failed selection validation.
    pub observed: Option<DaemonStatus>,
    /// Installation acknowledgement, retained if the following Ready observation fails.
    pub installed: Option<crate::Installed>,
    /// Exact original cause.
    pub cause: SandboxCause,
}
impl fmt::Debug for SandboxFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxFailure")
            .field("request", &self.request)
            .field("phase", &self.phase)
            .field("sandbox", &self.sandbox)
            .field("has_control", &self.control.is_some())
            .field("handshake", &self.handshake)
            .field("observed", &self.observed)
            .field("cause", &self.cause)
            .finish()
    }
}
impl fmt::Display for SandboxFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sandbox {:?}: {:?}", self.phase, self.cause)
    }
}
impl std::error::Error for SandboxFailure {}
/// Actual runtime and authenticated daemon lifecycle organization.
#[derive(Clone, Debug)]
pub struct SandboxApi {
    pub(crate) engine: Engine,
}
/// The runtime a [`SandboxApi`] starts Sandboxes on.
#[derive(Clone, Debug)]
pub(crate) enum Engine {
    Docker(Docker),
    #[cfg(target_os = "linux")]
    Process,
}
/// Exact acknowledged Sandbox and authenticated control owner.
pub struct ManagedSandbox {
    /// Owned runtime Sandbox; explicit stop/delete only.
    pub runtime: Runtime,
    /// Existing sequential control owner; correlation and failed state are never reset.
    pub(crate) control: Control,
    /// Original acknowledged installation, when this create installed the Project.
    pub installed: Option<crate::Installed>,
    pub(crate) request: SandboxCreate,
    /// Original correlated startup status; this is not kernel Workspace Ready.
    pub startup: DaemonStatus,
    /// Actual original duplex handshake work.
    pub handshake: ChannelWork,
    pub(crate) command_identity: layerfs_sandbox::CommandIdentity,
}
