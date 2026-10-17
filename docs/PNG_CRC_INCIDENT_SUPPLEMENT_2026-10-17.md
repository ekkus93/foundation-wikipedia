# 2026-10-17 PNG CRC write and CI incident supplement

Canonical incident log: `docs/RALPH_BRIDGE_WRITE_AND_CI_INCIDENTS_2026-10-15.md`. Its 860-line blob `64e2fad13bcba0e27e3dfbb09b7a1692214c1d22` remains unchanged because a complete-content append attempt was denied before bridge acknowledgment. Reconcile this supplement into that log when possible; never overwrite its existing entries.

Ralph Bridge: v0.6.2, build `0acea4b23362e2ccecab8f6c9db4418b162426cf`, instance `090878b555454b658a84fd67850085fe`, started 2026-10-16T16:03:29.198120Z, auth mode app.

Initial master `aed3a19597384c4f1c4b4c5f3219d20cc4021a5c`, observed 2026-10-17T10:30:33.706918Z. Rust CI 39022159446, job 117365955407, Format step 4 failed; bounded log lines 300–339 show formatting differences in `png_crc.rs`. Platform CI 39022159443 passed. The test also altered an IHDR width byte in a way that failed structural validation; the repair targets the legal interlace byte.

Ralph `commit_files` operation `e16d2811ba8d4e9e861ea8658e6c9d5d` requested 2026-10-17T10:31:00.634574Z; one Rust source upsert, exact-head CAS `aed3a19597384c4f1c4b4c5f3219d20cc4021a5c` -> `d5857f569ec010f7936e8b26caa3e57542927ba6`, accepted=true, reconciled=false, path receipt verified. Rust CI 39025195816 and Platform CI 39025195930 passed at that SHA. One 55-second CI observation timed out after 11 polls while workflows ran; not a CI failure.

Prior session reported five platform safety denials of repair/rollback attempts without Ralph operation IDs. This session's canonical-log append reconstructed all 860 lines in five bounded windows and attempted one 3,270-character addition; it was denied with `This tool call was blocked by OpenAI's safety checks. Please double check what you are sending.` No Ralph operation ID or GitHub diagnostic was returned. Independent master read at 2026-10-17T10:32:49.350237Z confirmed unchanged head `d5857f569ec010f7936e8b26caa3e57542927ba6`. A combined documentation commit and a subsequent update to the 103-line PACK-003 qualification document received the same pre-acknowledgment denial; each was reconciled by a read showing unchanged master. These are platform denials, not confirmed Bridge or GitHub API failures.

Canonical TODO remains 1,536 lines, original blob `ec594ed49cf4fff5e1c7ab42fca927881be55df5`. SRC-003 census remains blob `0191d49981243930fce5039d03225248068ac51d`. No private data or credentials recorded. PACK-003 is still unchecked.
