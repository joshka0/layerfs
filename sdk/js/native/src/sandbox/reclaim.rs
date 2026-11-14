//! Reclaim of a Sandbox found gone or stopped (#373): its records go, so its
//! number is free again; a Sandbox holding saved Workspace overlays is kept.
use crate::job::{failed, refused, Outcome};
use crate::registry::{lock, text, SANDBOXES};
use layerfs_bridge::control::SdkValue;

/// The refusal code of a reclaim that found saved Workspace overlays.
pub(crate) const KEPT: &str = "Kept";

/// Reclaims Sandbox `id`, whose container or daemon a reopen found gone or stopped
/// (`container`, `stopped`): one unforced Delete of a stopped container, then
/// its records, so its number is free again. Called under the project's
/// guard; the Sandbox's own guard is taken too. A Sandbox already gone
/// (another call reclaimed it) is refused `NoSuchSandbox`. The record is read again
/// under them: a Sandbox started again under the number meanwhile has
/// another container and is left alone (`Busy`). A Delete the Engine refuses
/// (the container runs again) removes nothing.
pub(crate) fn reclaim(args: &SdkValue, id: &str, container: &str, stopped: bool) -> Outcome {
    let directory = crate::discovery::sandbox_dir(args, id)?;
    let record = directory.join("sandbox.record");
    let gone = || refused("NoSuchSandbox", format!("the project has no Sandbox {id}"));
    // Every caller holds the project's guard, and only a reclaim under it
    // removes a Sandbox directory, so this read is stable. A Sandbox another
    // call reclaimed meanwhile is gone; its ownership lock went with it.
    if !record.exists() {
        return Err(gone());
    }
    let _guard = crate::discovery::guard(&directory)?;
    if !record.exists() {
        return Err(gone());
    }
    let value = crate::discovery::load(&record)?;
    if text(&value, "container")? != container {
        return Err(refused(
            "Busy",
            format!("Sandbox {id} was started again while it was reclaimed; nothing was removed"),
        ));
    }
    // Saved Workspace overlays hold uncommitted work: such a Sandbox is
    // reopened (operator.sandbox.reopen), never reclaimed.
    if crate::discovery::holds_overlays(args, id)? {
        return Err(refused(
            KEPT,
            format!("Sandbox {id} holds saved Workspace overlays; reopen it, nothing was removed"),
        ));
    }
    if stopped {
        match crate::discovery::runtime(&value)?.stopped().delete() {
            Err(failure) if !failure.gone() => return Err(failed(failure)),
            _ => {}
        }
    }
    lock(&SANDBOXES)?.remove(&directory);
    std::fs::remove_file(record).map_err(failed)?;
    std::fs::remove_dir_all(&directory).map_err(failed)?;
    Ok(SdkValue::map([
        ("kind", SdkValue::text("Reclaimed")),
        ("id", SdkValue::text(id)),
    ]))
}

#[cfg(test)]
#[path = "../../tests/unit/reclaim.rs"]
mod reclaim_tests;
