//! Ordinary commands in a Sandbox, run by its runtime owner; the daemon is not involved.
use crate::{ManagedSandbox, MountedWorkspace, WorkspaceApi};
use layerfs_sandbox::{
    backend::{
        docker::{
            ExecCreateFailure, ExecCreated, ExecRequest, OutputFailure, Sandbox, SplitFailure,
            StartFailure,
        },
        Runtime,
    },
    CommandIdentity, WireFailure,
};
use std::io::Write;

impl WorkspaceApi<'_> {
    /// Creates one ordinary command at this mount using the deployment's admitted identity.
    /// The returned Sandbox runtime owner owns Start, streams, exit status and cancellation.
    /// This sends no daemon request, registers no Exec and performs no Commit or unmount.
    ///
    /// # Panics
    ///
    /// On a daemon process Sandbox, which has no streaming launcher: use
    /// [`crate::internal::run`] there.
    pub fn exec(
        sandbox: &ManagedSandbox,
        workspace: &MountedWorkspace,
        arguments: Vec<String>,
        environment: Vec<String>,
        stdin: bool,
    ) -> Result<ExecCreated, Box<ExecCreateFailure>> {
        let Runtime::Docker(runtime) = &sandbox.runtime else {
            panic!("WorkspaceApi::exec streams Docker Sandboxes only; use internal::run");
        };
        launch(
            runtime,
            sandbox.command_identity,
            arguments,
            environment,
            workspace.ready.directory.clone(),
            stdin,
        )
    }
}

pub(crate) fn launch(
    sandbox: &Sandbox,
    identity: CommandIdentity,
    arguments: Vec<String>,
    environment: Vec<String>,
    directory: String,
    stdin: bool,
) -> Result<ExecCreated, Box<ExecCreateFailure>> {
    sandbox.create_exec(ExecRequest {
        identity,
        arguments,
        environment,
        directory,
        stdin,
    })
}
/// The step of [`run`] that failed, with its original failure.
#[derive(Debug)]
pub enum RunFailure {
    /// Engine exec create.
    Create(Box<ExecCreateFailure>),
    /// Engine exec start and attach.
    Start(Box<StartFailure>),
    /// Attached stream split.
    Split(Box<SplitFailure>),
    /// Output copy.
    Output(Box<OutputFailure>),
    /// Exit inspection.
    Inspect(Box<WireFailure>),
    /// The Engine reports no exit code for the ended command.
    UnknownExit,
    /// The daemon process runtime's spawn, copy or wait.
    Process(std::io::Error),
}
/// Runs `arguments` to their end as `identity` in `directory`, stdin
/// closed, copying stdout and stderr; returns the root command's exit code.
pub(crate) fn run(
    runtime: &Runtime,
    identity: CommandIdentity,
    arguments: Vec<String>,
    environment: Vec<String>,
    directory: String,
    stdout: &mut (impl Write + Send),
    stderr: &mut (impl Write + Send),
) -> Result<i32, RunFailure> {
    match runtime {
        Runtime::Docker(sandbox) => {
            let created = launch(sandbox, identity, arguments, environment, directory, false)
                .map_err(RunFailure::Create)?;
            let attached = created.start().map_err(RunFailure::Start)?;
            let (_input, output, handle) = attached.into_parts().map_err(RunFailure::Split)?;
            output.copy_to(stdout, stderr).map_err(RunFailure::Output)?;
            handle
                .inspect()
                .map_err(RunFailure::Inspect)?
                .known_root_exit()
                .ok_or(RunFailure::UnknownExit)
        }
        #[cfg(target_os = "linux")]
        Runtime::Process(sandbox) => sandbox
            .run(
                identity,
                &arguments,
                &environment,
                &directory,
                stdout,
                stderr,
            )
            .map_err(RunFailure::Process),
    }
}
