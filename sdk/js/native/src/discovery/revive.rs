//! Process Sandboxes found with their daemon ended while they hold saved
//! Workspace overlays: start the daemon again on the saved folder and reopen
//! each Workspace at its saved path (core/docs/issues/373/REOPEN.md). Every
//! check and action runs under the project's guard and the Sandbox's guard.
#[cfg(target_os = "linux")]
use super::revive_rules::{self, Found, Next};
use super::*;

/// One saved Workspace record of a Sandbox.
#[cfg(target_os = "linux")]
pub(crate) struct SavedWorkspace {
    /// Its overlay file `<root>/overlay/<namespace>.sqlite` exists.
    overlay: bool,
    path: String,
    branch: String,
    workspace: layerfs_history::WorkspaceId,
    namespace: i64,
    root: Option<[u8; 32]>,
}

/// The saved Workspace records of Sandbox folder `directory`, by namespace,
/// each with whether its overlay file in the Sandbox folder `root` exists.
#[cfg(target_os = "linux")]
fn saved(directory: &Path, root: &Path) -> Result<Vec<SavedWorkspace>> {
    let mut found = Vec::new();
    for item in fs::read_dir(directory).map_err(failed)? {
        let item = item.map_err(failed)?;
        let name = item.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("workspace-") && name.ends_with(".record")) {
            continue;
        }
        let value = load(&item.path())?;
        let namespace = value
            .get_int("namespace")
            .ok_or_else(|| refused("InvalidInput", "namespace"))?;
        let overlay = root
            .join("overlay")
            .join(format!("{namespace}.sqlite"))
            .is_file();
        found.push(SavedWorkspace {
            overlay,
            path: registry::text(&value, "path")?.to_owned(),
            branch: registry::text(&value, "branch")?.to_owned(),
            workspace: layerfs_history::WorkspaceId::from_slice(&bytes::<32>(&value, "workspace")?)
                .map_err(failed)?,
            namespace,
            root: match value.get("root") {
                Some(SdkValue::Bytes(_)) => Some(bytes::<32>(&value, "root")?),
                _ => None,
            },
        });
    }
    found.sort_by_key(|saved| saved.namespace);
    Ok(found)
}

/// A process Sandbox's folder, saved pid and start time, and its saved
/// Workspace records.
#[cfg(target_os = "linux")]
type SavedProcess = (
    layerfs_sandbox::backend::process::Layout,
    u32,
    u64,
    Vec<SavedWorkspace>,
);
/// The process Sandbox a record names and its saved Workspace records;
/// `None` for another runtime.
#[cfg(target_os = "linux")]
fn process(directory: &Path, value: &SdkValue) -> Result<Option<SavedProcess>> {
    match runtime(value)? {
        Saved::Process(layout, pid, start) => {
            let saved = saved(directory, &layout.directory)?;
            Ok(Some((layout, pid, start, saved)))
        }
        Saved::Docker(..) => Ok(None),
    }
}

/// True when Sandbox `id` is a process Sandbox holding a saved Workspace
/// overlay: discovery must not reclaim it.
pub(crate) fn holds_overlays(args: &SdkValue, id: &str) -> Result<bool> {
    #[cfg(target_os = "linux")]
    {
        let directory = sandbox_dir(args, id)?;
        let record = directory.join("sandbox.record");
        if !record.exists() {
            return Ok(false);
        }
        let value = load(&record)?;
        Ok(process(&directory, &value)?
            .is_some_and(|(.., saved)| saved.iter().any(|saved| saved.overlay)))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (args, id);
        Ok(false)
    }
}

/// `operator.sandbox.reopen()`: for each process Sandbox of
/// the project with a saved Workspace record, start its daemon again if it
/// ended and holds a saved overlay, and reopen each saved Workspace; a record
/// whose overlay file is gone is listed as not mounted. The answer shape is in
/// REOPEN.md, Interfaces.
pub(crate) fn reopen_all(args: &SdkValue) -> crate::job::Outcome {
    let _placing = guard(&project_dir(args)?)?;
    let mut sandboxes = Vec::new();
    for id in sandbox_ids(args)? {
        if let Some(item) = revive(args, &id)? {
            sandboxes.push(item.0);
        }
    }
    Ok(SdkValue::map([
        ("kind", SdkValue::text("ok")),
        ("sandboxes", SdkValue::List(sandboxes)),
    ]))
}

/// Revives Sandbox `id` under the caller's project guard: its answer item and,
/// when its daemon serves, its state. `None` when it is no process Sandbox
/// with a saved Workspace record.
pub(crate) fn revive(
    args: &SdkValue,
    id: &str,
) -> Result<Option<(SdkValue, Option<Arc<SandboxState>>)>> {
    #[cfg(target_os = "linux")]
    {
        revive_process(args, id)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (args, id);
        Ok(None)
    }
}

#[cfg(target_os = "linux")]
fn revive_process(
    args: &SdkValue,
    id: &str,
) -> Result<Option<(SdkValue, Option<Arc<SandboxState>>)>> {
    use layerfs_sandbox::backend::process;
    let directory = sandbox_dir(args, id)?;
    let _guard = guard(&directory)?;
    let record = directory.join("sandbox.record");
    if !record.exists() {
        return Ok(None);
    }
    let value = load(&record)?;
    let Some((layout, pid, start, records)) = process(&directory, &value)? else {
        return Ok(None);
    };
    if records.is_empty() {
        return Ok(None);
    }
    // A record whose overlay file is gone cannot reopen: it is listed so the
    // operator sees the loss (REOPEN.md F1, F37).
    let (saved, lost): (Vec<_>, Vec<_>) = records.into_iter().partition(|saved| saved.overlay);
    let lost: Vec<SdkValue> = lost
        .iter()
        .map(|saved| outcome(saved, Err("no saved overlay file".to_owned())))
        .collect();
    let name = value.get_text("name").unwrap_or_default().to_owned();
    let item = |state: &str, reason: Option<String>, mut workspaces: Vec<SdkValue>| {
        workspaces.extend(lost.iter().cloned());
        SdkValue::map([
            ("id", SdkValue::text(id)),
            ("name", SdkValue::text(&name)),
            ("state", SdkValue::text(state)),
            ("reason", SdkValue::opt_text(reason)),
            ("workspaces", SdkValue::List(workspaces)),
        ])
    };
    let not_reopened = |reason: &str| -> Vec<SdkValue> {
        saved
            .iter()
            .map(|saved| outcome(saved, Err(reason.to_owned())))
            .collect()
    };
    let found = match process::Sandbox::reopen(layout.clone(), pid, start) {
        Ok(_) => Found::Running,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Found::Ended,
        Err(error) => {
            let reason = format!("saved daemon could not be observed: {error}");
            return Ok(Some((
                item("failed", Some(reason.clone()), not_reopened(&reason)),
                None,
            )));
        }
    };
    let restart = match revive_rules::next(found, saved.len()) {
        Next::Use => false,
        Next::Reclaim => {
            let reason = "its daemon ended and no saved Workspace has an overlay file";
            return Ok(Some((
                item("ended", Some(reason.to_owned()), Vec::new()),
                None,
            )));
        }
        Next::Restart => true,
    };
    // A running daemon may lack saved Workspaces: a reopen that stopped after
    // the restart. Those are reopened on it; the ones it serves are left.
    let started = if restart {
        restart_daemon(args, id, &directory, &value, &name, layout, pid, start)
    } else {
        reopen(&registry::naming(args, id))
    };
    let state = match started {
        Ok(state) => state,
        Err(refusal) => {
            let reason = refusal.message;
            return Ok(Some((
                item("failed", Some(reason.clone()), not_reopened(&reason)),
                None,
            )));
        }
    };
    let served = registry::lock(&state.mounts)?
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let workspaces = saved
        .iter()
        .map(|saved| {
            let result = if served.contains(&saved.path) {
                Ok(false)
            } else {
                reattach(args, &state, saved).map(|()| true)
            };
            outcome(saved, result)
        })
        .collect();
    let label = if restart { "restarted" } else { "running" };
    Ok(Some((item(label, None, workspaces), Some(state))))
}

/// Starts the daemon of a saved Sandbox again and saves its new identity.
#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn restart_daemon(
    args: &SdkValue,
    id: &str,
    directory: &Path,
    value: &SdkValue,
    name: &str,
    layout: layerfs_sandbox::backend::process::Layout,
    pid: u32,
    start: u64,
) -> Result<Arc<SandboxState>> {
    use layerfs_sandbox::backend::process;
    let executable = PathBuf::from(registry::text(
        args.get("deploy")
            .ok_or_else(|| refused("InvalidInput", "missing `deploy`"))?,
        "daemon",
    )?);
    let sandbox = match process::Sandbox::restart(&executable, layout.clone(), pid, start, STARTUP)
    {
        Ok(sandbox) => sandbox,
        Err(failure) => {
            let tail = failure
                .sandbox
                .as_ref()
                .and_then(|s| s.log_tail(2048).ok())
                .map(|b| String::from_utf8_lossy(&b).trim_end().to_owned())
                .unwrap_or_default();
            let mut message = format!(
                "daemon restart stopped at {:?}: {}{}",
                failure.phase,
                failure.cause,
                if tail.is_empty() {
                    String::new()
                } else {
                    format!("; log: {tail}")
                }
            );
            // A start that failed at its listen line returns the daemon it
            // spawned, still running: it is stopped like any failed restart.
            if let Some(daemon) = failure.sandbox {
                message = format!(
                    "{message}; {}",
                    stop_spawned(directory, value, daemon.pid(), daemon.start_time(), daemon)
                );
            }
            return Err(refused("Failed", message));
        }
    };
    let spawned = (sandbox.pid(), sandbox.start_time());
    let runtime = Runtime::Process(sandbox);
    // Until its record names it, the new daemon is unknown to every later
    // call: one would start a second daemon on the folder. So a failure
    // before the record is saved stops it (Quint sandbox_reopen, OneDaemon).
    match record_restarted(args, id, directory, value, name, runtime) {
        Ok(state) => Ok(state),
        Err(mut refusal) => {
            let daemon = process::Sandbox::stopped(layout, spawned.0, spawned.1);
            refusal.message = format!(
                "{}; {}",
                refusal.message,
                stop_spawned(directory, value, spawned.0, spawned.1, daemon)
            );
            Err(refusal)
        }
    }
}

/// Stops a daemon a failed restart spawned. If the stop fails, the Sandbox
/// record is made to name it, so a later revive finds it running and starts
/// no second daemon on the folder. Returns what happened, for the refusal.
#[cfg(target_os = "linux")]
fn stop_spawned(
    directory: &Path,
    value: &SdkValue,
    pid: u32,
    start: u64,
    mut daemon: layerfs_sandbox::backend::process::Sandbox,
) -> String {
    match daemon.stop(Duration::from_secs(5)) {
        Ok(()) => "the restarted daemon is stopped".to_owned(),
        Err(error) => {
            let SdkValue::Map(mut fields) = value.clone() else {
                return format!("stopping the restarted daemon pid:{pid}@{start} failed: {error}");
            };
            for (key, field) in fields.iter_mut() {
                match key.as_str() {
                    "pid" => *field = SdkValue::Int(pid.into()),
                    "start" => *field = SdkValue::text(start.to_string()),
                    "container" => *field = SdkValue::text(format!("pid:{pid}@{start}")),
                    _ => {}
                }
            }
            let recorded = save(&directory.join("sandbox.record"), SdkValue::Map(fields));
            format!(
                "stopping the restarted daemon pid:{pid}@{start} failed: {error}; its record \
                 {} it",
                if recorded.is_ok() {
                    "now names"
                } else {
                    "could not be made to name"
                }
            )
        }
    }
}

/// The handshake with a restarted daemon and its saved identity.
#[cfg(target_os = "linux")]
fn record_restarted(
    args: &SdkValue,
    id: &str,
    directory: &Path,
    value: &SdkValue,
    name: &str,
    runtime: Runtime,
) -> Result<Arc<SandboxState>> {
    let directory = directory.to_owned();
    let name = name.to_owned();
    let mut dial = crate::pool::Dial {
        endpoint: runtime.endpoint().map_err(failed)?,
        host: crate::pool::daemon_host(args),
        secret: bytes(value, "secret")?,
        peer: bytes(value, "peer")?,
        instance: [0; 32],
    };
    let (control, instance) = dial.first()?;
    dial.instance = instance;
    let (secret, peer) = (dial.secret, dial.peer);
    let state = Arc::new(SandboxState {
        id: id.to_owned(),
        project: registry::text(value, "project")?.to_owned(),
        name: name.clone(),
        directory: directory.clone(),
        runtime,
        identity: CommandIdentity { uid: 501, gid: 20 },
        controls: crate::pool::Controls::new(control, dial),
        mounts: Default::default(),
        processes: Default::default(),
    });
    save_sandbox(&directory, &state, args, secret, peer, instance)?;
    replace_text(&directory.join("container"), &state.runtime.identity())?;
    registry::lock(&registry::SANDBOXES)?.insert(directory.clone(), state.clone());
    Ok(state)
}

/// The plain `container` text beside the record, replaced in one rename.
#[cfg(target_os = "linux")]
fn replace_text(path: &Path, text: &str) -> Result<()> {
    let temp = path.with_extension(format!("{}.new", std::process::id()));
    save_text(&temp, text)?;
    fs::rename(&temp, path).map_err(failed)
}

/// One Workspace of the answer: `Ok(true)` reopened by this call,
/// `Ok(false)` already served by the running daemon, `Err` not mounted.
/// `reason` is `None` exactly when `reopened` is true.
#[cfg(target_os = "linux")]
fn outcome(saved: &SavedWorkspace, result: std::result::Result<bool, String>) -> SdkValue {
    let (mounted, reopened, reason) = match result {
        Ok(true) => (true, true, None),
        Ok(false) => (true, false, Some("already mounted".to_owned())),
        Err(reason) => (false, false, Some(reason)),
    };
    SdkValue::map([
        ("path", SdkValue::text(&saved.path)),
        (
            "workspace",
            SdkValue::text(layerfs_bridge::sdk_values::hex(&saved.workspace.to_bytes())),
        ),
        ("namespace", SdkValue::Int(saved.namespace)),
        ("branch", SdkValue::text(&saved.branch)),
        ("mounted", SdkValue::Bool(mounted)),
        ("reopened", SdkValue::Bool(reopened)),
        ("reason", SdkValue::opt_text(reason)),
    ])
}

/// Reopens one saved Workspace on the restarted daemon `state` and saves its
/// record again, now with its root. Its record and overlay stay on failure.
#[cfg(target_os = "linux")]
fn reattach(
    args: &SdkValue,
    state: &SandboxState,
    saved: &SavedWorkspace,
) -> std::result::Result<(), String> {
    let branch = crate::direct::branch_id(args, &saved.branch).map_err(|r| r.message)?;
    let root = revive_rules::base(saved.root)?;
    let mounted = {
        let mut control = crate::trace::control(&state.controls).map_err(|r| r.message)?;
        workspace(&mut control).reopen(saved.workspace, branch, saved.namespace, root)
    }
    .map_err(|failure| format!("{failure:?}"))?;
    if mounted.ready.directory != saved.path {
        return Err(format!(
            "reopened at {}, not its saved path",
            mounted.ready.directory
        ));
    }
    save_workspace(state, &mounted, &saved.branch).map_err(|r| r.message)?;
    registry::lock(&state.mounts)
        .map_err(|r| r.message)?
        .insert(
            mounted.ready.directory.clone(),
            Mount {
                workspace: mounted,
                branch: saved.branch.clone(),
            },
        );
    Ok(())
}

/// Wait for a restarted daemon's listener.
#[cfg(target_os = "linux")]
const STARTUP: Duration = Duration::from_secs(30);

#[cfg(all(test, target_os = "linux"))]
#[path = "../../tests/unit/revive.rs"]
mod revive_tests;
