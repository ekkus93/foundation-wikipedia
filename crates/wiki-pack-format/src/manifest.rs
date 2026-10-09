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

impl Manifest {
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
