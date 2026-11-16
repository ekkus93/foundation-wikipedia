package org.foundation.wikipedia

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
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.LibraryException
import uniffi.wiki_ffi.PackTrust

/**
 * MOBPACK-001 live acceptance against the published test release (ADR 0009).
 * Skipped unless run with `-e officialLive download` (phase 1: fetch the
 * signed catalog from GitHub, download and install the official test pack,
 * activate it) or `-e officialLive offline` (phase 2, after the app process
 * is force-stopped and the device's network is turned off: the pack is still
 * Verified publisher, active and readable). CI never runs it.
 */
@RunWith(AndroidJUnit4::class)
class OfficialPackLiveTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val phase: String? =
        InstrumentationRegistry.getArguments().getString("officialLive")
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext

    @Test
    fun phase1DownloadsAndInstallsTheSignedTestPack(): Unit = runBlocking {
        assumeTrue(phase == "download")
        val library = OfflineLibraryHolder.get(context)
        // Start clean: the test pack must come from GitHub in this run.
        if (library.listPacks().any { it.snapshotId == TEST_PACK }) {
            if (library.activeSnapshot() == TEST_PACK) {
                val sample = File(context.cacheDir, "live-sample-v2.wpack")
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
            }
            library.removePack(TEST_PACK)
        }

        val catalog = library.refreshOfficialCatalog(context)
        assertEquals("foundation-wikipedia", catalog.publisher)
        val offered = catalog.packs.single { it.packId == TEST_PACK }
        assertTrue(!offered.installed)

        val reviews = mutableListOf<PackTrust>()
        var lastProgress = 0L to 0L
        val installed = library.downloadOfficialPack(
            context,
            TEST_PACK,
            onProgress = { done, total -> lastProgress = done to total },
        ) { review ->
            reviews += review.trust
            true
        }
        assertEquals(listOf(PackTrust.VERIFIED_PUBLISHER), reviews)
        assertEquals(PackTrust.VERIFIED_PUBLISHER, installed.trust)
        assertEquals(offered.bytes.toLong() to offered.bytes.toLong(), lastProgress)
        library.activate(TEST_PACK)
        assertEquals(TEST_PACK, library.activeSnapshot())
    }

    @Test
    fun phase2TheInstalledPackIsVerifiedAndReadableOffline(): Unit = runBlocking {
        assumeTrue(phase == "offline")
        val library = OfflineLibraryHolder.get(context)
        val pack = library.listPacks().single { it.snapshotId == TEST_PACK }
        assertEquals(PackTrust.VERIFIED_PUBLISHER, pack.trust)
        assertTrue(pack.active)
        // The verified catalog is still available without a network.
        assertTrue(library.officialCatalog()!!.packs.single { it.packId == TEST_PACK }.installed)

        // Its articles are searchable in the reader UI with no network.
        compose.activityRule.scenario.recreate()
        compose.onNode(hasSetTextAction()).performTextInput("tide")
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) {
            compose.onAllNodesWithText("Tide").fetchSemanticsNodes().isNotEmpty()
        }
        val page = library.openPage("enwiki", 2)!!
        assertTrue(page.html.contains("Tides follow"))
    }

    private companion object {
        const val TEST_PACK = "test-sample"
    }
}
