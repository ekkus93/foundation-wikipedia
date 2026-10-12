//! Verification and provisional custom export of unsigned FWPACK01 envelopes.
//! These commands do not install packs or verify publisher signatures.
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

fn usage() {
    println!("Foundation Wikipedia pack CLI (provisional)");
    println!("Usage: wiki-pack verify <file.wpack>");
    println!("       wiki-pack inspect <file.wpack>");
    println!("       wiki-pack export-custom <object-dir> <pack-id> <project> <snapshot> <definition-id> <output.wpack>");
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

fn main() {
    let mut args = env::args_os().skip(1);
    match args.next().as_deref().and_then(OsStr::to_str) {
        None | Some("--help" | "-h") if args.next().is_none() => usage(),
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
