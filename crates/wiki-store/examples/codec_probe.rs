use std::hint::black_box;
use std::time::Instant;

use wiki_model::{
    Article, ArticleKey, Block, BlockContent, Footnote, MediaAsset, PageRecord, Reference,
    Revision, Section, ARTICLE_SCHEMA_VERSION,
};
use wiki_store::record_codec::{
    append_record_frame, decode_record, decode_record_at, encode_record, MigrationStatus,
    CURRENT_CODEC_VERSION, MAX_UNCOMPRESSED_RECORD_BYTES,
};

fn fixture(page_id: u64) -> PageRecord {
    PageRecord::Article(Box::new(Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id,
        },
        revision: Revision {
            revision_id: 20261010,
            timestamp: "2026-10-10T00:00:00Z".into(),
            content_sha256: "a".repeat(64),
        },
        title: format!("Representative science article {page_id}"),
        display_title: "Representative science article".into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec!["Physics example".into(), "Unicode fixture".into()],
        wikidata_id: Some("Q1140".into()),
        lead: vec![
            Block {
                ordinal: 0,
                content: BlockContent::Paragraph(
                    "Gravity bends spacetime. Gravité, energía, 质量, and 🌍 exercise Unicode."
                        .repeat(12),
                ),
            },
            Block {
                ordinal: 1,
                content: BlockContent::Infobox(vec![
                    ("Field".into(), "Physics".into()),
                    ("Unit".into(), "m/s²".into()),
                ]),
            },
            Block {
                ordinal: 2,
                content: BlockContent::Footnote(Footnote {
                    source_id: "cite-note-1".into(),
                    label: "1".into(),
                    text: "A Wikipedia footnote is article-local evidence.".into(),
                    reference_ids: vec!["ref-1".into()],
                }),
            },
        ],
        sections: vec![Section {
            ordinal: 1,
            heading: "Theory".into(),
            blocks: vec![
                Block {
                    ordinal: 0,
                    content: BlockContent::Math {
                        source: "E = mc²".into(),
                        html: "<math>E = mc²</math>".into(),
                    },
                },
                Block {
                    ordinal: 1,
                    content: BlockContent::Table(vec![
                        vec!["Quantity".into(), "Value".into()],
                        vec!["g".into(), "9.81 m/s²".into()],
                    ]),
                },
                Block {
                    ordinal: 2,
                    content: BlockContent::List(vec![
                        "First structured point".into(),
                        "Second structured point".into(),
                        "Third structured point".into(),
                    ]),
                },
            ],
            subsections: vec![Section {
                ordinal: 2,
                heading: "Nested evidence".into(),
                blocks: vec![Block {
                    ordinal: 0,
                    content: BlockContent::Quote(
                        "Representative nested quotation for random-read fixtures.".into(),
                    ),
                }],
                subsections: vec![],
            }],
        }],
        references: vec![Reference {
            id: "ref-1".into(),
            label: "Example reference".into(),
            source_url: Some("https://example.org/reference".into()),
        }],
        links: vec![],
        media: vec![MediaAsset {
            source_url: "https://upload.wikimedia.org/example.svg".into(),
            mime_type: "image/svg+xml".into(),
            sha256: Some("b".repeat(64)),
            license: "CC BY-SA 4.0".into(),
            creator: "Example contributor".into(),
            attribution: "Example contributor — CC BY-SA 4.0".into(),
            is_av_preview: false,
        }],
        rendered_html: "<article><h1>Representative science article</h1><p>Gravity bends spacetime.</p></article>".into(),
        is_disambiguation: false,
    }))
}

fn vm_hwm_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        let rest = line.strip_prefix("VmHWM:")?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

fn main() {
    let record = fixture(42);
    let msgpack = rmp_serde::to_vec_named(&record).expect("MessagePack fixture");
    let mut cbor = Vec::new();
    ciborium::ser::into_writer(&record, &mut cbor).expect("CBOR fixture");
    let msgpack_zstd = zstd::stream::encode_all(msgpack.as_slice(), 3).expect("MessagePack zstd");
    let cbor_zstd = zstd::stream::encode_all(cbor.as_slice(), 3).expect("CBOR zstd");

    const FORMAT_ITERATIONS: usize = 200;
    let msgpack_encode_start = Instant::now();
    for _ in 0..FORMAT_ITERATIONS {
        black_box(rmp_serde::to_vec_named(black_box(&record)).expect("MessagePack encode"));
    }
    let msgpack_encode_us = msgpack_encode_start.elapsed().as_micros();

    let cbor_encode_start = Instant::now();
    for _ in 0..FORMAT_ITERATIONS {
        let mut candidate = Vec::new();
        ciborium::ser::into_writer(black_box(&record), &mut candidate).expect("CBOR encode");
        black_box(candidate);
    }
    let cbor_encode_us = cbor_encode_start.elapsed().as_micros();

    let msgpack_decode_start = Instant::now();
    for _ in 0..FORMAT_ITERATIONS {
        let decoded: PageRecord =
            rmp_serde::from_slice(black_box(&msgpack)).expect("MessagePack decode");
        black_box(decoded);
    }
    let msgpack_decode_us = msgpack_decode_start.elapsed().as_micros();

    let cbor_decode_start = Instant::now();
    for _ in 0..FORMAT_ITERATIONS {
        let decoded: PageRecord =
            ciborium::de::from_reader(black_box(cbor.as_slice())).expect("CBOR decode");
        black_box(decoded);
    }
    let cbor_decode_us = cbor_decode_start.elapsed().as_micros();

    const ITERATIONS: usize = 200;
    let encode_start = Instant::now();
    let mut encoded = Vec::new();
    for _ in 0..ITERATIONS {
        encoded = black_box(encode_record(black_box(&record)).expect("canonical encode"));
    }
    let encode_us = encode_start.elapsed().as_micros();

    let decode_start = Instant::now();
    for _ in 0..ITERATIONS {
        black_box(decode_record(black_box(&encoded)).expect("canonical decode"));
    }
    let decode_us = decode_start.elapsed().as_micros();

    const SHARD_RECORDS: usize = 128;
    let mut shard = Vec::new();
    let mut offsets = Vec::with_capacity(SHARD_RECORDS);
    for index in 0..SHARD_RECORDS {
        offsets.push(
            append_record_frame(&mut shard, &fixture(1_000 + index as u64))
                .expect("append shard record"),
        );
    }
    const RANDOM_READS: usize = 1_000;
    let random_start = Instant::now();
    for index in 0..RANDOM_READS {
        let offset = offsets[(index * 73) % SHARD_RECORDS];
        black_box(decode_record_at(black_box(&shard), offset).expect("random read"));
    }
    let random_us = random_start.elapsed().as_micros();

    let mut legacy = encoded.clone();
    legacy[10..12].copy_from_slice(&0u16.to_le_bytes());
    let migrated = decode_record(&legacy).expect("legacy v1.0 decode");
    assert!(matches!(
        migrated.migration,
        MigrationStatus::RewriteToCurrent { .. }
    ));
    let mut future_minor = encoded.clone();
    future_minor[10..12].copy_from_slice(&(CURRENT_CODEC_VERSION.minor + 1).to_le_bytes());
    assert!(decode_record(&future_minor).is_err());

    println!(
        "codec_probe target_os={} target_arch={}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!(
        "codec_probe fixture msgpack_bytes={} cbor_bytes={} msgpack_zstd_bytes={} cbor_zstd_bytes={}",
        msgpack.len(),
        cbor.len(),
        msgpack_zstd.len(),
        cbor_zstd.len()
    );
    println!(
        "codec_probe format_compare iterations={} msgpack_encode_us={} cbor_encode_us={} msgpack_decode_us={} cbor_decode_us={}",
        FORMAT_ITERATIONS,
        msgpack_encode_us,
        cbor_encode_us,
        msgpack_decode_us,
        cbor_decode_us
    );
    println!(
        "codec_probe roundtrip iterations={} encode_us={} decode_us={}",
        ITERATIONS, encode_us, decode_us
    );
    println!(
        "codec_probe shard records={} shard_bytes={} random_reads={} random_read_us={}",
        SHARD_RECORDS,
        shard.len(),
        RANDOM_READS,
        random_us
    );
    println!(
        "codec_probe bounds max_uncompressed_record_bytes={} vm_hwm_kib={}",
        MAX_UNCOMPRESSED_RECORD_BYTES,
        vm_hwm_kib()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unavailable".into())
    );
    println!(
        "codec_probe migration legacy=1.0 action=rewrite_to_{}.{} future_minor=rejected future_major=rejected",
        CURRENT_CODEC_VERSION.major, CURRENT_CODEC_VERSION.minor
    );
}
