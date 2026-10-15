# Foundation Wikipedia — Ralph Bridge write and CI incident evidence (2026-10-15)

## Scope and interpretation

This is an evidence record for the current recovery session, not a claim that every previous failure was caused by Ralph Bridge. Entries distinguish observable facts from hypotheses and never include credentials, authorization tokens or raw user data. Canonical implementation TODO is not edited.

## Identity and preflight

- Repository: `ekkus93/foundation-wikipedia`, branch: `master` (the only inventoried branch; no associated open PRs).
- Ralph Bridge ping: success; protocol schema 3, semantic version `0.6.2`, build `0acea4b23362e2ccecab8f6c9db4418b162426cf`, server instance `51c945dd809b4207800c3ba33151ad94`, started `2026-10-15T02:25:51.132200Z`, auth mode `app`.
- Pre-repair exact master head: `5f4db5db8ce2c918785a9744dc1ae5fe8c7bd766`.
- Canonical `docs/WIKIPEDIA_AI_READER_TODO.md`: `total_lines=1536`, original blob SHA `ec594ed49cf4fff5e1c7ab42fca927881be55df5`. The complete TODO was inspected through eight consecutive 200-line-or-shorter windows; do not infer completeness from a truncated response.
- `docs/SRC_003_CENSUS_PIPELINE_2026-10-14.md`: `total_lines=346`, blob `0191d49981243930fce5039d03225248068ac51d`; preserved.

## Incident 1: failing exact-head Rust formatting (confirmed)

- Original master SHA `5f4db5db8ce2c918785a9744dc1ae5fe8c7bd766`.
- Rust workspace workflow [run 38707257176](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38707257176), job `116329632286`, step `Format`: failed with exit code 1. Evidence lines 302–341 show two `rustfmt` line-wrap differences in `crates/wiki-source/src/public_dump_revision_jsonl.rs` (test at approximately line 325/341).
- Platform shells [run 38707257159](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38707257159): succeeded on the same SHA.
- Repair: Ralph Bridge `create_or_update_file`, expected head `5f4db5db8ce2c918785a9744dc1ae5fe8c7bd766`, exact path `crates/wiki-source/src/public_dump_revision_jsonl.rs`, only two formatting substitutions, UTF-8 content length `15107` bytes. Operation ID `dc71328cea6c4edd9ccbeb1d316a24e8`, request time `2026-10-15T10:50:05.727537Z`; returned `accepted=true` and `write_reconciled=true`.
- **Important warning**: `GitHub ref-update acknowledgement was uncertain; Ralph re-read the branch and confirmed the intended commit as its head.` Two further warnings confirmed exact parent/diff verification and that CI must be checked separately. New verified head: `fcbd4ed0cf815013bba8f1368b20be22c830a70c`, parent `5f4db5db8ce2c918785a9744dc1ae5fe8c7bd766`. **Do not blindly retry ambiguous writes.**
- Recovery exact-head qualification: [Rust 38723062099](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38723062099) and [Platform 38723062150](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38723062150), both successful. Canonical TODO remains unchanged.

## Incident 2: earlier write rejections (incomplete evidence)

- Previous run reported rejected write operations against `master`, including the formatting repair and an attempted restoration, before execution. The actual tool response status/error payload, operation IDs, input diagnostics and timestamps were not retained in this session. Therefore **the rejecting layer is unconfirmed**; do not attribute the failures definitively to bridge, GitHub or a platform safety gate.
- The repair succeeded on the first observed write attempt of this session. This is a successful retry on a newer bridge process, **not** proof that the underlying intermittent issue was resolved.

## Incident 3: unrelated intermittent CI subprocess failures (historical evidence)

- Historical Rust CI attempts sometimes reported `online_curl` tests returning `SpawnFailed` where `ResponseTooLarge` or an HTTP status was expected; documented in `docs/SRC_003_CENSUS_PIPELINE_2026-10-14.md` (for example run 38666642949 and first attempt of 38667126349). Later retries passed without changing curl transport code.
- `crates/wiki-source/src/online_curl.rs` previously collapsed all `std::process::Command::spawn` errors to the single `CurlTransportError::SpawnFailed` marker. The actual `ErrorKind` and raw OS errno were unavailable in test logs.
- The instrumentation added in the accompanying source change emits only `ErrorKind` and `raw_os_error` to stderr on a failed spawn, without exposing request URLs, executable paths, environment variables or tokens. A deterministic non-executable-path test protects the unchanged public `SpawnFailed` API contract. These diagnostics can distinguish `EAGAIN` / process-resource limits from `EACCES` / permission denial, `ENOENT` / missing binaries and `ETXTBSY` / executable contention on later failures. This instrumentation is **not** a fix for the underlying flake.

## Future incident capture checklist

For any future write or test failure, preserve: UTC timestamp; bridge version/build/instance/start/auth mode; operation/tool name; sanitized argument shape and paths (not credentials or full source content); branch/exact expected SHA and observed post-failure SHA; returned operation ID, accepted/reconciled status, warning/error type, message and structured diagnostics; CI run/job/step ID, run attempt, exact SHA and bounded log window. Re-read branch state before any retry of an ambiguous mutation. Record whether the failure happened before tool invocation, within bridge validation, after GitHub ref-update acknowledgment, or during CI; if the layer cannot be shown, mark it unknown. Follow source integrity and exact-head CI checks after recovery.

## Outstanding limitations

Full SRC-002/SRC-003 source authenticity, production Rust XML/bzip2 parsing, independent coverage and rendered/offline media acceptance remain open. This document is troubleshooting evidence, not completion of those TODOs.
