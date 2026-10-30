# PACK-004 — Custom export directory-sync rollback (partial)

**Date:** 2026-10-30 (America/Los_Angeles)  
**Corrected implementation:** `eaaadecb456b7e0de4361544c993d3ba7671eb53`  
**Scope:** Provisional `wiki-pack export-custom` unsigned envelope publication.

## Failure mode

The export CLI already wrote, fsynced and reverified a temporary envelope, then published it with a no-overwrite hard link. It removed the temporary link and finally fsynced the output directory. If that last directory durability barrier failed, the command returned an error but left the newly published output filename present. A caller could mistake this failed export for a successful, usable artifact.

## Change

- Added `sync_export_publication` on Unix. It attempts the parent-directory durability sync after publication. On failure, it removes the newly published destination and fsyncs the directory again to make the rollback durable, returning an error in all cases.
- If removal or rollback sync itself fails, the error explicitly identifies the incomplete rollback rather than claiming the destination is gone.
- Added an injected-failure regression proving a failed directory sync removes the new file, and a success-path regression proving a successful barrier preserves it.
- Existing no-overwrite publication and complete envelope self-verification are unchanged.

## CI and scope boundary

- Initial source `5e3eb491ef0e11838d8be8050c9ed83b3617e3ad` failed rustfmt ([Rust workspace 41059827047](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41059827047)); the formatting was corrected.
- Corrected source `eaaadecb456b7e0de4361544c993d3ba7671eb53`: [Rust workspace 41059958805](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41059958805), [Platform shells 41059958528](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41059958528). CI was still running when this note was written; verify the exact SHA before treating it as qualified.
- This is not a simulated process crash or power-loss test. It assumes exclusive control of the output parent and cannot guarantee rollback if filesystem deletion or its durability barrier also fails. No official publisher signature, required offline-media completeness or installed-pack activation is provided. **PACK-004 remains unchecked.**
