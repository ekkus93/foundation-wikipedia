# RAG-003 — Independent answer-support qualification protocol (2026-11-03)

## Purpose

The existing answer-support runner validates human-supplied labels, coverage, and citation IDs. It does **not** independently establish factual entailment, reviewer identity, source authenticity, or dataset representativeness. Further synthetic CLI gates alone cannot satisfy RAG-003. This protocol defines the missing external evidence and prevents a synthetic fixture from being misreported as qualification.

## Required evidence before checking RAG-003

1. **Source manifest:** record Wikimedia project, page ID, revision ID, original source URL, acquisition timestamp, source-generation identity, checksum, and the verification method for each sampled article. Pin the canonical article corpus and annotation suite with SHA-256. A checksum alone proves byte stability, not publisher authenticity.
2. **Sampling plan:** predeclare projects, strata (language, article length, topic, table/infobox/reference complexity), random seed, exclusions, minimum cases and distinct articles **per project**. Publish selected IDs and counts before looking at model scores; report missing and rejected examples. Do not reuse training/tuning examples as independent test examples.
3. **Human review:** use at least two independent reviewers per sampled answer, with reviewer pseudonymous IDs, timestamp, rubric version, claim spans, supporting or contradicting source spans, and citation judgments. Record disagreement and adjudication separately. Keep immutable raw judgments rather than only consensus labels.
4. **Entailment rubric:** classify each atomic claim as supported, contradicted, not established, or unreviewable against the **exact revision** cited. Verify that each citation points to a source passage that actually entails the claim; a valid revision/block ID is insufficient. Record unsupported claims and false-positive citations explicitly.
5. **Execution manifest:** capture runner version/commit SHA, exact command, model/configuration, corpus/suite digests, per-project gates, raw report JSON, aggregate/per-project results, and any failed thresholds. Use `--require-schema-v3`, `--corpus`, both `--expect-*-sha256` pins, and `--required-projects` for qualification.
6. **Acceptance:** obtain independent review of the provenance and sampling manifests, reproduce the pinned run from a clean checkout, and record desktop/Android device and offline-mode acceptance separately. Never infer real-device acceptance from host CI.

## Execution template

```sh
cargo run -p wiki-search --example answer_support_runner --locked -- \
  --suite reviewed_answers.json --corpus canonical_articles.json \
  --output answer_support_report.json --require-schema-v3 \
  --expect-suite-sha256 "$SUITE_SHA256" \
  --expect-corpus-sha256 "$CORPUS_SHA256" \
  --required-projects enwiki,frwiki \
  --min-cases 100 --min-distinct-articles 25 \
  --min-project-cases 10 --min-project-distinct-articles 5 \
  --min-claim-support-per-mille 800 \
  --min-citation-precision-per-mille 800 \
  --min-project-claim-support-per-mille 800 \
  --min-project-citation-precision-per-mille 800
```

These values are **illustrative, not approved acceptance thresholds**. Select thresholds and required projects in a preregistered evaluation plan. Never fabricate the input files, reviewer identities, publisher authentication, results, or device evidence.

## Current status

**RAG-003 remains unchecked.** This document specifies an evidence collection and review procedure; it does not assert that an independently reviewed corpus exists or that any qualification was run. Next useful work is building a versioned, machine-validated provenance/reviewer manifest and independent annotation workflow, not another synthetic score gate.
