# SRC-003 — Bounded interrupted reads in expected-census SHA-256 audit

Date: 2026-10-23
Status: Qualified portable Rust bounded-I/O improvement; SRC-003 remains unchecked.

## Problem and change

The `sha256_bounded` helper in `wiki-source::public_dump_census_gate`,
used for independently supplied expected-page census files, previously
retried `ErrorKind::Interrupted` indefinitely. A reader that repeatedly
signals `Interrupted` could monopolize the audit instead of failing
closed, despite the existing data-length bound.

The reader now permits at most **eight consecutive interrupted reads**
and returns a typed `CensusGateError::Io` on the next interruption.
The counter resets after any successful read, preserving valid streaming
with intermittent transient interruptions. The byte-count and digest
checks are otherwise unchanged.

## Regression coverage and CI

New deterministic synthetic input tests show a successful stream with
eight interruptions before its first read and another eight after its
first successful chunk. The calculated digest matches the full input.
A separate case returns nine interrupts without data and is rejected
without advancing the input position.

- Exact implementation SHA: `2ee8b93734eed1105ff5c621e7cc62924d822275`.
- [Rust workspace CI 40007253453](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40007253453) — **passed**, including formatting, strict Clippy and tests.
- [Platform shells CI 40007253334](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40007253334) — still running at note creation; verify final status separately.

## Explicit limits

This does not independently authenticate the expected census or
prove that its project/generation/namespace cutoff agrees with the
upstream dump publication. The existing comparison still requires
independently trusted expected input and full source/content/media
qualification before any snapshot is release-ready.

**Do not mark SRC-003 complete from this focused fault-injection result.**
