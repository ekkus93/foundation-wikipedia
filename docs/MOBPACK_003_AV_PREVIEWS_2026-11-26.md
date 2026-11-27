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
| Addresses | `AvStream` | Worked out from the file name the way Wikimedia lays out its media store (two directories from the MD5 of the name), so packs need no rebuild. Video: 480p, then 360p, then 240p VP9 renderings, then the file itself. Audio: the MP3 rendering, then the file itself. Every address is on `upload.wikimedia.org`. |
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

## Not done

- **Seeking:** the slider is not exercised by a test or by hand.
- **Desktop player:** the desktop app has no in-app player, and its offline message was checked by unit test only.
- **Physical phone:** the previews and the player have not been tried on the A54.
- **Audio metadata:** nothing of an audio file is in the pack, so its licence and creator are not stored; the note links to its Commons page.
- **No hidden requests:** shown for the reader by its request handler tests; not measured with a network log on a device.
- **Offline audio and video:** deferred (FUT-004). A size-limited option for short clips was discussed and left deferred.
