//! One daemon per Sandbox folder (#373 R3-B2): the daemon holds an exclusive
//! flock on `<config>.lock` beside its config for its whole life, and writes
//! its pid there. A daemon that finds the lock held exits before it binds or
//! starts anything, naming the holder, so a second daemon can never serve the
//! folder whatever the Sandbox records say. The kernel drops the lock when the
//! holder ends.
//!
//! decided: the lock lives in the daemon, not in its spawner (operator,
//! 2026-11-14, option 1 of the R3-B2 report).
use super::ApplicationError;
use nix::{
    errno::Errno,
    fcntl::{Flock, FlockArg},
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

/// The held lock of the folder whose config is `config`.
pub(super) fn hold(config: &Path) -> Result<Flock<File>, ApplicationError> {
    let mut path = config.as_os_str().to_owned();
    path.push(".lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .map_err(ApplicationError::Io)?;
    match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(mut held) => {
            held.set_len(0).map_err(ApplicationError::Io)?;
            held.seek(SeekFrom::Start(0))
                .map_err(ApplicationError::Io)?;
            write!(held, "{}", std::process::id()).map_err(ApplicationError::Io)?;
            Ok(held)
        }
        Err((mut file, Errno::EWOULDBLOCK)) => {
            let mut holder = String::new();
            file.read_to_string(&mut holder)
                .map_err(ApplicationError::Io)?;
            Err(ApplicationError::Held(holder.trim().to_owned()))
        }
        Err((_, errno)) => Err(ApplicationError::Io(errno.into())),
    }
}
