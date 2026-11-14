use layerfs_bridge::control::SdkValue;
use layerfs_sandbox::backend::{process, Runtime};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub fn unwrap<T>(r: Result<T, layerfs_bridge::control::SdkRefusal>) -> T {
    r.unwrap_or_else(|e| panic!("{}: {}", e.code, e.message))
}
pub fn private(path: &Path) {
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .expect("private directory");
}
pub fn args(root: &Path) -> SdkValue {
    unwrap(crate::discovery::load(&root.join("args.record")))
}
pub fn directory(root: &Path) -> PathBuf {
    root.join("home/proj/sandboxes/1")
}
pub fn layout(root: &Path) -> process::Layout {
    process::Layout {
        directory: root.join("runtime"),
    }
}
pub fn seed(root: &Path, overlays: usize, eligible: usize) {
    assert!(
        root.starts_with(std::env::temp_dir()),
        "temporary root required"
    );
    private(root);
    private(&root.join("home"));
    private(&root.join("source"));
    private(&root.join("global"));
    fs::write(root.join("source/kept.txt"), b"saved data\n").expect("source");
    let daemon = std::env::var("REOPEN_DAEMON").expect("REOPEN_DAEMON");
    let args = SdkValue::map([
        (
            "home",
            SdkValue::text(root.join("home").display().to_string()),
        ),
        ("project", SdkValue::text("proj")),
        ("name", SdkValue::text("proj")),
        (
            "path",
            SdkValue::text(root.join("source").display().to_string()),
        ),
        (
            "storePath",
            SdkValue::text(root.join("global/store.sqlite").display().to_string()),
        ),
        ("branch", SdkValue::text("work")),
        (
            "deploy",
            SdkValue::map([
                ("runtime", SdkValue::text("process")),
                ("daemon", SdkValue::text(&daemon)),
            ]),
        ),
    ]);
    unwrap(crate::project::init(&args));
    unwrap(crate::discovery::save(
        &root.join("args.record"),
        args.clone(),
    ));
    private(&root.join("home/proj/sandboxes"));
    let directory = directory(root);
    private(&directory);
    let project = unwrap(crate::discovery::project(&args));
    let controller = layerfs_bridge::duplex::generate_keypair().expect("controller key");
    let daemon_key = layerfs_bridge::duplex::generate_keypair().expect("daemon key");
    let layout = layout(root);
    let setup = layerfs_bridge::daemon_setup::DaemonSetup {
        listen: "127.0.0.1:0".into(),
        private_key: daemon_key.private,
        control_peer: controller.public,
        store: root.join("global/store.sqlite").display().to_string(),
        overlay: layout.overlay().display().to_string(),
        mounts: layout.mounts().display().to_string(),
        command_uid: 501,
        command_gid: 20,
        limits: layerfs_bridge::daemon_setup::DaemonLimits {
            connections: 8,
            handshake_ms: 2000,
            cache_bytes: 1024 * 1024,
            namespaces: 16,
            pager_kib: 1024,
            serial_low_water: 256,
            idle_exit_s: 0,
        },
        existing_store: project.installed.clone(),
    };
    let runtime = process::Sandbox::start(
        Path::new(&daemon),
        &setup.encode().expect("setup"),
        layout.clone(),
        Duration::from_secs(5),
    )
    .expect("seed daemon");
    let mut dial = crate::pool::Dial {
        endpoint: runtime.endpoint().expect("endpoint"),
        host: None,
        secret: controller.private,
        peer: daemon_key.public,
        instance: [0; 32],
    };
    let (mut control, instance) = unwrap(dial.first());
    dial.instance = instance;
    unwrap(crate::direct::acquire(&args));
    let SdkValue::Map(mut fork) = args.clone() else {
        unreachable!("fixture args map")
    };
    fork.iter_mut()
        .find(|(k, _)| k == "name")
        .expect("name argument")
        .1 = SdkValue::text("work");
    unwrap(crate::direct::call(
        "branch.fork",
        SdkValue::Map(fork),
        false,
    ));
    let branch = unwrap(crate::direct::branch_id(&args, "work"));
    let mut mounted = Vec::new();
    for index in 0..overlays.max(1) {
        let id = layerfs_history::WorkspaceId::from_authority([7 + index as u8; 32])
            .expect("workspace id");
        let workspace = layerfs_api::internal::workspace(&mut control)
            .mount(id, branch)
            .expect("seed mount");
        fs::write(
            Path::new(&workspace.ready.directory).join("uncommitted.txt"),
            b"overlay survives\n",
        )
        .expect("overlay write");
        mounted.push(workspace);
    }
    let state = Arc::new(crate::registry::SandboxState {
        id: "1".into(),
        project: "proj".into(),
        name: "replay-proj-1".into(),
        directory: directory.clone(),
        runtime: Runtime::Process(
            process::Sandbox::reopen(layout, runtime.pid(), runtime.start_time())
                .expect("live seed"),
        ),
        identity: layerfs_sandbox::CommandIdentity { uid: 501, gid: 20 },
        controls: crate::pool::Controls::new(control, dial),
        mounts: Default::default(),
        processes: Default::default(),
    });
    unwrap(crate::discovery::save_sandbox(
        &directory,
        &state,
        &args,
        controller.private,
        daemon_key.public,
        instance,
    ));
    for (index, workspace) in mounted.iter().enumerate() {
        unwrap(crate::discovery::save_workspace(&state, workspace, "work"));
        if index >= eligible {
            let path = crate::discovery::workspace_file(&state, workspace.bound.token);
            let SdkValue::Map(mut fields) = unwrap(crate::discovery::load(&path)) else {
                unreachable!("workspace record")
            };
            fields.retain(|(name, _)| name != "root");
            unwrap(crate::discovery::save(&path, SdkValue::Map(fields)));
        }
    }
    unwrap(crate::discovery::save_text(
        &directory.join("container"),
        &runtime.identity(),
    ));
    match std::env::var("REOPEN_FIXTURE").as_deref() {
        Ok("missing-base") => {
            let path = crate::discovery::workspace_file(&state, mounted[0].bound.token);
            let SdkValue::Map(mut fields) = unwrap(crate::discovery::load(&path)) else {
                unreachable!("workspace record")
            };
            fields.retain(|(name, _)| name != "root");
            unwrap(crate::discovery::save(&path, SdkValue::Map(fields)));
        }
        Ok("handshake-failure") => {
            let path = directory.join("sandbox.record");
            let SdkValue::Map(mut fields) = unwrap(crate::discovery::load(&path)) else {
                unreachable!("sandbox record")
            };
            fields
                .iter_mut()
                .find(|(name, _)| name == "secret")
                .expect("controller secret")
                .1 = SdkValue::Bytes(vec![9; 32]);
            unwrap(crate::discovery::save(&path, SdkValue::Map(fields)));
        }
        Ok("rootless") => {}
        Ok(other) => panic!("unknown failure fixture {other}"),
        Err(_) => {}
    }
    drop(state);
    unwrap(crate::direct::release(&args));
    // End the daemon as a host restart does, by exact known pid. Keep the
    // dead mount until restart's own detach logic runs.
    let status = std::process::Command::new("kill")
        .args(["-KILL", &runtime.pid().to_string()])
        .status()
        .expect("kill seed");
    assert!(status.success(), "seed kill failed");
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while runtime.running() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!runtime.running(), "seed daemon did not end");
    if overlays == 0 {
        // This is the F1 fixture: retain a record but remove its overlay.
        let status = std::process::Command::new("umount")
            .args(["-l", &mounted[0].ready.directory])
            .status()
            .expect("detach seed mount");
        assert!(status.success(), "seed detach failed");
        fs::remove_file(root.join(format!(
            "runtime/overlay/{}.sqlite",
            mounted[0].bound.token.namespace
        )))
        .expect("remove overlay fixture");
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).expect("root mode");
    if std::env::var("REOPEN_FIXTURE").as_deref() == Ok("rootless") {
        rootless_owner(root, &root.join("runtime/workspaces"));
    }
}

// Give the private fixture to the real unprivileged caller. Keep dead FUSE
// mount paths untouched: the kernel must refuse their unprivileged detach.
fn rootless_owner(path: &Path, mounts: &Path) {
    if path.starts_with(mounts) {
        return;
    }
    let metadata = fs::symlink_metadata(path).expect("rootless fixture metadata");
    assert!(
        !metadata.file_type().is_symlink(),
        "fixture must have no external symlink"
    );
    if metadata.is_dir() {
        for entry in fs::read_dir(path).expect("rootless fixture directory") {
            rootless_owner(&entry.expect("fixture entry").path(), mounts);
        }
    }
    std::os::unix::fs::chown(path, Some(501), Some(20)).expect("rootless fixture owner");
}
