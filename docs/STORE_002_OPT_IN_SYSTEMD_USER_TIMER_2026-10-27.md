# STORE-002 — Opt-in Linux systemd user timer

Date: 2026-10-27  
Status: **partial qualification; STORE-002 remains unchecked**

## Scope

The `wiki-pack media-maintenance --run-if-due` command already implements a durable, bound-registry, exclusive-lease due gate. This change adds **installable but disabled-by-default** Linux `systemd --user` templates:

- `tools/wiki-pack/systemd/foundation-wikipedia-media-maintenance.service`
- `tools/wiki-pack/systemd/foundation-wikipedia-media-maintenance.timer`
- `tools/wiki-pack/systemd/media-maintenance.env.example`

The timer wakes hourly (with randomized delay and catch-up after missed calendar events). The SQLite success checkpoint, not the timer frequency, decides whether to sweep. Each invocation validates both scan budgets and the interval, requires a correctly bound **existing** ownership registry outside the media root, and uses the established guarded maintenance path. Failed/partial cleanup does not advance the checkpoint.

The service uses `Type=oneshot`, `UMask=0077`, `NoNewPrivileges=true`, a 30-minute timeout, and a **direct executable invocation without a shell**. `systemd` expands `${VAR}` as one argument, including a path containing spaces. The executable location is deliberately fixed to `%h/.local/bin/wiki-pack`; a different installation must edit the unit before enabling it.

## Manual opt-in on a Linux desktop

From a clean repository checkout, after building `wiki-pack` and auditing your existing media:

```bash
cargo build --release -p wiki-pack
install -Dm755 target/release/wiki-pack "$HOME/.local/bin/wiki-pack"
install -Dm644 tools/wiki-pack/systemd/foundation-wikipedia-media-maintenance.service \
  "$HOME/.config/systemd/user/foundation-wikipedia-media-maintenance.service"
install -Dm644 tools/wiki-pack/systemd/foundation-wikipedia-media-maintenance.timer \
  "$HOME/.config/systemd/user/foundation-wikipedia-media-maintenance.timer"
install -Dm600 tools/wiki-pack/systemd/media-maintenance.env.example \
  "$HOME/.config/foundation-wikipedia/media-maintenance.env"
${EDITOR:-vi} "$HOME/.config/foundation-wikipedia/media-maintenance.env"
```

Set `MEDIA_ROOT` and `REGISTRY_PATH` to **existing absolute paths**; no `~` or `$HOME` expansion occurs in the environment file. The SQLite registry must be outside the media object tree and must already be explicitly bound to that exact root. Set `MAX_REGISTERED` and `MAX_SCANNED` to integers 1–10,000 and `INTERVAL_SECONDS` to 60–2,592,000. The example defaults to a one-day successful-sweep interval and does **not** enable deletion.

Before enabling, use the CLI's read-only audit/preview and reconcile any unregistered orphan provenance. On an unbound legacy registry, run `media-maintenance --audit-unbound` and only bind after a clean audit. **Do not** bind an unrelated registry to a populated media tree or delete orphan bytes merely to force binding.

To enable **only after** validating the paths and ownership database:

```bash
systemctl --user daemon-reload
systemctl --user enable --now foundation-wikipedia-media-maintenance.timer
systemctl --user list-timers foundation-wikipedia-media-maintenance.timer
journalctl --user -u foundation-wikipedia-media-maintenance.service --no-pager -n 80
```

Disable immediately with:

```bash
systemctl --user disable --now foundation-wikipedia-media-maintenance.timer
```

A user timer ordinarily runs only while the user's systemd manager is running; no `loginctl enable-linger` is invoked automatically. A failed service remains visible in the journal and will be retried at the next timer activation. The 30-minute service timeout can interrupt cleanup; existing SQLite-first deletion and orphan recovery are designed to make such interruption recoverable, **not** to certify physical power-loss durability.

## Verification and limits

The Rust test `packaged_user_timer_is_opt_in_and_invokes_the_bounded_due_gate` asserts the checked-in unit contract, environment variables, calendar timer, `Persistent=true`, absence of a shell and that only the guarded `--run-if-due` path is called. Source commit `7f954ab72220242d9fa2cb117aea4b1b6c16fa96` failed formatting ([Rust 40570763638](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40570763638)). Corrected source `b33dd84faa28de5e0bd3a9943ad44507136784f6` passed [Rust workspace CI 40570964771](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40570964771); [Platform shells CI 40570964795](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40570964795) was still running at documentation.

A local `systemd-analyze verify` syntax probe was attempted on the equivalent templates using systemd 257. It reported the expected missing `~/.local/bin/wiki-pack` executable and unavailable manpage in the isolated environment; **no activated host timer was qualified**.

Remaining: live systemd user-session installation and repeated wake/suspend/resume verification, Android job scheduling, production installed-pack lifecycle, hostile same-user/unleased writer defenses, physical disk-full/power-loss injection, authoritative publisher media provenance, and desktop/Android end-to-end offline acceptance.

**STORE-002 must remain unchecked.**
