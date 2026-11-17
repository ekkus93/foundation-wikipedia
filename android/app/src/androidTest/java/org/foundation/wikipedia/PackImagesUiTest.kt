package org.foundation.wikipedia

import android.graphics.Color
import android.net.Uri
import android.view.View
import android.view.ViewGroup
import android.webkit.WebResourceRequest
import android.webkit.WebView
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
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
 * PACK-003 on device/emulator: an article's figure is stored in the pack and
 * drawn by the reader with no network. The committed fixture pack holds one
 * article whose figure is a 32×32 solid red PNG; the reader asks for it at
 * the reserved origin and the app answers from the active pack.
 */
@RunWith(AndroidJUnit4::class)
class PackImagesUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private lateinit var imageSha256: String

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
        val png = library.openPage("enwiki", 3)?.images?.get("Red_square.png")
        imageSha256 = png ?: error("the fixture article has no pack image")
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

    /** Pixels of the figure's red (212, 0, 0) on screen right now. */
    private fun redPixels(): Int {
        val shot = InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
            ?: return 0
        var count = 0
        for (y in 0 until shot.height step 2) {
            for (x in 0 until shot.width step 2) {
                val pixel = shot.getPixel(x, y)
                if (Color.red(pixel) in 190..230 && Color.green(pixel) < 30 && Color.blue(pixel) < 30) count++
            }
        }
        shot.recycle()
        return count
    }

    @Test
    fun anArticleFigureIsDrawnFromThePack() {
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

        // The figure is drawn: a 220 CSS px red square, from the pack.
        compose.waitUntil(timeoutMillis = 15_000) { redPixels() > 2_000 }

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
        val statuses = listOf(
            request("https://foundation.invalid/media/${"ab".repeat(32)}"),
            request("https://foundation.invalid/media/$imageSha256", mainFrame = true),
            request("https://upload.wikimedia.org/wikipedia/commons/8/8a/Red_square.png"),
        ).map { client!!.shouldInterceptRequest(view, it)?.statusCode ?: 0 }
        assertEquals("image/png", mime)
        val digest = MessageDigest.getInstance("SHA-256").digest(bytes)
            .joinToString("") { "%02x".format(it) }
        assertEquals(imageSha256, digest)
        assertTrue(statuses.toString(), statuses.all { it == 403 })
    }
}
