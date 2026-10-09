//! Explicit checks before combining article HTML and structured blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionRef {
    pub project: String,
    pub page_id: u64,
    pub revision_id: u64,
    pub snapshot: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinError {
    InvalidIdentity,
    ProjectMismatch,
    PageMismatch,
    RevisionMismatch,
    SnapshotMismatch,
}

pub fn same_revision(html: &RevisionRef, blocks: &RevisionRef) -> Result<(), JoinError> {
    if html.project.is_empty()
        || blocks.project.is_empty()
        || html.snapshot.is_empty()
        || blocks.snapshot.is_empty()
        || html.page_id == 0
        || blocks.page_id == 0
        || html.revision_id == 0
        || blocks.revision_id == 0
    {
        return Err(JoinError::InvalidIdentity);
    }
    if html.project != blocks.project {
        return Err(JoinError::ProjectMismatch);
    }
    if html.page_id != blocks.page_id {
        return Err(JoinError::PageMismatch);
    }
    if html.revision_id != blocks.revision_id {
        return Err(JoinError::RevisionMismatch);
    }
    if html.snapshot != blocks.snapshot {
        return Err(JoinError::SnapshotMismatch);
    }
    Ok(())
}
