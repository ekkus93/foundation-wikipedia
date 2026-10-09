# SRC-001 — public dump discovery qualification (partial)

Canonical checklist: docs/WIKIPEDIA_AI_READER_TODO.md. SRC-001 remains unchecked.

Implemented read-only official dump metadata discovery, with bounded date scanning, completed-generation checks, exact file paths, size and checksum validation, and a clear distinction between legacy upstream SHA-1/MD5 and authenticated SHA-256. No source download or snapshot activation is performed.

- Commit a09832a7d8408a2a9adb8baadcb1e0bac524bece: Rust workspace CI 37975950720 passed; Platform shells CI 37975950847 passed.
- Commit c0d1eb374979cd71f35701c7e83af8a1e13cdfa2: missing status documents are skipped without ignoring unrelated server errors. Rust CI 37976424040 passed; Platform CI 37976424073 was still running at last observation.
- Commit 97c3ce36ab62856a097af8169edf8262b285d912: impossible generation dates and reserved source filenames are rejected. Rust CI 37976587866 passed; Platform CI 37976587923 was still running at last observation.

Offline synthetic tests do not establish that a live Wikimedia dump was downloaded, its checksums verified, or its articles rendered. Remaining: live discovery qualification, resumable source download, authenticated manifest provenance, bytewise checksum verification, staging/rollback, and official source joins.
