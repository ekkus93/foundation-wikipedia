# RAG-003 — Per-project reviewed-answer support gates (2026-11-02)

## Scope

The offline human-reviewed answer-support benchmark now supports opt-in **per-Wikimedia-project** quality floors, preventing high scores from a large project/language from hiding weak results in a smaller project. This is a structural quality gate over caller-provided human annotations, not an automated factual-entailment or reviewer-authentication system.

Implementation: `crates/wiki-search/examples/answer_support_runner.rs` on `master`. No new branches or pull requests.

## New CLI thresholds

- `--min-project-claim-support-per-mille N`: require every observed Wikimedia project to meet N/1000 supported reviewed claims.
- `--min-project-citation-precision-per-mille N`: require every observed Wikimedia project to meet N/1000 supported reviewed citations.

Both accept 0..1000, default to **0** (disabled for backward compatibility), reject malformed values and repeated flags, and preserve the existing aggregate and per-case gates. When enabled, an empty reviewed denominator fails instead of being treated as 100%.

Example for an independently reviewed, pinned canonical corpus:

```sh
cargo run -p wiki-search --example answer_support_runner --locked -- \
  --suite reviewed_answers.json --corpus canonical_articles.json \
  --output answer_support_report.json \
  --require-schema-v3 \
  --expect-suite-sha256 <64-lowercase-hex> \
  --expect-corpus-sha256 <64-lowercase-hex> \
  --min-cases 100 --min-distinct-articles 25 \
  --min-claim-support-per-mille 800 \
  --min-citation-precision-per-mille 800 \
  --min-project-claim-support-per-mille 800 \
  --min-project-citation-precision-per-mille 800
```

The output JSON adds `gate.project_claim_support_gate` and `gate.project_citation_precision_gate`. Each receipt records `enabled`, `min_per_mille`, `observed_projects`, sorted `project_scores` (`project`, `supported`, `reviewed`, `passed`), `failed_projects`, and `passed`. Any enabled project failure causes the top-level `gate.passed` to be false; a diagnostic report is still saved.

## Verification

- Initial source commit `5d63567acc2ab0cc9edb28075f87bbe551c42f54` failed the Rust formatting gate only: [Rust workspace 41556196721](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41556196721).
- Corrected source commit **`7c1491606626471dcdf9c8f9fde60bd807dc30f8`** passed [Rust workspace 41556278608](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41556278608), [Platform shells 41556278644](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41556278644), and [Android Rust ABI 41556278582](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41556278582).
- Rust workspace CI executes the executable example regression tests with `cargo test -p wiki-search --example answer_support_runner --locked`.
- Synthetic multilingual regression: one fully supported enwiki case and one unsupported frwiki case yield 50% aggregate support, but with an 800/1000 project floor, frwiki is correctly identified as failing. The existing canonical-corpus runner integration also verifies a failing project-level gate persists the failure receipt.
- Exact code SHA is the qualification boundary; CI for this documentation commit must be evaluated separately.

## Remaining RAG-003 acceptance

No independent, representative Wikimedia claim/retrieval corpus or reviewer identity verification was created here. Source and label hashes are content identity, not publisher or reviewer authenticity. These gates do not prove semantic entailment or generated-answer correctness, and no real-device measurements were made. Keep RAG-003 unchecked pending the governing TODO's independent corpus, provenance, retrieval/answer metrics, and device acceptance.
