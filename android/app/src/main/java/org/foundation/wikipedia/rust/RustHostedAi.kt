package org.foundation.wikipedia.rust

import android.content.Context
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.wiki_ffi.ChatProgressUpdate
import uniffi.wiki_ffi.ChatProgress
import uniffi.wiki_ffi.ChatReply
import uniffi.wiki_ffi.HostedAiCheck
import uniffi.wiki_ffi.HostedAiProvider
import uniffi.wiki_ffi.ArticleChat
import uniffi.wiki_ffi.HostedChatRequest
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

    /**
     * Prepare (no network) a question about an article of [library]'s active
     * pack for [provider]. Show the returned preview's `passages()`,
     * `priorTurns()`, `destination()` and `model()` before calling [send].
     * With [history], a supported answer is saved; [threadId] continues a
     * saved thread of the same article revision.
     */
    suspend fun prepareArticleChat(
        library: RustOfflineLibrary,
        project: String,
        pageId: Long,
        question: String,
        provider: HostedAiProvider,
        model: String,
        history: RustChatHistory? = null,
        threadId: Long? = null,
        selection: String? = null,
    ): ArticleChat {
        require(pageId > 0) { "pageId must be positive" }
        val key = io { keys.load(provider) } ?: throw HostedAiKeyMissingException()
        val request = HostedChatRequest(project, pageId.toULong(), question, provider, model, threadId, selection)
        return library.prepareHostedChat(request, key, history)
    }

    /**
     * Send a disclosed preview once the user agreed to share its passages
     * (and earlier turns) with its destination. Cancelling the coroutine
     * cancels the request.
     */
    suspend fun send(chat: ArticleChat, onProgress: ((ChatProgressUpdate) -> Unit)? = null): ChatReply =
        sendArticleChat(chat, dispatcher, onProgress) { RustTls.init(app) }

    private suspend fun <T> io(block: () -> T): T = withContext(dispatcher) {
        requireOffMainThread()
        block()
    }
}

/**
 * Send a disclosed [chat] once the user agreed to share its passages (and
 * earlier turns) with its destination. [onProgress] is called on a
 * background thread with counts and timings only. Cancelling the coroutine
 * cancels the request.
 */
internal suspend fun sendArticleChat(
    chat: ArticleChat,
    dispatcher: CoroutineDispatcher,
    onProgress: ((ChatProgressUpdate) -> Unit)?,
    prepare: () -> Unit = {},
): ChatReply = coroutineScope {
    val sink = onProgress?.let { report ->
        object : ChatProgress {
            override fun onProgress(update: ChatProgressUpdate) {
                // Never throw into Rust from the progress callback.
                try {
                    report(update)
                } catch (_: RuntimeException) {
                }
            }
        }
    }
    val request = async(dispatcher) {
        requireOffMainThread()
        prepare()
        chat.send(true, sink)
    }
    try {
        request.await()
    } catch (error: CancellationException) {
        chat.cancel()
        throw error
    }
}
