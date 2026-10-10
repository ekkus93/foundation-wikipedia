use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use wiki_model::{ArticleKey, PageRecord, Redirect};
use wiki_store::record_codec::{append_record_frame, RecordCodecError};
use wiki_store::shard_io::{read_path, ShardError};

fn redirect() -> PageRecord {
    PageRecord::Redirect(Redirect {
        from: ArticleKey { project: "enwiki".into(), page_id: 10 },
        title: "A redirect".into(),
        to: ArticleKey { project: "enwiki".into(), page_id: 20 },
    })
}

fn temp_path() -> std::path::PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("wiki-shard-test-{}-{nanos}", std::process::id()))
}

#[test]
fn malformed_record_magic_and_truncation_are_rejected() {
    let path = temp_path();
    let mut shard = Vec::new();
    append_record_frame(&mut shard, &redirect()).unwrap();
    let length = shard.len() as u64;
    fs::write(&path, &shard).unwrap();
    assert_eq!(read_path(&path, 0, length).unwrap().record, redirect());

    shard[8] ^= 0xff;
    fs::write(&path, &shard).unwrap();
    assert!(matches!(
        read_path(&path, 0, length),
        Err(ShardError::Codec(RecordCodecError::InvalidMagic))
    ));

    shard.truncate(shard.len() - 1);
    fs::write(&path, &shard).unwrap();
    assert!(matches!(read_path(&path, 0, length), Err(ShardError::OutOfBounds)));
    fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_shard_is_rejected_without_following() {
    use std::os::unix::fs::symlink;
    let path = temp_path();
    let link = path.with_extension("link");
    let mut shard = Vec::new();
    append_record_frame(&mut shard, &redirect()).unwrap();
    fs::write(&path, &shard).unwrap();
    symlink(&path, &link).unwrap();
    assert!(matches!(
        read_path(&link, 0, shard.len() as u64),
        Err(ShardError::UnsafePath)
    ));
    fs::remove_file(link).unwrap();
    fs::remove_file(path).unwrap();
}
