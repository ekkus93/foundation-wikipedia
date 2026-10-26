# SRC-003: Live official inventory + complete local generation integrity (2026-10-26)

Status: scoped source acquisition verification; **SRC-003 remains unchecked**.

Host-only read-only command:

```text
wiki-pack verify-public-generation-live [--json] PROJECT GENERATION INVENTORY_SHA256 PRIVATE_SOURCE_DIR MAX_MEMBER_BYTES
```

The command first verifies the entire private staged dump generation: independently pinned `SHA256SUMS`, exact declared compressed members, their complete SHA-256, byte bounds and a clean tree with no symlinks, hardlinks, unlisted entries or orphan checkpoints. Only after local verification succeeds does it fetch the official `SHA256SUMS` over the existing TLS-validated, no-redirect `dumps.wikimedia.org` transport and compare it with the pinned inventory. A changed or unavailable upstream inventory fails closed, as do missing/corrupt local members. It never downloads members, modifies the staging tree, installs packs or activates snapshots.

The optional JSON result is schema 3, kind `public_dump_live_https_and_local_integrity`. It preserves the full schema-v2 per-shard receipt and adds `official_https_inventory_retrieved` and `live_inventory_matches_local_pin` checks. **No successful receipt** is printed on failure, and `publisher_authenticated` remains `false`.

The HTTPS result is **observed official TLS origin at check time**, not a signed Wikimedia publication, an independently authenticated publisher statement or independent page-census completeness. Revision-matched rendering, offline visuals and device acceptance are still missing. As with previous private-stage verification, untrusted concurrent writers must be excluded.

Synthetic two-shard regression covers missing/corrupt local inputs failing before HTTP, live exact-match receipt, changed/failed HTTPS inventory, unchanged staged data, canonical member ordering and conservative trust claims.

## Verification evidence

The initial implementation at `f623d13e228f4fbf1d95091e97ced9411421af9a`
failed Rust tests because a fixture embedded literal backslash-n sequences
rather than inventory line terminators. Corrected implementation
`e867e0825bd348bb09464e211f4ae7464dfbe9f7` passed Rust workspace CI
[40322795806](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40322795806)
and Platform shells CI
[40322795810](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40322795810).

The actual CLI dispatch is also exercised by the Unix integration test at
`tools/wiki-pack/tests/verify_live_public_generation_cli.rs`. Its first SHA
`37b633939815ac8279f39c0d50c984bca45fafb5` failed rustfmt;
corrected exact code SHA `3a208bf3264184a3c69c38781a3a230eefa0051e`
passed [Rust workspace CI 40324004437](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40324004437)
and [Platform shells CI 40324004344](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40324004344).
The integration test checks success JSON/human mode, missing/corrupt members
before HTTP, live 404, bad pin, no success stdout on failures and immutable
staging. All HTTP observations are **synthetic**; no live publisher signature,
page census or revision-rendering claim is made.

Follow-up black-box cases also exercise a **synthetic HTTP 200 with a changed
upstream inventory** and a **synthetic transport failure after otherwise valid
response bytes/status**. Both must fail without a JSON success receipt and
without modifying the staged source. These are failure-path regressions only,
not evidence of real Wikimedia publication authenticity.

Extended black-box failure-path tests passed at source SHA `45efe09c6d7b312dd816ebb577c1eed54afd616e`: [Rust workspace CI 40336823014](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40336823014) and [Platform shells CI 40336823012](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40336823012). These CI passes do not qualify SRC-003 as complete.

## CLI staging-alias and transport-policy regression

The Unix black-box `verify_live_public_generation_cli` test at source SHA `352ad5e9b2c8edbdc0efa26dea31c859ddbd95eb` exercises the actual `wiki-pack verify-public-generation-live --json` process against a complete two-shard staged fixture. A fake curl marker proves that symlinked staging roots, symlinked or multiply hardlinked members and checksum inventories, orphan checkpoint files, undeclared directories, world-readable staging roots, and an incorrect independently supplied inventory pin all fail before any HTTP invocation. Each failure emits no JSON success receipt. After restoring the exact three declared files, the same fixture succeeds and captures curl argv to assert `-q`, HTTPS-only protocol, zero redirects, absence of insecure-TLS flags and the exact official Wikimedia checksum URL. No real Wikimedia HTTP request or signature verification occurs in this test.

Exact code SHA **`352ad5e9b2c8edbdc0efa26dea31c859ddbd95eb`** passed [Rust workspace CI 40342693738](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40342693738) and [Platform shells CI 40342693539](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40342693539). These regressions do not establish publisher authenticity, independent census completeness, rendered revision fidelity, mandatory offline media or desktop/Android acceptance. **SRC-003 remains unchecked.**
