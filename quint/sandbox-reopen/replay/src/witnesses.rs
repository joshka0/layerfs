use crate::driver::Replay;
use anyhow::{Context, Result, bail, ensure};
fn call_target(driver: &mut Replay, caller: &str, ok: bool) -> Result<Vec<String>> {
    call_target_outcome(driver, caller, if ok { "ok" } else { "stopped" })
}
fn call_target_outcome(driver: &mut Replay, caller: &str, outcome: &str) -> Result<Vec<String>> {
    let acquired = driver.request(&format!("acquire {caller}"))?;
    ensure!(acquired[6] == "held", "target acquisition");
    let checked = driver.request(&format!("check {caller}"))?;
    if checked[6] == "done" {
        return Ok(checked);
    }
    ensure!(checked[6].starts_with("checked "), "target check");
    let effect = driver.request(&format!("effect {caller}"))?;
    if effect[6].starts_with("spawned ") {
        driver.request(&format!("finish {caller} {outcome}"))
    } else {
        ensure!(effect[6] == "done", "target effect");
        Ok(effect)
    }
}
pub(crate) fn witness(driver: &mut Replay, property: &str) -> Result<()> {
    driver.request(if property == "boot" {
        "init 0 0"
    } else if property == "unattached" {
        "init 2 1"
    } else {
        "init 1 1"
    })?;
    let result = match property {
        "overlay" => {
            driver.request("observe list")?;
            let fields = call_target(driver, "list", true)?;
            ensure!(
                fields[1] == "1",
                "NoOverlayLoss violated: saved overlay folder was deleted"
            );
            fields
        }
        "boot" => {
            let fields = call_target(driver, "boot", true)?;
            ensure!(fields[1] == "1", "BootNeverDeletes violated");
            fields
        }
        "guard" => {
            let fields = driver.request("acquire boot")?;
            ensure!(fields[5] == "1", "project flock is absent at acquisition");
            fields
        }
        "release" => {
            let fields = call_target(driver, "boot", true)?;
            ensure!(
                fields[5] == "0",
                "GuardReleased violated after target completed"
            );
            fields
        }
        "running" => {
            call_target(driver, "mount", true)?;
            let fields = call_target(driver, "boot", true)?;
            ensure!(
                fields[4].split(',').filter(|id| !id.is_empty()).count() <= 1,
                "OneDaemon violated after restarting a running target: {}",
                fields[4]
            );
            fields
        }
        "report" | "fallback" => {
            let fields = call_target(driver, "mount", false)?;
            ensure!(fields[9] == "failed", "target revive did not fail");
            if property == "report" {
                ensure!(fields[10] == "true", "RevivedReported violated");
            } else {
                ensure!(fields[8] == "ok", "MountFallsBack violated: {}", fields[8]);
            }
            fields
        }
        "unattached" => {
            let fields = call_target(driver, "boot", true)?;
            ensure!(
                fields[2] == "1",
                "ReopenedWhenDone: only one of two saved overlays can reattach"
            );
            fields
        }
        "status" => {
            driver.request("acquire boot")?;
            driver.request("check boot")?;
            let spawned = driver.request("effect boot")?;
            ensure!(
                spawned[6].starts_with("spawned "),
                "restart must be underway"
            );
            let observed = driver.request("observe status")?;
            ensure!(
                observed[6] == "observed",
                "StatusWaitsOutRestart violated: {}",
                observed[8]
            );
            driver.request("finish boot ok")?;
            let held = driver.request("acquire status")?;
            ensure!(held[6] == "held", "status guard after restart");
            let fields = driver.request("check status")?;
            ensure!(
                fields[6] == "done" && fields[8] == "ok",
                "status did not settle after restart"
            );
            fields
        }
        "kill-deadline" => {
            let started = std::time::Instant::now();
            let fields = call_target_outcome(driver, "boot", "kill_delayed")?;
            let elapsed = started.elapsed();
            ensure!(
                elapsed >= std::time::Duration::from_secs(60)
                    && elapsed < std::time::Duration::from_secs(75),
                "unchanged post-KILL deadline: {elapsed:?}"
            );
            ensure!(
                fields[9] == "failed" && fields[5] == "0",
                "bounded stop must settle and release guard"
            );
            ensure!(
                !fields[4].is_empty(),
                "swallowed signals must leave the daemon live"
            );
            eprintln!(
                "LIVENESS post-KILL real-deadline=60s stop-returned=true holder-named={} elapsed={elapsed:?}",
                fields[11].contains(
                    fields[4]
                        .split('@')
                        .next()
                        .context("holder")?
                        .trim_start_matches("pid:")
                )
            );
            fields
        }
        "folder-lock" => {
            let first = call_target_outcome(driver, "boot", "recovery_save_failed")?;
            let holder = first[4].clone();
            ensure!(
                !holder.is_empty() && holder != first[3],
                "double-failure fixture must leave the first daemon unrecorded"
            );
            let fields = call_target_outcome(driver, "mount", "ok")?;
            ensure!(
                fields[4] == holder,
                "OneDaemon violated after second revive: holder={holder}, live={}",
                fields[4]
            );
            ensure!(fields[9] == "failed", "second target revive must refuse");
            let pid = holder
                .split('@')
                .next()
                .context("holder pid")?
                .strip_prefix("pid:")
                .context("holder identity")?;
            ensure!(
                fields[11].contains(pid),
                "refusal must name holder pid {pid}: {}",
                fields[11]
            );
            fields
        }
        "fallback-failure" => {
            let fields = call_target_outcome(driver, "mount", "fallback_failed")?;
            ensure!(
                fields[9] == "failed" && fields[5] == "0",
                "target must fail and release its guard"
            );
            ensure!(
                fields[8] == "ok",
                "MountFallsBack has an unmodeled replacement failure: {}",
                fields[8]
            );
            fields
        }
        "listener" | "after-record" | "stop-failed" | "recovery-save" => {
            let outcome = match property {
                "listener" => "listener",
                "after-record" => "after_record",
                "stop-failed" => "stop_failed",
                _ => "recovery_save_failed",
            };
            let fields = call_target_outcome(driver, "boot", outcome)?;
            ensure!(
                fields[9] == "failed" && fields[5] == "0",
                "failure must settle and release guard"
            );
            if property == "listener" || property == "after-record" {
                ensure!(
                    fields[4].is_empty(),
                    "NoOrphanDaemon: failed restart must stop its daemon"
                );
            } else {
                ensure!(
                    fields[4]
                        .split(',')
                        .filter(|id| !id.is_empty())
                        .all(|id| id == fields[3]),
                    "NoOrphanDaemon violated: live={} recorded={}",
                    fields[4],
                    fields[3]
                );
            }
            fields
        }
        "orphan" | "failed-duplicate" => {
            let first = call_target(driver, "boot", false)?;
            if property == "orphan" {
                ensure!(
                    first[4]
                        .split(',')
                        .filter(|id| !id.is_empty())
                        .all(|id| id == first[3]),
                    "NoOrphanDaemon violated after failed handshake: live={} recorded={}",
                    first[4],
                    first[3]
                );
                first
            } else {
                let fields = call_target(driver, "mount", true)?;
                ensure!(
                    fields[4].split(',').filter(|id| !id.is_empty()).count() <= 1,
                    "OneDaemon violated after failed restart: {}",
                    fields[4]
                );
                fields
            }
        }
        _ => bail!("unknown physical witness {property}"),
    };
    eprintln!(
        "PHYSICAL_PROPERTY {property}: PASS event={} live={}",
        result[6], result[4]
    );
    Ok(())
}
