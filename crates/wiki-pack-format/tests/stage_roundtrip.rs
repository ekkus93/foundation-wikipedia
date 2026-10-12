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
    fs::remove_dir_all(parent).unwrap();
}
