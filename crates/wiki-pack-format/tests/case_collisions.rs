use wiki_pack_format::manifest::{Manifest, ManifestError, Object, Origin, FORMAT_VERSION};

#[test]
fn case_insensitive_paths_must_be_unique() {
    let first = Object {
        path: "articles/one.pack".into(),
        sha256: "a".repeat(64),
        bytes: 10,
    };
    let mut second = first.clone();
    second.path = "ARTICLES/ONE.PACK".into();
    let manifest = Manifest {
        version: FORMAT_VERSION,
        pack_id: "sample".into(),
        project: "enwiki".into(),
        snapshot: "20261009".into(),
        origin: Origin::Custom {
            definition_id: "sample".into(),
        },
        objects: vec![first, second],
    };
    assert_eq!(manifest.validate(), Err(ManifestError::DuplicateObject));
}
