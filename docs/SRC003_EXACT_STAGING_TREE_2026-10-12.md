# SRC-003: Exact inventory staging preflight (partial)

The public-dump staging verifier now has a separate exact-tree preflight intended for callers that enumerate staged files. It first verifies all declared member digests and then rejects undeclared files, directories, symlinks and special filesystem objects. This reduces the risk of inadvertently importing a stale or unlisted file from a staging tree.

This preflight is not a race-free directory-handle sandbox. The staging directory must remain private and unchanged while it is verified and consumed. An independently authenticated publisher inventory is still required. XML parsing, renderer fidelity and offline pack acceptance are not implemented by this slice. The canonical SRC-003 checklist remains open.

Qualification pending: the initial source/test commit failed Rust formatting; do not treat this change as accepted until exact-head Rust and platform workflows pass.
