//! The host side of a process Sandbox: the files of its folder, the cleanup
//! of its mount home, and a process's kernel identity in `/proc`.
use super::process::Layout;
use nix::mount::{umount2, MntFlags};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

/// A spawned daemon that exited before its identity was read, with its
/// first output line when it wrote one.
pub(super) fn exited(layout: &Layout) -> io::Error {
    io::Error::other(match first_line(layout) {
        Some(line) => format!("daemon exited before its identity was read: {line}"),
        None => "daemon exited before its identity was read".to_owned(),
    })
}

/// The first output line of the daemon in `layout`, if it wrote one.
fn first_line(layout: &Layout) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(layout.log())
        .and_then(|file| file.take(256).read_to_end(&mut bytes))
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

/// Detaches every mount in the mount home `mounts` (its direct entries
/// only). A dead FUSE mount answers ENOTCONN; detaching it is the cleanup.
/// An entry that is not a mount point is left as it is.
pub(super) fn detach_mounts(mounts: &Path) -> io::Result<()> {
    if let Ok(entries) = fs::read_dir(mounts) {
        for entry in entries {
            let path = entry?.path();
            match umount2(&path, MntFlags::MNT_DETACH) {
                Ok(()) | Err(nix::errno::Errno::EINVAL) => {}
                Err(errno) => return Err(errno.into()),
            }
        }
    }
    Ok(())
}

/// Removes each empty directory in the mount home `mounts`: the mount
/// directories the ended daemon made. An attach never adopts an existing
/// directory (layerfs-fuse `create_directory`), so the reopened Workspace
/// makes its own again. A directory that is not empty is left as it is.
pub(super) fn remove_empty_homes(mounts: &Path) -> io::Result<()> {
    if let Ok(entries) = fs::read_dir(mounts) {
        for entry in entries {
            let path = entry?.path();
            match fs::remove_dir(&path) {
                Err(error)
                    if !matches!(
                        error.raw_os_error(),
                        Some(nix::libc::ENOTEMPTY) | Some(nix::libc::ENOTDIR)
                    ) && error.kind() != io::ErrorKind::NotFound =>
                {
                    return Err(error)
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// The Sandbox directory (0755, new), its private config and Overlay
/// directories (0700), the config file (0600) and the daemon log.
pub(super) fn prepare(layout: &Layout, config: &[u8]) -> io::Result<File> {
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
pub(super) fn start_time(pid: u32) -> io::Result<Option<u64>> {
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
