//! A restart whose daemon spawned but whose handshake failed leaves no daemon
//! running: the record still names the ended one, so a running leftover would
//! be a second daemon on the folder at the next revive (Quint sandbox_reopen,
//! OneDaemon). Included by discovery/revive.rs, Linux only.
use super::revive;
use layerfs_bridge::control::SdkValue;
use std::{
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// A private home with process Sandbox 1 of project `proj`: its daemon
/// (pid 0, never running) ended, one saved Workspace with an overlay file,
/// and a fake daemon executable that prints a listen line for a closed port,
/// writes its pid and sleeps.
fn fixture(tag: &str, first_line: &str) -> (PathBuf, SdkValue) {
    let base = std::env::temp_dir().join(format!("layerfs-revive-{}-{tag}", std::process::id()));
    let private = |path: &Path| {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(path)
            .unwrap()
    };
    let home = base.join("home");
    let sandbox = home.join("proj").join("sandboxes").join("1");
    private(&sandbox);
    let root = base.join("sandbox-root");
    private(&root.join("config"));
    private(&root.join("overlay"));
    private(&root.join("workspaces"));
    std::fs::write(root.join("config/daemon.setup"), b"setup").unwrap();
    std::fs::write(root.join("overlay/3.sqlite"), b"").unwrap();
    let daemon = base.join("fake-daemon");
    std::fs::write(
        &daemon,
        format!(
            "#!/bin/sh\necho $$ > {}\necho {first_line}\nexec sleep 600\n",
            base.join("pid").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&daemon, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bytes = |n: u8| SdkValue::Bytes(vec![n; 32]);
    crate::discovery::save(
        &sandbox.join("sandbox.record"),
        SdkValue::map([
            ("id", SdkValue::text("1")),
            ("project", SdkValue::text("proj")),
            ("name", SdkValue::text("layerfs-proj-1")),
            ("container", SdkValue::text("pid:0@0")),
            ("secret", bytes(1)),
            ("peer", bytes(2)),
            ("instance", bytes(3)),
            ("runtime", SdkValue::text("process")),
            ("root", SdkValue::text(root.to_str().unwrap())),
            ("pid", SdkValue::Int(0)),
            ("start", SdkValue::text("0")),
        ]),
    )
    .unwrap();
    crate::discovery::save(
        &sandbox.join(format!("workspace-{}.record", "ab".repeat(32))),
        SdkValue::map([
            (
                "path",
                SdkValue::text(root.join("workspaces/3").to_str().unwrap()),
            ),
            ("branch", SdkValue::text("work")),
            ("workspace", bytes(9)),
            ("namespace", SdkValue::Int(3)),
            ("root", bytes(7)),
        ]),
    )
    .unwrap();
    let args = SdkValue::map([
        ("project", SdkValue::text("proj")),
        ("home", SdkValue::text(home.to_str().unwrap())),
        (
            "deploy",
            SdkValue::map([("daemon", SdkValue::text(daemon.to_str().unwrap()))]),
        ),
    ]);
    (base, args)
}

fn running(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|stat| !matches!(stat.rsplit_once(") ").map(|(_, r)| &r[..1]), Some("Z")))
}

/// Revives Sandbox 1 of a fixture whose fake daemon prints `first_line`;
/// the restart must fail and leave its daemon stopped.
fn failed_restart_leaves_no_daemon(tag: &str, first_line: &str) {
    let (base, args) = fixture(tag, first_line);
    let (item, state) = revive(&args, "1", false).unwrap().expect("listed");
    assert!(state.is_none(), "no state for a failed restart");
    assert_eq!(item.get_text("state"), Some("failed"), "{item:?}");
    let pid: u32 = std::fs::read_to_string(base.join("pid"))
        .expect("the fake daemon ran")
        .trim()
        .parse()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while running(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let left = running(pid);
    if left {
        // leave nothing behind, then fail
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();
    }
    std::fs::remove_dir_all(&base).unwrap();
    assert!(
        !left,
        "the spawned daemon {pid} still runs after the failed restart: {item:?}"
    );
}

#[test]
fn a_restart_whose_handshake_fails_stops_its_daemon() {
    // a listen line for a closed port: the spawn succeeds, the handshake fails
    failed_restart_leaves_no_daemon("handshake", "LAYERFS_DAEMON_LISTEN 127.0.0.1:1");
}

#[test]
fn a_restart_whose_daemon_never_listens_stops_its_daemon() {
    // output before any listen line: the spawn fails at its Listener step
    // with the daemon still running (process.rs StartFailure::Listener)
    failed_restart_leaves_no_daemon("listener", "not-a-listen-line");
}

#[test]
fn an_ended_daemon_with_an_empty_log_is_ended_not_unobservable() {
    // F41: a restart moves the log aside and starts a new, empty one before
    // its daemon prints the listen line; meanwhile the record still names the
    // ended daemon. That daemon is ended: the revive restarts it.
    let (base, args) = fixture("empty-log", "LAYERFS_DAEMON_LISTEN 127.0.0.1:1");
    std::fs::write(base.join("sandbox-root/daemon.log"), b"").unwrap();
    let (item, _) = revive(&args, "1", false).unwrap().expect("listed");
    let restarted = base.join("pid").exists();
    if let Ok(pid) = std::fs::read_to_string(base.join("pid")) {
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.trim())
            .status();
    }
    std::fs::remove_dir_all(&base).unwrap();
    assert!(restarted, "the ended daemon was not restarted: {item:?}");
}
