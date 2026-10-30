use std::fs;
use std::io::Cursor;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use wiki_pack_format::manifest::{Manifest, Object, Origin, FORMAT_VERSION};
use wiki_pack_format::stage::stage_container;

#[test]
fn verified_envelope_stages_into_private_tree() {
    let parent = std::env::temp_dir().join(format!(
        "wpack-stage-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&parent).unwrap();

    let payload = b"article";
    let manifest = Manifest {
        version: FORMAT_VERSION,
        pack_id: "science".into(),
        project: "enwiki".into(),
        snapshot: "20261001".into(),
        origin: Origin::Custom {
            definition_id: "fixture".into(),
        },
        objects: vec![Object {
            path: "articles/1.cbor".into(),
            sha256: format!("{:x}", Sha256::digest(payload)),
            bytes: payload.len() as u64,
        }],
    };
    let transcript = manifest.identity_transcript().unwrap();
    let mut envelope = b"FWPACK01".to_vec();
    envelope.extend_from_slice(&(transcript.len() as u32).to_be_bytes());
    envelope.extend_from_slice(&transcript);
    envelope.extend_from_slice(payload);

    let (actual, directory) =
        stage_container(&mut Cursor::new(&envelope), &parent, "candidate").unwrap();
    assert_eq!(actual, manifest);
    assert_eq!(
        fs::read(directory.join("articles/1.cbor")).unwrap(),
        payload
    );
    assert!(directory.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o077,
            0
        );
    }
    // A mutable input may have grown after the initial complete-envelope
    // validation. Stage must reject a second-pass tail and delete all files.
    struct AppendAfterVerification {
        input: Cursor<Vec<u8>>,
        seeks: usize,
    }
    impl std::io::Read for AppendAfterVerification {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            std::io::Read::read(&mut self.input, buf)
        }
    }
    impl std::io::Seek for AppendAfterVerification {
        fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
            // The implementation may recheck the full transcript rather
            // than seeking directly to the first payload byte.
            if self.seeks == 1 {
                self.input.get_mut().push(0xff);
            }
            self.seeks += 1;
            std::io::Seek::seek(&mut self.input, pos)
        }
    }
    let mut changed = AppendAfterVerification {
        input: Cursor::new(envelope.clone()),
        seeks: 0,
    };
    assert!(matches!(
        stage_container(&mut changed, &parent, "changed"),
        Err(wiki_pack_format::container::ContainerError::TrailingBytes)
    ));
    assert!(!parent.join("changed").exists());

    fs::remove_dir_all(parent).unwrap();
}
