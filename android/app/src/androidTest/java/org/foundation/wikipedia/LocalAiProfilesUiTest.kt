package org.foundation.wikipedia

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
import java.net.InetAddress
import java.net.ServerSocket
import kotlin.concurrent.thread
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.LibraryException
import uniffi.wiki_ffi.LocalAiProtocol

/**
 * AI-004 on Android: saved local models can be tested, turned off, and
 * removed; removing the active one leaves local chat off rather than
 * switching to another saved server.
 */
@RunWith(AndroidJUnit4::class)
class LocalAiProfilesUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private var saved: LocalAiProfiles? = null
    private val socket = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
    private val fake = LocalAiChoice(LocalAiProtocol.OLLAMA, "http://127.0.0.1:${socket.localPort}", "fake:1b")
    private val other = LocalAiChoice(LocalAiProtocol.OLLAMA, "http://127.0.0.1:9", "other:2b")

    @Before
    fun twoSavedModels(): Unit = runBlocking {
        // A model list server only: it never receives a chat request here.
        thread(isDaemon = true) {
            while (!socket.isClosed) {
                val client = try { socket.accept() } catch (_: Exception) { break }
                client.use {
                    val input = it.getInputStream()
                    val head = StringBuilder()
                    while (!head.endsWith("\r\n\r\n")) {
                        val byte = input.read()
                        if (byte < 0) return@use
                        head.append(byte.toChar())
                    }
                    val body = """{"models":[{"name":"fake:1b"}]}"""
                    it.getOutputStream().write(
                        ("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n" +
                            "Content-Length: ${body.length}\r\n\r\n$body").toByteArray(),
                    )
                }
            }
        }
        val pack = File(context.cacheDir, "profiles-sample-v2.wpack")
        InstrumentationRegistry.getInstrumentation().context.assets
            .open("android-sample-v2.wpack").use { input ->
                pack.outputStream().use { input.copyTo(it) }
            }
        val library = OfflineLibraryHolder.get(context)
        try {
            library.installPack(pack, acceptCustomPack = true)
        } catch (_: LibraryException.AlreadyInstalled) {
        }
        library.activate("android-sample")
        pack.delete()
        saved = LocalAiPreference.profiles(context)
        LocalAiPreference.store(
            context,
            LocalAiProfiles().saveAndActivate(other)!!.saveAndActivate(fake)!!,
        )
        compose.activityRule.scenario.recreate()
    }

    @After
    fun restore() {
        socket.close()
        saved?.let { LocalAiPreference.store(context, it) }
    }

    private fun shown(text: String, substring: Boolean = true) =
        compose.onAllNodesWithText(text, substring = substring).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun savedModelsCanBeTestedTurnedOffAndRemovedWithoutSwitchingServers() {
        compose.onNode(hasSetTextAction()).performTextInput("tide")
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) { shown("Tide", substring = false) }
        compose.onAllNodesWithText("Tide").onFirst().performClick()
        compose.waitUntil(timeoutMillis = 30_000) {
            compose.onAllNodesWithContentDescription("Open reader actions").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithContentDescription("Open reader actions").performClick()
        compose.onNodeWithText("Settings").performClick()
        compose.onNodeWithText("AI Assistant").performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 10_000) { shown("Saved models", substring = false) }

        // Test the active model: only its model list is read.
        compose.onAllNodesWithText("Test", substring = false)[1].performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 15_000) {
            shown("Connected: fake:1b is available on a server on this device.", substring = false)
        }

        // Turn local chat off; both models stay saved.
        compose.onNodeWithText("Off — don't use a local model").performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 5_000) { shown("Chat does not use a local model.", substring = false) }
        assertNull(LocalAiPreference.load(context))
        assertEquals(2, LocalAiPreference.profiles(context).profiles.size)

        // Choose fake again, then remove it: nothing else becomes active.
        compose.onNodeWithText("fake:1b at ${fake.endpoint}").performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 5_000) { LocalAiPreference.load(context) == fake }
        compose.onAllNodesWithText("Remove", substring = false)[1].performScrollTo().performClick()
        compose.waitUntil(timeoutMillis = 5_000) { shown("Chat is using it.") }
        compose.onNode(
            androidx.compose.ui.test.hasText("Remove", substring = false) and
                androidx.compose.ui.test.hasAnyAncestor(androidx.compose.ui.test.isDialog()),
        ).performClick()
        compose.waitUntil(timeoutMillis = 5_000) { shown("Removed fake:1b.", substring = false) }
        assertNull(LocalAiPreference.load(context))
        assertEquals(listOf(other), LocalAiPreference.profiles(context).profiles)
    }
}
