package org.foundation.wikipedia

import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick

/**
 * Open the reader's action menu. A tap made while the keyboard is still
 * closing can land beside the moving button, so the tap is repeated until
 * the menu is open (the button then offers to close it).
 */
fun AndroidComposeTestRule<*, *>.openReaderActions() {
    waitUntil(timeoutMillis = 30_000) {
        onAllNodesWithContentDescription("Open reader actions").fetchSemanticsNodes().isNotEmpty()
    }
    repeat(3) {
        if (onAllNodesWithContentDescription("Close reader actions").fetchSemanticsNodes().isNotEmpty()) {
            return
        }
        waitForIdle()
        onNodeWithContentDescription("Open reader actions").performClick()
        try {
            waitUntil(timeoutMillis = 5_000) {
                onAllNodesWithContentDescription("Close reader actions").fetchSemanticsNodes().isNotEmpty()
            }
            return
        } catch (_: ComposeTimeoutException) {
            // Tap again.
        }
    }
    throw AssertionError("the reader action menu did not open after three taps")
}
