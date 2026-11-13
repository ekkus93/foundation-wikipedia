//! PACK-001 manifest v2: source snapshot, definition hash, licence, index
//! metadata and counts are validated and bound into the identity transcript.

use wiki_pack_format::container::{verify_container, write_container, ContainerError};
use wiki_pack_format::manifest::{
    Counts, IndexEntry, IndexRole, License, Manifest, ManifestError, Object, Origin, PackMetadata,
    SourceKind, SourceSnapshot, FORMAT_VERSION, TRANSCRIPT_PREFIX,
};

/// One mutation applied to otherwise valid metadata.
type Edit = Box<dyn Fn(&mut PackMetadata)>;

const CATALOG: &[u8] = b"catalog bytes";
const SEARCH: &[u8] = b"search bytes";
const SHARD: &[u8] = b"shard bytes";
const ATTRIBUTION: &[u8] = b"[]";

fn object(path: &str, bytes: &[u8]) -> Object {
    use sha2::{Digest, Sha256};
    Object {
        path: path.into(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len() as u64,
    }
}

fn full() -> Manifest {
    Manifest {
        version: FORMAT_VERSION,
        pack_id: "physics".into(),
        project: "enwiki".into(),
        snapshot: "20261101".into(),
        origin: Origin::Custom {
            definition_id: "physics-def".into(),
        },
        metadata: PackMetadata {
            source: SourceSnapshot {
                kind: SourceKind::PublicDump,
                snapshot_id: "enwiki-20261101".into(),
                manifest_sha256: "1".repeat(64),
            },
            definition_sha256: "2".repeat(64),
            license: License {
                text_spdx: "CC-BY-SA-4.0".into(),
                attribution_object: Some("attribution.json".into()),
            },
            indexes: vec![
                IndexEntry {
                    path: "search.sqlite".into(),
                    role: IndexRole::Fts5,
                    schema: 1,
                },
                IndexEntry {
                    path: "catalog.sqlite".into(),
                    role: IndexRole::Catalog,
                    schema: 1,
                },
            ],
            counts: Counts {
                articles: 120,
                redirects: 30,
                media: 4,
            },
        },
        objects: vec![
            object("articles.shard", SHARD),
            object("attribution.json", ATTRIBUTION),
            object("catalog.sqlite", CATALOG),
            object("search.sqlite", SEARCH),
        ],
    }
}

fn envelope(manifest: &Manifest) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_container(&mut bytes, manifest, |object, out| {
        out.write_all(match object.path.as_str() {
            "articles.shard" => SHARD,
            "attribution.json" => ATTRIBUTION,
            "catalog.sqlite" => CATALOG,
            _ => SEARCH,
        })
    })
    .unwrap();
    bytes
}

#[test]
fn full_metadata_round_trips_through_the_envelope() {
    let manifest = full();
    assert_eq!(manifest.validate(), Ok(()));
    let parsed = verify_container(&mut envelope(&manifest).as_slice()).unwrap();
    // Indexes come back in canonical (byte-wise path) order.
    let mut expected = manifest.metadata.clone();
    expected.indexes.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(parsed.metadata, expected);
    assert_eq!(parsed.version, 2);
    assert!(TRANSCRIPT_PREFIX.ends_with(b"/v2\0"));
}

#[test]
fn index_order_is_canonical_but_every_field_is_identity() {
    let base = full();
    let transcript = base.identity_transcript().unwrap();
    let mut reordered = full();
    reordered.metadata.indexes.reverse();
    assert_eq!(reordered.identity_transcript().unwrap(), transcript);

    let edits: Vec<Edit> = vec![
        Box::new(|m| m.source.kind = SourceKind::Enterprise),
        Box::new(|m| m.source.snapshot_id = "enwiki-20261102".into()),
        Box::new(|m| m.source.manifest_sha256 = "3".repeat(64)),
        Box::new(|m| m.definition_sha256 = "4".repeat(64)),
        Box::new(|m| m.license.text_spdx = "CC-BY-SA-3.0".into()),
        Box::new(|m| m.indexes[0].schema = 2),
        Box::new(|m| m.indexes.retain(|index| index.role != IndexRole::Fts5)),
        Box::new(|m| m.counts.articles += 1),
        Box::new(|m| m.counts.redirects += 1),
        Box::new(|m| m.counts.media += 1),
    ];
    for (n, edit) in edits.iter().enumerate() {
        let mut changed = full();
        edit(&mut changed.metadata);
        assert_eq!(changed.validate(), Ok(()), "edit {n}");
        assert_ne!(
            changed.identity_transcript().unwrap(),
            transcript,
            "edit {n}"
        );
    }
}

#[test]
fn invalid_metadata_is_rejected() {
    let edits: Vec<(&str, Edit)> = vec![
        (
            "uppercase source digest",
            Box::new(|m| m.source.manifest_sha256 = "A".repeat(64)),
        ),
        (
            "short definition digest",
            Box::new(|m| m.definition_sha256 = "2".repeat(63)),
        ),
        (
            "unsafe source snapshot",
            Box::new(|m| m.source.snapshot_id = "../x".into()),
        ),
        (
            "empty source snapshot",
            Box::new(|m| m.source.snapshot_id = String::new()),
        ),
        (
            "spdx with space",
            Box::new(|m| m.license.text_spdx = "CC BY-SA".into()),
        ),
        (
            "empty spdx",
            Box::new(|m| m.license.text_spdx = String::new()),
        ),
        (
            "undeclared attribution",
            Box::new(|m| m.license.attribution_object = Some("credits.json".into())),
        ),
        (
            "media without attribution",
            Box::new(|m| m.license.attribution_object = None),
        ),
        (
            "undeclared index",
            Box::new(|m| m.indexes[0].path = "other.sqlite".into()),
        ),
        ("schema zero", Box::new(|m| m.indexes[0].schema = 0)),
        (
            "duplicate role",
            Box::new(|m| m.indexes[0].role = IndexRole::Catalog),
        ),
        (
            "duplicate index path",
            Box::new(|m| m.indexes[0].path = "catalog.sqlite".into()),
        ),
        (
            "attribution is an index",
            Box::new(|m| m.license.attribution_object = Some("catalog.sqlite".into())),
        ),
    ];
    for (label, edit) in edits {
        let mut manifest = full();
        edit(&mut manifest.metadata);
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::InvalidPackMetadata),
            "{label}"
        );
        assert!(manifest.identity_transcript().is_err(), "{label}");
    }
    // No media and no attribution object is valid (text licence only).
    let mut text_only = full();
    text_only.metadata.counts.media = 0;
    text_only.metadata.license.attribution_object = None;
    assert_eq!(text_only.validate(), Ok(()));
}

#[test]
fn unknown_tags_in_the_transcript_are_rejected() {
    let manifest = full();
    let bytes = envelope(&manifest);
    let transcript = manifest.identity_transcript().unwrap();
    // Offset of the source-kind tag: prefix, version, three identity
    // fields, origin tag and definition-id field.
    let mut offset = TRANSCRIPT_PREFIX.len() + 4;
    for field in ["physics", "enwiki", "20261101"] {
        offset += 8 + field.len();
    }
    offset += 1 + 8 + "physics-def".len();
    assert_eq!(transcript[offset], SourceKind::PublicDump.tag());
    for tag in [0u8, 4, 0xff] {
        let mut forged = bytes.clone();
        forged[12 + offset] = tag;
        assert!(
            matches!(
                verify_container(&mut forged.as_slice()),
                Err(ContainerError::InvalidTranscript)
            ),
            "source tag {tag}"
        );
    }
}
