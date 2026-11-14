//! A Sandbox as one daemon child process on this Linux host, with no
//! container engine: the host itself (an Apple `container` VM, for instance)
//! supplies `/dev/fuse` and `CAP_SYS_ADMIN`. Every Sandbox of the host shares
//! its mount namespace, so each one owns a private directory whose
//! `workspaces` home holds its mounts. Commands run through `setpriv` as the
//! command identity, with no capabilities and `no_new_privs`.
use super::process_host::{detach_mounts, exited, prepare, remove_empty_homes, start_time};
use super::stop_rules::{wait_gone, StillRunning, KILL_WAIT};
use crate::CommandIdentity;
use nix::{
    sys::signal::{kill, Signal},
    unistd::Pid,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    net::SocketAddr,
    os::unix::{
        fs::OpenOptionsExt,
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// The daemon prints this, then its bound address, once it serves.
const LISTEN: &str = "LAYERFS_DAEMON_LISTEN ";
/// Pause between startup and stop observations.
const PAUSE: Duration = Duration::from_millis(20);
/// The `PATH` of a command whose environment names none.
const PATH: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// The private paths of one Sandbox directory. The directory is traversable
/// by the command identity (it holds the mount home); its config and
/// Overlay directories are the daemon's alone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    pub directory: PathBuf,
}
impl Layout {
    pub fn config(&self) -> PathBuf {
        self.directory.join("config/daemon.setup")
    }
    pub fn overlay(&self) -> PathBuf {
        self.directory.join("overlay/overlay.sqlite")
    }
    pub fn mounts(&self) -> PathBuf {
        self.directory.join("workspaces")
    }
    pub(super) fn log(&self) -> PathBuf {
        self.directory.join("daemon.log")
    }
    /// The log of the daemon before the last restart.
    fn previous_log(&self) -> PathBuf {
        self.directory.join("daemon.previous.log")
    }
}

/// The step a start reached.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartPhase {
    /// Sandbox directory, config and log creation; nothing runs yet.
    Prepare,
    /// The daemon spawn.
    Spawn,
    /// The listen line wait; the daemon runs, or ran.
    Listener,
}
/// A start that stopped at `phase`. `sandbox` is the spawned daemon, when
/// one was spawned: its caller stops and deletes it.
#[derive(Debug)]
pub struct StartFailure {
    pub phase: StartPhase,
    pub sandbox: Option<Sandbox>,
    pub cause: io::Error,
}

/// One daemon process, known by its pid and its kernel start time, so a
/// reused pid is never taken for it. No Drop cleanup.
#[derive(Debug)]
pub struct Sandbox {
    layout: Layout,
    pid: u32,
    start: u64,
    endpoint: SocketAddr,
    stopped: bool,
}

impl Sandbox {
    /// Makes `layout.directory` (it must not exist), writes `config` there,
    /// spawns `executable` on it and waits up to `wait` for its listen line.
    pub fn start(
        executable: &Path,
        config: &[u8],
        layout: Layout,
        wait: Duration,
    ) -> Result<Self, Box<StartFailure>> {
        let fail = |phase, sandbox, cause| {
            Box::new(StartFailure {
                phase,
                sandbox,
                cause,
            })
        };
        let log = prepare(&layout, config).map_err(|e| fail(StartPhase::Prepare, None, e))?;
        Self::spawn(executable, layout, log, wait)
    }

    /// Starts a daemon again on the existing folder of a saved Sandbox
    /// whose daemon (`pid` with `start`) ended, on its saved config. Refused
    /// while that daemon runs, or when the folder or config is missing. It
    /// detaches any dead mount left in the folder's own mount home, removes
    /// its empty mount directories, and keeps
    /// the ended daemon's log as `daemon.previous.log`. No overlay file is
    /// touched: the daemon finds them.
    pub fn restart(
        executable: &Path,
        layout: Layout,
        pid: u32,
        start: u64,
        wait: Duration,
    ) -> Result<Self, Box<StartFailure>> {
        let prepare = |cause| {
            Box::new(StartFailure {
                phase: StartPhase::Prepare,
                sandbox: None,
                cause,
            })
        };
        if !layout.directory.is_dir() || !layout.config().is_file() {
            return Err(prepare(io::Error::new(
                io::ErrorKind::NotFound,
                "no saved Sandbox folder with a daemon config",
            )));
        }
        if start_time(pid).map_err(prepare)? == Some(start) {
            return Err(prepare(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the saved daemon still runs",
            )));
        }
        detach_mounts(&layout.mounts()).map_err(prepare)?;
        remove_empty_homes(&layout.mounts()).map_err(prepare)?;
        fs::rename(layout.log(), layout.previous_log())
            .or_else(|e| match e.kind() {
                io::ErrorKind::NotFound => Ok(()),
                _ => Err(e),
            })
            .map_err(prepare)?;
        let log = OpenOptions::new()
            .append(true)
            .create_new(true)
            .mode(0o600)
            .open(layout.log())
            .map_err(prepare)?;
        Self::spawn(executable, layout, log, wait)
    }

    /// Spawns `executable` on the folder's config with output to `log`, and
    /// waits up to `wait` for its listen line.
    fn spawn(
        executable: &Path,
        layout: Layout,
        log: File,
        wait: Duration,
    ) -> Result<Self, Box<StartFailure>> {
        let fail = |phase, sandbox, cause| {
            Box::new(StartFailure {
                phase,
                sandbox,
                cause,
            })
        };
        let mut child = Command::new(executable)
            .arg("--config")
            .arg(layout.config())
            .stdin(Stdio::null())
            .stdout(
                log.try_clone()
                    .map_err(|e| fail(StartPhase::Spawn, None, e))?,
            )
            .stderr(log)
            // its own process group: a signal to this process's group (a
            // terminal's Ctrl-C) does not reach the daemon
            .process_group(0)
            .spawn()
            .map_err(|e| fail(StartPhase::Spawn, None, e))?;
        let pid = child.id();
        // The daemon outlives no reaper: this thread collects its exit.
        thread::spawn(move || child.wait());
        let start = match start_time(pid) {
            Ok(Some(start)) => start,
            // exited already: a daemon that finds its folder held says so and
            // exits at once, maybe before this read (it then vanishes during
            // the read, ESRCH); its line names the holder
            Ok(None) => return Err(fail(StartPhase::Spawn, None, exited(&layout))),
            Err(cause) if cause.raw_os_error() == Some(nix::libc::ESRCH) => {
                return Err(fail(StartPhase::Spawn, None, exited(&layout)))
            }
            Err(cause) => return Err(fail(StartPhase::Spawn, None, cause)),
        };
        let mut sandbox = Self {
            layout,
            pid,
            start,
            endpoint: SocketAddr::from(([127, 0, 0, 1], 0)),
            stopped: false,
        };
        let deadline = Instant::now() + wait;
        loop {
            match sandbox.listen_line() {
                Ok(Some(endpoint)) => {
                    sandbox.endpoint = endpoint;
                    return Ok(sandbox);
                }
                Ok(None) if !sandbox.running() => {
                    let cause = io::Error::other("daemon exited before it listened");
                    return Err(fail(StartPhase::Listener, Some(sandbox), cause));
                }
                Ok(None) if Instant::now() >= deadline => {
                    let cause = io::Error::from(io::ErrorKind::TimedOut);
                    return Err(fail(StartPhase::Listener, Some(sandbox), cause));
                }
                Ok(None) => thread::sleep(PAUSE),
                Err(cause) => return Err(fail(StartPhase::Listener, Some(sandbox), cause)),
            }
        }
    }

    /// Reopens a saved daemon: the same pid with the same start time, still
    /// running. Gone or exited is `NotFound`. The process is checked before
    /// the log: a restart replaces the log while the ended daemon is saved.
    pub fn reopen(layout: Layout, pid: u32, start: u64) -> io::Result<Self> {
        let mut sandbox = Self {
            layout,
            pid,
            start,
            endpoint: SocketAddr::from(([127, 0, 0, 1], 0)),
            stopped: false,
        };
        sandbox.endpoint()?;
        sandbox.endpoint = sandbox
            .listen_line()?
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no daemon listen line"))?;
        sandbox.endpoint()?;
        Ok(sandbox)
    }

    /// A saved daemon a reopen found gone: only its delete is admitted.
    pub fn stopped(layout: Layout, pid: u32, start: u64) -> Self {
        Self {
            layout,
            pid,
            start,
            endpoint: SocketAddr::from(([127, 0, 0, 1], 0)),
            stopped: true,
        }
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn start_time(&self) -> u64 {
        self.start
    }
    /// `pid:<pid>@<start>`, for records and messages.
    pub fn identity(&self) -> String {
        format!("pid:{}@{}", self.pid, self.start)
    }

    /// The daemon's loopback address, while it runs; `NotFound` once it exited.
    pub fn endpoint(&self) -> io::Result<SocketAddr> {
        if self.running() {
            Ok(self.endpoint)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "daemon process exited",
            ))
        }
    }

    /// True while the daemon process runs (not a zombie, same start time).
    pub fn running(&self) -> bool {
        matches!(start_time(self.pid), Ok(Some(start)) if start == self.start)
    }

    /// The final `cap` bytes of the daemon's output.
    pub fn log_tail(&self, cap: usize) -> io::Result<Vec<u8>> {
        let mut file = File::open(self.layout.log())?;
        let length = file.metadata()?.len();
        file.seek(SeekFrom::Start(length.saturating_sub(cap as u64)))?;
        let mut kept = Vec::new();
        file.read_to_end(&mut kept)?;
        Ok(kept)
    }

    /// TERM to the daemon, then up to `wait` until it is gone, then KILL and
    /// up to `KILL_WAIT` (stop_rules) until it is gone, else `TimedOut`. A
    /// daemon that already exited is stopped.
    pub fn stop(&mut self, wait: Duration) -> io::Result<()> {
        if self.running() {
            let pid = Pid::from_raw(self.pid as i32);
            let pause = || thread::sleep(PAUSE);
            kill(pid, Signal::SIGTERM).map_err(io::Error::from)?;
            let termed = wait_gone(
                || self.running(),
                Instant::now,
                pause,
                Instant::now() + wait,
            );
            if termed.is_err() {
                kill(pid, Signal::SIGKILL).map_err(io::Error::from)?;
                wait_gone(
                    || self.running(),
                    Instant::now,
                    pause,
                    Instant::now() + KILL_WAIT,
                )
                .map_err(|StillRunning| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "daemon pid {} still runs {} s after SIGKILL",
                            self.pid,
                            KILL_WAIT.as_secs()
                        ),
                    )
                })?;
            }
        }
        self.stopped = true;
        Ok(())
    }

    /// Detaches any mount the stopped daemon left in its mount home, then
    /// removes the Sandbox directory. The shared Store is not in it. A
    /// directory that is already gone (its host was recreated) is deleted.
    pub fn delete(&mut self) -> io::Result<()> {
        if !self.stopped {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "delete before a known stop",
            ));
        }
        detach_mounts(&self.layout.mounts())?;
        match fs::remove_dir_all(&self.layout.directory) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// Runs `arguments` to their end as `identity` in `directory`, with
    /// exactly `environment` (plus a default `PATH`), stdin closed, and
    /// copies stdout and stderr. A signal-ended command exits 128 + signal.
    pub fn run(
        &self,
        identity: CommandIdentity,
        arguments: &[String],
        environment: &[String],
        directory: &str,
        stdout: &mut (impl Write + Send),
        stderr: &mut (impl Write + Send),
    ) -> io::Result<i32> {
        identity
            .check()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "nonroot command uid/gid"))?;
        if !self.running() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "daemon process exited",
            ));
        }
        let mut command = Command::new("setpriv");
        command
            .arg(format!("--reuid={}", identity.uid))
            .arg(format!("--regid={}", identity.gid))
            .args(["--clear-groups", "--no-new-privs", "--inh-caps=-all"])
            .args(arguments)
            .env_clear()
            .current_dir(directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !environment.iter().any(|v| v.starts_with("PATH=")) {
            command.env("PATH", &PATH["PATH=".len()..]);
        }
        for variable in environment {
            let (name, value) = variable
                .split_once('=')
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "environment entry"))?;
            command.env(name, value);
        }
        let mut child = command.spawn()?;
        let (mut out, mut err) = (
            child.stdout.take().expect("piped"),
            child.stderr.take().expect("piped"),
        );
        let (copied_out, copied_err) = thread::scope(|scope| {
            let a = scope.spawn(|| io::copy(&mut out, stdout));
            let b = scope.spawn(|| io::copy(&mut err, stderr));
            (a.join(), b.join())
        });
        let status = child.wait()?;
        copied_out.map_err(|_| io::Error::other("stdout copy panicked"))??;
        copied_err.map_err(|_| io::Error::other("stderr copy panicked"))??;
        Ok(status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)))
    }

    /// The bound address from the daemon's first output line, once printed.
    fn listen_line(&self) -> io::Result<Option<SocketAddr>> {
        let mut text = String::new();
        File::open(self.layout.log())?
            .take(256)
            .read_to_string(&mut text)
            .or_else(|e| {
                if e.kind() == io::ErrorKind::InvalidData {
                    Ok(0)
                } else {
                    Err(e)
                }
            })?;
        let Some(line) = text.lines().next().filter(|_| text.contains('\n')) else {
            return Ok(None);
        };
        let Some(address) = line.strip_prefix(LISTEN) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("daemon output before its listen line: {line}"),
            ));
        };
        address
            .parse()
            .map(Some)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "daemon listen address"))
    }
}
