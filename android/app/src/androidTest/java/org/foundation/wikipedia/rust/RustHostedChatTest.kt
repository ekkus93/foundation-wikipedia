package org.foundation.wikipedia.rust

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.ChatException
import uniffi.wiki_ffi.HostedAiProvider

/**
 * AI-003 on device/emulator: hosted article chat grounded in an installed
 * pack, disclosed before consent, through the shared Rust RAG and HTTPS code.
 */
@RunWith(AndroidJUnit4::class)
class RustHostedChatTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val keyDir = File(context.noBackupFilesDir, "hosted-ai-keys")
    private lateinit var dataDir: File
    private lateinit var pack: File
    private lateinit var library: RustOfflineLibrary
    private val hosted = RustHostedAi(context)

    @Before
    fun setUp(): Unit = runBlocking {
        keyDir.deleteRecursively()
        dataDir = File(context.filesDir, "chat-test-${System.nanoTime()}")
        pack = File(context.cacheDir, "chat-sample-v2.wpack")
        InstrumentationRegistry.getInstrumentation().context.assets
            .open("android-sample-v2.wpack").use { input ->
                pack.outputStream().use { input.copyTo(it) }
            }
        library = RustOfflineLibrary.open(dataDir)
        val installed = library.installPack(pack, acceptCustomPack = true)
        library.activate(installed.snapshotId)
    }

    @After
    fun tearDown() {
        library.close()
        dataDir.deleteRecursively()
        pack.delete()
        keyDir.deleteRecursively()
    }

    @Test
    fun aPreviewDisclosesPassagesAndDestinationAndNeedsConsent(): Unit = runBlocking {
        assertThrows(HostedAiKeyMissingException::class.java) {
            runBlocking { prepare() }
        }
        hosted.saveKey(HostedAiProvider.ANTHROPIC, "sk-ant-invalid-instrumented-test-key")
        val chat = prepare()
        assertEquals("api.anthropic.com", chat.destination())
        assertEquals("claude-haiku-5-5", chat.model())
        assertEquals("Tide", chat.articleTitle())
        assertTrue(chat.passages().any { it.excerpt.contains("Tides follow the Moon's gravity.") })
        assertThrows(ChatException.ConsentRequired::class.java) { chat.send(false) }
    }

    /**
     * Live: the consented request goes through the shared RAG flow and
     * Android-verified TLS to api.anthropic.com, which refuses the
     * deliberately invalid key. Nothing is shown or saved.
     */
    @Test
    fun aConsentedQuestionReachesTheProviderThroughSharedRust(): Unit = runBlocking {
        hosted.saveKey(HostedAiProvider.ANTHROPIC, "sk-ant-invalid-instrumented-test-key")
        val chat = prepare()
        try {
            hosted.send(chat)
            fail("an invalid key must be refused by the provider")
        } catch (error: ChatException.Rejected) {
            // Expected: evidence packed, TLS verified, provider refused the key.
        }
        assertThrows(ChatException.AlreadySent::class.java) { chat.send(true) }
    }

    private suspend fun prepare() = hosted.prepareArticleChat(
        library,
        "enwiki",
        2,
        "What do tides follow?",
        HostedAiProvider.ANTHROPIC,
        "claude-haiku-5-5",
    )
}
