//! Literal calendar checks for lost-mount L7/L8. Mounted E2E checks cannot
//! select the daemon clock's epoch, leap day or year boundary.
use layerfs_daemon::control::events::iso_utc;

#[test]
fn iso_utc_formats_epoch_leap_day_milliseconds_and_year_boundary() {
    for (millis, expected) in [
        (0, "1970-01-01T00:00:00.000Z"),
        (1_709_164_800_000, "2024-02-29T00:00:00.000Z"),
        (1_794_955_960_123, "2026-11-17T22:52:40.123Z"),
        (1_798_761_599_999, "2026-12-31T23:59:59.999Z"),
        (1_798_761_600_000, "2027-01-01T00:00:00.000Z"),
    ] {
        assert_eq!(
            iso_utc(millis),
            expected,
            "milliseconds since epoch: {millis}"
        );
    }
}
