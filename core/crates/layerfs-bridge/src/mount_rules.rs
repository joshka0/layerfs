//! Pure rules: whether a Workspace that the daemon lists is mounted in the
//! kernel (core/docs/issues/lost-mount/README.md L1, L2). No I/O. The SDK
//! reads the kernel mount table and asks these rules; they live here, beside
//! the status types they read, so that integration tests can reach them.
//!
//! decided: 2026-11-17, the operator chose that LayerFS reads the kernel
//! mount; the daemon's status stays a maintained observation with no syscall,
//! so the kernel check is here, in the SDK.
use crate::control::{Activity, MountPhase};

/// Why a Workspace that the daemon lists is not mounted, or `None` when it
/// is. `devices_at_path` holds the device (major, minor) of every row of the
/// kernel mount table at the Workspace's path. The Workspace is mounted only
/// when its connection serves (Ready, or the reversible unmount probe, which
/// keeps serving), no detach is known, and the kernel holds a mount of that
/// connection's device at the path.
pub fn lost(
    phase: MountPhase,
    detached: bool,
    device: (u32, u32),
    devices_at_path: &[(u32, u32)],
) -> Option<String> {
    if !matches!(phase, MountPhase::Ready | MountPhase::Probing) {
        return Some(format!(
            "not mounted: the daemon's teardown of this Workspace is at phase {phase:?}"
        ));
    }
    if detached {
        return Some("not mounted: the daemon knows the kernel detached it".to_owned());
    }
    if !devices_at_path.contains(&device) {
        return Some(format!(
            "kernel mount gone: no mount of device {}:{} at this path in the kernel mount table",
            device.0, device.1
        ));
    }
    None
}

/// `reason` with the Workspace's activity, where the daemon's teardown
/// stopped (`Closing` after a refused unmount), so a lost Workspace names its
/// custody stage (L2). `None` stays `None`.
pub fn with_activity(reason: Option<String>, activity: Activity) -> Option<String> {
    reason.map(|reason| format!("{reason}; activity {activity:?}"))
}

/// The devices of the rows mounted exactly at `path`.
pub fn devices_at(rows: &[(String, (u32, u32))], path: &str) -> Vec<(u32, u32)> {
    rows.iter()
        .filter(|(point, _)| point == path)
        .map(|(_, device)| *device)
        .collect()
}
