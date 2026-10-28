//! Bounded in-memory cache of successfully revision-pinned online HTML.
//! A cache hit requires fresh current-revision metadata with full identity
//! equality. Cached HTML is NOT a complete offline article or source receipt.

use std::collections::{BTreeMap, VecDeque};

use crate::online_article::{OnlineRenderedRevision, OnlineRevision};

type CacheKey = (String, u64, u64);
const MAX_ENTRIES: usize = 128;
const MAX_HTML_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineCacheError {
    InvalidLimit,
}

/// Exact-revision LRU. HTML bytes are capped independently of bounded metadata.
#[derive(Clone, Debug)]
pub struct OnlineRevisionCache {
    entries: BTreeMap<CacheKey, OnlineRenderedRevision>,
    recency: VecDeque<CacheKey>,
    html_bytes: usize,
    max_entries: usize,
    max_html_bytes: usize,
}

impl Default for OnlineRevisionCache {
    fn default() -> Self {
        Self::new(32, 16 * 1024 * 1024).expect("valid cache defaults")
    }
}

impl OnlineRevisionCache {
    pub fn new(max_entries: usize, max_html_bytes: usize) -> Result<Self, OnlineCacheError> {
        if !(1..=MAX_ENTRIES).contains(&max_entries)
            || !(1..=MAX_HTML_BYTES).contains(&max_html_bytes)
        {
            return Err(OnlineCacheError::InvalidLimit);
        }
        Ok(Self {
            entries: BTreeMap::new(),
            recency: VecDeque::new(),
            html_bytes: 0,
            max_entries,
            max_html_bytes,
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn cached_html_bytes(&self) -> usize {
        self.html_bytes
    }

    fn key(revision: &OnlineRevision) -> CacheKey {
        (
            revision.key.project.clone(),
            revision.key.page_id,
            revision.revision_id,
        )
    }

    fn remove(&mut self, key: &CacheKey) {
        if let Some(old) = self.entries.remove(key) {
            self.html_bytes -= old.html.len();
        }
        self.recency.retain(|existing| existing != key);
    }

    /// Only called after a NEW verified current-revision metadata response.
    /// Full identity equality prevents reuse after title/timestamp drift.
    pub(crate) fn get(&mut self, revision: &OnlineRevision) -> Option<OnlineRenderedRevision> {
        let key = Self::key(revision);
        let record = self.entries.get(&key)?;
        if &record.revision != revision {
            return None;
        }
        let record = record.clone();
        self.recency.retain(|existing| existing != &key);
        self.recency.push_back(key);
        Some(record)
    }

    /// Only the successful exact-revision HTML parser may insert an article.
    pub(crate) fn insert_verified(&mut self, article: OnlineRenderedRevision) {
        let size = article.html.len();
        if size > self.max_html_bytes {
            return;
        }
        let key = Self::key(&article.revision);
        self.remove(&key);
        self.html_bytes += size;
        self.recency.push_back(key.clone());
        self.entries.insert(key, article);
        while self.entries.len() > self.max_entries || self.html_bytes > self.max_html_bytes {
            if let Some(oldest) = self.recency.pop_front() {
                if let Some(old) = self.entries.remove(&oldest) {
                    self.html_bytes -= old.html.len();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiki_model::ArticleKey;

    fn rendered(page: u64, revision: u64, html: &str) -> OnlineRenderedRevision {
        OnlineRenderedRevision {
            revision: OnlineRevision {
                key: ArticleKey {
                    project: "enwiki".into(),
                    page_id: page,
                },
                title: format!("Article {page}"),
                namespace: 0,
                revision_id: revision,
                timestamp: "2026-10-27T00:00:00Z".into(),
            },
            html: html.into(),
        }
    }

    #[test]
    fn lru_eviction_requires_exact_revision_metadata() {
        let mut cache = OnlineRevisionCache::new(2, 6).unwrap();
        let first = rendered(1, 10, "aaa");
        let second = rendered(2, 20, "bbb");
        cache.insert_verified(first.clone());
        cache.insert_verified(second.clone());
        assert_eq!(cache.cached_html_bytes(), 6);
        assert_eq!(cache.get(&first.revision), Some(first.clone()));
        let third = rendered(3, 30, "ccc");
        cache.insert_verified(third.clone());
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&second.revision), None);
        assert_eq!(cache.get(&first.revision), Some(first.clone()));
        assert_eq!(cache.get(&third.revision), Some(third));
        let mut changed = first.revision.clone();
        changed.title = "Renamed".into();
        assert_eq!(cache.get(&changed), None);
        changed = first.revision.clone();
        changed.timestamp = "2026-10-28T00:00:00Z".into();
        assert_eq!(cache.get(&changed), None);
        assert_eq!(cache.get(&first.revision), Some(first));
    }

    #[test]
    fn invalid_limits_and_oversized_renders_are_not_cached() {
        assert!(matches!(
            OnlineRevisionCache::new(0, 1),
            Err(OnlineCacheError::InvalidLimit)
        ));
        assert!(matches!(
            OnlineRevisionCache::new(1, 0),
            Err(OnlineCacheError::InvalidLimit)
        ));
        assert!(matches!(
            OnlineRevisionCache::new(MAX_ENTRIES + 1, 1),
            Err(OnlineCacheError::InvalidLimit)
        ));
        assert!(matches!(
            OnlineRevisionCache::new(1, MAX_HTML_BYTES + 1),
            Err(OnlineCacheError::InvalidLimit)
        ));
        let mut cache = OnlineRevisionCache::new(1, 3).unwrap();
        cache.insert_verified(rendered(1, 10, "too big"));
        assert!(cache.is_empty());
        cache.insert_verified(rendered(2, 20, "abc"));
        assert_eq!(cache.cached_html_bytes(), 3);
        cache.insert_verified(rendered(3, 30, "xy"));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.cached_html_bytes(), 2);
    }
}
