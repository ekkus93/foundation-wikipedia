package org.foundation.wikipedia

import android.graphics.Color
import android.net.Uri
import android.view.View
import android.view.ViewGroup
import android.webkit.WebResourceRequest
import android.webkit.WebView
import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.UiDevice
import java.io.File
import java.security.MessageDigest
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.LibraryException

/**
 * PACK-003 on device/emulator: an article's figures are stored in the pack
 * and drawn by the reader with no network. The committed fixture pack holds
 * one article with two figures: a 32×32 solid red PNG and a 32×32 solid blue
 * SVG kept as a vector; the reader asks for them at the reserved origin and
 * the app answers from the active pack.
 */
@RunWith(AndroidJUnit4::class)
class PackImagesUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private lateinit var imageSha256: String
    private lateinit var svgSha256: String

    @Before
    fun installImagePack(): Unit = runBlocking {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val pack = File(context.cacheDir, "images-sample-v2.wpack")
        instrumentation.context.assets.open("android-images-v2.wpack").use { input ->
            pack.outputStream().use { input.copyTo(it) }
        }
        val library = OfflineLibraryHolder.get(context)
        try {
            library.installPack(pack, acceptCustomPack = true)
        } catch (_: LibraryException.AlreadyInstalled) {
            // A previous test in this process installed it.
        }
        library.activate("android-images")
        pack.delete()
        val images = library.openPage("enwiki", 3)?.images.orEmpty()
        imageSha256 = images["Red_square.png"] ?: error("the fixture article has no pack image")
        svgSha256 = images["Blue_square.svg"] ?: error("the fixture article has no pack SVG")
    }

    private fun webView(): WebView {
        var found: WebView? = null
        compose.runOnUiThread {
            fun search(view: View) {
                if (view is WebView) found = view
                if (view is ViewGroup) for (i in 0 until view.childCount) search(view.getChildAt(i))
            }
            search(compose.activity.window.decorView)
        }
        return found ?: error("no article WebView")
    }

    /** Pixels of the figures' red (212, 0, 0) and blue (0, 0, 212) on screen right now. */
    private fun figurePixels(): Pair<Int, Int> {
        val shot = InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
            ?: return 0 to 0
        var red = 0
        var blue = 0
        for (y in 0 until shot.height step 2) {
            for (x in 0 until shot.width step 2) {
                val pixel = shot.getPixel(x, y)
                if (Color.green(pixel) >= 30) continue
                if (Color.red(pixel) in 190..230 && Color.blue(pixel) < 30) red++
                if (Color.blue(pixel) in 190..230 && Color.red(pixel) < 30) blue++
            }
        }
        shot.recycle()
        return red to blue
    }

    /** Wait for a figure's pixels; on timeout, fail with what the reader showed. */
    private fun waitForFigure(label: String, drawn: (Pair<Int, Int>) -> Boolean) {
        try {
            compose.waitUntil(timeoutMillis = 30_000) { drawn(figurePixels()) }
        } catch (timeout: ComposeTimeoutException) {
            var state = ""
            compose.runOnUiThread {
                val view = webView()
                state = "title=${view.title} progress=${view.progress} scrollY=${view.scrollY} " +
                    "contentHeight=${view.contentHeight} viewHeight=${view.height}"
            }
            // The client is read on the UI thread; WebView itself asks it on
            // a background thread, as here.
            val view = webView()
            var client: android.webkit.WebViewClient? = null
            compose.runOnUiThread { client = view.webViewClient }
            val served = client!!.shouldInterceptRequest(
                view,
                object : WebResourceRequest {
                    override fun getUrl(): Uri = Uri.parse("https://foundation.invalid/media/$imageSha256")
                    override fun isForMainFrame() = false
                    override fun isRedirect() = false
                    override fun hasGesture() = false
                    override fun getMethod() = "GET"
                    override fun getRequestHeaders(): Map<String, String> = emptyMap()
                },
            )
            throw AssertionError(
                "$label not drawn: (red, blue) pixels ${figurePixels()}; $state; " +
                    "PNG request now answers ${served?.statusCode} ${served?.mimeType}; viewer open=${viewerShown()}",
                timeout,
            )
        }
    }

    /** The middle of the red figure on screen, to tap it. */
    private fun redCentre(): Pair<Int, Int> {
        val shot = InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()!!
        var sumX = 0L
        var sumY = 0L
        var count = 0L
        for (y in 0 until shot.height step 2) {
            for (x in 0 until shot.width step 2) {
                val pixel = shot.getPixel(x, y)
                if (Color.red(pixel) in 190..230 && Color.green(pixel) < 30 && Color.blue(pixel) < 30) {
                    sumX += x
                    sumY += y
                    count++
                }
            }
        }
        shot.recycle()
        return (sumX / count).toInt() to (sumY / count).toInt()
    }

    private fun viewerShown() =
        compose.onAllNodesWithTag(IMAGE_VIEWER_TAG).fetchSemanticsNodes().isNotEmpty()

    /** MOB-002: a tapped figure opens alone, full screen, and Close returns. */
    @Test
    fun aTappedFigureOpensFullScreenSurvivesRotationAndCloses() {
        openRightTriangle()
        waitForFigure("the red PNG figure") { it.first > 2_000 }
        assertTrue(!viewerShown())
        val (x, y) = redCentre()
        UiDevice.getInstance(InstrumentationRegistry.getInstrumentation()).click(x, y)
        compose.waitUntil(timeoutMillis = 15_000) { viewerShown() }
        // Only the tapped image is drawn there: red, and no blue SVG.
        compose.waitUntil(timeoutMillis = 30_000) { figurePixels().first > 2_000 }
        assertEquals(0, figurePixels().second)

        // The open image is kept when the activity is recreated (rotation).
        compose.activityRule.scenario.recreate()
        compose.waitUntil(timeoutMillis = 30_000) { viewerShown() && figurePixels().first > 2_000 }

        compose.onNodeWithText("Close").performClick()
        compose.waitUntil(timeoutMillis = 15_000) { !viewerShown() }
        var title: String? = null
        compose.runOnUiThread { title = webView().title }
        assertEquals("Right triangle", title)
    }

    private fun openRightTriangle() {
        compose.onNode(hasSetTextAction()).performTextInput("triangle")
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) {
            compose.onAllNodesWithText("Right triangle").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithText("Right triangle").performClick()
        compose.waitUntil(timeoutMillis = 30_000) {
            var title: String? = null
            compose.runOnUiThread { title = webView().title }
            title == "Right triangle"
        }
    }

    @Test
    fun anArticleFigureIsDrawnFromThePack() {
        openRightTriangle()

        // The PNG figure is drawn: a 220 CSS px red square, from the pack.
        // On CI this has twice timed out (f54aa7f4, e38bf53a) and otherwise
        // passed; a timeout now reports what was on screen.
        waitForFigure("the red PNG figure") { it.first > 2_000 }
        // The SVG figure below it, drawn as a vector, once scrolled into view.
        compose.runOnUiThread { webView().pageDown(true) }
        waitForFigure("the blue SVG figure") { it.second > 2_000 }

        // The app answers the image request with the verified pack bytes,
        // and nothing else at the reserved origin or elsewhere.
        fun request(url: String, mainFrame: Boolean = false) = object : WebResourceRequest {
            override fun getUrl(): Uri = Uri.parse(url)
            override fun isForMainFrame() = mainFrame
            override fun isRedirect() = false
            override fun hasGesture() = false
            override fun getMethod() = "GET"
            override fun getRequestHeaders(): Map<String, String> = emptyMap()
        }
        // WebView asks on a background thread, as here (this test thread).
        val view = webView()
        var client: android.webkit.WebViewClient? = null
        compose.runOnUiThread { client = view.webViewClient }
        val image = client!!.shouldInterceptRequest(
            view, request("https://foundation.invalid/media/$imageSha256"),
        )!!
        val mime = image.mimeType
        val bytes = image.data.readBytes()
        val csp = image.responseHeaders["Content-Security-Policy"]
        val svg = client!!.shouldInterceptRequest(
            view, request("https://foundation.invalid/media/$svgSha256"),
        )!!
        val svgMime = svg.mimeType
        val svgBytes = svg.data.readBytes()
        val statuses = listOf(
            request("https://foundation.invalid/media/${"ab".repeat(32)}"),
            request("https://foundation.invalid/media/$imageSha256", mainFrame = true),
            request("https://upload.wikimedia.org/wikipedia/commons/8/8a/Red_square.png"),
        ).map { client!!.shouldInterceptRequest(view, it)?.statusCode ?: 0 }
        assertEquals("image/png", mime)
        fun sha256(data: ByteArray) = MessageDigest.getInstance("SHA-256").digest(data)
            .joinToString("") { "%02x".format(it) }
        assertEquals(imageSha256, sha256(bytes))
        assertEquals(PACK_IMAGE_CSP, csp)
        assertEquals("image/svg+xml", svgMime)
        assertEquals(svgSha256, sha256(svgBytes))
        assertEquals(PACK_IMAGE_CSP, svg.responseHeaders["Content-Security-Policy"])
        assertTrue(statuses.toString(), statuses.all { it == 403 })
    }
}
