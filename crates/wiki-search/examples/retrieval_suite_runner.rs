//! Offline retrieval benchmark runner for a caller-supplied canonical article corpus.
//!
//! Usage:
//! cargo run -p wiki-search --example retrieval_suite_runner --locked -- \\
//!   --pack corpus.wpack --suite labels.json --sha <40-hex-commit> \\
//!   --output report.json [--top-k 5] [--min-recall-per-mille 800] \\
//!   [--max-no-answer-fp 0] [--min-case-recall-per-mille 0] \\
//!   [--min-case-precision-per-mille 0] [--max-first-relevant-rank 0] \\
//!   [--expect-articles-sha256 <64-hex>] [--expect-suite-sha256 <64-hex>]
//!
//! Use --corpus articles.json instead of --pack for unpacked input.
//! Use --snapshot-root PATH for catalog.sqlite and MessagePack/zstd shard frames.
//! A provisional pack must contain an articles.json canonical article array.
//! This does not authenticate the source or labels. It must not be described as
//! a publisher-verified Wikimedia benchmark without independent provenance.

use std::{
    env, fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use wiki_model::{Article, ArticleKey, PageRecord};
use wiki_pack_format::container::verify_container;
use wiki_search::{
    evaluation::RetrievalThresholds, evaluation_suite::run_retrieval_suite_json, CorpusLexicalIndex,
};
use wiki_store::catalog::SnapshotCatalog;

const MAX_CORPUS_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARTICLES: usize = 20_000;
const MAX_SUITE_BYTES: u64 = 2 * 1024 * 1024;
static REPORT_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    corpus: Option<PathBuf>,
    pack: Option<PathBuf>,
    snapshot_root: Option<PathBuf>,
    suite: PathBuf,
    output: PathBuf,
    sha: String,
    top_k: usize,
    min_recall_per_mille: u16,
    max_no_answer_fp: usize,
    min_case_recall_per_mille: u16,
    min_case_precision_per_mille: u16,
    max_first_relevant_rank: usize,
    expected_articles_sha256: Option<String>,
    expected_suite_sha256: Option<String>,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut corpus = None;
    let mut pack = None;
    let mut snapshot_root = None;
    let mut suite = None;
    let mut output = None;
    let mut sha = None;
    let mut top_k = 5;
    let mut min_recall_per_mille = 800;
    let mut max_no_answer_fp = 0;
    let mut min_case_recall_per_mille = 0;
    let mut min_case_precision_per_mille = 0;
    let mut max_first_relevant_rank = 0;
    let mut expected_articles_sha256 = None;
    let mut expected_suite_sha256 = None;
    let mut args = args.into_iter();
    let mut seen_flags = std::collections::BTreeSet::new();
    while let Some(flag) = args.next() {
        if !seen_flags.insert(flag.clone()) {
            return Err(format!("duplicate argument: {flag}"));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--corpus" if corpus.is_none() => corpus = Some(PathBuf::from(value)),
            "--pack" if pack.is_none() => pack = Some(PathBuf::from(value)),
            "--snapshot-root" if snapshot_root.is_none() => {
                snapshot_root = Some(PathBuf::from(value))
            }
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
            "--min-case-recall-per-mille" => {
                min_case_recall_per_mille = value
                    .parse()
                    .map_err(|_| "invalid per-case recall threshold".to_owned())?;
            }
            "--min-case-precision-per-mille" => {
                min_case_precision_per_mille = value
                    .parse()
                    .map_err(|_| "invalid per-case precision threshold".to_owned())?;
            }
            "--max-first-relevant-rank" => {
                max_first_relevant_rank = value
                    .parse()
                    .map_err(|_| "invalid first relevant rank limit".to_owned())?;
            }
            "--expect-articles-sha256" => expected_articles_sha256 = Some(value),
            "--expect-suite-sha256" => expected_suite_sha256 = Some(value),
            _ => return Err(format!("unknown or duplicate argument: {flag}")),
        }
    }
    if !(1..=100).contains(&top_k)
        || min_recall_per_mille > 1_000
        || min_case_recall_per_mille > 1_000
        || min_case_precision_per_mille > 1_000
        || max_first_relevant_rank > 100
    {
        return Err("top-k must be 1..=100, recall/precision thresholds 0..=1000, and first relevant rank 0..=100".into());
    }
    let sha = sha.ok_or("missing --sha")?;
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("--sha must be exactly 40 hexadecimal characters".into());
    }
    if usize::from(corpus.is_some())
        + usize::from(pack.is_some())
        + usize::from(snapshot_root.is_some())
        != 1
    {
        return Err("specify exactly one of --corpus, --pack or --snapshot-root".into());
    }
    // Both content identities must be pinned together; pinning only labels
    // or only articles leaves the scored input pair ambiguous.
    if expected_articles_sha256.is_some() != expected_suite_sha256.is_some() {
        return Err(
            "both --expect-articles-sha256 and --expect-suite-sha256 are required together".into(),
        );
    }
    let normalize_digest = |value: String| -> Result<String, String> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("expected SHA-256 must be exactly 64 hexadecimal characters".into());
        }
        Ok(value.to_ascii_lowercase())
    };
    let expected_articles_sha256 = expected_articles_sha256.map(normalize_digest).transpose()?;
    let expected_suite_sha256 = expected_suite_sha256.map(normalize_digest).transpose()?;
    Ok(Args {
        corpus,
        pack,
        snapshot_root,
        suite: suite.ok_or("missing --suite")?,
        output: output.ok_or("missing --output")?,
        sha,
        top_k,
        min_recall_per_mille,
        max_no_answer_fp,
        min_case_recall_per_mille,
        min_case_precision_per_mille,
        max_first_relevant_rank,
        expected_articles_sha256,
        expected_suite_sha256,
    })
}

// Open once: checking a pathname and then reopening it permits substitution.
// A bounded reader also caps FIFOs and files that grow after the metadata
// check, rather than allocating the entire replacement before rejecting it.
fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let file =
        fs::File::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let size = file
        .metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?
        .len();
    if size > max {
        return Err(format!("{} exceeds {max} bytes", path.display()));
    }
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if bytes.len() as u64 > max {
        return Err(format!("{} exceeds {max} bytes", path.display()));
    }
    Ok(bytes)
}

/// Read a real immutable SQLite catalog and independent MessagePack/zstd
/// shard frames without copying the entire snapshot or modifying its files.
/// The catalog's complete frame coverage/title-index audit is mandatory;
/// byte integrity is not publisher authenticity or required-media completeness.
fn read_snapshot_corpus(root: &Path) -> Result<Vec<Article>, String> {
    let catalog_path = root.join("catalog.sqlite");
    let catalog = SnapshotCatalog::open_read_only(&catalog_path)
        .map_err(|error| format!("cannot open snapshot catalog: {error:?}"))?;
    catalog
        .verify_shard_coverage(root)
        .map_err(|error| format!("snapshot frame coverage audit failed: {error:?}"))?;
    let mut articles = Vec::new();
    let mut too_many = false;
    catalog
        .visit_records(root, |record| {
            if let PageRecord::Article(article) = record {
                if articles.len() == MAX_ARTICLES {
                    too_many = true;
                    return Err(wiki_store::catalog::CatalogError::InvalidCatalogEntry);
                }
                articles.push(*article);
            }
            Ok(())
        })
        .map_err(|error| {
            if too_many {
                format!("snapshot exceeds {MAX_ARTICLES} benchmark articles")
            } else {
                format!("cannot decode verified snapshot frame: {error:?}")
            }
        })?;
    if articles.is_empty() {
        return Err("snapshot has no articles to benchmark".into());
    }
    Ok(articles)
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

// Benchmark reports must not overwrite the evidence or the labeled suite.
// Canonical paths catch equivalent spellings and symlinked parent directories.
// Refuse output symlinks, including dangling ones, rather than following them.
fn validate_output_destination(args: &Args) -> Result<(), String> {
    // A report is a new evidence artifact, never an overwrite. This also
    // refuses dangling symlinks and eliminates the existing-file clobber path.
    if fs::symlink_metadata(&args.output).is_ok() {
        return Err("report output already exists (or is a symlink)".into());
    }
    let output = match fs::canonicalize(&args.output) {
        Ok(path) => path,
        Err(_) => {
            let parent = args
                .output
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let filename = args
                .output
                .file_name()
                .ok_or("invalid report output path")?;
            fs::canonicalize(parent)
                .map_err(|error| format!("cannot resolve report directory: {error}"))?
                .join(filename)
        }
    };
    let corpus = args
        .corpus
        .as_ref()
        .or(args.pack.as_ref())
        .or(args.snapshot_root.as_ref())
        .ok_or("missing benchmark corpus")?;
    for input in [corpus, &args.suite] {
        let resolved = fs::canonicalize(input)
            .map_err(|error| format!("cannot resolve {}: {error}", input.display()))?;
        if output == resolved || (args.snapshot_root.is_some() && output.starts_with(&resolved)) {
            return Err(format!(
                "report output must not overwrite input or modify snapshot {}",
                input.display()
            ));
        }
    }
    Ok(())
}

/// Publish a complete report without ever truncating an existing path.
/// The temporary file is written and synced in the destination directory;
/// hard-link publication is atomic and fails if another process created the
/// destination after preflight (including a symlink to an input file).
fn persist_new_report(output: &Path, content: &str) -> Result<(), String> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = output.file_name().ok_or("invalid report filename")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock before Unix epoch")?
        .as_nanos();
    let sequence = REPORT_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
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
    // A failed write or conflicting output must not leave a partial artifact.
    let _ = fs::remove_file(&temporary);
    published.map_err(|error| format!("cannot publish new report {}: {error}", output.display()))
}

// Hash the exact canonical article objects that were indexed, not a mutable
// pathname or an unauthenticated upstream identity. Sort by stable page and
// revision identity so equivalent input orderings yield the same receipt.
// Stream the serialization into SHA-256 to avoid a second corpus-sized buffer.
struct DigestWriter<'a>(&'a mut Sha256);

impl Write for DigestWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn canonical_articles_sha256(articles: &[Article]) -> Result<String, String> {
    let mut ordered = articles.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.key
            .project
            .cmp(&right.key.project)
            .then(left.key.page_id.cmp(&right.key.page_id))
            .then(left.revision.revision_id.cmp(&right.revision.revision_id))
    });
    let mut hasher = Sha256::new();
    serde_json::to_writer(DigestWriter(&mut hasher), &ordered)
        .map_err(|error| format!("cannot hash canonical benchmark articles: {error}"))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// The library's stable schema-v2 score report remains unchanged. The CLI
/// publishes schema v3 with input-byte receipts so a future reviewer can
/// detect corpus or label substitutions even when the claimed SHA is identical.
/// Digests identify content, not the authority or correctness of its source.
fn with_input_receipts_with_rank(
    score_json: &str,
    articles: &[Article],
    suite_bytes: &[u8],
    corpus_kind: &str,
    min_case_recall_per_mille: u16,
    min_case_precision_per_mille: u16,
    max_first_relevant_rank: usize,
) -> Result<String, String> {
    if min_case_recall_per_mille > 1_000 || min_case_precision_per_mille > 1_000 {
        return Err("per-case recall or precision threshold exceeds 1000".into());
    }
    if max_first_relevant_rank > 100 {
        return Err("first relevant rank limit exceeds 100".into());
    }
    let mut report: serde_json::Value = serde_json::from_str(score_json)
        .map_err(|error| format!("invalid retrieval score report: {error}"))?;
    let fields = report
        .as_object_mut()
        .ok_or("retrieval score report must be an object")?;
    let baseline_passed = fields
        .get("passed")
        .and_then(serde_json::Value::as_bool)
        .ok_or("retrieval score report has no gate decision")?;
    let cases = fields
        .get("retrieval")
        .and_then(|retrieval| retrieval.get("cases"))
        .and_then(serde_json::Value::as_array)
        .ok_or("retrieval score report has no scored cases")?;
    let mut answerable_cases = 0usize;
    let mut missed_case_ids = Vec::new();
    let mut low_precision_case_ids = Vec::new();
    let mut late_first_relevant_case_ids = Vec::new();
    let mut observed_first_relevant_ranks = Vec::new();
    for case in cases {
        let id = case
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or("scored case has no ID")?;
        let relevant = case
            .get("relevant_count")
            .and_then(serde_json::Value::as_u64)
            .ok_or("scored case has no relevant count")?;
        let found = case
            .get("found_count")
            .and_then(serde_json::Value::as_u64)
            .ok_or("scored case has no found count")?;
        let retrieved = case
            .get("retrieved_count")
            .and_then(serde_json::Value::as_u64)
            .ok_or("scored case has no retrieved count")?;
        if found > relevant || found > retrieved {
            return Err("scored case has impossible hit counts".into());
        }
        if relevant > 0 {
            answerable_cases += 1;
            if (found as u128) * 1_000 < (min_case_recall_per_mille as u128) * (relevant as u128) {
                missed_case_ids.push(id.to_owned());
            }
            // An answerable question with no hits has undefined precision;
            // it cannot satisfy a positive precision floor. No-answer cases
            // are governed by the separate false-positive gate.
            if min_case_precision_per_mille > 0
                && (retrieved == 0
                    || (found as u128) * 1_000
                        < (min_case_precision_per_mille as u128) * (retrieved as u128))
            {
                low_precision_case_ids.push(id.to_owned());
            }
            if max_first_relevant_rank > 0 {
                // The shared evaluator emits rank-ordered, gold-labeled hits.
                // Validate its receipt before gating: an absent or inconsistent
                // ranking must never be interpreted as an early gold hit.
                let ranked = case
                    .get("ranked_evidence")
                    .and_then(serde_json::Value::as_array)
                    .ok_or("scored case has no ranked evidence")?;
                if ranked.len() as u64 != retrieved {
                    return Err("scored case has inconsistent ranked evidence count".into());
                }
                let mut first_rank = None;
                let mut labeled_count = 0u64;
                for (position, hit) in ranked.iter().enumerate() {
                    if hit.get("rank").and_then(serde_json::Value::as_u64)
                        != Some((position + 1) as u64)
                    {
                        return Err("scored case has noncontiguous evidence ranks".into());
                    }
                    let labeled = hit
                        .get("labeled_relevant")
                        .and_then(serde_json::Value::as_bool)
                        .ok_or("scored hit has no relevance label")?;
                    if labeled {
                        labeled_count += 1;
                        first_rank.get_or_insert(position + 1);
                    }
                }
                if labeled_count != found {
                    return Err("scored case has inconsistent relevance labels".into());
                }
                if first_rank.is_none_or(|rank| rank > max_first_relevant_rank) {
                    late_first_relevant_case_ids.push(id.to_owned());
                }
                observed_first_relevant_ranks.push(serde_json::json!({
                    "case_id": id,
                    "first_relevant_rank": first_rank
                }));
            }
        }
    }
    // A nonzero minimum cannot be satisfied vacuously by a suite containing
    // only no-answer cases. Zero retains the existing aggregate-only policy.
    let case_gate_passed =
        min_case_recall_per_mille == 0 || (answerable_cases > 0 && missed_case_ids.is_empty());
    let precision_gate_passed = min_case_precision_per_mille == 0
        || (answerable_cases > 0 && low_precision_case_ids.is_empty());
    let first_rank_gate_passed = max_first_relevant_rank == 0
        || (answerable_cases > 0 && late_first_relevant_case_ids.is_empty());
    fields.insert("schema_version".into(), serde_json::json!(3));
    fields.insert(
        "input_receipts".into(),
        serde_json::json!({
            "canonical_articles_json_sha256": canonical_articles_sha256(articles)?,
            "label_suite_bytes_sha256": wiki_pack_format::sha256_hex(suite_bytes),
            "canonical_article_count": articles.len(),
            "corpus_source_kind": corpus_kind,
            "source_authentication": "caller_supplied_unverified"
        }),
    );
    fields.insert(
        "case_recall_gate".into(),
        serde_json::json!({
            "min_case_recall_per_mille": min_case_recall_per_mille,
            "answerable_cases": answerable_cases,
            "below_threshold_case_ids": missed_case_ids,
            "passed": case_gate_passed
        }),
    );
    fields.insert(
        "case_precision_gate".into(),
        serde_json::json!({
            "min_case_precision_per_mille": min_case_precision_per_mille,
            "answerable_cases": answerable_cases,
            "below_threshold_case_ids": low_precision_case_ids,
            "passed": precision_gate_passed
        }),
    );
    fields.insert(
        "first_relevant_rank_gate".into(),
        serde_json::json!({
            "max_first_relevant_rank": max_first_relevant_rank,
            "answerable_cases": answerable_cases,
            "below_threshold_case_ids": late_first_relevant_case_ids,
            "observed_first_relevant_ranks": observed_first_relevant_ranks,
            "passed": first_rank_gate_passed
        }),
    );
    fields.insert(
        "passed".into(),
        serde_json::json!(
            baseline_passed && case_gate_passed && precision_gate_passed && first_rank_gate_passed
        ),
    );
    let mut output = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("cannot serialize benchmark receipts: {error}"))?;
    output.push('\n');
    Ok(output)
}

#[cfg(test)]
fn with_input_receipts(
    score_json: &str,
    articles: &[Article],
    suite_bytes: &[u8],
    corpus_kind: &str,
    min_case_recall_per_mille: u16,
    min_case_precision_per_mille: u16,
) -> Result<String, String> {
    with_input_receipts_with_rank(
        score_json,
        articles,
        suite_bytes,
        corpus_kind,
        min_case_recall_per_mille,
        min_case_precision_per_mille,
        0,
    )
}

/// Operator-pinned digests are compared with the exact canonical objects and
/// label bytes used for scoring, before any report is published. These pins
/// prevent accidental input substitution, not forged labels or source claims.
fn verify_input_pins(args: &Args, articles: &[Article], suite_bytes: &[u8]) -> Result<(), String> {
    if let (Some(expected_articles), Some(expected_suite)) =
        (&args.expected_articles_sha256, &args.expected_suite_sha256)
    {
        if canonical_articles_sha256(articles)? != *expected_articles {
            return Err("canonical articles SHA-256 differs from the operator pin".into());
        }
        if wiki_pack_format::sha256_hex(suite_bytes) != *expected_suite {
            return Err("label suite bytes SHA-256 differs from the operator pin".into());
        }
    }
    Ok(())
}

fn run(args: Args) -> Result<(), String> {
    validate_output_destination(&args)?;
    let (articles, pack_project) = if let Some(root) = &args.snapshot_root {
        (read_snapshot_corpus(root)?, None)
    } else {
        let (corpus_bytes, pack_project) = if let Some(path) = &args.corpus {
            (read_bounded(path, MAX_CORPUS_BYTES)?, None)
        } else {
            let path = args.pack.as_ref().ok_or("missing --pack")?;
            let mut input = fs::File::open(path)
                .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
            let (bytes, project) = read_pack_corpus(&mut input)?;
            (bytes, Some(project))
        };
        let articles = serde_json::from_slice(&corpus_bytes)
            .map_err(|error| format!("invalid canonical article corpus: {error}"))?;
        (articles, pack_project)
    };
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
    verify_input_pins(&args, &articles, &suite_bytes)?;
    let thresholds = RetrievalThresholds {
        min_recall_per_mille: args.min_recall_per_mille,
        max_no_answer_false_positive_cases: args.max_no_answer_fp,
    };
    let result = run_retrieval_suite_json(&index, suite, args.top_k, &args.sha, &thresholds)
        .map_err(|error| format!("retrieval suite rejected: {error:?}"))?;
    let corpus_kind = if args.snapshot_root.is_some() {
        "local_snapshot_frames"
    } else if args.pack.is_some() {
        "pack_articles_object"
    } else {
        "json_article_array"
    };
    let report = with_input_receipts_with_rank(
        &result.report_json,
        &articles,
        &suite_bytes,
        corpus_kind,
        args.min_case_recall_per_mille,
        args.min_case_precision_per_mille,
        args.max_first_relevant_rank,
    )?;
    persist_new_report(&args.output, &report)?;
    let report_passed = serde_json::from_str::<serde_json::Value>(&report)
        .map_err(|error| format!("cannot read scored report: {error}"))?
        .get("passed")
        .and_then(serde_json::Value::as_bool)
        .ok_or("scored report missing gate decision")?;
    if !report_passed {
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
    fn bounded_input_reader_accepts_exact_limit_and_rejects_overflow() {
        let dir = std::env::temp_dir().join(format!(
            "wiki-retrieval-bounded-read-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("suite.json");
        fs::write(&input, b"12345678").unwrap();
        assert_eq!(read_bounded(&input, 8).unwrap(), b"12345678");
        assert!(read_bounded(&input, 7)
            .unwrap_err()
            .contains("exceeds 7 bytes"));
        fs::write(&input, b"").unwrap();
        assert_eq!(read_bounded(&input, 0).unwrap(), b"");
        fs::write(&input, b"x").unwrap();
        assert!(read_bounded(&input, 0).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn input_pins_require_both_valid_sha256_values() {
        let mut args = arguments();
        args.extend(["--expect-articles-sha256".into(), "a".repeat(64)]);
        assert!(parse_args(args).is_err());
        let mut args = arguments();
        args.extend(["--expect-suite-sha256".into(), "b".repeat(64)]);
        assert!(parse_args(args).is_err());
        for invalid in ["a".repeat(63), "z".repeat(64), "".into()] {
            let mut args = arguments();
            args.extend([
                "--expect-articles-sha256".into(),
                invalid,
                "--expect-suite-sha256".into(),
                "b".repeat(64),
            ]);
            assert!(parse_args(args).is_err());
        }
        let mut args = arguments();
        args.extend([
            "--expect-articles-sha256".into(),
            "A".repeat(64),
            "--expect-suite-sha256".into(),
            "B".repeat(64),
        ]);
        let parsed = parse_args(args).unwrap();
        assert_eq!(
            parsed.expected_articles_sha256.as_deref(),
            Some("a".repeat(64).as_str())
        );
        assert_eq!(
            parsed.expected_suite_sha256.as_deref(),
            Some("b".repeat(64).as_str())
        );
    }

    #[test]
    fn operator_pins_bind_canonical_articles_and_exact_label_bytes() {
        let mut args = parse_args(arguments()).unwrap();
        args.expected_articles_sha256 = Some(canonical_articles_sha256(&[]).unwrap());
        args.expected_suite_sha256 = Some(wiki_pack_format::sha256_hex(b"{}"));
        verify_input_pins(&args, &[], b"{}").unwrap();
        assert!(verify_input_pins(&args, &[], b"{ }")
            .unwrap_err()
            .contains("label suite bytes"));
        args.expected_articles_sha256 = Some("0".repeat(64));
        assert!(verify_input_pins(&args, &[], b"{}")
            .unwrap_err()
            .contains("canonical articles"));
    }

    #[test]
    fn report_receipts_bind_canonical_articles_and_exact_label_bytes() {
        let score = r#"{"schema_version":2,"passed":true,"retrieval":{"cases":[{"id":"a","relevant_count":1,"found_count":1,"retrieved_count":1}]}}"#;
        let first = with_input_receipts(score, &[], b"{}", "json_article_array", 0, 0).unwrap();
        let repeat = with_input_receipts(score, &[], b"{}", "json_article_array", 0, 0).unwrap();
        let changed = with_input_receipts(score, &[], b"{ }", "json_article_array", 0, 0).unwrap();
        assert_eq!(first, repeat);
        let first: serde_json::Value = serde_json::from_str(&first).unwrap();
        let changed: serde_json::Value = serde_json::from_str(&changed).unwrap();
        assert_eq!(first["schema_version"], 3);
        assert_eq!(first["passed"], true);
        assert_eq!(first["input_receipts"]["canonical_article_count"], 0);
        assert_eq!(
            first["input_receipts"]["canonical_articles_json_sha256"],
            wiki_pack_format::sha256_hex(b"[]")
        );
        assert_eq!(
            first["input_receipts"]["label_suite_bytes_sha256"],
            wiki_pack_format::sha256_hex(b"{}")
        );
        assert_ne!(
            first["input_receipts"]["label_suite_bytes_sha256"],
            changed["input_receipts"]["label_suite_bytes_sha256"]
        );
        assert_eq!(
            first["input_receipts"]["source_authentication"],
            "caller_supplied_unverified"
        );
    }

    #[test]
    fn per_case_recall_gate_exposes_failures_hidden_by_aggregate_recall() {
        // Aggregate recall is 2/3, so a 600-per-mille micro gate passes.
        // One answerable question still retrieves none of its labeled gold.
        let score = r#"{"schema_version":2,"passed":true,"retrieval":{"cases":[
            {"id":"answer-a","relevant_count":2,"found_count":2,"retrieved_count":2},
            {"id":"answer-b","relevant_count":1,"found_count":0,"retrieved_count":0},
            {"id":"no-answer","relevant_count":0,"found_count":0,"retrieved_count":0}
        ]}}"#;
        let strict = with_input_receipts(score, &[], b"{}", "json_article_array", 500, 0).unwrap();
        let strict: serde_json::Value = serde_json::from_str(&strict).unwrap();
        assert_eq!(strict["passed"], false);
        assert_eq!(strict["case_recall_gate"]["passed"], false);
        assert_eq!(strict["case_recall_gate"]["answerable_cases"], 2);
        assert_eq!(
            strict["case_recall_gate"]["below_threshold_case_ids"],
            serde_json::json!(["answer-b"])
        );

        let legacy = with_input_receipts(score, &[], b"{}", "json_article_array", 0, 0).unwrap();
        let legacy: serde_json::Value = serde_json::from_str(&legacy).unwrap();
        assert_eq!(legacy["passed"], true);
        assert_eq!(legacy["case_recall_gate"]["passed"], true);
        let no_answer_only = r#"{"passed":true,"retrieval":{"cases":[
            {"id":"negative","relevant_count":0,"found_count":0,"retrieved_count":0}
        ]}}"#;
        let empty =
            with_input_receipts(no_answer_only, &[], b"{}", "json_article_array", 1, 0).unwrap();
        let empty: serde_json::Value = serde_json::from_str(&empty).unwrap();
        assert_eq!(empty["passed"], false);
        assert_eq!(
            empty["case_recall_gate"]["below_threshold_case_ids"],
            serde_json::json!([])
        );
        assert!(with_input_receipts(score, &[], b"{}", "json_article_array", 1001, 0).is_err());
    }

    #[test]
    fn per_case_precision_gate_rejects_noisy_and_missing_evidence() {
        // A micro precision score can hide one noisy question. A positive
        // per-case floor must also reject answerable questions with no hits.
        let score = r#"{"passed":true,"retrieval":{"cases":[
            {"id":"noisy","relevant_count":1,"found_count":1,"retrieved_count":3},
            {"id":"clean","relevant_count":1,"found_count":1,"retrieved_count":1},
            {"id":"missing","relevant_count":1,"found_count":0,"retrieved_count":0},
            {"id":"negative","relevant_count":0,"found_count":0,"retrieved_count":1}
        ]}}"#;
        let strict = with_input_receipts(score, &[], b"{}", "json_article_array", 0, 500).unwrap();
        let strict: serde_json::Value = serde_json::from_str(&strict).unwrap();
        assert_eq!(strict["passed"], false);
        assert_eq!(strict["case_recall_gate"]["passed"], true);
        assert_eq!(strict["case_precision_gate"]["passed"], false);
        assert_eq!(strict["case_precision_gate"]["answerable_cases"], 3);
        assert_eq!(
            strict["case_precision_gate"]["below_threshold_case_ids"],
            serde_json::json!(["noisy", "missing"])
        );
        let legacy = with_input_receipts(score, &[], b"{}", "json_article_array", 0, 0).unwrap();
        let legacy: serde_json::Value = serde_json::from_str(&legacy).unwrap();
        assert_eq!(legacy["passed"], true);

        let no_answer = r#"{"passed":true,"retrieval":{"cases":[
            {"id":"negative","relevant_count":0,"found_count":0,"retrieved_count":0}
        ]}}"#;
        let empty = with_input_receipts(no_answer, &[], b"{}", "json_article_array", 0, 1).unwrap();
        let empty: serde_json::Value = serde_json::from_str(&empty).unwrap();
        assert_eq!(empty["case_precision_gate"]["passed"], false);
        assert!(with_input_receipts(score, &[], b"{}", "json_article_array", 0, 1001).is_err());
        let impossible = r#"{"passed":true,"retrieval":{"cases":[
            {"id":"bad","relevant_count":1,"found_count":1,"retrieved_count":0}
        ]}}"#;
        assert!(with_input_receipts(impossible, &[], b"{}", "json_article_array", 0, 0).is_err());
    }

    #[test]
    fn first_relevant_rank_gate_rejects_late_gold_and_invalid_receipts() {
        // Recall and precision can both pass while the first relevant
        // passage appears too late for a short evidence context window.
        let score = r#"{"passed":true,"retrieval":{"cases":[
            {"id":"late","relevant_count":1,"found_count":1,"retrieved_count":3,
             "ranked_evidence":[{"rank":1,"labeled_relevant":false},
                                {"rank":2,"labeled_relevant":false},
                                {"rank":3,"labeled_relevant":true}]},
            {"id":"early","relevant_count":1,"found_count":1,"retrieved_count":1,
             "ranked_evidence":[{"rank":1,"labeled_relevant":true}]},
            {"id":"absent","relevant_count":1,"found_count":0,"retrieved_count":0,
             "ranked_evidence":[]},
            {"id":"no-answer","relevant_count":0,"found_count":0,"retrieved_count":0,
             "ranked_evidence":[]}
        ]}}"#;
        let strict =
            with_input_receipts_with_rank(score, &[], b"{}", "json_article_array", 0, 0, 2)
                .unwrap();
        let strict: serde_json::Value = serde_json::from_str(&strict).unwrap();
        assert_eq!(strict["passed"], false);
        assert_eq!(strict["case_recall_gate"]["passed"], true);
        assert_eq!(strict["case_precision_gate"]["passed"], true);
        assert_eq!(strict["first_relevant_rank_gate"]["answerable_cases"], 3);
        assert_eq!(
            strict["first_relevant_rank_gate"]["below_threshold_case_ids"],
            serde_json::json!(["late", "absent"])
        );
        assert_eq!(
            strict["first_relevant_rank_gate"]["observed_first_relevant_ranks"],
            serde_json::json!([
                {"case_id":"late","first_relevant_rank":3},
                {"case_id":"early","first_relevant_rank":1},
                {"case_id":"absent","first_relevant_rank":null}
            ])
        );
        let relaxed =
            with_input_receipts_with_rank(score, &[], b"{}", "json_article_array", 0, 0, 3)
                .unwrap();
        let relaxed: serde_json::Value = serde_json::from_str(&relaxed).unwrap();
        assert_eq!(
            relaxed["first_relevant_rank_gate"]["below_threshold_case_ids"],
            serde_json::json!(["absent"])
        );
        assert!(
            with_input_receipts_with_rank(score, &[], b"{}", "json_article_array", 0, 0, 101)
                .is_err()
        );
        let no_answer = r#"{"passed":true,"retrieval":{"cases":[
            {"id":"negative","relevant_count":0,"found_count":0,"retrieved_count":0}
        ]}}"#;
        let vacuous =
            with_input_receipts_with_rank(no_answer, &[], b"{}", "json_article_array", 0, 0, 1)
                .unwrap();
        let vacuous: serde_json::Value = serde_json::from_str(&vacuous).unwrap();
        assert_eq!(vacuous["first_relevant_rank_gate"]["passed"], false);

        let malformed = score.replace(r#""rank":3"#, r#""rank":2"#);
        assert!(with_input_receipts_with_rank(
            &malformed,
            &[],
            b"{}",
            "json_article_array",
            0,
            0,
            2
        )
        .is_err());
        let missing = score.replace(r#""ranked_evidence":[]"#, r#""ranked_evidence":null"#);
        assert!(
            with_input_receipts_with_rank(&missing, &[], b"{}", "json_article_array", 0, 0, 2)
                .is_err()
        );
    }

    #[test]
    fn parses_bounded_reproducible_run() {
        let args = parse_args(arguments()).unwrap();
        assert_eq!(args.top_k, 5);
        assert_eq!(args.min_recall_per_mille, 800);
        assert_eq!(args.max_no_answer_fp, 0);
        assert_eq!(args.min_case_recall_per_mille, 0);
        assert_eq!(args.min_case_precision_per_mille, 0);
        assert_eq!(args.max_first_relevant_rank, 0);
    }

    #[test]
    fn rejects_invalid_limits_duplicate_flags_and_missing_inputs() {
        let mut args = arguments();
        args.extend(["--min-case-recall-per-mille".into(), "1001".into()]);
        assert!(parse_args(args).is_err());
        let mut args = arguments();
        args.extend(["--min-case-precision-per-mille".into(), "1001".into()]);
        assert!(parse_args(args).is_err());
        let mut args = arguments();
        args.extend(["--max-first-relevant-rank".into(), "101".into()]);
        assert!(parse_args(args).is_err());
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
        args.extend(["--snapshot-root".into(), "snapshot".into()]);
        assert!(parse_args(args).is_err());
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
    fn duplicate_threshold_flags_are_rejected() {
        for (flag, value) in [
            ("--top-k", "3"),
            ("--min-recall-per-mille", "900"),
            ("--max-no-answer-fp", "1"),
            ("--min-case-recall-per-mille", "500"),
            ("--min-case-precision-per-mille", "500"),
            ("--max-first-relevant-rank", "2"),
        ] {
            let mut args = arguments();
            args.extend([
                flag.to_owned(),
                value.to_owned(),
                flag.to_owned(),
                value.to_owned(),
            ]);
            assert!(parse_args(args).is_err(), "accepted duplicate {flag}");
        }
    }

    #[test]
    fn report_destination_must_not_alias_corpus_or_suite() {
        let dir = std::env::temp_dir().join(format!(
            "wiki-retrieval-output-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let corpus = dir.join("corpus.json");
        let suite = dir.join("suite.json");
        fs::write(&corpus, b"[]").unwrap();
        fs::write(&suite, b"{}").unwrap();
        let mut args = parse_args(arguments()).unwrap();
        args.corpus = Some(corpus.clone());
        args.suite = suite.clone();
        args.output = dir.join("report.json");
        assert!(validate_output_destination(&args).is_ok());
        fs::write(&args.output, b"old report").unwrap();
        assert!(validate_output_destination(&args).is_err());
        fs::remove_file(&args.output).unwrap();
        args.output = corpus.clone();
        assert!(validate_output_destination(&args).is_err());
        args.output = dir.join("./suite.json");
        assert!(validate_output_destination(&args).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = dir.join("linked-report.json");
            symlink(&suite, &link).unwrap();
            args.output = link;
            assert!(validate_output_destination(&args).is_err());
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn report_publication_is_complete_and_never_overwrites_a_competing_output() {
        let dir = std::env::temp_dir().join(format!(
            "wiki-retrieval-publish-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let report = dir.join("report.json");
        persist_new_report(&report, r#"{"passed":true}"#).unwrap();
        assert_eq!(fs::read_to_string(&report).unwrap(), r#"{"passed":true}"#);
        assert!(persist_new_report(&report, "replacement").is_err());
        assert_eq!(fs::read_to_string(&report).unwrap(), r#"{"passed":true}"#);
        // Failed publication must not leave temporary reports behind.
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let target = dir.join("input.json");
            fs::write(&target, b"source").unwrap();
            let alias = dir.join("linked.json");
            symlink(&target, &alias).unwrap();
            assert!(persist_new_report(&alias, "replacement").is_err());
            assert_eq!(fs::read(&target).unwrap(), b"source");
            assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn verified_snapshot_catalog_decodes_zstd_records_and_rejects_corruption() {
        use wiki_model::{Block, BlockContent, Revision, ARTICLE_SCHEMA_VERSION};
        use wiki_store::record_codec::append_record_frame;

        let root = std::env::temp_dir().join(format!(
            "wiki-retrieval-snapshot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let article = Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
            revision: Revision {
                revision_id: 99,
                timestamp: "2026-10-24T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Earth".into(),
            display_title: "Earth".into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph("Gravity shapes spacetime.".into()),
            }],
            sections: vec![],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<p>Gravity shapes spacetime.</p>".into(),
            is_disambiguation: false,
        };
        let mut shard = Vec::new();
        append_record_frame(&mut shard, &PageRecord::Article(Box::new(article.clone()))).unwrap();
        let shard_path = root.join("articles.shard");
        fs::write(&shard_path, &shard).unwrap();
        let mut catalog = SnapshotCatalog::open(&root.join("catalog.sqlite")).unwrap();
        catalog
            .insert_verified(&root, "articles.shard", 0, shard.len() as u64)
            .unwrap();
        drop(catalog);

        assert_eq!(read_snapshot_corpus(&root).unwrap(), vec![article]);
        let args = Args {
            corpus: None,
            pack: None,
            snapshot_root: Some(root.clone()),
            suite: root.join("suite.json"),
            output: root.join("report.json"),
            sha: "a".repeat(40),
            top_k: 5,
            min_recall_per_mille: 800,
            max_no_answer_fp: 0,
            min_case_recall_per_mille: 0,
            min_case_precision_per_mille: 0,
            max_first_relevant_rank: 1,
            expected_articles_sha256: None,
            expected_suite_sha256: None,
        };
        fs::write(
            &args.suite,
            r#"{"schema_version":1,"suite_id":"snapshot-v1","cases":[
                {"id":"a-answer","query":"gravity spacetime","project":"enwiki",
                 "relevant":[{"article":{"project":"enwiki","page_id":42},
                 "revision_id":99,"block_id":"wkb:enwiki:42:99:b0"}]},
                {"id":"b-no-answer","query":"unobtainium","project":"enwiki","relevant":[]}
            ]}"#,
        )
        .unwrap();
        assert!(validate_output_destination(&args).is_err());
        let mut external = args;
        external.output = root.with_extension("report.json");
        assert!(validate_output_destination(&external).is_ok());
        let report_path = external.output.clone();
        run(external.clone()).unwrap();
        let report = fs::read_to_string(&report_path).unwrap();
        assert!(report.contains("\"suite_id\": \"snapshot-v1\""));
        assert!(report.contains("\"passed\": true"));
        let parsed: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["schema_version"], 3);
        assert_eq!(parsed["first_relevant_rank_gate"]["passed"], true);
        assert_eq!(
            parsed["first_relevant_rank_gate"]["max_first_relevant_rank"],
            1
        );
        assert_eq!(parsed["input_receipts"]["canonical_article_count"], 1);
        assert_eq!(
            parsed["input_receipts"]["corpus_source_kind"],
            "local_snapshot_frames"
        );
        fs::remove_file(report_path).unwrap();

        // Exact input pins accept this snapshot and refuse changed labels
        // before publishing a misleading success or failure report.
        let mut pinned = external.clone();
        pinned.output = root.with_extension("pinned-report.json");
        pinned.expected_articles_sha256 = Some(canonical_articles_sha256(&[article]).unwrap());
        pinned.expected_suite_sha256 = Some(wiki_pack_format::sha256_hex(
            &fs::read(&pinned.suite).unwrap(),
        ));
        run(pinned.clone()).unwrap();
        fs::remove_file(&pinned.output).unwrap();
        pinned.expected_suite_sha256 = Some("0".repeat(64));
        assert!(run(pinned.clone())
            .unwrap_err()
            .contains("label suite bytes"));
        assert!(!pinned.output.exists());

        // A strict per-case floor must fail a missed labeled question even
        // when the aggregate recall floor is explicitly disabled. The
        // complete failed report is still published for diagnosis.
        let mut missed = external;
        missed.output = root.with_extension("missed-report.json");
        missed.suite = root.join("missed-suite.json");
        missed.min_recall_per_mille = 0;
        missed.min_case_recall_per_mille = 0;
        missed.min_case_precision_per_mille = 500;
        missed.max_first_relevant_rank = 1;
        fs::write(
            &missed.suite,
            r#"{"schema_version":1,"suite_id":"missed-gold-v1","cases":[
                {"id":"unretrieved","query":"unobtainium","project":"enwiki",
                 "relevant":[{"article":{"project":"enwiki","page_id":42},
                 "revision_id":99,"block_id":"wkb:enwiki:42:99:b0"}]}
            ]}"#,
        )
        .unwrap();
        let missed_report = missed.output.clone();
        assert!(run(missed)
            .unwrap_err()
            .contains("retrieval regression failed"));
        let missed_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&missed_report).unwrap()).unwrap();
        assert_eq!(missed_json["passed"], false);
        assert_eq!(missed_json["case_recall_gate"]["passed"], true);
        assert_eq!(missed_json["case_precision_gate"]["passed"], false);
        assert_eq!(missed_json["first_relevant_rank_gate"]["passed"], false);
        assert_eq!(
            missed_json["case_precision_gate"]["below_threshold_case_ids"],
            serde_json::json!(["unretrieved"])
        );
        fs::remove_file(missed_report).unwrap();

        let final_byte = shard.len() - 1;
        shard[final_byte] ^= 1;
        fs::write(&shard_path, &shard).unwrap();
        assert!(read_snapshot_corpus(&root).is_err());
        fs::remove_dir_all(root).unwrap();
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
