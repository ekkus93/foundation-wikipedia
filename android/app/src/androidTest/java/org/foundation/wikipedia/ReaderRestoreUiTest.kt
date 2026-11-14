package org.foundation.wikipedia

import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.LibraryException

/**
 * MOB-004: after the activity is recreated (as after a configuration change
 * or process death, which restore the same saved state), the reader shows
 * the same article at the same scroll position.
 */
@RunWith(AndroidJUnit4::class)
class ReaderRestoreUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext

    @Before
    fun installFixturePack(): Unit = runBlocking {
        val pack = File(context.cacheDir, "restore-sample-v2.wpack")
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

    private fun <T> onWebView(block: (WebView) -> T): T? {
        var result: T? = null
        compose.runOnUiThread {
            fun search(view: View) {
                if (view is WebView) result = block(view)
                if (view is ViewGroup) for (i in 0 until view.childCount) search(view.getChildAt(i))
            }
            search(compose.activity.window.decorView)
        }
        return result
    }

    private fun scrollable(web: WebView) =
        (web.contentHeight * web.resources.displayMetrics.density).toInt() - web.height

    @Test
    fun recreationReturnsToTheSameArticleAtTheSameScrollPosition() {
        compose.onNode(hasSetTextAction()).performTextInput("moon")
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) {
            compose.onAllNodesWithText("Moon").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onAllNodesWithText("Moon").onFirst().performClick()
        compose.waitUntil(timeoutMillis = 30_000) {
            onWebView { it.title == "Moon" && scrollable(it) > 2 * it.height } == true
        }
        // Scroll half way down, as a reader would.
        onWebView { it.scrollTo(0, scrollable(it) / 2) }
        compose.waitForIdle()
        val before = onWebView { it.scrollY.toFloat() / scrollable(it) }!!
        assertTrue("$before", before in 0.45f..0.55f)

        compose.activityRule.scenario.recreate()
        compose.waitUntil(timeoutMillis = 30_000) {
            onWebView { it.title == "Moon" && it.scrollY > 0 } == true
        }
        val after = onWebView { it.scrollY.toFloat() / scrollable(it) }!!
        assertTrue("before $before, after $after", kotlin.math.abs(after - before) < 0.05f)
    }
}
