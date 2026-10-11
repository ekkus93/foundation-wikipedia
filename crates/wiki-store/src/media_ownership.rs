//! Durable ownership accounting for deduplicated media bytes (partial STORE-002).
//!
//! The caller must separately persist and reverify content-addressed bytes.
//! This registry records verified byte identity and attribution notices; it
//! does not certify an object as installed or authorize deletion of its bytes.

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub enum MediaRegistryError {
    Sql(rusqlite::Error),
    InvalidInput,
    ConflictingSize,
    UnknownObject,
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
        !self.mime.trim().is_empty()
            && !self.source_url.trim().is_empty()
            && !self.creator.trim().is_empty()
            && !self.license.trim().is_empty()
            && self.mime.len() <= 255
            && self.source_url.len() <= 4096
            && self.creator.len() <= 4096
            && self.license.len() <= 4096
    }
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
    /// Register all staged media and grant their owner in one SQLite transaction.
    /// A failed item cannot leave a partially owned pack. The caller must
    /// persist and verify every object before calling this method.
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
        let tx = self.conn.transaction()?;
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
        let assets: &[(&[u8], &MediaNotice)] =
            &[(first, &attribution), (second, &attribution)];
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
}
