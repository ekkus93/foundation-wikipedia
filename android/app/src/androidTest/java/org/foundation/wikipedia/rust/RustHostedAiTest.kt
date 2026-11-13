package org.foundation.wikipedia.rust

import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.net.URL
import javax.net.ssl.HttpsURLConnection
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.HostedAiException
import uniffi.wiki_ffi.HostedAiProvider
import uniffi.wiki_ffi.checkHostedAiConnection
import uniffi.wiki_ffi.hostedAiReady

/**
 * AI-003 on device/emulator: Keystore-encrypted API keys and Android OS
 * certificate verification for the shared Rust HTTPS transport.
 */
@RunWith(AndroidJUnit4::class)
class RustHostedAiTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val keyDir = File(context.noBackupFilesDir, "hosted-ai-keys")
    private val store = HostedAiKeyStore(context)

    @Before
    fun clean() {
        keyDir.deleteRecursively()
    }

    @After
    fun cleanUp() {
        keyDir.deleteRecursively()
    }

    @Test
    fun keysAreEncryptedAtRestAndExcludedFromBackup() {
        val secret = "sk-ant-instrumented-secret-0123456789"
        assertFalse(store.isConfigured(HostedAiProvider.ANTHROPIC))
        store.save(HostedAiProvider.ANTHROPIC, "  $secret\n")
        assertTrue(store.isConfigured(HostedAiProvider.ANTHROPIC))
        assertFalse(store.isConfigured(HostedAiProvider.OPEN_AI))

        val file = File(keyDir, "anthropic.key")
        assertTrue(file.path.startsWith(context.noBackupFilesDir.path))
        val onDisk = String(file.readBytes(), Charsets.ISO_8859_1)
        assertFalse(onDisk.contains(secret))
        assertFalse(onDisk.contains("instrumented"))
        assertEquals(secret, store.load(HostedAiProvider.ANTHROPIC))

        store.save(HostedAiProvider.ANTHROPIC, "sk-ant-replacement")
        assertEquals("sk-ant-replacement", store.load(HostedAiProvider.ANTHROPIC))
        assertTrue(store.delete(HostedAiProvider.ANTHROPIC))
        assertFalse(store.delete(HostedAiProvider.ANTHROPIC))
        assertNull(store.load(HostedAiProvider.ANTHROPIC))
        assertEquals(emptyList<String>(), keyDir.list()?.toList().orEmpty())
    }

    @Test
    fun malformedKeysAreRefusedBeforeStorage() {
        for (bad in listOf("", "   ", "sk with space", "sk-\r\nX-Injected: 1", "sk-é")) {
            assertThrows(IllegalArgumentException::class.java) {
                store.save(HostedAiProvider.OPEN_AI, bad)
            }
        }
        assertFalse(store.isConfigured(HostedAiProvider.OPEN_AI))
    }

    @Test
    fun aKeyFileCannotBeReplayedForAnotherProviderOrTampered() {
        store.save(HostedAiProvider.ANTHROPIC, "sk-ant-bound-to-anthropic")
        val sealed = File(keyDir, "anthropic.key").readBytes()
        File(keyDir, "open_ai.key").writeBytes(sealed)
        assertThrows(HostedAiKeyStorageException::class.java) {
            store.load(HostedAiProvider.OPEN_AI)
        }
        sealed[sealed.size - 1] = (sealed[sealed.size - 1].toInt() xor 1).toByte()
        File(keyDir, "anthropic.key").writeBytes(sealed)
        assertThrows(HostedAiKeyStorageException::class.java) {
            store.load(HostedAiProvider.ANTHROPIC)
        }
    }

    @Test
    fun aMissingKeyFailsBeforeAnyConnection() {
        assertThrows(HostedAiKeyMissingException::class.java) {
            runBlocking { RustHostedAi(context).checkConnection(HostedAiProvider.OPEN_AI, "m") }
        }
    }

    /**
     * Live: a real TLS handshake with api.anthropic.com, verified by Android's
     * trust manager through rustls-platform-verifier. A deliberately invalid
     * key must reach the provider and be refused (HTTP 401 → Rejected), which
     * shows certificate verification succeeded; a verifier or network failure
     * would surface as NotReady or Network instead.
     */
    @Test
    fun androidVerifierCompletesARealProviderHandshake() {
        RustTls.init(context)
        RustTls.init(context)
        assertTrue(hostedAiReady())
        // Separate "the emulator has no route to the provider" from a Rust or
        // verifier failure: Android's own HTTPS stack must reach the host first.
        val platformStatus = try {
            val connection = URL("https://api.anthropic.com/v1/messages")
                .openConnection() as HttpsURLConnection
            connection.connectTimeout = 15_000
            connection.readTimeout = 15_000
            try {
                connection.responseCode
            } finally {
                connection.disconnect()
            }
        } catch (error: Exception) {
            fail("Android's own HTTPS stack could not reach api.anthropic.com: $error")
        }
        Log.i(TAG, "platform HTTPS to api.anthropic.com returned HTTP $platformStatus")
        try {
            checkHostedAiConnection(
                HostedAiProvider.ANTHROPIC,
                "sk-ant-invalid-instrumented-test-key",
                "claude-haiku-5-5",
            )
            fail("an invalid key must be refused by the provider")
        } catch (error: HostedAiException.Rejected) {
            // Expected: TLS verified, request sent, provider refused the key.
        } catch (error: HostedAiException) {
            fail(
                "Platform HTTPS reached the host (HTTP $platformStatus) but the Rust " +
                    "transport failed with ${error::class.simpleName}",
            )
        }
    }

    private companion object {
        const val TAG = "RustHostedAiTest"
    }
}
