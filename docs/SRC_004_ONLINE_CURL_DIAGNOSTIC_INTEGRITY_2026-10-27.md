# SRC-004 — Fail-closed online curl diagnostic and HTTP-status verification

**Date:** 2026-10-27 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `abe04a6ef71fe92801fc8615e2c162b40376f2cb`

## Problem

The host/desktop `wiki-source::online_curl::CurlTransport` uses a bounded subprocess with HTTPS-only URL preflight, certificate verification, fixed deadlines, and no redirect following. It previously retained only the **last** 64 KiB of stderr. If an earlier diagnostic contained a conflicting `FW_HTTP_CODE:` write-out marker, truncation could remove it. The parser then selected the last parseable `u16` marker, accepting noncanonical values such as `+200` and allowing a later `200` to override an earlier `503`.

The output body is read separately from stderr. A successful subprocess and apparently valid body must not override missing, truncated, ambiguous or malformed status evidence.

## Implementation

In `crates/wiki-source/src/online_curl.rs`:

1. Drain stderr concurrently with stdout to prevent pipe deadlock, but retain at most 64 KiB and track whether any additional bytes were observed. After the subprocess terminates, reject an oversized diagnostic stream with `CurlTransportError::DiagnosticTooLarge`; **never** accept a retained tail as a complete receipt.
2. Parse raw diagnostic bytes, requiring exactly one `FW_HTTP_CODE:` line whose value is **exactly three ASCII digits**. Reject duplicate, malformed, absent or noncanonical markers with `ProcessFailed`.
3. Preserve explicit `HttpStatus(code)` for valid non-200/nonzero codes; preserve `ProcessFailed` for `000`, unsuccessful subprocess exits, and invalid marker evidence.
4. Keep the pre-existing HTTPS URL preflight, response-body byte cap, no-redirect policy, and bounded subprocess deadlines unchanged.

## Regression evidence

Unix fake-curl subprocess tests now cover:
- Stderr larger than a normal OS pipe and the 64-KiB cap, followed by apparent HTTP 200 **or** 503: both are rejected, while both pipes are drained without deadlock.
- Bounded ordinary diagnostics plus exactly one canonical `200`: accepted.
- Malformed `+200`, `0200`, `200x`, `2O0`, `200 `, empty marker, absent marker: rejected.
- Duplicate `200`/`200` and conflicting `503`/`200` markers: rejected.
- Existing real-status, URL, transport and response-cap tests remain in the workspace suite.

The first code commit `f32ebd6c489d8d41854f8a3d6ddfa32f09c5f27f` failed **formatting only** ([Rust workspace 40638637884](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40638637884)). The corrected source SHA `abe04a6ef71fe92801fc8615e2c162b40376f2cb` passed [Rust workspace 40638702699](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40638702699) (format, strict Clippy, workspace tests) and [Platform shells 40638702299](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40638702299).

## Limitations and next steps

These are deterministic host-side transport and synthetic subprocess tests. They do not establish live Wikimedia Action API compatibility, Android HTTP transport parity, publisher-authenticated full-source generation, revision-matched required visual resources, installed pack integrity, UI recovery behavior or real-device acceptance. Continue SRC-004 online/offline integration and source/media qualification; do not mark SRC-004 complete based on this guard alone.
