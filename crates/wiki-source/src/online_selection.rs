//! Revision-aware selection between current online and verified local versions.
//!
//! This is a source-selection policy, not a content or media integrity verifier.
//! Callers must supply 'local_complete' only after independently verifying all
//! required article text, HTML, images, equations and attribution objects.
//! In particular, a temporary cache is not automatically offline-ready.

use std::collections::BTreeSet;
use wiki_model::ArticleKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArticleLocation {
    InstalledPack,
    ExplicitOfflineSave,
    TemporaryCache,
    OnlineCurrent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkState {
    Online,
    Offline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArticleVersion {
    pub key: ArticleKey,
    pub revision_id: u64,
    pub location: ArticleLocation,
    /// Independently verified local article and mandatory visual dependencies.
    /// The selector does not establish this property or verify source trust.
    pub local_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReaderSelection {
    /// Current revision from an explicit successful online request.
    CurrentOnline(ArticleVersion),
    /// An installed/saved/cached revision, labeled by its actual location.
    Local(ArticleVersion),
    /// An online request is needed; the optional fallback is NOT current.
    FetchCurrent { fallback: Option<ArticleVersion> },
    /// Do not start a hidden network request when the device is offline.
    MissingOffline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionError {
    InvalidIdentity,
    InvalidRevision,
    ForeignArticle,
    DuplicateLocation,
    TooManyVersions,
}

/// Choose article display state without conflating cached, pack-owned,
/// explicitly saved, or online-current revisions.
///
/// Online selection prefers an explicitly supplied current revision, otherwise
/// requests a fresh online fetch with an optional complete local fallback.
/// Offline selection never returns a version whose media is incomplete and
/// never attempts a network fetch. A cache hit is not publisher verification.
pub fn choose_article(
    requested: &ArticleKey,
    network: NetworkState,
    versions: &[ArticleVersion],
) -> Result<ReaderSelection, SelectionError> {
    requested
        .validate()
        .map_err(|_| SelectionError::InvalidIdentity)?;
    if requested.page_id > i64::MAX as u64 {
        return Err(SelectionError::InvalidIdentity);
    }
    if versions.len() > 4 {
        return Err(SelectionError::TooManyVersions);
    }
    let mut seen = BTreeSet::new();
    for version in versions {
        version
            .key
            .validate()
            .map_err(|_| SelectionError::InvalidIdentity)?;
        if version.key != *requested {
            return Err(SelectionError::ForeignArticle);
        }
        if version.revision_id == 0 || version.revision_id > i64::MAX as u64 {
            return Err(SelectionError::InvalidRevision);
        }
        if !seen.insert(version.location) {
            return Err(SelectionError::DuplicateLocation);
        }
    }
    let local = [
        ArticleLocation::InstalledPack,
        ArticleLocation::ExplicitOfflineSave,
        ArticleLocation::TemporaryCache,
    ]
    .into_iter()
    .find_map(|location| {
        versions
            .iter()
            .find(|version| version.location == location && version.local_complete)
            .cloned()
    });
    match network {
        NetworkState::Offline => Ok(local
            .map(ReaderSelection::Local)
            .unwrap_or(ReaderSelection::MissingOffline)),
        NetworkState::Online => {
            if let Some(current) = versions
                .iter()
                .find(|version| version.location == ArticleLocation::OnlineCurrent)
            {
                return Ok(ReaderSelection::CurrentOnline(current.clone()));
            }
            Ok(ReaderSelection::FetchCurrent { fallback: local })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ArticleKey {
        ArticleKey {
            project: "enwiki".into(),
            page_id: 42,
        }
    }

    fn item(location: ArticleLocation, revision_id: u64, local_complete: bool) -> ArticleVersion {
        ArticleVersion {
            key: key(),
            revision_id,
            location,
            local_complete,
        }
    }

    #[test]
    fn offline_never_uses_network_or_incomplete_cache() {
        let current = item(ArticleLocation::OnlineCurrent, 110, false);
        let partial_cache = item(ArticleLocation::TemporaryCache, 109, false);
        assert_eq!(
            choose_article(
                &key(),
                NetworkState::Offline,
                &[current.clone(), partial_cache]
            ),
            Ok(ReaderSelection::MissingOffline)
        );
        let installed = item(ArticleLocation::InstalledPack, 100, true);
        assert_eq!(
            choose_article(&key(), NetworkState::Offline, &[current, installed.clone()]),
            Ok(ReaderSelection::Local(installed))
        );
    }

    #[test]
    fn distinguishes_current_offline_saved_and_cached_revisions() {
        let cached = item(ArticleLocation::TemporaryCache, 107, true);
        let saved = item(ArticleLocation::ExplicitOfflineSave, 103, true);
        let installed = item(ArticleLocation::InstalledPack, 90, true);
        let current = item(ArticleLocation::OnlineCurrent, 110, false);
        assert_eq!(
            choose_article(
                &key(),
                NetworkState::Online,
                &[cached.clone(), current.clone()]
            ),
            Ok(ReaderSelection::CurrentOnline(current))
        );
        assert_eq!(
            choose_article(
                &key(),
                NetworkState::Online,
                &[cached.clone(), saved.clone()]
            ),
            Ok(ReaderSelection::FetchCurrent {
                fallback: Some(saved.clone())
            })
        );
        assert_eq!(
            choose_article(&key(), NetworkState::Offline, &[cached.clone(), saved]),
            Ok(ReaderSelection::Local(item(
                ArticleLocation::ExplicitOfflineSave,
                103,
                true
            )))
        );
        assert_eq!(
            choose_article(&key(), NetworkState::Offline, &[cached.clone()]),
            Ok(ReaderSelection::Local(cached))
        );
        assert_eq!(
            choose_article(&key(), NetworkState::Offline, &[installed.clone()]),
            Ok(ReaderSelection::Local(installed))
        );
        assert_eq!(
            choose_article(&key(), NetworkState::Online, &[]),
            Ok(ReaderSelection::FetchCurrent { fallback: None })
        );
    }

    #[test]
    fn all_candidates_are_validated_before_selection() {
        let good = item(ArticleLocation::OnlineCurrent, 110, false);
        let foreign = ArticleVersion {
            key: ArticleKey {
                project: "frwiki".into(),
                page_id: 42,
            },
            ..item(ArticleLocation::InstalledPack, 103, true)
        };
        assert_eq!(
            choose_article(&key(), NetworkState::Online, &[good.clone(), foreign]),
            Err(SelectionError::ForeignArticle)
        );
        assert_eq!(
            choose_article(&key(), NetworkState::Offline, &[good.clone(), good.clone()]),
            Err(SelectionError::DuplicateLocation)
        );
        assert_eq!(
            choose_article(
                &key(),
                NetworkState::Online,
                &[good.clone(), item(ArticleLocation::TemporaryCache, 0, true)]
            ),
            Err(SelectionError::InvalidRevision)
        );
        assert_eq!(
            choose_article(
                &key(),
                NetworkState::Online,
                &[
                    good.clone(),
                    item(ArticleLocation::TemporaryCache, u64::MAX, true)
                ]
            ),
            Err(SelectionError::InvalidRevision)
        );
        assert_eq!(
            choose_article(
                &key(),
                NetworkState::Online,
                &[good.clone(), good.clone(), good.clone(), good.clone(), good]
            ),
            Err(SelectionError::TooManyVersions)
        );
    }
}
