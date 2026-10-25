# SRC-003 — Crash-orphan checkpoint reclamation (2026-10-25)

Scope: host-side `wiki-source::public_dump_resume` acquisition only.

A crash after successfully publishing a digest-verified compressed member but
before deleting its partial checkpoint could leave a second, potentially large
copy in the private staging directory. On subsequent resumable requests, the
already published member is reverified using the pinned inventory first; only
after this successful cache hit, the downloader opportunistically removes the
obsolete private partial. It does not initiate HTTP requests.

Cleanup skips symlinks, hard-linked files, nonregular files, and files whose
metadata changes between inspection and open. A cleanup failure does not
invalidate an independently verified complete member. Synthetic regression
tests cover ordinary orphan cleanup, no HTTP requests, and preserving suspicious
symlink/hardlink checkpoint aliases.

This is disk-space recovery, not Wikimedia publisher authentication, an
independent expected census, complete rendering, mandatory offline media,
or device acceptance. SRC-003 remains unchecked.
