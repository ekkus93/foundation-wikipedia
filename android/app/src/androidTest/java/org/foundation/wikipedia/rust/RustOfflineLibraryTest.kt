package org.foundation.wikipedia.rust

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.LibraryException

/**
 * MOB-001 acceptance on device/emulator: Android installs a pack, then searches
 * and opens articles through the shared Rust core via UniFFI.
 */
@RunWith(AndroidJUnit4::class)
class RustOfflineLibraryTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private lateinit var dataDir: File
    private lateinit var pack: File

    @Before
    fun setUp() {
        dataDir = File(context.filesDir, "library-test-${System.nanoTime()}")
        pack = File(context.cacheDir, "android-sample-v2.wpack")
        InstrumentationRegistry.getInstrumentation().context.assets
            .open("android-sample-v2.wpack").use { input ->
                pack.outputStream().use { input.copyTo(it) }
            }
    }

    @After
    fun tearDown() {
        dataDir.deleteRecursively()
        pack.delete()
    }

    @Test
    fun installsSearchesAndOpensThroughSharedRust() = runBlocking {
        RustOfflineLibrary.open(dataDir).use { library ->
            assertNull(library.activeSnapshot())
            assertThrows(LibraryException.ConfirmationRequired::class.java) {
                runBlocking { library.installPack(pack, acceptCustomPack = false) }
            }
            val installed = library.installPack(pack, acceptCustomPack = true)
            assertEquals("android-sample", installed.snapshotId)
            assertEquals("Custom — integrity verified", installed.trustLabel)
            assertEquals(2UL, installed.articles)

            library.activate(installed.snapshotId)
            val hits = library.search("gravity")
            assertEquals(listOf("Tide"), hits.map { it.title })
            val page = library.openPage("enwiki", 1)!!
            assertEquals("Moon", page.title)
            assertTrue(page.html.contains("orbits the Earth"))
            assertNull(library.openPage("enwiki", 99))
            assertThrows(LibraryException.InvalidInput::class.java) {
                runBlocking { library.search("   ") }
            }
        }
        // Reopening (as after a process restart) finds the active snapshot.
        RustOfflineLibrary.open(dataDir).use { reopened ->
            assertEquals("android-sample", reopened.activeSnapshot())
            assertEquals(2, reopened.search("moon").size)
        }
    }

    @Test
    fun blockingRustCallsAreRefusedOnTheMainThread() {
        var error: Throwable? = null
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            error = runCatching { requireOffMainThread() }.exceptionOrNull()
        }
        assertTrue(error is IllegalStateException)
        requireOffMainThread()
    }
}
