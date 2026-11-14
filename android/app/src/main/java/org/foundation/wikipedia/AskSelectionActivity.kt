package org.foundation.wikipedia

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * Text the user chose to ask about from the reader's selection menu,
 * waiting for the chat sheet to take it.
 */
object ReaderSelection {
    /** Longer selections are cut; Rust then matches them to one passage. */
    const val MAX_CHARS = 2000

    private val pending = MutableStateFlow<String?>(null)
    val selections: StateFlow<String?> = pending

    fun offer(text: String?) {
        val trimmed = text?.trim()?.take(MAX_CHARS)
        if (!trimmed.isNullOrEmpty()) pending.value = trimmed
    }

    /** Take the pending selection, once. */
    fun take(): String? {
        val text = pending.value
        pending.value = null
        return text
    }
}

/**
 * MOB-003 "Ask about this": the WebView's text-selection menu starts this
 * private (non-exported) activity with the selected text, which it hands to
 * the chat sheet and closes. Nothing is sent anywhere from here.
 */
class AskSelectionActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (intent?.action == Intent.ACTION_PROCESS_TEXT) {
            ReaderSelection.offer(intent.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT)?.toString())
        }
        // Read-only: the selection in the page is never replaced.
        setResult(RESULT_CANCELED)
        finish()
    }
}
