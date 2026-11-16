package org.foundation.wikipedia

import android.content.Context
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.LibraryException

/**
 * MOBPACK-001: no official-pack request is made before the user accepts the
 * GitHub privacy disclosure, and none while Offline mode is on. The test
 * keeps Offline mode on, so it never touches the network.
 */
@RunWith(AndroidJUnit4::class)
class OfficialPacksUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private val prefs get() =
        context.getSharedPreferences(ReaderOfflinePreference.FILE, Context.MODE_PRIVATE)
    private val keys = listOf(
        ReaderOfflinePreference.KEY,
        OfficialPackPreference.DISCLOSURE_KEY,
        OfficialPackPreference.MOBILE_DATA_KEY,
    )
    private var saved: Map<String, String?> = emptyMap()

    @Before
    fun offlineWithoutDisclosure(): Unit = runBlocking {
        saved = keys.associateWith { prefs.getString(it, null) }
        val pack = File(context.cacheDir, "official-ui-sample-v2.wpack")
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
        prefs.edit()
            .putString(ReaderOfflinePreference.KEY, ReaderOfflinePreference.encode(true))
            .remove(OfficialPackPreference.DISCLOSURE_KEY)
            .remove(OfficialPackPreference.MOBILE_DATA_KEY)
            .commit()
        compose.activityRule.scenario.recreate()
    }

    @After
    fun restore() {
        val editor = prefs.edit()
        for ((key, value) in saved) {
            if (value == null) editor.remove(key) else editor.putString(key, value)
        }
        editor.commit()
    }

    private fun shown(text: String, substring: Boolean = false) =
        compose.onAllNodesWithText(text, substring = substring).fetchSemanticsNodes().isNotEmpty()

    private fun openOfflineDataSettings() {
        compose.onNode(hasSetTextAction()).performTextInput("tide")
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) { shown("Tide") }
        compose.onAllNodesWithText("Tide").onFirst().performClick()
        compose.waitUntil(timeoutMillis = 30_000) {
            compose.onAllNodesWithContentDescription("Open reader actions")
                .fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithContentDescription("Open reader actions").performClick()
        compose.onNodeWithText("Settings").performClick()
        compose.onNodeWithText("Wikipedia & Offline Data").performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 15_000) { shown("Check for official packs") }
    }

    @Test
    fun theDisclosureComesFirstAndOfflineModeBlocksTheRequest(): Unit = runBlocking {
        // A catalog stored by an earlier (live) run is fine; it must not change.
        val storedBefore = OfflineLibraryHolder.get(context).officialCatalog()
        openOfflineDataSettings()
        if (storedBefore == null) assertTrue(shown("No official pack list yet."))
        assertTrue("Wi-Fi only by default", shown("Download over mobile data"))
        assertFalse(
            OfficialPackPreference.mobileDataAllowed(
                prefs.getString(OfficialPackPreference.MOBILE_DATA_KEY, null),
            ),
        )

        // Declining the disclosure does nothing.
        compose.onNodeWithText("Check for official packs").performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 10_000) { shown("Download from GitHub?") }
        assertTrue(shown("IP address", substring = true))
        compose.onNodeWithText("Not now").performClick()
        compose.waitForIdle()
        assertFalse(shown("Download from GitHub?"))
        assertFalse(shown("Checking for official packs…"))
        assertNull(prefs.getString(OfficialPackPreference.DISCLOSURE_KEY, null))

        // Accepting it is remembered, but Offline mode still blocks the request.
        compose.onNodeWithText("Check for official packs").performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 10_000) { shown("Download from GitHub?") }
        compose.onNodeWithText("Continue").performClick()
        compose.waitUntil(timeoutMillis = 10_000) { shown("Offline mode is on", substring = true) }
        assertTrue(
            OfficialPackPreference.disclosureAccepted(
                prefs.getString(OfficialPackPreference.DISCLOSURE_KEY, null),
            ),
        )
        assertFalse(shown("Checking for official packs…"))
        assertEquals(storedBefore, OfflineLibraryHolder.get(context).officialCatalog())

        // Asking again does not show the disclosure a second time.
        compose.onNodeWithText("Check for official packs").performScrollTo().performClick()
        compose.waitForIdle()
        assertFalse(shown("Download from GitHub?"))
    }
}
