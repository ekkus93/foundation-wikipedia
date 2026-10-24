//! Offline retrieval benchmark runner for a caller-supplied canonical article corpus.
//!
//! Usage:
//! cargo run -p wiki-search --example retrieval_suite_runner --locked -- \\
//!   --corpus articles.json --suite labels.json --sha <40-hex-commit> \\
//!   --output report.json [--top-k 5] [--min-recall-per-mille 800] \\
//!   [--max-no-answer-fp 0]
//!
//! This does not authenticate the source or labels. It must not be described as
//! a publisher-verified Wikimedia benchmark without independent provenance.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
};

use wiki_model::Article;
use wiki_search::{
    evaluation::RetrievalThresholds, evaluation_suite::run_retrieval_suite_json, CorpusLexicalIndex,
};

const MAX_CORPUS_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARTICLES: usize = 20_000;
const MAX_SUITE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct Args {
    corpus: PathBuf,
    suite: PathBuf,
    output: PathBuf,
    sha: String,
    top_k: usize,
    min_recall_per_mille: u16,
    max_no_answer_fp: usize,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut corpus = None;
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
    Ok(Args {
        corpus: corpus.ok_or("missing --corpus")?,
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

fn run(args: Args) -> Result<(), String> {
    let corpus_bytes = read_bounded(&args.corpus, MAX_CORPUS_BYTES)?;
    let articles: Vec<Article> = serde_json::from_slice(&corpus_bytes)
        .map_err(|error| format!("invalid canonical article corpus: {error}"))?;
    if articles.is_empty() || articles.len() > MAX_ARTICLES {
        return Err(format!("corpus must contain 1..={MAX_ARTICLES} articles"));
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
    }
}
