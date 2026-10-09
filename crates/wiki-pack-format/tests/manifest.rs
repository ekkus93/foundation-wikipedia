use wiki_pack_format::manifest::{
    Manifest, ManifestError, Object, Origin, FORMAT_VERSION, MAX_OBJECT_PATH_BYTES,
    MAX_OBJECT_PATH_COMPONENTS, MAX_OBJECT_PATH_COMPONENT_BYTES,
};

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

#[test]
fn unsafe_pack_and_snapshot_identifiers_fail_closed() {
    let mut bad = sample();
    for id in ["../escape", "bad\\name", "bad:name", ".", "..", " padded "] {
        bad.pack_id = id.into();
        assert_eq!(bad.validate(), Err(ManifestError::UnsafeMetadata));
    }
    bad.pack_id = "physics".into();
    bad.project = "en/wiki".into();
    assert_eq!(bad.validate(), Err(ManifestError::UnsafeMetadata));
    bad.project = "enwiki".into();
    bad.snapshot = "2026/10/09".into();
    assert_eq!(bad.validate(), Err(ManifestError::UnsafeMetadata));
}

#[test]
fn object_paths_have_bounded_length_depth_and_components() {
    let mut bad = sample();
    bad.objects[0].path = "a".repeat(MAX_OBJECT_PATH_BYTES + 1);
    assert_eq!(bad.validate(), Err(ManifestError::UnsafeObject));

    bad = sample();
    bad.objects[0].path = format!("{}/x", "a".repeat(MAX_OBJECT_PATH_COMPONENT_BYTES + 1));
    assert_eq!(bad.validate(), Err(ManifestError::UnsafeObject));

    bad = sample();
    bad.objects[0].path = std::iter::repeat_n("a", MAX_OBJECT_PATH_COMPONENTS + 1)
        .collect::<Vec<_>>()
        .join("/");
    assert_eq!(bad.validate(), Err(ManifestError::UnsafeObject));

    let mut boundary = sample();
    boundary.objects[0].path = std::iter::repeat_n("a", MAX_OBJECT_PATH_COMPONENTS)
        .collect::<Vec<_>>()
        .join("/");
    assert!(boundary.objects[0].path.len() <= MAX_OBJECT_PATH_BYTES);
    assert_eq!(boundary.validate(), Ok(()));
}

#[test]
fn origin_metadata_is_not_a_trusted_filesystem_path() {
    let mut manifest = sample();
    for invalid in ["../escape", "bad\\\\name", "bad:name", " padded ", ".."] {
        manifest.origin = Origin::Custom {
            definition_id: invalid.into(),
        };
        assert_eq!(manifest.validate(), Err(ManifestError::UnsafeMetadata));
        manifest.origin = Origin::Official {
            publisher: invalid.into(),
        };
        assert_eq!(manifest.validate(), Err(ManifestError::UnsafeMetadata));
    }
    manifest.origin = Origin::Official {
        publisher: "Example Publisher".into(),
    };
    assert_eq!(manifest.validate(), Ok(()));
}
