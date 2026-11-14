//! The bounded wait of a daemon stop (#373 F46). A process can survive
//! SIGKILL for a while (uninterruptible sleep), so no wait for its end may be
//! unbounded. The decision takes the running probe, the clock and the pause as
//! inputs, so a test can drive the bound without a real process.

use std::time::{Duration, Instant};

/// How long a stop waits for the daemon to end after SIGKILL: the bound of
/// the SDK's discovery guard wait.
pub const KILL_WAIT: Duration = Duration::from_secs(60);

/// The deadline passed while the process still ran.
#[derive(Debug, PartialEq, Eq)]
pub struct StillRunning;

/// Waits until `running` reports false. It probes first, ends `Ok` as soon
/// as a probe reports the process gone, and otherwise ends `StillRunning`
/// once `now` reaches `deadline`; between probes it calls `pause`.
pub fn wait_gone(
    mut running: impl FnMut() -> bool,
    mut now: impl FnMut() -> Instant,
    mut pause: impl FnMut(),
    deadline: Instant,
) -> Result<(), StillRunning> {
    loop {
        if !running() {
            return Ok(());
        }
        if now() >= deadline {
            return Err(StillRunning);
        }
        pause();
    }
}
