//! Verification and provisional custom export of unsigned FWPACK01 envelopes.
//! These commands do not install packs or verify publisher signatures.
mod audit_diagnostic;
mod audit_public_dump;
mod css_graph_diagnostic;
mod curated_catalog;
mod fetch_public_dump;
mod media_maintenance;
mod preview_topic;
mod visual_diagnostic;

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use wiki_pack_builder::wiki_pack_format::container::verify_container;
use wiki_pack_builder::wiki_pack_format::export::{custom_manifest_from_tree, write_custom_tree};

static NEXT_EXPORT_TEMP: AtomicU64 = AtomicU64::new(0);
use wiki_pack_builder::wiki_pack_format::manifest::Origin;
use wiki_pack_builder::wiki_pack_format::stage::{
    stage_container, stage_container_stream_with_budget,
};

fn usage() {
    println!("Foundation Wikipedia pack CLI (provisional)");
    println!("Usage: wiki-pack verify <file.wpack>");
    println!("       wiki-pack inspect <file.wpack>");
    println!("       wiki-pack media-maintenance <--audit-unbound|--bind-registry|--dry-run|--confirm-delete> <media-root> <existing-registry.sqlite> <max-registered> <max-scanned>");
    println!("       wiki-pack media-maintenance --run-if-due <media-root> <existing-registry.sqlite> <max-registered> <max-scanned> <interval-seconds>");
    println!("       wiki-pack media-owner-release <--dry-run|--confirm-release> <media-root> <existing-registry.sqlite> <pack|cache|pin> <owner-id> <max-candidates>");
    println!("       wiki-pack preview-topic [--json] <local-category-graph.json>");
    println!("       wiki-pack list-curated [--json]");
    println!("       wiki-pack inventory-visuals <rendered.html> <document-url>");
    println!("       wiki-pack inventory-css-graph <supplied-css-graph.json>");
    println!("       wiki-pack audit-public-dump <private-source-dir> <project> <generation> <SHA256SUMS-sha256> <expected-ids> <expected-sha256> <namespace> <private-output-dir> <output-name> <max-compressed-bytes> <max-decoded-bytes>");
    println!("       wiki-pack fetch-public-inventory <project> <generation> <independent-SHA256SUMS-sha256> <private-source-dir>");
    println!("       wiki-pack confirm-public-inventory [--json] <project> <generation> <independent-SHA256SUMS-sha256> <private-source-dir>");
    println!("       wiki-pack fetch-public-member <project> <generation> <SHA256SUMS-sha256> <private-source-dir> <member-index> <max-compressed-bytes>");
    println!("       wiki-pack fetch-public-member-resume <project> <generation> <SHA256SUMS-sha256> <private-source-dir> <member-index> <max-compressed-bytes>");
    println!("       wiki-pack fetch-public-members <project> <generation> <SHA256SUMS-sha256> <private-source-dir> <max-compressed-bytes-per-member>");
    println!("       wiki-pack fetch-public-members-resume <project> <generation> <SHA256SUMS-sha256> <private-source-dir> <max-compressed-bytes-per-member>");
    println!("       wiki-pack verify-public-members [--json] <project> <generation> <SHA256SUMS-sha256> <private-source-dir> <max-compressed-bytes-per-member>");
    println!("       wiki-pack verify-public-generation-live [--json] <project> <generation> <SHA256SUMS-sha256> <private-source-dir> <max-compressed-bytes-per-member>");
    println!("       wiki-pack fetch-public-generation [--json] <project> <generation> <independent-SHA256SUMS-sha256> <private-source-dir> <max-compressed-bytes-per-member>");
    println!("       wiki-pack audit-diagnostic <revision-content.jsonl> <expected-ids.txt> <project> <generation> <namespace> <expected-sha256> [revision-sha256]");
    println!("       wiki-pack export-custom <object-dir> <pack-id> <project> <snapshot> <definition-id> <output.wpack>");
    println!("       wiki-pack stage-unsigned <file.wpack> <private-parent> <name>");
    println!("       wiki-pack stage-unsigned-stdin <private-parent> <name> <max-spool-bytes>");
    println!("       wiki-pack --help");
    println!(
        "verify/inspect check every object; export-custom creates an unsigned custom envelope."
    );
    println!("None of these operations installs a pack.");
    println!("Publisher signatures and offline media completeness are NOT verified.");
}

fn inspect_file(path: PathBuf, detailed: bool) -> Result<(), String> {
    let file = File::open(&path).map_err(|error| format!("cannot open input: {error}"))?;
    let manifest = verify_container(&mut BufReader::new(file))
        .map_err(|error| format!("envelope integrity verification failed: {error:?}"))?;
    let total_bytes: u64 = manifest.objects.iter().map(|object| object.bytes).sum();
    println!(
        "Envelope integrity verified: {} objects, {} declared payload bytes",
        manifest.objects.len(),
        total_bytes
    );
    println!("Publisher signature: NOT VERIFIED");
    println!("Required offline media completeness: NOT VERIFIED");
    if detailed {
        println!("Pack ID: {}", manifest.pack_id);
        println!("Project: {}", manifest.project);
        println!("Snapshot: {}", manifest.snapshot);
        println!("Format version: {}", manifest.version);
        match manifest.origin {
            Origin::Custom { definition_id } => {
                println!("Claimed origin: custom ({definition_id})");
            }
            Origin::Official { publisher } => {
                println!("Claimed origin: official ({publisher}); claim NOT authenticated");
            }
        }
    }
    Ok(())
}

/// Create a complete provisional envelope in a private temporary file. Never
/// replace an existing destination and never publish a partially built pack.
/// The caller must own the object tree exclusively throughout both passes.
fn export_custom_tree(
    source: PathBuf,
    pack_id: &str,
    project: &str,
    snapshot: &str,
    definition_id: &str,
    destination: PathBuf,
) -> Result<(), String> {
    let manifest = custom_manifest_from_tree(&source, pack_id, project, snapshot, definition_id)
        .map_err(|error| format!("invalid source object tree: {error:?}"))?;
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if destination.file_name().is_none() {
        return Err("output must be a filename".into());
    }
    for ancestor in parent.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let meta = fs::symlink_metadata(ancestor)
            .map_err(|error| format!("unsafe output parent: {error}"))?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("output parent cannot contain symlinks or non-directories".into());
        }
    }
    let canonical_source =
        fs::canonicalize(&source).map_err(|error| format!("source directory: {error}"))?;
    let canonical_parent =
        fs::canonicalize(parent).map_err(|error| format!("output directory: {error}"))?;
    if canonical_parent.starts_with(&canonical_source) {
        return Err("cannot place the output inside the source object tree".into());
    }

    let mut staged = None;
    for _ in 0..32 {
        let candidate = parent.join(format!(
            ".fwpack-export.{}.{}.tmp",
            std::process::id(),
            NEXT_EXPORT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(file) => {
                staged = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("cannot stage output: {error}")),
        }
    }
    let (temp_path, mut file) =
        staged.ok_or_else(|| "cannot allocate unique temporary output".to_owned())?;
    let result = (|| -> Result<(), String> {
        write_custom_tree(&source, &manifest, &mut file)
            .map_err(|error| format!("pack export failed: {error:?}"))?;
        file.sync_all()
            .map_err(|error| format!("cannot sync packed output: {error}"))?;
        drop(file);
        let mut reader = BufReader::new(
            File::open(&temp_path).map_err(|error| format!("reopen packed output: {error}"))?,
        );
        let verified = verify_container(&mut reader)
            .map_err(|error| format!("output self-verification failed: {error:?}"))?;
        if verified != manifest {
            return Err("output manifest does not match source inventory".into());
        }
        // Atomic no-overwrite publication; a second writer cannot clobber an
        // existing user file. The temporary hard link is removed afterward.
        fs::hard_link(&temp_path, &destination)
            .map_err(|error| format!("cannot publish output (will not overwrite): {error}"))?;
        Ok(())
    })();
    let _ = fs::remove_file(&temp_path);
    result?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync output directory: {error}"))?;
    println!("Custom envelope written: {}", destination.display());
    println!("Object digests: VERIFIED; publisher signature: NOT VERIFIED");
    println!("Offline media completeness: NOT VERIFIED; pack is NOT installed");
    Ok(())
}

/// Verify, then extract into a private staging directory. This deliberately
/// never changes installed packs or confers publisher trust.
fn stage_unsigned(source: PathBuf, parent: PathBuf, name: &str) -> Result<(), String> {
    let input = File::open(&source).map_err(|error| format!("cannot open input: {error}"))?;
    let (manifest, staged) = stage_container(&mut BufReader::new(input), &parent, name)
        .map_err(|error| format!("unsigned staging rejected: {error:?}"))?;
    println!("Envelope verified and staged: {}", staged.display());
    println!("Pack ID: {}", manifest.pack_id);
    match manifest.origin {
        Origin::Official { publisher } => {
            println!("Claimed official publisher: {publisher}; claim NOT authenticated");
        }
        Origin::Custom { definition_id } => {
            println!("Claimed custom definition: {definition_id}");
        }
    }
    println!("Status: UNTRUSTED STAGING ONLY; pack NOT INSTALLED");
    println!("Publisher signature: NOT VERIFIED; required offline media: NOT VERIFIED");
    Ok(())
}

/// Stage an unseekable stdin archive with an explicit spool quota. This is
/// useful for document-provider-like pipes and never activates an installation.
fn stage_unsigned_stdin(parent: PathBuf, name: &str, max_bytes: u64) -> Result<(), String> {
    let stdin = std::io::stdin();
    let (manifest, staged) =
        stage_container_stream_with_budget(&mut stdin.lock(), &parent, name, max_bytes)
            .map_err(|error| format!("unsigned stdin staging rejected: {error:?}"))?;
    println!("Envelope verified and staged: {}", staged.display());
    println!("Pack ID: {}", manifest.pack_id);
    match manifest.origin {
        Origin::Official { publisher } => {
            println!("Claimed official publisher: {publisher}; claim NOT authenticated");
        }
        Origin::Custom { definition_id } => {
            println!("Claimed custom definition: {definition_id}");
        }
    }
    println!("Status: UNTRUSTED STAGING ONLY; pack NOT INSTALLED");
    println!("Publisher signature: NOT VERIFIED; required offline media: NOT VERIFIED");
    Ok(())
}

fn main() {
    let mut args = env::args_os().skip(1);
    match args.next().as_deref().and_then(OsStr::to_str) {
        None | Some("--help" | "-h") if args.next().is_none() => usage(),
        Some("confirm-public-inventory") => {
            let supplied: Vec<OsString> = args.collect();
            let json = supplied.first().is_some_and(|value| value == "--json");
            let fields = if json { &supplied[1..] } else { &supplied[..] };
            if fields.len() != 4 {
                eprintln!("confirm-public-inventory requires four arguments, optionally prefixed with --json");
                usage();
                std::process::exit(2);
            }
            let Some(identity) = fields[0..3]
                .iter()
                .map(|value| value.to_str())
                .collect::<Option<Vec<_>>>()
            else {
                eprintln!("Project, generation and SHA-256 must be UTF-8");
                std::process::exit(2);
            };
            if let Err(error) = fetch_public_dump::confirm_public_inventory(
                identity[0],
                identity[1],
                identity[2],
                Path::new(&fields[3]),
                json,
            ) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("fetch-public-inventory") => {
            let fields: Vec<OsString> = args.collect();
            if fields.len() != 4 {
                eprintln!("fetch-public-inventory requires exactly four arguments");
                usage();
                std::process::exit(2);
            }
            let identity = fields[0..3]
                .iter()
                .map(|value| value.to_str())
                .collect::<Option<Vec<_>>>();
            let Some(identity) = identity else {
                eprintln!("Project, generation and SHA-256 must be UTF-8");
                std::process::exit(2);
            };
            if let Err(error) =
                fetch_public_dump::run(identity[0], identity[1], identity[2], Path::new(&fields[3]))
            {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(command @ ("fetch-public-member" | "fetch-public-member-resume")) => {
            let fields: Vec<OsString> = args.collect();
            if fields.len() != 6 {
                eprintln!("fetch-public-member requires exactly six arguments");
                usage();
                std::process::exit(2);
            }
            let Some(identity) = [0usize, 1, 2, 4, 5]
                .iter()
                .map(|index| fields[*index].to_str())
                .collect::<Option<Vec<_>>>()
            else {
                eprintln!("Member identity, index and limits must be UTF-8");
                std::process::exit(2);
            };
            let (Ok(index), Ok(max_bytes)) =
                (identity[3].parse::<usize>(), identity[4].parse::<u64>())
            else {
                eprintln!("Member index and compressed byte limit must be decimal integers");
                std::process::exit(2);
            };
            let result = if command == "fetch-public-member-resume" {
                fetch_public_dump::run_resumed_member(
                    identity[0],
                    identity[1],
                    identity[2],
                    Path::new(&fields[3]),
                    index,
                    max_bytes,
                )
            } else {
                fetch_public_dump::run_member(
                    identity[0],
                    identity[1],
                    identity[2],
                    Path::new(&fields[3]),
                    index,
                    max_bytes,
                )
            };
            if let Err(error) = result {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(command @ ("fetch-public-members" | "fetch-public-members-resume")) => {
            let fields: Vec<OsString> = args.collect();
            if fields.len() != 5 {
                eprintln!("fetch-public-members requires exactly five arguments");
                usage();
                std::process::exit(2);
            }
            let Some(identity) = [0usize, 1, 2, 4]
                .iter()
                .map(|index| fields[*index].to_str())
                .collect::<Option<Vec<_>>>()
            else {
                eprintln!("Generation identity and byte limit must be UTF-8");
                std::process::exit(2);
            };
            let Ok(max_bytes) = identity[3].parse::<u64>() else {
                eprintln!("Compressed byte limit must be a decimal integer");
                std::process::exit(2);
            };
            let result = if command == "fetch-public-members-resume" {
                fetch_public_dump::run_all_resumed_members(
                    identity[0],
                    identity[1],
                    identity[2],
                    Path::new(&fields[3]),
                    max_bytes,
                )
            } else {
                fetch_public_dump::run_all_members(
                    identity[0],
                    identity[1],
                    identity[2],
                    Path::new(&fields[3]),
                    max_bytes,
                )
            };
            if let Err(error) = result {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("fetch-public-generation") => {
            let supplied: Vec<OsString> = args.collect();
            let json = supplied.first().is_some_and(|value| value == "--json");
            let fields = if json { &supplied[1..] } else { &supplied[..] };
            if fields.len() != 5 {
                eprintln!("fetch-public-generation requires five arguments, optionally prefixed with --json");
                usage();
                std::process::exit(2);
            }
            let Some(identity) = [0usize, 1, 2, 4]
                .iter()
                .map(|index| fields[*index].to_str())
                .collect::<Option<Vec<_>>>()
            else {
                eprintln!("Generation identity and byte limit must be UTF-8");
                std::process::exit(2);
            };
            let Ok(max_bytes) = identity[3].parse::<u64>() else {
                eprintln!("Compressed byte limit must be a decimal integer");
                std::process::exit(2);
            };
            if let Err(error) = fetch_public_dump::fetch_public_generation(
                identity[0],
                identity[1],
                identity[2],
                Path::new(&fields[3]),
                max_bytes,
                json,
            ) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("verify-public-generation-live") => {
            let supplied: Vec<OsString> = args.collect();
            let json = supplied.first().is_some_and(|value| value == "--json");
            let fields = if json { &supplied[1..] } else { &supplied[..] };
            if fields.len() != 5 {
                eprintln!("verify-public-generation-live requires five arguments, optionally prefixed with --json");
                usage();
                std::process::exit(2);
            }
            let Some(identity) = [0usize, 1, 2, 4]
                .iter()
                .map(|index| fields[*index].to_str())
                .collect::<Option<Vec<_>>>()
            else {
                eprintln!("Generation identity and byte limit must be UTF-8");
                std::process::exit(2);
            };
            let Ok(max_bytes) = identity[3].parse::<u64>() else {
                eprintln!("Compressed byte limit must be a decimal integer");
                std::process::exit(2);
            };
            if let Err(error) = fetch_public_dump::verify_public_generation_live(
                identity[0],
                identity[1],
                identity[2],
                Path::new(&fields[3]),
                max_bytes,
                json,
            ) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("verify-public-members") => {
            let supplied: Vec<OsString> = args.collect();
            let json = supplied.first().is_some_and(|value| value == "--json");
            let fields = if json { &supplied[1..] } else { &supplied[..] };
            if fields.len() != 5 {
                eprintln!("verify-public-members requires five arguments, optionally prefixed with --json");
                usage();
                std::process::exit(2);
            }
            let Some(identity) = [0usize, 1, 2, 4]
                .iter()
                .map(|index| fields[*index].to_str())
                .collect::<Option<Vec<_>>>()
            else {
                eprintln!("Generation identity and byte limit must be UTF-8");
                std::process::exit(2);
            };
            let Ok(max_bytes) = identity[3].parse::<u64>() else {
                eprintln!("Compressed byte limit must be a decimal integer");
                std::process::exit(2);
            };
            let result = if json {
                fetch_public_dump::verify_public_members_json(
                    identity[0],
                    identity[1],
                    identity[2],
                    Path::new(&fields[3]),
                    max_bytes,
                )
            } else {
                fetch_public_dump::verify_public_members(
                    identity[0],
                    identity[1],
                    identity[2],
                    Path::new(&fields[3]),
                    max_bytes,
                )
            };
            if let Err(error) = result {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("audit-public-dump") => {
            let fields: Vec<OsString> = args.collect();
            if fields.len() != 11 {
                eprintln!("audit-public-dump requires exactly eleven arguments");
                usage();
                std::process::exit(2);
            }
            let identity = [1usize, 2, 3, 5, 6, 8, 9, 10]
                .iter()
                .map(|index| fields[*index].to_str())
                .collect::<Option<Vec<_>>>();
            let Some(identity) = identity else {
                eprintln!("Dump identity, digests, output name and limits must be UTF-8");
                std::process::exit(2);
            };
            let namespace = identity[4].parse::<i32>();
            let max_compressed = identity[6].parse::<u64>();
            let max_decoded = identity[7].parse::<u64>();
            let (Ok(namespace), Ok(max_compressed_bytes), Ok(max_decoded_bytes)) =
                (namespace, max_compressed, max_decoded)
            else {
                eprintln!("Namespace and byte limits must be decimal integers");
                std::process::exit(2);
            };
            let options = audit_public_dump::NativeDumpAudit {
                source_root: Path::new(&fields[0]),
                project: identity[0],
                generation: identity[1],
                inventory_sha256: identity[2],
                expected_path: Path::new(&fields[4]),
                expected_sha256: identity[3],
                namespace,
                output_root: Path::new(&fields[7]),
                output_name: identity[5],
                max_compressed_bytes,
                max_decoded_bytes,
            };
            if let Err(error) = audit_public_dump::run(&options) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("audit-diagnostic") => {
            let fields: Vec<OsString> = args.collect();
            if fields.len() != 6 && fields.len() != 7 {
                eprintln!(
                    "audit-diagnostic requires six arguments and an optional revision digest"
                );
                usage();
                std::process::exit(2);
            }
            let parsed = fields[2..]
                .iter()
                .map(|value| value.to_str())
                .collect::<Option<Vec<_>>>();
            let Some(parsed) = parsed else {
                eprintln!("Diagnostic scope and digest must be UTF-8");
                std::process::exit(2);
            };
            let namespace = match parsed[2].parse::<i32>() {
                Ok(value) if value >= 0 => value,
                _ => {
                    eprintln!("Diagnostic namespace must be a nonnegative 32-bit integer");
                    std::process::exit(2);
                }
            };
            if let Err(error) = audit_diagnostic::run(
                Path::new(&fields[0]),
                Path::new(&fields[1]),
                parsed[0],
                parsed[1],
                namespace,
                parsed[3],
                parsed.get(4).copied(),
            ) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("inventory-css-graph") => {
            let fields: Vec<OsString> = args.collect();
            if fields.len() != 1 {
                eprintln!("inventory-css-graph requires one local JSON input");
                usage();
                std::process::exit(2);
            }
            if let Err(error) = css_graph_diagnostic::run(Path::new(&fields[0])) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("inventory-visuals") => {
            let fields: Vec<OsString> = args.collect();
            if fields.len() != 2 {
                eprintln!("inventory-visuals requires a local HTML file and document URL");
                usage();
                std::process::exit(2);
            }
            let Some(document_url) = fields[1].to_str() else {
                eprintln!("Document URL must be UTF-8");
                std::process::exit(2);
            };
            if let Err(error) = visual_diagnostic::run(Path::new(&fields[0]), document_url) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("list-curated") => {
            let fields: Vec<OsString> = args.collect();
            let json_output = match fields.as_slice() {
                [] => false,
                [flag] if flag.as_os_str() == OsStr::new("--json") => true,
                _ => {
                    eprintln!("list-curated accepts only an optional --json flag");
                    usage();
                    std::process::exit(2);
                }
            };
            if let Err(error) = curated_catalog::run(json_output) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("preview-topic") => {
            let fields: Vec<OsString> = args.collect();
            let (json_output, input) = match fields.as_slice() {
                [input] => (false, input),
                [flag, input] if flag.as_os_str() == OsStr::new("--json") => (true, input),
                _ => {
                    eprintln!("preview-topic requires [--json] and one local JSON input");
                    usage();
                    std::process::exit(2);
                }
            };
            if let Err(error) = preview_topic::run(Path::new(input), json_output) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("media-owner-release") => {
            let fields: Vec<OsString> = args.collect();
            if fields.len() != 6
                || !matches!(fields[0].to_str(), Some("--dry-run" | "--confirm-release"))
            {
                eprintln!("media-owner-release requires explicit --dry-run or --confirm-release, root, registry, kind, owner-id and candidate limit");
                usage();
                std::process::exit(2);
            }
            let (Some(kind), Some(owner_id), Some(limit)) = (
                fields[3].to_str(),
                fields[4].to_str(),
                fields[5]
                    .to_str()
                    .and_then(|value| value.parse::<usize>().ok()),
            ) else {
                eprintln!("Owner kind/id and candidate limit must be UTF-8 decimal values");
                std::process::exit(2);
            };
            if let Err(error) = media_maintenance::release_owner(
                fields[0].to_str().unwrap(),
                Path::new(&fields[1]),
                Path::new(&fields[2]),
                kind,
                owner_id,
                limit,
            ) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("media-maintenance") => {
            let fields: Vec<OsString> = args.collect();
            let Some(mode) = fields.first().and_then(|value| value.to_str()) else {
                eprintln!("Media maintenance mode must be UTF-8");
                std::process::exit(2);
            };
            let scheduled = mode == "--run-if-due";
            if (scheduled && fields.len() != 6) || (!scheduled && fields.len() != 5) {
                eprintln!("media-maintenance requires mode, root, registry, two budgets and an interval only for --run-if-due");
                usage();
                std::process::exit(2);
            }
            if !matches!(
                mode,
                "--audit-unbound"
                    | "--bind-registry"
                    | "--dry-run"
                    | "--confirm-delete"
                    | "--run-if-due"
            ) {
                eprintln!("Explicit media maintenance mode required");
                std::process::exit(2);
            }
            let Some(limits) = fields[3..5]
                .iter()
                .map(|value| value.to_str()?.parse::<usize>().ok())
                .collect::<Option<Vec<_>>>()
            else {
                eprintln!("Media maintenance budgets must be decimal integers");
                std::process::exit(2);
            };
            let result = if scheduled {
                let interval = fields[5]
                    .to_str()
                    .and_then(|value| value.parse::<u64>().ok());
                match interval {
                    Some(seconds) => media_maintenance::run_if_due(
                        Path::new(&fields[1]),
                        Path::new(&fields[2]),
                        limits[0],
                        limits[1],
                        seconds,
                    ),
                    None => Err("scheduled interval must be a decimal integer".into()),
                }
            } else {
                media_maintenance::run(
                    mode,
                    Path::new(&fields[1]),
                    Path::new(&fields[2]),
                    limits[0],
                    limits[1],
                )
            };
            if let Err(error) = result {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("stage-unsigned-stdin") => {
            let fields: Vec<OsString> = args.collect();
            if fields.len() != 3 {
                eprintln!("stage-unsigned-stdin requires exactly three arguments");
                usage();
                std::process::exit(2);
            }
            let Some(name) = fields[1].to_str() else {
                eprintln!("Staging directory name must be UTF-8");
                std::process::exit(2);
            };
            let Some(max_bytes) = fields[2]
                .to_str()
                .and_then(|value| value.parse::<u64>().ok())
            else {
                eprintln!("Spool budget must be a decimal byte count");
                std::process::exit(2);
            };
            if let Err(error) = stage_unsigned_stdin(PathBuf::from(&fields[0]), name, max_bytes) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("stage-unsigned") => {
            let fields: Vec<OsString> = args.collect();
            if fields.len() != 3 {
                eprintln!("stage-unsigned requires exactly three arguments");
                usage();
                std::process::exit(2);
            }
            let Some(name) = fields[2].to_str() else {
                eprintln!("Staging directory name must be UTF-8");
                std::process::exit(2);
            };
            if let Err(error) =
                stage_unsigned(PathBuf::from(&fields[0]), PathBuf::from(&fields[1]), name)
            {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("export-custom") => {
            let fields: Vec<OsString> = args.collect();
            if fields.len() != 6 {
                eprintln!("export-custom requires exactly six arguments");
                usage();
                std::process::exit(2);
            }
            let identity = fields[1..5]
                .iter()
                .map(|value| value.to_str())
                .collect::<Option<Vec<_>>>();
            let Some(identity) = identity else {
                eprintln!("Pack identity fields must be UTF-8");
                std::process::exit(2);
            };
            if let Err(error) = export_custom_tree(
                PathBuf::from(&fields[0]),
                identity[0],
                identity[1],
                identity[2],
                identity[3],
                PathBuf::from(&fields[5]),
            ) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some("verify" | "inspect") => {
            let detailed = env::args_os().nth(1).as_deref() == Some(OsStr::new("inspect"));
            match (args.next(), args.next()) {
                (Some(path), None) => {
                    if let Err(error) = inspect_file(PathBuf::from(path), detailed) {
                        eprintln!("{error}");
                        std::process::exit(1);
                    }
                }
                _ => {
                    eprintln!("Expected exactly one input .wpack file");
                    usage();
                    std::process::exit(2);
                }
            }
        }
        _ => {
            eprintln!("Unsupported or malformed command");
            usage();
            std::process::exit(2);
        }
    }
}
