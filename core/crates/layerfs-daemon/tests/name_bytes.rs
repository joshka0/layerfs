//! Names Linux allows that the LayerFS name grammar refuses
//! (core/docs/issues/backslash-name). On Linux a missing name stats ENOENT
//! and mkdir creates it, for any bytes but `/` and NUL. This test states
//! that behavior for a name with a backslash and a name that is not UTF-8,
//! so it fails while `validate_name` (layerfs-content path.rs) refuses them.
//! It also records where the answer came from: the mount's count of frames
//! refused before any engine job, and the grammar's verdict on each name.
#![cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "support/complete_bytes.rs"]
mod bytes;
#[allow(dead_code)]
#[path = "support/complete_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/mounted.rs"]
mod mounted;
#[allow(dead_code)]
#[path = "support/host_install.rs"]
mod support;
use layerfs_content::filesystem::PathName;
use layerfs_history::BranchId;
use mounted::Harness;
use nix::errno::Errno;
use std::{ffi::OsString, fs, os::unix::ffi::OsStringExt, path::Path};

fn errno(error: &std::io::Error) -> Errno {
    Errno::from_raw(error.raw_os_error().unwrap())
}

/// stat, then mkdir, of `name` in `root`: each answer, and how many frames
/// the mount refused before any engine job meanwhile.
fn probe(
    h: &Harness,
    token: layerfs_bridge::control::WorkspaceToken,
    root: &Path,
    name: &[u8],
) -> (Result<(), Errno>, Result<(), Errno>, u64) {
    let refused = || h.status(token).kernel.unwrap().work.unwrap().refused;
    let before = refused();
    let path = root.join(OsString::from_vec(name.to_vec()));
    let stat = fs::symlink_metadata(&path).map(drop).map_err(|e| errno(&e));
    let made = fs::create_dir(&path).map_err(|e| errno(&e));
    (stat, made, refused() - before)
}

#[test]
fn a_backslash_name_and_a_non_utf8_name_behave_as_on_linux() {
    let f = fixture::Fixture::labeled(fixture::Shape::Mixed, "-names");
    let branch = BranchId::from_slice(&f.manifest.branch).unwrap();
    let h = Harness::new(f.opened.store.clone(), &f.directory, branch);
    let token = h.bind(2);
    let ready = h.attach(token);
    let root = Path::new(&ready.directory).to_owned();
    // Bytes the grammar accepts, beside the backslash: the control.
    let control: [&[u8]; 2] = [b"q\"b", "e\u{e9}".as_bytes()];
    let probed: [&[u8]; 2] = [b"a\\b", b"a\xff"];
    let mut report = Vec::new();
    for name in control.iter().chain(probed.iter()) {
        let (stat, made, refused) = probe(&h, token, &root, name);
        let grammar = PathName::from_bytes(name)
            .map(drop)
            .map_err(|e| format!("{e:?}"));
        report.push((
            String::from_utf8_lossy(name).into_owned(),
            stat,
            made,
            refused,
            grammar,
        ));
    }
    for row in &report {
        println!("NAME {row:?}");
    }
    // Discard what the control made; the mount drains before any assertion.
    let discarded = h.try_unmount(token);
    println!("UNMOUNT {:?}", discarded.as_ref().map(|done| &done.reply));
    for (name, stat, made, _, _) in &report {
        assert_eq!(*stat, Err(Errno::ENOENT), "stat of missing {name:?}");
        assert_eq!(*made, Ok(()), "mkdir of {name:?}");
    }
}
