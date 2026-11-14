//! Names with bytes Linux allows (core/docs/issues/backslash-name). On Linux
//! a missing name stats ENOENT and mkdir creates it, for any bytes but `/`
//! and NUL. A name with a backslash behaves so
//! (docs/adr/0006-backslash-in-names.md). A name that is not UTF-8 stays
//! outside the canonical name grammar: stat and mkdir refuse it EINVAL
//! before any engine job. The test also records the mount's count of frames
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
fn a_backslash_name_behaves_as_on_linux_and_a_non_utf8_name_is_refused() {
    let f = fixture::Fixture::labeled(fixture::Shape::Mixed, "-names");
    let branch = BranchId::from_slice(&f.manifest.branch).unwrap();
    let h = Harness::new(f.opened.store.clone(), &f.directory, branch);
    let token = h.bind(2);
    let ready = h.attach(token);
    let root = Path::new(&ready.directory).to_owned();
    // Two names the grammar accepted before (the control), then the backslash.
    let accepted: [&[u8]; 3] = [b"q\"b", "e\u{e9}".as_bytes(), b"a\\b"];
    let refused: &[u8] = b"a\xff";
    let mut report = Vec::new();
    for name in accepted.iter().chain([&refused]) {
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
    let (last, linux) = report.split_last().unwrap();
    for (name, stat, made, _, _) in linux {
        assert_eq!(*stat, Err(Errno::ENOENT), "stat of missing {name:?}");
        assert_eq!(*made, Ok(()), "mkdir of {name:?}");
    }
    let (name, stat, made, frames, grammar) = last;
    assert_eq!(*stat, Err(Errno::EINVAL), "stat of {name:?}");
    assert_eq!(*made, Err(Errno::EINVAL), "mkdir of {name:?}");
    assert_eq!(*frames, 2, "{name:?} refused before any engine job");
    assert_eq!(grammar.as_ref().unwrap_err(), "InvalidUtf8", "{name:?}");
}
