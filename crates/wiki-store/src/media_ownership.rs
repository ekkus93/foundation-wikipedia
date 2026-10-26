//! Durable ownership accounting for deduplicated media bytes (partial STORE-002).
//!
//! The caller must separately persist and reverify content-addressed bytes.
//! This registry records verified byte identity and attribution notices; it
//! does not certify an object as installed or authorize deletion of its bytes.

use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub enum MediaRegistryError {
    Sql(rusqlite::Error),
    InvalidInput,
    ConflictingSize,
    ConflictingInventory,
    ConflictingMime,
    UnknownObject,
    TooManyCandidates,
    TooManyNotices,
    TooManyOwners,
}

impl From<rusqlite::Error> for MediaRegistryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerKind {
    Pack,
    Cache,
    UserPin,
}

impl OwnerKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pack => "pack",
            Self::Cache => "cache",
            Self::UserPin => "pin",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaNotice {
    pub mime: String,
    pub source_url: String,
    pub creator: String,
    pub license: String,
}

impl MediaNotice {
    pub(crate) fn validate(&self) -> bool {
        safe_media_mime(&self.mime)
            && safe_media_source_url(&self.source_url)
            && safe_media_notice_text(&self.creator)
            && safe_media_notice_text(&self.license)
            && self.mime.len() <= 255
            && self.source_url.len() <= 4096
    }
}

/// Attribution text is untrusted display metadata, not HTML. Reject control
/// characters and bidi direction overrides/isolates that can spoof creators or
/// licenses in a future offline notice UI. The UI still must escape the text.
fn safe_media_notice_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 4096
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

/// Strict MIME type/subtype syntax keeps renderer and export metadata free
/// of parameters, whitespace, or injected HTTP header delimiters. This does
/// not validate the actual media bytes or decide whether a type is safe to render.
fn safe_media_mime(value: &str) -> bool {
    let Some((major, subtype)) = value.split_once('/') else {
        return false;
    };
    !major.is_empty()
        && !subtype.is_empty()
        && major
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && subtype
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'+'))
}

/// Attribution links must be absolute web URLs, never executable schemes,
/// whitespace-bearing browser ambiguities, or credential-bearing authorities.
fn safe_media_source_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    if url
        .chars()
        .any(|character| character.is_whitespace() || character.is_control() || character == '\\')
    {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') || authority.contains('%') {
        return false;
    }
    let port = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some((address, suffix)) = bracketed.split_once(']') else {
            return false;
        };
        if address.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        if suffix.is_empty() {
            None
        } else {
            let Some(port) = suffix.strip_prefix(':') else {
                return false;
            };
            Some(port)
        }
    } else {
        let (hostname, port) = match authority.rsplit_once(':') {
            Some((hostname, port)) => (hostname, Some(port)),
            None => (authority, None),
        };
        if hostname.len() > 253
            || !hostname.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && label.as_bytes()[0].is_ascii_alphanumeric()
                    && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
        {
            return false;
        }
        port
    };
    port.is_none_or(|port| {
        !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|number| number > 0)
    })
}

pub struct MediaRegistry {
    conn: Connection,
}

impl MediaRegistry {
    pub fn open(path: &std::path::Path) -> Result<Self, MediaRegistryError> {
        Self::initialize(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, MediaRegistryError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(conn: Connection) -> Result<Self, MediaRegistryError> {
        conn.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS media_objects (
               digest TEXT PRIMARY KEY,
               bytes INTEGER NOT NULL CHECK(bytes > 0)
             );
             CREATE TABLE IF NOT EXISTS media_notices (
               digest TEXT NOT NULL REFERENCES media_objects(digest) ON DELETE CASCADE,
               mime TEXT NOT NULL,
               source_url TEXT NOT NULL,
               creator TEXT NOT NULL,
               license TEXT NOT NULL,
               PRIMARY KEY(digest,mime,source_url,creator,license)
             );
             CREATE TABLE IF NOT EXISTS media_owners (
               digest TEXT NOT NULL REFERENCES media_objects(digest) ON DELETE CASCADE,
               kind TEXT NOT NULL CHECK(kind IN ('pack','cache','pin')),
               owner_id TEXT NOT NULL,
               PRIMARY KEY(digest,kind,owner_id)
             );",
        )?;
        Ok(Self { conn })
    }

    /// Register a digest derived from actual supplied bytes, never from a
    /// caller-provided hash. Identical bytes may have multiple attribution
    /// notices without duplicating object ownership.
    pub fn register_verified_bytes(
        &mut self,
        content: &[u8],
        notice: &MediaNotice,
    ) -> Result<String, MediaRegistryError> {
        if content.is_empty() || !notice.validate() {
            return Err(MediaRegistryError::InvalidInput);
        }
        let bytes = i64::try_from(content.len()).map_err(|_| MediaRegistryError::InvalidInput)?;
        let digest = format!("{:x}", Sha256::digest(content));
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO media_objects(digest,bytes) VALUES (?1,?2)",
            params![digest, bytes],
        )?;
        let actual: i64 = tx.query_row(
            "SELECT bytes FROM media_objects WHERE digest=?1",
            params![digest],
            |row| row.get(0),
        )?;
        if actual != bytes {
            return Err(MediaRegistryError::ConflictingSize);
        }
        tx.execute(
            "INSERT OR IGNORE INTO media_notices(digest,mime,source_url,creator,license)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                digest,
                notice.mime,
                notice.source_url,
                notice.creator,
                notice.license
            ],
        )?;
        tx.commit()?;
        Ok(digest)
    }
    /// Register staged media with an exact, immutable inventory for this owner.
    /// Reinstalling the same inventory is idempotent; a different inventory
    /// must be rejected, never silently appended. An IMMEDIATE transaction
    /// serializes competing batch installers before the ownership comparison.
    /// The caller must persist and verify every object before this operation.
    pub fn register_verified_owned_batch(
        &mut self,
        assets: &[(&[u8], &MediaNotice)],
        kind: OwnerKind,
        owner_id: &str,
    ) -> Result<Vec<String>, MediaRegistryError> {
        if !valid_owner_id(owner_id)
            || assets.is_empty()
            || assets.len() > 100_000
            || assets.iter().any(|(bytes, notice)| {
                bytes.is_empty() || bytes.len() > 64 * 1024 * 1024 || !notice.validate()
            })
        {
            return Err(MediaRegistryError::InvalidInput);
        }
        let mut expected = BTreeMap::new();
        for &(content, _) in assets {
            expected.insert(
                format!("{:x}", Sha256::digest(content)),
                content.len() as u64,
            );
        }
        // Acquire SQLite's writer reservation before inspecting the owner.
        // A second connection cannot both observe the old inventory and
        // append a different one while this transaction is in progress.
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = {
            let mut stmt = tx.prepare(
                "SELECT o.digest, o.bytes FROM media_owners r
                 JOIN media_objects o ON o.digest=r.digest
                 WHERE r.kind=?1 AND r.owner_id=?2 ORDER BY o.digest",
            )?;
            let rows = stmt.query_map(params![kind.as_str(), owner_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            let mut existing = BTreeMap::new();
            for row in rows {
                let (digest, size) = row?;
                existing.insert(
                    digest,
                    u64::try_from(size).map_err(|_| MediaRegistryError::InvalidInput)?,
                );
            }
            existing
        };
        if !existing.is_empty() && existing != expected {
            return Err(MediaRegistryError::ConflictingInventory);
        }
        let mut digests = Vec::with_capacity(assets.len());
        for &(content, notice) in assets {
            let bytes =
                i64::try_from(content.len()).map_err(|_| MediaRegistryError::InvalidInput)?;
            let digest = format!("{:x}", Sha256::digest(content));
            tx.execute(
                "INSERT OR IGNORE INTO media_objects(digest,bytes) VALUES (?1,?2)",
                params![digest, bytes],
            )?;
            let actual: i64 = tx.query_row(
                "SELECT bytes FROM media_objects WHERE digest=?1",
                params![digest],
                |row| row.get(0),
            )?;
            if actual != bytes {
                return Err(MediaRegistryError::ConflictingSize);
            }
            tx.execute(
                "INSERT OR IGNORE INTO media_notices(digest,mime,source_url,creator,license)
                 VALUES (?1,?2,?3,?4,?5)",
                params![
                    digest,
                    notice.mime,
                    notice.source_url,
                    notice.creator,
                    notice.license
                ],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO media_owners(digest,kind,owner_id) VALUES (?1,?2,?3)",
                params![digest, kind.as_str(), owner_id],
            )?;
            digests.push(digest);
        }
        tx.commit()?;
        Ok(digests)
    }

    pub fn add_owner(
        &self,
        digest: &str,
        kind: OwnerKind,
        owner_id: &str,
    ) -> Result<(), MediaRegistryError> {
        if !valid_digest(digest) || !valid_owner_id(owner_id) {
            return Err(MediaRegistryError::InvalidInput);
        }
        let count: Option<i64> = self
            .conn
            .query_row(
                "SELECT bytes FROM media_objects WHERE digest=?1",
                params![digest],
                |row| row.get(0),
            )
            .optional()?;
        if count.is_none() {
            return Err(MediaRegistryError::UnknownObject);
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO media_owners(digest,kind,owner_id) VALUES (?1,?2,?3)",
            params![digest, kind.as_str(), owner_id],
        )?;
        Ok(())
    }

    pub fn remove_owner(
        &self,
        digest: &str,
        kind: OwnerKind,
        owner_id: &str,
    ) -> Result<(), MediaRegistryError> {
        if !valid_digest(digest) || !valid_owner_id(owner_id) {
            return Err(MediaRegistryError::InvalidInput);
        }
        self.conn.execute(
            "DELETE FROM media_owners WHERE digest=?1 AND kind=?2 AND owner_id=?3",
            params![digest, kind.as_str(), owner_id],
        )?;
        Ok(())
    }

    /// Release every reference held by one owner in one SQLite transaction.
    /// The returned digests are *not* deletion authorization: another pack,
    /// cache or user pin may still own them, and byte GC must recheck under
    /// its own concurrency-safe protocol before deleting any object.
    pub fn remove_all_for_owner(
        &mut self,
        kind: OwnerKind,
        owner_id: &str,
    ) -> Result<Vec<String>, MediaRegistryError> {
        if !valid_owner_id(owner_id) {
            return Err(MediaRegistryError::InvalidInput);
        }
        let tx = self.conn.transaction()?;
        let digests = {
            let mut stmt = tx.prepare(
                "SELECT digest FROM media_owners
                 WHERE kind=?1 AND owner_id=?2 ORDER BY digest",
            )?;
            let rows = stmt.query_map(params![kind.as_str(), owner_id], |row| row.get(0))?;
            rows.collect::<Result<Vec<String>, _>>()?
        };
        tx.execute(
            "DELETE FROM media_owners WHERE kind=?1 AND owner_id=?2",
            params![kind.as_str(), owner_id],
        )?;
        tx.commit()?;
        Ok(digests)
    }

    /// Candidate digests for a separate, crash-safe byte GC transaction.
    /// This query never deletes objects or their attribution records.
    pub fn unowned_digests(&self) -> Result<Vec<String>, MediaRegistryError> {
        let mut stmt = self.conn.prepare(
            "SELECT o.digest FROM media_objects o
             WHERE NOT EXISTS (
               SELECT 1 FROM media_owners r WHERE r.digest=o.digest
             ) ORDER BY o.digest",
        )?;
        let hashes = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(hashes)
    }

    /// Bounded, read-only snapshot of registered objects with no pack, cache,
    /// or user-pin owners. This is advisory GC input, never deletion authority:
    /// ownership can change immediately after the query, and an interrupted
    /// installer may have stored bytes without creating a registry row.
    pub fn unowned_media_candidates(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, u64)>, MediaRegistryError> {
        if !(1..=10_000).contains(&limit) {
            return Err(MediaRegistryError::InvalidInput);
        }
        let mut stmt = self.conn.prepare(
            "SELECT o.digest, o.bytes FROM media_objects o
             WHERE NOT EXISTS (
               SELECT 1 FROM media_owners r WHERE r.digest=o.digest
             ) ORDER BY o.digest LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![(limit + 1) as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut candidates = Vec::new();
        for row in rows {
            let (digest, bytes) = row?;
            if !valid_digest(&digest) || !(1..=64 * 1024 * 1024).contains(&bytes) {
                return Err(MediaRegistryError::InvalidInput);
            }
            candidates.push((digest, bytes as u64));
            if candidates.len() > limit {
                return Err(MediaRegistryError::TooManyCandidates);
            }
        }
        Ok(candidates)
    }

    /// Bounded, deterministic inventory of *all* registered objects, including
    /// pack/cache/pin-owned media. The unowned GC candidate query is insufficient
    /// for an integrity audit: corruption of a still-owned figure must be caught.
    /// This is a read-only SQLite snapshot, not deletion authorization.
    pub fn registered_media_candidates(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, u64)>, MediaRegistryError> {
        if !(1..=10_000).contains(&limit) {
            return Err(MediaRegistryError::InvalidInput);
        }
        let mut stmt = self
            .conn
            .prepare("SELECT digest, bytes FROM media_objects ORDER BY digest LIMIT ?1")?;
        let rows = stmt.query_map(params![(limit + 1) as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut objects = Vec::new();
        for row in rows {
            let (digest, bytes) = row?;
            if !valid_digest(&digest) || !(1..=64 * 1024 * 1024).contains(&bytes) {
                return Err(MediaRegistryError::InvalidInput);
            }
            objects.push((digest, bytes as u64));
            if objects.len() > limit {
                return Err(MediaRegistryError::TooManyCandidates);
            }
        }
        Ok(objects)
    }

    /// Read-only registered-object lookup for advisory filesystem orphan
    /// audits. A row can be added concurrently after this observation.
    pub fn registered_digest_size(&self, digest: &str) -> Result<Option<u64>, MediaRegistryError> {
        if !valid_digest(digest) {
            return Err(MediaRegistryError::InvalidInput);
        }
        let bytes: Option<i64> = self
            .conn
            .query_row(
                "SELECT bytes FROM media_objects WHERE digest=?1",
                params![digest],
                |row| row.get(0),
            )
            .optional()?;
        bytes
            .map(|size| {
                if !(1..=64 * 1024 * 1024).contains(&size) {
                    return Err(MediaRegistryError::InvalidInput);
                }
                Ok(size as u64)
            })
            .transpose()
    }

    /// Efficient owner-scoped digest lookup for on-demand asset reads. This
    /// checks a registered claim, not the bytes, manifest, or media license.
    pub fn owned_digest_size(
        &self,
        kind: OwnerKind,
        owner_id: &str,
        digest: &str,
    ) -> Result<Option<u64>, MediaRegistryError> {
        if !valid_owner_id(owner_id) || !valid_digest(digest) {
            return Err(MediaRegistryError::InvalidInput);
        }
        let bytes: Option<i64> = self
            .conn
            .query_row(
                "SELECT o.bytes FROM media_objects o
                 JOIN media_owners r ON r.digest=o.digest
                 WHERE r.kind=?1 AND r.owner_id=?2 AND o.digest=?3",
                params![kind.as_str(), owner_id, digest],
                |row| row.get(0),
            )
            .optional()?;
        bytes
            .map(|size| u64::try_from(size).map_err(|_| MediaRegistryError::InvalidInput))
            .transpose()
    }

    /// Resolve one pack-owned object's unique declared MIME, if unambiguous.
    /// Neither this attribution metadata nor a registered digest proves the
    /// media bytes' actual type or upstream authenticity.
    pub fn owned_digest_mime(
        &self,
        kind: OwnerKind,
        owner_id: &str,
        digest: &str,
    ) -> Result<Option<String>, MediaRegistryError> {
        if self.owned_digest_size(kind, owner_id, digest)?.is_none() {
            return Ok(None);
        }
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT mime FROM media_notices WHERE digest=?1 ORDER BY mime LIMIT 2",
        )?;
        let mut rows = statement.query(params![digest])?;
        let first = rows.next()?.ok_or(MediaRegistryError::InvalidInput)?;
        let mime: String = first.get(0)?;
        if rows.next()?.is_some() {
            return Err(MediaRegistryError::ConflictingMime);
        }
        Ok(Some(mime))
    }

    /// Return every distinct attribution notice for a verified owner-scoped object.
    /// This preserves creator/source/license records without treating caller-supplied
    /// notice fields as authenticated Wikimedia provenance.
    pub fn owned_digest_notices(
        &self,
        kind: OwnerKind,
        owner_id: &str,
        digest: &str,
    ) -> Result<Vec<MediaNotice>, MediaRegistryError> {
        if self.owned_digest_size(kind, owner_id, digest)?.is_none() {
            return Ok(Vec::new());
        }
        let mut statement = self.conn.prepare(
            "SELECT mime, source_url, creator, license FROM media_notices
             WHERE digest=?1 ORDER BY source_url, creator, license, mime",
        )?;
        let rows = statement.query_map(params![digest], |row| {
            Ok(MediaNotice {
                mime: row.get(0)?,
                source_url: row.get(1)?,
                creator: row.get(2)?,
                license: row.get(3)?,
            })
        })?;
        let mut notices = Vec::new();
        for notice in rows {
            let notice = notice?;
            if !notice.validate() {
                return Err(MediaRegistryError::InvalidInput);
            }
            notices.push(notice);
        }
        if notices.is_empty() {
            return Err(MediaRegistryError::InvalidInput);
        }
        Ok(notices)
    }

    /// Return only this owner's registered digest/byte claims, in stable order.
    /// These claims are not proof that the objects still exist on disk.
    pub fn owned_media(
        &self,
        kind: OwnerKind,
        owner_id: &str,
    ) -> Result<Vec<(String, u64)>, MediaRegistryError> {
        if !valid_owner_id(owner_id) {
            return Err(MediaRegistryError::InvalidInput);
        }
        let mut stmt = self.conn.prepare(
            "SELECT o.digest, o.bytes FROM media_objects o
             JOIN media_owners r ON r.digest=o.digest
             WHERE r.kind=?1 AND r.owner_id=?2 ORDER BY o.digest",
        )?;
        let rows = stmt.query_map(params![kind.as_str(), owner_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut entries = Vec::new();
        for row in rows {
            let (digest, size) = row?;
            if !valid_digest(&digest) || size <= 0 {
                return Err(MediaRegistryError::InvalidInput);
            }
            entries.push((
                digest,
                u64::try_from(size).map_err(|_| MediaRegistryError::InvalidInput)?,
            ));
        }
        Ok(entries)
    }

    /// Check every persisted attribution notice for a registered digest,
    /// including media without a current owner. An object with no valid notice
    /// is not safe to present as an attributed offline visual. Bound rows to
    /// prevent a corrupted registry from exhausting memory or audit time.
    /// This is a read-only SQLite observation, not publisher authentication.
    pub fn audit_registered_notices(
        &self,
        digest: &str,
        max_notices: usize,
    ) -> Result<usize, MediaRegistryError> {
        if !valid_digest(digest) || !(1..=1024).contains(&max_notices) {
            return Err(MediaRegistryError::InvalidInput);
        }
        let mut stmt = self.conn.prepare(
            "SELECT mime, source_url, creator, license FROM media_notices
             WHERE digest=?1 ORDER BY mime, source_url, creator, license LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![digest, (max_notices + 1) as i64], |row| {
            Ok(MediaNotice {
                mime: row.get(0)?,
                source_url: row.get(1)?,
                creator: row.get(2)?,
                license: row.get(3)?,
            })
        })?;
        let mut count = 0usize;
        let mut mime: Option<String> = None;
        for notice in rows {
            count += 1;
            if count > max_notices {
                return Err(MediaRegistryError::TooManyNotices);
            }
            let notice = notice?;
            if !notice.validate() {
                return Err(MediaRegistryError::InvalidInput);
            }
            if let Some(previous) = &mime {
                if previous != &notice.mime {
                    return Err(MediaRegistryError::ConflictingMime);
                }
            } else {
                mime = Some(notice.mime);
            }
        }
        if count == 0 {
            return Err(MediaRegistryError::InvalidInput);
        }
        Ok(count)
    }

    /// Validate bounded pack/cache/pin owner claims for one registered digest.
    /// This read-only check is not authorization to delete unowned bytes.
    pub fn audit_registered_owners(
        &self,
        digest: &str,
        max_owners: usize,
    ) -> Result<usize, MediaRegistryError> {
        if !valid_digest(digest) || !(1..=1024).contains(&max_owners) {
            return Err(MediaRegistryError::InvalidInput);
        }
        let mut stmt = self.conn.prepare(
            "SELECT kind, owner_id FROM media_owners
             WHERE digest=?1 ORDER BY kind, owner_id LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![digest, (max_owners + 1) as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut count = 0usize;
        for row in rows {
            count += 1;
            if count > max_owners {
                return Err(MediaRegistryError::TooManyOwners);
            }
            let (kind, owner_id) = row?;
            if !matches!(kind.as_str(), "pack" | "cache" | "pin")
                || !valid_owner_id(&owner_id)
            {
                return Err(MediaRegistryError::InvalidInput);
            }
        }
        Ok(count)
    }

    /// Detect dangling owners/notices that object-scoped integrity checks miss.
    pub fn audit_unregistered_references(&self) -> Result<(), MediaRegistryError> {
        let dangling: i64 = self.conn.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM media_owners r
               WHERE NOT EXISTS (SELECT 1 FROM media_objects o WHERE o.digest=r.digest)
             ) OR EXISTS(
               SELECT 1 FROM media_notices n
               WHERE NOT EXISTS (SELECT 1 FROM media_objects o WHERE o.digest=n.digest)
             )",
            [],
            |row| row.get(0),
        )?;
        if dangling != 0 {
            return Err(MediaRegistryError::InvalidInput);
        }
        Ok(())
    }

    pub fn notice_count(&self, digest: &str) -> Result<u64, MediaRegistryError> {
        if !valid_digest(digest) {
            return Err(MediaRegistryError::InvalidInput);
        }
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM media_notices WHERE digest=?1",
            params![digest],
            |row| row.get(0),
        )?;
        u64::try_from(count).map_err(|_| MediaRegistryError::InvalidInput)
    }
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn valid_owner_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 255
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(creator: &str) -> MediaNotice {
        MediaNotice {
            mime: "image/png".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Example.png".into(),
            creator: creator.into(),
            license: "CC BY-SA 4.0".into(),
        }
    }

    #[test]
    fn scoped_digest_query_checks_owner_kind_and_revocation() {
        let mut registry = MediaRegistry::in_memory().unwrap();
        let digest = registry
            .register_verified_bytes(b"figure", &notice("A"))
            .unwrap();
        assert_eq!(
            registry
                .owned_digest_size(OwnerKind::Pack, "physics", &digest)
                .unwrap(),
            None
        );
        registry
            .add_owner(&digest, OwnerKind::Cache, "physics")
            .unwrap();
        assert_eq!(
            registry
                .owned_digest_size(OwnerKind::Pack, "physics", &digest)
                .unwrap(),
            None
        );
        registry
            .add_owner(&digest, OwnerKind::Pack, "physics")
            .unwrap();
        assert_eq!(
            registry
                .owned_digest_size(OwnerKind::Pack, "physics", &digest)
                .unwrap(),
            Some(6)
        );
        assert_eq!(
            registry
                .owned_digest_size(OwnerKind::Pack, "math", &digest)
                .unwrap(),
            None
        );
        assert!(matches!(
            registry.owned_digest_size(OwnerKind::Pack, "../physics", &digest),
            Err(MediaRegistryError::InvalidInput)
        ));
        registry
            .remove_owner(&digest, OwnerKind::Pack, "physics")
            .unwrap();
        assert_eq!(
            registry
                .owned_digest_size(OwnerKind::Pack, "physics", &digest)
                .unwrap(),
            None
        );
    }

    #[test]
    fn owned_digest_notices_preserve_creators_with_owner_scoping() {
        let mut registry = MediaRegistry::in_memory().unwrap();
        let alice = notice("Alice");
        let bob = notice("Bob");
        let digest = registry.register_verified_bytes(b"figure", &alice).unwrap();
        registry.register_verified_bytes(b"figure", &bob).unwrap();
        assert!(registry
            .owned_digest_notices(OwnerKind::Pack, "math", &digest)
            .unwrap()
            .is_empty());
        registry
            .add_owner(&digest, OwnerKind::Cache, "math")
            .unwrap();
        assert!(registry
            .owned_digest_notices(OwnerKind::Pack, "math", &digest)
            .unwrap()
            .is_empty());
        registry
            .add_owner(&digest, OwnerKind::Pack, "math")
            .unwrap();
        assert_eq!(
            registry
                .owned_digest_notices(OwnerKind::Pack, "math", &digest)
                .unwrap(),
            vec![alice, bob]
        );
        assert!(registry
            .owned_digest_notices(OwnerKind::Pack, "other", &digest)
            .unwrap()
            .is_empty());
        registry
            .remove_owner(&digest, OwnerKind::Pack, "math")
            .unwrap();
        assert!(registry
            .owned_digest_notices(OwnerKind::Pack, "math", &digest)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn owned_digest_mime_requires_pack_scope_and_rejects_conflicts() {
        let mut registry = MediaRegistry::in_memory().unwrap();
        let digest = registry
            .register_verified_bytes(b"asset", &notice("Alice"))
            .unwrap();
        assert_eq!(
            registry
                .owned_digest_mime(OwnerKind::Pack, "math", &digest)
                .unwrap(),
            None
        );
        registry
            .add_owner(&digest, OwnerKind::Pack, "math")
            .unwrap();
        assert_eq!(
            registry
                .owned_digest_mime(OwnerKind::Pack, "math", &digest)
                .unwrap(),
            Some("image/png".into())
        );
        let mut conflicting = notice("Bob");
        conflicting.mime = "image/svg+xml".into();
        registry
            .register_verified_bytes(b"asset", &conflicting)
            .unwrap();
        assert!(matches!(
            registry.owned_digest_mime(OwnerKind::Pack, "math", &digest),
            Err(MediaRegistryError::ConflictingMime)
        ));
        registry
            .remove_owner(&digest, OwnerKind::Pack, "math")
            .unwrap();
        assert_eq!(
            registry
                .owned_digest_mime(OwnerKind::Pack, "math", &digest)
                .unwrap(),
            None
        );
    }

    #[test]
    fn invalid_mime_types_cannot_enter_durable_media_attribution() {
        let mut registry = MediaRegistry::in_memory().unwrap();
        for mime in [
            "",
            "image",
            "image/",
            "/png",
            "image/png; charset=utf-8",
            "image/png\r\nX-Injected: yes",
            "image/png ",
            "image/\tjpeg",
            "image\\png",
            "image/svg+xml\0",
        ] {
            let mut bad = notice("Author");
            bad.mime = mime.into();
            assert!(!bad.validate(), "accepted unsafe MIME: {mime:?}");
            assert!(matches!(
                registry.register_verified_bytes(b"diagram", &bad),
                Err(MediaRegistryError::InvalidInput)
            ));
        }
        let mut svg = notice("Author");
        svg.mime = "image/svg+xml".into();
        assert!(svg.validate());
        assert!(registry.unowned_digests().unwrap().is_empty());
    }

    #[test]
    fn attribution_display_text_rejects_control_and_bidi_spoofing() {
        let mut registry = MediaRegistry::in_memory().unwrap();
        for text in [
            "",
            "  ",
            "Author\nInjected",
            "Author\rSpoof",
            "Person\0Other",
            "A\u{202e}evil",
            "A\u{2066}evil",
            "A\u{0085}evil",
        ] {
            for in_creator in [true, false] {
                let mut bad = notice("Valid");
                if in_creator {
                    bad.creator = text.into();
                } else {
                    bad.license = text.into();
                }
                assert!(!bad.validate(), "accepted malformed notice: {text:?}");
                assert!(matches!(
                    registry.register_verified_bytes(b"diagram", &bad),
                    Err(MediaRegistryError::InvalidInput)
                ));
            }
        }
        let mut international = notice("Renée 李");
        international.license = "CC BY-SA — 4.0".into();
        assert!(international.validate());
        assert!(registry.unowned_digests().unwrap().is_empty());
    }

    #[test]
    fn unsafe_attribution_source_urls_are_rejected_before_registration() {
        let mut registry = MediaRegistry::in_memory().unwrap();
        for source in [
            "",
            "javascript:alert(1)",
            "data:text/html,malicious",
            "//commons.wikimedia.org/wiki/File:Figure.png",
            "https:///wiki/File:Figure.png",
            "https://user@commons.wikimedia.org/wiki/File:Figure.png",
            "https://commons.wikimedia.org/evil\\path",
            "https://commons.wikimedia.org/evil\npath",
            "https://commons.wikimedia.org/space in path",
            "https://wiki.example.org:abc/File:Example.png",
            "https://wiki.example.org:0/File:Example.png",
            "https://wiki.example.org:65536/File:Example.png",
            "https://wiki.example.org:/File:Example.png",
            "https://wiki.example.org%40evil.test/File:Example.png",
            "https://-wiki.example.org/File:Example.png",
            "https://wiki..example.org/File:Example.png",
            "https://[not-an-ipv6]/File:Example.png",
            "https://[::1/File:Example.png",
            "https://[::1]oops/File:Example.png",
        ] {
            let mut bad = notice("Author");
            bad.source_url = source.to_owned();
            assert!(!bad.validate(), "accepted unsafe source: {source:?}");
            assert!(matches!(
                registry.register_verified_bytes(b"figure", &bad),
                Err(MediaRegistryError::InvalidInput)
            ));
        }
        assert!(notice("Author").validate());
        let mut ipv6 = notice("Author");
        ipv6.source_url = "https://[2001:db8::1]:443/wiki/File:Example.png".into();
        assert!(ipv6.validate());
        assert_eq!(registry.unowned_digests().unwrap().len(), 0);
    }

    #[test]
    fn shared_pack_ownership_and_pin_prevent_premature_gc() {
        let mut registry = MediaRegistry::in_memory().unwrap();
        let digest = registry
            .register_verified_bytes(b"image data", &notice("A"))
            .unwrap();
        assert_eq!(
            registry
                .register_verified_bytes(b"image data", &notice("B"))
                .unwrap(),
            digest
        );
        assert_eq!(registry.notice_count(&digest).unwrap(), 2);
        registry
            .add_owner(&digest, OwnerKind::Pack, "physics")
            .unwrap();
        registry
            .add_owner(&digest, OwnerKind::Pack, "math")
            .unwrap();
        registry
            .add_owner(&digest, OwnerKind::UserPin, "user")
            .unwrap();
        registry
            .remove_owner(&digest, OwnerKind::Pack, "physics")
            .unwrap();
        registry
            .remove_owner(&digest, OwnerKind::Pack, "math")
            .unwrap();
        assert!(registry.unowned_digests().unwrap().is_empty());
        registry
            .remove_owner(&digest, OwnerKind::UserPin, "user")
            .unwrap();
        assert_eq!(registry.unowned_digests().unwrap(), vec![digest]);
    }

    #[test]
    fn batch_rolls_back_earlier_rows_when_later_size_conflicts() {
        let mut registry = MediaRegistry::in_memory().unwrap();
        let first = b"diagram";
        let second = b"figure";
        let first_digest = format!("{:x}", Sha256::digest(first));
        let second_digest = format!("{:x}", Sha256::digest(second));
        registry
            .conn
            .execute(
                "INSERT INTO media_objects(digest,bytes) VALUES (?1,?2)",
                params![second_digest, 99],
            )
            .unwrap();
        let attribution = notice("Creator");
        let assets: &[(&[u8], &MediaNotice)] = &[(first, &attribution), (second, &attribution)];
        assert!(matches!(
            registry.register_verified_owned_batch(assets, OwnerKind::Pack, "physics"),
            Err(MediaRegistryError::ConflictingSize)
        ));
        assert!(registry
            .owned_media(OwnerKind::Pack, "physics")
            .unwrap()
            .is_empty());
        assert_eq!(registry.notice_count(&first_digest).unwrap(), 0);
        assert_eq!(registry.notice_count(&second_digest).unwrap(), 0);
        assert_eq!(registry.unowned_digests().unwrap(), vec![second_digest]);
    }

    #[test]
    fn invalid_owners_and_unregistered_objects_fail_closed() {
        let mut registry = MediaRegistry::in_memory().unwrap();
        assert!(matches!(
            registry.register_verified_bytes(b"", &notice("A")),
            Err(MediaRegistryError::InvalidInput)
        ));
        let digest = registry
            .register_verified_bytes(b"payload", &notice("A"))
            .unwrap();
        assert!(matches!(
            registry.add_owner(&digest, OwnerKind::Cache, "../outside"),
            Err(MediaRegistryError::InvalidInput)
        ));
        assert!(matches!(
            registry.add_owner(&"0".repeat(64), OwnerKind::Pack, "physics"),
            Err(MediaRegistryError::UnknownObject)
        ));
    }
    #[test]
    fn registry_rejects_inventory_conflict_before_writing_notices_or_owners() {
        let mut registry = MediaRegistry::in_memory().unwrap();
        let alice = notice("Alice");
        let bob = notice("Bob");
        let initial = [(b"diagram".as_slice(), &alice)];
        let first = registry
            .register_verified_owned_batch(&initial, OwnerKind::Pack, "physics")
            .unwrap();
        let conflicting = [(b"diagram".as_slice(), &bob), (b"plot".as_slice(), &bob)];
        assert!(matches!(
            registry.register_verified_owned_batch(&conflicting, OwnerKind::Pack, "physics"),
            Err(MediaRegistryError::ConflictingInventory)
        ));
        assert_eq!(
            registry.owned_media(OwnerKind::Pack, "physics").unwrap(),
            vec![(first[0].clone(), 7)]
        );
        assert_eq!(registry.notice_count(&first[0]).unwrap(), 1);
        let plot = format!("{:x}", Sha256::digest(b"plot"));
        assert_eq!(registry.notice_count(&plot).unwrap(), 0);
        assert!(registry.unowned_digests().unwrap().is_empty());

        // Repeating the same inventory may add a distinct valid attribution,
        // but never adds an extra digest to this owner's inventory.
        let same = [(b"diagram".as_slice(), &bob)];
        assert_eq!(
            registry
                .register_verified_owned_batch(&same, OwnerKind::Pack, "physics")
                .unwrap(),
            first
        );
        assert_eq!(registry.notice_count(&first[0]).unwrap(), 2);
    }
}
