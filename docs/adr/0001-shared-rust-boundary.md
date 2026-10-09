# ADR 0001 — Shared Rust core and platform adapters

**Status:** Accepted architectural boundary; runtime bridge pending.
**Date:** 2026-10-09
**SPEC:** §2, §5, §18
**TODO:** BOOT-001, BOOT-002, MOB-001

## Context

Desktop uses Tauri/React and Android uses Compose/WebView. Both must share normalization, provenance, pack verification and retrieval rules. Duplicating them in platform UI code risks inconsistent trust and citations.

## Decision

Keep shared domain and use-case logic in platform-independent Rust crates. Tauri and Android use thin adapters. The wiki-ffi crate builds as an rlib and a cdylib for future Android use. Cross-building a library does not prove a working UniFFI interface.

## Alternatives

Duplicated Kotlin and TypeScript domain logic was rejected because it could drift. An always-running local HTTP service was rejected as a mandatory mobile dependency.

## Consequences

Versioned DTOs, generated Kotlin bindings, lifecycle cancellation and physical device qualification remain MOB-001 work. BOOT-002 remains unchecked until real platform acceptance and bridge evidence are available.
