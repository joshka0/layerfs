pub use crate::hooks::{
    after_published, checked, held, is_target, kept, observed, placed, processes, status_observed,
    target_failed, target_ok, transport_fault,
};
use crate::{
    actor::Actor,
    observe::{clean, snapshot},
};
use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    path::PathBuf,
    time::Duration,
};
pub fn serve() {
    let mut root: Option<PathBuf> = None;
    let mut actors = BTreeMap::<String, Actor>::new();
    let mut count = 0;
    for line in std::io::stdin().lock().lines() {
        let line = line.expect("scheduler command");
        let words: Vec<_> = line.split_whitespace().collect();
        if words.first() == Some(&"init") {
            actors.clear();
            if let Some(old) = root.take() {
                clean(&old);
            }
            count += 1;
            let path =
                std::env::temp_dir().join(format!("layerfs-replay-{}-{count}", std::process::id()));
            let overlays = words[1].parse().expect("overlay count");
            let eligible = words
                .get(2)
                .map_or(overlays, |e| e.parse().expect("eligible count"));
            crate::fixture::seed(&path, overlays, eligible);
            snapshot(&path, "init", None);
            root = Some(path);
        } else if words.first() == Some(&"busy") {
            let root = root.as_ref().expect("initialized fixture");
            let kind = if words[1] == "project" {
                "project-lock"
            } else {
                "sandbox-lock"
            };
            let mut holder = Actor::start(root, kind);
            assert_eq!(holder.event, "held");
            let mut waiter = Actor::spawn(root, kind);
            assert!(!waiter.ready(100), "waiter entered held lock");
            assert!(
                waiter.ready(62000),
                "real 60-second lock deadline did not settle"
            );
            waiter.receive();
            assert_eq!(waiter.event, "busy", "lock deadline outcome");
            holder.advance();
            assert_eq!(holder.event, "done");
            let mut successor = Actor::start(root, kind);
            assert_eq!(successor.event, "held");
            successor.advance();
            assert_eq!(successor.event, "done");
            eprintln!("LIVENESS {kind} real-deadline=60s result=Busy successor=completed");
            snapshot(root, "busy-ok", None);
        } else if words.first() == Some(&"probe") {
            let root = root.as_ref().expect("initialized fixture");
            let mut actor = Actor::start(root, "boot");
            assert_eq!(actor.event, "held");
            actor.advance();
            assert_eq!(actor.event, "checked restart");
            actor.advance();
            if std::env::var("REOPEN_FIXTURE").as_deref() != Ok("rootless") {
                assert!(actor.event.starts_with("spawned "), "spawn checkpoint");
                actor.advance();
            }
            assert_eq!(actor.event, "done");
            snapshot(root, "probe-ok", Some(&actor));
        } else if words.first() == Some(&"liveness") {
            let root = root.as_ref().expect("initialized fixture");
            for kind in ["project-lock", "sandbox-lock"] {
                for crash in [false, true] {
                    let mut holder = Actor::start(root, kind);
                    assert_eq!(holder.event, "held");
                    let mut waiter = Actor::spawn(root, kind);
                    assert!(!waiter.ready(100), "{kind}: waiter entered before release");
                    if crash {
                        holder.child.kill().expect("holder crash");
                        holder.child.wait().expect("holder exit");
                    } else {
                        holder.advance();
                        assert_eq!(holder.event, "done");
                    }
                    assert!(
                        waiter.ready(2000),
                        "{kind}: waiter stayed blocked after release (crash={crash})"
                    );
                    waiter.receive();
                    assert_eq!(waiter.event, "held");
                    waiter.advance();
                    assert_eq!(waiter.event, "done");
                    eprintln!(
                        "LIVENESS {kind} crash={crash} blocked-before=true completed-after=true"
                    );
                }
            }
            snapshot(root, "liveness-ok", None);
        } else if words.first() == Some(&"crash-target") {
            let root = root.as_ref().expect("fixture");
            for identity in crate::observe::live_selected(root, false) {
                assert!(std::process::Command::new("kill")
                    .args(["-KILL", crate::admission::pid(&identity)])
                    .status()
                    .expect("target holder exit")
                    .success());
            }
            let until = std::time::Instant::now() + Duration::from_secs(2);
            while !crate::observe::live_selected(root, false).is_empty() {
                assert!(
                    std::time::Instant::now() < until,
                    "target holder stayed alive"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            snapshot(root, "target-ended", None);
        } else if words.first() == Some(&"stop") {
            break;
        } else {
            let root = root.as_ref().expect("initialized fixture");
            let action = words[0];
            let caller = words[1];
            if let Some(actor) = actors.get_mut(caller) {
                if action == "admit" {
                    crate::admission::advance(root, actor, words.get(2).copied().unwrap_or("true"));
                } else if action == "finish" {
                    writeln!(
                        actor.input,
                        "go {} {} {} {}",
                        words[2],
                        words.get(3).copied().unwrap_or("true"),
                        words.get(4).copied().unwrap_or("true"),
                        words.get(5).copied().unwrap_or("true")
                    )
                    .expect("finish choice");
                    actor.input.flush().expect("finish flush");
                    actor.receive();
                } else if action == "effect" && words.len() > 2 {
                    if actor.event == "checked restart" && words[2] == "false" {
                        crate::admission::prepare_refusal(root);
                    }
                    writeln!(actor.input, "go effect {} {}", words[2], words[3])
                        .expect("effect picks");
                    actor.input.flush().expect("effect flush");
                    actor.receive();
                } else {
                    actor.advance();
                }
            } else {
                actors.insert(caller.to_owned(), Actor::start(root, caller));
            }
            let actor = actors.get_mut(caller).expect("actor");
            // Synthetic 'none' check: Kept/Busy was checked by real reclaim;
            // for a running placement there is no mutation decision to pause.
            if action == "check" && actor.event == "done" { /* returned during check */ }
            crate::admission::spawned_ready(root, &actor.event);
            let event = actor.event.clone();
            snapshot(root, &event, Some(actor));
        }
    }
    actors.clear();
    if let Some(root) = root {
        clean(&root);
    }
    std::thread::sleep(Duration::from_millis(20));
}
