package org.foundation.wikipedia

import android.content.Context
import android.net.ConnectivityManager
import android.net.Uri
import android.view.View
import android.view.ViewGroup
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.Direction
import androidx.test.uiautomator.StaleObjectException
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import java.io.File
import java.security.MessageDigest
import java.util.regex.Pattern
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.LibraryException
import uniffi.wiki_ffi.PackTrust

/**
 * PACK-003 / MOBPACK-002 acceptance with a real desktop-built pack. Skipped
 * unless run with `-e realPack import` or `-e realPack offline`; CI never
 * runs it (it needs a pack built from Wikipedia, which is not committed).
 *
 * - `import` (with `-e realPackFile <name in Downloads>` and
 *   `-e realPackId <pack id>`): the pack is picked in the system file
 *   picker, reviewed as a custom pack (Custom — integrity verified), installed and activated.
 * - `offline` (after a force-stop, with the device in airplane mode, with
 *   `-e realPackId` and `-e realPackTitles "Title|Title|…"`): every listed
 *   article opens from search and every image its document shows is served
 *   from the pack, byte-for-byte, with no network.
 */
@RunWith(AndroidJUnit4::class)
class RealPackAcceptanceTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val arguments = InstrumentationRegistry.getArguments()
    private val phase: String? = arguments.getString("realPack")
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext

    private fun shown(text: String, substring: Boolean = false) =
        compose.onAllNodesWithText(text, substring = substring).fetchSemanticsNodes().isNotEmpty()

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

    private fun search(query: String, title: String) {
        compose.onNode(hasSetTextAction()).performTextReplacement(query)
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) { shown(title) }
        compose.onAllNodesWithText(title).onFirst().performClick()
        compose.waitUntil(timeoutMillis = 30_000) {
            var loaded: String? = null
            compose.runOnUiThread { loaded = webView().title }
            loaded == title
        }
    }

    @Test
    fun phase1ARealPackIsImportedThroughTheSystemFilePicker(): Unit = runBlocking {
        assumeTrue(phase == "import")
        val fileName = arguments.getString("realPackFile")!!
        val packId = arguments.getString("realPackId")!!
        val library = OfflineLibraryHolder.get(context)
        assertFalse(
            "remove $packId first: this run must import it",
            library.listPacks().any { it.snapshotId == packId },
        )
        // Settings are reached from an open article: use the small fixture.
        val sample = File(context.cacheDir, "real-pack-sample-v2.wpack")
        InstrumentationRegistry.getInstrumentation().context.assets
            .open("android-sample-v2.wpack").use { input ->
                sample.outputStream().use { input.copyTo(it) }
            }
        try {
            library.installPack(sample, acceptCustomPack = true)
        } catch (_: LibraryException.AlreadyInstalled) {
        }
        sample.delete()
        library.activate("android-sample")
        compose.activityRule.scenario.recreate()

        search("tide", "Tide")
        compose.waitUntil(timeoutMillis = 30_000) {
            compose.onAllNodesWithContentDescription("Open reader actions")
                .fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithContentDescription("Open reader actions").performClick()
        compose.waitUntil(timeoutMillis = 10_000) { shown("Settings") }
        compose.onNodeWithText("Settings").performClick()
        compose.onNodeWithText("Wikipedia & Offline Data").performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 15_000) { shown("Import a .wpack file") }
        compose.onNodeWithText("Import a .wpack file").performClick()

        // The system file picker (another app): pick the pack by its name.
        val device = UiDevice.getInstance(InstrumentationRegistry.getInstrumentation())
        // The picker may open on Recent files: go to Downloads through its
        // side menu when the pack is not listed.
        val file = device.wait(Until.findObject(By.text(fileName)), 8_000) ?: run {
            device.wait(Until.findObject(By.desc("Show roots")), 5_000)?.click()
            // The side menu animates in: a found entry can go stale.
            for (attempt in 1..5) {
                try {
                    device.wait(Until.findObject(By.text("Downloads")), 5_000)?.click()
                    break
                } catch (_: StaleObjectException) {
                    Thread.sleep(500)
                }
            }
            // A long Downloads list: scroll down to the pack.
            device.wait(Until.findObject(By.text(fileName)), 5_000)
                ?: device.wait(Until.findObject(By.res(Pattern.compile(".*:id/dir_list"))), 5_000)
                    ?.scrollUntil(Direction.DOWN, Until.findObject(By.text(fileName)))
        } ?: error("the file picker does not show $fileName, in Recent or in Downloads")
        file.click()

        // Verified in full before the review, which offers it as a custom pack.
        compose.waitUntil(timeoutMillis = 300_000) { shown("Install custom pack?") }
        assertTrue(shown("Pack: $packId"))
        assertTrue(
            compose.onAllNodesWithTag(packTrustTag(PackTrust.CUSTOM_INTEGRITY_VERIFIED))
                .fetchSemanticsNodes().isNotEmpty(),
        )
        compose.onNodeWithText("Install").performClick()
        compose.waitUntil(timeoutMillis = 300_000) {
            shown("Installed $packId. Activate it to read it offline.")
        }
        val installed = library.listPacks().single { it.snapshotId == packId }
        assertEquals(PackTrust.CUSTOM_INTEGRITY_VERIFIED, installed.trust)
        library.activate(packId)
        assertEquals(packId, library.activeSnapshot())
    }

    private fun request(url: String) = object : WebResourceRequest {
        override fun getUrl(): Uri = Uri.parse(url)
        override fun isForMainFrame() = false
        override fun isRedirect() = false
        override fun hasGesture() = false
        override fun getMethod() = "GET"
        override fun getRequestHeaders(): Map<String, String> = emptyMap()
    }

    @Test
    fun phase2EveryArticleImageIsServedFromThePackWithNoNetwork(): Unit = runBlocking {
        assumeTrue(phase == "offline")
        val packId = arguments.getString("realPackId")!!
        val titles = arguments.getString("realPackTitles")!!.split('|').filter { it.isNotBlank() }
        val connectivity = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        assertNull("turn airplane mode on first", connectivity.activeNetwork)

        val library = OfflineLibraryHolder.get(context)
        val pack = library.listPacks().single { it.snapshotId == packId }
        assertTrue(pack.active)
        assertEquals(PackTrust.CUSTOM_INTEGRITY_VERIFIED, pack.trust)

        val source = Regex("""<(?:img|source|image)\b[^>]*?\s(?:src|srcset|href|xlink:href)="([^"]*)"""")
        val media = Regex("""^https://foundation\.invalid/media/([0-9a-f]{64})$""")
        val report = StringBuilder()
        var total = 0
        for (title in titles) {
            // Lower case, so only the result (not the search box) has the title.
            search(title.lowercase(), title)
            val hit = library.search(title).first { it.title == title }
            val page = library.openReaderPage(hit.project, hit.pageId.toLong(), dark = false)!!
            val listed = library.openPage(hit.project, hit.pageId.toLong())!!.images
            val view = webView()
            var client: WebViewClient? = null
            compose.runOnUiThread { client = view.webViewClient }
            val shownImages = source.findAll(page.document).map { it.groupValues[1] }.toList()
            val byMime = sortedMapOf<String, Int>()
            for (url in shownImages) {
                val digest = media.matchEntire(url)?.groupValues?.get(1)
                    ?: throw AssertionError("$title shows an image not in the pack: $url")
                val response = client!!.shouldInterceptRequest(view, request(url))!!
                assertEquals("$title: $url", 200, response.statusCode)
                val bytes = response.data.readBytes()
                val sha = MessageDigest.getInstance("SHA-256").digest(bytes)
                    .joinToString("") { "%02x".format(it) }
                assertEquals("$title: $url", digest, sha)
                assertTrue("$title: $url is not listed", listed.containsValue(digest))
                byMime.merge(response.mimeType, 1, Int::plus)
            }
            total += shownImages.size
            report.append("$title: ${shownImages.size} images $byMime\n")
        }
        assertTrue("no article showed an image", total > 0)
        File(context.filesDir, "real-pack-acceptance.txt")
            .writeText("$packId offline, $total images\n$report")
    }
}
