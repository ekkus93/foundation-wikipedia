package org.foundation.wikipedia

import android.view.View
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import java.io.ByteArrayInputStream
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.launch
import org.foundation.wikipedia.rust.RustOfflineLibrary
import uniffi.wiki_ffi.ReaderPage
import uniffi.wiki_ffi.SearchHit

/** What the reader shows in the article WebView. */
private sealed interface ReaderContent {
    data object Welcome : ReaderContent
    /** [fragment] scrolls to a highlighted citation; [nonce] repeats the scroll. */
    data class Article(val page: ReaderPage, val fragment: String? = null, val nonce: Long = 0) : ReaderContent
}

/**
 * Every request other than the current document is refused: the document has
 * no scripts or styles to fetch, image sources are removed until verified
 * offline media exists (PACK-003), and the reserved origin never reaches the
 * network. Verified local media will be served here from the active pack.
 */
internal fun refuseResource(): WebResourceResponse =
    WebResourceResponse("text/plain", "utf-8", 403, "Forbidden", emptyMap(), ByteArrayInputStream(ByteArray(0)))

/** The current document, served by the WebView client for its exact URL. */
internal fun documentResponse(html: String): WebResourceResponse = WebResourceResponse(
    "text/html",
    "utf-8",
    200,
    "OK",
    mapOf("Cache-Control" to "no-store"),
    ByteArrayInputStream(html.toByteArray(Charsets.UTF_8)),
)

private const val WELCOME_HTML = """
<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'">
<style>
body{margin:24px;font:16px/1.6 sans-serif;color:#202122;background:#fff}
:root.dark body{color:#eaecf0;background:#101418}
h1{font:400 28px Georgia,serif;border-bottom:1px solid #a2a9b1}
</style></head><body>
<h1>Foundation Wikipedia</h1>
<p>Search above to read articles from your active offline pack.</p>
<p>Install or activate a pack in Settings › Wikipedia &amp; Offline Data.
Online reading is not available on Android yet.</p>
</body></html>
"""

/**
 * MOB-002 offline article reader: search the active pack, read a verified
 * article in a locked-down WebView, follow article links within the pack and
 * go Back through the articles opened.
 */
@Composable
fun ArticleReader(
    dark: Boolean,
    hidden: Boolean,
    hideFromAccessibility: Boolean,
    onMessage: (message: String, externalUrl: String?) -> Unit,
    modifier: Modifier = Modifier,
    citation: CitationRequest? = null,
    onCitationConsumed: () -> Unit = {},
    onArticleShown: (ArticleChatTarget?) -> Unit = {},
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var library by remember { mutableStateOf<RustOfflineLibrary?>(null) }
    var navigation by rememberSaveable(
        stateSaver = androidx.compose.runtime.saveable.listSaver<ReaderNavigation, String>(
            save = { it.save() },
            restore = { ReaderNavigation.restore(it) },
        ),
    ) { mutableStateOf(ReaderNavigation()) }
    var content by remember { mutableStateOf<ReaderContent>(ReaderContent.Welcome) }
    var query by rememberSaveable { mutableStateOf("") }
    var results by remember { mutableStateOf<List<SearchHit>?>(null) }
    var busy by remember { mutableStateOf(false) }
    val served = remember { AtomicReference<Pair<String, String>?>(null) }
    // The chat citation to highlight while its article is the current entry.
    var highlight by remember { mutableStateOf<CitationRequest?>(null) }

    LaunchedEffect(citation) {
        val request = citation ?: return@LaunchedEffect
        onCitationConsumed()
        results = null
        highlight = request
        navigation = navigation.open(
            ReaderEntry(request.target.project, request.target.pageId, request.target.title),
        )
    }

    LaunchedEffect(Unit) {
        library = try {
            OfflineLibraryHolder.get(context)
        } catch (_: LinkageError) {
            onMessage("The offline reader is not available in this build.", null)
            null
        } catch (error: Exception) {
            onMessage(packErrorMessage(error), null)
            null
        }
    }

    // Load (or reload, e.g. on theme change or after restore) the current entry.
    LaunchedEffect(library, navigation.current, dark, highlight) {
        val lib = library ?: return@LaunchedEffect
        val entry = navigation.current
        if (entry == null) {
            content = ReaderContent.Welcome
            return@LaunchedEffect
        }
        val cite = highlight?.takeIf {
            it.target.project == entry.project && it.target.pageId == entry.pageId
        }
        if (cite != null) {
            val cited = try {
                lib.openReaderCitation(
                    cite.target.project,
                    cite.target.pageId,
                    cite.target.revisionId,
                    cite.blockId,
                    dark,
                )
            } catch (error: Exception) {
                onMessage(packErrorMessage(error), null)
                null
            }
            if (cited == null) {
                onMessage("The cited revision of “${entry.title}” is not in the active offline pack.", null)
                highlight = null // reloads the current page without a highlight
            } else {
                if (!cited.located) {
                    onMessage("The cited passage is shown at the top: its exact place was not found.", null)
                }
                content = ReaderContent.Article(cited.page, cited.fragment, cite.nonce)
            }
            return@LaunchedEffect
        }
        content = try {
            lib.openReaderPage(entry.project, entry.pageId, dark)
                ?.let { ReaderContent.Article(it) }
                ?: run {
                    onMessage("“${entry.title}” is not in the active offline pack.", null)
                    ReaderContent.Welcome
                }
        } catch (error: Exception) {
            onMessage(packErrorMessage(error), null)
            ReaderContent.Welcome
        }
    }

    LaunchedEffect(content) {
        onArticleShown(
            (content as? ReaderContent.Article)?.page?.let {
                ArticleChatTarget(it.project, it.pageId.toLong(), it.revisionId.toLong(), it.title)
            },
        )
    }

    fun openPage(project: String, pageId: Long, title: String) {
        results = null
        highlight = null
        navigation = navigation.open(ReaderEntry(project, pageId, title))
    }

    fun followLink(title: String) {
        val lib = library ?: return
        val project = (content as? ReaderContent.Article)?.page?.project ?: return
        scope.launch {
            try {
                val page = lib.openReaderTitle(project, title, dark)
                if (page == null) {
                    onMessage("“$title” is not in the active offline pack.", null)
                } else {
                    openPage(page.project, page.pageId.toLong(), page.title)
                }
            } catch (error: Exception) {
                onMessage(packErrorMessage(error), null)
            }
        }
    }

    fun search() {
        val lib = library ?: return
        val text = query.trim()
        if (text.isEmpty()) return
        scope.launch {
            busy = true
            results = try {
                lib.search(text)
            } catch (error: Exception) {
                onMessage(packErrorMessage(error), null)
                null
            } finally {
                busy = false
            }
        }
    }

    BackHandler(enabled = !hidden && (results != null || navigation.canGoBack)) {
        if (results != null) results = null else navigation = navigation.back()
    }

    Column(modifier = modifier.fillMaxSize()) {
        if (!hidden) {
            Row(
                modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it.take(200) },
                    label = { Text("Search offline articles") },
                    singleLine = true,
                    enabled = library != null,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    keyboardActions = KeyboardActions(onSearch = { search() }),
                    modifier = Modifier.weight(1f),
                )
                TextButton(enabled = library != null && !busy, onClick = { search() }) {
                    Text("Search")
                }
            }
        }
        Box(modifier = Modifier.fillMaxSize()) {
            AndroidView(
                modifier = Modifier.fillMaxSize(),
                factory = { viewContext ->
                    WebView(viewContext).apply {
                        settings.javaScriptEnabled = false
                        settings.javaScriptCanOpenWindowsAutomatically = false
                        settings.domStorageEnabled = false
                        settings.allowFileAccess = false
                        settings.allowContentAccess = false
                        settings.blockNetworkLoads = true
                        settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
                        settings.setSupportMultipleWindows(false)
                    }
                },
                update = { view ->
                    view.webViewClient = object : WebViewClient() {
                        override fun shouldOverrideUrlLoading(
                            view: WebView?,
                            request: WebResourceRequest?,
                        ): Boolean {
                            val url = request?.url?.toString() ?: return true
                            val current = (content as? ReaderContent.Article)?.page?.baseUrl
                            return when (val link = ReaderLinks.classify(url, current)) {
                                is ReaderLink.InPage -> false
                                is ReaderLink.Article -> {
                                    followLink(link.title)
                                    true
                                }
                                is ReaderLink.External -> {
                                    onMessage("This link leads outside the offline reader.", link.url)
                                    true
                                }
                                ReaderLink.Blocked -> true
                            }
                        }

                        // Runs on a WebView background thread: read only the
                        // atomically published current document.
                        override fun shouldInterceptRequest(
                            view: WebView?,
                            request: WebResourceRequest?,
                        ): WebResourceResponse {
                            val shown = served.get() ?: return refuseResource()
                            val url = request?.url?.toString() ?: return refuseResource()
                            return if (ReaderLinks.isDocumentRequest(url, request.isForMainFrame, shown.first)) {
                                documentResponse(shown.second)
                            } else {
                                refuseResource()
                            }
                        }
                    }
                    val shown = content
                    val (url, html) = when (shown) {
                        is ReaderContent.Article -> shown.page.baseUrl to shown.page.document
                        ReaderContent.Welcome -> ReaderLinks.ORIGIN + "/" to
                            if (dark) WELCOME_HTML.replace("<html lang=\"en\">", "<html lang=\"en\" class=\"dark\">")
                            else WELCOME_HTML
                    }
                    val fragment = (shown as? ReaderContent.Article)?.fragment
                    val nonce = (shown as? ReaderContent.Article)?.nonce ?: 0
                    // Reload only when the document (or a citation tap) changes, so
                    // scrolling and in-page anchors survive unrelated recompositions.
                    val tag = "$nonce|$html"
                    if (view.tag != tag) {
                        view.tag = tag
                        // The document is served by the client above for this
                        // URL only (loadDataWithBaseURL with an https base is
                        // blocked as a network load by blockNetworkLoads). A
                        // fragment is not part of the request; it scrolls the
                        // loaded page to the highlighted citation.
                        served.set(url to html)
                        view.loadUrl(if (fragment != null) "$url#$fragment" else url)
                    }
                    view.visibility = if (hidden) View.INVISIBLE else View.VISIBLE
                    view.importantForAccessibility = if (hidden || hideFromAccessibility) {
                        View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
                    } else {
                        View.IMPORTANT_FOR_ACCESSIBILITY_AUTO
                    }
                },
            )
            val hits = results
            if (!hidden && hits != null) {
                LazyColumn(
                    modifier = Modifier.fillMaxSize()
                        .background(MaterialTheme.colorScheme.surface)
                        .semantics { contentDescription = "Search results" },
                ) {
                    if (hits.isEmpty()) {
                        item {
                            Text(
                                "No articles in the active pack match “${query.trim()}”.",
                                modifier = Modifier.padding(16.dp),
                            )
                        }
                    }
                    items(hits, key = { "${it.project}:${it.pageId}" }) { hit ->
                        Text(
                            hit.title,
                            style = MaterialTheme.typography.titleMedium,
                            modifier = Modifier.fillMaxWidth()
                                .clickable { openPage(hit.project, hit.pageId.toLong(), hit.title) }
                                .padding(horizontal = 16.dp, vertical = 14.dp),
                        )
                        HorizontalDivider()
                    }
                }
            }
        }
    }
}
