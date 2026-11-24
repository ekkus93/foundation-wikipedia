//! SRC-003: build an installable custom pack from a public Wikimedia dump.
//!
//! 1. [`select_dump_pages`] streams one checksum-pinned dump member and finds
//!    the requested titles: the dump names each page's exact current
//!    revision (redirects resolve one hop to their target and become aliases).
//! 2. [`render_dump_article`] renders that exact revision with the free public
//!    Action API (`action=parse&oldid=`), refuses any reply whose page,
//!    revision or title differs from the dump, reads it into blocks, and
//!    resolves its required images to real Commons metadata. Each image's
//!    bytes are downloaded into a [`DumpMediaStore`] and checked (PACK-003,
//!    see `wiki_source::media_download`); a single image that cannot be
//!    fetched or verified fails the article, so a pack is never built with
//!    an image missing.
//! 3. [`write_dump_pack`] writes the article shard, catalog, the images and
//!    their index, and a custom `.wpack` whose source is the pinned dump
//!    generation.
//!
//! Paid Enterprise access is never used; network access is the caller's
//! transport, which must accept only canonical Action API URLs
//! ([`MediaWikiTransport`]) and canonical Wikimedia media URLs
//! ([`MediaFileTransport`]).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::image_reencode::pack_image;
use crate::map_render::{
    draw_map, plan_view, MapDetail, MapPlan, NaturalEarth, MAP_CREATOR, MAP_CREATOR_OSM,
    MAP_LICENSE, MAP_LICENSE_OSM,
};
use crate::osm_extract::{extract, OsmFeature, OsmPlace, OsmRequest, OsmShape, OsmSource};
use wiki_model::{Article, ArticleKey, MediaAsset, MediaDerivation, PageRecord};
use wiki_pack_format::export::{custom_manifest_from_tree, write_custom_tree};
use wiki_pack_format::manifest::{SourceKind, SourceSnapshot};
use wiki_source::map_data::MapFrame;
use wiki_source::map_data::{interactive_maps, parse_map_data, MapDataError};
use wiki_source::media_download::{
    parse_image_downloads, verify_image, MediaFileTransport, MAX_IMAGE_BYTES,
};
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
use wiki_store::pack_media::{
    encode_media_index, media_object_path, required_images, PackImage, MEDIA_DIR,
    MEDIA_INDEX_OBJECT,
};
use wiki_store::pack_svg::sanitize_svg;
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

/// Downloaded, verified images, stored once each by SHA-256 in a private
/// directory until [`write_dump_pack`] copies them into the pack.
pub struct DumpMediaStore {
    dir: PathBuf,
    /// Download URL → what was stored, so a shared image is fetched once.
    fetched: HashMap<String, StoredImage>,
    svg_fallbacks: Vec<(String, String)>,
    map_notices: Vec<(String, String)>,
    maps_drawn: usize,
    maps_with_openstreetmap: usize,
}

impl DumpMediaStore {
    /// `dir` must not exist yet.
    pub fn create(dir: &Path) -> Result<Self, DumpPackError> {
        fs::create_dir(dir).map_err(|e| DumpPackError::Write(format!("image directory: {e}")))?;
        Ok(Self {
            dir: dir.to_owned(),
            fetched: HashMap::new(),
            svg_fallbacks: Vec::new(),
            map_notices: Vec::new(),
            maps_drawn: 0,
            maps_with_openstreetmap: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    fn fetch<T: MediaFileTransport>(
        &mut self,
        transport: &mut T,
        download: &wiki_source::media_download::ImageDownload,
    ) -> Result<StoredImage, String>
    where
        T::Error: std::fmt::Debug,
    {
        if let Some(found) = self.fetched.get(&download.url) {
            return Ok(found.clone());
        }
        if download.is_svg_original() {
            return self.fetch_svg(transport, download);
        }
        let bytes = transport
            .get_media(&download.url, download.accept(), MAX_IMAGE_BYTES)
            .map_err(|error| format!("{}: download failed: {error:?}", download.file_title))?;
        let mime = verify_image(download, &bytes)
            .map_err(|error| format!("{}: image refused: {error:?}", download.file_title))?;
        // Only verified bytes are re-encoded; see `image_reencode`.
        let packed = pack_image(bytes, mime);
        let digest = hex_sha256(&packed.bytes);
        let path = self.dir.join(&digest);
        if !path.exists() {
            fs::write(&path, &packed.bytes).map_err(|e| format!("image write: {e}"))?;
        }
        let found = StoredImage {
            sha256: digest,
            mime: packed.mime,
            derivation: packed.derivation,
        };
        self.fetched.insert(download.url.clone(), found.clone());
        Ok(found)
    }

    /// An SVG original, verified against Wikimedia's hash and kept as a
    /// sanitised vector; if the sanitiser refuses it, its rendering.
    fn fetch_svg<T: MediaFileTransport>(
        &mut self,
        transport: &mut T,
        download: &wiki_source::media_download::ImageDownload,
    ) -> Result<StoredImage, String>
    where
        T::Error: std::fmt::Debug,
    {
        let bytes = transport
            .get_media(&download.url, download.accept(), MAX_IMAGE_BYTES)
            .map_err(|error| format!("{}: download failed: {error:?}", download.file_title))?;
        verify_image(download, &bytes)
            .map_err(|error| format!("{}: image refused: {error:?}", download.file_title))?;
        let found = match sanitize_svg(&bytes) {
            Ok(clean) => {
                let digest = hex_sha256(&clean);
                let path = self.dir.join(&digest);
                if !path.exists() {
                    fs::write(&path, &clean).map_err(|e| format!("image write: {e}"))?;
                }
                StoredImage {
                    sha256: digest,
                    mime: "image/svg+xml",
                    derivation: Some(MediaDerivation::SanitizedSvg),
                }
            }
            Err(reason) => {
                let rendering = download
                    .fallback_download()
                    .ok_or_else(|| format!("{}: SVG has no rendering", download.file_title))?;
                self.svg_fallbacks
                    .push((download.file_title.clone(), format!("{reason:?}")));
                self.fetch(transport, &rendering)?
            }
        };
        self.fetched.insert(download.url.clone(), found.clone());
        Ok(found)
    }

    /// Maps drawn into the pack so far.
    pub fn maps_drawn(&self) -> usize {
        self.maps_drawn
    }

    /// Of those, maps with OpenStreetMap detail or outlines.
    pub fn maps_with_openstreetmap(&self) -> usize {
        self.maps_with_openstreetmap
    }

    /// Why maps were left as online-only notes, or drawn without some of
    /// their shapes: `(article title, reason)`.
    pub fn map_notices(&self) -> &[(String, String)] {
        &self.map_notices
    }

    /// SVGs packed as their rendering because the sanitiser refused them:
    /// `(file title, reason)`.
    pub fn svg_fallbacks(&self) -> &[(String, String)] {
        &self.svg_fallbacks
    }
}

/// One image as stored for a pack.
#[derive(Clone, Debug)]
struct StoredImage {
    sha256: String,
    mime: &'static str,
    derivation: Option<MediaDerivation>,
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Render one dump-named revision, attach its image metadata, and download
/// and verify every image it requires into `media`.
pub fn render_dump_article<T>(
    transport: &mut T,
    project: &str,
    language: &str,
    selection: &DumpSelection,
    media: &mut DumpMediaStore,
    maps: Option<&mut MapPlanner>,
) -> Result<Article, DumpPackError>
where
    T: MediaWikiTransport + MediaFileTransport,
    <T as MediaWikiTransport>::Error: std::fmt::Debug,
    <T as MediaFileTransport>::Error: std::fmt::Debug,
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
    let mut images = Vec::with_capacity(files.len());
    for batch in files.chunks(MAX_IMAGE_TITLES) {
        let url = requests
            .image_metadata(batch)
            .map_err(|e| fail(format!("{e:?}")))?;
        let body = get(transport, &url, METADATA_BYTES).map_err(fail)?;
        let assets = parse_image_metadata(&body, batch)
            .map_err(|e| fail(format!("image metadata rejected: {e:?}")))?;
        let downloads = parse_image_downloads(&body, batch)
            .map_err(|e| fail(format!("image download rejected: {e:?}")))?;
        for (mut asset, download) in assets.into_iter().zip(downloads) {
            let stored = media.fetch(transport, &download).map_err(fail)?;
            // The asset now describes the stored bytes; `source_url` still
            // names the original file, and `derivation` says how the bytes
            // were made from it.
            asset.sha256 = Some(stored.sha256);
            asset.mime_type = stored.mime.to_owned();
            asset.derivation = stored.derivation;
            images.push(asset);
        }
    }
    if let Some(planner) = maps {
        collect_article_maps(
            transport, &requests, page,
            // The raw parse output: map settings (`data-*`) are not kept
            // in the stored, sanitised HTML, whose n-th note is map n.
            &raw, planner,
        )?;
    }
    let mut article = assemble_article(&pinned, language, extracted, rendered.html, images)
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

/// One article's interactive maps, waiting to be drawn.
#[derive(Debug)]
struct ArticleMaps {
    key: ArticleKey,
    title: String,
    frames: Vec<MapFrame>,
    /// Each frame's map-data features (its groups, in order).
    data: Vec<Vec<serde_json::Value>>,
}

/// Collects every article's interactive maps while articles render, so the
/// OpenStreetMap extracts are read once for all of them, then draws them
/// ([`MapPlanner::draw`]).
#[derive(Debug, Default)]
pub struct MapPlanner {
    articles: Vec<ArticleMaps>,
    notices: Vec<(String, String)>,
}

/// Fetch the article's map data for every interactive map of `html`
/// (PACK-003) into `planner`. If the article's maps changed since the
/// pinned revision, nothing is planned and the reason is kept.
fn collect_article_maps<T>(
    transport: &mut T,
    requests: &WikipediaRequests,
    page: &PublicDumpPage,
    html: &str,
    planner: &mut MapPlanner,
) -> Result<(), DumpPackError>
where
    T: MediaWikiTransport,
    <T as MediaWikiTransport>::Error: std::fmt::Debug,
{
    let fail = |reason: String| DumpPackError::Render {
        title: page.title.clone(),
        reason,
    };
    let frames = interactive_maps(html);
    if frames.is_empty() {
        return Ok(());
    }
    let groups: Vec<String> = frames
        .iter()
        .flat_map(|frame| frame.groups.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let data = if groups.is_empty() {
        BTreeMap::new()
    } else {
        let url = requests
            .map_data(page.page_id, &groups)
            .map_err(|e| fail(format!("{e:?}")))?;
        let body = get(transport, &url, METADATA_BYTES).map_err(fail)?;
        match parse_map_data(&body, page.page_id, &groups) {
            Ok(data) => data,
            Err(MapDataError::GroupMismatch(group)) => {
                planner.notices.push((
                    page.title.clone(),
                    format!("map data group {group} is not the pinned revision's"),
                ));
                return Ok(());
            }
            Err(error) => return Err(fail(format!("map data rejected: {error:?}"))),
        }
    };
    let data = frames
        .iter()
        .map(|frame| {
            frame
                .groups
                .iter()
                .filter_map(|group| data.get(group))
                .flatten()
                .cloned()
                .collect()
        })
        .collect();
    planner.articles.push(ArticleMaps {
        key: ArticleKey {
            project: requests.project().to_owned(),
            page_id: page.page_id,
        },
        title: page.title.clone(),
        frames,
        data,
    });
    Ok(())
}

impl MapPlanner {
    /// Draw every planned map from `base` and, where they cover it,
    /// `extracts` (OpenStreetMap). Returns each article's map assets by
    /// `(project, page id)`, in document order; all or none per article. Notices go to `media`.
    pub fn draw(
        mut self,
        base: &NaturalEarth,
        extracts: &[&dyn OsmSource],
        media: &mut DumpMediaStore,
    ) -> Result<BTreeMap<(String, u64), Vec<MediaAsset>>, DumpPackError> {
        let write = |error: String| DumpPackError::Write(error);
        media.map_notices.append(&mut self.notices);
        let plans: Vec<Vec<MapPlan>> = self
            .articles
            .iter()
            .map(|article| {
                article
                    .frames
                    .iter()
                    .zip(&article.data)
                    .map(|(frame, data)| plan_view(frame, data))
                    .collect()
            })
            .collect();
        let requests: Vec<OsmRequest> = plans
            .iter()
            .flatten()
            .map(|plan| OsmRequest {
                bounds: plan.bounds,
                zoom: plan.view.map_or(0, |(_, _, zoom)| zoom),
                shapes: plan.shapes.clone(),
            })
            .collect();
        // Each request takes its detail from the first extract covering it;
        // outlines may come from any extract.
        let mut features: Vec<Option<Vec<OsmFeature>>> = vec![None; requests.len()];
        let mut places: Vec<Vec<OsmPlace>> = vec![Vec::new(); requests.len()];
        let mut shapes: HashMap<String, OsmShape> = HashMap::new();
        if !requests.is_empty() {
            for source in extracts {
                let data = extract(*source, &requests).map_err(write)?;
                for (n, covers) in data.covers.iter().enumerate() {
                    if *covers && features[n].is_none() {
                        features[n] = Some(data.features[n].clone());
                        places[n] = data.places[n].clone();
                    }
                }
                for (id, shape) in data.shapes {
                    shapes.entry(id).or_insert(shape);
                }
            }
        }

        let mut drawn_maps = BTreeMap::new();
        let mut request = 0;
        for article in &self.articles {
            let mut drawn = Vec::with_capacity(article.frames.len());
            let mut complete = true;
            for (n, (frame, data)) in article.frames.iter().zip(&article.data).enumerate() {
                let detail = MapDetail {
                    features: features[request + n].as_deref(),
                    places: &places[request + n],
                    shapes: &shapes,
                };
                match draw_map(base, frame, data, Some(&detail)) {
                    Some(map) => drawn.push(map),
                    None => {
                        media.map_notices.push((
                            article.title.clone(),
                            format!("map {} has no centre and nothing drawable to fit", n + 1),
                        ));
                        complete = false;
                        break;
                    }
                }
            }
            request += article.frames.len();
            if !complete {
                continue;
            }
            let mut assets = Vec::with_capacity(drawn.len());
            for map in drawn {
                if map.omitted_shapes > 0 {
                    media.map_notices.push((
                        article.title.clone(),
                        format!(
                            "{} map outline(s) not in any OpenStreetMap extract, not drawn",
                            map.omitted_shapes
                        ),
                    ));
                }
                let digest = hex_sha256(&map.svg);
                let path = media.dir.join(&digest);
                if !path.exists() {
                    fs::write(&path, &map.svg).map_err(|e| write(format!("map write: {e}")))?;
                }
                let (lat, lon) = map.centre;
                let (license, creator) = if map.uses_openstreetmap {
                    (MAP_LICENSE_OSM, MAP_CREATOR_OSM)
                } else {
                    (MAP_LICENSE, MAP_CREATOR)
                };
                assets.push(MediaAsset {
                    source_url: format!(
                        "https://www.openstreetmap.org/?mlat={lat:.5}&mlon={lon:.5}#map={}/{lat:.5}/{lon:.5}",
                        map.zoom
                    ),
                    mime_type: "image/svg+xml".into(),
                    sha256: Some(digest),
                    license: license.into(),
                    creator: creator.into(),
                    attribution: format!("{creator}. {license}"),
                    is_av_preview: false,
                    derivation: Some(MediaDerivation::RenderedMap),
                });
                media.maps_drawn += 1;
                if map.uses_openstreetmap {
                    media.maps_with_openstreetmap += 1;
                }
            }
            drawn_maps.insert((article.key.project.clone(), article.key.page_id), assets);
        }
        Ok(drawn_maps)
    }
}

/// Add an article's drawn maps (after its images, in document order) and
/// recompute its content digest.
pub fn attach_maps(article: &mut Article, maps: Vec<MediaAsset>) -> Result<(), DumpPackError> {
    if maps.is_empty() {
        return Ok(());
    }
    article.media.extend(maps);
    article.revision.content_sha256 = "0".repeat(64);
    article.revision.content_sha256 =
        wiki_source::enterprise_verified::enterprise_normalize::digest_article(article).map_err(
            |e| DumpPackError::Render {
                title: article.title.clone(),
                reason: format!("{e:?}"),
            },
        )?;
    Ok(())
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
/// `output`. `work_dir` must not exist yet. Every image the articles
/// require is copied from `media` and re-hashed; a missing one fails.
pub fn write_dump_pack(
    articles: &[Article],
    media: &DumpMediaStore,
    identity: &DumpPackIdentity<'_>,
    work_dir: &Path,
    output: &mut impl Write,
) -> Result<(), DumpPackError> {
    let write = |error: String| DumpPackError::Write(error);
    if articles.is_empty() {
        return Err(DumpPackError::InvalidRequest("no articles".into()));
    }
    fs::create_dir(work_dir).map_err(|e| write(format!("work directory: {e}")))?;
    let (required, without_bytes) =
        required_images(articles).map_err(|e| write(format!("{e:?}")))?;
    if let Some(title) = without_bytes.first() {
        return Err(write(format!("{title}: an image has no downloaded bytes")));
    }
    if !required.is_empty() {
        fs::create_dir(work_dir.join(MEDIA_DIR)).map_err(|e| write(e.to_string()))?;
        let mut index = Vec::with_capacity(required.len());
        for (digest, mime) in &required {
            let bytes = fs::read(media.path().join(digest))
                .map_err(|e| write(format!("image {digest}: {e}")))?;
            if hex_sha256(&bytes) != *digest {
                return Err(write(format!("image {digest} changed after download")));
            }
            fs::write(work_dir.join(media_object_path(digest)), &bytes)
                .map_err(|e| write(e.to_string()))?;
            index.push(PackImage {
                sha256: digest.clone(),
                mime: mime.clone(),
                bytes: bytes.len() as u64,
            });
        }
        let encoded = encode_media_index(&index).map_err(|e| write(format!("{e:?}")))?;
        fs::write(work_dir.join(MEDIA_INDEX_OBJECT), encoded).map_err(|e| write(e.to_string()))?;
    }
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
    /// Images are small PNG-signed blobs named after their URL (which no
    /// decoder accepts, so they are packed unchanged), or with `diagrams`
    /// real PNG diagrams; `broken` names one URL that fails.
    struct Replay {
        requests: Vec<String>,
        broken: Option<&'static str>,
        diagrams: bool,
        /// SVG originals served instead of the committed ones, by file name.
        svg: HashMap<String, Vec<u8>>,
    }

    fn replay() -> Replay {
        Replay {
            requests: Vec::new(),
            broken: None,
            diagrams: false,
            svg: HashMap::new(),
        }
    }

    /// A flat-colour diagram PNG, different for each URL.
    fn diagram_png(url: &str) -> Vec<u8> {
        use image::ImageEncoder;
        let shade = url
            .bytes()
            .fold(0u8, |acc, b| acc.wrapping_mul(31).wrapping_add(b));
        let image = image::RgbaImage::from_fn(400, 300, |x, y| {
            if y == 150 || x % 40 == 0 {
                image::Rgba([0, 0, 0, 255])
            } else {
                image::Rgba([shade, 120, 255 - shade, 255])
            }
        });
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(image.as_raw(), 400, 300, image::ExtendedColorType::Rgba8)
            .unwrap();
        png
    }

    impl MediaFileTransport for Replay {
        type Error = String;
        fn get_media(&mut self, url: &str, accept: &str, _max: usize) -> Result<Vec<u8>, String> {
            self.requests.push(url.to_owned());
            assert!(
                wiki_source::media_download::media_url_allowed(url),
                "non-canonical media URL {url}"
            );
            if self.broken.is_some_and(|broken| url.contains(broken)) {
                return Err("HTTP 404".into());
            }
            if accept == "image/svg+xml" {
                // The SVG originals, as committed (their SHA-1 is in IMAGES).
                let name = url.rsplit('/').next().unwrap();
                return Ok(self.svg.get(name).cloned().unwrap_or_else(|| {
                    fs::read(
                        Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../../fixtures/wikimedia/commonswiki")
                            .join(name),
                    )
                    .unwrap()
                }));
            }
            assert_eq!(
                accept,
                wiki_source::media_download::RENDERING_ACCEPT,
                "anything else is a rendering"
            );
            if self.diagrams {
                return Ok(diagram_png(url));
            }
            let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
            png.extend_from_slice(url.as_bytes());
            Ok(png)
        }
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
            } else if url.contains("prop=imageinfo") && self.diagrams {
                // SVGs too large to keep as vectors: packed as renderings.
                Ok(IMAGES
                    .replace("\"size\":3328", "\"size\":4194305")
                    .replace("\"size\":531", "\"size\":4194305")
                    .replace("\"size\":16278", "\"size\":4194305")
                    .into_bytes())
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

        let dir = std::env::temp_dir().join(format!(
            "dump-pack-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let mut media = DumpMediaStore::create(&dir.join("images")).unwrap();
        let mut replay = replay();
        let article =
            render_dump_article(&mut replay, "enwiki", "en", &selected[0], &mut media, None)
                .unwrap();
        assert_eq!(
            replay.requests.len(),
            5,
            "one render, one image-metadata request and three image downloads"
        );
        assert_eq!(article.revision.revision_id, 1383416967);
        assert_eq!(article.aliases, ["Hypotenuses"]);
        assert_eq!(article.media.len(), 3);
        for asset in &article.media {
            assert!(asset.sha256.is_some(), "every image is downloaded");
            assert_eq!(asset.mime_type, "image/svg+xml", "SVG kept as a vector");
            assert_eq!(asset.derivation, Some(MediaDerivation::SanitizedSvg));
            assert!(
                asset.source_url.ends_with(".svg"),
                "source names the original"
            );
        }
        // A second article using the same images fetches nothing new.
        let before = replay.requests.len();
        render_dump_article(&mut replay, "enwiki", "en", &selected[0], &mut media, None).unwrap();
        assert_eq!(replay.requests.len(), before + 2);
        assert!(matches!(
            article.lead[0].content,
            BlockContent::Media { media_index: 0 }
        ));
        article.validate().unwrap();

        let identity = DumpPackIdentity {
            pack_id: "hypotenuse-sample",
            project: "enwiki",
            generation: "2026-11-01",
            inventory_sha256: &"ab".repeat(32),
            definition_sha256: &"cd".repeat(32),
            definition_id: "hypotenuse",
        };
        let mut pack = Vec::new();
        write_dump_pack(
            std::slice::from_ref(&article),
            &media,
            &identity,
            &dir.join("tree"),
            &mut pack,
        )
        .unwrap();
        let manifest =
            wiki_pack_format::container::verify_container(&mut Cursor::new(&pack)).unwrap();
        assert_eq!(manifest.pack_id, "hypotenuse-sample");
        assert_eq!(manifest.snapshot, "20261101");
        assert_eq!(manifest.metadata.source.kind, SourceKind::PublicDump);
        assert_eq!(manifest.metadata.source.snapshot_id, "enwiki-2026-11-01");
        assert_eq!(manifest.metadata.counts.articles, 1);
        assert_eq!(
            manifest.metadata.counts.media, 3,
            "every image is in the pack"
        );
        assert_eq!(
            manifest.metadata.license.attribution_object.as_deref(),
            Some(MEDIA_INDEX_OBJECT)
        );

        // The pack installs, and each image reads back by its digest.
        let root = dir.join("library");
        fs::create_dir(&root).unwrap();
        let installed = wiki_store::pack_install::install_custom_pack(
            &root,
            &mut Cursor::new(&pack),
            pack.len() as u64,
            |_| true,
        )
        .unwrap();
        // Every required figure of the real revision is served locally, and
        // nothing else in the document loads: the other images are page
        // furniture (sister-project and portal icons) the extractor skips.
        let document = wiki_source::reader_document::reader_document(
            &article,
            wiki_source::reader_document::ReaderTheme::Light,
        );
        for asset in &article.media {
            let local = format!(
                "src=\"https://foundation.invalid/media/{}\"",
                asset.sha256.as_deref().unwrap()
            );
            assert_eq!(document.matches(&local).count(), 1, "{}", asset.source_url);
        }
        assert_eq!(
            document.matches(" src=").count(),
            article.media.len(),
            "only pack images have a source"
        );
        assert_eq!(document.matches("<img").count(), 6, "3 figures, 3 icons");

        // Reopening the installed snapshot (activation) sees the same article
        // shards as the installer: images are not shards.
        assert_eq!(
            wiki_store::pack_install::installed_shard_inventory(&installed.path).unwrap(),
            installed.shards
        );
        for asset in &article.media {
            let digest = asset.sha256.as_deref().unwrap();
            let (mime, bytes) = wiki_store::pack_media::read_pack_image(&installed.path, digest)
                .unwrap()
                .unwrap();
            assert_eq!(mime, "image/svg+xml");
            assert!(bytes.starts_with(b"<svg xmlns=\"http://www.w3.org/2000/svg\""));
            assert!(wiki_store::pack_svg::is_clean_svg(&bytes));
        }
        assert!(media.svg_fallbacks().is_empty());

        // An article whose image lost its bytes cannot be packed.
        let mut incomplete = article.clone();
        incomplete.media[1].sha256 = None;
        assert!(matches!(
            write_dump_pack(&[incomplete], &media, &identity, &dir.join("tree2"), &mut Vec::new()),
            Err(DumpPackError::Write(reason)) if reason.contains("no downloaded bytes")
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn diagrams_are_packed_as_lossless_webp_and_say_so() {
        let (bytes, member) = dump(1383416967);
        let selected = select(&bytes, &member, &["Hypotenuse"]).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "dump-pack-webp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let mut media = DumpMediaStore::create(&dir.join("images")).unwrap();
        let mut transport = Replay {
            diagrams: true,
            ..replay()
        };
        let article = render_dump_article(
            &mut transport,
            "enwiki",
            "en",
            &selected[0],
            &mut media,
            None,
        )
        .unwrap();
        article.validate().unwrap();
        let originals: Vec<Vec<u8>> = transport.requests[2..]
            .iter()
            .map(|url| diagram_png(url))
            .collect();
        assert_eq!(originals.len(), 3);
        for (asset, original) in article.media.iter().zip(&originals) {
            assert_eq!(asset.mime_type, "image/webp");
            assert_eq!(asset.derivation, Some(MediaDerivation::LosslessWebp));
            let stored = fs::read(media.path().join(asset.sha256.as_deref().unwrap())).unwrap();
            assert!(stored.len() < original.len());
            assert_eq!(
                image::load_from_memory(&stored).unwrap().to_rgba8(),
                image::load_from_memory(original).unwrap().to_rgba8(),
                "the same pixels as the downloaded file"
            );
        }

        let identity = DumpPackIdentity {
            pack_id: "hypotenuse-webp",
            project: "enwiki",
            generation: "2026-11-01",
            inventory_sha256: &"ab".repeat(32),
            definition_sha256: &"cd".repeat(32),
            definition_id: "hypotenuse",
        };
        let mut pack = Vec::new();
        write_dump_pack(
            std::slice::from_ref(&article),
            &media,
            &identity,
            &dir.join("tree"),
            &mut pack,
        )
        .unwrap();
        let root = dir.join("library");
        fs::create_dir(&root).unwrap();
        let installed = wiki_store::pack_install::install_custom_pack(
            &root,
            &mut Cursor::new(&pack),
            pack.len() as u64,
            |_| true,
        )
        .unwrap();
        // The installed record keeps the derivation, for attribution.
        let catalog =
            SnapshotCatalog::open_read_only(&installed.path.join("catalog.sqlite")).unwrap();
        let mut stored = Vec::new();
        catalog
            .visit_records(&installed.path, |record| {
                if let PageRecord::Article(article) = record {
                    stored.push(*article);
                }
                Ok::<(), wiki_store::catalog::CatalogError>(())
            })
            .unwrap();
        assert_eq!(stored, [article.clone()]);
        for asset in &article.media {
            let (mime, bytes) = wiki_store::pack_media::read_pack_image(
                &installed.path,
                asset.sha256.as_deref().unwrap(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(mime, "image/webp");
            assert!(bytes.starts_with(b"RIFF"));
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_svg_the_sanitiser_refuses_is_packed_as_its_rendering() {
        let dir = std::env::temp_dir().join(format!(
            "dump-pack-svg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut media = DumpMediaStore::create(&dir).unwrap();
        // Wikimedia's hash matches, so the bytes are genuine; the script
        // keeps them from being packed as a vector.
        let hostile = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>x()</script></svg>";
        let sha1 = format!("{:x}", sha1::Sha1::digest(hostile));
        let download = wiki_source::media_download::ImageDownload {
            file_title: "File:Hostile.svg".into(),
            url: "https://upload.wikimedia.org/wikipedia/commons/a/ab/Hostile.svg".into(),
            expected: Some(wiki_source::media_download::Original {
                mime: "image/svg+xml",
                sha1,
                size: hostile.len() as u64,
            }),
            fallback: Some(
                "https://thumb.wikimedia.org/wikipedia/commons/thumb/a/ab/Hostile.svg/1280px-Hostile.svg.png"
                    .into(),
            ),
        };
        let mut transport = replay();
        transport.svg.insert("Hostile.svg".into(), hostile.to_vec());
        let stored = media.fetch(&mut transport, &download).unwrap();
        assert_eq!(stored.mime, "image/png", "the rendering");
        assert_eq!(stored.derivation, None);
        assert_eq!(transport.requests.len(), 2, "original, then rendering");
        assert_eq!(media.svg_fallbacks().len(), 1);
        assert!(media.svg_fallbacks()[0].1.contains("script"));
        // Changed bytes are never packed in any form.
        let mut other = DumpMediaStore::create(&dir.join("other")).unwrap();
        transport.svg.insert(
            "Hostile.svg".into(),
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
        );
        assert!(other
            .fetch(&mut transport, &download)
            .unwrap_err()
            .contains("DigestMismatch"));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Serves one page's map data, as the Action API does.
    struct MapApi {
        reply: String,
        requests: Vec<String>,
    }

    impl MediaWikiTransport for MapApi {
        type Error = String;
        fn get(&mut self, url: &str, _max: usize) -> Result<Vec<u8>, String> {
            assert!(WikipediaRequests::new("simplewiki")
                .unwrap()
                .accepts_transport_url(url));
            assert!(url.contains("prop=mapdata"), "{url}");
            self.requests.push(url.to_owned());
            Ok(self.reply.clone().into_bytes())
        }
    }

    #[test]
    fn interactive_maps_are_drawn_from_the_base_map_and_the_articles_data() {
        let group = "_477ba0a3c627b1ed526835dae69bd369beac7de4";
        // Real simplewiki markup and map data for Eiffel Tower (page 5246),
        // moved to the tiny test island at (0, 0).
        let html = format!(
            "<p>x</p><a class=\"mw-kartographer-map\" data-zoom=\"6\" data-width=\"250\" \
             data-height=\"200\" data-overlays=\"[&quot;{group}&quot;]\"><img src=\"x\"></a>\
             <a class=\"mw-kartographer-map\" data-lat=\"0.2\" data-lon=\"0.2\" data-zoom=\"7\" \
             data-width=\"300\" data-height=\"200\"><img src=\"y\"></a>"
        );
        let reply = format!(
            "{{\"batchcomplete\":true,\"query\":{{\"pages\":[{{\"pageid\":5246,\"title\":\"Eiffel Tower\",\
             \"mapdata\":[\"{{\\\"{group}\\\":[{{\\\"type\\\":\\\"Feature\\\",\\\"geometry\\\":{{\\\"coordinates\\\":[0.1,0.1],\
             \\\"type\\\":\\\"Point\\\"}},\\\"properties\\\":{{\\\"marker-color\\\":\\\"#5E74F3\\\"}}}}]}}\"]}}]}}}}"
        );
        let page = PublicDumpPage {
            page_id: 5246,
            title: "Eiffel Tower".into(),
            namespace: 0,
            revision_id: 11033818,
            timestamp: "2026-10-01T00:00:00Z".into(),
            redirect_target: None,
        };
        let dir = std::env::temp_dir().join(format!(
            "dump-pack-maps-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut media = DumpMediaStore::create(&dir).unwrap();
        let requests = WikipediaRequests::new("simplewiki").unwrap();
        let base = crate::map_render::tests::base();
        let mut api = MapApi {
            reply: reply.clone(),
            requests: Vec::new(),
        };
        let mut planner = MapPlanner::default();
        collect_article_maps(&mut api, &requests, &page, &html, &mut planner).unwrap();
        let mut drawn = planner.draw(&base, &[], &mut media).unwrap();
        let maps = drawn.remove(&("simplewiki".to_owned(), 5246)).unwrap();
        assert_eq!(api.requests.len(), 1, "one request for all groups");
        assert_eq!(maps.len(), 2);
        assert_eq!(media.maps_drawn(), 2);
        for map in &maps {
            assert_eq!(map.derivation, Some(MediaDerivation::RenderedMap));
            assert_eq!(map.mime_type, "image/svg+xml");
            let svg = fs::read(dir.join(map.sha256.as_deref().unwrap())).unwrap();
            assert!(wiki_store::pack_svg::is_clean_svg(&svg));
        }
        // The first map fits its marker; the second is at its stated centre.
        assert!(
            maps[0]
                .source_url
                .contains("mlat=0.10000&mlon=0.10000#map=6/"),
            "{}",
            maps[0].source_url
        );
        assert!(
            maps[1].source_url.contains("#map=7/0.20000/0.20000"),
            "{}",
            maps[1].source_url
        );
        let marker =
            String::from_utf8(fs::read(dir.join(maps[0].sha256.as_deref().unwrap())).unwrap())
                .unwrap();
        assert!(marker.contains("fill=\"#5e74f3\""), "{marker}");

        // Map data for other groups (the article changed since the pinned
        // revision): nothing is drawn, and the reason is kept.
        let mut changed = MapApi {
            reply: reply.replace(group, "_0000000000000000000000000000000000000000"),
            requests: Vec::new(),
        };
        let mut other = DumpMediaStore::create(&dir.join("other")).unwrap();
        let mut planner = MapPlanner::default();
        collect_article_maps(&mut changed, &requests, &page, &html, &mut planner).unwrap();
        assert!(planner.draw(&base, &[], &mut other).unwrap().is_empty());
        assert!(
            other.map_notices()[0].1.contains("not the pinned revision"),
            "{:?}",
            other.map_notices()
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn maps_take_street_detail_from_an_extract_covering_them_and_credit_it() {
        use crate::osm_extract::memory::{Element, Memory};
        use crate::osm_extract::MemberKind;
        let extract = Memory {
            elements: vec![
                Element::Node(1, [-0.01, 0.0], vec![]),
                Element::Node(2, [0.01, 0.0], vec![]),
                Element::Node(3, [0.0, -0.02], vec![]),
                Element::Node(4, [0.02, 0.02], vec![]),
                Element::Node(5, [-0.02, 0.02], vec![]),
                Element::Way(10, vec![1, 2], vec![("highway", "primary")]),
                Element::Way(11, vec![3, 4, 5, 3], vec![]),
                Element::Relation(
                    20,
                    vec![(11, MemberKind::Way, "outer")],
                    vec![("type", "boundary"), ("wikidata", "Q42")],
                ),
            ],
            bounds: Some([-1.0, -1.0, 1.0, 1.0]),
            passes: std::cell::Cell::new(0),
        };
        let outline: serde_json::Value = serde_json::json!({
            "type": "ExternalData", "service": "geoshape",
            "url": "https://maps.wikimedia.org/geoshape?getgeojson=1&ids=Q42",
            "properties": {"stroke": "#ff0000"}
        });
        let mut planner = MapPlanner::default();
        planner.articles.push(ArticleMaps {
            key: ArticleKey {
                project: "simplewiki".into(),
                page_id: 7,
            },
            title: "Town".into(),
            frames: vec![MapFrame {
                lat: Some(0.0),
                lon: Some(0.0),
                zoom: Some(12),
                width: 300,
                height: 200,
                groups: vec![],
            }],
            data: vec![vec![outline]],
        });
        let dir = std::env::temp_dir().join(format!(
            "dump-pack-osm-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut media = DumpMediaStore::create(&dir).unwrap();
        let base = crate::map_render::tests::base();
        let mut drawn = planner.draw(&base, &[&extract], &mut media).unwrap();
        let maps = drawn.remove(&("simplewiki".to_owned(), 7)).unwrap();
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].license, MAP_LICENSE_OSM);
        assert_eq!(media.maps_with_openstreetmap(), 1);
        assert!(media.map_notices().is_empty(), "{:?}", media.map_notices());
        let svg =
            String::from_utf8(fs::read(dir.join(maps[0].sha256.as_deref().unwrap())).unwrap())
                .unwrap();
        assert!(
            svg.contains("stroke=\"#fcd6a4\""),
            "the primary road: {svg}"
        );
        assert!(
            svg.contains("stroke=\"#ff0000\""),
            "the Wikidata outline: {svg}"
        );
        assert!(svg.contains("© OpenStreetMap contributors"), "{svg}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_image_that_cannot_be_downloaded_fails_the_article() {
        let (bytes, member) = dump(1383416967);
        let selected = select(&bytes, &member, &["Hypotenuse"]).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "dump-pack-broken-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut media = DumpMediaStore::create(&dir).unwrap();
        let mut broken = Replay {
            broken: Some("Pythagorean.svg"),
            ..replay()
        };
        match render_dump_article(&mut broken, "enwiki", "en", &selected[0], &mut media, None) {
            Err(DumpPackError::Render { title, reason }) => {
                assert_eq!(title, "Hypotenuse");
                assert!(
                    reason.contains("File:Pythagorean.svg: download failed"),
                    "{reason}"
                );
            }
            other => panic!("expected a failed download, got {other:?}"),
        }
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
        let dir = std::env::temp_dir().join(format!(
            "dump-pack-mismatch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut media = DumpMediaStore::create(&dir).unwrap();
        let mut replay = replay();
        match render_dump_article(&mut replay, "enwiki", "en", &selected[0], &mut media, None) {
            Err(DumpPackError::Render { title, reason }) => {
                assert_eq!(title, "Hypotenuse");
                assert!(reason.contains("RevisionMismatch"), "{reason}");
            }
            other => panic!("expected a revision mismatch, got {other:?}"),
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
