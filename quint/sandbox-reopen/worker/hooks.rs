use layerfs_bridge::control::{SdkRefusal, SdkValue};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
pub(crate) static TARGET_REASON: Mutex<String> = Mutex::new(String::new());
pub(crate) static TARGET_OK: AtomicBool = AtomicBool::new(false);
pub(crate) static TARGET_FAILED: AtomicBool = AtomicBool::new(false);
pub(crate) static CHECKED: AtomicBool = AtomicBool::new(false);
static CANDIDATE: Mutex<Option<[u8; 32]>> = Mutex::new(None);
pub(crate) static UNUSABLE: Mutex<String> = Mutex::new(String::new());
pub(crate) static HELD: AtomicBool = AtomicBool::new(false);

fn is_actor() -> bool {
    std::env::var_os("REOPEN_ACTOR").is_some()
}
pub(crate) fn point(text: &str) {
    println!("{text}");
    std::io::stdout().flush().expect("checkpoint flush");
    let mut go = String::new();
    assert!(
        std::io::stdin()
            .read_line(&mut go)
            .expect("checkpoint input")
            > 0,
        "scheduler disconnected"
    );
    let words: Vec<_> = go.split_whitespace().collect();
    assert_eq!(words[0], "go", "checkpoint command");
    if words.get(1) == Some(&"effect") {
        if let Some(placed) = words.get(3) {
            std::env::set_var("REOPEN_PLACED", placed);
        }
        if text == "checked restart" && words.get(2) == Some(&"false") {
            deny_syscalls(false, false, false, true);
        }
    }
}
pub fn target_failed(reason: &str) {
    if is_actor() {
        TARGET_FAILED.store(true, Ordering::SeqCst);
        *TARGET_REASON.lock().expect("target reason") = reason.to_owned();
        crate::fault_files::release();
        if reason.contains("stopping the restarted daemon") && reason.contains("failed:") {
            if let (Some(candidate), Ok(identity)) = (
                *CANDIDATE.lock().expect("candidate"),
                std::env::var("REOPEN_CHILD_ID"),
            ) {
                let root = PathBuf::from(std::env::var("REOPEN_ROOT").expect("root"));
                let saved = crate::fixture::unwrap(crate::discovery::load(
                    &crate::fixture::directory(&root).join("sandbox.record"),
                ));
                if saved.get_text("container") == Some(identity.as_str())
                    && !matches!(saved.get("instance"),Some(SdkValue::Bytes(bytes)) if bytes.as_slice()==candidate)
                {
                    *UNUSABLE.lock().expect("unusable") = identity;
                }
            }
        }
        if std::env::var("REOPEN_PLACED").as_deref() == Ok("false") {
            deny_syscalls(false, true, false, false);
        }
    }
}
pub fn target_ok() {
    if is_actor() {
        TARGET_OK.store(true, Ordering::SeqCst);
    }
}
pub fn is_target(path: &Path) -> bool {
    std::env::var("REOPEN_ROOT")
        .ok()
        .is_some_and(|root| path == Path::new(&root).join("runtime"))
}
pub fn processes() -> String {
    PathBuf::from(std::env::var("REOPEN_ROOT").expect("actor root"))
        .join("fallback-runtimes")
        .display()
        .to_string()
}
pub fn status_observed(args: &SdkValue, ends: bool) {
    if std::env::var("REOPEN_ACTOR").as_deref() == Ok("status") {
        let path =
            crate::fixture::directory(Path::new(&std::env::var("REOPEN_ROOT").expect("root")))
                .join("sandbox.record");
        if path.exists() {
            let value = crate::fixture::unwrap(crate::discovery::load(&path));
            println!("seen {}", value.get_text("container").expect("identity"));
        }
        let _ = args;
        if !ends {
            point("observed");
        }
    }
}
pub fn transport_fault(dial: &mut crate::pool::Dial) {
    if !is_actor() {
        return;
    }
    let outcome = std::env::var("REOPEN_FINISH_OUTCOME").unwrap_or_default();
    if outcome == "before_record" {
        if std::env::var("REOPEN_STOPPED").as_deref() == Ok("false") {
            let (_, instance) = crate::fixture::unwrap(dial.clone().first());
            *CANDIDATE.lock().expect("candidate") = Some(instance);
            deny_syscalls(false, false, false, false);
            if std::env::var("REOPEN_SAVED").as_deref() == Ok("false") {
                crate::fault_files::refuse(
                    crate::fixture::directory(Path::new(
                        &std::env::var("REOPEN_ROOT").expect("root"),
                    ))
                    .join("sandbox.record"),
                );
            }
        }
        dial.secret = [9; 32];
        return;
    }
    if outcome == "after_record" && std::env::var("REOPEN_STOPPED").as_deref() == Ok("false") {
        let (_, instance) = crate::fixture::unwrap(dial.clone().first());
        *CANDIDATE.lock().expect("candidate") = Some(instance);
    }
    match std::env::var("REOPEN_FINISH_OUTCOME").as_deref() {
        Ok("false")
        | Ok("stopped")
        | Ok("stop_failed")
        | Ok("recovery_save_failed")
        | Ok("fallback_failed")
        | Ok("kill_delayed") => {
            if matches!(
                std::env::var("REOPEN_FINISH_OUTCOME").as_deref(),
                Ok("stop_failed") | Ok("recovery_save_failed") | Ok("kill_delayed")
            ) {
                let (_, instance) = crate::fixture::unwrap(dial.clone().first());
                *CANDIDATE.lock().expect("candidate") = Some(instance);
            }
            dial.secret = [9; 32];
            if std::env::var("REOPEN_FINISH_OUTCOME").as_deref() == Ok("recovery_save_failed")
                || (std::env::var("REOPEN_FINISH_OUTCOME").as_deref() == Ok("stop_failed")
                    && std::env::var("REOPEN_SAVED").as_deref() == Ok("false"))
            {
                crate::fault_files::refuse(
                    crate::fixture::directory(Path::new(
                        &std::env::var("REOPEN_ROOT").expect("root"),
                    ))
                    .join("sandbox.record"),
                );
            }
            match std::env::var("REOPEN_FINISH_OUTCOME").as_deref() {
                Ok("stop_failed") => deny_syscalls(false, false, false, false),
                Ok("recovery_save_failed") => deny_syscalls(false, false, false, false),
                Ok("fallback_failed") => deny_syscalls(false, true, false, false),
                Ok("kill_delayed") => deny_syscalls(false, false, true, false),
                _ => {}
            }
        }
        Ok("after_record") => {
            let path =
                crate::fixture::directory(Path::new(&std::env::var("REOPEN_ROOT").expect("root")))
                    .join("container");
            crate::fault_files::refuse(path);
        }
        _ => {}
    }
}
// These process-local syscall refusals exercise real failure handling.
// The supervisor is not filtered and can clean up the exact fixture.
fn deny_syscalls(
    deny_rename: bool,
    deny_exec: bool,
    kill_returns_success: bool,
    deny_umount: bool,
) {
    #[repr(C)]
    struct Filter {
        code: u16,
        jt: u8,
        jf: u8,
        k: u32,
    }
    #[repr(C)]
    struct Program {
        len: u16,
        filter: *const Filter,
    }
    extern "C" {
        fn prctl(option: i32, ...) -> i32;
    }
    #[cfg(target_arch = "aarch64")]
    const KILL: u32 = 129;
    #[cfg(target_arch = "x86_64")]
    const KILL: u32 = 62;
    #[cfg(target_arch = "aarch64")]
    let renames = [38, 276];
    #[cfg(target_arch = "x86_64")]
    let renames = [82, 264, 316];
    #[cfg(target_arch = "aarch64")]
    let execs = [221, 281];
    #[cfg(target_arch = "x86_64")]
    let execs = [59, 322];
    let mut refused = if deny_exec {
        execs.to_vec()
    } else {
        vec![KILL]
    };
    if deny_umount {
        #[cfg(target_arch = "aarch64")]
        let syscall = 39;
        #[cfg(target_arch = "x86_64")]
        let syscall = 166;
        refused = vec![syscall];
    }
    if deny_rename {
        refused.extend(renames);
    }
    let mut filters = vec![Filter {
        code: 0x20,
        jt: 0,
        jf: 0,
        k: 0,
    }];
    for syscall in refused {
        filters.push(Filter {
            code: 0x15,
            jt: 0,
            jf: 1,
            k: syscall,
        });
        filters.push(Filter {
            code: 0x06,
            jt: 0,
            jf: 0,
            // errno 0 acknowledges a signal without delivering it. This
            // exercises the unchanged post-KILL wait on a still-live daemon.
            k: 0x00050000 | u32::from(!(syscall == KILL && kill_returns_success)),
        });
    }
    filters.push(Filter {
        code: 0x06,
        jt: 0,
        jf: 0,
        k: 0x7fff0000,
    });
    let program = Program {
        len: filters.len() as u16,
        filter: filters.as_ptr(),
    };
    // SAFETY: Linux copies the correctly laid-out filter during prctl.
    assert_eq!(
        unsafe { prctl(38, 1usize, 0usize, 0usize, 0usize) },
        0,
        "no_new_privs"
    );
    assert_eq!(
        unsafe { prctl(22, 2usize, &program as *const Program) },
        0,
        "kill fault filter"
    );
}
pub fn held(directory: &Path) {
    if is_actor() && directory.ends_with("home/proj") {
        HELD.store(true, Ordering::SeqCst);
        point("held");
    }
}
pub fn observed(value: &SdkValue, running: bool) {
    if is_actor() {
        let root = PathBuf::from(std::env::var("REOPEN_ROOT").expect("root")).join("runtime");
        if value.get_text("root") != root.to_str() {
            return;
        }
        println!("seen {}", value.get_text("container").unwrap_or("missing"));
        if !running {
            point("observed");
        }
    }
}
pub fn checked(plan: &str) {
    if is_actor() && !CHECKED.swap(true, Ordering::SeqCst) {
        point(&format!("checked {plan}"));
    }
}
pub fn kept() {
    if std::env::var("REOPEN_ACTOR").as_deref() == Ok("list") {
        checked("none");
    }
}
pub fn placed() -> Result<(), SdkRefusal> {
    if is_actor() && !TARGET_FAILED.load(Ordering::SeqCst) {
        let root = PathBuf::from(std::env::var("REOPEN_ROOT").expect("actor root"));
        if crate::fixture::directory(&root)
            .join("sandbox.record")
            .exists()
        {
            checked("none");
        }
        return Err(crate::job::refused(
            "ReplayPlacementEnd",
            "model ends at target placement",
        ));
    }
    Ok(())
}

pub fn after_published() {
    if !is_actor()
        || std::env::var("REOPEN_FINISH_OUTCOME").as_deref() != Ok("after_record")
        || std::env::var("REOPEN_STOPPED").as_deref() != Ok("false")
    {
        return;
    }
    deny_syscalls(false, false, false, false);
    if std::env::var("REOPEN_SAVED").as_deref() == Ok("false") {
        crate::fault_files::refuse(
            crate::fixture::directory(Path::new(&std::env::var("REOPEN_ROOT").expect("root")))
                .join("sandbox.record"),
        );
    }
}
