package org.foundation.wikipedia

import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import org.foundation.wikipedia.rust.RustOfflineLibrary

internal const val IMAGE_VIEWER_TAG = "image-viewer"

/**
 * MOB-002 full-screen image: one pack image, alone on the screen, that the
 * reader can pinch to zoom and drag. It is drawn by a second WebView as
 * locked down as the article's (no scripts, storage, files or network): the
 * client answers only the viewer page and that one verified pack image, so a
 * vector diagram stays sharp at any zoom.
 */
@Composable
fun ImageViewer(
    digest: String,
    library: RustOfflineLibrary?,
    onClose: () -> Unit,
) {
    Dialog(
        onDismissRequest = onClose,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Box(
            modifier = Modifier.fillMaxSize()
                // White in both themes: diagrams are often black on transparency.
                .background(Color.White)
                .safeDrawingPadding()
                .testTag(IMAGE_VIEWER_TAG)
                .semantics { paneTitle = "Image, full screen. Pinch to zoom." },
        ) {
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
                        // Pinch and double-tap zoom, without on-screen buttons.
                        settings.setSupportZoom(true)
                        settings.builtInZoomControls = true
                        settings.displayZoomControls = false
                        settings.useWideViewPort = true
                        contentDescription = "Image"
                    }
                },
                update = { view ->
                    val url = ReaderLinks.viewerUrl(digest)
                    val html = ReaderLinks.viewerDocument(digest)
                    view.webViewClient = object : WebViewClient() {
                        // Nothing in the viewer leads anywhere.
                        override fun shouldOverrideUrlLoading(
                            view: WebView?,
                            request: WebResourceRequest?,
                        ): Boolean = true

                        override fun shouldInterceptRequest(
                            view: WebView?,
                            request: WebResourceRequest?,
                        ): WebResourceResponse {
                            val asked = request?.url?.toString() ?: return refuseResource()
                            if (ReaderLinks.isDocumentRequest(asked, request.isForMainFrame, url)) {
                                return documentResponse(html)
                            }
                            // Only the image being viewed.
                            if (ReaderLinks.imageDigest(asked, request.isForMainFrame) != digest) {
                                return refuseResource()
                            }
                            return imageResponse(library, digest)
                        }
                    }
                    if (view.tag != html) {
                        view.tag = html
                        view.loadUrl(url)
                    }
                },
            )
            FilledTonalButton(
                onClick = onClose,
                modifier = Modifier.align(Alignment.TopEnd).padding(12.dp),
            ) { Text("Close") }
        }
    }
}
