//! Versioned pack manifest structure; no signature verification yet.
use std::collections::BTreeSet;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Official { publisher: String },
    Custom { definition_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub version: u32,
    pub pack_id: String,
    pub project: String,
    pub snapshot: String,
    pub origin: Origin,
    pub objects: Vec<Object>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    Version,
    MissingMetadata,
    UnsafeMetadata,
    UnsafeObject,
    DuplicateObject,
}

fn safe_id(value: &str) -> bool {
    value == value.trim()
        && value.len() <= 256
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains(':')
        && !unsafe_component(value)
        && !value.chars().any(char::is_control)
}

fn unsafe_component(component: &str) -> bool {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.ends_with('.')
        || component.ends_with(' ')
    {
        return true;
    }
    let upper = component.to_ascii_uppercase();
    let base = upper.split('.').next().unwrap_or("");
    let port = base.len() == 4
        && (base.starts_with("COM") || base.starts_with("LPT"))
        && (b'1'..=b'9').contains(&base.as_bytes()[3]);
    ["CON", "PRN", "AUX", "NUL"].contains(&base) || port
}

/// Provisional, unambiguous binary encoding for manifest *identity* only.
///
/// This helper is not the final .wpack serialization or a publisher signature
/// payload. Full licensing, resource requirements, and update provenance must
/// be added before a future format can be signed or shipped.
fn append_field(bytes: &mut Vec<u8>, field: &str) {
    bytes.extend_from_slice(&(field.len() as u64).to_be_bytes());
    bytes.extend_from_slice(field.as_bytes());
}

impl Manifest {
    /// Deterministic identity transcript, independent of object input order
    /// and the casing of hexadecimal digests.
    pub fn identity_transcript(&self) -> Result<Vec<u8>, ManifestError> {
        self.validate()?;
        let mut out = b"foundation-wikipedia/manifest-identity/v1\0".to_vec();
        out.extend_from_slice(&self.version.to_be_bytes());
        append_field(&mut out, &self.pack_id);
        append_field(&mut out, &self.project);
        append_field(&mut out, &self.snapshot);
        match &self.origin {
            Origin::Official { publisher } => {
                out.push(1);
                append_field(&mut out, publisher);
            }
            Origin::Custom { definition_id } => {
                out.push(2);
                append_field(&mut out, definition_id);
            }
        }
        let mut objects = self.objects.iter().collect::<Vec<_>>();
        objects.sort_by(|a, b| a.path.cmp(&b.path));
        out.extend_from_slice(&(objects.len() as u64).to_be_bytes());
        for object in objects {
            append_field(&mut out, &object.path);
            append_field(&mut out, &object.sha256.to_ascii_lowercase());
            out.extend_from_slice(&object.bytes.to_be_bytes());
        }
        Ok(out)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.version != FORMAT_VERSION {
            return Err(ManifestError::Version);
        }
        if self.pack_id.trim().is_empty()
            || self.project.trim().is_empty()
            || self.snapshot.trim().is_empty()
        {
            return Err(ManifestError::MissingMetadata);
        }
        if !safe_id(&self.pack_id) || !safe_id(&self.project) || !safe_id(&self.snapshot) {
            return Err(ManifestError::UnsafeMetadata);
        }
        let origin_valid = match &self.origin {
            Origin::Official { publisher } => !publisher.trim().is_empty(),
            Origin::Custom { definition_id } => !definition_id.trim().is_empty(),
        };
        if !origin_valid || self.objects.is_empty() {
            return Err(ManifestError::MissingMetadata);
        }
        let origin_safe = match &self.origin {
            Origin::Official { publisher } => safe_id(publisher),
            Origin::Custom { definition_id } => safe_id(definition_id),
        };
        if !origin_safe {
            return Err(ManifestError::UnsafeMetadata);
        }
        let mut paths = BTreeSet::new();
        for object in &self.objects {
            if object.bytes == 0
                || object.sha256.len() != 64
                || !object.sha256.bytes().all(|c| c.is_ascii_hexdigit())
                || object.path.contains('\\')
                || object.path.chars().any(char::is_control)
                || object.path.contains(':')
                || object.path.split('/').any(unsafe_component)
            {
                return Err(ManifestError::UnsafeObject);
            }
            if !paths.insert(object.path.to_ascii_lowercase()) {
                return Err(ManifestError::DuplicateObject);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod transcript_tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            version: FORMAT_VERSION,
            pack_id: "physics".into(),
            project: "enwiki".into(),
            snapshot: "20261009".into(),
            origin: Origin::Custom {
                definition_id: "my-topic".into(),
            },
            objects: vec![
                Object {
                    path: "b/image.svg".into(),
                    sha256: "A".repeat(64),
                    bytes: 72,
                },
                Object {
                    path: "a/article.cbor".into(),
                    sha256: "b".repeat(64),
                    bytes: 105,
                },
            ],
        }
    }

    #[test]
    fn deterministic_across_reordering_and_digest_case() {
        let original = sample();
        let mut permutation = original.clone();
        permutation.objects.reverse();
        permutation.objects[0].sha256.make_ascii_uppercase();
        assert_eq!(
            original.identity_transcript(),
            permutation.identity_transcript()
        );
    }

    #[test]
    fn identity_fields_cannot_alias_or_be_removed() {
        let original = sample();
        let baseline = original.identity_transcript().unwrap();
        let mut changed = original.clone();
        changed.pack_id = "physic".into();
        assert_ne!(changed.identity_transcript().unwrap(), baseline);
        changed = original.clone();
        changed.objects[0].bytes += 1;
        assert_ne!(changed.identity_transcript().unwrap(), baseline);
        changed = original.clone();
        changed.origin = Origin::Official {
            publisher: "foundation".into(),
        };
        assert_ne!(changed.identity_transcript().unwrap(), baseline);
    }

    #[test]
    fn invalid_manifest_never_produces_transcript() {
        let mut invalid = sample();
        invalid.objects[0].path = "../escape".into();
        assert_eq!(
            invalid.identity_transcript(),
            Err(ManifestError::UnsafeObject)
        );
    }
}
