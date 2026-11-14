package org.foundation.wikipedia.rust

import android.os.Looper
import java.io.File
import java.io.InputStream
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import uniffi.wiki_ffi.ArticlePage
import uniffi.wiki_ffi.HostedChatRequest
import uniffi.wiki_ffi.ArticleChat
import uniffi.wiki_ffi.InstallReviewer
import uniffi.wiki_ffi.InstalledPack
import uniffi.wiki_ffi.InstalledSnapshot
import uniffi.wiki_ffi.LibraryException
import uniffi.wiki_ffi.LocalChatRequest
import uniffi.wiki_ffi.OfflineLibrary
import uniffi.wiki_ffi.PackReview
import uniffi.wiki_ffi.PackSource
import uniffi.wiki_ffi.CitedReaderPage
import uniffi.wiki_ffi.ReaderPage
import uniffi.wiki_ffi.SearchHit

/**
 * Coroutine-safe access to the shared Rust offline library (MOB-001).
 *
 * Every call runs on [dispatcher] (I/O by default) and refuses to execute on
 * the main thread. Rust failures surface as typed [LibraryException]
 * subclasses. Coroutine cancellation is honoured before and after each call;
 * a call already inside Rust runs to completion (offline operations are
 * bounded). One instance owns its data directory; create it once per process.
 */
class RustOfflineLibrary private constructor(
    private val library: OfflineLibrary,
    private val dispatcher: CoroutineDispatcher,
) : AutoCloseable {

    companion object {
        /** Spool limit for one imported pack (8 GiB, the Rust maximum). */
        const val DEFAULT_MAX_PACK_BYTES: Long = 8L shl 30

        /** Open (creating if needed) the library in an app-private directory. */
        suspend fun open(
            dataDir: File,
            dispatcher: CoroutineDispatcher = Dispatchers.IO,
        ): RustOfflineLibrary = withContext(dispatcher) {
            requireOffMainThread()
            RustOfflineLibrary(OfflineLibrary(dataDir.absolutePath), dispatcher)
        }
    }

    /**
     * Verify and install a custom pack file as an inactive snapshot.
     * [acceptCustomPack] must be the user's explicit confirmation.
     */
    suspend fun installPack(pack: File, acceptCustomPack: Boolean): InstalledPack =
        call { it.installPack(pack.absolutePath, acceptCustomPack) }

    /**
     * Import a pack from a stream (e.g. an Android document-picker URI),
     * without a temporary copy. Rust fully verifies it, then calls [approve]
     * with the verified review; the pack is published only if it returns
     * true. [approve] may suspend (e.g. to show a dialog and wait).
     */
    suspend fun importPack(
        input: InputStream,
        maxBytes: Long = DEFAULT_MAX_PACK_BYTES,
        approve: suspend (PackReview) -> Boolean,
    ): InstalledPack {
        require(maxBytes > 0) { "maxBytes must be positive" }
        val source = object : PackSource {
            override fun read(maxBytes: UInt): ByteArray {
                val buffer = ByteArray(maxBytes.toInt())
                val count = input.read(buffer)
                return if (count <= 0) ByteArray(0) else buffer.copyOf(count)
            }
        }
        // Rust calls back on this (I/O) thread; wait for the UI's decision.
        // (Bind the lambda first: inside the object `approve` is the override.)
        val decide = approve
        val reviewer = object : InstallReviewer {
            override fun approve(review: PackReview): Boolean = runBlocking { decide(review) }
        }
        return call { it.installPackStream(source, reviewer, maxBytes.toULong()) }
    }

    suspend fun listPacks(): List<InstalledSnapshot> = call { it.listPacks() }

    /** Remove an inactive pack; the active pack is refused. */
    suspend fun removePack(snapshotId: String) = call { it.removePack(snapshotId) }

    suspend fun activate(snapshotId: String) = call { it.activate(snapshotId) }

    suspend fun activeSnapshot(): String? = call { it.activeSnapshot() }

    suspend fun search(query: String, limit: Int = 20): List<SearchHit> {
        require(limit in 1..100) { "limit must be between 1 and 100" }
        return call { it.search(query, limit.toUInt()) }
    }

    suspend fun openPage(project: String, pageId: Long): ArticlePage? {
        require(pageId > 0) { "pageId must be positive" }
        return call { it.openPage(project, pageId.toULong()) }
    }

    /** An article of the active pack as a sanitized reader document. */
    suspend fun openReaderPage(project: String, pageId: Long, dark: Boolean): ReaderPage? {
        require(pageId > 0) { "pageId must be positive" }
        return call { it.openReaderPage(project, pageId.toULong(), dark) }
    }

    /**
     * The exact article revision a chat citation points to, with the cited
     * passage highlighted; `null` if the active pack no longer holds it.
     */
    suspend fun openReaderCitation(
        project: String,
        pageId: Long,
        revisionId: Long,
        blockId: String,
        dark: Boolean,
    ): CitedReaderPage? {
        require(pageId > 0 && revisionId > 0) { "pageId and revisionId must be positive" }
        return call {
            it.openReaderCitation(project, pageId.toULong(), revisionId.toULong(), blockId, dark)
        }
    }

    /** Resolve an in-app article link by title within the active pack. */
    suspend fun openReaderTitle(project: String, title: String, dark: Boolean): ReaderPage? =
        call { it.openReaderTitle(project, title, dark) }

    /** See [RustHostedAi.prepareArticleChat]; the key comes from secure storage. */
    internal suspend fun prepareHostedChat(
        request: HostedChatRequest,
        apiKey: String,
        history: RustChatHistory?,
    ): ArticleChat = call { it.prepareHostedChat(request, apiKey, history?.history) }

    /** See [RustLocalAi.prepareArticleChat]. */
    internal suspend fun prepareLocalChat(
        request: LocalChatRequest,
        history: RustChatHistory?,
    ): ArticleChat = call { it.prepareLocalChat(request, history?.history) }

    override fun close() = library.close()

    private suspend fun <T> call(block: (OfflineLibrary) -> T): T = withContext(dispatcher) {
        requireOffMainThread()
        block(library)
    }
}

/** Blocking Rust work must never run on the UI thread. */
internal fun requireOffMainThread() {
    check(Looper.myLooper() != Looper.getMainLooper()) {
        "Rust library calls must not run on the main thread"
    }
}
