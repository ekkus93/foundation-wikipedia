# Portable bzip2 dependency license note

The `wiki-source` Rust decoder uses `bzip2 0.6.1` with its default pure-Rust `libbz2-rs-sys` backend. `bzip2` is MIT OR Apache-2.0; `libbz2-rs-sys` reports the SPDX identifier `bzip2-1.0.6`.

SPDX publishes the exact bzip2 license terms at https://spdx.org/licenses/bzip2-1.0.6.html . This is a permissive redistribution license with a required copyright notice, conditions, disclaimer, accurate attribution of origin and no endorsement using the author's name. Packaging and distribution must retain the applicable notice and disclaimer for this dependency; adding the SPDX identifier to the dependency audit is **not** a waiver of notice obligations.

The dependency is included for portable multistream decompression and does not authorize external entity resolution, unverified Wikimedia sources, or publication of provisional output. The complete public-dump fallback remains unqualified.
