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
//!   [--expect-suite-sha256 64-lowercase-hex --expect-corpus-sha256 64-lowercase-hex]
//!   [--require-schema-v3] [--min-cases 100] [--min-distinct-articles 25]
//!   [--min-project-claim-support-per-mille 800]
//!   [--min-project-citation-precision-per-mille 800]
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    corpus: Option<PathBuf>,
    require_schema_v3: bool,
    expected_suite_sha256: Option<String>,
    expected_corpus_sha256: Option<String>,
    suite: PathBuf,
    output: PathBuf,
    min_claim_support_per_mille: u16,
    min_citation_precision_per_mille: u16,
    min_case_claim_support_per_mille: u16,
    min_case_citation_precision_per_mille: u16,
    min_project_claim_support_per_mille: u16,
    min_project_citation_precision_per_mille: u16,
    min_cases: usize,
    min_distinct_articles: usize,
}

fn parse_args(values: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut corpus = None;
    let mut require_schema_v3 = false;
    let mut expected_suite_sha256 = None;
    let mut expected_corpus_sha256 = None;
    let mut suite = None;
    let mut output = None;
    let mut claim_floor = 0;
    let mut citation_floor = 0;
    let mut case_claim_floor = 0;
    let mut case_citation_floor = 0;
    let mut project_claim_floor = 0;
    let mut project_citation_floor = 0;
    let mut min_cases = 0;
    let mut min_distinct_articles = 0;
    let mut seen = std::collections::BTreeSet::new();
    let mut values = values.into_iter();
    while let Some(flag) = values.next() {
        if !seen.insert(flag.clone()) {
            return Err(format!("duplicate argument: {flag}"));
        }
        if flag == "--require-schema-v3" {
            require_schema_v3 = true;
            continue;
        }
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--suite" => suite = Some(PathBuf::from(value)),
            "--corpus" => corpus = Some(PathBuf::from(value)),
            "--expect-suite-sha256" => expected_suite_sha256 = Some(value),
            "--expect-corpus-sha256" => expected_corpus_sha256 = Some(value),
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
            "--min-project-claim-support-per-mille" => {
                project_claim_floor = value
                    .parse::<u16>()
                    .map_err(|_| "invalid project claim floor")?;
            }
            "--min-project-citation-precision-per-mille" => {
                project_citation_floor = value
                    .parse::<u16>()
                    .map_err(|_| "invalid project citation floor")?;
            }
            "--min-cases" => {
                min_cases = value
                    .parse::<usize>()
                    .map_err(|_| "invalid case count floor")?;
            }
            "--min-distinct-articles" => {
                min_distinct_articles = value
                    .parse::<usize>()
                    .map_err(|_| "invalid distinct article floor")?;
            }
            _ => return Err(format!("unknown option: {flag}")),
        }
    }
    if claim_floor > 1_000
        || citation_floor > 1_000
        || case_claim_floor > 1_000
        || case_citation_floor > 1_000
        || project_claim_floor > 1_000
        || project_citation_floor > 1_000
    {
        return Err("thresholds must be 0..=1000".into());
    }
    if min_cases > 1_000 || min_distinct_articles > 1_000 {
        return Err("coverage floors must be 0..=1000".into());
    }
    if expected_suite_sha256.is_some() != expected_corpus_sha256.is_some() {
        return Err("suite and corpus SHA-256 pins must be supplied together".into());
    }
    if expected_suite_sha256.is_some() && corpus.is_none() {
        return Err("SHA-256 pins require --corpus".into());
    }
    for digest in [&expected_suite_sha256, &expected_corpus_sha256]
        .into_iter()
        .flatten()
    {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("expected SHA-256 must be 64 lowercase hex characters".into());
        }
    }
    Ok(Args {
        corpus,
        require_schema_v3,
        expected_suite_sha256,
        expected_corpus_sha256,
        suite: suite.ok_or("missing --suite")?,
        output: output.ok_or("missing --output")?,
        min_claim_support_per_mille: claim_floor,
        min_citation_precision_per_mille: citation_floor,
        min_case_claim_support_per_mille: case_claim_floor,
        min_case_citation_precision_per_mille: case_citation_floor,
        min_project_claim_support_per_mille: project_claim_floor,
        min_project_citation_precision_per_mille: project_citation_floor,
        min_cases,
        min_distinct_articles,
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

/// Group all cases by canonical Wikimedia project so a large high-scoring
/// language cannot hide poor answer support in a smaller project. This grades
/// only caller-supplied human labels; it does not authenticate them.
fn per_project_gate(
    cases: &[wiki_search::answer_support::CaseSupportScore],
    floor: u16,
    counts: impl Fn(&wiki_search::answer_support::SupportCounts) -> (usize, usize),
) -> serde_json::Value {
    let mut projects = std::collections::BTreeMap::<&str, (usize, usize)>::new();
    for case in cases {
        let (numerator, denominator) = counts(&case.counts);
        let totals = projects.entry(case.project.as_str()).or_default();
        totals.0 += numerator;
        totals.1 += denominator;
    }
    let failed_projects: Vec<&str> = if floor == 0 {
        Vec::new()
    } else {
        projects
            .iter()
            .filter(|(_, (numerator, denominator))| !meets_floor(*numerator, *denominator, floor))
            .map(|(project, _)| *project)
            .collect()
    };
    let project_scores: Vec<serde_json::Value> = projects
        .iter()
        .map(|(project, (numerator, denominator))| {
            serde_json::json!({
                "project": project,
                "supported": numerator,
                "reviewed": denominator,
                "passed": floor == 0 || meets_floor(*numerator, *denominator, floor)
            })
        })
        .collect();
    serde_json::json!({
        "enabled": floor > 0,
        "min_per_mille": floor,
        "observed_projects": projects.len(),
        "project_scores": project_scores,
        "failed_projects": failed_projects,
        "passed": failed_projects.is_empty() && (floor == 0 || !projects.is_empty())
    })
}

/// An independently reviewed benchmark should span more than one question
/// and more than one article. Multiple revisions or questions for the same
/// project/page ID do not count as distinct articles. These are structural
/// coverage floors, not a representativeness or provenance certification.
fn coverage_gate(
    cases: &[wiki_search::answer_support::CaseSupportScore],
    min_cases: usize,
    min_distinct_articles: usize,
) -> serde_json::Value {
    let distinct_articles = cases
        .iter()
        .map(|case| (case.project.as_str(), case.page_id))
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    serde_json::json!({
        "min_cases": min_cases,
        "min_distinct_articles": min_distinct_articles,
        "observed_cases": cases.len(),
        "observed_distinct_articles": distinct_articles,
        "passed": cases.len() >= min_cases && distinct_articles >= min_distinct_articles
    })
}

fn run(args: Args) -> Result<bool, String> {
    validate_output(&args.suite, &args.output)?;
    let raw = read_suite(&args.suite)?;
    let suite_sha256 = format!("{:x}", Sha256::digest(raw.as_bytes()));
    if args
        .expected_suite_sha256
        .as_deref()
        .is_some_and(|expected| expected != suite_sha256)
    {
        return Err("reviewed suite SHA-256 differs from operator pin".into());
    }
    let (report, corpus_sha256) = if let Some(path) = &args.corpus {
        let (articles, digest) = read_corpus(path)?;
        if args
            .expected_corpus_sha256
            .as_deref()
            .is_some_and(|expected| expected != digest)
        {
            return Err("canonical corpus SHA-256 differs from operator pin".into());
        }
        let report = evaluate_answer_support_with_corpus_json(&raw, &articles)
            .map_err(|error| format!("invalid reviewed canonical evidence: {error:?}"))?;
        (report, Some(digest))
    } else {
        let report = evaluate_answer_support_json(&raw)
            .map_err(|error| format!("invalid human-reviewed support suite: {error:?}"))?;
        (report, None)
    };
    // A benchmark configured for complete visible-prose review must never
    // silently accept a legacy partial-annotation suite.
    if args.require_schema_v3 && report.annotation_schema_version != 3 {
        return Err("reviewed suite must use annotation schema v3".into());
    }
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
    let project_claim_gate = per_project_gate(
        &report.cases,
        args.min_project_claim_support_per_mille,
        |counts| (counts.supported_claims, counts.reviewed_claims),
    );
    let project_citation_gate = per_project_gate(
        &report.cases,
        args.min_project_citation_precision_per_mille,
        |counts| (counts.supported_citations, counts.reviewed_citations),
    );
    let coverage = coverage_gate(&report.cases, args.min_cases, args.min_distinct_articles);
    let passed = coverage["passed"] == true
        && case_claim_gate["passed"] == true
        && case_citation_gate["passed"] == true
        && project_claim_gate["passed"] == true
        && project_citation_gate["passed"] == true
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
        "complete_prose_coverage_required": args.require_schema_v3,
        "operator_input_pins_verified": args.expected_suite_sha256.is_some(),
        "gate": {
            "min_claim_support_per_mille": args.min_claim_support_per_mille,
            "min_citation_precision_per_mille": args.min_citation_precision_per_mille,
            "case_claim_support_gate": case_claim_gate,
            "case_citation_precision_gate": case_citation_gate,
            "project_claim_support_gate": project_claim_gate,
            "project_citation_precision_gate": project_citation_gate,
            "coverage_gate": coverage,
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
                require_schema_v3: false,
                expected_suite_sha256: None,
                expected_corpus_sha256: None,
                suite: "a.json".into(),
                output: "b.json".into(),
                min_claim_support_per_mille: 0,
                min_citation_precision_per_mille: 0,
                min_case_claim_support_per_mille: 0,
                min_case_citation_precision_per_mille: 0,
                min_project_claim_support_per_mille: 0,
                min_project_citation_precision_per_mille: 0,
                min_cases: 0,
                min_distinct_articles: 0,
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
        assert!(
            parse_args(values(&[
                "--suite",
                "x",
                "--output",
                "z",
                "--require-schema-v3"
            ]))
            .unwrap()
            .require_schema_v3
        );
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--require-schema-v3",
            "--require-schema-v3"
        ]))
        .is_err());
        let pin = "a".repeat(64);
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--corpus",
            "a.json",
            "--expect-suite-sha256",
            &pin,
        ]))
        .is_err());
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--expect-suite-sha256",
            &pin,
            "--expect-corpus-sha256",
            &pin,
        ]))
        .is_err());
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--corpus",
            "a.json",
            "--expect-suite-sha256",
            &pin,
            "--expect-corpus-sha256",
            &pin.to_uppercase(),
        ]))
        .is_err());
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--corpus",
            "a.json",
            "--expect-suite-sha256",
            &pin,
            "--expect-corpus-sha256",
            &pin,
        ]))
        .is_ok());
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--min-claim-support-per-mille",
            "1001"
        ]))
        .is_err());
        for (flag, value) in [
            ("--min-cases", "1001"),
            ("--min-distinct-articles", "-1"),
            ("--min-distinct-articles", "abc"),
        ] {
            assert!(parse_args(values(&["--suite", "x", "--output", "z", flag, value])).is_err());
        }
        for (flag, value) in [
            ("--min-project-claim-support-per-mille", "1001"),
            ("--min-project-citation-precision-per-mille", "invalid"),
        ] {
            assert!(parse_args(values(&["--suite", "x", "--output", "z", flag, value])).is_err());
        }
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--min-project-claim-support-per-mille",
            "500",
            "--min-project-claim-support-per-mille",
            "800",
        ]))
        .is_err());
        let coverage_args = parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--min-cases",
            "20",
            "--min-distinct-articles",
            "5",
        ]))
        .unwrap();
        assert_eq!(
            (coverage_args.min_cases, coverage_args.min_distinct_articles),
            (20, 5)
        );
        assert!(parse_args(values(&[
            "--suite",
            "x",
            "--output",
            "z",
            "--min-cases",
            "1",
            "--min-cases",
            "2",
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
        // Strong enwiki results cannot mask weak frwiki results when a
        // per-project gate is enabled, even if aggregate scores pass.
        let mut multilingual = suite.clone();
        multilingual["cases"][1]["project"] = serde_json::json!("frwiki");
        multilingual["cases"][1]["answer"] =
            serde_json::json!("Unsupported [[cite:wkb:frwiki:42:100:b0]]");
        multilingual["cases"][1]["claims"][0]["citations"][0]["block_id"] =
            serde_json::json!("wkb:frwiki:42:100:b0");
        let multilingual_report = evaluate_answer_support_json(&multilingual.to_string()).unwrap();
        let project_claim = per_project_gate(&multilingual_report.cases, 800, |counts| {
            (counts.supported_claims, counts.reviewed_claims)
        });
        let project_citation = per_project_gate(&multilingual_report.cases, 800, |counts| {
            (counts.supported_citations, counts.reviewed_citations)
        });
        assert_eq!(
            project_claim["failed_projects"],
            serde_json::json!(["frwiki"])
        );
        assert_eq!(
            project_citation["failed_projects"],
            serde_json::json!(["frwiki"])
        );
        assert_eq!(project_claim["observed_projects"], 2);
        assert_eq!(project_claim["project_scores"][0]["project"], "enwiki");
        assert_eq!(project_claim["project_scores"][0]["passed"], true);
        assert_eq!(project_claim["project_scores"][1]["project"], "frwiki");
        assert_eq!(project_claim["project_scores"][1]["passed"], false);
        assert_eq!(
            per_project_gate(&multilingual_report.cases, 500, |counts| {
                (counts.supported_claims, counts.reviewed_claims)
            })["passed"],
            false
        );
        assert_eq!(
            per_project_gate(&multilingual_report.cases, 0, |_| (0, 0))["passed"],
            true
        );
        assert_eq!(per_project_gate(&[], 800, |_| (0, 0))["passed"], false);
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
            "schema_version": 3,
            "suite_id": "strict_test",
            "cases": [{
                "id": "c1", "project": "enwiki", "page_id": 42, "revision_id": 100,
                "question": "How many moons?",
                "answer": "Two moons [[cite:wkb:enwiki:42:100:b0]]",
                "claims": [{
                    "id": "claim", "text": "Two moons", "verdict": "supported",
                    "start_byte": 0, "end_byte": 9,
                    "citations": [{"block_id": "wkb:enwiki:42:100:b0", "verdict": "supported"}]
                }]
            }]
        });
        fs::write(&suite, labels.to_string()).unwrap();
        fs::write(&corpus, serde_json::to_vec(&vec![article.clone()]).unwrap()).unwrap();
        let args = Args {
            corpus: Some(corpus.clone()),
            require_schema_v3: true,
            expected_suite_sha256: None,
            expected_corpus_sha256: None,
            suite: suite.clone(),
            output: output.clone(),
            min_claim_support_per_mille: 1_000,
            min_citation_precision_per_mille: 1_000,
            min_case_claim_support_per_mille: 1_000,
            min_case_citation_precision_per_mille: 1_000,
            min_project_claim_support_per_mille: 0,
            min_project_citation_precision_per_mille: 0,
            min_cases: 0,
            min_distinct_articles: 0,
        };
        assert_eq!(run(args.clone()), Ok(true));
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(report["evidence_membership_checked"], true);
        assert_eq!(report["complete_prose_coverage_required"], true);
        assert_eq!(report["evaluation"]["annotation_schema_version"], 3);
        assert_eq!(report["gate"]["passed"], true);
        assert_eq!(report["gate"]["case_claim_support_gate"]["passed"], true);
        assert_eq!(
            report["gate"]["case_citation_precision_gate"]["passed"],
            true
        );
        assert_eq!(report["corpus_input_sha256"].as_str().unwrap().len(), 64);
        assert_eq!(report["operator_input_pins_verified"], false);
        assert_eq!(report["gate"]["coverage_gate"]["observed_cases"], 1);
        assert_eq!(
            report["gate"]["coverage_gate"]["observed_distinct_articles"],
            1
        );

        let coverage_output = root.join("coverage-fail.json");
        assert_eq!(
            run(Args {
                output: coverage_output.clone(),
                min_cases: 2,
                min_distinct_articles: 2,
                ..args.clone()
            }),
            Ok(false)
        );
        let coverage_report: serde_json::Value =
            serde_json::from_slice(&fs::read(&coverage_output).unwrap()).unwrap();
        assert_eq!(coverage_report["gate"]["coverage_gate"]["passed"], false);
        assert_eq!(
            coverage_report["gate"]["coverage_gate"]["observed_cases"],
            1
        );
        assert_eq!(
            coverage_report["gate"]["coverage_gate"]["observed_distinct_articles"],
            1
        );
        assert_eq!(coverage_report["gate"]["passed"], false);

        let pinned = Args {
            expected_suite_sha256: Some(format!("{:x}", Sha256::digest(fs::read(&suite).unwrap()))),
            expected_corpus_sha256: Some(format!(
                "{:x}",
                Sha256::digest(fs::read(&corpus).unwrap())
            )),
            ..args.clone()
        };
        let pinned_output = root.join("pinned.json");
        assert_eq!(
            run(Args {
                output: pinned_output.clone(),
                ..pinned.clone()
            }),
            Ok(true)
        );
        let pinned_report: serde_json::Value =
            serde_json::from_slice(&fs::read(&pinned_output).unwrap()).unwrap();
        assert_eq!(pinned_report["operator_input_pins_verified"], true);
        let wrong_corpus_output = root.join("wrong-corpus.json");
        assert!(run(Args {
            output: wrong_corpus_output.clone(),
            expected_corpus_sha256: Some("0".repeat(64)),
            ..pinned.clone()
        })
        .is_err());
        assert!(!wrong_corpus_output.exists());
        fs::write(&suite, format!("{}\n", labels)).unwrap();
        let changed_suite_output = root.join("changed-suite.json");
        assert!(run(Args {
            output: changed_suite_output.clone(),
            ..pinned
        })
        .is_err());
        assert!(!changed_suite_output.exists());
        fs::write(&suite, labels.to_string()).unwrap();

        // Schema v3 must reject an unannotated assertion in the executable
        // strict-corpus path, even when the original reviewed claim and its
        // canonical citation remain valid. Invalid suites publish no report.
        let mut unreviewed = labels.clone();
        unreviewed["cases"][0]["answer"] = serde_json::json!(
            "Two moons [[cite:wkb:enwiki:42:100:b0]] Additional unsupported assertion"
        );
        fs::write(&suite, unreviewed.to_string()).unwrap();
        let uncovered_output = root.join("unreviewed-prose.json");
        assert!(run(Args {
            output: uncovered_output.clone(),
            ..args.clone()
        })
        .is_err());
        assert!(!uncovered_output.exists());
        fs::write(&suite, labels.to_string()).unwrap();

        // An otherwise valid legacy suite may omit unreviewed prose. A strict
        // operator must not accidentally downgrade the benchmark to v2.
        let mut downgraded = labels.clone();
        downgraded["schema_version"] = serde_json::json!(2);
        downgraded["cases"][0]["answer"] =
            serde_json::json!("Two moons [[cite:wkb:enwiki:42:100:b0]] Unreviewed extra assertion");
        fs::write(&suite, downgraded.to_string()).unwrap();
        let downgraded_output = root.join("downgraded.json");
        assert!(run(Args {
            output: downgraded_output.clone(),
            ..args.clone()
        })
        .is_err());
        assert!(!downgraded_output.exists());
        fs::write(&suite, labels.to_string()).unwrap();

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
                min_project_claim_support_per_mille: 800,
                min_project_citation_precision_per_mille: 800,
                ..args.clone()
            }),
            Ok(false)
        );
        let weak_report: serde_json::Value =
            serde_json::from_slice(&fs::read(&weak_output).unwrap()).unwrap();
        assert_eq!(weak_report["gate"]["passed"], false);
        assert_eq!(
            weak_report["gate"]["project_claim_support_gate"]["failed_projects"],
            serde_json::json!(["enwiki"])
        );
        assert_eq!(
            weak_report["gate"]["project_citation_precision_gate"]["failed_projects"],
            serde_json::json!(["enwiki"])
        );
        assert_eq!(weak_report["gate"]["coverage_gate"]["observed_cases"], 2);
        assert_eq!(
            weak_report["gate"]["coverage_gate"]["observed_distinct_articles"],
            1
        );
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
