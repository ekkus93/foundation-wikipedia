//! Versioned physical record codec for independently addressable article records.
//!
//! Logical Wikipedia data remains in `wiki-model`. This module owns only the
//! stable on-disk envelope, MessagePack payload encoding, record-level zstd,
//! compatibility classification and direct-offset shard framing.

use std::error::Error;
use std::fmt;
use std::io::Read;

use wiki_model::{ModelError, PageRecord};

const MAGIC: &[u8; 8] = b"FWREC001";
const HEADER_LEN: usize = 32;
const FRAME_PREFIX_LEN: usize = 8;
const ENCODING_MESSAGEPACK_NAMED: u8 = 1;
const COMPRESSION_ZSTD: u8 = 1;
const ZSTD_LEVEL: i32 = 3;

/// Current canonical record-envelope version.
///
/// v1.0 and v1.1 use the same named-field MessagePack payload. v1.1 makes the
/// bounded length checks and reserved-header semantics normative. Reading v1.0
/// therefore requires validation plus rewrite on the next persisted write.
pub const CURRENT_CODEC_VERSION: CodecVersion = CodecVersion { major: 1, minor: 1 };

/// Guardrail for one uncompressed canonical article/redirect record.
///
/// This bounds decoder allocation on mobile; larger content must be split by
/// the upstream normalization/storage layer rather than forcing an unbounded
/// single-record allocation.
pub const MAX_UNCOMPRESSED_RECORD_BYTES: u64 = 32 * 1024 * 1024;

/// Guardrail for one compressed record frame.
pub const MAX_COMPRESSED_RECORD_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodecVersion {
    pub major: u16,
    pub minor: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationStatus {
    None,
    RewriteToCurrent { from: CodecVersion },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedRecord {
    pub record: PageRecord,
    pub source_version: CodecVersion,
    pub migration: MigrationStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordCodecError {
    InvalidModel(ModelError),
    Serialization(String),
    Compression(String),
    TruncatedHeader,
    InvalidMagic,
    InvalidReservedHeader,
    UnsupportedEncoding(u8),
    UnsupportedCompression(u8),
    UnsupportedMajor { found: u16, supported: u16 },
    UnsupportedMinor { found: u16, supported: u16 },
    CompressedLengthMismatch { declared: u64, actual: u64 },
    UncompressedLengthMismatch { declared: u64, actual: u64 },
    RecordTooLarge { declared: u64, limit: u64 },
    FrameOutOfBounds,
    FrameTooLarge(u64),
}

impl fmt::Display for RecordCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for RecordCodecError {}

impl From<ModelError> for RecordCodecError {
    fn from(value: ModelError) -> Self {
        Self::InvalidModel(value)
    }
}

/// Encode only the current canonical format. Older accepted records are read
/// and then rewritten through this function rather than perpetuating old wire
/// versions.
pub fn encode_record(record: &PageRecord) -> Result<Vec<u8>, RecordCodecError> {
    record.validate()?;
    let raw = rmp_serde::to_vec_named(record)
        .map_err(|error| RecordCodecError::Serialization(error.to_string()))?;
    let raw_len = u64::try_from(raw.len())
        .map_err(|_| RecordCodecError::RecordTooLarge {
            declared: u64::MAX,
            limit: MAX_UNCOMPRESSED_RECORD_BYTES,
        })?;
    if raw_len > MAX_UNCOMPRESSED_RECORD_BYTES {
        return Err(RecordCodecError::RecordTooLarge {
            declared: raw_len,
            limit: MAX_UNCOMPRESSED_RECORD_BYTES,
        });
    }

    let compressed = zstd::stream::encode_all(raw.as_slice(), ZSTD_LEVEL)
        .map_err(|error| RecordCodecError::Compression(error.to_string()))?;
    let compressed_len = u64::try_from(compressed.len())
        .map_err(|_| RecordCodecError::RecordTooLarge {
            declared: u64::MAX,
            limit: MAX_COMPRESSED_RECORD_BYTES,
        })?;
    if compressed_len > MAX_COMPRESSED_RECORD_BYTES {
        return Err(RecordCodecError::RecordTooLarge {
            declared: compressed_len,
            limit: MAX_COMPRESSED_RECORD_BYTES,
        });
    }

    let mut out = Vec::with_capacity(HEADER_LEN + compressed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&CURRENT_CODEC_VERSION.major.to_le_bytes());
    out.extend_from_slice(&CURRENT_CODEC_VERSION.minor.to_le_bytes());
    out.push(ENCODING_MESSAGEPACK_NAMED);
    out.push(COMPRESSION_ZSTD);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&raw_len.to_le_bytes());
    out.extend_from_slice(&compressed_len.to_le_bytes());
    out.extend_from_slice(&compressed);
    Ok(out)
}

pub fn decode_record(bytes: &[u8]) -> Result<DecodedRecord, RecordCodecError> {
    if bytes.len() < HEADER_LEN {
        return Err(RecordCodecError::TruncatedHeader);
    }
    if &bytes[..8] != MAGIC {
        return Err(RecordCodecError::InvalidMagic);
    }

    let version = CodecVersion {
        major: u16::from_le_bytes([bytes[8], bytes[9]]),
        minor: u16::from_le_bytes([bytes[10], bytes[11]]),
    };
    let migration = classify_version(version)?;

    if bytes[12] != ENCODING_MESSAGEPACK_NAMED {
        return Err(RecordCodecError::UnsupportedEncoding(bytes[12]));
    }
    if bytes[13] != COMPRESSION_ZSTD {
        return Err(RecordCodecError::UnsupportedCompression(bytes[13]));
    }
    if bytes[14] != 0 || bytes[15] != 0 {
        return Err(RecordCodecError::InvalidReservedHeader);
    }

    let declared_raw = u64::from_le_bytes(bytes[16..24].try_into().expect("fixed header slice"));
    let declared_compressed =
        u64::from_le_bytes(bytes[24..32].try_into().expect("fixed header slice"));
    if declared_raw > MAX_UNCOMPRESSED_RECORD_BYTES {
        return Err(RecordCodecError::RecordTooLarge {
            declared: declared_raw,
            limit: MAX_UNCOMPRESSED_RECORD_BYTES,
        });
    }
    if declared_compressed > MAX_COMPRESSED_RECORD_BYTES {
        return Err(RecordCodecError::RecordTooLarge {
            declared: declared_compressed,
            limit: MAX_COMPRESSED_RECORD_BYTES,
        });
    }

    let payload = &bytes[HEADER_LEN..];
    let actual_compressed =
        u64::try_from(payload.len()).map_err(|_| RecordCodecError::FrameTooLarge(u64::MAX))?;
    if actual_compressed != declared_compressed {
        return Err(RecordCodecError::CompressedLengthMismatch {
            declared: declared_compressed,
            actual: actual_compressed,
        });
    }

    let decoder = zstd::stream::read::Decoder::new(payload)
        .map_err(|error| RecordCodecError::Compression(error.to_string()))?;
    let mut limited = decoder.take(declared_raw.saturating_add(1));
    let initial_capacity = usize::try_from(declared_raw.min(1024 * 1024))
        .expect("capacity is capped to one MiB");
    let mut raw = Vec::with_capacity(initial_capacity);
    limited
        .read_to_end(&mut raw)
        .map_err(|error| RecordCodecError::Compression(error.to_string()))?;
    let actual_raw =
        u64::try_from(raw.len()).map_err(|_| RecordCodecError::FrameTooLarge(u64::MAX))?;
    if actual_raw != declared_raw {
        return Err(RecordCodecError::UncompressedLengthMismatch {
            declared: declared_raw,
            actual: actual_raw,
        });
    }

    let record: PageRecord = rmp_serde::from_slice(&raw)
        .map_err(|error| RecordCodecError::Serialization(error.to_string()))?;
    record.validate()?;
    Ok(DecodedRecord {
        record,
        source_version: version,
        migration,
    })
}

fn classify_version(version: CodecVersion) -> Result<MigrationStatus, RecordCodecError> {
    if version.major != CURRENT_CODEC_VERSION.major {
        return Err(RecordCodecError::UnsupportedMajor {
            found: version.major,
            supported: CURRENT_CODEC_VERSION.major,
        });
    }
    match version.minor {
        1 => Ok(MigrationStatus::None),
        0 => Ok(MigrationStatus::RewriteToCurrent { from: version }),
        found => Err(RecordCodecError::UnsupportedMinor {
            found,
            supported: CURRENT_CODEC_VERSION.minor,
        }),
    }
}

/// Append a length-prefixed encoded record to a shard and return the byte
/// offset of that frame. An external catalog can persist the offset for O(1)
/// record location without inflating or scanning preceding records.
pub fn append_record_frame(
    shard: &mut Vec<u8>,
    record: &PageRecord,
) -> Result<usize, RecordCodecError> {
    let offset = shard.len();
    let encoded = encode_record(record)?;
    let encoded_len =
        u64::try_from(encoded.len()).map_err(|_| RecordCodecError::FrameTooLarge(u64::MAX))?;
    let max_frame = MAX_COMPRESSED_RECORD_BYTES + HEADER_LEN as u64;
    if encoded_len > max_frame {
        return Err(RecordCodecError::FrameTooLarge(encoded_len));
    }
    shard.extend_from_slice(&encoded_len.to_le_bytes());
    shard.extend_from_slice(&encoded);
    Ok(offset)
}

/// Decode exactly one frame at a catalog-provided byte offset.
pub fn decode_record_at(shard: &[u8], offset: usize) -> Result<DecodedRecord, RecordCodecError> {
    let prefix_end = offset
        .checked_add(FRAME_PREFIX_LEN)
        .ok_or(RecordCodecError::FrameOutOfBounds)?;
    if prefix_end > shard.len() {
        return Err(RecordCodecError::FrameOutOfBounds);
    }
    let frame_len = u64::from_le_bytes(
        shard[offset..prefix_end]
            .try_into()
            .expect("checked frame prefix"),
    );
    let max_frame = MAX_COMPRESSED_RECORD_BYTES + HEADER_LEN as u64;
    if frame_len > max_frame {
        return Err(RecordCodecError::FrameTooLarge(frame_len));
    }
    let frame_len_usize =
        usize::try_from(frame_len).map_err(|_| RecordCodecError::FrameTooLarge(frame_len))?;
    let end = prefix_end
        .checked_add(frame_len_usize)
        .ok_or(RecordCodecError::FrameOutOfBounds)?;
    if end > shard.len() {
        return Err(RecordCodecError::FrameOutOfBounds);
    }
    decode_record(&shard[prefix_end..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiki_model::{
        Article, ArticleKey, Block, BlockContent, PageRecord, Revision, Section,
        ARTICLE_SCHEMA_VERSION,
    };

    fn article(page_id: u64, revision_id: u64) -> PageRecord {
        PageRecord::Article(Box::new(Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id,
            },
            revision: Revision {
                revision_id,
                timestamp: "2026-10-10T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: format!("Article {page_id}"),
            display_title: format!("Article {page_id}"),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph("Gravity bends spacetime.".into()),
            }],
            sections: vec![Section {
                ordinal: 1,
                heading: "Physics".into(),
                blocks: vec![Block {
                    ordinal: 0,
                    content: BlockContent::Paragraph("Unicode: Gravité 质量 🌍".into()),
                }],
                subsections: vec![],
            }],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<article><p>Gravity bends spacetime.</p></article>".into(),
            is_disambiguation: false,
        }))
    }

    #[test]
    fn current_record_roundtrips_and_is_bounded() {
        let expected = article(42, 7);
        let encoded = encode_record(&expected).unwrap();
        let decoded = decode_record(&encoded).unwrap();
        assert_eq!(decoded.record, expected);
        assert_eq!(decoded.source_version, CURRENT_CODEC_VERSION);
        assert_eq!(decoded.migration, MigrationStatus::None);
        assert!(encoded.len() < MAX_COMPRESSED_RECORD_BYTES as usize);
    }

    #[test]
    fn supported_legacy_minor_requires_rewrite_to_current() {
        let expected = article(42, 7);
        let mut encoded = encode_record(&expected).unwrap();
        encoded[10..12].copy_from_slice(&0u16.to_le_bytes());
        let decoded = decode_record(&encoded).unwrap();
        assert_eq!(decoded.record, expected);
        assert_eq!(
            decoded.migration,
            MigrationStatus::RewriteToCurrent {
                from: CodecVersion { major: 1, minor: 0 }
            }
        );
        let rewritten = encode_record(&decoded.record).unwrap();
        let current = decode_record(&rewritten).unwrap();
        assert_eq!(current.migration, MigrationStatus::None);
        assert_eq!(current.source_version, CURRENT_CODEC_VERSION);
    }

    #[test]
    fn unsupported_future_versions_fail_closed() {
        let mut future_minor = encode_record(&article(1, 1)).unwrap();
        future_minor[10..12].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(
            decode_record(&future_minor),
            Err(RecordCodecError::UnsupportedMinor {
                found: 2,
                supported: 1
            })
        );

        let mut future_major = encode_record(&article(1, 1)).unwrap();
        future_major[8..10].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(
            decode_record(&future_major),
            Err(RecordCodecError::UnsupportedMajor {
                found: 2,
                supported: 1
            })
        );
    }

    #[test]
    fn malformed_lengths_and_reserved_header_fail_closed() {
        let encoded = encode_record(&article(1, 1)).unwrap();
        assert_eq!(
            decode_record(&encoded[..HEADER_LEN - 1]),
            Err(RecordCodecError::TruncatedHeader)
        );

        let mut reserved = encoded.clone();
        reserved[14] = 1;
        assert_eq!(
            decode_record(&reserved),
            Err(RecordCodecError::InvalidReservedHeader)
        );

        let mut oversized = encoded.clone();
        oversized[16..24]
            .copy_from_slice(&(MAX_UNCOMPRESSED_RECORD_BYTES + 1).to_le_bytes());
        assert_eq!(
            decode_record(&oversized),
            Err(RecordCodecError::RecordTooLarge {
                declared: MAX_UNCOMPRESSED_RECORD_BYTES + 1,
                limit: MAX_UNCOMPRESSED_RECORD_BYTES
            })
        );

        let mut bad_compressed_length = encoded;
        bad_compressed_length[24..32].copy_from_slice(&1u64.to_le_bytes());
        assert!(matches!(
            decode_record(&bad_compressed_length),
            Err(RecordCodecError::CompressedLengthMismatch { .. })
        ));
    }

    #[test]
    fn catalog_offset_can_random_read_one_frame_without_scanning_predecessors() {
        let first = article(10, 1);
        let second = article(20, 2);
        let third = article(30, 3);
        let mut shard = Vec::new();
        let first_offset = append_record_frame(&mut shard, &first).unwrap();
        let second_offset = append_record_frame(&mut shard, &second).unwrap();
        let third_offset = append_record_frame(&mut shard, &third).unwrap();

        assert_eq!(first_offset, 0);
        assert!(second_offset > first_offset);
        assert!(third_offset > second_offset);
        assert_eq!(decode_record_at(&shard, second_offset).unwrap().record, second);
        assert_eq!(decode_record_at(&shard, third_offset).unwrap().record, third);
        assert_eq!(
            decode_record_at(&shard, shard.len()),
            Err(RecordCodecError::FrameOutOfBounds)
        );
    }
}
