# RAG-003 — Local evidence artifact SHA-256 verification

**Date:** 2026-11-03  
**Source commit:** `d184b1721b962fd13fa7bc96a1d8c4f30c6619c3`  
**Scope:** Partial RAG-003 validation; no independently reviewed Wikimedia corpus is claimed.

## Gap closed

The RAG-003 manifest contains SHA-256 digests for the preregistered plan, each Wikimedia source revision, canonical corpus, annotation suite, evaluation report and model configuration. Until this change, the manifest validator checked their **syntax**, not the corresponding **on-disk bytes**.

`scripts/validate_rag003_review_manifest.py` now has an explicit `--evidence-root DIR` mode. In this strict mode, the validator first enforces its Draft 2020-12 schema and semantic checks, then opens **all** declared local artifacts under an operator-selected root and compares independently computed SHA-256 values against their pinned manifest fields. Missing paths, files and differing digests fail the command.

## Required relative path receipts

In strict mode, provide the following JSON string fields in addition to the existing required fields:

| Manifest location | Relative path field | Compared digest |
| --- | --- | --- |
| `preregistration` | `plan_path` | `plan_sha256` |
| Each entry in `source_articles` | `artifact_path` | `sha256` |
| `execution` | `suite_path` | `suite_sha256` |
| `execution` | `corpus_path` | `corpus_sha256` |
| `execution` | `report_path` | `report_sha256` |
| `execution` | `model_config_path` | `model_config_sha256` |

These path fields are optional in manifest-only mode for backward compatibility. **Qualification requires strict mode** with actual readable artifacts; running without `--evidence-root` explicitly reports that artifact bytes were **not** verified.

### Command

```sh
python3 -m venv .rag003-venv
.rag003-venv/bin/python -m pip install -r scripts/requirements-rag003.txt
.rag003-venv/bin/python scripts/validate_rag003_review_manifest.py \
    review-manifest.json --evidence-root ./review-evidence
```

Each path is **relative to `./review-evidence`** and must use forward slashes; for example `source/enwiki-article-123-revision-456.json`. The recorded digest is the SHA-256 of the **exact file bytes** at that path, not the hash of a normalized/parsing representation.

## Input-safety boundaries

The strict reader uses directory-relative file descriptors with `O_NOFOLLOW` for every component. It rejects absolute paths, `.` and `..` components, empty segments, backslashes, embedded NUL, symbolic links, and nonregular file objects. It hashes 64-KiB chunks rather than reading full files into RAM and imposes a **256 MiB per-artifact cap**; the manifest itself retains the existing **2 MiB cap**. Unsupported platforms lacking secure POSIX open flags fail closed rather than silently following links. Files larger than the cap require an explicitly reviewed alternative representation, not disabling safety checks.

## Regression and CI evidence

Synthetic fixtures construct all six artifact types with real local byte hashes and test successful verification. Negative cases cover modified corpus, missing source, missing path receipt, traversal/absolute/backslash/NUL paths, symlinked files/directories, oversized input and the CLI strict-mode exit code. The checked-in Python unit suite runs inside the Rust workspace workflow.

- First implementation commit `15ded202382c13ca560f75652d959a1ab2c8521f` passed [Rust workspace 41738009284](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41738009284). A path guard was then tightened to reject **single** backslashes and embedded NUL explicitly.
- Corrected source `d184b1721b962fd13fa7bc96a1d8c4f30c6619c3` passed [Rust workspace 41738198579](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41738198579). [Platform shells 41738198705](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41738198705) was still running at report preparation.

## Qualification limits

A matching digest establishes **local byte consistency with the manifest**, not that the manifest accurately describes a Wikimedia-published revision. It does not authenticate a source URL/publisher, prove chronology, notarize preregistration, establish distinct reviewers as real independent humans, guarantee the annotated claim spans correspond to the submitted answer, or establish factual entailment. The manifest and evidence bundle can still be deliberately fabricated together. Independent source acquisition, reviewer attestation, independent replication, and physical desktop/Android acceptance remain outstanding. **Do not mark RAG-003 complete from synthetic tests or checksums alone.**
