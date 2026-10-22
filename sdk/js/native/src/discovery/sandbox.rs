//! Sandbox and Workspace records, and the runtime Sandbox a record names.
use super::*;

pub(crate) fn save_sandbox(
    directory: &Path,
    state: &SandboxState,
    args: &SdkValue,
    secret: [u8; 32],
    peer: [u8; 32],
    instance: [u8; 32],
) -> Result<()> {
    let deploy = args
        .get("deploy")
        .ok_or_else(|| refused("InvalidInput", "deploy"))?;
    let placed = match &state.runtime {
        Runtime::Docker(sandbox) => vec![
            ("runtime", SdkValue::text("docker")),
            ("docker", SdkValue::text(registry::text(deploy, "docker")?)),
            ("image", SdkValue::text(&sandbox.request().image)),
            ("volume", SdkValue::text(&sandbox.request().store_volume)),
            (
                "labels",
                SdkValue::List(
                    (sandbox.request().labels.iter())
                        .map(|(key, value)| {
                            SdkValue::map([
                                ("key", SdkValue::text(key)),
                                ("value", SdkValue::text(value)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ],
        #[cfg(target_os = "linux")]
        Runtime::Process(sandbox) => vec![
            ("runtime", SdkValue::text("process")),
            (
                "root",
                SdkValue::text(sandbox.layout().directory.display().to_string()),
            ),
            ("pid", SdkValue::Int(sandbox.pid().into())),
            ("start", SdkValue::text(sandbox.start_time().to_string())),
        ],
    };
    let SdkValue::Map(mut fields) = SdkValue::map([
        ("id", SdkValue::text(&state.id)),
        ("project", SdkValue::text(&state.project)),
        ("name", SdkValue::text(&state.name)),
        ("container", SdkValue::text(state.runtime.identity())),
        ("secret", SdkValue::Bytes(secret.to_vec())),
        ("peer", SdkValue::Bytes(peer.to_vec())),
        ("instance", SdkValue::Bytes(instance.to_vec())),
    ]) else {
        unreachable!("a map")
    };
    fields.extend(
        placed
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value)),
    );
    save(&directory.join("sandbox.record"), SdkValue::Map(fields))
}
/// A Sandbox whose container no longer runs it: the Engine no longer has the
/// container, or has it stopped (it closed when idle, or Docker restarted),
/// which lost its daemon and every mount in it. Only its records, and a
/// stopped container, hold its number; the next mount reclaims them.
pub(crate) fn gone(id: &str, container: &str, stopped: bool) -> SdkRefusal {
    let state = if stopped { "stopped" } else { "gone" };
    SdkRefusal {
        code: "NoSuchSandbox".to_owned(),
        message: format!(
            "the container of Sandbox {id} is {state}, with every Workspace that was in it"
        ),
        detail: SdkValue::map([
            ("sandbox", SdkValue::text(id)),
            ("container", SdkValue::text(container)),
            ("containerGone", SdkValue::Bool(true)),
            ("containerStopped", SdkValue::Bool(stopped)),
        ]),
    }
}
/// The container of a refusal from [`gone`], and whether the Engine still has it stopped.
pub(crate) fn gone_container(refusal: &SdkRefusal) -> Option<(&str, bool)> {
    match refusal.detail.get("containerGone") {
        Some(SdkValue::Bool(true)) if refusal.code == "NoSuchSandbox" => Some((
            refusal.detail.get_text("container")?,
            matches!(
                refusal.detail.get("containerStopped"),
                Some(SdkValue::Bool(true))
            ),
        )),
        _ => None,
    }
}
/// The runtime Sandbox a record names: its Engine, container and
/// deployment, or its daemon process directory, pid and start time.
pub(crate) enum Saved {
    Docker(Docker, ContainerId, SandboxRequest),
    #[cfg(target_os = "linux")]
    Process(layerfs_sandbox::backend::process::Layout, u32, u64),
}
impl Saved {
    /// The saved Sandbox, checked running.
    pub(crate) fn reopen(self) -> std::result::Result<Runtime, Failure> {
        match self {
            Self::Docker(docker, container, request) => {
                docker::Sandbox::reopen(docker, container, request)
                    .map(Runtime::Docker)
                    .map_err(Failure::Docker)
            }
            #[cfg(target_os = "linux")]
            Self::Process(layout, pid, start) => {
                layerfs_sandbox::backend::process::Sandbox::reopen(layout, pid, start)
                    .map(Runtime::Process)
                    .map_err(Failure::Process)
            }
        }
    }
    /// The saved Sandbox a reopen found stopped: only its delete is admitted.
    pub(crate) fn stopped(self) -> Runtime {
        match self {
            Self::Docker(docker, container, request) => {
                Runtime::Docker(docker::Sandbox::stopped(docker, container, request))
            }
            #[cfg(target_os = "linux")]
            Self::Process(layout, pid, start) => Runtime::Process(
                layerfs_sandbox::backend::process::Sandbox::stopped(layout, pid, start),
            ),
        }
    }
}
/// The runtime Sandbox a record names. A record without `runtime` is Docker's.
pub(crate) fn runtime(value: &SdkValue) -> Result<Saved> {
    match value.get_text("runtime").unwrap_or("docker") {
        "docker" => {}
        #[cfg(target_os = "linux")]
        "process" => {
            let layout = layerfs_sandbox::backend::process::Layout {
                directory: PathBuf::from(registry::text(value, "root")?),
            };
            let pid = value
                .get_int("pid")
                .and_then(|pid| u32::try_from(pid).ok())
                .ok_or_else(|| refused("Unauthorized", "Sandbox record pid"))?;
            let start = registry::text(value, "start")?
                .parse()
                .map_err(|_| refused("Unauthorized", "Sandbox record start"))?;
            return Ok(Saved::Process(layout, pid, start));
        }
        other => {
            return Err(refused(
                "Unsupported",
                format!("Sandbox runtime {other} is not available here"),
            ))
        }
    }
    let docker =
        Docker::new(registry::text(value, "docker")?, Duration::from_secs(30)).map_err(failed)?;
    let request = SandboxRequest {
        image: registry::text(value, "image")?.to_owned(),
        store_volume: registry::text(value, "volume")?.to_owned(),
        port: 30421,
        // a record written before #373 has no labels, nor has its container
        labels: match value.get("labels") {
            Some(SdkValue::List(items)) => items
                .iter()
                .map(|item| {
                    Ok((
                        registry::text(item, "key")?.to_owned(),
                        registry::text(item, "value")?.to_owned(),
                    ))
                })
                .collect::<Result<_>>()?,
            _ => Vec::new(),
        },
    };
    let container = ContainerId::parse(registry::text(value, "container")?).map_err(failed)?;
    Ok(Saved::Docker(docker, container, request))
}
pub(crate) fn reopen(args: &SdkValue) -> Result<Arc<SandboxState>> {
    let id = registry::text(args, "sandbox")?;
    let directory = sandbox_dir(args, id)?;
    if !directory.join("sandbox.record").exists() {
        return Err(refused(
            "NoSuchSandbox",
            format!("the project has no Sandbox {id}"),
        ));
    }
    private(&directory, true)?;
    let value = load(&directory.join("sandbox.record"))?;
    if registry::text(&value, "id")? != id {
        return Err(refused("Unauthorized", "Sandbox record mismatch"));
    }
    let project = registry::text(&value, "project")?;
    let container = registry::text(&value, "container")?;
    let runtime = runtime(&value)?.reopen().map_err(|failure| {
        if failure.gone() || failure.stopped() {
            gone(id, container, failure.stopped())
        } else {
            failed(failure)
        }
    })?;
    let dial = crate::pool::Dial {
        endpoint: runtime.endpoint().map_err(failed)?,
        host: crate::pool::daemon_host(args),
        secret: bytes(&value, "secret")?,
        peer: bytes(&value, "peer")?,
        instance: bytes(&value, "instance")?,
    };
    let control = dial.open_waiting()?;
    let state = Arc::new(SandboxState {
        id: id.to_owned(),
        project: project.to_owned(),
        // a record from before #373 has no name
        name: value.get_text("name").unwrap_or_default().to_owned(),
        directory,
        runtime,
        identity: CommandIdentity { uid: 501, gid: 20 },
        controls: crate::pool::Controls::new(control, dial),
        mounts: Default::default(),
        processes: Default::default(),
    });
    refresh(&state)?;
    Ok(state)
}
/// Refresh shared executor/mount metadata without repeating an operation.
pub(crate) fn refresh(state: &SandboxState) -> Result<()> {
    let mut mounts = std::collections::BTreeMap::new();
    let mut processes = std::collections::BTreeMap::new();
    for item in fs::read_dir(&state.directory).map_err(failed)? {
        let item = item.map_err(failed)?;
        let name = item.file_name().to_string_lossy().into_owned();
        if name.starts_with("workspace-") && name.ends_with(".record") {
            let Some(value) = present(&item.path())? else {
                continue;
            };
            let path = registry::text(&value, "path")?;
            let token = layerfs_bridge::control::WorkspaceToken {
                workspace: layerfs_history::WorkspaceId::from_slice(&bytes::<32>(
                    &value,
                    "workspace",
                )?)
                .map_err(failed)?,
                namespace: value
                    .get_int("namespace")
                    .ok_or_else(|| refused("InvalidInput", "namespace"))?,
            };
            let status =
                match workspace(&mut *crate::trace::control(&state.controls)?).status(token) {
                    Ok(status) => status,
                    // Unmounted by the daemon; its owner removes the record next.
                    Err(failure) if absent(&failure) => continue,
                    Err(failure) => return Err(failed(failure)),
                };
            let ready = status
                .kernel
                .and_then(|k| k.ready)
                .ok_or_else(|| refused("NotMounted", "saved Workspace is no longer mounted"))?;
            if ready.directory != path {
                return Err(refused("StaleSandbox", "mount path mismatch"));
            }
            mounts.insert(
                path.to_owned(),
                Mount {
                    workspace: MountedWorkspace {
                        bound: BoundWorkspace {
                            token,
                            binding: status.binding,
                        },
                        ready,
                    },
                    branch: registry::text(&value, "branch")?.to_owned(),
                },
            );
        } else if name.starts_with("process-") && name.ends_with(".record") {
            let Some(v) = present(&item.path())? else {
                continue;
            };
            processes.insert(
                registry::text(&v, "id")?.to_owned(),
                Process {
                    path: registry::text(&v, "path")?.to_owned(),
                    command: registry::text(&v, "command")?.to_owned(),
                    pid: v
                        .get_int("pid")
                        .ok_or_else(|| refused("InvalidInput", "pid"))?
                        as u32,
                    start: registry::text(&v, "start")?.to_owned(),
                    stopped: v.get_bool("stopped"),
                },
            );
        }
    }
    *registry::lock(&state.mounts)? = mounts;
    *registry::lock(&state.processes)? = processes;
    Ok(())
}
/// A listed record, or `None` when its owner removed it after the listing:
/// concurrent calls of this process unmount and refresh at once (#352 F-017).
fn present(path: &Path) -> Result<Option<SdkValue>> {
    match load(path) {
        Ok(value) => Ok(Some(value)),
        Err(_) if !path.exists() => Ok(None),
        Err(refusal) => Err(refusal),
    }
}
fn absent(failure: &layerfs_api::OperationFailure) -> bool {
    matches!(&failure.cause, layerfs_api::OperationCause::Remote(refusal)
        if refusal.code == layerfs_bridge::control::ControlCode::Missing)
}
pub(crate) fn workspace_file(
    state: &SandboxState,
    token: layerfs_bridge::control::WorkspaceToken,
) -> PathBuf {
    state.directory.join(format!(
        "workspace-{}.record",
        layerfs_bridge::sdk_values::hex(&token.workspace.to_bytes())
    ))
}
pub(crate) fn save_workspace(
    state: &SandboxState,
    mount: &MountedWorkspace,
    branch: &str,
) -> Result<()> {
    save(
        &workspace_file(state, mount.bound.token),
        SdkValue::map([
            ("path", SdkValue::text(&mount.ready.directory)),
            ("branch", SdkValue::text(branch)),
            (
                "workspace",
                SdkValue::Bytes(mount.bound.token.workspace.to_bytes().to_vec()),
            ),
            ("namespace", SdkValue::Int(mount.bound.token.namespace)),
        ]),
    )
}
pub(crate) fn save_process(state: &SandboxState, id: &str, p: &Process) -> Result<()> {
    save(
        &state.directory.join(format!("process-{id}.record")),
        SdkValue::map([
            ("id", SdkValue::text(id)),
            ("path", SdkValue::text(&p.path)),
            ("command", SdkValue::text(&p.command)),
            ("pid", SdkValue::Int(p.pid as i64)),
            ("start", SdkValue::text(&p.start)),
            ("stopped", SdkValue::Bool(p.stopped)),
        ]),
    )
}
