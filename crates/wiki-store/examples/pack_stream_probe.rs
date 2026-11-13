//! PACK-001 streamed export/install probe, run on the Android emulator by
//! `.github/workflows/android-codec-probe.yml` and runnable on any host:
//!
//! ```text
//! cargo run -p wiki-store --example pack_stream_probe --release [-- <work-dir>]
//! ```
//!
//! It exports a snapshot-layout tree as a `.wpack` by streaming into a file,
//! installs it through `install_custom_pack_stream` from a read-only,
//! non-seekable reader (as an Android document-provider stream is), activates
//! and searches it, and proves a tampered stream and an over-budget stream
//! fail closed with nothing published. Every check panics on failure; the
//! printed `pack_stream_probe` lines are the evidence the workflow greps.

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use wiki_model::{
    Article, ArticleKey, Block, BlockContent, PageRecord, Revision, Section, ARTICLE_SCHEMA_VERSION,
};
use wiki_pack_format::export::{custom_manifest_from_tree, write_custom_tree};
use wiki_store::activation::ActiveSnapshotStore;
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::offline_reader::VerifiedOfflineCatalogReader;
use wiki_store::pack_install::{install_custom_pack_stream, PackInstallError};
use wiki_store::record_codec::append_record_frame;

const ARTICLES: u64 = 200;

/// Read-only adapter: hides `Seek`, like a content-provider input stream.
struct StreamOnly<R>(R);

impl<R: Read> Read for StreamOnly<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

fn article(page_id: u64) -> Article {
    let paragraph = |ordinal: u32, text: String| Block {
        ordinal,
        content: BlockContent::Paragraph(text),
    };
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id,
        },
        revision: Revision {
            revision_id: 5_000_000 + page_id,
            timestamp: "2026-11-01T00:00:00Z".into(),
            content_sha256: "f".repeat(64),
        },
        title: format!("Orbit {page_id}"),
        display_title: format!("Orbit {page_id}"),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![paragraph(
            0,
            format!("Orbit {page_id} describes Kepler motion, gravité and 轨道 dynamics."),
        )],
        sections: vec![Section {
            ordinal: 0,
            heading: "Mechanics".into(),
            blocks: vec![paragraph(0, "Angular momentum is conserved. ".repeat(40))],
            subsections: vec![],
        }],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: format!("<article>Orbit {page_id}</article>"),
        is_disambiguation: false,
    }
}

/// Snapshot-layout tree without a search index: the installer rebuilds it.
fn build_tree(tree: &Path) {
    fs::create_dir_all(tree).unwrap();
    let mut shard = Vec::new();
    let mut frames = Vec::new();
    for page_id in 1..=ARTICLES {
        let offset = shard.len() as u64;
        append_record_frame(&mut shard, &PageRecord::Article(Box::new(article(page_id)))).unwrap();
        frames.push((offset, shard.len() as u64 - offset));
    }
    fs::write(tree.join("articles.shard"), &shard).unwrap();
    let mut catalog = SnapshotCatalog::open(&tree.join("catalog.sqlite")).unwrap();
    for (offset, bytes) in frames {
        catalog
            .insert_verified(tree, "articles.shard", offset, bytes)
            .unwrap();
    }
}

fn export(tree: &Path, pack_id: &str, output: &Path) -> u64 {
    let manifest =
        custom_manifest_from_tree(tree, pack_id, "enwiki", "20261101", "orbits").unwrap();
    let mut file = BufWriter::new(File::create(output).unwrap());
    write_custom_tree(tree, &manifest, &mut file).unwrap();
    file.into_inner().unwrap().sync_all().unwrap();
    fs::metadata(output).unwrap().len()
}

fn install_from(
    root: &Path,
    pack: &Path,
    budget: u64,
) -> Result<wiki_store::pack_install::InstalledPack, PackInstallError> {
    let mut stream = StreamOnly(File::open(pack).unwrap());
    install_custom_pack_stream(root, &mut stream, budget, |review| {
        assert!(review.rebuilt_search_index);
        true
    })
}

fn staging_is_empty(root: &Path) -> bool {
    fs::read_dir(root.join("staging")).unwrap().next().is_none()
}

fn main() {
    let base: PathBuf = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!(
            "foundation-pack-stream-probe-{}",
            std::process::id()
        ));
    fs::create_dir_all(&base).unwrap();
    println!(
        "pack_stream_probe target_os={} target_arch={}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    let tree = base.join("tree");
    build_tree(&tree);
    let pack = base.join("orbits.wpack");
    let started = Instant::now();
    let pack_bytes = export(&tree, "orbits", &pack);
    println!(
        "pack_stream_probe export articles={ARTICLES} wpack_bytes={pack_bytes} export_ms={}",
        started.elapsed().as_millis()
    );

    // Installs land in a separate root, as on a device importing a file.
    let root = base.join("root");
    fs::create_dir_all(&root).unwrap();

    // 1. A spool budget smaller than the envelope is refused before publication.
    let refused = install_from(&root, &pack, pack_bytes - 1);
    assert!(matches!(refused, Err(PackInstallError::Container(_))));
    assert!(!root.join("snapshots/orbits").exists() && staging_is_empty(&root));

    // 2. A stream with one flipped payload byte is refused the same way.
    let tampered = base.join("tampered.wpack");
    let mut bytes = fs::read(&pack).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    File::create(&tampered).unwrap().write_all(&bytes).unwrap();
    let refused_tamper = install_from(&root, &tampered, pack_bytes);
    assert!(matches!(
        refused_tamper,
        Err(PackInstallError::Container(_))
    ));
    assert!(!root.join("snapshots/orbits").exists() && staging_is_empty(&root));
    println!(
        "pack_stream_probe refusals budget=refused tampered=refused published=none staging=empty"
    );

    // 3. The genuine stream installs with an exact budget, then activates.
    let started = Instant::now();
    let installed = install_from(&root, &pack, pack_bytes).unwrap();
    let install_ms = started.elapsed().as_millis();
    assert!(staging_is_empty(&root));
    let store = ActiveSnapshotStore::new(&root);
    assert_eq!(store.current().unwrap(), None);
    store
        .switch_to_prevalidated(&installed.snapshot_id)
        .unwrap();
    assert_eq!(store.current().unwrap().as_deref(), Some("orbits"));

    let reader = VerifiedOfflineCatalogReader::open_with_manifest_shards(
        &installed.path.join("catalog.sqlite"),
        &installed.path.join("search.sqlite"),
        &installed.path,
        &installed.shards,
    )
    .unwrap();
    let key = ArticleKey {
        project: "enwiki".into(),
        page_id: 42,
    };
    let page = reader.reader().open_page(&key).unwrap().unwrap();
    assert_eq!(page.revision.revision_id, 5_000_042);
    let hits = reader.reader().search_articles("kepler", 50).unwrap();
    assert_eq!(hits.len(), 50);
    println!(
        "pack_stream_probe install stream=nonseekable trust={:?} rebuilt_index=true install_ms={install_ms} active=orbits page42_revision={} search_hits={}",
        installed.trust,
        page.revision.revision_id,
        hits.len()
    );
    drop(reader);

    fs::remove_dir_all(&base).unwrap();
    println!("pack_stream_probe result=pass");
}
