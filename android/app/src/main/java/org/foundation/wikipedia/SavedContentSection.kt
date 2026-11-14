package org.foundation.wikipedia

import android.content.Context
import android.text.format.DateUtils
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.foundation.wikipedia.rust.RustReaderState
import uniffi.wiki_ffi.ReaderStateException
import uniffi.wiki_ffi.SavedContent
import uniffi.wiki_ffi.SavedPage

/** One shared bookmarks/history store per process. */
object ReaderStateHolder {
    private val lock = Mutex()
    private var state: RustReaderState? = null

    suspend fun get(context: Context): RustReaderState = lock.withLock {
        state ?: RustReaderState.open(context).also { state = it }
    }
}

fun readerStateErrorMessage(error: Throwable): String = when (error) {
    is ReaderStateException.Changed -> "Saved data changed meanwhile. Try again."
    is ReaderStateException.InvalidInput -> "That article cannot be saved."
    is LinkageError -> "Bookmarks and history are not available in this build."
    else -> "Could not read or save bookmarks and history on this device."
}

private fun SavedPage.label(): String = title ?: "Page $pageId (not in the active offline pack)"

/**
 * Settings › History & Saved Content (MOB-004): the history choice,
 * bookmarks and recent history, each opening the article in the reader.
 * Everything is kept on this device only.
 */
@Composable
fun SavedContentSection(
    historyEnabled: Boolean,
    historyPersistenceFailed: Boolean,
    onHistoryChoice: (Boolean) -> Unit,
    onOpen: (ReaderEntry) -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var saved by remember { mutableStateOf<SavedContent?>(null) }
    var message by remember { mutableStateOf<String?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    var confirmClear by remember { mutableStateOf(false) }

    LaunchedEffect(reload, historyEnabled) {
        saved = try {
            OfflineLibraryHolder.get(context).savedContent(ReaderStateHolder.get(context))
        } catch (error: Exception) {
            message = readerStateErrorMessage(error)
            null
        } catch (error: LinkageError) {
            message = readerStateErrorMessage(error)
            null
        }
    }

    fun act(block: suspend (RustReaderState) -> Unit) {
        scope.launch {
            try {
                block(ReaderStateHolder.get(context))
            } catch (error: Exception) {
                message = readerStateErrorMessage(error)
            }
            reload++
        }
    }

    Text("History & Saved Content", style = MaterialTheme.typography.titleLarge)
    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text("Save reading history", modifier = Modifier.weight(1f))
        Switch(checked = historyEnabled, onCheckedChange = onHistoryChoice)
    }
    Text(
        if (historyEnabled) {
            "Articles you open are saved in your history on this device, and chats are saved."
        } else {
            "Articles you open are not recorded and chats are not saved. Existing history " +
                "stays until you clear it."
        },
        style = MaterialTheme.typography.bodySmall,
    )
    if (historyPersistenceFailed) {
        Text(
            "Could not save this choice. History stays off for this session; an older " +
                "saved choice may return after restart.",
            color = MaterialTheme.colorScheme.error,
        )
    }
    message?.let { Text(it, color = MaterialTheme.colorScheme.error) }

    val content = saved
    HorizontalDivider()
    Text("Bookmarks", style = MaterialTheme.typography.titleMedium)
    when {
        content == null -> Text("Loading…")
        content.bookmarks.isEmpty() -> Text(
            "No bookmarks. Use Bookmark in the reader's actions to add the open article.",
            style = MaterialTheme.typography.bodySmall,
        )
        else -> for (page in content.bookmarks) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                PageRow(page, Modifier.weight(1f), onOpen)
                TextButton(
                    onClick = { act { it.removeBookmark(page.project, page.pageId.toLong()) } },
                    modifier = Modifier.semantics { contentDescription = "Remove bookmark for ${page.label()}" },
                ) { Text("Remove") }
            }
        }
    }

    HorizontalDivider()
    Text("History", style = MaterialTheme.typography.titleMedium)
    when {
        content == null -> {}
        content.history.isEmpty() -> Text(
            if (historyEnabled) "No articles opened yet." else "No saved history.",
            style = MaterialTheme.typography.bodySmall,
        )
        else -> {
            for (page in content.history) PageRow(page, Modifier.fillMaxWidth(), onOpen)
            OutlinedButton(onClick = { confirmClear = true }) { Text("Clear history") }
        }
    }
    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            title = { Text("Clear history?") },
            text = { Text("This deletes the list of articles you opened on this device. Bookmarks are kept.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmClear = false
                    act { it.clearHistory() }
                }) { Text("Clear") }
            },
            dismissButton = { TextButton(onClick = { confirmClear = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun PageRow(page: SavedPage, modifier: Modifier, onOpen: (ReaderEntry) -> Unit) {
    val title = page.title
    Column(
        modifier = modifier
            .heightIn(min = 48.dp)
            .clickable(enabled = title != null) {
                onOpen(ReaderEntry(page.project, page.pageId.toLong(), title!!))
            }
            .padding(vertical = 6.dp),
        verticalArrangement = Arrangement.Center,
    ) {
        Text(page.label())
        page.visitedAt?.let {
            Text(
                DateUtils.getRelativeTimeSpanString(it * 1000).toString(),
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}
