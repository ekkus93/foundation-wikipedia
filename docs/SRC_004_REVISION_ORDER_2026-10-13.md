# SRC-004 cached revision ordering (2026-10-13)

Status: incremental implementation qualified; SRC-004 remains unchecked.

The reader now chooses the highest validated cached revision ID rather than the most recently fetched entry. The cache adapter rejects a newly fetched older revision without replacing newer stored content. Unit tests cover out-of-order fetch timestamps, newer revision removal, and regression rejection.

Code commits: 63199dfd54d72decf5218b48a0da89bc2bf220bd and fea92c33b794ee905bf98fc57970296f7f949220.

Exact-head CI passed at fea92c33b794ee905bf98fc57970296f7f949220: Rust workspace run 38457459399, Platform shells run 38457459431, Android Rust ABI run 38457459741. The intermediate commit 902821cdfe6ac4c8f93c3a24aac333d66d59cdab failed Rust formatting, corrected in the qualified head.

This does not provide authenticated HTTPS transport, offline media completeness, real UI integration or physical-device acceptance. Temporary HTML remains marked incomplete. See docs/WIKIPEDIA_AI_READER_TODO.md for remaining SRC-004 work.
