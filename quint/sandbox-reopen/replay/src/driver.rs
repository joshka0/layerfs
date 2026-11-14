//! Independent Quint replay driver. The Linux worker is built on Rust 1.85.1.
use anyhow::{Context, Result, bail, ensure};
use quint_connect::{Driver, State, Step};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observed {
    #[serde(rename = "folder")]
    folder: bool,
    #[serde(rename = "overlays")]
    overlays: u64,
    eligible: u64,
    #[serde(rename = "live")]
    live: BTreeSet<u64>,
    #[serde(rename = "recorded")]
    recorded: u64,
    #[serde(rename = "nextId")]
    next_id: u64,
    #[serde(rename = "guard")]
    guard: String,
    #[serde(rename = "phase")]
    phase: BTreeMap<String, String>,
    #[serde(rename = "seen")]
    seen: BTreeMap<String, u64>,
    #[serde(rename = "plan")]
    plan: BTreeMap<String, String>,
    spawned: BTreeMap<String, u64>,
    result: BTreeMap<String, String>,
    target: BTreeMap<String, String>,
    reported: bool,
    #[serde(rename = "goneMidRestart")]
    gone_mid_restart: bool,
    #[serde(rename = "mounted")]
    mounted: u64,
    #[serde(rename = "deleter")]
    deleter: String,
}
impl Observed {
    fn initial(o: u64, eligible: u64) -> Self {
        let callers = ["boot", "mount", "list", "status"];
        Self {
            folder: true,
            overlays: o,
            eligible,
            live: BTreeSet::new(),
            recorded: 0,
            next_id: 1,
            guard: String::new(),
            phase: callers.map(|c| (c.into(), "idle".into())).into(),
            seen: callers.map(|c| (c.into(), 0)).into(),
            plan: callers.map(|c| (c.into(), "none".into())).into(),
            spawned: callers.map(|c| (c.into(), 0)).into(),
            result: callers.map(|c| (c.into(), String::new())).into(),
            target: callers.map(|c| (c.into(), String::new())).into(),
            reported: false,
            gone_mid_restart: false,
            mounted: 0,
            deleter: String::new(),
        }
    }
}
pub(crate) struct Replay {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    observed: Observed,
    identities: BTreeMap<String, u64>,
    traces: u64,
    steps: u64,
}
impl Replay {
    pub(crate) fn new() -> Result<Self> {
        let mut child = Command::new("python3")
            .arg("quint/sandbox-reopen/container.py")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("start isolated Linux worker")?;
        Ok(Self {
            input: child.stdin.take().context("worker stdin")?,
            output: BufReader::new(child.stdout.take().context("worker stdout")?),
            child,
            observed: Observed::initial(0, 0),
            identities: BTreeMap::new(),
            traces: 0,
            steps: 0,
        })
    }
    pub(crate) fn request(&mut self, command: &str) -> Result<Vec<String>> {
        writeln!(self.input, "{command}")?;
        self.input.flush()?;
        let mut line = String::new();
        ensure!(
            self.output.read_line(&mut line)? > 0,
            "worker exited: {:?}",
            self.child.try_wait()
        );
        let fields: Vec<String> = line.trim().split('|').map(str::to_owned).collect();
        ensure!(
            fields.len() == 12 && fields[0] == "OK",
            "worker protocol: {line}"
        );
        eprintln!("WORKER {command}: {line}");
        Ok(fields)
    }
    fn id(&mut self, identity: &str) -> Result<u64> {
        ensure!(
            !identity.is_empty() && identity != "missing",
            "missing process identity"
        );
        let next = self.identities.len() as u64;
        Ok(*self.identities.entry(identity.to_owned()).or_insert(next))
    }
    fn update(&mut self, fields: &[String], caller: &str) -> Result<()> {
        let before = self.observed.folder;
        self.observed.folder = fields[1] == "1";
        self.observed.mounted = fields[2].parse()?;
        if fields[3] != "missing" {
            self.observed.recorded = self.id(&fields[3])?;
        }
        let live: Result<BTreeSet<_>> = fields[4]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|id| self.id(id))
            .collect();
        self.observed.live = live?;
        self.observed.next_id = self.identities.len() as u64;
        if fields[5] == "0" {
            self.observed.guard.clear();
        } else if self.observed.guard.is_empty() {
            self.observed.guard = caller.to_owned();
        }
        if before && !self.observed.folder {
            self.observed.deleter = caller.to_owned();
        }
        Ok(())
    }
}
impl State<Replay> for Observed {
    fn from_driver(d: &Replay) -> Result<Self> {
        Ok(d.observed.clone())
    }
    fn from_spec(value: itf::Value) -> Result<Self> {
        let itf::Value::Record(record) = value else {
            bail!("model state must be a record")
        };
        let mut short = itf::value::Record::new();
        for (key, value) in record {
            let key = key.rsplit("::").next().context("model field")?.to_owned();
            ensure!(short.insert(key, value).is_none(), "duplicate model field");
        }
        Ok(Self::deserialize(itf::Value::Record(short))?)
    }
}
impl Driver for Replay {
    type State = Observed;
    fn step(&mut self, step: &Step) -> Result<()> {
        self.steps += 1;
        if step.action_taken == "init" {
            self.traces += 1;
            let o = u64::deserialize(step.nondet_picks.get("o").context("init o")?.clone())?;
            let e = u64::deserialize(
                step.nondet_picks
                    .get("e")
                    .context("init eligibility")?
                    .clone(),
            )?;
            let fields = self.request(&format!("init {o} {e}"))?;
            self.identities.clear();
            self.observed = Observed::initial(o, e);
            self.id(&fields[3])?;
            self.update(&fields, "")?;
            return Ok(());
        }
        if step.action_taken == "stutter" {
            return Ok(());
        }
        let c = String::deserialize(step.nondet_picks.get("c").context("step c")?.clone())?;
        let action = step.action_taken.as_str();
        let command = if action == "finish" {
            let outcome = String::deserialize(
                step.nondet_picks
                    .get("outcome")
                    .context("finish outcome")?
                    .clone(),
            )?;
            format!("finish {c} {outcome}")
        } else {
            format!("{action} {c}")
        };
        let fields = self.request(&command)?;
        let event = fields[6].as_str();
        match action {
            "observe" => {
                if !fields[7].is_empty() {
                    let seen = self.id(&fields[7])?;
                    self.observed.seen.insert(c.clone(), seen);
                } else {
                    self.observed.seen.insert(c.clone(), self.observed.recorded);
                }
                ensure!(
                    event == "observed" || event == "done",
                    "observe checkpoint: {event}"
                );
                self.observed.phase.insert(
                    c.clone(),
                    if event == "done" { "done" } else { "observed" }.into(),
                );
            }
            "acquire" => {
                ensure!(event == "held", "acquire checkpoint: {event}");
                self.observed.phase.insert(c.clone(), "held".into());
            }
            "check" => {
                if event == "done" {
                    self.observed.phase.insert(c.clone(), "done".into());
                } else if let Some(plan) = event.strip_prefix("checked ") {
                    self.observed.plan.insert(c.clone(), plan.into());
                    self.observed.phase.insert(c.clone(), "checked".into());
                } else {
                    bail!("check checkpoint: {event}");
                }
            }
            "effect" => {
                if let Some(identity) = event.strip_prefix("spawned ") {
                    let id = self.id(identity)?;
                    self.observed.spawned.insert(c.clone(), id);
                    self.observed.phase.insert(c.clone(), "spawned".into());
                } else {
                    ensure!(event == "done", "effect checkpoint: {event}");
                    self.observed.phase.insert(c.clone(), "done".into());
                }
            }
            "finish" => {
                ensure!(event == "done", "finish checkpoint: {event}");
                self.observed.spawned.insert(c.clone(), 0);
                self.observed.phase.insert(c.clone(), "done".into());
            }
            _ => bail!("unbound model action {action}"),
        }
        if self.observed.phase.get(&c).map(String::as_str) == Some("done") {
            let result = if fields[8].starts_with("refusal:Busy:") {
                "busy"
            } else if c == "status"
                && (fields[8].starts_with("refusal:NoSuchSandbox:")
                    || fields[8].starts_with("refusal:NoSuchWorkspace:"))
            {
                "gone"
            } else if fields[8].starts_with("refusal:")
                || fields[8].starts_with("ok:failed")
                || fields[8] == "target-failed"
            {
                "failed"
            } else {
                "ok"
            };
            if c == "status"
                && result == "gone"
                && self.observed.phase.iter().any(|(caller, phase)| {
                    caller != &c
                        && (phase == "checked" || phase == "spawned")
                        && self.observed.plan.get(caller).map(String::as_str) == Some("restart")
                })
            {
                self.observed.gone_mid_restart = true;
            }
            self.observed.result.insert(c.clone(), result.into());
            self.observed.target.insert(c.clone(), fields[9].clone());
            self.observed.reported |= fields[10] == "true";
        }
        self.update(&fields, &c)?;
        if fields[8].starts_with("refusal:") {
            eprintln!("UNMODELED_RETURN {c}: {}", fields[8]);
        }
        Ok(())
    }
}
impl Drop for Replay {
    fn drop(&mut self) {
        let _ = writeln!(self.input, "stop");
        let _ = self.input.flush();
        let _ = self.child.wait();
        eprintln!("REPLAY_COUNTS traces={} steps={}", self.traces, self.steps);
    }
}
