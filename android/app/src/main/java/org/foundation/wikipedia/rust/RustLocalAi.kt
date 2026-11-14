package org.foundation.wikipedia.rust

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.wiki_ffi.ArticleChat
import uniffi.wiki_ffi.ChatProgressUpdate
import uniffi.wiki_ffi.ChatReply
import uniffi.wiki_ffi.LocalAiProtocol
import uniffi.wiki_ffi.LocalAiServer
import uniffi.wiki_ffi.LocalChatRequest
import uniffi.wiki_ffi.discoverLocalAi

/**
 * A model server on this device or the local network (MOB-003), for example
 * Ollama on a computer, or `http://10.0.2.2:11434` (the host) from an Android
 * emulator. Only `http://` URLs with a loopback or private-network IP address
 * are accepted; no DNS name or public address can be used.
 */
class RustLocalAi(private val dispatcher: CoroutineDispatcher = Dispatchers.IO) {
    /** Read the server's model list (no question or article text is sent). */
    suspend fun discover(protocol: LocalAiProtocol, endpoint: String): LocalAiServer =
        withContext(dispatcher) {
            requireOffMainThread()
            discoverLocalAi(protocol, endpoint)
        }

    /**
     * Prepare (no network) a question about an article of [library]'s active
     * pack for [model] of [server]. Disclose the preview before [send].
     */
    suspend fun prepareArticleChat(
        library: RustOfflineLibrary,
        project: String,
        pageId: Long,
        question: String,
        server: LocalAiServer,
        model: String,
        history: RustChatHistory? = null,
        threadId: Long? = null,
        selection: String? = null,
    ): ArticleChat {
        require(pageId > 0) { "pageId must be positive" }
        val request = LocalChatRequest(project, pageId.toULong(), question, server, model, threadId, selection)
        return library.prepareLocalChat(request, history)
    }

    suspend fun send(chat: ArticleChat, onProgress: ((ChatProgressUpdate) -> Unit)? = null): ChatReply =
        sendArticleChat(chat, dispatcher, onProgress)
}
