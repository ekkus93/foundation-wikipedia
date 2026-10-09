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
    UnsafeObject,
    DuplicateObject,
}

impl Manifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.version != FORMAT_VERSION {
            return Err(ManifestError::Version);
        }
        if self.pack_id.is_empty() || self.project.is_empty() || self.snapshot.is_empty() {
            return Err(ManifestError::MissingMetadata);
        }
        let origin_valid = match &self.origin {
            Origin::Official { publisher } => !publisher.is_empty(),
            Origin::Custom { definition_id } => !definition_id.is_empty(),
        };
        if !origin_valid || self.objects.is_empty() {
            return Err(ManifestError::MissingMetadata);
        }
        let mut paths = BTreeSet::new();
        for object in &self.objects {
            if object.bytes == 0
                || object.sha256.len() != 64
                || !object.sha256.bytes().all(|c| c.is_ascii_hexdigit())
                || object.path.contains('\\')
                || object.path.contains(':')
                || object
                    .path
                    .split('/')
                    .any(|c| c.is_empty() || c == "." || c == "..")
            {
                return Err(ManifestError::UnsafeObject);
            }
            if !paths.insert(&object.path) {
                return Err(ManifestError::DuplicateObject);
            }
        }
        Ok(())
    }
}
