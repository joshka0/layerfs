//! Private, permission-authorized project and Sandbox discovery. No mutation replay.
use crate::{
    job::{failed, refused},
    registry::{self, Mount, Process, Project, SandboxState},
};
use layerfs_api::{internal::workspace, BoundWorkspace, MountedWorkspace};
use layerfs_bridge::{
    control::{Reply, SdkRefusal, SdkValue},
    provision::StoreManifest,
};
use layerfs_persistence::{SealedStore, SqlitePersistenceProfile};
use layerfs_sandbox::{
    backend::{
        docker::{self, Docker, SandboxRequest},
        Failure, Runtime,
    },
    CommandIdentity, ContainerId,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
extern "C" {
    fn geteuid() -> u32;
}
type Result<T> = std::result::Result<T, SdkRefusal>;

pub(crate) fn home(args: &SdkValue) -> Result<PathBuf> {
    let home = args
        .get_text("home")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LAYERFS_HOME").map(PathBuf::from))
        .ok_or_else(|| refused("InvalidInput", "LAYERFS_HOME is required"))?;
    if !home.is_absolute() {
        return Err(refused("InvalidInput", "LAYERFS_HOME must be absolute"));
    }
    directory(&home)?;
    Ok(home)
}
fn private(path: &Path, dir: bool) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(failed)?;
    if meta.file_type().is_symlink()
        || meta.is_dir() != dir
        || (!dir && !meta.is_file())
        || meta.uid() != unsafe { geteuid() }
        || meta.mode() & 0o077 != 0
    {
        return Err(refused(
            "Unauthorized",
            format!("{} must be owned by this user and private", path.display()),
        ));
    }
    Ok(())
}
pub(crate) fn directory(path: &Path) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(failed(e)),
    }
    private(path, true)
}
/// Where a project's Sandboxes live: `LAYERFS_HOME/<project>/sandboxes/<n>/`
/// (#373), numbered from 1 within the project.
fn sandboxes(args: &SdkValue) -> Result<PathBuf> {
    let root = project_dir(args)?.join("sandboxes");
    directory(&root)?;
    Ok(root)
}
/// A Sandbox number as text: 1 to 9999, no leading zero.
fn number(id: &str) -> Option<u32> {
    let n = id.parse::<u32>().ok()?;
    ((1..=9999).contains(&n) && n.to_string() == id).then_some(n)
}
/// The directory of the project's Sandbox `id`.
pub(crate) fn sandbox_dir(args: &SdkValue, id: &str) -> Result<PathBuf> {
    if number(id).is_none() {
        return Err(refused(
            "InvalidInput",
            format!("a Sandbox is a number from 1 to 9999, not {id}"),
        ));
    }
    Ok(sandboxes(args)?.join(id))
}
/// The project's Sandbox numbers that have a directory, lowest first.
pub(crate) fn sandbox_ids(args: &SdkValue) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for item in fs::read_dir(sandboxes(args)?).map_err(failed)? {
        let name = item.map_err(failed)?.file_name();
        if let Some(n) = name.to_str().and_then(number) {
            ids.push(n);
        }
    }
    ids.sort_unstable();
    Ok(ids.into_iter().map(|n| n.to_string()).collect())
}
/// Claims the lowest free Sandbox number: its directory, made once. Called
/// under the project's guard, so no other claim runs meanwhile.
pub(crate) fn claim_next(args: &SdkValue) -> Result<(String, PathBuf)> {
    let root = sandboxes(args)?;
    let n = (1..=9999u32)
        .find(|n| !root.join(n.to_string()).exists())
        .ok_or_else(|| refused("Capacity", "the project has 9999 Sandboxes"))?;
    let path = root.join(n.to_string());
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .map_err(failed)?;
    private(&path, true)?;
    Ok((n.to_string(), path))
}
pub(crate) fn project_dir(args: &SdkValue) -> Result<PathBuf> {
    let name = registry::text(args, "project")?;
    layerfs_history::HistoryName::new(name).map_err(failed)?;
    let path = home(args)?.join(name);
    private(&path, true)?;
    Ok(path)
}
#[cfg(target_os = "macos")]
fn no_follow() -> i32 {
    0x100
}
#[cfg(target_os = "linux")]
fn no_follow() -> i32 {
    0x20000
}

pub(crate) fn save(path: &Path, value: SdkValue) -> Result<()> {
    let bytes = layerfs_bridge::control::Answer {
        id: 1,
        reply: Reply::Sdk(value),
    }
    .encode()
    .map_err(failed)?;
    let temp = path.with_extension(format!(
        "{}-{}",
        std::process::id(),
        layerfs_bridge::sdk_values::hex(&registry::random()?[..8])
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(no_follow())
        .open(&temp)
        .map_err(failed)?;
    file.write_all(&bytes).map_err(failed)?;
    drop(file);
    fs::rename(temp, path).map_err(failed)
}
/// A small private text file, written once (0600, not a symlink).
pub(crate) fn save_text(path: &Path, text: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(no_follow())
        .open(path)
        .map_err(failed)?;
    file.write_all(format!("{text}\n").as_bytes())
        .map_err(failed)
}
pub(crate) fn load(path: &Path) -> Result<SdkValue> {
    private(path, false)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(no_follow())
        .open(path)
        .map_err(failed)?;
    if file.metadata().map_err(failed)?.len() > 65535 {
        return Err(refused("InvalidInput", "discovery record too large"));
    }
    let mut bytes = Vec::new();
    file.take(65536).read_to_end(&mut bytes).map_err(failed)?;
    match layerfs_bridge::control::Answer::decode(&bytes)
        .map_err(failed)?
        .reply
    {
        Reply::Sdk(value) => Ok(value),
        _ => Err(refused("InvalidInput", "discovery record kind")),
    }
}
fn bytes<const N: usize>(value: &SdkValue, key: &str) -> Result<[u8; N]> {
    match value.get(key) {
        Some(SdkValue::Bytes(bytes)) => bytes.as_slice().try_into().map_err(failed),
        _ => Err(refused("InvalidInput", format!("discovery {key}"))),
    }
}
pub(crate) fn save_project(path: &Path, project: &Project) -> Result<()> {
    save(
        &path.join("project.record"),
        SdkValue::map([
            (
                "manifest",
                SdkValue::Bytes(project.manifest.encode().map_err(failed)?),
            ),
            (
                "installed",
                project
                    .installed
                    .as_ref()
                    .map(|m| m.encode().map(SdkValue::Bytes))
                    .transpose()
                    .map_err(failed)?
                    .unwrap_or_default(),
            ),
        ]),
    )
}
pub(crate) fn project(args: &SdkValue) -> Result<Project> {
    let path = project_dir(args)?;
    let value = load(&path.join("project.record"))?;
    let decode = |name| match value.get(name) {
        Some(SdkValue::Bytes(bytes)) => StoreManifest::decode(bytes).map_err(failed),
        _ => Err(refused("NoSuchProject", "no complete project manifest")),
    };
    let manifest = decode("manifest")?;
    if manifest.profile != layerfs_bridge::provision::StoreProfile::Disposable {
        return Err(refused("InvalidInput", "Disposable required"));
    }
    let installed = if matches!(value.get("installed"), Some(SdkValue::Bytes(_))) {
        Some(decode("installed")?)
    } else {
        None
    };
    Ok(Project {
        store: SealedStore {
            path: path.join("sealed.sqlite"),
            profile: SqlitePersistenceProfile::Disposable,
            sqlite_version: manifest.host_sqlite.clone(),
            bytes: manifest.bytes,
        },
        manifest,
        installed,
    })
}

mod lock;
mod sandbox;
pub(crate) use lock::guard;
pub(crate) use sandbox::{
    gone, gone_container, refresh, reopen, runtime, save_process, save_sandbox, save_workspace,
    workspace_file,
};
