package org.foundation.wikipedia

import android.content.Context
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.LibraryException

/**
 * MOB-004: the reader's Bookmark action saves the open article, Settings ›
 * History & Saved Content lists bookmarks and (only with history on) the
 * articles opened, and choosing one returns to the reader showing it.
 */
@RunWith(AndroidJUnit4::class)
class SavedContentUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private val consent get() =
        context.getSharedPreferences(ReaderOfflinePreference.FILE, Context.MODE_PRIVATE)
    private var savedConsent: String? = null

    @Before
    fun startEmpty(): Unit = runBlocking {
        val pack = File(context.cacheDir, "saved-sample-v2.wpack")
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
        savedConsent = consent.getString(ReaderHistoryPreference.KEY, null)
        val state = ReaderStateHolder.get(context)
        for (page in state.savedContent().bookmarks) state.removeBookmark(page.project, page.pageId.toLong())
        state.clearHistory()
        // Start with history off; the activity reads the choice when created.
        consent.edit().putString(ReaderHistoryPreference.KEY, ReaderHistoryPreference.encode(false)).commit()
        compose.activityRule.scenario.recreate()
    }

    @After
    fun restoreConsent() {
        consent.edit().putString(ReaderHistoryPreference.KEY, savedConsent).commit()
    }

    private fun shown(text: String) = compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    private fun open(query: String, title: String) {
        compose.onNode(hasSetTextAction()).performTextClearance()
        compose.onNode(hasSetTextAction()).performTextInput(query)
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) { shown(title) }
        compose.onAllNodesWithText(title).onFirst().performClick()
        compose.waitUntil(timeoutMillis = 30_000) {
            compose.onAllNodesWithContentDescription("Open reader actions").fetchSemanticsNodes().isNotEmpty() &&
                compose.onAllNodesWithContentDescription("Search results").fetchSemanticsNodes().isEmpty()
        }
        compose.waitForIdle()
    }

    private fun action(label: String) {
        compose.onNodeWithContentDescription("Open reader actions").performClick()
        compose.onNodeWithText(label).performClick()
    }

    private fun savedContentSettings() {
        action("Settings")
        compose.onNodeWithText("History & Saved Content").performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 10_000) { shown("Bookmarks") && !shown("Loading…") }
    }

    private fun back() {
        compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
    }

    @Test
    fun bookmarkingAndOpeningHistoryReturnToTheReader(): Unit = runBlocking {
        open("tide", "Tide")
        action("Bookmark")
        compose.waitUntil(timeoutMillis = 10_000) { shown("Bookmarked “Tide”.") }
        compose.onNodeWithContentDescription("Open reader actions").performClick()
        assertTrue("the action now removes it", shown("Remove bookmark"))
        compose.onNodeWithContentDescription("Close reader actions").performClick()

        // History is off: opening an article records nothing.
        savedContentSettings()
        assertTrue(shown("Tide"))
        assertTrue(shown("No saved history."))
        compose.onNode(isToggleable()).performClick() // turn history on
        compose.waitUntil(timeoutMillis = 10_000) { shown("No articles opened yet.") }
        back()
        back()
        open("moon", "Moon")
        val state = ReaderStateHolder.get(context)
        compose.waitUntil(timeoutMillis = 10_000) {
            runBlocking { state.savedContent().history.isNotEmpty() }
        }
        assertEquals(listOf(1uL), state.savedContent().history.map { it.pageId })

        // Choosing the bookmark returns to the reader showing it.
        savedContentSettings()
        assertTrue(shown("Moon"))
        compose.onAllNodesWithText("Tide").onFirst().performClick()
        compose.waitUntil(timeoutMillis = 30_000) {
            !shown("History & Saved Content")
        }
        compose.onNodeWithContentDescription("Open reader actions").performClick()
        compose.waitUntil(timeoutMillis = 10_000) { shown("Remove bookmark") }
        compose.onNodeWithContentDescription("Close reader actions").performClick()

        // Clearing history keeps bookmarks.
        savedContentSettings()
        compose.onNodeWithText("Clear history").performScrollTo().performClick()
        compose.onNodeWithText("Clear").performClick()
        compose.waitUntil(timeoutMillis = 10_000) { runBlocking { state.savedContent().history.isEmpty() } }
        assertFalse(runBlocking { state.savedContent().bookmarks.isEmpty() })
    }
}
