//! Control request `Reopen` against a real installed Store and one real owner
//! (core/docs/issues/373/REOPEN.md, F12-F16). Written by the independent test
//! author from the failure list and the public control API.
//!
//! A saved overlay file is a byte copy of the folder's template, under a
//! number no live Workspace holds: the file a mount makes before any write.
//! Every refusal is followed by a positive control on the same file, so a
//! refusal cannot pass because another guard refused.
#[allow(dead_code)]
#[path = "support/control_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/host_install.rs"]
mod support;
use layerfs_api::internal::Control;
use layerfs_daemon::control::Service;
use layerfs_history::{BranchId, HistoryName, WorkspaceId};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn identity(tag: u8) -> WorkspaceId {
    WorkspaceId::from_authority([tag; 32]).unwrap()
}

// Serves every call until the client closes the channel.
fn serve(service: Arc<Service>) -> (Control, support::Worker<usize>) {
    let (channel, worker) = support::pair(move |connection| {
        let mut count = 0;
        while let Ok(served) = service.serve_one(connection, |_, _, _| unreachable!("no commit here")) {
            count += 1;
            drop(served);
        }
        count
    });
    (Control::new(channel), worker)
}

// The folder that holds the template `overlay.sqlite` and the numbered files.
fn overlay_dir(f: &fixture::Fixture) -> PathBuf {
    let dir = f.host.directory.clone();
    assert!(dir.join("overlay.sqlite").exists(), "template not in {dir:?}");
    dir
}

fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

#[test]
fn reopen_refuses_wrong_branch_root_and_held_files_and_leaves_the_file() {
    let f = fixture::Fixture::new("reopen-control");
    let dir = overlay_dir(&f);
    let branch = f.host.project.branch.branch.id;
    let stack = f.host.project.branch.branch.stack;
    let (mut client, worker) = serve(f.service.clone());

    // One live Workspace: its number is held.
    let live = layerfs_api::internal::workspace(&mut client)
        .bind(identity(1), branch)
        .unwrap();
    let held = live.token.namespace;
    let root = live.binding.effective_root.to_bytes();

    // A saved file under a number above every file in the folder.
    let saved_ns = 40;
    let saved = dir.join(format!("{saved_ns}.sqlite"));
    std::fs::copy(dir.join("overlay.sqlite"), &saved).unwrap();
    let before = bytes(&saved);
    let saved_ws = identity(40);

    let mut refused = |label: &str, workspace: WorkspaceId, branch: BranchId, namespace: i64, root: [u8; 32]| {
        let failure = layerfs_api::internal::workspace(&mut client)
            .reopen(workspace, branch, namespace, root)
            .expect_err(label);
        assert!(failure.bound.is_none(), "{label}: bound after a refusal: {failure:?}");
        println!("REOPEN_REFUSED {label}: {:?}", failure.failure.cause);
        assert_eq!(bytes(&saved), before, "{label}: the saved file changed (F16)");
    };

    // F15: a Branch the Store does not have.
    let missing = BranchId::named(stack, &HistoryName::new("no-such-branch").unwrap());
    refused("F15 missing branch", saved_ws, missing, saved_ns, root);
    // F15: the zero root.
    refused("F15 zero root", saved_ws, branch, saved_ns, [0; 32]);
    // F14: a root that is not the Branch's current effective root.
    let mut moved = root;
    moved[0] ^= 0xff;
    refused("F14 root differs", saved_ws, branch, saved_ns, moved);
    // F12: the number a live Workspace holds, with a correct Branch and root.
    refused("F12 held number", identity(41), branch, held, root);
    // F1: a number with no file creates none.
    refused("F1 missing file", identity(42), branch, 41, root);
    assert!(!dir.join("41.sqlite").exists(), "F1: reopen created 41.sqlite");

    // Positive control: the same file, Branch and root bind. Attach may fail
    // where the test has no FUSE; the binding is what Reopen answers.
    let first = layerfs_api::internal::workspace(&mut client).reopen(saved_ws, branch, saved_ns, root);
    let bound = match &first {
        Ok(mounted) => &mounted.bound,
        Err(failure) => failure
            .bound
            .as_ref()
            .unwrap_or_else(|| panic!("positive control refused: {failure:?}")),
    };
    assert_eq!(bound.token.namespace, saved_ns, "reopened under another number");
    assert_eq!(bound.token.workspace, saved_ws);

    // F13: the same Workspace again.
    let again = layerfs_api::internal::workspace(&mut client)
        .reopen(saved_ws, branch, saved_ns, root)
        .expect_err("F13 second reopen");
    assert!(again.bound.is_none(), "F13: bound twice: {again:?}");

    // F12: the live Workspace is untouched by the refused Reopen of its number.
    layerfs_api::internal::workspace(&mut client)
        .status(live.token)
        .expect("F12: the live Workspace no longer answers status");
    // F11: a new mount takes neither held number.
    let next = layerfs_api::internal::workspace(&mut client)
        .bind(identity(2), branch)
        .unwrap();
    assert!(![held, saved_ns].contains(&next.token.namespace), "F11: new mount took {}", next.token.namespace);

    drop(client);
    worker.join();
    drop(first);
    f.cleanup();
}
