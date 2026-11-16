package org.foundation.wikipedia

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.performCustomAccessibilityActionWithLabel
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.tryPerformAccessibilityChecks
import androidx.compose.ui.test.junit4.accessibility.enableAccessibilityChecks
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.LibraryException
import uniffi.wiki_ffi.LocalAiProtocol

/**
 * MOB-003 streamed chat: while a (fake, on-device) Ollama model writes, the
 * sheet shows the text so far labelled as unchecked, with citation markers
 * as placeholders; the checked answer replaces it, and a draft whose
 * citations fail the check is removed rather than shown.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(AndroidJUnit4::class)
class StreamedDraftUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private var saved: LocalAiProfiles? = null
    private var server: FakeOllama? = null

    @Before
    fun installFixturePack(): Unit = runBlocking {
        saved = LocalAiPreference.profiles(context)
        val pack = File(context.cacheDir, "draft-sample-v2.wpack")
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

    @After
    fun restoreChoice() {
        server?.close()
        saved?.let { LocalAiPreference.store(context, it) }
    }

    /** Streams [first], waits for [release], then streams [rest]. */
    /** A local Ollama stand-in; with [failChat] it drops every chat request. */
    private class FakeOllama(
        private val first: String,
        private val rest: String,
        private val failChat: Boolean = false,
    ) : AutoCloseable {
        val chatRequests = java.util.concurrent.atomic.AtomicInteger()
        val release = CountDownLatch(1)
        private val socket = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
        val endpoint = "http://127.0.0.1:${socket.localPort}"

        init {
            thread(isDaemon = true) {
                while (!socket.isClosed) {
                    val client = try { socket.accept() } catch (_: Exception) { break }
                    thread(isDaemon = true) { client.use { serve(it) } }
                }
            }
        }

        private fun serve(client: Socket) {
            val input = client.getInputStream()
            val head = StringBuilder()
            while (!head.endsWith("\r\n\r\n")) {
                val byte = input.read()
                if (byte < 0) return
                head.append(byte.toChar())
            }
            val length = Regex("(?i)content-length: *(\\d+)").find(head)?.groupValues?.get(1)?.toInt() ?: 0
            repeat(length) { input.read() }
            val output = client.getOutputStream()
            if (head.startsWith("GET /api/tags ")) {
                val body = """{"models":[{"name":"fake:1b"}]}"""
                output.write(
                    ("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n" +
                        "Content-Length: ${body.length}\r\n\r\n$body").toByteArray(),
                )
                output.flush()
                return
            }
            chatRequests.incrementAndGet()
            if (failChat) return // the connection closes with no response
            fun event(text: String) =
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"$text\"},\"finish_reason\":null}]}\n\n"
            val start = event(first)
            val end = event(rest) +
                "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
            val bytes = start.toByteArray().size + end.toByteArray().size
            output.write(
                ("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n" +
                    "Content-Length: $bytes\r\n\r\n$start").toByteArray(),
            )
            output.flush()
            release.await(60, TimeUnit.SECONDS)
            output.write(end.toByteArray())
            output.flush()
        }

        override fun close() {
            release.countDown()
            socket.close()
        }
    }

    private fun openChatAndType(fake: FakeOllama) {
        server = fake
        assertTrue(LocalAiPreference.save(context, LocalAiChoice(LocalAiProtocol.OLLAMA, fake.endpoint, "fake:1b")))
        compose.onNode(hasSetTextAction()).performTextInput("tide")
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) {
            compose.onAllNodesWithText("Tide").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onAllNodesWithText("Tide").onFirst().performClick()
        compose.waitUntil(timeoutMillis = 30_000) {
            compose.onAllNodesWithContentDescription("Open reader actions").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithContentDescription("Open reader actions").performClick()
        compose.onNodeWithText("Chat").performClick()
        compose.waitUntil(timeoutMillis = 15_000) {
            compose.onAllNodesWithText("Model fake:1b", substring = true).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNode(hasSetTextAction() and hasTextLabel("Question")).performTextInput("What do tides follow?")
    }

    private fun askAboutTide(fake: FakeOllama) {
        openChatAndType(fake)
        compose.onNodeWithText("Review request").performClick()
        compose.waitUntil(timeoutMillis = 15_000) {
            compose.onAllNodesWithText("Send").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithText("Send").performClick()
    }

    private fun hasTextLabel(label: String) =
        androidx.compose.ui.test.hasText(label, substring = false, ignoreCase = false)

    private fun shown(text: String, substring: Boolean = true) =
        compose.onAllNodesWithText(text, substring = substring).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun theTextSoFarIsShownAsAnUncheckedDraftThenReplacedByTheCheckedAnswer() {
        val lead = "wkb:enwiki:2:7002:b0"
        askAboutTide(FakeOllama("Tides follow the Moon [[cite:$lead]]", " and the Sun."))
        compose.waitUntil(timeoutMillis = 30_000) { shown(DRAFT_LABEL, substring = false) }
        assertTrue(shown("Tides follow the Moon […]"))
        assertTrue("the draft must not show an evidence ID", !shown("cite:"))
        assertTrue("an unchecked citation is not a button", !shown("Show cited passage", substring = true))
        server!!.release.countDown()
        compose.waitUntil(timeoutMillis = 30_000) {
            compose.onAllNodesWithContentDescription("Show cited passage 1", substring = true)
                .fetchSemanticsNodes().isNotEmpty()
        }
        assertTrue(shown("Tides follow the Moon") && shown("[1] and the Sun."))
        assertTrue(!shown(DRAFT_LABEL, substring = false))

        // Tapping the citation lowers the sheet to its peek height (the
        // highlighted paragraph is visible) and keeps the answer in view.
        val handle = compose.onNodeWithContentDescription("Chat sheet size")
        handle.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "half height"))
        compose.onNodeWithContentDescription("Show cited passage 1", substring = true).performClick()
        compose.waitUntil(timeoutMillis = 10_000) {
            compose.onAllNodes(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "collapsed"))
                .fetchSemanticsNodes().isNotEmpty()
        }
        compose.waitForIdle()
        assertTrue(shown("Tides follow the Moon"))
        assertTrue("peek shows no question field", !shown("Review request", substring = false))
        // TalkBack's Expand action raises it again with the conversation.
        handle.performCustomAccessibilityActionWithLabel("Expand")
        compose.waitUntil(timeoutMillis = 10_000) { shown("Review request", substring = false) }
        handle.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "half height"))
    }

    /**
     * AI-004: with a hosted provider also configured, a failing local model
     * shows an error and stays selected; nothing is retried elsewhere.
     */
    @Test
    fun aFailedLocalModelNeverFallsBackToAConfiguredHostedProvider(): Unit = runBlocking {
        val hosted = org.foundation.wikipedia.rust.RustHostedAi(context)
        val provider = uniffi.wiki_ffi.HostedAiProvider.ANTHROPIC
        // Never overwrite or delete a real key on this device.
        val addedKey = !hosted.isKeyConfigured(provider)
        if (addedKey) hosted.saveKey(provider, "test-key-never-used")
        try {
            val fake = FakeOllama("unused", "unused", failChat = true)
            askAboutTide(fake)
            compose.waitUntil(timeoutMillis = 30_000) {
                shown("Could not connect securely", substring = true) ||
                    shown("unexpected response", substring = true) ||
                    shown("did not respond", substring = true)
            }
            assertTrue(fake.chatRequests.get() >= 1)
            // The local model is still the chosen provider and no hosted
            // request was reviewed or sent.
            assertTrue(shown("Model fake:1b", substring = true))
            assertTrue(!shown("api.anthropic.com", substring = true))
            assertTrue(!shown(DRAFT_LABEL, substring = false))
            assertTrue(!shown("Show cited passage", substring = true))
        } finally {
            if (addedKey) hosted.deleteKey(provider)
        }
    }

    /** AI-005: a model that finds no support says so; nothing is cited. */
    @Test
    fun anAnswerWithoutEvidenceIsShownAsUnanswered() {
        askAboutTide(FakeOllama("[NO_EVIDENCE]", ""))
        server!!.release.countDown()
        compose.waitUntil(timeoutMillis = 30_000) {
            shown("The article passages do not answer this question.", substring = false)
        }
        assertTrue(!shown("[NO_EVIDENCE]"))
        assertTrue(!shown("Show cited passage", substring = true))
        assertTrue(!shown(DRAFT_LABEL, substring = false))
    }

    /** AI-005: Stop cancels a streaming answer; the draft is discarded. */
    @Test
    fun stoppingAStreamingAnswerDiscardsItsDraft() {
        askAboutTide(FakeOllama("Tides follow the Moon [[cite:wkb:enwiki:2:7002:b0]]", " and more."))
        compose.waitUntil(timeoutMillis = 30_000) { shown(DRAFT_LABEL, substring = false) }
        compose.onNodeWithText("Stop").performClick()
        compose.waitUntil(timeoutMillis = 15_000) { shown("Stopped. No answer was shown.") }
        assertTrue(!shown(DRAFT_LABEL, substring = false))
        assertTrue(!shown("Show cited passage", substring = true))
        // A late end of the stream cannot bring the answer back.
        server!!.release.countDown()
        Thread.sleep(1_000)
        compose.waitForIdle()
        assertTrue(!shown("and more", substring = true))
        assertTrue(!shown("Show cited passage", substring = true))
    }

    @Test
    fun aDraftWhoseCitationsFailTheCheckIsRemoved() {
        askAboutTide(FakeOllama("Tides follow the Moon [[cite:wkb:enwiki:2:9999:b0]]", "."))
        compose.waitUntil(timeoutMillis = 30_000) { shown(DRAFT_LABEL, substring = false) }
        server!!.release.countDown()
        compose.waitUntil(timeoutMillis = 30_000) { shown("citations did not match this article") }
        assertTrue(!shown(DRAFT_LABEL, substring = false))
        assertTrue(!shown("Tides follow the Moon"))
    }

    @Test
    fun theChatSheetPassesAccessibilityChecksAtEveryHeightAndWhileDrafting() {
        val fake = FakeOllama("Tides follow the Moon [[cite:wkb:enwiki:2:7002:b0]]", ".")
        askAboutTide(fake)
        compose.waitUntil(timeoutMillis = 30_000) { shown(DRAFT_LABEL, substring = false) }
        // Touch target size, contrast, labels and duplicate descriptions
        // (Accessibility Test Framework); failures throw with details.
        compose.enableAccessibilityChecks()
        compose.onRoot().tryPerformAccessibilityChecks()
        fake.release.countDown()
        compose.waitUntil(timeoutMillis = 30_000) { !shown(DRAFT_LABEL, substring = false) }
        val handle = compose.onNodeWithContentDescription("Chat sheet size")
        for (label in listOf("Expand", "Collapse", "Collapse")) {
            compose.waitForIdle()
            compose.onRoot().tryPerformAccessibilityChecks()
            handle.performCustomAccessibilityActionWithLabel(label)
        }
        compose.waitUntil(timeoutMillis = 10_000) {
            compose.onAllNodes(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "collapsed"))
                .fetchSemanticsNodes().isNotEmpty()
        }
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    @Test
    fun leavingSettingsReturnsToTheOpenChatWithItsQuestionAndHeight() {
        openChatAndType(FakeOllama("unused", "unused"))
        val handle = compose.onNodeWithContentDescription("Chat sheet size")
        handle.performCustomAccessibilityActionWithLabel("Expand")
        compose.onNodeWithText("AI settings").performClick()
        compose.waitUntil(timeoutMillis = 10_000) { shown("Local or LAN model", substring = false) }
        // The chat is hidden (not just covered) while Settings is open.
        assertTrue(!shown("Ask about “Tide”", substring = false))
        assertTrue(!shown("What do tides follow?", substring = false))
        // Back goes to the app, not to a keyboard still closing on slow emulators.
        fun back() {
            compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
            compose.waitForIdle()
        }
        back() // AI Assistant → Settings
        back() // Settings → the reader with its chat
        compose.waitUntil(timeoutMillis = 10_000) { shown("Ask about “Tide”", substring = false) }
        assertTrue(shown("What do tides follow?", substring = false))
        handle.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "full height"))
        assertTrue(readerTitleIs("Tide"))
    }

    private fun readerTitleIs(title: String): Boolean {
        var found = false
        compose.runOnUiThread {
            fun search(view: android.view.View) {
                if (view is android.webkit.WebView && view.title == title) found = true
                if (view is android.view.ViewGroup) for (i in 0 until view.childCount) search(view.getChildAt(i))
            }
            search(compose.activity.window.decorView)
        }
        return found
    }
}
