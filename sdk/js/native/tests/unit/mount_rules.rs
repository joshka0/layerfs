//! L1/L2/L10 contract cases; included by discovery/mount_rules.rs.
use super::lost;
use crate::discovery::kernel_table::devices_at;
use layerfs_bridge::control::MountPhase;

const DEVICE: (u32, u32) = (0, 71);
const OTHER: (u32, u32) = (0, 72);

#[test]
fn serving_phases_with_the_connection_at_the_path_are_mounted() {
    for phase in [MountPhase::Ready, MountPhase::Probing] {
        for devices in [&[DEVICE][..], &[OTHER, DEVICE][..]] {
            assert_eq!(lost(phase, false, DEVICE, devices), None, "{phase:?}");
        }
    }
}

#[test]
fn serving_phases_without_the_connection_name_the_kernel() {
    for phase in [MountPhase::Ready, MountPhase::Probing] {
        for devices in [&[][..], &[OTHER][..]] {
            let reason = lost(phase, false, DEVICE, devices)
                .unwrap_or_else(|| panic!("{phase:?} without its kernel mount is lost"));
            assert!(reason.to_lowercase().contains("kernel"), "{reason}");
        }
    }
}

#[test]
fn nonserving_phases_name_the_phase_even_with_the_device_present() {
    for (phase, name) in [
        (MountPhase::Unattached, "unattached"),
        (MountPhase::Attaching, "attaching"),
        (MountPhase::Draining, "draining"),
        (MountPhase::Retained, "retained"),
        (MountPhase::Stopping, "stopping"),
    ] {
        for devices in [&[DEVICE][..], &[][..]] {
            let reason = lost(phase, false, DEVICE, devices)
                .unwrap_or_else(|| panic!("{phase:?} is not mounted"));
            assert!(reason.to_lowercase().contains(name), "{phase:?}: {reason}");
        }
    }
}

#[test]
fn detached_is_lost_even_when_the_kernel_still_lists_the_device() {
    for phase in [MountPhase::Ready, MountPhase::Probing] {
        assert!(lost(phase, true, DEVICE, &[DEVICE]).is_some(), "{phase:?}");
    }
}

#[test]
fn devices_at_selects_all_devices_at_the_exact_path_with_a_space() {
    // The mountinfo spelling is /workspace\040with\040space. rows() exposes
    // decoded paths; devices_at must match that path exactly, not a prefix.
    let rows = vec![
        ("/workspace with space".to_owned(), DEVICE),
        ("/workspace with space/child".to_owned(), (0, 73)),
        ("/workspace with space".to_owned(), OTHER),
        (r"/workspace\040with\040space".to_owned(), (0, 74)),
    ];
    assert_eq!(
        devices_at(&rows, "/workspace with space"),
        vec![DEVICE, OTHER]
    );
    assert!(devices_at(&rows, "/workspace").is_empty());
}
