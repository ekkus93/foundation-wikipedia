package org.foundation.wikipedia

import android.net.Uri
import android.view.View
import android.view.ViewGroup
import android.webkit.WebResourceRequest
import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.LibraryException

/**
 * MOB-003 on device/emulator: a chat citation opens its exact article
 * revision in the reader, scrolled to the highlighted paragraph; a citation
 * of a revision the pack does not hold is refused and never highlighted.
 */
@RunWith(AndroidJUnit4::class)
class ChatCitationUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val tide = ArticleChatTarget("enwiki", 2, 7002, "Tide")
    private val tideLead = "wkb:enwiki:2:7002:b0"
    private val messages = CopyOnWriteArrayList<String>()
    private var shown: ArticleChatTarget? = null
    private var citation by mutableStateOf<CitationRequest?>(null)

    @Before
    fun installFixturePackAndShowReader(): Unit = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val pack = File(context.cacheDir, "citation-sample-v2.wpack")
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
        compose.setContent {
            ArticleReader(
                dark = false,
                hidden = false,
                hideFromAccessibility = false,
                onMessage = { message, _ -> messages += message },
                citation = citation,
                onCitationConsumed = { citation = null },
                onArticleShown = { shown = it },
            )
        }
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

    private fun currentUrl(): String? {
        var url: String? = null
        compose.runOnUiThread { url = webView().url }
        return url
    }

    /** The document the reader serves for its current main-frame URL. */
    private fun servedDocument(): String {
        var html = ""
        compose.runOnUiThread {
            val view = webView()
            val request = object : WebResourceRequest {
                override fun getUrl(): Uri = Uri.parse("https://foundation.invalid/wiki/Tide")
                override fun isForMainFrame() = true
                override fun isRedirect() = false
                override fun hasGesture() = false
                override fun getMethod() = "GET"
                override fun getRequestHeaders(): Map<String, String> = emptyMap()
            }
            html = view.webViewClient.shouldInterceptRequest(view, request)!!.data
                .bufferedReader().readText()
        }
        return html
    }

    private fun waitFor(what: String, condition: () -> Boolean) {
        try {
            compose.waitUntil(timeoutMillis = 30_000) { condition() }
        } catch (error: Throwable) {
            throw AssertionError("$what; url=${currentUrl()} messages=$messages", error)
        }
    }

    @Test
    fun aCitationShowsItsParagraphHighlightedInTheExactRevision() {
        compose.runOnUiThread { citation = CitationRequest(tide, tideLead, 1) }
        waitFor("the cited article did not open at its passage") {
            currentUrl() == "https://foundation.invalid/wiki/Tide#foundation-cited"
        }
        waitFor("the cited article did not finish loading") {
            var title: String? = null
            compose.runOnUiThread { title = webView().title }
            title == "Tide"
        }
        assertEquals(tide, shown)
        val html = servedDocument()
        assertTrue(
            html,
            html.contains("<div id=\"foundation-cited\" class=\"foundation-cited\"><p>Tides follow the"),
        )
        assertEquals(1, Regex("id=\"foundation-cited\"").findAll(html).count())
        assertTrue(messages.toString(), messages.isEmpty())
    }

    @Test
    fun aCitationOfAnotherRevisionIsRefusedAndNotHighlighted() {
        val stale = tide.copy(revisionId = 7001)
        compose.runOnUiThread { citation = CitationRequest(stale, "wkb:enwiki:2:7001:b0", 1) }
        waitFor("the stale citation was not refused") {
            messages.any { it.startsWith("The cited revision of") }
        }
        // The article itself still opens, without any highlight.
        waitFor("the article did not open after the refused citation") {
            currentUrl() == "https://foundation.invalid/wiki/Tide"
        }
        assertFalse(servedDocument().contains("id=\"foundation-cited\""))
    }
}
