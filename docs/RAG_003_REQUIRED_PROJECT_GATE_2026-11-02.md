# RAG-003 — Explicit required-project benchmark gate (2026-11-02)

## Scope

The human-reviewed answer-support runner previously enforced per-project quality and coverage only for projects *present* in the supplied suite. A benchmark that silently omitted an intended Wikimedia edition could still pass.

The runner now accepts `--required-projects enwiki,frwiki` (a single comma-separated list of 1–32 unique lowercase ASCII alphanumeric project IDs, each 2–32 bytes). When enabled, it compares the declared set against observed case projects and fails the benchmark if any required project is absent. Its diagnostic JSON includes `gate.required_project_gate` with `enabled`, `required_projects`, `missing_projects` and `passed`. This is independent of the existing per-project sample-size and quality thresholds.

The option defaults to disabled to preserve existing callers. Duplicate option occurrences, duplicate project IDs, empty members, uppercase identifiers, punctuation, and overlong lists are rejected. Tests cover omitted French Wikipedia despite valid English cases, success with the required English project, and disabled behavior.

## Qualification

Exact source commit: `a34f2e17218ecd6010a9dc325c2cf6c72de3d680`.

- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41580351538 — passed.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41580351612 — passed.
- Android Rust ABI qualification: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41580351579 — passed.

Initial commits failed formatting and a missing test initializer; both were corrected before qualification. No branches or PRs were created.

## Limits

The declared set is operator supplied and is not independently authenticated; it does not prove dataset representativeness, reviewer authenticity, evidence provenance, or factual entailment. RAG-003 remains unchecked pending those acceptance requirements and real-device evidence.
