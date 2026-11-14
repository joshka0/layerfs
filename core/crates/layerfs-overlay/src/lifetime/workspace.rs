//! One Workspace's journaled runtime state; the database owns its filesystem rows.
use crate::{Generation, Overlay, OverlayError, OverlayResult, StatementKind, WorkspaceState};

impl Overlay {
    /// Binds one complete root without scanning it. Authority/root validation
    /// belongs to the Workspace/runtime caller; a second binding is refused.
    pub fn open_workspace(&self, base_root: [u8; 32]) -> OverlayResult<()> {
        self.atomic(|| {
            if self.workspace.get().is_some() {
                return Err(OverlayError::Invalid("database already holds a Workspace"));
            }
            self.workspace.set(Some(WorkspaceState {
                active: Generation(1),
                captured: None,
                captured_revision: None,
                installed: 0,
                revision: 0,
                base_root,
                dirty_inodes: 0,
                dirty_directory_entries: 0,
                closed: false,
                base_readers: 0,
                consolidating: None,
            }));
            Ok(())
        })
    }
    /// Binds the Workspace a saved overlay file holds, on `base_root`, after
    /// its daemon ended. The rows decide the engine state
    /// ([`super::resume_rules`]); a refused shape changes nothing.
    pub fn resume_workspace(&self, base_root: [u8; 32]) -> OverlayResult<()> {
        let one = |sql: &str| -> OverlayResult<i64> {
            self.query(StatementKind::Startup, sql, &[], 8, |row| row.get(0))?
                .pop()
                .ok_or(OverlayError::Invalid("resume aggregate"))
        };
        let count = |sql: &str| -> OverlayResult<u64> {
            u64::try_from(one(sql)?).map_err(|_| OverlayError::Invalid("resume row count"))
        };
        if self.workspace.get().is_some() {
            return Err(OverlayError::Invalid("database already holds a Workspace"));
        }
        let facts = super::resume_rules::Facts {
            generations: self.query(
                StatementKind::Startup,
                "SELECT generation FROM fs_inode UNION SELECT generation FROM fs_entry
                 UNION SELECT generation FROM fs_data UNION SELECT generation FROM fs_truncate
                 UNION SELECT generation FROM fs_data_ref ORDER BY 1",
                &[],
                8,
                |row| row.get(0),
            )?,
            cleanup_tasks: count("SELECT count(*) FROM cleanup_task")?,
            commit_records: count("SELECT count(*) FROM commit_record")?,
            max_sequence: self
                .query(
                    StatementKind::Startup,
                    "SELECT max(sequence) FROM fs_inode",
                    &[],
                    8,
                    |row| row.get::<_, Option<i64>>(0),
                )?
                .pop()
                .flatten(),
            inode_rows: count("SELECT count(*) FROM fs_inode")?,
            entry_rows: count("SELECT count(*) FROM fs_entry")?,
            pieces: one("SELECT EXISTS(SELECT 1 FROM fs_data_ref)")? != 0,
        };
        let resumed = super::resume_rules::resume(&facts)
            .map_err(|refusal| OverlayError::Invalid(refusal.reason()))?;
        self.pieces_seen.set(resumed.pieces);
        self.workspace.set(Some(WorkspaceState {
            active: Generation(resumed.active),
            captured: None,
            captured_revision: None,
            installed: resumed.installed,
            revision: resumed.revision,
            base_root,
            dirty_inodes: resumed.dirty_inodes,
            dirty_directory_entries: resumed.dirty_directory_entries,
            closed: false,
            base_readers: 0,
            consolidating: None,
        }));
        Ok(())
    }
    pub fn state(&self) -> OverlayResult<WorkspaceState> {
        self.available()?;
        self.workspace.get().ok_or(OverlayError::Stale)
    }
    pub(crate) fn change_state(
        &self,
        change: impl FnOnce(&mut WorkspaceState) -> OverlayResult<()>,
    ) -> OverlayResult<()> {
        let mut state = self.state()?;
        change(&mut state)?;
        self.workspace.set(Some(state));
        Ok(())
    }
    pub(crate) fn live(&self) -> OverlayResult<WorkspaceState> {
        let state = self.state()?;
        if state.closed {
            Err(OverlayError::Closed)
        } else {
            Ok(state)
        }
    }
    pub(crate) fn source_count(&self, add: bool) -> OverlayResult<()> {
        self.change_state(|state| {
            state.base_readers = changed_count(state.base_readers, if add { 1 } else { -1 })?;
            Ok(())
        })
    }
    pub(crate) fn dirty_counts(&self, inodes: i64, names: i64) -> OverlayResult<()> {
        self.change_state(|state| {
            state.dirty_inodes = changed_count(state.dirty_inodes, inodes)?;
            state.dirty_directory_entries = changed_count(state.dirty_directory_entries, names)?;
            Ok(())
        })
    }
}
/// Preserve the former nonnegative signed SQLite counter domain.
fn changed_count(value: u64, delta: i64) -> OverlayResult<u64> {
    value
        .checked_add_signed(delta)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or(OverlayError::Invalid("Workspace counter range"))
}
