//! Turns on one discovery directory: this process's, then the OS lock.
use super::*;
extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}
const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const LOCK_UN: i32 = 8;

/// Calls take turns on one directory: in this process before its OS lock,
/// and between processes on the lock itself. The wait is readiness, bounded,
/// and nothing has been attempted before it ends.
const IN_PROCESS_WAIT: Duration = Duration::from_secs(60);
/// Between two looks at another process's lock.
const LOCK_PAUSE: Duration = Duration::from_millis(2);
static HELD: Mutex<std::collections::BTreeSet<PathBuf>> =
    Mutex::new(std::collections::BTreeSet::new());
static FREED: std::sync::Condvar = std::sync::Condvar::new();

/// One discovery mutation's ownership of a directory: this process's turn and
/// the OS lock. Dropping it releases both.
pub(crate) struct Guard {
    file: File,
    directory: PathBuf,
}
impl Drop for Guard {
    fn drop(&mut self) {
        // The lock belongs to the open file description, which a child forked
        // meanwhile shares until its exec: closing only this descriptor would
        // leave it held (F-053). Unlock explicitly; the close follows.
        unsafe { flock(self.file.as_raw_fd(), LOCK_UN) };
        let mut held = HELD.lock().unwrap_or_else(|error| error.into_inner());
        held.remove(&self.directory);
        FREED.notify_all();
    }
}

/// Another call of this process, or another process holding the OS lock,
/// is waited for until the bounded wait ends; then the call is refused Busy,
/// which is not retried.
pub(crate) fn guard(directory: &Path) -> Result<Guard> {
    let deadline = std::time::Instant::now() + IN_PROCESS_WAIT;
    let mut held = registry::lock(&HELD)?;
    while held.contains(directory) {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(refused(
                "Busy",
                "another call of this process owns this discovery mutation",
            ));
        }
        held = FREED
            .wait_timeout(held, remaining)
            .map_err(|_| refused("Failed", "discovery turn poisoned"))?
            .0;
    }
    held.insert(directory.to_owned());
    drop(held);
    match os_lock(directory, deadline) {
        Ok(file) => Ok(Guard {
            file,
            directory: directory.to_owned(),
        }),
        Err(refusal) => {
            let mut held = HELD.lock().unwrap_or_else(|error| error.into_inner());
            held.remove(directory);
            FREED.notify_all();
            Err(refusal)
        }
    }
}
/// The OS lock, waited for until `deadline`; it is released even if its
/// process exits.
fn os_lock(directory: &Path, deadline: std::time::Instant) -> Result<File> {
    let path = directory.join("ownership.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(no_follow())
        .open(&path)
        .map_err(failed)?;
    private(&path, false)?;
    while unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } != 0 {
        if std::time::Instant::now() >= deadline {
            return Err(refused(
                "Busy",
                "another process owns this discovery mutation",
            ));
        }
        std::thread::sleep(LOCK_PAUSE);
    }
    Ok(file)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::{
        fd::{FromRawFd, OwnedFd, RawFd},
        unix::{fs::PermissionsExt, process::CommandExt},
    };
    use std::process::{Command, Stdio};
    extern "C" {
        fn pipe2(fds: *mut i32, flags: i32) -> i32;
        fn getpid() -> i32;
        fn poll(fds: *mut [i32; 2], count: u64, timeout: i32) -> i32;
        fn read(fd: i32, buffer: *mut u8, count: usize) -> isize;
        fn write(fd: i32, buffer: *const u8, count: usize) -> isize;
        fn close(fd: i32) -> i32;
    }
    const CLOEXEC: i32 = 0o2000000;
    const POLLIN: i32 = 1;

    fn pipe() -> (OwnedFd, OwnedFd) {
        let mut ends = [0 as RawFd; 2];
        assert_eq!(unsafe { pipe2(ends.as_mut_ptr(), CLOEXEC) }, 0);
        unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) }
    }

    /// F-053: a child forked while a Guard is held shares the lock's open
    /// file description until its exec. Dropping the Guard must release the
    /// lock itself, not only this process's descriptor.
    #[test]
    fn a_dropped_guard_releases_the_lock_while_a_forked_child_holds_a_copy() {
        let directory = std::env::temp_dir().join(format!("layerfs-f053-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let (signal_read, signal_write) = pipe();
        let (release_read, release_write) = pipe();
        let (signal, release, release_copy) = (
            signal_write.as_raw_fd(),
            release_read.as_raw_fd(),
            release_write.as_raw_fd(),
        );
        let held = guard(&directory).unwrap();
        // Another thread of the host starts a process meanwhile; its child
        // waits between fork and exec with the copied descriptor table.
        let spawner = std::thread::spawn(move || {
            let mut command = Command::new("true");
            command.stdin(Stdio::null());
            unsafe {
                command.pre_exec(move || {
                    close(release_copy);
                    let pid = getpid().to_ne_bytes();
                    write(signal, pid.as_ptr(), 4);
                    let mut byte = 0u8;
                    read(release, &mut byte, 1);
                    Ok(())
                });
            }
            command.spawn().unwrap().wait().unwrap()
        });
        // pollfd { fd, events, revents } as two i32 words (events|revents).
        let mut poll_fd = [signal_read.as_raw_fd(), POLLIN];
        assert_eq!(unsafe { poll(&mut poll_fd, 1, 10_000) }, 1, "fork signal");
        let mut pid = [0u8; 4];
        assert_eq!(
            unsafe { read(signal_read.as_raw_fd(), pid.as_mut_ptr(), 4) },
            4
        );
        let child = i32::from_ne_bytes(pid);
        let lock = directory.join("ownership.lock");
        let copied = fs::read_dir(format!("/proc/{child}/fd"))
            .unwrap()
            .filter_map(|entry| fs::read_link(entry.ok()?.path()).ok())
            .any(|target| target == lock);
        assert!(copied, "child {child} holds no copy of {}", lock.display());

        drop(held);
        // A second open of ownership.lock, as another process would take it.
        let taken = os_lock(&directory, std::time::Instant::now())
            .map_err(|refusal| (refusal.code, refusal.message));
        drop(release_write);
        assert!(spawner.join().unwrap().success());
        let _ = fs::remove_dir_all(&directory);
        assert!(
            taken.is_ok(),
            "lock still held through the child's copy: {taken:?}"
        );
    }
}
