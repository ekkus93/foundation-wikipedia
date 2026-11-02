//! CLI for explicitly human-reviewed generated-answer support evaluation.
//! This grades reviewer labels; it does not infer factual entailment or
//! authenticate the Wikipedia source, labels, reviewer or model output.
//!
//! cargo run -p wiki-search --example answer_support_runner --locked -- \
//!   --suite reviewed_answers.json --output answer_support_report.json \
//!   [--min-claim-support-per-mille 800] \
//!   [--min-citation-precision-per-mille 800] [--corpus canonical_articles.json]
//! Without --corpus, only reviewer-label/ID syntax is checked. With --corpus,
//! each citation must exist in an exact validated article revision; source and
//! human-label authenticity still require independent verification.

use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use sha2::{Digest, Sha256};
use wiki_model::Article;
use wiki_search::answer_support::{
    evaluate_answer_support_json, evaluate_answer_support_with_corpus_json,
};

const MAX_SUITE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_CORPUS_BYTES: u64 = 64 * 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
struct Args {
    corpus: Option<PathBuf>,
    suite: PathBuf,
    output: PathBuf,
    min_claim_support_per_mille: u16,
    min_citation_precision_per_mille: u16,
}

fn parse_args(values: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut corpus = None;
    let mut suite = None;
    let mut output = None;
    let mut claim_floor = 0;
    let mut citation_floor = 0;
    let mut seen = std::collections::BTreeSet::new();
    let mut values = values.into_iter();
    while let Some(flag) = values.next() {
        if !seen.insert(flag.clone()) {
            return Err(format!("duplicate argument: {flag}"));
        }
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--suite" => suite = Some(PathBuf::from(value)),
            "--corpus" => corpus = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            "--min-claim-support-per-mille" => {
                claim_floor = value.parse::<u16>().map_err(|_| "invalid claim floor")?;
            }
            "--min-citation-precision-per-mille" => {
                citation_floor = value.parse::<u16>().map_err(|_| "invalid citation floor")?;
            }
            _ => return Err(format!("unknown option: {flag}")),
        }
    }
    if claim_floor > 1_000 || citation_floor > 1_000 {
        return Err("thresholds must be 0..=1000".into());
    }
    Ok(Args {
        corpus,
        suite: suite.ok_or("missing --suite")?,
        output: output.ok_or("missing --output")?,
        min_claim_support_per_mille: claim_floor,
        min_citation_precision_per_mille: citation_floor,
    })
}

fn read_suite(path: &Path) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|error| format!("cannot open suite: {error}"))?;
    if file
        .metadata()
        .map_err(|error| format!("cannot stat suite: {error}"))?
        .len()
        > MAX_SUITE_BYTES
    {
        return Err("suite is larger than 2 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_SUITE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read suite: {error}"))?;
    if bytes.len() as u64 > MAX_SUITE_BYTES {
        return Err("suite is larger than 2 MiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "suite is not valid UTF-8".into())
}


fn read_corpus(path: &Path) -> Result<(Vec<Article>, String), String> {
    let file = fs::File::open(path).map_err(|error| format!("cannot open corpus: {error}"))?;
    if file
        .metadata()
        .map_err(|error| format!("cannot stat corpus: {error}"))?
        .len()
        > MAX_CORPUS_BYTES
    {
        return Err("corpus is larger than 64 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_CORPUS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read corpus: {error}"))?;
    if bytes.len() as u64 > MAX_CORPUS_BYTES {
        return Err("corpus is larger than 64 MiB".into());
    }
    let articles: Vec<Article> = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid canonical article array: {error}"))?;
    if articles.is_empty() || articles.len() > 1_000 {
        return Err("strict corpus must contain 1..=1000 articles".into());
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok((articles, digest))
}

fn validate_output(suite: &Path, output: &Path) -> Result<(), String> {
    if fs::symlink_metadata(output).is_ok() {
        return Err("report already exists or is a symlink".into());
    }
    let suite =
        fs::canonicalize(suite).map_err(|error| format!("cannot resolve suite: {error}"))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = output.file_name().ok_or("invalid report filename")?;
    let target = fs::canonicalize(parent)
        .map_err(|error| format!("cannot resolve report directory: {error}"))?
        .join(name);
    if target == suite {
        return Err("report must not replace the annotation suite".into());
    }
    Ok(())
}

fn persist_new_report(output: &Path, content: &str) -> Result<(), String> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = output.file_name().ok_or("invalid report name")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock before Unix epoch")?
        .as_nanos();
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{nonce}-{sequence}",
        name.to_string_lossy(),
        process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("cannot create report staging file: {error}"))?;
    let published = (|| -> std::io::Result<()> {
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        fs::hard_link(&temporary, output)?;
        Ok(())
    })();
    drop(file);
    let _ = fs::remove_file(&temporary);
    published.map_err(|error| format!("cannot publish report: {error}"))
}

fn meets_floor(numerator: usize, denominator: usize, floor: u16) -> bool {
    denominator != 0 && (numerator as u128) * 1_000 >= (denominator as u128) * u128::from(floor)
}

fn run(args: Args) -> Result<bool, String> {
    validate_output(&args.suite, &args.output)?;
    let raw = read_suite(&args.suite)?;
    let (report, corpus_sha256) = if let Some(path) = &args.corpus {
        let (articles, digest) = read_corpus(path)?;
        let report = evaluate_answer_support_with_corpus_json(&raw, &articles)
            .map_err(|error| format!("invalid reviewed canonical evidence: {error:?}"))?;
        (report, Some(digest))
    } else {
        let report = evaluate_answer_support_json(&raw)
            .map_err(|error| format!("invalid human-reviewed support suite: {error:?}"))?;
        (report, None)
    };
    let passed = report.all_claims_reviewed
        && report.all_citations_reviewed
        && meets_floor(
            report.totals.supported_claims,
            report.totals.reviewed_claims,
            args.min_claim_support_per_mille,
        )
        && meets_floor(
            report.totals.supported_citations,
            report.totals.reviewed_citations,
            args.min_citation_precision_per_mille,
        );
    let mut body = serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": 1,
        "evaluation": report,
        "corpus_input_sha256": corpus_sha256,
        "evidence_membership_checked": args.corpus.is_some(),
        "gate": {
            "min_claim_support_per_mille": args.min_claim_support_per_mille,
            "min_citation_precision_per_mille": args.min_citation_precision_per_mille,
            "passed": passed
        },
        "disclaimer": "Caller-supplied human labels only; no authenticated provenance or automated factual entailment"
    }))
    .map_err(|error| format!("cannot serialize report: {error}"))?;
    body.push('\n');
    // Failed gates still produce diagnostic evidence. Invalid labels never do.
    persist_new_report(&args.output, &body)?;
    Ok(passed)
}

fn main() {
    match parse_args(env::args().skip(1)).and_then(run) {
        Ok(true) => println!("Human-reviewed answer support gate passed"),
        Ok(false) => {
            eprintln!("Human-reviewed answer support gate failed; diagnostic report saved");
            process::exit(1);
        }
        Err(error) => {
            eprintln!("Answer support benchmark rejected: {error}");
            process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_requires_explicit_inputs_and_unique_bounded_thresholds() {
        let values = |args: &[&str]| args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            parse_args(values(&["--suite", "a.json", "--output", "b.json"])).unwrap(),
            Args {
                corpus: None,
                suite: "a.json".into(),
                output: "b.json".into(),
                min_claim_support_per_mille: 0,
                min_citation_precision_per_mille: 0,
            }
        );
        assert!(parse_args(values(&["--suite", "x"])).is_err());
        assert_eq!(parse_args(values(&["--suite", "x", "--output", "z", "--corpus", "a.json"])).unwrap().corpus, Some("a.json".into()));
        assert!(parse_args(values(&["--suite", "x", "--suite", "y", "--output", "z"])).is_err());
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--min-claim-support-per-mille",
            "1001"
        ]))
        .is_err());
    }

    #[test]
    fn thresholds_do_not_accept_empty_denominators() {
        assert!(!meets_floor(0, 0, 0));
        assert!(meets_floor(4, 5, 800));
        assert!(!meets_floor(3, 5, 800));
        assert!(meets_floor(1, 2, 0));
    }

    #[test]
    fn no_clobber_report_publication_and_cleanup() {
        let root = std::env::temp_dir().join(format!(
            "wiki-answer-support-{}-{}",
            process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let suite = root.join("suite.json");
        fs::write(&suite, "{}").unwrap();
        let output = root.join("report.json");
        validate_output(&suite, &output).unwrap();
        persist_new_report(&output, "first").unwrap();
        assert!(persist_new_report(&output, "second").is_err());
        assert_eq!(fs::read_to_string(&output).unwrap(), "first");
        assert!(validate_output(&suite, &output).is_err());
        assert!(validate_output(&suite, &suite).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}
