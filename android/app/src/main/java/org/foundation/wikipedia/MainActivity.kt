package org.foundation.wikipedia

import android.os.Build
import android.os.Bundle
import android.view.View
import android.content.Context
import android.webkit.WebView
import android.webkit.WebViewClient
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Row
import androidx.compose.ui.Alignment
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.TextButton
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
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
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
    val appearancePreferences = remember(context) {
        context.getSharedPreferences(ReaderAppearancePreference.FILE, Context.MODE_PRIVATE)
    }
    val initialTheme = remember(appearancePreferences) {
        // Legacy wrong-type preferences must not crash the reader.
        try {
            ReaderAppearancePreference.decode(
                appearancePreferences.getString(ReaderAppearancePreference.KEY, null)
            )
        } catch (_: ClassCastException) {
            ReaderThemeMode.SYSTEM
        }
    }
    var themeName by rememberSaveable { mutableStateOf(initialTheme.name) }
    val themeMode = ReaderThemeMode.entries.firstOrNull { it.name == themeName }
        ?: ReaderThemeMode.SYSTEM
    LaunchedEffect(themeName, appearancePreferences) {
        appearancePreferences.edit()
            .putString(ReaderAppearancePreference.KEY, ReaderAppearancePreference.encode(themeMode))
            .apply()
    }
    val isDark = when (themeMode) {
        ReaderThemeMode.SYSTEM -> isSystemInDarkTheme()
        ReaderThemeMode.LIGHT -> false
        ReaderThemeMode.DARK -> true
    }
    val scheme = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        if (isDark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
    } else {
        if (isDark) darkColorScheme() else lightColorScheme()
    }
    val offlinePreferences = remember(context) {
        context.getSharedPreferences(ReaderOfflinePreference.FILE, Context.MODE_PRIVATE)
    }
    val initialOffline = remember(offlinePreferences) {
        // A malformed or newer stored preference must not silently enable network access.
        try {
            ReaderOfflinePreference.decode(
                offlinePreferences.getString(ReaderOfflinePreference.KEY, null)
            )
        } catch (_: ClassCastException) {
            true
        }
    }
    // Consent only: article/history persistence is not wired to Rust yet.
    val historyPreferences = remember(context) {
        context.getSharedPreferences(ReaderOfflinePreference.FILE, Context.MODE_PRIVATE)
    }
    val initialHistoryEnabled = remember(historyPreferences) {
        try {
            ReaderHistoryPreference.decode(
                historyPreferences.getString(ReaderHistoryPreference.KEY, null)
            )
        } catch (_: ClassCastException) {
            false
        }
    }
    // Consent changes are synchronous: a deferred apply() could be lost on process death.
    // Do not restore a stale saveable consent value over the durable preference.
    var historyEnabled by remember { mutableStateOf(initialHistoryEnabled) }
    var historyPersistenceFailed by remember { mutableStateOf(false) }
    val shellSaver = remember {
        listSaver<ReaderShellState, Any>(
            save = {
                listOf(it.offline, it.settingsOpen, it.settingsCategory?.name ?: "")
            },
            restore = {
                ReaderShellState.restore(
                    it[0] as Boolean,
                    it[1] as Boolean,
                    it.getOrNull(2) as? String,
                )
            },
        )
    }
    var shell by rememberSaveable(stateSaver = shellSaver) {
        mutableStateOf(ReaderShellState(offline = initialOffline))
    }
    LaunchedEffect(shell.offline, offlinePreferences) {
        offlinePreferences.edit()
            .putString(ReaderOfflinePreference.KEY, ReaderOfflinePreference.encode(shell.offline))
            .apply()
    }
    val snackbarHostState = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val chatActionFocus = remember { FocusRequester() }

    BackHandler(enabled = shell.actionsOpen || shell.settingsOpen || shell.chatOpen) {
        shell = shell.back()
    }
    LaunchedEffect(shell.actionsOpen) {
        if (shell.actionsOpen) chatActionFocus.requestFocus()
    }

    MaterialTheme(colorScheme = scheme) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = {
                        Text(
                            if (shell.settingsOpen) shell.settingsCategory?.title ?: "Settings"
                            else "Wikipedia"
                        )
                    },
                    navigationIcon = {
                        if (shell.settingsOpen) {
                            TextButton(onClick = { shell = shell.back() }) {
                                Text(if (shell.settingsCategory != null) "← Settings" else "← Reader")
                            }
                        }
                    },
                )
            },
            snackbarHost = { SnackbarHost(hostState = snackbarHostState) },
            floatingActionButton = {
                if (!shell.settingsOpen) {
                    Column(horizontalAlignment = Alignment.End) {
                        // Visually nearest the main FAB: Chat, Bookmark, Offline, Settings.
                        if (shell.actionsOpen) {
                            for (label in listOf("Settings", "Offline", "Bookmark", "Chat")) {
                                OutlinedButton(
                                    onClick = {
                                        when (label) {
                                            "Settings" -> shell = shell.showSettings()
                                            "Offline" -> shell = shell.withOffline(!shell.offline)
                                            "Chat" -> shell = shell.showChat()
                                            else -> {
                                                shell = shell.copy(actionsOpen = false)
                                                scope.launch {
                                                    snackbarHostState.showSnackbar(
                                                        "$label requires a loaded, verified article; this build has none."
                                                    )
                                                }
                                            }
                                        }
                                    },
                                    modifier = if (label == "Chat") {
                                        Modifier.focusRequester(chatActionFocus)
                                    } else {
                                        Modifier
                                    },
                                ) {
                                    Text(if (label == "Offline" && shell.offline) "Go online" else label)
                                }
                            }
                        }
                        FloatingActionButton(
                            onClick = { shell = shell.toggleActions() },
                            modifier = Modifier.semantics {
                                contentDescription = if (shell.actionsOpen) {
                                    "Close reader actions"
                                } else {
                                    "Open reader actions"
                                }
                            },
                        ) {
                            Text(if (shell.actionsOpen) "Close" else "Actions")
                        }
                    }
                }
            },
        ) { padding ->
            Box(modifier = Modifier.fillMaxSize().padding(padding)) {
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
                            webViewClient = object : WebViewClient() {
                                override fun shouldOverrideUrlLoading(
                                    view: WebView?,
                                    request: WebResourceRequest?,
                                ): Boolean = true
                            }
                            loadDataWithBaseURL(
                                "https://foundation.invalid/",
                                BOOTSTRAP_HTML,
                                "text/html",
                                "UTF-8",
                                null,
                            )
                        }
                    },
                    update = { view ->
                        // Keep the same WebView mounted while Settings is open:
                        // no reload or loss of the current in-memory scroll position.
                        view.visibility = if (shell.settingsOpen) View.INVISIBLE else View.VISIBLE
                        view.importantForAccessibility = if (shell.settingsOpen || shell.actionsOpen || shell.chatOpen) {
                            View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
                        } else {
                            View.IMPORTANT_FOR_ACCESSIBILITY_AUTO
                        }
                    },
                )
                if (shell.actionsOpen && !shell.settingsOpen) {
                    // Intercept outside taps without reloading or obscuring the reader.
                    // The Scaffold FAB and its action buttons remain above this overlay.
                    Box(
                        modifier = Modifier.fillMaxSize().clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                            onClick = { shell = shell.dismissActions() },
                        ).semantics { contentDescription = "Dismiss reader actions" },
                    )
                }
                if (shell.settingsOpen) {
                    Column(
                        modifier = Modifier.fillMaxSize()
                            .background(MaterialTheme.colorScheme.background)
                            .verticalScroll(rememberScrollState()).padding(20.dp),
                    ) {
                        val category = shell.settingsCategory
                        if (category == null) {
                            Text("Settings", style = MaterialTheme.typography.titleLarge)
                            for (item in ReaderSettingsCategory.entries) {
                                OutlinedButton(
                                    onClick = { shell = shell.openSettingsCategory(item) },
                                    modifier = Modifier.fillMaxWidth(),
                                ) {
                                    Text(item.title)
                                }
                            }
                        } else if (category == ReaderSettingsCategory.APPEARANCE_READING) {
                            Text("Appearance & Reading", style = MaterialTheme.typography.titleLarge)
                            Text("Color theme")
                            for (mode in ReaderThemeMode.entries) {
                                Row(
                                    modifier = Modifier.fillMaxWidth().clickable {
                                        themeName = mode.name
                                    },
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    RadioButton(selected = themeMode == mode, onClick = null)
                                    Text(mode.title)
                                }
                            }
                            Text(
                                "System follows the Android appearance setting. Light and Dark " +
                                    "override it for this app. Other reading controls are not available yet.",
                            )
                        } else if (category == ReaderSettingsCategory.HISTORY_SAVED_CONTENT) {
                            Text("History & Saved Content", style = MaterialTheme.typography.titleLarge)
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text("Allow saving reading history", modifier = Modifier.weight(1f))
                                Switch(
                                    checked = historyEnabled,
                                    onCheckedChange = { requested ->
                                        val update = ReaderHistoryPreference.update(requested) { value ->
                                            historyPreferences.edit()
                                                .putString(ReaderHistoryPreference.KEY, value)
                                                .commit()
                                        }
                                        historyEnabled = update.enabled
                                        historyPersistenceFailed = !update.saved
                                    },
                                )
                            }
                            Text(
                                if (historyEnabled) {
                                    "History consent is on. No reading history is currently " +
                                        "recorded: verified articles and Rust storage are not connected."
                                } else {
                                    "History consent is off. Reading history is not recorded."
                                },
                            )
                            if (historyPersistenceFailed) {
                                Text(
                                    "Could not save reading-history consent. History stays disabled " +
                                        "for this session; an older saved choice may reappear after " +
                                        "restart. No history is collected by this preview."
                                )
                            }
                            Text(
                                "This preference does not enable article loading, create bookmarks, " +
                                    "or delete history from another installation."
                            )
                        } else if (category == ReaderSettingsCategory.WIKIPEDIA_OFFLINE_DATA) {
                            Text("Reading & offline", style = MaterialTheme.typography.titleLarge)
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text("Work offline", modifier = Modifier.weight(1f))
                                Switch(
                                    checked = shell.offline,
                                    onCheckedChange = { shell = shell.withOffline(it) },
                                )
                            }
                            Text(
                                if (shell.offline) "Offline mode selected on this device."
                                else "Offline mode is off. Online article fetching is not implemented yet.",
                            )
                            Text(
                                "No verified Wikipedia pack is installed. This development WebView never " +
                                    "requests network resources, even when Work offline is off.",
                            )
                            Text(
                                "The offline choice persists across app restarts. It does not download " +
                                    "articles, enable bookmarks, or certify missing images and attribution.",
                            )
                        } else {
                            Text(category.title, style = MaterialTheme.typography.titleLarge)
                            Text(
                                "No controls are available for this category in the development build. " +
                                    "Reader, AI and pack capabilities must be verified before settings " +
                                    "for those features are enabled.",
                            )
                        }
                    }
                }
            }
        }
        if (shell.chatOpen) {
            // Presentation-only: no unverified article data or prompts are sent.
            ModalBottomSheet(onDismissRequest = { shell = shell.closeChat() }) {
                Column(modifier = Modifier.fillMaxWidth().padding(24.dp)) {
                    Text("Article chat", style = MaterialTheme.typography.titleLarge)
                    Text(
                        "Chat is unavailable in this development build. " +
                            "A verified Wikipedia article, revision-scoped citations, " +
                            "and an explicitly configured AI provider are required.",
                    )
                    TextButton(onClick = { shell = shell.closeChat() }) {
                        Text("Close chat")
                    }
                }
            }
        }
    }
}