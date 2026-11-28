# MOB-002 — full-screen image viewer on Android (2026-11-26)

Code: `ab18579f` (viewer), `f1dd65b6` (test failure report). MOB-002 remains unchecked.

## What was built

- **Tap to open:** tapping an image in an article opens it alone, full screen. The reader runs no scripts, so the tap is read from the WebView's own hit test (`tappedImage`), a moment after the finger lifts (`onSingleTapConfirmed`): on a very quick tap the hit test is not ready when the finger lifts. A linked image opens the viewer and its link is not followed.
- **Viewer (`ImageViewer.kt`):** a second WebView, locked down like the article's (no scripts, storage, file or content access, or network). Its client answers only the viewer page (`https://foundation.invalid/viewer/<sha256>`) and that one pack image; every other request gets 403. Pinch and double-tap zoom are switched on, so an SVG stays sharp when enlarged.
- **Page (`ReaderLinks.viewerDocument`):** one `<img>`, a `default-src 'none'; img-src https://foundation.invalid` policy, white ground in both themes (diagrams are often black lines on transparency). A digest that is not 64 lowercase hex digits is refused, so nothing can break out of the attribute.
- **Closing:** a Close button and Android Back. The open image is saved state, so it survives the activity being recreated (rotation).

## A fault found on the way

The first version sized the image by height (`height:100%`, then `100vh`). In this WebView those resolved to nothing and the image was laid out with no height: the page loaded and the image was fetched (status 200) but nothing showed. The image is now sized by width only.

## Tests

- **JVM:** `ReaderLinksTest.theViewerPageShowsOneExactPackImageAndCanLoadNothingElse`.
- **Emulator (API 35):** `PackImagesUiTest.aTappedFigureOpensFullScreenSurvivesRotationAndCloses` taps the red figure, waits for the viewer, checks the red image is drawn and the article's blue SVG is not, recreates the activity, and closes. Full suite `OK (44 tests)`.
- **Samsung Galaxy A54 (Android 16):** the Eiffel Tower photograph from the real pack opened full screen and Back returned to the article at the same place. Suite: 42 of 44 passed, including both image tests; the two failures were the hosted-AI tests, which could not resolve `api.anthropic.com` because the phone was in airplane mode.

## Real rotation (2026-11-27, `a392a1fe`)

`aTappedFigureOpensFullScreenSurvivesRotationAndCloses` now turns the device itself to landscape and back (`UiDevice.setOrientationLeft`, then `setOrientationNatural`) instead of recreating the activity. After each turn the viewer must still be open and still drawing the red image. The test restores the natural orientation and unfreezes rotation even if it fails. It passed on the A54 (`PackImagesUiTest` `OK (2 tests)`, 14 s); the phone's rotation settings were unchanged afterwards. The test makes its own pack active, so the phone's previous active pack (`android-sample`) was restored by hand. CI on `a392a1fe`: all four workflows passed, incl. [Android FFI bridge 46026537810](https://github.com/ekkus93/foundation-wikipedia/actions/runs/46026537810) (`OK (48 tests)`, so the real turn also passes on the CI emulator) and [Rust workspace 46026538196](https://github.com/ekkus93/foundation-wikipedia/actions/runs/46026538196).

## CI

- **`ab18579f`:** Rust workspace, Platform shells and Android install smoke passed. [Android FFI bridge 45943533852](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45943533852) failed: both `PackImagesUiTest` tests timed out waiting for the article's red figure, and the test's failure report then crashed (it read the WebView client off the UI thread), so the log does not say what was on screen.
- **`f1dd65b6`** (failure report fixed, no app change): [Rust workspace 45944573722](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45944573722), [Platform shells 45944573639](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45944573639), [Android install smoke 45944573650](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45944573650), [Android FFI bridge 45944573640](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45944573640) (`OK (44 tests)`).

The "figure not drawn within 30 s" timeout has now happened on CI in three runs (`f54aa7f4`, `e38bf53a`, `ab18579f`) and never locally. Its cause is unknown; the next occurrence will report the page state.

## Not verified

- **Pinch zoom by hand:** confirmed working by the owner on the A54 (2026-11-28): an image opened full screen zooms with a two-finger pinch.
- **Online reading on Android:** not built (needs a rustls Wikipedia transport).
