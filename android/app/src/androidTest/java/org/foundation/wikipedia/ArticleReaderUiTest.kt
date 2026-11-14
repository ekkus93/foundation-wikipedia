package org.foundation.wikipedia

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
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.LibraryException

/**
 * MOB-002 on device/emulator: search the active offline pack, read an article
 * in the locked-down WebView, follow an in-pack link, go Back, and keep
 * external links and subresources from loading.
 */
@RunWith(AndroidJUnit4::class)
class ArticleReaderUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    @Before
    fun installFixturePack(): Unit = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val pack = File(context.cacheDir, "reader-sample-v2.wpack")
        InstrumentationRegistry.getInstrumentation().context.assets
            .open("android-sample-v2.wpack").use { input ->
                pack.outputStream().use { input.copyTo(it) }
            }
        val library = OfflineLibraryHolder.get(context)
        try {
            library.installPack(pack, acceptCustomPack = true)
        } catch (_: LibraryException.AlreadyInstalled) {
            // A previous test in this process installed it.
        }
        library.activate("android-sample")
        pack.delete()
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

    private fun waitForTitle(title: String) {
        compose.waitUntil(timeoutMillis = 15_000) {
            var current: String? = null
            compose.runOnUiThread { current = webView().title }
            current == title
        }
    }

    private fun tap(url: String): Boolean {
        val request = object : WebResourceRequest {
            override fun getUrl(): Uri = Uri.parse(url)
            override fun isForMainFrame() = true
            override fun isRedirect() = false
            override fun hasGesture() = true
            override fun getMethod() = "GET"
            override fun getRequestHeaders(): Map<String, String> = emptyMap()
        }
        var handled = false
        compose.runOnUiThread {
            val view = webView()
            handled = view.webViewClient.shouldOverrideUrlLoading(view, request)
        }
        return handled
    }

    @Test
    fun searchOpenFollowLinkAndGoBackWithoutNetwork() {
        compose.onNode(hasSetTextAction()).performTextInput("tide")
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) {
            compose.onAllNodesWithText("Tide").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithText("Tide").performClick()
        waitForTitle("Tide")

        // The in-pack link opens the linked article from the same pack.
        assertTrue(tap("https://foundation.invalid/wiki/Moon"))
        waitForTitle("Moon")

        // Back returns to the previous article.
        compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
        waitForTitle("Tide")

        // An external link is intercepted and offered, never loaded in place.
        assertTrue(tap("https://en.wikipedia.org/wiki/Tide"))
        compose.waitUntil(timeoutMillis = 5_000) {
            compose.onAllNodesWithText("This link leads outside the offline reader.")
                .fetchSemanticsNodes().isNotEmpty()
        }
        waitForTitle("Tide")

        // Every subresource request from the document is refused.
        var status = 0
        compose.runOnUiThread {
            val view = webView()
            val request = object : WebResourceRequest {
                override fun getUrl(): Uri = Uri.parse("https://upload.wikimedia.org/x.png")
                override fun isForMainFrame() = false
                override fun isRedirect() = false
                override fun hasGesture() = false
                override fun getMethod() = "GET"
                override fun getRequestHeaders(): Map<String, String> = emptyMap()
            }
            status = view.webViewClient.shouldInterceptRequest(view, request)?.statusCode ?: 0
        }
        assertEquals(403, status)
    }
}
