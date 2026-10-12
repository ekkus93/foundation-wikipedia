//! Pre-download topic preview from revision-joined Enterprise records.
//!
//! These diagnostics describe only the supplied source generation. Neither
//! a matching generation label nor a local SHA-256 receipt authenticates an
//! official Wikimedia publication or proves a complete source graph.

use std::collections::{BTreeMap, BTreeSet};

use wiki_source::enterprise::JoinedEnterpriseArticle;

use crate::enterprise_graph::{
    graph_from_joined, resolve_joined_revision_pinned_with_redirects, JoinedSelectionError,
    PageRedirectIdentity, RevisionPinnedSelection,
};
use crate::preview::{preview_pinned_selection, SelectionPreview};
use crate::selection::{ResolveError, Rules};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinedPreviewError {
    Joined(JoinedSelectionError),
    Preview(ResolveError),
    InconsistentSelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinedSelectionPreview {
    /// Revision-scoped identity map and full supplied-graph drift receipt.
    pub revision_pinned: RevisionPinnedSelection,
    /// Counts and fanout warnings for the exact same selected page IDs.
    pub preview: SelectionPreview,
}

/// Validate exact-generation redirects and mainspace identity before showing
/// preview counts. No media fetch, pack export, or trust transition occurs.
/// The repeated graph pass is intentional: a mismatch fails closed rather
/// than allowing preview counts to refer to a different selection receipt.
pub fn preview_joined_selection(
    records: &[JoinedEnterpriseArticle],
    administrative: &BTreeSet<String>,
    rules: &Rules,
    redirect_identities: &[PageRedirectIdentity],
) -> Result<JoinedSelectionPreview, JoinedPreviewError> {
    let revision_pinned =
        resolve_joined_revision_pinned_with_redirects(records, administrative, rules, redirect_identities)
            .map_err(JoinedPreviewError::Joined)?;
    let joined = graph_from_joined(records, administrative)
        .map_err(|error| JoinedPreviewError::Joined(JoinedSelectionError::Graph(error)))?;
    // Duplicate and conflicting redirect identities were rejected by the
    // revision-pinned resolver before this map can be used.
    let redirects: BTreeMap<u64, u64> = redirect_identities
        .iter()
        .map(|identity| (identity.from_page_id, identity.to_page_id))
        .collect();
    let snapshot_id = format!("{}-{}", joined.project, joined.generation_id);
    let preview = preview_pinned_selection(
        &joined.graph,
        rules,
        &redirects,
        &BTreeSet::new(),
        &snapshot_id,
    )
    .map_err(JoinedPreviewError::Preview)?;
    if revision_pinned.selection != preview.selection {
        return Err(JoinedPreviewError::InconsistentSelection);
    }
    Ok(JoinedSelectionPreview {
        revision_pinned,
        preview,
    })
}
