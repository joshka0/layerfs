//! Positive path with no container runtime: one Linux host (an Apple
//! `container` VM) runs Init, the daemon as a child process, and ordinary
//! commands through `setpriv`. Init, install, mount, mutate, commit, unmount,
//! then a fresh mount of the same Branch must see the committed file.
use layerfs_api::{internal, InitRequest, ProjectApi};
use layerfs_bridge::{
    control::{DaemonPhase, HelloRequest, Reply, Request},
    daemon_setup::{DaemonLimits, DaemonSetup},
    duplex as transport,
};
use layerfs_history::{BranchId, HistoryCatalogConfig, HistoryName, LayerStackId, WorkspaceId};
use layerfs_persistence::{PersistenceConfig, SqlitePersistenceProfile};
use layerfs_storage::StoragePolicy;
use layerfs_telemetry::timer::Timing;
use std::{
    error::Error,
    fs,
    io::{BufRead, BufReader},
    net::TcpStream,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn hello(control: &mut internal::Control, wait_for_store: bool) -> Result<DaemonPhase, String> {
    match control.call(Request::Hello(HelloRequest {
        expected_instance: None,
        wait_for_store,
    })) {
        Ok(Reply::Hello(status)) => Ok(status.phase),
        other => Err(format!("hello {other:?}")),
    }
}
/// Runs `script` as the ordinary command identity 501:20 in `directory`.
fn ordinary(directory: &str, script: &str) -> Result<String, Box<dyn Error>> {
    let output = Command::new("setpriv")
        .args([
            "--reuid=501",
            "--regid=20",
            "--clear-groups",
            "bash",
            "-c",
            script,
        ])
        .current_dir(directory)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "command {:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "daemon-executable fresh-host-directory");
    let directory = PathBuf::from(&args[2]);
    fs::create_dir(&directory)?;
    let source = directory.join("source");
    fs::create_dir_all(source.join("src"))?;
    fs::write(source.join("README"), b"layerfs on apple container\n")?;
    fs::write(source.join("src/main.txt"), b"one\n")?;

    let branch = BranchId::from_authority([62; 16]);
    let (stack_name, branch_name) = (HistoryName::new("project")?, HistoryName::new("main")?);
    let project = Timing::disabled("init", |timer| {
        ProjectApi::new().init(
            InitRequest {
                source: source.clone(),
                store: PersistenceConfig::sqlite(directory.join("sealed.sqlite"))
                    .with_sqlite_profile(SqlitePersistenceProfile::Disposable),
                locator: "/layerfs-store/global/store.sqlite".into(),
                policy: StoragePolicy::frozen_default(),
                catalog: HistoryCatalogConfig {
                    binding_key: b"process-kernel-mount".to_vec(),
                    cursor_key: [64; 32],
                    incarnation: 1,
                },
                stack: LayerStackId::from_authority([61; 16]),
                stack_name,
                branch,
                branch_name,
                scope_seed: [65; 32],
                deadline: Instant::now() + Duration::from_secs(5),
            },
            timer,
        )
    })
    .0?;
    println!("INIT root={:?}", project.initialized.root);

    // Private daemon resources, as the Docker upload lays them out.
    for path in [
        "/layerfs-local",
        "/layerfs-local/config",
        "/layerfs-local/overlay",
    ] {
        fs::DirBuilder::new().mode(0o700).create(path)?;
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create("/layerfs-store/global")?;
    fs::DirBuilder::new().mode(0o755).create("/workspaces")?;
    let keys = transport::generate_keypair()?;
    let daemon = transport::generate_keypair()?;
    let setup = DaemonSetup {
        listen: "127.0.0.1:30421".into(),
        private_key: daemon.private,
        control_peer: keys.public,
        store: "/layerfs-store/global/store.sqlite".into(),
        overlay: "/layerfs-local/overlay/overlay.sqlite".into(),
        mounts: "/workspaces".into(),
        command_uid: 501,
        command_gid: 20,
        limits: DaemonLimits {
            connections: 4,
            handshake_ms: 2000,
            cache_bytes: 65536,
            namespaces: 8,
            pager_kib: 1024,
            serial_low_water: 256,
            idle_exit_s: 0,
        },
        existing_store: None,
    };
    let config = "/layerfs-local/config/daemon.setup";
    let encoded = setup.encode().map_err(|e| format!("config {e:?}"))?;
    std::io::Write::write_all(
        &mut fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(config)?,
        &encoded,
    )?;

    let mut child = Command::new(&args[1])
        .args(["--config", config])
        .stdout(Stdio::piped())
        .spawn()?;
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("piped")).read_line(&mut line)?;
    let address = line
        .strip_prefix("LAYERFS_DAEMON_LISTEN ")
        .ok_or_else(|| format!("listen line {line:?}"))?
        .trim()
        .to_owned();
    println!("DAEMON pid={} listen={address}", child.id());

    let result = (|| -> Result<(), Box<dyn Error>> {
        // No socket timeouts: the crate-private policy clear has no caller here.
        let stream = TcpStream::connect(&address)?;
        let connection = transport::initiate(stream, &keys.private, daemon.public)?;
        let mut control = internal::Control::new(connection);
        assert_eq!(hello(&mut control, false)?, DaemonPhase::InstallPending);
        let installed = internal::install_project(&mut control, &project)
            .map_err(|f| format!("install {f:?}"))?;
        assert_eq!(hello(&mut control, true)?, DaemonPhase::ControlReady);
        println!(
            "INSTALLED daemon_sqlite={:?}",
            installed.manifest.daemon_sqlite
        );

        let mounted = internal::workspace(&mut control)
            .mount(WorkspaceId::from_authority([71; 32])?, branch)
            .map_err(|f| format!("mount {f:?}"))?;
        let root = mounted.ready.directory.clone();
        println!("MOUNTED {root}");
        print!(
            "{}",
            ordinary(
                &root,
                "set -e; cat README; printf two >> src/main.txt; mkdir made; \
                 printf new > made/file; id -u; grep \" $PWD \" /proc/self/mountinfo | cut -d' ' -f5,9"
            )?
        );
        let outcome = internal::workspace(&mut control)
            .commit(mounted.bound.token)
            .map_err(|f| format!("commit {f:?}"))?;
        println!("COMMITTED {outcome:?}");
        internal::workspace(&mut control)
            .unmount(mounted.bound.token, false)
            .map_err(|f| format!("unmount {f:?}"))?;
        assert!(!PathBuf::from(&root).exists());
        println!("UNMOUNTED {root}");

        let fresh = internal::workspace(&mut control)
            .mount(WorkspaceId::from_authority([72; 32])?, branch)
            .map_err(|f| format!("fresh mount {f:?}"))?;
        let seen = ordinary(&fresh.ready.directory, "cat src/main.txt made/file")?;
        assert_eq!(seen, "one\ntwonew");
        println!("FRESH_MOUNT_SEES_COMMIT {seen:?}");
        internal::workspace(&mut control)
            .unmount(fresh.bound.token, false)
            .map_err(|f| format!("fresh unmount {f:?}"))?;
        assert!(matches!(
            control.call(Request::EndSession),
            Ok(Reply::SessionEnded)
        ));
        Ok(())
    })();
    child.kill()?;
    child.wait()?;
    result?;
    println!("PROCESS_KERNEL_MOUNT ok");
    Ok(())
}
