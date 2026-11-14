//! Test-only process worker. Production functions are copied without semantic
//! changes; instrument.py records every checkpoint insertion as a byte diff.
#![allow(dead_code)]
mod actor;
mod admission;
#[path = "../generated/src/convert.rs"]
mod convert;
#[path = "../generated/src/direct.rs"]
mod direct;
#[path = "../generated/src/discovery.rs"]
mod discovery;
#[path = "../generated/src/exec.rs"]
mod exec;
mod fault_files;
mod fixture;
mod hooks;
#[path = "../generated/src/job.rs"]
mod job;
mod observe;
#[path = "../generated/src/output.rs"]
mod output;
#[path = "../generated/src/pool.rs"]
mod pool;
#[path = "../generated/src/project.rs"]
mod project;
#[path = "../generated/src/registry.rs"]
mod registry;
mod replay;
#[path = "../generated/src/sandbox.rs"]
mod sandbox;
#[path = "../generated/src/trace.rs"]
mod trace;
#[path = "../generated/src/workspace.rs"]
mod workspace;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("actor") {
        actor::actor(&args[2], &args[3]);
    } else if args.get(1).map(String::as_str) == Some("identity") {
        actor::unprivileged_identity(&args[2]);
    } else {
        let own = std::env::current_exe().expect("worker path");
        let daemon = std::env::var("REOPEN_DAEMON").expect("daemon path");
        let output = std::process::Command::new("sha256sum")
            .arg(own)
            .arg(daemon)
            .output()
            .expect("artifact hashes");
        assert!(output.status.success(), "artifact hash command");
        eprint!(
            "BUILT_ARTIFACTS {}",
            String::from_utf8_lossy(&output.stdout)
        );
        replay::serve();
    }
}
