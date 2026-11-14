use crate::hooks::{point, CHECKED, HELD, TARGET_FAILED, TARGET_OK, TARGET_REASON};
use layerfs_bridge::control::SdkValue;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::io::AsRawFd,
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::atomic::Ordering,
};
extern "C" {
    fn poll(fds: *mut [i32; 2], count: u64, timeout: i32) -> i32;
}
pub fn actor(root: &str, caller: &str) {
    std::env::set_var("REOPEN_ACTOR", caller);
    std::env::set_var("REOPEN_ROOT", root);
    if caller == "project-lock" || caller == "sandbox-lock" {
        std::env::remove_var("REOPEN_ACTOR");
        let directory = if caller == "project-lock" {
            Path::new(root).join("home/proj")
        } else {
            crate::fixture::directory(Path::new(root))
        };
        match crate::discovery::guard(&directory) {
            Ok(guard) => {
                point("held");
                drop(guard);
                point("done");
            }
            Err(refusal) => {
                assert_eq!(refusal.code, "Busy", "lock deadline refusal");
                point("busy");
            }
        }
        return;
    }
    if std::env::var("REOPEN_FIXTURE").as_deref() == Ok("rootless") {
        rootless_uid();
    }
    let args = crate::fixture::args(Path::new(root));
    // Mount and boot's Store client lives in this actor, as in the SDK.
    if caller != "list" {
        crate::fixture::unwrap(crate::direct::acquire(&args));
    }
    let outcome = match caller {
        "boot" => crate::discovery::reopen_all(&args),
        "mount" => crate::workspace::mount(&args),
        "list" => crate::workspace::list(&args),
        "status" => {
            crate::registry::sandbox(&crate::registry::naming(&args, "1")).map(|_| SdkValue::Null)
        }
        _ => panic!("invalid caller {caller}"),
    };
    // A boot that visits no record and a placement that finds no target end
    // during check. A read-only list has no acquisition or check callback.
    if !CHECKED.load(Ordering::SeqCst) && HELD.load(Ordering::SeqCst) {
        println!("returned");
    }
    let report_value = match &outcome {
        Ok(value) => value,
        Err(refusal) => &refusal.detail,
    };
    let reported =
        matches!(report_value.get("revived"), Some(SdkValue::List(items)) if !items.is_empty());
    println!(
        "target {}",
        if TARGET_FAILED.load(Ordering::SeqCst) {
            "failed"
        } else if TARGET_OK.load(Ordering::SeqCst) {
            "ok"
        } else {
            ""
        }
    );
    println!("reported {reported}");
    println!(
        "unusable {}",
        crate::hooks::UNUSABLE.lock().expect("unusable")
    );
    println!(
        "target-reason {}",
        TARGET_REASON
            .lock()
            .expect("target reason")
            .replace('\n', " ")
            .replace('|', "/")
    );
    match &outcome {
        Ok(value) => {
            let mut answer = "ok".to_owned();
            if let Some(SdkValue::List(items)) = value.get("sandboxes") {
                for item in items {
                    answer.push_str(&format!(
                        ":{}",
                        item.get_text("state").unwrap_or("missing-state")
                    ));
                    if let Some(SdkValue::List(workspaces)) = item.get("workspaces") {
                        for workspace in workspaces {
                            answer.push_str(&format!(
                                ":mounted={}:{}",
                                workspace.get_bool("mounted"),
                                workspace.get_text("reason").unwrap_or("reopened")
                            ));
                        }
                    }
                }
            }
            println!("result {}", answer.replace('\n', " ").replace('|', "/"));
        }
        Err(r) if r.code == "ReplayPlacementEnd" => println!(
            "result {}",
            if TARGET_FAILED.load(Ordering::SeqCst) {
                "target-failed"
            } else {
                "placement-end"
            }
        ),
        Err(r) => println!("result refusal:{}:{}", r.code, r.message),
    }
    if caller != "list" {
        crate::fixture::unwrap(crate::direct::release(&args));
    }
    point("done");
}
pub(crate) struct Actor {
    pub(crate) child: Child,
    pub(crate) input: ChildStdin,
    pub(crate) output: BufReader<ChildStdout>,
    pub(crate) event: String,
    pub(crate) seen: String,
    pub(crate) result: String,
    pub(crate) target: String,
    pub(crate) reported: bool,
    pub(crate) reason: String,
    pub(crate) unusable: String,
}
impl Actor {
    pub(crate) fn spawn(root: &Path, caller: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().expect("worker executable"))
            .arg("actor")
            .arg(root)
            .arg(caller)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("actor start");
        Self {
            input: child.stdin.take().expect("actor stdin"),
            output: BufReader::new(child.stdout.take().expect("actor stdout")),
            child,
            event: String::new(),
            seen: String::new(),
            result: String::new(),
            target: String::new(),
            reported: false,
            reason: String::new(),
            unusable: String::new(),
        }
    }
    pub(crate) fn start(root: &Path, caller: &str) -> Self {
        let mut actor = Self::spawn(root, caller);
        actor.receive();
        actor
    }
    pub(crate) fn ready(&self, ms: i32) -> bool {
        let mut fd = [self.output.get_ref().as_raw_fd(), 1];
        // SAFETY: fd is one Linux pollfd (i32 fd, i16 events, i16 revents).
        (unsafe { poll(&mut fd, 1, ms) }) == 1
    }
    pub(crate) fn receive(&mut self) {
        loop {
            let mut line = String::new();
            assert!(
                self.output.read_line(&mut line).expect("actor event") > 0,
                "actor ended before checkpoint: {:?}",
                self.child.try_wait()
            );
            let line = line.trim().to_owned();
            if let Some(seen) = line.strip_prefix("seen ") {
                self.seen = seen.to_owned();
            } else if let Some(result) = line.strip_prefix("result ") {
                self.result = result.to_owned();
            } else if line == "target" {
                self.target.clear();
            } else if let Some(target) = line.strip_prefix("target ") {
                self.target = target.to_owned();
            } else if let Some(reported) = line.strip_prefix("reported ") {
                self.reported = reported == "true";
            } else if line == "unusable" {
                self.unusable.clear();
            } else if let Some(value) = line.strip_prefix("unusable ") {
                self.unusable = value.to_owned();
            } else if line == "target-reason" {
                self.reason.clear();
            } else if let Some(reason) = line.strip_prefix("target-reason ") {
                self.reason = reason.to_owned();
            } else if line == "returned" {
                continue;
            } else {
                self.event = line;
                return;
            }
        }
    }
    pub(crate) fn advance(&mut self) {
        writeln!(self.input, "go").expect("release checkpoint");
        self.input.flush().expect("checkpoint write");
        self.receive();
    }
}
impl Drop for Actor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn rootless_uid() {
    extern "C" {
        fn setuid(uid: u32) -> i32;
        fn setgid(gid: u32) -> i32;
    }
    // SAFETY: this dedicated process permanently drops its own identity.
    assert_eq!(unsafe { setgid(20) }, 0, "rootless gid");
    assert_eq!(unsafe { setuid(501) }, 0, "rootless uid");
}
pub fn unprivileged_identity(root: &str) {
    rootless_uid();
    let path = crate::fixture::directory(Path::new(root)).join("sandbox.record");
    let value = crate::fixture::unwrap(crate::discovery::load(&path));
    println!("{}", value.get_text("container").expect("identity"));
}
