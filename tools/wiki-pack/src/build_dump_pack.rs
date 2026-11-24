//! `wiki-pack build-public-dump-pack`: a custom pack of selected articles
//! from a checksum-pinned public dump member, rendered by the free public
//! Action API at exactly the dump's revisions (SRC-003). Every image an
//! article shows is downloaded from Wikimedia's media hosts, checked and
//! included with its licence (PACK-003); the build fails if one cannot be.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::BufReader;
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use wiki_pack_builder::map_render::NaturalEarth;
use wiki_pack_builder::osm_extract::{OsmSource, PbfFile};
use wiki_pack_builder::public_dump_pack::{
    attach_maps, render_dump_article, select_dump_pages, write_dump_pack, DumpMediaStore,
    DumpPackIdentity, MapPlanner,
};
use wiki_source::online_curl::CurlTransport;
use wiki_source::public_dump::DumpChecksum;

const MAX_TITLES_FILE_BYTES: u64 = 64 * 1024;
const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024 * 1024;

pub const USAGE: &str = "       wiki-pack build-public-dump-pack [--natural-earth <dir> [--osm <extract.osm.pbf>]...] <project> <language> <generation> <SHA256SUMS-sha256> <member.xml.bz2> <member-sha256> <titles.txt> <pack-id> <definition-id> <new-work-dir> <new-output.wpack>";

fn digest(value: &str, name: &str) -> Result<String, String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(value.to_owned())
    } else {
        Err(format!("{name} must be 64 lowercase hex digits"))
    }
}

fn read_titles(path: &PathBuf) -> Result<(Vec<String>, String), String> {
    let size = fs::metadata(path)
        .map_err(|e| format!("titles file: {e}"))?
        .len();
    if size > MAX_TITLES_FILE_BYTES {
        return Err("titles file is too large".into());
    }
    let bytes = fs::read(path).map_err(|e| format!("titles file: {e}"))?;
    let text = String::from_utf8(bytes.clone()).map_err(|_| "titles file is not UTF-8")?;
    let titles = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect();
    Ok((titles, format!("{:x}", Sha256::digest(&bytes))))
}

pub fn run(args: &[OsString]) -> Result<String, String> {
    let mut fields: Vec<&str> = args
        .iter()
        .map(|value| value.to_str())
        .collect::<Option<_>>()
        .ok_or("arguments must be UTF-8")?;
    // Interactive maps are drawn from the pinned Natural Earth files and,
    // for street detail and Wikidata outlines, OpenStreetMap extracts
    // (PACK-003); without Natural Earth they stay online-only notes.
    let mut base_map = None;
    let mut extracts = Vec::new();
    loop {
        match fields.first() {
            Some(&"--natural-earth") => {
                let dir = fields
                    .get(1)
                    .ok_or(format!("--natural-earth needs a directory\n{USAGE}"))?;
                eprintln!("Loading the Natural Earth base map from {dir}…");
                base_map = Some(NaturalEarth::load(&PathBuf::from(dir))?);
                fields.drain(..2);
            }
            Some(&"--osm") => {
                let path = fields
                    .get(1)
                    .ok_or(format!("--osm needs an .osm.pbf extract\n{USAGE}"))?;
                extracts.push(PbfFile::open(&PathBuf::from(path))?);
                fields.drain(..2);
            }
            _ => break,
        }
    }
    if base_map.is_none() {
        if !extracts.is_empty() {
            return Err(format!("--osm needs --natural-earth as well\n{USAGE}"));
        }
        eprintln!("No --natural-earth directory: interactive maps will be online-only notes.");
    }
    let [project, language, generation, inventory, member_path, member_sha, titles_path, pack_id, definition_id, work_dir, output] =
        fields.as_slice()
    else {
        return Err(format!("expected eleven arguments\n{USAGE}"));
    };
    let inventory = digest(inventory, "SHA256SUMS-sha256")?;
    let member_sha = digest(member_sha, "member-sha256")?;
    let member_path = PathBuf::from(member_path);
    let member_name = member_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| name.ends_with(".xml.bz2"))
        .ok_or("the member must be a .xml.bz2 file")?
        .to_owned();
    let (titles, definition_sha256) = read_titles(&PathBuf::from(titles_path))?;
    let work_dir = PathBuf::from(work_dir);
    let output = PathBuf::from(output);
    // Downloaded images wait beside the work directory until packed.
    let image_dir = PathBuf::from(format!("{}-images", work_dir.display()));
    if work_dir.exists() || output.exists() || image_dir.exists() {
        return Err(format!(
            "the work directory, {} and the output must not exist yet",
            image_dir.display()
        ));
    }

    let file = File::open(&member_path).map_err(|e| format!("dump member: {e}"))?;
    let compressed = file.metadata().map_err(|e| e.to_string())?.len();
    eprintln!("Reading {member_name} ({compressed} bytes) and checking its SHA-256…");
    let selected = select_dump_pages(
        BufReader::new(file),
        &DumpChecksum {
            relative_path: member_name,
            sha256: member_sha,
        },
        project,
        &titles,
        compressed.max(1),
        MAX_DECODED_BYTES,
    )
    .map_err(|e| e.to_string())?;

    let mut transport = CurlTransport::new();
    let mut media = DumpMediaStore::create(&image_dir).map_err(|e| e.to_string())?;
    let mut articles = Vec::with_capacity(selected.len());
    let mut images = 0;
    let mut planner = base_map.as_ref().map(|_| MapPlanner::default());
    for (index, selection) in selected.iter().enumerate() {
        eprintln!(
            "[{}/{}] {} (page {}, revision {})",
            index + 1,
            selected.len(),
            selection.page.title,
            selection.page.page_id,
            selection.page.revision_id
        );
        let article = render_dump_article(
            &mut transport,
            project,
            language,
            selection,
            &mut media,
            planner.as_mut(),
        )
        .map_err(|e| e.to_string())?;
        images += article.media.len();
        articles.push(article);
    }
    if let (Some(planner), Some(base)) = (planner, &base_map) {
        if !extracts.is_empty() {
            eprintln!(
                "Reading map detail from {} OpenStreetMap extract(s)…",
                extracts.len()
            );
        }
        let sources: Vec<&dyn OsmSource> = extracts.iter().map(|e| e as &dyn OsmSource).collect();
        let mut drawn = planner
            .draw(base, &sources, &mut media)
            .map_err(|e| e.to_string())?;
        for article in &mut articles {
            if let Some(maps) = drawn.remove(&(article.key.project.clone(), article.key.page_id)) {
                images += maps.len();
                attach_maps(article, maps).map_err(|e| e.to_string())?;
            }
        }
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(|e| format!("output: {e}"))?;
    let identity = DumpPackIdentity {
        pack_id,
        project,
        generation,
        inventory_sha256: &inventory,
        definition_sha256: &definition_sha256,
        definition_id,
    };
    if let Err(error) = write_dump_pack(&articles, &media, &identity, &work_dir, &mut file) {
        drop(file);
        let _ = fs::remove_file(&output);
        return Err(error.to_string());
    }
    file.sync_all().map_err(|e| e.to_string())?;
    let stored = fs::read_dir(media.path()).map_or(0, Iterator::count);
    for (title, reason) in media.map_notices() {
        eprintln!("{title}: {reason}");
    }
    if media.maps_drawn() > 0 {
        eprintln!(
            "{} interactive map(s) drawn offline, {} with OpenStreetMap detail.",
            media.maps_drawn(),
            media.maps_with_openstreetmap()
        );
    }
    for (file, reason) in media.svg_fallbacks() {
        eprintln!("{file}: kept as its 1280 px rendering, not as a vector ({reason})");
    }
    fs::remove_dir_all(media.path()).map_err(|e| format!("image directory: {e}"))?;
    Ok(format!(
        "Wrote {} with {} articles from {project} {generation}. All {images} article images \
         are included ({stored} distinct files), each with its licence and creator.",
        output.display(),
        articles.len()
    ))
}
