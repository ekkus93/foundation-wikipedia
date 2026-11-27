package org.foundation.wikipedia

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * MOBPACK-003: the in-app player says so when nothing can be played, and
 * offers the browser. The streams here are files that do not exist, which
 * fail at once and contact no server.
 */
@RunWith(AndroidJUnit4::class)
class AvPlayerUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun shown(text: String) =
        compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun whenNoStreamPlaysTheReaderIsToldAndOfferedTheBrowser() {
        val events = mutableListOf<String>()
        compose.setContent {
            AvPlayer(
                title = "Launch film.webm",
                video = true,
                streams = listOf(
                    "file:///nonexistent/first.webm",
                    "file:///nonexistent/second.webm",
                ),
                onOpenInBrowser = { events += "browser" },
                onClose = { events += "close" },
            )
        }
        assertTrue(compose.onAllNodesWithTag(AV_PLAYER_TAG).fetchSemanticsNodes().isNotEmpty())
        assertTrue(shown("Launch film.webm"))
        // Both addresses are tried before giving up.
        compose.waitUntil(timeoutMillis = 60_000) { shown("This could not be played here.") }
        assertTrue(!shown("Pause"))
        compose.onNodeWithText("Open in browser").performClick()
        compose.onNodeWithText("Close").performClick()
        assertEquals(listOf("browser", "close"), events)
    }

    @Test
    fun thePlaybackClockReadsLikeAPlayer() {
        assertEquals("0:00", playbackClock(-5))
        assertEquals("1:35", playbackClock(95_000))
        assertEquals("1:35:53", playbackClock(5_753_000))
    }
}
