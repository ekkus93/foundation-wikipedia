package org.foundation.wikipedia.rust

import android.content.Context
import java.io.File
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.wiki_ffi.ChatHistory
import uniffi.wiki_ffi.SavedExchange

/**
 * Saved article chats on Android (AI-003): exact-revision threads in the
 * shared Rust user-state SQLite. Stored under `noBackupFilesDir`, so chats
 * are not copied to cloud backups or device transfers.
 */
class RustChatHistory private constructor(
    internal val history: ChatHistory,
    private val dispatcher: CoroutineDispatcher,
) : AutoCloseable {

    companion object {
        suspend fun open(
            context: Context,
            dispatcher: CoroutineDispatcher = Dispatchers.IO,
        ): RustChatHistory = open(
            File(context.applicationContext.noBackupFilesDir, "chat-history"),
            dispatcher,
        )

        suspend fun open(
            dir: File,
            dispatcher: CoroutineDispatcher = Dispatchers.IO,
        ): RustChatHistory = withContext(dispatcher) {
            requireOffMainThread()
            RustChatHistory(ChatHistory(dir.absolutePath), dispatcher)
        }
    }

    /** Saved thread IDs for one exact article revision, newest first. */
    suspend fun threads(project: String, pageId: Long, revisionId: Long): List<Long> =
        call { it.threads(project, pageId.toULong(), revisionId.toULong()) }

    /** A thread's saved exchanges, oldest first. Citations are historical. */
    suspend fun exchanges(
        project: String,
        pageId: Long,
        revisionId: Long,
        threadId: Long,
    ): List<SavedExchange> =
        call { it.exchanges(project, pageId.toULong(), revisionId.toULong(), threadId) }

    suspend fun deleteThread(project: String, pageId: Long, revisionId: Long, threadId: Long) =
        call { it.deleteThread(project, pageId.toULong(), revisionId.toULong(), threadId) }

    /** Delete every saved chat. */
    suspend fun clearAll() = call { it.clearAll() }

    override fun close() = history.close()

    private suspend fun <T> call(block: (ChatHistory) -> T): T = withContext(dispatcher) {
        requireOffMainThread()
        block(history)
    }
}
