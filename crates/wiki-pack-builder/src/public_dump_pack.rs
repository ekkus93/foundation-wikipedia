//! SRC-003: build an installable custom pack from a public Wikimedia dump.
//!
//! 1. [`select_dump_pages`] streams one checksum-pinned dump member and finds
//!    the requested titles: the dump names each page's exact current
//!    revision (redirects resolve one hop to their target and become aliases).
//! 2. [`render_dump_article`] renders that exact revision with the free public
//!    Action API (`action=parse&oldid=`), refuses any reply whose page,
//!    revision or title differs from the dump, reads it into blocks, and
//!    resolves its required images to real Commons metadata. Image bytes are
//!    not downloaded (owner decision 2026-11-16): each image is a required
//!    `MediaAsset` with no `sha256`, so the pack does not include images.
//! 3. [`write_dump_pack`] writes the article shard, catalog and a custom
//!    `.wpack` whose source is the pinned dump generation.
//!
//! Paid Enterprise access is never used; network access is the caller's
//! [`MediaWikiTransport`], which must accept only canonical Action API URLs.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use wiki_model::{Article, ArticleKey, PageRecord};
use wiki_pack_format::export::{custom_manifest_from_tree, write_custom_tree};
use wiki_pack_format::manifest::{SourceKind, SourceSnapshot};
use wiki_source::online_article::{parse_rendered_revision_with_source, OnlineRevision};
use wiki_source::online_fetch::MediaWikiTransport;
use wiki_source::online_requests::{WikipediaRequests, MAX_IMAGE_TITLES};
use wiki_source::public_dump::DumpChecksum;
use wiki_source::public_dump_bzip2::decode_bzip2_xml_page_metadata_for_project;
use wiki_source::public_dump_page_meta::PublicDumpPage;
use wiki_source::rendered_article::{assemble_article, extract_rendered_article};
use wiki_source::rendered_media::parse_image_metadata;
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::pack_install::snapshot_pack_metadata;
use wiki_store::record_codec::append_record_frame;

/// Articles in one dump-built pack (the builder is for curated selections).
pub const MAX_DUMP_PACK_ARTICLES: usize = 500;
/// Main-namespace pages indexed from one dump member.
const MAX_INDEXED_PAGES: usize = 20_000_000;
const RENDERED_BYTES: usize = 8 * 1024 * 1024;
const METADATA_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub enum DumpPackError {
    InvalidRequest(String),
    /// The dump member failed its checksum or could not be decoded.
    Dump(String),
    /// Requested titles that the dump does not contain as articles.
    NotInDump(Vec<String>),
    /// A title could not be rendered or read; names the title.
    Render {
        title: String,
        reason: String,
    },
    Write(String),
}

impl std::fmt::Display for DumpPackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(reason) => write!(f, "invalid request: {reason}"),
            Self::Dump(reason) => write!(f, "dump member rejected: {reason}"),
            Self::NotInDump(titles) => {
                write!(f, "not articles in this dump: {}", titles.join(", "))
            }
            Self::Render { title, reason } => write!(f, "could not build \"{title}\": {reason}"),
            Self::Write(reason) => write!(f, "could not write the pack: {reason}"),
        }
    }
}

/// A requested article as the dump names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DumpSelection {
    pub page: PublicDumpPage,
    /// Requested titles that are redirects to this page.
    pub aliases: Vec<String>,
}

fn title_key(title: &str) -> String {
    let title = title.trim().replace('_', " ");
    let mut chars = title.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Stream `member` (verified against its SHA-256 before anything is
/// returned) and select `titles`. Fails if any title is not an article.
pub fn select_dump_pages<R: Read>(
    input: R,
    member: &DumpChecksum,
    project: &str,
    titles: &[String],
    max_compressed_bytes: u64,
    max_decoded_bytes: u64,
) -> Result<Vec<DumpSelection>, DumpPackError> {
    if titles.is_empty() || titles.len() > MAX_DUMP_PACK_ARTICLES {
        return Err(DumpPackError::InvalidRequest(format!(
            "between 1 and {MAX_DUMP_PACK_ARTICLES} titles are required"
        )));
    }
    let mut pages: HashMap<String, PublicDumpPage> = HashMap::new();
    decode_bzip2_xml_page_metadata_for_project(
        input,
        member,
        project,
        max_compressed_bytes,
        max_decoded_bytes,
        |page| {
            if page.namespace == 0 {
                if pages.len() >= MAX_INDEXED_PAGES {
                    return Err(io::Error::other("too many pages"));
                }
                pages.insert(title_key(&page.title), page);
            }
            Ok(())
        },
    )
    .map_err(|error| DumpPackError::Dump(format!("{error:?}")))?;
    // Only now, after the whole member matched its checksum, use its pages.
    let mut selected: BTreeMap<u64, DumpSelection> = BTreeMap::new();
    let mut order = Vec::new();
    let mut missing = Vec::new();
    for title in titles {
        let mut page = pages.get(&title_key(title));
        let mut alias = None;
        if let Some(target) = page.and_then(|found| found.redirect_target.as_ref()) {
            alias = page.map(|found| found.title.clone());
            page = pages.get(&title_key(target));
        }
        match page.filter(|found| found.redirect_target.is_none()) {
            Some(found) => {
                let entry = selected.entry(found.page_id).or_insert_with(|| {
                    order.push(found.page_id);
                    DumpSelection {
                        page: found.clone(),
                        aliases: Vec::new(),
                    }
                });
                if let Some(alias) = alias {
                    if !entry.aliases.contains(&alias) {
                        entry.aliases.push(alias);
                    }
                }
            }
            None => missing.push(title.clone()),
        }
    }
    if !missing.is_empty() {
        return Err(DumpPackError::NotInDump(missing));
    }
    Ok(order
        .into_iter()
        .filter_map(|id| selected.remove(&id))
        .collect())
}

fn get<T: MediaWikiTransport>(
    transport: &mut T,
    url: &str,
    max_bytes: usize,
) -> Result<String, String>
where
    T::Error: std::fmt::Debug,
{
    let bytes = transport
        .get(url, max_bytes)
        .map_err(|error| format!("request failed: {error:?}"))?;
    String::from_utf8(bytes).map_err(|_| "reply is not UTF-8".to_owned())
}

/// Render, read and attach image metadata for one dump-named revision.
pub fn render_dump_article<T: MediaWikiTransport>(
    transport: &mut T,
    project: &str,
    language: &str,
    selection: &DumpSelection,
) -> Result<Article, DumpPackError>
where
    T::Error: std::fmt::Debug,
{
    let page = &selection.page;
    let fail = |reason: String| DumpPackError::Render {
        title: page.title.clone(),
        reason,
    };
    let requests = WikipediaRequests::new(project).map_err(|e| fail(format!("{e:?}")))?;
    let pinned = OnlineRevision {
        key: ArticleKey {
            project: project.to_owned(),
            page_id: page.page_id,
        },
        title: page.title.clone(),
        namespace: page.namespace,
        revision_id: page.revision_id,
        timestamp: page.timestamp.clone(),
    };
    let url = requests
        .rendered_revision(page.revision_id)
        .map_err(|e| fail(format!("{e:?}")))?;
    let body = get(transport, &url, RENDERED_BYTES).map_err(fail)?;
    let (rendered, raw) = parse_rendered_revision_with_source(&body, &pinned)
        .map_err(|e| fail(format!("rendered revision rejected: {e:?}")))?;
    let extracted = extract_rendered_article(&raw).map_err(|e| fail(format!("{e:?}")))?;
    let files: Vec<String> = extracted
        .images
        .iter()
        .map(|image| image.file_title.clone())
        .collect();
    let mut media = Vec::with_capacity(files.len());
    for batch in files.chunks(MAX_IMAGE_TITLES) {
        let url = requests
            .image_metadata(batch)
            .map_err(|e| fail(format!("{e:?}")))?;
        let body = get(transport, &url, METADATA_BYTES).map_err(fail)?;
        media.extend(
            parse_image_metadata(&body, batch)
                .map_err(|e| fail(format!("image metadata rejected: {e:?}")))?,
        );
    }
    let mut article = assemble_article(&pinned, language, extracted, rendered.html, media)
        .map_err(|e| fail(format!("{e:?}")))?;
    if !selection.aliases.is_empty() {
        article.aliases = selection.aliases.clone();
        article.revision.content_sha256 = "0".repeat(64);
        article.revision.content_sha256 =
            wiki_source::enterprise_verified::enterprise_normalize::digest_article(&article)
                .map_err(|e| fail(format!("{e:?}")))?;
    }
    Ok(article)
}

/// Identity of a dump-built pack.
pub struct DumpPackIdentity<'a> {
    pub pack_id: &'a str,
    pub project: &'a str,
    /// Dump generation date, e.g. `2026-11-01`.
    pub generation: &'a str,
    /// SHA-256 of the generation's `SHA256SUMS` inventory.
    pub inventory_sha256: &'a str,
    /// SHA-256 of the list of requested titles (the pack definition).
    pub definition_sha256: &'a str,
    pub definition_id: &'a str,
}

/// Write `articles` as a pack tree under `work_dir` and the `.wpack` to
/// `output`. `work_dir` must not exist yet.
pub fn write_dump_pack(
    articles: &[Article],
    identity: &DumpPackIdentity<'_>,
    work_dir: &Path,
    output: &mut impl Write,
) -> Result<(), DumpPackError> {
    let write = |error: String| DumpPackError::Write(error);
    if articles.is_empty() {
        return Err(DumpPackError::InvalidRequest("no articles".into()));
    }
    fs::create_dir(work_dir).map_err(|e| write(format!("work directory: {e}")))?;
    let mut shard = Vec::new();
    let mut frames = Vec::new();
    for article in articles {
        let offset = shard.len() as u64;
        append_record_frame(&mut shard, &PageRecord::Article(Box::new(article.clone())))
            .map_err(|e| write(format!("{e:?}")))?;
        frames.push((offset, shard.len() as u64 - offset));
    }
    fs::write(work_dir.join("articles.shard"), &shard).map_err(|e| write(e.to_string()))?;
    let mut catalog = SnapshotCatalog::open(&work_dir.join("catalog.sqlite"))
        .map_err(|e| write(format!("{e:?}")))?;
    for (offset, bytes) in frames {
        catalog
            .insert_verified(work_dir, "articles.shard", offset, bytes)
            .map_err(|e| write(format!("{e:?}")))?;
    }
    drop(catalog);
    let metadata = snapshot_pack_metadata(
        work_dir,
        SourceSnapshot {
            kind: SourceKind::PublicDump,
            snapshot_id: format!("{}-{}", identity.project, identity.generation),
            manifest_sha256: identity.inventory_sha256.to_owned(),
        },
        identity.definition_sha256,
        "CC-BY-SA-4.0",
    )
    .map_err(|e| write(format!("{e:?}")))?;
    let snapshot: String = identity.generation.chars().filter(|c| *c != '-').collect();
    let manifest = custom_manifest_from_tree(
        work_dir,
        identity.pack_id,
        identity.project,
        &snapshot,
        identity.definition_id,
        metadata,
    )
    .map_err(|e| write(format!("{e:?}")))?;
    write_custom_tree(work_dir, &manifest, output).map_err(|e| write(format!("{e:?}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::Cursor;
    use wiki_model::BlockContent;

    const PARSE: &str =
        include_str!("../../../fixtures/wikimedia/enwiki/hypotenuse-1383416967.parse.json");
    const IMAGES: &str =
        include_str!("../../../fixtures/wikimedia/enwiki/hypotenuse-1383416967.images.json");

    /// Replays recorded Action API replies; records every requested URL.
    struct Replay {
        requests: Vec<String>,
    }

    impl MediaWikiTransport for Replay {
        type Error = String;
        fn get(&mut self, url: &str, _max: usize) -> Result<Vec<u8>, String> {
            self.requests.push(url.to_owned());
            let api = WikipediaRequests::new("enwiki").unwrap();
            assert!(api.accepts_transport_url(url), "non-canonical URL {url}");
            if url.contains("action=parse") {
                // The recorded reply, whatever revision is asked for.
                Ok(PARSE.as_bytes().to_vec())
            } else if url.contains("prop=imageinfo") {
                Ok(IMAGES.as_bytes().to_vec())
            } else {
                Err(format!("unexpected request {url}"))
            }
        }
    }

    fn dump(revision: u64) -> (Vec<u8>, DumpChecksum) {
        let xml = format!(
            "<mediawiki><siteinfo><dbname>enwiki</dbname></siteinfo>\
             <page><title>Hypotenuse</title><ns>0</ns><id>251366</id><revision><id>{revision}</id><timestamp>2026-11-02T15:48:37Z</timestamp><text>x</text></revision></page>\
             <page><title>Hypotenuses</title><ns>0</ns><id>99</id><redirect title=\"Hypotenuse\" /><revision><id>5</id><timestamp>2020-01-01T00:00:00Z</timestamp><text>#REDIRECT</text></revision></page>\
             <page><title>Talk:Hypotenuse</title><ns>1</ns><id>7</id><revision><id>6</id><timestamp>2020-01-01T00:00:00Z</timestamp><text>t</text></revision></page>\
             </mediawiki>"
        );
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        encoder.write_all(xml.as_bytes()).unwrap();
        let bytes = encoder.finish().unwrap();
        let member = DumpChecksum {
            relative_path: "enwiki-2026-11-01-p1p2.xml.bz2".into(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        };
        (bytes, member)
    }

    fn select(
        bytes: &[u8],
        member: &DumpChecksum,
        titles: &[&str],
    ) -> Result<Vec<DumpSelection>, DumpPackError> {
        let titles: Vec<String> = titles.iter().map(|t| (*t).to_owned()).collect();
        select_dump_pages(
            Cursor::new(bytes),
            member,
            "enwiki",
            &titles,
            1 << 20,
            1 << 20,
        )
    }

    #[test]
    fn a_dump_named_revision_becomes_an_installable_pack() {
        let (bytes, member) = dump(1383416967);
        let selected = select(&bytes, &member, &["Hypotenuses", "hypotenuse"]).unwrap();
        assert_eq!(selected.len(), 1, "redirect and target are one article");
        assert_eq!(selected[0].page.revision_id, 1383416967);
        assert_eq!(selected[0].aliases, ["Hypotenuses"]);

        let mut replay = Replay {
            requests: Vec::new(),
        };
        let article = render_dump_article(&mut replay, "enwiki", "en", &selected[0]).unwrap();
        assert_eq!(
            replay.requests.len(),
            2,
            "one render and one image-metadata request"
        );
        assert_eq!(article.revision.revision_id, 1383416967);
        assert_eq!(article.aliases, ["Hypotenuses"]);
        assert_eq!(article.media.len(), 3);
        assert!(
            article.media.iter().all(|m| m.sha256.is_none()),
            "images not included"
        );
        assert!(matches!(
            article.lead[0].content,
            BlockContent::Media { media_index: 0 }
        ));
        article.validate().unwrap();

        let dir = std::env::temp_dir().join(format!(
            "dump-pack-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let identity = DumpPackIdentity {
            pack_id: "hypotenuse-sample",
            project: "enwiki",
            generation: "2026-11-01",
            inventory_sha256: &"ab".repeat(32),
            definition_sha256: &"cd".repeat(32),
            definition_id: "hypotenuse",
        };
        let mut pack = Vec::new();
        write_dump_pack(&[article], &identity, &dir.join("tree"), &mut pack).unwrap();
        let manifest =
            wiki_pack_format::container::verify_container(&mut Cursor::new(&pack)).unwrap();
        assert_eq!(manifest.pack_id, "hypotenuse-sample");
        assert_eq!(manifest.snapshot, "20261101");
        assert_eq!(manifest.metadata.source.kind, SourceKind::PublicDump);
        assert_eq!(manifest.metadata.source.snapshot_id, "enwiki-2026-11-01");
        assert_eq!(manifest.metadata.counts.articles, 1);
        assert_eq!(
            manifest.metadata.counts.media, 0,
            "image bytes are not in the pack"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mismatched_dumps_missing_titles_and_other_revisions_are_refused() {
        let (bytes, member) = dump(1383416967);
        // A corrupted member is refused before any page is used.
        let mut corrupt = bytes.clone();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        assert!(matches!(
            select(&corrupt, &member, &["Hypotenuse"]),
            Err(DumpPackError::Dump(_))
        ));
        // Talk pages and absent titles are not articles of this dump.
        match select(&bytes, &member, &["Talk:Hypotenuse", "Nope"]) {
            Err(DumpPackError::NotInDump(missing)) => {
                assert_eq!(missing, ["Talk:Hypotenuse", "Nope"])
            }
            other => panic!("expected NotInDump, got {other:?}"),
        }
        // The API renders a different revision than the dump names: refused.
        let (older, member) = dump(1375841497);
        let selected = select(&older, &member, &["Hypotenuse"]).unwrap();
        let mut replay = Replay {
            requests: Vec::new(),
        };
        match render_dump_article(&mut replay, "enwiki", "en", &selected[0]) {
            Err(DumpPackError::Render { title, reason }) => {
                assert_eq!(title, "Hypotenuse");
                assert!(reason.contains("RevisionMismatch"), "{reason}");
            }
            other => panic!("expected a revision mismatch, got {other:?}"),
        }
    }
}
