use crate::{actor::Actor, fixture};
use std::{
    fs,
    io::Write,
    os::unix::fs::MetadataExt,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

pub fn marker(root: &Path, pid: &str, stage: &str) -> std::path::PathBuf {
    root.join(format!("admission-{pid}.{stage}"))
}
pub fn enabled() -> bool {
    std::env::var_os("REOPEN_SCHEDULE_ADMISSION").is_some()
}
pub fn pid(identity: &str) -> &str {
    identity
        .strip_prefix("pid:")
        .expect("pid")
        .split('@')
        .next()
        .expect("pid number")
}
pub fn admitted(root: &Path, identity: &str) -> bool {
    !enabled() || marker(root, pid(identity), "admitted").exists()
}
pub fn advance(root: &Path, actor: &mut Actor, placed: &str) {
    let identity = actor
        .event
        .strip_prefix("spawned ")
        .expect("spawned child")
        .to_owned();
    let pid = pid(&identity);
    fs::write(marker(root, pid, "go"), b"go").expect("release daemon admission");
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if marker(root, pid, "admitted").exists() {
            eprintln!(
                "ADMISSION {identity} kernel-flock={}",
                owns_flock(root, pid)
            );
            actor.event = format!("admitted {identity}");
            return;
        }
        if !crate::observe::live_selected(root, false).contains(&identity) {
            break;
        }
        assert!(Instant::now() < until, "daemon admission did not settle");
        thread::sleep(Duration::from_millis(2));
    }
    writeln!(actor.input, "go ok {placed} true").expect("finish refused admission");
    actor.input.flush().expect("admit flush");
    actor.receive();
}
fn owns_flock(root: &Path, pid: &str) -> bool {
    let lock = fixture::layout(root)
        .config()
        .with_file_name("daemon.setup.lock");
    let Ok(meta) = fs::metadata(lock) else {
        return false;
    };
    let Ok(fds) = fs::read_dir(format!("/proc/{pid}/fdinfo")) else {
        return false;
    };
    fds.filter_map(Result::ok).any(|fd| {
        fs::read_to_string(fd.path()).is_ok_and(|text| {
            text.lines().any(|line| {
                line.starts_with("lock:")
                    && line.contains("FLOCK")
                    && line.ends_with("0 EOF")
                    && line.contains(&format!(":{} ", meta.ino()))
            })
        })
    })
}
pub fn prepare_refusal(root: &Path) {
    let mount = root.join("runtime/workspaces/prepare-refusal");
    fs::create_dir_all(&mount).expect("prepare fault home");
    assert!(Command::new("mount")
        .args(["-t", "tmpfs", "none"])
        .arg(mount)
        .status()
        .expect("prepare fault mount")
        .success());
}

pub fn spawned_ready(root: &Path, event: &str) {
    if !enabled() {
        return;
    }
    let Some(identity) = event.strip_prefix("spawned ") else {
        return;
    };
    let ready = marker(root, pid(identity), "ready");
    let until = Instant::now() + Duration::from_secs(3);
    while !ready.exists() {
        assert!(
            Instant::now() < until,
            "spawned child did not reach pre-flock checkpoint"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
