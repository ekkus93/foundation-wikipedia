//! Verified, in-memory title catalog for network-free article navigation.
//!
//! This is a bounded prefix/exact lookup over *caller-verified* local versions,
//! not a persisted full-text search index or proof of pack/media integrity.
//! Callers must authenticate and verify the complete article and mandatory
//! visual dependencies before setting ArticleVersion::local_complete.

use std::collections::{BTreeMap, HashSet};

use wiki_model::wikipedia_title_key;

use crate::online_requests::WikipediaRequests;
use crate::online_search::OnlineSearchHit;
use crate::online_selection::{ArticleLocation, ArticleVersion};

const MAX_TITLE_BYTES: usize = 4096;
const MAX_QUERY_BYTES: usize = 512;
const MAX_RESULTS: usize = 100;
const MAX_ENTRIES: usize = 10_000_000;
const MAX_REDIRECTS: usize = 10_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTitleEntry {
    pub title: String,
    pub version: ArticleVersion,
}

/// A redirect from the SAME independently verified snapshot as the indexed
/// target. The page ID guards against rebinding a historical alias to a
/// different article after a rename or pack update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTitleRedirect {
    pub alias: String,
    pub canonical_title: String,
    pub target_page_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalSearchHit {
    pub hit: OnlineSearchHit,
    /// Explicit pack/save/cache provenance; never labeled online-current.
    pub version: ArticleVersion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalTitleIndexError {
    InvalidProject,
    InvalidTitle,
    InvalidVersion,
    ForeignProject,
    NotOfflineComplete,
    NonLocalVersion,
    DuplicatePage,
    DuplicateTitle,
    TooManyEntries,
    InvalidRedirect,
    UnknownRedirectTarget,
    ConflictingRedirect,
    TooManyRedirects,
    InvalidQuery,
    InvalidLimit,
}

#[derive(Clone, Debug)]
pub struct LocalTitleIndex {
    project: String,
    titles: BTreeMap<String, LocalSearchHit>,
    pages: BTreeMap<u64, String>,
    redirects: BTreeMap<String, String>,
}

fn safe_text(value: &str, max_bytes: usize) -> bool {
    crate::online_requests::safe_navigation_text(value, max_bytes)
}

impl LocalTitleIndex {
    /// Reject every conflicting/invalid entry before publishing an index.
    /// A title is unique within a project after first-letter/underscore
    /// normalization. All indexed articles must have independently verified
    /// offline-complete media, including temporary caches.
    pub fn new(
        project: &str,
        entries: impl IntoIterator<Item = LocalTitleEntry>,
    ) -> Result<Self, LocalTitleIndexError> {
        WikipediaRequests::new(project).map_err(|_| LocalTitleIndexError::InvalidProject)?;
        let mut titles = BTreeMap::new();
        let mut pages = BTreeMap::new();
        for entry in entries {
            if titles.len() >= MAX_ENTRIES {
                return Err(LocalTitleIndexError::TooManyEntries);
            }
            if !safe_text(&entry.title, MAX_TITLE_BYTES) {
                return Err(LocalTitleIndexError::InvalidTitle);
            }
            entry
                .version
                .key
                .validate()
                .map_err(|_| LocalTitleIndexError::InvalidVersion)?;
            if entry.version.revision_id == 0 || entry.version.revision_id > i64::MAX as u64 {
                return Err(LocalTitleIndexError::InvalidVersion);
            }
            if entry.version.key.project != project {
                return Err(LocalTitleIndexError::ForeignProject);
            }
            if entry.version.location == ArticleLocation::OnlineCurrent {
                return Err(LocalTitleIndexError::NonLocalVersion);
            }
            if !entry.version.local_complete {
                return Err(LocalTitleIndexError::NotOfflineComplete);
            }
            if pages
                .insert(entry.version.key.page_id, wikipedia_title_key(&entry.title))
                .is_some()
            {
                return Err(LocalTitleIndexError::DuplicatePage);
            }
            let key = wikipedia_title_key(&entry.title);
            if titles.contains_key(&key) {
                return Err(LocalTitleIndexError::DuplicateTitle);
            }
            let hit = OnlineSearchHit {
                key: entry.version.key.clone(),
                title: entry.title,
                namespace: 0,
            };
            titles.insert(
                key,
                LocalSearchHit {
                    hit,
                    version: entry.version,
                },
            );
        }
        Ok(Self {
            project: project.to_owned(),
            titles,
            pages,
            redirects: BTreeMap::new(),
        })
    }

    /// Attach independently verified redirect aliases without changing article
    /// provenance or adding duplicate records to prefix search results.
    ///
    /// Redirects must point directly to a canonical title present in this
    /// verified catalog; cycles, chains, stale target IDs and ambiguous title
    /// normalization fail closed. A partially validated mapping is never
    /// published because this method consumes the original index.
    pub fn with_redirects(
        mut self,
        redirects: impl IntoIterator<Item = LocalTitleRedirect>,
    ) -> Result<Self, LocalTitleIndexError> {
        for redirect in redirects {
            if self.redirects.len() >= MAX_REDIRECTS {
                return Err(LocalTitleIndexError::TooManyRedirects);
            }
            if !safe_text(&redirect.alias, MAX_TITLE_BYTES)
                || !safe_text(&redirect.canonical_title, MAX_TITLE_BYTES)
                || redirect.target_page_id == 0
                || redirect.target_page_id > i64::MAX as u64
            {
                return Err(LocalTitleIndexError::InvalidRedirect);
            }
            let alias_key = wikipedia_title_key(&redirect.alias);
            let target_key = wikipedia_title_key(&redirect.canonical_title);
            if alias_key == target_key
                || self.titles.contains_key(&alias_key)
                || self.redirects.contains_key(&alias_key)
            {
                return Err(LocalTitleIndexError::ConflictingRedirect);
            }
            let target = self
                .titles
                .get(&target_key)
                .ok_or(LocalTitleIndexError::UnknownRedirectTarget)?;
            if target.hit.key.page_id != redirect.target_page_id {
                return Err(LocalTitleIndexError::UnknownRedirectTarget);
            }
            self.redirects.insert(alias_key, target_key);
        }
        Ok(self)
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn len(&self) -> usize {
        self.titles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.titles.is_empty()
    }

    /// Exact normalized title resolution, for offline navigation without HTTP.
    pub fn resolve_title(
        &self,
        title: &str,
    ) -> Result<Option<LocalSearchHit>, LocalTitleIndexError> {
        if !safe_text(title, MAX_TITLE_BYTES) {
            return Err(LocalTitleIndexError::InvalidTitle);
        }
        let key = wikipedia_title_key(title);
        let canonical = self.redirects.get(&key).unwrap_or(&key);
        Ok(self.titles.get(canonical).cloned())
    }

    /// Stable page-ID navigation over the same independently verified offline
    /// records as exact title and redirect lookup. Never resolves an alias to
    /// a different page or treats an online HTML preview as installed media.
    pub fn resolve_page_id(&self, page_id: u64) -> Option<LocalSearchHit> {
        self.pages
            .get(&page_id)
            .and_then(|title| self.titles.get(title))
            .cloned()
    }

    /// Deterministic prefix lookup over canonical titles and verified direct
    /// redirects, not full-text or fuzzy search. A redirect is navigation
    /// metadata: hits always show the canonical title and verified local
    /// revision, never claim that the alias is a second installed article.
    ///
    /// Merge the two sorted key ranges without collecting all matches. Multiple
    /// aliases for one page are deduplicated before applying the result limit.
    pub fn search_prefix(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<LocalSearchHit>, LocalTitleIndexError> {
        if !safe_text(query, MAX_QUERY_BYTES) {
            return Err(LocalTitleIndexError::InvalidQuery);
        }
        if !(1..=MAX_RESULTS).contains(&limit) {
            return Err(LocalTitleIndexError::InvalidLimit);
        }
        let prefix = wikipedia_title_key(query);
        let mut titles = self
            .titles
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .peekable();
        let mut redirects = self
            .redirects
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .peekable();
        let mut hits = Vec::with_capacity(limit);
        let mut seen_pages = HashSet::new();

        while hits.len() < limit {
            let use_title = match (titles.peek(), redirects.peek()) {
                (None, None) => break,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (Some((title, _)), Some((alias, _))) => title <= alias,
            };
            let hit = if use_title {
                titles.next().map(|(_, hit)| hit)
            } else {
                redirects
                    .next()
                    .and_then(|(_, canonical)| self.titles.get(canonical))
            }
            .ok_or(LocalTitleIndexError::UnknownRedirectTarget)?;
            if seen_pages.insert(hit.hit.key.page_id) {
                hits.push(hit.clone());
            }
        }
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiki_model::ArticleKey;

    fn entry(page_id: u64, title: &str, location: ArticleLocation) -> LocalTitleEntry {
        LocalTitleEntry {
            title: title.into(),
            version: ArticleVersion {
                key: ArticleKey {
                    project: "enwiki".into(),
                    page_id,
                },
                revision_id: 100 + page_id,
                location,
                local_complete: true,
            },
        }
    }

    #[test]
    fn offline_prefix_and_exact_title_navigation_preserve_provenance() {
        let index = LocalTitleIndex::new(
            "enwiki",
            [
                entry(1, "Physics", ArticleLocation::InstalledPack),
                entry(
                    2,
                    "Physical chemistry",
                    ArticleLocation::ExplicitOfflineSave,
                ),
                entry(3, "Énergie", ArticleLocation::TemporaryCache),
                entry(4, "رياضيات", ArticleLocation::InstalledPack),
            ],
        )
        .unwrap();
        assert_eq!(index.len(), 4);
        assert_eq!(index.project(), "enwiki");
        assert_eq!(
            index
                .resolve_title("physics")
                .unwrap()
                .unwrap()
                .hit
                .key
                .page_id,
            1
        );
        assert_eq!(
            index
                .resolve_title("Physical_chemistry")
                .unwrap()
                .unwrap()
                .hit
                .key
                .page_id,
            2
        );
        assert!(index.resolve_title("Missing").unwrap().is_none());
        let hits = index.search_prefix("phys", 2).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].hit.title, "Physical chemistry");
        assert_eq!(hits[1].hit.title, "Physics");
        assert_eq!(hits[1].version.location, ArticleLocation::InstalledPack);
        assert_eq!(hits[1].version.revision_id, 101);
        assert_eq!(index.search_prefix("رياض", 5).unwrap().len(), 1);
        assert_eq!(index.search_prefix("none", 5).unwrap(), vec![]);
        assert_eq!(index.search_prefix("phys", 1).unwrap().len(), 1);
    }

    #[test]
    fn verified_page_id_lookup_matches_title_identity_and_never_follows_aliases() {
        let index = LocalTitleIndex::new(
            "enwiki",
            [
                entry(1, "Physics", ArticleLocation::InstalledPack),
                entry(2, "رياضيات", ArticleLocation::ExplicitOfflineSave),
            ],
        )
        .unwrap()
        .with_redirects([LocalTitleRedirect {
            alias: "Physical science".into(),
            canonical_title: "Physics".into(),
            target_page_id: 1,
        }])
        .unwrap();
        for (page_id, title) in [(1, "Physics"), (2, "رياضيات")] {
            let by_id = index.resolve_page_id(page_id).unwrap();
            assert_eq!(by_id.hit.title, title);
            assert_eq!(by_id.hit.key.page_id, page_id);
            assert_eq!(by_id, index.resolve_title(title).unwrap().unwrap());
        }
        assert_eq!(index.resolve_page_id(0), None);
        assert_eq!(index.resolve_page_id(3), None);
        assert_eq!(index.resolve_page_id(i64::MAX as u64 + 1), None);
        assert_eq!(index.resolve_page_id(1).unwrap().hit.title, "Physics");
    }

    #[test]
    fn refuses_incomplete_untrusted_or_ambiguous_local_catalogs() {
        let mut incomplete = entry(1, "Physics", ArticleLocation::TemporaryCache);
        incomplete.version.local_complete = false;
        assert!(matches!(
            LocalTitleIndex::new("enwiki", [incomplete]),
            Err(LocalTitleIndexError::NotOfflineComplete)
        ));
        assert!(matches!(
            LocalTitleIndex::new(
                "enwiki",
                [entry(1, "Physics", ArticleLocation::OnlineCurrent)]
            ),
            Err(LocalTitleIndexError::NonLocalVersion)
        ));
        let mut foreign = entry(1, "Physics", ArticleLocation::InstalledPack);
        foreign.version.key.project = "frwiki".into();
        assert!(matches!(
            LocalTitleIndex::new("enwiki", [foreign]),
            Err(LocalTitleIndexError::ForeignProject)
        ));
        assert!(matches!(
            LocalTitleIndex::new(
                "enwiki",
                [
                    entry(1, "Physics", ArticleLocation::InstalledPack),
                    entry(2, "physics", ArticleLocation::InstalledPack),
                ]
            ),
            Err(LocalTitleIndexError::DuplicateTitle)
        ));
        assert!(matches!(
            LocalTitleIndex::new(
                "enwiki",
                [
                    entry(1, "Physics", ArticleLocation::InstalledPack),
                    entry(1, "Chemistry", ArticleLocation::InstalledPack),
                ]
            ),
            Err(LocalTitleIndexError::DuplicatePage)
        ));
        for title in [" A ", "Bad\u{202e}Title", "Bad\nTitle", ""] {
            assert!(matches!(
                LocalTitleIndex::new("enwiki", [entry(1, title, ArticleLocation::InstalledPack)]),
                Err(LocalTitleIndexError::InvalidTitle)
            ));
        }
    }

    #[test]
    fn offline_catalog_rejects_noncharacters_and_invisible_markers() {
        for marker in [
            '\u{200b}',
            '\u{2028}',
            '\u{2060}',
            '\u{206a}',
            '\u{fdd0}',
            '\u{ffff}',
            '\u{10ffff}',
        ] {
            let title = format!("Physics{marker}");
            assert!(matches!(
                LocalTitleIndex::new("enwiki", [entry(1, &title, ArticleLocation::InstalledPack)]),
                Err(LocalTitleIndexError::InvalidTitle)
            ));
            let index = LocalTitleIndex::new(
                "enwiki",
                [entry(1, "Physics", ArticleLocation::InstalledPack)],
            )
            .unwrap();
            assert!(matches!(
                index.with_redirects([LocalTitleRedirect {
                    alias: title,
                    canonical_title: "Physics".into(),
                    target_page_id: 1,
                }]),
                Err(LocalTitleIndexError::InvalidRedirect)
            ));
        }
    }

    #[test]
    fn verified_offline_redirects_resolve_without_creating_duplicate_search_hits() {
        let index = LocalTitleIndex::new(
            "enwiki",
            [
                entry(1, "Physics", ArticleLocation::InstalledPack),
                entry(2, "رياضيات", ArticleLocation::ExplicitOfflineSave),
            ],
        )
        .unwrap()
        .with_redirects([
            LocalTitleRedirect {
                alias: "Physical_science".into(),
                canonical_title: "Physics".into(),
                target_page_id: 1,
            },
            LocalTitleRedirect {
                alias: "علم_الرياضيات".into(),
                canonical_title: "رياضيات".into(),
                target_page_id: 2,
            },
        ])
        .unwrap();
        let resolved = index.resolve_title("physical science").unwrap().unwrap();
        assert_eq!(resolved.hit.key.page_id, 1);
        assert_eq!(resolved.hit.title, "Physics");
        assert_eq!(resolved.version.revision_id, 101);
        assert_eq!(
            index
                .resolve_title("علم الرياضيات")
                .unwrap()
                .unwrap()
                .hit
                .key
                .page_id,
            2
        );
        assert_eq!(index.len(), 2);
        let aliases = index.search_prefix("Physical science", 5).unwrap();
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].hit.title, "Physics");
        assert_eq!(aliases[0].version.revision_id, 101);
    }

    #[test]
    fn offline_alias_prefix_search_deduplicates_before_limit() {
        let index = LocalTitleIndex::new(
            "enwiki",
            [
                entry(1, "Physics", ArticleLocation::InstalledPack),
                entry(
                    2,
                    "Physical chemistry",
                    ArticleLocation::ExplicitOfflineSave,
                ),
            ],
        )
        .unwrap()
        .with_redirects([
            LocalTitleRedirect {
                alias: "Physical sciences".into(),
                canonical_title: "Physics".into(),
                target_page_id: 1,
            },
            LocalTitleRedirect {
                alias: "Physics basics".into(),
                canonical_title: "Physics".into(),
                target_page_id: 1,
            },
            LocalTitleRedirect {
                alias: "Phys chem".into(),
                canonical_title: "Physical chemistry".into(),
                target_page_id: 2,
            },
        ])
        .unwrap();

        // The earliest alias must not consume a slot again when its canonical
        // title appears later in the sorted merge.
        let hits = index.search_prefix("phys", 2).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].hit.key.page_id, 2);
        assert_eq!(hits[1].hit.key.page_id, 1);
        assert_eq!(
            hits[0].version.location,
            ArticleLocation::ExplicitOfflineSave
        );
        assert_eq!(hits[1].version.location, ArticleLocation::InstalledPack);
        assert_eq!(
            index.search_prefix("Physics basics", 1).unwrap()[0]
                .hit
                .title,
            "Physics"
        );
        assert_eq!(index.search_prefix("Phys", 1).unwrap().len(), 1);
    }

    #[test]
    fn offline_redirects_reject_unverified_or_ambiguous_targets() {
        let base = || {
            LocalTitleIndex::new(
                "enwiki",
                [entry(1, "Physics", ArticleLocation::InstalledPack)],
            )
            .unwrap()
        };
        let alias = |name: &str, target: &str, page_id| LocalTitleRedirect {
            alias: name.into(),
            canonical_title: target.into(),
            target_page_id: page_id,
        };
        for bad in [
            alias("Physics", "Physics", 1),
            alias("physics", "Physics", 1),
        ] {
            assert!(matches!(
                base().with_redirects([bad]),
                Err(LocalTitleIndexError::ConflictingRedirect)
            ));
        }
        assert!(matches!(
            base().with_redirects([alias("Legacy", "Other", 1)]),
            Err(LocalTitleIndexError::UnknownRedirectTarget)
        ));
        assert!(matches!(
            base().with_redirects([alias("Legacy", "Physics", 2)]),
            Err(LocalTitleIndexError::UnknownRedirectTarget)
        ));
        for bad in ["", " Bad", "Bad\nAlias", "Bad\u{202e}Alias"] {
            assert!(matches!(
                base().with_redirects([alias(bad, "Physics", 1)]),
                Err(LocalTitleIndexError::InvalidRedirect)
            ));
        }
        assert!(matches!(
            base().with_redirects([alias("Alias", "Physics", 0)]),
            Err(LocalTitleIndexError::InvalidRedirect)
        ));
        assert!(matches!(
            base().with_redirects([alias("Alias", "Physics", 1), alias("alias", "Physics", 1)]),
            Err(LocalTitleIndexError::ConflictingRedirect)
        ));
        assert!(matches!(
            base().with_redirects([alias("First", "Second", 1), alias("Second", "Physics", 1)]),
            Err(LocalTitleIndexError::UnknownRedirectTarget)
        ));
    }

    #[test]
    fn invalid_queries_fail_closed() {
        let index = LocalTitleIndex::new(
            "enwiki",
            [entry(1, "Physics", ArticleLocation::InstalledPack)],
        )
        .unwrap();
        for query in ["", " ", " phys", "bad\u{2066}query", "bad\nquery"] {
            assert_eq!(
                index.search_prefix(query, 5),
                Err(LocalTitleIndexError::InvalidQuery)
            );
        }
        assert_eq!(
            index.search_prefix("phys", 0),
            Err(LocalTitleIndexError::InvalidLimit)
        );
        assert_eq!(
            index.search_prefix("phys", 101),
            Err(LocalTitleIndexError::InvalidLimit)
        );
        assert_eq!(
            index.resolve_title("bad\u{202e}title"),
            Err(LocalTitleIndexError::InvalidTitle)
        );
        assert_eq!(
            LocalTitleIndex::new("ENWIKI", Vec::<LocalTitleEntry>::new()).err(),
            Some(LocalTitleIndexError::InvalidProject)
        );
    }
}
