use wiki_pack_format::manifest::{Manifest, ManifestError, Object, Origin, FORMAT_VERSION};

fn sample() -> Manifest {
    Manifest {
        version: FORMAT_VERSION,
        pack_id: "physics".into(),
        project: "enwiki".into(),
        snapshot: "20261001".into(),
        origin: Origin::Custom {
            definition_id: "physics.toml".into(),
        },
        objects: vec![Object {
            path: "articles/01.pack".into(),
            sha256: "a".repeat(64),
            bytes: 2048,
        }],
    }
}

#[test]
fn custom_manifest_is_well_formed_but_not_publisher_verified() {
    assert!(sample().validate().is_ok());
    let mut official = sample();
    official.origin = Origin::Official {
        publisher: "Example Publisher".into(),
    };
    assert!(official.validate().is_ok());
}

#[test]
fn duplicate_paths_and_unsafe_names_fail() {
    let mut bad = sample();
    bad.objects.push(bad.objects[0].clone());
    assert_eq!(bad.validate(), Err(ManifestError::DuplicateObject));
    bad.objects[1].path = "../outside".into();
    assert_eq!(bad.validate(), Err(ManifestError::UnsafeObject));
}

#[test]
fn invalid_hash_and_version_fail() {
    let mut bad = sample();
    bad.version = FORMAT_VERSION + 1;
    assert_eq!(bad.validate(), Err(ManifestError::Version));
    bad.version = FORMAT_VERSION;
    bad.objects[0].sha256 = "invalid".into();
    assert_eq!(bad.validate(), Err(ManifestError::UnsafeObject));
}

#[test]
fn control_characters_and_blank_metadata_are_rejected() {
    let mut bad = sample();
    bad.objects[0].path = "articles/bad\0.pack".into();
    assert_eq!(bad.validate(), Err(ManifestError::UnsafeObject));
    bad.objects[0].path = "articles/bad\n.pack".into();
    assert_eq!(bad.validate(), Err(ManifestError::UnsafeObject));

    let mut bad = sample();
    bad.pack_id = "   ".into();
    assert_eq!(bad.validate(), Err(ManifestError::MissingMetadata));
    bad.pack_id = "physics".into();
    bad.origin = Origin::Official {
        publisher: "  ".into(),
    };
    assert_eq!(bad.validate(), Err(ManifestError::MissingMetadata));
}
