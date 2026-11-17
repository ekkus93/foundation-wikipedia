//! `wiki-pack build-public-dump-pack`: a custom pack of selected articles
//! from a checksum-pinned public dump member, rendered by the free public
//! Action API at exactly the dump's revisions (SRC-003). Image bytes are not
//! included; each article records its required images with real licences.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::BufReader;
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use wiki_pack_builder::public_dump_pack::{
    render_dump_article, select_dump_pages, write_dump_pack, DumpPackIdentity,
};
use wiki_source::online_curl::CurlTransport;
use wiki_source::public_dump::DumpChecksum;

const MAX_TITLES_FILE_BYTES: u64 = 64 * 1024;
const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024 * 1024;

pub const USAGE: &str = "       wiki-pack build-public-dump-pack <project> <language> <generation> <SHA256SUMS-sha256> <member.xml.bz2> <member-sha256> <titles.txt> <pack-id> <definition-id> <new-work-dir> <new-output.wpack>";

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
    let fields: Vec<&str> = args
        .iter()
        .map(|value| value.to_str())
        .collect::<Option<_>>()
        .ok_or("arguments must be UTF-8")?;
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
    if work_dir.exists() || output.exists() {
        return Err("the work directory and output must not exist yet".into());
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
    let mut articles = Vec::with_capacity(selected.len());
    let mut images = 0;
    for (index, selection) in selected.iter().enumerate() {
        eprintln!(
            "[{}/{}] {} (page {}, revision {})",
            index + 1,
            selected.len(),
            selection.page.title,
            selection.page.page_id,
            selection.page.revision_id
        );
        let article = render_dump_article(&mut transport, project, language, selection)
            .map_err(|e| e.to_string())?;
        images += article.media.len();
        articles.push(article);
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
    if let Err(error) = write_dump_pack(&articles, &identity, &work_dir, &mut file) {
        drop(file);
        let _ = fs::remove_file(&output);
        return Err(error.to_string());
    }
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(format!(
        "Wrote {} with {} articles from {project} {generation}. {images} required images are \
         recorded with their licences but NOT included (image download is not built yet).",
        output.display(),
        articles.len()
    ))
}
