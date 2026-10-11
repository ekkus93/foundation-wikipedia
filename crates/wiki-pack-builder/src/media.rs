//! Validate a pack's declared offline media inventory before it can be called
//! visually complete. This does not discover HTML dependencies or hash bytes:
//! callers must derive the *complete* resource list from canonical rendering
//! and populate available_verified only after verifying object digests.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    Image,
    Diagram,
    Plot,
    Map,
    Math,
    Style,
    Font,
    AudioVideoPreview,
    AudioVideoStream,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    pub id: String,
    pub kind: ResourceKind,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
    pub preview_id: Option<String>,
    pub creator: String,
    pub license: String,
    pub attribution: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaError {
    DuplicateId(String),
    MissingMetadata(String),
    InvalidDigest(String),
    MissingVerifiedObject(String),
    ByteLengthMismatch(String),
    MissingPreview(String),
    InvalidStream(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaCompleteness {
    pub required_objects: usize,
    pub unique_verified_bytes: u64,
    pub on_demand_streams: usize,
}

fn digest_valid(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Verify that every declared *mandatory* offline visual is present in the
/// caller's digest-verified object set. Audio/video bytes are never required;
/// their thumbnails/previews and attribution MUST be available locally.
pub fn check_media_completeness(
    resources: &[Resource],
    available_verified: &BTreeMap<String, u64>,
) -> Result<MediaCompleteness, MediaError> {
    let mut by_id = BTreeMap::new();
    for resource in resources {
        if resource.id.trim().is_empty()
            || resource.creator.trim().is_empty()
            || resource.license.trim().is_empty()
            || resource.attribution.trim().is_empty()
        {
            return Err(MediaError::MissingMetadata(resource.id.clone()));
        }
        if by_id.insert(resource.id.as_str(), resource).is_some() {
            return Err(MediaError::DuplicateId(resource.id.clone()));
        }
    }

    let mut streams = 0;
    let mut counted = BTreeSet::new();
    let mut total_bytes = 0_u64;

    for resource in resources {
        if resource.kind == ResourceKind::AudioVideoStream {
            streams += 1;
            if resource.sha256.is_some() || resource.bytes.is_some() {
                return Err(MediaError::InvalidStream(resource.id.clone()));
            }
            let preview_id = resource
                .preview_id
                .as_deref()
                .ok_or_else(|| MediaError::MissingPreview(resource.id.clone()))?;
            if by_id.get(preview_id).map(|item| item.kind) != Some(ResourceKind::AudioVideoPreview)
            {
                return Err(MediaError::MissingPreview(resource.id.clone()));
            }
            continue;
        }
        if resource.preview_id.is_some() {
            return Err(MediaError::InvalidStream(resource.id.clone()));
        }
        let hash = resource
            .sha256
            .as_deref()
            .ok_or_else(|| MediaError::InvalidDigest(resource.id.clone()))?;
        if !digest_valid(hash) {
            return Err(MediaError::InvalidDigest(resource.id.clone()));
        }
        let size = resource
            .bytes
            .filter(|value| *value > 0)
            .ok_or_else(|| MediaError::MissingMetadata(resource.id.clone()))?;
        // Content-addressed object keys are canonical lowercase SHA-256.
        // Normalize the declared digest before lookup and deduplication so
        // case variants cannot inflate installed-byte accounting.
        let canonical_hash = hash.to_ascii_lowercase();
        let observed = available_verified
            .get(&canonical_hash)
            .ok_or_else(|| MediaError::MissingVerifiedObject(resource.id.clone()))?;
        if *observed != size {
            return Err(MediaError::ByteLengthMismatch(resource.id.clone()));
        }
        if counted.insert(canonical_hash) {
            total_bytes = total_bytes
                .checked_add(size)
                .ok_or_else(|| MediaError::MissingMetadata(resource.id.clone()))?;
        }
    }

    Ok(MediaCompleteness {
        required_objects: counted.len(),
        unique_verified_bytes: total_bytes,
        on_demand_streams: streams,
    })
}
