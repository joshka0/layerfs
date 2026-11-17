//! lost-mount L9, L11, L13 and the malformed-input part of L14.
//! Run after --no-run: timeout 120s cargo +1.85.1 test --manifest-path
//! core/Cargo.toml --locked -p layerfs-fuse --test namespace_copies
use layerfs_fuse::mount::copies::{self, Peers};

const DEVICE: (u32, u32) = (0, 82);

fn own() -> Peers {
    Peers {
        shared: Some(306),
        master: None,
    }
}

#[test]
fn l9_private_copy_pins() {
    let line = b"781 29 0:82 / /var/lib/layerfs/workspaces/1 rw,nosuid,nodev,relatime - fuse.layerfs layerfs rw,user_id=0,group_id=0";
    let (device, peers) = copies::row(line).expect("valid private mountinfo row");
    assert_eq!(device, DEVICE);
    assert_eq!(peers.shared, None);
    assert_eq!(peers.master, None);
    assert!(copies::pins(own(), DEVICE, line));
}

#[test]
fn l11_propagating_copies_do_not_pin() {
    for line in [
        &b"782 29 0:82 / /var/lib/layerfs/workspaces/1 rw,relatime master:306 - fuse.layerfs layerfs rw"[..],
        &b"783 29 0:82 / /var/lib/layerfs/workspaces/1 rw,relatime shared:306 - fuse.layerfs layerfs rw"[..],
        &b"784 29 0:82 / /var/lib/layerfs/workspaces/1 rw,relatime shared:412 master:306 - fuse.layerfs layerfs rw"[..],
    ] {
        assert!(!copies::pins(own(), DEVICE, line), "{line:?}");
    }
    let peers = copies::peers(
        [
            &b"shared:412"[..],
            &b"master:306"[..],
            &b"propagate_from:99"[..],
        ]
        .into_iter(),
    );
    assert_eq!(peers.shared, Some(412));
    assert_eq!(peers.master, Some(306));
}

#[test]
fn l11_unrelated_propagation_groups_still_pin() {
    let line = b"785 29 0:82 / /var/lib/layerfs/workspaces/1 rw,relatime shared:412 master:307 - fuse.layerfs layerfs rw";
    assert!(copies::pins(own(), DEVICE, line));
}

#[test]
fn l13_identity_is_device_not_path() {
    for line in [
        &b"786 29 0:83 / /var/lib/layerfs/workspaces/1 rw,relatime - fuse.layerfs layerfs rw"[..],
        &b"787 29 1:82 / /var/lib/layerfs/workspaces/1 rw,relatime - fuse.layerfs layerfs rw"[..],
    ] {
        assert!(!copies::pins(own(), DEVICE, line), "{line:?}");
    }
    let moved = b"788 29 0:82 / /some/other/path rw,relatime - fuse.layerfs layerfs rw";
    assert!(copies::pins(own(), DEVICE, moved));
    let bind = b"789 29 0:82 /subdirectory /bind\\040copy rw,relatime - fuse.layerfs layerfs rw";
    assert!(copies::pins(own(), DEVICE, bind));
}

#[test]
fn no_own_shared_group_reaches_no_copy() {
    for line in [
        &b"790 29 0:82 / /copy rw,relatime - fuse.layerfs layerfs rw"[..],
        &b"791 29 0:82 / /copy rw,relatime shared:306 - fuse.layerfs layerfs rw"[..],
        &b"792 29 0:82 / /copy rw,relatime master:306 - fuse.layerfs layerfs rw"[..],
    ] {
        assert!(
            copies::pins(
                Peers {
                    shared: None,
                    master: Some(306)
                },
                DEVICE,
                line
            ),
            "{line:?}"
        );
    }
}

#[test]
fn l14_malformed_rows_are_skipped_without_panicking() {
    // These have no complete mountinfo record or no valid device identity.
    for line in [
        &b""[..],
        &b"781 29"[..],
        &b"781 29 0:82"[..],
        &b"781 29 0:82 / /copy rw"[..],
        &b"781 29 bad / /copy rw - fuse.layerfs layerfs rw"[..],
        &b"781 29 0:x / /copy rw - fuse.layerfs layerfs rw"[..],
        &b"781 29 :82 / /copy rw - fuse.layerfs layerfs rw"[..],
        &b"781 29 0: / /copy rw - fuse.layerfs layerfs rw"[..],
        &b"781 29 4294967296:82 / /copy rw - fuse.layerfs layerfs rw"[..],
        &b"781 29 0:4294967296 / /copy rw - fuse.layerfs layerfs rw"[..],
        &b"781 29 \xff:82 / /copy rw - fuse.layerfs layerfs rw"[..],
    ] {
        assert!(
            copies::row(line).is_none(),
            "malformed row accepted: {line:?}"
        );
        assert!(
            !copies::pins(own(), DEVICE, line),
            "malformed row pins: {line:?}"
        );
    }
    let peers = copies::peers(
        [
            &b"shared:bad"[..],
            &b"master:18446744073709551616"[..],
            &b"unbindable"[..],
        ]
        .into_iter(),
    );
    assert_eq!(peers.shared, None);
    assert_eq!(peers.master, None);
}
