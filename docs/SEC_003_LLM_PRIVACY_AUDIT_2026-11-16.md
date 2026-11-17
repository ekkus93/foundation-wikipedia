# SEC-003 LLM privacy and credentials audit — 2026-11-16

Scope: the desktop app (Tauri) and the Android app (through `wiki-ffi`) as of
`master` on 2026-11-16. Question: can AI chat reach a hosted provider, or send
telemetry, when the user has chosen a local-only model?

## 1. Where network traffic can come from (static inventory)

| Source | Destination | When |
|---|---|---|
| `wiki-ai::hosted_http` | `api.anthropic.com`, `api.openai.com` (fixed hosts, port 443) | Only a hosted chat or *Test connection* the user reviewed and sent |
| `wiki-ai::local_http`, `local_http_stream` | The user-entered local server | Discovery, test, chat; loopback or private-network addresses only, DNS names rejected |
| `wiki-ffi::pack_download` / `official_packs` | GitHub release hosts (allowlisted redirect) | Android official packs, after the disclosure |
| Desktop reader (WebKit) | `*.wikipedia.org`, Wikimedia image hosts (window CSP) | Online reading, never while *Work offline* is on |
| Wikipedia source adapters (`wiki-source`, `tools/`) | Wikimedia | Pack building and live smoke tools, not the apps' chat |

There are no analytics, crash-reporting or telemetry clients in any crate, the
desktop app or the Android app. No app crate logs (`println!`, `eprintln!`,
`log`, `tracing`) or calls Android `Log`, so there is no log stream for a
secret or a question to leak into.

Hosted HTTPS opens connections in exactly one function,
`HostedChatProvider::connect`. Each request is bound at review time to one
`ChatTarget` (Hosted *or* Local); `send` contacts only that target, and a
local failure is returned to the user as an error. There is no code path that
retries a failed local request elsewhere.

## 2. Secrets

* Desktop keys: OS keychain via `keyring` (`desktop/src-tauri/src/hosted_ai_keys.rs`);
  Android keys: Android Keystore-encrypted storage, excluded from backup.
  Neither is written to settings files, the user-state database or packs.
* `ApiKey` has no `Display` and its `Debug` prints `ApiKey(<redacted>)`
  (`hosted_codec::tests::api_keys_are_redacted_and_header_safe`).
  `ChatError`/`HostedAiError` variants carry no key, passage or reply text.
* Deleting a key asks first on both platforms (STATE-003).

## 3. Endpoint clarity and disclosure

* Locality is classified as this device, local network or internet
  (`wiki-ai::local_selection` / `hosted_selection`) and shown in Settings and
  in every request review ("This will be sent to http://127.0.0.1:11434, on
  this device (model …, profile …)"; hosted reviews add an internet notice).
* Nothing is sent before the user reviews the exact passages and presses
  Send. Privacy & Network pages on both apps describe each destination
  (desktop `SettingsPages.tsx`, Android `PrivacyNetworkSection.kt`).

## 4. Dynamic evidence: local-only chat makes zero hosted connections

**Automated (CI):** `hosted_http::hosted_connection_attempts()` counts every
hosted connection attempt before name resolution; a hosted unit test checks the
counter rises when a hosted request is made. The separate test binary
`crates/wiki-ffi/tests/local_only_traffic.rs` (the API the Android app uses)
runs a local chat that is answered with a citation, one answered
`[NO_EVIDENCE]`, and one whose local server has gone away (error, no fallback),
then asserts the counter is still `0`.

**Live, desktop app:** the debug desktop app was run under an `LD_PRELOAD`
shim that logs every `connect()` and `getaddrinfo()` of the process and its
WebKit child processes (source below). With Work offline on, a saved local
Ollama profile (`qwen3.5:9b` at `http://127.0.0.1:11434`) and the sample pack
active, the user asked about *Tide*, reviewed the request and sent it; the
model answered "Tides follow the Moon's gravity. [1]" with a valid citation.
The whole session's log:

```
32 connect inet6 [::1]:1420        (WebKitNetworkProcess: Vite dev server, dev build only)
 3 connect inet 127.0.0.1:11434    (app process: 1 discovery, 2 during the chat)
 0 lookup                           (no DNS resolution at all)
```

No connection to any hosted provider or other host was made. A control run of
the shim against `curl https://api.anthropic.com/` logged the lookup and the
TCP connects, so hosted traffic would have been visible.

Not covered live: the Android app (no packet capture on the emulator image);
it uses the same `wiki-ffi` chat code verified by the automated test above.

Shim used (`gcc -shared -fPIC -O2 -o netlog.so netlog.c -ldl`, run with
`NETLOG_FILE=… LD_PRELOAD=…/netlog.so`):

```c
#define _GNU_SOURCE
#include <dlfcn.h>
#include <netdb.h>
#include <arpa/inet.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
static void out(const char *line) {
  const char *p = getenv("NETLOG_FILE");
  if (!p) return;
  int fd = open(p, O_WRONLY | O_APPEND | O_CREAT, 0600);
  if (fd < 0) return;
  (void)!write(fd, line, strlen(line));
  close(fd);
}
int connect(int s, const struct sockaddr *a, socklen_t l) {
  static int (*real)(int, const struct sockaddr *, socklen_t);
  if (!real) real = dlsym(RTLD_NEXT, "connect");
  char b[300], ip[INET6_ADDRSTRLEN] = "";
  if (a && a->sa_family == AF_INET) {
    const struct sockaddr_in *v = (const void *)a;
    inet_ntop(AF_INET, &v->sin_addr, ip, sizeof ip);
    snprintf(b, sizeof b, "%d connect inet %s:%d\n", getpid(), ip, ntohs(v->sin_port));
    out(b);
  } else if (a && a->sa_family == AF_INET6) {
    const struct sockaddr_in6 *v = (const void *)a;
    inet_ntop(AF_INET6, &v->sin6_addr, ip, sizeof ip);
    snprintf(b, sizeof b, "%d connect inet6 [%s]:%d\n", getpid(), ip, ntohs(v->sin6_port));
    out(b);
  }
  return real(s, a, l);
}
int getaddrinfo(const char *n, const char *sv, const struct addrinfo *h, struct addrinfo **r) {
  static int (*real)(const char *, const char *, const struct addrinfo *, struct addrinfo **);
  if (!real) real = dlsym(RTLD_NEXT, "getaddrinfo");
  char b[400];
  snprintf(b, sizeof b, "%d lookup %s\n", getpid(), n ? n : "(null)");
  out(b);
  return real(n, sv, h, r);
}
```

## 5. Findings

No hidden cloud fallback, telemetry or secret logging was found. No code
change was needed beyond the audit counter and its tests.
