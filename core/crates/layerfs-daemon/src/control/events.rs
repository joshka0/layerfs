//! One `daemon.log` line per kernel mount event of a Workspace
//! (core/docs/issues/lost-mount/README.md L7, L8): an ISO 8601 UTC time, the
//! event, the Workspace path, id and namespace, and the stage or cause. The
//! daemon's stderr is its log file. The listen line on stdout is a protocol
//! line its spawner parses, and it keeps its exact form.
use layerfs_bridge::control::WorkspaceToken;
use std::{
    fmt::Write as _,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

/// The current time as `iso_utc` writes it, for every other log line (L8).
pub fn now() -> String {
    iso_utc(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64),
    )
}

/// Writes the event line. A failed write is ignored: logging never changes
/// an operation's outcome. Its callers attach and unmount kernel mounts,
/// which exist on Linux only.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) fn record(event: &str, path: &str, token: WorkspaceToken, detail: &str) {
    let id = token
        .workspace
        .to_bytes()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
    let _ = writeln!(
        std::io::stderr().lock(),
        "{} {event} path={path} workspace={id} namespace={} {detail}",
        now(),
        token.namespace
    );
}

/// `millis` since the Unix epoch as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn iso_utc(millis: u64) -> String {
    let seconds = millis / 1000;
    let (year, month, day) = civil(seconds / 86_400);
    let of_day = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60,
        millis % 1000
    )
}

/// The proleptic Gregorian date of a day count since 1970-01-01
/// (H. Hinnant's days-to-civil algorithm).
fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}
