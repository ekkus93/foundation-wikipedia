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
fn rejects_reserved_device_identifiers_across_metadata_fields() {
    let mut manifest = sample();
    for name in ["CON", "nul.txt", "COM1", "LPT9.log", "physics."] {
        manifest.pack_id = name.into();
        assert_eq!(manifest.validate(), Err(ManifestError::UnsafeMetadata));
        manifest.pack_id = "physics".into();
        manifest.snapshot = name.into();
        assert_eq!(manifest.validate(), Err(ManifestError::UnsafeMetadata));
        manifest.snapshot = "20261001".into();
        manifest.origin = Origin::Custom {
            definition_id: name.into(),
        };
        assert_eq!(manifest.validate(), Err(ManifestError::UnsafeMetadata));
        manifest.origin = Origin::Custom {
            definition_id: "physics.toml".into(),
        };
    }
    assert_eq!(manifest.validate(), Ok(()));
}
