package org.foundation.wikipedia

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.widget.Toast
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
 * MOB-003 "Ask about this": the reader WebView's text-selection menu starts
 * this activity with the selected text, which it hands to the chat sheet
 * and closes. Nothing is sent anywhere from here.
 *
 * It must be exported for the WebView to offer it, so it also appears in
 * other apps' selection menus. Text is accepted only when this app started
 * it (the WebView uses startActivityForResult, so the caller is known);
 * anything else is refused without reading the text.
 */
class AskSelectionActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (intent?.action == Intent.ACTION_PROCESS_TEXT) {
            if (acceptsCaller(callingPackage, packageName)) {
                ReaderSelection.offer(intent.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT)?.toString())
            } else {
                Toast.makeText(this, R.string.ask_about_selection_elsewhere, Toast.LENGTH_LONG).show()
            }
        }
        // Read-only: the selection in the page is never replaced.
        setResult(RESULT_CANCELED)
        finish()
    }

    companion object {
        /** Only this app's own reader may hand over a selection. */
        fun acceptsCaller(callingPackage: String?, ownPackage: String): Boolean =
            callingPackage != null && callingPackage == ownPackage
    }
}
