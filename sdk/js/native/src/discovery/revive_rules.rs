//! Pure rules for a process Sandbox found with its daemon ended. No I/O.
//!
//! decided: 2026-11-14, a dead process Sandbox that holds a saved Workspace
//! overlay is started again and its Workspaces reopened, never reclaimed: the
//! overlays hold uncommitted work (core/docs/issues/373/REOPEN.md).

/// What an observation of the saved daemon found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Found {
    /// The saved daemon (same pid and start time) runs.
    Running,
    /// The saved daemon no longer runs.
    Ended,
}

/// What discovery does with the Sandbox.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Next {
    /// Use the running daemon.
    Use,
    /// Start its daemon again and reopen its saved Workspaces.
    Restart,
    /// Remove it: nothing of it is worth keeping.
    Reclaim,
}

/// `saved` counts the Sandbox's Workspace records whose overlay file exists.
pub(crate) fn next(found: Found, saved: usize) -> Next {
    match (found, saved) {
        (Found::Running, _) => Next::Use,
        (Found::Ended, 0) => Next::Reclaim,
        (Found::Ended, _) => Next::Restart,
    }
}

/// The base a saved Workspace reopens on: the root its record saved at
/// mount. A record without one (written before reopen existed) does not
/// reopen.
pub(crate) fn base(saved: Option<[u8; 32]>) -> Result<[u8; 32], &'static str> {
    saved.ok_or("its record has no saved base root (written before reopen existed)")
}
