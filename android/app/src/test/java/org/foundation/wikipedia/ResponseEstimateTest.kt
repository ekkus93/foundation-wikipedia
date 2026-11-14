package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ResponseEstimateTest {
    // A slow CPU model: 10 ms per prompt character, 20 characters per second.
    private val slow = ModelTiming(10.0, 20.0, 400.0, 3)

    @Test
    fun withoutHistoryTheFirstRequestIsIndeterminateUntilItHasWrittenAWhile() {
        val reading = ResponseEstimate.of(null, 3000, 4096, 5000, null, 0, false)
        assertEquals(ResponsePhase.READING, reading.phase)
        assertNull(reading.fraction)
        assertNull(reading.remainingMs)
        assertNull(ResponseEstimate.of(null, 3000, 4096, 5500, 5000, 10, false).remainingMs)
        // After 2 s of writing at 20 chars/s, expect ~40% of 4096 chars.
        val writing = ResponseEstimate.of(null, 3000, 4096, 7000, 5000, 40, false)
        assertEquals(ResponsePhase.WRITING, writing.phase)
        val left = writing.remainingMs!!
        assertTrue("$left", left in 79_000L..80_000L)
        assertTrue(writing.fraction!! in 0.07f..0.09f)
    }

    @Test
    fun historyGivesAnEstimateBeforeTheFirstWord() {
        // 3000 prompt chars → 30 s reading; 400 chars at 20/s → 20 s writing.
        val start = ResponseEstimate.of(slow, 3000, 4096, 0, null, 0, false)
        assertEquals(50_000L, start.remainingMs)
        assertEquals(0f, start.fraction)
        val later = ResponseEstimate.of(slow, 3000, 4096, 10_000, null, 0, false)
        assertEquals(40_000L, later.remainingMs)
        assertEquals(0.2f, later.fraction!!, 0.001f)
        // Reading overruns: the estimate never goes to zero or negative.
        val overdue = ResponseEstimate.of(slow, 3000, 4096, 60_000, null, 0, false)
        assertTrue(overdue.remainingMs!! >= 20_000L)
        assertTrue(overdue.fraction!! < 1f)
    }

    @Test
    fun writingUsesTheLiveSpeedAndGrowsTheExpectedLength() {
        // Writing faster (40 chars/s) than history, and already past 400 chars.
        val estimate = ResponseEstimate.of(slow, 3000, 4096, 40_000, 30_000, 400, false)
        assertEquals(ResponsePhase.WRITING, estimate.phase)
        // Expected 460 chars → 60 left at 40/s → 1.5 s.
        assertEquals(1500L, estimate.remainingMs)
        val done = ResponseEstimate.of(slow, 3000, 4096, 41_000, 30_000, 420, true)
        assertEquals(ResponsePhase.CHECKING, done.phase)
        assertEquals(1f, done.fraction)
    }

    @Test
    fun timingsAreLearnedEncodedAndRejectedWhenMalformed() {
        val sample = ModelTiming.measured(promptChars = 2000, firstOutputMs = 20_000, finishedMs = 30_000, outputChars = 300)!!
        assertEquals(10.0, sample.msPerPromptChar, 1e-9)
        assertEquals(30.0, sample.outputCharsPerSecond, 1e-9)
        val first = ModelTiming(0.0, 1.0, 0.0, 0).plus(sample)
        assertEquals(sample, first)
        val second = first.plus(ModelTiming(20.0, 10.0, 100.0, 1))
        assertEquals(13.5, second.msPerPromptChar, 1e-9)
        assertEquals(2, second.samples)
        assertEquals(second, ModelTiming.decode(second.encode()))
        for (bad in listOf(null, "", "v2:1:1:1:1", "v1:1:0:1:1", "v1:1:1:1:0", "v1:x:1:1:1", "v1:NaN:1:1:1", "v1:-1:1:1:1")) {
            assertNull(bad, ModelTiming.decode(bad))
        }
        assertNull(ModelTiming.measured(2000, 20_000, 20_000, 300))
        assertNotNull(ModelTiming.measured(1, 0, 1, 1))
    }

    @Test
    fun remainingTimeIsDescribedInRoundedSteps() {
        assertEquals("a few seconds left", ResponseEstimate.describe(3_000))
        assertEquals("about 15 s left", ResponseEstimate.describe(12_400))
        assertEquals("about 1 min left", ResponseEstimate.describe(55_100 + 4_000))
        assertEquals("about 1 min 20 s left", ResponseEstimate.describe(75_000))
        assertEquals("about 12 min left", ResponseEstimate.describe(11 * 60_000L + 5_000))
    }
}
