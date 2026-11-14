package org.foundation.wikipedia.rust

import android.content.Context
import java.io.File
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.wiki_ffi.ReaderState
import uniffi.wiki_ffi.SavedContent

/**
 * Bookmarks and opt-in reading history on Android (MOB-004), in the same
 * Rust user-state SQLite as saved chats, under `noBackupFilesDir`. Every
 * call runs off the main thread; failures are [uniffi.wiki_ffi.ReaderStateException].
 */
class RustReaderState private constructor(
    internal val state: ReaderState,
    private val dispatcher: CoroutineDispatcher,
) : AutoCloseable {

    companion object {
        suspend fun open(
            context: Context,
            dispatcher: CoroutineDispatcher = Dispatchers.IO,
        ): RustReaderState = withContext(dispatcher) {
            requireOffMainThread()
            val dir = File(context.applicationContext.noBackupFilesDir, "chat-history")
            RustReaderState(ReaderState(dir.absolutePath), dispatcher)
        }
    }

    suspend fun isBookmarked(project: String, pageId: Long): Boolean =
        call { it.isBookmarked(project, pageId.toULong()) }

    /** Returns whether the page is bookmarked now. */
    suspend fun toggleBookmark(project: String, pageId: Long, revisionId: Long): Boolean =
        call { it.toggleBookmark(project, pageId.toULong(), revisionId.toULong()) }

    suspend fun removeBookmark(project: String, pageId: Long): Boolean =
        call { it.removeBookmark(project, pageId.toULong()) }

    /** Saved only while history is on; returns whether it was saved. */
    suspend fun recordVisit(project: String, pageId: Long, revisionId: Long): Boolean =
        call { it.recordVisit(project, pageId.toULong(), revisionId.toULong()) }

    suspend fun setHistoryEnabled(enabled: Boolean) = call { it.setHistoryEnabled(enabled) }

    suspend fun clearHistory(): Long = call { it.clearHistory().toLong() }

    /** Without titles (no library); see [RustOfflineLibrary.savedContent]. */
    suspend fun savedContent(): SavedContent = call { it.savedContent(null) }

    override fun close() = state.close()

    private suspend fun <T> call(block: (ReaderState) -> T): T = withContext(dispatcher) {
        requireOffMainThread()
        block(state)
    }
}
