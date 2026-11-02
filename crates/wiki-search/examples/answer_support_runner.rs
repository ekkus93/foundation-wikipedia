//! CLI for explicitly human-reviewed generated-answer support evaluation.
//! This grades reviewer labels; it does not infer factual entailment or
//! authenticate the Wikipedia source, labels, reviewer or model output.
//!
//! cargo run -p wiki-search --example answer_support_runner --locked -- \
//!   --suite reviewed_answers.json --output answer_support_report.json \
//!   [--min-claim-support-per-mille 800] \
//!   [--min-citation-precision-per-mille 800] [--corpus canonical_articles.json]
//!   [--min-case-claim-support-per-mille 800]
//!   [--min-case-citation-precision-per-mille 800]
//! Without --corpus, only reviewer-label/ID syntax is checked. With --corpus,
//! each citation must exist in an exact validated article revision; source and
//! human-label authenticity still require independent verification.

use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
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
    min_case_claim_support_per_mille: u16,
    min_case_citation_precision_per_mille: u16,
}

fn parse_args(values: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut corpus = None;
    let mut suite = None;
    let mut output = None;
    let mut claim_floor = 0;
    let mut citation_floor = 0;
    let mut case_claim_floor = 0;
    let mut case_citation_floor = 0;
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
            "--min-case-claim-support-per-mille" => {
                case_claim_floor = value
                    .parse::<u16>()
                    .map_err(|_| "invalid case claim floor")?;
            }
            "--min-case-citation-precision-per-mille" => {
                case_citation_floor = value
                    .parse::<u16>()
                    .map_err(|_| "invalid case citation floor")?;
            }
            _ => return Err(format!("unknown option: {flag}")),
        }
    }
    if claim_floor > 1_000
        || citation_floor > 1_000
        || case_claim_floor > 1_000
        || case_citation_floor > 1_000
    {
        return Err("thresholds must be 0..=1000".into());
    }
    Ok(Args {
        corpus,
        suite: suite.ok_or("missing --suite")?,
        output: output.ok_or("missing --output")?,
        min_claim_support_per_mille: claim_floor,
        min_citation_precision_per_mille: citation_floor,
        min_case_claim_support_per_mille: case_claim_floor,
        min_case_citation_precision_per_mille: case_citation_floor,
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

fn per_case_gate(
    cases: &[wiki_search::answer_support::CaseSupportScore],
    floor: u16,
    counts: impl Fn(&wiki_search::answer_support::SupportCounts) -> (usize, usize),
) -> serde_json::Value {
    let failed_case_ids: Vec<&str> = if floor == 0 {
        Vec::new()
    } else {
        cases
            .iter()
            .filter(|case| {
                let (numerator, denominator) = counts(&case.counts);
                !meets_floor(numerator, denominator, floor)
            })
            .map(|case| case.id.as_str())
            .collect()
    };
    serde_json::json!({
        "enabled": floor > 0,
        "min_per_mille": floor,
        "evaluated_case_count": if floor > 0 { cases.len() } else { 0 },
        "failed_case_ids": failed_case_ids,
        "passed": failed_case_ids.is_empty() && (floor == 0 || !cases.is_empty())
    })
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
    let case_claim_gate = per_case_gate(
        &report.cases,
        args.min_case_claim_support_per_mille,
        |counts| (counts.supported_claims, counts.reviewed_claims),
    );
    let case_citation_gate = per_case_gate(
        &report.cases,
        args.min_case_citation_precision_per_mille,
        |counts| (counts.supported_citations, counts.reviewed_citations),
    );
    let passed = case_claim_gate["passed"] == true
        && case_citation_gate["passed"] == true
        && report.all_claims_reviewed
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
            "case_claim_support_gate": case_claim_gate,
            "case_citation_precision_gate": case_citation_gate,
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
                min_case_claim_support_per_mille: 0,
                min_case_citation_precision_per_mille: 0,
            }
        );
        assert!(parse_args(values(&["--suite", "x"])).is_err());
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--min-case-claim-support-per-mille",
            "1001"
        ]))
        .is_err());
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--min-case-citation-precision-per-mille",
            "bad"
        ]))
        .is_err());
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--min-case-claim-support-per-mille",
            "1",
            "--min-case-claim-support-per-mille",
            "2"
        ]))
        .is_err());
        assert_eq!(
            parse_args(values(&[
                "--suite", "x", "--output", "z", "--corpus", "a.json"
            ]))
            .unwrap()
            .corpus,
            Some("a.json".into())
        );
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

    #[test]
    fn per_case_gates_expose_weak_case_masked_by_strong_case() {
        use wiki_search::answer_support::evaluate_answer_support_json;
        let suite = serde_json::json!({
            "schema_version": 1, "suite_id": "case_floor_test",
            "cases": [
                {
                    "id": "strong", "project": "enwiki", "page_id": 42,
                    "revision_id": 100, "question": "Strong?",
                    "answer": "Supported [[cite:wkb:enwiki:42:100:b0]]",
                    "claims": [{
                        "id": "c1", "text": "Supported", "verdict": "supported",
                        "citations": [{"block_id": "wkb:enwiki:42:100:b0", "verdict": "supported"}]
                    }]
                },
                {
                    "id": "weak", "project": "enwiki", "page_id": 42,
                    "revision_id": 100, "question": "Weak?",
                    "answer": "Unsupported [[cite:wkb:enwiki:42:100:b0]]",
                    "claims": [{
                        "id": "c2", "text": "Unsupported", "verdict": "unsupported",
                        "citations": [{"block_id": "wkb:enwiki:42:100:b0", "verdict": "unsupported"}]
                    }]
                }
            ]
        });
        let report = evaluate_answer_support_json(&suite.to_string()).unwrap();
        assert!(meets_floor(
            report.totals.supported_claims,
            report.totals.reviewed_claims,
            500
        ));
        let claim_gate = per_case_gate(&report.cases, 800, |counts| {
            (counts.supported_claims, counts.reviewed_claims)
        });
        let citation_gate = per_case_gate(&report.cases, 800, |counts| {
            (counts.supported_citations, counts.reviewed_citations)
        });
        assert_eq!(claim_gate["failed_case_ids"], serde_json::json!(["weak"]));
        assert_eq!(
            citation_gate["failed_case_ids"],
            serde_json::json!(["weak"])
        );
        assert_eq!(claim_gate["passed"], false);
        assert_eq!(citation_gate["passed"], false);
        assert_eq!(per_case_gate(&report.cases, 0, |_| (0, 0))["passed"], true);
        assert_eq!(per_case_gate(&[], 800, |_| (0, 0))["passed"], false);
        assert_eq!(
            per_case_gate(&report.cases, 800, |_| (0, 0))["failed_case_ids"],
            serde_json::json!(["strong", "weak"])
        );
    }

    #[test]
    fn strict_runner_scores_only_citations_present_in_exact_canonical_corpus() {
        use wiki_model::{ArticleKey, Block, BlockContent, Revision, ARTICLE_SCHEMA_VERSION};

        let root = std::env::temp_dir().join(format!(
            "wiki-answer-corpus-{}-{}",
            process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let suite = root.join("suite.json");
        let corpus = root.join("articles.json");
        let output = root.join("report.json");
        let article = Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
            revision: Revision {
                revision_id: 100,
                timestamp: "2026-11-02T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Moons".into(),
            display_title: "Moons".into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph("Two moons orbit.".into()),
            }],
            sections: vec![],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<article>Moons</article>".into(),
            is_disambiguation: false,
        };
        let labels = serde_json::json!({
            "schema_version": 1,
            "suite_id": "strict_test",
            "cases": [{
                "id": "c1", "project": "enwiki", "page_id": 42, "revision_id": 100,
                "question": "How many moons?",
                "answer": "Two moons [[cite:wkb:enwiki:42:100:b0]]",
                "claims": [{
                    "id": "claim", "text": "Two moons", "verdict": "supported",
                    "citations": [{"block_id": "wkb:enwiki:42:100:b0", "verdict": "supported"}]
                }]
            }]
        });
        fs::write(&suite, labels.to_string()).unwrap();
        fs::write(&corpus, serde_json::to_vec(&vec![article.clone()]).unwrap()).unwrap();
        let args = Args {
            corpus: Some(corpus.clone()),
            suite: suite.clone(),
            output: output.clone(),
            min_claim_support_per_mille: 1_000,
            min_citation_precision_per_mille: 1_000,
            min_case_claim_support_per_mille: 1_000,
            min_case_citation_precision_per_mille: 1_000,
        };
        assert_eq!(run(args.clone()), Ok(true));
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(report["evidence_membership_checked"], true);
        assert_eq!(report["gate"]["passed"], true);
        assert_eq!(report["gate"]["case_claim_support_gate"]["passed"], true);
        assert_eq!(
            report["gate"]["case_citation_precision_gate"]["passed"],
            true
        );
        assert_eq!(report["corpus_input_sha256"].as_str().unwrap().len(), 64);

        // The aggregate 50% score passes, but the weak case must fail both
        // per-case floors. The executable path must persist that diagnosis.
        let mut weak_suite = labels.clone();
        let mut weak_case = weak_suite["cases"][0].clone();
        weak_case["id"] = serde_json::json!("weak");
        weak_case["answer"] = serde_json::json!("Incorrect [[cite:wkb:enwiki:42:100:b0]]");
        weak_case["claims"][0]["text"] = serde_json::json!("Incorrect");
        weak_case["claims"][0]["verdict"] = serde_json::json!("unsupported");
        weak_case["claims"][0]["citations"][0]["verdict"] = serde_json::json!("unsupported");
        weak_suite["cases"].as_array_mut().unwrap().push(weak_case);
        fs::write(&suite, weak_suite.to_string()).unwrap();
        let weak_output = root.join("weak.json");
        assert_eq!(
            run(Args {
                output: weak_output.clone(),
                min_claim_support_per_mille: 500,
                min_citation_precision_per_mille: 500,
                min_case_claim_support_per_mille: 800,
                min_case_citation_precision_per_mille: 800,
                ..args.clone()
            }),
            Ok(false)
        );
        let weak_report: serde_json::Value =
            serde_json::from_slice(&fs::read(&weak_output).unwrap()).unwrap();
        assert_eq!(weak_report["gate"]["passed"], false);
        assert_eq!(
            weak_report["gate"]["case_claim_support_gate"]["failed_case_ids"],
            serde_json::json!(["weak"])
        );
        assert_eq!(
            weak_report["gate"]["case_citation_precision_gate"]["failed_case_ids"],
            serde_json::json!(["weak"])
        );

        let mut stale = article;
        stale.revision.revision_id = 101;
        fs::write(&corpus, serde_json::to_vec(&vec![stale]).unwrap()).unwrap();
        let stale_output = root.join("stale.json");
        assert!(run(Args {
            output: stale_output.clone(),
            ..args
        })
        .is_err());
        assert!(!stale_output.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
