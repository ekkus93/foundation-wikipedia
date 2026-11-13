//! Versioned pack manifest structure; no signature verification yet.
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

/// Manifest/identity-transcript version (ADR 0007). Version 1 is rejected.
pub const FORMAT_VERSION: u32 = 2;
/// Bound on declared index entries; a snapshot needs only a few.
pub const MAX_PACK_INDEXES: usize = 16;
/// Provisional safeguards until archive framing and mobile benchmarks are final.
pub const MAX_PACK_OBJECTS: usize = 1_000_000;
pub const MAX_OBJECT_BYTES: u64 = 1_u64 << 40;
pub const MAX_DECLARED_BYTES: u64 = 4_u64 << 40;
pub const MAX_OBJECT_PATH_BYTES: usize = 4096;
pub const MAX_OBJECT_PATH_COMPONENTS: usize = 64;
pub const MAX_OBJECT_PATH_COMPONENT_BYTES: usize = 255;
/// Domain-separation prefix of the version-2 identity transcript.
pub const TRANSCRIPT_PREFIX: &[u8] = b"foundation-wikipedia/manifest-identity/v2\0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Official { publisher: String },
    Custom { definition_id: String },
}

/// Where the pack's article content came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Enterprise,
    PublicDump,
    Custom,
}

impl SourceKind {
    pub const fn tag(self) -> u8 {
        match self {
            Self::Enterprise => 1,
            Self::PublicDump => 2,
            Self::Custom => 3,
        }
    }

    pub const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            1 => Some(Self::Enterprise),
            2 => Some(Self::PublicDump),
            3 => Some(Self::Custom),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Enterprise => "enterprise",
            Self::PublicDump => "public-dump",
            Self::Custom => "custom",
        }
    }
}

/// Source snapshot identity: which upstream snapshot was imported and the
/// SHA-256 of the source manifest (for example a dump's `SHA256SUMS`) the
/// importer verified. This is a provenance claim, not a signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSnapshot {
    pub kind: SourceKind,
    pub snapshot_id: String,
    pub manifest_sha256: String,
}

/// Text licence (SPDX identifier) and, when the pack carries media, the
/// declared object holding per-media attribution notices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct License {
    pub text_spdx: String,
    pub attribution_object: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IndexRole {
    Catalog,
    Fts5,
}

impl IndexRole {
    pub const fn tag(self) -> u8 {
        match self {
            Self::Catalog => 1,
            Self::Fts5 => 2,
        }
    }

    pub const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            1 => Some(Self::Catalog),
            2 => Some(Self::Fts5),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Fts5 => "fts5",
        }
    }
}

/// A declared object that is an index rather than canonical content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub path: String,
    pub role: IndexRole,
    pub schema: u32,
}

/// Declared content counts. Installers must check them against the catalog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub articles: u64,
    pub redirects: u64,
    pub media: u64,
}

/// Version-2 manifest metadata bound into the identity transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackMetadata {
    pub source: SourceSnapshot,
    /// SHA-256 of the canonical topic definition the pack was built from.
    pub definition_sha256: String,
    pub license: License,
    pub indexes: Vec<IndexEntry>,
    pub counts: Counts,
}

impl PackMetadata {
    /// Deterministic, valid metadata for fixtures, tests and probes that do
    /// not model provenance: custom source, CC BY-SA 4.0 text, no indexes,
    /// zero counts. Real builders must supply measured values.
    pub fn synthetic_fixture() -> Self {
        Self {
            source: SourceSnapshot {
                kind: SourceKind::Custom,
                snapshot_id: "fixture".into(),
                manifest_sha256: "0".repeat(64),
            },
            definition_sha256: "0".repeat(64),
            license: License {
                text_spdx: "CC-BY-SA-4.0".into(),
                attribution_object: None,
            },
            indexes: Vec::new(),
            counts: Counts::default(),
        }
    }
}

fn lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_spdx(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
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
    pub metadata: PackMetadata,
    pub objects: Vec<Object>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    Version,
    MissingMetadata,
    UnsafeMetadata,
    UnsafeObject,
    DuplicateObject,
    ConflictingDigest,
    ResourceBudget,
    /// Invalid v2 source/definition/licence/index/count metadata.
    InvalidPackMetadata,
}

/// Reject invisible/spoofing format characters and Unicode noncharacters in
/// pack display identities and object paths. Retain meaningful ZWNJ/ZWJ in
/// multilingual labels, matching the canonical article-title policy.
fn unsafe_label_character(c: char) -> bool {
    let scalar = c as u32;
    matches!(
        c,
        '\u{061c}'
            | '\u{200b}'
            | '\u{200e}'..='\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2060}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
    ) || (0xfdd0..=0xfdef).contains(&scalar)
        || (scalar & 0xfffe) == 0xfffe
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
        && !value
            .chars()
            .any(|c| c.is_control() || unsafe_label_character(c))
}

/// Pack project identity must match the source and article-key edition syntax.
fn valid_wiki_project(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 255
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

pub(crate) fn unsafe_component(component: &str) -> bool {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.ends_with('.')
        || component.ends_with(' ')
        // Some filesystems normalize decomposed Unicode path components.
        // Require one canonical spelling before constructing on-disk paths.
        || !component.nfc().eq(component.chars())
        // Windows forbids these characters even on filesystems that allow them.
        || component
            .chars()
            .any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return true;
    }
    let upper = component.to_ascii_uppercase();
    // Win32 ignores trailing ASCII spaces in a DOS device-name stem even
    // when an extension follows it (for example, "CON .txt"). Validate the
    // normalized stem rather than only the literal bytes before the dot.
    let base = upper.split('.').next().unwrap_or("").trim_end_matches(' ');
    // Win32 reserves superscript 1/2/3 spellings as COM/LPT port names too.
    let port = ["COM", "LPT"].iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&base) || port
}

/// Unambiguous binary encoding of the manifest identity transcript (v2,
/// ADR 0007): identity, origin, source snapshot, definition hash, licence,
/// index metadata, counts and the object inventory. It is the byte string a
/// future publisher signature (TRUST-001) covers; it carries no signature.
fn append_field(bytes: &mut Vec<u8>, field: &str) {
    bytes.extend_from_slice(&(field.len() as u64).to_be_bytes());
    bytes.extend_from_slice(field.as_bytes());
}

impl Manifest {
    /// Deterministic identity transcript, independent of object input order
    /// and the casing of hexadecimal digests.
    pub fn identity_transcript(&self) -> Result<Vec<u8>, ManifestError> {
        self.validate()?;
        let mut out = TRANSCRIPT_PREFIX.to_vec();
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
        let meta = &self.metadata;
        out.push(meta.source.kind.tag());
        append_field(&mut out, &meta.source.snapshot_id);
        append_field(&mut out, &meta.source.manifest_sha256);
        append_field(&mut out, &meta.definition_sha256);
        append_field(&mut out, &meta.license.text_spdx);
        match &meta.license.attribution_object {
            None => out.push(0),
            Some(path) => {
                out.push(1);
                append_field(&mut out, path);
            }
        }
        let mut indexes = meta.indexes.iter().collect::<Vec<_>>();
        indexes.sort_by(|a, b| a.path.cmp(&b.path));
        out.extend_from_slice(&(indexes.len() as u64).to_be_bytes());
        for index in indexes {
            append_field(&mut out, &index.path);
            out.push(index.role.tag());
            out.extend_from_slice(&index.schema.to_be_bytes());
        }
        out.extend_from_slice(&meta.counts.articles.to_be_bytes());
        out.extend_from_slice(&meta.counts.redirects.to_be_bytes());
        out.extend_from_slice(&meta.counts.media.to_be_bytes());
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
        if !safe_id(&self.pack_id) || !valid_wiki_project(&self.project) || !safe_id(&self.snapshot)
        {
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
        if self.objects.len() > MAX_PACK_OBJECTS {
            return Err(ManifestError::ResourceBudget);
        }
        let mut paths_lower = BTreeSet::new();
        let mut paths_upper = BTreeSet::new();
        // Distinct spellings of an intermediate directory must not merge on
        // case-insensitive filesystems, even when leaf filenames differ.
        let mut prefixes_lower = BTreeMap::new();
        let mut prefixes_upper = BTreeMap::new();
        let mut digest_sizes = BTreeMap::new();
        let mut declared_bytes = 0_u64;
        for object in &self.objects {
            if object.bytes == 0
                || object.sha256.len() != 64
                || !object.sha256.bytes().all(|c| c.is_ascii_hexdigit())
                || object.path.len() > MAX_OBJECT_PATH_BYTES
                || object.path.contains('\\')
                || object
                    .path
                    .chars()
                    .any(|c| c.is_control() || unsafe_label_character(c))
                || object.path.contains(':')
                || object.path.split('/').count() > MAX_OBJECT_PATH_COMPONENTS
                || object.path.split('/').any(|component| {
                    component.len() > MAX_OBJECT_PATH_COMPONENT_BYTES || unsafe_component(component)
                })
            {
                return Err(ManifestError::UnsafeObject);
            }
            // Unicode lowercasing alone misses case-insensitive aliases
            // such as Greek sigma/final sigma and long-s/ASCII-s. Check
            // both folds conservatively before touching the filesystem.
            if !paths_lower.insert(object.path.to_lowercase())
                || !paths_upper.insert(object.path.to_uppercase())
            {
                return Err(ManifestError::DuplicateObject);
            }
            let mut prefix = String::new();
            for component in object.path.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(component);
                if prefixes_lower
                    .insert(prefix.to_lowercase(), prefix.clone())
                    .is_some_and(|previous| previous != prefix)
                    || prefixes_upper
                        .insert(prefix.to_uppercase(), prefix.clone())
                        .is_some_and(|previous| previous != prefix)
                {
                    return Err(ManifestError::DuplicateObject);
                }
            }
            // One content digest must have one consistent byte length.
            if digest_sizes
                .insert(object.sha256.to_ascii_lowercase(), object.bytes)
                .is_some_and(|previous| previous != object.bytes)
            {
                return Err(ManifestError::ConflictingDigest);
            }
            if object.bytes > MAX_OBJECT_BYTES {
                return Err(ManifestError::ResourceBudget);
            }
            declared_bytes = declared_bytes
                .checked_add(object.bytes)
                .ok_or(ManifestError::ResourceBudget)?;
            if declared_bytes > MAX_DECLARED_BYTES {
                return Err(ManifestError::ResourceBudget);
            }
        }
        // A file object cannot also be a parent directory of another object.
        // Validate after collecting all names so input order cannot affect
        // acceptance, including on case-insensitive filesystems.
        for paths in [&paths_lower, &paths_upper] {
            for name in paths {
                let mut ancestor = name.as_str();
                while let Some((parent, _)) = ancestor.rsplit_once('/') {
                    if paths.contains(parent) {
                        return Err(ManifestError::DuplicateObject);
                    }
                    ancestor = parent;
                }
            }
        }
        self.validate_metadata()
    }

    fn validate_metadata(&self) -> Result<(), ManifestError> {
        let meta = &self.metadata;
        let invalid = Err(ManifestError::InvalidPackMetadata);
        if !safe_id(&meta.source.snapshot_id)
            || !lowercase_sha256(&meta.source.manifest_sha256)
            || !lowercase_sha256(&meta.definition_sha256)
            || !valid_spdx(&meta.license.text_spdx)
            || meta.indexes.len() > MAX_PACK_INDEXES
        {
            return invalid;
        }
        let declared: BTreeSet<&str> = self.objects.iter().map(|o| o.path.as_str()).collect();
        // Media attribution travels with the media it describes.
        match &meta.license.attribution_object {
            Some(path) if !declared.contains(path.as_str()) => return invalid,
            None if meta.counts.media > 0 => return invalid,
            _ => {}
        }
        let mut index_paths = BTreeSet::new();
        let mut roles = BTreeSet::new();
        for index in &meta.indexes {
            if index.schema == 0
                || !declared.contains(index.path.as_str())
                || !index_paths.insert(index.path.as_str())
                || !roles.insert(index.role)
                || meta.license.attribution_object.as_deref() == Some(index.path.as_str())
            {
                return invalid;
            }
        }
        if meta.counts.articles > MAX_PACK_OBJECTS as u64 * 1_000
            || meta.counts.redirects > MAX_PACK_OBJECTS as u64 * 1_000
            || meta.counts.media > MAX_PACK_OBJECTS as u64
        {
            return Err(ManifestError::ResourceBudget);
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
            metadata: crate::manifest::PackMetadata::synthetic_fixture(),
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
    fn pack_project_must_use_canonical_wikimedia_edition_identity() {
        let long = "a".repeat(256);
        for project in [
            "ENWIKI",
            "en-wiki",
            "en wiki",
            "_enwiki",
            "enwiki_",
            "éwiki",
            long.as_str(),
        ] {
            let mut invalid = sample();
            invalid.project = project.into();
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeMetadata));
            assert_eq!(
                invalid.identity_transcript(),
                Err(ManifestError::UnsafeMetadata)
            );
        }
        let mut valid = sample();
        valid.project = "zh_min_nanwiki".into();
        assert_eq!(valid.validate(), Ok(()));
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
    fn resource_budgets_reject_oversized_and_overflowing_pack_claims() {
        let mut invalid = sample();
        invalid.objects[0].bytes = MAX_OBJECT_BYTES + 1;
        assert_eq!(invalid.validate(), Err(ManifestError::ResourceBudget));
        assert_eq!(
            invalid.identity_transcript(),
            Err(ManifestError::ResourceBudget)
        );

        invalid = sample();
        invalid.objects.clear();
        for index in 0..5 {
            invalid.objects.push(Object {
                path: format!("records/{index}.pack"),
                sha256: "a".repeat(64),
                bytes: MAX_OBJECT_BYTES,
            });
        }
        assert_eq!(invalid.validate(), Err(ManifestError::ResourceBudget));

        invalid.objects.pop();
        assert_eq!(invalid.validate(), Ok(()));
    }

    #[test]
    fn rejects_bidi_controls_that_spoof_displayed_pack_names_or_paths() {
        let mut invalid = sample();
        invalid.pack_id = "science\u{202e}txt".into();
        assert_eq!(invalid.validate(), Err(ManifestError::UnsafeMetadata));
        invalid = sample();
        invalid.objects[0].path = "media/\u{2066}figure.svg".into();
        assert_eq!(invalid.validate(), Err(ManifestError::UnsafeObject));
    }

    #[test]
    fn rejects_invisible_and_noncharacter_pack_identifiers_and_object_paths() {
        for invisible in [
            "\u{200b}",
            "\u{2060}",
            "\u{2029}",
            "\u{206a}",
            "\u{feff}",
            "\u{fdd0}",
            "\u{ffff}",
            "\u{10ffff}",
        ] {
            let mut invalid = sample();
            invalid.pack_id = format!("science{invisible}topic");
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeMetadata));
            invalid = sample();
            invalid.snapshot = format!("2026{invisible}1009");
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeMetadata));
            invalid = sample();
            invalid.origin = Origin::Official {
                publisher: format!("Wiki{invisible}media"),
            };
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeMetadata));
            invalid = sample();
            invalid.origin = Origin::Custom {
                definition_id: format!("custom{invisible}topic"),
            };
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeMetadata));
            invalid = sample();
            invalid.objects[0].path = format!("media/{invisible}image.svg");
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeObject));
            assert_eq!(
                invalid.identity_transcript(),
                Err(ManifestError::UnsafeObject)
            );
        }
        let mut valid = sample();
        valid.pack_id = "می\u{200c}روم".into();
        valid.objects[0].path = "media/क्\u{200d}ष.png".into();
        assert_eq!(valid.validate(), Ok(()));
    }

    #[test]
    fn rejects_windows_reserved_names_and_invalid_filename_characters() {
        for reserved in [
            "CON",
            "con.txt",
            "PRN",
            "AUX.svg",
            "NUL",
            "COM1",
            "com9.log",
            "LPT1",
            "LPT9.txt",
            "COM¹",
            "COM².txt",
            "COM³",
            "LPT¹",
            "LPT².png",
            "LPT³",
            "CONIN$",
            "CONOUT$.txt",
            "CON .txt",
            "con  .svg",
            "NUL .json",
            "COM1 .bin",
            "com9  .log",
            "LPT² .png",
            "CONIN$ .txt",
            "CONOUT$  .dat",
        ] {
            let mut invalid = sample();
            invalid.objects[0].path = format!("media/{reserved}");
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeObject));
            assert_eq!(
                invalid.identity_transcript(),
                Err(ManifestError::UnsafeObject)
            );

            invalid = sample();
            invalid.pack_id = reserved.into();
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeMetadata));
        }

        for forbidden in ['<', '>', '"', '|', '?', '*'] {
            let mut invalid = sample();
            invalid.objects[0].path = format!("media/fig{forbidden}ure.svg");
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeObject));

            invalid = sample();
            invalid.origin = Origin::Custom {
                definition_id: format!("topic{forbidden}name"),
            };
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeMetadata));
        }

        let mut valid = sample();
        valid.objects[0].path = "media/compact-figures.svg".into();
        valid.pack_id = "scientific-figures".into();
        assert_eq!(valid.validate(), Ok(()));
        // Device-name prefixes that are not reserved must remain valid.
        valid.objects[0].path = "media/CONtent .svg".into();
        valid.pack_id = "COM10 .txt".into();
        assert_eq!(valid.validate(), Ok(()));
    }

    #[test]
    fn rejects_file_directory_path_collisions_regardless_of_input_order() {
        let mut invalid = sample();
        invalid.objects[0].path = "media".into();
        invalid.objects[1].path = "media/charts/chart.svg".into();
        assert_eq!(invalid.validate(), Err(ManifestError::DuplicateObject));
        assert_eq!(
            invalid.identity_transcript(),
            Err(ManifestError::DuplicateObject)
        );

        invalid.objects.reverse();
        assert_eq!(invalid.validate(), Err(ManifestError::DuplicateObject));
        invalid.objects[0].path = "MEDIA/charts/chart.svg".into();
        assert_eq!(invalid.validate(), Err(ManifestError::DuplicateObject));

        invalid.objects[1].path = "medial".into();
        assert_eq!(invalid.validate(), Ok(()));
    }

    #[test]
    fn rejects_unicode_case_equivalent_paths_and_directory_aliases() {
        let mut invalid = sample();
        invalid.objects[0].path = "media/Étage.svg".into();
        invalid.objects[1].path = "media/étage.svg".into();
        assert_eq!(invalid.validate(), Err(ManifestError::DuplicateObject));
        invalid.objects.reverse();
        assert_eq!(invalid.validate(), Err(ManifestError::DuplicateObject));

        invalid.objects[0].path = "media/Étage.svg/child".into();
        assert_eq!(invalid.validate(), Err(ManifestError::DuplicateObject));
        assert_eq!(
            invalid.identity_transcript(),
            Err(ManifestError::DuplicateObject)
        );

        let mut valid = sample();
        valid.objects[0].path = "media/étage.svg".into();
        valid.objects[1].path = "media/etage.svg".into();
        assert_eq!(valid.validate(), Ok(()));
    }

    #[test]
    fn rejects_uppercase_equivalent_leaf_and_directory_names() {
        for (left, right) in [
            ("media/σ.svg", "media/ς.svg"),
            ("media/ſ.svg", "media/s.svg"),
        ] {
            let mut invalid = sample();
            invalid.objects[0].path = left.into();
            invalid.objects[1].path = right.into();
            assert_eq!(invalid.validate(), Err(ManifestError::DuplicateObject));
            invalid.objects.reverse();
            assert_eq!(invalid.validate(), Err(ManifestError::DuplicateObject));
            assert_eq!(
                invalid.identity_transcript(),
                Err(ManifestError::DuplicateObject)
            );
        }

        for (left, right) in [
            ("media/σ/a.svg", "media/ς/b.svg"),
            ("media/É/a.svg", "media/é/b.svg"),
        ] {
            let mut invalid = sample();
            invalid.objects[0].path = left.into();
            invalid.objects[1].path = right.into();
            assert_eq!(invalid.validate(), Err(ManifestError::DuplicateObject));
            invalid.objects.reverse();
            assert_eq!(invalid.validate(), Err(ManifestError::DuplicateObject));
        }

        let mut valid = sample();
        valid.objects[0].path = "media/σ/a.svg".into();
        valid.objects[1].path = "media/τ/b.svg".into();
        assert_eq!(valid.validate(), Ok(()));
    }

    #[test]
    fn rejects_non_nfc_filesystem_names_without_rejecting_multilingual_nfc() {
        // macOS filesystems can resolve canonically equivalent Unicode names
        // to the same directory entry. Reject decomposed paths *before* staging
        // rather than allowing a manifest's signed identity to diverge from disk.
        let mut invalid = sample();
        invalid.objects[0].path = "media/cafe\u{301}.svg".into();
        assert_eq!(invalid.validate(), Err(ManifestError::UnsafeObject));
        assert_eq!(
            invalid.identity_transcript(),
            Err(ManifestError::UnsafeObject)
        );
        invalid = sample();
        invalid.objects[0].path = "re\u{301}cords/figure.svg".into();
        assert_eq!(invalid.validate(), Err(ManifestError::UnsafeObject));

        for name in ["cafe\u{301}", "A\u{30a}"] {
            let mut invalid = sample();
            invalid.pack_id = name.into();
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeMetadata));
            invalid = sample();
            invalid.snapshot = name.into();
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeMetadata));
            invalid = sample();
            invalid.origin = Origin::Official {
                publisher: name.into(),
            };
            assert_eq!(invalid.validate(), Err(ManifestError::UnsafeMetadata));
        }

        let mut valid = sample();
        valid.pack_id = "café".into();
        valid.objects[0].path = "media/café.svg".into();
        valid.objects[1].path = "articles/日本語.cbor".into();
        assert_eq!(valid.validate(), Ok(()));
    }

    #[test]
    fn conflicting_digest_lengths_fail_closed() {
        let mut bad = sample();
        bad.objects[1].sha256 = bad.objects[0].sha256.to_ascii_lowercase();
        assert_eq!(bad.validate(), Err(ManifestError::ConflictingDigest));
        bad.objects.reverse();
        assert_eq!(
            bad.identity_transcript(),
            Err(ManifestError::ConflictingDigest)
        );
        bad.objects[0].bytes = bad.objects[1].bytes;
        assert_eq!(bad.validate(), Ok(()));
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
