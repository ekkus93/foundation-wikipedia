package org.foundation.wikipedia

import android.os.Build
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.ui.Alignment
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.launch

/** Native Compose application shell; Wikimedia loading will be bound through Rust. */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { FoundationApp() }
    }
}

private const val BOOTSTRAP_HTML = """
<!doctype html>
<html lang="en">
<head>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
body{margin:24px;color:#202122;font:16px/1.65 Georgia,serif}
h1{font-weight:400;font-size:30px;border-bottom:1px solid #a2a9b1}
h2{font-weight:400;border-bottom:1px solid #a2a9b1}
.note{color:#54595d;font:12px sans-serif}
</style>
</head>
<body>
<p class="note">FOUNDATION WIKIPEDIA · DEVELOPMENT PREVIEW</p>
<h1>Welcome to Foundation Wikipedia</h1>
<p>A Wikipedia-first reader with optional AI assistance and offline packs.</p>
<h2>Under development</h2>
<p>This is a local placeholder, not a downloaded Wikipedia article. Wikimedia
content, offline packs, article citation bridges and AI are not implemented yet.</p>
</body>
</html>
"""

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FoundationApp() {
    val context = LocalContext.current
    val isDark = isSystemInDarkTheme()
    val scheme = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        if (isDark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
    } else {
        if (isDark) darkColorScheme() else lightColorScheme()
    }
    var expanded by remember { mutableStateOf(false) }
    val snackbarHostState = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val chatActionFocus = remember { FocusRequester() }

    // Back dismisses the action menu before leaving the reader.
    BackHandler(enabled = expanded) { expanded = false }
    LaunchedEffect(expanded) {
        if (expanded) chatActionFocus.requestFocus()
    }

    MaterialTheme(colorScheme = scheme) {
        Scaffold(
            topBar = { TopAppBar(title = { Text("Wikipedia") }) },
            snackbarHost = { SnackbarHost(hostState = snackbarHostState) },
            floatingActionButton = {
                Column(horizontalAlignment = Alignment.End) {
                    // Visually nearest the main FAB: Chat, Bookmark, Offline, Settings.
                    if (expanded) {
                        for (label in listOf("Settings", "Offline", "Bookmark", "Chat")) {
                            OutlinedButton(onClick = {
                                expanded = false
                                scope.launch {
                                    snackbarHostState.showSnackbar(
                                        "$label is not implemented in this development shell"
                                    )
                                }
                            },
                                modifier = if (label == "Chat") Modifier.focusRequester(chatActionFocus) else Modifier
                            ) {
                                Text(label)
                            }
                        }
                    }
                    FloatingActionButton(
                        onClick = { expanded = !expanded },
                        modifier = Modifier.semantics {
                            contentDescription = if (expanded) "Close reader actions" else "Open reader actions"
                        }
                    ) {
                        Text(if (expanded) "Close" else "Actions")
                    }
                }
            }
        ) { padding ->
            AndroidView(
                modifier = Modifier.fillMaxSize().padding(padding),
                factory = { viewContext ->
                    WebView(viewContext).apply {
                        settings.javaScriptEnabled = false
                        settings.allowFileAccess = false
                        settings.allowContentAccess = false
                        loadDataWithBaseURL(
                            "https://foundation.invalid/",
                            BOOTSTRAP_HTML,
                            "text/html",
                            "UTF-8",
                            null
                        )
                    }
                }
            )
        }
    }
}
