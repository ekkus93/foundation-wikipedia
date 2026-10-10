//! Bounded single-record reads from independently compressed immutable shards.
//! A verified SQLite catalog will supply offsets and frame lengths.
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use crate::record_codec::{decode_record, DecodedRecord, RecordCodecError, MAX_COMPRESSED_RECORD_BYTES};

const PREFIX: u64 = 8;
const HEADER: u64 = 32;

#[derive(Debug)]
pub enum ShardError {
    Io(io::Error),
    UnsafePath,
    InvalidLength,
    OutOfBounds,
    CatalogMismatch,
    Codec(RecordCodecError),
}

impl From<io::Error> for ShardError {
    fn from(error: io::Error) -> Self { Self::Io(error) }
}
impl From<RecordCodecError> for ShardError {
    fn from(error: RecordCodecError) -> Self { Self::Codec(error) }
}

/// Reads exactly one length-prefixed record; never inflates an entire shard.
pub fn read_at(file: &mut File, offset: u64, frame_bytes: u64) -> Result<DecodedRecord, ShardError> {
    if !(PREFIX + HEADER..=PREFIX + HEADER + MAX_COMPRESSED_RECORD_BYTES).contains(&frame_bytes) {
        return Err(ShardError::InvalidLength);
    }
    let end = offset.checked_add(frame_bytes).ok_or(ShardError::OutOfBounds)?;
    let before = file.metadata()?;
    if !before.is_file() || end > before.len() { return Err(ShardError::OutOfBounds); }
    file.seek(SeekFrom::Start(offset))?;
    let mut prefix = [0u8; 8];
    file.read_exact(&mut prefix)?;
    let declared = u64::from_le_bytes(prefix);
    if declared != frame_bytes - PREFIX { return Err(ShardError::CatalogMismatch); }
    let size = usize::try_from(declared).map_err(|_| ShardError::InvalidLength)?;
    let mut encoded = vec![0; size];
    file.read_exact(&mut encoded)?;
    if file.metadata()?.len() != before.len() { return Err(ShardError::OutOfBounds); }
    Ok(decode_record(&encoded)?)
}

/// Reject a symlink path before opening the shard. Snapshot integrity and
/// digest verification remain separate prerequisites for activation.
pub fn read_path(path: &Path, offset: u64, frame_bytes: u64) -> Result<DecodedRecord, ShardError> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() { return Err(ShardError::UnsafePath); }
    let mut file = File::open(path)?;
    let opened = file.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != meta.dev() || opened.ino() != meta.ino() {
            return Err(ShardError::UnsafePath);
        }
    }
    if !fs::symlink_metadata(path)?.is_file() { return Err(ShardError::UnsafePath); }
    read_at(&mut file, offset, frame_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_codec::append_record_frame;
    use wiki_model::{ArticleKey, PageRecord, Redirect};

    fn record(id: u64) -> PageRecord {
        PageRecord::Redirect(Redirect {
            from: ArticleKey { project: "enwiki".into(), page_id: id },
            title: format!("Alias {id}"),
            to: ArticleKey { project: "enwiki".into(), page_id: id + 100 },
        })
    }

    #[test]
    fn direct_offset_read_and_corrupt_catalog_fail_closed() {
        let mut shard = Vec::new();
        append_record_frame(&mut shard, &record(10)).unwrap();
        let offset = append_record_frame(&mut shard, &record(20)).unwrap();
        let frame_bytes = (shard.len() - offset) as u64;
        let path = std::env::temp_dir().join(format!("wiki-shard-{}-{}", std::process::id(), offset));
        fs::write(&path, &shard).unwrap();
        assert_eq!(read_path(&path, offset as u64, frame_bytes).unwrap().record, record(20));
        assert!(matches!(read_path(&path, offset as u64, frame_bytes - 1), Err(ShardError::CatalogMismatch)));
        assert!(matches!(read_path(&path, u64::MAX, frame_bytes), Err(ShardError::OutOfBounds)));
        assert!(matches!(read_path(&path, 0, u64::MAX), Err(ShardError::InvalidLength)));
        fs::remove_file(path).unwrap();
    }
}
