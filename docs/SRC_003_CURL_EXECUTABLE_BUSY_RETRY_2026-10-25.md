# SRC-003 — Bounded curl executable-busy retry (2026-10-25)

Status: host transport resilience; SRC-003 remains unchecked.

The host-side public-dump HTTPS inventory, member and byte-range subprocess
launchers now retry only `ExecutableFileBusy` (ETXTBSY) up to three times
with short bounded backoff. This mirrors the existing host online-article
curl transport behavior. A failed exec has not launched an HTTP request,
so retrying this precise OS error cannot duplicate a transfer. Permission
denial, missing executable and all other spawn errors still fail immediately.
Exhausted retries retain the existing public `SpawnFailed` result while
emitting only the OS error kind and numeric code for CI diagnostics.

A deterministic unit regression checks retry counts, successful recovery
and immediate permission-denied failure. This change targets the intermittent
fake-curl subprocess launch failures seen in Rust CI, not publisher
authentication, source completeness, HTML/media fidelity or device acceptance.
