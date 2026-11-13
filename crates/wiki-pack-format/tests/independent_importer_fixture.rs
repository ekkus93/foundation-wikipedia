//! PACK-001 cross-implementation fixture for the independent Python importer
//! (`scripts/wpack_importer.py`).
//!
//! `fixtures/synthetic/wpack/roundtrip-v2.wpack` must be exactly what this
//! Rust writer produces for the deterministic content below, and must verify
//! and stage in Rust. The Python test verifies the same bytes, extracts them
//! and rewrites a byte-identical envelope, so both implementations agree on
//! every byte of the v2 format. After an intentional format change,
//! regenerate with:
//!
//! ```text
//! WPACK_WRITE_FIXTURE=1 cargo test -p wiki-pack-format --test independent_importer_fixture
//! ```
//!
//! and update the fixture's `.provenance.json` SHA-256.

use std::fs;
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use wiki_pack_format::container::{verify_container, write_container};
use wiki_pack_format::manifest::{
    Counts, IndexEntry, IndexRole, License, Manifest, Object, Origin, PackMetadata, SourceKind,
    SourceSnapshot, FORMAT_VERSION,
};
use wiki_pack_format::stage::stage_container;

/// Deterministic objects: nested paths, a non-ASCII NFC name, index and
/// attribution objects. Bytes are arbitrary at the envelope layer.
fn objects() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "articles/0001.shard",
            b"FWREC001 synthetic shard frames".to_vec(),
        ),
        ("articles/0002.shard", vec![0u8, 1, 2, 3, 254, 255]),
        (
            "attribution.json",
            r#"[{"object":"media/Ωmega.svg","creator":"Fixture","license":"CC0-1.0"}]"#
                .as_bytes()
                .to_vec(),
        ),
        ("catalog.sqlite", b"synthetic catalog bytes".to_vec()),
        (
            "media/\u{03a9}mega.svg",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
        ),
        ("search.sqlite", b"synthetic fts5 bytes".to_vec()),
    ]
}

fn manifest() -> Manifest {
    Manifest {
        version: FORMAT_VERSION,
        pack_id: "roundtrip".into(),
        project: "enwiki".into(),
        snapshot: "20261101".into(),
        origin: Origin::Custom {
            definition_id: "roundtrip-def".into(),
        },
        metadata: PackMetadata {
            source: SourceSnapshot {
                kind: SourceKind::PublicDump,
                snapshot_id: "enwiki-20261101".into(),
                manifest_sha256: format!("{:x}", Sha256::digest(b"SHA256SUMS fixture")),
            },
            definition_sha256: format!("{:x}", Sha256::digest(b"roundtrip definition")),
            license: License {
                text_spdx: "CC-BY-SA-4.0".into(),
                attribution_object: Some("attribution.json".into()),
            },
            indexes: vec![
                IndexEntry {
                    path: "catalog.sqlite".into(),
                    role: IndexRole::Catalog,
                    schema: 1,
                },
                IndexEntry {
                    path: "search.sqlite".into(),
                    role: IndexRole::Fts5,
                    schema: 1,
                },
            ],
            counts: Counts {
                articles: 2,
                redirects: 1,
                media: 1,
            },
        },
        objects: objects()
            .into_iter()
            .map(|(path, bytes)| Object {
                path: path.into(),
                sha256: format!("{:x}", Sha256::digest(&bytes)),
                bytes: bytes.len() as u64,
            })
            .collect(),
    }
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/wpack/roundtrip-v2.wpack")
}

fn rust_envelope() -> Vec<u8> {
    let contents = objects();
    let mut bytes = Vec::new();
    write_container(&mut bytes, &manifest(), |object, out| {
        let (_, data) = contents
            .iter()
            .find(|(path, _)| *path == object.path)
            .unwrap();
        out.write_all(data)
    })
    .unwrap();
    bytes
}

#[test]
fn committed_fixture_is_exactly_the_rust_writer_output() {
    let expected = rust_envelope();
    if std::env::var_os("WPACK_WRITE_FIXTURE").is_some() {
        fs::create_dir_all(fixture_path().parent().unwrap()).unwrap();
        fs::write(fixture_path(), &expected).unwrap();
        eprintln!(
            "wrote {} sha256={:x}",
            fixture_path().display(),
            Sha256::digest(&expected)
        );
    }
    assert_eq!(
        fs::read(fixture_path()).unwrap(),
        expected,
        "fixture drifted from the Rust writer; regenerate it (see module docs)"
    );
}

#[test]
fn committed_fixture_verifies_and_stages_in_rust() {
    let bytes = fs::read(fixture_path()).unwrap();
    let parsed = verify_container(&mut bytes.as_slice()).unwrap();
    assert_eq!(parsed, manifest());

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let parent = std::env::temp_dir().join(format!("wpack-fixture-{}-{nonce}", std::process::id()));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&parent).unwrap();
    let (staged, root) =
        stage_container(&mut std::io::Cursor::new(&bytes), &parent, "roundtrip").unwrap();
    assert_eq!(staged, manifest());
    for (path, data) in objects() {
        assert_eq!(fs::read(root.join(path)).unwrap(), data, "{path}");
    }
    fs::remove_dir_all(parent).unwrap();
}
