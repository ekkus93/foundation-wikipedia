//! Read-only inspection of provisional, unsigned FWPACK01 envelopes.
//! These commands do not install packs or verify publisher signatures.
use std::env;
use std::ffi::OsStr;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use wiki_pack_format::container::verify_container;
use wiki_pack_format::manifest::Origin;

fn usage() {
    println!("Foundation Wikipedia pack CLI (provisional)");
    println!("Usage: wiki-pack verify <file.wpack>");
    println!("       wiki-pack inspect <file.wpack>");
    println!("       wiki-pack --help");
    println!("Both commands verify every object digest; neither installs a pack.");
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

fn main() {
    let mut args = env::args_os().skip(1);
    match args.next().as_deref().and_then(OsStr::to_str) {
        None | Some("--help" | "-h") if args.next().is_none() => usage(),
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
