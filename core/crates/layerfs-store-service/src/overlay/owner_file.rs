//! Workspace owners started on one overlay file: a copy of the closed
//! template for a new Workspace, or the saved file of an ended daemon.
use super::{
    owner::{Context, Owner, OwnerClient, OwnerError},
    state::{elapsed, Shared},
};
use layerfs_overlay::{Overlay, ProfileConfig};
use std::{path::Path, sync::Arc, time::Instant};

impl Owner {
    pub(super) fn start_copy(
        path: &Path,
        template: &Path,
        profile: ProfileConfig,
        shared: Arc<Shared>,
    ) -> crate::OwnerStart {
        let started = Instant::now();
        Self::start_file(
            Overlay::copy_template_observed(path, profile, template),
            started,
            shared,
        )
    }
    /// A Workspace owner on the saved overlay file `path` of an ended daemon,
    /// opened as it is.
    pub(super) fn start_existing(
        path: &Path,
        profile: ProfileConfig,
        shared: Arc<Shared>,
    ) -> crate::OwnerStart {
        let started = Instant::now();
        Self::start_file(Overlay::reopen_observed(path, profile), started, shared)
    }
    fn start_file(
        created: layerfs_overlay::Creation,
        started: Instant,
        shared: Arc<Shared>,
    ) -> crate::OwnerStart {
        let startup = Arc::new(created.work);
        let result = created.result.map_err(OwnerError::Overlay).map(|db| {
            let profile = db.profile().clone();
            {
                let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                state.db = Some(Box::new(db));
                state.initial = Some(startup.clone());
            }
            Self {
                client: OwnerClient {
                    context: Context::Workspace(shared),
                },
                profile,
            }
        });
        crate::OwnerStart {
            result,
            startup,
            creation_reported: true,
            elapsed_ns: elapsed(started),
        }
    }
}
