//! Offline article availability policy independent of UI and network transport.
//! This describes known local ownership only; it never initiates a fetch or
//! authorizes deletion of a shared content-addressed object.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArticleAvailability {
    /// A verified installed topic pack owns the article.
    PackOwned,
    /// Explicitly saved by the user, independent of temporary cache eviction.
    SavedOffline,
    /// Temporary cache copy; may be evicted by cache policy.
    Cached,
    /// No local bytes; a separate explicit online fetch is required.
    RequiresOnlineFetch,
    /// Offline and no local copy. The reader should show a missing-content state.
    MissingOffline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArticleCopies {
    /// Number of installed pack owners of this article's verified object.
    pub pack_owners: u32,
    pub saved: bool,
    pub cached: bool,
}

impl ArticleCopies {
    /// Precedence is pack-owned, user-pinned, cached, then online fetch.
    /// An offline lookup never requests network traffic.
    pub fn availability(self, network_allowed: bool) -> ArticleAvailability {
        if self.pack_owners > 0 {
            ArticleAvailability::PackOwned
        } else if self.saved {
            ArticleAvailability::SavedOffline
        } else if self.cached {
            ArticleAvailability::Cached
        } else if network_allowed {
            ArticleAvailability::RequiresOnlineFetch
        } else {
            ArticleAvailability::MissingOffline
        }
    }

    /// Removing an individually saved copy must never remove a pack-owned
    /// shared object. This is a deletion guard, not a garbage collector.
    pub fn may_delete_unowned_saved_object(self) -> bool {
        self.saved && self.pack_owners == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_and_user_pins_take_precedence_over_cache_and_network() {
        let copies = ArticleCopies {
            pack_owners: 2,
            saved: true,
            cached: true,
        };
        assert_eq!(copies.availability(false), ArticleAvailability::PackOwned);
        assert_eq!(copies.availability(true), ArticleAvailability::PackOwned);
        assert!(!copies.may_delete_unowned_saved_object());

        let saved = ArticleCopies {
            pack_owners: 0,
            saved: true,
            cached: true,
        };
        assert_eq!(saved.availability(false), ArticleAvailability::SavedOffline);
        assert!(saved.may_delete_unowned_saved_object());
    }

    #[test]
    fn cached_and_missing_articles_do_not_silently_fetch_offline() {
        let cached = ArticleCopies {
            cached: true,
            ..ArticleCopies::default()
        };
        assert_eq!(cached.availability(false), ArticleAvailability::Cached);
        assert!(!cached.may_delete_unowned_saved_object());

        let absent = ArticleCopies::default();
        assert_eq!(
            absent.availability(false),
            ArticleAvailability::MissingOffline
        );
        assert_eq!(
            absent.availability(true),
            ArticleAvailability::RequiresOnlineFetch
        );
        assert!(!absent.may_delete_unowned_saved_object());
    }
}
