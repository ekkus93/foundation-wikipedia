package org.foundation.wikipedia

import androidx.compose.ui.semantics.SemanticsProperties
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
import uniffi.wiki_ffi.PackTrust

/**
 * TRUST-002: the installed custom pack's card in Settings › Wikipedia &
 * Offline Data shows "Custom — integrity verified", never a publisher label.
 */
@RunWith(AndroidJUnit4::class)
class PackLibraryTrustUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext

    @Before
    fun installSample(): Unit = runBlocking {
        val pack = File(context.cacheDir, "trust-sample-v2.wpack")
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
        compose.activityRule.scenario.recreate()
    }

    private fun shown(text: String) = compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun theLibraryCardShowsCustomIntegrityVerified() {
        compose.onNode(hasSetTextAction()).performTextInput("tide")
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) { shown("Tide") }
        compose.onAllNodesWithText("Tide").onFirst().performClick()
        compose.waitUntil(timeoutMillis = 30_000) {
            compose.onAllNodesWithContentDescription("Open reader actions").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithContentDescription("Open reader actions").performClick()
        compose.onNodeWithText("Settings").performClick()
        compose.onNodeWithText("Wikipedia & Offline Data").performScrollTo().performClick()
        val custom = packTrustTag(PackTrust.CUSTOM_INTEGRITY_VERIFIED)
        compose.waitUntil(timeoutMillis = 15_000) {
            compose.onAllNodesWithTag(custom).fetchSemanticsNodes().isNotEmpty()
        }
        val card = compose.onAllNodesWithTag(custom).onFirst().fetchSemanticsNode()
        val spoken = card.config[SemanticsProperties.ContentDescription].single()
        assertTrue(spoken, spoken.startsWith("Trust status: Custom — integrity verified."))
        assertEquals(
            "a custom pack never shows a publisher status",
            0,
            compose.onAllNodesWithTag(packTrustTag(PackTrust.VERIFIED_PUBLISHER))
                .fetchSemanticsNodes().size,
        )
    }
}
