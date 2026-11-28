# MOBPACK-003 — audio and video: local previews and an in-app player (2026-11-26)

Code: `2716810c` (previews), `e72344dc`–`9a0ca9d0` (in-app player). MOBPACK-003 remains unchecked.

## Before

Both readers removed every audio and video element and kept only the caption (`audio_and_video_never_stream_from_either_sanitizer`). The spec (§10) asks for a local preview and metadata, a message when offline, and playing only after a tap, under the cellular/Wi-Fi policy.

## What was built

| Part | Code | Behaviour |
|---|---|---|
| Preview | `wiki_source::av_preview` | Each player MediaWiki renders (`<video>`/`<audio>` naming its file in `data-mwtitle`) becomes a note: kind, length (from `data-durationhint`) and a "Play online (needs Internet)" link to the file's page on Wikimedia Commons. A video also keeps its still, only if the poster is Wikimedia's own rendering of that same file. A player without a usable file name is left for the sanitiser, which removes it. Runs before sanitising in both readers; stored (already sanitised) articles read the same. |
| Still in the pack | `rendered_article`, `rendered_media` | A video's still is a required image, fetched by the existing image pipeline as Wikimedia's rendering, and recorded with `is_av_preview`. Image metadata now accepts a video file's type (`video/*`, `application/ogg`); the stored type is the still's. |
| Tap on Android | `OutsideLinkPolicy.kt` | An outside link is offered for the browser only when it could work. In Offline mode or with no network the reader says a connection is needed. Audio and video also follow the Wi-Fi-only setting; other links do not. |
| Keyboard | `ArticleReader.kt` | Opening an article leaves the search box, so the keyboard closes. It used to stay open over half the article. |
| Desktop | `readerLayout.ts` | The note's style only. |

No audio or video bytes are stored or requested by the app.

## Real run

`build-public-dump-pack` on simplewiki 2026-11-01, articles *Latin* (two audio, two video) and *Movie* (one video): 10 images, 5.0 MB, pack SHA-256 `20a0cde7…85e1adeb`. The audit lists three video stills (97 KB, 259 KB, 50 KB JPEG), each marked as a preview.

The first attempt failed with `image metadata rejected: InvalidResponse`: the metadata parser accepted only `image/*` and Wikimedia states `video/webm` for the file. Fixed as above.

API 35 emulator (`-gpu swangle_indirect`), pack imported through the file picker with airplane mode on:

- `RealPackAcceptanceTest` offline: Latin 8 images, Movie 2, each served from the pack.
- *Movie* shows the still and "Video · 1:35:53 · Play online (needs Internet)".
- Tap in airplane mode: "Playing this needs an Internet connection." Nothing is offered.
- Tap with a network: "This plays on Wikimedia Commons, in your browser." with Open in browser.

## Tests

- `av_preview` (5): still plus note with length and Commons link; audio is a note only; players without a usable name are left alone; a poster of another file or host is not shown and odd lengths are not stated; names are escaped and encoded.
- `reader_document::audio_and_video_players_become_local_previews_with_a_play_online_link`: online, from a pack, and from stored HTML.
- `rendered_article::a_video_requires_its_still_as_an_av_preview`.
- `OutsideLinkPolicyTest` (3, JVM).
- Workspace 1,372 tests; emulator suite `OK (44 tests)`; desktop build, 46 Rust tests and lint; 654 script tests.

## CI on `2716810c`

[Rust workspace 45948230816](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45948230816), [Platform shells 45948230846](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45948230846), [Android FFI bridge 45948230810](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45948230810) (`OK (44 tests)`), [Android install smoke 45948230840](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45948230840), [Android Rust ABI qualification 45948230866](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45948230866), [Desktop launch smoke 45948230856](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45948230856).

## In-app player (`e72344dc`, `54a6aa52`, `4ca51ac0`, `9a0ca9d0`)

Owner decision (2026-11-26): play audio and video inside the app, not in the browser. The owner also accepted that audio and video stay online only: packing all of it is too much, and nothing may depend on Wikimedia's servers being up. Images are the part that must always be offline.

| Part | Code | Behaviour |
|---|---|---|
| Notice | `AvStream.kt` (`AV_STREAM_DISCLOSURE`), `MainActivity.kt` | The first tap on "Play online" shows "Play from Wikimedia?" before any request. It says the app connects to `upload.wikimedia.org`, which sees the IP address and the file played, and that nothing is saved. Playing starts only once the choice is stored. |
| Addresses | `AvStream` | Worked out from the file name the way Wikimedia lays out its media store (two directories from the MD5 of the name), so packs need no rebuild. Video: the 360p MPEG-4 rendering, then 480p, 360p and 240p VP9 renderings, then the file itself (order changed on 2026-11-27, see below). Audio: the MP3 rendering, then the file itself. Every address is on `upload.wikimedia.org`. |
| Player | `AvPlayer.kt` | A full-screen `VideoView` with Play/Pause, a position slider, elapsed and total time, and Close (Back also closes). Playback stops when it closes; nothing is saved. An address that fails, or has not started within 30 seconds, gives way to the next; when none is left the reader is told "This could not be played here." and offered the browser. |
| Policy | `OutsideLinkPolicy.kt` | Unchanged gate, applied before the notice: Offline mode, no network and the Wi-Fi-only setting each refuse with the reason. |
| Privacy page | `PrivacyNetworkSection.kt` | Now states that audio and video stream from Wikimedia's media servers after the notice, over mobile data only if allowed. |
| Files not on Commons | `av_preview` | A file kept on the wiki itself gets the note without a link ("online only"). |
| Desktop | `articleLinks.ts`, `OnlineReader.tsx` | No in-app player. When offline, an outside link gives "Playing this needs an Internet connection." (or "This link needs…") instead of offering the browser. |

**Checked on the API 35 emulator:**

- By hand: the notice appeared; after Play, *Night of the Living Dead* (1:35:52) streamed, showing 0:12 elapsed in the screenshot.
- `AvPlayerLiveTest` (opt-in, `-e avLive true`, skipped in CI) streams a real audio clip (1:28) and a real video clip (2:33): each plays, pauses, stays at the same position while paused, and resumes.
- `AvPlayerUiTest`: with addresses that cannot be played the reader is told and offered the browser. It uses files that do not exist, so it contacts no server.
- JVM: `AvStreamTest` (3) checks the derived addresses against ones confirmed on Wikimedia on 2026-11-26, and that every address stays on the media host whatever the file name. Desktop: one new test (106 pass).
- Full suite `OK (48 tests)`; workspace 1,373 tests.

**CI:**

- `e72344dc` (player): Android FFI bridge [45952726121](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45952726121) failed with 2 of 46 tests. The player's failure test waited for an unresolvable host to fail, which on CI took over 60 seconds; this also showed the player could wait indefinitely on a stalled connection. `StreamedDraftUiTest.theChatSheetPassesAccessibilityChecksAtEveryHeightAndWhileDrafting` timed out opening an article in the same run.
- `54a6aa52` (stalled-stream limit; failure test without a network): [Rust workspace 45954339622](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45954339622), [Platform shells 45954339592](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45954339592), [Android install smoke 45954339657](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45954339657), [Android FFI bridge 45954339568](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45954339568) (`OK (46 tests)`; the chat-sheet test passed, so its earlier timeout is recorded as a one-off).
- `4ca51ac0` (desktop offline message): [Rust workspace 45954519127](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45954519127), [Platform shells 45954519204](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45954519204), [Desktop launch smoke 45954519115](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45954519115).
- `9a0ca9d0` (live test, skipped in CI): [Rust workspace 45955098390](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45955098390), [Platform shells 45955098374](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45955098374), [Android install smoke 45955098572](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45955098572), [Android FFI bridge 45955098544](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45955098544) (`OK (48 tests)`).

## On the Galaxy A54 (2026-11-27)

`AvPlayerLiveTest` on the owner's A54 (Android 16), over Wi-Fi:

- **Audio:** passed first time. The MP3 rendering was ready in 0.3 s; it played, paused, held its position and resumed.
- **Video:** failed. The 480p VP9 WebM never became ready. The media server log shows why: Samsung's WebM reader alternates between the end of the 5.6 MB file (offset about 5.38 MB) and a position that advances about 300 KB at a time. Every jump is a new HTTP range request, about 3 s each, about 34 jumps in 47 s. It was slowly progressing, not stalled, but the player's 30 s no-start limit gave way to the next rendering, and that did the same. The addresses were all fine from a computer (HTTP 206).
- **Fix:** Commons also makes a 360p MPEG-4 rendering (`.360p.mpeg4.mov`). Its index is at the start of the file (`ftyp` then `moov`), and every Android device decodes it. It is now tried first, with the VP9 renderings as fallbacks. It is larger (12.2 MB against 5.6 MB for this clip), and audio and video stream over Wi-Fi only unless mobile data is allowed.
- **After the fix:** both tests passed on the A54 in 16 s; the video was ready in 1.1 s from the MPEG-4 rendering. Both also passed on the API 35 emulator.
- **CI:** `e2c3ca33` (includes `e4963cec`): all four workflows passed, incl. [Android FFI bridge 46024259175](https://github.com/ekkus93/foundation-wikipedia/actions/runs/46024259175) (`OK (48 tests)`).
- **Seeking (`f4b1b345`):** the live test now also moves the position slider to halfway, through its accessibility action. Each clip must then report a position between 45% and 60% of its length and keep advancing from there. It passed on the A54 for both the audio and the video (27 s); it was not rerun on the emulator. Dragging the slider by hand has not been tried.

## Desktop player

The desktop reader now plays a "Play online" file inside the app, as Android does.

- **Shared code:** `wiki-source::av_stream` parses and validates the Commons page link (same rules as `AvStream.kt`: https only, host exactly `commons.wikimedia.org`, no port, userinfo or query, `/wiki/File:`, name at most 255, no control characters or `/#|<>"?`, the same extension lists) and derives the `upload.wikimedia.org` addresses from the MD5 of the name (`md-5` crate, RustCrypto like `sha2`). Its tests use the addresses checked against Wikimedia for the Android test, and reject traversal, encoded slashes, control characters and other hosts.
- **Order for desktop:** desktop webviews stream WebM well and may not decode MPEG-4 Part 2, so video tries `480p.vp9.webm`, `360p.vp9.webm`, `240p.vp9.webm`, then `360p.mpeg4.mov`, then the original. Audio tries the `mp3` rendering (not for an MP3 file), then the original. Android keeps its own order.
- **Command and CSP:** the `av_stream` Tauri command only works the addresses out; Rust makes no request. The CSP gained `media-src https://upload.wikimedia.org` and nothing else.
- **Player:** `AvPlayer.tsx` uses a plain `<video>` or `<audio>` element with its own Play/Pause, a position slider, an `m:ss / m:ss` clock and Close (Escape also closes). An address that errors, or has not loaded within 30 s, gives way to the next; when none is left it shows "This could not be played here." with a button to open the file's page in the browser (the existing `open_external_link`).
- **First use:** a notice (the Android wording) is shown before the first contact with Wikimedia's media servers; playing starts only once it is accepted, and the choice is kept in `localStorage` (`foundation-wikipedia.avStreamDisclosure`, value `v1:accepted`), like the other small desktop preferences. If storage fails, the notice is shown again next time.
- **Offline:** with offline mode on or no connection, the reader still says "Playing this needs an Internet connection." and the player and command are never reached.
- **Tests:** `desktop/scripts/avPlayer.test.mjs` (clock, candidate stepping, disclosure value, address check), the `av_stream` unit tests in `desktop/src-tauri`, and the `wiki-source` tests.
- **Limits:** which codecs play depends on the operating system's webview (WebView2, WebKitGTK, WKWebView); a WebKitGTK without GStreamer's VP9 plugins may fail every rendering and fall to the "could not be played here" offer. Not tried by hand in a window. WebM seeking over HTTP may be as slow as on Android's Samsung player; the 30 s fall-through applies equally.

## Desktop player CI (2026-11-27)

`170800ae`…`421aa22f` (player) plus `cae2479d`: Work offline, or a lost connection, closes the player at once, so nothing more is fetched. Exact-head CI on `cae2479d`: all seven workflows passed, incl. [Desktop launch smoke 46027659784](https://github.com/ekkus93/foundation-wikipedia/actions/runs/46027659784), [Rust workspace 46027659679](https://github.com/ekkus93/foundation-wikipedia/actions/runs/46027659679) and [Android FFI bridge 46027659689](https://github.com/ekkus93/foundation-wikipedia/actions/runs/46027659689) (`OK (48 tests)`).

## Desktop player in a real window (2026-11-27)

Tauri dev build of `c7dd6ab9` on Linux (WebKitGTK), on a virtual X display (Xvfb), with live Wikipedia. The desktop app's data folder was backed up first and restored byte for byte afterwards.

- **Video:** enwiki *Night of the Living Dead* shows the film's still and "Video · 1:35:53 · Play online (needs Internet)".
  - The first click showed the notice, with the same wording as Android. After "Accept and play" the film was playing in the app within about 20 s (0:20 / 1:35:52).
  - Pause stopped it at 0:28, with the button changing to Play. Clicking halfway along the slider and pressing Play resumed at 49:24, still playing.
  - Escape closed the player and returned to the article.
- **Offline:** with Work offline on, the same link gave "Playing this needs an Internet connection." No notice or player appeared.
- **Audio:** simplewiki *Latin*, "Audio · 1:14 · Play online". The compact audio player opened straight away, because the notice is shown once. *Pronunciation Latin (Specials).ogg* was playing (0:11 / 1:13), and Close closed it.
- **Found on the way:** an enwiki musical score (*Gymnopédies*) has its own generated recording ("download the audio file"). That is not a Commons file, so it gets no "Play online" note and is treated as an ordinary outside link.

## Not done

- **Desktop player on Windows and macOS:** played in a real window on Linux (WebKitGTK) only, see above. WebView2 and WKWebView may differ in the formats they play. Which address played was not logged. Closing the player when Work offline is switched on while it is open was not tried by hand, because the player covers that switch.
- **Physical phone:** the player was qualified on the A54 by `AvPlayerLiveTest` (above). The previews in a real pack, and the first-use notice, have not been tried by hand there.
- **Audio metadata:** nothing of an audio file is in the pack, so its licence and creator are not stored; the note links to its Commons page, which carries both. A pack distributes no audio, so the licence asks nothing of it. A video is credited through its still, which is the video's own Commons file. Storing credit-only records would change the pack contract: every media record must have its bytes, and the builder and installer refuse one without. So this is left as is unless the owner wants it.
- **No hidden requests:** measured on the A54 on 2026-11-28, see "Network use on the A54" below. Only the app's own byte counters were read; individual connections and their hosts were not logged (that needs a rooted phone or a proxy).
- **Offline audio and video:** deferred (FUT-004). A size-limited option for short clips was discussed and left deferred.

## Network use on the A54 (2026-11-28, build `4010c32e`)

Android counts the bytes each app sends and receives. The app's counters were read with `dumpsys netstats --poll` and `dumpsys netstats detail` (untagged totals for its UID) before and after each instrumented test class, run one class at a time on the A54 over Wi-Fi. At rest the counters did not move.

| Test classes | Received | Sent |
|---|---|---|
| All 17 classes that read packs, show articles and images, open the player UI, save content, restore the reader, check trust, use local AI and run the Rust offline library (31 tests) | 0 | 0 |
| `RustHostedAiTest` (5): a real TLS handshake with api.anthropic.com using a deliberately invalid key | 9,808 B | 3,024 B |
| `RustHostedChatTest` (3): a consented question to the provider, refused for its invalid key | 5,524 B | 2,431 B |
| `AvPlayerLiveTest` with `avLive=true` (2): Commons audio and video streamed, paused, seeked (positive control) | 16.5 MB | 130 KB |

The live and official-pack download tests skip themselves without their opt-in flags, which is why they show 0 in the first row. The positive control shows the counters capture the app's streaming traffic. Offline reading made no network traffic at all, and the only other traffic came from the two hosted-AI tests, which connect on purpose. The full suite also passed in one run on the A54 with this build: `OK (48 tests)` in 94 s. Afterwards the active pack was still `android-sample` and no staging was left behind.
