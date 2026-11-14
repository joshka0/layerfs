//! F41: calls that find a Sandbox's saved daemon ended while another call
//! holds the project's guard (an `operator.sandbox.reopen` holds it across
//! every Sandbox it revives, so it may be restarting this one). A listing
//! lists the Sandbox. A Workspace call waits for the guard, at most its
//! bound, then refuses `Busy`: the Sandbox is restarting. Neither fails nor
//! answers `NoSuchSandbox`. Included by workspace.rs, Linux only.
use super::{list, status};
use layerfs_bridge::control::SdkValue;
use std::{
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

/// A home with process Sandbox 1 of project `proj`, whose saved daemon
/// (pid 0) has ended, and the call arguments naming it.
fn ended(tag: &str) -> (PathBuf, SdkValue) {
    let base = std::env::temp_dir().join(format!("layerfs-{tag}-{}", std::process::id()));
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
    private(&base.join("sandbox-root"));
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
            (
                "root",
                SdkValue::text(base.join("sandbox-root").to_str().unwrap()),
            ),
            ("pid", SdkValue::Int(0)),
            ("start", SdkValue::text("0")),
        ]),
    )
    .unwrap();
    let args = SdkValue::map([
        ("project", SdkValue::text("proj")),
        ("home", SdkValue::text(home.to_str().unwrap())),
        ("sandbox", SdkValue::text("1")),
        (
            "path",
            SdkValue::text(base.join("sandbox-root/workspaces/3").to_str().unwrap()),
        ),
    ]);
    (base, args)
}

/// `call` on Sandbox 1 of `ended(tag)` while another thread, the long
/// reopen, holds the project's guard until `call` answers.
fn while_held(tag: &str, call: fn(&SdkValue) -> crate::job::Outcome) -> crate::job::Outcome {
    let (base, args) = ended(tag);
    let project = base.join("home").join("proj");
    let (held, wait_held) = mpsc::channel();
    let (answered, wait_answered) = mpsc::channel::<()>();
    let reopen = std::thread::spawn(move || {
        let _guard = crate::discovery::guard(&project).unwrap();
        held.send(()).unwrap();
        let _ = wait_answered.recv_timeout(Duration::from_secs(100));
    });
    wait_held.recv().unwrap();
    let outcome = call(&args);
    answered.send(()).unwrap();
    reopen.join().unwrap();
    std::fs::remove_dir_all(&base).unwrap();
    outcome
}

#[test]
fn a_sandbox_found_ended_while_the_project_guard_stays_held_is_listed() {
    let started = Instant::now();
    let outcome = while_held("list", list);
    let value = outcome.unwrap_or_else(|refusal| {
        panic!("the list failed after {:?}: {refusal:?}", started.elapsed())
    });
    let Some(SdkValue::List(items)) = value.get("items") else {
        panic!("no items: {value:?}");
    };
    assert_eq!(items.len(), 1, "{value:?}");
    assert_eq!(items[0].get_text("id"), Some("1"), "{value:?}");
}

#[test]
fn a_workspace_call_that_waits_out_the_guard_refuses_busy_restarting() {
    let started = Instant::now();
    let refusal = match while_held("status", status) {
        Ok(value) => panic!("status answered while the Sandbox may restart: {value:?}"),
        Err(refusal) => refusal,
    };
    assert_eq!(
        refusal.code,
        "Busy",
        "after {:?}: {refusal:?}",
        started.elapsed()
    );
    assert!(
        refusal.message.contains("Sandbox 1 is restarting"),
        "{refusal:?}"
    );
}
