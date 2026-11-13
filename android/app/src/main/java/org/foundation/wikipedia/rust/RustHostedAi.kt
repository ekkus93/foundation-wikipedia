package org.foundation.wikipedia.rust

import android.content.Context
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.wiki_ffi.HostedAiCheck
import uniffi.wiki_ffi.HostedAiProvider
import uniffi.wiki_ffi.checkHostedAiConnection

/** No key is saved for the requested provider. */
class HostedAiKeyMissingException : Exception("No API key is saved for this provider")

/** The fixed host each provider talks to (mirrors wiki-ai `provider_host`). */
val HostedAiProvider.host: String
    get() = when (this) {
        HostedAiProvider.ANTHROPIC -> "api.anthropic.com"
        HostedAiProvider.OPEN_AI -> "api.openai.com"
    }

/**
 * Hosted AI on Android (AI-003): app-owned keys in [HostedAiKeyStore] and
 * the shared Rust HTTPS transport, verified by Android's certificate checks
 * ([RustTls]). Keys are read only to make an explicit request, are passed to
 * Rust for that request alone, and are never returned to the UI.
 */
class RustHostedAi(
    context: Context,
    private val dispatcher: CoroutineDispatcher = Dispatchers.IO,
) {
    private val app = context.applicationContext
    private val keys = HostedAiKeyStore(app)

    suspend fun saveKey(provider: HostedAiProvider, apiKey: String) =
        io { keys.save(provider, apiKey) }

    suspend fun isKeyConfigured(provider: HostedAiProvider): Boolean =
        io { keys.isConfigured(provider) }

    suspend fun deleteKey(provider: HostedAiProvider): Boolean = io { keys.delete(provider) }

    /**
     * Send one tiny prompt to [provider] with the saved key. This IS cloud
     * traffic; call it only from a user action that discloses [host].
     */
    suspend fun checkConnection(provider: HostedAiProvider, model: String): HostedAiCheck = io {
        RustTls.init(app)
        val key = keys.load(provider) ?: throw HostedAiKeyMissingException()
        checkHostedAiConnection(provider, key, model.trim())
    }

    private suspend fun <T> io(block: () -> T): T = withContext(dispatcher) {
        requireOffMainThread()
        block()
    }
}
