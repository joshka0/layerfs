//! F41: `operator.sandbox.list` while another call holds the project's guard
//! (an `operator.sandbox.reopen` holds it across every Sandbox it revives)
//! lists a Sandbox it found not running as present: it may be restarting.
//! It neither fails nor leaves the Sandbox out. Included by workspace.rs,
//! Linux only.
use super::list;
use layerfs_bridge::control::SdkValue;
use std::{
    os::unix::fs::DirBuilderExt,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

#[test]
fn a_sandbox_found_ended_while_the_project_guard_stays_held_is_listed() {
    let base = std::env::temp_dir().join(format!("layerfs-list-{}", std::process::id()));
    let private = |path: &Path| {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(path)
            .unwrap()
    };
    let home = base.join("home");
    let project = home.join("proj");
    let sandbox = project.join("sandboxes").join("1");
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
    ]);
    // the long reopen: holds the guard until the list has answered
    let (held, wait_held) = mpsc::channel();
    let (answered, wait_answered) = mpsc::channel::<()>();
    let reopen = std::thread::spawn(move || {
        let _guard = crate::discovery::guard(&project).unwrap();
        held.send(()).unwrap();
        let _ = wait_answered.recv_timeout(Duration::from_secs(100));
    });
    wait_held.recv().unwrap();
    let started = Instant::now();
    let outcome = list(&args);
    answered.send(()).unwrap();
    reopen.join().unwrap();
    std::fs::remove_dir_all(&base).unwrap();
    let value = outcome.unwrap_or_else(|refusal| {
        panic!("the list failed after {:?}: {refusal:?}", started.elapsed())
    });
    let Some(SdkValue::List(items)) = value.get("items") else {
        panic!("no items: {value:?}");
    };
    assert_eq!(items.len(), 1, "{value:?}");
    assert_eq!(items[0].get_text("id"), Some("1"), "{value:?}");
}
