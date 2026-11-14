mod driver;
mod witnesses;
use anyhow::{Result, ensure};
use driver::Replay;
use quint_connect::runner::{self, RunConfig};
fn main() -> Result<()> {
    let seed = std::env::var("REOPEN_SEED").unwrap_or_else(|_| "42".into());
    let max_samples = std::env::var("REOPEN_SAMPLES")
        .unwrap_or_else(|_| "8".into())
        .parse()?;
    let mut driver = Replay::new()?;
    if let Ok(property) = std::env::var("REOPEN_WITNESS") {
        return witnesses::witness(&mut driver, &property);
    }
    if let Ok(kind) = std::env::var("REOPEN_BUSY") {
        driver.request("init 0")?;
        let fields = driver.request(&format!("busy {kind}"))?;
        ensure!(fields[6] == "busy-ok", "lock timeout outcome");
        return Ok(());
    }
    if std::env::var_os("REOPEN_FIXTURE").is_some() {
        driver.request("init 1")?;
        let fields = driver.request("probe boot")?;
        ensure!(fields[6] == "probe-ok", "failure probe outcome");
        ensure!(
            fields[1] == "1" && fields[2] == "0",
            "failed reopen must keep saved folder and serve no Workspace"
        );
        ensure!(
            fields[4].split(',').filter(|s| !s.is_empty()).count()
                == if matches!(
                    std::env::var("REOPEN_FIXTURE")?.as_str(),
                    "handshake-failure" | "rootless"
                ) {
                    0
                } else {
                    1
                },
            "failed restart daemon custody"
        );
        ensure!(
            fields[8].contains("mounted=false"),
            "failed Workspace outcome"
        );
        return Ok(());
    }
    if std::env::var_os("REOPEN_LIVENESS").is_some() {
        driver.request("init 1")?;
        let fields = driver.request("liveness")?;
        ensure!(fields[6] == "liveness-ok", "liveness result");
        return Ok(());
    }
    runner::run_test(
        driver,
        runner::Config {
            test_name: "sandbox-reopen production shell".into(),
            gen_config: RunConfig {
                spec: if std::env::var("REOPEN_CONTROL").as_deref() == Ok("no_revive_check") {
                    "quint/sandbox-reopen/replay-control.qnt"
                } else {
                    "quint/sandbox-reopen/replay.qnt"
                }
                .into(),
                main: Some(
                    if std::env::var("REOPEN_CONTROL").as_deref() == Ok("no_revive_check") {
                        "replay_control"
                    } else {
                        "replay"
                    }
                    .into(),
                ),
                init: Some("M::init".into()),
                step: Some("stepOrDone".into()),
                max_samples: Some(max_samples),
                max_steps: Some(28),
                seed,
            },
        },
    )
}
