use wiki_model::{ArticleKey, PageRecord, Redirect};
use wiki_store::record_codec::{append_record_frame, RecordCodecError};
use wiki_store::shard_io::{read_at, ShardError};

#[test]
fn corrupt_record_is_rejected() {
    let record = PageRecord::Redirect(Redirect {
        from: ArticleKey {
            project: "enwiki".into(),
            page_id: 10,
        },
        title: "Alias".into(),
        to: ArticleKey {
            project: "enwiki".into(),
            page_id: 20,
        },
    });
    let mut bytes = Vec::new();
    append_record_frame(&mut bytes, &record).unwrap();
    bytes[8] ^= 0xff;
    let path = std::env::temp_dir().join(format!("wiki-shard-{}", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let mut file = std::fs::File::open(&path).unwrap();
    assert!(matches!(
        read_at(&mut file, 0, bytes.len() as u64),
        Err(ShardError::Codec(RecordCodecError::InvalidMagic))
    ));
    std::fs::remove_file(path).unwrap();
}
