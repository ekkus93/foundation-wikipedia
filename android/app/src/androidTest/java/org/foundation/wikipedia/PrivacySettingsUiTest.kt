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
import org.foundation.wikipedia.rust.RustHostedAi
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeFalse
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.HostedAiProvider
import uniffi.wiki_ffi.LibraryException

/**
 * STATE-003: the Privacy & Network page shows only controls Android has
 * (the mobile-data download rule, Wi-Fi only by default) with an accurate
 * account of network use, and deleting a saved API key asks first.
 */
@RunWith(AndroidJUnit4::class)
class PrivacySettingsUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private val prefs get() =
        context.getSharedPreferences(ReaderOfflinePreference.FILE, Context.MODE_PRIVATE)
    private val keys = listOf(ReaderOfflinePreference.KEY, OfficialPackPreference.MOBILE_DATA_KEY)
    private var saved: Map<String, String?> = emptyMap()

    @Before
    fun offlineWithSamplePack(): Unit = runBlocking {
        saved = keys.associateWith { prefs.getString(it, null) }
        val pack = File(context.cacheDir, "privacy-ui-sample-v2.wpack")
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

    private fun openSettings(category: String, ready: String) {
        compose.onNode(hasSetTextAction()).performTextInput("tide")
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) { shown("Tide") }
        compose.onAllNodesWithText("Tide").onFirst().performClick()
        compose.waitUntil(timeoutMillis = 30_000) {
            compose.onAllNodesWithContentDescription("Open reader actions")
                .fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithContentDescription("Open reader actions").performClick()
        // The menu opens a frame later; on a slow emulator it is not there yet.
        compose.waitUntil(timeoutMillis = 10_000) {
            compose.onAllNodesWithText("Settings").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithText("Settings").performClick()
        compose.onNodeWithText(category).performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 15_000) { shown(ready, substring = true) }
    }

    private fun mobileDataAllowed() = OfficialPackPreference.mobileDataAllowed(
        prefs.getString(OfficialPackPreference.MOBILE_DATA_KEY, null),
    )

    @Test
    fun privacyPageShowsOnlyAndroidControlsAndSavesTheMobileDataRule() {
        openSettings("Privacy & Network", "What goes over the network")
        for (line in ANDROID_NETWORK_DISCLOSURES) {
            assertTrue(line, shown("• $line"))
        }
        // Desktop-only or unbuilt features have no controls here.
        for (absent in listOf("Work offline", "Data folder", "audio and video", "Stream")) {
            assertFalse(absent, shown(absent, substring = false))
        }
        assertFalse(mobileDataAllowed())
        assertTrue(shown("Pack downloads wait for Wi-Fi", substring = true))

        compose.onNodeWithText("Download packs over mobile data").performClick()
        compose.waitUntil(timeoutMillis = 5_000) { mobileDataAllowed() }
        assertTrue(shown("may use mobile data", substring = true))

        compose.onNodeWithText("Download packs over mobile data").performClick()
        compose.waitUntil(timeoutMillis = 5_000) { !mobileDataAllowed() }
    }

    @Test
    fun deletingASavedApiKeyAsksFirst(): Unit = runBlocking {
        val hosted = RustHostedAi(context)
        // Never touch a key someone saved on this device.
        assumeFalse(hosted.isKeyConfigured(HostedAiProvider.ANTHROPIC))
        hosted.saveKey(HostedAiProvider.ANTHROPIC, "sk-ant-test-not-a-real-key")
        try {
            openSettings("AI Assistant", "Hosted AI provider")
            compose.waitUntil(timeoutMillis = 10_000) { shown("Delete key…") }

            compose.onNodeWithText("Delete key…").performScrollTo().performClick()
            compose.waitUntil(timeoutMillis = 5_000) { shown("Delete the Anthropic key?") }
            compose.onNodeWithText("Cancel").performClick()
            compose.waitForIdle()
            assertTrue("Cancel keeps the key", hosted.isKeyConfigured(HostedAiProvider.ANTHROPIC))

            compose.onNodeWithText("Delete key…").performScrollTo().performClick()
            compose.waitUntil(timeoutMillis = 5_000) { shown("Delete the Anthropic key?") }
            compose.onNodeWithText("Delete").performClick()
            compose.waitUntil(timeoutMillis = 10_000) { shown("Deleted the Anthropic key.") }
            assertFalse(hosted.isKeyConfigured(HostedAiProvider.ANTHROPIC))
        } finally {
            hosted.deleteKey(HostedAiProvider.ANTHROPIC)
        }
    }
}
