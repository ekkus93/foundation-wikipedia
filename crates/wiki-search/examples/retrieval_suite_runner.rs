//! Offline retrieval benchmark runner for a caller-supplied canonical article corpus.
//!
//! Usage:
//! cargo run -p wiki-search --example retrieval_suite_runner --locked -- \\
//!   --pack corpus.wpack --suite labels.json --sha <40-hex-commit> \\
//!   --output report.json [--top-k 5] [--min-recall-per-mille 800] \\
//!   [--max-no-answer-fp 0]
//!
//! Use --corpus articles.json instead of --pack for unpacked input.
//! A provisional pack must contain an articles.json canonical article array.
//! This does not authenticate the source or labels. It must not be described as
//! a publisher-verified Wikimedia benchmark without independent provenance.

use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process,
};

use wiki_model::{Article, ArticleKey};
use wiki_pack_format::container::verify_container;
use wiki_search::{
    evaluation::RetrievalThresholds, evaluation_suite::run_retrieval_suite_json, CorpusLexicalIndex,
};

const MAX_CORPUS_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARTICLES: usize = 20_000;
const MAX_SUITE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct Args {
    corpus: Option<PathBuf>,
    pack: Option<PathBuf>,
    suite: PathBuf,
    output: PathBuf,
    sha: String,
    top_k: usize,
    min_recall_per_mille: u16,
    max_no_answer_fp: usize,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut corpus = None;
    let mut pack = None;
    let mut suite = None;
    let mut output = None;
    let mut sha = None;
    let mut top_k = 5;
    let mut min_recall_per_mille = 800;
    let mut max_no_answer_fp = 0;
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--corpus" if corpus.is_none() => corpus = Some(PathBuf::from(value)),
            "--pack" if pack.is_none() => pack = Some(PathBuf::from(value)),
            "--suite" if suite.is_none() => suite = Some(PathBuf::from(value)),
            "--output" if output.is_none() => output = Some(PathBuf::from(value)),
            "--sha" if sha.is_none() => sha = Some(value),
            "--top-k" => top_k = value.parse().map_err(|_| "invalid top-k".to_owned())?,
            "--min-recall-per-mille" => {
                min_recall_per_mille = value.parse().map_err(|_| "invalid recall".to_owned())?;
            }
            "--max-no-answer-fp" => {
                max_no_answer_fp = value
                    .parse()
                    .map_err(|_| "invalid no-answer limit".to_owned())?;
            }
            _ => return Err(format!("unknown or duplicate argument: {flag}")),
        }
    }
    if !(1..=100).contains(&top_k) || min_recall_per_mille > 1_000 {
        return Err("top-k must be 1..=100 and recall threshold 0..=1000".into());
    }
    let sha = sha.ok_or("missing --sha")?;
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("--sha must be exactly 40 hexadecimal characters".into());
    }
    if corpus.is_some() == pack.is_some() {
        return Err("specify exactly one of --corpus or --pack".into());
    }
    Ok(Args {
        corpus,
        pack,
        suite: suite.ok_or("missing --suite")?,
        output: output.ok_or("missing --output")?,
        sha,
        top_k,
        min_recall_per_mille,
        max_no_answer_fp,
    })
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let size = fs::metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?
        .len();
    if size > max {
        return Err(format!("{} exceeds {max} bytes", path.display()));
    }
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if bytes.len() as u64 > max {
        return Err(format!("{} exceeds {max} bytes", path.display()));
    }
    Ok(bytes)
}

fn read_pack_corpus<R: Read + Seek>(input: &mut R) -> Result<(Vec<u8>, String), String> {
    // Verify every object before exposing article bytes. This verifies
    // manifest-declared integrity, not publisher authenticity.
    let manifest =
        verify_container(input).map_err(|error| format!("invalid pack envelope: {error:?}"))?;
    let transcript_len = manifest
        .identity_transcript()
        .map_err(|error| format!("invalid pack manifest: {error:?}"))?
        .len() as u64;
    let mut offset = 12u64
        .checked_add(transcript_len)
        .ok_or("pack object offset overflow")?;
    let mut objects = manifest.objects.iter().collect::<Vec<_>>();
    objects.sort_by(|a, b| a.path.cmp(&b.path));
    for object in objects {
        if object.path == "articles.json" {
            if object.bytes > MAX_CORPUS_BYTES {
                return Err(format!(
                    "pack articles.json exceeds {MAX_CORPUS_BYTES} bytes"
                ));
            }
            input
                .seek(SeekFrom::Start(offset))
                .map_err(|error| format!("cannot seek to pack articles.json: {error}"))?;
            let mut bytes = vec![0; object.bytes as usize];
            input
                .read_exact(&mut bytes)
                .map_err(|error| format!("cannot read pack articles.json: {error}"))?;
            if wiki_pack_format::sha256_hex(&bytes) != object.sha256.to_ascii_lowercase() {
                return Err("pack articles.json changed after envelope verification".into());
            }
            return Ok((bytes, manifest.project));
        }
        offset = offset
            .checked_add(object.bytes)
            .ok_or("pack object offset overflow")?;
    }
    Err("pack contains no articles.json canonical article array".into())
}

// A provisional pack's project declaration must agree with every canonical
// article key. An intact envelope hash alone does not establish this join.
fn validate_pack_project<'a>(
    project: &str,
    keys: impl IntoIterator<Item = &'a ArticleKey>,
) -> Result<(), String> {
    for key in keys {
        if key.project != project {
            return Err(format!(
                "pack project {project} conflicts with article project {} at page {}",
                key.project, key.page_id
            ));
        }
    }
    Ok(())
}

fn run(args: Args) -> Result<(), String> {
    let (corpus_bytes, pack_project) = if let Some(path) = &args.corpus {
        (read_bounded(path, MAX_CORPUS_BYTES)?, None)
    } else {
        let path = args.pack.as_ref().ok_or("missing --pack")?;
        let mut input = fs::File::open(path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        let (bytes, project) = read_pack_corpus(&mut input)?;
        (bytes, Some(project))
    };
    let articles: Vec<Article> = serde_json::from_slice(&corpus_bytes)
        .map_err(|error| format!("invalid canonical article corpus: {error}"))?;
    if articles.is_empty() || articles.len() > MAX_ARTICLES {
        return Err(format!("corpus must contain 1..={MAX_ARTICLES} articles"));
    }
    if let Some(project) = &pack_project {
        validate_pack_project(project, articles.iter().map(|article| &article.key))?;
    }
    let index = CorpusLexicalIndex::build(&articles)
        .map_err(|error| format!("invalid article identity or revision: {error:?}"))?;
    let suite_bytes = read_bounded(&args.suite, MAX_SUITE_BYTES)?;
    let suite = std::str::from_utf8(&suite_bytes)
        .map_err(|error| format!("suite must be UTF-8: {error}"))?;
    let thresholds = RetrievalThresholds {
        min_recall_per_mille: args.min_recall_per_mille,
        max_no_answer_false_positive_cases: args.max_no_answer_fp,
    };
    let result = run_retrieval_suite_json(&index, suite, args.top_k, &args.sha, &thresholds)
        .map_err(|error| format!("retrieval suite rejected: {error:?}"))?;
    fs::write(&args.output, result.report_json)
        .map_err(|error| format!("cannot persist {}: {error}", args.output.display()))?;
    if !result.passed {
        return Err(format!(
            "retrieval regression failed; inspect {}",
            args.output.display()
        ));
    }
    println!(
        "Retrieval gate passed for {} caller-supplied articles at {} (source/labels not authenticated)",
        articles.len(),
        args.sha
    );
    Ok(())
}

fn main() {
    let args = match parse_args(env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            process::exit(2);
        }
    };
    if let Err(error) = run(args) {
        eprintln!("{error}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments() -> Vec<String> {
        [
            "--corpus",
            "articles.json",
            "--suite",
            "suite.json",
            "--output",
            "report.json",
            "--sha",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
    }

    #[test]
    fn parses_bounded_reproducible_run() {
        let args = parse_args(arguments()).unwrap();
        assert_eq!(args.top_k, 5);
        assert_eq!(args.min_recall_per_mille, 800);
        assert_eq!(args.max_no_answer_fp, 0);
    }

    #[test]
    fn rejects_invalid_limits_duplicate_flags_and_missing_inputs() {
        let mut args = arguments();
        args.extend(["--top-k".into(), "101".into()]);
        assert!(parse_args(args).is_err());
        let mut args = arguments();
        args.extend(["--min-recall-per-mille".into(), "1001".into()]);
        assert!(parse_args(args).is_err());
        let mut args = arguments();
        args.extend(["--corpus".into(), "other.json".into()]);
        assert!(parse_args(args).is_err());
        assert!(parse_args(vec!["--suite".into(), "suite.json".into()]).is_err());
        let mut args = arguments();
        args.extend(["--pack".into(), "test.wpack".into()]);
        assert!(parse_args(args).is_err());
        let mut args = arguments();
        args[0] = "--pack".into();
        assert_eq!(
            parse_args(args).unwrap().pack,
            Some(PathBuf::from("articles.json"))
        );
    }

    #[test]
    fn verified_pack_article_object_is_read_without_extracting_media() {
        use std::io::Cursor;
        use wiki_pack_format::{
            container::write_container,
            manifest::{Manifest, Object, Origin, FORMAT_VERSION},
            sha256_hex,
        };

        let corpus = br#"[{"sample":"canonical-article-array"}]"#;
        let media = b"figure";
        let manifest = Manifest {
            version: FORMAT_VERSION,
            pack_id: "benchmark".into(),
            project: "enwiki".into(),
            snapshot: "20261024".into(),
            origin: Origin::Custom {
                definition_id: "benchmark".into(),
            },
            objects: vec![
                Object {
                    path: "media/figure.bin".into(),
                    sha256: sha256_hex(media),
                    bytes: media.len() as u64,
                },
                Object {
                    path: "articles.json".into(),
                    sha256: sha256_hex(corpus),
                    bytes: corpus.len() as u64,
                },
            ],
        };
        let mut envelope = Vec::new();
        write_container(&mut envelope, &manifest, |object, output| {
            if object.path == "articles.json" {
                output.write_all(corpus)
            } else {
                output.write_all(media)
            }
        })
        .unwrap();
        assert_eq!(
            read_pack_corpus(&mut Cursor::new(&envelope)).unwrap(),
            (corpus.to_vec(), "enwiki".to_owned())
        );
        let mut tampered = envelope.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(read_pack_corpus(&mut Cursor::new(tampered)).is_err());
        envelope.push(0);
        assert!(read_pack_corpus(&mut Cursor::new(envelope)).is_err());
    }

    #[test]
    fn provisional_pack_project_must_match_every_article_key() {
        let en = ArticleKey {
            project: "enwiki".into(),
            page_id: 12,
        };
        let other_en = ArticleKey {
            project: "enwiki".into(),
            page_id: 34,
        };
        let fr = ArticleKey {
            project: "frwiki".into(),
            page_id: 56,
        };
        assert!(validate_pack_project("enwiki", [&en, &other_en]).is_ok());
        assert!(validate_pack_project("enwiki", [&en, &fr]).is_err());
        assert!(validate_pack_project("frwiki", [&en]).is_err());
        // Raw --corpus remains intentionally multi-project; this check is
        // applied only when the caller supplies a pack manifest.
    }

    #[test]
    fn pack_without_canonical_article_array_fails_closed() {
        use std::io::Cursor;
        use wiki_pack_format::{
            container::write_container,
            manifest::{Manifest, Object, Origin, FORMAT_VERSION},
            sha256_hex,
        };

        let bytes = b"other";
        let manifest = Manifest {
            version: FORMAT_VERSION,
            pack_id: "benchmark".into(),
            project: "enwiki".into(),
            snapshot: "20261024".into(),
            origin: Origin::Custom {
                definition_id: "benchmark".into(),
            },
            objects: vec![Object {
                path: "other.json".into(),
                sha256: sha256_hex(bytes),
                bytes: bytes.len() as u64,
            }],
        };
        let mut envelope = Vec::new();
        write_container(&mut envelope, &manifest, |_object, output| {
            output.write_all(bytes)
        })
        .unwrap();
        assert!(read_pack_corpus(&mut Cursor::new(envelope)).is_err());
    }
}
