# MOBPACK-003 — audio and video as local previews (2026-11-26)

Code: `2716810c`. MOBPACK-003 remains unchecked.

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

## Not done

- **Playing in the app:** playing happens in the browser. The policy gate is in the app; streaming is not. An in-app player would make requests to Wikimedia's media hosts, which needs its own disclosure.
- **Audio metadata:** nothing of an audio file is in the pack, so its licence and creator are not stored; the note links to its Commons page.
- **Desktop:** not tried there; it has no offline message for outside links.
- **Physical phone:** not tried on the A54.
- **No hidden requests:** shown for the reader by its request handler tests; not measured with a network log on a device.
