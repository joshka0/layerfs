use crate::actor::Actor;
use std::{fs, io::Write, os::unix::io::AsRawFd, path::Path, process::Command};
extern "C" {
    fn flock(fd: i32, op: i32) -> i32;
}
fn identity(root: &Path) -> String {
    let path = crate::fixture::directory(root).join("sandbox.record");
    if !path.exists() {
        return "missing".into();
    }
    if std::env::var("REOPEN_FIXTURE").as_deref() == Ok("rootless") {
        let output = Command::new(std::env::current_exe().expect("worker executable"))
            .arg("identity")
            .arg(root)
            .output()
            .expect("owner observer");
        assert!(output.status.success(), "owner observer failed");
        return String::from_utf8(output.stdout)
            .expect("identity utf8")
            .trim()
            .to_owned();
    }
    crate::fixture::unwrap(crate::discovery::load(&path))
        .get_text("container")
        .expect("identity")
        .to_owned()
}
pub(crate) fn live_selected(root: &Path, all: bool) -> Vec<String> {
    let config = crate::fixture::layout(root).config();
    let mut out = Vec::new();
    for entry in fs::read_dir("/proc").expect("proc") {
        let entry = entry.expect("proc entry");
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(cmd) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let words: Vec<_> = cmd.split(|c| *c == 0).collect();
        if !words.iter().any(|w| {
            if all {
                std::str::from_utf8(w).is_ok_and(|s| {
                    s.starts_with(root.to_str().expect("root path"))
                        && s.ends_with("/config/daemon.setup")
                })
            } else {
                *w == config.as_os_str().as_encoded_bytes()
            }
        }) {
            continue;
        }
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let fields: Vec<_> = stat
            .rsplit_once(") ")
            .expect("stat comm")
            .1
            .split_whitespace()
            .collect();
        if fields[0] == "Z" || fields[0] == "X" {
            continue;
        }
        out.push(format!("pid:{pid}@{}", fields[19]));
    }
    out.sort();
    out
}
fn lock_held(root: &Path) -> bool {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("home/proj/ownership.lock"))
        .expect("project lock");
    // A separate open description asks the real kernel, not a driver flag.
    // SAFETY: the live File owns this fd; Linux flock takes no pointers.
    if unsafe { flock(file.as_raw_fd(), 2 | 4) } == 0 {
        // SAFETY: release only the lock on this probe's open description.
        assert_eq!(unsafe { flock(file.as_raw_fd(), 8) }, 0, "probe unlock");
        false
    } else {
        assert_eq!(
            std::io::Error::last_os_error().kind(),
            std::io::ErrorKind::WouldBlock,
            "kernel lock probe failed"
        );
        true
    }
}
pub(crate) fn snapshot(root: &Path, event: &str, actor: Option<&Actor>) {
    let folder = crate::fixture::directory(root)
        .join("sandbox.record")
        .exists();
    let mounts = fs::read_to_string("/proc/self/mountinfo").expect("mountinfo");
    let prefix = root.join("runtime/workspaces").display().to_string() + "/";
    // A dead FUSE entry is still in mountinfo until restart detaches it.
    // Count a served saved view only when ordinary filesystem access reads
    // its uncommitted payload. ENOTCONN is not a reopened Workspace.
    let mounted=mounts.lines().filter_map(|l| l.split(' ').nth(4)).filter(|p|p.starts_with(&prefix))
        .filter(|p| matches!(fs::read(Path::new(p).join("uncommitted.txt")), Ok(bytes) if bytes == b"overlay survives\n")).count();
    let raw = live_selected(root, false);
    let admitted: Vec<_> = raw
        .iter()
        .filter(|id| crate::admission::admitted(root, id))
        .cloned()
        .collect();
    let pending: Vec<_> = raw
        .iter()
        .filter(|id| !crate::admission::admitted(root, id))
        .cloned()
        .collect();
    println!(
        "OK|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        u8::from(folder),
        mounted,
        identity(root),
        admitted.join(","),
        u8::from(lock_held(root)),
        event,
        actor.map_or("", |a| a.seen.as_str()),
        actor.map_or("", |a| a.result.as_str()),
        actor.map_or("", |a| a.target.as_str()),
        actor.is_some_and(|a| a.reported),
        actor.map_or("", |a| a.reason.as_str()),
        raw.join(","),
        pending.join(","),
        actor.map_or("", |a| a.unusable.as_str())
    );
    std::io::stdout().flush().expect("snapshot flush");
}
pub(crate) fn clean(root: &Path) {
    // Exact fixture-owned processes and mount targets only.
    for id in live_selected(root, true) {
        let pid = id
            .strip_prefix("pid:")
            .expect("pid")
            .split('@')
            .next()
            .expect("pid number");
        let _ = Command::new("kill").args(["-KILL", pid]).status();
    }
    let mounts = fs::read_to_string("/proc/self/mountinfo").expect("mountinfo");
    let prefix = root.display().to_string() + "/";
    for path in mounts
        .lines()
        .filter_map(|l| l.split(' ').nth(4))
        .filter(|p| p.starts_with(&prefix))
    {
        let _ = Command::new("umount").args(["-l", path]).status();
    }
    // Keep all fixture paths for the runner's exact top-level cleanup.
}
