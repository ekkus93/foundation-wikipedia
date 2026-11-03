# RAG-003 — Preregistered source-project scope

**Date:** 2026-11-03  
**Implementation commit:** `3e7e8e4eb481c23a1c30db931212fb3cc254c10d`  
**Status:** Partial; RAG-003 remains unchecked.

## Issue

The machine-validated independent-review manifest already required a nonempty, unique list of `preregistration.required_projects` and minimum case/article counts for those projects. However, the source manifest accepted acquired articles from additional projects absent from that preregistered list. Those entries could be carried into the purported qualification artifact without an explicit out-of-scope warning. A required project with no acquired source articles also lacked a dedicated diagnostic.

## Change

`scripts/validate_rag003_review_manifest.py` now rejects any source whose `project` is not present in the preregistered required-project list. It also reports each required project with no acquired source articles, in addition to existing case and article coverage failures. The validator still enforces exact source revision, acquisition time, two distinct qualifying reviewer IDs per case, and review timing.

`scripts/test_validate_rag003_review_manifest.py` adds deterministic negative tests for an unplanned project and for a required project with no acquired sources. A positive fixture verifies that adding a second preregistered project with an acquired source and two distinct matching reviews remains accepted.

## Evidence

- [Rust workspace CI 41737154077](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41737154077) — running at documentation.
- [Platform shells CI 41737153683](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41737153683) — running at documentation.

These are synthetic manifest tests. A listed project, timestamp, source URL, digest or reviewer ID is not proof of authentic source acquisition, independent review, preregistration or factual entailment. The project still requires external qualification evidence under `docs/RAG_003_INDEPENDENT_QUALIFICATION_PROTOCOL_2026-11-03.md`.
