//! A Sandbox as one daemon child process on this Linux host, with no
//! container engine: the host itself (an Apple `container` VM, for instance)
//! supplies `/dev/fuse` and `CAP_SYS_ADMIN`. Every Sandbox of the host shares
//! its mount namespace, so each one owns a private directory whose
//! `workspaces` home holds its mounts. Commands run through `setpriv` as the
//! command identity, with no capabilities and `no_new_privs`.
use crate::CommandIdentity;
use nix::{
    mount::{umount2, MntFlags},
    sys::signal::{kill, Signal},
    unistd::Pid,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    net::SocketAddr,
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
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
    fn log(&self) -> PathBuf {
        self.directory.join("daemon.log")
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
            Ok(None) => {
                let cause = io::Error::other("daemon exited before its identity was read");
                return Err(fail(StartPhase::Spawn, None, cause));
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
    /// running. Gone or exited is `NotFound`.
    pub fn reopen(layout: Layout, pid: u32, start: u64) -> io::Result<Self> {
        let mut sandbox = Self {
            layout,
            pid,
            start,
            endpoint: SocketAddr::from(([127, 0, 0, 1], 0)),
            stopped: false,
        };
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

    /// TERM to the daemon, then up to `wait` until it is gone, then KILL.
    /// A daemon that already exited is stopped.
    pub fn stop(&mut self, wait: Duration) -> io::Result<()> {
        if self.running() {
            let pid = Pid::from_raw(self.pid as i32);
            kill(pid, Signal::SIGTERM).map_err(io::Error::from)?;
            let deadline = Instant::now() + wait;
            while self.running() && Instant::now() < deadline {
                thread::sleep(PAUSE);
            }
            if self.running() {
                kill(pid, Signal::SIGKILL).map_err(io::Error::from)?;
                while self.running() {
                    thread::sleep(PAUSE);
                }
            }
        }
        self.stopped = true;
        Ok(())
    }

    /// Detaches any mount the stopped daemon left in its mount home, then
    /// removes the Sandbox directory. The shared Store is not in it.
    pub fn delete(&mut self) -> io::Result<()> {
        if !self.stopped {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "delete before a known stop",
            ));
        }
        let mounts = self.layout.mounts();
        if let Ok(entries) = fs::read_dir(&mounts) {
            for entry in entries {
                let path = entry?.path();
                // a dead FUSE mount answers ENOTCONN; detaching it is the cleanup
                match umount2(&path, MntFlags::MNT_DETACH) {
                    Ok(()) | Err(nix::errno::Errno::EINVAL) => {}
                    Err(errno) => return Err(errno.into()),
                }
            }
        }
        fs::remove_dir_all(&self.layout.directory)
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

/// The Sandbox directory (0755, new), its private config and Overlay
/// directories (0700), the config file (0600) and the daemon log.
fn prepare(layout: &Layout, config: &[u8]) -> io::Result<File> {
    let created = |path: &Path, mode| -> io::Result<()> {
        fs::DirBuilder::new().mode(mode).create(path)?;
        // the process umask may narrow the create mode
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    };
    if let Some(parent) = layout.directory.parent() {
        fs::DirBuilder::new()
            .mode(0o755)
            .recursive(true)
            .create(parent)?;
    }
    created(&layout.directory, 0o755)?;
    for private in [layout.config(), layout.overlay()] {
        created(private.parent().expect("in the directory"), 0o700)?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(layout.config())?
        .write_all(config)?;
    OpenOptions::new()
        .append(true)
        .create_new(true)
        .mode(0o600)
        .open(layout.log())
}

/// The kernel start time of a live (not zombie) `pid`, from field 22 of
/// `/proc/<pid>/stat`; `None` when no such process runs.
fn start_time(pid: u32) -> io::Result<Option<u64>> {
    let stat = match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    // after `(comm) `: state is field 3, start time field 22
    let fields: Vec<&str> = stat
        .rsplit_once(") ")
        .map(|(_, rest)| rest.split(' ').collect())
        .unwrap_or_default();
    match (fields.first(), fields.get(19)) {
        (Some(&"Z"), _) | (Some(&"X"), _) => Ok(None),
        (Some(_), Some(start)) => start
            .parse()
            .map(Some)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "process start time")),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "process stat")),
    }
}
