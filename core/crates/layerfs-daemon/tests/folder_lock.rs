//! One daemon per Sandbox folder (#373, third model review R3-B2). A restart
//! whose handshake failed, whose stop failed and whose record save failed
//! leaves its daemon running while the Sandbox record names an ended one; the
//! next revive then spawned a second daemon on the same folder (measured on
//! d08e3b656 by fault injection). Whatever the records say, a daemon started
//! on a config whose folder another daemon holds must exit at once and name
//! the holder, and the holder must keep running.
#![cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "support/host_install.rs"]
mod support;
use layerfs_bridge::{
    daemon_setup::{DaemonLimits, DaemonSetup},
    duplex,
};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The private config of the Sandbox folder `f.directory`.
fn configure(f: &support::Fixture) -> PathBuf {
    fs::set_permissions(&f.directory, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir(f.directory.join("mounts")).unwrap();
    let setup = DaemonSetup {
        listen: "127.0.0.1:0".into(),
        private_key: support::SERVER_PRIVATE,
        control_peer: duplex::public_key(&support::CLIENT_PRIVATE).unwrap(),
        store: f.project.manifest.locator.clone(),
        overlay: f.directory.join("overlay.sqlite").to_str().unwrap().into(),
        mounts: f.directory.join("mounts").to_str().unwrap().into(),
        command_uid: 65534,
        command_gid: 65534,
        limits: DaemonLimits {
            connections: 4,
            handshake_ms: 3000,
            cache_bytes: 64 * 1024,
            namespaces: 8,
            pager_kib: 1024,
            serial_low_water: 0,
            idle_exit_s: 0,
        },
        existing_store: None,
    };
    let config = f.directory.join("daemon.setup");
    fs::write(&config, setup.encode().unwrap()).unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
    config
}

/// The executable on `config`, its stderr in `log`.
fn spawn(config: &Path, log: &Path) -> Process {
    Process(
        Command::new(env!("CARGO_BIN_EXE_layerfs-daemon"))
            .arg("--config")
            .arg(config)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(File::create(log).unwrap()))
            .spawn()
            .unwrap(),
    )
}

/// The first stdout line of `process`, read within 5 s.
fn first_line(process: &mut Process) -> String {
    let stdout = process.0.stdout.take().unwrap();
    let (send, receive) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout.take(256))
            .read_line(&mut line)
            .map(|_| line);
        let _ = send.send(result);
    });
    let line = receive
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    reader.join().unwrap();
    line
}

#[test]
fn a_second_daemon_on_a_held_folder_exits_and_names_the_holder() {
    let f = support::Fixture::new("folder-lock", None);
    let config = configure(&f);
    let mut holder = spawn(&config, &f.directory.join("holder.log"));
    let holder_pid = holder.0.id();
    let listen = first_line(&mut holder);
    assert!(
        listen.starts_with("LAYERFS_DAEMON_LISTEN "),
        "{listen:?} {}",
        fs::read_to_string(f.directory.join("holder.log")).unwrap()
    );

    let log = f.directory.join("second.log");
    let mut second = spawn(&config, &log);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = second.0.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        thread::sleep(Duration::from_millis(5));
    };
    let held = holder.0.try_wait().unwrap().is_none();
    let mut stdout = String::new();
    if status.is_some() {
        second
            .0
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
    }
    let stderr = fs::read_to_string(&log).unwrap();
    drop(second);
    drop(holder);
    f.cleanup();

    let status = status.expect("the second daemon still runs on the held folder");
    assert!(!status.success(), "{status:?}");
    assert_eq!(stdout, "", "the second daemon wrote a listen line");
    assert_eq!(
        stderr,
        format!(
            "layerfs-daemon: the Sandbox folder is held by daemon pid {}\n",
            holder_pid
        )
    );
    assert!(held, "the holder stopped");
}
