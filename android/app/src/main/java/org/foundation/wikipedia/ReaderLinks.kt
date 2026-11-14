package org.foundation.wikipedia

import java.net.URI
import java.net.URISyntaxException

/** Where a tapped link in an article document may go (MOB-002). */
sealed interface ReaderLink {
    /** Another article, resolved by title within the active offline pack. */
    data class Article(val title: String, val fragment: String?) : ReaderLink

    /** An anchor in the current document; the WebView scrolls to it. */
    data class InPage(val fragment: String) : ReaderLink

    /** An outside web page; opened only by explicit user choice. */
    data class External(val url: String) : ReaderLink

    /** Anything else (other schemes, malformed URLs): ignored. */
    data object Blocked : ReaderLink
}

/**
 * Classifies URLs from documents built by `wiki_source::reader_document`,
 * which rewrites article links to [ORIGIN]`/wiki/<Title>`. Pure, so it can be
 * unit-tested without Android.
 */
object ReaderLinks {
    const val ORIGIN = "https://foundation.invalid"
    private const val HOST = "foundation.invalid"
    private const val ARTICLE_PATH = "/wiki/"
    private const val MAX_TITLE_LENGTH = 512

    /**
     * Whether a WebView request asks for exactly the document currently
     * shown (same reserved origin and decoded path, no query). Only that
     * main-frame request is answered with the document; all others are
     * refused.
     */
    fun isDocumentRequest(url: String, mainFrame: Boolean, documentUrl: String?): Boolean {
        if (!mainFrame || documentUrl == null) return false
        return try {
            val request = URI(url)
            val document = URI(documentUrl)
            request.scheme.equals("https", ignoreCase = true) &&
                request.host.equals(HOST, ignoreCase = true) &&
                request.port == -1 && request.rawQuery == null && request.userInfo == null &&
                request.path != null && request.path == document.path &&
                document.host.equals(HOST, ignoreCase = true)
        } catch (_: URISyntaxException) {
            false
        }
    }

    fun classify(url: String, currentDocumentUrl: String?): ReaderLink {
        val uri = try {
            URI(url)
        } catch (_: URISyntaxException) {
            return ReaderLink.Blocked
        }
        val scheme = uri.scheme?.lowercase() ?: return ReaderLink.Blocked
        if (scheme != "https" && scheme != "http") return ReaderLink.Blocked
        val host = uri.host?.lowercase() ?: return ReaderLink.Blocked
        if (host != HOST) {
            return if (uri.userInfo == null) ReaderLink.External(url) else ReaderLink.Blocked
        }
        if (scheme != "https" || uri.port != -1 || uri.rawQuery != null) return ReaderLink.Blocked
        val path = uri.path ?: return ReaderLink.Blocked
        if (!path.startsWith(ARTICLE_PATH)) return ReaderLink.Blocked
        val fragment = uri.fragment?.takeIf { it.isNotEmpty() }
        val current = currentDocumentUrl?.let {
            try {
                URI(it).path
            } catch (_: URISyntaxException) {
                null
            }
        }
        if (current == path) {
            return fragment?.let { ReaderLink.InPage(it) } ?: ReaderLink.Blocked
        }
        val title = path.removePrefix(ARTICLE_PATH).replace('_', ' ').trim()
        if (title.isEmpty() || title.length > MAX_TITLE_LENGTH || title.any { it.isISOControl() }) {
            return ReaderLink.Blocked
        }
        return ReaderLink.Article(title, fragment)
    }
}

/** One opened article in the reader's back stack. */
data class ReaderEntry(val project: String, val pageId: Long, val title: String)

/**
 * The reader's in-app history: Back returns to the previous article. Bounded
 * so a long reading session cannot grow without limit.
 */
data class ReaderNavigation(val entries: List<ReaderEntry> = emptyList()) {
    val current: ReaderEntry? get() = entries.lastOrNull()
    val canGoBack: Boolean get() = entries.size > 1

    fun open(entry: ReaderEntry): ReaderNavigation {
        if (current?.project == entry.project && current?.pageId == entry.pageId) return this
        return ReaderNavigation((entries + entry).takeLast(MAX_DEPTH))
    }

    fun back(): ReaderNavigation =
        if (canGoBack) ReaderNavigation(entries.dropLast(1)) else this

    /** Flat form for saved instance state. */
    fun save(): List<String> = entries.flatMap { listOf(it.project, it.pageId.toString(), it.title) }

    companion object {
        const val MAX_DEPTH = 50

        /** Restores [save] output; malformed input yields an empty stack. */
        fun restore(saved: List<*>?): ReaderNavigation {
            if (saved == null || saved.size % 3 != 0) return ReaderNavigation()
            val entries = saved.chunked(3).map { (project, pageId, title) ->
                val id = (pageId as? String)?.toLongOrNull()
                if (project !is String || title !is String || id == null || id <= 0) {
                    return ReaderNavigation()
                }
                ReaderEntry(project, id, title)
            }
            return ReaderNavigation(entries.takeLast(MAX_DEPTH))
        }
    }
}
