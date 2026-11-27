package org.foundation.wikipedia

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * MOBPACK-003 live check of the in-app player against Wikimedia's media
 * servers. Skipped unless run with `-e avLive true`; CI never runs it. It
 * streams two short files named by real "Play online" links, pauses and
 * resumes each, and seeks to halfway.
 */
@RunWith(AndroidJUnit4::class)
class AvPlayerLiveTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun shown(text: String) =
        compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    /** The clock text, e.g. `0:03 / 1:28`. */
    private fun clock(): String? = compose.onAllNodesWithText(" / ", substring = true)
        .fetchSemanticsNodes().firstOrNull()
        ?.config?.getOrNull(androidx.compose.ui.semantics.SemanticsProperties.Text)
        ?.firstOrNull()?.text

    private fun playsPausesAndResumes(page: String, length: String) {
        assumeTrue(InstrumentationRegistry.getArguments().getString("avLive") == "true")
        val file = AvStream.fromCommonsPage(page)!!
        compose.setContent {
            AvPlayer(file.title, file.video, AvStream.candidates(file), onOpenInBrowser = {}, onClose = {})
        }
        compose.waitUntil(timeoutMillis = 90_000) { shown("Pause") }
        // The length Wikimedia states, and a position that advances.
        compose.waitUntil(timeoutMillis = 30_000) {
            clock()?.let { it.endsWith(" / $length") && !it.startsWith("0:00") } == true
        }
        compose.onNodeWithText("Pause").performClick()
        compose.waitUntil(timeoutMillis = 10_000) { shown("Play") }
        val paused = clock()
        Thread.sleep(2_500)
        assertTrue("still at $paused while paused", clock() == paused)
        compose.onNodeWithText("Play").performClick()
        compose.waitUntil(timeoutMillis = 30_000) { shown("Pause") && clock() != paused }

        // Seeking: move the slider to halfway; the player must report a
        // position near the middle and keep playing from there.
        val total = seconds(length)
        compose.onNodeWithContentDescription("Position")
            .performSemanticsAction(SemanticsActions.SetProgress) { it(0.5f) }
        compose.waitUntil(timeoutMillis = 30_000) {
            elapsed()?.let { it in (total * 45 / 100)..(total * 60 / 100) } == true
        }
        val sought = elapsed()!!
        compose.waitUntil(timeoutMillis = 30_000) { (elapsed() ?: 0) > sought }
    }

    private fun seconds(clock: String): Int =
        clock.split(':').fold(0) { total, part -> total * 60 + part.trim().toInt() }

    /** Elapsed seconds from the clock text. */
    private fun elapsed(): Int? = clock()?.substringBefore(" / ")?.let(::seconds)

    @Test
    fun audioStreamsPausesAndResumes() = playsPausesAndResumes(
        "https://commons.wikimedia.org/wiki/File:Pronunciation_of_Latin_(Text).ogg",
        length = "1:28",
    )

    @Test
    fun videoStreamsPausesAndResumes() = playsPausesAndResumes(
        "https://commons.wikimedia.org/wiki/File:WIKITONGUES-_Titus_speaking_Latin.webm",
        length = "2:33",
    )
}
