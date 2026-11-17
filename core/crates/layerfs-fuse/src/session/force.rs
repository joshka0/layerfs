//! Forced teardown of one connection: one abort write, the terminal fence,
//! the local drain, then one plain detach. Nothing here is repeated. When the
//! kernel mount is already gone (`mount_gone`), `abort_gone` skips that detach:
//! the kernel made it (lost-mount L4, decided 2026-11-17).
use super::state::{DetachAttempt, DrainStage, Drained, ForceRefusal, KernelSession, Undrained};
use crate::mount::{syscalls, AbortWrite};
use std::{sync::atomic::Ordering, time::Instant};

impl KernelSession {
    /// The kernel table no longer holds this connection's mount at its
    /// directory: something outside LayerFS detached it (lost-mount L4). An
    /// unreadable table, or a session with no recorded entry, is not gone.
    pub fn mount_gone(&self) -> bool {
        let Some(entry) = self.entry.as_ref() else {
            return false;
        };
        match syscalls::mount_entry(&self.directory) {
            Ok(now) => now.device != entry.device,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        }
    }
    /// After the kernel mount is gone (`mount_gone`), no detach can end the
    /// connection: its receive loops stay in `read()` until the connection is
    /// aborted. One abort write through the control bound at attach, then the
    /// forced drain, which skips its detach because the kernel already made it
    /// (lost-mount L4). Without a bound control it stops at Detach, as before.
    pub fn abort_gone(mut self) -> Result<Box<Drained>, Box<Undrained>> {
        match self.abort() {
            Ok(AbortWrite::Written) => {}
            _ => return Err(self.undrained(DrainStage::Detach, None)),
        }
        self.forced_detach = DetachAttempt::Detached;
        self.detached.store(true, Ordering::Release);
        let _ = self.queue.stop_receive();
        self.force_drain()
    }
    /// The abort control bound at attach is still held, unused.
    pub fn abort_bound(&self) -> bool {
        self.abort.is_some()
    }
    /// The connection's single abort write. Refused before any effect when
    /// the session is already detached or aborted, or when no control was
    /// bound. The control leaves the session before the write, so no second
    /// write can follow any result. Only `Written` stops the mount's request
    /// service; a short or failed write does nothing further.
    pub fn abort(&mut self) -> Result<AbortWrite, ForceRefusal> {
        if self.is_detached() || self.aborted.get().is_some() {
            return Err(ForceRefusal::NotServing);
        }
        let Some(control) = self.abort.take() else {
            return Err(ForceRefusal::AbortUnavailable);
        };
        let written = syscalls::abort(&control);
        drop(control);
        let _ = self.aborted.set(written);
        if written == AbortWrite::Written {
            // Refuse later receives and wake every request waiting inside its
            // service so that it observes the stop.
            let _ = self.queue.stop_service();
        }
        Ok(written)
    }
    /// After a `Written` abort: the same local drain as a normal unmount,
    /// then exactly one plain detach, then the request owner and directory. The detach
    /// is attempted only once the local drain held, and never again: `EBUSY`
    /// or any other errno stops at `Detach` with the connection aborted and
    /// still mounted. Called again, it observes the same predicate; a
    /// recorded failed detach is returned as it was, with no syscall.
    pub fn force_drain(mut self) -> Result<Box<Drained>, Box<Undrained>> {
        let attempted = match self.forced_detach {
            DetachAttempt::Failed(_) => return Err(self.undrained(DrainStage::Detach, None)),
            DetachAttempt::Detached => true,
            DetachAttempt::NotAttempted => false,
        };
        if self.aborted.get() != Some(&AbortWrite::Written) || self.is_detached() != attempted {
            return Err(self.undrained(DrainStage::Detach, None));
        }
        let deadline = Instant::now() + self.drain_wait;
        let work = match self.quiesce(deadline) {
            Ok(work) => work,
            Err(stop) => return Err(self.stopped(stop)),
        };
        if !attempted {
            match syscalls::detach(&self.directory) {
                Ok(()) => {
                    self.forced_detach = DetachAttempt::Detached;
                    self.detached.store(true, Ordering::Release);
                }
                Err(errno) => {
                    self.forced_detach = DetachAttempt::Failed(errno);
                    return Err(self.undrained(DrainStage::Detach, None));
                }
            }
        }
        self.release(work)
    }
}
