//! Pure rule: the engine state of a saved overlay file, from facts its rows
//! give, for a Workspace reopened after its daemon ended. No I/O here.
//!
//! decided: 2026-11-14, reopen supports only a Workspace that never captured
//! (every row in generation 1). A later generation means the Workspace
//! committed, and its base moved at each install; no record holds that base.
//! See core/docs/issues/373/REOPEN.md (P1).

/// What the overlay rows say. Each field is one bounded aggregate query.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Facts {
    /// Distinct generations over every layered table, ascending.
    pub generations: Vec<i64>,
    pub cleanup_tasks: u64,
    pub commit_records: u64,
    /// Largest `fs_inode.sequence`, if any row exists.
    pub max_sequence: Option<i64>,
    /// Rows of `fs_inode` and `fs_entry` (all in the active generation).
    pub inode_rows: u64,
    pub entry_rows: u64,
    /// Whether any `fs_data_ref` row exists.
    pub pieces: bool,
}

/// The rebuilt engine state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Resumed {
    pub active: i64,
    pub installed: i64,
    pub revision: i64,
    pub dirty_inodes: u64,
    pub dirty_directory_entries: u64,
    pub pieces: bool,
}

/// Why a saved overlay cannot be reopened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// Rows in the orphan domain: open-file state of the ended daemon.
    Orphans,
    /// Rows in a generation other than 1: the Workspace captured before.
    Captured,
    CleanupPending,
    CommitRecords,
    NegativeSequence,
}

impl Refusal {
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Orphans => "saved overlay holds orphan rows",
            Self::Captured => "saved overlay was captured (the Workspace committed before)",
            Self::CleanupPending => "saved overlay has pending cleanup tasks",
            Self::CommitRecords => "saved overlay has commit records",
            Self::NegativeSequence => "saved overlay has a negative content sequence",
        }
    }
}

pub fn resume(facts: &Facts) -> Result<Resumed, Refusal> {
    if facts.generations.contains(&-1) {
        return Err(Refusal::Orphans);
    }
    if facts.generations.iter().any(|&generation| generation != 1) {
        return Err(Refusal::Captured);
    }
    if facts.cleanup_tasks != 0 {
        return Err(Refusal::CleanupPending);
    }
    if facts.commit_records != 0 {
        return Err(Refusal::CommitRecords);
    }
    let revision = facts.max_sequence.unwrap_or(0);
    if revision < 0 {
        return Err(Refusal::NegativeSequence);
    }
    Ok(Resumed {
        active: 1,
        installed: 0,
        revision,
        dirty_inodes: facts.inode_rows,
        dirty_directory_entries: facts.entry_rows,
        pieces: facts.pieces,
    })
}
