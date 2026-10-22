//! Ordinary Sandbox startup, original deployment inputs and failure custody.
pub(crate) use super::create_types::Engine;
pub use super::create_types::{
    Deployment, ManagedSandbox, SandboxApi, SandboxCause, SandboxCreate, SandboxFailure,
    SandboxPhase,
};
use crate::{internal::Control, OperationFailure};
use layerfs_bridge::{
    control::{ControlError, DaemonPhase, DaemonStatus, HelloRequest, Reply, Request},
    duplex::{initiate_observed, public_key, ChannelWork},
};
use layerfs_sandbox::backend::{docker::Docker, Failure, Runtime};
use std::{fs::File, io, net::TcpStream};
/// The fixed paths each runtime's daemon must use. A container mounts the
/// shared volume at `/layerfs-store` and keeps its own `/layerfs-local`; a
/// process daemon on a host shared with other daemons keeps its Overlay and
/// mount home in its own Sandbox directory and listens on loopback.
fn topology(engine: &Engine, request: &SandboxCreate) -> bool {
    let setup = &request.setup;
    match (engine, &request.deployment) {
        (Engine::Docker(_), Deployment::Docker(deployment)) => {
            setup.listen == format!("0.0.0.0:{}", deployment.port)
                && setup.store == "/layerfs-store/global/store.sqlite"
                && setup.overlay == "/layerfs-local/overlay/overlay.sqlite"
                && setup.mounts == "/workspaces"
        }
        #[cfg(target_os = "linux")]
        (Engine::Process, Deployment::Process(layout)) => {
            let store = std::path::Path::new(&setup.store);
            setup.listen == "127.0.0.1:0"
                && store.is_absolute()
                && !store.starts_with(&layout.directory)
                && layout.directory.is_absolute()
                && setup.overlay == layout.overlay().to_string_lossy()
                && setup.mounts == layout.mounts().to_string_lossy()
        }
        _ => false,
    }
}
/// The address to dial for a daemon `endpoint` observed on the Docker host's
/// loopback: the endpoint itself, or the port on `host` (one resolution, its
/// first address; nothing is tried twice).
pub fn connect_address(
    endpoint: std::net::SocketAddr,
    host: Option<&str>,
) -> io::Result<std::net::SocketAddr> {
    let Some(host) = host else {
        return Ok(endpoint);
    };
    std::net::ToSocketAddrs::to_socket_addrs(&(host, endpoint.port()))?
        .next()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("daemon host {host} has no address"),
            )
        })
}
impl SandboxApi {
    /// Uses one explicit local Engine endpoint and metadata wait policy.
    pub fn new(docker: Docker) -> Self {
        Self {
            engine: Engine::Docker(docker),
        }
    }
    /// Starts each Sandbox as a daemon child process on this Linux host.
    #[cfg(target_os = "linux")]
    pub fn process() -> Self {
        Self {
            engine: Engine::Process,
        }
    }
    /// Creates, privately configures, starts and authenticates once, preserving partial ownership on failure.
    pub fn create(
        &self,
        request: SandboxCreate,
        project: &crate::SealedProject,
    ) -> Result<ManagedSandbox, Box<SandboxFailure>> {
        self.create_file(request, &project.store, &project.manifest)
    }
    /// Creates a new daemon using exact persisted sealed-file and manifest facts.
    pub fn create_file(
        &self,
        request: SandboxCreate,
        store: &layerfs_persistence::SealedStore,
        manifest: &layerfs_bridge::provision::StoreManifest,
    ) -> Result<ManagedSandbox, Box<SandboxFailure>> {
        let existing = request.setup.existing_store.is_some();
        if let Some(selected) = &request.setup.existing_store {
            let mut expected = manifest.clone();
            expected.daemon_sqlite = selected.daemon_sqlite.clone();
            if *selected != expected {
                return Err(Box::new(SandboxFailure {
                    request,
                    phase: SandboxPhase::Validate,
                    sandbox: None,
                    control: None,
                    handshake: ChannelWork::default(),
                    observed: None,
                    installed: None,
                    cause: SandboxCause::Setup(ControlError("selected existing Project")),
                }));
            }
        }
        let mut owner = create(&self.engine, request)?;
        if existing {
            // The original startup already selected and waited for this Store.
            // Installing again would change the existing one-attempt lifecycle.
            return Ok(owner);
        }
        let installed = owner
            .control
            .install_file(store, manifest)
            .map_err(|cause| {
                Box::new(SandboxFailure {
                    request: owner.request.clone(),
                    phase: SandboxPhase::Install,
                    sandbox: None,
                    control: None,
                    handshake: owner.handshake,
                    observed: Some(owner.startup.clone()),
                    installed: None,
                    cause: SandboxCause::Install(cause),
                })
            });
        let installed = match installed {
            Ok(installed) => installed,
            Err(mut failure) => {
                failure.sandbox = Some(owner.runtime);
                failure.control = Some(owner.control);
                return Err(failure);
            }
        };
        owner.installed = Some(installed);
        match hello(
            &mut owner.control,
            HelloRequest {
                expected_instance: Some(owner.startup.instance),
                wait_for_store: true,
            },
        ) {
            Ok(status) if status.phase == DaemonPhase::ControlReady => {
                owner.startup = status;
                Ok(owner)
            }
            Ok(status) => Err(Box::new(SandboxFailure {
                request: owner.request,
                phase: SandboxPhase::Hello,
                sandbox: Some(owner.runtime),
                control: Some(owner.control),
                handshake: owner.handshake,
                observed: Some(status),
                installed: owner.installed,
                cause: SandboxCause::Setup(ControlError("original installed startup phase")),
            })),
            Err(cause) => Err(Box::new(SandboxFailure {
                request: owner.request,
                phase: SandboxPhase::Hello,
                sandbox: Some(owner.runtime),
                control: Some(owner.control),
                handshake: owner.handshake,
                observed: Some(owner.startup),
                installed: owner.installed,
                cause: SandboxCause::Control(cause),
            })),
        }
    }
}
impl ManagedSandbox {
    /// Explicit startup observation/wait on the same incarnation and Control owner.
    /// A later observation never resolves an earlier unknown operation.
    pub(crate) fn status(
        &mut self,
        wait_for_store: bool,
    ) -> Result<DaemonStatus, Box<OperationFailure>> {
        hello(
            &mut self.control,
            HelloRequest {
                expected_instance: Some(self.startup.instance),
                wait_for_store,
            },
        )
    }
}
pub(super) fn hello(
    control: &mut Control,
    request: HelloRequest,
) -> Result<DaemonStatus, Box<OperationFailure>> {
    let request = Request::Hello(request);
    match crate::base::failure::exchange(control, request.clone())? {
        Reply::Hello(status) => Ok(status),
        reply => Err(OperationFailure::unexpected(request, reply)),
    }
}
pub(crate) fn create(
    engine: &Engine,
    request: SandboxCreate,
) -> Result<ManagedSandbox, Box<SandboxFailure>> {
    let mut phase = SandboxPhase::Validate;
    let mut sandbox = None;
    let mut control = None;
    let mut handshake = ChannelWork::default();
    let mut observed = None;
    let result = (|| {
        let config = request.setup.encode().map_err(SandboxCause::Setup)?;
        if !topology(engine, &request)
            || request.startup_wait.is_zero()
            || public_key(&request.controller_private).map_err(SandboxCause::Channel)?
                != request.setup.control_peer
            || public_key(&request.setup.private_key).map_err(SandboxCause::Channel)?
                != request.daemon_peer
        {
            return Err(SandboxCause::Setup(ControlError(
                "concrete deployment topology/identity",
            )));
        }
        phase = SandboxPhase::Executable;
        let mut file = File::open(&request.executable).map_err(SandboxCause::Io)?;
        let address = match (engine, &request.deployment) {
            (Engine::Docker(docker), Deployment::Docker(deployment)) => {
                phase = SandboxPhase::Create;
                let created = docker
                    .create_sandbox(deployment.clone())
                    .map_err(SandboxCause::Create)?;
                let owner = sandbox.insert(Runtime::Docker(created));
                let Runtime::Docker(owner) = owner else {
                    unreachable!("just made")
                };
                phase = SandboxPhase::Upload;
                owner
                    .upload(&mut file, &config)
                    .map_err(SandboxCause::Upload)?;
                phase = SandboxPhase::Start;
                owner
                    .start()
                    .map_err(|f| SandboxCause::Runtime(Failure::Docker(f)))?;
                phase = SandboxPhase::Listener;
                owner
                    .wait_listener(request.startup_wait)
                    .map_err(SandboxCause::Listener)?;
                phase = SandboxPhase::Endpoint;
                let endpoint = owner
                    .endpoint()
                    .map_err(|f| SandboxCause::Runtime(Failure::Docker(f)))?;
                connect_address(endpoint, request.connect_host.as_deref())
                    .map_err(SandboxCause::Io)?
            }
            #[cfg(target_os = "linux")]
            (Engine::Process, Deployment::Process(layout)) => {
                use layerfs_sandbox::backend::process;
                drop(file);
                phase = SandboxPhase::Start;
                let started = process::Sandbox::start(
                    &request.executable,
                    &config,
                    layout.clone(),
                    request.startup_wait,
                );
                let owner = match started {
                    Ok(owner) => sandbox.insert(Runtime::Process(owner)),
                    Err(failure) => {
                        let failure = *failure;
                        if failure.phase == process::StartPhase::Listener {
                            phase = SandboxPhase::Listener;
                        }
                        sandbox = failure.sandbox.map(Runtime::Process);
                        return Err(SandboxCause::Process(failure.phase, failure.cause));
                    }
                };
                phase = SandboxPhase::Endpoint;
                owner.endpoint().map_err(SandboxCause::Runtime)?
            }
            _ => {
                return Err(SandboxCause::Setup(ControlError(
                    "deployment of another runtime",
                )))
            }
        };
        phase = SandboxPhase::Connect;
        let stream =
            TcpStream::connect_timeout(&address, request.startup_wait).map_err(SandboxCause::Io)?;
        stream
            .set_read_timeout(Some(request.startup_wait))
            .map_err(SandboxCause::Io)?;
        stream
            .set_write_timeout(Some(request.startup_wait))
            .map_err(SandboxCause::Io)?;
        phase = SandboxPhase::Authenticate;
        let start = initiate_observed(stream, &request.controller_private, request.daemon_peer);
        handshake = start.work;
        let connection = start.result.map_err(SandboxCause::Channel)?;
        // The startup handshake waits are cleared before control/product operations.
        // Bridge owns cloned duplex directions; clear policy through that actual owner.
        control = Some(Control::new(connection));
        phase = SandboxPhase::Policy;
        control
            .as_ref()
            .expect("authenticated")
            .clear_io_waits()
            .map_err(SandboxCause::Channel)?;
        phase = SandboxPhase::Hello;
        let status = hello(
            control.as_mut().expect("authenticated"),
            HelloRequest {
                expected_instance: None,
                wait_for_store: request.setup.existing_store.is_some(),
            },
        )
        .map_err(SandboxCause::Control)?;
        let expected = if request.setup.existing_store.is_some() {
            DaemonPhase::ControlReady
        } else {
            DaemonPhase::InstallPending
        };
        observed = Some(status.clone());
        if status.phase != expected {
            return Err(SandboxCause::Setup(ControlError("original startup phase")));
        }
        Ok(status)
    })();
    match result {
        Ok(startup) => Ok(ManagedSandbox {
            request: request.clone(),
            installed: None,
            runtime: sandbox.take().expect("owned"),
            control: control.take().expect("authenticated"),
            startup,
            handshake,
            command_identity: layerfs_sandbox::CommandIdentity {
                uid: request.setup.command_uid,
                gid: request.setup.command_gid,
            },
        }),
        Err(cause) => Err(Box::new(SandboxFailure {
            request,
            phase,
            sandbox,
            control,
            handshake,
            observed,
            installed: None,
            cause,
        })),
    }
}
