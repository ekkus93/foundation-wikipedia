# PACK-001 / TRUST-003 — Portable staging-directory names (2026-10-29)

The provisional `stage_container` staging-directory `name` argument now reuses the pack manifest's Win32 reserved-name component guard. Previously the ASCII-only staging-name check allowed `CON`, `PRN`, `NUL`, `COM1`, `LPT9` and their case/extension variants; these are reserved devices on Windows and are unsafe as extraction destinations. The existing ASCII-only name grammar, maximum length, private-parent validation and fail-closed object verification are unchanged.

Regression `rejects_windows_device_names_before_touching_staging_parent` verifies that invalid names fail before reading input or touching a nonexistent parent path. This remains a provisional unsigned envelope stager; it does not authenticate a publisher or activate a pack.

**PACK-001 and TRUST-003 remain unchecked** pending complete publisher signature, hostile import, offline media and platform/device acceptance.
