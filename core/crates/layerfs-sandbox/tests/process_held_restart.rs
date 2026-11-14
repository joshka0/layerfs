//! #373 R4-B1. A restarted daemon that finds its Sandbox folder held writes
//! one line naming the holder and exits at once. The restart must name that
//! line in its failure however early the child exits, also when it is gone
//! before the spawner reads its identity; the revive answer carries the
//! failure, so it names the holder pid.
#![cfg(target_os = "linux")]
use layerfs_sandbox::backend::process::{Layout, Sandbox};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    time::Duration,
};

const HELD: &str = "layerfs-daemon: the Sandbox folder is held by daemon pid 4242";

#[test]
fn a_restart_names_the_output_of_a_daemon_that_exits_at_once() {
    let base = std::env::temp_dir().join(format!("layerfs-held-restart-{}", std::process::id()));
    let directory = base.join("sandbox");
    fs::DirBuilder::new()
        .mode(0o755)
        .recursive(true)
        .create(directory.join("config"))
        .unwrap();
    fs::write(directory.join("config/daemon.setup"), b"setup").unwrap();
    // stands in for a daemon that finds the folder's lock held
    let daemon = base.join("held-daemon");
    fs::write(&daemon, format!("#!/bin/sh\necho '{HELD}' >&2\nexit 1\n")).unwrap();
    fs::set_permissions(&daemon, fs::Permissions::from_mode(0o755)).unwrap();
    let layout = Layout { directory };
    // The child's exit races the spawner's read of its identity: repeat so
    // both orders occur.
    let mut unnamed = Vec::new();
    for _ in 0..200 {
        match Sandbox::restart(&daemon, layout.clone(), 0, 0, Duration::from_secs(5)) {
            Ok(_) => panic!("the restart of a daemon that exited succeeded"),
            Err(failure) => {
                let text = failure.cause.to_string();
                if !text.contains(HELD) {
                    unnamed.push((failure.phase, text));
                }
            }
        }
    }
    fs::remove_dir_all(&base).unwrap();
    assert!(
        unnamed.is_empty(),
        "{} of 200 failures do not name the holder, e.g. {:?}",
        unnamed.len(),
        unnamed.first()
    );
}
