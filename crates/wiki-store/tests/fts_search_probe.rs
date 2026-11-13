//! STORE-004 measurement probe behind ADR 0006 (SQLite FTS5 for pack search).
//!
//! Ignored by default because it builds a large synthetic pack. Run with:
//!
//! ```text
//! WIKI_FTS_PROBE_ARTICLES=20000 cargo test -p wiki-store --release \
//!     --test fts_search_probe -- --ignored --nocapture
//! ```
//!
//! It measures the real production path: canonical shard frames, the SQLite
//! catalog, `replace_persisted_from_catalog` (including its full membership
//! and native FTS5 integrity audits), verified read-only opening, and
//! `search_articles` through `OfflineReader`.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, PageRecord, Revision, Section, ARTICLE_SCHEMA_VERSION,
};
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::offline_reader::OfflineReader;
use wiki_store::record_codec::append_record_frame;
use wiki_store::search_index::OfflineSearchIndex;

/// Deterministic xorshift so every run indexes identical text.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Roughly Zipf-shaped word choice over a 20,000-term vocabulary.
    fn word(&mut self) -> String {
        let a = self.next() % 20_000 + 1;
        let b = self.next() % a + 1;
        format!("term{b}")
    }

    fn sentence(&mut self, words: usize) -> String {
        (0..words)
            .map(|_| self.word())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn article(page_id: u64, rng: &mut Rng) -> Article {
    let paragraph = |rng: &mut Rng, ordinal| Block {
        ordinal,
        content: BlockContent::Paragraph(rng.sentence(60)),
    };
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id,
        },
        revision: Revision {
            revision_id: 1_000_000 + page_id,
            timestamp: "2026-11-01T00:00:00Z".into(),
            content_sha256: "e".repeat(64),
        },
        title: format!("Topic {page_id} {}", rng.word()),
        display_title: format!("Topic {page_id}"),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![paragraph(rng, 0), paragraph(rng, 1)],
        sections: (0..3)
            .map(|s| Section {
                ordinal: s,
                heading: format!("Heading {s}"),
                blocks: vec![paragraph(rng, 0), paragraph(rng, 1)],
                subsections: vec![],
            })
            .collect(),
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: format!("<article>Topic {page_id}</article>"),
        is_disambiguation: false,
    }
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let index = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[index]
}

#[test]
#[ignore = "measurement probe; run explicitly in --release"]
fn fts5_pack_search_probe() {
    let articles: u64 = std::env::var("WIKI_FTS_PROBE_ARTICLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5_000);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root: PathBuf =
        std::env::temp_dir().join(format!("wiki-fts-probe-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&root).unwrap();

    let mut rng = Rng(0x5eed_1234_abcd_0001);
    let mut shard = Vec::new();
    let mut frames = Vec::new();
    for page_id in 1..=articles {
        let offset = shard.len() as u64;
        append_record_frame(
            &mut shard,
            &PageRecord::Article(Box::new(article(page_id, &mut rng))),
        )
        .unwrap();
        frames.push((offset, shard.len() as u64 - offset));
    }
    fs::write(root.join("articles.shard"), &shard).unwrap();

    let started = Instant::now();
    let mut catalog = SnapshotCatalog::open(&root.join("catalog.sqlite")).unwrap();
    for (offset, bytes) in &frames {
        catalog
            .insert_verified(&root, "articles.shard", *offset, *bytes)
            .unwrap();
    }
    let catalog_time = started.elapsed();

    let index_path = root.join("search.sqlite");
    let started = Instant::now();
    OfflineSearchIndex::replace_persisted_from_catalog(&index_path, &catalog, &root).unwrap();
    let index_time = started.elapsed();
    drop(catalog);

    let started = Instant::now();
    let catalog_ro = SnapshotCatalog::open_read_only(&root.join("catalog.sqlite")).unwrap();
    let index =
        OfflineSearchIndex::open_verified_read_only(&index_path, &catalog_ro, &root).unwrap();
    let verified_open_time = started.elapsed();
    drop(index);
    drop(catalog_ro);

    let catalog_ro = SnapshotCatalog::open_read_only(&root.join("catalog.sqlite")).unwrap();
    let index = OfflineSearchIndex::open_read_only(&index_path).unwrap();
    let reader = OfflineReader::new(&catalog_ro, &index, &root);

    // Three query shapes: a term present in nearly every article, two
    // mid-frequency terms, and one rare term. Each is timed through the raw
    // FTS5 ranking (`OfflineSearchIndex::search`) and through the reader
    // (`search_articles`), which also decodes and verifies each hit's frame.
    let shapes = ["common", "mid-pair", "rare"];
    let mut raw = vec![Vec::new(); 3];
    let mut verified = vec![Vec::new(); 3];
    let mut hits_by_shape = [0usize; 3];
    let mut query_rng = Rng(0x0dd_ba11_cafe_0002);
    for q in 0..450 {
        let shape = q % 3;
        let query = match shape {
            0 => "term1".to_owned(),
            1 => format!(
                "term{} term{}",
                query_rng.next() % 50 + 1,
                query_rng.next() % 500 + 1
            ),
            _ => format!("term{}", query_rng.next() % 20_000 + 1),
        };
        let started = Instant::now();
        let ranked = index.search(&query, 20).unwrap();
        raw[shape].push(started.elapsed());
        let started = Instant::now();
        let hits = reader.search_articles(&query, 20).unwrap();
        verified[shape].push(started.elapsed());
        assert_eq!(ranked.len(), hits.len());
        hits_by_shape[shape] += hits.len();
    }

    let size = |name: &str| fs::metadata(root.join(name)).unwrap().len();
    eprintln!("fts probe: articles={articles}");
    eprintln!(
        "fts probe: shard_bytes={} catalog_bytes={} index_bytes={}",
        size("articles.shard"),
        size("catalog.sqlite"),
        size("search.sqlite")
    );
    eprintln!(
        "fts probe: catalog_build_ms={} index_build_ms={} verified_open_ms={}",
        catalog_time.as_millis(),
        index_time.as_millis(),
        verified_open_time.as_millis()
    );
    for shape in 0..3 {
        raw[shape].sort();
        verified[shape].sort();
        eprintln!(
            "fts probe: {:<8} limit=20 raw_p50_us={} raw_p95_us={} \
             verified_p50_us={} verified_p95_us={} mean_hits={:.1}",
            shapes[shape],
            percentile(&raw[shape], 0.50).as_micros(),
            percentile(&raw[shape], 0.95).as_micros(),
            percentile(&verified[shape], 0.50).as_micros(),
            percentile(&verified[shape], 0.95).as_micros(),
            hits_by_shape[shape] as f64 / raw[shape].len() as f64
        );
    }
    assert!(hits_by_shape.iter().all(|&hits| hits > 0));
    drop(index);
    drop(catalog_ro);
    fs::remove_dir_all(root).unwrap();
}
