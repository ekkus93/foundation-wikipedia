package org.foundation.wikipedia.rust

import android.os.Looper
import java.io.File
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.wiki_ffi.ArticlePage
import uniffi.wiki_ffi.InstalledPack
import uniffi.wiki_ffi.LibraryException
import uniffi.wiki_ffi.OfflineLibrary
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
